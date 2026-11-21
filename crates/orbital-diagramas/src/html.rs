//! Escapado para HTML (el nombre accesible del SVG, el fuente de un diagrama con error).

/// Agrega `s` a `out` con `&`, `<`, `>` y `"` escapados.
pub fn escapar_en(out: &mut String, s: &str) {
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            c => out.push(c),
        }
    }
}

/// `s` con `&`, `<`, `>` y `"` escapados.
pub fn escapar(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 16);
    escapar_en(&mut out, s);
    out
}
