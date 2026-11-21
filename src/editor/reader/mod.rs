//! Vista formateada de las notas `.md` (Lectura y la mitad derecha de Dividida), nativa (research
//! R2 y R3 de la 002).
//!
//! El `GtkTextView` va directo en su `GtkScrolledWindow` (si no, GTK calcula todo el documento en
//! cada cambio). Casi todo es texto con etiquetas: tablas con tabulaciones y separadores como una
//! franja. Los fondos de los callouts, las guías de las listas y las flechitas del plegado se
//! dibujan a mano en capas de la vista (`capas`). Solo son widgets anclados las propiedades, las
//! casillas y las imágenes: cada widget anclado cuesta, y con miles la vista tardaba segundos en
//! dibujarse.
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
    Align, Block, BlockKind, CalloutKind, Inline, ListItem, RenderDoc, SeccionKind, plain_text,
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
type PlegarFn = Box<dyn Fn(u32)>;
type EnfocarFn = Box<dyn Fn(Option<u32>)>;
type SubirEnfoqueFn = Box<dyn Fn()>;

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
    /// Nivel si el bloque es un encabezado (las secciones plegadas llegan hasta el próximo de
    /// nivel menor o igual, 028).
    nivel: Option<u8>,
    /// Elementos plegables del bloque (028).
    plegables: Vec<plegado::PlegableLector>,
    /// Lo que se dibuja a mano sobre el bloque (fondos de callouts, guías de listas).
    capas: Vec<capas::Capa>,
    /// El bloque analizado: se vuelve a volcar al plegar o desplegar algo de él (028/046).
    fuente: Rc<Block>,
    /// Cómo quedó volcado respecto del plegado y el enfoque.
    firma: plegado::Firma,
}

impl BlockInfo {
    /// Todas las marcas del bloque (para trasplantarlo de buffer).
    fn marcas_mut(&mut self) -> Vec<&mut gtk::TextMark> {
        let mut out = vec![&mut self.mark];
        for l in &mut self.links {
            out.push(&mut l.start);
            out.push(&mut l.end);
        }
        for p in &mut self.plegables {
            out.push(&mut p.glifo);
            if let Some((d, h)) = &mut p.contenido {
                out.push(d);
                out.push(h);
            }
        }
        for c in &mut self.capas {
            out.extend(c.marcas_mut());
        }
        out
    }
}

struct Inner {
    view: gtk::TextView,
    scroller: gtk::ScrolledWindow,
    /// Contenido de las definiciones de notas al pie para los tooltips (012).
    fn_defs: RefCell<HashMap<String, String>>,
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
    /// Bloque a resaltar cuando termine de dibujarse (navegación a `#^id`, 026).
    pending_resaltar: Cell<Option<usize>>,
    attach_scheduled: Cell<bool>,
    on_toggle: RefCell<Option<ToggleFn>>,
    on_link: RefCell<Option<LinkFn>>,
    is_missing: RefCell<Option<IsMissingFn>>,
    on_rendered: RefCell<Option<RenderedFn>>,
    /// Líneas fuente de los elementos plegados (028); la verdad está en el buffer de Código.
    plegado: RefCell<Rc<Vec<u32>>>,
    on_plegar: RefCell<Option<PlegarFn>>,
    /// Rango `linea..=ultima` del elemento enfocado en el editor (028, T032).
    enfoque: RefCell<Option<(u32, u32)>>,
    on_enfocar: RefCell<Option<EnfocarFn>>,
    on_subir_enfoque: RefCell<Option<SubirEnfoqueFn>>,
    /// El elemento plegable bajo el mouse: su flechita se ve aunque esté desplegado.
    hover_plegable: Cell<Option<u32>>,
    /// Contador para nombrar la etiqueta del cuerpo de cada callout plegable.
    callouts: Cell<u64>,
    /// Etiquetas de renglones con flechita colgada: (etiqueta, sangría del texto).
    colgados: RefCell<Vec<(gtk::TextTag, i32)>>,
    /// Ancho de la flechita y su espacio.
    ancho_flecha: Cell<i32>,
    /// La forma del puntero puesta (`None`: todavía ninguna); se cambia solo si cambia.
    zona: Cell<Option<puntero::Zona>>,
    /// Líneas fuente de los elementos plegables del documento volcado (028).
    plegables: RefCell<Rc<Vec<u32>>>,
    /// Carpeta para resolver las imágenes del documento volcado (al volver a volcar un bloque).
    base_dir: RefCell<Option<PathBuf>>,
    /// Vuelve a volcar lo que cambió de plegado por tramos (desplegar todo de una nota grande).
    replegado: Cell<u64>,
    /// Callouts plegables y tarjetas que se abrieron o cerraron a mano, por (inicio en la fuente
    /// del bloque de primer nivel, inicio del propio bloque): al volver a volcarse (desplegar su
    /// sección) quedan como estaban.
    alternados: RefCell<std::collections::HashSet<(usize, usize)>>,
    /// La clave de `alternados` de cada etiqueta de cuerpo o respuesta, por su nombre.
    clave_de: RefCell<HashMap<String, (usize, usize)>>,
    /// Tamaño de letra fijo para las fórmulas (OnStage lo cambia por CSS, 050); `None`: el de la
    /// vista.
    tamano_formulas: Cell<Option<f32>>,
    /// Lo volcado tiene alguna fórmula (050).
    hay_formulas: Cell<bool>,
    /// Los gráficos (052) sin interacción (OnStage).
    graficos_estaticos: Cell<bool>,
    /// Los diagramas (058) con su tamaño natural: se achican al ancho de la columna.
    diagramas: RefCell<Vec<(glib::WeakRef<gtk::Widget>, f32, f32)>>,
    /// Bloques de primer nivel con código por resaltar en reposo (055).
    resaltar: RefCell<Vec<resaltar::Pendiente>>,
    resaltar_programado: Cell<bool>,
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
    /// Líneas fuente de los elementos plegables (028); vacío fuera del editor (OnStage).
    pub plegables: Vec<u32>,
}

impl Reader {
    pub fn new() -> Self {
        // Una subclase solo para dibujar las capas propias (callouts, guías, flechitas).
        let view: gtk::TextView = capas::VistaLector::new().upcast();
        view.set_editable(false);
        view.set_cursor_visible(false);
        view.set_focusable(true);
        view.set_wrap_mode(gtk::WrapMode::WordChar);
        view.set_top_margin(28);
        view.set_bottom_margin(64);
        view.set_pixels_below_lines(4);
        view.set_pixels_inside_wrap(3);
        view.set_vexpand(true);
        view.set_hexpand(true);
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
            fn_defs: RefCell::new(HashMap::new()),
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
            pending_resaltar: Cell::new(None),
            attach_scheduled: Cell::new(false),
            on_toggle: RefCell::new(None),
            on_link: RefCell::new(None),
            is_missing: RefCell::new(None),
            on_rendered: RefCell::new(None),
            plegado: RefCell::new(Rc::new(Vec::new())),
            on_plegar: RefCell::new(None),
            enfoque: RefCell::new(None),
            on_enfocar: RefCell::new(None),
            on_subir_enfoque: RefCell::new(None),
            hover_plegable: Cell::new(None),
            callouts: Cell::new(0),
            colgados: RefCell::new(Vec::new()),
            ancho_flecha: Cell::new(14),
            zona: Cell::new(None),
            plegables: RefCell::new(Rc::new(Vec::new())),
            base_dir: RefCell::new(None),
            replegado: Cell::new(0),
            alternados: RefCell::default(),
            clave_de: RefCell::default(),
            tamano_formulas: Cell::new(None),
            hay_formulas: Cell::new(false),
            graficos_estaticos: Cell::new(false),
            diagramas: RefCell::new(Vec::new()),
            resaltar: RefCell::new(Vec::new()),
            resaltar_programado: Cell::new(false),
        }));
        this.create_tags();
        // Tooltip de notas al pie (012, US1): si el mouse está sobre una referencia, muestra
        // el contenido de la definición. Las coordenadas del mouse (en px del widget) se pasan
        // por `window_to_buffer_coords` antes de buscar el tag bajo el cursor.
        {
            let weak = Rc::downgrade(&this.0);
            let vista = this.0.view.clone();
            let vista_closure = vista.clone();
            vista.set_has_tooltip(true);
            vista.connect_query_tooltip(move |_, x, y, _keyboard, tooltip| {
                let Some(i) = weak.upgrade() else {
                    return false;
                };
                let (tx, ty) =
                    vista_closure.window_to_buffer_coords(gtk::TextWindowType::Widget, x, y);
                // La franja de una flechita dice qué hace y su atajo.
                match Reader(i.clone()).accion_en(tx, ty) {
                    Some(puntero::Accion::Plegar(_)) => {
                        tooltip.set_text(Some("Plegar o desplegar (Ctrl+Arriba)"));
                        return true;
                    }
                    Some(puntero::Accion::Callout(_)) => {
                        tooltip.set_text(Some("Abrir o cerrar"));
                        return true;
                    }
                    None => {}
                }
                let Some(inicio) = crate::editor::posicion::iter_en(&vista_closure, tx, ty) else {
                    return false;
                };
                if let Some(motivo) = Reader(i.clone()).error_de_formula_en(&inicio) {
                    tooltip.set_text(Some(&motivo));
                    return true;
                }
                for tag in inicio.tags() {
                    let Some(nombre) = tag
                        .name()
                        .and_then(|n| n.strip_prefix("fnref-").map(str::to_owned))
                    else {
                        continue;
                    };
                    if let Some(contenido) = i.fn_defs.borrow().get(&nombre) {
                        tooltip.set_text(Some(contenido));
                        return true;
                    }
                }
                false
            });
        }
        this.center();
        this.setup_links();
        this.setup_capas();
        this.setup_puntero();
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
        i.replegado.set(i.replegado.get() + 1);
        i.alternados.borrow_mut().clear();
        i.clave_de.borrow_mut().clear();
        i.hay_formulas.set(false);
        let table = self.buffer().tag_table();
        for b in i.blocks.borrow_mut().drain(..) {
            for t in b.tags {
                table.remove(&t);
            }
        }
        i.links.borrow_mut().clear();
        i.pending.borrow_mut().clear();
        i.pending_line.set(None);
        i.pending_resaltar.set(None);
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
        // Lo recién desplegado tiene que estar volcado antes de ir (046).
        self.completar_plegado();
        let blocks = self.0.blocks.borrow();
        let idx = blocks.partition_point(|b| b.line <= line).saturating_sub(1);
        if let Some(b) = blocks.get(idx) {
            self.0.view.scroll_to_mark(&b.mark, 0.0, true, 0.0, 0.0);
        }
    }

    /// Resalta un momento el bloque que contiene la línea `line` del texto fuente; si la nota se
    /// está volcando, cuando termine (026).
    pub fn resaltar_linea(&self, line: usize) {
        self.completar_plegado();
        let blocks = self.0.blocks.borrow();
        if blocks.is_empty() {
            self.0.pending_resaltar.set(Some(line));
            return;
        }
        let idx = blocks.partition_point(|b| b.line <= line).saturating_sub(1);
        let Some(b) = blocks.get(idx) else { return };
        let buf = self.buffer();
        let a = buf.iter_at_mark(&b.mark);
        let z = blocks
            .get(idx + 1)
            .map_or_else(|| buf.end_iter(), |n| buf.iter_at_mark(&n.mark));
        super::vistas::resaltar(&buf, &a, &z);
    }

    /// Desplaza la vista hasta el encabezado con el texto indicado. Devuelve si lo encontró.
    pub fn scroll_to_heading(&self, heading: &str) -> bool {
        let wanted = heading.trim().to_lowercase();
        self.completar_plegado();
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
        if s.provisorio.get() {
            s.lugares.borrow_mut().push((w, end.offset()));
            s.buffer.insert(end, "\u{fffc}");
            return;
        }
        let anchor = s.buffer.create_child_anchor(end);
        if s.attach_now {
            self.0.view.add_child_at_anchor(&w, &anchor);
        } else {
            s.children.borrow_mut().push((w, anchor));
        }
    }
}

mod actualizar;
mod bloques;
mod capas;
mod consulta;
mod diagramas;
mod enlaces;
mod etiquetas;
mod formulas;
mod generado;
mod graficos;
mod plegado;
mod puntero;
mod resaltar;
#[cfg(test)]
#[path = "resaltar_tests.rs"]
mod resaltar_tests;
use bloques::*;
pub use generado::complete_generated;

#[cfg(test)]
#[path = "reader_tests.rs"]
mod reader_tests;
