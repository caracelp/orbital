//! Vista Código: GtkSourceView con resaltado de Markdown y HTML (research R1).

use gtk::prelude::*;
use sourceview5::prelude::*;

static INIT_SCHEME: std::sync::Once = std::sync::Once::new();

/// Genera la definición XML del esquema de color `orbital-dark`.
pub fn scheme_xml() -> String {
    format!(
        r##"<?xml version="1.0" encoding="UTF-8"?>
<style-scheme id="orbital-dark" _name="Orbital Dark" version="1.0">
  <author>Orbital</author>
  <_description>Esquema oscuro y rosa para Orbital</_description>

  <style name="text" foreground="{text}" background="{bg}"/>
  <style name="selection" background="#3d2331"/>
  <style name="cursor" foreground="{accent}"/>
  <style name="current-line" background="#1e1e23"/>
  <style name="line-numbers" foreground="{comment}" background="{bg}"/>

  <style name="def:comment" foreground="{comment}"/>
  <style name="def:shebang" foreground="{comment}" bold="true"/>
  <style name="def:doc-comment-element" italic="true"/>
  <style name="def:constant" foreground="{type_col}"/>
  <style name="def:string" foreground="{string_col}"/>
  <style name="def:special-char" foreground="{accent}"/>
  <style name="def:identifier" foreground="{text}"/>
  <style name="def:statement" foreground="{accent}" bold="true"/>
  <style name="def:type" foreground="{type_col}"/>
  <style name="def:preprocessor" foreground="{accent}"/>

  <style name="def:emphasis" italic="true"/>
  <style name="def:strong-emphasis" foreground="{accent}" bold="true"/>
  <style name="def:inline-code" foreground="{string_col}"/>
  <style name="def:heading" foreground="{accent}" bold="true"/>
  <style name="def:link-text" foreground="{accent}"/>
  <style name="def:link-symbol" foreground="{accent}"/>
  <style name="def:list-marker" foreground="{accent}" bold="true"/>

  <style name="xml:tags" foreground="{accent}"/>
  <style name="xml:attribute-name" foreground="{type_col}"/>
  <style name="xml:element-name" foreground="{accent}"/>
</style-scheme>
"##,
        text = crate::style::SYNTAX_TEXT.hex(),
        bg = crate::style::VIEW_BG.hex(),
        accent = crate::style::SYNTAX_KEYWORD.hex(),
        comment = crate::style::SYNTAX_COMMENT.hex(),
        string_col = crate::style::SYNTAX_STRING.hex(),
        type_col = crate::style::SYNTAX_TYPE.hex(),
    )
}

/// Asegura que el esquema de estilo `orbital-dark` esté registrado en el StyleSchemeManager.
pub fn ensure_scheme() {
    INIT_SCHEME.call_once(|| {
        let dir = gtk::glib::user_cache_dir().join("orbital/styles");
        let _ = std::fs::create_dir_all(&dir);
        let scheme_file = dir.join("orbital-dark.xml");
        let _ = std::fs::write(&scheme_file, scheme_xml());

        let sm = sourceview5::StyleSchemeManager::default();
        sm.prepend_search_path(&dir.to_string_lossy());
        sm.force_rescan();
    });
}

/// Crea el buffer y la vista de código.
pub fn new_view() -> (sourceview5::View, sourceview5::Buffer) {
    let buffer = sourceview5::Buffer::new(None);
    buffer.set_highlight_syntax(true);
    buffer.set_highlight_matching_brackets(false);
    ensure_scheme();
    let sm = sourceview5::StyleSchemeManager::default();
    if let Some(scheme) = sm
        .scheme("orbital-dark")
        .or_else(|| sm.scheme("Adwaita-dark"))
    {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scheme_xml_contains_colors_and_id() {
        let xml = scheme_xml();
        assert!(xml.contains("id=\"orbital-dark\""));
        assert!(xml.contains(&crate::style::SYNTAX_KEYWORD.hex()));
        assert!(xml.contains(&crate::style::VIEW_BG.hex()));
    }
}
