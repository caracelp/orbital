//! Vista Código: GtkSourceView con resaltado de Markdown y HTML (research R1).

use gtk::prelude::*;
use sourceview5::prelude::*;

/// Crea el buffer y la vista de código.
pub fn new_view() -> (sourceview5::View, sourceview5::Buffer) {
    let buffer = sourceview5::Buffer::new(None);
    buffer.set_highlight_syntax(true);
    buffer.set_highlight_matching_brackets(false);
    if let Some(scheme) = sourceview5::StyleSchemeManager::default().scheme("Adwaita-dark") {
        buffer.set_style_scheme(Some(&scheme));
    }
    let view = sourceview5::View::with_buffer(&buffer);
    view.set_wrap_mode(gtk::WrapMode::WordChar);
    view.set_monospace(true);
    view.set_tab_width(4);
    view.set_insert_spaces_instead_of_tabs(true);
    view.set_auto_indent(true);
    view.set_top_margin(24);
    view.set_bottom_margin(48);
    view.set_left_margin(12);
    view.set_right_margin(12);
    view.set_vexpand(true);
    view.add_css_class("orbital-code");
    view.upcast_ref::<gtk::Widget>()
        .update_property(&[gtk::accessible::Property::Label("Texto de la nota")]);
    (view, buffer)
}

/// Elige el resaltado según el formato de la nota.
pub fn set_language(buffer: &sourceview5::Buffer, language: &str) {
    let lang = sourceview5::LanguageManager::default().language(language);
    buffer.set_language(lang.as_ref());
}

/// Centra el texto en una columna de hasta `max` píxeles con márgenes, sin envolver la vista en
/// otro widget (que le haría perder el cálculo incremental de la disposición).
pub fn center_column(scroller: &gtk::ScrolledWindow, view: &impl IsA<gtk::TextView>, max: i32) {
    let view = view.as_ref().clone();
    let apply = move |width: f64| {
        let side = ((width as i32 - max) / 2).max(12);
        if view.left_margin() != side {
            view.set_left_margin(side);
            view.set_right_margin(side);
        }
    };
    let h = scroller.hadjustment();
    apply(h.page_size());
    h.connect_page_size_notify(move |a| apply(a.page_size()));
}
