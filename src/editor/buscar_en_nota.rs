//! Buscar y reemplazar en la nota abierta (Ctrl+F, Ctrl+H) en Código, Lectura y Visual.

use super::*;

impl Editor {
    /// Abre la barra de búsqueda (o búsqueda y reemplazo si `replace` es verdadero).
    pub fn open_find(&self, replace: bool) {
        let i = &self.0;
        let mode = i.mode.get();
        i.find_bar
            .set_supports_replace(matches!(mode, View::Codigo | View::Dividida));
        i.find_bar.set_search_mode(true);

        if let Some(sel) = self.selected_text()
            && !sel.is_empty()
            && !sel.contains('\n')
            && sel.chars().count() <= 100
        {
            i.find_bar.set_search_text(&sel);
        }

        if replace && matches!(mode, View::Codigo | View::Dividida) {
            i.find_bar.set_replace_revealed(true);
            if i.find_bar.search_text().is_empty() {
                i.find_bar.entry().grab_focus();
            } else {
                i.find_bar.replace_entry().grab_focus();
                i.find_bar.replace_entry().select_region(0, -1);
            }
        } else {
            i.find_bar.entry().grab_focus();
            i.find_bar.entry().select_region(0, -1);
        }

        self.find_search(&i.find_bar.search_text());
    }

    /// Cierra la barra de búsqueda y quita los resaltados.
    pub fn close_find(&self) {
        let i = &self.0;
        i.find_bar.set_search_mode(false);
        self.clear_search();
        self.grab_focus();
    }

    /// Devuelve el texto seleccionado actualmente en la vista activa, si hay.
    pub fn selected_text(&self) -> Option<String> {
        let i = &self.0;
        match i.mode.get() {
            View::Codigo | View::Dividida => {
                if i.buffer.has_selection() {
                    let (s, e) = (
                        i.buffer.iter_at_mark(&i.buffer.get_insert()),
                        i.buffer.iter_at_mark(&i.buffer.selection_bound()),
                    );
                    Some(i.buffer.text(&s, &e, true).to_string())
                } else {
                    None
                }
            }
            View::Lectura => {
                let b = i.reader.buffer();
                if b.has_selection() {
                    let (s, e) = (
                        b.iter_at_mark(&b.get_insert()),
                        b.iter_at_mark(&b.selection_bound()),
                    );
                    Some(b.text(&s, &e, true).to_string())
                } else {
                    None
                }
            }
            View::Visual => None,
        }
    }

    /// Limpia los resaltados de búsqueda en todas las vistas.
    pub fn clear_search(&self) {
        let i = &self.0;
        i.search_ctx.set_highlight(false);
        i.search_settings.set_search_text(None);
        i.reader.clear_find_highlights();
        i.reader_matches.borrow_mut().clear();
        i.reader_current_idx.set(None);
        if let Some(fc) = i.visual.find_controller() {
            fc.search_finish();
        }
        i.visual_matches.set(0);
        i.visual_current_idx.set(0);
        i.find_bar.set_counter(None, 0);
    }

    /// Ejecuta la búsqueda en la vista activa.
    pub fn find_search(&self, query: &str) {
        let i = &self.0;
        match i.mode.get() {
            View::Codigo | View::Dividida => {
                if query.is_empty() {
                    i.search_settings.set_search_text(None);
                    i.search_ctx.set_highlight(false);
                    i.find_bar.set_counter(None, 0);
                } else {
                    i.search_settings.set_search_text(Some(query));
                    i.search_ctx.set_highlight(true);
                    let insert = i.buffer.get_insert();
                    let cursor = i.buffer.iter_at_mark(&insert);
                    if let Some((m_start, m_end, _)) = i.search_ctx.forward(&cursor) {
                        i.buffer.select_range(&m_start, &m_end);
                        i.view
                            .scroll_to_iter(&mut m_start.clone(), 0.1, false, 0.0, 0.5);
                        let pos = i.search_ctx.occurrence_position(&m_start, &m_end);
                        let total = i.search_ctx.occurrences_count();
                        if total > 0 && pos > 0 {
                            i.find_bar.set_counter(Some(pos as usize), total as usize);
                        } else if total == 0 {
                            i.find_bar.set_counter(None, 0);
                        }
                    } else {
                        let total = i.search_ctx.occurrences_count();
                        if total <= 0 {
                            i.find_bar.set_counter(None, 0);
                        }
                    }
                }
            }
            View::Lectura => {
                i.reader.clear_find_highlights();
                i.reader_matches.borrow_mut().clear();
                i.reader_current_idx.set(None);
                if query.is_empty() {
                    i.find_bar.set_counter(None, 0);
                    return;
                }
                let b = i.reader.buffer();
                // `slice` cuenta los widgets anclados como un carácter y lo invisible (lo plegado,
                // 028) también: las posiciones coinciden con las del buffer.
                let text = b.slice(&b.start_iter(), &b.end_iter(), true);
                let folded_text = orbital_vault::fold(&text);
                let folded_query = orbital_vault::fold(query);
                if folded_query.is_empty() {
                    i.find_bar.set_counter(None, 0);
                    return;
                }

                let query_chars = folded_query.chars().count();
                let mut matches = Vec::new();
                let mut char_offset = 0;
                let mut remainder = &folded_text[..];
                while let Some(idx) = remainder.find(&folded_query) {
                    let chars_before = remainder[..idx].chars().count();
                    let m_start = char_offset + chars_before;
                    let m_end = m_start + query_chars;
                    matches.push((m_start, m_end));
                    let advance_bytes = idx + folded_query.len().max(1);
                    char_offset += chars_before + remainder[idx..advance_bytes].chars().count();
                    remainder = &remainder[advance_bytes..];
                }

                for &(start, end) in &matches {
                    let iter_start = b.iter_at_offset(start as i32);
                    let iter_end = b.iter_at_offset(end as i32);
                    b.apply_tag_by_name("find-highlight", &iter_start, &iter_end);
                }

                if matches.is_empty() {
                    i.find_bar.set_counter(None, 0);
                } else {
                    i.reader_current_idx.set(Some(0));
                    let (start, end) = matches[0];
                    self.revelar_en_lector(start);
                    let iter_start = b.iter_at_offset(start as i32);
                    let iter_end = b.iter_at_offset(end as i32);
                    b.apply_tag_by_name("find-current", &iter_start, &iter_end);
                    let mut scroll_iter = iter_start;
                    i.reader
                        .text_view()
                        .scroll_to_iter(&mut scroll_iter, 0.1, false, 0.0, 0.5);
                    i.find_bar.set_counter(Some(1), matches.len());
                }
                *i.reader_matches.borrow_mut() = matches;
            }
            View::Visual => {
                if let Some(fc) = i.visual.find_controller() {
                    if query.is_empty() {
                        fc.search_finish();
                        i.find_bar.set_counter(None, 0);
                    } else {
                        let options = webkit6::FindOptions::CASE_INSENSITIVE.bits()
                            | webkit6::FindOptions::WRAP_AROUND.bits();
                        fc.search(query, options, 1000);
                        fc.count_matches(query, options, 1000);
                    }
                }
            }
        }
    }

    /// Salta a la coincidencia siguiente en la vista activa.
    pub fn find_next(&self) {
        let i = &self.0;
        match i.mode.get() {
            View::Codigo | View::Dividida => {
                let insert = i.buffer.get_insert();
                let sel = i.buffer.selection_bound();
                let iter1 = i.buffer.iter_at_mark(&insert);
                let iter2 = i.buffer.iter_at_mark(&sel);
                let from = iter1.max(iter2);
                if let Some((m_start, m_end, _)) = i.search_ctx.forward(&from) {
                    i.buffer.select_range(&m_start, &m_end);
                    i.view
                        .scroll_to_iter(&mut m_start.clone(), 0.1, false, 0.0, 0.5);
                    let pos = i.search_ctx.occurrence_position(&m_start, &m_end);
                    let total = i.search_ctx.occurrences_count();
                    if pos > 0 && total > 0 {
                        i.find_bar.set_counter(Some(pos as usize), total as usize);
                    }
                }
            }
            View::Lectura => {
                let matches = i.reader_matches.borrow();
                if !matches.is_empty() {
                    let b = i.reader.buffer();
                    let old_idx = i.reader_current_idx.get().unwrap_or(0);
                    let (old_s, old_e) = matches[old_idx];
                    b.remove_tag_by_name(
                        "find-current",
                        &b.iter_at_offset(old_s as i32),
                        &b.iter_at_offset(old_e as i32),
                    );

                    let new_idx = (old_idx + 1) % matches.len();
                    i.reader_current_idx.set(Some(new_idx));
                    let (s, e) = matches[new_idx];
                    self.revelar_en_lector(s);
                    b.apply_tag_by_name(
                        "find-current",
                        &b.iter_at_offset(s as i32),
                        &b.iter_at_offset(e as i32),
                    );
                    let mut iter_start = b.iter_at_offset(s as i32);
                    i.reader
                        .text_view()
                        .scroll_to_iter(&mut iter_start, 0.1, false, 0.0, 0.5);
                    i.find_bar.set_counter(Some(new_idx + 1), matches.len());
                }
            }
            View::Visual => {
                if let Some(fc) = i.visual.find_controller() {
                    fc.search_next();
                }
            }
        }
    }

    /// Salta a la coincidencia anterior en la vista activa.
    pub fn find_prev(&self) {
        let i = &self.0;
        match i.mode.get() {
            View::Codigo | View::Dividida => {
                let insert = i.buffer.get_insert();
                let sel = i.buffer.selection_bound();
                let iter1 = i.buffer.iter_at_mark(&insert);
                let iter2 = i.buffer.iter_at_mark(&sel);
                let from = iter1.min(iter2);
                if let Some((m_start, m_end, _)) = i.search_ctx.backward(&from) {
                    i.buffer.select_range(&m_start, &m_end);
                    i.view
                        .scroll_to_iter(&mut m_start.clone(), 0.1, false, 0.0, 0.5);
                    let pos = i.search_ctx.occurrence_position(&m_start, &m_end);
                    let total = i.search_ctx.occurrences_count();
                    if pos > 0 && total > 0 {
                        i.find_bar.set_counter(Some(pos as usize), total as usize);
                    }
                }
            }
            View::Lectura => {
                let matches = i.reader_matches.borrow();
                if !matches.is_empty() {
                    let b = i.reader.buffer();
                    let old_idx = i.reader_current_idx.get().unwrap_or(0);
                    let (old_s, old_e) = matches[old_idx];
                    b.remove_tag_by_name(
                        "find-current",
                        &b.iter_at_offset(old_s as i32),
                        &b.iter_at_offset(old_e as i32),
                    );

                    let new_idx = if old_idx == 0 {
                        matches.len() - 1
                    } else {
                        old_idx - 1
                    };
                    i.reader_current_idx.set(Some(new_idx));
                    let (s, e) = matches[new_idx];
                    self.revelar_en_lector(s);
                    b.apply_tag_by_name(
                        "find-current",
                        &b.iter_at_offset(s as i32),
                        &b.iter_at_offset(e as i32),
                    );
                    let mut iter_start = b.iter_at_offset(s as i32);
                    i.reader
                        .text_view()
                        .scroll_to_iter(&mut iter_start, 0.1, false, 0.0, 0.5);
                    i.find_bar.set_counter(Some(new_idx + 1), matches.len());
                }
            }
            View::Visual => {
                if let Some(fc) = i.visual.find_controller() {
                    fc.search_previous();
                }
            }
        }
    }

    /// Reemplaza la coincidencia actual y avanza a la siguiente (Código y Dividida).
    pub fn find_replace(&self, replace_text: &str) {
        let i = &self.0;
        if !matches!(i.mode.get(), View::Codigo | View::Dividida) {
            return;
        }
        let has_sel = i.buffer.has_selection();
        let insert = i.buffer.get_insert();
        let sel = i.buffer.selection_bound();
        let mut iter1 = i.buffer.iter_at_mark(&insert);
        let mut iter2 = i.buffer.iter_at_mark(&sel);
        if iter1.offset() > iter2.offset() {
            std::mem::swap(&mut iter1, &mut iter2);
        }
        let is_match = has_sel && i.search_ctx.occurrence_position(&iter1, &iter2) > 0;

        if is_match {
            let _ = i.search_ctx.replace(&mut iter1, &mut iter2, replace_text);
            if let Some((m_start, m_end, _)) = i.search_ctx.forward(&iter2) {
                i.buffer.select_range(&m_start, &m_end);
                i.view
                    .scroll_to_iter(&mut m_start.clone(), 0.1, false, 0.0, 0.5);
                let pos = i.search_ctx.occurrence_position(&m_start, &m_end);
                let total = i.search_ctx.occurrences_count();
                if pos > 0 && total > 0 {
                    i.find_bar.set_counter(Some(pos as usize), total as usize);
                }
            } else {
                let total = i.search_ctx.occurrences_count();
                if total <= 0 {
                    i.find_bar.set_counter(None, 0);
                }
            }
        } else if let Some((m_start, m_end, _)) = i.search_ctx.forward(&iter2) {
            i.buffer.select_range(&m_start, &m_end);
            i.view
                .scroll_to_iter(&mut m_start.clone(), 0.1, false, 0.0, 0.5);
            let pos = i.search_ctx.occurrence_position(&m_start, &m_end);
            let total = i.search_ctx.occurrences_count();
            if pos > 0 && total > 0 {
                i.find_bar.set_counter(Some(pos as usize), total as usize);
            }
        }
    }

    /// Reemplaza todas las coincidencias en un solo paso deshacible (Código y Dividida).
    pub fn find_replace_all(&self, replace_text: &str) {
        let i = &self.0;
        if !matches!(i.mode.get(), View::Codigo | View::Dividida) {
            return;
        }
        // Lo que se reemplaza tiene que estar a la vista (028, decisión 6): se despliegan antes
        // los elementos con coincidencias, de una vez.
        if self.tiene_pliegues() {
            let mut lineas = Vec::new();
            let mut it = i.buffer.start_iter();
            while let Some((a, b, vuelta)) = i.search_ctx.forward(&it) {
                if vuelta || b.offset() <= it.offset() {
                    break;
                }
                lineas.push(a.line() as u32);
                it = b;
            }
            self.revelar_lineas(&lineas);
        }
        // Sin coincidencias, `replace_all` de sourceview5 0.11 entra en pánico (GtkSourceView
        // devuelve 0 reemplazos sin error y el enlace lo toma como una falla sin error).
        if i.search_ctx.forward(&i.buffer.start_iter()).is_none() {
            i.find_bar.set_counter(None, 0);
            return;
        }
        i.buffer.begin_user_action();
        let _ = i.search_ctx.replace_all(replace_text);
        i.buffer.end_user_action();
        i.find_bar.set_counter(None, 0);
    }

    /// Una coincidencia de Lectura dentro de algo plegado o fuera del enfoque: se revela antes de
    /// mostrarla (FR-011, FR-014, 028).
    fn revelar_en_lector(&self, offset: usize) {
        if self.0.reader.esta_fuera_de_enfoque(offset as i32) {
            self.salir_enfoque();
        }
        let lineas = self.0.reader.plegados_en(offset as i32);
        if !lineas.is_empty() {
            self.desplegar_lineas(&lineas);
        }
    }
}
