//! Pestañas de notas y barra derecha del diseño de interfaz (025).

use super::*;

impl Window {
    /// Guarda el estado de vista de la nota del editor en su pestaña (antes de dejarla).
    pub(super) fn guardar_estado_pestana(&self) {
        let i = &self.0;
        if i.content_stack.visible_child_name().as_deref() != Some("codigo") {
            return;
        }
        if let Some(path) = i.code_editor.current_path() {
            i.pestanas
                .guardar_estado(path.as_str(), i.code_editor.estado_vista());
        }
    }

    /// Barra derecha (025): metadatos y etiquetas de la nota abierta, enlaces entrantes y, con
    /// su modo a la vista, el grafo local (armado en segundo plano).
    pub(super) fn refresh_panel(&self) {
        use crate::panel_derecho::Metadatos;
        let i = &self.0;
        if i.content_stack.visible_child_name().as_deref() == Some("brujula") {
            self.actualizar_brujula(None);
        }
        if !i.right_split.shows_sidebar() {
            return;
        }
        let Some(entry) = i.current_entry.borrow().clone() else {
            i.panel.set_nota(None);
            i.panel.set_entrantes(&[]);
            i.panel
                .set_grafo(None, "Abrí una nota para ver sus conexiones.");
            return;
        };
        let file = entry.path.file_name();
        let formato = Format::from_name(file);
        let meta = {
            let note = i.current_note.borrow();
            let note = note.as_ref().filter(|n| n.path == entry.path);
            let nombre = note
                .and_then(|n| n.prop("title"))
                .and_then(|v| v.as_text())
                .map(str::to_owned)
                .unwrap_or_else(|| file.rsplit_once('.').map_or(file, |(s, _)| s).to_owned());
            let palabras = if i.code_editor.current_path().as_ref() == Some(&entry.path) {
                let b = i.code_editor.buffer();
                Some(crate::status_bar::contar(&b.text(&b.start_iter(), &b.end_iter(), true)).1)
            } else {
                note.map(|n| crate::status_bar::contar(&n.source).1)
            };
            let mut etiquetas: Vec<String> = Vec::new();
            if let Some(n) = note {
                for t in orbital_vault::tags::extract_tags(&n.source, n.format == Format::Html) {
                    if !etiquetas.contains(&t.tag) {
                        etiquetas.push(t.tag);
                    }
                }
            }
            Metadatos {
                nombre,
                formato: match formato {
                    Some(Format::Md) => "Markdown".to_owned(),
                    Some(Format::Html) => "HTML".to_owned(),
                    None => file
                        .rsplit_once('.')
                        .map_or_else(|| "Archivo".to_owned(), |(_, e)| e.to_uppercase()),
                },
                ubicacion: entry
                    .path
                    .as_str()
                    .rsplit_once('/')
                    .map_or_else(|| "/".to_owned(), |(d, _)| d.to_owned()),
                palabras,
                etiquetas,
            }
        };
        i.panel.set_nota(Some(meta));
        let graph = i.graph.borrow().clone();
        let Some(graph) = graph else {
            i.panel.set_entrantes(&[]);
            i.panel.set_grafo(None, "Armando las conexiones…");
            return;
        };
        let mut entrantes: Vec<RelPath> = match graph.lock() {
            Ok(g) => g
                .backlinks_for(&entry.path)
                .iter()
                .map(|b| b.from.clone())
                .collect(),
            Err(_) => Vec::new(),
        };
        entrantes.sort();
        entrantes.dedup();
        i.panel.set_entrantes(&entrantes);
        if i.panel.modo() != "grafo" {
            return;
        }
        if formato.is_none() {
            i.panel
                .set_grafo(None, "Los archivos que no son notas no tienen enlaces.");
            return;
        }
        let this = self.clone();
        let centro = entry.path.clone();
        let saltos = i.panel.saltos();
        glib::spawn_future_local(async move {
            let c = centro.clone();
            let datos = gio::spawn_blocking(move || {
                let g = graph.lock().ok()?;
                let enlaces: Vec<(String, String)> = g
                    .enlaces_resueltos()
                    .into_iter()
                    .map(|(a, b)| (a.to_string(), b.to_string()))
                    .collect();
                Some(crate::grafo::GrafoDatos::local_saltos(
                    c.to_string(),
                    saltos,
                    &enlaces,
                ))
            })
            .await
            .ok()
            .flatten();
            // La nota pudo cambiar mientras tanto.
            let sigue = this.0.current_entry.borrow().as_ref().map(|e| &e.path) == Some(&centro);
            if !sigue {
                return;
            }
            match datos {
                Some(d) if d.rutas.len() > 1 => this.0.panel.set_grafo(Some(Rc::new(d)), ""),
                Some(d) => this.0.panel.set_grafo(
                    Some(Rc::new(d)),
                    "Ninguna nota se conecta con esta todavía.",
                ),
                None => this
                    .0
                    .panel
                    .set_grafo(None, "No se pudieron leer las conexiones."),
            }
        });
    }

    /// Abre la página de la Brújula (017) para la nota abierta.
    pub fn ver_brujula(&self) {
        let i = &self.0;
        self.guardar_estado_pestana();
        i.pestanas.abrir(crate::pestanas::BRUJULA, "Brújula");
        i.content_stack.set_visible_child_name("brujula");
        self.actualizar_brujula(None);
    }

    /// Cierra la Brújula y vuelve a la nota abierta.
    pub fn cerrar_brujula(&self) {
        self.0.pestanas.cerrar(crate::pestanas::BRUJULA);
    }

    /// Calcula en segundo plano el vecindario N/S/O/E de la nota abierta (o de `centro_override`).
    pub(super) fn actualizar_brujula(&self, centro_override: Option<RelPath>) {
        let i = &self.0;
        let Some(vista) = crate::grafo::brujula::VistaBrujula::desde_stack(&i.content_stack) else {
            return;
        };
        let centro = centro_override.or_else(|| {
            i.current_entry
                .borrow()
                .as_ref()
                .filter(|e| Format::from_name(e.path.file_name()).is_some())
                .map(|e| e.path.clone())
        });
        let Some(centro) = centro else {
            vista.set_vacio("Abrí una nota de la bóveda para explorar sus conexiones cardinales.");
            return;
        };
        let Some(graph) = i.graph.borrow().clone() else {
            vista.set_vacio("Armando las conexiones de la bóveda…");
            return;
        };
        let c_str = centro.to_string();
        glib::spawn_future_local(async move {
            let vecindario = gio::spawn_blocking(move || {
                let g = graph.lock().ok()?;
                let enlaces: Vec<(String, String)> = g
                    .enlaces_resueltos()
                    .into_iter()
                    .map(|(a, b)| (a.to_string(), b.to_string()))
                    .collect();
                Some(crate::grafo::brujula::VecindarioBrujula::calcular(
                    &c_str, &enlaces,
                ))
            })
            .await
            .ok()
            .flatten();
            match vecindario {
                Some(v) => vista.set_vecindario(v),
                None => vista.set_vacio("No se pudieron leer las conexiones."),
            }
        });
    }

    /// Diálogo para crear un archivo `.canvas` nuevo en `wiki/` y abrirlo en `VistaCanvas` (038).
    pub fn ask_new_canvas(&self) {
        let Some(vault) = self.0.vault.borrow().clone() else {
            self.toast("Abrí una bóveda antes de crear un lienzo");
            return;
        };
        let entry = gtk::Entry::builder()
            .placeholder_text("Nombre del lienzo (por ejemplo: mapa-conceptual)")
            .activates_default(true)
            .build();
        entry.update_property(&[gtk::accessible::Property::Label("Nombre del lienzo")]);
        let dialog = adw::AlertDialog::builder()
            .heading("Nuevo lienzo (Canvas)")
            .body("Se creará un archivo .canvas en wiki/ para organizar tarjetas y conexiones.")
            .extra_child(&entry)
            .default_response("create")
            .close_response("cancel")
            .build();
        dialog.add_response("cancel", "Cancelar");
        dialog.add_response("create", "Crear");
        dialog.set_response_appearance("create", adw::ResponseAppearance::Suggested);

        let this = self.clone();
        dialog.choose(
            Some(&self.0.window),
            None::<&gio::Cancellable>,
            move |resp| {
                if resp != "create" {
                    return;
                }
                let raw = entry.text();
                let limpio = raw
                    .trim()
                    .trim_matches('/')
                    .strip_suffix(".canvas")
                    .unwrap_or_else(|| raw.trim().trim_matches('/'))
                    .trim();
                if limpio.is_empty() {
                    this.toast("Escribí un nombre para el lienzo");
                    return;
                }
                let Ok(rel) = RelPath::new(&format!("wiki/{limpio}.canvas")) else {
                    this.toast("Nombre de lienzo inválido");
                    return;
                };
                let v = vault.clone();
                let w = this.clone();
                glib::spawn_future_local(async move {
                    let r = rel.clone();
                    let creado = gio::spawn_blocking(move || -> Result<RelPath, String> {
                        let abs = r.to_path(v.root());
                        if abs.exists() {
                            return Ok(r);
                        }
                        if let Some(parent) = abs.parent() {
                            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
                        }
                        let inicial = crate::grafo::canvas::DocumentoCanvas::default().serialize();
                        std::fs::write(&abs, inicial.as_bytes()).map_err(|e| e.to_string())?;
                        Ok(r)
                    })
                    .await;
                    match creado {
                        Ok(Ok(path)) => {
                            w.reload(Some(path.clone())).await;
                            w.show_path(path.as_str());
                        }
                        Ok(Err(e)) => w.toast(&format!("No se pudo crear el lienzo: {e}")),
                        Err(_) => w.toast("No se pudo crear el lienzo"),
                    }
                });
            },
        );
    }

    /// Carga en `VistaCanvas` el archivo `.canvas` que está en `current_entry` (038).
    pub(super) fn abrir_canvas_actual(&self) {
        let i = &self.0;
        let Some(vista) = crate::grafo::canvas::VistaCanvas::desde_stack(&i.content_stack) else {
            return;
        };
        let Some(vault) = i.vault.borrow().clone() else {
            return;
        };
        let Some(entry) = i.current_entry.borrow().clone() else {
            return;
        };
        if entry.kind != EntryKind::Canvas {
            return;
        }
        let path = entry.path.clone();
        glib::spawn_future_local(async move {
            let p = path.clone();
            let leido = gio::spawn_blocking(move || vault.read_aux(&p))
                .await
                .ok()
                .and_then(Result::ok);
            if let Some(aux) = leido {
                vista.cargar(aux);
            }
        });
    }

    /// Abre el Mapa mental de la nota abierta (039).
    pub fn ver_mapa_mental(&self) {
        let i = &self.0;
        let Some(entry) = i.current_entry.borrow().clone() else {
            self.toast("Abrí una nota antes de ver su mapa mental");
            return;
        };
        let Some(formato) = Format::from_name(entry.path.file_name()) else {
            self.toast("El mapa mental solo está disponible para notas Markdown o HTML");
            return;
        };
        let Some(vista) = crate::grafo::mapa_mental::VistaMapaMental::desde_stack(&i.content_stack)
        else {
            return;
        };
        self.guardar_estado_pestana();
        let file = entry.path.file_name();
        let (titulo, fuente) = {
            let note = i.current_note.borrow();
            let note = note.as_ref().filter(|n| n.path == entry.path);
            let titulo = note
                .and_then(|n| n.prop("title"))
                .and_then(|v| v.as_text())
                .map(str::to_owned)
                .unwrap_or_else(|| file.rsplit_once('.').map_or(file, |(s, _)| s).to_owned());
            let fuente = if i.code_editor.current_path().as_ref() == Some(&entry.path) {
                i.code_editor.text()
            } else {
                note.map(|n| n.source.clone()).unwrap_or_default()
            };
            (titulo, fuente)
        };
        vista.cargar_nota(
            entry.path.as_str(),
            &titulo,
            &fuente,
            formato == Format::Html,
        );
        i.pestanas
            .abrir(crate::pestanas::MAPA_MENTAL, "Mapa mental");
        i.content_stack.set_visible_child_name("mapa-mental");
    }

    /// Cierra la pestaña del Mapa mental y vuelve a la nota.
    pub fn cerrar_mapa_mental(&self) {
        self.0.pestanas.cerrar(crate::pestanas::MAPA_MENTAL);
    }

    /// Abre el editor de Ramas libres de la nota abierta (040).
    pub fn ver_ramas_libres(&self) {
        let i = &self.0;
        let Some(entry) = i.current_entry.borrow().clone() else {
            self.toast("Abrí una nota antes de abrir sus ramas libres");
            return;
        };
        if Format::from_name(entry.path.file_name()) != Some(Format::Md) {
            self.toast("Las ramas libres están disponibles para notas Markdown (.md)");
            return;
        };
        let Some(vista) =
            crate::grafo::ramas_libres::VistaRamasLibres::desde_stack(&i.content_stack)
        else {
            return;
        };
        self.guardar_estado_pestana();
        let (fuente, id_sug) = {
            let note = i.current_note.borrow();
            let note = note.as_ref().filter(|n| n.path == entry.path);
            let tipo = note
                .and_then(|n| n.prop("type"))
                .and_then(|v| v.as_text())
                .unwrap_or("atomo");
            let id_existente = note
                .and_then(|n| n.prop("id"))
                .and_then(|v| v.as_text())
                .map(str::to_owned);
            let id_sug = id_existente.unwrap_or_else(|| {
                format!("{}-001", crate::grafo::ramas_libres::prefijo_de_tipo(tipo))
            });
            let fuente = if i.code_editor.current_path().as_ref() == Some(&entry.path) {
                i.code_editor.text()
            } else {
                note.map(|n| n.source.clone()).unwrap_or_default()
            };
            (fuente, id_sug)
        };
        vista.cargar_nota(entry.path.as_str(), &fuente, Some(&id_sug));
        i.pestanas
            .abrir(crate::pestanas::RAMAS_LIBRES, "Ramas libres");
        i.content_stack.set_visible_child_name("ramas-libres");
    }

    /// Cierra la pestaña de Ramas libres y vuelve a la nota.
    pub fn cerrar_ramas_libres(&self) {
        self.0.pestanas.cerrar(crate::pestanas::RAMAS_LIBRES);
    }

    /// Conecta las pestañas: elegir abre su nota con el estado guardado; cerrar la última deja
    /// el centro vacío (guardando antes lo pendiente).
    pub(super) fn setup_pestanas(&self) {
        if let Some(vista_rl) =
            crate::grafo::ramas_libres::VistaRamasLibres::desde_stack(&self.0.content_stack)
        {
            let weak_s = Rc::downgrade(&self.0);
            vista_rl.connect_sincronizar(move |nuevo_md| {
                if let Some(i) = weak_s.upgrade()
                    && i.code_editor.current_path().is_some()
                {
                    i.code_editor.replace_text(&nuevo_md);
                }
            });
            let weak_x = Rc::downgrade(&self.0);
            vista_rl.connect_cerrar(move || {
                if let Some(i) = weak_x.upgrade() {
                    Window(i).cerrar_ramas_libres();
                }
            });
        }
        if let Some(vista_mm) =
            crate::grafo::mapa_mental::VistaMapaMental::desde_stack(&self.0.content_stack)
        {
            let weak_s = Rc::downgrade(&self.0);
            vista_mm.connect_saltar_linea(move |ruta, linea| {
                let Some(i) = weak_s.upgrade() else { return };
                let Ok(rel) = RelPath::new(ruta) else { return };
                let w = Window(i);
                w.0.pending_line.set(Some((rel, linea)));
                w.0.content_stack.set_visible_child_name("codigo");
                w.show_path(ruta);
            });
            let weak_x = Rc::downgrade(&self.0);
            vista_mm.connect_cerrar(move || {
                if let Some(i) = weak_x.upgrade() {
                    Window(i).cerrar_mapa_mental();
                }
            });
        }
        if let Some(vista_c) = crate::grafo::canvas::VistaCanvas::desde_stack(&self.0.content_stack)
        {
            let weak_g = Rc::downgrade(&self.0);
            vista_c.connect_guardar({
                let vista_c = vista_c.clone();
                move |aux, nuevo_yaml, confirm| {
                    let Some(i) = weak_g.upgrade() else { return };
                    let Some(vault) = i.vault.borrow().clone() else {
                        return;
                    };
                    let w = Window(i);
                    let vc = vista_c.clone();
                    glib::spawn_future_local(async move {
                        let v = vault.clone();
                        let ruta = aux.path.clone();
                        let res = gio::spawn_blocking(move || {
                            let ctx = write_ctx::now(Some(v.config()));
                            v.save_aux(&aux, &nuevo_yaml, confirm, &ctx)
                                .and_then(|_| v.read_aux(&ruta))
                        })
                        .await;
                        match res {
                            Ok(Ok(nuevo_aux)) => {
                                if vc.ruta_actual().as_ref() == Some(&nuevo_aux.path) {
                                    vc.actualizar_aux(nuevo_aux);
                                }
                            }
                            Ok(Err(e)) => w.toast(&e.to_string()),
                            Err(_) => w.toast("No se pudo guardar el lienzo"),
                        }
                    });
                }
            });
            let weak_n = Rc::downgrade(&self.0);
            vista_c.connect_abrir_nota(move |ruta| {
                if let Some(i) = weak_n.upgrade() {
                    Window(i).show_path(ruta);
                }
            });
            let weak_p = Rc::downgrade(&self.0);
            vista_c.connect_pedir_nota({
                let vista_c = vista_c.clone();
                move || {
                    let Some(i) = weak_p.upgrade() else { return };
                    let Some(vault) = i.vault.borrow().clone() else {
                        return;
                    };
                    let candidates = i.candidates.borrow().clone();
                    let recent = AppState::load().recent_notes_for(vault.root()).to_vec();
                    let vc = vista_c.clone();
                    crate::switcher::SwitcherDialog::present(
                        &i.window,
                        candidates,
                        recent,
                        move |path| {
                            vc.agregar_tarjeta_nota(path.as_str());
                        },
                    );
                }
            });
        }
        {
            let weak_st = Rc::downgrade(&self.0);
            self.0
                .content_stack
                .connect_visible_child_name_notify(move |st| {
                    if st.visible_child_name().as_deref() == Some("nota")
                        && let Some(i) = weak_st.upgrade()
                    {
                        let es_canvas = i
                            .current_entry
                            .borrow()
                            .as_ref()
                            .is_some_and(|e| e.kind == EntryKind::Canvas);
                        if es_canvas {
                            st.set_visible_child_name("canvas");
                            Window(i).abrir_canvas_actual();
                        }
                    }
                });
        }
        if let Some(vista) = crate::grafo::brujula::VistaBrujula::desde_stack(&self.0.content_stack)
        {
            let weak_a = Rc::downgrade(&self.0);
            vista.connect_abrir(move |ruta| {
                if let Some(i) = weak_a.upgrade() {
                    i.content_stack.set_visible_child_name("codigo");
                    Window(i).show_path(ruta);
                }
            });
            let weak_c = Rc::downgrade(&self.0);
            vista.connect_centrar(move |ruta| {
                if let Some(i) = weak_c.upgrade()
                    && let Ok(rel) = RelPath::new(ruta)
                {
                    Window(i).actualizar_brujula(Some(rel));
                }
            });
            let weak_x = Rc::downgrade(&self.0);
            vista.connect_cerrar(move || {
                if let Some(i) = weak_x.upgrade() {
                    Window(i).cerrar_brujula();
                }
            });
        }
        let weak = Rc::downgrade(&self.0);
        self.0.pestanas.connect_elegir(move |clave, estado| {
            let Some(i) = weak.upgrade() else { return };
            let w = Window(i);
            if clave == crate::pestanas::CONEXIONES {
                w.ver_grafo();
                return;
            }
            if clave == crate::pestanas::BRUJULA {
                w.ver_brujula();
                return;
            }
            if clave == crate::pestanas::MAPA_MENTAL {
                w.ver_mapa_mental();
                return;
            }
            if clave == crate::pestanas::RAMAS_LIBRES {
                w.ver_ramas_libres();
                return;
            }
            w.guardar_estado_pestana();
            if let (Some(estado), Ok(path)) = (estado, RelPath::new(clave)) {
                *w.0.estado_pendiente.borrow_mut() = Some((path, estado));
            }
            w.show_path(clave);
        });
        let weak = Rc::downgrade(&self.0);
        self.0.pestanas.connect_cerrar(move |clave, quedan| {
            let Some(i) = weak.upgrade() else { return };
            if quedan {
                return;
            }
            let w = Window(i);
            if (clave == crate::pestanas::CONEXIONES
                || clave == crate::pestanas::BRUJULA
                || clave == crate::pestanas::MAPA_MENTAL
                || clave == crate::pestanas::RAMAS_LIBRES)
                && w.0.code_editor.current_path().is_some()
            {
                // Quedó solo la nota del editor sin pestaña: se vuelve a ella.
                if let Some(e) = w.0.current_entry.borrow().clone() {
                    w.0.pestanas.abrir(e.path.as_str(), e.path.file_name());
                }
                w.0.content_stack.set_visible_child_name("codigo");
                return;
            }
            glib::spawn_future_local(async move {
                let editor = w.0.code_editor.clone();
                editor.flush().await;
                if editor.has_pending() {
                    w.toast("La nota tiene cambios sin guardar: quedó abierta");
                    if let Some(e) = w.0.current_entry.borrow().clone() {
                        w.0.pestanas.abrir(e.path.as_str(), e.path.file_name());
                    }
                    return;
                }
                editor.close();
                *w.0.current_entry.borrow_mut() = None;
                w.habilitar_acciones_listas(false);
                *w.0.current_note.borrow_mut() = None;
                w.0.content_title.set_title("");
                w.0.content_title.set_subtitle("");
                if let Some(model) = w.0.list_view.model().and_downcast::<gtk::SingleSelection>() {
                    model.set_selected(gtk::INVALID_LIST_POSITION);
                }
                w.0.content_stack.set_visible_child_name("vacio");
                w.refresh_backlinks();
            });
        });
    }
}
