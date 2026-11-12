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
                BlockKind::Quote(body) | BlockKind::Callout { body, .. } => {
                    self.expand(body, visited, depth, deps)
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

    fn embed(
        &self,
        target: &str,
        anchor: Option<&str>,
        visited: &mut Vec<RelPath>,
        depth: usize,
        deps: &mut Vec<RelPath>,
    ) -> Embed {
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
            match section(blocks, a) {
                Some(s) => blocks = s,
                None => return Embed::SectionMissing { path },
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
                BlockKind::Quote(body) | BlockKind::Callout { body, .. } => walk(body, acc),
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
        out.push_str(&escape(&href(path)));
        out.push_str("\">");
        out.push_str(&escape(text));
        out.push_str("</a>");
    };
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
            BlockKind::Code { text, .. } => {
                out.push_str("<pre><code>");
                out.push_str(&escape(text));
                out.push_str("</code></pre>");
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
        }
    }
}

fn inlines_html(out: &mut String, inlines: &[crate::render::Inline]) {
    use crate::render::Inline;
    for i in inlines {
        match i {
            Inline::Text(t) => out.push_str(&escape(t)),
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
            Inline::Image { url, alt } => {
                out.push_str("<img src=\"");
                out.push_str(&escape(url));
                out.push_str("\" alt=\"");
                out.push_str(&escape(alt));
                out.push_str("\">");
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
