//! Dónde están las fórmulas LaTeX de una nota `.md` (050), con las mismas reglas que la Lectura
//! (`render`): `$…$`, `$$…$$` y ` ```math `, nunca en código. Lo usa la vista previa de Código.

use std::ops::Range;

use pulldown_cmark::{Event, Options, Parser};

/// Una fórmula de una nota `.md`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FormulaEn {
    /// Rango de la fórmula con sus delimitadores (o de todo el codeblock ` ```math `).
    pub rango: Range<usize>,
    /// Rango del LaTeX, sin los delimitadores ni los renglones de la cerca (051: el editor de
    /// ecuaciones reescribe solo esto).
    pub contenido: Range<usize>,
    pub tex: String,
    /// `$$…$$` o ` ```math ` (en bloque); si no, en línea.
    pub bloque: bool,
}

/// La fórmula que contiene el byte `offset` de `texto`. `texto` puede ser un tramo de la nota que
/// empiece al comienzo de un renglón fuera de un bloque de código.
pub fn formula_en(texto: &str, offset: usize) -> Option<FormulaEn> {
    let offset = offset.min(texto.len());
    // 1. Cercas de código: dentro de una, solo cuenta si es ` ```math `.
    let mut inicio = 0;
    // (carácter, largo, inicio, es math)
    let mut cerca: Option<(u8, usize, usize, bool)> = None;
    let mut parrafo = 0;
    for linea in texto.split_inclusive('\n') {
        let fin = inicio + linea.len();
        let t = linea.trim_start();
        let marca = t.bytes().next().filter(|c| *c == b'`' || *c == b'~');
        let largo = marca.map_or(0, |c| t.bytes().take_while(|b| *b == c).count());
        match cerca {
            Some((c, n, desde, es_math)) => {
                if marca == Some(c) && largo >= n && t[largo..].trim().is_empty() {
                    cerca = None;
                    if (desde..fin).contains(&offset) {
                        let primera = texto[desde..inicio]
                            .find('\n')
                            .map_or(inicio, |n| desde + n + 1);
                        let cuerpo = &texto[primera..inicio];
                        return es_math.then(|| FormulaEn {
                            rango: desde..fin,
                            contenido: primera..inicio,
                            tex: cuerpo.to_owned(),
                            bloque: true,
                        });
                    }
                    parrafo = fin;
                }
            }
            None => {
                if offset < inicio {
                    break;
                }
                if marca.is_some() && largo >= 3 {
                    let lang = t[largo..].split_whitespace().next().unwrap_or("");
                    cerca = Some((t.as_bytes()[0], largo, inicio, lang == "math"));
                } else if linea.trim().is_empty() {
                    parrafo = fin;
                }
            }
        }
        inicio = fin;
    }
    if let Some((_, _, desde, es_math)) = cerca
        && offset >= desde
    {
        // Una cerca sin cerrar que contiene el cursor.
        let primera = texto[desde..]
            .find('\n')
            .map_or(texto.len(), |n| desde + n + 1);
        let cuerpo = &texto[primera..];
        return es_math.then(|| FormulaEn {
            rango: desde..texto.len(),
            contenido: primera..texto.len(),
            tex: cuerpo.to_owned(),
            bloque: true,
        });
    }
    // 2. El párrafo del cursor, analizado como Markdown.
    let fin = texto[offset..]
        .find("\n\n")
        .map_or(texto.len(), |n| offset + n);
    let parrafo = parrafo.min(offset);
    let tramo = &texto[parrafo..fin];
    let mut opts = Options::empty();
    opts.insert(Options::ENABLE_MATH);
    opts.insert(Options::ENABLE_TABLES);
    for (ev, r) in Parser::new_ext(tramo, opts).into_offset_iter() {
        let rango = parrafo + r.start..parrafo + r.end;
        if !(rango.start..=rango.end).contains(&offset) {
            continue;
        }
        match ev {
            Event::InlineMath(t) if !crate::render::cierre_con_digito(texto, &rango) => {
                return Some(FormulaEn {
                    contenido: rango.start + 1..rango.end - 1,
                    rango,
                    tex: t.to_string(),
                    bloque: false,
                });
            }
            Event::DisplayMath(t) => {
                return Some(FormulaEn {
                    contenido: rango.start + 2..rango.end - 2,
                    rango,
                    tex: t.to_string(),
                    bloque: true,
                });
            }
            _ => {}
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn en(texto: &str, marca: &str) -> Option<(String, bool)> {
        let off = texto.find(marca).expect("marca");
        formula_en(texto, off).map(|f| (f.tex, f.bloque))
    }

    #[test]
    fn en_linea_bloque_y_cerca() {
        let t = "Sea $x^2$ y nada.\n\n$$\n\\frac{a}{b}\n$$\n\n```math\n\\sqrt{2}\n```\n\n```rust\nlet $a$ = 1;\n```\n";
        assert_eq!(en(t, "x^2"), Some(("x^2".into(), false)));
        assert_eq!(en(t, "nada"), None);
        assert_eq!(en(t, "\\frac"), Some(("\n\\frac{a}{b}\n".into(), true)));
        assert_eq!(en(t, "\\sqrt"), Some(("\\sqrt{2}\n".into(), true)));
        assert_eq!(en(t, "$a$"), None, "en código no hay fórmulas");
    }

    #[test]
    fn dinero_y_codigo_en_linea() {
        assert_eq!(en("Cuesta $5 y $10.", "5"), None);
        assert_eq!(en("Pagué $5,$10 ya", "5"), None);
        assert_eq!(en("Usá `$x$` así", "x"), None);
        assert_eq!(en("a $y$", "$y"), Some(("y".into(), false)));
    }

    #[test]
    fn cerca_sin_cerrar() {
        let t = "```math\na+b";
        assert_eq!(en(t, "a+b"), Some(("a+b".into(), true)));
    }

    #[test]
    fn contenido_sin_delimitadores() {
        let t = "Sea $x^2$ y\n\n$$\n\\frac{a}{b}\n$$\n\n```math\n\\sqrt{2}\n```\n\n$$a$$ fin";
        let c = |m: &str| {
            let f = formula_en(t, t.find(m).unwrap()).unwrap();
            t[f.contenido].to_owned()
        };
        assert_eq!(c("x^2"), "x^2");
        assert_eq!(c("\\frac"), "\n\\frac{a}{b}\n");
        assert_eq!(c("\\sqrt"), "\\sqrt{2}\n");
        assert_eq!(c("a$$"), "a");
        let t = "```math\na+b";
        let f = formula_en(t, 9).unwrap();
        assert_eq!(&t[f.contenido], "a+b");
    }
}
