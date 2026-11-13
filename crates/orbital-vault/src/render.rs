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
    /// Un codeblock `indice` o `toc` (005): el fuente queda en el disco y la app completa el
    /// contenido en la vista. El rango apunta al codeblock completo, cercas incluidas.
    Generated(Generated),
    /// Una definición de nota al pie `[^nombre]: contenido` (012): la Lectura no la muestra
    /// como párrafo; guarda el contenido para el tooltip de la referencia.
    FootnoteDefinition {
        nombre: String,
        inlines: Vec<Inline>,
    },
    /// Un párrafo que es solo `![[nota]]` o `![[nota#encabezado]]` (US5 de la 003). Lo expande
    /// [`crate::Vault::expand_transclusions`]; `render_markdown` lo deja en [`Embed::Pending`].
    Transclusion(Transclusion),
}

/// Clases de codeblock que la vista completa con contenido generado (005).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GeneratedKind {
    /// Lista de notas con una etiqueta (subárbol del árbol de etiquetas).
    Indice,
    /// Índice de encabezados de la nota, con opciones de nivel.
    Toc,
    /// Tarjeta de enlace externo (013): url, title, description, host, favicon, image.
    CardLink,
}

impl GeneratedKind {
    /// Clase por lenguaje del codeblock; otro lenguaje no es un bloque generado.
    fn from_lang(lang: &str) -> Option<Self> {
        match lang {
            "indice" => Some(Self::Indice),
            "toc" => Some(Self::Toc),
            "cardlink" => Some(Self::CardLink),
            _ => None,
        }
    }
}

/// Un codeblock `indice` o `toc` con sus parámetros YAML. Si el cuerpo no parsea o al bloque
/// `indice` le falta `tag`, queda como [`BlockKind::Code`] común.
#[derive(Clone, Debug, PartialEq)]
pub struct Generated {
    pub kind: GeneratedKind,
    /// Parámetros del cuerpo, en el orden del archivo.
    pub params: Vec<crate::props::PropEntry>,
    /// Texto crudo del cuerpo del codeblock, para mostrarlo donde todavía no se completa.
    pub raw: String,
    /// Rango del codeblock completo (cercas incluidas) en el texto fuente.
    pub source: Range<usize>,
}

/// Una transclusión `![[destino#ancla]]`.
#[derive(Clone, Debug, PartialEq)]
pub struct Transclusion {
    /// El destino tal como está escrito (`nota`, `carpeta/nota`, `nota.md`).
    pub target: String,
    pub anchor: Option<String>,
    pub content: Embed,
}

/// Qué muestra una transclusión.
#[derive(Clone, Debug, PartialEq)]
pub enum Embed {
    /// Todavía no se expandió.
    Pending,
    /// El contenido de la nota (o de su sección), ya expandido. `hash`: del archivo leído, para
    /// saber si cambió.
    Note {
        path: crate::RelPath,
        hash: u64,
        blocks: Vec<Block>,
    },
    /// La nota existe pero no tiene ese encabezado.
    SectionMissing {
        path: crate::RelPath,
    },
    /// Una nota `.html`: no se incrusta en la vista formateada, se ofrece abrirla.
    Html {
        path: crate::RelPath,
    },
    /// No existe: se puede crear (OKF §6.1).
    Missing,
    Ambiguous(Vec<crate::RelPath>),
    /// La nota ya se está mostrando más arriba (A incrusta a B, que incrusta a A).
    Cycle {
        path: crate::RelPath,
    },
    /// Más de [`crate::transclusion::MAX_DEPTH`] niveles de anidación.
    TooDeep,
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
    /// Una referencia de nota al pie `[^nombre]` (005/012): la Lectura la muestra como
    /// superíndice con tooltip del contenido de la definición.
    FootnoteRef(String),
    Strong(Vec<Inline>),
    Emph(Vec<Inline>),
    Strike(Vec<Inline>),
    Code(String),
    Link {
        url: String,
        children: Vec<Inline>,
    },
    Image {
        url: String,
        alt: String,
    },
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
    tags: Vec<crate::tags::TagOccurrence>,
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
    parse_doc(text, &[])
}

/// Analiza una nota `.md` completa con los marcadores `[%clave]` sustituidos por su valor (005).
/// Los rangos de bloque siguen apuntando al texto fuente: la sustitución cambia el contenido de
/// los eventos de texto, no sus rangos.
pub fn render_markdown_interpolated(
    text: &str,
    repl: &[(std::ops::Range<usize>, String)],
) -> RenderDoc {
    parse_doc(text, repl)
}

/// Sustituye los marcadores en las corridas de eventos de texto consecutivos (005).
/// pulldown-cmark divide el texto en varios eventos (en `[`, `]`, escapes), así que un
/// marcador puede abarcar varios: se reconstruye la corrida completa con el valor en el
/// lugar del marcador. Los rangos siguen apuntando al texto fuente. Un marcador cortado por
/// un nodo inline (negrita, código) no se sustituye: queda como fuente.
/// Emite el segmento `[from, to)` repartido entre los eventos de la corrida: cada tramo que
/// cae dentro de un evento sale como tajo *borrowed*; los tramos cruzados se realocan.
fn emit_segment<'a>(
    run: &[(pulldown_cmark::CowStr<'a>, std::ops::Range<usize>)],
    out: &mut Vec<Ev<'a>>,
    ev_ix: &mut usize,
    from: usize,
    to: usize,
) {
    let mut at = from;
    while at < to && *ev_ix < run.len() {
        let (t, r) = &run[*ev_ix];
        if r.end <= at {
            *ev_ix += 1;
            continue;
        }
        let start = at.max(r.start);
        let stop = to.min(r.end);
        if start < stop {
            // Tajo sin copiar cuando el evento es prestado (el caso común del parser).
            let piece = match t {
                pulldown_cmark::CowStr::Borrowed(s) => {
                    let s: &'a str = s; // copia la referencia interna: no depende del préstamo
                    pulldown_cmark::CowStr::Borrowed(&s[start - r.start..stop - r.start])
                }
                other => pulldown_cmark::CowStr::Boxed(
                    other[start - r.start..stop - r.start]
                        .to_string()
                        .into_boxed_str(),
                ),
            };
            out.push((Event::Text(piece), start..stop));
        }
        if stop == r.end {
            *ev_ix += 1;
        }
        at = stop;
    }
}

fn substitute_runs<'a>(
    mut events: Vec<Ev<'a>>,
    base: usize,
    repl: &[(std::ops::Range<usize>, String)],
) -> Vec<Ev<'a>> {
    let repl_rel: Vec<(std::ops::Range<usize>, &str)> = repl
        .iter()
        .map(|(range, value)| (range.start - base..range.end - base, value.as_str()))
        .collect();
    let mut out: Vec<Ev> = Vec::with_capacity(events.len());
    let mut run: Vec<(pulldown_cmark::CowStr, std::ops::Range<usize>)> = Vec::new();
    fn flush<'a>(
        run: &mut Vec<(pulldown_cmark::CowStr<'a>, std::ops::Range<usize>)>,
        out: &mut Vec<Ev<'a>>,
        repl: &[(std::ops::Range<usize>, &str)],
        base: usize,
    ) {
        if run.is_empty() {
            return;
        }
        let run_start = run[0].1.start;
        let run_end = run.last().expect("corrida no vacía").1.end;
        // Corrida sin marcadores: los eventos originales pasan intactos, sin realocar.
        let hay = repl.iter().any(|(r, _)| {
            r.start.saturating_sub(base) < run_end && r.end.saturating_sub(base) > run_start
        });
        if !hay {
            out.extend(run.drain(..).map(|(t, r)| (Event::Text(t), r)));
            return;
        }
        // Con marcadores: los segmentos entre marcadores se emiten como tajos *borrowed* de los
        // eventos originales cuando caen dentro de uno solo (el caso común); solo el valor es
        // una cadena nueva.
        let mut ev_ix = 0usize;
        let mut at = run_start;
        for (range, value) in repl {
            if range.start < at || range.end > run_end {
                continue;
            }
            emit_segment(run, out, &mut ev_ix, at, range.start);
            out.push((Event::Text((*value).to_owned().into()), range.clone()));
            at = range.end;
        }
        emit_segment(run, out, &mut ev_ix, at, run_end);
        run.clear();
    }
    for (ev, range) in events.drain(..) {
        if let Event::Text(t) = ev {
            run.push((t, range));
        } else {
            flush(&mut run, &mut out, &repl_rel, base);
            out.push((ev, range));
        }
    }
    flush(&mut run, &mut out, &repl_rel, base);
    for (ev, range) in events.drain(..) {
        if let Event::Text(t) = ev {
            run.push((t, range));
        } else {
            flush(&mut run, &mut out, &repl_rel, base);
            out.push((ev, range));
        }
    }
    flush(&mut run, &mut out, &repl_rel, base);
    out
}

fn parse_doc(text: &str, repl: &[(std::ops::Range<usize>, String)]) -> RenderDoc {
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
    opts.insert(Options::ENABLE_FOOTNOTES);
    let events: Vec<Ev> = Parser::new_ext(body, opts).into_offset_iter().collect();
    let events = if repl.is_empty() {
        events
    } else {
        substitute_runs(events, base, repl)
    };
    let mut line_starts = vec![0];
    line_starts.extend(text.match_indices('\n').map(|(i, _)| i + 1));
    let mut tags = crate::tags::extract_tags(text, false);
    tags.retain(|tag| !tag.from_prop);
    // Rangos excluidos fusionados: un solo barrido, sin recorrer las etiquetas por evento.
    let mut excluded: Vec<Range<usize>> = events
        .iter()
        .filter_map(|(event, range)| {
            matches!(
                event,
                Event::Start(Tag::Link { .. } | Tag::Image { .. } | Tag::CodeBlock(_))
                    | Event::Code(_)
            )
            .then_some(base + range.start..base + range.end)
        })
        .collect();
    excluded.sort_unstable_by_key(|range| range.start);
    let mut merged: Vec<Range<usize>> = Vec::new();
    for range in excluded {
        if let Some(last) = merged.last_mut()
            && range.start <= last.end
        {
            last.end = last.end.max(range.end);
        } else {
            merged.push(range);
        }
    }
    let mut at = 0;
    tags.retain(|tag| {
        while at < merged.len() && merged[at].end <= tag.byte_range.start {
            at += 1;
        }
        at == merged.len() || merged[at].start >= tag.byte_range.end
    });
    let ctx = Ctx {
        text,
        base,
        line_starts,
        tags,
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
                let inlines = parse_inlines(ctx, evs, i, TagEnd::Paragraph);
                match transclusion(&ctx.text[abs.clone()]) {
                    Some(t) => BlockKind::Transclusion(t),
                    None => BlockKind::Paragraph(inlines),
                }
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
                    inlines: parse_inlines(ctx, evs, i, end),
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
                match GeneratedKind::from_lang(&lang).and_then(|kind| parse_generated(kind, &text))
                {
                    Some(params) => BlockKind::Generated(Generated {
                        kind: GeneratedKind::from_lang(&lang).expect("lenguaje conocido"),
                        params,
                        raw: text.clone(),
                        source: ctx.abs(range),
                    }),
                    None => BlockKind::Code { lang, text },
                }
            }
            Event::Start(Tag::FootnoteDefinition(nombre)) => {
                let inlines = parse_inlines(ctx, evs, i, TagEnd::FootnoteDefinition);
                BlockKind::FootnoteDefinition {
                    nombre: nombre.to_string(),
                    inlines,
                }
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
            Event::Start(Tag::Table(align)) => parse_table(ctx, evs, i, align),
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
                loose.extend(parse_inline_many(ctx, evs, i));
            }
        }
    }
    flush(&mut loose, &mut loose_start, &mut blocks);
    ListItem { task, blocks }
}

/// Valida y completa los parámetros de un codeblock generado. `None` = no es un bloque
/// generado (el codeblock queda común): YAML inválido o un `indice` sin tag.
fn parse_generated(kind: GeneratedKind, body: &str) -> Option<Vec<crate::props::PropEntry>> {
    let mut params = crate::props::yaml::parse_block(body.trim()).ok()?;
    if kind == GeneratedKind::Indice {
        let tag = crate::props::find(&params, "tag")
            .and_then(|entry| entry.value.as_text().map(str::to_owned))
            .or_else(|| raw_line_value(body, "tag"))?;
        match params.iter_mut().find(|entry| entry.key == "tag") {
            Some(entry) => entry.value = crate::props::PropValue::Text(tag),
            None => {
                params.push(crate::props::PropEntry {
                    key: "tag".into(),
                    value: crate::props::PropValue::Text(tag),
                    lines: 0..0,
                });
            }
        }
    }
    Some(params)
}

/// Valor crudo de `clave: valor` en el cuerpo del codeblock, sin reglas de comentario YAML:
/// admite `tag: #fisica` sin comillas.
fn raw_line_value(body: &str, key: &str) -> Option<String> {
    body.lines().find_map(|line| {
        let rest = line.trim().strip_prefix(key)?.strip_prefix(':')?;
        let value = rest.trim();
        (!value.is_empty()).then(|| value.trim_matches(['"', '\'']).to_owned())
    })
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

fn parse_table(ctx: &Ctx, evs: &[Ev], i: &mut usize, align: &[Alignment]) -> BlockKind {
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
                let cell = parse_inlines(ctx, evs, i, TagEnd::TableCell);
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

fn parse_inlines(ctx: &Ctx, evs: &[Ev], i: &mut usize, until: TagEnd) -> Vec<Inline> {
    let mut raw = Vec::new();
    while *i < evs.len() {
        if let Event::End(e) = &evs[*i].0
            && *e == until
        {
            *i += 1;
            break;
        }
        raw.extend(parse_inline_many(ctx, evs, i));
    }
    collapse_and_expand_inlines(raw)
}

/// Fusiona nodos `Inline::Text` contiguos y expande los wikilinks `[[...]]`.
fn collapse_and_expand_inlines(raw: Vec<Inline>) -> Vec<Inline> {
    let mut merged: Vec<Inline> = Vec::with_capacity(raw.len());
    for item in raw {
        if let Inline::Text(t) = item {
            if let Some(Inline::Text(last_t)) = merged.last_mut() {
                last_t.push_str(&t);
            } else {
                merged.push(Inline::Text(t));
            }
        } else {
            merged.push(item);
        }
    }

    let mut out = Vec::with_capacity(merged.len());
    for item in merged {
        match item {
            Inline::Text(t) => out.extend(expand_text_wikilinks(&t)),
            Inline::Strong(c) => out.push(Inline::Strong(collapse_and_expand_inlines(c))),
            Inline::Emph(c) => out.push(Inline::Emph(collapse_and_expand_inlines(c))),
            Inline::Strike(c) => out.push(Inline::Strike(collapse_and_expand_inlines(c))),
            Inline::Link { url, children } => out.push(Inline::Link {
                url,
                children: collapse_and_expand_inlines(children),
            }),
            other => out.push(other),
        }
    }
    out
}

/// Expande texto plano separando los wikilinks `[[...]]` en nodos `Inline::Link`.
fn expand_text_wikilinks(text: &str) -> Vec<Inline> {
    let mut out = Vec::new();
    let mut search_from = 0;

    while let Some(rel_start) = text[search_from..].find("[[") {
        let start_idx = search_from + rel_start;
        if start_idx > 0 && text.as_bytes()[start_idx - 1] == b'!' {
            search_from = start_idx + 2;
            continue;
        }

        if let Some(rel_end) = text[start_idx + 2..].find("]]") {
            let end_idx = start_idx + 2 + rel_end + 2;
            if start_idx > search_from {
                out.push(Inline::Text(text[search_from..start_idx].to_string()));
            }
            let inner = &text[start_idx + 2..end_idx - 2];
            let target = crate::links::parse_link_target(inner);
            let visible = target.alias.unwrap_or_else(|| match &target.anchor {
                Some(a) => format!("{}#{}", target.path, a),
                None => target.path.clone(),
            });
            out.push(Inline::Link {
                url: format!("[[{inner}]]"),
                children: vec![Inline::Text(visible)],
            });
            search_from = end_idx;
        } else {
            break;
        }
    }

    if search_from < text.len() {
        out.push(Inline::Text(text[search_from..].to_string()));
    }
    out
}

/// Usa rangos del fuente; nunca busca nuevamente `#` en texto que el parser haya decodificado.
fn text_with_tags(ctx: &Ctx, text: &str, range: &Range<usize>) -> Vec<Inline> {
    if ctx.text.get(range.clone()) != Some(text) {
        return vec![Inline::Text(text.to_owned())];
    }
    let mut out = Vec::new();
    let mut at = 0;
    let first = ctx
        .tags
        .partition_point(|tag| tag.byte_range.start < range.start);
    for tag in &ctx.tags[first..] {
        if tag.byte_range.start >= range.end {
            break;
        }
        if tag.byte_range.end > range.end {
            continue;
        }
        let start = tag.byte_range.start - range.start;
        let end = tag.byte_range.end - range.start;
        if at < start {
            out.push(Inline::Text(text[at..start].to_owned()));
        }
        out.push(Inline::Link {
            url: format!("orbital-tag:{}", tag.tag),
            children: vec![Inline::Text(text[start..end].to_owned())],
        });
        at = end;
    }
    if at < text.len() {
        out.push(Inline::Text(text[at..].to_owned()));
    }
    out
}

/// Analiza un elemento en línea (con sus hijos) y avanza `i`.
fn parse_inline_many(ctx: &Ctx, evs: &[Ev], i: &mut usize) -> Vec<Inline> {
    let (ev, range) = &evs[*i];
    *i += 1;
    match ev {
        Event::Text(t) => text_with_tags(ctx, t, &ctx.abs(range)),
        Event::Code(t) => vec![Inline::Code(t.to_string())],
        Event::InlineHtml(t) | Event::Html(t) => vec![Inline::Text(t.to_string())],
        Event::FootnoteReference(t) => vec![Inline::FootnoteRef(t.to_string())],
        Event::SoftBreak => vec![Inline::SoftBreak],
        Event::HardBreak => vec![Inline::HardBreak],
        Event::Start(Tag::Emphasis) => {
            vec![Inline::Emph(parse_inlines(ctx, evs, i, TagEnd::Emphasis))]
        }
        Event::Start(Tag::Strong) => {
            vec![Inline::Strong(parse_inlines(ctx, evs, i, TagEnd::Strong))]
        }
        Event::Start(Tag::Strikethrough) => {
            vec![Inline::Strike(parse_inlines(
                ctx,
                evs,
                i,
                TagEnd::Strikethrough,
            ))]
        }
        Event::Start(Tag::Link { dest_url, .. }) => vec![Inline::Link {
            url: dest_url.to_string(),
            children: parse_inlines(ctx, evs, i, TagEnd::Link),
        }],
        Event::Start(Tag::Image { dest_url, .. }) => {
            let alt = plain_text(&parse_inlines(ctx, evs, i, TagEnd::Image));
            vec![Inline::Image {
                url: dest_url.to_string(),
                alt,
            }]
        }
        Event::Start(tag) => {
            let inner = parse_inlines(ctx, evs, i, tag.to_end());
            vec![Inline::Text(plain_text(&inner))]
        }
        _ => Vec::new(),
    }
}

/// Texto plano de una lista de elementos en línea.
pub fn plain_text(inlines: &[Inline]) -> String {
    let mut s = String::new();
    for i in inlines {
        match i {
            Inline::Text(t) | Inline::Code(t) => s.push_str(t),
            Inline::FootnoteRef(nombre) => s.push_str(&format!("[^{nombre}]")),
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

/// Un párrafo que es solo `![[destino]]` (con ancla o alias opcionales) es una transclusión.
fn transclusion(source: &str) -> Option<Transclusion> {
    let inner = source.trim().strip_prefix("![[")?.strip_suffix("]]")?;
    if inner.contains("]]") || inner.contains("[[") || inner.contains('\n') {
        return None;
    }
    let t = crate::links::parse_link_target(inner);
    if t.path.is_empty() {
        return None;
    }
    Some(Transclusion {
        target: t.path,
        anchor: t.anchor.filter(|a| !a.is_empty()),
        content: Embed::Pending,
    })
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
