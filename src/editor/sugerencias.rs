//! Autocompletado de enlaces, etiquetas y marcadores mientras se escribe.

use super::*;

impl Editor {
    pub(super) fn check_link_completion(&self) {
        let i = &self.0;
        if i.loading.get()
            || !i.view.has_focus()
            || i.mode.get() == View::Lectura
            || i.mode.get() == View::Visual
        {
            return;
        }

        if let Some((nota, filtro, start)) = code::block_query_at_cursor(&i.buffer) {
            self.sugerir_bloques(nota, &filtro, start);
            return;
        }
        let link_query = code::link_query_at_cursor(&i.buffer);
        let tag_query = link_query
            .is_none()
            .then(|| code::tag_query_at_cursor(&i.buffer, i.format.get() == Format::Html))
            .flatten();
        let is_tag = tag_query.is_some();
        let marker_query = tag_query
            .is_none()
            .then(|| code::marker_query_at_cursor(&i.buffer, i.format.get() == Format::Html))
            .flatten();
        let is_marker = marker_query.is_some();
        let query_info = link_query.or(tag_query).or(marker_query);
        i.completion_is_tag.set(is_tag);
        i.completion_is_marker.set(is_marker);
        match query_info {
            Some((query, start_offset)) => {
                if i.completion_dismissed_at.get() == Some(start_offset) {
                    return;
                }
                i.completion_active_start.set(Some(start_offset));
                let matches = if is_tag {
                    i.completion.filter_tag_query(&query)
                } else if is_marker {
                    // Las claves salen de la nota abierta: inline de toda la nota y frontmatter.
                    // Es un recorrido lineal del texto, chico al escribir.
                    let text = i
                        .buffer
                        .text(&i.buffer.start_iter(), &i.buffer.end_iter(), false);
                    i.completion.set_markers(completion::marker_candidates(
                        &text,
                        i.format.get() == Format::Html,
                    ));
                    i.completion.filter_marker_query(&query)
                } else {
                    i.completion.filter(&query)
                };
                if matches > 0 {
                    let rect = code::cursor_popup_rect(&i.view, &i.buffer);
                    i.completion.popup_at(&rect);
                } else {
                    i.completion.popdown();
                }
            }
            None => {
                *i.completion_bloques.borrow_mut() = None;
                i.completion_dismissed_at.set(None);
                i.completion_active_start.set(None);
                if i.completion.is_visible() {
                    i.completion.popdown();
                }
            }
        }
    }

    /// Sugerencias de `[[nota#^filtro` (026, FR-010): los bloques con id de la nota, leída en
    /// otro hilo una vez por consulta (`completion_bloques`).
    fn sugerir_bloques(&self, nota: String, filtro: &str, start: i32) {
        let i = &self.0;
        if i.completion_dismissed_at.get() == Some(start) {
            return;
        }
        i.completion_active_start.set(Some(start));
        i.completion_is_tag.set(false);
        i.completion_is_marker.set(false);
        let cache = i.completion_bloques.borrow().clone();
        let bloques = match cache {
            Some((n, Some(b))) if n == nota => b,
            Some((n, None)) if n == nota => return, // se está leyendo
            _ => {
                let es_html = |p: &str| p.ends_with(".html") || p.ends_with(".htm");
                let listar_todos = |t: &str, html: bool| {
                    if !html {
                        let delim = crate::grafo::ramas_libres::listar_bloques_delimitados(t);
                        if !delim.is_empty() {
                            return delim;
                        }
                    }
                    orbital_vault::bloques::listar(t, html)
                };
                if nota.is_empty() {
                    // La nota abierta: el texto ya está en el buffer.
                    let texto = i
                        .buffer
                        .text(&i.buffer.start_iter(), &i.buffer.end_iter(), false);
                    let b = Rc::new(listar_todos(&texto, i.format.get() == Format::Html));
                    *i.completion_bloques.borrow_mut() = Some((nota.clone(), Some(b.clone())));
                    b
                } else {
                    let Some(vault) = i.vault.borrow().clone() else {
                        return;
                    };
                    let orbital_vault::Resolution::Found(path) = vault.resolve_wikilink(&nota)
                    else {
                        i.completion.popdown();
                        return;
                    };
                    *i.completion_bloques.borrow_mut() = Some((nota.clone(), None));
                    let this = self.clone();
                    glib::spawn_future_local(async move {
                        let html = es_html(path.as_str());
                        let leidos = gio::spawn_blocking(move || {
                            std::fs::read_to_string(path.to_path(vault.root()))
                                .map(|t| listar_todos(&t, html))
                                .unwrap_or_default()
                        })
                        .await
                        .unwrap_or_default();
                        let mut cache = this.0.completion_bloques.borrow_mut();
                        if cache.as_ref().is_some_and(|(n, _)| *n == nota) {
                            *cache = Some((nota, Some(Rc::new(leidos))));
                            drop(cache);
                            this.check_link_completion();
                        }
                    });
                    return;
                }
            }
        };
        if i.completion.filter_block_query(&nota, &bloques, filtro) > 0 {
            let rect = code::cursor_popup_rect(&i.view, &i.buffer);
            i.completion.popup_at(&rect);
        } else {
            i.completion.popdown();
        }
    }

    pub fn set_tag_candidates(&self, tags: Vec<completion::TagCandidate>) {
        self.0.completion.set_tags(tags);
        self.check_link_completion();
    }

    /// Arma los candidatos de autocompletado de la bóveda (US3): primero los nombres, enseguida;
    /// después títulos y alias, leídos en otro hilo. Solo cuando cambia la bóveda, no al abrir
    /// cada nota (con 5000 notas serían 5000 lecturas por nota abierta).
    pub fn refresh_completion_candidates(&self, vault: &Arc<Vault>) {
        let i = &self.0;
        if std::ptr::eq(i.completion_vault.borrow().as_ptr(), Arc::as_ptr(vault)) {
            return;
        }
        *i.completion_vault.borrow_mut() = Arc::downgrade(vault);
        let key = Arc::as_ptr(vault);
        let notes = |v: &Vault| -> Vec<RelPath> {
            v.entries()
                .iter()
                .filter(|e| {
                    e.zone == Zone::Wiki
                        && matches!(e.kind, EntryKind::AtomNote | EntryKind::SynthesisNote)
                })
                .map(|e| e.path.clone())
                .collect()
        };
        let mut baseline: Vec<_> = notes(vault)
            .into_iter()
            .map(|p| completion::CompletionCandidate::new(p, None, Vec::new()))
            .collect();
        completion::qualify_duplicates(&mut baseline);
        i.completion.set_candidates(baseline.clone());
        i.visual.set_candidates(baseline);

        let vault = vault.clone();
        let this = self.clone();
        glib::spawn_future_local(async move {
            let full = gio::spawn_blocking(move || {
                let mut list: Vec<_> = notes(&vault)
                    .into_iter()
                    .map(|path| {
                        let props = vault.read_props(&path).ok().flatten();
                        let entries = props.as_ref().map_or(&[][..], |p| &p.entries[..]);
                        let title = orbital_vault::props::find(entries, "title")
                            .and_then(|v| v.value.as_text())
                            .map(str::to_owned);
                        let aliases = orbital_vault::props::find(entries, "aliases")
                            .map(|v| match &v.value {
                                orbital_vault::props::PropValue::List(l) => l
                                    .iter()
                                    .filter_map(|x| x.as_text().map(str::to_owned))
                                    .collect(),
                                orbital_vault::props::PropValue::Text(t) => vec![t.clone()],
                                _ => Vec::new(),
                            })
                            .unwrap_or_default();
                        completion::CompletionCandidate::new(path, title, aliases)
                    })
                    .collect();
                completion::qualify_duplicates(&mut list);
                list
            })
            .await
            .unwrap_or_default();
            // Si mientras tanto se abrió otra bóveda, estos candidatos ya no sirven.
            if std::ptr::eq(this.0.completion_vault.borrow().as_ptr(), key) {
                this.0.completion.set_candidates(full.clone());
                this.0.visual.set_candidates(full);
            }
        });
    }
}
