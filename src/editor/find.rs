//! Barra de búsqueda y reemplazo en la nota abierta (US3, FR-013).

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gtk::glib;
use gtk::prelude::*;

type SearchFn = Box<dyn Fn(&str)>;
type ActionFn = Box<dyn Fn()>;
type ReplaceFn = Box<dyn Fn(&str)>;

struct Callbacks {
    on_search: RefCell<Option<SearchFn>>,
    on_next: RefCell<Option<ActionFn>>,
    on_prev: RefCell<Option<ActionFn>>,
    on_replace: RefCell<Option<ReplaceFn>>,
    on_replace_all: RefCell<Option<ReplaceFn>>,
    on_close: RefCell<Option<ActionFn>>,
}

/// Barra de búsqueda y reemplazo en la nota abierta.
#[derive(Clone)]
pub struct FindBar {
    bar: gtk::SearchBar,
    entry: gtk::SearchEntry,
    counter: gtk::Label,
    btn_prev: gtk::Button,
    btn_next: gtk::Button,
    btn_toggle_replace: gtk::ToggleButton,
    btn_close: gtk::Button,
    replace_revealer: gtk::Revealer,
    replace_entry: gtk::Entry,
    btn_replace: gtk::Button,
    btn_replace_all: gtk::Button,
    supports_replace: Rc<Cell<bool>>,
    callbacks: Rc<Callbacks>,
}

impl Default for FindBar {
    fn default() -> Self {
        Self::new()
    }
}

impl FindBar {
    pub fn new() -> Self {
        let bar = gtk::SearchBar::new();
        bar.set_show_close_button(false);

        // Fila de búsqueda
        let search_box = gtk::Box::new(gtk::Orientation::Horizontal, 6);

        let entry = gtk::SearchEntry::new();
        entry.set_hexpand(true);
        entry.set_placeholder_text(Some("Buscar en la nota…"));
        entry.update_property(&[gtk::accessible::Property::Label("Buscar en la nota")]);

        let counter = gtk::Label::new(None);
        counter.add_css_class("dim-label");
        counter.add_css_class("numeric");
        counter.set_valign(gtk::Align::Center);
        counter.set_margin_start(4);
        counter.set_margin_end(4);
        counter.set_tooltip_text(Some("Posición de coincidencia"));
        counter.update_property(&[gtk::accessible::Property::Description(
            "Posición de coincidencia",
        )]);

        let btn_prev = gtk::Button::from_icon_name("go-up-symbolic");
        btn_prev.add_css_class("flat");
        btn_prev.set_tooltip_text(Some("Coincidencia anterior (Mayús+Intro)"));
        btn_prev.update_property(&[gtk::accessible::Property::Label("Coincidencia anterior")]);

        let btn_next = gtk::Button::from_icon_name("go-down-symbolic");
        btn_next.add_css_class("flat");
        btn_next.set_tooltip_text(Some("Coincidencia siguiente (Intro)"));
        btn_next.update_property(&[gtk::accessible::Property::Label("Coincidencia siguiente")]);

        let btn_toggle_replace = gtk::ToggleButton::new();
        btn_toggle_replace.set_icon_name("edit-find-replace-symbolic");
        btn_toggle_replace.add_css_class("flat");
        btn_toggle_replace.set_tooltip_text(Some("Alternar reemplazo (Ctrl+H)"));
        btn_toggle_replace
            .update_property(&[gtk::accessible::Property::Label("Alternar reemplazo")]);

        let btn_close = gtk::Button::from_icon_name("window-close-symbolic");
        btn_close.add_css_class("flat");
        btn_close.set_tooltip_text(Some("Cerrar búsqueda (Escape)"));
        btn_close.update_property(&[gtk::accessible::Property::Label("Cerrar búsqueda")]);

        search_box.append(&entry);
        search_box.append(&counter);
        search_box.append(&btn_prev);
        search_box.append(&btn_next);
        search_box.append(&btn_toggle_replace);
        search_box.append(&btn_close);

        // Fila de reemplazo
        let replace_box = gtk::Box::new(gtk::Orientation::Horizontal, 6);

        let replace_entry = gtk::Entry::new();
        replace_entry.set_hexpand(true);
        replace_entry.set_placeholder_text(Some("Reemplazar con…"));
        replace_entry.update_property(&[gtk::accessible::Property::Label("Reemplazar con")]);

        let btn_replace = gtk::Button::with_label("Reemplazar");
        btn_replace.update_property(&[gtk::accessible::Property::Label("Reemplazar")]);

        let btn_replace_all = gtk::Button::with_label("Reemplazar todo");
        btn_replace_all.update_property(&[gtk::accessible::Property::Label("Reemplazar todo")]);

        replace_box.append(&replace_entry);
        replace_box.append(&btn_replace);
        replace_box.append(&btn_replace_all);

        let replace_revealer = gtk::Revealer::new();
        replace_revealer.set_child(Some(&replace_box));
        replace_revealer.set_transition_type(gtk::RevealerTransitionType::SlideDown);

        let outer = gtk::Box::new(gtk::Orientation::Vertical, 6);
        outer.set_margin_start(12);
        outer.set_margin_end(12);
        outer.set_margin_top(6);
        outer.set_margin_bottom(6);
        outer.append(&search_box);
        outer.append(&replace_revealer);

        bar.set_child(Some(&outer));
        bar.connect_entry(&entry);

        let callbacks = Rc::new(Callbacks {
            on_search: RefCell::new(None),
            on_next: RefCell::new(None),
            on_prev: RefCell::new(None),
            on_replace: RefCell::new(None),
            on_replace_all: RefCell::new(None),
            on_close: RefCell::new(None),
        });

        let rev = replace_revealer.clone();
        btn_toggle_replace.connect_toggled(move |btn| {
            rev.set_reveal_child(btn.is_active());
        });

        let bar_clone = bar.clone();
        btn_close.connect_clicked(move |_| {
            bar_clone.set_search_mode(false);
        });

        // Al cambiar el modo de búsqueda a falso, notificar cierre
        let cb_clone = callbacks.clone();
        bar.connect_search_mode_enabled_notify(move |b| {
            if !b.is_search_mode()
                && let Some(f) = cb_clone.on_close.borrow().as_ref()
            {
                f();
            }
        });

        // Búsqueda en tiempo real
        let cb_clone = callbacks.clone();
        entry.connect_search_changed(move |e| {
            if let Some(f) = cb_clone.on_search.borrow().as_ref() {
                f(e.text().as_str());
            }
        });

        // Teclado en search_entry
        let cb_clone = callbacks.clone();
        let bar_clone = bar.clone();
        let key_ctrl = gtk::EventControllerKey::new();
        key_ctrl.connect_key_pressed(move |_, keyval, _, state| match keyval {
            gtk::gdk::Key::Escape => {
                bar_clone.set_search_mode(false);
                glib::Propagation::Stop
            }
            gtk::gdk::Key::Return | gtk::gdk::Key::KP_Enter => {
                if state.contains(gtk::gdk::ModifierType::SHIFT_MASK) {
                    if let Some(f) = cb_clone.on_prev.borrow().as_ref() {
                        f();
                    }
                } else if let Some(f) = cb_clone.on_next.borrow().as_ref() {
                    f();
                }
                glib::Propagation::Stop
            }
            _ => glib::Propagation::Proceed,
        });
        entry.add_controller(key_ctrl);

        // Teclado en replace_entry
        let cb_clone = callbacks.clone();
        let rep_clone = replace_entry.clone();
        let bar_clone = bar.clone();
        let rep_key_ctrl = gtk::EventControllerKey::new();
        rep_key_ctrl.connect_key_pressed(move |_, keyval, _, state| match keyval {
            gtk::gdk::Key::Escape => {
                bar_clone.set_search_mode(false);
                glib::Propagation::Stop
            }
            gtk::gdk::Key::Return | gtk::gdk::Key::KP_Enter => {
                let text = rep_clone.text().to_string();
                if state.contains(gtk::gdk::ModifierType::CONTROL_MASK) {
                    if let Some(f) = cb_clone.on_replace_all.borrow().as_ref() {
                        f(&text);
                    }
                } else if let Some(f) = cb_clone.on_replace.borrow().as_ref() {
                    f(&text);
                }
                glib::Propagation::Stop
            }
            _ => glib::Propagation::Proceed,
        });
        replace_entry.add_controller(rep_key_ctrl);

        // Botones de navegación
        let cb_clone = callbacks.clone();
        btn_next.connect_clicked(move |_| {
            if let Some(f) = cb_clone.on_next.borrow().as_ref() {
                f();
            }
        });

        let cb_clone = callbacks.clone();
        btn_prev.connect_clicked(move |_| {
            if let Some(f) = cb_clone.on_prev.borrow().as_ref() {
                f();
            }
        });

        // Botones de reemplazo
        let cb_clone = callbacks.clone();
        let rep_clone = replace_entry.clone();
        btn_replace.connect_clicked(move |_| {
            if let Some(f) = cb_clone.on_replace.borrow().as_ref() {
                f(rep_clone.text().as_str());
            }
        });

        let cb_clone = callbacks.clone();
        let rep_clone = replace_entry.clone();
        btn_replace_all.connect_clicked(move |_| {
            if let Some(f) = cb_clone.on_replace_all.borrow().as_ref() {
                f(rep_clone.text().as_str());
            }
        });

        Self {
            bar,
            entry,
            counter,
            btn_prev,
            btn_next,
            btn_toggle_replace,
            btn_close,
            replace_revealer,
            replace_entry,
            btn_replace,
            btn_replace_all,
            supports_replace: Rc::new(Cell::new(true)),
            callbacks,
        }
    }

    pub fn widget(&self) -> &gtk::SearchBar {
        &self.bar
    }

    pub fn entry(&self) -> &gtk::SearchEntry {
        &self.entry
    }

    pub fn replace_entry(&self) -> &gtk::Entry {
        &self.replace_entry
    }

    #[allow(dead_code)]
    pub fn counter_label(&self) -> &gtk::Label {
        &self.counter
    }

    #[allow(dead_code)]
    pub fn btn_prev(&self) -> &gtk::Button {
        &self.btn_prev
    }

    #[allow(dead_code)]
    pub fn btn_next(&self) -> &gtk::Button {
        &self.btn_next
    }

    #[allow(dead_code)]
    pub fn btn_close(&self) -> &gtk::Button {
        &self.btn_close
    }

    #[allow(dead_code)]
    pub fn btn_replace(&self) -> &gtk::Button {
        &self.btn_replace
    }

    #[allow(dead_code)]
    pub fn btn_replace_all(&self) -> &gtk::Button {
        &self.btn_replace_all
    }

    pub fn is_search_mode(&self) -> bool {
        self.bar.is_search_mode()
    }

    pub fn set_search_mode(&self, mode: bool) {
        self.bar.set_search_mode(mode);
    }

    pub fn search_text(&self) -> String {
        self.entry.text().to_string()
    }

    #[allow(dead_code)]
    pub fn replace_text(&self) -> String {
        self.replace_entry.text().to_string()
    }

    pub fn set_search_text(&self, text: &str) {
        self.entry.set_text(text);
    }

    pub fn set_replace_revealed(&self, reveal: bool) {
        if self.supports_replace.get() {
            self.btn_toggle_replace.set_active(reveal);
            self.replace_revealer.set_reveal_child(reveal);
        }
    }

    pub fn set_supports_replace(&self, supports: bool) {
        self.supports_replace.set(supports);
        self.btn_toggle_replace.set_visible(supports);
        if !supports {
            self.replace_revealer.set_reveal_child(false);
        } else {
            self.replace_revealer
                .set_reveal_child(self.btn_toggle_replace.is_active());
        }
    }

    pub fn set_counter(&self, current: Option<usize>, total: usize) {
        let text = self.search_text();
        if text.is_empty() {
            self.counter.set_text("");
        } else if total == 0 {
            self.counter.set_text("0 de 0");
        } else if let Some(idx) = current {
            self.counter.set_text(&format!("{idx} de {total}"));
        } else {
            self.counter.set_text(&format!("1 de {total}"));
        }
    }

    pub fn set_on_search(&self, f: impl Fn(&str) + 'static) {
        *self.callbacks.on_search.borrow_mut() = Some(Box::new(f));
    }

    pub fn set_on_next(&self, f: impl Fn() + 'static) {
        *self.callbacks.on_next.borrow_mut() = Some(Box::new(f));
    }

    pub fn set_on_prev(&self, f: impl Fn() + 'static) {
        *self.callbacks.on_prev.borrow_mut() = Some(Box::new(f));
    }

    pub fn set_on_replace(&self, f: impl Fn(&str) + 'static) {
        *self.callbacks.on_replace.borrow_mut() = Some(Box::new(f));
    }

    pub fn set_on_replace_all(&self, f: impl Fn(&str) + 'static) {
        *self.callbacks.on_replace_all.borrow_mut() = Some(Box::new(f));
    }

    pub fn set_on_close(&self, f: impl Fn() + 'static) {
        *self.callbacks.on_close.borrow_mut() = Some(Box::new(f));
    }
}

/// Vista plana de texto para diffs (historial 010): monoespaciada, sin resaltado de sintaxis,
/// con los tags `diff-added`/`diff-removed` ya registrados en su buffer.
pub fn vista_diff() -> sourceview5::View {
    let buffer = sourceview5::Buffer::new(None);
    let colores = [
        ("diff-removed", crate::style::DIFF_REMOVED_BG.hex()),
        (
            "diff-removed-marker",
            crate::style::DIFF_REMOVED_MARKER.hex(),
        ),
        ("diff-added", crate::style::DIFF_ADDED_BG.hex()),
        ("diff-added-marker", crate::style::DIFF_ADDED_MARKER.hex()),
    ];
    for (nombre, color) in colores {
        buffer.tag_table().add(
            &gtk::TextTag::builder()
                .name(nombre)
                .foreground(color)
                .build(),
        );
    }
    let view = sourceview5::View::with_buffer(&buffer);
    view.set_editable(false);
    view.set_monospace(true);
    view.upcast_ref::<gtk::Widget>()
        .update_property(&[gtk::accessible::Property::Label(
            "Diferencia con el contenido actual",
        )]);
    let _ = &view;
    view
}
