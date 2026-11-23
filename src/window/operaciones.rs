//! Crear, renombrar, archivar, borrar y restaurar notas.

use super::*;

impl Window {
    /// Arma el menú «Nueva nota» con las plantillas de la bóveda.
    pub(super) fn fill_new_menu(&self, templates: &[RelPath]) {
        let Some(popover) = self.0.new_button.popover() else {
            return;
        };
        let mut items = vec![
            MenuItem::new("Nota", "win.new-note", "Ctrl+N"),
            MenuItem::new("Nota HTML", "win.new-html", ""),
            MenuItem::new("Diario de hoy", "win.journal", "Ctrl+Alt+D"),
        ];
        for t in templates {
            let name = t.file_name();
            let stem = name.rsplit_once('.').map_or(name, |(s, _)| s);
            let mut item = MenuItem::new(
                &format!("Desde la plantilla «{stem}»"),
                "win.new-from-template",
                "",
            );
            item.target = Some(t.to_string());
            items.push(item);
        }
        fill_menu(&popover, &items);
    }

    /// Carpeta donde se crean las notas nuevas: la de la nota abierta, o `wiki/`.
    pub(super) fn target_dir(&self) -> Option<RelPath> {
        let vault = self.0.vault.borrow().clone()?;
        let from_current = self.0.current_entry.borrow().as_ref().and_then(|e| {
            let s = e.path.as_str();
            s.rsplit_once('/')
                .filter(|(d, _)| d.starts_with("wiki"))
                .and_then(|(d, _)| RelPath::new(d).ok())
        });
        from_current.or_else(|| {
            vault
                .root()
                .join("wiki")
                .is_dir()
                .then(|| RelPath::new("wiki").ok())
                .flatten()
        })
    }

    /// Pide el nombre de una nota nueva y la crea.
    pub(super) fn ask_new_note(&self, kind: NewKind) {
        let Some(vault) = self.0.vault.borrow().clone() else {
            return;
        };
        let this = self.clone();
        glib::spawn_future_local(async move {
            let field = gtk::Entry::builder().activates_default(true).build();
            field.update_property(&[gtk::accessible::Property::Label("Nombre de la nota")]);
            let dialog = adw::AlertDialog::new(Some("Nota nueva"), Some("¿Cómo se llama?"));
            dialog.set_extra_child(Some(&field));
            dialog.add_response("cancel", "Cancelar");
            dialog.add_response("create", "Crear");
            dialog.set_response_appearance("create", adw::ResponseAppearance::Suggested);
            dialog.set_default_response(Some("create"));
            dialog.set_close_response("cancel");
            let f = field.clone();
            glib::idle_add_local_once(move || {
                f.grab_focus();
            });
            if dialog.choose_future(Some(&this.0.window)).await != "create" {
                return;
            }
            let name = field.text().trim().to_owned();
            if name.is_empty() || name.contains('/') {
                return this.toast("El nombre no puede estar vacío ni tener «/»");
            }
            warn_portable(&this, &name);
            let dir = this.target_dir();
            let result = gio::spawn_blocking(move || {
                let ctx = write_ctx::now(Some(vault.config()));
                let dir = match dir {
                    Some(d) => d,
                    None => return Err(orbital_vault::VaultError::InvalidPath(name)),
                };
                match kind {
                    NewKind::Md => vault
                        .create_note(&dir, &name, Format::Md, &ctx)
                        .map(|p| (p, Vec::new())),
                    NewKind::Html => vault
                        .create_note(&dir, &name, Format::Html, &ctx)
                        .map(|p| (p, Vec::new())),
                    NewKind::Template(t) => {
                        let t = RelPath::new(&t)?;
                        vault.create_from_template(&dir, &name, &t, &ctx)
                    }
                }
            })
            .await;
            match result {
                Ok(Ok((path, unknown))) => {
                    this.reload(Some(path.clone())).await;
                    if unknown.is_empty() {
                        this.toast(&format!("Se creó «{path}»"));
                    } else {
                        this.toast(&format!(
                            "Se creó «{path}»; la plantilla usa variables desconocidas: {}",
                            unknown.join(", ")
                        ));
                    }
                }
                Ok(Err(e)) => this.toast(&e.to_string()),
                Err(_) => this.toast("No se pudo crear la nota"),
            }
        });
    }

    /// «Nota nueva con esta etiqueta» (036): pide el nombre y crea la nota `.md` en la carpeta de
    /// la nota abierta (la del índice), con `tags: [etiqueta]`, y la abre. El índice la toma con
    /// la actualización del índice de búsqueda.
    pub(super) fn ask_new_note_tagged(&self, tag: String) {
        let Some(vault) = self.0.vault.borrow().clone() else {
            return;
        };
        let tag = tag.trim().trim_start_matches('#').to_owned();
        if tag.is_empty() {
            return;
        }
        let this = self.clone();
        glib::spawn_future_local(async move {
            let field = gtk::Entry::builder().activates_default(true).build();
            field.update_property(&[gtk::accessible::Property::Label("Nombre de la nota")]);
            let dialog = adw::AlertDialog::new(
                Some("Nota nueva"),
                Some(&format!("Se crea con la etiqueta #{tag}. ¿Cómo se llama?")),
            );
            dialog.set_extra_child(Some(&field));
            dialog.add_response("cancel", "Cancelar");
            dialog.add_response("create", "Crear");
            dialog.set_response_appearance("create", adw::ResponseAppearance::Suggested);
            dialog.set_default_response(Some("create"));
            dialog.set_close_response("cancel");
            let f = field.clone();
            glib::idle_add_local_once(move || {
                f.grab_focus();
            });
            if dialog.choose_future(Some(&this.0.window)).await != "create" {
                return;
            }
            let name = field.text().trim().to_owned();
            if name.is_empty() || name.contains('/') {
                return this.toast("El nombre no puede estar vacío ni tener «/»");
            }
            warn_portable(&this, &name);
            // La carpeta de la nota del índice; en la raíz (o sin nota), la de las notas nuevas.
            let dir = this
                .0
                .current_entry
                .borrow()
                .as_ref()
                .and_then(|e| e.path.as_str().rsplit_once('/').map(|(d, _)| d.to_owned()))
                .and_then(|d| RelPath::new(&d).ok())
                .or_else(|| this.target_dir());
            let result = gio::spawn_blocking(move || {
                let ctx = write_ctx::now(Some(vault.config()));
                let Some(dir) = dir else {
                    return Err(orbital_vault::VaultError::InvalidPath(name));
                };
                vault.create_note_with_tags(&dir, &name, &[tag], &ctx)
            })
            .await;
            match result {
                Ok(Ok(path)) => {
                    this.reload(Some(path.clone())).await;
                    this.toast(&format!("Se creó «{path}»"));
                }
                Ok(Err(e)) => this.toast(&e.to_string()),
                Err(_) => this.toast("No se pudo crear la nota"),
            }
        });
    }

    /// «Diario de hoy»: lo abre, creándolo si no existe.
    pub(super) fn open_journal(&self) {
        let Some(vault) = self.0.vault.borrow().clone() else {
            return;
        };
        let this = self.clone();
        glib::spawn_future_local(async move {
            let result = gio::spawn_blocking(move || {
                let ctx = write_ctx::now(Some(vault.config()));
                vault.open_journal(&ctx)
            })
            .await;
            match result {
                Ok(Ok(path)) => this.reload(Some(path)).await,
                Ok(Err(e)) => this.toast(&e.to_string()),
                Err(_) => this.toast("No se pudo abrir el diario"),
            }
        });
    }

    /// «Archivar» la nota abierta (o desarchivar, al deshacer).
    pub(super) fn archive_current(&self) {
        let Some(entry) = self.0.current_entry.borrow().clone() else {
            return;
        };
        let this = self.clone();
        glib::spawn_future_local(async move { this.set_archived(entry.path.clone(), true).await });
    }

    pub(super) async fn set_archived(&self, path: RelPath, archived: bool) {
        self.0.code_editor.flush().await;
        if self.0.code_editor.has_pending() {
            return;
        }
        let Some(vault) = self.0.vault.borrow().clone() else {
            return;
        };
        let p = path.clone();
        let result = gio::spawn_blocking(move || {
            let ctx = write_ctx::now(Some(vault.config()));
            vault.archive(&p, archived, &ctx)
        })
        .await;
        match result {
            Ok(Ok(_)) if archived => {
                self.reload(None).await;
                let this = self.clone();
                self.toast_undo("Nota archivada", move || {
                    let (this, path) = (this.clone(), path.clone());
                    glib::spawn_future_local(async move { this.set_archived(path, false).await });
                });
            }
            Ok(Ok(_)) => self.reload(Some(path)).await,
            Ok(Err(e)) => self.toast(&e.to_string()),
            Err(_) => self.toast("No se pudo archivar"),
        }
    }

    /// «Renombrar o mover»: pide la ruta nueva (relativa a la bóveda).
    pub(super) fn ask_rename(&self) {
        let Some(entry) = self.0.current_entry.borrow().clone() else {
            return;
        };
        let this = self.clone();
        glib::spawn_future_local(async move {
            let field = gtk::Entry::builder()
                .text(entry.path.as_str())
                .activates_default(true)
                .build();
            field.update_property(&[gtk::accessible::Property::Label("Ruta nueva")]);
            let dialog = adw::AlertDialog::new(
                Some("Renombrar o mover"),
                Some("Ruta dentro de la bóveda. Cambiá el nombre o la carpeta."),
            );
            dialog.set_extra_child(Some(&field));
            dialog.add_response("cancel", "Cancelar");
            dialog.add_response("rename", "Renombrar");
            dialog.set_response_appearance("rename", adw::ResponseAppearance::Suggested);
            dialog.set_default_response(Some("rename"));
            dialog.set_close_response("cancel");
            // Selecciona el nombre sin la extensión, que es lo que casi siempre se cambia.
            let path = entry.path.as_str().to_owned();
            let start = path.rfind('/').map_or(0, |p| p + 1);
            let end = path.rfind('.').filter(|e| *e > start).unwrap_or(path.len());
            let f = field.clone();
            glib::idle_add_local_once(move || {
                f.grab_focus();
                f.select_region(start as i32, end as i32);
            });
            if dialog.choose_future(Some(&this.0.window)).await != "rename" {
                return;
            }
            let Ok(to) = RelPath::new(field.text().trim()) else {
                return this.toast("La ruta nueva no es válida");
            };
            warn_portable(&this, to.file_name());
            if to == entry.path {
                return;
            }
            this.do_rename(entry.path.clone(), to, true).await;
        });
    }

    /// Renombra en segundo plano; si `undoable`, ofrece deshacer con el renombre inverso. Si la
    /// nota tiene enlaces entrantes, ofrece actualizarlos (US6 de la 003).
    pub(super) async fn do_rename(&self, from: RelPath, to: RelPath, undoable: bool) {
        self.0.code_editor.flush().await;
        if self.0.code_editor.has_pending() {
            return;
        }
        // «Solo en Drive» (061b): se renombra o mueve en Drive (mismo id); no hay nada en el disco.
        // Sin conexión queda en cola: el explorador lo muestra ya, «pendiente», y se aplica en
        // Drive al volver la red (aunque se cierre Orbital).
        if self.drive_es_marcador(from.as_str()) {
            let en_linea = self.drive_en_linea();
            self.drive_orden(orbital_drive::Orden::Mover {
                de: from.as_str().to_owned(),
                a: to.as_str().to_owned(),
            });
            return self.toast(&if en_linea {
                format!("«{}» se renombra en Drive", from.file_name())
            } else {
                format!(
                    "Sin conexión: «{}» se renombra en Drive cuando vuelva la red",
                    from.file_name()
                )
            });
        }
        let Some(vault) = self.0.vault.borrow().clone() else {
            return;
        };
        // Los enlaces que hoy apuntan a la nota (antes de renombrar: la resolución mira la bóveda
        // actual). Con el grafo armado se revisan solo sus retroenlaces; si no, toda la bóveda.
        let mut plan = None;
        if undoable && Format::from_name(from.file_name()).is_some() {
            let sources: Option<Vec<RelPath>> = self.0.graph.borrow().as_ref().and_then(|g| {
                g.lock().ok().map(|g| {
                    let mut v: Vec<RelPath> = g
                        .backlinks_for(&from)
                        .iter()
                        .map(|b| b.from.clone())
                        .collect();
                    v.sort();
                    v.dedup();
                    v
                })
            });
            let (v, f, t) = (vault.clone(), from.clone(), to.clone());
            let found =
                gio::spawn_blocking(move || v.plan_link_updates(&f, &t, sources.as_deref()))
                    .await
                    .unwrap_or_default();
            if !found.is_empty() {
                match self.ask_link_update(&from, &found).await {
                    LinkAnswer::Cancel => return,
                    LinkAnswer::Skip => {}
                    LinkAnswer::Update => plan = Some(Arc::new(found)),
                }
            }
        }
        let (f, t, p) = (from.clone(), to.clone(), plan.clone());
        let result = gio::spawn_blocking(move || {
            let ctx = write_ctx::now(Some(vault.config()));
            let renamed = vault.rename(&f, &t, &ctx)?;
            // Los enlaces se escriben después de renombrar (la nota puede enlazarse a sí misma).
            let (receipt, failed) = p.map_or_else(
                || (orbital_vault::WriteReceipt::default(), Vec::new()),
                |p| vault.apply_link_updates(&p, &ctx),
            );
            // La URL asociada migra con el archivo (009, FR-3); los errores van junto a los demás.
            let enlaces_err = vault.migrar_enlace(&f, &t, &ctx).err();
            Ok::<_, orbital_vault::VaultError>((renamed, receipt, failed, enlaces_err))
        })
        .await;
        match result {
            Ok(Ok((_, receipt, failed, enlaces_err))) => {
                let plan = plan.as_ref().map(|plan| Arc::new(plan.applied(&receipt)));
                // La ruta cambió: el editor vuelve a abrir la nota en su lugar nuevo.
                self.0.code_editor.close();
                self.reload(Some(to.clone())).await;
                // Favoritos y Guardados siguen al archivo o a la carpeta (027, FR-008); también
                // en el deshacer, que llama a `do_rename` al revés. Si no se pueden escribir,
                // `aplicar_listas` lo avisa aparte.
                self.seguir_renombre_en_listas(&from, &to);
                // El plegado también (028).
                self.seguir_renombre_en_plegado(&from, &to);
                if let Some(error) = enlaces_err {
                    self.toast(&format!("No se pudo migrar la URL asociada: {error}"));
                }
                if !failed.is_empty() {
                    let names: Vec<&str> = failed.iter().map(|(p, _)| p.file_name()).collect();
                    self.toast(&format!(
                        "No se actualizaron los enlaces de {} (cambiaron mientras tanto)",
                        names.join(", ")
                    ));
                }
                if undoable {
                    let this = self.clone();
                    let message = match &plan {
                        Some(p) => format!(
                            "«{}» ahora es «{to}»; {} {} actualizados",
                            from.file_name(),
                            p.links(),
                            if p.links() == 1 { "enlace" } else { "enlaces" }
                        ),
                        None => format!("«{}» ahora es «{to}»", from.file_name()),
                    };
                    self.toast_undo(&message, move || {
                        let (this, from, to, plan) =
                            (this.clone(), from.clone(), to.clone(), plan.clone());
                        glib::spawn_future_local(async move {
                            // Primero los enlaces, después el nombre: todo vuelve a ser igual.
                            let vault = this.0.vault.borrow().clone();
                            if let (Some(plan), Some(vault)) = (plan, vault) {
                                this.0.code_editor.flush().await;
                                let failed = gio::spawn_blocking(move || {
                                    let ctx = write_ctx::now(Some(vault.config()));
                                    vault.revert_link_updates(&plan, &ctx).1
                                })
                                .await
                                .unwrap_or_default();
                                if !failed.is_empty() {
                                    this.toast(
                                        "Algunos enlaces cambiaron después y no se revirtieron",
                                    );
                                }
                            }
                            this.do_rename(to, from, false).await;
                        });
                    });
                }
            }
            Ok(Err(e)) => self.toast(&e.to_string()),
            Err(_) => self.toast("No se pudo renombrar"),
        }
    }

    /// «Se encontraron N enlaces a esta nota en M archivos. ¿Querés actualizarlos?» (FR-018).
    pub(super) async fn ask_link_update(
        &self,
        from: &RelPath,
        plan: &orbital_vault::LinkUpdatePlan,
    ) -> LinkAnswer {
        let (links, files) = (plan.links(), plan.files());
        let dialog = adw::AlertDialog::new(
            Some("¿Actualizar los enlaces?"),
            Some(&format!(
                "Se {} {links} {} a «{}» en {files} {}. ¿Querés actualizarlos para que sigan \
                 apuntando a la nota?",
                if links == 1 {
                    "encontró"
                } else {
                    "encontraron"
                },
                if links == 1 { "enlace" } else { "enlaces" },
                from.file_name(),
                if files == 1 { "archivo" } else { "archivos" },
            )),
        );
        dialog.add_response("cancel", "Cancelar");
        dialog.add_response("skip", "No actualizar");
        dialog.add_response("update", "Actualizar enlaces");
        dialog.set_response_appearance("update", adw::ResponseAppearance::Suggested);
        dialog.set_default_response(Some("update"));
        dialog.set_close_response("cancel");
        match dialog.choose_future(Some(&self.0.window)).await.as_str() {
            "update" => LinkAnswer::Update,
            "skip" => LinkAnswer::Skip,
            _ => LinkAnswer::Cancel,
        }
    }

    /// «Mover a la papelera», con confirmación.
    pub(super) fn ask_trash(&self) {
        let Some(entry) = self.0.current_entry.borrow().clone() else {
            return;
        };
        let this = self.clone();
        glib::spawn_future_local(async move {
            let dialog = adw::AlertDialog::new(
                Some(&format!(
                    "¿Mover «{}» a la papelera?",
                    entry.path.file_name()
                )),
                Some("Vas a poder recuperarla con «Deshacer» o desde la papelera de GNOME."),
            );
            dialog.add_response("cancel", "Cancelar");
            dialog.add_response("trash", "Mover a la papelera");
            dialog.set_response_appearance("trash", adw::ResponseAppearance::Destructive);
            dialog.set_close_response("cancel");
            if dialog.choose_future(Some(&this.0.window)).await == "trash" {
                this.do_trash(entry.path.clone()).await;
            }
        });
    }

    pub(super) async fn do_trash(&self, path: RelPath) {
        self.0.code_editor.flush().await;
        if self.0.code_editor.has_pending() {
            return;
        }
        // «Solo en Drive» (061b): a la papelera de Drive (se recupera desde la web).
        // Sin conexión queda en cola (sale ya del explorador; va a la papelera al volver la red).
        if self.drive_es_marcador(path.as_str()) {
            let en_linea = self.drive_en_linea();
            self.drive_orden(orbital_drive::Orden::Borrar(path.as_str().to_owned()));
            return self.toast(&if en_linea {
                format!(
                    "«{}» se mandó a la papelera de Google Drive",
                    path.file_name()
                )
            } else {
                format!(
                    "Sin conexión: «{}» va a la papelera de Google Drive cuando vuelva la red",
                    path.file_name()
                )
            });
        }
        let Some(vault) = self.0.vault.borrow().clone() else {
            return;
        };
        let abs = path.to_path(vault.root());
        let (v, p) = (vault.clone(), path.clone());
        let plan = match gio::spawn_blocking(move || v.delete(&p)).await {
            Ok(Ok(plan)) => plan,
            Ok(Err(e)) => return self.toast(&e.to_string()),
            Err(_) => return self.toast("No se pudo borrar"),
        };
        // Papelera del sistema; si ese disco no tiene (tmpfs, FAT…), la papelera de la bóveda.
        let in_vault: Option<PathBuf> = match gio::File::for_path(&abs)
            .trash_future(glib::Priority::DEFAULT)
            .await
        {
            Ok(()) => None,
            Err(_) => {
                let (v, p) = (vault.clone(), path.clone());
                let moved = gio::spawn_blocking(move || {
                    let ctx = write_ctx::now(Some(v.config()));
                    v.move_to_vault_trash(&p, &ctx)
                })
                .await;
                match moved {
                    Ok(Ok(dest)) => Some(dest),
                    Ok(Err(e)) => return self.toast(&format!("No se pudo borrar: {e}")),
                    Err(_) => return self.toast("No se pudo borrar"),
                }
            }
        };
        let v = vault.clone();
        let for_log = plan.clone();
        let _ = gio::spawn_blocking(move || {
            let ctx = write_ctx::now(Some(v.config()));
            v.after_delete(&for_log, &ctx)
        })
        .await;
        self.0.code_editor.close();
        self.reload(None).await;
        // Sale de Favoritos y de las colecciones (027, FR-008); «Deshacer» la repone.
        let quitadas = self.quitar_de_listas(&path);
        let quitadas_plegado = self.quitar_de_plegado(&path);
        let msg = match &in_vault {
            None => format!("«{}» se movió a la papelera", path.file_name()),
            Some(_) => format!(
                "«{}» se movió a la papelera de la bóveda (.orbital/papelera)",
                path.file_name()
            ),
        };
        let this = self.clone();
        self.toast_undo(&msg, move || {
            let (this, plan, abs, in_vault, quitadas, quitadas_plegado) = (
                this.clone(),
                plan.clone(),
                abs.clone(),
                in_vault.clone(),
                quitadas.clone(),
                quitadas_plegado.clone(),
            );
            glib::spawn_future_local(async move {
                let repuesta = match in_vault {
                    None => this.restore(plan, abs).await,
                    Some(trashed) => this.restore_vault_trash(plan, trashed).await,
                };
                if repuesta {
                    this.reponer_en_listas(&quitadas);
                    this.reponer_en_plegado(&quitadas_plegado);
                }
            });
        });
    }

    /// Deshacer un borrado que fue a la papelera de la bóveda. Devuelve si se recuperó.
    pub(super) async fn restore_vault_trash(&self, plan: DeletePlan, trashed: PathBuf) -> bool {
        let Some(vault) = self.0.vault.borrow().clone() else {
            return false;
        };
        let path = plan.path.clone();
        let result = gio::spawn_blocking(move || {
            vault.restore_from(&trashed, &plan.path)?;
            let ctx = write_ctx::now(Some(vault.config()));
            vault.after_restore(&plan, &ctx)
        })
        .await;
        match result {
            Ok(Ok(_)) => {
                self.reload(Some(path)).await;
                true
            }
            Ok(Err(e)) => {
                self.toast(&format!("No se pudo recuperar: {e}"));
                false
            }
            Err(_) => {
                self.toast("No se pudo recuperar");
                false
            }
        }
    }

    /// Deshacer un borrado: busca el archivo en la papelera por su ruta original y lo devuelve.
    /// Devuelve si se recuperó.
    pub(super) async fn restore(&self, plan: DeletePlan, abs: PathBuf) -> bool {
        let path = plan.path.clone();
        let Some(vault) = self.0.vault.borrow().clone() else {
            return false;
        };
        match find_in_trash(&abs).await {
            Some(trashed) => {
                let dest = gio::File::for_path(&abs);
                // Mover desde la papelera es local y rápido, pero igual va fuera del hilo principal.
                let moved = gio::spawn_blocking(move || {
                    trashed.move_(
                        &dest,
                        gio::FileCopyFlags::NONE,
                        None::<&gio::Cancellable>,
                        None,
                    )
                })
                .await;
                match moved {
                    Ok(Ok(())) => {}
                    Ok(Err(e)) => {
                        self.toast(&format!("No se pudo recuperar: {e}"));
                        return false;
                    }
                    Err(_) => {
                        self.toast("No se pudo recuperar");
                        return false;
                    }
                }
                let v = vault.clone();
                let _ = gio::spawn_blocking(move || {
                    let ctx = write_ctx::now(Some(v.config()));
                    v.after_restore(&plan, &ctx)
                })
                .await;
                self.reload(Some(path)).await;
                true
            }
            None => {
                self.toast("No encontré el archivo en la papelera");
                false
            }
        }
    }
}
