//! Markdown → representación intermedia para la vista formateada (research R2 de la 002).
//!
//! No sabe nada de GTK: la app vuelca el [`RenderDoc`] a widgets. Los rangos y líneas apuntan al
//! texto completo que se pasa (con frontmatter), así la vista formateada puede ubicar cada bloque en
//! el editor y marcar casillas cambiando un solo carácter.

use std::ops::Range;

use pulldown_cmark::{Alignment, CodeBlockKind, Event, Options, Parser, Tag, TagEnd};

use crate::props::markdown;

/// Documento listo para dibujar.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RenderDoc {
    pub blocks: Vec<Block>,
}

/// Un bloque con su ubicación en el texto.
#[derive(Clone, Debug, PartialEq)]
pub struct Block {
    pub kind: BlockKind,
    /// Rango en bytes del bloque dentro del texto completo.
    pub source: Range<usize>,
    /// Línea (desde 0) donde empieza el bloque.
    pub line: usize,
}

#[derive(Clone, Debug, PartialEq)]
pub enum BlockKind {
    Heading {
        level: u8,
        inlines: Vec<Inline>,
    },
    Paragraph(Vec<Inline>),
    List {
        start: Option<u64>,
        items: Vec<ListItem>,
    },
    Quote(Vec<Block>),
    Callout {
        kind: CalloutKind,
        title: String,
        folded: Option<bool>,
        body: Vec<Block>,
    },
    Code {
        lang: String,
        text: String,
    },
    Table {
        align: Vec<Align>,
        header: Vec<Vec<Inline>>,
        rows: Vec<Vec<Vec<Inline>>>,
    },
    Rule,
    /// HTML en bruto dentro del Markdown: se muestra como texto.
    Html(String),
}

#[derive(Clone, Debug, PartialEq)]
pub struct ListItem {
    pub task: Option<TaskMark>,
    pub blocks: Vec<Block>,
}

/// Casilla de una lista de tareas.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TaskMark {
    pub checked: bool,
    /// Byte exacto del carácter de la casilla (`' '` o `'x'`) en el texto completo.
    pub offset: usize,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Inline {
    Text(String),
    Strong(Vec<Inline>),
    Emph(Vec<Inline>),
    Strike(Vec<Inline>),
    Code(String),
    Link { url: String, children: Vec<Inline> },
    Image { url: String, alt: String },
    SoftBreak,
    HardBreak,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Align {
    None,
    Left,
    Center,
    Right,
}

/// Tipos de callout (`> [!tipo]`), con alias en inglés.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CalloutKind {
    Nota,
    Consejo,
    Importante,
    Advertencia,
    Peligro,
    Otro(String),
}

impl CalloutKind {
    fn parse(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "nota" | "note" | "info" => Self::Nota,
            "consejo" | "tip" | "hint" => Self::Consejo,
            "importante" | "important" => Self::Importante,
            "advertencia" | "warning" | "aviso" => Self::Advertencia,
            "peligro" | "danger" | "caution" | "error" => Self::Peligro,
            other => Self::Otro(other.to_owned()),
        }
    }

    /// Título por defecto, en español.
    pub fn label(&self) -> String {
        match self {
            Self::Nota => "Nota".into(),
            Self::Consejo => "Consejo".into(),
            Self::Importante => "Importante".into(),
            Self::Advertencia => "Advertencia".into(),
            Self::Peligro => "Peligro".into(),
            Self::Otro(s) => {
                let mut c = s.chars();
                c.next()
                    .map(|f| f.to_uppercase().collect::<String>() + c.as_str())
                    .unwrap_or_default()
            }
        }
    }
}

type Ev<'a> = (Event<'a>, Range<usize>);

struct Ctx<'a> {
    text: &'a str,
    base: usize,
    line_starts: Vec<usize>,
}

impl Ctx<'_> {
    fn abs(&self, r: &Range<usize>) -> Range<usize> {
        self.base + r.start..self.base + r.end
    }
    fn line(&self, abs: usize) -> usize {
        self.line_starts
            .partition_point(|&s| s <= abs)
            .saturating_sub(1)
    }
}

/// Analiza una nota `.md` completa. El frontmatter y el BOM no forman parte del resultado.
pub fn render_markdown(text: &str) -> RenderDoc {
    let base = match markdown::locate(text) {
        Some(region) => {
            // Después de la línea `---` que cierra el bloque.
            let rest = &text[region.end..];
            region.end + rest.find('\n').map_or(rest.len(), |i| i + 1)
        }
        None => markdown::bom_len(text),
    };
    let body = &text[base..];
    let mut opts = Options::empty();
    opts.insert(Options::ENABLE_TABLES);
    opts.insert(Options::ENABLE_STRIKETHROUGH);
    opts.insert(Options::ENABLE_TASKLISTS);
    let events: Vec<Ev> = Parser::new_ext(body, opts).into_offset_iter().collect();
    let mut line_starts = vec![0];
    line_starts.extend(text.match_indices('\n').map(|(i, _)| i + 1));
    let ctx = Ctx {
        text,
        base,
        line_starts,
    };
    let mut i = 0;
    let blocks = parse_blocks(&ctx, &events, &mut i, None);
    RenderDoc { blocks }
}

fn parse_blocks(ctx: &Ctx, evs: &[Ev], i: &mut usize, until: Option<TagEnd>) -> Vec<Block> {
    let mut out = Vec::new();
    while *i < evs.len() {
        let (ev, range) = &evs[*i];
        if let (Event::End(end), Some(u)) = (ev, until)
            && *end == u
        {
            *i += 1;
            return out;
        }
        let abs = ctx.abs(range);
        let line = ctx.line(abs.start);
        *i += 1;
        let kind = match ev {
            Event::Start(Tag::Paragraph) => {
                BlockKind::Paragraph(parse_inlines(evs, i, TagEnd::Paragraph))
            }
            Event::Start(Tag::Heading { level, .. }) => {
                let level = *level as u8;
                let end = TagEnd::Heading(match level {
                    1 => pulldown_cmark::HeadingLevel::H1,
                    2 => pulldown_cmark::HeadingLevel::H2,
                    3 => pulldown_cmark::HeadingLevel::H3,
                    4 => pulldown_cmark::HeadingLevel::H4,
                    5 => pulldown_cmark::HeadingLevel::H5,
                    _ => pulldown_cmark::HeadingLevel::H6,
                });
                BlockKind::Heading {
                    level,
                    inlines: parse_inlines(evs, i, end),
                }
            }
            Event::Start(Tag::BlockQuote(k)) => {
                let body = parse_blocks(ctx, evs, i, Some(TagEnd::BlockQuote(*k)));
                callout_or_quote(&ctx.text[abs.clone()], body)
            }
            Event::Start(Tag::CodeBlock(kind)) => {
                let lang = match kind {
                    CodeBlockKind::Fenced(info) => {
                        info.split_whitespace().next().unwrap_or("").to_owned()
                    }
                    CodeBlockKind::Indented => String::new(),
                };
                let mut text = String::new();
                while *i < evs.len() {
                    match &evs[*i].0 {
                        Event::End(TagEnd::CodeBlock) => {
                            *i += 1;
                            break;
                        }
                        Event::Text(t) => text.push_str(t),
                        _ => {}
                    }
                    *i += 1;
                }
                BlockKind::Code { lang, text }
            }
            Event::Start(Tag::HtmlBlock) => {
                let mut text = String::new();
                while *i < evs.len() {
                    match &evs[*i].0 {
                        Event::End(TagEnd::HtmlBlock) => {
                            *i += 1;
                            break;
                        }
                        Event::Html(t) | Event::Text(t) => text.push_str(t),
                        _ => {}
                    }
                    *i += 1;
                }
                BlockKind::Html(text)
            }
            Event::Start(Tag::List(start)) => {
                let ordered = start.is_some();
                let mut items = Vec::new();
                while *i < evs.len() {
                    match &evs[*i].0 {
                        Event::End(TagEnd::List(_)) => {
                            *i += 1;
                            break;
                        }
                        Event::Start(Tag::Item) => {
                            *i += 1;
                            items.push(parse_item(ctx, evs, i));
                        }
                        _ => *i += 1,
                    }
                }
                let _ = ordered;
                BlockKind::List {
                    start: *start,
                    items,
                }
            }
            Event::Start(Tag::Table(align)) => parse_table(evs, i, align),
            Event::Rule => BlockKind::Rule,
            Event::Html(t) => BlockKind::Html(t.to_string()),
            // Cualquier otra apertura (notas al pie, definiciones…): se saltea su contenido.
            Event::Start(tag) => {
                let end = tag.to_end();
                skip_to(evs, i, end);
                continue;
            }
            _ => continue,
        };
        out.push(Block {
            kind,
            source: abs,
            line,
        });
    }
    out
}

fn skip_to(evs: &[Ev], i: &mut usize, end: TagEnd) {
    let mut depth = 0usize;
    while *i < evs.len() {
        match &evs[*i].0 {
            Event::Start(t) if t.to_end() == end => depth += 1,
            Event::End(e) if *e == end => {
                if depth == 0 {
                    *i += 1;
                    return;
                }
                depth -= 1;
            }
            _ => {}
        }
        *i += 1;
    }
}

fn parse_item(ctx: &Ctx, evs: &[Ev], i: &mut usize) -> ListItem {
    let mut task = None;
    let mut blocks = Vec::new();
    // En listas «compactas» el texto llega sin párrafo: se agrupa en uno.
    let mut loose: Vec<Inline> = Vec::new();
    let mut loose_start: Option<Range<usize>> = None;
    let flush =
        |loose: &mut Vec<Inline>, start: &mut Option<Range<usize>>, blocks: &mut Vec<Block>| {
            if let Some(r) = start.take() {
                blocks.push(Block {
                    line: ctx.line(r.start),
                    source: r,
                    kind: BlockKind::Paragraph(std::mem::take(loose)),
                });
            }
        };
    while *i < evs.len() {
        let (ev, range) = &evs[*i];
        match ev {
            Event::End(TagEnd::Item) => {
                *i += 1;
                break;
            }
            Event::TaskListMarker(checked) => {
                let abs = ctx.abs(range);
                // El rango es `[ ]` o `[x]`: la casilla es el segundo byte.
                task = Some(TaskMark {
                    checked: *checked,
                    offset: abs.start + 1,
                });
                *i += 1;
            }
            Event::Start(
                Tag::Paragraph
                | Tag::Heading { .. }
                | Tag::BlockQuote(_)
                | Tag::CodeBlock(_)
                | Tag::HtmlBlock
                | Tag::List(_)
                | Tag::Table(_),
            )
            | Event::Rule => {
                flush(&mut loose, &mut loose_start, &mut blocks);
                let mut sub = parse_blocks_one(ctx, evs, i);
                blocks.append(&mut sub);
            }
            _ => {
                let abs = ctx.abs(range);
                loose_start.get_or_insert(abs.clone()).end = abs.end;
                loose.extend(parse_inline_one(evs, i));
            }
        }
    }
    flush(&mut loose, &mut loose_start, &mut blocks);
    ListItem { task, blocks }
}

/// Analiza exactamente un bloque empezando en `i`.
fn parse_blocks_one(ctx: &Ctx, evs: &[Ev], i: &mut usize) -> Vec<Block> {
    // Se arma una vista de un solo bloque: desde `i` hasta su cierre.
    let start = *i;
    let end_tag = match &evs[start].0 {
        Event::Start(t) => Some(t.to_end()),
        _ => None,
    };
    let mut j = start + 1;
    if let Some(end) = end_tag {
        skip_to(evs, &mut j, end);
    }
    let slice = &evs[start..j];
    let mut k = 0;
    let blocks = parse_blocks(ctx, slice, &mut k, None);
    *i = j;
    blocks
}

fn parse_table(evs: &[Ev], i: &mut usize, align: &[Alignment]) -> BlockKind {
    let align = align
        .iter()
        .map(|a| match a {
            Alignment::None => Align::None,
            Alignment::Left => Align::Left,
            Alignment::Center => Align::Center,
            Alignment::Right => Align::Right,
        })
        .collect();
    let mut header = Vec::new();
    let mut rows: Vec<Vec<Vec<Inline>>> = Vec::new();
    let mut in_head = false;
    while *i < evs.len() {
        match &evs[*i].0 {
            Event::End(TagEnd::Table) => {
                *i += 1;
                break;
            }
            Event::Start(Tag::TableHead) => {
                in_head = true;
                *i += 1;
            }
            Event::End(TagEnd::TableHead) => {
                in_head = false;
                *i += 1;
            }
            Event::Start(Tag::TableRow) => {
                rows.push(Vec::new());
                *i += 1;
            }
            Event::Start(Tag::TableCell) => {
                *i += 1;
                let cell = parse_inlines(evs, i, TagEnd::TableCell);
                if in_head {
                    header.push(cell);
                } else if let Some(r) = rows.last_mut() {
                    r.push(cell);
                }
            }
            _ => *i += 1,
        }
    }
    BlockKind::Table {
        align,
        header,
        rows,
    }
}

fn parse_inlines(evs: &[Ev], i: &mut usize, until: TagEnd) -> Vec<Inline> {
    let mut out = Vec::new();
    while *i < evs.len() {
        if let Event::End(e) = &evs[*i].0
            && *e == until
        {
            *i += 1;
            return out;
        }
        out.extend(parse_inline_one(evs, i));
    }
    out
}

/// Analiza un elemento en línea (con sus hijos) y avanza `i`.
fn parse_inline_one(evs: &[Ev], i: &mut usize) -> Option<Inline> {
    let (ev, _) = &evs[*i];
    *i += 1;
    Some(match ev {
        Event::Text(t) => Inline::Text(t.to_string()),
        Event::Code(t) => Inline::Code(t.to_string()),
        Event::InlineHtml(t) | Event::Html(t) => Inline::Text(t.to_string()),
        Event::FootnoteReference(t) => Inline::Text(format!("[^{t}]")),
        Event::SoftBreak => Inline::SoftBreak,
        Event::HardBreak => Inline::HardBreak,
        Event::Start(Tag::Emphasis) => Inline::Emph(parse_inlines(evs, i, TagEnd::Emphasis)),
        Event::Start(Tag::Strong) => Inline::Strong(parse_inlines(evs, i, TagEnd::Strong)),
        Event::Start(Tag::Strikethrough) => {
            Inline::Strike(parse_inlines(evs, i, TagEnd::Strikethrough))
        }
        Event::Start(Tag::Link { dest_url, .. }) => Inline::Link {
            url: dest_url.to_string(),
            children: parse_inlines(evs, i, TagEnd::Link),
        },
        Event::Start(Tag::Image { dest_url, .. }) => {
            let alt = plain_text(&parse_inlines(evs, i, TagEnd::Image));
            Inline::Image {
                url: dest_url.to_string(),
                alt,
            }
        }
        Event::Start(tag) => {
            let inner = parse_inlines(evs, i, tag.to_end());
            return Some(Inline::Text(plain_text(&inner)));
        }
        _ => return None,
    })
}

/// Texto plano de una lista de elementos en línea.
pub fn plain_text(inlines: &[Inline]) -> String {
    let mut s = String::new();
    for i in inlines {
        match i {
            Inline::Text(t) | Inline::Code(t) => s.push_str(t),
            Inline::Strong(c)
            | Inline::Emph(c)
            | Inline::Strike(c)
            | Inline::Link { children: c, .. } => {
                s.push_str(&plain_text(c));
            }
            Inline::Image { alt, .. } => s.push_str(alt),
            Inline::SoftBreak => s.push(' '),
            Inline::HardBreak => s.push('\n'),
        }
    }
    s
}

/// Si la cita empieza con `[!tipo]`, es un callout (FR-009).
fn callout_or_quote(source: &str, mut body: Vec<Block>) -> BlockKind {
    let first = source.lines().next().unwrap_or("");
    let first = first.trim_start().trim_start_matches('>').trim_start();
    let Some(rest) = first.strip_prefix("[!") else {
        return BlockKind::Quote(body);
    };
    let Some(close) = rest.find(']') else {
        return BlockKind::Quote(body);
    };
    let kind_name = &rest[..close];
    if kind_name.is_empty() || kind_name.contains(char::is_whitespace) {
        return BlockKind::Quote(body);
    }
    let after = &rest[close + 1..];
    let (folded, title) = match after.chars().next() {
        Some('+') => (Some(false), after[1..].trim()),
        Some('-') => (Some(true), after[1..].trim()),
        _ => (None, after.trim()),
    };
    let kind = CalloutKind::parse(kind_name);
    let title = if title.is_empty() {
        kind.label()
    } else {
        title.to_owned()
    };
    // Quitar la primera línea (el marcador) del primer párrafo.
    if let Some(Block {
        kind: BlockKind::Paragraph(inlines),
        ..
    }) = body.first_mut()
    {
        match inlines
            .iter()
            .position(|x| matches!(x, Inline::SoftBreak | Inline::HardBreak))
        {
            Some(p) => {
                inlines.drain(..=p);
            }
            None => {
                body.remove(0);
            }
        }
    }
    BlockKind::Callout {
        kind,
        title,
        folded,
        body,
    }
}

/// Cambia la casilla de una tarea: `' '` ↔ `'x'` en `offset`. Devuelve el texto nuevo, o `None` si en
/// esa posición no hay una casilla.
pub fn toggle_task(source: &str, offset: usize) -> Option<String> {
    let b = source.as_bytes();
    if offset == 0 || offset + 1 >= b.len() || b[offset - 1] != b'[' || b[offset + 1] != b']' {
        return None;
    }
    let new = match b[offset] {
        b' ' => "x",
        b'x' | b'X' => " ",
        _ => return None,
    };
    let mut out = String::with_capacity(source.len());
    out.push_str(&source[..offset]);
    out.push_str(new);
    out.push_str(&source[offset + 1..]);
    Some(out)
}
