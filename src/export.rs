//! Exportación de notas (007): HTML autónomo del render de Lectura.
//!
//! Tema claro (papel blanco) para imprimir y compartir; sin recursos externos. Sin red: todo es
//! render local.

use pulldown_cmark::{CodeBlockKind, CowStr, Event, Options, Parser, Tag, TagEnd, html::push_html};

/// CSS de la página exportada: papel blanco, tipografía limpia, ancho de lectura.
const ESTILOS: &str = "body{font-family:sans-serif;line-height:1.5;color:#1a1a1c;background:#ffffff;\
max-width:860px;margin:2em auto;padding:0 1.5em}\
h1,h2,h3,h4,h5,h6{line-height:1.25}code{background:#f2f2f4;border-radius:3px;padding:0.05em 0.3em;\
font-family:monospace}pre{background:#f2f2f4;border-radius:6px;padding:0.75em 1em;overflow-x:auto}\
pre code{background:none;padding:0}blockquote{border-left:4px solid #ff8cc6;margin:0.75em 0;\
padding:0.25em 1em;color:#55555c;background:#fff7fb}table{border-collapse:collapse}\
th,td{border:1px solid #d0d0d4;padding:0.35em 0.6em;text-align:left}\
img{max-width:100%}input[type=checkbox]{margin-right:0.4em}\
.formula-bloque{display:block;text-align:center;margin:0.6em 0;overflow-x:auto}\
.formula-error{text-decoration:underline wavy #c2185b}\
figure.grafico{margin:1em 0}figure.grafico svg{max-width:100%;height:auto}\
.grafico-aviso{color:#c2185b}";

/// Título de la página: el título de la nota (frontmatter) o el nombre del archivo.
fn titulo(texto: &str, defecto: &str) -> String {
    orbital_vault::props::markdown::locate(texto)
        .and_then(|r| orbital_vault::props::yaml::parse_block(&texto[r]).ok())
        .and_then(|entries| {
            orbital_vault::props::find(&entries, "titulo")
                .or_else(|| orbital_vault::props::find(&entries, "title"))
                .and_then(|e| e.value.as_text().map(str::to_owned))
        })
        .unwrap_or_else(|| defecto.to_owned())
}

/// HTML autónomo de una nota `.md`: cuerpo renderizado sin frontmatter (FR-3), con las mismas
/// extensiones que Lectura (tablas, tachado, casillas).
pub fn html_md(texto: &str, nombre_archivo: &str) -> String {
    // Los ids de bloque (`^id`, 026) no se exportan.
    let cuerpo = orbital_vault::bloques::sin_ids(cuerpo_sin_frontmatter(texto));
    let cuerpo = cuerpo.as_ref();
    let mut opts = Options::empty();
    opts.insert(Options::ENABLE_TABLES);
    opts.insert(Options::ENABLE_STRIKETHROUGH);
    opts.insert(Options::ENABLE_TASKLISTS);
    opts.insert(Options::ENABLE_MATH);
    let mut body = String::with_capacity(cuerpo.len() * 2);
    push_html(
        &mut body,
        con_formulas(cuerpo, Parser::new_ext(cuerpo, opts)),
    );
    pagina(&body, &titulo(texto, nombre_archivo))
}

/// Las fórmulas (050) como SVG del mismo motor que la Lectura: `$…$`, `$$…$$` y ` ```math `. Un
/// `$` de cierre seguido de un dígito no cierra (dinero), igual que en Lectura. Los gráficos
/// (052, ` ```grafico `), como SVG estático para papel. Los bloques de código con lenguaje
/// conocido, resaltados con los colores de Código pasados a papel (055).
fn con_formulas<'a>(cuerpo: &'a str, parser: Parser<'a>) -> impl Iterator<Item = Event<'a>> + 'a {
    let mut en_bloque: Option<(String, Cercado)> = None;
    parser.into_offset_iter().filter_map(move |(ev, rango)| {
        if let Some((texto, _)) = &mut en_bloque {
            return match ev {
                Event::Text(t) => {
                    texto.push_str(&t);
                    None
                }
                Event::End(TagEnd::CodeBlock) => {
                    let (texto, cercado) = en_bloque.take()?;
                    Some(Event::Html(
                        match cercado {
                            Cercado::Grafico => grafico_html(&texto),
                            Cercado::Formula => formula_html(&texto, true, true),
                            Cercado::Codigo(lang) => codigo_html(&lang, &texto),
                        }
                        .into(),
                    ))
                }
                _ => None,
            };
        }
        match ev {
            Event::Start(Tag::CodeBlock(CodeBlockKind::Fenced(ref lang)))
                if matches!(lang.split_whitespace().next(), Some("math" | "grafico")) =>
            {
                en_bloque = Some((
                    String::new(),
                    if lang.split_whitespace().next() == Some("grafico") {
                        Cercado::Grafico
                    } else {
                        Cercado::Formula
                    },
                ));
                None
            }
            Event::Start(Tag::CodeBlock(CodeBlockKind::Fenced(ref info)))
                if crate::resaltado::puede(info.split_whitespace().next().unwrap_or("")) =>
            {
                let lang = info.split_whitespace().next().unwrap_or("").to_owned();
                en_bloque = Some((String::new(), Cercado::Codigo(lang)));
                None
            }
            Event::InlineMath(t) => {
                if cuerpo
                    .as_bytes()
                    .get(rango.end)
                    .is_some_and(u8::is_ascii_digit)
                {
                    Some(Event::Text(CowStr::from(cuerpo[rango].to_owned())))
                } else {
                    Some(Event::InlineHtml(formula_html(&t, false, false).into()))
                }
            }
            Event::DisplayMath(t) => Some(Event::InlineHtml(formula_html(&t, true, false).into())),
            ev => Some(ev),
        }
    })
}

/// Una fórmula como SVG accesible; con error, su fuente subrayada y el motivo como título.
fn formula_html(tex: &str, bloque: bool, sola: bool) -> String {
    let modo = if bloque {
        orbital_formulas::Modo::Bloque
    } else {
        orbital_formulas::Modo::EnLinea
    };
    let (etiqueta, clase) = if sola {
        ("div", "formula formula-bloque")
    } else if bloque {
        ("span", "formula formula-bloque")
    } else {
        ("span", "formula")
    };
    match orbital_formulas::svg(tex, modo) {
        Ok(svg) => format!("<{etiqueta} class=\"{clase}\">{}</{etiqueta}>", svg.marcado),
        Err(e) => {
            let d = if bloque { "$$" } else { "$" };
            format!(
                "<code class=\"formula-error\" title=\"{}\">{}</code>",
                orbital_formulas::html::escapar_atributo(&e.mensaje),
                orbital_formulas::html::escapar_atributo(&format!("{d}{tex}{d}"))
            )
        }
    }
}

/// Qué se junta de un bloque cercado para dibujarlo aparte.
enum Cercado {
    Formula,
    Grafico,
    /// Código para resaltar (055), con su lenguaje.
    Codigo(String),
}

/// Un bloque de código resaltado (055); con un lenguaje que GtkSourceView no conoce, como lo
/// escribe `pulldown-cmark`.
fn codigo_html(lang: &str, texto: &str) -> String {
    let clase = orbital_formulas::html::escapar_atributo(lang);
    // Sin tramos, el texto escapado igual que `pulldown-cmark` (`&`, `<` y `>`).
    let cuerpo = crate::resaltado::html(lang, texto)
        .unwrap_or_else(|| crate::resaltado::html_de(texto, &[]));
    format!("<pre><code class=\"language-{clase}\">{cuerpo}</code></pre>\n")
}

/// Ancho de los gráficos exportados, en píxeles (el SVG se achica si la página es más angosta).
const ANCHO_GRAFICO: u32 = 760;

/// Un gráfico (052) como SVG accesible; con error, su fuente y el motivo.
fn grafico_html(fuente: &str) -> String {
    let fuente = fuente.strip_suffix('\n').unwrap_or(fuente);
    match crate::graficos::svg(fuente, ANCHO_GRAFICO) {
        Ok(svg) => format!("<figure class=\"grafico\">{svg}</figure>\n"),
        Err(e) => {
            let e = orbital_formulas::html::escapar_atributo(&e.to_string());
            format!(
                "<pre class=\"grafico-error\" title=\"{e}\"><code>{}</code></pre>\n<p class=\"grafico-aviso\">⚠ {e}</p>\n",
                orbital_formulas::html::escapar_atributo(fuente)
            )
        }
    }
}

/// Formato de exportación.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Formato {
    Html,
    Pdf,
}

/// El cuerpo de la nota: después del frontmatter si lo hay (igual que el render).
fn cuerpo_sin_frontmatter(texto: &str) -> &str {
    let base = match orbital_vault::props::markdown::locate(texto) {
        Some(region) => {
            let rest = &texto[region.end..];
            region.end + rest.find('\n').map_or(rest.len(), |i| i + 1)
        }
        None => orbital_vault::props::markdown::bom_len(texto),
    };
    &texto[base..]
}

/// Página completa con los estilos embebidos (FR-2).
pub fn pagina(body: &str, titulo: &str) -> String {
    format!(
        "<!doctype html>\n<html lang=\"es\">\n<head>\n<meta charset=\"utf-8\">\n\
<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n\
<title>{titulo}</title>\n<style>{ESTILOS}{sintaxis}</style>\n</head>\n<body>\n{body}\n</body>\n</html>\n",
        // Los colores del código (055), solo si hay código resaltado.
        sintaxis = if body.contains("<span class=\"sx-") {
            crate::resaltado::CSS_PAPEL
        } else {
            ""
        }
    )
}

/// HTML de una nota `.html`: el contenido tal cual (FR-3), con las fórmulas (050) dibujadas como
/// SVG en el texto visible. Sin fórmulas, una copia exacta.
pub fn html_passthrough(texto: &str) -> String {
    let con_formulas = orbital_formulas::html::reemplazar(texto, |tex, modo| {
        formula_html(tex, modo == orbital_formulas::Modo::Bloque, false)
    });
    // Los gráficos (052): `<pre><code class="language-grafico">`.
    orbital_graficos::html::reemplazar(&con_formulas, grafico_html)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exporta_cuerpo_sin_frontmatter_con_tabla() {
        let nota = "---\ntitulo: Mi nota\ntype: nota\n---\n\n# Encabezado\n\n| a | b |\n|---|---|\n| 1 | 2 |\n\n- [ ] tarea\n";
        let out = html_md(nota, "mi-nota.md");
        assert!(out.contains("<h1>Encabezado</h1>"), "{out}");
        assert!(out.contains("<table>"), "{out}");
        assert!(out.contains("checkbox"), "{out}");
        assert!(out.contains("<title>Mi nota</title>"), "{out}");
        // El frontmatter no aparece (FR-3).
        assert!(!out.contains("type: nota"), "{out}");
        // Autónomo: estilos embebidos, sin recursos externos (FR-2).
        assert!(out.contains("<style>"), "{out}");
        assert!(!out.contains("http://"), "{out}");
        assert!(!out.contains("https://"), "{out}");
    }

    #[test]
    fn exporta_sin_ids_de_bloque() {
        let nota = "Un párrafo ^abc123\n\n| a |\n|---|\n| 1 |\n^tab001\n\n- ítem ^def456\n";
        let out = html_md(nota, "n.md");
        assert!(!out.contains('^'), "{out}");
        assert!(out.contains("<p>Un párrafo</p>"), "{out}");
        assert!(!out.contains("tab001"), "{out}");
    }

    #[test]
    fn exporta_formulas_como_svg() {
        let nota = "Sea $x^2$ y cuesta $5 o $10.\n\n$$\n\\frac{a}{b}\n$$\n\n```math\n\\sqrt{2}\n```\n\nMal: $x^^2$ y `$code$`\n";
        let out = html_md(nota, "n.md");
        assert_eq!(out.matches("<svg").count(), 3, "{out}");
        assert!(out.contains("aria-label=\"Fórmula: x^2\""), "{out}");
        assert!(out.contains("cuesta $5 o $10"), "{out}");
        assert!(
            out.contains("<div class=\"formula formula-bloque\">"),
            "{out}"
        );
        assert!(out.contains("class=\"formula-error\""), "{out}");
        assert!(out.contains("<code>$code$</code>"), "{out}");
        assert!(!out.contains("http://"), "{out}");
        let html = "<html><body><p>Sea $x$ y <code>$y$</code></p></body></html>";
        let out = html_passthrough(html);
        assert_eq!(out.matches("<svg").count(), 1, "{out}");
        assert!(out.contains("<code>$y$</code>"), "{out}");
    }

    #[test]
    fn exporta_graficos_como_svg() {
        let nota = "# G\n\n```grafico\ntítulo: Ondas\ny = sin(x)\nr = 2\n```\n\n```grafico\ny = (x\n```\n\n```rust\nlet y = 1;\n```\n";
        let out = html_md(nota, "n.md");
        assert_eq!(
            out.matches("<figure class=\"grafico\"><svg").count(),
            1,
            "{out}"
        );
        assert!(
            out.contains("aria-label=\"Gráfico: Ondas. y = sin(x); r = 2."),
            "{out}"
        );
        assert!(out.contains("class=\"grafico-error\""), "{out}");
        assert!(out.contains("Error en el gráfico (renglón 1)"), "{out}");
        assert!(out.contains("<code class=\"language-rust\">"), "{out}");
        assert!(!out.contains("http"), "{out}");
        let html = "<html><body><pre><code class=\"language-grafico\">y = x^2</code></pre><p>$x$</p></body></html>";
        let out = html_passthrough(html);
        assert_eq!(out.matches("<svg").count(), 2, "{out}");
        assert!(!out.contains("language-grafico"), "{out}");
    }

    #[test]
    fn nota_html_pasa_tal_cual() {
        let nota = "<!doctype html><html><body><p>hola</p></body></html>";
        assert_eq!(html_passthrough(nota), nota);
    }

    #[test]
    fn sin_titulo_usa_el_nombre_de_archivo() {
        let out = html_md("# Solo encabezado\n", "apunte.md");
        assert!(out.contains("<title>apunte.md</title>"), "{out}");
    }
}
