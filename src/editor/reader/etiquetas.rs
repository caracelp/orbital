//! Etiquetas de texto de la vista formateada (estilos de encabezados, enlaces, código, citas…).

use super::*;

impl Reader {
    pub(super) fn create_tags(&self) {
        let b = self.buffer();
        let t = b.tag_table();
        let add = |tag: gtk::TextTag| {
            t.add(&tag);
        };
        let scales = [1.9, 1.55, 1.3, 1.15, 1.05, 1.0];
        for (n, s) in scales.iter().enumerate() {
            add(gtk::TextTag::builder()
                .name(format!("h{}", n + 1))
                .scale(*s)
                .weight(700)
                .pixels_above_lines(if n < 2 { 18 } else { 12 })
                .pixels_below_lines(6)
                .build());
        }
        add(gtk::TextTag::builder().name("strong").weight(700).build());
        add(gtk::TextTag::builder()
            .name("emph")
            .style(pango::Style::Italic)
            .build());
        add(gtk::TextTag::builder()
            .name("strike")
            .strikethrough(true)
            .build());
        add(gtk::TextTag::builder()
            .name("code")
            .family("monospace")
            .background("rgba(255,255,255,0.08)")
            .build());
        add(gtk::TextTag::builder()
            .name("link")
            .foreground(crate::style::LINK_FG.hex())
            .underline(pango::Underline::Single)
            .build());
        add(gtk::TextTag::builder()
            .name("link-missing")
            .foreground(crate::style::LINK_MISSING_FG.hex())
            .underline(pango::Underline::Single)
            .build());
        add(gtk::TextTag::builder()
            .name("link-focused")
            .background(crate::style::ACCENT_BG.hex())
            .foreground(crate::style::ACCENT_FG.hex())
            .build());
        add(gtk::TextTag::builder()
            .name("find-highlight")
            .background(crate::style::SEARCH_MATCH_BG.hex())
            .foreground(crate::style::SEARCH_MATCH_FG.hex())
            .build());
        add(gtk::TextTag::builder()
            .name("find-current")
            .background(crate::style::SEARCH_CURRENT_BG.hex())
            .foreground(crate::style::SEARCH_CURRENT_FG.hex())
            .build());
        add(gtk::TextTag::builder()
            .name("dim")
            .foreground("rgba(255,255,255,0.70)")
            .build());
        add(gtk::TextTag::builder()
            .name("codeblock")
            .family("monospace")
            .paragraph_background("#1e1e22")
            .wrap_mode(gtk::WrapMode::Char)
            .build());
        add(gtk::TextTag::builder()
            .name("table")
            .paragraph_background("#1e1e22")
            .wrap_mode(gtk::WrapMode::None)
            .build());
        add(gtk::TextTag::builder()
            .name("table-head")
            .weight(700)
            .build());
        // Separador: una línea casi vacía, muy baja, con fondo.
        add(gtk::TextTag::builder()
            .name("rule")
            .paragraph_background("rgba(255,255,255,0.22)")
            .scale(0.12)
            .pixels_below_lines(0)
            .build());
        add(gtk::TextTag::builder()
            .name("rule-space")
            .pixels_above_lines(10)
            .pixels_below_lines(10)
            .build());
        for (name, bg) in [
            ("callout-nota", crate::style::CALLOUT_NOTA_BG.hex()),
            ("callout-consejo", crate::style::CALLOUT_CONSEJO_BG.hex()),
            (
                "callout-importante",
                crate::style::CALLOUT_IMPORTANTE_BG.hex(),
            ),
            (
                "callout-advertencia",
                crate::style::CALLOUT_ADVERTENCIA_BG.hex(),
            ),
            ("callout-peligro", crate::style::CALLOUT_PELIGRO_BG.hex()),
            ("callout-otro", crate::style::CARD_BG.hex()),
        ] {
            add(gtk::TextTag::builder()
                .name(name)
                .paragraph_background(bg)
                .build());
        }
        add(gtk::TextTag::builder()
            .name("callout-title")
            .weight(700)
            .build());
        add(gtk::TextTag::builder()
            .name("embed-gap")
            .scale(0.4)
            .pixels_below_lines(0)
            .build());
        add(gtk::TextTag::builder()
            .name("embed")
            .paragraph_background(crate::style::EMBED_BG.hex())
            .build());
    }

    /// Etiqueta de sangría: se recalcula con el margen de la columna.
    pub(super) fn indent_tag(&self, extra: i32) -> gtk::TextTag {
        let name = format!("indent-{extra}");
        let table = self.buffer().tag_table();
        if let Some(t) = table.lookup(&name) {
            return t;
        }
        let tag = gtk::TextTag::builder()
            .name(&name)
            .left_margin(self.0.side.get() + extra)
            .build();
        table.add(&tag);
        self.0.indents.borrow_mut().push((tag.clone(), extra));
        tag
    }

    /// Centra la columna con márgenes (la vista no se envuelve en otro widget).
    pub(super) fn center(&self) {
        let weak = Rc::downgrade(&self.0);
        let apply = move |width: f64| {
            let Some(i) = weak.upgrade() else { return };
            let side = ((width as i32 - COLUMN) / 2).max(16);
            let text = (width as i32 - 2 * side).clamp(120, COLUMN);
            if i.side.get() == side && i.view.left_margin() == side && i.width.get() == text {
                return;
            }
            i.side.set(side);
            i.width.set(text);
            i.wide.borrow_mut().retain(|w| match w.upgrade() {
                Some(w) => {
                    w.set_size_request(text, -1);
                    true
                }
                None => false,
            });
            i.view.set_left_margin(side);
            i.view.set_right_margin(side);
            for (tag, extra) in i.indents.borrow().iter() {
                tag.set_left_margin(side + extra);
            }
        };
        let h = self.0.scroller.hadjustment();
        apply(h.page_size());
        h.connect_page_size_notify(move |a| apply(a.page_size()));
    }

    pub(super) fn new_tag(&self, s: &RenderState, tag: gtk::TextTag) -> gtk::TextTag {
        s.buffer.tag_table().add(&tag);
        s.tags.borrow_mut().push(tag.clone());
        tag
    }
}
