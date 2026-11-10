//! Escáner mínimo del `<head>` de las notas `.html` (research R3).
//!
//! Solo recorre hasta `</head>` o `<body>`, saltea comentarios y el contenido de cualquier
//! `<script>`, y busca la primera etiqueta `<script type="application/yaml">`. Trabaja con
//! desplazamientos en bytes para poder editar el archivo sin tocar nada más.

use std::ops::Range;

/// Resultado del escaneo.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct HeadScan {
    /// Contenido de la primera etiqueta `<script type="application/yaml">`.
    pub block: Option<Range<usize>>,
    /// Cuántas etiquetas de propiedades más hay (solo cuenta la primera).
    pub extra_blocks: usize,
    /// Dónde insertar un bloque nuevo: inicio de la línea de `</head>` (o, sin `</head>`, de
    /// `<body>`), siempre que exista `<head>`.
    pub insert_at: Option<usize>,
}

fn find_ci(hay: &[u8], from: usize, needle: &[u8]) -> Option<usize> {
    if needle.len() > hay.len() {
        return None;
    }
    (from..=hay.len() - needle.len())
        .find(|&i| hay[i..i + needle.len()].eq_ignore_ascii_case(needle))
}

fn starts_ci(hay: &[u8], at: usize, needle: &[u8]) -> bool {
    hay.len() >= at + needle.len() && hay[at..at + needle.len()].eq_ignore_ascii_case(needle)
}

/// Fin de una etiqueta (posición después de `>`), respetando comillas en los atributos.
fn tag_end(b: &[u8], from: usize) -> Option<usize> {
    let mut quote = 0u8;
    for (i, &c) in b.iter().enumerate().skip(from) {
        match (quote, c) {
            (0, b'"' | b'\'') => quote = c,
            (0, b'>') => return Some(i + 1),
            (q, c) if q == c => quote = 0,
            _ => {}
        }
    }
    None
}

/// Valor del atributo `type` de una etiqueta.
fn type_attr(tag: &[u8]) -> Option<&[u8]> {
    let mut i = 0;
    while let Some(p) = find_ci(tag, i, b"type") {
        let before_ok = p > 0 && tag[p - 1].is_ascii_whitespace();
        let mut j = p + 4;
        while j < tag.len() && tag[j].is_ascii_whitespace() {
            j += 1;
        }
        if before_ok && j < tag.len() && tag[j] == b'=' {
            j += 1;
            while j < tag.len() && tag[j].is_ascii_whitespace() {
                j += 1;
            }
            let (start, end) = match tag.get(j) {
                Some(&q @ (b'"' | b'\'')) => {
                    let end = tag[j + 1..].iter().position(|&c| c == q)? + j + 1;
                    (j + 1, end)
                }
                Some(_) => {
                    let end = tag[j..]
                        .iter()
                        .position(|&c| c.is_ascii_whitespace() || c == b'>')
                        .map_or(tag.len(), |e| j + e);
                    (j, end)
                }
                None => return None,
            };
            return Some(&tag[start..end]);
        }
        i = p + 4;
    }
    None
}

fn line_start(b: &[u8], at: usize) -> usize {
    let ls = b[..at]
        .iter()
        .rposition(|&c| c == b'\n')
        .map_or(0, |p| p + 1);
    // Solo si lo anterior en la línea es espacio; si no, se inserta justo antes.
    if b[ls..at].iter().all(u8::is_ascii_whitespace) {
        ls
    } else {
        at
    }
}

/// Escanea el `<head>` de un documento HTML.
pub fn scan(text: &str) -> HeadScan {
    let b = text.as_bytes();
    let mut out = HeadScan::default();
    let mut seen_head = false;
    let mut i = 0;
    while let Some(lt) = b[i..].iter().position(|&c| c == b'<').map(|p| p + i) {
        if starts_ci(b, lt, b"<!--") {
            match find_ci(b, lt + 4, b"-->") {
                Some(end) => i = end + 3,
                None => break,
            }
            continue;
        }
        if starts_ci(b, lt, b"</head") {
            if seen_head {
                out.insert_at = Some(line_start(b, lt));
            }
            break;
        }
        if starts_ci(b, lt, b"<body") {
            if seen_head {
                out.insert_at = Some(line_start(b, lt));
            }
            break;
        }
        if starts_ci(b, lt, b"<head") && b.get(lt + 5).is_some_and(|c| !c.is_ascii_alphanumeric()) {
            seen_head = true;
        }
        if starts_ci(b, lt, b"<script")
            && b.get(lt + 7)
                .is_some_and(|c| c.is_ascii_whitespace() || *c == b'>')
        {
            let Some(open_end) = tag_end(b, lt) else {
                break;
            };
            let is_props = type_attr(&b[lt..open_end])
                .is_some_and(|t| t.trim_ascii().eq_ignore_ascii_case(b"application/yaml"));
            let Some(close) = find_ci(b, open_end, b"</script") else {
                break;
            };
            if is_props {
                if out.block.is_none() {
                    out.block = Some(open_end..close);
                } else {
                    out.extra_blocks += 1;
                }
            }
            i = tag_end(b, close).unwrap_or(b.len());
            continue;
        }
        i = lt + 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_block_and_insertion_point() {
        let t =
            "<html><head>\n<script type=\"application/yaml\">\na: 1\n</script>\n  </head><body>";
        let s = scan(t);
        assert_eq!(&t[s.block.unwrap()], "\na: 1\n");
        assert_eq!(&t[s.insert_at.unwrap()..], "  </head><body>");
    }

    #[test]
    fn attribute_forms() {
        for t in [
            "<head><script type=application/yaml>a</script></head>",
            "<head><script TYPE='Application/YAML'>a</script></head>",
            "<head><script data-x=\"type=x\" type = \"application/yaml\">a</script></head>",
        ] {
            assert!(scan(t).block.is_some(), "{t}");
        }
        assert!(
            scan("<head><script type=\"module\">a</script></head>")
                .block
                .is_none()
        );
        assert!(
            scan("<head><scripts type=\"application/yaml\">a</script></head>")
                .block
                .is_none()
        );
    }

    #[test]
    fn no_head_means_no_insertion() {
        assert_eq!(scan("<p>hola</p>").insert_at, None);
        let t = "<html><head><title>x</title>\n<body></body>";
        assert_eq!(&t[scan(t).insert_at.unwrap()..], "<body></body>");
    }
}
