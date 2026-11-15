//! Exportación de notas (007): HTML autónomo del render de Lectura.
//!
//! Tema claro (papel blanco) para imprimir y compartir; sin recursos externos. Sin red: todo es
//! render local.

use pulldown_cmark::{Options, Parser, html::push_html};

/// CSS de la página exportada: papel blanco, tipografía limpia, ancho de lectura.
const ESTILOS: &str = "body{font-family:sans-serif;line-height:1.5;color:#1a1a1c;background:#ffffff;\
max-width:860px;margin:2em auto;padding:0 1.5em}\
h1,h2,h3,h4,h5,h6{line-height:1.25}code{background:#f2f2f4;border-radius:3px;padding:0.05em 0.3em;\
font-family:monospace}pre{background:#f2f2f4;border-radius:6px;padding:0.75em 1em;overflow-x:auto}\
pre code{background:none;padding:0}blockquote{border-left:4px solid #ff8cc6;margin:0.75em 0;\
padding:0.25em 1em;color:#55555c;background:#fff7fb}table{border-collapse:collapse}\
th,td{border:1px solid #d0d0d4;padding:0.35em 0.6em;text-align:left}\
img{max-width:100%}input[type=checkbox]{margin-right:0.4em}";

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
    let mut body = String::with_capacity(cuerpo.len() * 2);
    push_html(&mut body, Parser::new_ext(cuerpo, opts));
    pagina(&body, &titulo(texto, nombre_archivo))
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
<title>{titulo}</title>\n<style>{ESTILOS}</style>\n</head>\n<body>\n{body}\n</body>\n</html>\n"
    )
}

/// HTML de una nota `.html`: el contenido tal cual (FR-3).
pub fn html_passthrough(texto: &str) -> String {
    texto.to_owned()
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
