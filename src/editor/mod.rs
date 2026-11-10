//! Editor de notas (especificación 002): sesión, guardado automático y vistas.
//!
//! El guardado nunca toca el buffer (no se mueve el cursor ni se pierde el deshacer) y la escritura
//! corre en otro hilo. Se guarda 2 s después de la última tecla, y antes de cambiar de nota, renombrar
//! o cerrar ([`Editor::flush`]).

pub mod code;
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
use orbital_vault::{Format, Note, RelPath, Vault};

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

struct Inner {
    view: sourceview5::View,
    buffer: sourceview5::Buffer,
    widget: gtk::ScrolledWindow,
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
        let widget = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .child(&view)
            .build();
        code::center_column(&widget, &view, 860);
        let this = Self(Rc::new(Inner {
            view,
            buffer,
            widget,
            session: RefCell::new(None),
            vault: RefCell::new(None),
            loading: Cell::new(false),
            generation: Cell::new(0),
            saving: Cell::new(false),
            edited_during_save: Cell::new(false),
            on_status: RefCell::new(None),
            on_error: RefCell::new(None),
            on_saved: RefCell::new(None),
        }));
        let weak = Rc::downgrade(&this.0);
        this.0.buffer.connect_changed(move |_| {
            if let Some(inner) = weak.upgrade() {
                Editor(inner).on_changed();
            }
        });
        this
    }

    pub fn widget(&self) -> &gtk::ScrolledWindow {
        &self.0.widget
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
    }

    /// Carga una nota en el editor. El texto cargado no se puede deshacer.
    pub fn load(&self, vault: Arc<Vault>, note: &Note) {
        let (session, text) = EditSession::open(note);
        let i = &self.0;
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
        *i.session.borrow_mut() = Some(session);
        *i.vault.borrow_mut() = Some(vault);
        self.emit(&Status::Saved(None));
    }

    /// Cierra la nota sin guardar (después de un `flush`, o para recargarla).
    pub fn close(&self) {
        self.0.generation.set(self.0.generation.get() + 1);
        *self.0.session.borrow_mut() = None;
    }

    fn on_changed(&self) {
        let i = &self.0;
        if i.loading.get() {
            return;
        }
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
        if let orbital_vault::session::SaveResult::Written { hash } = &result
            && let (Some(f), Some(path)) = (i.on_saved.borrow().as_ref(), self.current_path())
        {
            f(&path, *hash);
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
