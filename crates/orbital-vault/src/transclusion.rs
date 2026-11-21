//! Transclusiones `![[nota]]` y `![[nota#encabezado]]` (US5 de la 003, research R6).
//!
//! [`crate::render::render_markdown`] deja cada transclusión sin expandir; [`Vault::expand_transclusions`]
//! lee las notas incrustadas (en el hilo de fondo donde se analiza el Markdown) y las reemplaza
//! por su contenido, con un conjunto de notas visitadas contra los ciclos y un máximo de
//! [`MAX_DEPTH`] niveles.

use crate::entry::RelPath;
use crate::links::Resolution;
use crate::render::{Block, BlockKind, Embed, RenderDoc, plain_text, render_markdown};
use crate::vault::Vault;
use crate::write::hash;

/// Niveles de transclusión anidada como máximo (FR-017).
pub const MAX_DEPTH: usize = 3;

impl Vault {
    /// Expande las transclusiones de `doc` (el contenido de `from`). Devuelve las notas
    /// incrustadas, a cualquier profundidad: si una de ellas cambia, hay que volver a dibujar.
    pub fn expand_transclusions(&self, from: &RelPath, doc: &mut RenderDoc) -> Vec<RelPath> {
        let mut deps = Vec::new();
        let mut visited = vec![from.clone()];
        self.expand(&mut doc.blocks, &mut visited, 1, &mut deps);
        deps.sort();
        deps.dedup();
        deps
    }

    fn expand(
        &self,
        blocks: &mut [Block],
        visited: &mut Vec<RelPath>,
        depth: usize,
        deps: &mut Vec<RelPath>,
    ) {
        for b in blocks {
            match &mut b.kind {
                BlockKind::Transclusion(t) if t.content == Embed::Pending => {
                    t.content = self.embed(&t.target, t.anchor.as_deref(), visited, depth, deps);
                }
                // Los medios de un carrusel (032), desde la nota que lo tiene.
                BlockKind::Carrusel(c) => {
                    if let Some(from) = visited.last() {
                        self.resolver_carrusel(from, c);
                    }
                }
                BlockKind::Quote(body)
                | BlockKind::Callout { body, .. }
                | BlockKind::Seccion { body, .. }
                | BlockKind::Group(body) => self.expand(body, visited, depth, deps),
                BlockKind::Flashcard(f) => {
                    self.expand(&mut f.question, visited, depth, deps);
                    self.expand(&mut f.answer, visited, depth, deps);
                }
                BlockKind::List { items, .. } => {
                    for item in items {
                        self.expand(&mut item.blocks, visited, depth, deps);
                    }
                }
                _ => {}
            }
        }
    }

    fn embed_base(&self, target: &str, anchor: Option<&str>, deps: &mut Vec<RelPath>) -> Embed {
        use crate::render::Inline;
        let matches: Vec<_> = self
            .entries()
            .iter()
            .filter(|e| e.kind == crate::EntryKind::Base)
            .filter(|e| {
                if target.contains('/') {
                    e.path.as_str() == target
                } else {
                    e.path.file_name() == target
                }
            })
            .map(|e| e.path.clone())
            .collect();
        let path = match matches.as_slice() {
            [] => return Embed::Missing,
            [path] => path.clone(),
            _ => return Embed::Ambiguous(matches),
        };
        deps.push(path.clone());
        let message = |text: String| {
            vec![Block {
                kind: BlockKind::Paragraph(vec![Inline::Text(text)]),
                source: 0..0,
                line: 0,
                id: None,
            }]
        };
        let document = match crate::bases::read(self, &path) {
            Ok(document) => document,
            Err(error) => {
                return Embed::Note {
                    path,
                    hash: 0,
                    blocks: message(format!("No se pudo consultar la base: {error}")),
                };
            }
        };
        if anchor.is_some() {
            return Embed::SectionMissing { path };
        }
        let snapshot = self.base_snapshot(&document.definition);
        let header = document
            .definition
            .columns
            .iter()
            .map(|c| vec![Inline::Text(c.label.clone())])
            .collect();
        let rows: Vec<Vec<Vec<Inline>>> = snapshot
            .rows
            .iter()
            .take(100)
            .map(|row| {
                document
                    .definition
                    .columns
                    .iter()
                    .map(|c| {
                        if c.property == "title" || c.property == "file.name" {
                            vec![Inline::Link {
                                url: crate::bases::path_url(&row.path),
                                children: vec![Inline::Text(if c.property == "file.name" {
                                    row.path.file_name().to_owned()
                                } else {
                                    row.title.clone()
                                })],
                            }]
                        } else {
                            vec![Inline::Text(
                                row.value(&c.property)
                                    .map(base_value_text)
                                    .unwrap_or_else(|| "—".to_owned()),
                            )]
                        }
                    })
                    .collect()
            })
            .collect();
        let mut blocks = if document.definition.view == crate::bases::BaseView::Cards {
            rows.into_iter()
                .map(|cells| {
                    let mut inlines = Vec::new();
                    for (column, cell) in document.definition.columns.iter().zip(cells) {
                        if !inlines.is_empty() {
                            inlines.push(Inline::HardBreak);
                        }
                        inlines.push(Inline::Strong(vec![Inline::Text(format!(
                            "{}: ",
                            column.label
                        ))]));
                        inlines.extend(cell);
                    }
                    Block {
                        kind: BlockKind::Quote(vec![Block {
                            kind: BlockKind::Paragraph(inlines),
                            source: 0..0,
                            line: 0,
                            id: None,
                        }]),
                        source: 0..0,
                        line: 0,
                        id: None,
                    }
                })
                .collect::<Vec<_>>()
        } else {
            vec![Block {
                kind: BlockKind::Table {
                    align: vec![crate::render::Align::None; document.definition.columns.len()],
                    header,
                    rows,
                },
                source: 0..0,
                line: 0,
                id: None,
            }]
        };
        if snapshot.rows.len() > 100 {
            blocks.extend(message(format!(
                "Mostrando 100 de {} notas. Abrí la base desde su título para verlas todas.",
                snapshot.rows.len()
            )));
        }
        if !snapshot.errors.is_empty() {
            blocks.extend(message(format!(
                "{} archivos no se pudieron consultar",
                snapshot.errors.len()
            )));
        }
        let digest = hash(format!("{blocks:?}").as_bytes());
        Embed::Note {
            path,
            hash: document.hash ^ digest,
            blocks,
        }
    }

    fn embed(
        &self,
        target: &str,
        anchor: Option<&str>,
        visited: &mut Vec<RelPath>,
        depth: usize,
        deps: &mut Vec<RelPath>,
    ) -> Embed {
        if target.to_ascii_lowercase().ends_with(".base") {
            return self.embed_base(target, anchor, deps);
        }
        // Imágenes, audio y video (057): el archivo, resuelto desde la nota que lo incrusta.
        if let Some(tipo) = crate::medios::tipo_de(target) {
            let Some(from) = visited.last() else {
                return Embed::Missing;
            };
            return match self.resolve_attachment(from, target) {
                Resolution::Found(path) => Embed::Archivo {
                    abs: path.to_path(self.root()),
                    path,
                    tipo,
                },
                _ => Embed::Missing,
            };
        }
        let path = match self.resolve_wikilink(target) {
            Resolution::Found(p) => p,
            Resolution::Missing => return Embed::Missing,
            Resolution::Ambiguous(all) => return Embed::Ambiguous(all),
        };
        if visited.contains(&path) {
            return Embed::Cycle { path };
        }
        if depth > MAX_DEPTH {
            return Embed::TooDeep;
        }
        deps.push(path.clone());
        if matches!(path.file_name().rsplit('.').next(), Some("html" | "htm")) {
            return Embed::Html { path };
        }
        let Ok(text) = std::fs::read_to_string(path.to_path(self.root())) else {
            return Embed::Missing;
        };
        let mut blocks = render_markdown(&text).blocks;
        if let Some(a) = anchor {
            if let Some(id) = a.strip_prefix('^') {
                match bloque(blocks, id) {
                    Some(b) => blocks = vec![b],
                    None => return Embed::BlockMissing { path },
                }
            } else {
                match section(blocks, a) {
                    Some(s) => blocks = s,
                    None => return Embed::SectionMissing { path },
                }
            }
        }
        visited.push(path.clone());
        self.expand(&mut blocks, visited, depth + 1, deps);
        visited.pop();
        Embed::Note {
            path,
            hash: hash(text.as_bytes()),
            blocks,
        }
    }
}

/// El encabezado `anchor` y lo que sigue hasta el próximo encabezado de igual o mayor nivel.
fn section(blocks: Vec<Block>, anchor: &str) -> Option<Vec<Block>> {
    let wanted = normalize(anchor);
    let start = blocks.iter().position(|b| {
        matches!(&b.kind, BlockKind::Heading { inlines, .. } if normalize(&plain_text(inlines)) == wanted)
    })?;
    let BlockKind::Heading { level, .. } = blocks[start].kind else {
        return None;
    };
    let end = blocks[start + 1..]
        .iter()
        .position(|b| matches!(b.kind, BlockKind::Heading { level: l, .. } if l <= level))
        .map_or(blocks.len(), |n| start + 1 + n);
    Some(blocks.into_iter().take(end).skip(start).collect())
}

/// El bloque con ese id (026), buscado también dentro de listas, citas y callouts. Un ítem de
/// lista vuelve como una lista de un solo ítem (con sus subítems), para que se vea como ítem.
fn bloque(blocks: Vec<Block>, id: &str) -> Option<Block> {
    for b in blocks {
        if b.id.as_deref() == Some(id) {
            return Some(b);
        }
        match b.kind {
            BlockKind::Quote(body) | BlockKind::Callout { body, .. } => {
                if let Some(x) = bloque(body, id) {
                    return Some(x);
                }
            }
            BlockKind::List { start, items } => {
                for item in items {
                    if let Some(p) = item.blocks.first()
                        && p.id.as_deref() == Some(id)
                    {
                        let (source, line) = (p.source.clone(), p.line);
                        return Some(Block {
                            kind: BlockKind::List {
                                start,
                                items: vec![item],
                            },
                            source,
                            line,
                            id: Some(id.to_owned()),
                        });
                    }
                    if let Some(x) = bloque(item.blocks, id) {
                        return Some(x);
                    }
                }
            }
            _ => {}
        }
    }
    None
}

/// Encabezados comparados sin mayúsculas, sin `#` al principio y con los espacios colapsados.
fn normalize(s: &str) -> String {
    s.trim()
        .trim_start_matches('#')
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

/// Huella de lo incrustado en un bloque (a cualquier profundidad): cambia si cambia una nota
/// incrustada, aunque el texto del bloque sea el mismo. 0 si no incrusta nada.
pub fn fingerprint(block: &Block) -> u64 {
    fn walk(blocks: &[Block], acc: &mut Vec<u8>) {
        for b in blocks {
            match &b.kind {
                BlockKind::Transclusion(t) => {
                    match &t.content {
                        Embed::Note { path, hash, blocks } => {
                            acc.extend_from_slice(path.as_str().as_bytes());
                            acc.extend_from_slice(&hash.to_le_bytes());
                            walk(blocks, acc);
                        }
                        other => acc.extend_from_slice(format!("{other:?}").as_bytes()),
                    }
                    acc.push(0);
                }
                // Un carrusel por carpeta cambia si cambia la carpeta (032).
                BlockKind::Carrusel(c) => {
                    for d in &c.diapositivas {
                        acc.extend_from_slice(d.escrito.as_bytes());
                        acc.push(u8::from(d.abs().is_some()));
                    }
                    acc.push(0);
                }
                BlockKind::Quote(body)
                | BlockKind::Callout { body, .. }
                | BlockKind::Seccion { body, .. }
                | BlockKind::Group(body) => walk(body, acc),
                BlockKind::Flashcard(f) => {
                    walk(&f.question, acc);
                    walk(&f.answer, acc);
                }
                BlockKind::List { items, .. } => {
                    for i in items {
                        walk(&i.blocks, acc);
                    }
                }
                _ => {}
            }
        }
    }
    let mut acc = Vec::new();
    walk(std::slice::from_ref(block), &mut acc);
    if acc.is_empty() { 0 } else { hash(&acc) }
}

impl Vault {
    /// Expande una transclusión suelta de `from` (las de una nota `.html`, que no pasa por
    /// [`render_markdown`]). Devuelve también las notas incrustadas.
    pub fn embed_one(
        &self,
        from: &RelPath,
        target: &str,
        anchor: Option<&str>,
    ) -> (Embed, Vec<RelPath>) {
        let mut deps = Vec::new();
        let mut visited = vec![from.clone()];
        let e = self.embed(target, anchor, &mut visited, 1, &mut deps);
        deps.sort();
        deps.dedup();
        (e, deps)
    }
}

/// Tarjeta HTML de una transclusión, para las notas `.html` (US5, T027). Es un elemento no
/// editable marcado con `data-orbital-embed`: la vista Visual lo quita antes de guardar. `href`
/// da el enlace a una nota (relativo a la nota que la incrusta).
pub fn card_html(
    target: &str,
    anchor: Option<&str>,
    embed: &Embed,
    href: &dyn Fn(&RelPath) -> String,
) -> String {
    let mut out = String::from(
        "<div class=\"orbital-embed\" data-orbital-embed=\"\" contenteditable=\"false\">",
    );
    let shown = match anchor {
        Some(a) => format!("{target} › {a}"),
        None => target.to_owned(),
    };
    let title = |out: &mut String, path: &RelPath, text: &str| {
        out.push_str("<a href=\"");
        let url = if path.file_name().to_ascii_lowercase().ends_with(".base") {
            crate::bases::path_url(path)
        } else {
            href(path)
        };
        out.push_str(&escape(&url));
        out.push_str("\">");
        out.push_str(&escape(text));
        out.push_str("</a>");
    };
    // Un medio (057) va sin tarjeta: el elemento del WebView con sus controles.
    if let Embed::Archivo { path, tipo, .. } = embed {
        return medio_html(path, *tipo, anchor, href);
    }
    out.push_str("<div class=\"orbital-embed-title\">↪ ");
    match embed {
        Embed::Note { path, blocks, .. } => {
            title(&mut out, path, &shown);
            out.push_str("</div>");
            blocks_html(&mut out, blocks, href);
        }
        Embed::Html { path } => {
            title(&mut out, path, &shown);
            out.push_str(" <span class=\"orbital-embed-note\">es una nota HTML: abrila para verla</span></div>");
        }
        Embed::BlockMissing { path } => {
            title(&mut out, path, target);
            out.push_str(&format!(
                " <span class=\"orbital-embed-note\">no tiene el bloque «{}»</span></div>",
                escape(anchor.unwrap_or(""))
            ));
        }
        Embed::SectionMissing { path } => {
            title(&mut out, path, target);
            out.push_str(&format!(
                " <span class=\"orbital-embed-note\">no tiene el encabezado «{}»</span></div>",
                escape(anchor.unwrap_or(""))
            ));
        }
        Embed::Cycle { path } => {
            out.push_str("<span class=\"orbital-embed-note\">↻ Referencia circular a </span>");
            title(&mut out, path, &shown);
            out.push_str("</div>");
        }
        Embed::Archivo { .. } => out.push_str("</div>"),
        Embed::Missing if crate::medios::tipo_de(target).is_some() => out.push_str(&format!(
            "<span class=\"orbital-embed-note\">Archivo inexistente: «{}»</span></div>",
            escape(target)
        )),
        Embed::Missing => out.push_str(&format!(
            "<span class=\"orbital-embed-note\">Nota inexistente: «{}»</span></div>",
            escape(&shown)
        )),
        Embed::Ambiguous(all) => out.push_str(&format!(
            "<span class=\"orbital-embed-note\">Hay {} notas «{}»: escribí la carpeta</span></div>",
            all.len(),
            escape(target)
        )),
        Embed::TooDeep => out.push_str(&format!(
            "<span class=\"orbital-embed-note\">Demasiados niveles de transclusión (máximo {MAX_DEPTH}): «{}»</span></div>",
            escape(&shown)
        )),
        Embed::Pending => out.push_str(&format!("{}</div>", escape(&format!("![[{target}]]")))),
    }
    out.push_str("</div>");
    out
}

/// Un medio incrustado en una nota `.html` (057), marcado con `data-orbital-embed` como las
/// tarjetas: `<audio>`/`<video>` con controles, sin cargar nada hasta que se reproduce
/// (`preload="none"`), y debajo el nombre del archivo (lo que queda si el WebView no lo
/// reproduce); o la imagen. Con `#t=` empieza en ese segundo (fragmento de medio del WebView).
pub fn medio_html(
    path: &RelPath,
    tipo: crate::medios::Tipo,
    anchor: Option<&str>,
    href: &dyn Fn(&RelPath) -> String,
) -> String {
    use crate::medios::Tipo;
    let mut src = href(path);
    if let Some(t) = anchor.and_then(crate::medios::inicio).filter(|t| *t > 0.0) {
        src.push_str(&format!("#t={t}"));
    }
    let src = escape(&src);
    let nombre = escape(path.file_name());
    let marca = "data-orbital-embed=\"\" contenteditable=\"false\"";
    let (clase, etiqueta) = match tipo {
        Tipo::Imagen => {
            return format!(
                "<img class=\"orbital-medio\" {marca} src=\"{src}\" alt=\"{nombre}\" style=\"max-width:100%\">"
            );
        }
        Tipo::Audio => ("Audio", "audio"),
        Tipo::Video => ("Video", "video"),
    };
    format!(
        "<figure class=\"orbital-medio\" {marca} style=\"margin:0.75em 0\">\
<{etiqueta} controls preload=\"none\" src=\"{src}\" aria-label=\"{clase}: {nombre}\" style=\"max-width:100%;width:100%\">{clase}: {nombre}</{etiqueta}>\
<figcaption style=\"opacity:0.75;font-size:0.9em\">{clase}: {nombre}</figcaption></figure>"
    )
}

fn blocks_html(out: &mut String, blocks: &[Block], href: &dyn Fn(&RelPath) -> String) {
    use crate::render::Align;
    for b in blocks {
        match &b.kind {
            BlockKind::Heading { level, inlines } => {
                out.push_str(&format!("<h{level}>"));
                inlines_html(out, inlines);
                out.push_str(&format!("</h{level}>"));
            }
            BlockKind::Paragraph(inlines) => {
                out.push_str("<p>");
                inlines_html(out, inlines);
                out.push_str("</p>");
            }
            BlockKind::List { start, items } => {
                let tag = if start.is_some() { "ol" } else { "ul" };
                match start {
                    Some(n) if *n != 1 => out.push_str(&format!("<ol start=\"{n}\">")),
                    _ => out.push_str(&format!("<{tag}>")),
                }
                for item in items {
                    match item.task {
                        Some(t) => {
                            out.push_str("<li class=\"orbital-task\">");
                            out.push_str(if t.checked {
                                "<input type=\"checkbox\" disabled checked> "
                            } else {
                                "<input type=\"checkbox\" disabled> "
                            });
                        }
                        None => out.push_str("<li>"),
                    }
                    // El primer párrafo va en la misma línea que la viñeta o la casilla.
                    let rest = match item.blocks.first().map(|b| &b.kind) {
                        Some(BlockKind::Paragraph(inlines)) => {
                            inlines_html(out, inlines);
                            &item.blocks[1..]
                        }
                        _ => &item.blocks[..],
                    };
                    blocks_html(out, rest, href);
                    out.push_str("</li>");
                }
                out.push_str(&format!("</{tag}>"));
            }
            BlockKind::Quote(body) => {
                out.push_str("<blockquote>");
                blocks_html(out, body, href);
                out.push_str("</blockquote>");
            }
            BlockKind::Callout { title, body, .. } => {
                out.push_str("<blockquote><p><strong>");
                out.push_str(&escape(title));
                out.push_str("</strong></p>");
                blocks_html(out, body, href);
                out.push_str("</blockquote>");
            }
            // Un carrusel (032) de una nota incrustada: la grilla estática.
            BlockKind::Carrusel(c) => {
                let src = |d: &crate::carrusel::Diapositiva| match &d.estado {
                    crate::carrusel::Estado::Encontrado { path, .. } => Some(href(path)),
                    _ => None,
                };
                out.push_str(&crate::carrusel::html(c, &src, true));
            }
            BlockKind::Code { text, .. } => {
                out.push_str("<pre><code>");
                out.push_str(&escape(text));
                out.push_str("</code></pre>");
            }
            // Las fórmulas (050) de una nota incrustada en una `.html` quedan como su fuente.
            BlockKind::Math { tex } => {
                out.push_str("<p>");
                out.push_str(&escape(&format!("$${tex}$$")));
                out.push_str("</p>");
            }
            BlockKind::FootnoteDefinition { inlines, .. } => {
                out.push_str("<p>");
                inlines_html(out, inlines);
                out.push_str("</p>");
            }
            // En una transclusión el contenido generado no se expande: se ve el codeblock.
            BlockKind::Generated(g) => {
                out.push_str("<pre><code>");
                out.push_str(&escape(&g.raw));
                out.push_str("</code></pre>");
            }
            BlockKind::Table {
                align,
                header,
                rows,
            } => {
                let cell = |out: &mut String, tag: &str, n: usize, c: &[crate::render::Inline]| {
                    let a = match align.get(n) {
                        Some(Align::Center) => " align=\"center\"",
                        Some(Align::Right) => " align=\"right\"",
                        _ => "",
                    };
                    out.push_str(&format!("<{tag}{a}>"));
                    inlines_html(out, c);
                    out.push_str(&format!("</{tag}>"));
                };
                out.push_str("<table><thead><tr>");
                for (n, c) in header.iter().enumerate() {
                    cell(out, "th", n, c);
                }
                out.push_str("</tr></thead><tbody>");
                for row in rows {
                    out.push_str("<tr>");
                    for (n, c) in row.iter().enumerate() {
                        cell(out, "td", n, c);
                    }
                    out.push_str("</tr>");
                }
                out.push_str("</tbody></table>");
            }
            BlockKind::Rule => out.push_str("<hr>"),
            // El HTML crudo de un átomo se muestra como texto: no se mete código ajeno en la nota.
            BlockKind::Html(t) => {
                out.push_str("<pre>");
                out.push_str(&escape(t));
                out.push_str("</pre>");
            }
            BlockKind::Transclusion(t) => {
                out.push_str(&card_html(&t.target, t.anchor.as_deref(), &t.content, href));
            }
            BlockKind::Group(body) => blocks_html(out, body, href),
            BlockKind::Seccion { kind, body } => {
                out.push_str("<section><p><strong>");
                out.push_str(kind.label());
                out.push_str("</strong></p>");
                blocks_html(out, body, href);
                out.push_str("</section>");
            }
            // La tarjeta: la pregunta a la vista y la respuesta plegada.
            BlockKind::Flashcard(f) => {
                out.push_str("<div class=\"orbital-flashcard\">");
                blocks_html(out, &f.question, href);
                out.push_str("<details><summary>Mostrar respuesta</summary>");
                blocks_html(out, &f.answer, href);
                out.push_str("</details></div>");
            }
        }
    }
}

fn inlines_html(out: &mut String, inlines: &[crate::render::Inline]) {
    use crate::render::Inline;
    for i in inlines {
        match i {
            Inline::Text(t) => out.push_str(&escape(t)),
            Inline::FootnoteRef(nombre) => out.push_str(&escape(&format!("[^{nombre}]"))),
            Inline::Code(t) => {
                out.push_str("<code>");
                out.push_str(&escape(t));
                out.push_str("</code>");
            }
            Inline::Strong(c) => wrap(out, "strong", c),
            Inline::Emph(c) => wrap(out, "em", c),
            Inline::Strike(c) => wrap(out, "s", c),
            Inline::Link { url, children } => {
                // Sin `javascript:` (las notas no ejecutan código).
                if url
                    .trim_start()
                    .to_ascii_lowercase()
                    .starts_with("javascript:")
                {
                    inlines_html(out, children);
                } else {
                    out.push_str("<a href=\"");
                    out.push_str(&escape(url));
                    out.push_str("\">");
                    inlines_html(out, children);
                    out.push_str("</a>");
                }
            }
            Inline::Image { url, alt } if crate::medios::es_reproducible(url) => {
                let etiqueta = match crate::medios::tipo_de(url) {
                    Some(crate::medios::Tipo::Video) => "video",
                    _ => "audio",
                };
                out.push_str(&format!(
                    "<{etiqueta} controls preload=\"none\" src=\"{}\" aria-label=\"{}\"></{etiqueta}>",
                    escape(url),
                    escape(if alt.is_empty() { url } else { alt })
                ));
            }
            Inline::Image { url, alt } => {
                out.push_str("<img src=\"");
                out.push_str(&escape(url));
                out.push_str("\" alt=\"");
                out.push_str(&escape(alt));
                out.push_str("\">");
            }
            Inline::Math { tex, display } => {
                let d = if *display { "$$" } else { "$" };
                out.push_str(&escape(&format!("{d}{tex}{d}")));
            }
            Inline::SoftBreak => out.push(' '),
            Inline::HardBreak => out.push_str("<br>"),
        }
    }
}

fn wrap(out: &mut String, tag: &str, c: &[crate::render::Inline]) {
    out.push_str(&format!("<{tag}>"));
    inlines_html(out, c);
    out.push_str(&format!("</{tag}>"));
}

/// Escapa texto para HTML (contenido y atributos entre comillas dobles).
fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            c => out.push(c),
        }
    }
    out
}

fn base_value_text(value: &crate::props::PropValue) -> String {
    use crate::props::PropValue;
    match value {
        PropValue::Null => "nulo".to_owned(),
        PropValue::Text(s)
        | PropValue::Number(s)
        | PropValue::Date(s)
        | PropValue::DateTime(s)
        | PropValue::Link(s) => s.clone(),
        PropValue::Bool(b) => b.to_string(),
        PropValue::List(values) => values
            .iter()
            .map(base_value_text)
            .collect::<Vec<_>>()
            .join(", "),
        PropValue::Map(values) => values
            .iter()
            .map(|(k, v)| format!("{k}: {}", base_value_text(v)))
            .collect::<Vec<_>>()
            .join(", "),
    }
}
