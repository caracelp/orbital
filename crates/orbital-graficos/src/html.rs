//! Los gráficos dentro de una nota `.html`: un `<pre><code class="language-grafico">` con el
//! fuente del bloque (lo mismo que sale de exportar un ` ```grafico ` de Markdown).

use std::ops::Range;

/// Un gráfico de una página: el rango del `<pre>…</pre>` entero y el fuente ya decodificado.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GraficoHtml {
    pub rango: Range<usize>,
    pub fuente: String,
}

/// Decodifica las entidades de un texto de HTML (`&lt;`, `&amp;`, `&#960;`, `&#x3c0;`).
pub fn decodificar(s: &str) -> String {
    if !s.contains('&') {
        return s.to_owned();
    }
    let mut out = String::with_capacity(s.len());
    let mut resto = s;
    while let Some(k) = resto.find('&') {
        out.push_str(&resto[..k]);
        resto = &resto[k..];
        let Some(fin) = resto[..resto.len().min(12)].find(';') else {
            out.push('&');
            resto = &resto[1..];
            continue;
        };
        let ent = &resto[1..fin];
        let c = match ent {
            "lt" => Some('<'),
            "gt" => Some('>'),
            "amp" => Some('&'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            "nbsp" => Some(' '),
            "pi" => Some('π'),
            "theta" => Some('θ'),
            _ => ent
                .strip_prefix("#x")
                .or_else(|| ent.strip_prefix("#X"))
                .and_then(|h| u32::from_str_radix(h, 16).ok())
                .or_else(|| ent.strip_prefix('#').and_then(|d| d.parse().ok()))
                .and_then(char::from_u32),
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

/// Los gráficos de un HTML, en orden.
pub fn graficos(html: &str) -> Vec<GraficoHtml> {
    bloques(html, "grafico")
}

/// Los `<pre><code class="language-{lang}">` de un HTML, en orden (los gráficos y, desde la 058,
/// los diagramas Mermaid). `lang` va en minúsculas.
pub fn bloques(html: &str, lang: &str) -> Vec<GraficoHtml> {
    let clases = [format!("language-{lang}"), format!("lang-{lang}")];
    let mut out = Vec::new();
    let bajo = html.to_ascii_lowercase();
    let mut desde = 0;
    while let Some(k) = bajo[desde..].find("<pre") {
        let ini = desde + k;
        desde = ini + 4;
        let Some(fin_pre) = bajo[ini..].find("</pre>").map(|f| ini + f + "</pre>".len()) else {
            break;
        };
        let pre = &bajo[ini..fin_pre];
        // `<pre><code class="language-grafico">` (con otras clases o atributos alrededor).
        let Some(c) = pre.find("<code") else { continue };
        let Some(cierre_tag) = pre[c..].find('>') else {
            continue;
        };
        let tag = &pre[c..c + cierre_tag];
        let es = clases.iter().any(|c| tiene_clase(tag, c));
        if !es {
            desde = fin_pre;
            continue;
        }
        let cuerpo_ini = ini + c + cierre_tag + 1;
        let Some(cuerpo_fin) = bajo[cuerpo_ini..fin_pre]
            .find("</code>")
            .map(|f| cuerpo_ini + f)
        else {
            continue;
        };
        let crudo = &html[cuerpo_ini..cuerpo_fin];
        // Sin etiquetas adentro (un `<br>` o `<span>` de un editor): se quitan.
        let mut texto = String::with_capacity(crudo.len());
        let mut en_tag = false;
        for ch in crudo.chars() {
            match ch {
                '<' => en_tag = true,
                '>' if en_tag => en_tag = false,
                c if !en_tag => texto.push(c),
                _ => {}
            }
        }
        out.push(GraficoHtml {
            rango: ini..fin_pre,
            fuente: decodificar(&texto),
        });
        desde = fin_pre;
    }
    out
}

/// `clase` está en `tag` como palabra entera (`language-grafico` no es `language-grafico3d`).
fn tiene_clase(tag: &str, clase: &str) -> bool {
    tag.match_indices(clase).any(|(i, _)| {
        let antes = tag[..i].chars().next_back();
        let despues = tag[i + clase.len()..].chars().next();
        !antes.is_some_and(|c| c.is_alphanumeric() || c == '-' || c == '_')
            && !despues.is_some_and(|c| c.is_alphanumeric() || c == '-' || c == '_')
    })
}

/// Cambia cada gráfico de `html` por lo que devuelve `f` con su fuente.
pub fn reemplazar(html: &str, f: impl FnMut(&str) -> String) -> String {
    reemplazar_bloques(html, "grafico", f)
}

/// Cambia cada `<pre><code class="language-{lang}">` de `html` por lo que devuelve `f` con su
/// fuente.
pub fn reemplazar_bloques(html: &str, lang: &str, mut f: impl FnMut(&str) -> String) -> String {
    let gs = bloques(html, lang);
    if gs.is_empty() {
        return html.to_owned();
    }
    let mut out = String::with_capacity(html.len() + gs.len() * 8192);
    let mut ultimo = 0;
    for g in gs {
        out.push_str(&html[ultimo..g.rango.start]);
        out.push_str(&f(&g.fuente));
        ultimo = g.rango.end;
    }
    out.push_str(&html[ultimo..]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encuentra_los_graficos() {
        let html = "<p>a</p><pre><code class=\"language-rust\">x</code></pre><pre id=\"q1\"><code class=\"language-grafico\">y = x^2\nx &lt; 1 &amp;&amp; 2&#960;</code></pre><p>b</p>";
        let g = graficos(html);
        assert_eq!(g.len(), 1);
        assert_eq!(g[0].fuente, "y = x^2\nx < 1 && 2π");
        assert!(html[g[0].rango.clone()].starts_with("<pre id"));
        let r = reemplazar(html, |f| format!("[{}]", f.len()));
        assert!(r.contains("</pre>[20]<p>b</p>"), "{r}");
        assert_eq!(reemplazar("<p>sin</p>", |_| String::new()), "<p>sin</p>");
        // Otros lenguajes (058) y la clase entera.
        let m = "<pre><code class=\"language-mermaid\">pie\n\"A\" : 1</code></pre><pre><code class=\"language-mermaidx\">no</code></pre>";
        let b = bloques(m, "mermaid");
        assert_eq!(b.len(), 1);
        assert_eq!(b[0].fuente, "pie\n\"A\" : 1");
        assert!(graficos(m).is_empty());
    }
}
