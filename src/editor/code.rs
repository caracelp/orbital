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
        // Solo se reescribe si cambió (060): leer 2 KB es más barato que escribirlos siempre.
        let xml = scheme_xml();
        if std::fs::read(&scheme_file).ok().as_deref() != Some(xml.as_bytes()) {
            let _ = std::fs::write(&scheme_file, xml);
        }

        let sm = sourceview5::StyleSchemeManager::default();
        sm.prepend_search_path(&dir.to_string_lossy());
        sm.force_rescan();
    });
}

/// El esquema `orbital-dark` o, si faltara, `Adwaita-dark`.
fn esquema() -> Option<sourceview5::StyleScheme> {
    ensure_scheme();
    let sm = sourceview5::StyleSchemeManager::default();
    sm.scheme("orbital-dark")
        .or_else(|| sm.scheme("Adwaita-dark"))
}

thread_local! {
    /// Buffers creados antes de cargar los esquemas (060), que esperan al primer *idle*.
    static SIN_ESQUEMA: std::cell::RefCell<Vec<gtk::glib::WeakRef<sourceview5::Buffer>>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

/// Pone el esquema de colores en `buffer`. Cargar los esquemas (leer y analizar todos los XML
/// de las carpetas de GtkSourceView) cuesta ≈ 10 ms en el Celeron: si todavía no se cargaron,
/// se hace en el primer *idle* (después del primer cuadro) y no al armar la ventana, cuando
/// ninguna vista de código se ve todavía.
fn aplicar_esquema(buffer: &sourceview5::Buffer) {
    if INIT_SCHEME.is_completed() {
        if let Some(scheme) = esquema() {
            buffer.set_style_scheme(Some(&scheme));
        }
        return;
    }
    let primero = SIN_ESQUEMA.with(|p| {
        let mut p = p.borrow_mut();
        p.push(buffer.downgrade());
        p.len() == 1
    });
    if primero {
        gtk::glib::idle_add_local_once(|| {
            let scheme = esquema();
            for b in SIN_ESQUEMA.with(|p| std::mem::take(&mut *p.borrow_mut())) {
                if let (Some(b), Some(s)) = (b.upgrade(), scheme.as_ref()) {
                    b.set_style_scheme(Some(s));
                }
            }
        });
    }
}

/// Crea el buffer y la vista de código.
pub fn new_view() -> (sourceview5::View, sourceview5::Buffer) {
    let buffer = sourceview5::Buffer::new(None);
    buffer.set_highlight_syntax(true);
    buffer.set_highlight_matching_brackets(false);
    aplicar_esquema(&buffer);
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

/// Margen entre el canal de las flechitas (028) o los números de línea (054) y el texto.
const MARGEN_CANAL: i32 = 4;
/// Ancho mínimo del canal: el ícono de 16 px y su relleno.
pub const CANAL_MIN: i32 = 20;
/// Relleno a cada lado de un número de línea.
pub const RELLENO_NUMEROS: i32 = 4;

thread_local! {
    /// Preferencia de los números de línea (054): la última que se aplicó.
    static NUMEROS: std::cell::Cell<bool> = const { std::cell::Cell::new(true) };
    /// Las vistas de nota que la siguen (una por ventana), con su renderer de números; se limpian
    /// las que ya no existen.
    static NUMERADAS: std::cell::RefCell<Vec<(
        gtk::glib::WeakRef<sourceview5::View>,
        gtk::glib::WeakRef<sourceview5::GutterRendererText>,
    )>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// Si los números de línea están prendidos.
pub fn line_numbers() -> bool {
    NUMEROS.with(|n| n.get())
}

/// Prende o apaga los números de línea en todas las vistas de nota abiertas, en el acto.
pub fn set_line_numbers(on: bool) {
    NUMEROS.with(|n| n.set(on));
    NUMERADAS.with(|v| {
        v.borrow_mut()
            .retain(|(view, r)| match (view.upgrade(), r.upgrade()) {
                (Some(view), Some(r)) => {
                    apply_line_numbers(&view, &r, on);
                    true
                }
                _ => false,
            })
    });
}

/// Hace que la vista y su renderer de números sigan la preferencia (y la aplica ya).
pub fn follow_line_numbers(view: &sourceview5::View, numeros: &sourceview5::GutterRendererText) {
    apply_line_numbers(view, numeros, line_numbers());
    NUMERADAS.with(|v| v.borrow_mut().push((view.downgrade(), numeros.downgrade())));
}

fn apply_line_numbers(
    view: &sourceview5::View,
    numeros: &sourceview5::GutterRendererText,
    on: bool,
) {
    numeros.set_visible(on);
    // La línea del cursor, apenas resaltada (su número, más claro, lo pone el renderer).
    view.set_highlight_current_line(on);
}

/// Cifras del número de línea más alto (al menos dos, para que el canal no cambie de ancho
/// entre la línea 9 y la 10).
fn cifras(lineas: i32) -> u32 {
    (lineas.max(10) as u32).ilog10() + 1
}

/// Centra el texto en una columna de hasta `max` píxeles con márgenes, sin envolver la vista en
/// otro widget (que le haría perder el cálculo incremental de la disposición). Con `canal` (las
/// flechitas del plegado, 028) visible, su ancho toma el lugar del margen izquierdo: la flechita
/// queda al lado del texto y no en el borde de la ventana.
///
/// Con `numeros` (054) visible, el que se estira es el renderer de los números (alineados a la
/// derecha) y el canal de las flechitas queda en su ancho mínimo: de izquierda a derecha, el
/// margen libre, los números, las flechitas y el texto, que no se corre de la columna centrada.
/// Si la columna no deja lugar (ventana angosta, Dividida), los números toman el ancho de sus
/// cifras y el texto se corre lo justo. El ancho de las cifras se vuelve a medir solo cuando la
/// nota pasa a tener una cifra más o menos (no en cada tecla).
///
/// Con un canal, cambiar márgenes o anchos dentro del aviso de `page-size` (que GTK emite
/// mientras valida lo visible) rompe la aserción `onscreen_validated` de `GtkTextView`: el ajuste
/// va a un *idle*, uno por vez.
pub fn center_column(
    scroller: &gtk::ScrolledWindow,
    view: &impl IsA<gtk::TextView>,
    max: i32,
    canal: Option<&sourceview5::GutterRendererPixbuf>,
    numeros: Option<&sourceview5::GutterRendererText>,
) {
    use std::cell::Cell;
    use std::rc::Rc;
    let view = view.as_ref().clone();
    let buffer = view.buffer();
    let con_canales = canal.is_some() || numeros.is_some();
    let avisa_canal = canal.cloned();
    let avisa_numeros = numeros.cloned();
    let canal = canal.cloned();
    let numeros = numeros.cloned();
    // Cifras medidas y su ancho.
    let medidas: Rc<Cell<(u32, i32)>> = Rc::new(Cell::new((0, 0)));
    let h = scroller.hadjustment();
    let ajustar = {
        let h = h.clone();
        let medidas = medidas.clone();
        let view = view.clone();
        move || {
            let canal_visible = canal.as_ref().filter(|c| c.is_visible());
            let numeros_visibles = numeros.as_ref().filter(|n| n.is_visible());
            // El ancho total: el de la vista (área de texto más canales). Antes de la primera
            // disposición, la página del ajuste horizontal (el área de texto, sin los canales)
            // más lo pedido por los canales; nunca la página sola: agrandar un canal achica la
            // página y el ancho oscilaría.
            let total = if view.width() > 0 {
                view.width()
            } else {
                h.page_size() as i32
                    + canal_visible.map_or(0, |c| c.width_request().max(0))
                    + numeros_visibles.map_or(0, |n| n.width_request().max(0))
            };
            let side = ((total - max) / 2).max(12);
            let left = match (canal_visible, numeros_visibles) {
                (canal, Some(n)) => {
                    let fijo = match canal {
                        Some(c) => {
                            if c.width_request() != CANAL_MIN {
                                c.set_size_request(CANAL_MIN, -1);
                            }
                            CANAL_MIN
                        }
                        None => 0,
                    };
                    let cifras = cifras(buffer.line_count());
                    let (medidas_cifras, mut minimo) = medidas.get();
                    if medidas_cifras != cifras {
                        let muestra = "0".repeat(cifras as usize);
                        minimo =
                            GutterRendererTextExt::measure(n, &muestra).0 + 2 * RELLENO_NUMEROS;
                        medidas.set((cifras, minimo));
                    }
                    let ancho = (side - MARGEN_CANAL - fijo).max(minimo);
                    if n.width_request() != ancho {
                        n.set_size_request(ancho, -1);
                    }
                    MARGEN_CANAL
                }
                (Some(c), None) => {
                    let ancho = (side - MARGEN_CANAL).max(CANAL_MIN);
                    if c.width_request() != ancho {
                        c.set_size_request(ancho, -1);
                    }
                    MARGEN_CANAL
                }
                (None, None) => side,
            };
            if view.left_margin() != left || view.right_margin() != side {
                view.set_left_margin(left);
                view.set_right_margin(side);
            }
        }
    };
    if !con_canales {
        // Sin canales, como siempre: en el acto.
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
    if let Some(c) = avisa_canal {
        let p = programar.clone();
        c.connect_visible_notify(move |_| p());
    }
    if let Some(n) = avisa_numeros {
        let p = programar.clone();
        n.connect_visible_notify(move |_| p());
        // Una cifra más o menos: medir de nuevo (una comparación por tecla).
        let p = programar.clone();
        view.buffer().connect_changed(move |b| {
            let (medidas_cifras, _) = medidas.get();
            if medidas_cifras != 0 && medidas_cifras != cifras(b.line_count()) {
                p();
            }
        });
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

/// Detecta `:prefijo` en el cursor para autocompletar íconos en línea (062).
/// Exige al menos 1 carácter después de `:` y descarta `::`, `12:30`, URLs, código y frontmatter.
pub fn icon_query_at_cursor(buffer: &sourceview5::Buffer) -> Option<(String, i32)> {
    let cursor = buffer.iter_at_offset(buffer.cursor_position());
    let mut line_start = cursor;
    line_start.set_line_offset(0);
    let before = buffer.text(&line_start, &cursor, true);
    let before_str = before.as_str();
    let colon = before_str.rfind(':')?;
    let query = &before_str[colon + 1..];
    if query.is_empty()
        || !query
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        || !query.as_bytes()[0].is_ascii_alphabetic()
    {
        return None;
    }
    let prev_ok = colon == 0
        || !matches!(before_str.as_bytes()[colon - 1], b':' | b'/' | b'\\')
            && !before_str[..colon]
                .chars()
                .next_back()
                .is_some_and(char::is_alphanumeric);
    if !prev_ok {
        return None;
    }
    // Evitar dentro de código en línea simple (cantidad impar de backticks en la línea).
    if before_str[..colon].bytes().filter(|&b| b == b'`').count() % 2 == 1 {
        return None;
    }
    // Evitar dentro de frontmatter YAML o bloque cercado.
    let full = buffer.text(&buffer.start_iter(), &cursor, true);
    let full_str = full.as_str();
    let mut in_fence = false;
    let mut in_front = false;
    for (idx, l) in full_str.lines().enumerate() {
        let t = l.trim();
        if idx == 0 && t == "---" {
            in_front = true;
            continue;
        }
        if in_front {
            if t == "---" {
                in_front = false;
            }
            continue;
        }
        if t.starts_with("```") || t.starts_with("~~~") {
            in_fence = !in_fence;
        }
    }
    if in_front || in_fence {
        return None;
    }
    let char_pos = before_str[..colon].chars().count() as i32;
    Some((query.to_ascii_lowercase(), line_start.offset() + char_pos))
}

/// Reemplaza `:prefijo` desde `start_offset` hasta el cursor por `:nombre:` (062).
pub fn insert_icon_completion(buffer: &sourceview5::Buffer, start_offset: i32, icon_name: &str) {
    let mut end_iter = buffer.iter_at_offset(buffer.cursor_position());
    if end_iter.char() == ':' {
        end_iter.forward_char();
    }
    buffer.begin_user_action();
    let mut start_iter = buffer.iter_at_offset(start_offset);
    buffer.delete(&mut start_iter, &mut end_iter);
    let replacement = format!(":{icon_name}:");
    let mut at = buffer.iter_at_offset(start_offset);
    buffer.insert(&mut at, &replacement);
    buffer.place_cursor(&at);
    buffer.end_user_action();
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
