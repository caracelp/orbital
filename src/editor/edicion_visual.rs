//! Edición visual de notas HTML: sincronización con el texto, marcadores y transclusiones.

use super::*;

impl Editor {
    /// Un cambio en la vista Visual: pasa al texto 300 ms después del último.
    pub(super) fn visual_changed(&self) {
        let i = &self.0;
        i.visual_pending.set(true);
        self.emit(&Status::Pending);
        let generation = i.visual_gen.get() + 1;
        i.visual_gen.set(generation);
        let this = self.clone();
        glib::timeout_add_local_once(VISUAL_SYNC, move || {
            if this.0.visual_gen.get() == generation {
                glib::spawn_future_local(async move { this.sync_visual().await });
            }
        });
    }

    /// Lleva lo de la vista Visual al texto: solo el `<body>` y el `<style data-orbital>` (FR-015).
    pub(super) async fn sync_visual(&self) {
        let i = &self.0;
        if !i.visual_pending.get() {
            return;
        }
        let path = self.current_path();
        let generation = i.visual_gen.get();
        let Some((body, css)) = i.visual.contents().await else {
            return;
        };
        // La nota cambió mientras se leía la página: no se mezcla con la otra.
        if self.current_path() != path
            || !i.visual_pending.get()
            || i.visual_gen.get() != generation
        {
            return;
        }
        let b = &i.buffer;
        let text = b.text(&b.start_iter(), &b.end_iter(), true).to_string();
        let Some(spliced) = orbital_vault::html_edit::splice_body(&text, &body) else {
            self.error("La nota no tiene <body>: editala en Código");
            return;
        };
        let Some(new) = orbital_vault::html_edit::upsert_orbital_style(&spliced, &css) else {
            self.error("La nota no tiene </head>: agregalo en Código para guardar los estilos");
            return;
        };
        i.visual_pending.set(false);
        if new != text {
            replace_changed(b, &text, &new);
        }
        i.visual.mark_loaded(&new);
        // Una fórmula recién escrita (o editada) se dibuja (050); un gráfico, también (052).
        if new.contains('$') {
            self.visual_formulas();
        }
        if new.contains("language-grafico") {
            self.visual_graficos();
        }
        if new.contains("language-mermaid") {
            self.visual_diagramas();
        }
        if new.contains("language-carrusel") || new.contains("language-media-slider") {
            self.visual_carruseles();
        }
    }

    /// Muestra la vista Visual con el texto actual.
    pub(super) fn show_visual(&self) {
        let i = &self.0;
        let b = &i.buffer;
        let text = b.text(&b.start_iter(), &b.end_iter(), true).to_string();
        if orbital_vault::html_edit::body_range(&text).is_none()
            || orbital_vault::html_edit::upsert_orbital_style(&text, "/* comprobar head */")
                .is_none()
        {
            self.error("Para editar en Visual, agregá <head></head> y <body></body> en Código");
            self.set_view(View::Codigo);
            return;
        }
        if orbital_vault::html_edit::body_has_scripts(&text) {
            i.html_scripts.set(true);
            if let Some(toggle) = i.switcher.toggle_by_name("visual") {
                toggle.set_enabled(false);
                toggle.set_tooltip("Esta nota tiene JavaScript: editala en Código");
            }
            self.error("Esta nota tiene JavaScript: editala en Código");
            self.set_view(View::Codigo);
            return;
        }
        let (Some(vault), Some(path)) = (i.vault.borrow().clone(), self.current_path()) else {
            return;
        };
        let file = path.to_path(vault.root());
        let dir = file.parent().unwrap_or(vault.root());
        i.visual.load(&text, dir, vault.root());
        self.visual_header();
        self.visual_embeds();
        self.visual_markers();
        self.visual_formulas();
        self.visual_graficos();
        self.visual_diagramas();
        self.visual_carruseles();
    }

    /// Los carruseles de la nota `.html` (032): sus medios se resuelven contra la bóveda en
    /// segundo plano y la Visual los muestra como una grilla estática (la navegación es de la
    /// Lectura). Sin carruseles, nada.
    pub(super) fn visual_carruseles(&self) {
        let i = &self.0;
        let b = &i.buffer;
        let text = b.text(&b.start_iter(), &b.end_iter(), true).to_string();
        if !text.contains("language-carrusel") && !text.contains("language-media-slider") {
            i.visual.set_carruseles(&[]);
            return;
        }
        let (Some(vault), Some(from)) = (i.vault.borrow().clone(), self.current_path()) else {
            return;
        };
        let this = self.clone();
        glib::spawn_future_local(async move {
            let desde = from.clone();
            let result = gio::spawn_blocking(move || {
                let body =
                    orbital_vault::html_edit::body_range(&text).map_or(&text[..0], |r| &text[r]);
                let mut out = Vec::new();
                for lang in ["carrusel", "media-slider"] {
                    for g in orbital_graficos::html::bloques(body, lang) {
                        let mut c = orbital_vault::carrusel::parse(&g.fuente);
                        vault.resolver_carrusel(&desde, &mut c);
                        // La página está en la carpeta de la nota: los enlaces relativos sirven.
                        let src = |d: &orbital_vault::carrusel::Diapositiva| d.enlace.clone();
                        out.push((g.fuente, orbital_vault::carrusel::html(&c, &src, true)));
                    }
                }
                out
            })
            .await;
            let Ok(carruseles) = result else { return };
            if this.current_path() != Some(from) {
                return;
            }
            this.0.visual.set_carruseles(&carruseles);
        });
    }

    /// Dibuja en segundo plano los diagramas Mermaid de la nota `.html` (058) como SVG con el
    /// texto en `currentColor` (el fondo de la página es de la nota) y se los pasa a la Visual.
    /// Sin diagramas, nada.
    pub(super) fn visual_diagramas(&self) {
        let i = &self.0;
        let b = &i.buffer;
        let text = b.text(&b.start_iter(), &b.end_iter(), true).to_string();
        if !text.contains("language-mermaid") {
            i.visual.set_diagramas(&[]);
            return;
        }
        let path = self.current_path();
        let this = self.clone();
        glib::spawn_future_local(async move {
            let result = gio::spawn_blocking(move || {
                let body =
                    orbital_vault::html_edit::body_range(&text).map_or(&text[..0], |r| &text[r]);
                orbital_graficos::html::bloques(body, "mermaid")
                    .into_iter()
                    .map(|g| {
                        let html =
                            match crate::diagramas::svg(&g.fuente, crate::diagramas::Tema::Html) {
                                Ok(d) => d.marcado.clone(),
                                Err(e) => format!("!{e}"),
                            };
                        (g.fuente, html)
                    })
                    .collect::<Vec<_>>()
            })
            .await;
            let Ok(diagramas) = result else { return };
            if this.current_path() != path {
                return;
            }
            this.0.visual.set_diagramas(&diagramas);
        });
    }

    /// Dibuja en segundo plano los gráficos de la nota `.html` (052) como SVG estático (papel:
    /// el fondo de la página es de la nota) y se los pasa a la vista Visual. Sin gráficos, nada.
    pub(super) fn visual_graficos(&self) {
        let i = &self.0;
        let b = &i.buffer;
        let text = b.text(&b.start_iter(), &b.end_iter(), true).to_string();
        if !text.contains("language-grafico") {
            i.visual.set_graficos(&[]);
            return;
        }
        let path = self.current_path();
        let this = self.clone();
        glib::spawn_future_local(async move {
            let result = gio::spawn_blocking(move || {
                let body =
                    orbital_vault::html_edit::body_range(&text).map_or(&text[..0], |r| &text[r]);
                orbital_graficos::html::graficos(body)
                    .into_iter()
                    .map(|g| {
                        let html = match crate::graficos::svg(&g.fuente, 720) {
                            Ok(svg) => svg,
                            Err(e) => format!("!{e}"),
                        };
                        (g.fuente, html)
                    })
                    .collect::<Vec<_>>()
            })
            .await;
            let Ok(graficos) = result else { return };
            if this.current_path() != path {
                return;
            }
            this.0.visual.set_graficos(&graficos);
        });
    }

    /// Dibuja en segundo plano las fórmulas de la nota `.html` (050) y se las pasa a la vista
    /// Visual: SVG del mismo motor que la Lectura, con nombre accesible. Sin `$`, nada.
    pub(super) fn visual_formulas(&self) {
        let i = &self.0;
        let b = &i.buffer;
        let text = b.text(&b.start_iter(), &b.end_iter(), true).to_string();
        if !text.contains('$') {
            i.visual.set_formulas(&[]);
            return;
        }
        let path = self.current_path();
        let this = self.clone();
        glib::spawn_future_local(async move {
            let result = gio::spawn_blocking(move || {
                let body =
                    orbital_vault::html_edit::body_range(&text).map_or(&text[..0], |r| &text[r]);
                orbital_formulas::html::formulas(body)
                    .into_iter()
                    .map(|(clave, tex, modo)| {
                        let html = match crate::formulas::svg(&tex, modo) {
                            Ok(svg) => svg,
                            Err(e) => format!("!{}", e.mensaje),
                        };
                        (clave, html)
                    })
                    .collect::<Vec<_>>()
            })
            .await;
            let Ok(formulas) = result else { return };
            if this.current_path() != path {
                return;
            }
            this.0.visual.set_formulas(&formulas);
        });
    }

    /// Resuelve los marcadores `[%clave]` de la nota `.html` en segundo plano y se los pasa a
    /// la vista Visual (005, T010): cada marcador se muestra con su valor pero no es editable,
    /// y `body()` los restituye al fuente al serializar.
    pub(super) fn visual_markers(&self) {
        let i = &self.0;
        let b = &i.buffer;
        let text = b.text(&b.start_iter(), &b.end_iter(), true).to_string();
        let this = self.clone();
        glib::spawn_future_local(async move {
            let result = gio::spawn_blocking(move || {
                let head = orbital_vault::props::html::scan(&text);
                let yaml = head
                    .block
                    .and_then(|region| orbital_vault::props::yaml::parse_block(&text[region]).ok())
                    .unwrap_or_default();
                orbital_vault::markers::resolve_markers_html(&text, &yaml)
            })
            .await;
            let Ok(resolved) = result else { return };
            this.0.visual.set_markers(&resolved);
        });
    }

    /// Calcula en segundo plano las tarjetas de las transclusiones `![[…]]` de la nota `.html`
    /// y se las pasa a la vista Visual (US5, T027).
    pub(super) fn visual_embeds(&self) {
        let i = &self.0;
        let (Some(vault), Some(from)) = (i.vault.borrow().clone(), self.current_path()) else {
            return;
        };
        let b = &i.buffer;
        let text = b.text(&b.start_iter(), &b.end_iter(), true).to_string();
        let this = self.clone();
        glib::spawn_future_local(async move {
            let from_c = from.clone();
            let result = gio::spawn_blocking(move || {
                let dir = from_c
                    .as_str()
                    .rsplit_once('/')
                    .map_or("", |(d, _)| d)
                    .to_owned();
                // Enlace de una tarjeta: la nota, relativa a la carpeta de la nota `.html`.
                let href = move |p: &RelPath| relative_href(&dir, p.as_str());
                let body =
                    orbital_vault::html_edit::body_range(&text).map_or(&text[..0], |r| &text[r]);
                let mut cards: Vec<(String, String)> = Vec::new();
                let mut deps = Vec::new();
                for occ in orbital_vault::links::extract_links(body, true) {
                    if occ.kind != orbital_vault::links::LinkKind::Transclusion {
                        continue;
                    }
                    let raw = body[occ.byte_range.clone()].to_owned();
                    if cards.iter().any(|(k, _)| *k == raw) {
                        continue;
                    }
                    let anchor = occ.target.anchor.as_deref();
                    let (embed, d) = vault.embed_one(&from_c, &occ.target.path, anchor);
                    deps.extend(d);
                    let html = orbital_vault::transclusion::card_html(
                        &occ.target.path,
                        anchor,
                        &embed,
                        &href,
                    );
                    cards.push((raw, html));
                }
                deps.sort();
                deps.dedup();
                (cards, deps)
            })
            .await;
            let Ok((cards, deps)) = result else { return };
            if this.current_path() != Some(from) {
                return;
            }
            *this.0.embed_deps.borrow_mut() = deps;
            this.0.visual.set_embeds(&cards);
        });
    }

    /// Las propiedades arriba de la vista Visual.
    pub(super) fn visual_header(&self) {
        let i = &self.0;
        let b = &i.buffer;
        let text = b.text(&b.start_iter(), &b.end_iter(), true).to_string();
        let note = i.meta.borrow().as_ref().and_then(|m| m.reparse(text).ok());
        let Some(note) = note else { return };
        let (entries, error) = match note.props {
            Some(p) => (p.entries, p.error),
            None => (Vec::new(), None),
        };
        let h = Header {
            entries,
            error,
            read_only: note.read_only,
        };
        let w = match (i.on_header.borrow().as_ref(), self.current_path()) {
            (Some(f), Some(path)) => f(&path, &h),
            _ => None,
        };
        i.visual.set_header(w);
    }
}
