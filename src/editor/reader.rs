//! Vista formateada de las notas `.md` (Lectura y la mitad derecha de Dividida), nativa (research
//! R2 y R3 de la 002).
//!
//! El `GtkTextView` va directo en su `GtkScrolledWindow` (si no, GTK calcula todo el documento en
//! cada cambio). Casi todo es texto con etiquetas: tablas con tabulaciones, callouts con un glifo y
//! separadores como una franja. Solo son widgets anclados las propiedades, las casillas, las
//! imágenes y el botón de los callouts plegables: cada widget anclado cuesta, y con miles la vista
//! tardaba segundos en dibujarse.
//!
//! Al abrir una nota el documento se arma en un buffer nuevo, por tramos de a lo sumo ~8 ms. Al
//! editar (Dividida) solo se reemplazan los bloques de primer nivel que cambiaron: se comparan por
//! hash el prefijo y el sufijo comunes.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::{Duration, Instant};

use gtk::prelude::*;
use gtk::{glib, pango};
use orbital_vault::render::{
    Align, Block, BlockKind, CalloutKind, Inline, ListItem, RenderDoc, plain_text,
};

/// Tiempo máximo de volcado por tramo, para no pasar el cuadro de 16 ms.
const SLICE: Duration = Duration::from_millis(8);
/// Ancho máximo de la columna de texto.
const COLUMN: i32 = 760;
/// Con más bloques cambiados que esto, se rearma todo por tramos en lugar de reemplazar en el sitio.
const MAX_IN_PLACE: usize = 48;
/// Alcance en caracteres desde lo visible para anclar widgets pendientes.
const WINDOW_CHARS: i32 = 6000;
/// Widgets anclados por vuelta del bucle principal.
const ATTACH_BATCH: usize = 8;

type ToggleFn = Box<dyn Fn(usize)>;
type LinkFn = Box<dyn Fn(&str)>;
type IsMissingFn = Rc<dyn Fn(&str) -> bool>;
type RenderedFn = Box<dyn Fn()>;

#[derive(Clone)]
struct LinkItem {
    url: String,
    start: gtk::TextMark,
    end: gtk::TextMark,
}

/// Lo que la vista guarda de cada bloque de primer nivel.
struct BlockInfo {
    hash: u64,
    /// Línea del texto fuente donde empieza (para el desplazamiento sincronizado).
    line: usize,
    /// Inicio del bloque en la vista (gravedad izquierda).
    mark: gtk::TextMark,
    /// Byte de inicio del bloque en el texto fuente; cambia si se edita algo antes.
    base: Rc<Cell<usize>>,
    /// Etiquetas propias del bloque (enlaces, plegado, tablas): se quitan con el bloque.
    tags: Vec<gtk::TextTag>,
    /// Texto plano del encabezado si el bloque es un título.
    heading: Option<String>,
    /// Enlaces contenidos en el bloque (para navegación por teclado).
    links: Vec<LinkItem>,
}

struct Inner {
    view: gtk::TextView,
    scroller: gtk::ScrolledWindow,
    /// Cada volcado nuevo cancela el anterior.
    generation: Cell<u64>,
    blocks: RefCell<Vec<BlockInfo>>,
    /// Hash del frontmatter mostrado arriba (`None`: sin mostrar todavía).
    header: Cell<Option<u64>>,
    links: RefCell<HashMap<gtk::TextTag, String>>,
    focused_link: Cell<Option<usize>>,
    /// Etiquetas con sangría: (etiqueta, sangría extra en píxeles).
    indents: RefCell<Vec<(gtk::TextTag, i32)>>,
    side: Cell<i32>,
    /// Ancho disponible para el texto (columna menos márgenes).
    width: Cell<i32>,
    /// Widgets anclados que ocupan el ancho de la columna (propiedades).
    wide: RefCell<Vec<glib::WeakRef<gtk::Widget>>>,
    /// Widgets que todavía no se anclaron: se anclan al acercarse a la parte visible. Anclar
    /// cientos de una vez congelaba la ventana medio segundo.
    pending: RefCell<Vec<(gtk::Widget, gtk::TextChildAnchor)>>,
    /// Línea del texto fuente a mostrar cuando termine el volcado en curso (al abrir una nota
    /// desde un retroenlace).
    pending_line: Cell<Option<usize>>,
    attach_scheduled: Cell<bool>,
    on_toggle: RefCell<Option<ToggleFn>>,
    on_link: RefCell<Option<LinkFn>>,
    is_missing: RefCell<Option<IsMissingFn>>,
    on_rendered: RefCell<Option<RenderedFn>>,
}

#[derive(Clone)]
pub struct Reader(Rc<Inner>);

impl Default for Reader {
    fn default() -> Self {
        Self::new()
    }
}

/// Documento listo para volcar: se prepara fuera del hilo principal.
pub struct Prepared {
    pub doc: RenderDoc,
    /// Hash del texto fuente de cada bloque de primer nivel.
    pub hashes: Vec<u64>,
    /// Hash del frontmatter.
    pub header: u64,
    /// Notas incrustadas con `![[…]]`: si una cambia, hay que volver a dibujar (US5).
    pub deps: Vec<orbital_vault::RelPath>,
}

/// Completa los codeblocks generados del doc (005, T015 y T019): `toc` con los encabezados del
/// propio doc y `indice` con las notas de cada tag desde el índice. Reemplaza cada bloque
/// manteniendo su rango fuente (el codeblock completo) y actualiza su hash, así la actualización
/// en el sitio ve el contenido nuevo aunque el fuente no cambie. Devuelve true si completó algo.
pub fn complete_generated(
    doc: &mut RenderDoc,
    hashes: &mut [u64],
    index: Option<&orbital_vault::SearchIndex>,
    path: Option<&orbital_vault::RelPath>,
) -> bool {
    let encabezados = headings_flat(&doc.blocks);
    let mut completado = false;
    for (n, block) in doc.blocks.iter_mut().enumerate() {
        let BlockKind::Generated(g) = &block.kind else {
            continue;
        };
        let items = match g.kind {
            orbital_vault::render::GeneratedKind::Toc => {
                let (min, max) = niveles(&g.params);
                toc_items(&encabezados, path, min, max)
            }
            orbital_vault::render::GeneratedKind::Indice => {
                let tag = g
                    .params
                    .iter()
                    .find(|e| e.key == "tag")
                    .and_then(|e| e.value.as_text())
                    .map(str::to_owned);
                match (tag, index) {
                    (Some(tag), Some(index)) => {
                        let fold_tag = orbital_vault::fold(tag.trim_start_matches('#'));
                        let tree = index.tags();
                        match nodo_del_tag(&tree, &fold_tag) {
                            Some(node) => indice_items(index, node),
                            None => Vec::new(),
                        }
                    }
                    _ => Vec::new(),
                }
            }
        };
        let nuevo = if items.is_empty() {
            let tag = g
                .params
                .iter()
                .find(|e| e.key == "tag")
                .and_then(|e| e.value.as_text())
                .map(|t| t.trim_start_matches('#').to_owned())
                .unwrap_or_default();
            BlockKind::Paragraph(vec![Inline::Text(format!("«sin notas con #{tag}»"))])
        } else {
            BlockKind::List { start: None, items }
        };
        block.kind = nuevo;
        if let Some(h) = hashes.get_mut(n) {
            *h = orbital_vault::write::hash(format!("{:?}", block.kind).as_bytes());
        }
        completado = true;
    }
    completado
}

/// Encabezados de primer nivel (y dentro de citas), en orden del documento.
fn headings_flat(blocks: &[Block]) -> Vec<(u8, String)> {
    let mut out = Vec::new();
    fn walk(out: &mut Vec<(u8, String)>, blocks: &[Block]) {
        for b in blocks {
            match &b.kind {
                BlockKind::Heading { level, inlines } => {
                    out.push((*level, plain_text(inlines)));
                }
                BlockKind::Quote(body) => walk(out, body),
                _ => {}
            }
        }
    }
    walk(&mut out, blocks);
    out
}

/// `minLevel` y `maxLevel` del codeblock `toc`; fuera de rango o inválido: sin límite.
fn niveles(params: &[orbital_vault::props::PropEntry]) -> (u8, u8) {
    let nivel = |key: &str, defecto: u8| -> u8 {
        params
            .iter()
            .find(|e| e.key == key)
            .and_then(|e| e.value.as_text())
            .and_then(|v| v.trim().parse::<u8>().ok())
            .unwrap_or(defecto)
    };
    (nivel("minLevel", 0), nivel("maxLevel", 6).min(6))
}

/// Ítems del TOC: enlaces al encabezado de la propia nota, anidados por nivel.
fn toc_items(
    encabezados: &[(u8, String)],
    path: Option<&orbital_vault::RelPath>,
    min: u8,
    max: u8,
) -> Vec<ListItem> {
    let Some(path) = path else {
        return Vec::new();
    };
    // Apilar por nivel: cada encabezado entra en la lista de su nivel; los niveles menores
    // cerradas se agregan como lista anidada en el ítem anterior.
    struct Pila {
        nivel: u8,
        items: Vec<ListItem>,
    }
    let mut pilas: Vec<Pila> = Vec::new();
    for (nivel, titulo) in encabezados {
        if *nivel < min || *nivel > max || titulo.is_empty() {
            continue;
        }
        let item = ListItem {
            task: None,
            blocks: vec![Block {
                line: 0,
                source: 0..0,
                kind: BlockKind::Paragraph(vec![Inline::Link {
                    url: format!("/{}#{}", ruta_sin_wiki(path), titulo),
                    children: vec![Inline::Text(titulo.clone())],
                }]),
            }],
        };
        while pilas.last().is_some_and(|p| p.nivel >= *nivel) {
            let cerrada = pilas.pop().expect("verificada no vacía");
            let lista = Block {
                line: 0,
                source: 0..0,
                kind: BlockKind::List {
                    start: None,
                    items: cerrada.items,
                },
            };
            match pilas.last_mut() {
                Some(padre) => match &mut padre.items.last_mut() {
                    Some(ultimo) => ultimo.blocks.push(lista),
                    None => padre.items.push(ListItem {
                        task: None,
                        blocks: vec![lista],
                    }),
                },
                None => pilas.push(Pila {
                    nivel: *nivel,
                    items: vec![item.clone()],
                }),
            }
        }
        match pilas.last_mut() {
            Some(p) if p.nivel < *nivel => p.items.push(item),
            _ => pilas.push(Pila {
                nivel: *nivel,
                items: vec![item],
            }),
        }
    }
    while pilas.len() > 1 {
        let cerrada = pilas.pop().expect("verificada no vacía");
        let lista = Block {
            line: 0,
            source: 0..0,
            kind: BlockKind::List {
                start: None,
                items: cerrada.items,
            },
        };
        if let Some(ultimo) = pilas.last_mut().and_then(|p| p.items.last_mut()) {
            ultimo.blocks.push(lista);
        }
    }
    pilas.pop().map(|p| p.items).unwrap_or_default()
}

/// `wiki/x.md` -> `x.md`: las URLs `/x.md` se resuelven desde la raíz de `wiki/`.
fn ruta_sin_wiki(path: &orbital_vault::RelPath) -> &str {
    path.as_str().strip_prefix("wiki/").unwrap_or(path.as_str())
}

/// Ítems del índice: notas propias del tag (alfabéticas) y debajo, anidado, cada hija con sus
/// notas. La fila del tag abre la búsqueda por etiqueta (como el panel).
fn indice_items(
    index: &orbital_vault::SearchIndex,
    node: &orbital_vault::TagNode,
) -> Vec<ListItem> {
    let mut items: Vec<ListItem> = index
        .notes_with_tag(&node.path)
        .into_iter()
        .map(|p| ListItem {
            task: None,
            blocks: vec![nota_item(&p)],
        })
        .collect();
    for child in &node.children {
        let hijos = indice_items(index, child);
        if hijos.is_empty() {
            continue;
        }
        let mut blocks = vec![Block {
            line: 0,
            source: 0..0,
            kind: BlockKind::Paragraph(vec![Inline::Link {
                url: format!("orbital-tag:{}", child.path),
                children: vec![Inline::Text(format!("#{}", child.name))],
            }]),
        }];
        blocks.push(Block {
            line: 0,
            source: 0..0,
            kind: BlockKind::List {
                start: None,
                items: hijos,
            },
        });
        items.push(ListItem { task: None, blocks });
    }
    items
}

/// La fila de una nota del índice: enlace a `/ruta` con el nombre del archivo.
fn nota_item(path: &orbital_vault::RelPath) -> Block {
    Block {
        line: 0,
        source: 0..0,
        kind: BlockKind::Paragraph(vec![Inline::Link {
            url: format!("/{}", ruta_sin_wiki(path)),
            children: vec![Inline::Text(path.file_name().to_owned())],
        }]),
    }
}

/// El nodo del subárbol con esta ruta plegada, si existe.
fn nodo_del_tag<'t>(
    tree: &'t orbital_vault::TagTree,
    path: &str,
) -> Option<&'t orbital_vault::TagNode> {
    let mut actuales: &[orbital_vault::TagNode] = &tree.roots;
    let mut restante = path;
    loop {
        let (tramo, resto) = restante.split_once('/').unwrap_or((restante, ""));
        let node = actuales
            .iter()
            .find(|n| orbital_vault::fold(&n.name) == tramo)?;
        if resto.is_empty() {
            return Some(node);
        }
        actuales = &node.children;
        restante = resto;
    }
}

impl Prepared {
    /// Con las propiedades del frontmatter para resolver los marcadores `[%clave]` (005).
    pub fn new(text: &str, yaml: &[orbital_vault::props::PropEntry]) -> Self {
        Self::build(text, None, yaml)
    }

    /// Como [`Prepared::new`], con las transclusiones expandidas (lee las notas incrustadas: va
    /// en el hilo de fondo) y los marcadores resueltos.
    pub fn with_vault(
        text: &str,
        vault: &orbital_vault::Vault,
        from: &orbital_vault::RelPath,
        yaml: &[orbital_vault::props::PropEntry],
    ) -> Self {
        Self::build(text, Some((vault, from)), yaml)
    }

    fn build(
        text: &str,
        vault: Option<(&orbital_vault::Vault, &orbital_vault::RelPath)>,
        yaml: &[orbital_vault::props::PropEntry],
    ) -> Self {
        let repl = orbital_vault::markers::resolve_markers(text, yaml);
        let mut doc = orbital_vault::render::render_markdown_interpolated(text, &repl);
        let deps = vault.map_or_else(Vec::new, |(v, from)| v.expand_transclusions(from, &mut doc));
        // Un bloque con transclusiones cambia también si cambia lo incrustado: la huella entra en
        // su hash, y la actualización en el sitio reemplaza solo esa tarjeta.
        let hashes = doc
            .blocks
            .iter()
            .map(|b| {
                let h = orbital_vault::write::hash(text[b.source.clone()].as_bytes());
                match orbital_vault::transclusion::fingerprint(b) {
                    0 => h,
                    fp => orbital_vault::write::hash(&[h.to_le_bytes(), fp.to_le_bytes()].concat()),
                }
            })
            .collect();
        let header = orbital_vault::props::markdown::locate(text)
            .map(|r| orbital_vault::write::hash(text[r].as_bytes()))
            .unwrap_or(0);
        Self {
            doc,
            hashes,
            header,
            deps,
        }
    }
}

impl Reader {
    pub fn new() -> Self {
        let view = gtk::TextView::builder()
            .editable(false)
            .cursor_visible(false)
            .focusable(true)
            .wrap_mode(gtk::WrapMode::WordChar)
            .top_margin(28)
            .bottom_margin(64)
            .pixels_below_lines(4)
            .pixels_inside_wrap(3)
            .vexpand(true)
            .hexpand(true)
            .build();
        view.add_css_class("orbital-reader");
        view.update_property(&[gtk::accessible::Property::Label("Nota formateada")]);
        let scroller = gtk::ScrolledWindow::builder()
            // External: sin barra horizontal y sin que el mínimo del contenido fije el del panel.
            .hscrollbar_policy(gtk::PolicyType::External)
            .child(&view)
            .build();
        let this = Self(Rc::new(Inner {
            view,
            scroller,
            generation: Cell::new(0),
            blocks: RefCell::new(Vec::new()),
            header: Cell::new(None),
            links: RefCell::new(HashMap::new()),
            focused_link: Cell::new(None),
            indents: RefCell::new(Vec::new()),
            side: Cell::new(12),
            width: Cell::new(120),
            wide: RefCell::new(Vec::new()),
            pending: RefCell::new(Vec::new()),
            pending_line: Cell::new(None),
            attach_scheduled: Cell::new(false),
            on_toggle: RefCell::new(None),
            on_link: RefCell::new(None),
            is_missing: RefCell::new(None),
            on_rendered: RefCell::new(None),
        }));
        this.create_tags();
        this.center();
        this.setup_links();
        let weak = Rc::downgrade(&this.0);
        let v = this.0.scroller.vadjustment();
        let attach = move |_: &gtk::Adjustment| {
            if let Some(i) = weak.upgrade() {
                Reader(i).schedule_attach();
            }
        };
        v.connect_value_changed(attach.clone());
        v.connect_page_size_notify(attach);
        this
    }

    /// Anclar durante la disposición rompe una aserción de GTK: se hace en el próximo reposo.
    fn schedule_attach(&self) {
        let i = &self.0;
        if i.attach_scheduled.get() || i.pending.borrow().is_empty() {
            return;
        }
        i.attach_scheduled.set(true);
        let this = self.clone();
        glib::idle_add_local_once(move || {
            this.0.attach_scheduled.set(false);
            this.attach_visible();
        });
    }

    /// Ancla los widgets pendientes que están a menos de una pantalla de lo visible.
    fn attach_visible(&self) {
        let i = &self.0;
        if i.pending.borrow().is_empty() {
            return;
        }
        let rect = i.view.visible_rect();
        let h = rect.height().max(400);
        let top = i.view.iter_at_location(0, (rect.y() - h).max(0));
        let bottom = i.view.iter_at_location(0, rect.y() + 2 * h);
        // Antes de calcular la disposición, GTK estima alturas y todo parece cerca: la ventana
        // también se acota en caracteres.
        let from = top.map_or(0, |t| t.offset());
        let to = bottom
            .map_or(i32::MAX, |b| {
                let mut b = b;
                b.forward_line();
                b.offset()
            })
            .min(from + WINDOW_CHARS);
        let buffer = self.buffer();
        let mut ready = Vec::new();
        i.pending.borrow_mut().retain(|(w, a)| {
            if a.is_deleted() {
                return false;
            }
            if ready.len() >= ATTACH_BATCH {
                return true;
            }
            let at = buffer.iter_at_child_anchor(a).offset();
            if (from..=to).contains(&at) {
                ready.push((w.clone(), a.clone()));
                false
            } else {
                true
            }
        });
        let more = ready.len() >= ATTACH_BATCH;
        for (w, a) in ready {
            i.view.add_child_at_anchor(&w, &a);
        }
        // Por tandas: cada widget nuevo se mide y se dibuja en el cuadro siguiente.
        if more {
            self.schedule_attach();
        }
    }

    pub fn widget(&self) -> &gtk::ScrolledWindow {
        &self.0.scroller
    }

    /// La vista de texto del lector.
    pub fn text_view(&self) -> &gtk::TextView {
        &self.0.view
    }

    /// Limpia los resaltados de búsqueda en la vista formateada.
    pub fn clear_find_highlights(&self) {
        let b = self.buffer();
        b.remove_tag_by_name("find-highlight", &b.start_iter(), &b.end_iter());
        b.remove_tag_by_name("find-current", &b.start_iter(), &b.end_iter());
    }

    /// Al marcar o desmarcar una casilla: el byte de la casilla en el texto fuente.
    pub fn set_on_toggle(&self, f: impl Fn(usize) + 'static) {
        *self.0.on_toggle.borrow_mut() = Some(Box::new(f));
    }

    /// Al hacer clic en un enlace.
    pub fn set_on_link(&self, f: impl Fn(&str) + 'static) {
        *self.0.on_link.borrow_mut() = Some(Box::new(f));
    }

    /// Consulta si un enlace apunta a una nota pendiente o inexistente.
    pub fn set_is_missing(&self, f: impl Fn(&str) -> bool + 'static) {
        *self.0.is_missing.borrow_mut() = Some(Rc::new(f));
    }

    /// Avisa cuando termina de volcar el documento a la vista.
    pub fn set_on_rendered(&self, f: impl Fn() + 'static) {
        *self.0.on_rendered.borrow_mut() = Some(Box::new(f));
    }

    /// Olvida lo mostrado: el próximo volcado arma todo de nuevo (al cambiar de nota).
    pub fn reset(&self) {
        let i = &self.0;
        i.generation.set(i.generation.get() + 1);
        let table = self.buffer().tag_table();
        for b in i.blocks.borrow_mut().drain(..) {
            for t in b.tags {
                table.remove(&t);
            }
        }
        i.links.borrow_mut().clear();
        i.pending.borrow_mut().clear();
        i.pending_line.set(None);
        i.header.set(None);
        i.view.set_buffer(Some(&gtk::TextBuffer::new(Some(&table))));
    }

    /// El encabezado mostrado ya corresponde a este frontmatter (la lista de propiedades se
    /// actualizó sola): el próximo volcado no lo rearma.
    pub fn adopt_header(&self, hash: u64) {
        if self.0.header.get().is_some() {
            self.0.header.set(Some(hash));
        }
    }

    pub fn buffer(&self) -> gtk::TextBuffer {
        self.0.view.buffer()
    }

    fn create_tags(&self) {
        let b = self.buffer();
        let t = b.tag_table();
        let add = |tag: gtk::TextTag| {
            t.add(&tag);
        };
        let scales = [1.9, 1.55, 1.3, 1.15, 1.05, 1.0];
        for (n, s) in scales.iter().enumerate() {
            add(gtk::TextTag::builder()
                .name(format!("h{}", n + 1))
                .scale(*s)
                .weight(700)
                .pixels_above_lines(if n < 2 { 18 } else { 12 })
                .pixels_below_lines(6)
                .build());
        }
        add(gtk::TextTag::builder().name("strong").weight(700).build());
        add(gtk::TextTag::builder()
            .name("emph")
            .style(pango::Style::Italic)
            .build());
        add(gtk::TextTag::builder()
            .name("strike")
            .strikethrough(true)
            .build());
        add(gtk::TextTag::builder()
            .name("code")
            .family("monospace")
            .background("rgba(255,255,255,0.08)")
            .build());
        add(gtk::TextTag::builder()
            .name("link")
            .foreground(crate::style::LINK_FG.hex())
            .underline(pango::Underline::Single)
            .build());
        add(gtk::TextTag::builder()
            .name("link-missing")
            .foreground(crate::style::LINK_MISSING_FG.hex())
            .underline(pango::Underline::Single)
            .build());
        add(gtk::TextTag::builder()
            .name("link-focused")
            .background(crate::style::ACCENT_BG.hex())
            .foreground(crate::style::ACCENT_FG.hex())
            .build());
        add(gtk::TextTag::builder()
            .name("find-highlight")
            .background(crate::style::SEARCH_MATCH_BG.hex())
            .foreground(crate::style::SEARCH_MATCH_FG.hex())
            .build());
        add(gtk::TextTag::builder()
            .name("find-current")
            .background(crate::style::SEARCH_CURRENT_BG.hex())
            .foreground(crate::style::SEARCH_CURRENT_FG.hex())
            .build());
        add(gtk::TextTag::builder()
            .name("dim")
            .foreground("rgba(255,255,255,0.70)")
            .build());
        add(gtk::TextTag::builder()
            .name("codeblock")
            .family("monospace")
            .paragraph_background("#1e1e22")
            .wrap_mode(gtk::WrapMode::Char)
            .build());
        add(gtk::TextTag::builder()
            .name("table")
            .paragraph_background("#1e1e22")
            .wrap_mode(gtk::WrapMode::None)
            .build());
        add(gtk::TextTag::builder()
            .name("table-head")
            .weight(700)
            .build());
        // Separador: una línea casi vacía, muy baja, con fondo.
        add(gtk::TextTag::builder()
            .name("rule")
            .paragraph_background("rgba(255,255,255,0.22)")
            .scale(0.12)
            .pixels_below_lines(0)
            .build());
        add(gtk::TextTag::builder()
            .name("rule-space")
            .pixels_above_lines(10)
            .pixels_below_lines(10)
            .build());
        for (name, bg) in [
            ("callout-nota", crate::style::CALLOUT_NOTA_BG.hex()),
            ("callout-consejo", crate::style::CALLOUT_CONSEJO_BG.hex()),
            (
                "callout-importante",
                crate::style::CALLOUT_IMPORTANTE_BG.hex(),
            ),
            (
                "callout-advertencia",
                crate::style::CALLOUT_ADVERTENCIA_BG.hex(),
            ),
            ("callout-peligro", crate::style::CALLOUT_PELIGRO_BG.hex()),
            ("callout-otro", crate::style::CARD_BG.hex()),
        ] {
            add(gtk::TextTag::builder()
                .name(name)
                .paragraph_background(bg)
                .build());
        }
        add(gtk::TextTag::builder()
            .name("callout-title")
            .weight(700)
            .build());
        add(gtk::TextTag::builder()
            .name("embed-gap")
            .scale(0.4)
            .pixels_below_lines(0)
            .build());
        add(gtk::TextTag::builder()
            .name("embed")
            .paragraph_background(crate::style::EMBED_BG.hex())
            .build());
    }

    /// Etiqueta de sangría: se recalcula con el margen de la columna.
    fn indent_tag(&self, extra: i32) -> gtk::TextTag {
        let name = format!("indent-{extra}");
        let table = self.buffer().tag_table();
        if let Some(t) = table.lookup(&name) {
            return t;
        }
        let tag = gtk::TextTag::builder()
            .name(&name)
            .left_margin(self.0.side.get() + extra)
            .build();
        table.add(&tag);
        self.0.indents.borrow_mut().push((tag.clone(), extra));
        tag
    }

    /// Centra la columna con márgenes (la vista no se envuelve en otro widget).
    fn center(&self) {
        let weak = Rc::downgrade(&self.0);
        let apply = move |width: f64| {
            let Some(i) = weak.upgrade() else { return };
            let side = ((width as i32 - COLUMN) / 2).max(16);
            let text = (width as i32 - 2 * side).clamp(120, COLUMN);
            if i.side.get() == side && i.view.left_margin() == side && i.width.get() == text {
                return;
            }
            i.side.set(side);
            i.width.set(text);
            i.wide.borrow_mut().retain(|w| match w.upgrade() {
                Some(w) => {
                    w.set_size_request(text, -1);
                    true
                }
                None => false,
            });
            i.view.set_left_margin(side);
            i.view.set_right_margin(side);
            for (tag, extra) in i.indents.borrow().iter() {
                tag.set_left_margin(side + extra);
            }
        };
        let h = self.0.scroller.hadjustment();
        apply(h.page_size());
        h.connect_page_size_notify(move |a| apply(a.page_size()));
    }

    fn all_link_items(&self) -> Vec<LinkItem> {
        self.0
            .blocks
            .borrow()
            .iter()
            .flat_map(|b| b.links.iter().cloned())
            .collect()
    }

    fn set_focused_link(&self, next: Option<usize>) {
        let i = &self.0;
        let buf = self.buffer();
        if let Some(tag) = buf.tag_table().lookup("link-focused") {
            let (start, end) = buf.bounds();
            buf.remove_tag(&tag, &start, &end);
        }
        i.focused_link.set(next);
        if let Some(idx) = next {
            let items = self.all_link_items();
            if let Some(item) = items.get(idx) {
                let start = buf.iter_at_mark(&item.start);
                let end = buf.iter_at_mark(&item.end);
                if let Some(tag) = buf.tag_table().lookup("link-focused") {
                    buf.apply_tag(&tag, &start, &end);
                }
                let mut it = start;
                i.view.scroll_to_iter(&mut it, 0.1, false, 0.0, 0.0);
            }
        }
    }

    fn setup_links(&self) {
        let click = gtk::GestureClick::new();
        let weak = Rc::downgrade(&self.0);
        click.connect_released(move |g, _, x, y| {
            let Some(i) = weak.upgrade() else { return };
            Reader(i.clone()).set_focused_link(None);
            let (bx, by) =
                i.view
                    .window_to_buffer_coords(gtk::TextWindowType::Widget, x as i32, y as i32);
            let Some(iter) = i.view.iter_at_location(bx, by) else {
                return;
            };
            for tag in iter.tags() {
                let url = i.links.borrow().get(&tag).cloned();
                if let (Some(url), Some(f)) = (url, i.on_link.borrow().as_ref()) {
                    g.set_state(gtk::EventSequenceState::Claimed);
                    f(&url);
                    return;
                }
            }
        });
        self.0.view.add_controller(click);

        let key = gtk::EventControllerKey::new();
        let weak_key = Rc::downgrade(&self.0);
        key.connect_key_pressed(move |_, keyval, _, state| {
            let Some(i) = weak_key.upgrade() else {
                return glib::Propagation::Proceed;
            };
            let reader = Reader(i.clone());
            let items = reader.all_link_items();
            if items.is_empty() {
                return glib::Propagation::Proceed;
            }
            let is_shift = state.contains(gtk::gdk::ModifierType::SHIFT_MASK);
            if keyval == gtk::gdk::Key::Tab || keyval == gtk::gdk::Key::ISO_Left_Tab {
                let cur = i.focused_link.get();
                let next = if is_shift {
                    match cur {
                        None => Some(items.len().saturating_sub(1)),
                        Some(0) => None,
                        Some(idx) => Some(idx.saturating_sub(1)),
                    }
                } else {
                    match cur {
                        None => Some(0),
                        Some(idx) if idx + 1 < items.len() => Some(idx + 1),
                        Some(_) => None,
                    }
                };
                reader.set_focused_link(next);
                if next.is_some() {
                    glib::Propagation::Stop
                } else {
                    glib::Propagation::Proceed
                }
            } else if (keyval == gtk::gdk::Key::Return
                || keyval == gtk::gdk::Key::KP_Enter
                || keyval == gtk::gdk::Key::space)
                && reader.activate_focused_link()
            {
                glib::Propagation::Stop
            } else if keyval == gtk::gdk::Key::Escape && i.focused_link.get().is_some() {
                reader.set_focused_link(None);
                glib::Propagation::Stop
            } else {
                glib::Propagation::Proceed
            }
        });
        self.0.view.add_controller(key);
    }

    fn activate_focused_link(&self) -> bool {
        let Some(index) = self.0.focused_link.get() else {
            return false;
        };
        let Some(item) = self.all_link_items().get(index).cloned() else {
            return false;
        };
        if let Some(f) = self.0.on_link.borrow().as_ref() {
            f(&item.url);
        }
        true
    }

    /// Vuelca un documento. `header` arma las propiedades (solo se llama si cambiaron);
    /// `base_dir` resuelve las imágenes.
    pub fn render(
        &self,
        prepared: Prepared,
        header: impl FnOnce() -> Option<gtk::Widget>,
        base_dir: Option<PathBuf>,
    ) {
        let i = &self.0;
        let generation = i.generation.get() + 1;
        i.generation.set(generation);
        let Prepared {
            doc,
            hashes,
            header: header_hash,
            ..
        } = prepared;
        let in_place = {
            let old = i.blocks.borrow();
            let p = old
                .iter()
                .zip(&hashes)
                .take_while(|(b, h)| b.hash == **h)
                .count();
            let max_s = old.len().min(hashes.len()) - p;
            let s = old
                .iter()
                .rev()
                .zip(hashes.iter().rev())
                .take(max_s)
                .take_while(|(b, h)| b.hash == **h)
                .count();
            let changed = hashes.len() - p - s;
            (!old.is_empty() && changed <= MAX_IN_PLACE).then_some((p, s))
        };
        match in_place {
            Some((p, s)) => self.update(doc, hashes, header_hash, header, base_dir, p, s),
            None => self.rebuild(doc, hashes, header_hash, header, base_dir, generation),
        }
    }

    /// Arma todo en un buffer nuevo por tramos y lo pone al final: la vista no parpadea.
    fn rebuild(
        &self,
        doc: RenderDoc,
        hashes: Vec<u64>,
        header_hash: u64,
        header: impl FnOnce() -> Option<gtk::Widget>,
        base_dir: Option<PathBuf>,
        generation: u64,
    ) {
        let table = self.buffer().tag_table();
        let fresh = gtk::TextBuffer::new(Some(&table));
        let state = Rc::new(RenderState {
            buffer: fresh,
            attach_now: false,
            children: RefCell::new(Vec::new()),
            base_dir,
            tags: RefCell::new(Vec::new()),
            links: RefCell::new(Vec::new()),
            link_items: RefCell::new(Vec::new()),
            block_base: RefCell::new(Rc::new(Cell::new(0))),
            block_start: Cell::new(0),
            embedded: Cell::new(0),
        });
        if let Some(h) = header() {
            let mut end = state.buffer.end_iter();
            self.anchor(&state, &mut end, h.clone());
            self.track_wide(&h);
            state.buffer.insert(&mut end, "\n");
        }
        let built: Rc<RefCell<Vec<BlockInfo>>> = Rc::default();
        let blocks = RefCell::new(doc.blocks.into_iter().zip(hashes));
        let this = self.clone();
        glib::idle_add_local(move || {
            if this.0.generation.get() != generation {
                // Cancelado: las etiquetas creadas no llegaron a la vista.
                let table = state.buffer.tag_table();
                for b in built.borrow_mut().drain(..) {
                    b.tags.iter().for_each(|t| table.remove(t));
                }
                state.tags.borrow().iter().for_each(|t| table.remove(t));
                return glib::ControlFlow::Break;
            }
            let start = Instant::now();
            loop {
                let next = blocks.borrow_mut().next();
                let Some((block, hash)) = next else {
                    this.swap_in(
                        &state,
                        std::mem::take(&mut *built.borrow_mut()),
                        header_hash,
                    );
                    return glib::ControlFlow::Break;
                };
                let mut end = state.buffer.end_iter();
                let info = this.top_block(&state, &mut end, &block, hash);
                built.borrow_mut().push(info);
                if start.elapsed() >= SLICE {
                    return glib::ControlFlow::Continue;
                }
            }
        });
    }

    /// Pone en la vista el buffer armado por `rebuild` y suelta lo anterior.
    fn swap_in(&self, s: &RenderState, blocks: Vec<BlockInfo>, header_hash: u64) {
        let i = &self.0;
        i.focused_link.set(None);
        let table = s.buffer.tag_table();
        let mut links = i.links.borrow_mut();
        for b in i.blocks.borrow_mut().drain(..) {
            for t in &b.tags {
                links.remove(t);
                table.remove(t);
            }
        }
        links.extend(s.links.borrow_mut().drain(..));
        drop(links);
        let top = s.buffer.start_iter();
        i.view.set_buffer(Some(&s.buffer));
        s.buffer.place_cursor(&top);
        // Los widgets se anclan después de poner el buffer en la vista (si se anclan antes, el
        // cambio de buffer los descarta), y solo los cercanos a lo visible.
        *i.pending.borrow_mut() = std::mem::take(&mut *s.children.borrow_mut());
        *i.blocks.borrow_mut() = blocks;
        i.header.set(Some(header_hash));
        // La disposición todavía no está calculada: se ancla lo visible en el próximo reposo.
        self.schedule_attach();
        if let Some(line) = i.pending_line.take() {
            self.scroll_to_line(line);
        }
        if let Some(f) = i.on_rendered.borrow().as_ref() {
            f();
        }
    }

    /// Reemplaza en el sitio los bloques que cambiaron; el prefijo `p` y el sufijo `s` quedan.
    #[allow(clippy::too_many_arguments)]
    fn update(
        &self,
        doc: RenderDoc,
        hashes: Vec<u64>,
        header_hash: u64,
        header: impl FnOnce() -> Option<gtk::Widget>,
        base_dir: Option<PathBuf>,
        p: usize,
        s: usize,
    ) {
        let i = &self.0;
        let buffer = self.buffer();
        let table = buffer.tag_table();
        let state = RenderState {
            buffer: buffer.clone(),
            attach_now: true,
            children: RefCell::new(Vec::new()),
            base_dir,
            tags: RefCell::new(Vec::new()),
            links: RefCell::new(Vec::new()),
            link_items: RefCell::new(Vec::new()),
            block_base: RefCell::new(Rc::new(Cell::new(0))),
            block_start: Cell::new(0),
            embedded: Cell::new(0),
        };
        i.focused_link.set(None);
        let mut old = std::mem::take(&mut *i.blocks.borrow_mut());
        let suffix = old.split_off(old.len() - s);
        let removed = old.split_off(p);
        let prefix = old;

        let start_at = |b: Option<&BlockInfo>| match b {
            Some(b) => buffer.iter_at_mark(&b.mark),
            None => buffer.end_iter(),
        };
        let mut from = start_at(removed.first().or(suffix.first()));
        let mut to = start_at(suffix.first());
        {
            let mut links = i.links.borrow_mut();
            for b in &removed {
                for t in &b.tags {
                    links.remove(t);
                    table.remove(t);
                }
                for item in &b.links {
                    buffer.delete_mark(&item.start);
                    buffer.delete_mark(&item.end);
                }
                buffer.delete_mark(&b.mark);
            }
        }
        buffer.delete(&mut from, &mut to);

        let new_len = doc.blocks.len();
        let mut end = from;
        let mut middle = Vec::with_capacity(new_len - p - s);
        for (block, hash) in doc.blocks[p..new_len - s]
            .iter()
            .zip(&hashes[p..new_len - s])
        {
            middle.push(self.top_block(&state, &mut end, block, *hash));
        }
        // La marca del primer bloque que queda tiene gravedad izquierda: quedó antes de lo insertado.
        if let Some(b) = suffix.first() {
            buffer.move_mark(&b.mark, &end);
        }
        i.links
            .borrow_mut()
            .extend(state.links.borrow_mut().drain(..));

        let mut blocks = prefix;
        blocks.extend(middle);
        blocks.extend(suffix);
        for (info, block) in blocks.iter_mut().zip(&doc.blocks) {
            info.line = block.line;
            info.base.set(block.source.start);
        }

        if i.header.get() != Some(header_hash) {
            let mut start = buffer.start_iter();
            let mut first = start_at(blocks.first());
            buffer.delete(&mut start, &mut first);
            let mut at = buffer.start_iter();
            if let Some(h) = header() {
                self.anchor(&state, &mut at, h.clone());
                self.track_wide(&h);
                buffer.insert(&mut at, "\n");
            }
            if let Some(b) = blocks.first() {
                buffer.move_mark(&b.mark, &at);
            }
            i.header.set(Some(header_hash));
        }
        *i.blocks.borrow_mut() = blocks;
        if let Some(f) = i.on_rendered.borrow().as_ref() {
            f();
        }
    }

    /// Desplaza la vista para mostrar el bloque que contiene la línea `line` del texto fuente.
    /// Como [`Reader::scroll_to_line`], pero si la nota todavía se está volcando, espera a que
    /// termine.
    pub fn scroll_to_line_when_ready(&self, line: usize) {
        if self.0.blocks.borrow().is_empty() {
            self.0.pending_line.set(Some(line));
        } else {
            self.scroll_to_line(line);
        }
    }

    pub fn scroll_to_line(&self, line: usize) {
        let blocks = self.0.blocks.borrow();
        let idx = blocks.partition_point(|b| b.line <= line).saturating_sub(1);
        if let Some(b) = blocks.get(idx) {
            self.0.view.scroll_to_mark(&b.mark, 0.0, true, 0.0, 0.0);
        }
    }

    /// Desplaza la vista hasta el encabezado con el texto indicado. Devuelve si lo encontró.
    pub fn scroll_to_heading(&self, heading: &str) -> bool {
        let wanted = heading.trim().to_lowercase();
        let blocks = self.0.blocks.borrow();
        for b in blocks.iter() {
            if let Some(h) = &b.heading
                && h.trim().to_lowercase() == wanted
            {
                self.0.view.scroll_to_mark(&b.mark, 0.0, true, 0.0, 0.0);
                return true;
            }
        }
        false
    }

    /// Un widget anclado que sigue el ancho de la columna (sin fijar un mínimo mayor que el panel).
    fn track_wide(&self, w: &gtk::Widget) {
        w.set_size_request(self.0.width.get(), -1);
        self.0.wide.borrow_mut().push(w.downgrade());
    }

    fn anchor(&self, s: &RenderState, end: &mut gtk::TextIter, w: gtk::Widget) {
        let anchor = s.buffer.create_child_anchor(end);
        if s.attach_now {
            self.0.view.add_child_at_anchor(&w, &anchor);
        } else {
            s.children.borrow_mut().push((w, anchor));
        }
    }

    fn new_tag(&self, s: &RenderState, tag: gtk::TextTag) -> gtk::TextTag {
        s.buffer.tag_table().add(&tag);
        s.tags.borrow_mut().push(tag.clone());
        tag
    }

    fn top_block(
        &self,
        s: &RenderState,
        end: &mut gtk::TextIter,
        block: &Block,
        hash: u64,
    ) -> BlockInfo {
        let mark = s.buffer.create_mark(None, end, true);
        let base = Rc::new(Cell::new(block.source.start));
        *s.block_base.borrow_mut() = base.clone();
        s.block_start.set(block.source.start);
        self.block(s, end, block, 0);
        let heading = match &block.kind {
            BlockKind::Heading { inlines, .. } => Some(orbital_vault::render::plain_text(inlines)),
            _ => None,
        };
        BlockInfo {
            hash,
            line: block.line,
            mark,
            base,
            tags: std::mem::take(&mut *s.tags.borrow_mut()),
            heading,
            links: std::mem::take(&mut *s.link_items.borrow_mut()),
        }
    }

    fn block(&self, s: &RenderState, end: &mut gtk::TextIter, b: &Block, depth: i32) {
        let buf = &s.buffer;
        let table = buf.tag_table();
        let indent = (depth > 0).then(|| self.indent_tag(depth * 24));
        let base: Vec<gtk::TextTag> = indent.into_iter().collect();
        match &b.kind {
            BlockKind::Heading { level, inlines } => {
                let mut tags = base.clone();
                tags.extend(table.lookup(&format!("h{level}")));
                self.inlines(s, end, inlines, &tags);
                buf.insert(end, "\n");
            }
            BlockKind::Paragraph(inlines) => {
                self.inlines(s, end, inlines, &base);
                buf.insert(end, "\n");
            }
            BlockKind::List { start, items } => {
                for (n, item) in items.iter().enumerate() {
                    let tag = self.indent_tag((depth + 1) * 24);
                    let mut tags = base.clone();
                    tags.push(tag.clone());
                    let item_start = end.offset();
                    if let Some(task) = item.task {
                        self.task(s, end, item, task);
                    } else {
                        let bullet = match start {
                            Some(s) => format!("{}. ", s + n as u64),
                            None => "•  ".to_owned(),
                        };
                        let refs: Vec<&gtk::TextTag> = tags.iter().collect();
                        buf.insert_with_tags(end, &bullet, &refs);
                    }
                    for (k, sub) in item.blocks.iter().enumerate() {
                        match &sub.kind {
                            // El primer párrafo va en la misma línea que la viñeta.
                            BlockKind::Paragraph(p) if k == 0 => {
                                self.inlines(s, end, p, &tags);
                                buf.insert(end, "\n");
                            }
                            _ => self.block(s, end, sub, depth + 1),
                        }
                    }
                    if item.blocks.is_empty() {
                        buf.insert(end, "\n");
                    }
                    // La sangría cubre también la casilla anclada al principio de la línea.
                    let first = buf.iter_at_offset(item_start);
                    let mut line_end = first;
                    if !line_end.ends_line() {
                        line_end.forward_to_line_end();
                    }
                    buf.apply_tag(&tag, &first, &line_end);
                }
            }
            BlockKind::Quote(body) => {
                for sub in body {
                    let start = end.offset();
                    self.block(s, end, sub, depth + 1);
                    if let Some(dim) = table.lookup("dim") {
                        buf.apply_tag(&dim, &buf.iter_at_offset(start), end);
                    }
                }
            }
            BlockKind::Callout {
                kind,
                title,
                folded,
                body,
            } => self.callout(s, end, kind, title, *folded, body, depth, &base),
            BlockKind::Code { text, .. } => {
                let mut tags = base.clone();
                tags.extend(table.lookup("codeblock"));
                let refs: Vec<&gtk::TextTag> = tags.iter().collect();
                let body = text.strip_suffix('\n').unwrap_or(text);
                buf.insert_with_tags(end, body, &refs);
                buf.insert(end, "\n");
            }
            // Hasta que la vista complete el contenido (T015/T019), el bloque generado se ve
            // como su codeblock fuente.
            BlockKind::Generated(g) => self.block(
                s,
                end,
                &Block {
                    kind: BlockKind::Code {
                        lang: String::new(),
                        text: g.raw.clone(),
                    },
                    source: g.source.clone(),
                    line: b.line,
                },
                depth,
            ),
            BlockKind::Table {
                align,
                header,
                rows,
            } => self.table(s, end, align, header, rows, &base),
            BlockKind::Rule => {
                let space: Vec<gtk::TextTag> = table.lookup("rule-space").into_iter().collect();
                let rule: Vec<gtk::TextTag> = table.lookup("rule").into_iter().collect();
                buf.insert_with_tags(end, "\n", &space.iter().collect::<Vec<_>>());
                buf.insert_with_tags(end, "\u{200b}\n", &rule.iter().collect::<Vec<_>>());
                buf.insert_with_tags(end, "\n", &space.iter().collect::<Vec<_>>());
            }
            BlockKind::Html(text) => {
                let mut tags = base.clone();
                tags.extend(table.lookup("code"));
                let refs: Vec<&gtk::TextTag> = tags.iter().collect();
                buf.insert_with_tags(end, text.trim_end(), &refs);
                buf.insert(end, "\n");
            }
            BlockKind::Transclusion(t) => self.transclusion(s, end, t, depth, &base),
        }
    }

    /// Tarjeta de una transclusión (US5 de la 003): un encabezado con la nota de origen (un enlace
    /// que la abre) y su contenido, con fondo y sangría. Lo incrustado no se edita desde acá: sus
    /// casillas quedan deshabilitadas.
    fn transclusion(
        &self,
        s: &RenderState,
        end: &mut gtk::TextIter,
        t: &orbital_vault::render::Transclusion,
        depth: i32,
        base: &[gtk::TextTag],
    ) {
        use orbital_vault::render::Embed;
        let buf = &s.buffer;
        let table = buf.tag_table();
        let start = end.offset();
        let url = match &t.anchor {
            Some(a) => format!("[[{}#{a}]]", t.target),
            None => format!("[[{}]]", t.target),
        };
        let shown = match &t.anchor {
            Some(a) => format!("{} › {a}", t.target),
            None => t.target.clone(),
        };
        let link = |text: String| orbital_vault::render::Inline::Link {
            url: url.clone(),
            children: vec![orbital_vault::render::Inline::Text(text)],
        };
        let mut title: Vec<gtk::TextTag> = base.to_vec();
        title.extend(table.lookup("callout-title"));
        let mut dim: Vec<gtk::TextTag> = base.to_vec();
        dim.extend(table.lookup("dim"));
        let text = |end: &mut gtk::TextIter, tags: &[gtk::TextTag], t: &str| {
            buf.insert_with_tags(end, t, &tags.iter().collect::<Vec<_>>());
        };
        text(end, &title, "↪  ");
        match &t.content {
            Embed::Note { blocks, .. } => {
                self.inlines(s, end, &[link(shown)], &title);
                buf.insert(end, "\n");
                s.embedded.set(s.embedded.get() + 1);
                for b in blocks {
                    self.block(s, end, b, depth + 1);
                }
                s.embedded.set(s.embedded.get() - 1);
            }
            Embed::Missing => {
                text(end, &dim, "Nota inexistente: ");
                self.inlines(s, end, &[link(shown)], &title);
                text(end, &dim, " (creala desde el enlace)");
                buf.insert(end, "\n");
            }
            Embed::SectionMissing { .. } => {
                self.inlines(s, end, &[link(t.target.clone())], &title);
                let a = t.anchor.as_deref().unwrap_or("");
                text(end, &dim, &format!(" no tiene el encabezado «{a}»"));
                buf.insert(end, "\n");
            }
            Embed::Html { .. } => {
                self.inlines(s, end, &[link(shown)], &title);
                text(end, &dim, " es una nota HTML: abrila para verla");
                buf.insert(end, "\n");
            }
            Embed::Ambiguous(all) => {
                text(
                    end,
                    &dim,
                    &format!(
                        "Hay {} notas «{}»: escribí la carpeta, por ejemplo ![[{}]]",
                        all.len(),
                        t.target,
                        all[0]
                            .as_str()
                            .strip_prefix("wiki/")
                            .unwrap_or(all[0].as_str())
                            .rsplit_once('.')
                            .map_or("", |(p, _)| p),
                    ),
                );
                buf.insert(end, "\n");
            }
            Embed::Cycle { .. } => {
                text(end, &dim, "↻ Referencia circular a ");
                self.inlines(s, end, &[link(shown)], &title);
                text(end, &dim, ": ya se está mostrando más arriba");
                buf.insert(end, "\n");
            }
            Embed::TooDeep => {
                text(
                    end,
                    &dim,
                    &format!(
                        "Demasiados niveles de transclusión (máximo {}): ",
                        orbital_vault::transclusion::MAX_DEPTH
                    ),
                );
                self.inlines(s, end, &[link(shown)], &title);
                buf.insert(end, "\n");
            }
            Embed::Pending => {
                text(end, &dim, &format!("![[{}]]", t.target));
                buf.insert(end, "\n");
            }
        }
        if let Some(bg) = table.lookup("embed") {
            buf.apply_tag(&bg, &buf.iter_at_offset(start), end);
        }
        // Un espacio corto sin fondo: dos tarjetas seguidas no se ven como una sola.
        let gap: Vec<gtk::TextTag> = table.lookup("embed-gap").into_iter().collect();
        buf.insert_with_tags(end, "\n", &gap.iter().collect::<Vec<_>>());
    }

    /// Casilla de una tarea: un `CheckButton` que cambia un solo byte del texto fuente.
    fn task(
        &self,
        s: &RenderState,
        end: &mut gtk::TextIter,
        item: &orbital_vault::render::ListItem,
        task: orbital_vault::render::TaskMark,
    ) {
        let check = gtk::CheckButton::builder()
            .active(task.checked)
            .valign(gtk::Align::Center)
            .build();
        // Una casilla de una nota incrustada no es de esta nota: no se marca desde acá.
        if s.embedded.get() > 0 {
            check.set_sensitive(false);
            check.update_property(&[gtk::accessible::Property::Label(&format!(
                "Tarea de otra nota: {}",
                item.blocks
                    .first()
                    .map(|b| match &b.kind {
                        BlockKind::Paragraph(p) => plain_text(p),
                        _ => String::new(),
                    })
                    .unwrap_or_default()
            ))]);
            self.anchor(s, end, check.upcast());
            s.buffer.insert(end, " ");
            return;
        }
        let label = item
            .blocks
            .first()
            .map(|b| match &b.kind {
                BlockKind::Paragraph(p) => plain_text(p),
                _ => String::new(),
            })
            .unwrap_or_default();
        check.update_property(&[gtk::accessible::Property::Label(&format!("Tarea: {label}"))]);
        // Relativo al bloque: si se edita algo antes, el bloque se conserva y su base cambia.
        let rel = task.offset - s.block_start.get();
        let base = s.block_base.borrow().clone();
        let weak = Rc::downgrade(&self.0);
        check.connect_toggled(move |_| {
            if let Some(i) = weak.upgrade()
                && let Some(f) = i.on_toggle.borrow().as_ref()
            {
                f(base.get() + rel);
            }
        });
        self.anchor(s, end, check.upcast());
        s.buffer.insert(end, " ");
    }

    #[allow(clippy::too_many_arguments)]
    fn callout(
        &self,
        s: &RenderState,
        end: &mut gtk::TextIter,
        kind: &CalloutKind,
        title: &str,
        folded: Option<bool>,
        body: &[Block],
        depth: i32,
        base: &[gtk::TextTag],
    ) {
        let buf = &s.buffer;
        let table = buf.tag_table();
        let (name, glyph) = match kind {
            CalloutKind::Nota => ("callout-nota", "ⓘ"),
            CalloutKind::Consejo => ("callout-consejo", "✦"),
            CalloutKind::Importante => ("callout-importante", "✱"),
            CalloutKind::Advertencia => ("callout-advertencia", "⚠"),
            CalloutKind::Peligro => ("callout-peligro", "✖"),
            CalloutKind::Otro(_) => ("callout-otro", "ⓘ"),
        };
        let start = end.offset();
        let mut ttags: Vec<gtk::TextTag> = base.to_vec();
        ttags.extend(table.lookup("callout-title"));
        let trefs: Vec<&gtk::TextTag> = ttags.iter().collect();
        buf.insert_with_tags(end, &format!("{glyph}  {title}"), &trefs);
        // Plegable: un botón que oculta el cuerpo (el único widget del callout).
        let fold = folded.map(|f| {
            let tag = self.new_tag(s, gtk::TextTag::builder().invisible(f).build());
            let icon = |open: bool| {
                if open {
                    "pan-down-symbolic"
                } else {
                    "pan-end-symbolic"
                }
            };
            let button = gtk::ToggleButton::builder()
                .icon_name(icon(!f))
                .active(!f)
                .valign(gtk::Align::Center)
                .css_classes(["flat", "circular"])
                .build();
            button.update_property(&[gtk::accessible::Property::Label(&format!(
                "Mostrar u ocultar «{title}»"
            ))]);
            let t = tag.clone();
            button.connect_toggled(move |b| {
                t.set_invisible(!b.is_active());
                b.set_icon_name(icon(b.is_active()));
            });
            buf.insert(end, " ");
            self.anchor(s, end, button.upcast());
            tag
        });
        buf.insert(end, "\n");
        let body_start = end.offset();
        for sub in body {
            self.block(s, end, sub, depth + 1);
        }
        if let Some(bg) = table.lookup(name) {
            buf.apply_tag(&bg, &buf.iter_at_offset(start), end);
        }
        if let Some(tag) = &fold {
            buf.apply_tag(tag, &buf.iter_at_offset(body_start), end);
        }
    }

    /// Tabla como texto con tabulaciones: sin widgets, seleccionable y barata de dibujar.
    fn table(
        &self,
        s: &RenderState,
        end: &mut gtk::TextIter,
        align: &[Align],
        header: &[Vec<Inline>],
        rows: &[Vec<Vec<Inline>>],
        base: &[gtk::TextTag],
    ) {
        const PAD: i32 = 12;
        const GAP: i32 = 28;
        let buf = &s.buffer;
        let table = buf.tag_table();
        let text = |cells: &[Vec<Inline>]| -> Vec<String> {
            cells
                .iter()
                .map(|c| plain_text(c).replace(['\t', '\n'], " "))
                .collect()
        };
        let head = text(header);
        let body: Vec<Vec<String>> = rows.iter().map(|r| text(r)).collect();
        let cols = head.len().max(body.iter().map(Vec::len).max().unwrap_or(0));
        let mut widths = vec![0; cols];
        let layout = self.0.view.create_pango_layout(None);
        let measure = |t: &str, bold: bool| {
            if bold {
                layout.set_markup(&format!("<b>{}</b>", glib::markup_escape_text(t)));
            } else {
                layout.set_text(t);
            }
            layout.pixel_size().0
        };
        for (c, t) in head.iter().enumerate() {
            widths[c] = widths[c].max(measure(t, true));
        }
        for row in &body {
            for (c, t) in row.iter().enumerate() {
                widths[c] = widths[c].max(measure(t, false));
            }
        }
        let mut tabs = pango::TabArray::new(cols as i32, true);
        let mut x = PAD;
        for (c, w) in widths.iter().enumerate() {
            let (a, pos) = match align.get(c) {
                Some(Align::Right) => (pango::TabAlign::Right, x + w),
                Some(Align::Center) => (pango::TabAlign::Center, x + w / 2),
                _ => (pango::TabAlign::Left, x),
            };
            tabs.set_tab(c as i32, a, pos);
            x += w + GAP;
        }
        let tab_tag = self.new_tag(s, gtk::TextTag::builder().tabs(&tabs).build());
        let mut tags: Vec<gtk::TextTag> = base.to_vec();
        tags.extend(table.lookup("table"));
        tags.push(tab_tag);
        let line = |end: &mut gtk::TextIter, cells: &[String], bold: bool| {
            let mut l = String::new();
            for c in 0..cols {
                l.push('\t');
                l.push_str(cells.get(c).map(String::as_str).unwrap_or(""));
            }
            l.push('\n');
            let mut t = tags.clone();
            if bold {
                t.extend(table.lookup("table-head"));
            }
            buf.insert_with_tags(end, &l, &t.iter().collect::<Vec<_>>());
        };
        line(end, &head, true);
        for row in &body {
            line(end, row, false);
        }
    }

    fn inlines(
        &self,
        s: &RenderState,
        end: &mut gtk::TextIter,
        inlines: &[Inline],
        tags: &[gtk::TextTag],
    ) {
        let buf = &s.buffer;
        let table = buf.tag_table();
        for inl in inlines {
            match inl {
                Inline::Text(t) => {
                    let refs: Vec<&gtk::TextTag> = tags.iter().collect();
                    buf.insert_with_tags(end, t, &refs);
                }
                Inline::Code(t) => {
                    let mut more = tags.to_vec();
                    more.extend(table.lookup("code"));
                    let refs: Vec<&gtk::TextTag> = more.iter().collect();
                    buf.insert_with_tags(end, t, &refs);
                }
                Inline::Strong(c) | Inline::Emph(c) | Inline::Strike(c) => {
                    let name = match inl {
                        Inline::Strong(_) => "strong",
                        Inline::Emph(_) => "emph",
                        _ => "strike",
                    };
                    let mut more = tags.to_vec();
                    more.extend(table.lookup(name));
                    self.inlines(s, end, c, &more);
                }
                Inline::Link { url, children } => {
                    let is_missing = self.0.is_missing.borrow().as_ref().is_some_and(|f| f(url));
                    let tag_name = if is_missing { "link-missing" } else { "link" };
                    let start_mark = buf.create_mark(None, end, true);
                    let tag = self.link_tag(s, url);
                    let mut more = tags.to_vec();
                    more.extend(table.lookup(tag_name));
                    more.push(tag);
                    self.inlines(s, end, children, &more);
                    let end_mark = buf.create_mark(None, end, false);
                    s.link_items.borrow_mut().push(LinkItem {
                        url: url.clone(),
                        start: start_mark,
                        end: end_mark,
                    });
                }
                Inline::Image { url, alt } => self.image(s, end, url, alt, tags),
                Inline::SoftBreak => buf.insert(end, " "),
                Inline::HardBreak => buf.insert(end, "\n"),
            }
        }
    }

    /// Etiqueta anónima para un enlace (el clic busca la URL por la etiqueta).
    fn link_tag(&self, s: &RenderState, url: &str) -> gtk::TextTag {
        let tag = self.new_tag(s, gtk::TextTag::new(None));
        s.links.borrow_mut().push((tag.clone(), url.to_owned()));
        tag
    }

    fn image(
        &self,
        s: &RenderState,
        end: &mut gtk::TextIter,
        url: &str,
        alt: &str,
        tags: &[gtk::TextTag],
    ) {
        let buf = &s.buffer;
        let local = (!url.contains("://"))
            .then(|| s.base_dir.as_ref().map(|d| d.join(url)))
            .flatten()
            .filter(|p| p.is_file());
        match local {
            Some(path) => {
                let pic = picture(&path, alt, self.0.width.get());
                self.anchor(s, end, pic);
            }
            // Las imágenes remotas no se cargan (principio V): se muestran como enlace.
            None => {
                let tag = self.link_tag(s, url);
                let mut more = tags.to_vec();
                more.extend(buf.tag_table().lookup("link"));
                more.push(tag);
                let refs: Vec<&gtk::TextTag> = more.iter().collect();
                let label = if alt.is_empty() { url } else { alt };
                buf.insert_with_tags(end, &format!("🖼 {label}"), &refs);
            }
        }
    }
}

/// Imagen local reducida al ancho de la columna. La vista de texto da a los widgets anclados su
/// tamaño mínimo, así que se pide el tamaño explícito (leído de la cabecera, sin decodificar).
fn picture(path: &Path, alt: &str, max_width: i32) -> gtk::Widget {
    let pic = gtk::Picture::for_filename(path);
    pic.set_can_shrink(true);
    pic.set_content_fit(gtk::ContentFit::Contain);
    pic.set_alternative_text(Some(alt));
    if let Some((_, w, h)) = gtk::gdk_pixbuf::Pixbuf::file_info(path)
        && w > 0
        && h > 0
    {
        let width = w.min(max_width.max(120));
        pic.set_size_request(width, h * width / w);
    }
    pic.upcast()
}

struct RenderState {
    buffer: gtk::TextBuffer,
    /// En el sitio (el buffer ya está en la vista) los widgets se anclan enseguida.
    attach_now: bool,
    children: RefCell<Vec<(gtk::Widget, gtk::TextChildAnchor)>>,
    base_dir: Option<PathBuf>,
    /// Etiquetas propias del bloque de primer nivel en curso.
    tags: RefCell<Vec<gtk::TextTag>>,
    links: RefCell<Vec<(gtk::TextTag, String)>>,
    link_items: RefCell<Vec<LinkItem>>,
    block_base: RefCell<Rc<Cell<usize>>>,
    block_start: Cell<usize>,
    /// Profundidad dentro de transclusiones (lo incrustado no se edita desde acá).
    embedded: Cell<u32>,
}

#[cfg(test)]
#[path = "reader_tests.rs"]
mod reader_tests;
