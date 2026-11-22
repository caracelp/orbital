//! Enlaces de la vista formateada: recorrerlos con el teclado y activarlos.

use super::*;

impl Reader {
    /// Las direcciones de los enlaces de la vista, en orden (pruebas).
    #[cfg(test)]
    pub(crate) fn link_urls(&self) -> Vec<String> {
        self.all_link_items().into_iter().map(|i| i.url).collect()
    }

    pub(super) fn all_link_items(&self) -> Vec<LinkItem> {
        // Lo plegado y lo fuera del enfoque no se recorren con Tab (028).
        let buf = self.buffer();
        let table = buf.tag_table();
        let oculto = table.lookup("plegado");
        let fuera = table.lookup("fuera-de-enfoque");
        self.0
            .blocks
            .borrow()
            .iter()
            .flat_map(|b| b.links.iter().cloned())
            .filter(|l| {
                let it = buf.iter_at_mark(&l.start);
                if !(oculto.as_ref().is_none_or(|t| !it.has_tag(t))
                    && fuera.as_ref().is_none_or(|t| !it.has_tag(t)))
                {
                    return false;
                }
                // Tampoco lo de un callout cerrado o una respuesta oculta. El botón de una
                // tarjeta empieza en un texto que se oculta al abrirla: ese se recorre siempre.
                l.url.starts_with(super::bloques::URL_CALLOUT)
                    || !it.tags().iter().any(|t| t.is_invisible())
            })
            .collect()
    }

    pub(super) fn set_focused_link(&self, next: Option<usize>) {
        let i = &self.0;
        let buf = self.buffer();
        if let Some(tag) = buf.tag_table().lookup("link-focused") {
            let (start, end) = buf.bounds();
            buf.remove_tag(&tag, &start, &end);
        }
        i.focused_link.set(next);
        if let Some(idx) = next {
            let items = self.all_link_items();
            if let Some(item) = items.get(idx) {
                let start = buf.iter_at_mark(&item.start);
                let end = buf.iter_at_mark(&item.end);
                if let Some(tag) = buf.tag_table().lookup("link-focused") {
                    buf.apply_tag(&tag, &start, &end);
                }
                let mut it = start;
                i.view.scroll_to_iter(&mut it, 0.1, false, 0.0, 0.0);
            }
        }
    }

    pub(super) fn setup_links(&self) {
        let click = gtk::GestureClick::new();
        let weak = Rc::downgrade(&self.0);
        click.connect_released(move |g, _, x, y| {
            let Some(i) = weak.upgrade() else { return };
            Reader(i.clone()).set_focused_link(None);
            // Las flechitas del plegado y los títulos plegables se toman al apretar (`puntero`).
            let (bx, by) = crate::editor::posicion::al_buffer(&i.view, x, y);
            let Some(iter) = crate::editor::posicion::iter_en(&i.view, bx, by) else {
                return;
            };
            let reader = Reader(i.clone());
            for tag in iter.tags() {
                let url = i.links.borrow().get(&tag).cloned();
                if let Some(u) = &url
                    && reader.alternar_callout(u)
                {
                    g.set_state(gtk::EventSequenceState::Claimed);
                    return;
                }
                if let (Some(url), Some(f)) = (url, i.on_link.borrow().as_ref()) {
                    g.set_state(gtk::EventSequenceState::Claimed);
                    f(&url);
                    return;
                }
            }
        });
        self.0.view.add_controller(click);

        let key = gtk::EventControllerKey::new();
        let weak_key = Rc::downgrade(&self.0);
        key.connect_key_pressed(move |_, keyval, _, state| {
            let Some(i) = weak_key.upgrade() else {
                return glib::Propagation::Proceed;
            };
            let reader = Reader(i.clone());
            let ctrl = state.contains(gtk::gdk::ModifierType::CONTROL_MASK);
            let alt = state.contains(gtk::gdk::ModifierType::ALT_MASK);
            let is_shift = state.contains(gtk::gdk::ModifierType::SHIFT_MASK);
            // Re Pág y Av Pág: GTK busca el cursor nuevo por píxel y aborta si cae debajo de un
            // renglón con algo invisible (`editor::posicion`). En Lectura el cursor no se ve: se
            // desplaza la página y el cursor va al primer renglón visible.
            if !ctrl && !alt {
                let paginas = match keyval {
                    gtk::gdk::Key::Page_Down | gtk::gdk::Key::KP_Page_Down => 1.0,
                    gtk::gdk::Key::Page_Up | gtk::gdk::Key::KP_Page_Up => -1.0,
                    _ => 0.0,
                };
                if paginas != 0.0 {
                    reader.pasar_pagina(paginas);
                    return glib::Propagation::Stop;
                }
            }
            if alt && !ctrl && !is_shift {
                if keyval == gtk::gdk::Key::Left && i.enfoque.borrow().is_some() {
                    if let Some(f) = i.on_subir_enfoque.borrow().as_ref() {
                        f();
                    }
                    return glib::Propagation::Stop;
                }
                if keyval == gtk::gdk::Key::Right
                    && let Some(idx) = i.focused_link.get()
                    && let Some(item) = reader.all_link_items().get(idx)
                    && let Some(l) = item
                        .url
                        .strip_prefix(super::plegado::URL_PLEGAR)
                        .and_then(|s| s.parse().ok())
                {
                    if let Some(f) = i.on_enfocar.borrow().as_ref() {
                        f(Some(l));
                    }
                    return glib::Propagation::Stop;
                }
            }
            let items = reader.all_link_items();
            if items.is_empty() {
                return glib::Propagation::Proceed;
            }
            if keyval == gtk::gdk::Key::Tab || keyval == gtk::gdk::Key::ISO_Left_Tab {
                let cur = i.focused_link.get();
                let next = if is_shift {
                    match cur {
                        None => Some(items.len().saturating_sub(1)),
                        Some(0) => None,
                        Some(idx) => Some(idx.saturating_sub(1)),
                    }
                } else {
                    match cur {
                        None => Some(0),
                        Some(idx) if idx + 1 < items.len() => Some(idx + 1),
                        Some(_) => None,
                    }
                };
                reader.set_focused_link(next);
                if next.is_some() {
                    glib::Propagation::Stop
                } else {
                    glib::Propagation::Proceed
                }
            } else if (keyval == gtk::gdk::Key::Return
                || keyval == gtk::gdk::Key::KP_Enter
                || keyval == gtk::gdk::Key::space)
                && reader.activate_focused_link()
            {
                glib::Propagation::Stop
            } else if keyval == gtk::gdk::Key::Escape && i.focused_link.get().is_some() {
                reader.set_focused_link(None);
                glib::Propagation::Stop
            } else {
                glib::Propagation::Proceed
            }
        });
        self.0.view.add_controller(key);
    }

    /// Desplaza `paginas` páginas (negativo: hacia arriba) y deja el cursor en el primer renglón
    /// visible.
    pub(super) fn pasar_pagina(&self, paginas: f64) {
        let i = &self.0;
        let adj = i.scroller.vadjustment();
        let destino = (adj.value() + paginas * adj.page_increment()).clamp(
            adj.lower(),
            (adj.upper() - adj.page_size()).max(adj.lower()),
        );
        adj.set_value(destino);
        let (it, _) = i.view.line_at_y(destino as i32);
        self.buffer().place_cursor(&it);
    }

    pub(super) fn activate_focused_link(&self) -> bool {
        let Some(index) = self.0.focused_link.get() else {
            return false;
        };
        let Some(item) = self.all_link_items().get(index).cloned() else {
            return false;
        };
        if let Some(linea) = item
            .url
            .strip_prefix(super::plegado::URL_PLEGAR)
            .and_then(|l| l.parse().ok())
        {
            self.plegar(linea);
            return true;
        }
        if self.alternar_callout(&item.url) {
            return true;
        }
        if let Some(f) = self.0.on_link.borrow().as_ref() {
            f(&item.url);
        }
        true
    }

    /// Abre o cierra un callout plegable si `url` es el de su título, o muestra u oculta la
    /// respuesta de una tarjeta de estudio (y cambia el texto de su botón).
    pub(super) fn alternar_callout(&self, url: &str) -> bool {
        let Some(nombre) = url.strip_prefix(super::bloques::URL_CALLOUT) else {
            return false;
        };
        let table = self.buffer().tag_table();
        if let Some(tag) = table.lookup(nombre) {
            let oculto = !tag.is_invisible();
            tag.set_invisible(oculto);
            if let Some(clave) = self.0.clave_de.borrow().get(nombre) {
                let mut alternados = self.0.alternados.borrow_mut();
                if !alternados.remove(clave) {
                    alternados.insert(*clave);
                }
            }
            if let Some(t) = table.lookup(&format!("{nombre}-mostrar")) {
                t.set_invisible(!oculto);
            }
            if let Some(t) = table.lookup(&format!("{nombre}-ocultar")) {
                t.set_invisible(oculto);
            }
            self.0.view.queue_draw();
        }
        true
    }

    /// Muestra la respuesta de la tarjeta de estudio con ese id (si está oculta). Para las
    /// capturas de desarrollo (`ORBITAL_CAPTURA_TARJETA`).
    pub fn mostrar_respuesta(&self, id: &str) -> bool {
        let buf = self.buffer();
        let Some((_, desde)) = buf.start_iter().forward_search(
            &format!("·  {id}\n"),
            gtk::TextSearchFlags::empty(),
            None,
        ) else {
            return false;
        };
        let Some(boton) = self.all_link_items().into_iter().find(|l| {
            l.url.starts_with(super::bloques::URL_CALLOUT)
                && buf.iter_at_mark(&l.start).offset() >= desde.offset()
        }) else {
            return false;
        };
        let nombre = &boton.url[super::bloques::URL_CALLOUT.len()..];
        if buf
            .tag_table()
            .lookup(nombre)
            .is_some_and(|t| t.is_invisible())
        {
            self.alternar_callout(&boton.url);
        }
        true
    }

    /// Si el callout plegable o la tarjeta `nombre` (que empieza en `origen` de la fuente) se
    /// alternó a mano antes de volver a volcarse; anota su clave.
    pub(super) fn alternado(&self, s: &RenderState, nombre: &str, origen: usize) -> bool {
        let clave = (s.block_start.get(), origen);
        self.0
            .clave_de
            .borrow_mut()
            .insert(nombre.to_owned(), clave);
        self.0.alternados.borrow().contains(&clave)
    }

    /// Etiqueta anónima para un enlace (el clic busca la URL por la etiqueta).
    pub(super) fn link_tag(&self, s: &RenderState, url: &str) -> gtk::TextTag {
        let tag = self.new_tag(s, gtk::TextTag::new(None));
        s.links.borrow_mut().push((tag.clone(), url.to_owned()));
        tag
    }
}
