//! Edición de notas `.html` desde la vista Visual (US4 de la 002, research R6).
//!
//! Guardar desde Visual reemplaza solo el contenido de `<body>` y, si hace falta, el único
//! `<style data-orbital>` del `<head>`; el resto del archivo queda igual byte a byte (FR-015,
//! SC-004). No es un analizador HTML completo: un escáner que conoce lo justo para no confundirse
//! con comentarios, `<script>` (incluido el bloque de propiedades) y `<style>`.

use std::ops::Range;

/// Una etiqueta encontrada por el escáner.
struct Tag {
    /// Desde `<` hasta después de `>`.
    range: Range<usize>,
    /// Nombre en minúsculas (`/body` para el cierre).
    name: String,
    /// Nombres de los atributos, en minúsculas.
    attrs: Vec<String>,
}

/// Recorre las etiquetas de `s` desde `from`, salteando comentarios y el contenido de
/// `<script>` y `<style>`.
fn tags(s: &str, from: usize) -> impl Iterator<Item = Tag> + '_ {
    let mut i = from;
    std::iter::from_fn(move || {
        loop {
            let lt = i + s.get(i..)?.find('<')?;
            if s[lt..].starts_with("<!--") {
                i = s[lt + 4..].find("-->").map_or(s.len(), |e| lt + 4 + e + 3);
                continue;
            }
            let tag = parse_tag(s, lt);
            let Some(tag) = tag else {
                i = lt + 1;
                continue;
            };
            i = tag.range.end;
            if matches!(
                tag.name.as_str(),
                "script"
                    | "style"
                    | "title"
                    | "textarea"
                    | "xmp"
                    | "iframe"
                    | "noembed"
                    | "noframes"
            ) {
                // El contenido es texto crudo hasta su cierre.
                let close = format!("</{}", tag.name);
                let lower = s[i..].to_ascii_lowercase();
                i = lower.find(&close).map_or(s.len(), |e| i + e);
            }
            return Some(tag);
        }
    })
}

/// Lee la etiqueta que empieza en `lt` (`<nombre …>` o `</nombre>`).
fn parse_tag(s: &str, lt: usize) -> Option<Tag> {
    let b = s.as_bytes();
    let mut i = lt + 1;
    let closing = b.get(i) == Some(&b'/');
    if closing {
        i += 1;
    }
    let start = i;
    while i < b.len() && (b[i].is_ascii_alphanumeric() || b[i] == b'-') {
        i += 1;
    }
    if i == start {
        return None;
    }
    let mut name = s[start..i].to_ascii_lowercase();
    if closing {
        name.insert(0, '/');
    }
    let mut attrs = Vec::new();
    loop {
        while i < b.len() && (b[i].is_ascii_whitespace() || b[i] == b'/') {
            i += 1;
        }
        match b.get(i) {
            None => return None,
            Some(b'>') => break,
            _ => {}
        }
        let a = i;
        while i < b.len() && !b[i].is_ascii_whitespace() && !matches!(b[i], b'=' | b'>' | b'/') {
            i += 1;
        }
        attrs.push(s[a..i].to_ascii_lowercase());
        while i < b.len() && b[i].is_ascii_whitespace() {
            i += 1;
        }
        if b.get(i) == Some(&b'=') {
            i += 1;
            while i < b.len() && b[i].is_ascii_whitespace() {
                i += 1;
            }
            match b.get(i) {
                Some(&q @ (b'"' | b'\'')) => {
                    i += 1;
                    i += s[i..].find(q as char)? + 1;
                }
                _ => {
                    while i < b.len() && !b[i].is_ascii_whitespace() && b[i] != b'>' {
                        i += 1;
                    }
                }
            }
        }
    }
    Some(Tag {
        range: lt..i + 1,
        name,
        attrs,
    })
}

/// Rango del contenido de `<body>` (sin las etiquetas). `None` si no hay `<body>`.
pub fn body_range(source: &str) -> Option<Range<usize>> {
    let mut it = tags(source, 0);
    let open = it.find(|t| t.name == "body")?;
    let end = tags(source, open.range.end)
        .find(|t| t.name == "/body" || t.name == "/html")
        .map_or(source.len(), |t| t.range.start);
    Some(open.range.end..end)
}

/// Reemplaza el contenido de `<body>` por `inner`; todo lo demás queda igual. `None` si no hay
/// `<body>`.
pub fn splice_body(source: &str, inner: &str) -> Option<String> {
    let r = body_range(source)?;
    let mut out = String::with_capacity(source.len() - r.len() + inner.len());
    out.push_str(&source[..r.start]);
    out.push_str(inner);
    out.push_str(&source[r.end..]);
    Some(out)
}

/// Pone `css` en el único `<style data-orbital>` del `<head>`: lo reemplaza si existe o lo crea
/// antes de `</head>`. Sin `css` y sin estilo previo, no cambia nada. `None` si hace falta crearlo
/// y no hay `</head>`.
pub fn upsert_orbital_style(source: &str, css: &str) -> Option<String> {
    let body = body_range(source).map_or(source.len(), |r| r.start);
    let mut found = None;
    let mut head_end = None;
    for t in tags(source, 0) {
        if t.range.start >= body {
            break;
        }
        if t.name == "style" && t.attrs.iter().any(|a| a == "data-orbital") {
            let lower = source[t.range.end..].to_ascii_lowercase();
            let close = lower
                .find("</style")
                .map_or(source.len(), |e| t.range.end + e);
            found = Some(t.range.end..close);
            break;
        }
        if t.name == "/head" {
            head_end = Some(t.range.start);
        }
    }
    let mut out = String::with_capacity(source.len() + css.len() + 32);
    match (found, head_end) {
        (Some(r), _) => {
            if source[r.clone()] == *css {
                return Some(source.to_owned());
            }
            out.push_str(&source[..r.start]);
            out.push_str(css);
            out.push_str(&source[r.end..]);
        }
        (None, _) if css.is_empty() => return Some(source.to_owned()),
        (None, Some(at)) => {
            out.push_str(&source[..at]);
            out.push_str("<style data-orbital>");
            out.push_str(css);
            out.push_str("</style>\n");
            out.push_str(&source[at..]);
        }
        (None, None) => return None,
    }
    Some(out)
}

/// El contenido de `<style data-orbital>`, si existe.
pub fn orbital_style(source: &str) -> Option<&str> {
    let body = body_range(source).map_or(source.len(), |r| r.start);
    let t = tags(source, 0)
        .take_while(|t| t.range.start < body)
        .find(|t| t.name == "style" && t.attrs.iter().any(|a| a == "data-orbital"))?;
    let lower = source[t.range.end..].to_ascii_lowercase();
    let close = lower
        .find("</style")
        .map_or(source.len(), |e| t.range.end + e);
    Some(&source[t.range.end..close])
}

/// El cuerpo tiene `<script>` o atributos `on…`: la vista Visual los perdería al guardar
/// (WebKit los quita al cargar), así que queda desactivada.
pub fn body_has_scripts(source: &str) -> bool {
    let Some(r) = body_range(source) else {
        return false;
    };
    tags(source, 0)
        .skip_while(|t| t.name != "body")
        .take_while(|t| t.range.start < r.end)
        .any(|t| t.name == "script" || t.attrs.iter().any(|a| a.len() > 2 && a.starts_with("on")))
}
