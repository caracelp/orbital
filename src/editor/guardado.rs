//! Carga, cambios, autoguardado con GuardadoSeguro, conflictos con el disco y deshacer.

use super::*;

impl Editor {
    /// Carga una nota en el editor. El texto cargado no se puede deshacer.
    pub fn load(&self, vault: Arc<Vault>, note: &Note) {
        let (session, text) = EditSession::open(note);
        let i = &self.0;
        i.extra.borrow_mut().clear(&i.buffer);
        i.caren.limpiar();
        i.reader.reset();
        i.visual.forget();
        i.visual_pending.set(false);
        i.visual_gen.set(i.visual_gen.get() + 1);
        if i.format.get() != note.format || i.switcher.n_toggles() == 0 {
            i.switching.set(true);
            fill_toggles(&i.switcher, note.format);
            i.switching.set(false);
        }
        i.format.set(note.format);
        self.reiniciar_plegado();
        let scripts =
            note.format == Format::Html && orbital_vault::html_edit::body_has_scripts(&text);
        i.html_scripts.set(scripts);
        if let Some(t) = i.switcher.toggle_by_name("visual") {
            t.set_enabled(!scripts);
            t.set_tooltip(if scripts {
                "Esta nota tiene JavaScript: editala en Código"
            } else {
                "Visual (Ctrl+2)"
            });
        }
        code::set_language(
            &i.buffer,
            match note.format {
                Format::Md => "markdown",
                Format::Html => "html",
            },
        );
        i.loading.set(true);
        i.buffer.begin_irreversible_action();
        i.buffer.set_text(&text);
        i.buffer.end_irreversible_action();
        i.buffer.place_cursor(&i.buffer.start_iter());
        i.loading.set(false);
        i.view.set_editable(!note.read_only);
        i.generation.set(i.generation.get() + 1);
        *i.base_dir.borrow_mut() = note.path.to_path(vault.root()).parent().map(Into::into);
        *i.session.borrow_mut() = Some(session);
        // El árbol de plegables (028), en otro hilo: hasta que llega, las flechitas usan la regla
        // local de cada línea.
        self.reconciliar_en(Duration::ZERO);
        let vault_weak = Arc::downgrade(&vault);
        let from_note = note.path.clone();
        i.reader.set_is_missing(move |url| {
            if url.starts_with("orbital-tag:")
                || url.starts_with(crate::editor::reader::NOTA_NUEVA)
                || url.starts_with("http://")
                || url.starts_with("https://")
                || url.starts_with("mailto:")
            {
                return false;
            }
            let Some(v) = vault_weak.upgrade() else {
                return false;
            };
            v.resolve_link(&from_note, url) == orbital_vault::Resolution::Missing
        });
        *i.vault.borrow_mut() = Some(vault.clone());
        self.refresh_completion_candidates(&vault);
        let mut meta = note.clone();
        meta.source = String::new();
        *i.meta.borrow_mut() = Some(meta);
        self.emit(&Status::Saved(None));
        let mut v = match note.format {
            Format::Md => i.md_pref.get(),
            Format::Html => i.html_pref.get(),
        };
        if v == View::Visual && (scripts || note.read_only) {
            v = View::Codigo;
        }
        self.enter(v);
        if i.find_bar.is_search_mode() {
            let query = i.find_bar.search_text();
            self.find_search(&query);
        }
    }

    /// Cambia (o quita, con `None`) una propiedad sobre el texto del editor, no sobre el disco
    /// (research R7): edita solo sus líneas y lo guarda el autoguardado. Con `rebuild` en `false`,
    /// la lista ya muestra el valor nuevo y no se vuelve a armar (la fila no pierde el foco).
    pub fn edit_prop(
        &self,
        key: &str,
        value: Option<&orbital_vault::props::PropValue>,
        rebuild: bool,
    ) -> Result<(), String> {
        let i = &self.0;
        let b = &i.buffer;
        let text = b.text(&b.start_iter(), &b.end_iter(), true).to_string();
        let note = i
            .meta
            .borrow()
            .as_ref()
            .ok_or("No hay ninguna nota abierta")?
            .reparse(text.clone())
            .map_err(|e| e.to_string())?;
        let new = match value {
            Some(v) => note.with_prop(key, v),
            None => note.without_prop(key),
        }
        .map_err(|e| e.to_string())?;
        if new == text {
            return Ok(());
        }
        if !rebuild {
            let h = orbital_vault::props::markdown::locate(&new)
                .map_or(0, |r| orbital_vault::write::hash(new[r].as_bytes()));
            i.reader.adopt_header(h);
        }
        replace_changed(b, &text, &new);
        if i.mode.get() == View::Visual && rebuild {
            self.visual_header();
        }
        Ok(())
    }

    /// Deshace la última acción en el buffer de código.
    pub fn undo(&self) -> bool {
        let b = &self.0.buffer;
        if b.can_undo() {
            b.undo();
            true
        } else {
            false
        }
    }

    /// Deshace el último cambio si nada cambió después (para el «Deshacer» de quitar una propiedad).
    pub fn undo_if_last(&self, edits: u64) -> bool {
        let b = &self.0.buffer;
        if self.0.edits.get() != edits || !b.can_undo() {
            return false;
        }
        b.undo();
        true
    }

    /// Contador de cambios del texto (ver [`Editor::undo_if_last`]).
    pub fn edits(&self) -> u64 {
        self.0.edits.get()
    }

    /// Una nota de la bóveda cambió (se guardó o la cambió otro programa). Si la nota abierta la
    /// incrusta con `![[…]]`, la vista formateada se vuelve a dibujar: solo cambia esa tarjeta y
    /// no se pierde la posición de lectura (FR-016).
    pub fn dependency_changed(&self, path: &RelPath) {
        let has_base = self
            .0
            .embed_deps
            .borrow()
            .iter()
            .any(|p| p.as_str().ends_with(".base"));
        if !has_base && !self.0.embed_deps.borrow().contains(path) {
            return;
        }
        let this = self.clone();
        let vault = self.0.vault.borrow().clone();
        let generation = self.0.generation.get();
        let current = self.current_path();
        let path = path.clone();
        glib::spawn_future_local(async move {
            if let Some(vault) = vault {
                let _ = gio::spawn_blocking(move || vault.update_base_index(&path)).await;
            }
            if this.0.generation.get() != generation || this.current_path() != current {
                return;
            }
            if this.0.mode.get() == View::Visual {
                this.visual_embeds();
            } else {
                this.render_now();
            }
        });
    }

    /// Renueva el contexto de archivos sin reemplazar el borrador ni su deshacer.
    pub fn refresh_vault_context(&self, vault: Arc<Vault>) {
        *self.0.vault.borrow_mut() = Some(vault.clone());
        self.refresh_completion_candidates(&vault);
        if self.0.mode.get() == View::Visual {
            self.visual_embeds();
        } else {
            self.render_now();
        }
    }

    /// Cierra la nota sin guardar (después de un `flush`, o para recargarla).
    pub fn close(&self) {
        self.0.extra.borrow_mut().clear(&self.0.buffer);
        self.0.caren.limpiar();
        self.0.completion.popdown();
        self.0.generation.set(self.0.generation.get() + 1);
        self.0.reader.reset();
        self.reiniciar_plegado();
        *self.0.session.borrow_mut() = None;
    }

    pub(super) fn on_changed(&self) {
        let i = &self.0;
        if i.loading.get() || i.asignando_ids.get() {
            return;
        }
        self.reconciliar_pronto();
        if let Some(f) = i.on_text_changed.borrow().as_ref() {
            f();
        }
        if i.format.get() == Format::Html && i.mode.get() == View::Codigo {
            // Se vuelve a validar al entrar a Visual, no en cada tecla.
            i.html_scripts.set(false);
            if let Some(toggle) = i.switcher.toggle_by_name("visual") {
                toggle.set_enabled(true);
                toggle.set_tooltip("Visual (Ctrl+2)");
            }
        }
        i.edits.set(i.edits.get() + 1);
        self.check_link_completion();
        let Some(session) = i.session.borrow_mut().as_mut().map(|s| {
            s.edited();
            s.state().clone()
        }) else {
            return;
        };
        if i.saving.get() {
            i.edited_during_save.set(true);
        }
        if matches!(session, SessionState::Conflict { .. }) {
            self.emit(&Status::Conflict);
            return;
        }
        self.emit(&Status::Pending);
        self.schedule(AUTOSAVE);
        if i.mode.get() != View::Codigo {
            let generation = i.render_generation.get() + 1;
            i.render_generation.set(generation);
            let this = self.clone();
            glib::timeout_add_local_once(RENDER_DELAY, move || {
                if this.0.render_generation.get() == generation {
                    this.render_now();
                }
            });
        }
    }

    /// Guarda dentro de `delay`, salvo que llegue otra tecla antes.
    pub(super) fn schedule(&self, delay: Duration) {
        let generation = self.0.generation.get() + 1;
        self.0.generation.set(generation);
        let this = self.clone();
        glib::timeout_add_local_once(delay, move || {
            if this.0.generation.get() == generation {
                glib::spawn_future_local(async move { this.save_now(true).await });
            }
        });
    }

    /// Guarda ya lo pendiente y espera a que termine (FR-002).
    pub async fn flush(&self) {
        self.sync_visual().await;
        if self.0.visual_pending.get() {
            self.error("No se pudo sincronizar la edición visual; los cambios siguen en la página");
            return;
        }
        // Si hay una escritura en curso, espera a que termine y vuelve a revisar.
        while self.0.saving.get() {
            glib::timeout_future(Duration::from_millis(20)).await;
        }
        self.0.generation.set(self.0.generation.get() + 1);
        self.save_now(false).await;
    }

    /// `cursores`: el bloque de cada cursor todavía no recibe id (autoguardado); al dejar la nota,
    /// sí (026).
    pub(super) async fn save_now(&self, cursores: bool) {
        let i = &self.0;
        if i.saving.get() {
            return;
        }
        let Some(vault) = i.vault.borrow().clone() else {
            return;
        };
        let texto_buffer = || {
            i.buffer
                .text(&i.buffer.start_iter(), &i.buffer.end_iter(), true)
                .to_string()
        };
        let mut text = texto_buffer();
        let mut job = match i.session.borrow_mut().as_mut() {
            Some(s) => s.prepare_save(&text),
            None => return,
        };
        // Solo si hay algo que guardar: abrir o dejar una nota sin tocarla no la modifica.
        if job.is_some() && self.asignar_ids(&text, cursores) {
            text = texto_buffer();
            job = match i.session.borrow_mut().as_mut() {
                Some(s) => s.prepare_save(&text),
                None => return,
            };
        }
        let Some(job) = job else {
            let state = i.session.borrow().as_ref().map(|s| s.state().clone());
            if let Some(SessionState::Clean) = state {
                self.emit(&Status::Saved(self.last_saved()));
            }
            return;
        };
        i.saving.set(true);
        i.edited_during_save.set(false);
        self.emit(&Status::Saving);
        let ctx = write_ctx::now(Some(vault.config()));
        let at = ctx.time;
        // Historial de versiones (010): el contenido que se está reemplazando queda en
        // .orbital/historial/. Solo notas; los errores son avisos discretos (SC-1: una escritura
        // atómica extra).
        if let Some(path) = self.current_path() {
            let (v, p) = (vault.clone(), path.clone());
            let guardado = gio::spawn_blocking(move || {
                let abs = p.to_path(v.root());
                match std::fs::read(&abs) {
                    Ok(bytes) => match String::from_utf8(bytes) {
                        Ok(previo) => {
                            orbital_vault::historial::guardar_version(&v, &p, &previo, "")
                        }
                        Err(_) => Ok(false),
                    },
                    Err(_) => Ok(false), // primera versión: nada que historiar
                }
            })
            .await;
            if let Ok(Err(error)) = guardado {
                self.error(&format!(
                    "No se pudo guardar la versión en el historial: {error}"
                ));
            }
        }
        let result = gio::spawn_blocking(move || job.run(&vault, &ctx)).await;
        i.saving.set(false);
        let Ok(result) = result else {
            self.error("No se pudo guardar la nota");
            return;
        };
        let edited = i.edited_during_save.get();
        if let orbital_vault::session::SaveResult::Written { hash, also, .. } = &result
            && let (Some(f), Some(path)) = (i.on_saved.borrow().as_ref(), self.current_path())
        {
            f(&path, *hash);
            for (other, h) in also {
                f(other, *h);
            }
        }
        let outcome = match i.session.borrow_mut().as_mut() {
            Some(s) => s.apply(result, at, edited),
            None => return,
        };
        match outcome {
            Ok(SaveOutcome::Saved { .. }) if edited => {
                self.emit(&Status::Pending);
                self.schedule(AUTOSAVE);
            }
            Ok(SaveOutcome::Saved { .. } | SaveOutcome::NothingToSave) => {
                self.emit(&Status::Saved(self.last_saved()));
            }
            Ok(SaveOutcome::Conflict) => {
                self.emit(&Status::Conflict);
                self.error(
                    "La nota cambió en el disco mientras la editabas. Tus cambios siguen acá y no se guardan solos.",
                );
            }
            Err(e) => {
                let failures = i.session.borrow().as_ref().map_or(1, EditSession::failures);
                let wait = RETRIES[(failures as usize).saturating_sub(1).min(RETRIES.len() - 1)];
                self.emit(&Status::Failed(e.to_string()));
                self.error(&format!(
                    "No se pudo guardar: {e}. Reintento en {wait} s; tus cambios siguen acá."
                ));
                self.schedule(Duration::from_secs(wait));
            }
        }
    }

    pub(super) fn last_saved(&self) -> Option<String> {
        self.0
            .session
            .borrow()
            .as_ref()
            .and_then(EditSession::last_saved)
            .map(|t| format!("{:02}:{:02}", t.hour, t.minute))
    }

    /// Reemplaza el texto del editor marcando el cambio como editado (historial 010, restaurar;
    /// también para pruebas).
    pub fn replace_text(&self, new: &str) {
        let b = &self.0.buffer;
        let text = self.text();
        replace_changed(b, &text, new);
    }

    /// «Conservar la mía»: guarda los cambios del editor; la versión del disco queda en respaldo.
    pub async fn keep_mine(&self, vault: Arc<Vault>) -> Result<(), orbital_vault::VaultError> {
        self.sync_visual().await;
        let text = self.text();
        let session = self.0.session.borrow().clone().ok_or_else(|| {
            orbital_vault::VaultError::Io(std::io::Error::other("no hay sesión de edición"))
        })?;
        let (v, ctx) = (vault.clone(), write_ctx::now(Some(vault.config())));
        let res = gio::spawn_blocking(move || {
            let mut s = session;
            s.keep_mine(&v, &text, &ctx).map(|()| s)
        })
        .await
        .map_err(|_| {
            orbital_vault::VaultError::Io(std::io::Error::other("hilo de guardado cancelado"))
        })?;

        match res {
            Ok(updated) => {
                let path = updated.path().clone();
                let hash = updated.base_hash();
                *self.0.session.borrow_mut() = Some(updated);
                if let Some(f) = self.0.on_saved.borrow().as_ref() {
                    f(&path, hash);
                }
                self.emit(&Status::Saved(self.last_saved()));
                Ok(())
            }
            Err(e) => {
                self.emit(&Status::Failed(e.to_string()));
                Err(e)
            }
        }
    }

    /// «Usar la del disco»: carga la versión del disco en el editor; la del editor queda en respaldo.
    pub async fn use_disk(&self, vault: Arc<Vault>) -> Result<(), orbital_vault::VaultError> {
        self.sync_visual().await;
        let mine = self.text();
        let session = self.0.session.borrow().clone().ok_or_else(|| {
            orbital_vault::VaultError::Io(std::io::Error::other("no hay sesión de edición"))
        })?;
        let (v, ctx) = (vault.clone(), write_ctx::now(Some(vault.config())));
        let res = gio::spawn_blocking(move || {
            let mut s = session;
            s.use_disk(&v, &mine, &ctx).map(|disk_text| (s, disk_text))
        })
        .await
        .map_err(|_| {
            orbital_vault::VaultError::Io(std::io::Error::other("hilo de guardado cancelado"))
        })?;

        match res {
            Ok((updated, disk_text)) => {
                *self.0.session.borrow_mut() = Some(updated);
                let i = &self.0;
                // `set_text` deja todas las marcas al comienzo: lo plegado se vuelve a resolver
                // por sus claves sobre el texto nuevo (research R6).
                let claves = self.claves_plegadas();
                self.reiniciar_plegado();
                i.loading.set(true);
                i.buffer.begin_irreversible_action();
                i.buffer.set_text(&disk_text);
                i.buffer.end_irreversible_action();
                i.buffer.place_cursor(&i.buffer.start_iter());
                i.loading.set(false);
                i.generation.set(i.generation.get() + 1);
                self.aplicar_claves(&claves);
                if i.mode.get() == View::Visual {
                    self.show_visual();
                } else if i.mode.get() != View::Codigo {
                    self.render_now();
                }
                self.emit(&Status::Saved(None));
                Ok(())
            }
            Err(e) => {
                self.emit(&Status::Failed(e.to_string()));
                Err(e)
            }
        }
    }

    /// «Descartar a respaldo»: respalda la versión del editor sin tocar el disco y cierra la nota.
    pub async fn discard_to_backup(
        &self,
        vault: Arc<Vault>,
    ) -> Result<(), orbital_vault::VaultError> {
        self.sync_visual().await;
        let mine = self.text();
        let session = self.0.session.borrow().clone().ok_or_else(|| {
            orbital_vault::VaultError::Io(std::io::Error::other("no hay sesión de edición"))
        })?;
        let (v, ctx) = (vault.clone(), write_ctx::now(Some(vault.config())));
        let res = gio::spawn_blocking(move || {
            let mut s = session;
            s.discard_to_backup(&v, &mine, &ctx)
        })
        .await
        .map_err(|_| {
            orbital_vault::VaultError::Io(std::io::Error::other("hilo de guardado cancelado"))
        })?;

        res?;
        self.close();
        Ok(())
    }

    /// Un cambio externo de la nota abierta, con el hash que tiene ahora en el disco.
    pub fn external_change(&self, disk_hash: u64) -> ExternalAction {
        let action = self
            .0
            .session
            .borrow_mut()
            .as_mut()
            .map_or(ExternalAction::Reload, |s| s.external_change(disk_hash));
        if action == ExternalAction::AskUser {
            self.emit(&Status::Conflict);
        }
        action
    }
}
