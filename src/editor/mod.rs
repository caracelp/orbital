//! Editor de notas (especificación 002): sesión, guardado automático y vistas.
//!
//! El guardado nunca toca el buffer (no se mueve el cursor ni se pierde el deshacer) y la escritura
//! corre en otro hilo. Se guarda 2 s después de la última tecla, y antes de cambiar de nota, renombrar
//! o cerrar ([`Editor::flush`]).

mod buscar_en_nota;
pub mod code;
pub mod completion;
pub mod conflict;
mod cursores;
mod ecuacion;
#[cfg(test)]
#[path = "ecuaciones_tests.rs"]
mod ecuaciones_tests;
mod edicion_visual;
mod enfoque;
pub mod find;
mod flechas;
#[cfg(test)]
#[path = "formulas_tests.rs"]
mod formulas_tests;
mod guardado;
pub mod html;
mod ids_bloque;
mod medicion;
pub mod multi_cursor;
pub mod notas_al_pie;
pub mod outliner;
mod plegado;
pub(crate) mod posicion;
pub mod props;
pub mod reader;
pub mod right_sidebar;
mod sugerencias;
mod vista_previa;
mod vistas;

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use gtk::prelude::*;
use gtk::{gio, glib};
use orbital_vault::session::{EditSession, ExternalAction, SaveOutcome, SessionState};
use orbital_vault::{EntryKind, Format, Note, RelPath, Vault, Zone};
use sourceview5::prelude::*;

use crate::write_ctx;

/// Segundos sin teclas antes de guardar (FR-002).
const AUTOSAVE: Duration = Duration::from_secs(2);
/// Reintentos después de un guardado fallido (research R4).
const RETRIES: [u64; 3] = [5, 15, 60];

/// Estado visible del guardado (FR-007).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Status {
    /// Todo guardado; con la hora del último guardado de esta sesión, si hubo.
    Saved(Option<String>),
    Pending,
    Saving,
    Conflict,
    Failed(String),
}

impl Status {
    /// Texto para la interfaz (no depende del color).
    pub fn label(&self) -> String {
        match self {
            Self::Saved(Some(at)) => format!("Guardado a las {at}"),
            Self::Saved(None) => String::new(),
            Self::Pending => "Cambios sin guardar".into(),
            Self::Saving => "Guardando…".into(),
            Self::Conflict => "Cambió en el disco: no se guarda solo".into(),
            Self::Failed(_) => "No se pudo guardar; reintentando".into(),
        }
    }
}

type StatusFn = Box<dyn Fn(&Status)>;
type ErrorFn = Box<dyn Fn(&str)>;
type SavedFn = Box<dyn Fn(&RelPath, u64)>;
type HeaderFn = Box<dyn Fn(&RelPath, &Header) -> Option<gtk::Widget>>;

/// Lo que se muestra arriba de la vista formateada: las propiedades de la nota.
pub struct Header {
    pub entries: Vec<orbital_vault::props::PropEntry>,
    /// Error de sintaxis del bloque (la lista no se edita).
    pub error: Option<orbital_vault::props::PropError>,
    pub read_only: bool,
}
type LinkFn = Box<dyn Fn(&RelPath, &str)>;
type ViewFn = Box<dyn Fn(Format, View)>;
/// Cambio del texto de la nota abierta (OnStage, 014).
type TextChangedFn = Box<dyn Fn()>;

/// Vistas de una nota `.md` (FR-008).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum View {
    Codigo,
    Lectura,
    Dividida,
    /// Edición directa de una nota `.html` (US4).
    Visual,
}

impl View {
    pub fn name(self) -> &'static str {
        match self {
            Self::Codigo => "codigo",
            Self::Lectura => "lectura",
            Self::Dividida => "dividida",
            Self::Visual => "visual",
        }
    }
    pub fn from_name(s: &str) -> Option<Self> {
        match s {
            "codigo" => Some(Self::Codigo),
            "lectura" => Some(Self::Lectura),
            "dividida" => Some(Self::Dividida),
            "visual" => Some(Self::Visual),
            _ => None,
        }
    }
}

/// Espera después de un cambio en la vista Visual antes de llevarlo al texto.
const VISUAL_SYNC: Duration = Duration::from_millis(300);

/// Actualización de la vista formateada después de la última tecla (research R3).
const RENDER_DELAY: Duration = Duration::from_millis(150);

/// Los bloques con id de una nota, para las sugerencias de `[[nota#^` (026).
type BloquesDeNota = Rc<Vec<orbital_vault::bloques::BloqueConId>>;

struct Inner {
    view: sourceview5::View,
    buffer: sourceview5::Buffer,
    code_scroller: gtk::ScrolledWindow,
    /// Scroller de Código con la capa de cursores extra encima (el hijo izquierdo del `Paned`).
    code_capa: gtk::Overlay,
    /// Cursores extra del multi-cursor (006): el principal sigue siendo la selección nativa.
    extra: RefCell<multi_cursor::ExtraCursors>,
    /// Selección nativa guardada durante un Alt+clic para reponerla al soltar.
    extra_base: Cell<Option<(i32, i32)>>,
    caren: multi_cursor::CarenOverlay,
    reader: reader::Reader,
    root: gtk::Paned,
    /// `texto` (código y vista formateada) o `visual` (notas `.html`).
    stack: gtk::Stack,
    visual: html::Visual,
    format: Cell<Format>,
    /// Última vista usada por formato (FR-013).
    md_pref: Cell<View>,
    html_pref: Cell<View>,
    /// La nota `.html` tiene JavaScript en el cuerpo: sin vista Visual.
    html_scripts: Cell<bool>,
    /// Hay cambios de la vista Visual que todavía no pasaron al texto.
    visual_pending: Cell<bool>,
    visual_gen: Cell<u64>,
    /// Ignorar el cambio de vista del selector mientras se rearma.
    switching: Cell<bool>,
    switcher: adw::ToggleGroup,
    mode: Cell<View>,
    render_generation: Cell<u64>,
    /// La nota en pantalla tiene codeblocks generados: al llegar un snapshot de tags hay que
    /// volver a renderizar (005, T016).
    doc_has_generated: Cell<bool>,
    base_dir: RefCell<Option<std::path::PathBuf>>,
    on_header: RefCell<Option<HeaderFn>>,
    on_link: RefCell<Option<LinkFn>>,
    on_view: RefCell<Option<ViewFn>>,
    on_text_changed: RefCell<Option<TextChangedFn>>,
    session: RefCell<Option<EditSession>>,
    vault: RefCell<Option<Arc<Vault>>>,
    /// Ignorar `changed` mientras se carga un texto.
    loading: Cell<bool>,
    /// Cada cambio sube la generación; un temporizador viejo no guarda.
    generation: Cell<u64>,
    saving: Cell<bool>,
    /// Los cambios del buffer son ids de bloque que agrega el guardado (026): no son teclas.
    asignando_ids: Cell<bool>,
    edited_during_save: Cell<bool>,
    on_status: RefCell<Option<StatusFn>>,
    on_error: RefCell<Option<ErrorFn>>,
    on_saved: RefCell<Option<SavedFn>>,
    /// La nota abierta sin su texto (ruta, zona, formato): para editar propiedades sobre el texto
    /// del editor con `Note::reparse`.
    meta: RefCell<Option<Note>>,
    /// Cuenta los cambios del texto; sirve para saber si «Deshacer» sigue siendo lo último.
    edits: Cell<u64>,
    completion: completion::LinkCompletionPopover,
    completion_dismissed_at: Cell<Option<i32>>,
    completion_active_start: Cell<Option<i32>>,
    completion_is_tag: Cell<bool>,
    completion_is_marker: Cell<bool>,
    /// El autocompletado abierto es de tipos de relación (049).
    completion_is_relation: Cell<bool>,
    /// Bloques de la nota de `[[nota#^` (026): `(nota, bloques)`; `None` mientras se lee.
    completion_bloques: RefCell<Option<(String, Option<BloquesDeNota>)>>,
    /// Bóveda de la que salieron los candidatos de autocompletado. `Weak`: mientras exista, la
    /// dirección no se reutiliza, así que comparar punteros es seguro.
    completion_vault: RefCell<std::sync::Weak<Vault>>,
    /// Notas que incrusta la nota abierta (US5).
    embed_deps: RefCell<Vec<RelPath>>,
    /// Índice de búsqueda compartido con el panel, para los codeblocks `indice` (005).
    search_index: RefCell<Option<Arc<std::sync::RwLock<orbital_vault::SearchIndex>>>>,
    container: gtk::Box,
    find_bar: find::FindBar,
    search_ctx: sourceview5::SearchContext,
    search_settings: sourceview5::SearchSettings,
    reader_matches: RefCell<Vec<buscar_en_nota::Coincidencia>>,
    reader_current_idx: Cell<Option<usize>>,
    visual_matches: Cell<u32>,
    visual_current_idx: Cell<u32>,
    /// Outliner plegable (028): marcas al comienzo de la línea de cada elemento plegado.
    pliegues: RefCell<Vec<gtk::TextMark>>,
    /// Última pasada del árbol de plegables.
    arbol: RefCell<Option<plegado::Arbol>>,
    /// Sube con cada cambio del texto (también los del guardado y la carga).
    texto_gen: Cell<u64>,
    /// Reconciliación pendiente (300 ms después de la última tecla).
    plegado_gen: Cell<u64>,
    /// Ids del guardado y outliner: sus ediciones no despliegan (research R5).
    sin_desplegar: Cell<bool>,
    tag_plegado: gtk::TextTag,
    /// Lo que queda fuera del elemento enfocado (028, research R9).
    tag_fuera: gtk::TextTag,
    /// Marca al comienzo del elemento enfocado (gravedad derecha).
    enfoque: RefCell<Option<gtk::TextMark>>,
    /// Miga de pan del enfoque, arriba del editor.
    migas: enfoque::Migas,
    on_enfoque: RefCell<Option<Box<dyn Fn()>>>,
    /// Canal de las flechitas (`flechas.rs`).
    flechas: sourceview5::GutterRendererPixbuf,
    on_plegado: RefCell<Option<plegado::PlegadoFn>>,
    /// Últimas claves avisadas a la ventana.
    plegado_claves: RefCell<Vec<orbital_vault::Clave>>,
    /// Sube con cada cambio de lo plegado (el canal guarda lo oculto de cada cuadro).
    plegado_version: Cell<u64>,
    /// Globito con la fórmula bajo el cursor en Código (050); se arma con la primera.
    vista_previa: RefCell<Option<vista_previa::VistaPrevia>>,
    vista_previa_gen: Cell<u64>,
}

/// El editor de la nota abierta.
#[derive(Clone)]
pub struct Editor(Rc<Inner>);

impl Default for Editor {
    fn default() -> Self {
        Self::new()
    }
}

impl Editor {
    pub fn new() -> Self {
        let (view, buffer) = code::new_view();
        // El editor va directo en la ventana con desplazamiento: así GTK solo calcula la parte
        // visible del texto. Dentro de un adw::Clamp calculaba todo el documento en cada tecla
        // (30 ms por tecla con 100 KB). El ancho de la columna se limita con márgenes.
        let caren = multi_cursor::CarenOverlay::new();
        // La vista va directo en su `ScrolledWindow` (si no, GTK dispone el documento entero en
        // cada tecla: 8 ms con 100 KB contra 2 ms). La capa de los cursores extra (006) envuelve el
        // scroller desde afuera y dibuja en coordenadas de lo visible.
        let code_scroller = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .child(&view)
            .build();
        let flechas = flechas::nuevo();
        code::center_column(&code_scroller, &view, 860, Some(&flechas));
        let (tag_plegado, tag_fuera) = plegado::crear_etiquetas(&buffer);
        let capa_cursor = gtk::Overlay::new();
        capa_cursor.set_child(Some(&code_scroller));
        capa_cursor.add_overlay(&caren);
        let reader = reader::Reader::new();
        let root = gtk::Paned::builder()
            .orientation(gtk::Orientation::Horizontal)
            .start_child(&capa_cursor)
            .end_child(reader.widget())
            .resize_start_child(true)
            .resize_end_child(true)
            .shrink_start_child(false)
            .shrink_end_child(false)
            .wide_handle(true)
            .build();
        // Dividida reparte el ancho según una proporción (la mitad, o la que deje el usuario al
        // arrastrar), y la vuelve a aplicar cuando cambia el ancho disponible.
        let ratio = Rc::new(Cell::new(0.5_f64));
        let applying = Rc::new(Cell::new(false));
        {
            let (ratio, applying) = (ratio.clone(), applying.clone());
            root.connect_max_position_notify(move |p| {
                let max = p.max_position();
                if max > 0 && p.start_child().is_some_and(|c| c.is_visible()) {
                    applying.set(true);
                    p.set_position((max as f64 * ratio.get()) as i32);
                    applying.set(false);
                }
            });
        }
        root.connect_position_notify(move |p| {
            let max = p.max_position();
            let both = p.start_child().is_some_and(|c| c.is_visible())
                && p.end_child().is_some_and(|c| c.is_visible());
            if !applying.get() && both && max > 0 {
                ratio.set((p.position() as f64 / max as f64).clamp(0.2, 0.8));
            }
        });
        let switcher = adw::ToggleGroup::new();
        fill_toggles(&switcher, Format::Md);
        let visual = html::Visual::new();
        let stack = gtk::Stack::new();
        stack.add_named(&root, Some("texto"));
        stack.add_named(visual.widget(), Some("visual"));
        switcher.add_css_class("round");
        switcher
            .upcast_ref::<gtk::Widget>()
            .update_property(&[gtk::accessible::Property::Label("Vista de la nota")]);

        let search_settings = sourceview5::SearchSettings::new();
        search_settings.set_case_sensitive(false);
        search_settings.set_wrap_around(true);
        // Buscar en la nota encuentra también lo plegado (028): al ir a la coincidencia, se
        // despliega.
        if search_settings.find_property("visible-only").is_some() {
            search_settings.set_property("visible-only", false);
        }
        let search_ctx = sourceview5::SearchContext::new(&buffer, Some(&search_settings));
        search_ctx.set_highlight(false);

        let find_bar = find::FindBar::new();
        let container = gtk::Box::new(gtk::Orientation::Vertical, 0);
        stack.set_vexpand(true);
        stack.set_hexpand(true);

        let completion = completion::LinkCompletionPopover::new(&view);
        let this = Self(Rc::new_cyclic(|weak| {
            let migas = enfoque::Migas::new(weak.clone());
            container.append(find_bar.widget());
            container.append(migas.widget());
            container.append(&stack);
            Inner {
                view,
                buffer,
                code_scroller,
                code_capa: capa_cursor,
                extra: RefCell::new(multi_cursor::ExtraCursors::new()),
                extra_base: Cell::new(None),
                caren,
                reader,
                root,
                stack,
                visual,
                format: Cell::new(Format::Md),
                md_pref: Cell::new(View::Lectura),
                html_pref: Cell::new(View::Visual),
                html_scripts: Cell::new(false),
                visual_pending: Cell::new(false),
                visual_gen: Cell::new(0),
                switching: Cell::new(false),
                switcher,
                mode: Cell::new(View::Codigo),
                render_generation: Cell::new(0),
                doc_has_generated: Cell::new(false),
                base_dir: RefCell::new(None),
                on_header: RefCell::new(None),
                on_link: RefCell::new(None),
                on_view: RefCell::new(None),
                on_text_changed: RefCell::new(None),
                session: RefCell::new(None),
                vault: RefCell::new(None),
                loading: Cell::new(false),
                generation: Cell::new(0),
                saving: Cell::new(false),
                asignando_ids: Cell::new(false),
                edited_during_save: Cell::new(false),
                on_status: RefCell::new(None),
                on_error: RefCell::new(None),
                on_saved: RefCell::new(None),
                meta: RefCell::new(None),
                edits: Cell::new(0),
                completion,
                completion_dismissed_at: Cell::new(None),
                completion_active_start: Cell::new(None),
                completion_is_tag: Cell::new(false),
                completion_is_marker: Cell::new(false),
                completion_is_relation: Cell::new(false),
                completion_bloques: RefCell::new(None),
                completion_vault: RefCell::new(std::sync::Weak::new()),
                embed_deps: RefCell::new(Vec::new()),
                search_index: RefCell::new(None),
                container,
                find_bar,
                search_ctx,
                search_settings,
                reader_matches: RefCell::new(Vec::new()),
                reader_current_idx: Cell::new(None),
                visual_matches: Cell::new(0),
                visual_current_idx: Cell::new(0),
                pliegues: RefCell::new(Vec::new()),
                arbol: RefCell::new(None),
                texto_gen: Cell::new(0),
                plegado_gen: Cell::new(0),
                sin_desplegar: Cell::new(false),
                tag_plegado,
                tag_fuera,
                enfoque: RefCell::new(None),
                migas,
                on_enfoque: RefCell::new(None),
                flechas,
                on_plegado: RefCell::new(None),
                plegado_claves: RefCell::new(Vec::new()),
                plegado_version: Cell::new(0),
                vista_previa: RefCell::new(None),
                vista_previa_gen: Cell::new(0),
            }
        }));
        let key_ctrl = gtk::EventControllerKey::new();
        key_ctrl.set_propagation_phase(gtk::PropagationPhase::Capture);
        let weak = Rc::downgrade(&this.0);
        key_ctrl.connect_key_pressed(move |_, keyval, _keycode, _state| {
            let Some(i) = weak.upgrade() else {
                return glib::Propagation::Proceed;
            };
            if i.completion.is_visible() {
                match keyval {
                    gtk::gdk::Key::Up => {
                        i.completion.select_prev();
                        return glib::Propagation::Stop;
                    }
                    gtk::gdk::Key::Down => {
                        i.completion.select_next();
                        return glib::Propagation::Stop;
                    }
                    gtk::gdk::Key::Escape => {
                        i.completion.popdown();
                        i.completion_dismissed_at
                            .set(i.completion_active_start.get());
                        return glib::Propagation::Stop;
                    }
                    gtk::gdk::Key::Return | gtk::gdk::Key::KP_Enter | gtk::gdk::Key::Tab
                        if i.completion.activate_selected() =>
                    {
                        return glib::Propagation::Stop;
                    }
                    _ => {}
                }
            } else if i.find_bar.is_search_mode() && keyval == gtk::gdk::Key::Escape {
                Editor(i.clone()).close_find();
                return glib::Propagation::Stop;
            }
            glib::Propagation::Proceed
        });
        this.0.view.add_controller(key_ctrl);

        let weak = Rc::downgrade(&this.0);
        this.0.find_bar.set_on_search(move |query| {
            if let Some(i) = weak.upgrade() {
                Editor(i).find_search(query);
            }
        });

        let weak = Rc::downgrade(&this.0);
        this.0.find_bar.set_on_next(move || {
            if let Some(i) = weak.upgrade() {
                Editor(i).find_next();
            }
        });

        let weak = Rc::downgrade(&this.0);
        this.0.find_bar.set_on_prev(move || {
            if let Some(i) = weak.upgrade() {
                Editor(i).find_prev();
            }
        });

        let weak = Rc::downgrade(&this.0);
        this.0.find_bar.set_on_replace(move |text| {
            if let Some(i) = weak.upgrade() {
                Editor(i).find_replace(text);
            }
        });

        let weak = Rc::downgrade(&this.0);
        this.0.find_bar.set_on_replace_all(move |text| {
            if let Some(i) = weak.upgrade() {
                Editor(i).find_replace_all(text);
            }
        });

        let weak = Rc::downgrade(&this.0);
        this.0.find_bar.set_on_close(move || {
            if let Some(i) = weak.upgrade() {
                Editor(i).close_find();
            }
        });

        let weak = Rc::downgrade(&this.0);
        this.0
            .search_ctx
            .connect_occurrences_count_notify(move |ctx| {
                let Some(i) = weak.upgrade() else { return };
                if !i.find_bar.is_search_mode()
                    || !matches!(i.mode.get(), View::Codigo | View::Dividida)
                {
                    return;
                }
                let query = i.find_bar.search_text();
                if query.is_empty() {
                    i.find_bar.set_counter(None, 0);
                    return;
                }
                let total = ctx.occurrences_count();
                if total < 0 {
                    return;
                }
                if total == 0 {
                    i.find_bar.set_counter(None, 0);
                    return;
                }
                let insert = i.buffer.get_insert();
                let sel = i.buffer.selection_bound();
                let mut iter1 = i.buffer.iter_at_mark(&insert);
                let mut iter2 = i.buffer.iter_at_mark(&sel);
                if iter1.offset() > iter2.offset() {
                    std::mem::swap(&mut iter1, &mut iter2);
                }
                let pos = ctx.occurrence_position(&iter1, &iter2);
                if pos > 0 {
                    i.find_bar.set_counter(Some(pos as usize), total as usize);
                } else if let Some((m_start, m_end, _)) = ctx.forward(&iter1) {
                    i.buffer.select_range(&m_start, &m_end);
                    let pos = ctx.occurrence_position(&m_start, &m_end);
                    let p = if pos > 0 { pos as usize } else { 1 };
                    i.find_bar.set_counter(Some(p), total as usize);
                } else {
                    i.find_bar.set_counter(Some(1), total as usize);
                }
            });

        let weak = Rc::downgrade(&this.0);
        this.0.visual.set_on_find_counted(move |count| {
            let Some(i) = weak.upgrade() else { return };
            if i.mode.get() == View::Visual && i.find_bar.is_search_mode() {
                i.visual_matches.set(count);
                if count == 0 {
                    i.find_bar.set_counter(None, 0);
                } else {
                    let curr = i.visual_current_idx.get();
                    let curr_opt = if curr > 0 { Some(curr as usize) } else { None };
                    i.find_bar.set_counter(curr_opt, count as usize);
                }
            }
        });

        let weak = Rc::downgrade(&this.0);
        this.0.visual.set_on_find_found(move |match_idx| {
            let Some(i) = weak.upgrade() else { return };
            if i.mode.get() == View::Visual && i.find_bar.is_search_mode() {
                i.visual_current_idx.set(match_idx);
                let total = i.visual_matches.get();
                if total > 0 && match_idx > 0 {
                    i.find_bar
                        .set_counter(Some(match_idx as usize), total as usize);
                }
            }
        });

        let weak = Rc::downgrade(&this.0);
        this.0.reader.set_on_rendered(move || {
            let Some(i) = weak.upgrade() else { return };
            if i.find_bar.is_search_mode() && matches!(i.mode.get(), View::Lectura | View::Dividida)
            {
                let query = i.find_bar.search_text();
                Editor(i).find_search(&query);
            }
        });

        let weak = Rc::downgrade(&this.0);
        this.0.visual.set_on_ready(move || {
            let Some(i) = weak.upgrade() else { return };
            if i.find_bar.is_search_mode() && i.mode.get() == View::Visual {
                let query = i.find_bar.search_text();
                Editor(i).find_search(&query);
            }
        });

        let weak = Rc::downgrade(&this.0);
        this.0.completion.connect_selected(move |target| {
            let Some(i) = weak.upgrade() else { return };
            if let Some(start_offset) = i.completion_active_start.get() {
                // Cerrar el contexto antes de editar: los cambios del buffer disparan detección.
                let is_tag = i.completion_is_tag.get();
                let is_marker = i.completion_is_marker.get();
                let is_relation = i.completion_is_relation.get();
                i.completion_active_start.set(None);
                i.completion_dismissed_at.set(Some(start_offset));
                if is_tag {
                    code::insert_tag_completion(&i.buffer, start_offset, &target);
                } else if is_marker {
                    code::insert_marker_completion(&i.buffer, start_offset, &target);
                } else if is_relation {
                    code::insert_relation_completion(&i.buffer, start_offset, &target);
                } else {
                    code::insert_link_completion(&i.buffer, &i.view, start_offset, &target);
                }
                i.completion.popdown();
                i.completion_dismissed_at.set(Some(start_offset));
            }
        });

        let weak = Rc::downgrade(&this.0);
        this.0.buffer.connect_cursor_position_notify(move |_| {
            if let Some(inner) = weak.upgrade() {
                let e = Editor(inner);
                e.check_link_completion();
                e.programar_vista_previa();
            }
        });

        let weak = Rc::downgrade(&this.0);
        this.0.buffer.connect_changed(move |_| {
            if let Some(inner) = weak.upgrade() {
                // Primero: cualquier otro manejador que pida el árbol de plegables ya lo ve viejo.
                inner.texto_gen.set(inner.texto_gen.get() + 1);
                Editor(inner).on_changed();
            }
        });
        let weak = Rc::downgrade(&this.0);
        this.0.switcher.connect_active_name_notify(move |g| {
            if let (Some(inner), Some(v)) = (
                weak.upgrade(),
                g.active_name().and_then(|n| View::from_name(&n)),
            ) && !inner.switching.get()
            {
                Editor(inner).set_view(v);
            }
        });
        let weak = Rc::downgrade(&this.0);
        this.0.visual.set_on_input(move || {
            if let Some(inner) = weak.upgrade() {
                Editor(inner).visual_changed();
            }
        });
        let weak = Rc::downgrade(&this.0);
        this.0.visual.set_on_error(move |msg| {
            if let Some(inner) = weak.upgrade() {
                Editor(inner).error(msg);
            }
        });
        let weak = Rc::downgrade(&this.0);
        this.0.reader.set_on_toggle(move |offset| {
            if let Some(inner) = weak.upgrade() {
                Editor(inner).toggle_task(offset);
            }
        });
        let weak = Rc::downgrade(&this.0);
        this.0.reader.set_on_link(move |url| {
            if let Some(inner) = weak.upgrade()
                && let (Some(f), Some(path)) = (
                    inner.on_link.borrow().as_ref(),
                    Editor(inner.clone()).current_path(),
                )
            {
                f(&path, url);
            }
        });
        let weak = Rc::downgrade(&this.0);
        this.0.visual.set_on_link(move |url| {
            if let Some(inner) = weak.upgrade()
                && let (Some(f), Some(path)) = (
                    inner.on_link.borrow().as_ref(),
                    Editor(inner.clone()).current_path(),
                )
            {
                f(&path, url);
            }
        });
        // En Dividida, la vista formateada sigue al código.
        let weak = Rc::downgrade(&this.0);
        this.0
            .code_scroller
            .vadjustment()
            .connect_value_changed(move |_| {
                if let Some(inner) = weak.upgrade()
                    && inner.mode.get() == View::Dividida
                {
                    let rect = inner.view.visible_rect();
                    let (it, _) = inner.view.line_at_y(rect.y());
                    inner.reader.scroll_to_line(it.line() as usize);
                }
            });
        this.setup_multi_cursor();
        this.setup_plegado();
        let weak = Rc::downgrade(&this.0);
        this.0.reader.set_on_plegar(move |linea| {
            if let Some(i) = weak.upgrade() {
                Editor(i).alternar_plegado(linea);
            }
        });
        this.instalar_ecuaciones();
        this.apply_mode(View::Codigo);
        this
    }

    /// El widget del editor: barra de búsqueda arriba y el contenido abajo.
    pub fn widget(&self) -> &gtk::Box {
        &self.0.container
    }

    /// Barra de búsqueda en la nota abierta.
    #[allow(dead_code)]
    pub fn find_bar(&self) -> &find::FindBar {
        &self.0.find_bar
    }

    /// Pone el foco en la vista activa del editor.
    pub fn grab_focus(&self) {
        let i = &self.0;
        match i.mode.get() {
            View::Codigo | View::Dividida => {
                i.view.grab_focus();
            }
            View::Lectura => {
                i.reader.text_view().grab_focus();
            }
            View::Visual => {
                if let Some(w) = i.visual.web_view() {
                    w.grab_focus();
                }
            }
        }
    }

    /// Selector de vista para la cabecera.
    pub fn switcher(&self) -> &adw::ToggleGroup {
        &self.0.switcher
    }

    /// Construye el encabezado de la vista formateada (las propiedades).
    pub fn set_on_header(&self, f: impl Fn(&RelPath, &Header) -> Option<gtk::Widget> + 'static) {
        *self.0.on_header.borrow_mut() = Some(Box::new(f));
    }

    /// Clic en un enlace de la vista formateada.
    pub fn set_on_link(&self, f: impl Fn(&RelPath, &str) + 'static) {
        *self.0.on_link.borrow_mut() = Some(Box::new(f));
    }

    /// Cambio de vista elegido por el usuario (para recordarlo).
    pub fn set_on_view(&self, f: impl Fn(Format, View) + 'static) {
        *self.0.on_view.borrow_mut() = Some(Box::new(f));
    }

    /// Avisa cada cambio del texto de la nota abierta (edición en vivo de OnStage, 014).
    /// Va por tecla: el receptor aplica su propia pausa.
    pub fn set_on_text_changed(&self, f: impl Fn() + 'static) {
        *self.0.on_text_changed.borrow_mut() = Some(Box::new(f));
    }

    /// Índice de búsqueda para los codeblocks `indice` (005); se pasa junto con el del panel.
    pub fn set_search_index(
        &self,
        index: Option<Arc<std::sync::RwLock<orbital_vault::SearchIndex>>>,
    ) {
        *self.0.search_index.borrow_mut() = index;
    }

    /// La nota en pantalla tiene bloques generados (005).
    pub fn has_generated(&self) -> bool {
        self.0.doc_has_generated.get()
    }

    /// Avisos de estado del guardado.
    pub fn set_on_status(&self, f: impl Fn(&Status) + 'static) {
        *self.0.on_status.borrow_mut() = Some(Box::new(f));
    }

    /// Avisos de error para mostrar al usuario.
    pub fn set_on_error(&self, f: impl Fn(&str) + 'static) {
        *self.0.on_error.borrow_mut() = Some(Box::new(f));
    }

    /// Avisa cada escritura propia (ruta y hash), para que el monitor de archivos no la tome como
    /// un cambio externo.
    pub fn set_on_saved(&self, f: impl Fn(&RelPath, u64) + 'static) {
        *self.0.on_saved.borrow_mut() = Some(Box::new(f));
    }

    fn emit(&self, s: &Status) {
        if let Some(f) = self.0.on_status.borrow().as_ref() {
            f(s);
        }
    }

    fn error(&self, msg: &str) {
        if let Some(f) = self.0.on_error.borrow().as_ref() {
            f(msg);
        }
    }

    /// Ruta de la nota abierta, si hay una.
    pub fn current_path(&self) -> Option<RelPath> {
        self.0.session.borrow().as_ref().map(|s| s.path().clone())
    }

    /// El texto de la nota (para los contadores de la barra inferior).
    pub fn buffer(&self) -> &sourceview5::Buffer {
        &self.0.buffer
    }

    /// Línea del cursor en el texto (desde 1), para las pruebas.
    #[cfg(test)]
    pub fn cursor_line(&self) -> i32 {
        let b = &self.0.buffer;
        b.iter_at_mark(&b.get_insert()).line() + 1
    }

    /// Hay cambios que todavía no están en el disco.
    pub fn has_pending(&self) -> bool {
        self.0
            .session
            .borrow()
            .as_ref()
            .is_some_and(EditSession::has_pending)
            || self.0.saving.get()
            || self.0.visual_pending.get()
    }

    /// Obtiene el texto actual del buffer de edición.
    pub fn text(&self) -> String {
        let b = &self.0.buffer;
        b.text(&b.start_iter(), &b.end_iter(), true).to_string()
    }

    /// Formato de la nota abierta.
    pub fn format(&self) -> Format {
        self.0.format.get()
    }
}

/// Reemplaza en el buffer solo la parte que difiere entre `old` y `new`, como una sola acción
/// del usuario (se puede deshacer y el resto del texto, con el cursor, queda donde estaba).
fn replace_changed(b: &sourceview5::Buffer, old: &str, new: &str) {
    let prefix = old
        .chars()
        .zip(new.chars())
        .take_while(|(a, b)| a == b)
        .count();
    let old_rest: Vec<char> = old.chars().skip(prefix).collect();
    let new_rest: Vec<char> = new.chars().skip(prefix).collect();
    let suffix = old_rest
        .iter()
        .rev()
        .zip(new_rest.iter().rev())
        .take_while(|(a, b)| a == b)
        .count();
    let inserted: String = new_rest[..new_rest.len() - suffix].iter().collect();
    let start = prefix as i32;
    let end = (prefix + old_rest.len() - suffix) as i32;
    b.begin_user_action();
    let mut from = b.iter_at_offset(start);
    let mut to = b.iter_at_offset(end);
    b.delete(&mut from, &mut to);
    let mut at = b.iter_at_offset(start);
    b.insert(&mut at, &inserted);
    b.end_user_action();
}

/// Las vistas del selector según el formato de la nota.
fn fill_toggles(group: &adw::ToggleGroup, format: Format) {
    group.remove_all();
    let toggles: &[(&str, &str, &str)] = match format {
        Format::Md => &[
            ("codigo", "Código", "Código (Ctrl+1)"),
            ("lectura", "Lectura", "Lectura (Ctrl+2)"),
            ("dividida", "Dividida", "Dividida (Ctrl+3)"),
        ],
        Format::Html => &[
            ("codigo", "Código", "Código (Ctrl+1)"),
            ("visual", "Visual", "Visual (Ctrl+2)"),
        ],
    };
    for (name, label, tip) in toggles {
        group.add(
            adw::Toggle::builder()
                .name(*name)
                .label(*label)
                .tooltip(*tip)
                .build(),
        );
    }
}

/// Ruta de `target` (relativa a la bóveda) vista desde la carpeta `dir`, con `/` y espacios
/// codificados: el `href` de una tarjeta de transclusión en una nota `.html`.
fn relative_href(dir: &str, target: &str) -> String {
    let a: Vec<&str> = dir.split('/').filter(|p| !p.is_empty()).collect();
    let b: Vec<&str> = target.split('/').collect();
    let common = a
        .iter()
        .zip(&b[..b.len().saturating_sub(1)])
        .take_while(|(x, y)| x == y)
        .count();
    let mut parts: Vec<&str> = vec![".."; a.len() - common];
    parts.extend(&b[common..]);
    parts.join("/").replace(' ', "%20")
}
