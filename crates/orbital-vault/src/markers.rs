//! Marcadores `[%clave]` y secciones de una nota (005, contenido generado).
//!
//! El disco siempre tiene el marcador; la resolución es solo de vista. Todo corre sobre el texto
//! completo de la nota (con frontmatter, como el [`crate::render::RenderDoc`]) y es lineal en su
//! tamaño.

use std::ops::Range;

use pulldown_cmark::{Event, Parser, Tag};

use crate::props::{self, PropEntry};

/// Un marcador `[%clave]` con su rango en bytes sobre el texto completo.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Marker {
    pub key: String,
    pub range: Range<usize>,
}

/// Rango del frontmatter completo (incluidas las líneas `---`), si lo hay.
fn frontmatter_block(text: &str) -> Option<Range<usize>> {
    let content = props::markdown::locate(text)?;
    let start = props::markdown::bom_len(text);
    let end = text[content.end..]
        .find('\n')
        .map_or(text.len(), |i| content.end + i + 1);
    Some(start..end)
}

/// Verdadero si `c` puede ser parte de la clave de un marcador: el mismo criterio de
/// `inline_fields` (sin espacios, `:`, comillas de código ni delimitadores).
fn is_marker_key_char(c: char) -> bool {
    !c.is_whitespace() && !matches!(c, ':' | '`' | '[' | ']' | '\\' | '"' | '\'' | '<' | '>')
}

/// Detecta los marcadores `[%clave]` del texto, fuera de código, enlaces, imágenes y frontmatter,
/// y sin contar los escapados con `\`. El cierre `]` debe estar en la misma línea. Es una sola
/// pasada sobre el texto.
pub fn find_markers(text: &str, is_html: bool) -> Vec<Marker> {
    let mut excluded = crate::tags::excluded_ranges(text, is_html);
    if is_html && let Some(region) = crate::props::html::scan(text).block {
        excluded.push(region);
    }
    if let Some(front) = frontmatter_block(text) {
        excluded.push(front);
        excluded.sort_unstable_by_key(|range| range.start);
    }
    let mut out = Vec::new();
    let mut at = 0;
    for (pos, _) in text.match_indices("[%") {
        if pos < at {
            continue;
        }
        let start = pos + 2; // `[` en pos, `%` en pos + 1: la clave empieza ahí
        let Some(close) = text[start..].find([']', '\n']) else {
            continue;
        };
        if text.as_bytes()[start + close] == b'\n' {
            continue;
        }
        let key = &text[start..start + close];
        if key.is_empty() || !key.chars().all(is_marker_key_char) {
            continue;
        }
        let range = pos..start + close + 1;
        at = range.end;
        if !crate::tags::escaped(text, range.start) && !excluded.iter().any(|e| e.contains(&pos)) {
            out.push(Marker {
                key: key.to_owned(),
                range,
            });
        }
    }
    out
}

/// Rangos de bytes de cada sección del cuerpo: el texto previo al primer encabezado es una
/// sección propia y cada encabezado ATX (`#` a `######`) abre otra hasta el siguiente. Los
/// encabezados dentro de código no cuentan (el análisis de pulldown-cmark ya los excluye).
/// En una nota `.html` el cuerpo entero es una sola sección.
pub fn section_ranges(text: &str) -> Vec<Range<usize>> {
    let body_start = frontmatter_block(text).map_or(0, |front| front.end);
    let mut starts: Vec<usize> = Parser::new(text)
        .into_offset_iter()
        .filter_map(|(event, range)| {
            matches!(event, Event::Start(Tag::Heading { .. })).then_some(range.start)
        })
        .filter(|&start| start >= body_start)
        .collect();
    if starts.first().is_some_and(|&first| first > body_start) {
        starts.insert(0, body_start);
    }
    if starts.is_empty() {
        return (body_start < text.len())
            .then_some(body_start..text.len())
            .into_iter()
            .collect();
    }
    let mut out = Vec::with_capacity(starts.len());
    for (n, &start) in starts.iter().enumerate() {
        let end = starts.get(n + 1).copied().unwrap_or(text.len());
        out.push(start..end);
    }
    out
}

/// Resuelve cada marcador de una nota `.md`: campo inline `clave:: valor` de la misma sección
/// (gana el primero si repite), después propiedad del YAML y por último el rótulo `«faltante»`.
pub fn resolve_markers(text: &str, yaml: &[PropEntry]) -> Vec<(Range<usize>, String)> {
    let markers = find_markers(text, false);
    if markers.is_empty() {
        return Vec::new();
    }
    let sections = section_ranges(text);
    resolve_values(text, &markers, &sections, yaml)
        .into_iter()
        .zip(markers)
        .map(|(value, marker)| (marker.range, value))
        .collect()
}

/// Resolución de una nota `.html` como pares `(clave, valor)` en orden de aparición: el DOM de la
/// vista Visual vuelve a buscar `[%clave]`, así que lo que necesita son los valores por clave.
/// La nota entera es una sola sección: inline gana sobre el YAML del `<script>` de props.
pub fn resolve_markers_html(text: &str, yaml: &[PropEntry]) -> Vec<(String, String)> {
    let markers = find_markers(text, true);
    if markers.is_empty() {
        return Vec::new();
    }
    let visible = crate::html_text::html_text(text);
    let section = 0..visible.len();
    let sections = std::slice::from_ref(&section);
    let values = resolve_values(&visible, &markers, &sections, yaml);
    let mut out: Vec<(String, String)> = Vec::new();
    for (marker, value) in markers.into_iter().zip(values) {
        if !out.iter().any(|(k, _)| *k == marker.key) {
            out.push((marker.key, value));
        }
    }
    out
}

/// Resuelve cada marcador contra su sección: inline primero (el primero si repite), YAML
/// después, `«faltante»` si nadie define la clave.
fn resolve_values(
    text: &str,
    markers: &[Marker],
    sections: &[Range<usize>],
    yaml: &[PropEntry],
) -> Vec<String> {
    let whole = 0..text.len();
    let section_of = |pos: usize| -> &Range<usize> {
        sections
            .iter()
            .find(|section| section.contains(&pos))
            .unwrap_or(sections.first().unwrap_or(&whole))
    };
    let mut out = Vec::with_capacity(markers.len());
    let mut cached: Option<(usize, std::collections::HashMap<String, String>)> = None;
    for marker in markers {
        let section_start = section_of(marker.range.start).start;
        let inline = match &cached {
            Some((start, map)) if *start == section_start => map.clone(),
            _ => {
                let map = inline_map(&text[section_of(marker.range.start).clone()]);
                cached = Some((section_start, map.clone()));
                map
            }
        };
        let value = inline
            .get(&marker.key)
            .cloned()
            .or_else(|| {
                props::find(yaml, &marker.key).and_then(|e| e.value.as_text().map(str::to_owned))
            })
            .unwrap_or_else(|| "«faltante»".to_owned());
        out.push(value);
    }
    out
}

/// Mapa clave→valor de los campos inline de un texto; con claves repetidas gana el primero.
fn inline_map(text: &str) -> std::collections::HashMap<String, String> {
    let mut map = std::collections::HashMap::new();
    for (key, value) in crate::tags::inline_fields(text) {
        map.entry(key).or_insert(value);
    }
    map
}
