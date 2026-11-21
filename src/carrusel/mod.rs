//! Carrusel de medios (032) en la Lectura, en Dividida y en OnStage (usan el mismo `Reader`).
//!
//! Un solo widget anclado por carrusel. Muestra una diapositiva a la vez: solo esa tiene widgets,
//! y las imágenes de la vigente y de las vecinas se decodifican fuera del hilo de GTK, achicadas
//! al escenario ([`imagenes`]). Video y audio usan el reproductor de la 057, que no abre el
//! archivo hasta que se pide reproducir; un PDF se abre en el visor de la 016.
//!
//! Teclado: ←/→, Inicio/Fin y F (pantalla completa, con la vista ampliada); Esc sale de la
//! pantalla completa. El pase automático se detiene al interactuar, fuera de la vista y con la
//! nota cerrada: sin temporizador vivo.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

use adw::prelude::*;
use gtk::{gdk, glib};
use orbital_vault::carrusel::{Carrusel, Clase, Diapositiva, Leyenda, Posicion};

use crate::imagenes;

mod zoom;

/// Abrir un enlace (un PDF en su visor), relativo a la nota del carrusel.
pub type AbrirFn = Rc<dyn Fn(&str)>;

/// Alto y ancho de las miniaturas.
const MINI_ANCHO: i32 = 96;
const MINI_ALTO: i32 = 64;
/// Duración del fundido entre diapositivas.
const FUNDIDO_MS: u32 = 180;

/// En la nota o en su propia ventana a pantalla completa.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Modo {
    EnLinea,
    PantallaCompleta,
}

struct Inner {
    c: Rc<Carrusel>,
    modo: Modo,
    abrir: AbrirFn,
    indice: Cell<usize>,
    /// Débil: el widget es dueño del carrusel (lo guarda en sus datos), no al revés.
    raiz: glib::WeakRef<gtk::Box>,
    escenario: gtk::Stack,
    anterior: gtk::Button,
    siguiente: gtk::Button,
    contador: gtk::Label,
    leyenda_encima: gtk::Label,
    leyenda_debajo: gtk::Label,
    pausa: Option<gtk::Button>,
    miniaturas: Option<Miniaturas>,
    /// Tamaño del escenario en píxeles.
    ancho: Cell<i32>,
    alto: Cell<i32>,
    /// El pase automático está detenido por la usuaria (o por interactuar).
    pausado: Cell<bool>,
    en_vista: Cell<bool>,
    reloj: RefCell<Option<glib::SourceId>>,
    /// La señal del desplazamiento de la vista que lo contiene (para saber si está a la vista).
    vigia: RefCell<Option<(gtk::Adjustment, glib::SignalHandlerId)>>,
    /// Las miniaturas ya se pidieron.
    miniaturas_pedidas: Cell<bool>,
    /// Hay una copia a pantalla completa abierta: esta no avanza sola.
    tapado: Cell<bool>,
    /// La imagen ampliable de la diapositiva vigente (solo a pantalla completa).
    zoom: RefCell<Option<Rc<zoom::Zoom>>>,
    /// El panel de información (solo a pantalla completa).
    info: Option<Info>,
}

/// El panel con el nombre, las medidas, el tamaño y la fecha del archivo vigente.
struct Info {
    revelador: gtk::Revealer,
    texto: gtk::Label,
    boton: gtk::ToggleButton,
}

impl Inner {
    fn raiz(&self) -> gtk::Box {
        self.raiz.upgrade().unwrap_or_default()
    }
}

struct Miniaturas {
    scroller: gtk::ScrolledWindow,
    botones: Vec<gtk::ToggleButton>,
    imagenes: Vec<Option<gtk::Picture>>,
}

/// Un carrusel: el widget se consigue con [`Vista::widget`].
#[derive(Clone)]
pub struct Vista(Rc<Inner>);

/// El carrusel de la nota, del ancho de la columna `ancho`.
pub fn widget(c: Rc<Carrusel>, ancho: i32, abrir: AbrirFn) -> gtk::Widget {
    Vista::nueva(c, Modo::EnLinea, abrir, 0, ancho).1.upcast()
}

/// Alto del escenario en la nota para un ancho de columna.
fn alto_en_linea(ancho: i32) -> i32 {
    (ancho * 9 / 16).clamp(180, 480)
}

/// Si el foco de la ventana está dentro de un carrusel (sus teclas no las toma OnStage).
pub fn tiene_foco(ventana: &gtk::Window) -> bool {
    let mut w = gtk::prelude::GtkWindowExt::focus(ventana);
    while let Some(x) = w {
        if x.has_css_class("orbital-carrusel") {
            return true;
        }
        w = x.parent();
    }
    false
}

impl Vista {
    /// El carrusel y su widget (el widget es el dueño: hay que guardarlo).
    fn nueva(
        c: Rc<Carrusel>,
        modo: Modo,
        abrir: AbrirFn,
        indice: usize,
        ancho: i32,
    ) -> (Self, gtk::Box) {
        let n = c.diapositivas.len();
        let o = &c.opciones;
        let raiz = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(6)
            .focusable(true)
            .css_classes(["orbital-carrusel"])
            .build();
        raiz.set_accessible_role(gtk::AccessibleRole::Group);
        raiz.update_property(&[
            gtk::accessible::Property::Label(&format!(
                "Carrusel: {n} {}",
                if n == 1 { "medio" } else { "medios" }
            )),
            gtk::accessible::Property::Description(
                "Flechas izquierda y derecha para pasar de diapositiva; Inicio y Fin para ir al principio o al final",
            ),
        ]);
        if modo == Modo::PantallaCompleta {
            raiz.update_property(&[gtk::accessible::Property::Description(
                "Flechas para pasar de foto; Inicio y Fin; más, menos y cero para el zoom; I para la información; P para el pase automático; Escape para salir",
            )]);
            raiz.add_css_class("orbital-carrusel-completo");
            raiz.set_vexpand(true);
        }
        let escenario = gtk::Stack::builder()
            .transition_type(gtk::StackTransitionType::Crossfade)
            .transition_duration(FUNDIDO_MS)
            .hhomogeneous(false)
            .vhomogeneous(false)
            .hexpand(true)
            .vexpand(modo == Modo::PantallaCompleta)
            .css_classes(["orbital-carrusel-escenario"])
            .build();
        let flecha = |icono: &str, nombre: &str, lado: gtk::Align| {
            gtk::Button::builder()
                .icon_name(icono)
                .tooltip_text(nombre)
                .halign(lado)
                .valign(gtk::Align::Center)
                .margin_start(8)
                .margin_end(8)
                .css_classes(["circular", "osd", "orbital-carrusel-flecha"])
                .visible(n > 1)
                .build()
        };
        let anterior = flecha(
            "go-previous-symbolic",
            "Diapositiva anterior",
            gtk::Align::Start,
        );
        let siguiente = flecha("go-next-symbolic", "Diapositiva siguiente", gtk::Align::End);
        let leyenda_encima = gtk::Label::builder()
            .wrap(true)
            .wrap_mode(gtk::pango::WrapMode::WordChar)
            .xalign(0.5)
            .halign(gtk::Align::Center)
            .valign(gtk::Align::End)
            .margin_bottom(10)
            .margin_start(56)
            .margin_end(56)
            .css_classes(["orbital-carrusel-leyenda", "osd"])
            .visible(false)
            .build();
        let sobre = gtk::Overlay::builder().child(&escenario).build();
        sobre.add_overlay(&leyenda_encima);
        sobre.add_overlay(&anterior);
        sobre.add_overlay(&siguiente);
        let info = (modo == Modo::PantallaCompleta).then(|| {
            let texto = gtk::Label::builder()
                .xalign(0.0)
                .selectable(true)
                .wrap(true)
                .wrap_mode(gtk::pango::WrapMode::WordChar)
                .max_width_chars(40)
                .build();
            let caja = gtk::Box::builder()
                .css_classes(["orbital-visor-info"])
                .build();
            caja.append(&texto);
            caja.set_accessible_role(gtk::AccessibleRole::Group);
            caja.update_property(&[gtk::accessible::Property::Label("Información del archivo")]);
            let revelador = gtk::Revealer::builder()
                .child(&caja)
                .transition_type(gtk::RevealerTransitionType::Crossfade)
                .halign(gtk::Align::End)
                .valign(gtk::Align::Start)
                .margin_top(12)
                .margin_end(12)
                .build();
            sobre.add_overlay(&revelador);
            let boton = gtk::ToggleButton::builder()
                .icon_name("dialog-information-symbolic")
                .tooltip_text("Información (I)")
                .css_classes(["flat"])
                .build();
            boton.update_property(&[gtk::accessible::Property::Label(
                "Información del archivo (I)",
            )]);
            Info {
                revelador,
                texto,
                boton,
            }
        });
        let leyenda_debajo = gtk::Label::builder()
            .wrap(true)
            .wrap_mode(gtk::pango::WrapMode::WordChar)
            .xalign(0.5)
            .css_classes(["orbital-carrusel-leyenda-debajo"])
            .visible(false)
            .build();

        // La barra: contador, pase automático, copiar y pantalla completa.
        let barra = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(4)
            .build();
        let contador = gtk::Label::builder()
            .xalign(0.0)
            .hexpand(true)
            .css_classes(["dim-label", "numeric"])
            .visible(n > 1)
            .build();
        barra.append(&contador);
        let pausa = (o.autoplay && n > 1).then(|| {
            gtk::Button::builder()
                .icon_name("media-playback-pause-symbolic")
                .tooltip_text("Pausar el pase automático")
                .css_classes(["flat", "orbital-carrusel-pausa"])
                .build()
        });
        if let Some(p) = &pausa {
            barra.append(p);
        }
        let ampliada = o.ampliada || modo == Modo::PantallaCompleta;
        let copiar = gtk::Button::builder()
            .icon_name("edit-copy-symbolic")
            .tooltip_text("Copiar embed")
            .css_classes(["flat"])
            .visible(ampliada && n > 0)
            .build();
        barra.append(&copiar);
        let completa = gtk::Button::builder()
            .icon_name(if modo == Modo::PantallaCompleta {
                "view-restore-symbolic"
            } else {
                "view-fullscreen-symbolic"
            })
            .tooltip_text(if modo == Modo::PantallaCompleta {
                "Salir de pantalla completa (Esc)"
            } else {
                "Pantalla completa (F)"
            })
            .css_classes(["flat"])
            .visible(ampliada && n > 0)
            .build();
        if let Some(i) = &info {
            barra.append(&i.boton);
        }
        barra.append(&completa);

        // Las miniaturas, armadas sin imágenes (se piden al mostrarse).
        let miniaturas = (o.miniaturas && n > 1).then(|| Miniaturas::nuevas(&c, o.posicion));
        let horizontal = matches!(o.posicion, Posicion::Izquierda | Posicion::Derecha);
        let centro = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(6)
            .vexpand(modo == Modo::PantallaCompleta)
            .build();
        if let Some(m) = &miniaturas
            && matches!(o.posicion, Posicion::Arriba)
        {
            raiz.append(&m.scroller);
        }
        if let Some(m) = &miniaturas
            && matches!(o.posicion, Posicion::Izquierda)
        {
            centro.append(&m.scroller);
        }
        centro.append(&sobre);
        if let Some(m) = &miniaturas
            && matches!(o.posicion, Posicion::Derecha)
        {
            centro.append(&m.scroller);
        }
        raiz.append(&centro);
        raiz.append(&leyenda_debajo);
        if n > 1 || ampliada {
            raiz.append(&barra);
        }
        if let Some(m) = &miniaturas
            && matches!(o.posicion, Posicion::Abajo)
        {
            raiz.append(&m.scroller);
        }
        let ancho_escenario = if horizontal && miniaturas.is_some() {
            ancho - MINI_ANCHO - 30
        } else {
            ancho
        };
        let this = Self(Rc::new(Inner {
            modo,
            abrir,
            indice: Cell::new(indice.min(n.saturating_sub(1))),
            raiz: raiz.downgrade(),
            escenario,
            anterior,
            siguiente,
            contador,
            leyenda_encima,
            leyenda_debajo,
            pausa,
            miniaturas,
            ancho: Cell::new(ancho_escenario.max(120)),
            alto: Cell::new(alto_en_linea(ancho)),
            pausado: Cell::new(false),
            en_vista: Cell::new(modo == Modo::PantallaCompleta),
            reloj: RefCell::new(None),
            vigia: RefCell::new(None),
            miniaturas_pedidas: Cell::new(false),
            tapado: Cell::new(false),
            zoom: RefCell::new(None),
            info,
            c,
        }));
        if modo == Modo::EnLinea {
            this.0.escenario.set_size_request(-1, this.0.alto.get());
        }
        this.conectar(&copiar, &completa);
        // El widget es dueño del carrusel: se libera con él.
        // SAFETY: la clave es propia de este módulo y solo se guarda un `Rc<Inner>`.
        unsafe { raiz.set_data("orbital-carrusel", this.0.clone()) };
        (this, raiz)
    }

    fn conectar(&self, copiar: &gtk::Button, completa: &gtk::Button) {
        let i = &self.0;
        let debil = || Rc::downgrade(&self.0);
        {
            let d = debil();
            i.anterior.connect_clicked(move |_| {
                if let Some(i) = d.upgrade() {
                    let v = Vista(i);
                    v.interaccion();
                    v.ir_a(v.0.indice.get().saturating_sub(1));
                }
            });
        }
        {
            let d = debil();
            i.siguiente.connect_clicked(move |_| {
                if let Some(i) = d.upgrade() {
                    let v = Vista(i);
                    v.interaccion();
                    v.ir_a(v.0.indice.get() + 1);
                }
            });
        }
        {
            let d = debil();
            copiar.connect_clicked(move |_| {
                if let Some(i) = d.upgrade() {
                    Vista(i).copiar();
                }
            });
        }
        {
            let d = debil();
            completa.connect_clicked(move |_| {
                if let Some(i) = d.upgrade() {
                    Vista(i).alternar_pantalla_completa();
                }
            });
        }
        if let Some(info) = &i.info {
            let d = debil();
            info.boton.connect_toggled(move |_| {
                if let Some(i) = d.upgrade() {
                    Vista(i).mostrar_info();
                }
            });
        }
        if let Some(p) = &i.pausa {
            let d = debil();
            p.connect_clicked(move |_| {
                if let Some(i) = d.upgrade() {
                    let v = Vista(i);
                    v.0.pausado.set(!v.0.pausado.get());
                    v.mostrar_pausa();
                    v.programar();
                }
            });
        }
        if let Some(m) = &i.miniaturas {
            for (n, b) in m.botones.iter().enumerate() {
                let d = debil();
                b.connect_toggled(move |b| {
                    if !b.is_active() {
                        return;
                    }
                    if let Some(i) = d.upgrade() {
                        let v = Vista(i);
                        if v.0.indice.get() != n {
                            v.interaccion();
                            v.ir_a(n);
                        }
                    }
                });
            }
        }
        // Teclado (en burbuja: un reproductor con el foco usa sus flechas primero).
        let teclas = gtk::EventControllerKey::new();
        {
            let d = debil();
            teclas.connect_key_pressed(move |_, k, _, mods| {
                d.upgrade()
                    .map_or(glib::Propagation::Proceed, |i| Vista(i).tecla(k, mods))
            });
        }
        i.raiz().add_controller(teclas);
        // Cualquier clic adentro cuenta como interactuar (pausa el pase automático), menos el
        // del botón de pausa, que decide solo.
        if i.c.opciones.autoplay {
            let clic = gtk::GestureClick::new();
            clic.set_propagation_phase(gtk::PropagationPhase::Capture);
            let d = debil();
            clic.connect_pressed(move |g, _, x, y| {
                let Some(i) = d.upgrade() else { return };
                let sobre_pausa = g
                    .widget()
                    .and_then(|w| w.pick(x, y, gtk::PickFlags::DEFAULT))
                    .is_some_and(|w| {
                        i.pausa.as_ref().is_some_and(|p| {
                            w == *p.upcast_ref::<gtk::Widget>() || w.is_ancestor(p)
                        })
                    });
                if !sobre_pausa {
                    Vista(i).interaccion();
                }
            });
            i.raiz().add_controller(clic);
        }
        // Mostrar la primera diapositiva al aparecer (no antes: el reproductor y la imagen no
        // se piden hasta que el carrusel está en la vista) y seguir si está a la vista.
        {
            let d = debil();
            i.raiz().connect_map(move |_| {
                if let Some(i) = d.upgrade() {
                    let v = Vista(i);
                    if v.0.escenario.visible_child().is_none() {
                        v.mostrar(v.0.indice.get(), false);
                    }
                    v.vigilar_vista();
                    v.programar();
                }
            });
        }
        {
            let d = debil();
            i.raiz().connect_unmap(move |_| {
                if let Some(i) = d.upgrade() {
                    let v = Vista(i);
                    v.detener();
                    if let Some((adj, id)) = v.0.vigia.borrow_mut().take() {
                        adj.disconnect(id);
                    }
                }
            });
        }
        // La columna cambió de ancho: el escenario sigue.
        if i.modo == Modo::EnLinea {
            let d = debil();
            i.raiz().connect_width_request_notify(move |r| {
                let Some(i) = d.upgrade() else { return };
                let w = r.width_request();
                if w <= 0 {
                    return;
                }
                let lateral = matches!(
                    i.c.opciones.posicion,
                    Posicion::Izquierda | Posicion::Derecha
                ) && i.miniaturas.is_some();
                let ancho = if lateral { w - MINI_ANCHO - 30 } else { w }.max(120);
                let alto = alto_en_linea(w);
                if ancho == i.ancho.get() && alto == i.alto.get() {
                    return;
                }
                i.ancho.set(ancho);
                i.alto.set(alto);
                i.escenario.set_size_request(-1, alto);
                let v = Vista(i);
                if v.0.escenario.visible_child().is_some() {
                    v.mostrar(v.0.indice.get(), false);
                }
            });
        }
        // Al salir el escenario de su fundido, quedan solo los widgets de la vigente.
        {
            let d = debil();
            i.escenario.connect_transition_running_notify(move |s| {
                if s.is_transition_running() {
                    return;
                }
                if d.upgrade().is_some() {
                    limpiar_escenario(s);
                }
            });
        }
    }

    fn tecla(&self, k: gdk::Key, mods: gdk::ModifierType) -> glib::Propagation {
        use gdk::Key;
        if mods.intersects(
            gdk::ModifierType::CONTROL_MASK
                | gdk::ModifierType::ALT_MASK
                | gdk::ModifierType::SUPER_MASK,
        ) {
            return glib::Propagation::Proceed;
        }
        let n = self.0.c.diapositivas.len();
        let i = self.0.indice.get();
        // A pantalla completa: zoom, información y pase automático.
        if self.0.modo == Modo::PantallaCompleta {
            let z = self.0.zoom.borrow().clone();
            if let Some(z) = z
                && z.tecla(k)
            {
                return glib::Propagation::Stop;
            }
            match k {
                Key::i | Key::I => {
                    if let Some(info) = &self.0.info {
                        info.boton.set_active(!info.boton.is_active());
                    }
                    return glib::Propagation::Stop;
                }
                Key::p | Key::P if self.0.pausa.is_some() => {
                    if let Some(p) = &self.0.pausa {
                        p.emit_clicked();
                    }
                    return glib::Propagation::Stop;
                }
                _ => {}
            }
        }
        let destino = match k {
            Key::Left | Key::KP_Left if n > 1 => Some(i.saturating_sub(1)),
            Key::Right | Key::KP_Right if n > 1 => Some(i + 1),
            Key::Home | Key::KP_Home if n > 1 => Some(0),
            Key::End | Key::KP_End if n > 1 => Some(n - 1),
            Key::f | Key::F
                if self.0.c.opciones.ampliada || self.0.modo == Modo::PantallaCompleta =>
            {
                self.alternar_pantalla_completa();
                return glib::Propagation::Stop;
            }
            _ => None,
        };
        match destino {
            Some(d) => {
                self.interaccion();
                self.ir_a(d);
                glib::Propagation::Stop
            }
            None => glib::Propagation::Proceed,
        }
    }

    /// La diapositiva vigente (desde 0).
    pub fn indice(&self) -> usize {
        self.0.indice.get()
    }

    /// Va a la diapositiva `n` (acotada a las que hay).
    pub fn ir_a(&self, n: usize) {
        let total = self.0.c.diapositivas.len();
        if total == 0 {
            return;
        }
        let n = n.min(total - 1);
        if n == self.0.indice.get() && self.0.escenario.visible_child().is_some() {
            return;
        }
        self.0.indice.set(n);
        self.mostrar(n, true);
    }

    /// Pone la diapositiva `n` en el escenario y actualiza contador, leyenda, flechas y
    /// miniaturas. Solo crea los widgets de esa diapositiva.
    fn mostrar(&self, n: usize, fundido: bool) {
        let i = &self.0;
        let total = i.c.diapositivas.len();
        let Some(d) = i.c.diapositivas.get(n) else {
            return;
        };
        *i.zoom.borrow_mut() = None;
        let pagina = self.pagina(d, n);
        // El nombre de cada página es único: el fundido va de la vieja a la nueva.
        let nombre = format!("d{n}-{}", glib::monotonic_time());
        i.escenario.set_transition_type(if fundido && total > 1 {
            gtk::StackTransitionType::Crossfade
        } else {
            gtk::StackTransitionType::None
        });
        i.escenario.add_named(&pagina, Some(&nombre));
        i.escenario.set_visible_child(&pagina);
        if !i.escenario.is_transition_running() {
            limpiar_escenario(&i.escenario);
        }
        i.contador.set_label(&format!("{} de {total}", n + 1));
        let leyenda = d.leyenda.as_deref().unwrap_or("");
        let (visible, oculta) = match i.c.opciones.leyenda {
            Leyenda::Encima => (&i.leyenda_encima, &i.leyenda_debajo),
            Leyenda::Debajo => (&i.leyenda_debajo, &i.leyenda_encima),
        };
        visible.set_label(leyenda);
        visible.set_visible(!leyenda.is_empty());
        oculta.set_visible(false);
        // El foco no se pierde al llegar a un extremo: pasa al carrusel.
        for (b, puede) in [(&i.anterior, n > 0), (&i.siguiente, n + 1 < total)] {
            if !puede && b.has_focus() {
                i.raiz().grab_focus();
            }
            b.set_sensitive(puede);
        }
        if let Some(m) = &i.miniaturas {
            m.marcar(n);
        }
        // Las vecinas, por adelantado (solo imágenes: lo demás no se prepara).
        let (w, h) = self.medida_imagen();
        for v in [n + 1, n.wrapping_sub(1)] {
            if let Some(d) = i.c.diapositivas.get(v)
                && d.clase == Clase::Imagen
                && let Some(p) = d.abs()
            {
                imagenes::adelantar(p, w, h);
            }
        }
        self.pedir_miniaturas();
        self.mostrar_info();
    }

    /// Escribe en el panel de información (si está abierto) el nombre, las medidas, el tamaño y
    /// la fecha del archivo vigente; las medidas y el tamaño se leen en el pool de `gio`.
    fn mostrar_info(&self) {
        let i = &self.0;
        let Some(info) = &i.info else { return };
        info.revelador.set_reveal_child(info.boton.is_active());
        if !info.boton.is_active() {
            return;
        }
        let n = i.indice.get();
        let Some(d) = i.c.diapositivas.get(n) else {
            return;
        };
        let nombre = d.nombre().to_owned();
        info.texto.set_label(&nombre);
        let Some(abs) = d.abs().map(std::path::Path::to_path_buf) else {
            info.texto
                .set_label(&format!("{nombre}\nArchivo inexistente"));
            return;
        };
        let imagen = d.clase == Clase::Imagen;
        let debil = Rc::downgrade(&self.0);
        glib::spawn_future_local(async move {
            let datos = gtk::gio::spawn_blocking(move || {
                let m = std::fs::metadata(&abs).ok();
                let medida = if imagen {
                    imagenes::medida_de_archivo(&abs)
                } else {
                    None
                };
                (
                    medida,
                    m.as_ref().map(std::fs::Metadata::len),
                    m.and_then(|m| m.modified().ok()),
                )
            })
            .await;
            let (Some(i), Ok((medida, bytes, fecha))) = (debil.upgrade(), datos) else {
                return;
            };
            // Mientras tanto se pasó a otra.
            if i.indice.get() != n {
                return;
            }
            let mut t = nombre;
            if let Some((w, h)) = medida {
                t.push_str(&format!("\n{w} × {h} px"));
            }
            if let Some(b) = bytes {
                t.push_str(&format!("\n{}", glib::format_size(b)));
            }
            if let Some(f) = fecha
                .and_then(|f| f.duration_since(std::time::UNIX_EPOCH).ok())
                .and_then(|d| glib::DateTime::from_unix_local(d.as_secs() as i64).ok())
                .and_then(|d| d.format("%Y-%m-%d %H:%M").ok())
            {
                t.push_str(&format!("\n{f}"));
            }
            if let Some(info) = &i.info {
                info.texto.set_label(&t);
            }
        });
    }

    /// Tamaño en píxeles del dispositivo al que se decodifican las imágenes del escenario.
    fn medida_imagen(&self) -> (i32, i32) {
        let e = self.0.raiz().scale_factor().max(1);
        if self.0.modo == Modo::PantallaCompleta {
            let (w, h) = self.medida_pantalla();
            return (w * e, h * e);
        }
        (self.0.ancho.get() * e, self.0.alto.get() * e)
    }

    /// El tamaño del monitor de la ventana (o uno razonable si todavía no tiene).
    fn medida_pantalla(&self) -> (i32, i32) {
        let raiz = &self.0.raiz();
        raiz.native()
            .and_then(|n| n.surface())
            .and_then(|s| raiz.display().monitor_at_surface(&s))
            .map(|m| {
                let g = m.geometry();
                (g.width(), g.height())
            })
            .unwrap_or((1920, 1080))
    }

    /// Los widgets de una diapositiva.
    fn pagina(&self, d: &Diapositiva, n: usize) -> gtk::Widget {
        let i = &self.0;
        let total = i.c.diapositivas.len();
        let caja = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .halign(gtk::Align::Fill)
            .valign(gtk::Align::Fill)
            .hexpand(true)
            .vexpand(true)
            .css_classes(["orbital-carrusel-diapositiva"])
            .build();
        caja.set_accessible_role(gtk::AccessibleRole::Group);
        caja.update_property(&[gtk::accessible::Property::Label(&format!(
            "Diapositiva {} de {total}: {}",
            n + 1,
            d.descripcion()
        ))]);
        let Some(abs) = d.abs() else {
            caja.append(&tarjeta(
                "image-missing-symbolic",
                &format!("Archivo inexistente: «{}»", d.nombre()),
                None,
            ));
            return caja.upcast();
        };
        match d.clase {
            Clase::Imagen => {
                let alt = d.leyenda.as_deref().unwrap_or_else(|| d.nombre());
                // A pantalla completa, ampliable; en la nota, una imagen común.
                let pic = if i.modo == Modo::PantallaCompleta {
                    let debil = Rc::downgrade(&self.0);
                    let z = zoom::Zoom::nueva(
                        abs.to_path_buf(),
                        alt,
                        Rc::new(move |adelante| {
                            if let Some(i) = debil.upgrade() {
                                let v = Vista(i);
                                v.interaccion();
                                let n = v.0.indice.get();
                                v.ir_a(if adelante { n + 1 } else { n.saturating_sub(1) });
                            }
                        }),
                    );
                    caja.append(z.widget());
                    let p = z.picture().clone();
                    *i.zoom.borrow_mut() = Some(z);
                    p
                } else {
                    let p = gtk::Picture::builder()
                        .can_shrink(true)
                        .content_fit(gtk::ContentFit::Contain)
                        .hexpand(true)
                        .vexpand(true)
                        .build();
                    p.set_alternative_text(Some(alt));
                    caja.append(&p);
                    p
                };
                let (w, h) = self.medida_imagen();
                let debil = pic.downgrade();
                let nombre = d.nombre().to_owned();
                let caja_d = caja.downgrade();
                imagenes::pedir(abs, w, h, move |t| {
                    let Some(pic) = debil.upgrade() else { return };
                    match t {
                        Some(t) => pic.set_paintable(Some(&t)),
                        None => {
                            if let Some(c) = caja_d.upgrade() {
                                c.remove(&pic);
                                c.append(&tarjeta(
                                    "image-missing-symbolic",
                                    &format!("No se pudo abrir la imagen «{nombre}»"),
                                    None,
                                ));
                            }
                        }
                    }
                });
            }
            Clase::Audio | Clase::Video => {
                let tipo = if d.clase == Clase::Video {
                    orbital_vault::medios::Tipo::Video
                } else {
                    orbital_vault::medios::Tipo::Audio
                };
                let r = crate::medios::reproductor(
                    abs,
                    tipo,
                    d.fragmento
                        .as_deref()
                        .and_then(orbital_vault::medios::inicio),
                );
                // El video entra en el escenario a 16:9.
                let (ancho, alto) = if i.modo == Modo::PantallaCompleta {
                    self.medida_pantalla()
                } else {
                    (i.ancho.get(), i.alto.get())
                };
                let w = ancho.min((alto - 70).max(90) * 16 / 9) - 24;
                r.set_size_request(w.max(200), -1);
                r.set_halign(gtk::Align::Center);
                r.set_valign(gtk::Align::Center);
                r.set_vexpand(true);
                caja.append(&r);
            }
            Clase::Pdf => {
                let b = gtk::Button::builder()
                    .label("Abrir en el visor PDF")
                    .css_classes(["pill", "suggested-action"])
                    .halign(gtk::Align::Center)
                    .build();
                b.update_property(&[gtk::accessible::Property::Label(&format!(
                    "Abrir {} en el visor PDF",
                    d.nombre()
                ))]);
                let enlace = d.enlace.clone();
                let d_vista = Rc::downgrade(&self.0);
                b.connect_clicked(move |_| {
                    let (Some(e), Some(i)) = (&enlace, d_vista.upgrade()) else {
                        return;
                    };
                    let abrir = i.abrir.clone();
                    // En pantalla completa, primero se vuelve a la ventana: el visor está ahí.
                    if i.modo == Modo::PantallaCompleta
                        && let Some(w) = i.raiz().root().and_downcast::<gtk::Window>()
                    {
                        w.close();
                    }
                    abrir(e);
                });
                caja.append(&tarjeta("x-office-document-symbolic", d.nombre(), Some(&b)));
            }
            Clase::Otro => {
                let b = gtk::Button::builder()
                    .label("Abrir con otra aplicación")
                    .css_classes(["pill"])
                    .halign(gtk::Align::Center)
                    .build();
                let p = abs.to_path_buf();
                b.connect_clicked(move |b| crate::medios::abrir_afuera(b, &p));
                caja.append(&tarjeta("text-x-generic-symbolic", d.nombre(), Some(&b)));
            }
        }
        caja.upcast()
    }

    fn copiar(&self) {
        let i = &self.0;
        let Some(d) = i.c.diapositivas.get(i.indice.get()) else {
            return;
        };
        let embed = d.embed();
        i.raiz().clipboard().set_text(&embed);
        avisar(&i.raiz(), &format!("Embed copiado: {embed}"));
    }

    fn alternar_pantalla_completa(&self) {
        if self.0.modo == Modo::PantallaCompleta {
            if let Some(w) = self.0.raiz().root().and_downcast::<gtk::Window>() {
                w.close();
            }
            return;
        }
        self.detener();
        self.0.tapado.set(true);
        // Al cerrar, la nota queda en la diapositiva en que se salió y con el foco en el carrusel.
        let en_linea = Rc::downgrade(&self.0);
        let completa = visor(
            &self.0.raiz(),
            self.0.c.clone(),
            self.0.indice.get(),
            self.0.abrir.clone(),
            self.0.pausado.get(),
        );
        let c = completa.clone();
        completa.al_cerrar(move || {
            if let Some(i) = en_linea.upgrade() {
                let v = Vista(i);
                v.0.tapado.set(false);
                v.0.pausado.set(c.0.pausado.get());
                v.mostrar_pausa();
                v.ir_a(c.indice());
                v.0.raiz().grab_focus();
                v.programar();
            }
        });
    }

    /// Lo que corre al cerrar la ventana del visor (después de soltar su reloj).
    fn al_cerrar(&self, f: impl Fn() + 'static) {
        let Some(ventana) = self.0.raiz().root().and_downcast::<gtk::Window>() else {
            return;
        };
        let c = self.clone();
        ventana.connect_close_request(move |_| {
            c.detener();
            f();
            glib::Propagation::Proceed
        });
    }

    /// La usuaria hizo algo en el carrusel: el pase automático se detiene hasta que lo reanude
    /// o vuelva a abrir la nota.
    fn interaccion(&self) {
        if self.0.c.opciones.autoplay && !self.0.pausado.get() {
            self.0.pausado.set(true);
            self.mostrar_pausa();
            self.detener();
        }
    }

    fn mostrar_pausa(&self) {
        if let Some(p) = &self.0.pausa {
            let (icono, texto) = if self.0.pausado.get() {
                (
                    "media-playback-start-symbolic",
                    "Reanudar el pase automático",
                )
            } else {
                ("media-playback-pause-symbolic", "Pausar el pase automático")
            };
            p.set_icon_name(icono);
            p.set_tooltip_text(Some(texto));
            p.update_property(&[gtk::accessible::Property::Label(texto)]);
        }
    }

    /// Arranca el reloj del pase automático si corresponde (y no hay uno).
    fn programar(&self) {
        let i = &self.0;
        let corre = i.c.opciones.autoplay
            && i.c.diapositivas.len() > 1
            && !i.pausado.get()
            && !i.tapado.get()
            && i.en_vista.get()
            && i.raiz().is_mapped();
        if !corre {
            self.detener();
            return;
        }
        if i.reloj.borrow().is_some() {
            return;
        }
        let d = Rc::downgrade(&self.0);
        let id = glib::timeout_add_local_once(
            Duration::from_secs_f32(i.c.opciones.velocidad),
            move || {
                let Some(i) = d.upgrade() else { return };
                i.reloj.borrow_mut().take();
                let v = Vista(i);
                let n = v.0.c.diapositivas.len();
                v.ir_a((v.0.indice.get() + 1) % n.max(1));
                v.programar();
            },
        );
        *i.reloj.borrow_mut() = Some(id);
    }

    fn detener(&self) {
        if let Some(id) = self.0.reloj.borrow_mut().take() {
            id.remove();
        }
    }

    /// Sigue si el carrusel está a la vista en la vista que lo contiene (para el pase
    /// automático). Sin pase automático, nada.
    fn vigilar_vista(&self) {
        let i = &self.0;
        if !i.c.opciones.autoplay || i.modo == Modo::PantallaCompleta {
            return;
        }
        let Some(sw) = i
            .raiz()
            .ancestor(gtk::ScrolledWindow::static_type())
            .and_downcast::<gtk::ScrolledWindow>()
        else {
            i.en_vista.set(true);
            return;
        };
        if i.vigia.borrow().is_some() {
            return;
        }
        let adj = sw.vadjustment();
        let d = Rc::downgrade(&self.0);
        let sw_d = sw.downgrade();
        let id = adj.connect_value_changed(move |_| {
            if let (Some(i), Some(sw)) = (d.upgrade(), sw_d.upgrade()) {
                Vista(i).revisar_vista(&sw);
            }
        });
        *i.vigia.borrow_mut() = Some((adj, id));
        // La primera vez, cuando ya tiene lugar.
        let d = Rc::downgrade(&self.0);
        glib::idle_add_local_once(move || {
            if let (Some(i), Some(sw)) = (d.upgrade(), sw.downgrade().upgrade()) {
                Vista(i).revisar_vista(&sw);
            }
        });
    }

    fn revisar_vista(&self, sw: &gtk::ScrolledWindow) {
        let i = &self.0;
        let visto = i.raiz().compute_bounds(sw).is_some_and(|r| {
            r.y() + r.height() > 0.0 && r.y() < sw.height() as f32 && r.height() > 0.0
        });
        if visto != i.en_vista.get() {
            i.en_vista.set(visto);
            self.programar();
        }
    }

    /// Pide las imágenes de las miniaturas una vez, de a una y empezando por la vigente.
    fn pedir_miniaturas(&self) {
        let i = &self.0;
        let Some(m) = &i.miniaturas else { return };
        if i.miniaturas_pedidas.replace(true) {
            return;
        }
        let e = i.raiz().scale_factor().max(1);
        let n = i.indice.get();
        let mut orden: Vec<usize> = (0..m.imagenes.len()).collect();
        orden.sort_by_key(|k| k.abs_diff(n));
        let trabajo: Vec<(usize, std::path::PathBuf)> = orden
            .into_iter()
            .filter(|k| m.imagenes[*k].is_some())
            .filter_map(|k| Some((k, i.c.diapositivas[k].abs()?.to_path_buf())))
            .collect();
        let d = Rc::downgrade(&self.0);
        glib::spawn_future_local(async move {
            for (k, p) in trabajo {
                let (tx, rx) = futures_oneshot();
                imagenes::pedir(&p, MINI_ANCHO * e, MINI_ALTO * e, tx);
                let t = rx.await;
                let Some(i) = d.upgrade() else { return };
                if let (Some(m), Some(t)) = (&i.miniaturas, t)
                    && let Some(Some(pic)) = m.imagenes.get(k)
                {
                    pic.set_paintable(Some(&t));
                }
            }
        });
    }
}

/// El visor a pantalla completa (compartido por el carrusel y la galería de la 033): una
/// ventana propia, modal y a pantalla completa, con otra instancia del carrusel que empieza en
/// `indice`. La ventana principal nunca pasa a pantalla completa (GTK 4.24 le pierde el árbol
/// AT-SPI al salir).
fn visor(desde: &gtk::Box, c: Rc<Carrusel>, indice: usize, abrir: AbrirFn, pausado: bool) -> Vista {
    let (completa, raiz_completa) = Vista::nueva(c, Modo::PantallaCompleta, abrir, indice, 0);
    completa.0.pausado.set(pausado);
    completa.mostrar_pausa();
    let tostadas = adw::ToastOverlay::new();
    tostadas.set_child(Some(&raiz_completa));
    let ventana = gtk::Window::builder()
        .title("Visor")
        .modal(true)
        .child(&tostadas)
        .css_classes(["orbital-carrusel-ventana"])
        .build();
    if let Some(madre) = desde.root().and_downcast::<gtk::Window>() {
        ventana.set_transient_for(Some(&madre));
        ventana.set_application(madre.application().as_ref());
    }
    let esc = gtk::EventControllerKey::new();
    {
        let v = ventana.downgrade();
        esc.connect_key_pressed(move |_, k, _, _| {
            if k == gdk::Key::Escape
                && let Some(v) = v.upgrade()
            {
                v.close();
                return glib::Propagation::Stop;
            }
            glib::Propagation::Proceed
        });
    }
    ventana.add_controller(esc);
    ventana.fullscreen();
    ventana.present();
    completa.0.raiz().grab_focus();
    completa
}

/// Abre el visor a pantalla completa desde otro widget (la galería, 033) en la diapositiva
/// `indice`. `pausado`: el pase automático espera a que se lo pida. `al_cerrar` recibe la
/// diapositiva en que se salió.
pub fn abrir_visor(
    desde: &gtk::Box,
    c: Rc<Carrusel>,
    indice: usize,
    abrir: AbrirFn,
    pausado: bool,
    al_cerrar: impl Fn(usize) + 'static,
) {
    let v = visor(desde, c, indice, abrir, pausado);
    let c = v.clone();
    v.al_cerrar(move || al_cerrar(c.indice()));
}

/// Un canal de un solo uso sobre el hilo de GTK.
fn futures_oneshot() -> (
    impl FnOnce(Option<gdk::Texture>),
    impl std::future::Future<Output = Option<gdk::Texture>>,
) {
    type Celda = (Option<Option<gdk::Texture>>, Option<std::task::Waker>);
    let celda: Rc<RefCell<Celda>> = Rc::default();
    let c2 = celda.clone();
    let tx = move |t: Option<gdk::Texture>| {
        let mut c = c2.borrow_mut();
        c.0 = Some(t);
        if let Some(w) = c.1.take() {
            w.wake();
        }
    };
    let rx = std::future::poll_fn(move |cx| {
        let mut c = celda.borrow_mut();
        match c.0.take() {
            Some(t) => std::task::Poll::Ready(t),
            None => {
                c.1 = Some(cx.waker().clone());
                std::task::Poll::Pending
            }
        }
    });
    (tx, rx)
}

impl Miniaturas {
    fn nuevas(c: &Carrusel, posicion: Posicion) -> Self {
        let vertical = matches!(posicion, Posicion::Izquierda | Posicion::Derecha);
        let caja = gtk::Box::builder()
            .orientation(if vertical {
                gtk::Orientation::Vertical
            } else {
                gtk::Orientation::Horizontal
            })
            .spacing(4)
            .css_classes(["orbital-carrusel-miniaturas"])
            .build();
        caja.set_accessible_role(gtk::AccessibleRole::Group);
        caja.update_property(&[gtk::accessible::Property::Label("Miniaturas")]);
        let total = c.diapositivas.len();
        let mut botones: Vec<gtk::ToggleButton> = Vec::with_capacity(total);
        let mut imagenes = Vec::with_capacity(total);
        for (n, d) in c.diapositivas.iter().enumerate() {
            let (hijo, pic): (gtk::Widget, Option<gtk::Picture>) =
                if d.clase == Clase::Imagen && d.abs().is_some() {
                    let p = gtk::Picture::builder()
                        .can_shrink(true)
                        .content_fit(gtk::ContentFit::Cover)
                        .width_request(MINI_ANCHO)
                        .height_request(MINI_ALTO)
                        .build();
                    p.set_accessible_role(gtk::AccessibleRole::Presentation);
                    (p.clone().upcast(), Some(p))
                } else {
                    // Lo que no es imagen: un marcador con su tipo.
                    let b = gtk::Box::builder()
                        .orientation(gtk::Orientation::Vertical)
                        .valign(gtk::Align::Center)
                        .width_request(MINI_ANCHO)
                        .height_request(MINI_ALTO)
                        .spacing(2)
                        .css_classes(["orbital-carrusel-marcador"])
                        .build();
                    let (icono, tipo) = if d.abs().is_none() {
                        ("image-missing-symbolic", "Falta")
                    } else {
                        (icono_de(d.clase), d.clase.nombre())
                    };
                    let im = gtk::Image::from_icon_name(icono);
                    im.set_vexpand(true);
                    im.set_valign(gtk::Align::End);
                    b.append(&im);
                    let l = gtk::Label::builder()
                        .label(tipo)
                        .css_classes(["caption"])
                        .vexpand(true)
                        .valign(gtk::Align::Start)
                        .build();
                    b.append(&l);
                    (b.upcast(), None)
                };
            let boton = gtk::ToggleButton::builder()
                .child(&hijo)
                .css_classes(["flat", "orbital-carrusel-miniatura"])
                .tooltip_text(d.leyenda.as_deref().unwrap_or_else(|| d.nombre()))
                .build();
            boton.update_property(&[gtk::accessible::Property::Label(&format!(
                "Ir a la diapositiva {} de {total}: {}",
                n + 1,
                d.descripcion()
            ))]);
            if let Some(p) = botones.first() {
                boton.set_group(Some(p));
            }
            caja.append(&boton);
            botones.push(boton);
            imagenes.push(pic);
        }
        let scroller = gtk::ScrolledWindow::builder()
            .child(&caja)
            .css_classes(["orbital-carrusel-tira"])
            .build();
        if vertical {
            scroller.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
            scroller.set_width_request(MINI_ANCHO + 24);
            scroller.set_vexpand(true);
        } else {
            scroller.set_policy(gtk::PolicyType::Automatic, gtk::PolicyType::Never);
            scroller.set_height_request(MINI_ALTO + 24);
        }
        Self {
            scroller,
            botones,
            imagenes,
        }
    }

    /// Marca la vigente y la trae a la vista.
    fn marcar(&self, n: usize) {
        let Some(b) = self.botones.get(n) else { return };
        if !b.is_active() {
            b.set_active(true);
        }
        let b = b.clone();
        let sw = self.scroller.downgrade();
        // Después de la disposición (las miniaturas recién creadas no tienen lugar todavía).
        glib::idle_add_local_once(move || {
            if let Some(vp) = sw
                .upgrade()
                .and_then(|s| s.child())
                .and_downcast::<gtk::Viewport>()
            {
                vp.scroll_to(&b, None);
            }
        });
    }
}

fn icono_de(c: Clase) -> &'static str {
    match c {
        Clase::Imagen => "image-x-generic-symbolic",
        Clase::Audio => "audio-x-generic-symbolic",
        Clase::Video => "video-x-generic-symbolic",
        Clase::Pdf => "x-office-document-symbolic",
        Clase::Otro => "text-x-generic-symbolic",
    }
}

/// Una tarjeta centrada: ícono, texto y un botón opcional.
fn tarjeta(icono: &str, texto: &str, boton: Option<&gtk::Button>) -> gtk::Widget {
    let b = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(10)
        .halign(gtk::Align::Center)
        .valign(gtk::Align::Center)
        .vexpand(true)
        .build();
    let im = gtk::Image::from_icon_name(icono);
    im.set_pixel_size(48);
    im.add_css_class("dim-label");
    im.set_accessible_role(gtk::AccessibleRole::Presentation);
    b.append(&im);
    b.append(
        &gtk::Label::builder()
            .label(texto)
            .wrap(true)
            .justify(gtk::Justification::Center)
            .build(),
    );
    if let Some(boton) = boton {
        b.append(boton);
    }
    b.upcast()
}

/// Saca del escenario todo menos la página visible (suelta reproductores e imágenes).
fn limpiar_escenario(s: &gtk::Stack) {
    let visible = s.visible_child();
    let mut h = s.first_child();
    while let Some(w) = h {
        h = w.next_sibling();
        if Some(&w) != visible.as_ref() {
            s.remove(&w);
        }
    }
}

/// Confirma una acción: con un aviso emergente si la ventana tiene dónde, y para el lector de
/// pantalla.
fn avisar(w: &gtk::Box, texto: &str) {
    if let Some(t) = w
        .ancestor(adw::ToastOverlay::static_type())
        .and_downcast::<adw::ToastOverlay>()
    {
        t.add_toast(adw::Toast::new(texto));
    }
    w.announce(texto, gtk::AccessibleAnnouncementPriority::Medium);
}
