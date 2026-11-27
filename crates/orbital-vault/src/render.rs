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
    /// Identificador de bloque (`^id`, 026): se quita del texto que se muestra.
    pub id: Option<String>,
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
    /// Una parte especial de la nota (041): `<!-- summary|sources|connections ID -->`, su
    /// contenido y el cierre `<!---->`. El rango fuente es la sección completa, delimitadores
    /// incluidos; `body` es el contenido analizado como Markdown.
    Seccion {
        kind: SeccionKind,
        body: Vec<Block>,
    },
    /// Bloques que la vista dibuja seguidos, como uno solo (el contenido generado de una tabla
    /// de contenidos con título). Vacío: no se muestra nada.
    Group(Vec<Block>),
    /// Una tarjeta de estudio `:::flashcard id=…` … `---` … `:::` (024). El rango fuente va del
    /// `:::flashcard` al `:::` de cierre; el `---` interno separa la pregunta de la respuesta.
    Flashcard(Flashcard),
    /// Una fórmula en bloque (050): un párrafo que es solo `$$…$$` o un codeblock ` ```math `.
    /// `tex` es el LaTeX sin los delimitadores.
    Math {
        tex: String,
    },
    /// Un carrusel de medios (032): un codeblock ` ```carrusel ` o ` ```media-slider `. Lo
    /// resuelve contra la bóveda [`crate::Vault::expand_transclusions`].
    Carrusel(Box<crate::carrusel::Carrusel>),
    /// Una galería (033): un codeblock ` ```galeria ` con una carpeta de la bóveda. La resuelve
    /// [`crate::Vault::expand_transclusions`].
    Galeria(Box<crate::galeria::Galeria>),
}

/// Una tarjeta de estudio (024): la pregunta y la respuesta, analizadas como Markdown.
#[derive(Clone, Debug, PartialEq)]
pub struct Flashcard {
    /// El valor de `id=` del renglón de apertura.
    pub id: Option<String>,
    /// Posición de la tarjeta entre las de la nota (desde 1) y cuántas tiene la nota.
    pub numero: usize,
    pub total: usize,
    pub question: Vec<Block>,
    pub answer: Vec<Block>,
}

/// Las partes especiales de una nota (041), además de las Propiedades y el Cuerpo.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SeccionKind {
    /// `<!-- summary ID -->`: la Síntesis, entre las Propiedades y el Cuerpo.
    Sintesis,
    /// `<!-- sources ID -->`: las Fuentes, después del Cuerpo.
    Fuentes,
    /// `<!-- connections ID -->`: las Conexiones, siempre al final.
    Conexiones,
}

impl SeccionKind {
    /// La sección que abre una línea `<!-- summary ID -->` (o sus alias en español).
    pub fn apertura(linea: &str) -> Option<Self> {
        let interior = linea.trim().strip_prefix("<!--")?.strip_suffix("-->")?;
        let clave = interior.split_whitespace().next()?.to_lowercase();
        match clave.as_str() {
            "summary" | "sintesis" | "síntesis" => Some(Self::Sintesis),
            "sources" | "fuentes" | "bibliography" => Some(Self::Fuentes),
            "connections" | "conexiones" => Some(Self::Conexiones),
            _ => None,
        }
    }

    /// Nombre que muestra la Lectura.
    pub fn label(self) -> &'static str {
        match self {
            Self::Sintesis => "Síntesis",
            Self::Fuentes => "Fuentes",
            Self::Conexiones => "Conexiones",
        }
    }
}

/// Clases de codeblock que la vista completa con contenido generado (005).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GeneratedKind {
    /// Índice de las notas de una rama de etiquetas (036; el cuerpo lo lee
    /// [`crate::indice::parse`]).
    Indice,
    /// Tabla de contenidos automática de la nota (`toc`, `table-of-contents` o
    /// `tabla-de-contenidos`; opciones en
    /// [`crate::toc`]).
    Toc,
    /// Tarjeta de enlace externo (013): url, title, description, host, favicon, image.
    CardLink,
    /// Consulta viva (048): el cuerpo lo lee [`crate::consulta::parse`].
    Consulta,
    /// Consulta Dataview sin JS (065): `dataview` o `dv` ([`crate::dataview`]).
    Dataview,
}

impl GeneratedKind {
    /// Clase por lenguaje del codeblock; otro lenguaje no es un bloque generado.
    fn from_lang(lang: &str) -> Option<Self> {
        match lang {
            "indice" => Some(Self::Indice),
            "toc" | "table-of-contents" | "tabla-de-contenidos" => Some(Self::Toc),
            "cardlink" => Some(Self::CardLink),
            "consulta" => Some(Self::Consulta),
            "dataview" | "dv" => Some(Self::Dataview),
            _ => None,
        }
    }
}

/// Un codeblock generado (`indice`, `toc`, `consulta`, `cardlink`). Los parámetros YAML son
/// los de la tarjeta de enlace; los demás leen `raw`. Una tarjeta con YAML inválido queda como
/// [`BlockKind::Code`] común.
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
    /// La nota existe pero no tiene un bloque con ese id (`![[nota#^id]]`, 026).
    BlockMissing {
        path: crate::RelPath,
    },
    /// Una nota `.html`: no se incrusta en la vista formateada, se ofrece abrirla.
    Html {
        path: crate::RelPath,
    },
    /// Un archivo que no es nota (imagen, audio o video, 057): la vista lo muestra en el lugar.
    /// `abs`: la ruta absoluta en disco.
    Archivo {
        path: crate::RelPath,
        abs: std::path::PathBuf,
        tipo: crate::medios::Tipo,
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
    /// Una fórmula LaTeX (050): `$…$` en línea o `$$…$$` dentro de un párrafo con más texto.
    Math {
        tex: String,
        display: bool,
    },
    /// Un ícono vectorial en línea `:nombre:` (062). Si `nombre` no existe al dibujar, se muestra
    /// el texto literal `:nombre:`.
    Icon(String),
    /// Texto coloreado o resaltado con sintaxis propia `{color:texto}` / `==texto==` (063).
    /// `raw_spec` conserva el especificador original (`"=="` para resaltado rápido) para volver al
    /// texto literal si un nombre personalizado no existe.
    Styled {
        raw_spec: String,
        fg: Option<String>,
        bg: Option<String>,
        children: Vec<Inline>,
    },
    /// Expresión Dataview en línea `` `= expr` `` (065): se evalúa en Lectura contra el índice.
    DataviewExpr(String),
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
    line_starts: &'a [usize],
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
    ) {
        if run.is_empty() {
            return;
        }
        let run_start = run[0].1.start;
        let run_end = run.last().expect("corrida no vacía").1.end;
        // Corrida sin marcadores: los eventos originales pasan intactos, sin realocar. Los rangos
        // de `repl` ya son relativos al cuerpo (sin el frontmatter): no se resta `base` de nuevo.
        let hay = repl
            .iter()
            .any(|(r, _)| r.start < run_end && r.end > run_start);
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
            flush(&mut run, &mut out, &repl_rel);
            out.push((ev, range));
        }
    }
    flush(&mut run, &mut out, &repl_rel);
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
    let mut line_starts = vec![0];
    line_starts.extend(text.match_indices('\n').map(|(i, _)| i + 1));
    let mut tags = crate::tags::extract_tags(text, false);
    tags.retain(|tag| !tag.from_prop);
    let tarjetas = crate::tarjetas::regiones(body);
    let mut blocks = if tarjetas.is_empty() {
        parse_fragment(text, base, body, repl, &line_starts, tags)
    } else {
        // Las tarjetas (024) se analizan aparte: en el resto del texto quedan como renglones en
        // blanco (mismos bytes de largo, así los rangos no cambian).
        let mut limpio = body.as_bytes().to_vec();
        for r in &tarjetas {
            for b in &mut limpio[r.completa.clone()] {
                if *b != b'\n' {
                    *b = b' ';
                }
            }
        }
        let limpio = String::from_utf8(limpio).expect("los cortes caen en comienzos de renglón");
        let mut blocks = parse_fragment(text, base, &limpio, repl, &line_starts, tags.clone());
        let total = tarjetas.len();
        for (n, r) in tarjetas.into_iter().enumerate() {
            let parte = |rango: &Range<usize>| {
                let sus: Vec<_> = tags
                    .iter()
                    .filter(|t| {
                        t.byte_range.start >= base + rango.start
                            && t.byte_range.end <= base + rango.end
                    })
                    .cloned()
                    .collect();
                parse_fragment(
                    text,
                    base + rango.start,
                    &body[rango.clone()],
                    repl,
                    &line_starts,
                    sus,
                )
            };
            let source = base + r.completa.start..base + r.completa.end;
            let block = Block {
                kind: BlockKind::Flashcard(Flashcard {
                    id: r.id,
                    numero: n + 1,
                    total,
                    question: parte(&r.pregunta),
                    answer: parte(&r.respuesta),
                }),
                line: line_starts
                    .partition_point(|&s| s <= source.start)
                    .saturating_sub(1),
                source,
                id: None,
            };
            let at = blocks.partition_point(|b| b.source.start < block.source.start);
            blocks.insert(at, block);
        }
        blocks
    };
    agrupar_secciones(&mut blocks);
    RenderDoc { blocks }
}

/// Analiza el tramo `fragment` del texto, que empieza en el byte `base` de `text` (el cuerpo de la
/// nota o una parte de una tarjeta). Los marcadores de `repl` fuera del tramo se ignoran.
fn parse_fragment(
    text: &str,
    base: usize,
    fragment: &str,
    repl: &[(std::ops::Range<usize>, String)],
    line_starts: &[usize],
    mut tags: Vec<crate::tags::TagOccurrence>,
) -> Vec<Block> {
    let mut opts = Options::empty();
    opts.insert(Options::ENABLE_TABLES);
    opts.insert(Options::ENABLE_STRIKETHROUGH);
    opts.insert(Options::ENABLE_TASKLISTS);
    opts.insert(Options::ENABLE_FOOTNOTES);
    opts.insert(Options::ENABLE_MATH);
    let events: Vec<Ev> = Parser::new_ext(fragment, opts).into_offset_iter().collect();
    let end = base + fragment.len();
    let events = if repl.is_empty() {
        events
    } else if repl.iter().all(|(r, _)| r.start >= base && r.end <= end) {
        substitute_runs(events, base, repl)
    } else {
        let dentro: Vec<_> = repl
            .iter()
            .filter(|(r, _)| r.start >= base && r.end <= end)
            .cloned()
            .collect();
        substitute_runs(events, base, &dentro)
    };
    // Rangos excluidos fusionados: un solo barrido, sin recorrer las etiquetas por evento.
    let mut excluded: Vec<Range<usize>> = events
        .iter()
        .filter_map(|(event, range)| {
            matches!(
                event,
                Event::Start(Tag::Link { .. } | Tag::Image { .. } | Tag::CodeBlock(_))
                    | Event::Code(_)
                    | Event::InlineMath(_)
                    | Event::DisplayMath(_)
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
    let mut blocks = parse_blocks(&ctx, &events, &mut i, None);
    extraer_ids(&mut blocks);
    promover_formulas(&mut blocks);
    separar_incrustaciones(&ctx, &mut blocks);
    blocks
}

/// Un párrafo que es solo una fórmula `$$…$$` (con saltos o espacios alrededor) pasa a ser un
/// [`BlockKind::Math`] (050), también dentro de citas, callouts e ítems. Va después de
/// [`extraer_ids`]: el `^id` de la línea siguiente ya quedó en el bloque.
fn promover_formulas(blocks: &mut [Block]) {
    for b in blocks {
        match &mut b.kind {
            BlockKind::Paragraph(inlines) => {
                let mut solo = None;
                for inl in inlines.iter() {
                    match inl {
                        Inline::Math { tex, display: true } if solo.is_none() => solo = Some(tex),
                        Inline::SoftBreak | Inline::HardBreak => {}
                        Inline::Text(t) if t.trim().is_empty() => {}
                        _ => {
                            solo = None;
                            break;
                        }
                    }
                }
                if let Some(tex) = solo {
                    b.kind = BlockKind::Math { tex: tex.clone() };
                }
            }
            BlockKind::Quote(body) | BlockKind::Callout { body, .. } => promover_formulas(body),
            BlockKind::List { items, .. } => {
                for item in items {
                    promover_formulas(&mut item.blocks);
                }
            }
            _ => {}
        }
    }
}

/// Un párrafo con `![[…]]` entre otras cosas (varias incrustaciones en un renglón, o texto e
/// incrustaciones) pasa a ser un [`BlockKind::Group`]: cada `![[…]]` es su propia
/// [`BlockKind::Transclusion`] y lo que queda entre ellas, párrafos. Así cada imagen, audio,
/// video, nota o archivo faltante se ve igual que solo en su párrafo, sin restos de texto. El
/// párrafo que es solo `![[…]]` ya es una transclusión ([`transclusion`]). Va después de
/// [`extraer_ids`]: el `^id` queda en el grupo.
fn separar_incrustaciones(ctx: &Ctx, blocks: &mut [Block]) {
    for b in blocks {
        match &mut b.kind {
            BlockKind::Paragraph(inlines) => {
                if inlines
                    .iter()
                    .any(|i| matches!(i, Inline::Text(t) if t.contains("![[")))
                    && let Some(partes) = partes_con_incrustaciones(ctx, &b.source, inlines)
                {
                    b.kind = BlockKind::Group(partes);
                }
            }
            BlockKind::Quote(body)
            | BlockKind::Callout { body, .. }
            | BlockKind::Seccion { body, .. }
            | BlockKind::Group(body) => separar_incrustaciones(ctx, body),
            BlockKind::Flashcard(f) => {
                separar_incrustaciones(ctx, &mut f.question);
                separar_incrustaciones(ctx, &mut f.answer);
            }
            BlockKind::List { items, .. } => {
                for item in items {
                    separar_incrustaciones(ctx, &mut item.blocks);
                }
            }
            _ => {}
        }
    }
}

/// Las partes de un párrafo cortado en cada `![[…]]` de su texto (no en código ni en enlaces).
/// `None` si no hay ninguna incrustación válida.
fn partes_con_incrustaciones(
    ctx: &Ctx,
    source: &Range<usize>,
    inlines: &[Inline],
) -> Option<Vec<Block>> {
    let fuente = ctx.text.get(source.clone())?;
    let mut partes: Vec<Block> = Vec::new();
    let mut actual: Vec<Inline> = Vec::new();
    // Desde dónde se busca la próxima incrustación en el fuente y dónde empieza el tramo actual.
    let mut cursor = 0usize;
    let mut inicio_tramo = 0usize;
    let mut alguna = false;
    let cerrar = |actual: &mut Vec<Inline>, rango: Range<usize>, partes: &mut Vec<Block>| {
        let inl = recortar(std::mem::take(actual));
        if !inl.is_empty() {
            let start = source.start + rango.start;
            partes.push(Block {
                kind: BlockKind::Paragraph(inl),
                line: ctx.line(start),
                source: start..source.start + rango.end.max(rango.start),
                id: None,
            });
        }
    };
    for inl in inlines {
        let Inline::Text(t) = inl else {
            actual.push(inl.clone());
            continue;
        };
        let mut desde = 0;
        let mut buscar = 0;
        while let Some(r) = t[buscar..].find("![[") {
            let ini = buscar + r;
            let Some(fin_rel) = t[ini + 3..].find("]]") else {
                break;
            };
            let fin = ini + 3 + fin_rel + 2;
            let escrito = &t[ini..fin];
            let inner = &escrito[3..escrito.len() - 2];
            let target = crate::links::parse_link_target(inner);
            // Dónde está en el fuente (para el rango y el renglón); escapado (`\![[`), es texto.
            let en_fuente = fuente[cursor..].find(escrito).map(|p| cursor + p);
            let valida = !inner.contains("[[")
                && !inner.contains('\n')
                && !target.path.is_empty()
                && en_fuente.is_some_and(|p| p == 0 || fuente.as_bytes()[p - 1] != b'\\');
            let Some(pos) = en_fuente.filter(|_| valida) else {
                buscar = ini + 3;
                continue;
            };
            if ini > desde {
                actual.push(Inline::Text(t[desde..ini].to_owned()));
            }
            cerrar(&mut actual, inicio_tramo..pos, &mut partes);
            let start = source.start + pos;
            partes.push(Block {
                kind: BlockKind::Transclusion(Transclusion {
                    target: target.path,
                    anchor: target.anchor.filter(|a| !a.is_empty()),
                    content: Embed::Pending,
                }),
                line: ctx.line(start),
                source: start..start + escrito.len(),
                id: None,
            });
            alguna = true;
            cursor = pos + escrito.len();
            inicio_tramo = cursor;
            desde = fin;
            buscar = fin;
        }
        if desde < t.len() {
            actual.push(Inline::Text(t[desde..].to_owned()));
        }
    }
    if !alguna {
        return None;
    }
    cerrar(&mut actual, inicio_tramo..fuente.len(), &mut partes);
    Some(partes)
}

/// Un tramo de párrafo sin los saltos y espacios del principio y del final. Vacío si no tiene
/// nada más que eso.
fn recortar(mut inl: Vec<Inline>) -> Vec<Inline> {
    let blanco = |i: &Inline| match i {
        Inline::SoftBreak | Inline::HardBreak => true,
        Inline::Text(t) => t.trim().is_empty(),
        _ => false,
    };
    while inl.last().is_some_and(blanco) {
        inl.pop();
    }
    let primero = inl.iter().position(|i| !blanco(i)).unwrap_or(inl.len());
    inl.drain(..primero);
    if let Some(Inline::Text(t)) = inl.first_mut() {
        let sin = t.trim_start().len();
        t.drain(..t.len() - sin);
    }
    if let Some(Inline::Text(t)) = inl.last_mut() {
        t.truncate(t.trim_end().len());
    }
    inl
}

/// Un `$` de cierre seguido de un dígito no cierra una fórmula en línea (050): así `$5,$10` sigue
/// siendo dinero. pulldown-cmark solo exige que la apertura no vaya seguida de un espacio y que el
/// cierre no vaya precedido de uno.
pub(crate) fn cierre_con_digito(text: &str, rango: &Range<usize>) -> bool {
    text.as_bytes()
        .get(rango.end)
        .is_some_and(u8::is_ascii_digit)
}

/// Junta cada `<!-- summary|sources|connections ID -->` de primer nivel con su contenido y su
/// cierre `<!---->` en un [`BlockKind::Seccion`] (041). pulldown-cmark da los comentarios como
/// bloques HTML sueltos; una sección sin cierre llega hasta la próxima apertura o el final.
/// Sin secciones no se mueve nada (un solo recorrido que solo mira los bloques HTML).
fn agrupar_secciones(blocks: &mut Vec<Block>) {
    let abre = |b: &Block| match &b.kind {
        BlockKind::Html(t) => t.lines().next().and_then(SeccionKind::apertura),
        _ => None,
    };
    if !blocks.iter().any(|b| abre(b).is_some()) {
        return;
    }
    let es_cierre = |b: &Block| matches!(&b.kind, BlockKind::Html(t) if t.trim() == "<!---->");
    let es_apertura_de_bloque = |b: &Block| matches!(&b.kind, BlockKind::Html(t) if t.trim_start().starts_with("<!-- block"));
    let viejos = std::mem::take(blocks);
    let mut it = viejos.into_iter().peekable();
    while let Some(b) = it.next() {
        let Some(kind) = abre(&b) else {
            blocks.push(b);
            continue;
        };
        let mut source = b.source.clone();
        let mut body = Vec::new();
        while let Some(sig) = it.peek() {
            if abre(sig).is_some() || es_apertura_de_bloque(sig) {
                break;
            }
            let sig = it.next().expect("peek");
            source.end = sig.source.end;
            if es_cierre(&sig) {
                break;
            }
            body.push(sig);
        }
        blocks.push(Block {
            kind: BlockKind::Seccion { kind, body },
            source,
            line: b.line,
            id: None,
        });
    }
}

/// Saca los identificadores de bloque (`^id`, 026) del texto que se muestra y los guarda en cada
/// bloque: ` ^id` al final de un párrafo (también dentro de ítems, citas y callouts) y los párrafos
/// que son solo `^id`, que pasan su id al bloque anterior (tablas, código).
fn extraer_ids(blocks: &mut Vec<Block>) {
    let mut n = 0;
    while n < blocks.len() {
        let b = &mut blocks[n];
        match &mut b.kind {
            BlockKind::Paragraph(inlines) => {
                if let [Inline::Text(t)] = inlines.as_slice()
                    && let Some(id) = solo_id(t)
                {
                    if n > 0 {
                        blocks[n - 1].id = Some(id);
                    }
                    blocks.remove(n);
                    continue;
                }
                if let Some(Inline::Text(t)) = inlines.last_mut()
                    && let Some(r) = crate::bloques::id_al_final(t, 0)
                {
                    b.id = Some(t[r.clone()].to_owned());
                    let corte = t[..r.start - 1].trim_end().len();
                    t.truncate(corte);
                    if t.is_empty() {
                        inlines.pop();
                        // `^id` solo en la última línea del párrafo: se va también el salto.
                        if matches!(inlines.last(), Some(Inline::SoftBreak | Inline::HardBreak)) {
                            inlines.pop();
                        }
                    }
                }
            }
            BlockKind::Quote(body) | BlockKind::Callout { body, .. } => {
                extraer_ids(body);
                // El id del último párrafo de una cita o un callout es el de todo el bloque.
                if let Some(id) = body.last().and_then(|p| p.id.clone()) {
                    b.id = Some(id);
                }
            }
            BlockKind::List { items, .. } => {
                for item in items {
                    extraer_ids(&mut item.blocks);
                }
            }
            // En GFM, una línea `^id` pegada a la tabla es su última fila: una sola celda con el id.
            BlockKind::Table { rows, .. } => {
                if let Some(fila) = rows.last()
                    && let Some(id) = fila_de_id(fila)
                {
                    b.id = Some(id);
                    rows.pop();
                }
            }
            _ => {}
        }
        n += 1;
    }
}

/// Una fila de tabla cuya primera celda es solo `^id` y las demás están vacías.
fn fila_de_id(fila: &[Vec<Inline>]) -> Option<String> {
    let (primera, resto) = fila.split_first()?;
    if !resto.iter().all(Vec::is_empty) {
        return None;
    }
    match primera.as_slice() {
        [Inline::Text(t)] => solo_id(t),
        _ => None,
    }
}

/// Un texto que es solo `^id`.
fn solo_id(t: &str) -> Option<String> {
    let t = t.trim();
    let id = t.strip_prefix('^')?;
    (!id.is_empty() && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-'))
        .then(|| id.to_owned())
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
                if crate::galeria::es_lenguaje(&lang) {
                    out.push(Block {
                        kind: BlockKind::Galeria(Box::new(crate::galeria::parse(&text))),
                        source: abs,
                        line,
                        id: None,
                    });
                    continue;
                }
                if crate::carrusel::es_lenguaje(&lang) {
                    out.push(Block {
                        kind: BlockKind::Carrusel(Box::new(crate::carrusel::parse(&text))),
                        source: abs,
                        line,
                        id: None,
                    });
                    continue;
                }
                if lang == "math" {
                    out.push(Block {
                        kind: BlockKind::Math { tex: text },
                        source: abs,
                        line,
                        id: None,
                    });
                    continue;
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
            id: None,
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
                    // Como en los párrafos: texto contiguo junto y wikilinks expandidos (en un
                    // ítem «compacto» `[[x]]` también es un enlace).
                    kind: BlockKind::Paragraph(collapse_and_expand_inlines(std::mem::take(loose))),
                    id: None,
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
                // En una lista suelta (renglón vacío entre ítems) la casilla llega como primer
                // evento del primer párrafo del ítem.
                if task.is_none()
                    && blocks.is_empty()
                    && loose_start.is_none()
                    && matches!(ev, Event::Start(Tag::Paragraph))
                    && let Some((Event::TaskListMarker(checked), r)) = evs.get(*i + 1)
                {
                    task = Some(TaskMark {
                        checked: *checked,
                        offset: ctx.abs(r).start + 1,
                    });
                }
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
/// generado (el codeblock queda común): YAML inválido en una tarjeta de enlace.
fn parse_generated(kind: GeneratedKind, body: &str) -> Option<Vec<crate::props::PropEntry>> {
    // La tabla de contenidos lee su cuerpo línea por línea ([`crate::toc::parse_options`]), con
    // comentarios y valores vacíos que no siempre son YAML válido: siempre es generada.
    // La consulta (048) y el índice por etiqueta (036, [`crate::indice::parse`]) tampoco son
    // YAML: un error de su cuerpo se muestra en la vista.
    if matches!(
        kind,
        GeneratedKind::Toc
            | GeneratedKind::Consulta
            | GeneratedKind::Indice
            | GeneratedKind::Dataview
    ) {
        return Some(Vec::new());
    }
    crate::props::yaml::parse_block(body.trim()).ok()
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
            Inline::Text(t) => {
                let limpio = clean_inline_fields(&t);
                for piece in expand_text_wikilinks(&limpio) {
                    match piece {
                        Inline::Text(plain) => {
                            for st in expand_text_styled(&plain) {
                                out.push(expand_icons_in_inline(st));
                            }
                        }
                        other => out.push(other),
                    }
                }
            }
            Inline::Strong(c) => out.push(Inline::Strong(collapse_and_expand_inlines(c))),
            Inline::Emph(c) => out.push(Inline::Emph(collapse_and_expand_inlines(c))),
            Inline::Strike(c) => out.push(Inline::Strike(collapse_and_expand_inlines(c))),
            Inline::Link { url, children } => out.push(Inline::Link {
                url,
                children: collapse_and_expand_inlines(children),
            }),
            Inline::Styled {
                raw_spec,
                fg,
                bg,
                children,
            } => out.push(Inline::Styled {
                raw_spec,
                fg,
                bg,
                children: collapse_and_expand_inlines(children),
            }),
            other => out.push(other),
        }
    }
    expand_cross_inline_styled(out)
}

fn expand_icons_in_inline(inl: Inline) -> Inline {
    match inl {
        Inline::Text(t) => {
            let mut v = expand_text_icons(&t);
            if v.len() == 1 {
                v.pop().unwrap()
            } else {
                Inline::Styled {
                    raw_spec: String::new(),
                    fg: None,
                    bg: None,
                    children: v,
                }
            }
        }
        Inline::Styled {
            raw_spec,
            fg,
            bg,
            children,
        } => {
            let mut flat = Vec::with_capacity(children.len());
            for c in children {
                match c {
                    Inline::Text(t) => flat.extend(expand_text_icons(&t)),
                    other => flat.push(expand_icons_in_inline(other)),
                }
            }
            Inline::Styled {
                raw_spec,
                fg,
                bg,
                children: flat,
            }
        }
        other => other,
    }
}

/// Expande `{spec:texto}` y `==texto==` dentro de un tramo de texto plano (063).
fn expand_text_styled(text: &str) -> Vec<Inline> {
    if !text.contains('{') && !text.contains("==") {
        return vec![Inline::Text(text.to_owned())];
    }
    let bytes = text.as_bytes();
    let mut out = Vec::new();
    let mut emitted = 0;
    let mut i = 0;
    while i < bytes.len() {
        // 1. Resaltado rápido `==…==`
        if i + 4 <= bytes.len()
            && bytes[i] == b'='
            && bytes[i + 1] == b'='
            && (i == 0 || bytes[i - 1] != b'\\')
            && bytes[i + 2] != b'='
            && bytes[i + 2] != b' '
            && let Some(rel) = text[i + 2..].find("==")
        {
            let inner = &text[i + 2..i + 2 + rel];
            if !inner.is_empty() && !inner.ends_with(' ') && !inner.contains('\n') {
                if i > emitted {
                    out.push(Inline::Text(text[emitted..i].to_owned()));
                }
                out.push(Inline::Styled {
                    raw_spec: "==".to_owned(),
                    fg: None,
                    bg: Some("==".to_owned()),
                    children: vec![Inline::Text(inner.to_owned())],
                });
                emitted = i + 2 + rel + 2;
                i = emitted;
                continue;
            }
        }
        // 2. Color / fondo `{spec:texto}`
        if bytes[i] == b'{'
            && (i == 0 || bytes[i - 1] != b'\\')
            && let Some(colon_rel) = text[i + 1..].find(':')
        {
            let colon = i + 1 + colon_rel;
            let spec = &text[i + 1..colon];
            if !spec.contains('}')
                && !spec.contains('{')
                && !spec.contains('\n')
                && let Some(estilo) = crate::colores::parse_spec(spec)
                && let Some(close_rel) = text[colon + 1..].find('}')
            {
                let close = colon + 1 + close_rel;
                let inner = &text[colon + 1..close];
                if !inner.is_empty() && !inner.contains("\n\n") {
                    if i > emitted {
                        out.push(Inline::Text(text[emitted..i].to_owned()));
                    }
                    out.push(Inline::Styled {
                        raw_spec: spec.to_owned(),
                        fg: estilo.fg,
                        bg: estilo.bg,
                        children: vec![Inline::Text(inner.to_owned())],
                    });
                    emitted = close + 1;
                    i = emitted;
                    continue;
                }
            }
        }
        i += 1;
    }
    if emitted < text.len() {
        out.push(Inline::Text(text[emitted..].to_owned()));
    }
    out
}

/// Resuelve también casos donde el texto coloreado envuelve formato Markdown (`{rosa:**negrita**}`
/// o `==**negrita**==`) y aplana envoltorios vacíos de `expand_icons_in_inline`.
fn expand_cross_inline_styled(items: Vec<Inline>) -> Vec<Inline> {
    let mut flat = Vec::with_capacity(items.len());
    for it in items {
        match it {
            Inline::Styled {
                raw_spec,
                fg: None,
                bg: None,
                children,
            } if raw_spec.is_empty() => flat.extend(children),
            other => flat.push(other),
        }
    }
    // Buscar `{spec:` al final de un Inline::Text y `}` al inicio de un Inline::Text posterior.
    let mut out = Vec::with_capacity(flat.len());
    let mut i = 0;
    while i < flat.len() {
        if let Inline::Text(t) = &flat[i]
            && let Some(open) = t.rfind('{')
            && (open == 0 || t.as_bytes()[open - 1] != b'\\')
            && let Some(rest) = t[open + 1..].strip_suffix(':')
            && let Some(estilo) = crate::colores::parse_spec(rest)
            && let Some((close_idx, close_pos)) =
                flat[i + 1..].iter().enumerate().find_map(|(k, n)| {
                    if let Inline::Text(nt) = n
                        && let Some(pos) = nt.find('}')
                    {
                        return Some((i + 1 + k, pos));
                    }
                    None
                })
        {
            if open > 0 {
                out.push(Inline::Text(t[..open].to_owned()));
            }
            let mut inner: Vec<Inline> = flat[i + 1..close_idx].to_vec();
            let mut tail = String::new();
            if let Inline::Text(nt) = &flat[close_idx] {
                if close_pos > 0 {
                    inner.push(Inline::Text(nt[..close_pos].to_owned()));
                }
                if close_pos + 1 < nt.len() {
                    tail.push_str(&nt[close_pos + 1..]);
                }
            }
            out.push(Inline::Styled {
                raw_spec: rest.to_owned(),
                fg: estilo.fg,
                bg: estilo.bg,
                children: inner,
            });
            if !tail.is_empty() {
                out.push(Inline::Text(tail));
            }
            i = close_idx + 1;
            continue;
        }
        out.push(flat[i].clone());
        i += 1;
    }
    out
}

/// Expande `:nombre:` en nodos `Inline::Icon(nombre)` (062). Evita falsos positivos en horas
/// (`12:30:00`), campos inline (`clave:: valor`) o URLs.
fn expand_text_icons(text: &str) -> Vec<Inline> {
    if !text.as_bytes().contains(&b':') {
        return vec![Inline::Text(text.to_owned())];
    }
    let bytes = text.as_bytes();
    let mut out = Vec::new();
    let mut emitted = 0;
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b':' {
            let prev_ok = i == 0
                || !matches!(bytes[i - 1], b':' | b'/' | b'\\')
                    && !text[..i]
                        .chars()
                        .next_back()
                        .is_some_and(char::is_alphanumeric);
            if prev_ok {
                let start = i + 1;
                let mut end = start;
                while end < bytes.len()
                    && (bytes[end].is_ascii_lowercase()
                        || bytes[end].is_ascii_digit()
                        || bytes[end] == b'-'
                        || bytes[end] == b'_')
                {
                    end += 1;
                }
                let len = end - start;
                if (2..=48).contains(&len)
                    && bytes[start].is_ascii_alphabetic()
                    && end < bytes.len()
                    && bytes[end] == b':'
                {
                    let after = end + 1;
                    let next_ok = after == bytes.len()
                        || !matches!(bytes[after], b':' | b'/')
                            && !text[after..]
                                .chars()
                                .next()
                                .is_some_and(char::is_alphanumeric);
                    if next_ok {
                        if i > emitted {
                            out.push(Inline::Text(text[emitted..i].to_owned()));
                        }
                        out.push(Inline::Icon(text[start..end].to_owned()));
                        emitted = after;
                        i = after;
                        continue;
                    }
                }
            }
        }
        i += 1;
    }
    if emitted < text.len() {
        out.push(Inline::Text(text[emitted..].to_owned()));
    }
    out
}

/// Limpia campos inline `[clave:: valor]` (muestra `clave: valor`) y `(clave:: valor)` (oculta la
/// clave y muestra solo `valor`) para Lectura (065), sin modificar el archivo en disco.
fn clean_inline_fields(text: &str) -> std::borrow::Cow<'_, str> {
    if !text.contains("::") {
        return std::borrow::Cow::Borrowed(text);
    }
    let bytes = text.as_bytes();
    let mut out = String::with_capacity(text.len());
    let mut emitted = 0;
    let mut i = 0;
    while i < bytes.len() {
        let open = bytes[i];
        let close = match open {
            b'(' => b')',
            b'[' => b']',
            _ => {
                i += 1;
                continue;
            }
        };
        // Evitar `[[...]]`, `![[...]]` y aperturas escapadas `\(`.
        if (i > 0 && bytes[i - 1] == b'\\')
            || (open == b'[' && bytes.get(i + 1) == Some(&b'['))
            || (open == b'[' && i > 0 && bytes[i - 1] == b'[')
        {
            i += 1;
            continue;
        }
        if let Some(sep_rel) = text[i + 1..].find("::") {
            let sep = i + 1 + sep_rel;
            let clave = text[i + 1..sep].trim();
            let clave_ok = !clave.is_empty()
                && clave.len() <= 64
                && !clave.contains(['\n', '[', ']', '(', ')', ':'])
                && clave
                    .chars()
                    .next()
                    .is_some_and(|c| c.is_alphabetic() || c == '_');
            if clave_ok {
                // Buscar el cierre correspondiente respetando anidación (p. ej. `[autor:: [[Ana]]]`).
                let mut depth = 1usize;
                let mut j = sep + 2;
                let mut fin = None;
                while j < bytes.len() {
                    let b = bytes[j];
                    if b == b'\n' {
                        break;
                    }
                    if b == open {
                        depth += 1;
                    } else if b == close {
                        depth -= 1;
                        if depth == 0 {
                            fin = Some(j);
                            break;
                        }
                    }
                    j += 1;
                }
                if let Some(end) = fin {
                    let valor = text[sep + 2..end].trim();
                    if !valor.is_empty() {
                        out.push_str(&text[emitted..i]);
                        if open == b'[' {
                            out.push_str(clave);
                            out.push_str(": ");
                        }
                        out.push_str(valor);
                        emitted = end + 1;
                        i = emitted;
                        continue;
                    }
                }
            }
        }
        i += 1;
    }
    if emitted == 0 {
        std::borrow::Cow::Borrowed(text)
    } else {
        out.push_str(&text[emitted..]);
        std::borrow::Cow::Owned(out)
    }
}

/// Expande texto plano separando los wikilinks `[[...]]` en nodos `Inline::Link`.
fn expand_text_wikilinks(text: &str) -> Vec<Inline> {
    let mut out = Vec::new();
    // `search_from`: desde dónde falta volcar el texto; `scan`: desde dónde se busca el próximo
    // `[[`. Un `![[…]]` (incrustación) se saltea pero queda en el texto: antes se perdía junto
    // con lo anterior (`![[a.png]] ![[b.png]]` dejaba solo «b.png]]»).
    let mut search_from = 0;
    let mut scan = 0;

    while let Some(rel_start) = text[scan..].find("[[") {
        let start_idx = scan + rel_start;
        if start_idx > 0 && text.as_bytes()[start_idx - 1] == b'!' {
            scan = text[start_idx + 2..]
                .find("]]")
                .map_or(start_idx + 2, |r| start_idx + 2 + r + 2);
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
            scan = end_idx;
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
        Event::Code(t) => {
            if let Some(rest) = t.strip_prefix('=') {
                let valido = rest.starts_with(' ')
                    || (!rest.is_empty() && !rest.starts_with(['=', '>', '<', '!']));
                let expr = rest.trim();
                if valido && !expr.is_empty() {
                    return vec![Inline::DataviewExpr(expr.to_owned())];
                }
            }
            vec![Inline::Code(t.to_string())]
        }
        Event::InlineHtml(t) | Event::Html(t) => vec![Inline::Text(t.to_string())],
        Event::FootnoteReference(t) => vec![Inline::FootnoteRef(t.to_string())],
        Event::SoftBreak => vec![Inline::SoftBreak],
        Event::HardBreak => vec![Inline::HardBreak],
        Event::InlineMath(t) => {
            let abs = ctx.abs(range);
            if cierre_con_digito(ctx.text, &abs) {
                vec![Inline::Text(ctx.text[abs].to_owned())]
            } else {
                vec![Inline::Math {
                    tex: t.to_string(),
                    display: false,
                }]
            }
        }
        Event::DisplayMath(t) => vec![Inline::Math {
            tex: t.to_string(),
            display: true,
        }],
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
            | Inline::Link { children: c, .. }
            | Inline::Styled { children: c, .. } => {
                s.push_str(&plain_text(c));
            }
            Inline::Image { alt, .. } => s.push_str(alt),
            Inline::SoftBreak => s.push(' '),
            Inline::HardBreak => s.push('\n'),
            Inline::Math { tex, display } => {
                let d = if *display { "$$" } else { "$" };
                s.push_str(d);
                s.push_str(tex);
                s.push_str(d);
            }
            Inline::Icon(nombre) => {
                s.push(':');
                s.push_str(nombre);
                s.push(':');
            }
            Inline::DataviewExpr(expr) => {
                s.push_str("= ");
                s.push_str(expr);
            }
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
