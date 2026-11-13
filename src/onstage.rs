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

/// Una diapositiva: el encabezado que la abre (nivel 1 o 2) y su texto fuente, con el encabezado
/// incluido (la diapositiva muestra su título).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Diapositiva {
    /// Texto del encabezado de corte, sin `#`.
    pub titulo: String,
    /// Nivel del encabezado de corte (1 o 2); 0 si la diapositiva abre sin encabezado.
    pub nivel: u8,
    /// Texto fuente completo de la diapositiva.
    pub texto: String,
}

/// El material vivo de la nota que OnStage presenta. Lo arma la ventana principal (texto del
/// editor + bóveda actual) en cada refresco.
pub struct Fuente {
    pub texto: String,
    pub ruta: RelPath,
    pub vault: Option<Arc<orbital_vault::Vault>>,
}

type ProveedorFn = Box<dyn Fn() -> Option<Fuente>>;
type EnlaceFn = Box<dyn Fn(&str)>;

/// Corta el texto en diapositivas por encabezados ATX de nivel 1 o 2. El texto previo al primer
/// encabezado (sin contar el frontmatter) va como primera diapositiva sin título si tiene
/// contenido. Un `#` dentro de un codeblock cerrado no corta; los encabezados con sangría de 4 o
/// más espacios son código, no encabezados.
pub fn dividir_diapositivas(texto: &str) -> Vec<Diapositiva> {
    let mut salidas: Vec<Diapositiva> = Vec::new();
    let mut desde = fin_frontmatter(texto);
    let mut titulo_actual: Option<(String, u8)> = None;
    let mut en_codeblock: Option<char> = None;
    let mut offset = desde;
    for linea in texto[desde..].lines() {
        let largo_linea = linea.len() + 1; // + '\n' (la última línea puede no llevarlo)
        let recortada = linea.trim_start();
        match en_codeblock {
            Some(marca) => {
                if cierra_codeblock(recortada, marca) {
                    en_codeblock = None;
                }
            }
            None => {
                if let Some(nivel) = nivel_encabezado(linea) {
                    if nivel <= 2 {
                        let slice = &texto[desde..offset.min(texto.len())];
                        if !slice.trim().is_empty() || titulo_actual.is_some() {
                            let (titulo, niv) =
                                titulo_actual.take().unwrap_or_else(|| (String::new(), 0));
                            salidas.push(Diapositiva {
                                titulo,
                                nivel: niv,
                                texto: slice.to_owned(),
                            });
                        }
                        desde = offset;
                        titulo_actual = Some((titulo_atx(recortada), nivel));
                    }
                } else {
                    en_codeblock = abre_codeblock(recortada);
                }
            }
        }
        offset = (offset + largo_linea).min(texto.len());
    }
    let slice = &texto[desde..];
    if titulo_actual.is_some() || !slice.trim().is_empty() {
        let (titulo, niv) = titulo_actual.take().unwrap_or_else(|| (String::new(), 0));
        salidas.push(Diapositiva {
            titulo,
            nivel: niv,
            texto: slice.to_owned(),
        });
    }
    salidas
}

/// Byte donde termina el frontmatter (con su salto de línea), si el texto abre con `---`.
fn fin_frontmatter(texto: &str) -> usize {
    let mut resto = texto;
    if let Some(sin_bom) = resto.strip_prefix('\u{feff}') {
        resto = sin_bom;
    }
    if !resto.starts_with("---\n") {
        return 0;
    }
    let base = texto.len() - resto.len();
    let mut offset = base + 4;
    for linea in texto[offset..].lines() {
        let largo_linea = linea.len() + 1;
        if linea.trim() == "---" {
            return (offset + largo_linea).min(texto.len());
        }
        offset = (offset + largo_linea).min(texto.len());
    }
    0
}

/// Sangría en espacios, con cada tabulador contando como 4 (regla de Markdown).
fn sangria(linea: &str) -> usize {
    linea
        .chars()
        .take_while(|c| c.is_whitespace())
        .map(|c| if c == '\t' { 4 } else { 1 })
        .sum()
}

/// Nivel de un encabezado ATX (`#` a `######` seguido de espacio o fin de línea), si la sangría
/// es menor a 4 espacios. `#etiqueta` no es encabezado.
fn nivel_encabezado(linea: &str) -> Option<u8> {
    if sangria(linea) >= 4 {
        return None;
    }
    let recortada = linea.trim_start();
    let nivel = recortada.chars().take_while(|c| *c == '#').count();
    if nivel == 0 || nivel > 6 {
        return None;
    }
    let resto = &recortada[nivel..];
    (resto.is_empty() || resto.starts_with(' ') || resto.starts_with('\t')).then_some(nivel as u8)
}

/// Texto del encabezado ATX sin `#` inicial ni cierre `###` final.
fn titulo_atx(linea: &str) -> String {
    let sin_hashes = linea.trim_start_matches('#').trim_start();
    let sin_cierre = sin_hashes.trim_end_matches('#').trim_end();
    sin_cierre.to_owned()
}

/// Marcador de codeblock si la línea abre uno (``` o ~~~).
fn abre_codeblock(linea: &str) -> Option<char> {
    let marca = linea.chars().next()?;
    (marca == '`' || marca == '~')
        .then(|| linea.chars().take_while(|c| *c == marca).count())
        .filter(|n| *n >= 3)
        .map(|_| marca)
}

/// Si la línea cierra un codeblock abierto con la marca dada (mismo carácter, corrida de 3+).
fn cierra_codeblock(linea: &str, marca: char) -> bool {
    let corrida = linea.chars().take_while(|c| *c == marca).count();
    corrida >= 3 && linea[corrida..].trim().is_empty()
}

/// Propiedades del frontmatter (solo `title` y `description` escalares para la portada), fuera
/// del hilo principal. Sin título no hay portada.
fn portada_de(yaml: &[PropEntry]) -> Option<(String, Option<String>)> {
    let titulo = yaml
        .iter()
        .find(|e| e.key == "title")
        .and_then(|e| e.value.as_text().map(str::to_owned))?;
    let descripcion = yaml
        .iter()
        .find(|e| e.key == "description")
        .and_then(|e| e.value.as_text().map(str::to_owned));
    Some((titulo, descripcion))
}

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

    fn conectar_navegacion(&self) {
        let this = self.clone();
        self.0.atras.connect_clicked(move |_| this.retroceder());
        let this = self.clone();
        self.0.adelante.connect_clicked(move |_| this.avanzar());
        let this = self.clone();
        let teclas = gtk::EventControllerKey::new();
        teclas.set_propagation_phase(gtk::PropagationPhase::Capture);
        teclas.connect_key_pressed(move |_, keyval, _, _| {
            use gtk::gdk::Key;
            if keyval == Key::Escape {
                this.0.window.set_visible(false);
                return glib::Propagation::Stop;
            }
            if keyval == Key::F11 {
                this.alternar_calm();
                return glib::Propagation::Stop;
            }
            // Con el índice visible, las teclas van a sus botones (teclado nativo).
            if this
                .0
                .stack
                .visible_child_name()
                .is_some_and(|n| n == "indice")
            {
                return glib::Propagation::Proceed;
            }
            match keyval {
                Key::Right
                | Key::Down
                | Key::Page_Down
                | Key::Return
                | Key::KP_Enter
                | Key::space => {
                    this.avanzar();
                    glib::Propagation::Stop
                }
                Key::Left | Key::Up | Key::Page_Up | Key::BackSpace => {
                    this.retroceder();
                    glib::Propagation::Stop
                }
                Key::Home => {
                    this.ir_a(0);
                    glib::Propagation::Stop
                }
                Key::End => {
                    this.ir_a(usize::MAX);
                    glib::Propagation::Stop
                }
                _ => glib::Propagation::Proceed,
            }
        });
        self.0.window.add_controller(teclas);
    }

    fn conectar_toggles(&self) {
        let this = self.clone();
        self.0.toggle_portada.connect_toggled(move |_| {
            this.render_actual();
        });
        let this = self.clone();
        self.0.toggle_toc.connect_toggled(move |_| {
            this.rearmar_toc();
            this.render_actual();
        });
        let this = self.clone();
        self.0.toggle_calm.connect_toggled(move |t| {
            if t.is_active() {
                this.0.window.fullscreen();
            } else {
                this.0.window.unfullscreen();
            }
            this.0.contador_flotante.set_visible(false);
        });
        // El gestor de ventanas puede salir de pantalla completa por su cuenta: sincroniza.
        let this = self.clone();
        self.0
            .window
            .connect_notify_local(Some("fullscreened"), move |_, _| {
                if !this.0.window.is_fullscreen() {
                    this.0.toggle_calm.set_active(false);
                    this.0.contador_flotante.set_visible(false);
                }
            });
        let this = self.clone();
        self.0.window.connect_close_request(move |_| {
            if let Some(id) = this.0.refresco_pendiente.borrow_mut().take() {
                id.remove();
            }
            glib::Propagation::Proceed
        });
    }

    /// El contador flotante solo vive en modo calm y reaparece al mover el mouse.
    fn conectar_contador_flotante(&self) {
        let this = self.clone();
        let motion = gtk::EventControllerMotion::new();
        motion.connect_motion(move |_, _, _| {
            if !this.0.toggle_calm.is_active() {
                return;
            }
            this.0.contador_flotante.set_visible(true);
            if let Some(previo) = this.0.contador_pendiente.borrow_mut().take() {
                previo.remove();
            }
            let este = this.clone();
            let id = glib::timeout_add_local_once(CONTADOR_MUESTRA, move || {
                *este.0.contador_pendiente.borrow_mut() = None;
                este.0.contador_flotante.set_visible(false);
            });
            *this.0.contador_pendiente.borrow_mut() = Some(id);
        });
        self.0.stack.add_controller(motion);
    }

    fn alternar_calm(&self) {
        self.0
            .toggle_calm
            .set_active(!self.0.toggle_calm.is_active());
    }

    /// Portada activada y con datos (título del frontmatter).
    fn portada_visible(&self) -> bool {
        self.0.toggle_portada.is_active() && self.0.portada_datos.borrow().is_some()
    }

    /// Índice activado y con diapositivas.
    fn toc_visible(&self) -> bool {
        self.0.toggle_toc.is_active() && !self.0.diapositivas.borrow().is_empty()
    }

    fn prefijo(&self) -> usize {
        usize::from(self.portada_visible()) + usize::from(self.toc_visible())
    }

    fn total(&self) -> usize {
        self.prefijo() + self.0.diapositivas.borrow().len()
    }

    fn avanzar(&self) {
        self.ir_a(self.0.indice.get() + 1);
    }

    fn retroceder(&self) {
        self.ir_a(self.0.indice.get().saturating_sub(1));
    }

    fn ir_a(&self, destino: usize) {
        let total = self.total();
        let indice = destino.min(total.saturating_sub(1));
        if self.0.indice.get() == indice {
            return;
        }
        self.0.indice.set(indice);
        self.render_actual();
    }

    /// Contador y sensibilidad de los botones de navegación según el índice vigente.
    fn actualizar_estado(&self) {
        let total = self.total();
        let indice = self.0.indice.get().min(total.saturating_sub(1));
        self.0.indice.set(indice);
        let texto = format!("Diapositiva {} de {}", indice + 1, total.max(1));
        let corto = format!("{} de {}", indice + 1, total);
        self.0.contador_barra.set_label(&corto);
        self.0.contador_barra.set_tooltip_text(Some(&texto));
        self.0.contador_flotante.set_label(&corto);
        self.0.atras.set_sensitive(indice > 0);
        self.0.adelante.set_sensitive(indice + 1 < total);
    }

    /// Muestra la diapositiva virtual o real que toca según el índice actual.
    fn render_actual(&self) {
        let portada_v = self.portada_visible();
        let toc_v = self.toc_visible();
        let total =
            usize::from(portada_v) + usize::from(toc_v) + self.0.diapositivas.borrow().len();
        if total == 0 {
            // Nota sin contenido: una diapositiva vacía (nunca falla).
            self.actualizar_estado();
            self.0.stack.set_visible_child_name("diapositiva");
            self.volcar(Prepared::new("", &[]));
            return;
        }
        let indice = self.0.indice.get().min(total - 1);
        self.0.indice.set(indice);
        self.actualizar_estado();
        if portada_v && indice == 0 {
            self.0.stack.set_visible_child_name("portada");
            return;
        }
        if toc_v && indice == usize::from(portada_v) {
            self.0.stack.set_visible_child_name("indice");
            return;
        }
        let prefijo = usize::from(portada_v) + usize::from(toc_v);
        let real = indice - prefijo;
        self.cargar_diapositiva(real);
    }

    /// Vuelve a armar la lista del índice con las diapositivas actuales.
    fn rearmar_toc(&self) {
        let i = &self.0;
        while let Some(hijo) = i.toc_lista.first_child() {
            i.toc_lista.remove(&hijo);
        }
        let diapositivas = i.diapositivas.borrow();
        for (n, d) in diapositivas.iter().enumerate() {
            let rotulo = if d.titulo.is_empty() {
                format!("{}. (sin título)", n + 1)
            } else {
                format!("{}. {}", n + 1, d.titulo)
            };
            let texto = gtk::Label::builder()
                .label(&rotulo)
                .ellipsize(gtk::pango::EllipsizeMode::End)
                .halign(gtk::Align::Start)
                .build();
            let boton = gtk::Button::builder()
                .css_classes(["flat"])
                .child(&texto)
                .build();
            if d.nivel > 1 {
                texto.set_margin_start(INDENT_NIVEL2);
            }
            boton.update_property(&[gtk::accessible::Property::Description(
                format!("Ir a la diapositiva {}: {}", n + 1, d.titulo).as_str(),
            )]);
            let this = self.clone();
            let destino = n;
            boton.connect_clicked(move |_| {
                this.ir_a(this.prefijo() + destino);
            });
            i.toc_lista.append(&boton);
        }
        if diapositivas.is_empty() {
            let vacio = gtk::Label::builder()
                .label("La nota no tiene encabezados de nivel 1 ni 2")
                .css_classes(["dim-label"])
                .halign(gtk::Align::Start)
                .build();
            i.toc_lista.append(&vacio);
        }
    }

    /// Material nuevo del editor: divide, resuelve el frontmatter y prepara la diapositiva
    /// vigente, todo fuera del hilo principal.
    fn refrescar(&self) {
        let i = &self.0;
        let Some(fuente) = i.proveedor.borrow().as_ref().and_then(|f| f()) else {
            return;
        };
        let generation = i.generation.get() + 1;
        i.generation.set(generation);
        let indice_viejo = i.indice.get();
        let portada_on = i.toggle_portada.is_active();
        let toc_on = i.toggle_toc.is_active();
        let nombre = fuente
            .ruta
            .as_str()
            .rsplit('/')
            .next()
            .unwrap_or("")
            .to_owned();
        let job = {
            let texto = fuente.texto.clone();
            let ruta = fuente.ruta.clone();
            let vault = fuente.vault.clone();
            async move {
                gio::spawn_blocking(move || {
                    let yaml = orbital_vault::props::markdown::locate(&texto)
                        .map(|r| orbital_vault::props::yaml::parse_block(&texto[r]))
                        .map(|parsed| parsed.unwrap_or_default())
                        .unwrap_or_default();
                    let diapositivas = dividir_diapositivas(&texto);
                    let portada = portada_de(&yaml);
                    let prefijo = usize::from(portada_on && portada.is_some())
                        + usize::from(toc_on && !diapositivas.is_empty());
                    let total = prefijo + diapositivas.len();
                    let indice = if total == 0 {
                        0
                    } else {
                        indice_viejo.min(total - 1)
                    };
                    let real = indice
                        .checked_sub(prefijo)
                        .filter(|r| *r < diapositivas.len());
                    let prepared = real.map(|r| {
                        let slice = diapositivas[r].texto.as_str();
                        match &vault {
                            Some(v) => Prepared::with_vault(slice, v, &ruta, &yaml),
                            None => Prepared::new(slice, &yaml),
                        }
                    });
                    (diapositivas, yaml, portada, indice, prepared)
                })
                .await
            }
        };
        let this = self.clone();
        glib::spawn_future_local(async move {
            let Ok((diapositivas, yaml, portada, indice, prepared)) = job.await else {
                return;
            };
            if this.0.generation.get() != generation {
                return;
            }
            let i = &this.0;
            *i.fuente.borrow_mut() = Some(fuente);
            *i.yaml.borrow_mut() = yaml;
            *i.diapositivas.borrow_mut() = diapositivas;
            *i.portada_datos.borrow_mut() = portada.clone();
            i.titulo_barra.set_label(&nombre);
            if let Some((titulo, descripcion)) = &portada {
                i.portada_titulo.set_label(titulo);
                i.portada_desc
                    .set_label(descripcion.as_deref().unwrap_or(""));
            }
            i.indice.set(indice);
            this.rearmar_toc();
            // is_missing con la nota vigente: los enlaces a notas ausentes quedan atenuados.
            if let Some(f) = i.fuente.borrow().as_ref()
                && let Some(v) = f.vault.clone()
            {
                let ruta = f.ruta.clone();
                let vault = Arc::downgrade(&v);
                i.reader.set_is_missing(move |url| {
                    if url.starts_with("orbital-tag:")
                        || url.starts_with("http://")
                        || url.starts_with("https://")
                        || url.starts_with("mailto:")
                    {
                        return false;
                    }
                    let Some(v) = vault.upgrade() else {
                        return false;
                    };
                    v.resolve_link(&ruta, url) == orbital_vault::Resolution::Missing
                });
            }
            match prepared {
                Some(prepared) => {
                    this.actualizar_estado();
                    this.volcar(prepared);
                }
                None => this.render_actual(),
            }
        });
    }

    /// Vuelca una diapositiva preparada en el Reader y muestra la vista.
    fn volcar(&self, prepared: Prepared) {
        let i = &self.0;
        let base_dir = i.fuente.borrow().as_ref().and_then(|f| {
            f.vault
                .as_ref()
                .and_then(|v| f.ruta.to_path(v.root()).parent().map(PathBuf::from))
        });
        i.stack.set_visible_child_name("diapositiva");
        i.reader.render(prepared, || None, base_dir);
    }

    /// Prepara y vuelca la diapositiva real que toca (navegación y toggles).
    fn cargar_diapositiva(&self, real: usize) {
        let i = &self.0;
        let prestamo = i.fuente.borrow();
        let Some(fuente) = prestamo.as_ref() else {
            return;
        };
        let Some(slide) = i.diapositivas.borrow().get(real).cloned() else {
            return;
        };
        let vault = fuente.vault.clone();
        let ruta = fuente.ruta.clone();
        let yaml = i.yaml.borrow().clone();
        let generation = i.generation.get() + 1;
        i.generation.set(generation);
        let texto = slide.texto;
        let this = self.clone();
        glib::spawn_future_local(async move {
            let Ok(prepared) = gio::spawn_blocking(move || match &vault {
                Some(v) => Prepared::with_vault(&texto, v, &ruta, &yaml),
                None => Prepared::new(&texto, &yaml),
            })
            .await
            else {
                return;
            };
            if this.0.generation.get() != generation {
                return;
            }
            this.actualizar_estado();
            this.volcar(prepared);
        });
    }

    /// Ajuste de texto: el paso de fuente más grande que entra en la ventana. La altura se mide
    /// con height-for-width al paso actual y se estima el del resto por linealidad; con el mismo
    /// contenido y la misma ventana la decisión es siempre la misma, así que no oscila.
    fn ajustar(&self) {
        let i = &self.0;
        if i.stack
            .visible_child_name()
            .is_none_or(|n| n != "diapositiva")
        {
            return;
        }
        let view = i.reader.text_view();
        let scroller = i.reader.widget();
        let ancho = view.width();
        let disponible = scroller.height();
        if ancho < 120 || disponible < 120 {
            return;
        }
        let (_, natural, _, _) = view.measure(gtk::Orientation::Vertical, ancho);
        if natural <= 0 {
            return;
        }
        let px = i.px.get();
        // Altura estimada al paso base (30): los tamaños escalan casi linealmente.
        let en_base = natural as f64 * (ESCALAS_PX[0] as f64) / (px as f64);
        if en_base <= (disponible + TOLERANCIA_PX) as f64 {
            self.set_px(ESCALAS_PX[0]);
            return;
        }
        let escala = (ESCALAS_PX[0] as f64) * (disponible as f64) / en_base;
        let destino = ESCALAS_PX
            .iter()
            .copied()
            .find(|p| (*p as f64) <= escala)
            .unwrap_or(*ESCALAS_PX.last().unwrap());
        self.set_px(destino);
    }

    /// Programa el ajuste en un idle: nunca dentro de la disposición.
    fn ajustar_en_reposo(&self) {
        if self.0.ajuste_programado.get() {
            return;
        }
        self.0.ajuste_programado.set(true);
        let this = self.clone();
        glib::idle_add_local_once(move || {
            this.0.ajuste_programado.set(false);
            this.ajustar();
        });
    }

    /// Escala la fuente de la diapositiva por el proveedor CSS propio (solo si cambia).
    fn set_px(&self, px: i32) {
        let i = &self.0;
        if i.px.get() == px {
            return;
        }
        i.px.set(px);
        // El cambio de fuente dispara el señal del adjustment: se consume una vez para no
        // reajustar en cascada (el próximo cambio genuino de contenido o ventana reevalúa).
        self.0.ajuste_propio.set(true);
        i.proveedor_css.load_from_string(&format!(
            ".onstage-escala textview {{ font-size: {px}px; }}"
        ));
    }

    fn conectar_ajuste(&self) {
        // El volcado terminó: la diapositiva nueva puede necesitar otro paso.
        let this = self.clone();
        self.0.reader.set_on_rendered(move || {
            this.ajustar_en_reposo();
        });
        // La ventana o el contenido cambiaron de alto: reevalúa. El propio cambio de fuente
        // dispara este señal una vez; se consume para no reajustar en cascada.
        let this = self.clone();
        let vadj = self.0.reader.widget().vadjustment();
        vadj.connect_changed(move |_| {
            if this.0.ajuste_propio.get() {
                this.0.ajuste_propio.set(false);
                return;
            }
            this.ajustar_en_reposo();
        });
        // Un enlace de la diapositiva se abre en la ventana principal.
        let this = self.clone();
        self.0.reader.set_on_link(move |url| {
            if let Some(f) = this.0.abrir.borrow().as_ref() {
                f(url);
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// T002: corte por nivel 1 y 2; niveles 3+ quedan dentro.
    #[test]
    fn t002_corte_por_nivel_1_y_2() {
        let texto = "# A\n\nuno\n\n## B\n\ndos\n\n### C\n\nsigue en B\n\n## D\n\nfin\n";
        let d = dividir_diapositivas(texto);
        assert_eq!(d.len(), 3, "{d:?}");
        assert_eq!(d[0].titulo, "A");
        assert_eq!(d[0].nivel, 1);
        assert!(d[0].texto.starts_with("# A"));
        assert_eq!(d[1].titulo, "B");
        assert!(d[1].texto.contains("### C"), "{:?}", d[1].texto);
        assert!(d[1].texto.contains("sigue en B"));
        assert_eq!(d[2].titulo, "D");
    }

    /// T002: el frontmatter no genera diapositiva ni corta.
    #[test]
    fn t002_frontmatter_se_ignora() {
        let texto = "---\ntitle: X\n---\n\n# T\n\ncuerpo\n";
        let d = dividir_diapositivas(texto);
        assert_eq!(d.len(), 1, "{d:?}");
        assert_eq!(d[0].titulo, "T");
    }

    /// T002: texto previo al primer encabezado va como diapositiva sin título.
    #[test]
    fn t002_previo_al_primer_encabezado() {
        let texto = "intro\n\n# T\n\ncuerpo\n";
        let d = dividir_diapositivas(texto);
        assert_eq!(d.len(), 2, "{d:?}");
        assert_eq!(d[0].titulo, "");
        assert_eq!(d[0].nivel, 0);
        assert_eq!(d[1].titulo, "T");
    }

    /// T002: nota sin encabezados es una sola diapositiva; nota vacía no tiene ninguna.
    #[test]
    fn t002_sin_encabezados_y_vacia() {
        let d = dividir_diapositivas("solo texto\n");
        assert_eq!(d.len(), 1);
        assert_eq!(d[0].titulo, "");
        assert!(dividir_diapositivas("").is_empty());
        assert!(dividir_diapositivas("\n\n").is_empty());
    }

    /// T002: un `#` dentro de un codeblock cerrado no corta; el que abre el codeblock sí.
    #[test]
    fn t002_codeblock_no_corta() {
        let texto =
            "# A\n\n```rust\n# no es encabezado\n```\n\n# B\n\n~~~\n# tampoco\n~~~\n\nfin\n";
        let d = dividir_diapositivas(texto);
        assert_eq!(d.len(), 2, "{d:?}");
        assert!(d[0].texto.contains("# no es encabezado"));
        assert!(d[1].texto.contains("# tampoco"), "{:?}", d[1].texto);
        assert!(d[1].texto.contains("~~~"));
        assert_eq!(d[1].titulo, "B");
    }

    /// T002: `#etiqueta` y encabezados con sangría de 4+ no cortan; `##` con cierre `##` sí.
    #[test]
    fn t002_atx_estricto() {
        let texto =
            "# A\n\n#etiqueta no corta\n\n    ## con sangría de 4 no corta\n\n## B ##\n\nfin\n";
        let d = dividir_diapositivas(texto);
        assert_eq!(d.len(), 2, "{d:?}");
        assert!(d[0].texto.contains("#etiqueta"));
        assert!(d[0].texto.contains("    ## con sangría"));
        assert_eq!(d[1].titulo, "B");
    }

    /// T002: el título se recorta sin `#` inicial ni cierre final.
    #[test]
    fn t002_titulo_limpio() {
        let texto = "# El título ##\n\nx\n";
        let d = dividir_diapositivas(texto);
        assert_eq!(d[0].titulo, "El título");
    }

    /// T002: frontmatter con BOM se ignora igual.
    #[test]
    fn t002_frontmatter_con_bom() {
        let texto = "\u{feff}---\ntitle: X\n---\n\n# T\n\ncuerpo\n";
        let d = dividir_diapositivas(texto);
        assert_eq!(d.len(), 1, "{d:?}");
        assert_eq!(d[0].titulo, "T");
    }

    /// FR-3 de la portada: solo `title` y `description` escalares del frontmatter.
    #[test]
    fn t003_portada_de_yaml() {
        let texto =
            "---\ntitle: Entropía\ndescription: Medida del desorden.\ntags: [fisica]\n---\n";
        let parsed = orbital_vault::props::markdown::locate(texto)
            .map(|r| orbital_vault::props::yaml::parse_block(&texto[r]))
            .unwrap();
        let yaml = parsed.unwrap();
        let portada = portada_de(&yaml).unwrap();
        assert_eq!(portada.0, "Entropía");
        assert_eq!(portada.1.as_deref(), Some("Medida del desorden."));
        // Sin título: sin portada.
        assert!(portada_de(&[]).is_none());
    }

    /// US1/US3 en proceso: ventana, portada, navegación por botones, índice y recorte al
    /// alternar los toggles. Requiere sesión gráfica; lo corre el verificador con
    /// `--include-ignored`. Las flechas y Esc van por revisión manual (sin teclas sintéticas).
    #[test]
    #[ignore = "requiere una sesión gráfica"]
    fn us1_ventana_portada_navegacion_e_indice() {
        gtk::init().expect("se necesita acceso GTK");
        let raiz =
            std::env::temp_dir().join(format!("orbital-onstage-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&raiz);
        std::fs::create_dir_all(raiz.join("wiki")).unwrap();
        let nota = "---\ntype: atomo\ntitle: Órbitas\ndescription: Demo.\n---\n\n# Uno\n\nTexto uno.\n\n## Dos\n\nTexto dos.\n";
        std::fs::write(raiz.join("wiki/a.md"), nota).unwrap();
        let vault = Arc::new(orbital_vault::open(&raiz).unwrap());
        let ruta = orbital_vault::RelPath::new("wiki/a.md").unwrap();

        let principal = gtk::Window::new();
        principal.present();
        let onstage = OnStage::new(&principal);
        onstage.set_proveedor(move || {
            Some(Fuente {
                texto: nota.to_owned(),
                ruta: ruta.clone(),
                vault: Some(vault.clone()),
            })
        });
        onstage.presentar();
        let context = glib::MainContext::default();
        context.block_on(async {
            // El refresco corre en dos saltos async (trabajo de fondo + volcado).
            glib::timeout_future(Duration::from_millis(400)).await;
            glib::timeout_future(Duration::from_millis(400)).await;
            // Portada activada por defecto y con título: 1 de 3 (portada + dos diapositivas).
            assert_eq!(onstage.0.contador_barra.label().as_str(), "1 de 3");
            assert_eq!(
                onstage.0.stack.visible_child_name().as_deref(),
                Some("portada")
            );
            // Navegación por botones: el contenido de cada diapositiva queda volcado.
            onstage.0.adelante.emit_clicked();
            assert_eq!(onstage.0.contador_barra.label().as_str(), "2 de 3");
            glib::timeout_future(Duration::from_millis(300)).await;
            let buffer = onstage.0.reader.buffer();
            let texto = buffer
                .text(&buffer.start_iter(), &buffer.end_iter(), false)
                .to_string();
            assert!(texto.contains("Texto uno."), "{texto:?}");
            onstage.0.adelante.emit_clicked();
            assert_eq!(onstage.0.contador_barra.label().as_str(), "3 de 3");
            assert!(!onstage.0.adelante.is_sensitive());
            // El índice se inserta en la lista sin perder la posición (3 de 4): llega
            // navegando hacia atrás.
            onstage.0.toggle_toc.set_active(true);
            assert_eq!(onstage.0.contador_barra.label().as_str(), "3 de 4");
            assert_eq!(
                onstage.0.stack.visible_child_name().as_deref(),
                Some("diapositiva")
            );
            onstage.0.atras.emit_clicked();
            assert_eq!(
                onstage.0.stack.visible_child_name().as_deref(),
                Some("indice")
            );
            assert_eq!(onstage.0.contador_barra.label().as_str(), "2 de 4");
            let botones = {
                let mut out = Vec::new();
                let mut hijo = onstage.0.toc_lista.first_child();
                while let Some(w) = hijo {
                    if let Ok(boton) = w.clone().downcast::<gtk::Button>() {
                        out.push(boton);
                    }
                    hijo = w.next_sibling();
                }
                out
            };
            assert_eq!(botones.len(), 2, "el índice lista las dos diapositivas");
            botones[1].emit_clicked();
            glib::timeout_future(Duration::from_millis(300)).await;
            assert_eq!(onstage.0.contador_barra.label().as_str(), "4 de 4");
            assert_eq!(
                onstage.0.stack.visible_child_name().as_deref(),
                Some("diapositiva")
            );
            // Sin índice, la posición se recorta a «Dos» (3 de 3).
            onstage.0.toggle_toc.set_active(false);
            assert_eq!(onstage.0.contador_barra.label().as_str(), "3 de 3");
            // Sin portada quedan las diapositivas; la posición sigue en «Dos» (2 de 2).
            onstage.0.toggle_portada.set_active(false);
            assert_eq!(onstage.0.contador_barra.label().as_str(), "2 de 2");
            // Reabrir vuelve a la primera diapositiva con los valores por defecto (FR-6).
            onstage.presentar();
            glib::timeout_future(Duration::from_millis(400)).await;
            glib::timeout_future(Duration::from_millis(400)).await;
            assert_eq!(onstage.0.contador_barra.label().as_str(), "1 de 3");
            assert_eq!(
                onstage.0.stack.visible_child_name().as_deref(),
                Some("portada")
            );
            assert!(!onstage.0.toggle_toc.is_active());
        });
        let _ = std::fs::remove_dir_all(&raiz);
    }
}

#[cfg(test)]
mod bench_sc1 {
    use super::*;

    /// SC-1: el análisis de una nota de ~100 KiB con 40 diapositivas es lineal y queda por
    /// debajo del cuadro de 16 ms (corre en el hilo de fondo). La parte de hilo principal por
    /// refresco es el volcado de la diapositiva, acotado por tramos de 8 ms (`SLICE` en
    /// editor/reader.rs); el peor caso por diapositiva se mide acá con `Prepared::new`.
    #[test]
    fn sc1_split_y_preparacion_en_budget() {
        let mut nota = String::from("---\ntype: atomo\ntitle: Grande\n---\n");
        for n in 0..40 {
            nota.push_str(&format!("\n## Diapositiva {}\n\n", n));
            for p in 0..25 {
                nota.push_str(&format!(
                    "Párrafo {p} con [[enlace {n}]] y #etiqueta, texto de relleno para llegar \
                     al tamaño de una nota real con muchas diapositivas.\n\n"
                ));
            }
        }
        assert!(nota.len() > 100_000, "{} bytes", nota.len());
        let t0 = std::time::Instant::now();
        let diapositivas = dividir_diapositivas(&nota);
        let split = t0.elapsed();
        assert_eq!(diapositivas.len(), 40);
        println!(
            "SC-1 dividir_diapositivas en {} KiB, 40 diapositivas: {:.2} ms",
            nota.len() / 1024,
            split.as_secs_f64() * 1000.0
        );
        let (mas_larga, largo) = diapositivas
            .iter()
            .map(|d| (d, d.texto.len()))
            .fold((&diapositivas[0], 0), |a, b| if b.1 > a.1 { b } else { a });
        let t1 = std::time::Instant::now();
        let _prepared = Prepared::new(&mas_larga.texto, &[]);
        let prep = t1.elapsed();
        println!(
            "SC-1 Prepared::new de la diapositiva más larga ({} KiB): {:.2} ms",
            largo / 1024,
            prep.as_secs_f64() * 1000.0
        );
        assert!(
            split.as_millis() < 16,
            "split {:?} fuera de presupuesto",
            split
        );
        assert!(
            prep.as_millis() < 16,
            "prepared {:?} fuera de presupuesto",
            prep
        );
    }
}
