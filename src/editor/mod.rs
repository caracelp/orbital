//! Editor de notas (especificación 002): sesión, guardado automático y vistas.
//!
//! El guardado nunca toca el buffer (no se mueve el cursor ni se pierde el deshacer) y la escritura
//! corre en otro hilo. Se guarda 2 s después de la última tecla, y antes de cambiar de nota, renombrar
//! o cerrar ([`Editor::flush`]).

pub mod code;
pub mod completion;
pub mod conflict;
pub mod find;
pub mod html;
pub mod multi_cursor;
pub mod props;
pub mod reader;
pub mod right_sidebar;

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

struct Inner {
    view: sourceview5::View,
    buffer: sourceview5::Buffer,
    code_scroller: gtk::ScrolledWindow,
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
    session: RefCell<Option<EditSession>>,
    vault: RefCell<Option<Arc<Vault>>>,
    /// Ignorar `changed` mientras se carga un texto.
    loading: Cell<bool>,
    /// Cada cambio sube la generación; un temporizador viejo no guarda.
    generation: Cell<u64>,
    saving: Cell<bool>,
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
    reader_matches: RefCell<Vec<(usize, usize)>>,
    reader_current_idx: Cell<Option<usize>>,
    visual_matches: Cell<u32>,
    visual_current_idx: Cell<u32>,
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
        let capa_cursor = gtk::Overlay::new();
        capa_cursor.set_child(Some(&view));
        capa_cursor.add_overlay(&caren);
        let code_scroller = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .child(&capa_cursor)
            .build();
        code::center_column(&code_scroller, &view, 860);
        let reader = reader::Reader::new();
        let root = gtk::Paned::builder()
            .orientation(gtk::Orientation::Horizontal)
            .start_child(&code_scroller)
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
        let search_ctx = sourceview5::SearchContext::new(&buffer, Some(&search_settings));
        search_ctx.set_highlight(false);

        let find_bar = find::FindBar::new();
        let container = gtk::Box::new(gtk::Orientation::Vertical, 0);
        container.append(find_bar.widget());
        container.append(&stack);
        stack.set_vexpand(true);
        stack.set_hexpand(true);

        let completion = completion::LinkCompletionPopover::new(&view);
        let this = Self(Rc::new(Inner {
            view,
            buffer,
            code_scroller,
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
            session: RefCell::new(None),
            vault: RefCell::new(None),
            loading: Cell::new(false),
            generation: Cell::new(0),
            saving: Cell::new(false),
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
                i.completion_active_start.set(None);
                i.completion_dismissed_at.set(Some(start_offset));
                if is_tag {
                    code::insert_tag_completion(&i.buffer, start_offset, &target);
                } else if is_marker {
                    code::insert_marker_completion(&i.buffer, start_offset, &target);
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
                Editor(inner).check_link_completion();
            }
        });

        let weak = Rc::downgrade(&this.0);
        this.0.buffer.connect_changed(move |_| {
            if let Some(inner) = weak.upgrade() {
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
                    if let Some(it) = inner.view.iter_at_location(rect.x(), rect.y()) {
                        inner.reader.scroll_to_line(it.line() as usize);
                    }
                }
            });
        this.setup_multi_cursor();
        this.apply_mode(View::Codigo);
        this
    }

    /// Multi-cursor (006): gestos con Alt, teclas en fase captura y el redibujado del overlay.
    /// Sin cursores extra, nada de esto cambia el comportamiento (FR-7).
    fn setup_multi_cursor(&self) {
        let i = &self.0;

        // Redibujado: scroll del lienzo y cambios del texto.
        let this = self.clone();
        i.code_scroller
            .vadjustment()
            .connect_value_changed(move |_| {
                this.actualizar_carenes();
            });
        let this = self.clone();
        i.code_scroller
            .hadjustment()
            .connect_value_changed(move |_| {
                this.actualizar_carenes();
            });
        let this = self.clone();
        i.buffer.connect_changed(move |_| {
            this.actualizar_carenes();
        });

        // Alt+clic / Alt+arrastre con un GestureDrag (la distancia decide clic vs selección).
        let drag = gtk::GestureDrag::new();
        drag.set_button(1);
        drag.set_propagation_phase(gtk::PropagationPhase::Capture);
        let this = self.clone();
        drag.connect_drag_begin(move |g, _x, _y| {
            let mods = g.current_event_state();
            if !mods.contains(gtk::gdk::ModifierType::ALT_MASK) {
                return;
            }
            let inner = &this.0;
            // Alt+presionar no mueve el cursor principal: se guarda y se repone al soltar.
            let ins = inner.buffer.iter_at_mark(&inner.buffer.get_insert());
            let bound = inner.buffer.iter_at_mark(&inner.buffer.selection_bound());
            inner.extra_base.set(Some((ins.offset(), bound.offset())));
        });
        let this = self.clone();
        drag.connect_drag_end(move |g, dx, dy| {
            let inner = &this.0;
            let Some((ins_off, bound_off)) = inner.extra_base.take() else {
                return; // sin Alt: nada que hacer
            };
            g.set_state(gtk::EventSequenceState::Claimed);
            let (x0, y0) = {
                let (sx, sy) = g.start_point().unwrap_or((0.0, 0.0));
                (sx, sy)
            };
            let (x1, y1) = (x0 + dx, y0 + dy);
            let arrastre = ((x1 - x0) as i32).abs() + ((y1 - y0) as i32).abs() > 4;
            let mut extra = inner.extra.borrow_mut();
            if arrastre {
                let Some(a) = inner.view.iter_at_location(x0 as i32, y0 as i32) else {
                    return;
                };
                let Some(b) = inner.view.iter_at_location(x1 as i32, y1 as i32) else {
                    return;
                };
                extra.add_selection(&inner.buffer, &a, &b);
            } else {
                let Some(it) = inner.view.iter_at_location(x0 as i32, y0 as i32) else {
                    return;
                };
                if !extra.remove_at(&inner.buffer, &it) {
                    extra.add_cursor(&inner.buffer, &it);
                }
            }
            drop(extra);
            // El cursor principal queda donde estaba antes del Alt+clic.
            let ins = inner.buffer.iter_at_offset(ins_off);
            let bound = inner.buffer.iter_at_offset(bound_off);
            inner.buffer.select_range(&ins, &bound);
            this.actualizar_carenes();
        });
        i.view.add_controller(drag);

        // Teclas en captura: solo con cursores extra (FR-7). Esc, columnas y ediciones.
        let teclas = gtk::EventControllerKey::new();
        teclas.set_propagation_phase(gtk::PropagationPhase::Capture);
        let this = self.clone();
        teclas.connect_key_pressed(move |_, keyval, _code, state| {
            let inner = &this.0;
            if inner.extra.borrow().is_empty() {
                return glib::Propagation::Proceed;
            }
            let ctrl = state.contains(gtk::gdk::ModifierType::CONTROL_MASK);
            let alt = state.contains(gtk::gdk::ModifierType::ALT_MASK);
            let shift = state.contains(gtk::gdk::ModifierType::SHIFT_MASK);
            if ctrl && alt && !shift {
                let delta = match keyval {
                    gtk::gdk::Key::Up => -1,
                    gtk::gdk::Key::Down => 1,
                    _ => return glib::Propagation::Proceed,
                };
                let base = multi_cursor::base_columna(&inner.buffer, &inner.extra.borrow(), delta);
                if let Some(it) = multi_cursor::iter_columna_adyacente(&inner.buffer, &base, delta)
                {
                    inner.extra.borrow_mut().add_cursor(&inner.buffer, &it);
                    this.actualizar_carenes();
                    return glib::Propagation::Stop;
                }
                return glib::Propagation::Stop;
            }
            if alt {
                return glib::Propagation::Proceed;
            }
            if keyval == gtk::gdk::Key::Escape {
                inner.extra.borrow_mut().clear(&inner.buffer);
                this.actualizar_carenes();
                return glib::Propagation::Stop;
            }
            // Con el popover de completado visible, sus teclas ganan (US3: solo cursor principal).
            if inner.completion.is_visible() {
                return glib::Propagation::Proceed;
            }
            let edit = if state
                .intersects(gtk::gdk::ModifierType::CONTROL_MASK | gtk::gdk::ModifierType::ALT_MASK)
            {
                return glib::Propagation::Proceed;
            } else {
                match keyval {
                    gtk::gdk::Key::BackSpace => Some(multi_cursor::EditKind::Backspace),
                    gtk::gdk::Key::Delete => Some(multi_cursor::EditKind::Delete),
                    gtk::gdk::Key::Return | gtk::gdk::Key::KP_Enter | gtk::gdk::Key::ISO_Enter => {
                        Some(multi_cursor::EditKind::Insert("\n".into()))
                    }
                    gtk::gdk::Key::Tab | gtk::gdk::Key::KP_Tab => {
                        Some(multi_cursor::EditKind::Insert("\t".into()))
                    }
                    _ => keyval
                        .to_unicode()
                        .filter(|c| !c.is_control())
                        .map(|c| multi_cursor::EditKind::Insert(c.to_string())),
                }
            };
            let Some(edit) = edit else {
                return glib::Propagation::Proceed;
            };
            let _ = multi_cursor::editar_tanda(&inner.buffer, &inner.extra.borrow(), &edit);
            this.actualizar_carenes();
            glib::Propagation::Stop
        });
        i.view.add_controller(teclas);
    }

    /// Recalcula los rectángulos de los cursores extra para el overlay (research R2).
    fn actualizar_carenes(&self) {
        let i = &self.0;
        if i.mode.get() != View::Codigo && i.mode.get() != View::Dividida {
            i.caren.limpiar();
            return;
        }
        let extra = i.extra.borrow();
        if extra.is_empty() {
            i.caren.limpiar();
            return;
        }
        let rects = std::rc::Rc::new(multi_cursor::rects_para_dibujar(&i.view, &i.buffer, &extra));
        i.caren.set_rects(rects);
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

    /// Abre la barra de búsqueda (o búsqueda y reemplazo si `replace` es verdadero).
    pub fn open_find(&self, replace: bool) {
        let i = &self.0;
        let mode = i.mode.get();
        i.find_bar
            .set_supports_replace(matches!(mode, View::Codigo | View::Dividida));
        i.find_bar.set_search_mode(true);

        if let Some(sel) = self.selected_text()
            && !sel.is_empty()
            && !sel.contains('\n')
            && sel.chars().count() <= 100
        {
            i.find_bar.set_search_text(&sel);
        }

        if replace && matches!(mode, View::Codigo | View::Dividida) {
            i.find_bar.set_replace_revealed(true);
            if i.find_bar.search_text().is_empty() {
                i.find_bar.entry().grab_focus();
            } else {
                i.find_bar.replace_entry().grab_focus();
                i.find_bar.replace_entry().select_region(0, -1);
            }
        } else {
            i.find_bar.entry().grab_focus();
            i.find_bar.entry().select_region(0, -1);
        }

        self.find_search(&i.find_bar.search_text());
    }

    /// Cierra la barra de búsqueda y quita los resaltados.
    pub fn close_find(&self) {
        let i = &self.0;
        i.find_bar.set_search_mode(false);
        self.clear_search();
        self.grab_focus();
    }

    /// Devuelve el texto seleccionado actualmente en la vista activa, si hay.
    pub fn selected_text(&self) -> Option<String> {
        let i = &self.0;
        match i.mode.get() {
            View::Codigo | View::Dividida => {
                if i.buffer.has_selection() {
                    let (s, e) = (
                        i.buffer.iter_at_mark(&i.buffer.get_insert()),
                        i.buffer.iter_at_mark(&i.buffer.selection_bound()),
                    );
                    Some(i.buffer.text(&s, &e, false).to_string())
                } else {
                    None
                }
            }
            View::Lectura => {
                let b = i.reader.buffer();
                if b.has_selection() {
                    let (s, e) = (
                        b.iter_at_mark(&b.get_insert()),
                        b.iter_at_mark(&b.selection_bound()),
                    );
                    Some(b.text(&s, &e, false).to_string())
                } else {
                    None
                }
            }
            View::Visual => None,
        }
    }

    /// Limpia los resaltados de búsqueda en todas las vistas.
    pub fn clear_search(&self) {
        let i = &self.0;
        i.search_ctx.set_highlight(false);
        i.search_settings.set_search_text(None);
        i.reader.clear_find_highlights();
        i.reader_matches.borrow_mut().clear();
        i.reader_current_idx.set(None);
        if let Some(fc) = i.visual.find_controller() {
            fc.search_finish();
        }
        i.visual_matches.set(0);
        i.visual_current_idx.set(0);
        i.find_bar.set_counter(None, 0);
    }

    /// Ejecuta la búsqueda en la vista activa.
    pub fn find_search(&self, query: &str) {
        let i = &self.0;
        match i.mode.get() {
            View::Codigo | View::Dividida => {
                if query.is_empty() {
                    i.search_settings.set_search_text(None);
                    i.search_ctx.set_highlight(false);
                    i.find_bar.set_counter(None, 0);
                } else {
                    i.search_settings.set_search_text(Some(query));
                    i.search_ctx.set_highlight(true);
                    let insert = i.buffer.get_insert();
                    let cursor = i.buffer.iter_at_mark(&insert);
                    if let Some((m_start, m_end, _)) = i.search_ctx.forward(&cursor) {
                        i.buffer.select_range(&m_start, &m_end);
                        i.view
                            .scroll_to_iter(&mut m_start.clone(), 0.1, false, 0.0, 0.5);
                        let pos = i.search_ctx.occurrence_position(&m_start, &m_end);
                        let total = i.search_ctx.occurrences_count();
                        if total > 0 && pos > 0 {
                            i.find_bar.set_counter(Some(pos as usize), total as usize);
                        } else if total == 0 {
                            i.find_bar.set_counter(None, 0);
                        }
                    } else {
                        let total = i.search_ctx.occurrences_count();
                        if total <= 0 {
                            i.find_bar.set_counter(None, 0);
                        }
                    }
                }
            }
            View::Lectura => {
                i.reader.clear_find_highlights();
                i.reader_matches.borrow_mut().clear();
                i.reader_current_idx.set(None);
                if query.is_empty() {
                    i.find_bar.set_counter(None, 0);
                    return;
                }
                let b = i.reader.buffer();
                let text = b.text(&b.start_iter(), &b.end_iter(), false);
                let folded_text = orbital_vault::fold(&text);
                let folded_query = orbital_vault::fold(query);
                if folded_query.is_empty() {
                    i.find_bar.set_counter(None, 0);
                    return;
                }

                let query_chars = folded_query.chars().count();
                let mut matches = Vec::new();
                let mut char_offset = 0;
                let mut remainder = &folded_text[..];
                while let Some(idx) = remainder.find(&folded_query) {
                    let chars_before = remainder[..idx].chars().count();
                    let m_start = char_offset + chars_before;
                    let m_end = m_start + query_chars;
                    matches.push((m_start, m_end));
                    let advance_bytes = idx + folded_query.len().max(1);
                    char_offset += chars_before + remainder[idx..advance_bytes].chars().count();
                    remainder = &remainder[advance_bytes..];
                }

                for &(start, end) in &matches {
                    let iter_start = b.iter_at_offset(start as i32);
                    let iter_end = b.iter_at_offset(end as i32);
                    b.apply_tag_by_name("find-highlight", &iter_start, &iter_end);
                }

                if matches.is_empty() {
                    i.find_bar.set_counter(None, 0);
                } else {
                    i.reader_current_idx.set(Some(0));
                    let (start, end) = matches[0];
                    let iter_start = b.iter_at_offset(start as i32);
                    let iter_end = b.iter_at_offset(end as i32);
                    b.apply_tag_by_name("find-current", &iter_start, &iter_end);
                    let mut scroll_iter = iter_start;
                    i.reader
                        .text_view()
                        .scroll_to_iter(&mut scroll_iter, 0.1, false, 0.0, 0.5);
                    i.find_bar.set_counter(Some(1), matches.len());
                }
                *i.reader_matches.borrow_mut() = matches;
            }
            View::Visual => {
                if let Some(fc) = i.visual.find_controller() {
                    if query.is_empty() {
                        fc.search_finish();
                        i.find_bar.set_counter(None, 0);
                    } else {
                        let options = webkit6::FindOptions::CASE_INSENSITIVE.bits()
                            | webkit6::FindOptions::WRAP_AROUND.bits();
                        fc.search(query, options, 1000);
                        fc.count_matches(query, options, 1000);
                    }
                }
            }
        }
    }

    /// Salta a la coincidencia siguiente en la vista activa.
    pub fn find_next(&self) {
        let i = &self.0;
        match i.mode.get() {
            View::Codigo | View::Dividida => {
                let insert = i.buffer.get_insert();
                let sel = i.buffer.selection_bound();
                let iter1 = i.buffer.iter_at_mark(&insert);
                let iter2 = i.buffer.iter_at_mark(&sel);
                let from = iter1.max(iter2);
                if let Some((m_start, m_end, _)) = i.search_ctx.forward(&from) {
                    i.buffer.select_range(&m_start, &m_end);
                    i.view
                        .scroll_to_iter(&mut m_start.clone(), 0.1, false, 0.0, 0.5);
                    let pos = i.search_ctx.occurrence_position(&m_start, &m_end);
                    let total = i.search_ctx.occurrences_count();
                    if pos > 0 && total > 0 {
                        i.find_bar.set_counter(Some(pos as usize), total as usize);
                    }
                }
            }
            View::Lectura => {
                let matches = i.reader_matches.borrow();
                if !matches.is_empty() {
                    let b = i.reader.buffer();
                    let old_idx = i.reader_current_idx.get().unwrap_or(0);
                    let (old_s, old_e) = matches[old_idx];
                    b.remove_tag_by_name(
                        "find-current",
                        &b.iter_at_offset(old_s as i32),
                        &b.iter_at_offset(old_e as i32),
                    );

                    let new_idx = (old_idx + 1) % matches.len();
                    i.reader_current_idx.set(Some(new_idx));
                    let (s, e) = matches[new_idx];
                    b.apply_tag_by_name(
                        "find-current",
                        &b.iter_at_offset(s as i32),
                        &b.iter_at_offset(e as i32),
                    );
                    let mut iter_start = b.iter_at_offset(s as i32);
                    i.reader
                        .text_view()
                        .scroll_to_iter(&mut iter_start, 0.1, false, 0.0, 0.5);
                    i.find_bar.set_counter(Some(new_idx + 1), matches.len());
                }
            }
            View::Visual => {
                if let Some(fc) = i.visual.find_controller() {
                    fc.search_next();
                }
            }
        }
    }

    /// Salta a la coincidencia anterior en la vista activa.
    pub fn find_prev(&self) {
        let i = &self.0;
        match i.mode.get() {
            View::Codigo | View::Dividida => {
                let insert = i.buffer.get_insert();
                let sel = i.buffer.selection_bound();
                let iter1 = i.buffer.iter_at_mark(&insert);
                let iter2 = i.buffer.iter_at_mark(&sel);
                let from = iter1.min(iter2);
                if let Some((m_start, m_end, _)) = i.search_ctx.backward(&from) {
                    i.buffer.select_range(&m_start, &m_end);
                    i.view
                        .scroll_to_iter(&mut m_start.clone(), 0.1, false, 0.0, 0.5);
                    let pos = i.search_ctx.occurrence_position(&m_start, &m_end);
                    let total = i.search_ctx.occurrences_count();
                    if pos > 0 && total > 0 {
                        i.find_bar.set_counter(Some(pos as usize), total as usize);
                    }
                }
            }
            View::Lectura => {
                let matches = i.reader_matches.borrow();
                if !matches.is_empty() {
                    let b = i.reader.buffer();
                    let old_idx = i.reader_current_idx.get().unwrap_or(0);
                    let (old_s, old_e) = matches[old_idx];
                    b.remove_tag_by_name(
                        "find-current",
                        &b.iter_at_offset(old_s as i32),
                        &b.iter_at_offset(old_e as i32),
                    );

                    let new_idx = if old_idx == 0 {
                        matches.len() - 1
                    } else {
                        old_idx - 1
                    };
                    i.reader_current_idx.set(Some(new_idx));
                    let (s, e) = matches[new_idx];
                    b.apply_tag_by_name(
                        "find-current",
                        &b.iter_at_offset(s as i32),
                        &b.iter_at_offset(e as i32),
                    );
                    let mut iter_start = b.iter_at_offset(s as i32);
                    i.reader
                        .text_view()
                        .scroll_to_iter(&mut iter_start, 0.1, false, 0.0, 0.5);
                    i.find_bar.set_counter(Some(new_idx + 1), matches.len());
                }
            }
            View::Visual => {
                if let Some(fc) = i.visual.find_controller() {
                    fc.search_previous();
                }
            }
        }
    }

    /// Reemplaza la coincidencia actual y avanza a la siguiente (Código y Dividida).
    pub fn find_replace(&self, replace_text: &str) {
        let i = &self.0;
        if !matches!(i.mode.get(), View::Codigo | View::Dividida) {
            return;
        }
        let has_sel = i.buffer.has_selection();
        let insert = i.buffer.get_insert();
        let sel = i.buffer.selection_bound();
        let mut iter1 = i.buffer.iter_at_mark(&insert);
        let mut iter2 = i.buffer.iter_at_mark(&sel);
        if iter1.offset() > iter2.offset() {
            std::mem::swap(&mut iter1, &mut iter2);
        }
        let is_match = has_sel && i.search_ctx.occurrence_position(&iter1, &iter2) > 0;

        if is_match {
            let _ = i.search_ctx.replace(&mut iter1, &mut iter2, replace_text);
            if let Some((m_start, m_end, _)) = i.search_ctx.forward(&iter2) {
                i.buffer.select_range(&m_start, &m_end);
                i.view
                    .scroll_to_iter(&mut m_start.clone(), 0.1, false, 0.0, 0.5);
                let pos = i.search_ctx.occurrence_position(&m_start, &m_end);
                let total = i.search_ctx.occurrences_count();
                if pos > 0 && total > 0 {
                    i.find_bar.set_counter(Some(pos as usize), total as usize);
                }
            } else {
                let total = i.search_ctx.occurrences_count();
                if total <= 0 {
                    i.find_bar.set_counter(None, 0);
                }
            }
        } else if let Some((m_start, m_end, _)) = i.search_ctx.forward(&iter2) {
            i.buffer.select_range(&m_start, &m_end);
            i.view
                .scroll_to_iter(&mut m_start.clone(), 0.1, false, 0.0, 0.5);
            let pos = i.search_ctx.occurrence_position(&m_start, &m_end);
            let total = i.search_ctx.occurrences_count();
            if pos > 0 && total > 0 {
                i.find_bar.set_counter(Some(pos as usize), total as usize);
            }
        }
    }

    /// Reemplaza todas las coincidencias en un solo paso deshacible (Código y Dividida).
    pub fn find_replace_all(&self, replace_text: &str) {
        let i = &self.0;
        if !matches!(i.mode.get(), View::Codigo | View::Dividida) {
            return;
        }
        i.buffer.begin_user_action();
        let _ = i.search_ctx.replace_all(replace_text);
        i.buffer.end_user_action();
        i.find_bar.set_counter(None, 0);
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

    /// Cambia de vista (FR-008). Al pasar a Lectura o Dividida, la vista formateada se actualiza.
    pub fn set_view(&self, v: View) {
        let i = &self.0;
        let format = i.format.get();
        // Ctrl+2 es Lectura en `.md` y Visual en `.html`; Dividida no existe en `.html`.
        let v = match (format, v) {
            (Format::Html, View::Lectura | View::Visual) => View::Visual,
            (Format::Html, View::Dividida) => return,
            (Format::Md, View::Visual) => View::Lectura,
            (_, v) => v,
        };
        if v == View::Visual && i.html_scripts.get() {
            self.error("Esta nota tiene JavaScript: editala en Código");
            self.sync_switcher(i.mode.get());
            return;
        }
        if i.mode.get() == v {
            return;
        }
        match format {
            Format::Md => i.md_pref.set(v),
            Format::Html => i.html_pref.set(v),
        }
        if i.mode.get() == View::Visual {
            // Lo último escrito en Visual pasa al texto antes de mostrar el código.
            let this = self.clone();
            glib::spawn_future_local(async move {
                this.sync_visual().await;
                if !this.0.visual_pending.get() {
                    this.enter(v);
                } else {
                    this.sync_switcher(this.0.mode.get());
                }
            });
        } else {
            self.enter(v);
        }
        if let Some(f) = i.on_view.borrow().as_ref() {
            f(format, v);
        }
    }

    fn enter(&self, v: View) {
        self.apply_mode(v);
        match v {
            View::Codigo => {}
            View::Visual => self.show_visual(),
            View::Lectura | View::Dividida => self.render_now(),
        }
    }

    /// Vistas iniciales por formato (las últimas usadas, FR-013), sin avisar el cambio.
    pub fn set_initial_views(&self, md: View, html: View) {
        let i = &self.0;
        i.md_pref.set(if md == View::Visual {
            View::Lectura
        } else {
            md
        });
        i.html_pref.set(match html {
            View::Codigo => View::Codigo,
            _ => View::Visual,
        });
        if i.session.borrow().is_none() {
            self.apply_mode(i.md_pref.get());
        }
    }

    fn sync_switcher(&self, v: View) {
        let i = &self.0;
        if i.switcher.active_name().as_deref() != Some(v.name()) {
            i.switching.set(true);
            i.switcher.set_active_name(Some(v.name()));
            i.switching.set(false);
        }
    }

    fn apply_mode(&self, v: View) {
        let i = &self.0;
        let old_v = i.mode.get();
        i.mode.set(v);
        if i.mode.get() != v && !matches!(v, View::Codigo | View::Dividida) {
            i.extra.borrow_mut().clear(&i.buffer);
            i.caren.limpiar();
        }
        i.stack
            .set_visible_child_name(if v == View::Visual { "visual" } else { "texto" });
        i.code_scroller.set_visible(v != View::Lectura);
        i.reader.widget().set_visible(v != View::Codigo);
        self.sync_switcher(v);
        if v == View::Dividida {
            let max = i.root.max_position();
            if max > 0 {
                i.root.set_position(max / 2);
            }
        }
        i.find_bar
            .set_supports_replace(matches!(v, View::Codigo | View::Dividida));
        if i.find_bar.is_search_mode() && old_v != v {
            let query = i.find_bar.search_text();
            match old_v {
                View::Codigo | View::Dividida => {
                    if !matches!(v, View::Codigo | View::Dividida) {
                        i.search_ctx.set_highlight(false);
                    }
                }
                View::Lectura => {
                    i.reader.clear_find_highlights();
                    i.reader_matches.borrow_mut().clear();
                    i.reader_current_idx.set(None);
                }
                View::Visual => {
                    if let Some(fc) = i.visual.find_controller() {
                        fc.search_finish();
                    }
                    i.visual_matches.set(0);
                    i.visual_current_idx.set(0);
                }
            }
            self.find_search(&query);
        }
    }

    /// Construye el encabezado de la vista formateada (las propiedades).
    pub fn set_on_header(&self, f: impl Fn(&RelPath, &Header) -> Option<gtk::Widget> + 'static) {
        *self.0.on_header.borrow_mut() = Some(Box::new(f));
    }

    /// Clic en un enlace de la vista formateada.
    pub fn set_on_link(&self, f: impl Fn(&RelPath, &str) + 'static) {
        *self.0.on_link.borrow_mut() = Some(Box::new(f));
    }

    /// Desplaza la vista (Lectura, Dividida o Código) hacia el encabezado indicado.
    pub fn scroll_to_heading(&self, heading: &str) {
        let wanted = heading.trim();
        if wanted.is_empty() {
            return;
        }
        if self.0.reader.scroll_to_heading(wanted) {
            return;
        }
        let buf = &self.0.buffer;
        let text = buf.text(&buf.start_iter(), &buf.end_iter(), false);
        let wanted_lower = wanted.to_lowercase();
        for (idx, line) in text.lines().enumerate() {
            let trimmed = line.trim();
            if trimmed.starts_with('#') {
                let title = trimmed.trim_start_matches('#').trim().to_lowercase();
                if title == wanted_lower {
                    if let Some(mut it) = buf.iter_at_line(idx as i32) {
                        self.0.view.scroll_to_iter(&mut it, 0.0, true, 0.0, 0.0);
                    }
                    break;
                }
            }
        }
    }

    /// Cambio de vista elegido por el usuario (para recordarlo).
    pub fn set_on_view(&self, f: impl Fn(Format, View) + 'static) {
        *self.0.on_view.borrow_mut() = Some(Box::new(f));
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

    /// Vuelve a dibujar la vista formateada con el texto actual (análisis en otro hilo).
    pub fn render_now(&self) {
        let i = &self.0;
        if !matches!(i.mode.get(), View::Lectura | View::Dividida) || i.session.borrow().is_none() {
            return;
        }
        let generation = i.render_generation.get() + 1;
        i.render_generation.set(generation);
        let text = i
            .buffer
            .text(&i.buffer.start_iter(), &i.buffer.end_iter(), false)
            .to_string();
        let this = self.clone();
        let place = (i.vault.borrow().clone(), self.current_path());
        let index = i.search_index.borrow().clone();
        glib::spawn_future_local(async move {
            let parsed = gio::spawn_blocking(move || {
                let props = orbital_vault::props::markdown::locate(&text)
                    .map(|r| orbital_vault::props::yaml::parse_block(&text[r]));
                // El YAML resuelto alimenta los marcadores `[%clave]` (005); un frontmatter con
                // error de sintaxis no resuelve nada (queda «faltante»).
                let yaml = props
                    .as_ref()
                    .and_then(|parsed| parsed.as_ref().ok())
                    .map(Vec::as_slice)
                    .unwrap_or(&[]);
                // Con la bóveda se expanden las transclusiones `![[…]]` (lee las notas incrustadas).
                let mut prepared = match &place {
                    (Some(v), Some(from)) => reader::Prepared::with_vault(&text, v, from, yaml),
                    _ => reader::Prepared::new(&text, yaml),
                };
                // Los codeblocks generados (`indice`, `toc`) se completan con el índice y la ruta
                // de la nota (005, T015/T019); los rangos y casillas quedan intactos.
                let idx = index.as_deref().and_then(|i| i.read().ok());
                let genero = reader::complete_generated(
                    &mut prepared.doc,
                    &mut prepared.hashes,
                    idx.as_deref(),
                    place.1.as_ref(),
                );
                (prepared, props, genero)
            })
            .await;
            let i = &this.0;
            if i.render_generation.get() != generation {
                return;
            }
            let Ok((prepared, props, genero)) = parsed else {
                return;
            };
            i.doc_has_generated.set(genero);
            *i.embed_deps.borrow_mut() = prepared.deps.clone();
            let header = || {
                let read_only = i.meta.borrow().as_ref().is_none_or(|m| m.read_only);
                let (entries, error) = match props {
                    Some(Ok(e)) => (e, None),
                    Some(Err(e)) => (Vec::new(), Some(e)),
                    None => (Vec::new(), None),
                };
                let h = Header {
                    entries,
                    error,
                    read_only,
                };
                match (i.on_header.borrow().as_ref(), this.current_path()) {
                    (Some(f), Some(path)) => f(&path, &h),
                    _ => None,
                }
            };
            let base = i.base_dir.borrow().clone();
            i.reader.render(prepared, header, base);
            if i.find_bar.is_search_mode() && matches!(i.mode.get(), View::Lectura | View::Dividida)
            {
                let query = i.find_bar.search_text();
                this.find_search(&query);
            }
        });
    }

    /// Marca o desmarca una casilla: cambia un solo carácter del texto (FR-011).
    fn toggle_task(&self, offset: usize) {
        let b = &self.0.buffer;
        let text = b.text(&b.start_iter(), &b.end_iter(), false).to_string();
        let Some(prefix) = text.get(..offset) else {
            return;
        };
        let chars = prefix.chars().count() as i32;
        let mut start = b.iter_at_offset(chars);
        let mut end = b.iter_at_offset(chars + 1);
        let new = match start.char() {
            ' ' => "x",
            'x' | 'X' => " ",
            _ => return,
        };
        b.begin_user_action();
        b.delete(&mut start, &mut end);
        let mut at = b.iter_at_offset(chars);
        b.insert(&mut at, new);
        b.end_user_action();
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

    /// Carga una nota en el editor. El texto cargado no se puede deshacer.
    pub fn load(&self, vault: Arc<Vault>, note: &Note) {
        let (session, text) = EditSession::open(note);
        let i = &self.0;
        i.extra.borrow_mut().clear(&i.buffer);
        i.caren.limpiar();
        i.reader.reset();
        i.visual.forget();
        i.visual_pending.set(false);
        i.visual_gen.set(i.visual_gen.get() + 1);
        if i.format.get() != note.format || i.switcher.n_toggles() == 0 {
            i.switching.set(true);
            fill_toggles(&i.switcher, note.format);
            i.switching.set(false);
        }
        i.format.set(note.format);
        let scripts =
            note.format == Format::Html && orbital_vault::html_edit::body_has_scripts(&text);
        i.html_scripts.set(scripts);
        if let Some(t) = i.switcher.toggle_by_name("visual") {
            t.set_enabled(!scripts);
            t.set_tooltip(if scripts {
                "Esta nota tiene JavaScript: editala en Código"
            } else {
                "Visual (Ctrl+2)"
            });
        }
        code::set_language(
            &i.buffer,
            match note.format {
                Format::Md => "markdown",
                Format::Html => "html",
            },
        );
        i.loading.set(true);
        i.buffer.begin_irreversible_action();
        i.buffer.set_text(&text);
        i.buffer.end_irreversible_action();
        i.buffer.place_cursor(&i.buffer.start_iter());
        i.loading.set(false);
        i.view.set_editable(!note.read_only);
        i.generation.set(i.generation.get() + 1);
        *i.base_dir.borrow_mut() = note.path.to_path(vault.root()).parent().map(Into::into);
        *i.session.borrow_mut() = Some(session);
        let vault_weak = Arc::downgrade(&vault);
        let from_note = note.path.clone();
        i.reader.set_is_missing(move |url| {
            if url.starts_with("orbital-tag:")
                || url.starts_with("http://")
                || url.starts_with("https://")
                || url.starts_with("mailto:")
            {
                return false;
            }
            let Some(v) = vault_weak.upgrade() else {
                return false;
            };
            v.resolve_link(&from_note, url) == orbital_vault::Resolution::Missing
        });
        *i.vault.borrow_mut() = Some(vault.clone());
        self.refresh_completion_candidates(&vault);
        let mut meta = note.clone();
        meta.source = String::new();
        *i.meta.borrow_mut() = Some(meta);
        self.emit(&Status::Saved(None));
        let mut v = match note.format {
            Format::Md => i.md_pref.get(),
            Format::Html => i.html_pref.get(),
        };
        if v == View::Visual && (scripts || note.read_only) {
            v = View::Codigo;
        }
        self.enter(v);
        if i.find_bar.is_search_mode() {
            let query = i.find_bar.search_text();
            self.find_search(&query);
        }
    }

    /// Cambia (o quita, con `None`) una propiedad sobre el texto del editor, no sobre el disco
    /// (research R7): edita solo sus líneas y lo guarda el autoguardado. Con `rebuild` en `false`,
    /// la lista ya muestra el valor nuevo y no se vuelve a armar (la fila no pierde el foco).
    pub fn edit_prop(
        &self,
        key: &str,
        value: Option<&orbital_vault::props::PropValue>,
        rebuild: bool,
    ) -> Result<(), String> {
        let i = &self.0;
        let b = &i.buffer;
        let text = b.text(&b.start_iter(), &b.end_iter(), false).to_string();
        let note = i
            .meta
            .borrow()
            .as_ref()
            .ok_or("No hay ninguna nota abierta")?
            .reparse(text.clone())
            .map_err(|e| e.to_string())?;
        let new = match value {
            Some(v) => note.with_prop(key, v),
            None => note.without_prop(key),
        }
        .map_err(|e| e.to_string())?;
        if new == text {
            return Ok(());
        }
        if !rebuild {
            let h = orbital_vault::props::markdown::locate(&new)
                .map_or(0, |r| orbital_vault::write::hash(new[r].as_bytes()));
            i.reader.adopt_header(h);
        }
        replace_changed(b, &text, &new);
        if i.mode.get() == View::Visual && rebuild {
            self.visual_header();
        }
        Ok(())
    }

    /// Un cambio en la vista Visual: pasa al texto 300 ms después del último.
    fn visual_changed(&self) {
        let i = &self.0;
        i.visual_pending.set(true);
        self.emit(&Status::Pending);
        let generation = i.visual_gen.get() + 1;
        i.visual_gen.set(generation);
        let this = self.clone();
        glib::timeout_add_local_once(VISUAL_SYNC, move || {
            if this.0.visual_gen.get() == generation {
                glib::spawn_future_local(async move { this.sync_visual().await });
            }
        });
    }

    /// Lleva lo de la vista Visual al texto: solo el `<body>` y el `<style data-orbital>` (FR-015).
    async fn sync_visual(&self) {
        let i = &self.0;
        if !i.visual_pending.get() {
            return;
        }
        let path = self.current_path();
        let generation = i.visual_gen.get();
        let Some((body, css)) = i.visual.contents().await else {
            return;
        };
        // La nota cambió mientras se leía la página: no se mezcla con la otra.
        if self.current_path() != path
            || !i.visual_pending.get()
            || i.visual_gen.get() != generation
        {
            return;
        }
        let b = &i.buffer;
        let text = b.text(&b.start_iter(), &b.end_iter(), false).to_string();
        let Some(spliced) = orbital_vault::html_edit::splice_body(&text, &body) else {
            self.error("La nota no tiene <body>: editala en Código");
            return;
        };
        let Some(new) = orbital_vault::html_edit::upsert_orbital_style(&spliced, &css) else {
            self.error("La nota no tiene </head>: agregalo en Código para guardar los estilos");
            return;
        };
        i.visual_pending.set(false);
        if new != text {
            replace_changed(b, &text, &new);
        }
        i.visual.mark_loaded(&new);
    }

    /// Muestra la vista Visual con el texto actual.
    fn show_visual(&self) {
        let i = &self.0;
        let b = &i.buffer;
        let text = b.text(&b.start_iter(), &b.end_iter(), false).to_string();
        if orbital_vault::html_edit::body_range(&text).is_none()
            || orbital_vault::html_edit::upsert_orbital_style(&text, "/* comprobar head */")
                .is_none()
        {
            self.error("Para editar en Visual, agregá <head></head> y <body></body> en Código");
            self.set_view(View::Codigo);
            return;
        }
        if orbital_vault::html_edit::body_has_scripts(&text) {
            i.html_scripts.set(true);
            if let Some(toggle) = i.switcher.toggle_by_name("visual") {
                toggle.set_enabled(false);
                toggle.set_tooltip("Esta nota tiene JavaScript: editala en Código");
            }
            self.error("Esta nota tiene JavaScript: editala en Código");
            self.set_view(View::Codigo);
            return;
        }
        let (Some(vault), Some(path)) = (i.vault.borrow().clone(), self.current_path()) else {
            return;
        };
        let file = path.to_path(vault.root());
        let dir = file.parent().unwrap_or(vault.root());
        i.visual.load(&text, dir, vault.root());
        self.visual_header();
        self.visual_embeds();
        self.visual_markers();
    }

    /// Resuelve los marcadores `[%clave]` de la nota `.html` en segundo plano y se los pasa a
    /// la vista Visual (005, T010): cada marcador se muestra con su valor pero no es editable,
    /// y `body()` los restituye al fuente al serializar.
    fn visual_markers(&self) {
        let i = &self.0;
        let b = &i.buffer;
        let text = b.text(&b.start_iter(), &b.end_iter(), false).to_string();
        let this = self.clone();
        glib::spawn_future_local(async move {
            let result = gio::spawn_blocking(move || {
                let head = orbital_vault::props::html::scan(&text);
                let yaml = head
                    .block
                    .and_then(|region| orbital_vault::props::yaml::parse_block(&text[region]).ok())
                    .unwrap_or_default();
                orbital_vault::markers::resolve_markers_html(&text, &yaml)
            })
            .await;
            let Ok(resolved) = result else { return };
            this.0.visual.set_markers(&resolved);
        });
    }

    /// Calcula en segundo plano las tarjetas de las transclusiones `![[…]]` de la nota `.html`
    /// y se las pasa a la vista Visual (US5, T027).
    fn visual_embeds(&self) {
        let i = &self.0;
        let (Some(vault), Some(from)) = (i.vault.borrow().clone(), self.current_path()) else {
            return;
        };
        let b = &i.buffer;
        let text = b.text(&b.start_iter(), &b.end_iter(), false).to_string();
        let this = self.clone();
        glib::spawn_future_local(async move {
            let from_c = from.clone();
            let result = gio::spawn_blocking(move || {
                let dir = from_c
                    .as_str()
                    .rsplit_once('/')
                    .map_or("", |(d, _)| d)
                    .to_owned();
                // Enlace de una tarjeta: la nota, relativa a la carpeta de la nota `.html`.
                let href = move |p: &RelPath| relative_href(&dir, p.as_str());
                let body =
                    orbital_vault::html_edit::body_range(&text).map_or(&text[..0], |r| &text[r]);
                let mut cards: Vec<(String, String)> = Vec::new();
                let mut deps = Vec::new();
                for occ in orbital_vault::links::extract_links(body, true) {
                    if occ.kind != orbital_vault::links::LinkKind::Transclusion {
                        continue;
                    }
                    let raw = body[occ.byte_range.clone()].to_owned();
                    if cards.iter().any(|(k, _)| *k == raw) {
                        continue;
                    }
                    let anchor = occ.target.anchor.as_deref();
                    let (embed, d) = vault.embed_one(&from_c, &occ.target.path, anchor);
                    deps.extend(d);
                    let html = orbital_vault::transclusion::card_html(
                        &occ.target.path,
                        anchor,
                        &embed,
                        &href,
                    );
                    cards.push((raw, html));
                }
                deps.sort();
                deps.dedup();
                (cards, deps)
            })
            .await;
            let Ok((cards, deps)) = result else { return };
            if this.current_path() != Some(from) {
                return;
            }
            *this.0.embed_deps.borrow_mut() = deps;
            this.0.visual.set_embeds(&cards);
        });
    }

    /// Las propiedades arriba de la vista Visual.
    fn visual_header(&self) {
        let i = &self.0;
        let b = &i.buffer;
        let text = b.text(&b.start_iter(), &b.end_iter(), false).to_string();
        let note = i.meta.borrow().as_ref().and_then(|m| m.reparse(text).ok());
        let Some(note) = note else { return };
        let (entries, error) = match note.props {
            Some(p) => (p.entries, p.error),
            None => (Vec::new(), None),
        };
        let h = Header {
            entries,
            error,
            read_only: note.read_only,
        };
        let w = match (i.on_header.borrow().as_ref(), self.current_path()) {
            (Some(f), Some(path)) => f(&path, &h),
            _ => None,
        };
        i.visual.set_header(w);
    }

    /// Deshace la última acción en el buffer de código.
    pub fn undo(&self) -> bool {
        let b = &self.0.buffer;
        if b.can_undo() {
            b.undo();
            true
        } else {
            false
        }
    }

    /// Deshace el último cambio si nada cambió después (para el «Deshacer» de quitar una propiedad).
    pub fn undo_if_last(&self, edits: u64) -> bool {
        let b = &self.0.buffer;
        if self.0.edits.get() != edits || !b.can_undo() {
            return false;
        }
        b.undo();
        true
    }

    /// Contador de cambios del texto (ver [`Editor::undo_if_last`]).
    pub fn edits(&self) -> u64 {
        self.0.edits.get()
    }

    /// Una nota de la bóveda cambió (se guardó o la cambió otro programa). Si la nota abierta la
    /// incrusta con `![[…]]`, la vista formateada se vuelve a dibujar: solo cambia esa tarjeta y
    /// no se pierde la posición de lectura (FR-016).
    pub fn dependency_changed(&self, path: &RelPath) {
        if !self.0.embed_deps.borrow().contains(path) {
            return;
        }
        if self.0.mode.get() == View::Visual {
            self.visual_embeds();
        } else {
            self.render_now();
        }
    }

    /// Muestra la línea `line` (desde 1) en la vista actual, sin cambiarla: el cursor del código
    /// va ahí y la vista formateada se desplaza cuando termina de dibujarse (retroenlaces, US4).
    pub fn reveal_line(&self, line: usize) {
        let i = &self.0;
        let target = line.saturating_sub(1);
        if let Some(it) = i.buffer.iter_at_line(target as i32) {
            i.buffer.place_cursor(&it);
            i.view
                .scroll_to_mark(&i.buffer.get_insert(), 0.0, true, 0.0, 0.3);
        }
        if matches!(i.mode.get(), View::Lectura | View::Dividida) {
            i.reader.scroll_to_line_when_ready(target);
        }
    }

    /// Pasa a la vista Código y pone el cursor en una línea.
    pub fn show_code_line(&self, line: i32) {
        self.set_view(View::Codigo);
        let b = &self.0.buffer;
        if let Some(it) = b.iter_at_line(line) {
            b.place_cursor(&it);
            self.0
                .view
                .scroll_to_iter(&mut it.clone(), 0.1, false, 0.0, 0.0);
        }
        self.0.view.grab_focus();
    }

    /// Cierra la nota sin guardar (después de un `flush`, o para recargarla).
    pub fn close(&self) {
        self.0.extra.borrow_mut().clear(&self.0.buffer);
        self.0.caren.limpiar();
        self.0.completion.popdown();
        self.0.generation.set(self.0.generation.get() + 1);
        self.0.reader.reset();
        *self.0.session.borrow_mut() = None;
    }

    fn check_link_completion(&self) {
        let i = &self.0;
        if i.loading.get()
            || !i.view.has_focus()
            || i.mode.get() == View::Lectura
            || i.mode.get() == View::Visual
        {
            return;
        }

        let link_query = code::link_query_at_cursor(&i.buffer);
        let tag_query = link_query
            .is_none()
            .then(|| code::tag_query_at_cursor(&i.buffer, i.format.get() == Format::Html))
            .flatten();
        let is_tag = tag_query.is_some();
        let marker_query = tag_query
            .is_none()
            .then(|| code::marker_query_at_cursor(&i.buffer, i.format.get() == Format::Html))
            .flatten();
        let is_marker = marker_query.is_some();
        let query_info = link_query.or(tag_query).or(marker_query);
        i.completion_is_tag.set(is_tag);
        i.completion_is_marker.set(is_marker);
        match query_info {
            Some((query, start_offset)) => {
                if i.completion_dismissed_at.get() == Some(start_offset) {
                    return;
                }
                i.completion_active_start.set(Some(start_offset));
                let matches = if is_tag {
                    i.completion.filter_tag_query(&query)
                } else if is_marker {
                    // Las claves salen de la nota abierta: inline de toda la nota y frontmatter.
                    // Es un recorrido lineal del texto, chico al escribir.
                    let text = i
                        .buffer
                        .text(&i.buffer.start_iter(), &i.buffer.end_iter(), false);
                    i.completion.set_markers(completion::marker_candidates(
                        &text,
                        i.format.get() == Format::Html,
                    ));
                    i.completion.filter_marker_query(&query)
                } else {
                    i.completion.filter(&query)
                };
                if matches > 0 {
                    let rect = code::cursor_popup_rect(&i.view, &i.buffer);
                    i.completion.popup_at(&rect);
                } else {
                    i.completion.popdown();
                }
            }
            None => {
                i.completion_dismissed_at.set(None);
                i.completion_active_start.set(None);
                if i.completion.is_visible() {
                    i.completion.popdown();
                }
            }
        }
    }

    pub fn set_tag_candidates(&self, tags: Vec<completion::TagCandidate>) {
        self.0.completion.set_tags(tags);
        self.check_link_completion();
    }

    /// Arma los candidatos de autocompletado de la bóveda (US3): primero los nombres, enseguida;
    /// después títulos y alias, leídos en otro hilo. Solo cuando cambia la bóveda, no al abrir
    /// cada nota (con 5000 notas serían 5000 lecturas por nota abierta).
    pub fn refresh_completion_candidates(&self, vault: &Arc<Vault>) {
        let i = &self.0;
        if std::ptr::eq(i.completion_vault.borrow().as_ptr(), Arc::as_ptr(vault)) {
            return;
        }
        *i.completion_vault.borrow_mut() = Arc::downgrade(vault);
        let key = Arc::as_ptr(vault);
        let notes = |v: &Vault| -> Vec<RelPath> {
            v.entries()
                .iter()
                .filter(|e| {
                    e.zone == Zone::Wiki
                        && matches!(e.kind, EntryKind::AtomNote | EntryKind::SynthesisNote)
                })
                .map(|e| e.path.clone())
                .collect()
        };
        let mut baseline: Vec<_> = notes(vault)
            .into_iter()
            .map(|p| completion::CompletionCandidate::new(p, None, Vec::new()))
            .collect();
        completion::qualify_duplicates(&mut baseline);
        i.completion.set_candidates(baseline.clone());
        i.visual.set_candidates(baseline);

        let vault = vault.clone();
        let this = self.clone();
        glib::spawn_future_local(async move {
            let full = gio::spawn_blocking(move || {
                let mut list: Vec<_> = notes(&vault)
                    .into_iter()
                    .map(|path| {
                        let props = vault.read_props(&path).ok().flatten();
                        let entries = props.as_ref().map_or(&[][..], |p| &p.entries[..]);
                        let title = orbital_vault::props::find(entries, "title")
                            .and_then(|v| v.value.as_text())
                            .map(str::to_owned);
                        let aliases = orbital_vault::props::find(entries, "aliases")
                            .map(|v| match &v.value {
                                orbital_vault::props::PropValue::List(l) => l
                                    .iter()
                                    .filter_map(|x| x.as_text().map(str::to_owned))
                                    .collect(),
                                orbital_vault::props::PropValue::Text(t) => vec![t.clone()],
                                _ => Vec::new(),
                            })
                            .unwrap_or_default();
                        completion::CompletionCandidate::new(path, title, aliases)
                    })
                    .collect();
                completion::qualify_duplicates(&mut list);
                list
            })
            .await
            .unwrap_or_default();
            // Si mientras tanto se abrió otra bóveda, estos candidatos ya no sirven.
            if std::ptr::eq(this.0.completion_vault.borrow().as_ptr(), key) {
                this.0.completion.set_candidates(full.clone());
                this.0.visual.set_candidates(full);
            }
        });
    }

    fn on_changed(&self) {
        let i = &self.0;
        if i.loading.get() {
            return;
        }
        if i.format.get() == Format::Html && i.mode.get() == View::Codigo {
            // Se vuelve a validar al entrar a Visual, no en cada tecla.
            i.html_scripts.set(false);
            if let Some(toggle) = i.switcher.toggle_by_name("visual") {
                toggle.set_enabled(true);
                toggle.set_tooltip("Visual (Ctrl+2)");
            }
        }
        i.edits.set(i.edits.get() + 1);
        self.check_link_completion();
        let Some(session) = i.session.borrow_mut().as_mut().map(|s| {
            s.edited();
            s.state().clone()
        }) else {
            return;
        };
        if i.saving.get() {
            i.edited_during_save.set(true);
        }
        if matches!(session, SessionState::Conflict { .. }) {
            self.emit(&Status::Conflict);
            return;
        }
        self.emit(&Status::Pending);
        self.schedule(AUTOSAVE);
        if i.mode.get() != View::Codigo {
            let generation = i.render_generation.get() + 1;
            i.render_generation.set(generation);
            let this = self.clone();
            glib::timeout_add_local_once(RENDER_DELAY, move || {
                if this.0.render_generation.get() == generation {
                    this.render_now();
                }
            });
        }
    }

    /// Guarda dentro de `delay`, salvo que llegue otra tecla antes.
    fn schedule(&self, delay: Duration) {
        let generation = self.0.generation.get() + 1;
        self.0.generation.set(generation);
        let this = self.clone();
        glib::timeout_add_local_once(delay, move || {
            if this.0.generation.get() == generation {
                glib::spawn_future_local(async move { this.save_now().await });
            }
        });
    }

    /// Guarda ya lo pendiente y espera a que termine (FR-002).
    pub async fn flush(&self) {
        self.sync_visual().await;
        if self.0.visual_pending.get() {
            self.error("No se pudo sincronizar la edición visual; los cambios siguen en la página");
            return;
        }
        // Si hay una escritura en curso, espera a que termine y vuelve a revisar.
        while self.0.saving.get() {
            glib::timeout_future(Duration::from_millis(20)).await;
        }
        self.0.generation.set(self.0.generation.get() + 1);
        self.save_now().await;
    }

    async fn save_now(&self) {
        let i = &self.0;
        if i.saving.get() {
            return;
        }
        let Some(vault) = i.vault.borrow().clone() else {
            return;
        };
        let text = i
            .buffer
            .text(&i.buffer.start_iter(), &i.buffer.end_iter(), false)
            .to_string();
        let job = match i.session.borrow_mut().as_mut() {
            Some(s) => s.prepare_save(&text),
            None => return,
        };
        let Some(job) = job else {
            let state = i.session.borrow().as_ref().map(|s| s.state().clone());
            if let Some(SessionState::Clean) = state {
                self.emit(&Status::Saved(self.last_saved()));
            }
            return;
        };
        i.saving.set(true);
        i.edited_during_save.set(false);
        self.emit(&Status::Saving);
        let ctx = write_ctx::now(Some(vault.config()));
        let at = ctx.time;
        // Historial de versiones (010): el contenido que se está reemplazando queda en
        // .orbital/historial/. Solo notas; los errores son avisos discretos (SC-1: una escritura
        // atómica extra).
        if let Some(path) = self.current_path() {
            let (v, p) = (vault.clone(), path.clone());
            let guardado = gio::spawn_blocking(move || {
                let abs = p.to_path(v.root());
                match std::fs::read(&abs) {
                    Ok(bytes) => match String::from_utf8(bytes) {
                        Ok(previo) => {
                            orbital_vault::historial::guardar_version(&v, &p, &previo, "")
                        }
                        Err(_) => Ok(false),
                    },
                    Err(_) => Ok(false), // primera versión: nada que historiar
                }
            })
            .await;
            if let Ok(Err(error)) = guardado {
                self.error(&format!(
                    "No se pudo guardar la versión en el historial: {error}"
                ));
            }
        }
        let result = gio::spawn_blocking(move || job.run(&vault, &ctx)).await;
        i.saving.set(false);
        let Ok(result) = result else {
            self.error("No se pudo guardar la nota");
            return;
        };
        let edited = i.edited_during_save.get();
        if let orbital_vault::session::SaveResult::Written { hash, also, .. } = &result
            && let (Some(f), Some(path)) = (i.on_saved.borrow().as_ref(), self.current_path())
        {
            f(&path, *hash);
            for (other, h) in also {
                f(other, *h);
            }
        }
        let outcome = match i.session.borrow_mut().as_mut() {
            Some(s) => s.apply(result, at, edited),
            None => return,
        };
        match outcome {
            Ok(SaveOutcome::Saved { .. }) if edited => {
                self.emit(&Status::Pending);
                self.schedule(AUTOSAVE);
            }
            Ok(SaveOutcome::Saved { .. } | SaveOutcome::NothingToSave) => {
                self.emit(&Status::Saved(self.last_saved()));
            }
            Ok(SaveOutcome::Conflict) => {
                self.emit(&Status::Conflict);
                self.error(
                    "La nota cambió en el disco mientras la editabas. Tus cambios siguen acá y no se guardan solos.",
                );
            }
            Err(e) => {
                let failures = i.session.borrow().as_ref().map_or(1, EditSession::failures);
                let wait = RETRIES[(failures as usize).saturating_sub(1).min(RETRIES.len() - 1)];
                self.emit(&Status::Failed(e.to_string()));
                self.error(&format!(
                    "No se pudo guardar: {e}. Reintento en {wait} s; tus cambios siguen acá."
                ));
                self.schedule(Duration::from_secs(wait));
            }
        }
    }

    fn last_saved(&self) -> Option<String> {
        self.0
            .session
            .borrow()
            .as_ref()
            .and_then(EditSession::last_saved)
            .map(|t| format!("{:02}:{:02}", t.hour, t.minute))
    }

    /// Obtiene el texto actual del buffer de edición.
    pub fn text(&self) -> String {
        let b = &self.0.buffer;
        b.text(&b.start_iter(), &b.end_iter(), false).to_string()
    }

    /// Formato de la nota abierta.
    pub fn format(&self) -> Format {
        self.0.format.get()
    }

    /// Reemplaza el texto del editor marcando el cambio como editado (historial 010, restaurar;
    /// también para pruebas).
    pub fn replace_text(&self, new: &str) {
        let b = &self.0.buffer;
        let text = self.text();
        replace_changed(b, &text, new);
    }

    /// «Conservar la mía»: guarda los cambios del editor; la versión del disco queda en respaldo.
    pub async fn keep_mine(&self, vault: Arc<Vault>) -> Result<(), orbital_vault::VaultError> {
        self.sync_visual().await;
        let text = self.text();
        let session = self.0.session.borrow().clone().ok_or_else(|| {
            orbital_vault::VaultError::Io(std::io::Error::other("no hay sesión de edición"))
        })?;
        let (v, ctx) = (vault.clone(), write_ctx::now(Some(vault.config())));
        let res = gio::spawn_blocking(move || {
            let mut s = session;
            s.keep_mine(&v, &text, &ctx).map(|()| s)
        })
        .await
        .map_err(|_| {
            orbital_vault::VaultError::Io(std::io::Error::other("hilo de guardado cancelado"))
        })?;

        match res {
            Ok(updated) => {
                let path = updated.path().clone();
                let hash = updated.base_hash();
                *self.0.session.borrow_mut() = Some(updated);
                if let Some(f) = self.0.on_saved.borrow().as_ref() {
                    f(&path, hash);
                }
                self.emit(&Status::Saved(self.last_saved()));
                Ok(())
            }
            Err(e) => {
                self.emit(&Status::Failed(e.to_string()));
                Err(e)
            }
        }
    }

    /// «Usar la del disco»: carga la versión del disco en el editor; la del editor queda en respaldo.
    pub async fn use_disk(&self, vault: Arc<Vault>) -> Result<(), orbital_vault::VaultError> {
        self.sync_visual().await;
        let mine = self.text();
        let session = self.0.session.borrow().clone().ok_or_else(|| {
            orbital_vault::VaultError::Io(std::io::Error::other("no hay sesión de edición"))
        })?;
        let (v, ctx) = (vault.clone(), write_ctx::now(Some(vault.config())));
        let res = gio::spawn_blocking(move || {
            let mut s = session;
            s.use_disk(&v, &mine, &ctx).map(|disk_text| (s, disk_text))
        })
        .await
        .map_err(|_| {
            orbital_vault::VaultError::Io(std::io::Error::other("hilo de guardado cancelado"))
        })?;

        match res {
            Ok((updated, disk_text)) => {
                *self.0.session.borrow_mut() = Some(updated);
                let i = &self.0;
                i.loading.set(true);
                i.buffer.begin_irreversible_action();
                i.buffer.set_text(&disk_text);
                i.buffer.end_irreversible_action();
                i.buffer.place_cursor(&i.buffer.start_iter());
                i.loading.set(false);
                i.generation.set(i.generation.get() + 1);
                if i.mode.get() == View::Visual {
                    self.show_visual();
                } else if i.mode.get() != View::Codigo {
                    self.render_now();
                }
                self.emit(&Status::Saved(None));
                Ok(())
            }
            Err(e) => {
                self.emit(&Status::Failed(e.to_string()));
                Err(e)
            }
        }
    }

    /// «Descartar a respaldo»: respalda la versión del editor sin tocar el disco y cierra la nota.
    pub async fn discard_to_backup(
        &self,
        vault: Arc<Vault>,
    ) -> Result<(), orbital_vault::VaultError> {
        self.sync_visual().await;
        let mine = self.text();
        let session = self.0.session.borrow().clone().ok_or_else(|| {
            orbital_vault::VaultError::Io(std::io::Error::other("no hay sesión de edición"))
        })?;
        let (v, ctx) = (vault.clone(), write_ctx::now(Some(vault.config())));
        let res = gio::spawn_blocking(move || {
            let mut s = session;
            s.discard_to_backup(&v, &mine, &ctx)
        })
        .await
        .map_err(|_| {
            orbital_vault::VaultError::Io(std::io::Error::other("hilo de guardado cancelado"))
        })?;

        res?;
        self.close();
        Ok(())
    }

    /// Un cambio externo de la nota abierta, con el hash que tiene ahora en el disco.
    pub fn external_change(&self, disk_hash: u64) -> ExternalAction {
        let action = self
            .0
            .session
            .borrow_mut()
            .as_mut()
            .map_or(ExternalAction::Reload, |s| s.external_change(disk_hash));
        if action == ExternalAction::AskUser {
            self.emit(&Status::Conflict);
        }
        action
    }

    /// Desarrollo: inserta `chars` caracteres de a uno al final del texto y devuelve, en ms, el
    /// trabajo de cada tecla: la inserción más el cálculo y dibujo de su cuadro (sin contar la
    /// espera de la sincronización vertical). Para comprobar FR-022 (< 16 ms).
    pub async fn measure_typing(&self, chars: usize) -> Vec<f64> {
        use std::time::Instant;
        let view = self.0.view.clone();
        let Some(clock) = view.frame_clock() else {
            return Vec::new();
        };
        // Marcas del primer cuadro que empieza después de la tecla (t0).
        let t0: Rc<Cell<Instant>> = Rc::new(Cell::new(Instant::now()));
        let update_at: Rc<Cell<Option<Instant>>> = Rc::default();
        let painted: Rc<Cell<Option<Instant>>> = Rc::default();
        let (u, t) = (update_at.clone(), t0.clone());
        let h1 = clock.connect_update(move |_| {
            let now = Instant::now();
            if u.get().is_none() && now > t.get() {
                u.set(Some(now));
            }
        });
        let (pa, u2) = (painted.clone(), update_at.clone());
        let h2 = clock.connect_after_paint(move |_| {
            if pa.get().is_none() && u2.get().is_some() {
                pa.set(Some(Instant::now()));
            }
        });
        let buffer = self.0.buffer.clone();
        buffer.place_cursor(&buffer.end_iter());
        let mut out = Vec::with_capacity(chars);
        for k in 0..chars {
            update_at.set(None);
            painted.set(None);
            let start = Instant::now();
            t0.set(start);
            buffer.insert_at_cursor(if k % 60 == 59 { "\n" } else { "a" });
            let insert = start.elapsed();
            view.queue_draw();
            // Si la ventana está tapada, GNOME puede frenar los cuadros: no se espera más de 1 s.
            while painted.get().is_none() && start.elapsed() < Duration::from_secs(1) {
                glib::timeout_future(Duration::from_millis(1)).await;
            }
            let frame = match (update_at.get(), painted.get()) {
                (Some(u), Some(p)) => p.saturating_duration_since(u),
                _ => continue,
            };
            out.push((insert + frame).as_secs_f64() * 1000.0);
        }
        clock.disconnect(h1);
        clock.disconnect(h2);
        out
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
