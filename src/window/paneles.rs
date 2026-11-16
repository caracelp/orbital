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
        glib::spawn_future_local(async move {
            let c = centro.clone();
            let datos = gio::spawn_blocking(move || {
                let g = graph.lock().ok()?;
                let enlaces: Vec<(String, String)> = g
                    .enlaces_resueltos()
                    .into_iter()
                    .map(|(a, b)| (a.to_string(), b.to_string()))
                    .collect();
                let vecinos: Vec<String> = enlaces
                    .iter()
                    .filter_map(|(a, b)| {
                        if *a == c.as_str() {
                            Some(b.clone())
                        } else if *b == c.as_str() {
                            Some(a.clone())
                        } else {
                            None
                        }
                    })
                    .collect();
                Some(crate::grafo::GrafoDatos::local(
                    c.to_string(),
                    vecinos,
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

    /// Conecta las pestañas: elegir abre su nota con el estado guardado; cerrar la última deja
    /// el centro vacío (guardando antes lo pendiente).
    pub(super) fn setup_pestanas(&self) {
        let weak = Rc::downgrade(&self.0);
        self.0.pestanas.connect_elegir(move |clave, estado| {
            let Some(i) = weak.upgrade() else { return };
            let w = Window(i);
            if clave == crate::pestanas::CONEXIONES {
                w.ver_grafo();
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
            if clave == crate::pestanas::CONEXIONES && w.0.code_editor.current_path().is_some() {
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
