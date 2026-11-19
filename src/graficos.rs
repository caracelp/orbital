//! Gráficos 2D y polares en la app (052): el motor es `orbital_graficos` (sin GTK); acá viven el
//! widget que los dibuja con GSK, la interacción de la Lectura y las cachés.
//!
//! - [`Lienzo`] dibuja la grilla, los ejes, las series, el título y la leyenda con
//!   `gsk::Path` (sin Cairo ni texturas). Las líneas están en coordenadas del plano: al mover o
//!   acercar se vuelven a llevar a píxeles en cada cuadro (unos miles de puntos), y el muestreo de
//!   la vista nueva va al pool de `gio`, de a uno por gráfico (si llegan más pedidos mientras
//!   tanto, se hace solo el último). Lo ya muestreado cubre un margen alrededor de la vista: al
//!   arrastrar no aparecen huecos mientras llega lo nuevo.
//! - [`GraficoVista`] suma la barra de la Lectura: coordenadas bajo el puntero, «Volver a la vista
//!   original» y un deslizador por parámetro. Nada de eso toca la nota.
//! - Estático (OnStage, globito de Código): solo el lienzo, sin controles.
//! - Cachés: el bloque leído (por fuente), el muestreo de la vista inicial (por fuente y tamaño)
//!   y el SVG (exportar y notas `.html`). Reabrir una nota no vuelve a muestrear.

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, VecDeque};
use std::rc::Rc;
use std::sync::{Arc, LazyLock, Mutex};

use gtk::{gdk, gio, glib, graphene, gsk, pango, prelude::*, subclass::prelude::*};
use orbital_graficos::{
    Estilo, Grafico, Paleta, Trazo, Vista, bloque, color_de_serie, guias, muestreo,
};

/// Lo que se muestrea de más alrededor de la vista, por lado (fracción del ancho y del alto).
const MARGEN: f64 = 0.25;
/// Entradas de cada caché.
const MAX_CACHE: usize = 96;

// --- Cachés ---------------------------------------------------------------------------------

struct Lru<K, V> {
    mapa: HashMap<K, V>,
    orden: VecDeque<K>,
}

impl<K: std::hash::Hash + Eq + Clone, V: Clone> Lru<K, V> {
    fn new() -> Self {
        Self {
            mapa: HashMap::new(),
            orden: VecDeque::new(),
        }
    }
    fn get(&self, k: &K) -> Option<V> {
        self.mapa.get(k).cloned()
    }
    fn put(&mut self, k: K, v: V) {
        if self.mapa.insert(k.clone(), v).is_none() {
            self.orden.push_back(k);
        }
        while self.orden.len() > MAX_CACHE {
            if let Some(viejo) = self.orden.pop_front() {
                self.mapa.remove(&viejo);
            }
        }
    }
}

type Leido = Result<Arc<Grafico>, bloque::Error>;
static LEIDOS: LazyLock<Mutex<Lru<Arc<str>, Leido>>> = LazyLock::new(|| Mutex::new(Lru::new()));

/// El bloque leído, con caché por fuente.
pub fn leer(fuente: &str) -> Leido {
    let clave: Arc<str> = Arc::from(fuente);
    if let Some(r) = LEIDOS.lock().ok().and_then(|c| c.get(&clave)) {
        return r;
    }
    let r = orbital_graficos::leer(fuente).map(Arc::new);
    if let Ok(mut c) = LEIDOS.lock() {
        c.put(clave, r.clone());
    }
    r
}

type Muestras = (Vista, Arc<Vec<Trazo>>);
/// (fuente, ancho, alto) → el muestreo de la vista inicial con los valores iniciales.
type CacheMuestras = Mutex<Lru<(Arc<str>, u32, u32), Muestras>>;
static MUESTRAS: LazyLock<CacheMuestras> = LazyLock::new(|| Mutex::new(Lru::new()));

type CacheSvg = Mutex<Lru<(Arc<str>, u32), Result<String, bloque::Error>>>;
static SVGS: LazyLock<CacheSvg> = LazyLock::new(|| Mutex::new(Lru::new()));

/// El SVG estático (exportar, notas `.html`): papel, `ancho` píxeles.
pub fn svg(fuente: &str, ancho: u32) -> Result<String, bloque::Error> {
    let clave = (Arc::from(fuente), ancho);
    if let Some(r) = SVGS.lock().ok().and_then(|c| c.get(&clave)) {
        return r;
    }
    let r = leer(fuente).map(|g| {
        orbital_graficos::svg::svg(
            &g,
            &orbital_graficos::svg::Opciones {
                ancho,
                alto: None,
                paleta: Paleta::Papel,
            },
        )
    });
    if let Ok(mut c) = SVGS.lock() {
        c.put(clave, r.clone());
    }
    r
}

/// La vista muestreada: la pedida con un margen alrededor, con la misma resolución.
fn con_margen(v: &Vista) -> Vista {
    let (w, h) = (v.x1 - v.x0, v.y1 - v.y0);
    Vista {
        x0: v.x0 - w * MARGEN,
        x1: v.x1 + w * MARGEN,
        y0: v.y0 - h * MARGEN,
        y1: v.y1 + h * MARGEN,
        ancho: v.ancho * (1.0 + 2.0 * MARGEN),
        alto: v.alto * (1.0 + 2.0 * MARGEN),
    }
}

// --- Colores --------------------------------------------------------------------------------

fn rgba(c: crate::style::Rgb, a: f32) -> gdk::RGBA {
    gdk::RGBA::new(
        f32::from(c.0) / 255.0,
        f32::from(c.1) / 255.0,
        f32::from(c.2) / 255.0,
        a,
    )
}

fn rgba3(c: [u8; 3]) -> gdk::RGBA {
    gdk::RGBA::new(
        f32::from(c[0]) / 255.0,
        f32::from(c[1]) / 255.0,
        f32::from(c[2]) / 255.0,
        1.0,
    )
}

// --- El lienzo ------------------------------------------------------------------------------

type AlPuntero = Box<dyn Fn(Option<(f64, f64)>)>;
type AlCambiarVista = Box<dyn Fn(bool)>;

mod imp {
    use super::*;

    #[derive(Default)]
    pub struct Lienzo {
        pub grafico: RefCell<Option<Arc<Grafico>>>,
        pub fuente: RefCell<Option<Arc<str>>>,
        /// La vista que se ve (`None` hasta el primer tamaño).
        pub vista: Cell<Option<Vista>>,
        /// La vista es la inicial: sigue al tamaño del widget.
        pub original: Cell<bool>,
        /// Lo muestreado y la vista (con margen) para la que se muestreó.
        pub trazos: RefCell<Option<Muestras>>,
        pub params: RefCell<Vec<f64>>,
        /// Los parámetros siguen en sus valores iniciales (para la caché).
        pub params_iniciales: Cell<bool>,
        pub ocupado: Cell<bool>,
        pub pendiente: Cell<bool>,
        /// Sube con cada cambio de gráfico: lo muestreado para uno viejo se descarta.
        pub generacion: Cell<u64>,
        pub alto: Cell<i32>,
        pub interactivo: Cell<bool>,
        pub arrastre: Cell<Option<Vista>>,
        pub puntero: Cell<Option<(f64, f64)>>,
        pub al_puntero: RefCell<Option<AlPuntero>>,
        pub al_cambiar_vista: RefCell<Option<AlCambiarVista>>,
        /// Muestreos terminados (para las pruebas y las mediciones).
        pub muestreos: Cell<u64>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for Lienzo {
        const NAME: &'static str = "OrbitalGrafico";
        type Type = super::Lienzo;
        type ParentType = gtk::Widget;

        fn class_init(klass: &mut Self::Class) {
            klass.set_css_name("orbital-grafico");
            klass.set_accessible_role(gtk::AccessibleRole::Img);
        }
    }

    impl ObjectImpl for Lienzo {}

    impl WidgetImpl for Lienzo {
        fn measure(&self, o: gtk::Orientation, _for_size: i32) -> (i32, i32, i32, i32) {
            match o {
                gtk::Orientation::Horizontal => (120, 640, -1, -1),
                _ => {
                    let a = self.alto.get().max(80);
                    (a, a, -1, -1)
                }
            }
        }

        fn size_allocate(&self, w: i32, h: i32, baseline: i32) {
            self.parent_size_allocate(w, h, baseline);
            self.obj().al_cambiar_tamano(f64::from(w), f64::from(h));
        }

        fn snapshot(&self, s: &gtk::Snapshot) {
            self.obj().dibujar(s);
        }
    }
}

glib::wrapper! {
    /// El dibujo de un gráfico (interactivo o estático).
    pub struct Lienzo(ObjectSubclass<imp::Lienzo>) @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl Lienzo {
    pub fn new(g: Arc<Grafico>, fuente: &str, interactivo: bool) -> Self {
        let l: Self = glib::Object::new();
        let imp = l.imp();
        imp.alto
            .set(g.alto.unwrap_or(orbital_graficos::ALTO) as i32);
        imp.interactivo.set(interactivo);
        imp.original.set(true);
        imp.params_iniciales.set(true);
        imp.params.replace(g.valores());
        let desc = g.descripcion();
        imp.grafico.replace(Some(g));
        imp.fuente.replace(Some(Arc::from(fuente)));
        l.set_overflow(gtk::Overflow::Hidden);
        l.set_hexpand(true);
        if interactivo {
            l.set_focusable(true);
            l.set_can_focus(true);
            l.update_property(&[
                gtk::accessible::Property::Label(&desc),
                gtk::accessible::Property::Description(
                    "Arrastrar o flechas: mover. Rueda o más y menos: acercar y alejar. 0: volver a la vista original.",
                ),
            ]);
            l.instalar_controles();
        } else {
            l.set_can_target(false);
            l.update_property(&[gtk::accessible::Property::Label(&desc)]);
        }
        l
    }

    /// Cambia el alto (el globito y el editor usan uno más chico que el del bloque).
    pub fn fijar_alto(&self, px: i32) {
        self.imp().alto.set(px.max(80));
        self.queue_resize();
    }

    pub fn grafico(&self) -> Option<Arc<Grafico>> {
        self.imp().grafico.borrow().clone()
    }

    /// La vista actual (para las pruebas).
    pub fn vista(&self) -> Option<Vista> {
        self.imp().vista.get()
    }

    /// Cuántos muestreos terminaron (para las pruebas y las mediciones).
    pub fn muestreos(&self) -> u64 {
        self.imp().muestreos.get()
    }

    /// ¿Hay un muestreo en curso?
    pub fn ocupado(&self) -> bool {
        self.imp().ocupado.get() || self.imp().pendiente.get()
    }

    pub fn set_al_puntero(&self, f: impl Fn(Option<(f64, f64)>) + 'static) {
        self.imp().al_puntero.replace(Some(Box::new(f)));
    }

    /// Avisa si la vista dejó de ser (o volvió a ser) la inicial.
    pub fn set_al_cambiar_vista(&self, f: impl Fn(bool) + 'static) {
        self.imp().al_cambiar_vista.replace(Some(Box::new(f)));
    }

    /// Cambia el valor de un parámetro (deslizador) y vuelve a muestrear.
    pub fn fijar_parametro(&self, k: usize, v: f64) {
        let imp = self.imp();
        {
            let mut p = imp.params.borrow_mut();
            if let Some(x) = p.get_mut(k) {
                if *x == v {
                    return;
                }
                *x = v;
            }
        }
        imp.params_iniciales.set(false);
        self.pedir();
    }

    fn al_cambiar_tamano(&self, w: f64, h: f64) {
        let imp = self.imp();
        if w < 1.0 || h < 1.0 {
            return;
        }
        let Some(g) = self.grafico() else { return };
        let nueva = match imp.vista.get() {
            Some(v) if (v.ancho - w).abs() < 0.5 && (v.alto - h).abs() < 0.5 => return,
            Some(v) if !imp.original.get() => v.con_tamano(w, h),
            _ => Vista::nueva(g.x, g.y, w, h),
        };
        imp.vista.set(Some(nueva));
        self.pedir();
    }

    /// Pone una vista nueva (mover, acercar) y pide su muestreo.
    pub fn fijar_vista(&self, v: Vista) {
        let imp = self.imp();
        imp.vista.set(Some(v));
        let era = imp.original.replace(false);
        if era && let Some(f) = imp.al_cambiar_vista.borrow().as_ref() {
            f(false);
        }
        self.queue_draw();
        self.pedir();
    }

    /// Vuelve a la vista inicial.
    pub fn restablecer(&self) {
        let imp = self.imp();
        let Some(g) = self.grafico() else { return };
        let (w, h) = (f64::from(self.width()), f64::from(self.height()));
        if w < 1.0 {
            return;
        }
        imp.vista.set(Some(Vista::nueva(g.x, g.y, w, h)));
        imp.original.set(true);
        if let Some(f) = imp.al_cambiar_vista.borrow().as_ref() {
            f(true);
        }
        self.queue_draw();
        self.pedir();
    }

    /// ¿Lo muestreado cubre la vista actual con la resolución justa?
    fn cubre(&self, v: &Vista) -> bool {
        let t = self.imp().trazos.borrow();
        let Some((m, _)) = t.as_ref() else {
            return false;
        };
        let (ux, uy) = v.por_pixel();
        let (mx, my) = m.por_pixel();
        let misma_escala = (ux / mx - 1.0).abs() < 1e-6 && (uy / my - 1.0).abs() < 1e-6;
        misma_escala && m.x0 <= v.x0 && m.x1 >= v.x1 && m.y0 <= v.y0 && m.y1 >= v.y1
    }

    /// Muestrea la vista actual fuera del hilo de GTK (uno por vez; el último pedido gana).
    fn pedir(&self) {
        let imp = self.imp();
        let Some(v) = imp.vista.get() else { return };
        let Some(g) = self.grafico() else { return };
        let fuente = imp.fuente.borrow().clone().unwrap_or_else(|| Arc::from(""));
        let inicial = imp.original.get() && imp.params_iniciales.get();
        let clave = (fuente, v.ancho.round() as u32, v.alto.round() as u32);
        if inicial && let Some(m) = MUESTRAS.lock().ok().and_then(|c| c.get(&clave)) {
            imp.trazos.replace(Some(m));
            imp.muestreos.set(imp.muestreos.get() + 1);
            self.queue_draw();
            return;
        }
        // Mover dentro de lo ya muestreado no necesita nada nuevo (los parámetros, sí).
        if imp.params_iniciales.get() && !imp.pendiente.get() && self.cubre(&v) && !inicial {
            self.queue_draw();
            return;
        }
        if imp.ocupado.get() {
            imp.pendiente.set(true);
            return;
        }
        imp.ocupado.set(true);
        let params = imp.params.borrow().clone();
        let generacion = imp.generacion.get();
        let m = con_margen(&v);
        let weak = self.downgrade();
        glib::spawn_future_local(async move {
            let r = gio::spawn_blocking(move || {
                let t = muestreo::muestrear(&g, &m, &params);
                (m, Arc::new(t))
            })
            .await;
            let Some(this) = weak.upgrade() else { return };
            let imp = this.imp();
            imp.ocupado.set(false);
            if let Ok(hecho) = r
                && imp.generacion.get() == generacion
            {
                if inicial && let Ok(mut c) = MUESTRAS.lock() {
                    c.put(clave, hecho.clone());
                }
                imp.trazos.replace(Some(hecho));
                imp.muestreos.set(imp.muestreos.get() + 1);
                this.queue_draw();
            }
            if imp.pendiente.replace(false) {
                this.pedir_otra_vez();
            }
        });
    }

    /// Un pedido que llegó mientras se muestreaba: siempre se hace (puede haber cambiado un
    /// parámetro aunque la vista esté cubierta).
    fn pedir_otra_vez(&self) {
        let imp = self.imp();
        let cubierta = imp
            .vista
            .get()
            .is_some_and(|v| imp.params_iniciales.get() && self.cubre(&v));
        if cubierta {
            self.queue_draw();
            return;
        }
        // Que `pedir` no corte por «cubierta»: se invalida lo muestreado.
        if !imp.params_iniciales.get()
            && let Some((m, _)) = imp.trazos.borrow_mut().as_mut()
        {
            m.x1 = m.x0;
        }
        self.pedir();
    }

    // --- Interacción ---

    fn instalar_controles(&self) {
        let arrastre = gtk::GestureDrag::new();
        arrastre.set_button(gdk::BUTTON_PRIMARY);
        {
            let w = self.downgrade();
            arrastre.connect_drag_begin(move |_, _, _| {
                if let Some(l) = w.upgrade() {
                    l.grab_focus();
                    l.imp().arrastre.set(l.imp().vista.get());
                }
            });
        }
        {
            let w = self.downgrade();
            arrastre.connect_drag_update(move |g, dx, dy| {
                let Some(l) = w.upgrade() else { return };
                if let Some(v) = l.imp().arrastre.get() {
                    g.set_state(gtk::EventSequenceState::Claimed);
                    l.fijar_vista(v.mover(dx, dy));
                }
            });
        }
        {
            let w = self.downgrade();
            arrastre.connect_drag_end(move |_, _, _| {
                if let Some(l) = w.upgrade() {
                    l.imp().arrastre.set(None);
                }
            });
        }
        self.add_controller(arrastre);

        let rueda = gtk::EventControllerScroll::new(gtk::EventControllerScrollFlags::VERTICAL);
        {
            let w = self.downgrade();
            rueda.connect_scroll(move |_, _, dy| {
                let Some(l) = w.upgrade() else {
                    return glib::Propagation::Proceed;
                };
                let (px, py) = l
                    .imp()
                    .puntero
                    .get()
                    .unwrap_or((f64::from(l.width()) / 2.0, f64::from(l.height()) / 2.0));
                l.acercar(1.15f64.powf(dy), px, py);
                glib::Propagation::Stop
            });
        }
        self.add_controller(rueda);

        let movimiento = gtk::EventControllerMotion::new();
        {
            let w = self.downgrade();
            movimiento.connect_motion(move |_, x, y| {
                let Some(l) = w.upgrade() else { return };
                l.imp().puntero.set(Some((x, y)));
                if let (Some(v), Some(f)) =
                    (l.imp().vista.get(), l.imp().al_puntero.borrow().as_ref())
                {
                    f(Some(v.mundo(x, y)));
                }
            });
        }
        {
            let w = self.downgrade();
            movimiento.connect_leave(move |_| {
                let Some(l) = w.upgrade() else { return };
                l.imp().puntero.set(None);
                if let Some(f) = l.imp().al_puntero.borrow().as_ref() {
                    f(None);
                }
            });
        }
        self.add_controller(movimiento);

        let doble = gtk::GestureClick::new();
        {
            let w = self.downgrade();
            doble.connect_pressed(move |_, n, _, _| {
                if let Some(l) = w.upgrade() {
                    l.grab_focus();
                    if n == 2 {
                        l.restablecer();
                    }
                }
            });
        }
        self.add_controller(doble);

        let teclas = gtk::EventControllerKey::new();
        {
            let w = self.downgrade();
            teclas.connect_key_pressed(move |_, tecla, _, mods| {
                let Some(l) = w.upgrade() else {
                    return glib::Propagation::Proceed;
                };
                if mods.intersects(gdk::ModifierType::CONTROL_MASK | gdk::ModifierType::ALT_MASK) {
                    return glib::Propagation::Proceed;
                }
                if l.tecla(tecla) {
                    glib::Propagation::Stop
                } else {
                    glib::Propagation::Proceed
                }
            });
        }
        self.add_controller(teclas);
        self.connect_has_focus_notify(|l| l.queue_draw());
    }

    /// Acerca (`factor` < 1) o aleja alrededor del píxel (`px`, `py`).
    pub fn acercar(&self, factor: f64, px: f64, py: f64) {
        if let Some(v) = self.imp().vista.get() {
            let z = v.zoom(factor, px, py);
            if z != v {
                self.fijar_vista(z);
            }
        }
    }

    /// Una tecla sobre el gráfico enfocado. Devuelve si la usó.
    pub fn tecla(&self, tecla: gdk::Key) -> bool {
        let Some(v) = self.imp().vista.get() else {
            return false;
        };
        let (cx, cy) = (v.ancho / 2.0, v.alto / 2.0);
        let paso = 40.0;
        match tecla {
            gdk::Key::Left | gdk::Key::KP_Left => self.fijar_vista(v.mover(paso, 0.0)),
            gdk::Key::Right | gdk::Key::KP_Right => self.fijar_vista(v.mover(-paso, 0.0)),
            gdk::Key::Up | gdk::Key::KP_Up => self.fijar_vista(v.mover(0.0, paso)),
            gdk::Key::Down | gdk::Key::KP_Down => self.fijar_vista(v.mover(0.0, -paso)),
            gdk::Key::plus | gdk::Key::equal | gdk::Key::KP_Add => self.acercar(1.0 / 1.25, cx, cy),
            gdk::Key::minus | gdk::Key::KP_Subtract => self.acercar(1.25, cx, cy),
            gdk::Key::_0 | gdk::Key::KP_0 | gdk::Key::Home => self.restablecer(),
            _ => return false,
        }
        true
    }

    // --- Dibujo ---

    fn dibujar(&self, s: &gtk::Snapshot) {
        use crate::style;
        let imp = self.imp();
        let (w, h) = (self.width() as f32, self.height() as f32);
        if w < 1.0 || h < 1.0 {
            return;
        }
        let Some(g) = self.grafico() else { return };
        let v = imp
            .vista
            .get()
            .unwrap_or_else(|| Vista::nueva(g.x, g.y, f64::from(w), f64::from(h)));
        let rect = graphene::Rect::new(0.0, 0.0, w, h);
        let redondo = gsk::RoundedRect::from_rect(rect, 10.0);
        s.push_rounded_clip(&redondo);
        s.append_color(&rgba(style::CARD_BG, 1.0), &rect);

        let gu = guias::guias(&g, &v);
        let fino = gsk::Stroke::new(1.0);
        if !gu.grilla.is_empty() || !gu.circulos.is_empty() {
            let pb = gsk::PathBuilder::new();
            for l in &gu.grilla {
                pb.move_to(l[0] as f32, l[1] as f32);
                pb.line_to(l[2] as f32, l[3] as f32);
            }
            for c in &gu.circulos {
                pb.add_circle(&graphene::Point::new(c[0] as f32, c[1] as f32), c[2] as f32);
            }
            s.append_stroke(&pb.to_path(), &fino, &rgba(style::TEXT_FG, 0.08));
        }
        if !gu.ejes.is_empty() {
            let pb = gsk::PathBuilder::new();
            for l in &gu.ejes {
                pb.move_to(l[0] as f32, l[1] as f32);
                pb.line_to(l[2] as f32, l[3] as f32);
            }
            s.append_stroke(&pb.to_path(), &fino, &rgba(style::TEXT_FG, 0.42));
        }

        if let Some((_, trazos)) = imp.trazos.borrow().as_ref() {
            for (k, (serie, t)) in g.series.iter().zip(trazos.iter()).enumerate() {
                let color = rgba3(color_de_serie(&g, k, Paleta::Oscura));
                if serie.estilo != Estilo::Puntos && !t.lineas.is_empty() {
                    let lineas = muestreo::a_pixeles(&t.lineas, &v);
                    if !lineas.is_empty() {
                        let pb = gsk::PathBuilder::new();
                        for l in &lineas {
                            pb.move_to(l[0][0], l[0][1]);
                            for p in &l[1..] {
                                pb.line_to(p[0], p[1]);
                            }
                        }
                        let trazo = gsk::Stroke::new(serie.grosor.unwrap_or(2.0) as f32);
                        trazo.set_line_join(gsk::LineJoin::Round);
                        trazo.set_line_cap(gsk::LineCap::Round);
                        if serie.estilo == Estilo::Discontinua {
                            trazo.set_dash(&[7.0, 5.0]);
                        }
                        s.append_stroke(&pb.to_path(), &trazo, &color);
                    }
                }
                if matches!(serie.estilo, Estilo::Puntos | Estilo::LineaYPuntos)
                    && !t.puntos.is_empty()
                {
                    let pb = gsk::PathBuilder::new();
                    let mut alguno = false;
                    for p in &t.puntos {
                        let (x, y) = v.px(p[0], p[1]);
                        if (-8.0..=v.ancho + 8.0).contains(&x) && (-8.0..=v.alto + 8.0).contains(&y)
                        {
                            pb.add_circle(&graphene::Point::new(x as f32, y as f32), 3.5);
                            alguno = true;
                        }
                    }
                    if alguno {
                        s.append_fill(&pb.to_path(), gsk::FillRule::Winding, &color);
                    }
                }
            }
        }

        // Rótulos, título y leyenda.
        let mut fd = self.pango_context().font_description().unwrap_or_default();
        fd.set_size(9 * pango::SCALE);
        let tenue = rgba(style::MUTED_FG, 1.0);
        for r in &gu.rotulos {
            let layout = self.create_pango_layout(Some(&r.texto));
            layout.set_font_description(Some(&fd));
            let (lw, lh) = layout.pixel_size();
            let (lw, lh) = (f64::from(lw), f64::from(lh));
            let (x, y) = match r.ancla {
                guias::Ancla::Arriba => (r.x - lw / 2.0, r.y),
                guias::Ancla::Abajo => (r.x - lw / 2.0, r.y - lh),
                guias::Ancla::Derecha => (r.x - lw, r.y - lh / 2.0),
                guias::Ancla::Izquierda => (r.x, r.y - lh / 2.0),
            };
            s.save();
            s.translate(&graphene::Point::new(x as f32, y as f32));
            s.append_layout(&layout, &tenue);
            s.restore();
        }
        if let Some(t) = &g.titulo {
            let mut fdt = fd.clone();
            fdt.set_size(11 * pango::SCALE);
            fdt.set_weight(pango::Weight::Bold);
            let layout = self.create_pango_layout(Some(t));
            layout.set_font_description(Some(&fdt));
            s.save();
            s.translate(&graphene::Point::new(12.0, 8.0));
            s.append_layout(&layout, &rgba(style::TEXT_FG, 1.0));
            s.restore();
        }
        if g.con_leyenda() {
            let mut y = 10.0f32;
            for (k, serie) in g.series.iter().enumerate() {
                let layout = self.create_pango_layout(Some(&guias::nombre_de_serie(serie)));
                layout.set_font_description(Some(&fd));
                let (lw, lh) = layout.pixel_size();
                let x = w - 30.0 - lw as f32;
                s.save();
                s.translate(&graphene::Point::new(x, y));
                s.append_layout(&layout, &rgba(style::TEXT_FG, 0.9));
                s.restore();
                s.append_color(
                    &rgba3(color_de_serie(&g, k, Paleta::Oscura)),
                    &graphene::Rect::new(w - 24.0, y + lh as f32 / 2.0 - 1.5, 14.0, 3.0),
                );
                y += lh as f32 + 2.0;
            }
        }
        s.pop();
        // Borde y anillo de foco.
        let (ancho_borde, color_borde) = if self.has_focus() {
            (2.0, rgba(style::FOCUS_RING, 1.0))
        } else {
            (1.0, rgba(style::LINE, 1.0))
        };
        s.append_border(
            &redondo,
            &[ancho_borde; 4],
            &[color_borde, color_borde, color_borde, color_borde],
        );
    }
}

// --- El gráfico con sus controles -----------------------------------------------------------

/// Un gráfico de la Lectura: el lienzo, las coordenadas, «Volver a la vista original» y los
/// deslizadores. Estático: solo el lienzo.
#[derive(Clone)]
pub struct GraficoVista {
    raiz: gtk::Box,
    lienzo: Lienzo,
    #[cfg_attr(not(test), allow(dead_code))]
    deslizadores: Rc<Vec<gtk::Scale>>,
}

impl GraficoVista {
    pub fn new(g: Arc<Grafico>, fuente: &str, interactivo: bool) -> Self {
        let raiz = gtk::Box::new(gtk::Orientation::Vertical, 4);
        raiz.add_css_class("orbital-grafico-caja");
        let lienzo = Lienzo::new(g.clone(), fuente, interactivo);
        raiz.append(&lienzo);
        let mut deslizadores = Vec::new();
        if interactivo {
            let barra = gtk::Box::new(gtk::Orientation::Horizontal, 6);
            let coords = gtk::Label::builder()
                .label(" ")
                .xalign(0.0)
                .hexpand(true)
                .css_classes(["dim-label", "caption", "numeric"])
                .build();
            let volver = gtk::Button::builder()
                .icon_name("zoom-original-symbolic")
                .tooltip_text("Volver a la vista original (0)")
                .css_classes(["flat", "circular"])
                .sensitive(false)
                .build();
            volver.update_property(&[gtk::accessible::Property::Label(
                "Volver a la vista original",
            )]);
            barra.append(&coords);
            barra.append(&volver);
            raiz.append(&barra);
            {
                let coords = coords.clone();
                let polar = g.grilla == orbital_graficos::Grilla::Polar;
                lienzo.set_al_puntero(move |p| match p {
                    Some((x, y)) => {
                        let f = |v: f64| orbital_graficos::ejes::rotulo(v, 1e-3);
                        let mut t = format!("x = {}   y = {}", f(x), f(y));
                        if polar {
                            let mut th = y.atan2(x);
                            if th < 0.0 {
                                th += std::f64::consts::TAU;
                            }
                            t.push_str(&format!("   r = {}   θ = {}", f(x.hypot(y)), f(th)));
                        }
                        coords.set_label(&t);
                    }
                    None => coords.set_label(" "),
                });
            }
            {
                let volver_w = volver.clone();
                lienzo.set_al_cambiar_vista(move |original| volver_w.set_sensitive(!original));
            }
            {
                let l = lienzo.clone();
                volver.connect_clicked(move |_| {
                    l.restablecer();
                    l.grab_focus();
                });
            }
            for (k, p) in g.params.iter().enumerate() {
                let Some((min, max)) = p.rango else { continue };
                let paso = p.paso.unwrap_or((max - min) / 100.0);
                let fila = gtk::Box::new(gtk::Orientation::Horizontal, 8);
                let nombre = gtk::Label::builder()
                    .label(&p.nombre)
                    .css_classes(["monospace"])
                    .width_chars(3)
                    .xalign(0.0)
                    .build();
                let escala = gtk::Scale::with_range(gtk::Orientation::Horizontal, min, max, paso);
                escala.set_value(p.valor);
                escala.set_hexpand(true);
                escala.set_draw_value(false);
                let decimales = if paso >= 1.0 {
                    0
                } else {
                    (-paso.log10().floor()) as usize
                };
                escala.update_property(&[gtk::accessible::Property::Label(&format!(
                    "Parámetro {}",
                    p.nombre
                ))]);
                let valor = gtk::Label::builder()
                    .label(format!("{:.decimales$}", p.valor))
                    .css_classes(["numeric", "caption"])
                    .width_chars(6)
                    .xalign(1.0)
                    .build();
                {
                    let l = lienzo.clone();
                    let valor = valor.clone();
                    escala.connect_value_changed(move |e| {
                        let v = e.value();
                        valor.set_label(&format!("{v:.decimales$}"));
                        l.fijar_parametro(k, v);
                    });
                }
                nombre.set_mnemonic_widget(Some(&escala));
                fila.append(&nombre);
                fila.append(&escala);
                fila.append(&valor);
                raiz.append(&fila);
                deslizadores.push(escala);
            }
        }
        Self {
            raiz,
            lienzo,
            deslizadores: Rc::new(deslizadores),
        }
    }

    pub fn widget(&self) -> &gtk::Box {
        &self.raiz
    }

    pub fn lienzo(&self) -> &Lienzo {
        &self.lienzo
    }

    /// Los deslizadores de los parámetros, en orden.
    #[cfg(test)]
    pub fn deslizadores(&self) -> &[gtk::Scale] {
        &self.deslizadores
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn caches_de_lectura_y_svg() {
        let a = leer("y = x^2").unwrap();
        let b = leer("y = x^2").unwrap();
        assert!(Arc::ptr_eq(&a, &b));
        assert!(leer("y = (x").is_err());
        let s = svg("y = x^2", 500).unwrap();
        assert!(s.starts_with("<svg class=\"grafico\""));
        assert!(svg("y = (x", 500).is_err());
    }

    #[test]
    fn margen_con_la_misma_resolucion() {
        let v = Vista::nueva((-10.0, 10.0), None, 400.0, 200.0);
        let m = con_margen(&v);
        let (a, b) = v.por_pixel();
        let (c, d) = m.por_pixel();
        assert!((a - c).abs() < 1e-12 && (b - d).abs() < 1e-12);
        assert!(m.x0 < v.x0 && m.y1 > v.y1);
    }
}
