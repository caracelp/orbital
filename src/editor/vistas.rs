//! Vistas de la nota (Código, Lectura, Dividida, Visual), render de la vista formateada y estado de vista.

use super::*;

impl Editor {
    /// Cambia de vista (FR-008). Al pasar a Lectura o Dividida, la vista formateada se actualiza.
    pub fn set_view(&self, v: View) {
        let i = &self.0;
        let format = i.format.get();
        // Ctrl+2 es Lectura en `.md` y Visual en `.html`; Dividida no existe en `.html`.
        let v = match (format, v) {
            (Format::Html, View::Lectura | View::Visual) => View::Visual,
            (Format::Html, View::Dividida) => return,
            (Format::Md, View::Visual) => View::Lectura,
            (_, v) => v,
        };
        if v == View::Visual && i.html_scripts.get() {
            self.error("Esta nota tiene JavaScript: editala en Código");
            self.sync_switcher(i.mode.get());
            return;
        }
        if i.mode.get() == v {
            return;
        }
        match format {
            Format::Md => i.md_pref.set(v),
            Format::Html => i.html_pref.set(v),
        }
        if i.mode.get() == View::Visual {
            // Lo último escrito en Visual pasa al texto antes de mostrar el código.
            let this = self.clone();
            glib::spawn_future_local(async move {
                this.sync_visual().await;
                if !this.0.visual_pending.get() {
                    this.enter(v);
                } else {
                    this.sync_switcher(this.0.mode.get());
                }
            });
        } else {
            self.enter(v);
        }
        if let Some(f) = i.on_view.borrow().as_ref() {
            f(format, v);
        }
    }

    pub(super) fn enter(&self, v: View) {
        self.apply_mode(v);
        match v {
            View::Codigo => {}
            View::Visual => self.show_visual(),
            View::Lectura | View::Dividida => self.render_now(),
        }
    }

    /// Vistas iniciales por formato (las últimas usadas, FR-013), sin avisar el cambio.
    pub fn set_initial_views(&self, md: View, html: View) {
        let i = &self.0;
        i.md_pref.set(if md == View::Visual {
            View::Lectura
        } else {
            md
        });
        i.html_pref.set(match html {
            View::Codigo => View::Codigo,
            _ => View::Visual,
        });
        if i.session.borrow().is_none() {
            self.apply_mode(i.md_pref.get());
        }
    }

    pub(super) fn sync_switcher(&self, v: View) {
        let i = &self.0;
        if i.switcher.active_name().as_deref() != Some(v.name()) {
            i.switching.set(true);
            i.switcher.set_active_name(Some(v.name()));
            i.switching.set(false);
        }
    }

    pub(super) fn apply_mode(&self, v: View) {
        let i = &self.0;
        let old_v = i.mode.get();
        i.mode.set(v);
        if i.mode.get() != v && !matches!(v, View::Codigo | View::Dividida) {
            i.extra.borrow_mut().clear(&i.buffer);
            i.caren.limpiar();
        }
        i.stack
            .set_visible_child_name(if v == View::Visual { "visual" } else { "texto" });
        i.code_capa.set_visible(v != View::Lectura);
        i.reader.widget().set_visible(v != View::Codigo);
        self.sync_switcher(v);
        if v == View::Dividida {
            let max = i.root.max_position();
            if max > 0 {
                i.root.set_position(max / 2);
            }
        }
        i.find_bar
            .set_supports_replace(matches!(v, View::Codigo | View::Dividida));
        if i.find_bar.is_search_mode() && old_v != v {
            let query = i.find_bar.search_text();
            match old_v {
                View::Codigo | View::Dividida => {
                    if !matches!(v, View::Codigo | View::Dividida) {
                        i.search_ctx.set_highlight(false);
                    }
                }
                View::Lectura => {
                    i.reader.clear_find_highlights();
                    i.reader_matches.borrow_mut().clear();
                    i.reader_current_idx.set(None);
                }
                View::Visual => {
                    if let Some(fc) = i.visual.find_controller() {
                        fc.search_finish();
                    }
                    i.visual_matches.set(0);
                    i.visual_current_idx.set(0);
                }
            }
            self.find_search(&query);
        }
    }

    /// Desplaza la vista (Lectura, Dividida o Código) hacia el encabezado indicado.
    pub fn scroll_to_heading(&self, heading: &str) {
        let wanted = heading.trim();
        if wanted.is_empty() {
            return;
        }
        if self.0.reader.scroll_to_heading(wanted) {
            return;
        }
        let buf = &self.0.buffer;
        let text = buf.text(&buf.start_iter(), &buf.end_iter(), false);
        let wanted_lower = wanted.to_lowercase();
        for (idx, line) in text.lines().enumerate() {
            let trimmed = line.trim();
            if trimmed.starts_with('#') {
                let title = trimmed.trim_start_matches('#').trim().to_lowercase();
                if title == wanted_lower {
                    if let Some(mut it) = buf.iter_at_line(idx as i32) {
                        self.0.view.scroll_to_iter(&mut it, 0.0, true, 0.0, 0.0);
                    }
                    break;
                }
            }
        }
    }

    /// Lleva al bloque con ese id (`[[nota#^id]]`, 026) y lo resalta un momento. Devuelve si existe.
    pub fn scroll_to_block(&self, id: &str) -> bool {
        let i = &self.0;
        let b = &i.buffer;
        let text = b.text(&b.start_iter(), &b.end_iter(), false);
        let Some((rango, linea)) =
            orbital_vault::bloques::buscar(&text, id, i.format.get() == Format::Html)
        else {
            return false;
        };
        let inicio_linea = text[..rango.start].rfind('\n').map_or(0, |n| n + 1);
        let fin = rango.end.min(text.len());
        let lineas_fin = text[rango.start..fin].matches('\n').count();
        let fin_linea = text[..fin].rfind('\n').map_or(0, |n| n + 1);
        if let (Some(a), Some(z)) = (
            b.iter_at_line_index(linea as i32, (rango.start - inicio_linea) as i32),
            b.iter_at_line_index((linea + lineas_fin) as i32, (fin - fin_linea) as i32),
        ) {
            resaltar(b, &a, &z);
        }
        self.reveal_line(linea + 1);
        if matches!(i.mode.get(), View::Lectura | View::Dividida) {
            i.reader.resaltar_linea(linea);
        }
        true
    }

    /// Vuelve a dibujar la vista formateada con el texto actual (análisis en otro hilo).
    pub fn render_now(&self) {
        let i = &self.0;
        if !matches!(i.mode.get(), View::Lectura | View::Dividida) || i.session.borrow().is_none() {
            return;
        }
        let generation = i.render_generation.get() + 1;
        i.render_generation.set(generation);
        let text = i
            .buffer
            .text(&i.buffer.start_iter(), &i.buffer.end_iter(), false)
            .to_string();
        let this = self.clone();
        let place = (i.vault.borrow().clone(), self.current_path());
        let index = i.search_index.borrow().clone();
        glib::spawn_future_local(async move {
            let parsed = gio::spawn_blocking(move || {
                let props = orbital_vault::props::markdown::locate(&text)
                    .map(|r| orbital_vault::props::yaml::parse_block(&text[r]));
                // El YAML resuelto alimenta los marcadores `[%clave]` (005); un frontmatter con
                // error de sintaxis no resuelve nada (queda «faltante»).
                let yaml = props
                    .as_ref()
                    .and_then(|parsed| parsed.as_ref().ok())
                    .map(Vec::as_slice)
                    .unwrap_or(&[]);
                // Con la bóveda se expanden las transclusiones `![[…]]` (lee las notas incrustadas).
                let mut prepared = match &place {
                    (Some(v), Some(from)) => reader::Prepared::with_vault(&text, v, from, yaml),
                    _ => reader::Prepared::new(&text, yaml),
                };
                // Los codeblocks generados (`indice`, `toc`) se completan con el índice y la ruta
                // de la nota (005, T015/T019); los rangos y casillas quedan intactos.
                let idx = index.as_deref().and_then(|i| i.read().ok());
                let genero = reader::complete_generated(
                    &mut prepared.doc,
                    &mut prepared.hashes,
                    idx.as_deref(),
                    place.1.as_ref(),
                );
                (prepared, props, genero)
            })
            .await;
            let i = &this.0;
            if i.render_generation.get() != generation {
                return;
            }
            let Ok((prepared, props, genero)) = parsed else {
                return;
            };
            i.doc_has_generated.set(genero);
            *i.embed_deps.borrow_mut() = prepared.deps.clone();
            let header = || {
                let read_only = i.meta.borrow().as_ref().is_none_or(|m| m.read_only);
                let (entries, error) = match props {
                    Some(Ok(e)) => (e, None),
                    Some(Err(e)) => (Vec::new(), Some(e)),
                    None => (Vec::new(), None),
                };
                let h = Header {
                    entries,
                    error,
                    read_only,
                };
                match (i.on_header.borrow().as_ref(), this.current_path()) {
                    (Some(f), Some(path)) => f(&path, &h),
                    _ => None,
                }
            };
            let base = i.base_dir.borrow().clone();
            i.reader.render(prepared, header, base);
            if i.find_bar.is_search_mode() && matches!(i.mode.get(), View::Lectura | View::Dividida)
            {
                let query = i.find_bar.search_text();
                this.find_search(&query);
            }
        });
    }

    /// Marca o desmarca una casilla: cambia un solo carácter del texto (FR-011).
    pub(super) fn toggle_task(&self, offset: usize) {
        let b = &self.0.buffer;
        let text = b.text(&b.start_iter(), &b.end_iter(), false).to_string();
        let Some(prefix) = text.get(..offset) else {
            return;
        };
        let chars = prefix.chars().count() as i32;
        let mut start = b.iter_at_offset(chars);
        let mut end = b.iter_at_offset(chars + 1);
        let new = match start.char() {
            ' ' => "x",
            'x' | 'X' => " ",
            _ => return,
        };
        b.begin_user_action();
        b.delete(&mut start, &mut end);
        let mut at = b.iter_at_offset(chars);
        b.insert(&mut at, new);
        b.end_user_action();
    }

    /// Estado de vista de la nota abierta (025, pestañas): posición del cursor, desplazamiento
    /// de Código y vista elegida.
    pub fn estado_vista(&self) -> (i32, f64, Option<String>) {
        let i = &self.0;
        let cursor = i.buffer.iter_at_mark(&i.buffer.get_insert()).offset();
        let scroll = i.code_scroller.vadjustment().value();
        let vista = i.switcher.active_name().map(|n| n.to_string());
        (cursor, scroll, vista)
    }

    /// Repone un estado de [`Editor::estado_vista`] después de cargar la nota. El desplazamiento
    /// se aplica en un *idle*, cuando la vista ya calculó su alto.
    pub fn restaurar_vista(&self, cursor: i32, scroll: f64, vista: Option<&str>) {
        let i = &self.0;
        if let Some(v) = vista.and_then(View::from_name) {
            self.set_view(v);
        }
        let it = i.buffer.iter_at_offset(cursor);
        i.buffer.place_cursor(&it);
        let adj = i.code_scroller.vadjustment();
        glib::idle_add_local_once(move || adj.set_value(scroll));
    }

    /// Muestra la línea `line` (desde 1) en la vista actual, sin cambiarla: el cursor del código
    /// va ahí y la vista formateada se desplaza cuando termina de dibujarse (retroenlaces, US4).
    pub fn reveal_line(&self, line: usize) {
        let i = &self.0;
        let target = line.saturating_sub(1);
        if let Some(it) = i.buffer.iter_at_line(target as i32) {
            i.buffer.place_cursor(&it);
            i.view
                .scroll_to_mark(&i.buffer.get_insert(), 0.0, true, 0.0, 0.3);
        }
        if matches!(i.mode.get(), View::Lectura | View::Dividida) {
            i.reader.scroll_to_line_when_ready(target);
        }
    }

    /// Pasa a la vista Código y pone el cursor en una línea.
    pub fn show_code_line(&self, line: i32) {
        self.set_view(View::Codigo);
        let b = &self.0.buffer;
        if let Some(it) = b.iter_at_line(line) {
            b.place_cursor(&it);
            self.0
                .view
                .scroll_to_iter(&mut it.clone(), 0.1, false, 0.0, 0.0);
        }
        self.0.view.grab_focus();
    }
}

/// Resalta `a..b` 1,5 s (navegación a un bloque, 026). La etiqueta se crea la primera vez.
pub(super) fn resaltar(buf: &impl IsA<gtk::TextBuffer>, a: &gtk::TextIter, b: &gtk::TextIter) {
    let tabla = buf.tag_table();
    let tag = tabla.lookup("bloque-resaltado").unwrap_or_else(|| {
        let t = gtk::TextTag::builder()
            .name("bloque-resaltado")
            .paragraph_background_rgba(&gtk::gdk::RGBA::new(1.0, 0.55, 0.78, 0.18))
            .build();
        tabla.add(&t);
        t
    });
    buf.apply_tag(&tag, a, b);
    let buf = buf.clone().upcast::<gtk::TextBuffer>();
    glib::timeout_add_local_once(std::time::Duration::from_millis(1500), move || {
        buf.remove_tag(&tag, &buf.start_iter(), &buf.end_iter());
    });
}
