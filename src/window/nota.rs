//! La nota abierta: mostrarla, propiedades, enlaces, editor y contadores.

use super::*;

impl Window {
    /// Muestra una nota (propiedades + texto) o un archivo de texto.
    pub(super) fn show_entry(&self, entry: Arc<Entry>) {
        let Some(vault) = self.0.vault.borrow().clone() else {
            return;
        };
        if entry.kind == EntryKind::Base {
            self.open_base(entry.path.clone());
            return;
        }
        self.0.base_generation.set(self.0.base_generation.get() + 1);
        *self.0.base_document.borrow_mut() = None;
        let generation = self.0.base_generation.get();
        self.guardar_estado_pestana();
        let this = self.clone();
        glib::spawn_future_local(async move {
            // Antes de cambiar de nota, se guarda lo pendiente de la anterior (FR-002).
            let editor = this.0.code_editor.clone();
            if editor.current_path().is_some_and(|p| p != entry.path) {
                editor.flush().await;
                if this.0.base_generation.get() != generation {
                    return;
                }
                if editor.has_pending() {
                    return;
                }
                editor.close();
            }
            let is_note = Format::from_name(entry.path.file_name()).is_some();
            let loaded = if is_note {
                let (v, p) = (vault.clone(), entry.path.clone());
                match gio::spawn_blocking(move || v.read_note(&p)).await {
                    Ok(Ok(note)) => Ok(Some(note)),
                    Ok(Err(e)) => Err(e.to_string()),
                    Err(_) => Err("No se pudo leer la nota".to_owned()),
                }
            } else if entry.path.as_str().ends_with(".pdf") {
                // Un PDF va a la vista externa con «Ver el PDF» (016): nunca se muestra como
                // texto, aunque el archivo sea ASCII limpio.
                Err("pdf".to_owned())
            } else {
                // Cualquier otro archivo: si es texto UTF-8 se muestra; si no, se ofrece abrirlo afuera.
                // Los NUL interiores panean `set_text`: se recortan.
                let abs = entry.path.to_path(vault.root());
                gio::spawn_blocking(move || read_text(&abs).map(|t| t.replace('\0', "")))
                    .await
                    .unwrap_or_else(|_| Err("No se pudo leer el archivo".into()))
                    .map(|t| {
                        this.0.editor.buffer().set_text(&t);
                        None
                    })
            };
            if this.0.base_generation.get() != generation {
                return;
            }
            let i = &this.0;
            *i.current_entry.borrow_mut() = Some(entry.clone());
            i.pestanas
                .abrir(entry.path.as_str(), entry.path.file_name());
            this.refresh_backlinks();
            i.content_title.set_title(entry.path.file_name());
            let mut subtitle = entry.path.to_string();
            if entry.read_only {
                subtitle.push_str(" · solo lectura");
            }
            i.content_title.set_subtitle(&subtitle);
            *i.subtitle_base.borrow_mut() = subtitle;
            this.clear_note_view();
            let raw = entry.zone == orbital_vault::Zone::Raw;
            if raw {
                // La importación (015) y la asociación de URL operan sobre el archivo de raw/
                // abierto, sea que se muestre en el editor o en la vista externa.
                *i.externo_ruta.borrow_mut() = Some(entry.path.clone());
            }
            match loaded {
                Ok(Some(note)) => {
                    {
                        let mut r = this.0.recientes.borrow_mut();
                        let rel = note.path.as_str();
                        r.retain(|p| p != rel);
                        r.insert(0, rel.to_owned());
                    }
                    this.refresh_recent();
                    if this.0.persist.get() {
                        let rel = note.path.as_str().to_string();
                        let vroot = vault.root().to_path_buf();
                        glib::spawn_future_local(async move {
                            let _ = gio::spawn_blocking(move || {
                                let mut s = AppState::load();
                                s.note_opened(&vroot, &rel);
                                s.save()
                            })
                            .await;
                        });
                    }
                    this.show_note(note);
                }
                Ok(None) => i.content_stack.set_visible_child_name("nota"),
                Err(_) => {
                    i.external_page.set_title(entry.path.file_name());
                    i.external_page.set_description(Some(if raw {
                        "Fuente original, solo lectura. Orbital no la modifica."
                    } else {
                        "Este archivo no es una nota de texto."
                    }));
                    let url = if raw {
                        let v = vault.clone();
                        let p = entry.path.clone();
                        gio::spawn_blocking(move || v.url_de(&p))
                            .await
                            .ok()
                            .flatten()
                    } else {
                        None
                    };
                    *i.externo_ruta.borrow_mut() = Some(entry.path.clone());
                    i.quitar_url.set_visible(url.is_some());
                    // Importar a Markdown solo se ofrece para PDF, DOCX y HTML.
                    let extension = entry
                        .path
                        .file_name()
                        .rsplit_once('.')
                        .map(|(_, e)| e.to_ascii_lowercase())
                        .unwrap_or_default();
                    i.importar_md
                        .set_visible(orbital_vault::importar::formato_de_ext(&extension).is_some());
                    i.ver_pdf.set_visible(extension == "pdf");
                    match url {
                        Some(url) => {
                            Window::url_web(i).load_uri(url.as_str());
                            i.content_stack.set_visible_child_name("url-webview");
                        }
                        None => i.content_stack.set_visible_child_name("externo"),
                    }
                }
            }
            if raw {
                i.note_banner.set_title("Fuente original, solo lectura");
                i.note_banner.set_visible(true);
            }
            // En las fuentes de raw/ no se puede renombrar, archivar, adjuntar ni borrar.
            let is_note = Format::from_name(entry.path.file_name()).is_some();
            for (name, enabled) in [
                ("rename", !raw),
                ("trash", !raw),
                ("archive", !raw && is_note),
                ("paste-image", !raw && is_note),
            ] {
                if let Some(action) = i
                    .window
                    .lookup_action(name)
                    .and_downcast::<gio::SimpleAction>()
                {
                    action.set_enabled(enabled);
                }
            }
            // Favoritos y Guardados valen para cualquier archivo abierto, también de raw/.
            this.habilitar_acciones_listas(true);
            this.habilitar_acciones_plegado();
        });
    }

    pub(super) fn clear_note_view(&self) {
        let i = &self.0;
        i.code_editor.switcher().set_visible(false);
        i.props_list.remove_all();
        i.props_list.set_visible(false);
        i.html_props.remove_all();
        i.html_props.set_visible(false);
        i.note_banner.set_visible(false);
        i.type_banner.set_visible(false);
        *i.current_note.borrow_mut() = None;
    }

    /// Si otra nota de la bóveda usa el mismo id que `note` (en `id` o en sus delimitadores), lo
    /// avisa en la barra de avisos de la nota. No bloquea ni reescribe nada (043). El índice de ids
    /// se consulta en otro hilo; si al volver ya hay otra nota abierta, no muestra nada.
    fn avisar_ids_repetidos(&self, note: &Note) {
        let ids =
            orbital_vault::note_ids::ids_of(note.prop("id").and_then(|v| v.as_text()), note.body());
        if ids.is_empty() {
            return;
        }
        let Some(vault) = self.0.vault.borrow().clone() else {
            return;
        };
        let path = note.path.clone();
        let this = self.clone();
        glib::spawn_future_local(async move {
            let ruta = path.clone();
            let Ok(repetido) = gio::spawn_blocking(move || {
                ids.into_iter().find_map(|id| {
                    let otras: Vec<_> = vault
                        .notes_with_note_id(&id)
                        .into_iter()
                        .filter(|p| *p != ruta)
                        .collect();
                    (!otras.is_empty()).then_some((id, otras))
                })
            })
            .await
            else {
                return;
            };
            let Some((id, otras)) = repetido else {
                return;
            };
            let i = &this.0;
            let sigue = i
                .current_note
                .borrow()
                .as_ref()
                .is_some_and(|n| n.path == path);
            if !sigue || i.note_banner.is_visible() {
                return;
            }
            let primera = otras[0].file_name();
            let titulo = match otras.len() {
                1 => format!("El id «{id}» también lo usa «{primera}»"),
                n => format!(
                    "El id «{id}» también lo usan «{primera}» y {} notas más",
                    n - 1
                ),
            };
            i.note_banner.set_title(&titulo);
            i.note_banner.set_visible(true);
        });
    }

    /// Dibuja las propiedades, los avisos y el texto de una nota.
    pub(super) fn show_note(&self, note: Note) {
        let i = &self.0;
        let error = note.props.as_ref().and_then(|b| b.error.clone());
        if let Some(err) = &error {
            // Propiedades inválidas: se muestra todo el archivo tal cual y no se edita nada.
            i.note_banner.set_title(&format!(
                "Las propiedades tienen un error en la línea {}: {}",
                err.line, err.message
            ));
            i.note_banner.set_visible(true);
            if note.format == Format::Md && !note.read_only {
                // Las propiedades inválidas se corrigen en la vista Código (FR-018).
                self.open_in_code(&note);
            } else {
                i.editor.buffer().set_text(&note.source);
                i.content_stack.set_visible_child_name("nota");
            }
        } else {
            let list = match note.format {
                Format::Md => &i.props_list,
                Format::Html => &i.html_props,
            };
            if let Some(block) = &note.props {
                self.fill_props(list, &note.path, &block.entries);
            }
            let warnings = note.warnings();
            if warnings.contains(&Warning::MissingType) && !note.read_only {
                i.type_banner.set_visible(true);
            }
            if let Some(w) = warnings.iter().find(|w| **w != Warning::MissingType) {
                i.note_banner.set_title(&w.message());
                i.note_banner.set_visible(true);
            } else {
                self.avisar_ids_repetidos(&note);
            }
            match note.format {
                Format::Md if !note.read_only => self.open_in_code(&note),
                Format::Md => {
                    i.editor.buffer().set_text(note.body());
                    i.content_stack.set_visible_child_name("nota");
                }
                // Las notas `.html` editables van al editor (Visual y Código, US4); las de `raw/`
                // se muestran como página de solo lectura.
                Format::Html if !note.read_only => self.open_in_code(&note),
                Format::Html => {
                    self.load_html(&note);
                    i.content_stack.set_visible_child_name("html");
                }
            }
        }
        *i.current_note.borrow_mut() = Some(note);
        self.refresh_panel();
    }

    /// Abre una nota `.md` en la vista Código. Si ya está abierta (por ejemplo, al recargar la
    /// lista), no se recarga el texto: se conservan el cursor, la selección y el deshacer.
    pub(super) fn open_in_code(&self, note: &Note) {
        let i = &self.0;
        if i.code_editor.current_path().as_ref() != Some(&note.path)
            && let Some(vault) = i.vault.borrow().clone()
        {
            i.code_editor.load(vault, note);
            // Antes de ir a una línea o a un ancla: la navegación despliega lo que la oculta.
            self.plegado_al_abrir(&note.path);
        }
        // Las propiedades se ven en el texto (Código) o arriba de la vista formateada.
        i.props_list.set_visible(false);
        i.code_editor.switcher().set_visible(true);
        i.content_stack.set_visible_child_name("codigo");
        if let Some((path, line)) = i.pending_line.take() {
            if path == note.path {
                i.code_editor.reveal_line(line);
            } else {
                i.pending_line.set(Some((path, line)));
            }
        }
        let ancla = i.pending_ancla.borrow_mut().take();
        if let Some((path, ancla)) = ancla {
            if path == note.path {
                self.ir_al_ancla(ancla);
            } else {
                *i.pending_ancla.borrow_mut() = Some((path, ancla));
            }
        }
        let pendiente = i.estado_pendiente.borrow_mut().take();
        if let Some((path, (cursor, scroll, vista))) = pendiente
            && path == note.path
        {
            i.code_editor
                .restaurar_vista(cursor, scroll, vista.as_deref());
        }
    }

    /// Contadores de la barra inferior: se recalculan 150 ms después del último cambio de texto o
    /// de selección (nunca en cada tecla sobre todo el texto: rompería el presupuesto por tecla).
    pub(super) fn setup_status_counts(&self) {
        let generation = Rc::new(Cell::new(0u64));
        let schedule = {
            let weak = Rc::downgrade(&self.0);
            let generation = generation.clone();
            move || {
                let g = generation.get() + 1;
                generation.set(g);
                let weak = weak.clone();
                let generation = generation.clone();
                glib::timeout_add_local_once(std::time::Duration::from_millis(150), move || {
                    if generation.get() != g {
                        return;
                    }
                    let Some(i) = weak.upgrade() else { return };
                    let b = i.code_editor.buffer();
                    let (start, end) = b.bounds();
                    let total = crate::status_bar::contar(&b.text(&start, &end, true));
                    let seleccion = b
                        .selection_bounds()
                        .map(|(a, z)| crate::status_bar::contar(&b.text(&a, &z, true)));
                    i.status.set_contadores(total, seleccion);
                    let en_editor = i.current_entry.borrow().as_ref().map(|e| &e.path)
                        == i.code_editor.current_path().as_ref();
                    if en_editor {
                        i.panel.set_palabras(total.1);
                    }
                });
            }
        };
        let buffer = self.0.code_editor.buffer();
        let s = schedule.clone();
        buffer.connect_changed(move |_| s());
        let s = schedule.clone();
        buffer.connect_has_selection_notify(move |_| s());
        // Extender una selección mueve la marca sin cambiar `has-selection`.
        buffer.connect_mark_set(move |b, _, mark| {
            if b.has_selection() && (mark == &b.selection_bound() || mark == &b.get_insert()) {
                schedule();
            }
        });
    }

    /// Conecta el editor con la ventana: estado del guardado, errores y cierre.
    pub(super) fn setup_editor(&self) {
        // Retroenlaces: elegir uno abre la nota de origen en la línea del enlace (FR-013).
        let weak = Rc::downgrade(&self.0);
        self.0.right.connect_open(move |from, line| {
            let Some(i) = weak.upgrade() else { return };
            let w = Window(i);
            if w.0.code_editor.current_path().as_ref() == Some(from) {
                w.0.code_editor.reveal_line(line);
            } else {
                w.0.pending_line.set(Some((from.clone(), line)));
                w.show_path(from.as_str());
            }
        });
        let weak = Rc::downgrade(&self.0);
        self.0.right_split.connect_show_sidebar_notify(move |_| {
            if let Some(i) = weak.upgrade() {
                Window(i).refresh_backlinks();
            }
        });
        // Las propiedades arriba de la vista formateada (con los botones de fuentes).
        let weak = Rc::downgrade(&self.0);
        self.0.code_editor.set_on_header(move |path, header| {
            let i = weak.upgrade()?;
            Window(i).props_header(path, header)
        });
        // Enlaces: web afuera (con un clic, principio V); notas de la bóveda, adentro.
        let weak = Rc::downgrade(&self.0);
        self.0.code_editor.set_on_link(move |from, url| {
            let Some(i) = weak.upgrade() else { return };
            Window(i).open_link(Some(from), url);
        });
        // La vista elegida se recuerda (FR-013).
        let persist = self.0.persist.get();
        self.0.code_editor.set_on_view(move |format, v| {
            if !persist {
                return;
            }
            let name = v.name().to_owned();
            glib::spawn_future_local(async move {
                let _ = gio::spawn_blocking(move || {
                    let mut state = AppState::load();
                    match format {
                        Format::Md => state.md_view = Some(name),
                        Format::Html => state.html_view = Some(name),
                    }
                    state.save()
                })
                .await;
            });
        });
        let editor = self.0.code_editor.clone();
        glib::spawn_future_local(async move {
            let saved = gio::spawn_blocking(|| {
                let s = AppState::load();
                (s.md_view, s.html_view)
            })
            .await
            .unwrap_or_default();
            let view = |s: Option<String>, default| {
                s.as_deref()
                    .and_then(crate::editor::View::from_name)
                    .unwrap_or(default)
            };
            editor.set_initial_views(
                view(saved.0, crate::editor::View::Lectura),
                view(saved.1, crate::editor::View::Visual),
            );
        });
        let weak = Rc::downgrade(&self.0);
        self.0.code_editor.set_on_status(move |status| {
            let Some(i) = weak.upgrade() else { return };
            let base = i.subtitle_base.borrow().clone();
            let label = status.label();
            i.content_title.set_subtitle(&if label.is_empty() {
                base
            } else {
                format!("{base} · {label}")
            });
        });
        let weak = Rc::downgrade(&self.0);
        self.0.code_editor.set_on_saved(move |path, hash| {
            if let Some(i) = weak.upgrade() {
                i.own_writes.borrow_mut().insert(path.clone(), hash);
                Window(i.clone()).update_graph(path.clone(), false);
                Window(i).update_search_index(path.clone(), false);
            }
        });
        let weak = Rc::downgrade(&self.0);
        self.0.code_editor.set_on_error(move |msg| {
            if let Some(i) = weak.upgrade() {
                Window(i).toast(msg);
            }
        });
        // Visor PDF (016): copiar enlace al portapapeles, resaltados y volver.
        let visor = self.0.visor.clone();
        visor.set_on_copiar({
            let weak = Rc::downgrade(&self.0);
            move |texto| {
                let Some(i) = weak.upgrade() else {
                    return;
                };
                let bytes = gtk::glib::Bytes::from(texto.as_bytes());
                let contenido =
                    gtk::gdk::ContentProvider::for_bytes("text/plain;charset=utf-8", &bytes);
                match i.window.clipboard().set_content(Some(&contenido)) {
                    Ok(()) => Window(i).toast("Enlace copiado al portapapeles"),
                    _ => Window(i).toast("No se pudo copiar el enlace"),
                }
            }
        });
        visor.set_on_resaltados({
            let weak = Rc::downgrade(&self.0);
            move || {
                let Some(i) = weak.upgrade() else {
                    return;
                };
                let nombre = i
                    .visor_ruta
                    .borrow()
                    .as_ref()
                    .map(|r| r.file_name().to_owned())
                    .unwrap_or_default();
                let consulta = format!("\"raw/{nombre}\"");
                i.search_panel.set_search_query(&consulta);
                Window(i).focus_search();
            }
        });
        visor.set_on_volver({
            let weak = Rc::downgrade(&self.0);
            move || {
                let Some(i) = weak.upgrade() else {
                    return;
                };
                i.content_stack.set_visible_child_name("externo");
            }
        });
        // OnStage (014): material vivo, apertura de enlaces y aviso de cambios del editor.
        let onstage = self.0.onstage.clone();
        onstage.set_on_link({
            let weak = Rc::downgrade(&self.0);
            move |url| {
                let Some(i) = weak.upgrade() else {
                    return;
                };
                let from = i.code_editor.current_path();
                Window(i).open_link(from.as_ref(), url);
            }
        });
        onstage.set_proveedor({
            let weak = Rc::downgrade(&self.0);
            move || {
                let i = weak.upgrade()?;
                let ruta = i.code_editor.current_path()?;
                if ruta.as_str().ends_with(".html") {
                    return None;
                }
                Some(crate::onstage::Fuente {
                    texto: i.code_editor.text(),
                    ruta,
                    vault: i.vault.borrow().clone(),
                })
            }
        });
        {
            let onstage = self.0.onstage.clone();
            self.0
                .code_editor
                .set_on_text_changed(move || onstage.editar());
        }
        // Al cerrar la ventana, primero se guarda lo pendiente.
        let this = self.clone();
        self.0.window.connect_close_request(move |w| {
            if this.0.closing.get() || !this.0.code_editor.has_pending() {
                return glib::Propagation::Proceed;
            }
            let (this, w) = (this.clone(), w.clone());
            glib::spawn_future_local(async move {
                this.0.code_editor.flush().await;
                if this.0.code_editor.has_pending() {
                    return;
                }
                this.0.closing.set(true);
                w.close();
            });
            glib::Propagation::Stop
        });
    }

    /// Muestra una nota HTML como página, creando la vista web la primera vez.
    pub(super) fn load_html(&self, note: &Note) {
        let i = &self.0;
        let Some(vault) = i.vault.borrow().clone() else {
            return;
        };
        let view = i
            .web
            .borrow_mut()
            .get_or_insert_with(|| {
                let weak_link = Rc::downgrade(i);
                let v = html_view::new_view(Some(Rc::new(move |url| {
                    if let Some(inner) = weak_link.upgrade() {
                        Window(inner).open_link(None, url);
                    }
                })));
                v.update_property(&[gtk::accessible::Property::Label("Página de la nota")]);
                i.html_page.append(&v);
                v
            })
            .clone();
        // Las rutas relativas (imágenes, CSS) se resuelven desde la carpeta de la nota.
        let dir = note
            .path
            .to_path(vault.root())
            .parent()
            .map(|d| gio::File::for_path(d).uri().to_string() + "/");
        view.load_html(&html_view::with_csp(&note.source), dir.as_deref());
    }

    /// Abre un archivo con la aplicación predeterminada del sistema.
    pub(super) fn launch_file(&self, path: &Path) {
        let launcher = gtk::FileLauncher::new(Some(&gio::File::for_path(path)));
        let this = self.clone();
        launcher.launch(Some(&self.0.window), None::<&gio::Cancellable>, move |r| {
            if let Err(e) = r
                && !e.matches(gtk::DialogError::Dismissed)
            {
                this.toast(&format!("No se pudo abrir: {e}"));
            }
        });
    }
}
