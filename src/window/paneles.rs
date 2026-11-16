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
                Some(crate::status_bar::contar(&b.text(&b.start_iter(), &b.end_iter(), false)).1)
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

    /// Conecta las pestañas: elegir abre su nota con el estado guardado; cerrar la última deja
    /// el centro vacío (guardando antes lo pendiente).
    pub(super) fn setup_pestanas(&self) {
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
            if (clave == crate::pestanas::CONEXIONES || clave == crate::pestanas::BRUJULA)
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
