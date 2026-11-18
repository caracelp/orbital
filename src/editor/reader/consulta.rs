//! El contenido de un codeblock `consulta` (048): lista, tabla, coincidencias o conteo, armado con
//! bloques comunes (texto con etiquetas y enlaces, sin widgets).

use super::*;
use orbital_vault::consulta::{self, Resultado, Vista};

fn bloque(kind: BlockKind) -> Block {
    Block {
        line: 0,
        id: None,
        source: 0..0,
        kind,
    }
}

fn aviso(texto: String) -> BlockKind {
    BlockKind::Paragraph(vec![Inline::Emph(vec![Inline::Text(texto)])])
}

fn enlace(path: &orbital_vault::RelPath) -> Inline {
    Inline::Link {
        url: format!("/{}", super::generado::ruta_sin_wiki(path)),
        children: vec![Inline::Text(path.file_name().to_owned())],
    }
}

fn notas(n: usize) -> String {
    if n == 1 {
        "1 nota".to_owned()
    } else {
        format!("{n} notas")
    }
}

/// El bloque que reemplaza al codeblock. Sin índice (todavía se está armando) queda un aviso: la
/// vista se vuelve a generar cuando el índice llega.
pub(super) fn consulta_block(
    raw: &str,
    index: Option<&orbital_vault::SearchIndex>,
    raiz: Option<&Path>,
) -> BlockKind {
    let c = match consulta::parse(raw) {
        Ok(c) => c,
        Err(e) => return aviso(format!("La consulta tiene un error en la {e}")),
    };
    let Some(index) = index else {
        return aviso("Consulta: esperando el índice de la bóveda…".into());
    };
    let r = consulta::ejecutar(index, raiz, &c);
    if c.vista == Vista::Conteo {
        return BlockKind::Paragraph(vec![Inline::Text(notas(r.total))]);
    }
    if r.total == 0 {
        return aviso("Consulta: ninguna nota coincide".into());
    }
    match c.vista {
        Vista::Tabla => tabla(&c.columnas, r),
        Vista::Coincidencias => coincidencias(r),
        _ => BlockKind::List {
            start: None,
            items: r
                .filas
                .iter()
                .map(|f| ListItem {
                    task: None,
                    blocks: vec![bloque(BlockKind::Paragraph(vec![enlace(&f.path)]))],
                })
                .collect(),
        },
    }
}

fn tabla(columnas: &[String], r: Resultado) -> BlockKind {
    let mut header = vec![vec![Inline::Text("Nota".into())]];
    header.extend(columnas.iter().map(|c| vec![Inline::Text(c.clone())]));
    let rows = r
        .filas
        .iter()
        .map(|f| {
            let mut fila = vec![vec![enlace(&f.path)]];
            fila.extend(f.valores.iter().map(|v| {
                // Una celda vacía se lee igual que una con valor: el guion lo dice.
                let t = v.as_ref().map(consulta::mostrar).unwrap_or_default();
                vec![Inline::Text(if t.is_empty() { "—".into() } else { t })]
            }));
            fila
        })
        .collect();
    BlockKind::Table {
        align: vec![Align::None; columnas.len() + 1],
        header,
        rows,
    }
}

/// El fragmento con los tramos resaltados en negrita, como en el panel de búsqueda. Los rangos
/// son de caracteres.
fn resaltado(m: &orbital_vault::Match) -> Vec<Inline> {
    let chars: Vec<char> = m.snippet.chars().collect();
    let mut out = vec![Inline::Text(format!("{}: ", m.line))];
    let mut desde = 0;
    for r in &m.highlights {
        let (a, z) = (r.start.min(chars.len()), r.end.min(chars.len()));
        if a < desde || z <= a {
            continue;
        }
        if a > desde {
            out.push(Inline::Text(chars[desde..a].iter().collect()));
        }
        out.push(Inline::Strong(vec![Inline::Text(
            chars[a..z].iter().collect(),
        )]));
        desde = z;
    }
    if desde < chars.len() {
        out.push(Inline::Text(chars[desde..].iter().collect()));
    }
    out
}

fn coincidencias(r: Resultado) -> BlockKind {
    let items = r
        .filas
        .iter()
        .map(|f| {
            let mut blocks = vec![bloque(BlockKind::Paragraph(vec![enlace(&f.path)]))];
            let mut lineas: Vec<ListItem> = f
                .coincidencias
                .iter()
                .map(|m| ListItem {
                    task: None,
                    blocks: vec![bloque(BlockKind::Paragraph(resaltado(m)))],
                })
                .collect();
            if f.mas > 0 {
                lineas.push(ListItem {
                    task: None,
                    blocks: vec![bloque(aviso(format!(
                        "y {} {} más",
                        f.mas,
                        if f.mas == 1 { "línea" } else { "líneas" }
                    )))],
                });
            }
            if !lineas.is_empty() {
                blocks.push(bloque(BlockKind::List {
                    start: None,
                    items: lineas,
                }));
            }
            ListItem { task: None, blocks }
        })
        .collect();
    BlockKind::List { start: None, items }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[allow(clippy::single_range_in_vec_init)]
    fn resalta_por_caracteres() {
        let m = orbital_vault::Match {
            line: 3,
            snippet: "Año del motor".into(),
            highlights: vec![8..13],
        };
        let p = resaltado(&m);
        assert_eq!(plain_text(&p), "3: Año del motor");
        assert!(matches!(&p[2], Inline::Strong(c) if plain_text(c) == "motor"));
    }

    #[test]
    fn error_y_conteo_sin_indice() {
        let BlockKind::Paragraph(p) = consulta_block("tag:#a (", None, None) else {
            panic!()
        };
        assert_eq!(
            plain_text(&p),
            "La consulta tiene un error en la línea 1, columna 9: Paréntesis sin cerrar"
        );
        let index = orbital_vault::SearchIndex::new();
        let BlockKind::Paragraph(p) = consulta_block("x\nvista: conteo", Some(&index), None) else {
            panic!()
        };
        assert_eq!(plain_text(&p), "0 notas");
        let BlockKind::Paragraph(p) = consulta_block("x", Some(&index), None) else {
            panic!()
        };
        assert_eq!(plain_text(&p), "Consulta: ninguna nota coincide");
    }
}
