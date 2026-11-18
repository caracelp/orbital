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

  <style name="search-match" background="{search_bg}" foreground="{search_fg}"/>
</style-scheme>
"##,
        text = crate::style::SYNTAX_TEXT.hex(),
        bg = crate::style::VIEW_BG.hex(),
        accent = crate::style::SYNTAX_KEYWORD.hex(),
        comment = crate::style::SYNTAX_COMMENT.hex(),
        string_col = crate::style::SYNTAX_STRING.hex(),
        type_col = crate::style::SYNTAX_TYPE.hex(),
        search_bg = crate::style::SEARCH_MATCH_BG.hex(),
        search_fg = crate::style::SEARCH_MATCH_FG.hex(),
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

/// Margen entre el canal de las flechitas (028) y el texto.
const MARGEN_CANAL: i32 = 4;
/// Ancho mínimo del canal: el ícono de 16 px y su relleno.
pub const CANAL_MIN: i32 = 20;

/// Centra el texto en una columna de hasta `max` píxeles con márgenes, sin envolver la vista en
/// otro widget (que le haría perder el cálculo incremental de la disposición). Con `canal` (las
/// flechitas del plegado, 028) visible, su ancho toma el lugar del margen izquierdo: la flechita
/// queda al lado del texto y no en el borde de la ventana.
///
/// Con un canal, cambiar márgenes o anchos dentro del aviso de `page-size` (que GTK emite
/// mientras valida lo visible) rompe la aserción `onscreen_validated` de `GtkTextView`: el ajuste
/// va a un *idle*, uno por vez.
pub fn center_column(
    scroller: &gtk::ScrolledWindow,
    view: &impl IsA<gtk::TextView>,
    max: i32,
    canal: Option<&sourceview5::GutterRendererPixbuf>,
) {
    use std::cell::Cell;
    use std::rc::Rc;
    let view = view.as_ref().clone();
    let notificar = canal.cloned();
    let canal = canal.cloned();
    let h = scroller.hadjustment();
    let ajustar = {
        let h = h.clone();
        move || {
            // La página del ajuste horizontal es el área de texto, sin el canal: con el canal
            // visible, el ancho total es la página más el canal (si no, agrandar el canal
            // achica la página y el ancho oscila).
            let canal_visible = canal.as_ref().filter(|c| c.is_visible());
            let total =
                h.page_size() as i32 + canal_visible.map_or(0, |c| c.width_request().max(0));
            let side = ((total - max) / 2).max(12);
            let left = match canal_visible {
                Some(c) => {
                    let ancho = (side - MARGEN_CANAL).max(CANAL_MIN);
                    if c.width_request() != ancho {
                        c.set_size_request(ancho, -1);
                    }
                    MARGEN_CANAL
                }
                None => side,
            };
            if view.left_margin() != left || view.right_margin() != side {
                view.set_left_margin(left);
                view.set_right_margin(side);
            }
        }
    };
    if notificar.is_none() {
        // Sin canal, como siempre: en el acto.
        ajustar();
        h.connect_page_size_notify(move |_| ajustar());
        return;
    }
    let pendiente = Rc::new(Cell::new(false));
    let ajustar = Rc::new(ajustar);
    let programar = Rc::new(move || {
        if pendiente.replace(true) {
            return;
        }
        let (p, a) = (pendiente.clone(), ajustar.clone());
        gtk::glib::idle_add_local_once(move || {
            p.set(false);
            a();
        });
    });
    programar();
    if let Some(c) = notificar {
        let p = programar.clone();
        c.connect_visible_notify(move |_| p());
    }
    h.connect_page_size_notify(move |_| programar());
}

/// Verifica si el cursor está dentro de una secuencia `[[...` no cerrada en la misma línea.
/// Retorna `Some((query, start_offset))` donde `start_offset` es la posición de carácter donde
/// empieza `[[`, y `query` es el texto entre `[[` y el cursor.
pub fn link_query_at_cursor(buffer: &sourceview5::Buffer) -> Option<(String, i32)> {
    let cursor_iter = buffer.iter_at_offset(buffer.cursor_position());
    let mut line_start = cursor_iter;
    line_start.set_line_offset(0);

    let line_text = buffer.text(&line_start, &cursor_iter, true);
    // Buscar el último "[[" antes del cursor en la línea
    let last_open = line_text.rfind("[[")?;
    let after_open = &line_text[last_open + 2..];

    // Si entre "[[" y el cursor ya hay "]]", "|" o "#", no estamos autocompletando el nombre de la nota
    if after_open.contains("]]") || after_open.contains('|') || after_open.contains('#') {
        return None;
    }

    let chars_before = line_text[..last_open].chars().count() as i32;
    let start_offset = line_start.offset() + chars_before;
    Some((after_open.to_string(), start_offset))
}

/// `[[nombre#^filtro` antes del cursor (026): `(nombre, filtro, offset de «[[»)`. `nombre` vacío es
/// la nota abierta.
pub fn block_query_at_cursor(buffer: &sourceview5::Buffer) -> Option<(String, String, i32)> {
    let cursor = buffer.iter_at_offset(buffer.cursor_position());
    let mut line_start = cursor;
    line_start.set_line_offset(0);
    let line = buffer.text(&line_start, &cursor, true);
    let open = line.rfind("[[")?;
    let after = &line[open + 2..];
    if after.contains("]]") || after.contains('|') {
        return None;
    }
    let (nombre, filtro) = after.split_once("#^")?;
    if nombre.contains('#')
        || !filtro
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-')
    {
        return None;
    }
    let start = line_start.offset() + line[..open].chars().count() as i32;
    Some((nombre.to_owned(), filtro.to_owned(), start))
}

/// Busca primero el prefijo en la línea actual para no analizar la nota en cada movimiento.
pub fn tag_query_at_cursor(buffer: &sourceview5::Buffer, is_html: bool) -> Option<(String, i32)> {
    let cursor = buffer.iter_at_offset(buffer.cursor_position());
    let mut start = cursor;
    start.set_line_offset(0);
    let line = buffer.text(&start, &cursor, true);
    let hash = line.rfind('#')?;
    if !line[hash + 1..]
        .chars()
        .all(|c| c.is_alphanumeric() || "_-/".contains(c))
    {
        return None;
    }
    let text = buffer.text(&buffer.start_iter(), &buffer.end_iter(), true);
    let byte_cursor = text
        .char_indices()
        .nth(cursor.offset() as usize)
        .map_or(text.len(), |(at, _)| at);
    let (query, at) = orbital_vault::tags::completion_query(&text, byte_cursor, is_html)?;
    Some((query.to_owned(), text[..at].chars().count() as i32))
}

/// El reemplazo completo es una sola acción de deshacer; incluye el sufijo si se completa en medio.
pub fn insert_tag_completion(buffer: &sourceview5::Buffer, start: i32, tag: &str) {
    let mut end = buffer.iter_at_offset(buffer.cursor_position());
    while end.char().is_alphanumeric() || "_-/".contains(end.char()) {
        if !end.forward_char() {
            break;
        }
    }
    buffer.begin_user_action();
    let mut at = buffer.iter_at_offset(start);
    buffer.delete(&mut at, &mut end);
    buffer.insert(&mut at, &format!("#{tag}"));
    buffer.place_cursor(&at);
    buffer.end_user_action();
}

/// Verdadero si `c` puede ser parte de la clave de un marcador `[%clave]`: el mismo criterio del
/// núcleo (`orbital_vault::markers`, sin espacios ni delimitadores de sintaxis).
fn is_marker_key_char(c: char) -> bool {
    !c.is_whitespace() && !matches!(c, ':' | '`' | '[' | ']' | '\\' | '"' | '\'' | '<' | '>')
}

/// Verifica si el cursor está dentro de un marcador `[%…` sin cerrar en la misma línea.
/// Retorna `Some((query, start_offset))` donde `start_offset` es la posición de carácter del `[`,
/// y `query` el prefijo de clave escrito hasta el cursor. Sin sugerir si el resto de la línea ya
/// tiene `]`, ni dentro de código, enlaces o frontmatter: la misma exclusión del núcleo decide,
/// cerrando el marcador de prueba y viendo si `find_markers` lo habría detectado.
pub fn marker_query_at_cursor(
    buffer: &sourceview5::Buffer,
    is_html: bool,
) -> Option<(String, i32)> {
    let cursor = buffer.iter_at_offset(buffer.cursor_position());
    let mut line_start = cursor;
    line_start.set_line_offset(0);
    let hasta_cursor = buffer.text(&line_start, &cursor, true);
    let mut line_end = cursor;
    if !line_end.ends_line() {
        line_end.forward_to_line_end();
    }
    let line = buffer.text(&line_start, &line_end, true);

    let bracket = hasta_cursor.rfind("[%")?;
    let prefix = &hasta_cursor[bracket + 2..];
    if !prefix.chars().all(is_marker_key_char) {
        return None;
    }
    // Un `]` en el resto de la línea ya cierra el marcador: no se sugiere nada.
    if line[bracket..].contains(']') {
        return None;
    }
    // No sugerir dentro de enlaces todavía sin cerrar (igual que el completado de etiquetas).
    if hasta_cursor[..bracket]
        .rfind('[')
        .is_some_and(|open| !hasta_cursor[open..bracket].contains(']'))
        || hasta_cursor[..bracket]
            .rfind("](")
            .is_some_and(|open| !hasta_cursor[open + 2..bracket].contains(')'))
    {
        return None;
    }
    // Mientras se escribe, un backtick sin cerrar expresa intención de código aunque CommonMark
    // todavía lo interprete como texto literal (igual que el completado de etiquetas).
    if !is_html {
        let mut chars = hasta_cursor[..bracket].chars().peekable();
        let mut open_ticks = 0;
        while let Some(c) = chars.next() {
            if c == '\\' {
                chars.next();
            } else if c == '`' {
                let mut width = 1;
                while chars.peek() == Some(&'`') {
                    chars.next();
                    width += 1;
                }
                if open_ticks == 0 {
                    open_ticks = width;
                } else if open_ticks == width {
                    open_ticks = 0;
                }
            }
        }
        if open_ticks != 0 {
            return None;
        }
    }

    let text = buffer.text(&buffer.start_iter(), &buffer.end_iter(), true);
    let line_bytes = buffer.text(&buffer.start_iter(), &line_start, true).len();
    let at = line_bytes + bracket;
    // Cerrar el marcador en la posición del cursor y dejar la línea como esté: la exclusión del
    // núcleo (código, enlaces, frontmatter, escape) es una sola fuente de verdad. Con prefijo
    // vacío el núcleo no detecta claves vacías: se cierra con una clave de prueba.
    let key_prueba = if prefix.is_empty() { "k" } else { prefix };
    let mut sample = String::with_capacity(text.len() + 2);
    sample.push_str(&text[..at + 2]);
    sample.push_str(key_prueba);
    sample.push(']');
    sample.push_str(&text[line_bytes + line.len()..]);
    let detectado = orbital_vault::markers::find_markers(&sample, is_html)
        .into_iter()
        .any(|m| m.range.start == at);
    if !detectado {
        return None;
    }
    Some((prefix.to_owned(), text[..at].chars().count() as i32))
}

/// El reemplazo completo es una sola acción de deshacer: borra el prefijo escrito e inserta
/// `[%clave]` cerrado, con el cursor después del `]`.
pub fn insert_marker_completion(buffer: &sourceview5::Buffer, start: i32, key: &str) {
    let mut end = buffer.iter_at_offset(buffer.cursor_position());
    while is_marker_key_char(end.char()) && end.forward_char() {}
    buffer.begin_user_action();
    let mut at = buffer.iter_at_offset(start);
    buffer.delete(&mut at, &mut end);
    buffer.insert(&mut at, &format!("[%{key}]"));
    buffer.place_cursor(&at);
    buffer.end_user_action();
}

/// El tipo de relación que se escribe al principio de un ítem de la parte Conexiones (049):
/// `Some((consulta, inicio))` con la posición de carácter donde empieza el tipo. Solo en notas
/// Markdown y si el delimitador anterior más cercano es `<!-- connections ID -->`.
pub fn relation_query_at_cursor(buffer: &sourceview5::Buffer) -> Option<(String, i32)> {
    let cursor = buffer.iter_at_offset(buffer.cursor_position());
    let mut line_start = cursor;
    line_start.set_line_offset(0);
    let hasta_cursor = buffer.text(&line_start, &cursor, true);
    let (tipo, inicio) = orbital_vault::relaciones::tipo_en_escritura(&hasta_cursor)?;
    // Una parte Conexiones es una lista corta: no hace falta mirar más de 400 renglones atrás
    // (así escribir una lista común en una nota larga no recorre la nota en cada tecla).
    let actual = line_start.line();
    let mut n = actual;
    loop {
        if n == 0 || actual - n >= 400 {
            return None;
        }
        n -= 1;
        let ini = buffer.iter_at_line(n)?;
        let mut fin = ini;
        if !fin.ends_line() {
            fin.forward_to_line_end();
        }
        let linea = buffer.text(&ini, &fin, true);
        let t = linea.trim();
        if !t.starts_with("<!--") {
            continue;
        }
        // Una apertura de Conexiones deja dentro lo que sigue; cualquier otro delimitador,
        // afuera. Un comentario cualquiera no decide: se sigue buscando.
        let mut r = orbital_vault::relaciones::Rastreador::default();
        r.paso(t);
        if r.paso("x") {
            break;
        }
        if t == "<!---->" || orbital_vault::note_ids::delimiter_id(t).is_some() {
            return None;
        }
    }
    let inicio_chars = hasta_cursor[..inicio].chars().count() as i32;
    Some((tipo.to_owned(), line_start.offset() + inicio_chars))
}

/// Reemplaza el tipo que se está escribiendo por `tipo:: ` (049). Si el renglón ya tenía un
/// `::` más adelante (se corrige un tipo), reemplaza hasta ahí. Una sola acción de deshacer.
pub fn insert_relation_completion(buffer: &sourceview5::Buffer, start: i32, tipo: &str) {
    let cursor = buffer.iter_at_offset(buffer.cursor_position());
    let mut line_end = cursor;
    if !line_end.ends_line() {
        line_end.forward_to_line_end();
    }
    let resto = buffer.text(&cursor, &line_end, true);
    let mut end = cursor;
    if let Some(at) = resto.find("::")
        && !resto[..at].contains("[[")
    {
        let mut n = resto[..at + 2].chars().count();
        if resto[at + 2..].starts_with(' ') {
            n += 1;
        }
        end.forward_chars(n as i32);
    }
    buffer.begin_user_action();
    let mut at = buffer.iter_at_offset(start);
    buffer.delete(&mut at, &mut end);
    buffer.insert(&mut at, &format!("{tipo}:: "));
    buffer.place_cursor(&at);
    buffer.end_user_action();
}

/// Rectángulo en coordenadas de ventana para anclar el popover de autocompletado al cursor.
pub fn cursor_popup_rect(
    view: &sourceview5::View,
    buffer: &sourceview5::Buffer,
) -> gtk::gdk::Rectangle {
    let it = buffer.iter_at_offset(buffer.cursor_position());
    let loc = view.iter_location(&it);
    let (wx, wy) = view.buffer_to_window_coords(gtk::TextWindowType::Widget, loc.x(), loc.y());
    gtk::gdk::Rectangle::new(wx, wy, loc.width().max(1), loc.height().max(18))
}

/// Reemplaza la secuencia `[[...` por el wikilink seleccionado `[[target]]`,
/// cerrando automáticamente con `]]` y situando el cursor después de `]]`.
pub fn insert_link_completion(
    buffer: &sourceview5::Buffer,
    view: &sourceview5::View,
    start_offset: i32,
    target: &str,
) {
    let cursor_iter = buffer.iter_at_offset(buffer.cursor_position());
    let mut end_iter = cursor_iter;

    // Si inmediatamente después del cursor hay ']]', incluirlos en el reemplazo para no duplicar
    if end_iter.char() == ']' {
        let mut next = end_iter;
        if next.forward_char() && next.char() == ']' {
            next.forward_char();
            end_iter = next;
        }
    }

    buffer.begin_user_action();
    let mut start_iter = buffer.iter_at_offset(start_offset);
    buffer.delete(&mut start_iter, &mut end_iter);

    let replacement = format!("[[{target}]]");
    let mut at = buffer.iter_at_offset(start_offset);
    buffer.insert(&mut at, &replacement);
    buffer.place_cursor(&at);
    buffer.end_user_action();

    view.grab_focus();
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

    #[test]
    fn code_completion_helpers() {
        let _ = gtk::init();
        let (view, buffer) = new_view();

        buffer.set_text("Inicio de la nota [[bio");
        buffer.place_cursor(&buffer.end_iter());

        let res = link_query_at_cursor(&buffer);
        assert_eq!(res, Some(("bio".to_string(), 18)));

        // Si ya está cerrado con ]]
        buffer.set_text("Inicio de la nota [[bio]] y más texto");
        buffer.place_cursor(&buffer.end_iter());
        assert_eq!(link_query_at_cursor(&buffer), None);

        // Con caracteres acentuados antes
        buffer.set_text("Café con [[al");
        buffer.place_cursor(&buffer.end_iter());
        assert_eq!(link_query_at_cursor(&buffer), Some(("al".to_string(), 9)));

        // Reemplazo y posicionamiento del cursor
        buffer.set_text("Ver [[bio");
        buffer.place_cursor(&buffer.end_iter());

        let (query, start_offset) = link_query_at_cursor(&buffer).unwrap();
        assert_eq!(query, "bio");

        insert_link_completion(&buffer, &view, start_offset, "biologia");

        let text = buffer.text(&buffer.start_iter(), &buffer.end_iter(), true);
        assert_eq!(text, "Ver [[biologia]]");

        // El cursor debe estar al final, justo después de ]]
        assert_eq!(buffer.cursor_position(), text.chars().count() as i32);
    }

    #[test]
    #[ignore = "requiere acceso GTK; tools/verificar-005-us5.sh"]
    fn marker_completion_context() {
        let _ = gtk::init();
        let (_, buffer) = new_view();

        // Justo después de `[`
        buffer.set_text("Hola [");
        buffer.place_cursor(&buffer.end_iter());
        assert_eq!(marker_query_at_cursor(&buffer, false), None);

        // Justo después de `[%`: prefijo vacío, se sugieren todas
        buffer.set_text("Hola [%");
        buffer.place_cursor(&buffer.end_iter());
        assert_eq!(
            marker_query_at_cursor(&buffer, false),
            Some((String::new(), 5))
        );

        // Con prefijo de clave
        buffer.set_text("Hola [%est");
        buffer.place_cursor(&buffer.end_iter());
        assert_eq!(
            marker_query_at_cursor(&buffer, false),
            Some(("est".to_string(), 5))
        );

        // Sin marcador abierto
        buffer.set_text("Hola mundo");
        buffer.place_cursor(&buffer.end_iter());
        assert_eq!(marker_query_at_cursor(&buffer, false), None);

        // El `[` de otro contexto no alcanza: hace falta `[%`
        buffer.set_text("Hola [%x] y [");
        buffer.place_cursor(&buffer.end_iter());
        assert_eq!(marker_query_at_cursor(&buffer, false), None);

        // Ya cerrado y el cursor al final de la línea
        buffer.set_text("Hola [%estado] resto");
        buffer.place_cursor(&buffer.end_iter());
        assert_eq!(marker_query_at_cursor(&buffer, false), None);

        // El cursor antes del `]` que cierra
        buffer.set_text("Hola [%estado] resto");
        buffer.place_cursor(&buffer.iter_at_offset(11));
        assert_eq!(marker_query_at_cursor(&buffer, false), None);

        // Cerrado en la misma línea aunque el cursor esté antes
        buffer.set_text("[%e]\nHola [%");
        buffer.place_cursor(&buffer.end_iter());
        assert_eq!(
            marker_query_at_cursor(&buffer, false),
            Some((String::new(), 10))
        );

        // Código en línea cerrado: el cursor antes del backtick final
        buffer.set_text("Código `[%e`");
        buffer.place_cursor(&buffer.iter_at_offset(11));
        assert_eq!(marker_query_at_cursor(&buffer, false), None);

        // Cerca sin cerrar: CommonMark lo trata como código hasta el fin de línea
        buffer.set_text("Código `[%e");
        buffer.place_cursor(&buffer.end_iter());
        assert_eq!(marker_query_at_cursor(&buffer, false), None);

        // Codeblock con cercas
        buffer.set_text("```\n[%e\n```");
        buffer.place_cursor(&buffer.iter_at_offset(7));
        assert_eq!(marker_query_at_cursor(&buffer, false), None);

        // Dentro de un enlace Markdown
        buffer.set_text("[texto](url[%e");
        buffer.place_cursor(&buffer.end_iter());
        assert_eq!(marker_query_at_cursor(&buffer, false), None);

        // Frontmatter: fuera del bloque sí
        buffer.set_text("---\n[%e\n---\ncuerpo");
        buffer.place_cursor(&buffer.iter_at_offset(7));
        assert_eq!(marker_query_at_cursor(&buffer, false), None);
        buffer.set_text("---\nclave: valor\n---\n[%e");
        buffer.place_cursor(&buffer.end_iter());
        assert_eq!(
            marker_query_at_cursor(&buffer, false),
            Some(("e".to_string(), 21))
        );

        // Escapado con `\`
        buffer.set_text("\\[%e");
        buffer.place_cursor(&buffer.end_iter());
        assert_eq!(marker_query_at_cursor(&buffer, false), None);

        // Unicode alrededor y en la clave
        buffer.set_text("Café 😀 [%pría");
        buffer.place_cursor(&buffer.end_iter());
        assert_eq!(
            marker_query_at_cursor(&buffer, false),
            Some(("pría".to_string(), 7))
        );

        // Inserción: reemplaza el prefijo y cierra, en una acción de deshacer
        buffer.set_text("Café 😀 [%pría");
        buffer.place_cursor(&buffer.end_iter());
        let (_, start) = marker_query_at_cursor(&buffer, false).unwrap();
        insert_marker_completion(&buffer, start, "prioridad");
        let text = buffer.text(&buffer.start_iter(), &buffer.end_iter(), true);
        assert_eq!(text, "Café 😀 [%prioridad]");
        assert_eq!(buffer.cursor_position(), text.chars().count() as i32);
        buffer.undo();
        assert_eq!(
            buffer.text(&buffer.start_iter(), &buffer.end_iter(), true),
            "Café 😀 [%pría"
        );
    }
}
