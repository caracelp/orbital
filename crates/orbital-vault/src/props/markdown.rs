//! Frontmatter de las notas `.md`: bloque YAML entre dos líneas `---` al inicio (OKF §4).

use std::ops::Range;

/// Largo en bytes del BOM UTF-8 inicial, si lo hay.
pub fn bom_len(text: &str) -> usize {
    if text.starts_with('\u{feff}') { 3 } else { 0 }
}

/// Si la línea (sin su final) es un delimitador `---`, admitiendo espacios al final.
fn is_fence(line: &str) -> bool {
    line.trim_end_matches(['\r', '\n']).trim_end() == "---"
}

/// Rango en bytes del contenido YAML del frontmatter (sin las líneas `---`).
///
/// Devuelve `None` si la nota no empieza con `---` o si el bloque nunca se cierra: en ese caso
/// el `---` es parte del cuerpo, no un frontmatter.
pub fn locate(text: &str) -> Option<Range<usize>> {
    let start = bom_len(text);
    let rest = &text[start..];
    let first_end = rest.find('\n')? + 1;
    if !is_fence(&rest[..first_end]) {
        return None;
    }
    let content_start = start + first_end;
    let mut pos = content_start;
    while pos < text.len() {
        let line_end = text[pos..].find('\n').map_or(text.len(), |i| pos + i + 1);
        if is_fence(&text[pos..line_end]) {
            return Some(content_start..pos);
        }
        pos = line_end;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn locates_blocks() {
        let t = "---\na: 1\n---\ncuerpo\n";
        assert_eq!(&t[locate(t).unwrap()], "a: 1\n");
        let t = "\u{feff}---\r\na: 1\r\n---\r\n";
        assert_eq!(&t[locate(t).unwrap()], "a: 1\r\n");
        let t = "---\n---\n";
        assert_eq!(locate(t), Some(4..4));
        assert_eq!(locate("# título\n---\na: 1\n---\n"), None);
        assert_eq!(locate("---\nsin cierre\n"), None);
        assert_eq!(locate("---"), None);
    }
}
