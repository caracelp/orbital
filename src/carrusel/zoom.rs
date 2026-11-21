//! La imagen ampliable del visor a pantalla completa (033, compartido con el carrusel de la
//! 032): la rueda (con o sin Ctrl), `+`, `-` y doble clic acercan y alejan alrededor del
//! puntero; arrastrar recorre la foto ampliada sin salirse de los bordes; `0` o doble clic la
//! vuelven a ajustar. Al acercar se pide la imagen con más resolución (en el pool de `gio`,
//! acotada a [`crate::imagenes::LADO_MAXIMO`]). Con la imagen ajustada, deslizar con el dedo
//! pasa de foto.

use std::cell::Cell;
use std::path::PathBuf;
use std::rc::Rc;

use gtk::prelude::*;
use gtk::{gdk, glib};

const MAXIMO: f64 = 8.0;
const PASO: f64 = 1.25;

pub struct Zoom {
    sw: gtk::ScrolledWindow,
    pic: gtk::Picture,
    nivel: Cell<f64>,
    /// El puntero, en coordenadas de la vista (para acercar a su alrededor).
    puntero: Cell<Option<(f64, f64)>>,
    abs: PathBuf,
    /// Lado de la imagen con más resolución ya pedida.
    pedida: Cell<i32>,
    inicio_arrastre: Cell<(f64, f64)>,
}

/// Lo que hace un deslizamiento con la imagen ajustada: `true` hacia la siguiente.
pub type Deslizar = Rc<dyn Fn(bool)>;

impl Zoom {
    /// La imagen (todavía sin textura) y su control.
    pub fn nueva(abs: PathBuf, alt: &str, deslizar: Deslizar) -> Rc<Self> {
        let pic = gtk::Picture::builder()
            .can_shrink(true)
            .content_fit(gtk::ContentFit::Contain)
            .hexpand(true)
            .vexpand(true)
            .build();
        pic.set_alternative_text(Some(alt));
        let sw = gtk::ScrolledWindow::builder()
            .child(&pic)
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vscrollbar_policy(gtk::PolicyType::Never)
            .hexpand(true)
            .vexpand(true)
            .css_classes(["orbital-visor-zoom"])
            .build();
        sw.update_property(&[gtk::accessible::Property::Description(
            "Rueda, más y menos para acercar; doble clic o cero para ajustar; arrastrar para recorrer",
        )]);
        let z = Rc::new(Self {
            sw,
            pic,
            nivel: Cell::new(1.0),
            puntero: Cell::new(None),
            abs,
            pedida: Cell::new(0),
            inicio_arrastre: Cell::new((0.0, 0.0)),
        });
        z.conectar(deslizar);
        z
    }

    pub fn widget(&self) -> &gtk::ScrolledWindow {
        &self.sw
    }

    pub fn picture(&self) -> &gtk::Picture {
        &self.pic
    }

    pub fn nivel(&self) -> f64 {
        self.nivel.get()
    }

    fn conectar(self: &Rc<Self>, deslizar: Deslizar) {
        let movimiento = gtk::EventControllerMotion::new();
        {
            let d = Rc::downgrade(self);
            movimiento.connect_motion(move |_, x, y| {
                if let Some(z) = d.upgrade() {
                    z.puntero.set(Some((x, y)));
                }
            });
        }
        self.sw.add_controller(movimiento);
        let rueda = gtk::EventControllerScroll::new(gtk::EventControllerScrollFlags::VERTICAL);
        {
            let d = Rc::downgrade(self);
            rueda.connect_scroll(move |_, _, dy| {
                let Some(z) = d.upgrade() else {
                    return glib::Propagation::Proceed;
                };
                if dy < 0.0 {
                    z.acercar(PASO);
                } else if dy > 0.0 {
                    z.acercar(1.0 / PASO);
                }
                glib::Propagation::Stop
            });
        }
        self.sw.add_controller(rueda);
        let doble = gtk::GestureClick::new();
        {
            let d = Rc::downgrade(self);
            doble.connect_pressed(move |_, n, x, y| {
                let Some(z) = d.upgrade() else { return };
                if n == 2 {
                    z.puntero.set(Some((x, y)));
                    if z.nivel() > 1.0 {
                        z.ajustar();
                    } else {
                        z.poner(2.5);
                    }
                }
            });
        }
        self.sw.add_controller(doble);
        let arrastre = gtk::GestureDrag::new();
        {
            let d = Rc::downgrade(self);
            arrastre.connect_drag_begin(move |_, _, _| {
                if let Some(z) = d.upgrade() {
                    z.inicio_arrastre
                        .set((z.sw.hadjustment().value(), z.sw.vadjustment().value()));
                }
            });
            let d = Rc::downgrade(self);
            arrastre.connect_drag_update(move |_, dx, dy| {
                let Some(z) = d.upgrade() else { return };
                if z.nivel() <= 1.0 {
                    return;
                }
                let (h, v) = z.inicio_arrastre.get();
                // Los ajustes ya se acotan a los bordes de la imagen.
                z.sw.hadjustment().set_value(h - dx);
                z.sw.vadjustment().set_value(v - dy);
            });
        }
        self.sw.add_controller(arrastre);
        let dedo = gtk::GestureSwipe::new();
        dedo.set_touch_only(true);
        {
            let d = Rc::downgrade(self);
            dedo.connect_swipe(move |_, vx, vy| {
                if d.upgrade().is_some_and(|z| z.nivel() <= 1.0)
                    && vx.abs() > vy.abs()
                    && vx.abs() > 300.0
                {
                    deslizar(vx < 0.0);
                }
            });
        }
        self.sw.add_controller(dedo);
    }

    /// Multiplica el nivel por `factor` (acotado entre ajustada y [`MAXIMO`]).
    pub fn acercar(self: &Rc<Self>, factor: f64) {
        self.poner(self.nivel() * factor);
    }

    /// Vuelve a la imagen entera en la pantalla.
    pub fn ajustar(self: &Rc<Self>) {
        self.poner(1.0);
    }

    fn poner(self: &Rc<Self>, nivel: f64) {
        let nivel = nivel.clamp(1.0, MAXIMO);
        let antes = self.nivel.replace(nivel);
        if (antes - nivel).abs() < f64::EPSILON {
            return;
        }
        let pic = &self.pic;
        let Some(t) = pic.paintable() else { return };
        if nivel <= 1.0 {
            self.sw
                .set_policy(gtk::PolicyType::Never, gtk::PolicyType::Never);
            pic.set_size_request(-1, -1);
            pic.set_halign(gtk::Align::Fill);
            pic.set_valign(gtk::Align::Fill);
            pic.set_content_fit(gtk::ContentFit::Contain);
            pic.set_cursor(None);
            return;
        }
        let (vw, vh) = (f64::from(self.sw.width()), f64::from(self.sw.height()));
        let (tw, th) = (
            f64::from(t.intrinsic_width().max(1)),
            f64::from(t.intrinsic_height().max(1)),
        );
        let ajuste = (vw / tw).min(vh / th);
        let (dw, dh) = (tw * ajuste * nivel, th * ajuste * nivel);
        self.sw
            .set_policy(gtk::PolicyType::External, gtk::PolicyType::External);
        pic.set_content_fit(gtk::ContentFit::Fill);
        pic.set_halign(gtk::Align::Center);
        pic.set_valign(gtk::Align::Center);
        pic.set_size_request(dw.round() as i32, dh.round() as i32);
        pic.set_cursor_from_name(Some("grab"));
        // Que el punto bajo el puntero (o el centro) quede en su lugar, cuando la vista ya
        // tenga el tamaño nuevo.
        let (px, py) = self.puntero.get().unwrap_or((vw / 2.0, vh / 2.0));
        let (h0, v0) = (self.sw.hadjustment().value(), self.sw.vadjustment().value());
        // El desplazamiento del contenido cuando era más chico que la vista (centrado).
        let (w0, h0d) = if antes <= 1.0 {
            (tw * ajuste, th * ajuste)
        } else {
            (tw * ajuste * antes, th * ajuste * antes)
        };
        let (ox, oy) = (((vw - w0) / 2.0).max(0.0), ((vh - h0d) / 2.0).max(0.0));
        let r = nivel / antes;
        let hx = ((h0 + px - ox) * r - px).max(0.0);
        let vy = ((v0 + py - oy) * r - py).max(0.0);
        let d = Rc::downgrade(self);
        self.sw.add_tick_callback(move |_, _| {
            if let Some(z) = d.upgrade() {
                z.sw.hadjustment().set_value(hx);
                z.sw.vadjustment().set_value(vy);
            }
            glib::ControlFlow::Break
        });
        self.pedir_resolucion(dw.max(dh));
    }

    /// Con la foto ampliada, la pide con más píxeles (si la de la pantalla queda chica).
    fn pedir_resolucion(self: &Rc<Self>, lado: f64) {
        let e = self.sw.scale_factor().max(1);
        let lado = ((lado * f64::from(e)) as i32).min(crate::imagenes::LADO_MAXIMO);
        let actual = self
            .pic
            .paintable()
            .map_or(0, |t| t.intrinsic_width().max(t.intrinsic_height()));
        if lado <= actual * 5 / 4 || lado <= self.pedida.get() {
            return;
        }
        // De a saltos grandes: no se decodifica en cada paso de la rueda.
        let lado = (lado * 2).min(crate::imagenes::LADO_MAXIMO);
        self.pedida.set(lado);
        let d = Rc::downgrade(self);
        crate::imagenes::pedir(&self.abs, lado, lado, move |t| {
            if let (Some(z), Some(t)) = (d.upgrade(), t)
                && z.nivel() > 1.0
            {
                z.pic.set_paintable(Some(&t));
            }
        });
    }

    /// La tecla de zoom, si lo es.
    pub fn tecla(self: &Rc<Self>, k: gdk::Key) -> bool {
        use gdk::Key;
        match k {
            Key::plus | Key::equal | Key::KP_Add => self.acercar(PASO),
            Key::minus | Key::KP_Subtract => self.acercar(1.0 / PASO),
            Key::_0 | Key::KP_0 => self.ajustar(),
            _ => return false,
        }
        true
    }
}
