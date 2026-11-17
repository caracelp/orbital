//! Bases: consultas y escrituras fuera del hilo GTK, con versiones de archivo.
use super::*;
use orbital_vault::bases::{self, BaseDefinition};

impl Window {
    fn base_context_matches(&self, vault: &Arc<Vault>, generation: u64) -> bool {
        self.0.base_generation.get() == generation
            && self
                .0
                .vault
                .borrow()
                .as_ref()
                .is_some_and(|v| Arc::ptr_eq(v, vault))
    }

    pub(super) fn setup_bases(&self) {
        let action = gio::ActionEntry::builder("new-base")
            .activate({
                let this = self.clone();
                move |_: &adw::ApplicationWindow, _, _| this.ask_new_base()
            })
            .build();
        self.0.window.add_action_entries([action]);
    }

    fn base_panel(&self) -> &crate::bases::BasePanel {
        self.0.base_panel.get_or_init(|| {
            let panel = crate::bases::BasePanel::new();
            self.0.content_stack.add_named(panel.widget(), Some("base"));
            let weak = Rc::downgrade(&self.0);
            panel.connect_open(move |path| {
                if let Some(i) = weak.upgrade() {
                    Window(i).show_path(path.as_str());
                }
            });
            let weak = Rc::downgrade(&self.0);
            panel.connect_configure(move |definition| {
                if let Some(i) = weak.upgrade() {
                    Window(i).save_base(definition);
                }
            });
            let weak = Rc::downgrade(&self.0);
            panel.connect_edit(move |path, hash, key, value| {
                if let Some(i) = weak.upgrade() {
                    Window(i).edit_base_cell(path, hash, key, value);
                }
            });
            let weak = Rc::downgrade(&self.0);
            panel.connect_rename(move |old, new| {
                if let Some(i) = weak.upgrade() {
                    Window(i).rename_base_property(old, new);
                }
            });
            panel
        })
    }

    pub(super) fn base_note_changed(&self, path: RelPath) {
        let Some(vault) = self.0.vault.borrow().clone() else {
            return;
        };
        let generation = self.0.base_generation.get();
        let current = self
            .0
            .base_document
            .borrow()
            .as_ref()
            .map(|(p, _)| p.clone());
        let this = self.clone();
        glib::spawn_future_local(async move {
            let v = vault.clone();
            let result = gio::spawn_blocking(move || {
                v.update_base_index(&path);
                current.map(|current| {
                    bases::read(&v, &current).map(|document| {
                        let snapshot = v.base_snapshot(&document.definition);
                        (current, document, snapshot)
                    })
                })
            })
            .await;
            if !this.base_context_matches(&vault, generation) {
                return;
            }
            if this.0.content_stack.visible_child_name().as_deref() != Some("base") {
                return;
            }
            match result {
                Ok(Some(Ok((path, document, snapshot)))) => {
                    *this.0.base_document.borrow_mut() = Some((path, document.hash));
                    this.base_panel().set_data(document.definition, snapshot);
                }
                Ok(Some(Err(error))) => {
                    *this.0.base_document.borrow_mut() = None;
                    this.base_panel()
                        .set_data(BaseDefinition::default(), Default::default());
                    this.base_panel().set_error(Some(&error.to_string()));
                }
                _ => {}
            }
        });
    }

    pub(super) fn open_base(&self, path: RelPath) {
        let Some(vault) = self.0.vault.borrow().clone() else {
            return;
        };
        let generation = self.0.base_generation.get() + 1;
        self.0.base_generation.set(generation);
        let this = self.clone();
        glib::spawn_future_local(async move {
            this.0.code_editor.flush().await;
            if this.0.code_editor.has_pending() {
                this.toast("La nota tiene cambios pendientes; no se abrió la base");
                return;
            }
            if this.0.base_generation.get() != generation {
                return;
            }
            this.0.code_editor.close();
            this.clear_note_view();
            *this.0.base_document.borrow_mut() = None;
            *this.0.current_entry.borrow_mut() = this.entry(path.as_str());
            this.base_panel()
                .set_data(BaseDefinition::default(), Default::default());
            this.0.content_stack.set_visible_child_name("base");
            this.base_panel().set_busy(true);
            let p = path.clone();
            let result = gio::spawn_blocking(move || {
                let document = bases::read(&vault, &p)?;
                let snapshot = vault.base_snapshot(&document.definition);
                Ok::<_, orbital_vault::VaultError>((document, snapshot))
            })
            .await;
            if this.0.base_generation.get() != generation {
                return;
            }
            this.base_panel().set_busy(false);
            match result {
                Ok(Ok((document, snapshot))) => {
                    *this.0.base_document.borrow_mut() = Some((path.clone(), document.hash));
                    *this.0.current_entry.borrow_mut() = this.entry(path.as_str());
                    let raw = path.first_component() == "raw";
                    for (name, enabled) in [
                        ("rename", !raw),
                        ("trash", !raw),
                        ("archive", false),
                        ("paste-image", false),
                    ] {
                        if let Some(action) = this
                            .0
                            .window
                            .lookup_action(name)
                            .and_downcast::<gio::SimpleAction>()
                        {
                            action.set_enabled(enabled);
                        }
                    }
                    this.habilitar_acciones_listas(true);
                    this.0.content_title.set_title(path.file_name());
                    this.0.content_title.set_subtitle(path.as_str());
                    this.0.pestanas.abrir(path.as_str(), path.file_name());
                    this.base_panel().set_data(document.definition, snapshot);
                }
                Ok(Err(e)) => this.base_panel().set_error(Some(&e.to_string())),
                Err(_) => this
                    .base_panel()
                    .set_error(Some("No se pudo consultar la base")),
            }
        });
    }

    fn ask_new_base(&self) {
        let Some(dialog_vault) = self.0.vault.borrow().clone() else {
            self.toast("Abrí una bóveda para crear una base");
            return;
        };
        let dialog_generation = self.0.base_generation.get();
        let entry = gtk::Entry::builder()
            .placeholder_text("Nombre de la base")
            .activates_default(true)
            .build();
        entry.update_property(&[gtk::accessible::Property::Label("Nombre de la base")]);
        let dialog = adw::AlertDialog::new(
            Some("Crear base"),
            Some("Elegí un nombre libre. Se guardará como archivo .base."),
        );
        dialog.set_extra_child(Some(&entry));
        dialog.add_responses(&[("cancel", "Cancelar"), ("create", "Crear")]);
        dialog.set_default_response(Some("create"));
        dialog.set_close_response("cancel");
        let this = self.clone();
        glib::spawn_future_local(async move {
            if dialog.choose_future(Some(&this.0.window)).await != "create" {
                return;
            }
            if !this.base_context_matches(&dialog_vault, dialog_generation) {
                return;
            }
            let name = entry.text();
            let name = name.trim();
            if name.is_empty() || name.contains(['/', '\\']) {
                this.toast("Usá un nombre de archivo sin carpetas");
                return;
            }
            let Some(vault) = this.0.vault.borrow().clone() else {
                return;
            };
            let generation = this.0.base_generation.get();
            let context_vault = vault.clone();
            let file = if name.ends_with(".base") {
                name.to_owned()
            } else {
                format!("{name}.base")
            };
            let prefix = if vault.root().join("wiki").is_dir() {
                "wiki/"
            } else {
                ""
            };
            let Ok(path) = RelPath::new(&format!("{prefix}{file}")) else {
                this.toast("Nombre de base inválido");
                return;
            };
            let p = path.clone();
            let result = gio::spawn_blocking(move || {
                bases::save(
                    &vault,
                    &p,
                    &BaseDefinition::default(),
                    None,
                    &write_ctx::now(Some(vault.config())),
                )
            })
            .await;
            if !this.base_context_matches(&context_vault, generation) {
                return;
            }
            match result {
                Ok(Ok(_)) => {
                    this.reload(None).await;
                    this.open_base(path);
                }
                Ok(Err(e)) => this.toast(&format!("No se pudo crear la base: {e}")),
                Err(_) => this.toast("No se pudo crear la base"),
            }
        });
    }

    fn save_base(&self, definition: BaseDefinition) {
        let (Some(vault), Some((path, hash))) = (
            self.0.vault.borrow().clone(),
            self.0.base_document.borrow().clone(),
        ) else {
            return;
        };
        let generation = self.0.base_generation.get();
        let context_vault = vault.clone();
        let this = self.clone();
        glib::spawn_future_local(async move {
            let p = path.clone();
            let result = gio::spawn_blocking(move || {
                bases::save(
                    &vault,
                    &p,
                    &definition,
                    Some(hash),
                    &write_ctx::now(Some(vault.config())),
                )
            })
            .await;
            if !this.base_context_matches(&context_vault, generation) {
                return;
            }
            match result {
                Ok(Ok(_)) => this.open_base(path),
                Ok(Err(e)) => this.base_panel().set_error(Some(&e.to_string())),
                Err(_) => this
                    .base_panel()
                    .set_error(Some("No se pudo guardar la configuración")),
            }
        });
    }

    fn edit_base_cell(&self, path: RelPath, hash: u64, key: String, value: Option<PropValue>) {
        let Some(vault) = self.0.vault.borrow().clone() else {
            return;
        };
        let generation = self.0.base_generation.get();
        let context_vault = vault.clone();
        let this = self.clone();
        glib::spawn_future_local(async move {
            if this.0.code_editor.current_path().as_ref() == Some(&path) {
                if let Err(e) = this.0.code_editor.edit_prop(&key, value.as_ref(), true) {
                    this.base_panel().set_error(Some(&e));
                    return;
                }
                this.0.code_editor.flush().await;
                if this.0.code_editor.has_pending() {
                    this.base_panel()
                        .set_error(Some("No se pudo guardar la nota abierta"));
                    return;
                }
            } else {
                let p = path.clone();
                let result = gio::spawn_blocking(move || {
                    let receipt = bases::edit_cell(
                        &vault,
                        &p,
                        hash,
                        &key,
                        value.as_ref(),
                        &write_ctx::now(Some(vault.config())),
                    )?;
                    vault.update_base_index(&p);
                    Ok::<_, orbital_vault::VaultError>(receipt)
                })
                .await;
                if !this.base_context_matches(&context_vault, generation) {
                    return;
                }
                match result {
                    Ok(Ok(_)) => {}
                    Ok(Err(e)) => {
                        this.base_panel().set_error(Some(&e.to_string()));
                        return;
                    }
                    Err(_) => {
                        this.base_panel()
                            .set_error(Some("No se pudo editar la celda"));
                        return;
                    }
                }
            }
            if !this.base_context_matches(&context_vault, generation) {
                return;
            }
            let current = this
                .0
                .base_document
                .borrow()
                .as_ref()
                .map(|(p, _)| p.clone());
            if let Some(current) = current {
                this.open_base(current);
            }
        });
    }

    fn rename_base_property(&self, old: String, new: String) {
        let Some(vault) = self.0.vault.borrow().clone() else {
            return;
        };
        let generation = self.0.base_generation.get();
        let this = self.clone();
        glib::spawn_future_local(async move {
            this.0.code_editor.flush().await;
            if this.0.code_editor.has_pending() {
                this.toast("Hay cambios pendientes en la nota abierta");
                return;
            }
            let v = vault.clone();
            let result =
                gio::spawn_blocking(move || bases::plan_property_rename(&v, &old, &new)).await;
            if !this.base_context_matches(&vault, generation) {
                return;
            }
            let plan = match result {
                Ok(Ok(p)) => p,
                Ok(Err(e)) => {
                    this.toast(&e);
                    return;
                }
                Err(_) => {
                    this.toast("No se pudo planificar el renombre");
                    return;
                }
            };
            let mut body = format!("Se modificarán {} archivos.\n", plan.edits.files());
            for (path, error) in &plan.conflicts {
                body.push_str(&format!("\nConflicto (queda intacto): {path}: {error}"));
            }
            for (path, error) in &plan.edits.errors {
                body.push_str(&format!("\nNo inspeccionado: {path}: {error}"));
            }
            let dialog = adw::AlertDialog::new(Some("Renombrar propiedad"), Some(&body));
            dialog.add_responses(&[("cancel", "Cancelar"), ("apply", "Aplicar")]);
            dialog.set_close_response("cancel");
            if dialog.choose_future(Some(&this.0.window)).await != "apply" {
                return;
            }
            if !this.base_context_matches(&vault, generation) {
                return;
            }
            let edits = Arc::new(plan.edits);
            let (v, p) = (vault.clone(), edits.clone());
            let Ok((receipt, failed)) = gio::spawn_blocking(move || {
                let result = v.apply_edits(&p, &write_ctx::now(Some(v.config())));
                for (path, _) in &result.0.written {
                    v.update_base_index(path);
                }
                result
            })
            .await
            else {
                this.toast("No se pudo aplicar el renombre");
                return;
            };
            if !this.base_context_matches(&vault, generation) {
                return;
            }
            let applied = Arc::new(edits.applied(&receipt));
            let current = this
                .0
                .base_document
                .borrow()
                .as_ref()
                .map(|(p, _)| p.clone());
            this.reload(current).await;
            let undo_this = this.clone();
            this.toast_undo(
                &format!(
                    "{} archivos modificados; {} fallos",
                    applied.files(),
                    failed.len()
                ),
                move || {
                    let (this, vault, applied) =
                        (undo_this.clone(), vault.clone(), applied.clone());
                    glib::spawn_future_local(async move {
                        if !this
                            .0
                            .vault
                            .borrow()
                            .as_ref()
                            .is_some_and(|v| v.root() == vault.root())
                        {
                            return;
                        }
                        this.0.code_editor.flush().await;
                        if this.0.code_editor.has_pending() {
                            this.toast("Hay cambios pendientes; no se pudo deshacer");
                            return;
                        }
                        let generation = this.0.base_generation.get();
                        let current_vault = this.0.vault.borrow().clone();
                        let Ok((_, failed)) = gio::spawn_blocking(move || {
                            let result =
                                vault.revert_edits(&applied, &write_ctx::now(Some(vault.config())));
                            for (path, _) in &result.0.written {
                                vault.update_base_index(path);
                            }
                            result
                        })
                        .await
                        else {
                            this.toast("No se pudo deshacer");
                            return;
                        };
                        if !current_vault
                            .as_ref()
                            .is_some_and(|v| this.base_context_matches(v, generation))
                        {
                            return;
                        }
                        if !failed.is_empty() {
                            this.toast(&format!(
                                "{} archivos no se pudieron deshacer porque cambiaron",
                                failed.len()
                            ));
                        }
                        let current = this
                            .0
                            .base_document
                            .borrow()
                            .as_ref()
                            .map(|(p, _)| p.clone());
                        this.reload(current).await;
                    });
                },
            );
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    async fn until(check: impl Fn() -> bool) {
        for _ in 0..200 {
            if check() {
                return;
            }
            glib::timeout_future(Duration::from_millis(10)).await;
        }
        assert!(check(), "la operación de base no terminó");
    }

    #[test]
    #[ignore = "requiere GTK y D-Bus; instancia y bóveda temporales propias"]
    fn base_window_is_lazy_edits_safely_and_clears_failed_loads() {
        adw::init().unwrap();
        let app = adw::Application::builder()
            .application_id("io.github.caracelp.Orbital.BaseTest")
            .flags(gio::ApplicationFlags::NON_UNIQUE)
            .build();
        app.register(None::<&gio::Cancellable>).unwrap();
        let root =
            std::env::temp_dir().join(format!("orbital-029-window-{}", glib::uuid_string_random()));
        std::fs::create_dir_all(root.join("wiki")).unwrap();
        let source = "---\ntitle: Alfa\nprecio: 10\n---\nCuerpo intacto\n";
        std::fs::write(root.join("wiki/a.md"), source).unwrap();
        std::fs::write(
            root.join("wiki/a.base"),
            bases::emit(&BaseDefinition::default()),
        )
        .unwrap();
        std::fs::write(root.join("wiki/bad.base"), "format_version: 99\n").unwrap();
        glib::MainContext::default().block_on(async {
            let w = Window::new(&app);
            w.set_persist(false);
            assert!(
                w.0.base_panel.get().is_none(),
                "Bases no construye widgets al iniciar"
            );
            let (v, t, m) = prepare(orbital_vault::open(&root).unwrap());
            w.show_vault(v, t, m);
            w.open_base(RelPath::new("wiki/a.base").unwrap());
            until(|| w.0.base_document.borrow().is_some()).await;
            assert_eq!(
                w.0.content_stack.visible_child_name().as_deref(),
                Some("base")
            );
            let path = RelPath::new("wiki/a.md").unwrap();
            let hash = orbital_vault::write::hash(source.as_bytes());
            w.edit_base_cell(
                path.clone(),
                hash,
                "precio".into(),
                Some(PropValue::Number("12".into())),
            );
            until(|| {
                std::fs::read_to_string(root.join("wiki/a.md"))
                    .unwrap()
                    .contains("precio: 12")
            })
            .await;
            assert_eq!(
                std::fs::read_to_string(root.join("wiki/a.md")).unwrap(),
                source.replace("precio: 10", "precio: 12")
            );
            until(|| w.0.base_document.borrow().is_some()).await;
            w.edit_base_cell(
                path,
                hash,
                "precio".into(),
                Some(PropValue::Number("99".into())),
            );
            glib::timeout_future(Duration::from_millis(100)).await;
            assert!(
                !std::fs::read_to_string(root.join("wiki/a.md"))
                    .unwrap()
                    .contains("99")
            );
            w.open_base(RelPath::new("wiki/bad.base").unwrap());
            glib::timeout_future(Duration::from_millis(200)).await;
            assert!(
                w.0.base_document.borrow().is_none(),
                "una base inválida no conserva documento editable anterior"
            );
            // Una recarga del árbol por otra nota conserva el borrador y su deshacer.
            w.show_path("wiki/a.md");
            until(|| {
                w.0.code_editor.current_path().as_ref().map(RelPath::as_str) == Some("wiki/a.md")
            })
            .await;
            w.0.code_editor
                .buffer()
                .insert_at_cursor("Borrador pendiente\n");
            let draft = w.0.code_editor.text();
            let can_undo = w.0.code_editor.buffer().can_undo();
            let page = w.0.content_stack.visible_child_name();
            assert!(w.0.code_editor.has_pending());
            w.reload(Some(RelPath::new("wiki/a.md").unwrap())).await;
            assert_eq!(w.0.code_editor.text(), draft);
            assert_eq!(w.0.code_editor.buffer().can_undo(), can_undo);
            assert_eq!(w.0.content_stack.visible_child_name(), page);
            w.0.code_editor.flush().await;
            w.0.window.destroy();
        });
        std::fs::remove_dir_all(root).unwrap();
    }
}
