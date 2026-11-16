//! Enlaces de la vista formateada: recorrerlos con el teclado y activarlos.

use super::*;

impl Reader {
    pub(super) fn all_link_items(&self) -> Vec<LinkItem> {
        // Lo plegado no se recorre con Tab (028).
        let buf = self.buffer();
        let oculto = buf.tag_table().lookup("plegado");
        self.0
            .blocks
            .borrow()
            .iter()
            .flat_map(|b| b.links.iter().cloned())
            .filter(|l| {
                oculto
                    .as_ref()
                    .is_none_or(|t| !buf.iter_at_mark(&l.start).has_tag(t))
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
            let (bx, by) =
                i.view
                    .window_to_buffer_coords(gtk::TextWindowType::Widget, x as i32, y as i32);
            let Some(iter) = i.view.iter_at_location(bx, by) else {
                return;
            };
            // Una flechita del plegado (028).
            let reader = Reader(i.clone());
            if iter
                .buffer()
                .tag_table()
                .lookup("flecha")
                .is_some_and(|t| iter.has_tag(&t))
                && let Some(linea) = reader.flecha_en(&iter)
            {
                g.set_state(gtk::EventSequenceState::Claimed);
                reader.plegar(linea);
                return;
            }
            for tag in iter.tags() {
                let url = i.links.borrow().get(&tag).cloned();
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
            let items = reader.all_link_items();
            if items.is_empty() {
                return glib::Propagation::Proceed;
            }
            let is_shift = state.contains(gtk::gdk::ModifierType::SHIFT_MASK);
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
        if let Some(f) = self.0.on_link.borrow().as_ref() {
            f(&item.url);
        }
        true
    }

    /// Etiqueta anónima para un enlace (el clic busca la URL por la etiqueta).
    pub(super) fn link_tag(&self, s: &RenderState, url: &str) -> gtk::TextTag {
        let tag = self.new_tag(s, gtk::TextTag::new(None));
        s.links.borrow_mut().push((tag.clone(), url.to_owned()));
        tag
    }
}
