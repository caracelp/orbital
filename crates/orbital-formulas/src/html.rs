//! Fórmulas dentro de una nota `.html` (050): se buscan en el texto que se ve, nunca dentro de
//! `<code>`, `<pre>`, `<script>`, `<style>`, `<textarea>`, `<svg>` o `<math>`, ni en los
//! atributos. Una fórmula no cruza etiquetas (`$a <b>x</b>$` no es una fórmula).

use std::ops::Range;

use crate::delimitadores::{self, Hallada};

/// Elementos cuyo contenido no se mira.
const CRUDOS: [&str; 8] = [
    "code", "pre", "script", "style", "textarea", "svg", "math", "title",
];

/// Los tramos de texto entre etiquetas (rangos del HTML fuente, sin decodificar), fuera de los
/// elementos de [`CRUDOS`] y de los comentarios.
pub fn tramos(html: &str) -> Vec<Range<usize>> {
    let b = html.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    let mut desde = 0;
    let mut crudo: Option<&str> = None;
    let mut profundidad = 0usize;
    while i < b.len() {
        if b[i] != b'<' {
            i += 1;
            continue;
        }
        if crudo.is_none() && desde < i {
            out.push(desde..i);
        }
        // Comentario: hasta `-->`.
        if html[i..].starts_with("<!--") {
            i = html[i + 4..].find("-->").map_or(b.len(), |n| i + 4 + n + 3);
            desde = i;
            continue;
        }
        let fin = fin_de_etiqueta(b, i + 1);
        let hasta = if b[fin - 1] == b'>' { fin - 1 } else { fin };
        let etiqueta = &html[i + 1..hasta.max(i + 1)];
        let cierre = etiqueta.starts_with('/');
        let nombre: String = etiqueta
            .trim_start_matches('/')
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric())
            .collect::<String>()
            .to_ascii_lowercase();
        let autocierre = etiqueta.ends_with('/');
        if let Some(c) = crudo {
            if nombre == c {
                if cierre {
                    profundidad -= 1;
                    if profundidad == 0 {
                        crudo = None;
                    }
                } else if !autocierre {
                    profundidad += 1;
                }
            }
        } else if !cierre
            && !autocierre
            && let Some(c) = CRUDOS.iter().find(|c| **c == nombre)
        {
            crudo = Some(c);
            profundidad = 1;
        }
        i = fin;
        desde = i;
    }
    if crudo.is_none() && desde < b.len() {
        out.push(desde..b.len());
    }
    out
}

/// El byte después del `>` que cierra la etiqueta que empieza en `desde` (respeta comillas).
fn fin_de_etiqueta(b: &[u8], desde: usize) -> usize {
    let mut i = desde;
    let mut comilla = None;
    while i < b.len() {
        match (b[i], comilla) {
            (q @ (b'"' | b'\''), None) => comilla = Some(q),
            (q, Some(c)) if q == c => comilla = None,
            (b'>', None) => return i + 1,
            _ => {}
        }
        i += 1;
    }
    b.len()
}

/// Decodifica las entidades de un tramo de texto (las nombradas comunes y las numéricas).
pub fn decodificar(s: &str) -> String {
    if !s.contains('&') {
        return s.to_owned();
    }
    let mut out = String::with_capacity(s.len());
    let mut resto = s;
    while let Some(n) = resto.find('&') {
        out.push_str(&resto[..n]);
        resto = &resto[n..];
        let Some(fin) = resto[1..].find(';').map(|f| f + 1).filter(|f| *f <= 12) else {
            out.push('&');
            resto = &resto[1..];
            continue;
        };
        let nombre = &resto[1..fin];
        let c = match nombre {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            "nbsp" => Some('\u{a0}'),
            _ => nombre.strip_prefix('#').and_then(|num| {
                let v = match num.strip_prefix(['x', 'X']) {
                    Some(hex) => u32::from_str_radix(hex, 16).ok(),
                    None => num.parse().ok(),
                };
                v.and_then(char::from_u32)
            }),
        };
        match c {
            Some(c) => {
                out.push(c);
                resto = &resto[fin + 1..];
            }
            None => {
                out.push('&');
                resto = &resto[1..];
            }
        }
    }
    out.push_str(resto);
    out
}

/// Las fórmulas de una nota `.html`: (texto tal como aparece en la página, con delimitadores y
/// entidades decodificadas; LaTeX; modo), sin repetir.
pub fn formulas(html: &str) -> Vec<(String, String, crate::Modo)> {
    let mut out: Vec<(String, String, crate::Modo)> = Vec::new();
    for t in tramos(html) {
        let tramo = &html[t];
        for Hallada {
            rango,
            contenido,
            modo,
        } in delimitadores::buscar(tramo)
        {
            let clave = decodificar(&tramo[rango]);
            if !out.iter().any(|(k, ..)| *k == clave) {
                out.push((clave, decodificar(&tramo[contenido]), modo));
            }
        }
    }
    out
}

/// La fórmula que contiene el byte `offset` del HTML fuente (vista previa de Código): (rango con
/// delimitadores, LaTeX decodificado, modo).
pub fn formula_en(html: &str, offset: usize) -> Option<(Range<usize>, String, crate::Modo)> {
    let t = tramos(html)
        .into_iter()
        .find(|t| t.start <= offset && offset <= t.end)?;
    delimitadores::buscar(&html[t.clone()])
        .into_iter()
        .find(|h| t.start + h.rango.start <= offset && offset <= t.start + h.rango.end)
        .map(|h| {
            (
                t.start + h.rango.start..t.start + h.rango.end,
                decodificar(&html[t.start + h.contenido.start..t.start + h.contenido.end]),
                h.modo,
            )
        })
}

/// El HTML con cada fórmula cambiada por lo que devuelve `f(LaTeX, modo)` (exportar).
pub fn reemplazar(html: &str, mut f: impl FnMut(&str, crate::Modo) -> String) -> String {
    let mut out = String::with_capacity(html.len());
    let mut copiado = 0;
    for t in tramos(html) {
        for h in delimitadores::buscar(&html[t.clone()]) {
            let rango = t.start + h.rango.start..t.start + h.rango.end;
            out.push_str(&html[copiado..rango.start]);
            out.push_str(&f(
                &decodificar(&html[t.start + h.contenido.start..t.start + h.contenido.end]),
                h.modo,
            ));
            copiado = rango.end;
        }
    }
    out.push_str(&html[copiado..]);
    out
}

/// Para un valor de atributo entre comillas dobles.
pub fn escapar_atributo(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '"' => out.push_str("&quot;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            c => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Modo;

    #[test]
    fn solo_el_texto_visible() {
        let html = r#"<html><head><script type="application/yaml">a: $x$</script>
<style>p::after{content:"$y$"}</style></head><body>
<p title="$z$">Sea $a &lt; b$ y <code>$no$</code>, <pre>$$no$$</pre> $$\frac{1}{2}$$</p>
<!-- $comentario$ --><p>Cuesta $5 y $10</p></body></html>"#;
        let f = formulas(html);
        assert_eq!(
            f,
            vec![
                ("$a < b$".into(), "a < b".into(), Modo::EnLinea),
                (
                    r"$$\frac{1}{2}$$".into(),
                    r"\frac{1}{2}".into(),
                    Modo::Bloque
                ),
            ]
        );
    }

    #[test]
    fn reemplaza_solo_las_formulas() {
        let html = "<title>$t$</title><p a=\"$z$\">Sea $a &lt; b$, <code>$c$</code> y $5 o $6</p>";
        let out = reemplazar(html, |tex, modo| format!("[{tex}|{modo:?}]"));
        assert_eq!(
            out,
            "<title>$t$</title><p a=\"$z$\">Sea [a < b|EnLinea], <code>$c$</code> y $5 o $6</p>"
        );
        assert_eq!(reemplazar("sin nada", |_, _| String::new()), "sin nada");
    }

    #[test]
    fn entidades() {
        assert_eq!(
            decodificar("a &amp; b &#60; &#x3E; &nbsp;&zz; &"),
            "a & b < > \u{a0}&zz; &"
        );
    }

    #[test]
    fn crudos_anidados_y_sin_cerrar() {
        let f = formulas("<pre><pre>$a$</pre>$b$</pre>$c$");
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].1, "c");
        assert!(formulas("<code>$a$").is_empty());
        assert_eq!(formulas("<br/>$d$").len(), 1);
    }
}
