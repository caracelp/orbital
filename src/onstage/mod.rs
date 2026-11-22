//! OnStage (014): la nota `.md` abierta presentada como diapositivas en una ventana separada.
//!
//! Una sola ventana con un `Reader` (el renderizador de Lectura): cada diapositiva es el texto
//! entre encabezados ATX de nivel 1 o 2, volcado con `Prepared::with_vault` fuera del hilo
//! principal. La portada (título y descripción del frontmatter) y el índice de encabezados son
//! diapositivas virtuales con toggles; «Calm» va a pantalla completa sin controles y muestra el
//! contador al mover el mouse. El texto escala por pasos para entrar en la ventana (ajuste medido
//! con height-for-width del TextView; la decisión es determinista, así que no oscila).
//!
//! La presentación es de solo lectura: la edición es en la nota. El editor avisa por
//! `set_on_text_changed` y OnStage refresca con una pausa de 250 ms. Nada escribe en la bóveda.

use std::cell::Cell;
use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use gtk::prelude::*;
use gtk::{gio, glib};
use orbital_vault::RelPath;
use orbital_vault::props::PropEntry;

use crate::editor::reader::{Prepared, Reader};

/// Pausa del refresco por edición (evita rearma por tecla).
const PAUSA_EDICION: Duration = Duration::from_millis(250);
/// Pasos del ajuste de texto (px), de mayor a menor. El ajuste elige el más grande que entra.
const ESCALAS_PX: &[i32] = &[30, 26, 22, 18, 15];
/// Tolerancia del ajuste (px) para no rebanar el pie del contenido.
const TOLERANCIA_PX: i32 = 8;
/// Sangría de los encabezados de nivel 2 en el índice (px).
const INDENT_NIVEL2: i32 = 24;
/// Cuánto se muestra el contador al mover el mouse en modo calm.
const CONTADOR_MUESTRA: Duration = Duration::from_millis(1500);

mod ajuste;
#[cfg(test)]
mod bench_sc1;
mod diapositivas;
mod navegacion;
#[cfg(test)]
mod tests;
mod volcado;

use diapositivas::portada_de;
pub use diapositivas::{Diapositiva, dividir_diapositivas};

/// El material vivo de la nota que OnStage presenta. Lo arma la ventana principal (texto del
/// editor + bóveda actual) en cada refresco.
pub struct Fuente {
    pub texto: String,
    pub ruta: RelPath,
    pub vault: Option<Arc<orbital_vault::Vault>>,
    /// El índice de búsqueda de la bóveda: los bloques generados (`toc`, `indice`, `consulta`)
    /// se completan como en la Lectura, sin interacción (036).
    pub indice: Option<Arc<std::sync::RwLock<orbital_vault::SearchIndex>>>,
}

/// Una diapositiva preparada fuera del hilo de GTK: transclusiones y bloques generados.
pub(crate) fn preparar(
    texto: &str,
    vault: Option<&Arc<orbital_vault::Vault>>,
    ruta: &RelPath,
    yaml: &[orbital_vault::props::PropEntry],
    indice: Option<&Arc<std::sync::RwLock<orbital_vault::SearchIndex>>>,
) -> Prepared {
    let mut p = match vault {
        Some(v) => Prepared::with_vault(texto, v, ruta, yaml),
        None => Prepared::new(texto, yaml),
    };
    let idx = indice.and_then(|i| i.read().ok());
    crate::editor::reader::complete_generated_con(
        &mut p.doc,
        &mut p.hashes,
        idx.as_deref(),
        vault.map(|v| v.root()),
        Some(ruta),
        false,
    );
    p
}

type ProveedorFn = Box<dyn Fn() -> Option<Fuente>>;
type EnlaceFn = Box<dyn Fn(&str)>;

struct Inner {
    window: gtk::Window,
    stack: gtk::Stack,
    reader: Reader,
    portada_titulo: gtk::Label,
    portada_desc: gtk::Label,
    toc_lista: gtk::Box,
    contador_barra: gtk::Label,
    contador_flotante: gtk::Label,
    titulo_barra: gtk::Label,
    atras: gtk::Button,
    adelante: gtk::Button,
    toggle_portada: gtk::ToggleButton,
    toggle_toc: gtk::ToggleButton,
    toggle_calm: gtk::ToggleButton,
    proveedor: RefCell<Option<ProveedorFn>>,
    abrir: RefCell<Option<EnlaceFn>>,
    fuente: RefCell<Option<Fuente>>,
    yaml: RefCell<Vec<PropEntry>>,
    diapositivas: RefCell<Vec<Diapositiva>>,
    portada_datos: RefCell<Option<(String, Option<String>)>>,
    indice: Cell<usize>,
    px: Cell<i32>,
    ajuste_programado: Cell<bool>,
    ajuste_propio: Cell<bool>,
    generation: Cell<u64>,
    refresco_pendiente: RefCell<Option<glib::SourceId>>,
    contador_pendiente: RefCell<Option<glib::SourceId>>,
    proveedor_css: gtk::CssProvider,
}

#[derive(Clone)]
pub struct OnStage(Rc<Inner>);

impl OnStage {
    pub fn new<P: glib::object::IsA<gtk::Window>>(principal: &P) -> Self {
        let reader = Reader::new();
        reader.widget().add_css_class("onstage-escala");
        // Los gráficos (052), estáticos: la diapositiva no se maneja con el puntero.
        reader.fijar_graficos_estaticos(true);
        let window = gtk::Window::builder()
            .title("OnStage")
            .transient_for(principal)
            .modal(false)
            .default_width(1024)
            .default_height(768)
            .hide_on_close(true)
            .build();
        let titulo_barra = gtk::Label::builder()
            .css_classes(["title"])
            .single_line_mode(true)
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .build();
        let marca = gtk::Label::builder()
            .label("OnStage")
            .css_classes(["dim-label", "caption"])
            .build();
        let grupo = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(1)
            .build();
        grupo.append(&titulo_barra);
        grupo.append(&marca);
        let header = gtk::HeaderBar::builder().show_title_buttons(true).build();
        header.set_title_widget(Some(&grupo));
        let atras = gtk::Button::builder()
            .icon_name("go-previous-symbolic")
            .tooltip_text("Diapositiva anterior")
            .build();
        atras.update_property(&[gtk::accessible::Property::Label("Diapositiva anterior")]);
        let adelante = gtk::Button::builder()
            .icon_name("go-next-symbolic")
            .tooltip_text("Diapositiva siguiente")
            .build();
        adelante.update_property(&[gtk::accessible::Property::Label("Diapositiva siguiente")]);
        let contador_barra = gtk::Label::builder()
            .label("0 de 0")
            .css_classes(["dim-label"])
            .tooltip_text("Diapositiva actual")
            .build();
        header.pack_start(&atras);
        header.pack_start(&adelante);
        header.pack_start(&contador_barra);
        let toggle_portada = gtk::ToggleButton::builder()
            .label("Portada")
            .active(true)
            .build();
        toggle_portada.update_property(&[gtk::accessible::Property::Label(
            "Mostrar u ocultar la portada",
        )]);
        let toggle_toc = gtk::ToggleButton::builder().label("Índice").build();
        toggle_toc.update_property(&[gtk::accessible::Property::Label(
            "Mostrar u ocultar el índice de encabezados",
        )]);
        let toggle_calm = gtk::ToggleButton::builder()
            .label("Calm")
            .tooltip_text("Pantalla completa sin controles (F11)")
            .build();
        toggle_calm.update_property(&[gtk::accessible::Property::Label(
            "Modo calm a pantalla completa",
        )]);
        header.pack_end(&toggle_calm);
        header.pack_end(&toggle_toc);
        header.pack_end(&toggle_portada);
        window.set_titlebar(Some(&header));
        let portada_titulo = gtk::Label::builder()
            .css_classes(["title-1"])
            .wrap(true)
            .justify(gtk::Justification::Center)
            .build();
        portada_titulo.update_property(&[gtk::accessible::Property::Label("Portada")]);
        let portada_desc = gtk::Label::builder()
            .css_classes(["dim-label", "title-3"])
            .wrap(true)
            .justify(gtk::Justification::Center)
            .build();
        let portada = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .valign(gtk::Align::Center)
            .spacing(18)
            .margin_start(48)
            .margin_end(48)
            .build();
        portada.append(&portada_titulo);
        portada.append(&portada_desc);
        let toc_lista = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(4)
            .valign(gtk::Align::Start)
            .margin_top(24)
            .margin_bottom(24)
            .margin_start(32)
            .margin_end(32)
            .build();
        toc_lista.update_property(&[gtk::accessible::Property::Label("Índice de encabezados")]);
        let toc = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::External)
            .child(&toc_lista)
            .build();
        let stack = gtk::Stack::builder()
            .vexpand(true)
            .hexpand(true)
            .transition_type(gtk::StackTransitionType::None)
            .build();
        stack.add_named(reader.widget(), Some("diapositiva"));
        stack.add_named(&portada, Some("portada"));
        stack.add_named(&toc, Some("indice"));
        let contador_flotante = gtk::Label::builder()
            .label("0 de 0")
            .css_classes(["onstage-contador"])
            .halign(gtk::Align::Center)
            .valign(gtk::Align::End)
            .margin_bottom(24)
            .visible(false)
            .build();
        let overlay = gtk::Overlay::builder().child(&stack).build();
        overlay.add_overlay(&contador_flotante);
        window.set_child(Some(&overlay));
        let proveedor_css = gtk::CssProvider::new();
        let contador_css = gtk::CssProvider::new();
        contador_css.load_from_string(
            ".onstage-contador { background: alpha(black, 0.75); color: white; \
             border-radius: 12px; padding: 6px 14px; }",
        );
        if let Some(display) = gtk::gdk::Display::default() {
            gtk::style_context_add_provider_for_display(
                &display,
                &proveedor_css,
                gtk::STYLE_PROVIDER_PRIORITY_APPLICATION + 1,
            );
            gtk::style_context_add_provider_for_display(
                &display,
                &contador_css,
                gtk::STYLE_PROVIDER_PRIORITY_APPLICATION + 1,
            );
        }
        let this = Self(Rc::new(Inner {
            window,
            stack,
            reader,
            portada_titulo,
            portada_desc,
            toc_lista,
            contador_barra,
            contador_flotante,
            titulo_barra,
            atras,
            adelante,
            toggle_portada,
            toggle_toc,
            toggle_calm,
            proveedor: RefCell::new(None),
            abrir: RefCell::new(None),
            fuente: RefCell::new(None),
            yaml: RefCell::new(Vec::new()),
            diapositivas: RefCell::new(Vec::new()),
            portada_datos: RefCell::new(None),
            indice: Cell::new(0),
            px: Cell::new(ESCALAS_PX[0]),
            ajuste_programado: Cell::new(false),
            ajuste_propio: Cell::new(false),
            generation: Cell::new(0),
            refresco_pendiente: RefCell::new(None),
            contador_pendiente: RefCell::new(None),
            proveedor_css,
        }));
        this.conectar_navegacion();
        this.conectar_toggles();
        this.conectar_contador_flotante();
        this.conectar_ajuste();
        this
    }

    /// El material de la nota (texto del editor y bóveda actual), lo arma la ventana principal.
    pub fn set_proveedor(&self, f: impl Fn() -> Option<Fuente> + 'static) {
        *self.0.proveedor.borrow_mut() = Some(Box::new(f));
    }

    /// Un enlace de la diapositiva se abre en la ventana principal.
    pub fn set_on_link(&self, f: impl Fn(&str) + 'static) {
        *self.0.abrir.borrow_mut() = Some(Box::new(f));
    }

    /// Abre la presentación: primera diapositiva, tamaño base y material fresco.
    pub fn presentar(&self) {
        self.0.indice.set(0);
        self.set_px(ESCALAS_PX[0]);
        // Sin persistencia: cada presentación abre con los valores por defecto (FR-6).
        self.0.toggle_portada.set_active(true);
        self.0.toggle_toc.set_active(false);
        self.0.toggle_calm.set_active(false);
        self.0.window.present();
        self.refrescar();
    }

    /// Cambios del editor: refresco con pausa (edición en vivo, US2). Oculta, no refresca nada:
    /// al volver a abrir, `presentar` trae el material fresco.
    pub fn editar(&self) {
        if !self.0.window.is_visible() || self.0.refresco_pendiente.borrow().is_some() {
            return;
        }
        let this = self.clone();
        let id = glib::timeout_add_local_once(PAUSA_EDICION, move || {
            *this.0.refresco_pendiente.borrow_mut() = None;
            this.refrescar();
        });
        *self.0.refresco_pendiente.borrow_mut() = Some(id);
    }
}
