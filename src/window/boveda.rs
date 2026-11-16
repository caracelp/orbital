//! Abrir, mostrar y recargar la bóveda, y los cambios hechos por otros programas en el disco.

use super::*;

impl Window {
    /// Diálogo del sistema para elegir una carpeta.
    pub(super) async fn choose_folder(&self, title: &str) -> Option<PathBuf> {
        let dialog = gtk::FileDialog::builder().title(title).modal(true).build();
        match dialog.select_folder_future(Some(&self.0.window)).await {
            Ok(folder) => folder.path(),
            Err(e)
                if e.matches(gtk::DialogError::Dismissed)
                    || e.matches(gtk::DialogError::Cancelled) =>
            {
                None
            }
            Err(e) => {
                self.toast(&format!("No se pudo elegir la carpeta: {e}"));
                None
            }
        }
    }

    /// Elige una carpeta y la abre con «Abrir».
    pub fn choose_and_open(&self) {
        let this = self.clone();
        glib::spawn_future_local(async move {
            if let Some(path) = this.choose_folder("Abrir una carpeta").await {
                this.open_path(path).await;
            }
        });
    }

    /// Elige una carpeta y la abre con «Abrir como bóveda de Orbital».
    pub fn choose_and_open_as(&self) {
        let this = self.clone();
        glib::spawn_future_local(async move {
            if let Some(path) = this.choose_folder("Abrir como bóveda de Orbital").await {
                this.open_as_path(path).await;
            }
        });
    }

    /// «Abrir como»: muestra lo que falta, pide confirmación y crea solo eso.
    pub async fn open_as_path(&self, path: PathBuf) {
        let for_plan = path.clone();
        let plan = match gio::spawn_blocking(move || orbital_vault::plan_open_as(&for_plan)).await {
            Ok(Ok(plan)) => plan,
            Ok(Err(e)) => return self.toast(&format!("No se pudo revisar la carpeta: {e}")),
            Err(_) => return self.toast("No se pudo revisar la carpeta"),
        };
        if open_dialog::confirm(&self.0.window, &plan).await == Answer::Cancel {
            return;
        }
        let for_apply = path.clone();
        let result = gio::spawn_blocking(move || {
            let (config, _) = orbital_vault::VaultConfig::load(&for_apply);
            let ctx = write_ctx::now(Some(&config));
            orbital_vault::apply_open_as(&for_apply, &plan, &ctx).map(prepare)
        })
        .await;
        match result {
            Ok(Ok((vault, tree, meta))) => {
                self.show_vault(vault, tree, meta);
                self.remember(path);
            }
            Ok(Err(e)) => self.toast(&format!("No se pudo crear la estructura: {e}")),
            Err(_) => self.toast("No se pudo crear la estructura"),
        }
    }

    /// Abre `path` con «Abrir»: no escribe nada en la bóveda.
    pub async fn open_path(&self, path: PathBuf) {
        let for_thread = path.clone();
        let result =
            gio::spawn_blocking(move || orbital_vault::open(&for_thread).map(prepare)).await;
        match result {
            Ok(Ok((vault, tree, meta))) => {
                crate::medir_arranque::marca("bóveda leída");
                self.show_vault(vault, tree, meta);
                self.remember(path);
            }
            Ok(Err(e)) => self.toast(&format!("No se pudo abrir la bóveda: {e}")),
            Err(_) => self.toast("No se pudo abrir la bóveda"),
        }
    }

    pub(super) fn show_vault(&self, vault: Vault, tree: Tree, meta: NoteMeta) {
        if self
            .0
            .vault
            .borrow()
            .as_ref()
            .is_none_or(|current| current.root() != vault.root())
        {
            self.0.vault_session.set(self.0.vault_session.get() + 1);
            self.0.pestanas.limpiar();
        }
        let i = &self.0;
        self.fill_new_menu(&meta.templates);
        let name = vault
            .root()
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| vault.root().display().to_string());
        i.pie_boveda
            .set_boveda(vault.root(), &AppState::load().recent);
        i.window.set_title(Some(&format!("{name} — Orbital")));

        let incomplete = !vault.layout().is_complete();
        i.banner.set_visible(incomplete);
        for w in vault.warnings() {
            self.toast(w);
        }

        let tree = Rc::new(tree);
        *i.tree.borrow_mut() = tree.clone();
        let folders: Vec<PathBuf> = vault
            .entries()
            .iter()
            .filter(|e| e.kind == EntryKind::Folder)
            .map(|e| e.path.to_path(vault.root()))
            .collect();
        let this = self.clone();
        *i.watcher.borrow_mut() =
            Some(Watcher::new(
                vault.root(),
                folders.into_iter(),
                move |b| match b {
                    Batch::Changed(paths) => this.on_disk_changes(paths),
                    Batch::Gone => this.vault_gone(),
                },
            ));
        self.set_gone(false);
        let arc_vault = Arc::new(vault);
        *i.vault.borrow_mut() = Some(arc_vault.clone());
        i.search_panel
            .set_vault(Some(arc_vault.clone()), i.vault_session.get());
        i.search_gen.set(i.search_gen.get() + 1);
        i.search_updates.borrow_mut().clear();
        i.search_updating.set(false);
        *i.search_index.borrow_mut() = None;
        i.search_panel.set_search_index(None);
        i.code_editor.set_search_index(None);
        i.tag_panel.set_tree(None);
        i.code_editor.set_tag_candidates(Vec::new());

        crate::medir_arranque::marca("show_vault: hasta candidatos");
        let baseline = Self::collect_candidates_baseline(&arc_vault);
        crate::medir_arranque::marca("show_vault: candidatos");
        *i.candidates.borrow_mut() = baseline;
        let vault_for_cand = arc_vault.clone();
        let this_cand = self.clone();
        glib::spawn_future_local(async move {
            let full = gio::spawn_blocking(move || Self::collect_candidates_full(&vault_for_cand))
                .await
                .ok();
            if let Some(full) = full {
                *this_cand.0.candidates.borrow_mut() = full;
            }
        });

        self.rebuild_graph();
        crate::medir_arranque::marca("show_vault: grafo pedido");
        let this = self.clone();
        let generation = i.search_gen.get();
        self.after_next_frame(move || {
            if this.0.search_gen.get() == generation {
                this.rebuild_search_index();
            }
        });

        // Totales de la barra inferior (maqueta: notas, carpetas y archivos).
        let (mut notas, mut carpetas, mut archivos) = (0, 0, 0);
        for e in arc_vault.entries() {
            match e.kind {
                EntryKind::Folder => carpetas += 1,
                EntryKind::AtomNote | EntryKind::SynthesisNote => {
                    notas += 1;
                    archivos += 1;
                }
                _ => archivos += 1,
            }
        }
        i.status.set_totales(notas, carpetas, archivos);
        crate::medir_arranque::marca("show_vault: totales");
        *i.recientes.borrow_mut() = AppState::load().recent_notes_for(arc_vault.root()).to_vec();
        self.refresh_recent();
        crate::medir_arranque::marca("show_vault: recientes");
        // Favoritos y Guardados (027): se cargan después del primer cuadro.
        self.listas_en_show_vault();
        self.set_list_model(tree);
        crate::medir_arranque::marca("show_vault: lista");

        i.editor.buffer().set_text("");
        i.content_stack.set_visible_child_name("vacio");
        // Bóveda vacía: decir qué hacer en vez de mostrar una lista en blanco.
        if let Some(empty) = i
            .content_stack
            .child_by_name("vacio")
            .and_downcast::<adw::StatusPage>()
        {
            let vault_empty = i
                .vault
                .borrow()
                .as_ref()
                .is_some_and(|v| v.entries().iter().all(|e| e.kind == EntryKind::Folder));
            if vault_empty {
                empty.set_title("La bóveda todavía no tiene notas");
                empty.set_description(Some(
                    "Creá una con Ctrl+N o con el botón + de la barra lateral.",
                ));
            } else {
                empty.set_title("Ninguna nota abierta");
                empty.set_description(Some("Elegí una nota de la lista para leerla."));
            }
        }
        i.content_title.set_title("");
        i.content_title.set_subtitle("");
        i.stack.set_visible_child_name("boveda");
        i.list_view.grab_focus();
        if incomplete {
            i.list_view.announce(
                "La estructura de la bóveda está incompleta",
                gtk::AccessibleAnnouncementPriority::Medium,
            );
        }
    }

    pub(super) fn collect_candidates_baseline(
        vault: &Vault,
    ) -> Vec<crate::editor::completion::CompletionCandidate> {
        let mut list: Vec<_> = vault
            .entries()
            .iter()
            .filter(|e| {
                e.zone == orbital_vault::Zone::Wiki
                    && matches!(
                        e.kind,
                        orbital_vault::EntryKind::AtomNote
                            | orbital_vault::EntryKind::SynthesisNote
                    )
            })
            .map(|e| {
                crate::editor::completion::CompletionCandidate::new(
                    e.path.clone(),
                    None,
                    Vec::new(),
                )
            })
            .collect();
        crate::editor::completion::qualify_duplicates(&mut list);
        list
    }

    pub(super) fn collect_candidates_full(
        vault: &Vault,
    ) -> Vec<crate::editor::completion::CompletionCandidate> {
        let mut list: Vec<_> = vault
            .entries()
            .iter()
            .filter(|e| {
                e.zone == orbital_vault::Zone::Wiki
                    && matches!(
                        e.kind,
                        orbital_vault::EntryKind::AtomNote
                            | orbital_vault::EntryKind::SynthesisNote
                    )
            })
            .map(|e| {
                let props = vault.read_props(&e.path).ok().flatten();
                let entries = props.as_ref().map_or(&[][..], |p| &p.entries[..]);
                let title = orbital_vault::props::find(entries, "title")
                    .and_then(|v| v.value.as_text())
                    .map(str::to_owned);
                let aliases = orbital_vault::props::find(entries, "aliases")
                    .map(|v| match &v.value {
                        orbital_vault::props::PropValue::List(l) => l
                            .iter()
                            .filter_map(|x| x.as_text().map(str::to_owned))
                            .collect(),
                        orbital_vault::props::PropValue::Text(t) => vec![t.clone()],
                        _ => Vec::new(),
                    })
                    .unwrap_or_default();
                crate::editor::completion::CompletionCandidate::new(e.path.clone(), title, aliases)
            })
            .collect();
        crate::editor::completion::qualify_duplicates(&mut list);
        list
    }

    /// Vuelve a leer la bóveda (después de renombrar o borrar) y muestra `show`, si se indica.
    pub(super) async fn reload(&self, show: Option<RelPath>) {
        let Some(root) = self
            .0
            .vault
            .borrow()
            .as_ref()
            .map(|v| v.root().to_path_buf())
        else {
            return;
        };
        let result = gio::spawn_blocking(move || orbital_vault::open(&root).map(prepare)).await;
        if let Ok(Ok((vault, tree, meta))) = result {
            self.show_vault(vault, tree, meta);
            if let Some(p) = show {
                self.show_path(p.as_str());
            }
        }
    }

    /// Otro programa cambió archivos: si de verdad cambió algo, vuelve a leer la bóveda y deja
    /// abierta la misma nota (FR-030). Como todavía no hay editor, nunca hay cambios locales que
    /// se puedan perder.
    pub(super) fn on_disk_changes(&self, paths: Vec<PathBuf>) {
        if self.0.tag_operation.get() {
            self.0.tag_disk_changes.borrow_mut().extend(paths);
            return;
        }
        let Some(vault) = self.0.vault.borrow().clone() else {
            return;
        };
        let retry_paths = paths.clone();
        let this = self.clone();
        glib::spawn_future_local(async move {
            let v = vault.clone();
            let own: Vec<(RelPath, u64)> = this
                .0
                .own_writes
                .borrow()
                .iter()
                .map(|(p, h)| (p.clone(), *h))
                .collect();
            let (mut changes, echoes) = gio::spawn_blocking(move || {
                let changes = v.on_external_change(&paths);
                // Un cambio cuyo contenido es el que escribió el editor es un eco propio.
                let echoes: Vec<RelPath> = own
                    .into_iter()
                    .filter(|(p, h)| {
                        std::fs::read(p.to_path(v.root()))
                            .is_ok_and(|b| orbital_vault::write::hash(&b) == *h)
                    })
                    .map(|(p, _)| p)
                    .collect();
                (changes, echoes)
            })
            .await
            .unwrap_or_default();
            if this.0.tag_operation.get() {
                this.0.tag_disk_changes.borrow_mut().extend(retry_paths);
                return;
            }
            changes.retain(
                |c| !matches!(c, orbital_vault::ExternalChange::Modified(p) if echoes.contains(p)),
            );
            // El grafo de enlaces sigue los cambios de otros programas (FR-014).
            for c in &changes {
                match c {
                    orbital_vault::ExternalChange::Modified(p) => {
                        this.update_graph(p.clone(), false);
                        this.update_search_index(p.clone(), false);
                    }
                    orbital_vault::ExternalChange::Removed(p) => {
                        this.update_graph(p.clone(), true);
                        this.update_search_index(p.clone(), true);
                    }
                    _ => {}
                }
            }
            let current = this
                .0
                .current_entry
                .borrow()
                .as_ref()
                .map(|e| e.path.clone());
            // La nota abierta en el editor: el eco de un guardado propio se ignora, y con cambios
            // sin guardar no se pisa nada (FR-019).
            let editor = this.0.code_editor.clone();
            if let Some(open) = editor.current_path() {
                let touched = changes
                    .iter()
                    .any(|c| matches!(c, orbital_vault::ExternalChange::Modified(p) if *p == open));
                if touched {
                    let abs = open.to_path(vault.root());
                    let disk = gio::spawn_blocking(move || std::fs::read(abs).ok())
                        .await
                        .ok()
                        .flatten();
                    let disk_hash = disk.as_deref().map(orbital_vault::write::hash).unwrap_or(0);
                    match editor.external_change(disk_hash) {
                        orbital_vault::session::ExternalAction::Ignore => changes.retain(|c| {
                            !matches!(c, orbital_vault::ExternalChange::Modified(p) if *p == open)
                        }),
                        orbital_vault::session::ExternalAction::Reload => {
                            let (v, p) = (vault.clone(), open.clone());
                            let note = gio::spawn_blocking(move || v.read_note(&p))
                                .await
                                .ok()
                                .and_then(Result::ok);
                            if let Some(note) = note {
                                editor.load(vault.clone(), &note);
                            }
                            changes.retain(|c| {
                                !matches!(c, orbital_vault::ExternalChange::Modified(p) if *p == open)
                            });
                        }
                        orbital_vault::session::ExternalAction::AskUser => {
                            changes.retain(|c| {
                                !matches!(c, orbital_vault::ExternalChange::Modified(p) if *p == open)
                            });
                            if !this.0.conflict_dialog_open.get() {
                                this.0.conflict_dialog_open.set(true);
                                let disk_text = disk
                                    .and_then(|b| String::from_utf8(b).ok())
                                    .unwrap_or_default();
                                let mine_text = editor.text();
                                let name = open.file_name().to_string();
                                let format = editor.format();
                                let this_c = this.clone();
                                let v_c = vault.clone();
                                let ed = editor.clone();
                                crate::editor::conflict::show_conflict_dialog(
                                    &this.0.window,
                                    &name,
                                    mine_text,
                                    disk_text,
                                    format,
                                    move |choice| {
                                        this_c.0.conflict_dialog_open.set(false);
                                        let this_choice = this_c.clone();
                                        let v_choice = v_c.clone();
                                        let ed_choice = ed.clone();
                                        glib::spawn_future_local(async move {
                                            match choice {
                                                crate::editor::conflict::ConflictChoice::KeepMine => {
                                                    if let Err(e) = ed_choice.keep_mine(v_choice).await {
                                                        this_choice.toast(&format!("Error al conservar cambios: {e}"));
                                                    }
                                                }
                                                crate::editor::conflict::ConflictChoice::UseDisk => {
                                                    if let Err(e) = ed_choice.use_disk(v_choice).await {
                                                        this_choice.toast(&format!("Error al usar la versión del disco: {e}"));
                                                    }
                                                }
                                            }
                                        });
                                    },
                                );
                            }
                        }
                    }
                }
                let gone = changes
                    .iter()
                    .any(|c| matches!(c, orbital_vault::ExternalChange::Removed(p) if *p == open));
                if gone && editor.has_pending() {
                    if !this.0.conflict_dialog_open.get() {
                        this.0.conflict_dialog_open.set(true);
                        let name = open.file_name().to_string();
                        let this_c = this.clone();
                        let v_c = vault.clone();
                        let ed = editor.clone();
                        crate::editor::conflict::show_deleted_dialog(
                            &this.0.window,
                            &name,
                            move |choice| {
                                this_c.0.conflict_dialog_open.set(false);
                                let this_choice = this_c.clone();
                                let v_choice = v_c.clone();
                                let ed_choice = ed.clone();
                                glib::spawn_future_local(async move {
                                    match choice {
                                        crate::editor::conflict::DeletedChoice::SaveAgain => {
                                            if let Err(e) = ed_choice.keep_mine(v_choice).await {
                                                this_choice.toast(&format!(
                                                    "Error al guardar la nota: {e}"
                                                ));
                                            }
                                        }
                                        crate::editor::conflict::DeletedChoice::DiscardToBackup => {
                                            if let Err(e) =
                                                ed_choice.discard_to_backup(v_choice).await
                                            {
                                                this_choice.toast(&format!(
                                                    "Error al descartar a respaldo: {e}"
                                                ));
                                            }
                                        }
                                    }
                                });
                            },
                        );
                    }
                    return;
                }
            }
            if changes.is_empty() {
                return;
            }
            let removed = current.as_ref().is_some_and(|c| {
                changes
                    .iter()
                    .any(|ch| matches!(ch, orbital_vault::ExternalChange::Removed(p) if p == c))
            });
            if removed {
                this.reload(None).await;
                this.toast("La nota abierta se borró o se movió desde otro programa");
            } else {
                this.reload(current).await;
            }
        });
    }

    /// La carpeta de la bóveda desapareció: solo lectura hasta que vuelva.
    pub(super) fn vault_gone(&self) {
        if self.0.gone.get() {
            return;
        }
        self.set_gone(true);
        let Some(root) = self
            .0
            .vault
            .borrow()
            .as_ref()
            .map(|v| v.root().to_path_buf())
        else {
            return;
        };
        let this = self.clone();
        glib::spawn_future_local(async move {
            // Revisa cada 2 s (fuera del hilo principal: un disco de red puede tardar) si volvió.
            loop {
                glib::timeout_future_seconds(2).await;
                let r = root.clone();
                if gio::spawn_blocking(move || r.is_dir())
                    .await
                    .unwrap_or(false)
                {
                    let current = this
                        .0
                        .current_entry
                        .borrow()
                        .as_ref()
                        .map(|e| e.path.clone());
                    this.reload(current).await;
                    this.toast("La bóveda volvió a estar disponible");
                    return;
                }
                if !this.0.gone.get() {
                    return;
                }
            }
        });
    }

    /// Activa o desactiva el modo «bóveda no disponible».
    pub(super) fn set_gone(&self, gone: bool) {
        self.0.gone.set(gone);
        self.0.gone_banner.set_visible(gone);
        for name in [
            "rename",
            "trash",
            "archive",
            "paste-image",
            "new-note",
            "new-html",
            "journal",
            "new-from-template",
            "complete-type",
            "complete-structure",
        ] {
            if let Some(a) = self
                .0
                .window
                .lookup_action(name)
                .and_downcast::<gio::SimpleAction>()
            {
                a.set_enabled(!gone);
            }
        }
        if gone {
            self.0.list_view.announce(
                "La bóveda no está disponible",
                gtk::AccessibleAnnouncementPriority::High,
            );
        }
    }
}
