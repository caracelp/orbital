//! Renderizado nativo de consultas Dataview sin JS (`dataview` / `dv` y `` `= expr` ``, 065).
//!
//! Convierte [`ResultadoDv`] y [`Valor`] en bloques estándar de Lectura (`Table`, `List`, `Group`,
//! `Heading`, `Paragraph`) —sin widgets anclados— y también ofrece salida HTML estática para la
//! exportación.

use super::*;
use orbital_vault::dataview::{self, MesCalendario, ResultadoDv, TareaResultado, Valor};

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

fn enlace_ruta(path: &orbital_vault::RelPath, etiqueta: &str) -> Inline {
    Inline::Link {
        url: format!("/{}", super::generado::ruta_sin_wiki(path)),
        children: vec![Inline::Text(etiqueta.to_owned())],
    }
}

fn valor_a_inlines(v: &Valor) -> Vec<Inline> {
    match v {
        Valor::Null => vec![Inline::Text("—".into())],
        Valor::Enlace { ruta, etiqueta } => vec![enlace_ruta(ruta, etiqueta)],
        Valor::Lista(items) => {
            if items.is_empty() {
                return vec![Inline::Text("—".into())];
            }
            let mut out = Vec::new();
            for (idx, it) in items.iter().enumerate() {
                if idx > 0 {
                    out.push(Inline::Text(", ".into()));
                }
                out.extend(valor_a_inlines(it));
            }
            out
        }
        other => {
            let t = other.a_texto();
            vec![Inline::Text(if t.is_empty() { "—".into() } else { t })]
        }
    }
}

/// Bloque de Lectura que reemplaza un codeblock `dataview` / `dv`.
pub(super) fn dataview_block(
    raw: &str,
    index: Option<&orbital_vault::SearchIndex>,
    path: Option<&orbital_vault::RelPath>,
) -> BlockKind {
    let consulta = match dataview::parse_consulta(raw) {
        Ok(c) => c,
        Err(e) => return aviso(format!("Dataview: {e}")),
    };
    let Some(index) = index else {
        return aviso("Dataview: esperando el índice de la bóveda…".into());
    };
    let res = match dataview::ejecutar(index, path, &consulta) {
        Ok(r) => r,
        Err(e) => return aviso(format!("Dataview: {e}")),
    };
    resultado_a_bloque(res)
}

fn resultado_a_bloque(res: ResultadoDv) -> BlockKind {
    match res {
        ResultadoDv::Tabla {
            encabezados, filas, ..
        } => {
            if filas.is_empty() {
                return aviso("Dataview: ninguna nota coincide".into());
            }
            let header: Vec<Vec<Inline>> = encabezados
                .into_iter()
                .map(|h| vec![Inline::Text(h)])
                .collect();
            let cols = header.len();
            let rows: Vec<Vec<Vec<Inline>>> = filas
                .into_iter()
                .map(|(id, vals)| {
                    let mut fila = Vec::with_capacity(cols);
                    if let Some(v) = id {
                        fila.push(valor_a_inlines(&v));
                    }
                    for v in vals {
                        fila.push(valor_a_inlines(&v));
                    }
                    fila
                })
                .collect();
            BlockKind::Table {
                align: vec![Align::None; cols],
                header,
                rows,
            }
        }
        ResultadoDv::Lista { grupos } => {
            let total: usize = grupos.iter().map(|(_, items)| items.len()).sum();
            if total == 0 {
                return aviso("Dataview: ninguna nota coincide".into());
            }
            if grupos.len() == 1 && grupos[0].0.is_none() {
                return BlockKind::List {
                    start: None,
                    items: lista_items(&grupos[0].1),
                };
            }
            let mut bloques = Vec::new();
            for (clave, items) in grupos {
                if let Some(k) = clave {
                    bloques.push(bloque(BlockKind::Heading {
                        level: 4,
                        inlines: valor_a_inlines(&k),
                    }));
                }
                bloques.push(bloque(BlockKind::List {
                    start: None,
                    items: lista_items(&items),
                }));
            }
            BlockKind::Group(bloques)
        }
        ResultadoDv::Tareas { grupos } => {
            let total: usize = grupos.iter().map(|(_, ts)| ts.len()).sum();
            if total == 0 {
                return aviso("Dataview: ninguna tarea coincide".into());
            }
            if grupos.len() == 1 && grupos[0].0.is_none() {
                return BlockKind::List {
                    start: None,
                    items: tarea_items(&grupos[0].1),
                };
            }
            let mut bloques = Vec::new();
            for (clave, ts) in grupos {
                if let Some(k) = clave {
                    bloques.push(bloque(BlockKind::Heading {
                        level: 4,
                        inlines: valor_a_inlines(&k),
                    }));
                }
                bloques.push(bloque(BlockKind::List {
                    start: None,
                    items: tarea_items(&ts),
                }));
            }
            BlockKind::Group(bloques)
        }
        ResultadoDv::Calendario { meses } => {
            if meses.is_empty() {
                return aviso("Dataview: ninguna nota tiene fecha para el calendario".into());
            }
            let mut bloques = Vec::new();
            for m in meses {
                bloques.extend(mes_a_bloques(&m));
            }
            BlockKind::Group(bloques)
        }
    }
}

fn lista_items(items: &[(Valor, Option<Valor>)]) -> Vec<ListItem> {
    items
        .iter()
        .map(|(id, extra)| {
            let mut inlines = valor_a_inlines(id);
            if let Some(ex) = extra {
                let s = ex.a_texto();
                if !s.is_empty() {
                    inlines.push(Inline::Text(": ".into()));
                    inlines.extend(valor_a_inlines(ex));
                }
            }
            ListItem {
                task: None,
                blocks: vec![bloque(BlockKind::Paragraph(inlines))],
            }
        })
        .collect()
}

fn tarea_items(tareas: &[TareaResultado]) -> Vec<ListItem> {
    tareas
        .iter()
        .map(|t| {
            let marca = if t.completed { "☑ " } else { "☐ " };
            let mut inlines = vec![Inline::Text(marca.into())];
            if t.completed {
                inlines.push(Inline::Strike(vec![Inline::Text(t.texto.clone())]));
            } else {
                inlines.push(Inline::Text(t.texto.clone()));
            }
            inlines.push(Inline::Text(" — ".into()));
            inlines.push(enlace_ruta(&t.ruta, t.ruta.file_name()));
            ListItem {
                task: None,
                blocks: vec![bloque(BlockKind::Paragraph(inlines))],
            }
        })
        .collect()
}

fn nombre_mes(mes: u8) -> &'static str {
    match mes {
        1 => "Enero",
        2 => "Febrero",
        3 => "Marzo",
        4 => "Abril",
        5 => "Mayo",
        6 => "Junio",
        7 => "Julio",
        8 => "Agosto",
        9 => "Septiembre",
        10 => "Octubre",
        11 => "Noviembre",
        12 => "Diciembre",
        _ => "",
    }
}

fn mes_a_bloques(m: &MesCalendario) -> Vec<Block> {
    let titulo = bloque(BlockKind::Heading {
        level: 4,
        inlines: vec![Inline::Text(format!("{} {}", nombre_mes(m.mes), m.anio))],
    });
    let header: Vec<Vec<Inline>> = ["Lu", "Ma", "Mi", "Ju", "Vi", "Sá", "Do"]
        .into_iter()
        .map(|d| vec![Inline::Text(d.into())])
        .collect();
    let primero = dataview::Fecha {
        anio: m.anio,
        mes: m.mes,
        dia: 1,
    };
    let offset = usize::from(primero.dia_semana_lunes());
    let total_dias = dataview::dias_del_mes(m.anio, m.mes);
    let mut rows: Vec<Vec<Vec<Inline>>> = Vec::new();
    let mut semana: Vec<Vec<Inline>> = vec![Vec::new(); 7];
    let mut col = offset;
    for dia in 1..=total_dias {
        let mut celda = vec![Inline::Text(dia.to_string())];
        if let Some(notas) = m.dias.get(&dia) {
            for n in notas {
                celda.push(Inline::Text(" · ".into()));
                celda.extend(valor_a_inlines(n));
            }
        }
        semana[col] = celda;
        col += 1;
        if col == 7 {
            rows.push(semana);
            semana = vec![Vec::new(); 7];
            col = 0;
        }
    }
    if col > 0 {
        rows.push(semana);
    }
    let tabla = bloque(BlockKind::Table {
        align: vec![Align::Left; 7],
        header,
        rows,
    });
    vec![titulo, tabla]
}

/// Recorre `blocks` y reemplaza cada `Inline::DataviewExpr` por su valor evaluado.
/// Devuelve `true` si encontró (y procesó) al menos una expresión inline.
pub(super) fn evaluar_inlines(
    blocks: &mut [Block],
    index: Option<&orbital_vault::SearchIndex>,
    path: Option<&orbital_vault::RelPath>,
) -> bool {
    let mut encontrado = false;
    for b in blocks {
        match &mut b.kind {
            BlockKind::Paragraph(inls)
            | BlockKind::Heading { inlines: inls, .. }
            | BlockKind::FootnoteDefinition { inlines: inls, .. } => {
                encontrado |= eval_inlines_vec(inls, index, path);
            }
            BlockKind::List { items, .. } => {
                for it in items {
                    encontrado |= evaluar_inlines(&mut it.blocks, index, path);
                }
            }
            BlockKind::Table { header, rows, .. } => {
                for c in header {
                    encontrado |= eval_inlines_vec(c, index, path);
                }
                for r in rows {
                    for c in r {
                        encontrado |= eval_inlines_vec(c, index, path);
                    }
                }
            }
            BlockKind::Quote(body)
            | BlockKind::Callout { body, .. }
            | BlockKind::Seccion { body, .. }
            | BlockKind::Group(body) => {
                encontrado |= evaluar_inlines(body, index, path);
            }
            BlockKind::Flashcard(f) => {
                encontrado |= evaluar_inlines(&mut f.question, index, path);
                encontrado |= evaluar_inlines(&mut f.answer, index, path);
            }
            _ => {}
        }
    }
    encontrado
}

fn eval_inlines_vec(
    inlines: &mut Vec<Inline>,
    index: Option<&orbital_vault::SearchIndex>,
    path: Option<&orbital_vault::RelPath>,
) -> bool {
    let mut encontrado = false;
    let old = std::mem::take(inlines);
    for inl in old {
        match inl {
            Inline::DataviewExpr(expr) => {
                encontrado = true;
                match index {
                    Some(idx) => match dataview::evaluar_inline(idx, path, &expr) {
                        Ok(val) => inlines.extend(valor_a_inlines(&val)),
                        Err(e) => {
                            inlines.push(Inline::Emph(vec![Inline::Text(format!("Error ({e})"))]))
                        }
                    },
                    None => inlines.push(Inline::Emph(vec![Inline::Text("…".into())])),
                }
            }
            Inline::Strong(mut c) => {
                encontrado |= eval_inlines_vec(&mut c, index, path);
                inlines.push(Inline::Strong(c));
            }
            Inline::Emph(mut c) => {
                encontrado |= eval_inlines_vec(&mut c, index, path);
                inlines.push(Inline::Emph(c));
            }
            Inline::Strike(mut c) => {
                encontrado |= eval_inlines_vec(&mut c, index, path);
                inlines.push(Inline::Strike(c));
            }
            Inline::Link { url, mut children } => {
                encontrado |= eval_inlines_vec(&mut children, index, path);
                inlines.push(Inline::Link { url, children });
            }
            Inline::Styled {
                raw_spec,
                fg,
                bg,
                mut children,
            } => {
                encontrado |= eval_inlines_vec(&mut children, index, path);
                inlines.push(Inline::Styled {
                    raw_spec,
                    fg,
                    bg,
                    children,
                });
            }
            other => inlines.push(other),
        }
    }
    encontrado
}

/// HTML estático de un bloque `dataview` / `dv` para exportación (065).
pub(super) fn html_bloque(
    raw: &str,
    index: Option<&orbital_vault::SearchIndex>,
    href: &dyn Fn(&orbital_vault::RelPath) -> String,
) -> Option<String> {
    let esc = |t: &str| {
        t.replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
            .replace('"', "&quot;")
    };
    let consulta = match dataview::parse_consulta(raw) {
        Ok(c) => c,
        Err(e) => return Some(format!("<p><em>Dataview: {}</em></p>", esc(&e))),
    };
    let index = index?;
    let res = match dataview::ejecutar(index, None, &consulta) {
        Ok(r) => r,
        Err(e) => return Some(format!("<p><em>Dataview: {}</em></p>", esc(&e))),
    };
    let valor_html = |v: &Valor| -> String {
        fn rec(v: &Valor, href: &dyn Fn(&orbital_vault::RelPath) -> String) -> String {
            let esc = |t: &str| {
                t.replace('&', "&amp;")
                    .replace('<', "&lt;")
                    .replace('>', "&gt;")
                    .replace('"', "&quot;")
            };
            match v {
                Valor::Null => "—".into(),
                Valor::Enlace { ruta, etiqueta } => {
                    format!("<a href=\"{}\">{}</a>", esc(&href(ruta)), esc(etiqueta))
                }
                Valor::Lista(items) => {
                    if items.is_empty() {
                        "—".into()
                    } else {
                        items
                            .iter()
                            .map(|it| rec(it, href))
                            .collect::<Vec<_>>()
                            .join(", ")
                    }
                }
                other => {
                    let s = other.a_texto();
                    if s.is_empty() { "—".into() } else { esc(&s) }
                }
            }
        }
        rec(v, href)
    };
    let mut out = String::new();
    match res {
        ResultadoDv::Tabla {
            encabezados, filas, ..
        } => {
            if filas.is_empty() {
                return Some("<p><em>Dataview: ninguna nota coincide</em></p>".into());
            }
            out.push_str("<table><thead><tr>");
            for h in &encabezados {
                out.push_str(&format!("<th>{}</th>", esc(h)));
            }
            out.push_str("</tr></thead><tbody>");
            for (id, vals) in &filas {
                out.push_str("<tr>");
                if let Some(v) = id {
                    out.push_str(&format!("<td>{}</td>", valor_html(v)));
                }
                for v in vals {
                    out.push_str(&format!("<td>{}</td>", valor_html(v)));
                }
                out.push_str("</tr>");
            }
            out.push_str("</tbody></table>");
        }
        ResultadoDv::Lista { grupos } => {
            for (clave, items) in &grupos {
                if let Some(k) = clave {
                    out.push_str(&format!("<h4>{}</h4>", valor_html(k)));
                }
                out.push_str("<ul>");
                for (id, extra) in items {
                    out.push_str("<li>");
                    out.push_str(&valor_html(id));
                    if let Some(ex) = extra
                        && !ex.a_texto().is_empty()
                    {
                        out.push_str(": ");
                        out.push_str(&valor_html(ex));
                    }
                    out.push_str("</li>");
                }
                out.push_str("</ul>");
            }
        }
        ResultadoDv::Tareas { grupos } => {
            for (clave, ts) in &grupos {
                if let Some(k) = clave {
                    out.push_str(&format!("<h4>{}</h4>", valor_html(k)));
                }
                out.push_str("<ul>");
                for t in ts {
                    let chk = if t.completed { " checked" } else { "" };
                    out.push_str(&format!(
                        "<li><input type=\"checkbox\" disabled{chk}> {} — <a href=\"{}\">{}</a></li>",
                        esc(&t.texto),
                        esc(&href(&t.ruta)),
                        esc(t.ruta.file_name())
                    ));
                }
                out.push_str("</ul>");
            }
        }
        ResultadoDv::Calendario { meses } => {
            for m in &meses {
                out.push_str(&format!("<h4>{} {}</h4>", nombre_mes(m.mes), m.anio));
                out.push_str("<ul>");
                for (dia, notas) in &m.dias {
                    let ls: Vec<String> = notas.iter().map(&valor_html).collect();
                    out.push_str(&format!("<li>Día {dia}: {}</li>", ls.join(", ")));
                }
                out.push_str("</ul>");
            }
        }
    }
    Some(out)
}
