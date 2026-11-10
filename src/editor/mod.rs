//! Editor de notas (especificación 002): sesión, guardado automático y vistas.
//!
//! El guardado nunca toca el buffer (no se mueve el cursor ni se pierde el deshacer) y la escritura
//! corre en otro hilo. Se guarda 2 s después de la última tecla, y antes de cambiar de nota, renombrar
//! o cerrar ([`Editor::flush`]).

pub mod code;
pub mod completion;
pub mod conflict;
pub mod html;
pub mod props;
pub mod reader;

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use gtk::prelude::*;
use gtk::{gio, glib};
use orbital_vault::session::{EditSession, ExternalAction, SaveOutcome, SessionState};
use orbital_vault::{EntryKind, Format, Note, RelPath, Vault, Zone};

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
    /// Bóveda de la que salieron los candidatos de autocompletado. `Weak`: mientras exista, la
    /// dirección no se reutiliza, así que comparar punteros es seguro.
    completion_vault: RefCell<std::sync::Weak<Vault>>,
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
        let code_scroller = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .child(&view)
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
        let completion = completion::LinkCompletionPopover::new(&view);
        let this = Self(Rc::new(Inner {
            view,
            buffer,
            code_scroller,
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
            completion_vault: RefCell::new(std::sync::Weak::new()),
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
            }
            glib::Propagation::Proceed
        });
        this.0.view.add_controller(key_ctrl);

        let weak = Rc::downgrade(&this.0);
        this.0
            .completion
            .connect_selected(move |_candidate, target| {
                let Some(i) = weak.upgrade() else { return };
                if let Some(start_offset) = i.completion_active_start.get() {
                    code::insert_link_completion(&i.buffer, &i.view, start_offset, &target);
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
        this.apply_mode(View::Codigo);
        this
    }

    /// El widget del editor: código y vista formateada en un panel, o la vista Visual.
    pub fn widget(&self) -> &gtk::Stack {
        &self.0.stack
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
        i.mode.set(v);
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
        glib::spawn_future_local(async move {
            let parsed = gio::spawn_blocking(move || {
                let prepared = reader::Prepared::new(&text);
                let props = orbital_vault::props::markdown::locate(&text)
                    .map(|r| orbital_vault::props::yaml::parse_block(&text[r]));
                (prepared, props)
            })
            .await;
            let i = &this.0;
            if i.render_generation.get() != generation {
                return;
            }
            let Ok((prepared, props)) = parsed else {
                return;
            };
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
            if url.starts_with("http://")
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
        self.0.generation.set(self.0.generation.get() + 1);
        self.0.reader.reset();
        *self.0.session.borrow_mut() = None;
    }

    fn check_link_completion(&self) {
        let i = &self.0;
        if i.loading.get() || i.mode.get() == View::Lectura || i.mode.get() == View::Visual {
            return;
        }

        let query_info = code::link_query_at_cursor(&i.buffer);
        match query_info {
            Some((query, start_offset)) => {
                if i.completion_dismissed_at.get() == Some(start_offset) {
                    return;
                }
                i.completion_active_start.set(Some(start_offset));
                let matches = i.completion.filter(&query);
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

    /// Reemplaza el texto del editor marcando el cambio como editado (para pruebas).
    #[cfg(test)]
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
