//! Enlaces: navegar un enlace (y crear la nota que falta), grafo global y retroenlaces.

use super::*;

impl Window {
    /// Arma el grafo de enlaces de la bóveda en segundo plano (US4; 136 ms con 5000 notas).
    pub(super) fn rebuild_graph(&self) {
        let i = &self.0;
        let Some(vault) = i.vault.borrow().clone() else {
            return;
        };
        let generation = i.graph_gen.get() + 1;
        i.graph_gen.set(generation);
        *i.graph.borrow_mut() = None;
        self.refresh_backlinks();
        let this = self.clone();
        glib::spawn_future_local(async move {
            let built = gio::spawn_blocking(move || orbital_vault::LinkGraph::build(&vault))
                .await
                .ok()
                .and_then(Result::ok);
            if this.0.graph_gen.get() != generation {
                return;
            }
            let nuevo = built.map(|g| Arc::new(std::sync::Mutex::new(g)));
            *this.0.graph.borrow_mut() = nuevo.clone();
            this.refresh_backlinks();
            this.reconstruir_grafo_visual(nuevo.as_ref());
        });
    }

    /// Pone al día en el grafo los enlaces de `path` (al guardarse o cambiar afuera), en otro hilo.
    pub(super) fn update_graph(&self, path: RelPath, removed: bool) {
        let i = &self.0;
        // Las transclusiones de la nota abierta que incrustan a `path` se ponen al día (US5).
        i.code_editor.dependency_changed(&path);
        let (Some(graph), Some(vault)) = (i.graph.borrow().clone(), i.vault.borrow().clone())
        else {
            return;
        };
        let Some(format) = Format::from_name(path.file_name()) else {
            return;
        };
        if !path.as_str().starts_with("wiki/") {
            return;
        }
        let this = self.clone();
        glib::spawn_future_local(async move {
            let _ = gio::spawn_blocking(move || {
                let text = if removed {
                    None
                } else {
                    std::fs::read_to_string(path.to_path(vault.root())).ok()
                };
                let Ok(mut g) = graph.lock() else { return };
                match text {
                    Some(t) => g.update_file(&vault, &path, &t, format == Format::Html),
                    None => g.remove_file(&path),
                }
            })
            .await;
            this.refresh_backlinks();
        });
    }

    /// Abre la página del grafo (008) y construye sus datos si no los tiene.
    pub fn ver_grafo(&self) {
        let i = &self.0;
        self.guardar_estado_pestana();
        i.pestanas.abrir(crate::pestanas::CONEXIONES, "Conexiones");
        if i.grafo_datos.borrow().is_none() {
            let graph = i.graph.borrow().clone();
            self.reconstruir_grafo_visual(graph.as_ref());
        }
        i.content_stack.set_visible_child_name("grafo");
    }

    /// Cierra el grafo y vuelve a la nota (Esc: SC-3, nada cambia).
    pub fn cerrar_grafo(&self) {
        // Cerrar el grafo es cerrar su pestaña: la barra elige la vecina (y la abre).
        self.0.pestanas.cerrar(crate::pestanas::CONEXIONES);
    }

    /// Arma los `GrafoDatos` del grafo actual en segundo plano (SC-1).
    pub(super) fn reconstruir_grafo_visual(
        &self,
        graph: Option<&Arc<std::sync::Mutex<orbital_vault::LinkGraph>>>,
    ) {
        let Some(graph) = graph else {
            return;
        };
        let graph = graph.clone();
        let this = self.clone();
        glib::spawn_future_local(async move {
            let built = gio::spawn_blocking(move || {
                let guard = graph.lock().ok();
                guard.map(|g| (g.notas(), g.enlaces_resueltos()))
            })
            .await
            .ok();
            let Some(Some((notas, enlaces))) = built else {
                return;
            };
            let datos = std::rc::Rc::new(crate::grafo::GrafoDatos::desde(
                notas.iter().map(|p| p.as_str().to_owned()).collect(),
                enlaces
                    .iter()
                    .map(|(a, b)| (a.as_str().to_owned(), b.as_str().to_owned()))
                    .collect(),
            ));
            *this.0.grafo_datos.borrow_mut() = Some(datos.clone());
            this.0.grafo_vista.set_datos(datos);
        });
    }

    /// Muestra en el panel los retroenlaces de la nota abierta (solo si el panel está a la vista).
    pub(super) fn refresh_backlinks(&self) {
        use crate::editor::right_sidebar::Backlinks;
        let i = &self.0;
        if !i.right_split.shows_sidebar() {
            return;
        }
        self.refresh_panel();
        let path = i
            .current_entry
            .borrow()
            .as_ref()
            .filter(|e| Format::from_name(e.path.file_name()).is_some())
            .map(|e| e.path.clone());
        let Some(path) = path else {
            i.right.set_backlinks(Backlinks::NoNote);
            return;
        };
        let graph = i.graph.borrow().clone();
        let Some(graph) = graph else {
            i.right.set_backlinks(Backlinks::Loading);
            return;
        };
        // Se copian y se suelta el candado antes de armar los widgets.
        let refs = match graph.lock() {
            Ok(g) => g.backlinks_for(&path).to_vec(),
            Err(_) => Vec::new(),
        };
        i.right.set_backlinks(Backlinks::Ready(&refs));
    }

    /// Muestra una nota por su ruta relativa (para el modo de captura).
    /// Abre un enlace externo o interno, resolviendo anclas de encabezado.
    /// Lleva al encabezado o, si el ancla empieza con `^`, al bloque (026) de la nota abierta.
    pub(super) fn ir_al_ancla(&self, anchor: String) {
        let w = self.clone();
        glib::idle_add_local_once(move || match anchor.strip_prefix('^') {
            Some(id) => {
                if !w.0.code_editor.scroll_to_block(id) {
                    w.toast(&format!("No hay un bloque «^{id}» en esta nota"));
                }
            }
            None => w.0.code_editor.scroll_to_heading(&anchor),
        });
    }

    pub fn open_link(&self, from: Option<&RelPath>, url: &str) {
        if let Some(tag) = url.strip_prefix("orbital-tag:") {
            self.search_tag(tag);
            return;
        }
        if url.starts_with("http://") || url.starts_with("https://") || url.starts_with("mailto:") {
            gtk::UriLauncher::new(url).launch(
                Some(&self.0.window),
                None::<&gio::Cancellable>,
                |_| {},
            );
            return;
        }

        let Some(vault) = self.0.vault.borrow().clone() else {
            return;
        };

        // Si viene de WebKit como file://, convertirlo a relativo a la bóveda
        let clean_url = if let Some(file_path) = url.strip_prefix("file://") {
            let decoded = orbital_vault::percent_decode(file_path);
            let p = std::path::Path::new(&decoded);
            if let Ok(rel) = p.strip_prefix(vault.root()) {
                rel.to_string_lossy().to_string()
            } else {
                decoded
            }
        } else {
            url.to_string()
        };

        let parsed = orbital_vault::parse_link_target(&clean_url);
        let target_str = if clean_url.starts_with("[[") {
            format!("[[{}]]", parsed.path)
        } else {
            parsed.path.clone()
        };

        let default_rel = RelPath::new("wiki/index.md").ok();
        let from_ref = from.or(default_rel.as_ref());
        let res = from_ref.map_or(orbital_vault::Resolution::Missing, |f| {
            vault.resolve_link(f, &target_str)
        });

        match res {
            orbital_vault::Resolution::Found(p) => {
                // Un PDF va al visor embebido, con su ancla de página/selección si la hay (016).
                if p.as_str().ends_with(".pdf") {
                    let ancla_txt = clean_url.split_once('#').map(|(_, a)| a).unwrap_or("");
                    let ancla = crate::visor_pdf::parse_ancla(ancla_txt);
                    self.ver_pdf(&p, &ancla);
                    return;
                }
                // El ancla se sigue cuando la nota termina de abrirse (se lee en otro hilo).
                let abierta = self.0.code_editor.current_path().as_ref() == Some(&p);
                match parsed.anchor {
                    Some(anchor) if abierta => self.ir_al_ancla(anchor),
                    Some(anchor) => *self.0.pending_ancla.borrow_mut() = Some((p.clone(), anchor)),
                    None => {}
                }
                self.show_path(p.as_str());
            }
            orbital_vault::Resolution::Ambiguous(hits) => {
                if let Some(first) = hits.first() {
                    self.show_path(first.as_str());
                    self.toast(&format!("Enlace ambiguo: abriendo «{}»", first));
                }
            }
            orbital_vault::Resolution::Missing => {
                let note_name = parsed.path.trim().to_string();
                if note_name.is_empty() {
                    // `[[#encabezado]]` o `[[#^id]]`: en la nota abierta.
                    if let Some(anchor) = parsed.anchor {
                        self.ir_al_ancla(anchor);
                    }
                    return;
                }
                let clean_name = note_name
                    .strip_suffix(".md")
                    .or_else(|| note_name.strip_suffix(".html"))
                    .unwrap_or(&note_name)
                    .to_string();

                let (dir_rel, file_name) = match clean_name.rsplit_once('/') {
                    Some((d, f)) => (format!("wiki/{d}"), f.to_string()),
                    None => ("wiki".to_string(), clean_name),
                };

                let Ok(dir) = RelPath::new(&dir_rel) else {
                    self.toast(&format!("Ruta inválida «{}»", dir_rel));
                    return;
                };

                let ask = vault.config().ask_create_missing_notes;
                if !ask {
                    self.create_missing_note(dir, file_name);
                    return;
                }

                let check = gtk::CheckButton::builder()
                    .label("No volver a preguntar")
                    .margin_top(12)
                    .build();

                let heading = format!("¿Crear la nota «{file_name}» en wiki/?");
                let dialog = adw::AlertDialog::builder()
                    .heading(&heading)
                    .body("El enlace apunta a una nota que todavía no existe en la bóveda.")
                    .extra_child(&check)
                    .default_response("create")
                    .close_response("cancel")
                    .build();
                dialog.add_response("cancel", "Cancelar");
                dialog.add_response("create", "Crear");
                dialog.set_response_appearance("create", adw::ResponseAppearance::Suggested);

                let this = self.clone();
                let v = vault.clone();
                dialog.choose(
                    Some(&self.0.window),
                    None::<&gio::Cancellable>,
                    move |response| {
                        if response == "create" {
                            if check.is_active() {
                                let mut cfg = v.config().clone();
                                cfg.ask_create_missing_notes = false;
                                let root = v.root().to_path_buf();
                                gio::spawn_blocking(move || {
                                    let _ = cfg.save(&root);
                                });
                            }
                            this.create_missing_note(dir, file_name);
                        }
                    },
                );
            }
        }
    }

    pub(super) fn create_missing_note(&self, dir: RelPath, name: String) {
        let this = self.clone();
        let Some(vault) = self.0.vault.borrow().clone() else {
            return;
        };
        glib::spawn_future_local(async move {
            let n = name.clone();
            let v = vault.clone();
            let d = dir.clone();
            let result = gio::spawn_blocking(move || {
                let ctx = write_ctx::now(Some(v.config()));
                v.create_note(&d, &n, orbital_vault::Format::Md, &ctx)
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
    pub(super) fn test_open_missing(&self) {
        let from = self
            .0
            .current_entry
            .borrow()
            .as_ref()
            .map(|e| e.path.clone());
        self.open_link(from.as_ref(), "[[nota-nueva-pendiente]]");
    }
}
