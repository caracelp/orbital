//! Índice de búsqueda, etiquetas y renombrar etiquetas en toda la bóveda.

use super::*;

impl Window {
    /// Construye el índice de búsqueda de la bóveda en segundo plano (US1 de la 004).
    pub(super) fn rebuild_search_index(&self) {
        let i = &self.0;
        let Some(vault) = i.vault.borrow().clone() else {
            return;
        };
        let generation = i.search_gen.get() + 1;
        i.search_gen.set(generation);
        *i.search_index.borrow_mut() = None;
        i.search_panel.set_search_index(None);
        i.code_editor.set_search_index(None);

        let measure_ticks = std::env::var_os("ORBITAL_MEDIR_INDICE").is_some();
        let tick_state = if measure_ticks {
            let last_tick: Rc<Cell<Option<std::time::Instant>>> = Rc::new(Cell::new(None));
            let max_gap = Rc::new(Cell::new(std::time::Duration::ZERO));
            let running = Rc::new(Cell::new(true));
            let last = last_tick.clone();
            let max = max_gap.clone();
            let run = running.clone();
            self.0.window.add_tick_callback(move |w, _| {
                if !run.get() {
                    return glib::ControlFlow::Break;
                }
                let now = std::time::Instant::now();
                if let Some(prev) = last.get() {
                    let dt = now.duration_since(prev);
                    if dt > max.get() {
                        max.set(dt);
                    }
                }
                last.set(Some(now));
                w.queue_draw();
                glib::ControlFlow::Continue
            });
            Some((running, max_gap, std::time::Instant::now()))
        } else {
            None
        };

        let this = self.clone();
        glib::spawn_future_local(async move {
            let built = gio::spawn_blocking(move || {
                let index = orbital_vault::SearchIndex::build(&vault);
                let snapshot = crate::tag_panel::TagSnapshot::from_index(&index);
                (index, snapshot)
            })
            .await
            .ok();
            if let Some((run, max_gap, start)) = tick_state {
                run.set(false);
                let elapsed = start.elapsed();
                let worst_ms = max_gap.get().as_secs_f64() * 1000.0;
                println!(
                    "ORBITAL_MEDIR_INDICE: armado en {:.2} ms, peor intervalo entre cuadros: {:.2} ms",
                    elapsed.as_secs_f64() * 1000.0,
                    worst_ms
                );
            }
            if this.0.search_gen.get() != generation {
                return;
            }
            if let Some((index, snapshot)) = built {
                let arc_idx = Some(Arc::new(std::sync::RwLock::new(index)));
                *this.0.search_index.borrow_mut() = arc_idx.clone();
                this.0.search_panel.set_search_index(arc_idx.clone());
                this.0.code_editor.set_search_index(arc_idx);
                this.apply_tag_snapshot(snapshot);
                this.refrescar_iconos_ui();
                this.drain_search_updates();
            }
        });
    }

    /// Una sola tarea aplica cambios por vez: nunca publica una lectura vieja después de otra nueva.
    /// Los eventos recibidos durante la construcción inicial quedan pendientes.
    pub(super) fn update_search_index(&self, path: RelPath, removed: bool) {
        self.0.search_updates.borrow_mut().insert(path, removed);
        self.drain_search_updates();
    }

    pub(super) fn drain_search_updates(&self) {
        let i = &self.0;
        if i.search_updating.get() || i.search_updates.borrow().is_empty() {
            return;
        }
        let (Some(index), Some(vault)) =
            (i.search_index.borrow().clone(), i.vault.borrow().clone())
        else {
            return;
        };
        let updates = std::mem::take(&mut *i.search_updates.borrow_mut());
        let generation = i.search_gen.get();
        i.search_updating.set(true);
        let this = self.clone();
        glib::spawn_future_local(async move {
            let snapshot = gio::spawn_blocking(move || {
                let texts: Vec<_> = updates
                    .into_iter()
                    .map(|(path, removed)| {
                        let text = (!removed)
                            .then(|| std::fs::read_to_string(path.to_path(vault.root())).ok())
                            .flatten();
                        (path, text)
                    })
                    .collect();
                let mut idx = index.write().ok()?;
                for (path, text) in texts {
                    match text {
                        Some(t) => idx.update_file(&vault, &path, &t),
                        None => idx.remove_file(&path),
                    }
                }
                Some(crate::tag_panel::TagSnapshot::from_index(&idx))
            })
            .await
            .ok()
            .flatten();
            if this.0.search_gen.get() != generation {
                return;
            }
            this.0.search_updating.set(false);
            if let Some(snapshot) = snapshot {
                this.apply_tag_snapshot(snapshot);
            }
            this.0
                .search_panel
                .set_search_index(this.0.search_index.borrow().clone());
            this.0
                .code_editor
                .set_search_index(this.0.search_index.borrow().clone());
            this.drain_search_updates();
        });
    }

    pub(super) fn apply_tag_snapshot(&self, snapshot: crate::tag_panel::TagSnapshot) {
        self.0.code_editor.set_tag_candidates(snapshot.names);
        self.0.tag_panel.set_tree(Some(snapshot.tree));
        // Los codeblocks `indice` se generan desde el árbol: si la nota en pantalla tiene
        // alguno, la vista se regenera con el snapshot nuevo (005, T016).
        if self.0.code_editor.has_generated() {
            self.0.code_editor.render_now();
        }
    }

    pub(super) fn search_tag(&self, tag: &str) {
        self.0
            .search_panel
            .set_search_query(&orbital_vault::query::tag_query(tag));
        self.focus_search();
    }

    pub(super) fn same_vault(&self, vault: &Arc<Vault>, session: u64) -> bool {
        self.0.vault.borrow().as_ref().is_some_and(|current| {
            current.root() == vault.root() && self.0.vault_session.get() == session
        })
    }

    pub(super) fn ask_tag_rename(&self, old: String) {
        if self.0.tag_operation.get() {
            return;
        }
        let Some(vault) = self.0.vault.borrow().clone() else {
            return;
        };
        let session = self.0.vault_session.get();
        let this = self.clone();
        glib::spawn_future_local(async move {
            let field = gtk::Entry::builder()
                .text(&old)
                .activates_default(true)
                .build();
            field.update_property(&[gtk::accessible::Property::Label("Nombre nuevo de etiqueta")]);
            let dialog = adw::AlertDialog::new(
                Some("Renombrar etiqueta"),
                Some("Usá / para etiquetas anidadas. También se renombrarán las hijas."),
            );
            dialog.set_extra_child(Some(&field));
            dialog.add_response("cancel", "Cancelar");
            dialog.add_response("plan", "Continuar");
            dialog.set_default_response(Some("plan"));
            dialog.set_close_response("cancel");
            dialog.set_response_appearance("plan", adw::ResponseAppearance::Suggested);
            let weak = dialog.downgrade();
            field.connect_changed(move |entry| {
                if let Some(dialog) = weak.upgrade() {
                    dialog.set_response_enabled(
                        "plan",
                        orbital_vault::tags::valid_tag_name(
                            entry.text().trim().trim_start_matches('#'),
                        ),
                    );
                }
            });
            if dialog.choose_future(Some(&this.0.window)).await != "plan"
                || !this.same_vault(&vault, session)
            {
                return;
            }
            let new = field.text().trim().trim_start_matches('#').to_owned();
            if !orbital_vault::tags::valid_tag_name(&new) || new == old {
                return;
            }
            this.0.code_editor.flush().await;
            if !this.same_vault(&vault, session) || this.0.code_editor.has_pending() {
                return this
                    .toast("Guardá o resolvé los cambios pendientes antes de renombrar etiquetas");
            }
            let (v, from, to) = (vault.clone(), old.clone(), new.clone());
            let index = this.0.search_index.borrow().clone();
            let built = gio::spawn_blocking(move || {
                let sources: Vec<_> = v
                    .entries()
                    .iter()
                    .filter(|entry| {
                        entry.zone == orbital_vault::Zone::Wiki
                            && Format::from_name(entry.path.file_name()).is_some()
                    })
                    .map(|entry| entry.path.clone())
                    .collect();
                let plan = v.plan_tag_rename(&from, &to, &sources);
                let merging = orbital_vault::fold(&from) != orbital_vault::fold(&to)
                    && index
                        .as_ref()
                        .and_then(|index| index.read().ok())
                        .is_some_and(|index| {
                            index
                                .tag_names()
                                .iter()
                                .any(|name| orbital_vault::fold(name) == orbital_vault::fold(&to))
                        });
                (plan, merging)
            })
            .await;
            if !this.same_vault(&vault, session) {
                return;
            }
            let Ok((plan, merging)) = built else {
                return this.toast("No se pudo preparar el renombre");
            };
            if let Some((path, error)) = plan.errors.first() {
                return this.toast(&format!(
                    "No se aplicó el renombre: {} notas no pudieron revisarse. {path}: {error}",
                    plan.errors.len()
                ));
            }
            if plan.is_empty() {
                return this.toast("No se encontraron notas para renombrar");
            }
            let confirm = adw::AlertDialog::new(
                Some("¿Renombrar la etiqueta?"),
                Some(&format!(
                    "«#{old}» y sus hijas pasarán a «#{new}» en {} notas.{}",
                    plan.files(),
                    if merging {
                        " El destino ya existe: las etiquetas se fusionarán."
                    } else {
                        ""
                    }
                )),
            );
            confirm.add_response("cancel", "Cancelar");
            confirm.add_response("rename", "Renombrar");
            confirm.set_response_appearance("rename", adw::ResponseAppearance::Suggested);
            confirm.set_close_response("cancel");
            if confirm.choose_future(Some(&this.0.window)).await != "rename"
                || !this.same_vault(&vault, session)
            {
                return;
            }
            this.run_tag_edits(vault, session, Arc::new(plan), false)
                .await;
        });
    }

    pub(super) async fn run_tag_edits(
        &self,
        vault: Arc<Vault>,
        session: u64,
        plan: Arc<orbital_vault::EditPlan>,
        undo: bool,
    ) {
        if !self.same_vault(&vault, session) || self.0.tag_operation.get() {
            return;
        }
        self.0.code_editor.flush().await;
        if !self.same_vault(&vault, session)
            || self.0.code_editor.has_pending()
            || self.0.tag_operation.get()
        {
            return self.toast("Hay cambios pendientes: no se modificaron las etiquetas");
        }
        self.0.tag_operation.set(true);
        self.0.window.set_sensitive(false);
        let (v, p) = (vault.clone(), plan.clone());
        let result = gio::spawn_blocking(move || {
            let ctx = write_ctx::now(Some(v.config()));
            if undo {
                v.revert_edits(&p, &ctx)
            } else {
                v.apply_edits(&p, &ctx)
            }
        })
        .await;
        if self.same_vault(&vault, session) {
            match result {
                Ok((receipt, failed)) => {
                    for (path, hash) in &receipt.written {
                        self.0.own_writes.borrow_mut().insert(path.clone(), *hash);
                        self.update_graph(path.clone(), false);
                        self.update_search_index(path.clone(), false);
                    }
                    if let Some(open) =
                        self.0.code_editor.current_path().filter(|path| {
                            receipt.written.iter().any(|(written, _)| written == path)
                        })
                    {
                        let (v, path) = (vault.clone(), open.clone());
                        let note = gio::spawn_blocking(move || v.read_note(&path))
                            .await
                            .ok()
                            .and_then(Result::ok);
                        if self.same_vault(&vault, session)
                            && self.0.code_editor.current_path().as_ref() == Some(&open)
                            && !self.0.code_editor.has_pending()
                            && let Some(note) = note
                        {
                            self.0.code_editor.load(vault.clone(), &note);
                        }
                    }
                    let message = format!(
                        "{} notas {}; {} sin modificar{}",
                        receipt.written.len(),
                        if undo { "restauradas" } else { "actualizadas" },
                        failed.len(),
                        if failed.is_empty() {
                            ""
                        } else {
                            " (cambios externos o error de escritura)"
                        }
                    );
                    let applied = Arc::new(plan.applied(&receipt));
                    if !undo && !applied.is_empty() {
                        let (this, captured) = (self.clone(), vault.clone());
                        self.toast_undo(&message, move || {
                            let (this, v, p) = (this.clone(), captured.clone(), applied.clone());
                            glib::spawn_future_local(async move {
                                this.run_tag_edits(v, session, p, true).await;
                            });
                        });
                    } else {
                        self.toast(&message);
                    }
                }
                Err(_) => self.toast("No se pudo completar la edición de etiquetas"),
            }
        }
        self.0.window.set_sensitive(true);
        self.0.tag_operation.set(false);
        let deferred = std::mem::take(&mut *self.0.tag_disk_changes.borrow_mut());
        if !deferred.is_empty() {
            self.on_disk_changes(deferred);
        }
    }

    pub fn focus_search(&self) {
        self.0.sidebar_stack.set_visible_child_name("busqueda");
        self.0.search_panel.grab_focus();
    }

    pub fn toggle_search_raw(&self) {
        self.0.search_panel.toggle_raw();
    }
}
