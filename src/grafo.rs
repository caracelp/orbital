//! Grafo de enlaces de la bóveda (008): canvas con nodos por nota y aristas por enlace resuelto.
//!
//! Layout en cuadrícula determinista calculado en segundo plano; zoom y paneo del lienzo; clic
//! abre la nota; arrastre de nodos (posiciones efímeras) y vecinos resaltados al pasar el mouse.

use std::cell::Cell;
use std::cell::RefCell;
use std::rc::Rc;

use gtk::prelude::*;
use gtk::subclass::prelude::*;
use gtk::{glib, gsk};

/// Corte de nodos por apertura (SC-1: la bóveda de 5000 entra completa).
pub const MAX_NODOS: usize = 5000;

/// Separación de la cuadrícula inicial (unidades del mundo).
const PASO: f64 = 220.0;
/// Radio del nodo en unidades del mundo.
const RADIO: f64 = 9.0;

/// Datos del grafo: nodos por nota y aristas resueltas, más la posiciones de la cuadrícula.
#[derive(Clone, Debug, Default)]
pub struct GrafoDatos {
    /// Rutas de los nodos, en el orden de la cuadrícula (ordenadas por ruta).
    pub rutas: Vec<String>,
    /// Índices `(origen, destino)` dentro de `rutas`.
    pub aristas: Vec<(usize, usize)>,
    /// Se recortó al máximo (aviso discreto).
    pub cortado: bool,
    /// Posiciones propias (grafo local de la barra derecha, 025); vacío = cuadrícula.
    pub fijas: Vec<(f64, f64)>,
}

impl GrafoDatos {
    /// Arma los datos desde `LinkGraph` (ya resuelto) con corte en `MAX_NODOS`.
    pub fn desde(notas: Vec<String>, enlaces: Vec<(String, String)>) -> Self {
        // Ordenar primero y recortar después: el corte es determinista (alfabético).
        let cortado = notas.len() > MAX_NODOS;
        let mut rutas: Vec<String> = notas;
        rutas.sort();
        rutas.truncate(MAX_NODOS);
        let indice: std::collections::HashMap<&str, usize> = rutas
            .iter()
            .enumerate()
            .map(|(n, r)| (r.as_str(), n))
            .collect();
        let mut aristas: Vec<(usize, usize)> = enlaces
            .into_iter()
            .filter_map(|(a, b)| {
                let ia = indice.get(a.as_str()).copied()?;
                let ib = indice.get(b.as_str()).copied()?;
                (ia != ib).then_some((ia.min(ib), ia.max(ib)))
            })
            .collect();
        aristas.sort();
        aristas.dedup();
        Self {
            rutas,
            aristas,
            cortado,
            fijas: Vec::new(),
        }
    }

    /// Grafo local (025): `centro` en el origen y sus vecinos en anillos alrededor (16 por
    /// anillo), con aristas del centro a cada vecino y entre vecinos si se enlazan.
    pub fn local(centro: String, mut vecinos: Vec<String>, enlaces: &[(String, String)]) -> Self {
        vecinos.sort();
        vecinos.dedup();
        vecinos.retain(|v| *v != centro);
        let mut rutas = Vec::with_capacity(vecinos.len() + 1);
        rutas.push(centro);
        rutas.extend(vecinos);
        let indice: std::collections::HashMap<&str, usize> = rutas
            .iter()
            .enumerate()
            .map(|(n, r)| (r.as_str(), n))
            .collect();
        let mut aristas: Vec<(usize, usize)> = enlaces
            .iter()
            .filter_map(|(a, b)| {
                let ia = *indice.get(a.as_str())?;
                let ib = *indice.get(b.as_str())?;
                (ia != ib).then_some((ia.min(ib), ia.max(ib)))
            })
            .collect();
        aristas.sort();
        aristas.dedup();
        let n = rutas.len() - 1;
        let mut fijas = vec![(0.0, 0.0)];
        for k in 0..n {
            let anillo = k / 16;
            let en_anillo = (n - anillo * 16).min(16);
            let angulo = (k % 16) as f64 / en_anillo as f64 * std::f64::consts::TAU
                - std::f64::consts::FRAC_PI_2;
            let radio = 90.0 + anillo as f64 * 60.0;
            fijas.push((radio * angulo.cos(), radio * angulo.sin()));
        }
        Self {
            rutas,
            aristas,
            cortado: false,
            fijas,
        }
    }

    /// Posición inicial en la cuadrícula (columnas de ~70 nodos por fila).
    pub fn posicion(&self, n: usize) -> (f64, f64) {
        if let Some(p) = self.fijas.get(n) {
            return *p;
        }
        let por_fila = 70usize;
        let x = (n % por_fila) as f64 * PASO;
        let y = (n / por_fila) as f64 * PASO * 0.8;
        (x, y)
    }

    pub fn extension_mundo(&self) -> (f64, f64) {
        self.posicion(self.rutas.len().saturating_sub(1))
    }
}

/// Rectángulo en coords del widget (para detectar el nodo bajo el mouse).
#[derive(Clone, Copy, Default)]
pub(crate) struct Vista {
    /// Esquina superior izquierda del mundo visible (unidades del mundo).
    pan_x: f64,
    pan_y: f64,
    /// Escala: 1.0 = 1 unidad del mundo = 1 px.
    zoom: f64,
}

/// Estado del arrastre en curso: fondo (paneo) o nodo (reposicionar).
#[derive(Default)]
pub(crate) enum Arrastre {
    #[default]
    Ninguno,
    Fondo(f64, f64, f64, f64),
    Nodo(usize, f64, f64),
}

/// Callback al hacer clic en un nodo del grafo (recibe la ruta de la nota).
type CallbackAbrir = Box<dyn Fn(&str)>;

mod imp {
    use super::*;

    #[derive(Default)]
    pub struct VistaGrafo {
        pub datos: RefCell<Option<Rc<GrafoDatos>>>,
        /// Posiciones efímeras por índice (el arrastre las mueve; cerrar las olvida).
        pub posiciones: RefCell<Vec<(f64, f64)>>,
        pub(crate) vista: Cell<Vista>,
        pub hover: Cell<Option<usize>>,
        pub seleccion: Cell<Option<usize>>,
        /// Callback al hacer clic en un nodo (abre la nota).
        pub on_abrir: RefCell<Option<CallbackAbrir>>,
        pub on_cerrar: RefCell<Option<Box<dyn Fn()>>>,
        pub(crate) arrastre: RefCell<Arrastre>,
        /// Grafo local: el origen del mundo va al centro del lienzo en el próximo cuadro.
        pub centrar: Cell<bool>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for VistaGrafo {
        const NAME: &'static str = "OrbitalVistaGrafo";
        type Type = super::VistaGrafo;
        type ParentType = gtk::Widget;
    }

    impl ObjectImpl for VistaGrafo {}

    impl WidgetImpl for VistaGrafo {
        fn snapshot(&self, snapshot: &gtk::Snapshot) {
            let Some(datos) = self.datos.borrow().clone() else {
                return;
            };
            let width = self.obj().width() as f64;
            let height = self.obj().height() as f64;
            if self.centrar.get() && width > 0.0 {
                self.centrar.set(false);
                let mut v = self.vista.get();
                v.pan_x = -width / 2.0;
                v.pan_y = -height / 2.0;
                self.vista.set(v);
            }
            let v = self.vista.get();
            let rosa = gtk::gdk::RGBA::parse("#ff8cc6").expect("rosa de paleta");
            let linea = gtk::gdk::RGBA::parse("#685061").expect("línea de la maqueta");
            let relleno = gtk::gdk::RGBA::parse("#392532").expect("relleno de la maqueta");
            let texto = gtk::gdk::RGBA::parse("#eee9ed").expect("texto de la maqueta");
            let hovered = self.hover.get();
            let vecinos: Vec<usize> = hovered
                .map(|n| {
                    let mut out: Vec<usize> = datos
                        .aristas
                        .iter()
                        .filter(|(a, b)| *a == n || *b == n)
                        .flat_map(|(a, b)| [*a, *b])
                        .filter(|m| *m != n)
                        .collect();
                    out.sort();
                    out.dedup();
                    out
                })
                .unwrap_or_default();
            let pos = |n: usize| {
                let (base_x, base_y) = datos.posicion(n);
                let mut posiciones = self.posiciones.borrow_mut();
                if posiciones.len() <= n {
                    posiciones.resize(datos.rutas.len(), (0.0, 0.0));
                }
                let (dx, dy) = posiciones[n];
                (base_x + dx, base_y + dy)
            };
            let a_pantalla = |wx: f64, wy: f64| (wx * v.zoom - v.pan_x, wy * v.zoom - v.pan_y);
            // Aristas: un solo trazo por color (barato con miles de aristas).
            let aristas = gsk::PathBuilder::new();
            let aristas_rosa = gsk::PathBuilder::new();
            let (mut hay, mut hay_rosa) = (false, false);
            for (a, b) in &datos.aristas {
                let resaltar = hovered.is_some_and(|h| *a == h || *b == h);
                if hovered.is_some() && !resaltar {
                    continue; // con hover, el resto se atenúa (no se dibuja)
                }
                let (x0, y0) = {
                    let (x, y) = pos(*a);
                    a_pantalla(x, y)
                };
                let (x1, y1) = {
                    let (x, y) = pos(*b);
                    a_pantalla(x, y)
                };
                // Fuera del lienzo por completo: se omite.
                if (x0 < 0.0 && x1 < 0.0)
                    || (y0 < 0.0 && y1 < 0.0)
                    || (x0 > width && x1 > width)
                    || (y0 > height && y1 > height)
                {
                    continue;
                }
                let destino = if resaltar { &aristas_rosa } else { &aristas };
                destino.move_to(x0 as f32, y0 as f32);
                destino.line_to(x1 as f32, y1 as f32);
                if resaltar {
                    hay_rosa = true
                } else {
                    hay = true
                }
            }
            let trazo = gsk::Stroke::new(1.5);
            if hay {
                snapshot.append_stroke(&aristas.to_path(), &trazo, &linea);
            }
            if hay_rosa {
                snapshot.append_stroke(&aristas_rosa.to_path(), &trazo, &rosa);
            }
            // Nodos: círculos con relleno oscuro y borde rosa (como la maqueta); el hover y sus
            // vecinos, rellenos de rosa.
            let r = (RADIO * v.zoom).max(2.0);
            let nodos = gsk::PathBuilder::new();
            let nodos_rosa = gsk::PathBuilder::new();
            let mut etiquetas: Vec<(usize, f64, f64)> = Vec::new();
            for n in 0..datos.rutas.len() {
                let (wx, wy) = pos(n);
                let (x, y) = a_pantalla(wx, wy);
                if x < -r || y < -r || x > width + r || y > height + r {
                    continue;
                }
                let resaltar = hovered.is_some_and(|h| h == n || vecinos.contains(&n));
                if hovered.is_some() && !resaltar {
                    continue; // atenuar el resto
                }
                let centro = gtk::graphene::Point::new(x as f32, y as f32);
                if resaltar { &nodos_rosa } else { &nodos }.add_circle(&centro, r as f32);
                if v.zoom > 0.5 || hovered == Some(n) {
                    etiquetas.push((n, x, y));
                }
            }
            let nodos = nodos.to_path();
            snapshot.append_fill(&nodos, gsk::FillRule::Winding, &relleno);
            snapshot.append_stroke(&nodos, &trazo, &rosa);
            snapshot.append_fill(&nodos_rosa.to_path(), gsk::FillRule::Winding, &rosa);
            // Etiquetas centradas debajo de cada nodo.
            let widget = self.obj();
            for (n, x, y) in etiquetas {
                let Some(nombre) = datos.rutas.get(n) else {
                    continue;
                };
                let archivo = nombre.rsplit_once('/').map(|(_, f)| f).unwrap_or(nombre);
                let etiqueta = archivo.rsplit_once('.').map_or(archivo, |(s, _)| s);
                let (w, _, layout) = pango_label(widget.upcast_ref::<gtk::Widget>(), etiqueta);
                snapshot.save();
                snapshot.translate(&gtk::graphene::Point::new(
                    (x - w / 2.0) as f32,
                    (y + r + 4.0) as f32,
                ));
                snapshot.append_layout(&layout, &texto);
                snapshot.restore();
            }
        }
    }
}

glib::wrapper! {
    pub struct VistaGrafo(ObjectSubclass<imp::VistaGrafo>)
        @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

/// Un layout de Pango y su tamaño, para las etiquetas.
fn pango_label(w: &impl IsA<gtk::Widget>, texto: &str) -> (f64, f64, gtk::pango::Layout) {
    let layout = w.create_pango_layout(Some(texto));
    let mut fuente = gtk::pango::FontDescription::new();
    fuente.set_size(10 * gtk::pango::SCALE);
    layout.set_font_description(Some(&fuente));
    let (w, h) = layout.pixel_size();
    (w as f64, h as f64, layout)
}

impl VistaGrafo {
    pub fn new() -> Self {
        let this: Self = glib::Object::builder().build();
        this.set_can_focus(true);
        this.set_focusable(true);
        // FR-6: el grafo es un complemento visual; etiqueta accesible del lienzo.
        this.update_property(&[gtk::accessible::Property::Label(
            "Grafo de enlaces de la bóveda: nodos por nota, aristas por enlace",
        )]);
        this.imp().vista.set(Vista {
            pan_x: 0.0,
            pan_y: 0.0,
            zoom: 1.0,
        });
        this.setup_gestos();
        this
    }

    pub fn set_datos(&self, datos: Rc<GrafoDatos>) {
        self.imp().centrar.set(!datos.fijas.is_empty());
        *self.imp().datos.borrow_mut() = Some(datos);
        self.imp().posiciones.borrow_mut().clear();
        self.queue_draw();
    }

    pub fn limpiar(&self) {
        *self.imp().datos.borrow_mut() = None;
        self.imp().posiciones.borrow_mut().clear();
        self.imp().hover.set(None);
        self.queue_draw();
    }

    pub fn set_on_abrir(&self, f: impl Fn(&str) + 'static) {
        *self.imp().on_abrir.borrow_mut() = Some(Box::new(f));
    }

    pub fn set_on_cerrar(&self, f: impl Fn() + 'static) {
        *self.imp().on_cerrar.borrow_mut() = Some(Box::new(f));
    }

    /// Índice del nodo bajo un punto del widget, si cae en el radio.
    fn nodo_en(&self, px: f64, py: f64) -> Option<usize> {
        let datos = self.imp().datos.borrow().clone()?;
        let v = self.imp().vista.get();
        let mut mejor: Option<(usize, f64)> = None;
        for n in 0..datos.rutas.len() {
            let (wx, wy) = self.pos_de(n);
            let (x, y) = (wx * v.zoom - v.pan_x, wy * v.zoom - v.pan_y);
            let dist = ((px - x).powi(2) + (py - y).powi(2)).sqrt();
            let radio = (RADIO * v.zoom).max(2.0) + 4.0;
            if dist <= radio && mejor.is_none_or(|(_, m)| dist < m) {
                mejor = Some((n, dist));
            }
        }
        mejor.map(|(n, _)| n)
    }

    fn pos_de(&self, n: usize) -> (f64, f64) {
        let Some(datos) = self.imp().datos.borrow().clone() else {
            return (0.0, 0.0);
        };
        let mut posiciones = self.imp().posiciones.borrow_mut();
        if posiciones.len() <= n {
            posiciones.resize(datos.rutas.len(), (0.0, 0.0));
        }
        let (dx, dy) = posiciones[n];
        let (bx, by) = datos.posicion(n);
        (bx + dx, by + dy)
    }

    fn setup_gestos(&self) {
        // Rueda: zoom centrado en el mouse.
        let scroll = gtk::EventControllerScroll::new(gtk::EventControllerScrollFlags::VERTICAL);
        let this = self.clone();
        let ctl_scroll = scroll.clone();
        scroll.connect_scroll(move |_, _dx, dy| {
            let g = &ctl_scroll;
            let imp = this.imp();
            let mut v = imp.vista.get();
            let factor = if dy > 0.0 { 0.9 } else { 1.1 };
            let nuevo = (v.zoom * factor).clamp(0.05, 8.0);
            // Centrar en el mouse: el punto del mundo bajo el mouse queda bajo el mouse.
            if let Some(e) = g.current_event()
                && let Some(pos) = e.position()
            {
                let (mx, my) = pos;
                let wx = (mx + v.pan_x) / v.zoom;
                let wy = (my + v.pan_y) / v.zoom;
                // pantalla = mundo × zoom − pan, así que pan = mundo × zoom − pantalla.
                v.pan_x = wx * nuevo - mx;
                v.pan_y = wy * nuevo - my;
            }
            v.zoom = nuevo;
            imp.vista.set(v);
            this.queue_draw();
            gtk::glib::Propagation::Stop
        });
        self.add_controller(scroll);

        // Arrastre: fondo = paneo; nodo = reposicionar (efímero).
        let drag = gtk::GestureDrag::new();
        drag.set_button(1);
        let this = self.clone();
        drag.connect_drag_begin(move |g, x, y| {
            let imp = this.imp();
            if let Some(n) = this.nodo_en(x, y) {
                let (wx, wy) = this.pos_de(n);
                imp.arrastre.replace(Arrastre::Nodo(n, wx, wy));
            } else {
                let v = imp.vista.get();
                imp.arrastre
                    .replace(Arrastre::Fondo(x, y, v.pan_x, v.pan_y));
            }
            let _ = g;
        });
        let this = self.clone();
        drag.connect_drag_update(move |g, dx, dy| {
            let imp = this.imp();
            let (x0, y0) = g.start_point().unwrap_or((0.0, 0.0));
            match &*imp.arrastre.borrow() {
                Arrastre::Fondo(sx, sy, px, py) => {
                    let mut v = imp.vista.get();
                    v.pan_x = px - ((x0 - sx) + dx);
                    v.pan_y = py - ((y0 - sy) + dy);
                    imp.vista.set(v);
                    this.queue_draw();
                }
                Arrastre::Nodo(n, wx, wy) => {
                    let v = imp.vista.get();
                    let (px, py) = ((x0 + dx), (y0 + dy));
                    let nuevo_x = (px + v.pan_x) / v.zoom;
                    let nuevo_y = (py + v.pan_y) / v.zoom;
                    let mut posiciones = imp.posiciones.borrow_mut();
                    if posiciones.len() <= *n {
                        posiciones.resize(
                            imp.datos
                                .borrow()
                                .as_ref()
                                .map(|d| d.rutas.len())
                                .unwrap_or(0),
                            (0.0, 0.0),
                        );
                    }
                    posiciones[*n] = (nuevo_x - wx, nuevo_y - wy);
                    drop(posiciones);
                    this.queue_draw();
                }
                Arrastre::Ninguno => {}
            }
            let _ = this;
        });
        let this = self.clone();
        drag.connect_drag_end(move |g, dx, dy| {
            let (sx, sy) = g.start_point().unwrap_or((0.0, 0.0));
            let quieto = (dx.abs() + dy.abs()) < 5.0;
            if quieto
                && let Some(n) = this.nodo_en(sx, sy)
                && let Some(ruta) = this
                    .imp()
                    .datos
                    .borrow()
                    .as_ref()
                    .and_then(|d| d.rutas.get(n).cloned())
                && let Some(f) = this.imp().on_abrir.borrow().as_ref()
            {
                f(&ruta);
            }
            this.imp().arrastre.replace(Arrastre::Ninguno);
        });
        self.add_controller(drag);

        // Movimiento: hover de vecinos.
        let motion = gtk::EventControllerMotion::new();
        let this = self.clone();
        motion.connect_motion(move |_m, x, y| {
            let n = this.nodo_en(x, y);
            if n != this.imp().hover.get() {
                this.imp().hover.set(n);
                this.queue_draw();
            }
        });
        let this = self.clone();
        motion.connect_leave(move |_| {
            this.imp().hover.set(None);
            this.queue_draw();
        });
        self.add_controller(motion);

        // Clic simple sobre un nodo: abrir la nota (sin arrastre) — va en drag_end.

        // Esc: cerrar el grafo.
        let teclas = gtk::EventControllerKey::new();
        teclas.set_propagation_phase(gtk::PropagationPhase::Capture);
        let this = self.clone();
        teclas.connect_key_pressed(move |_, keyval, _, _| {
            if keyval == gtk::gdk::Key::Escape {
                if let Some(f) = this.imp().on_cerrar.borrow().as_ref() {
                    f();
                }
                return glib::Propagation::Stop;
            }
            glib::Propagation::Proceed
        });
        self.add_controller(teclas);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// T002: orden, deduplicación de aristas y recorte en 5000.
    #[test]
    fn us8_datos_orden_dedup_y_corte() {
        let mut notas: Vec<String> = (0..(MAX_NODOS + 10))
            .map(|n| format!("wiki/n{n}.md"))
            .collect();
        notas.push("wiki/a.md".into());
        notas.push("wiki/b.md".into());
        let enlaces = vec![
            ("wiki/a.md".to_owned(), "wiki/b.md".to_owned()),
            ("wiki/b.md".to_owned(), "wiki/a.md".to_owned()), // duplicada al dedup
            ("wiki/a.md".to_owned(), "wiki/ausente.md".to_owned()), // destino fuera del corte
        ];
        let datos = GrafoDatos::desde(notas, enlaces);
        assert!(datos.cortado);
        assert_eq!(datos.rutas.len(), MAX_NODOS);
        assert!(
            datos.rutas[0] == "wiki/a.md" && datos.rutas[1] == "wiki/b.md",
            "{:?}",
            &datos.rutas[..2]
        );
        assert_eq!(datos.aristas, [(0, 1)]);
        // Cuadrícula determinista.
        assert_eq!(datos.posicion(0), (0.0, 0.0));
        assert_eq!(datos.posicion(70), (0.0, PASO * 0.8));
    }

    /// T002: sin corte cuando la bóveda es chica.
    #[test]
    fn us8_datos_sin_corte() {
        let datos = GrafoDatos::desde(vec!["wiki/a.md".into(), "wiki/b.md".into()], vec![]);
        assert!(!datos.cortado);
        assert_eq!(datos.rutas.len(), 2);
    }

    /// T003/T004: el nodo bajo el mouse se encuentra por coordenadas del widget.
    #[test]
    #[ignore = "requiere acceso GTK; tools/verificar-008-us1.sh"]
    fn us8_nodo_en_por_coordenadas() {
        gtk::init().expect("se necesita acceso GTK");
        let vista = VistaGrafo::new();
        let datos = Rc::new(GrafoDatos::desde(
            vec!["wiki/a.md".into(), "wiki/b.md".into()],
            vec![("wiki/a.md".into(), "wiki/b.md".into())],
        ));
        vista.set_datos(datos);
        // Nodo 0 en (0,0) del mundo; con pan/zoom por defecto coincide con el widget.
        assert_eq!(vista.nodo_en(0.0, 0.0), Some(0));
        // Cerca de la posición del nodo 1.
        let (x, y) = {
            let d = vista.imp().datos.borrow().clone().expect("datos");
            d.posicion(1)
        };
        assert_eq!(vista.nodo_en(x, y), Some(1));
        // Lejos de ambos: nada.
        assert_eq!(vista.nodo_en(5000.0, 5000.0), None);
    }

    /// SC-1/SC-2 de la 008: tiempo de armar el cuadro (snapshot) con 5000 nodos y ~15000 aristas
    /// a tres niveles de zoom, en una ventana de 1280×800. Se corre en release:
    /// `cargo test --release --bin orbital grafo::tests::sc_cuadro_5000 -- --ignored --nocapture`.
    #[test]
    #[ignore = "medición; requiere acceso GTK"]
    fn sc_cuadro_5000() {
        gtk::init().expect("se necesita acceso GTK");
        let notas: Vec<String> = (0..MAX_NODOS).map(|n| format!("wiki/n{n:05}.md")).collect();
        let enlaces: Vec<(String, String)> = (0..MAX_NODOS)
            .flat_map(|n| {
                [n * 7 + 13, n * 31 + 5, n + 1].map(|m| {
                    (
                        format!("wiki/n{n:05}.md"),
                        format!("wiki/n{:05}.md", m % MAX_NODOS),
                    )
                })
            })
            .collect();
        let datos = Rc::new(GrafoDatos::desde(notas, enlaces));
        let aristas = datos.aristas.len();
        let vista = VistaGrafo::new();
        vista.set_datos(datos);
        let ventana = gtk::Window::builder()
            .default_width(1280)
            .default_height(800)
            .child(&vista)
            .build();
        ventana.present();
        let ctx = glib::MainContext::default();
        for _ in 0..30 {
            ctx.iteration(false);
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert!(
            vista.width() > 1000 && vista.height() > 600,
            "el lienzo tiene que estar asignado: {}×{}",
            vista.width(),
            vista.height()
        );
        for (nombre, zoom, pan) in [
            ("todo el grafo", 0.25, 0.0),
            ("normal", 1.0, 900.0),
            ("con etiquetas", 2.5, 3000.0),
        ] {
            vista.imp().vista.set(Vista {
                pan_x: pan,
                pan_y: pan,
                zoom,
            });
            let mut ms: Vec<f64> = (0..60)
                .map(|_| {
                    // Directo al `snapshot` del lienzo: `WidgetPaintable` reutiliza el cuadro
                    // guardado si nada lo invalida, y mediría la caché.
                    let snap = gtk::Snapshot::new();
                    let t = std::time::Instant::now();
                    WidgetImpl::snapshot(vista.imp(), &snap);
                    let nodo = snap.to_node();
                    let ms = t.elapsed().as_secs_f64() * 1000.0;
                    assert!(nodo.is_some(), "el cuadro dibuja algo");
                    ms
                })
                .collect();
            ms.sort_by(f64::total_cmp);
            println!(
                "GRAFO {nombre} (zoom {zoom}, {aristas} aristas): mediana {:.2} ms, p95 {:.2} ms, máx {:.2} ms",
                ms[30], ms[57], ms[59]
            );
        }
        // Con ORBITAL_CAPTURA_GRAFO=<png>, guarda el cuadro con zoom normal para revisarlo.
        if let Some(png) = std::env::var_os("ORBITAL_CAPTURA_GRAFO") {
            vista.imp().vista.set(Vista {
                pan_x: 0.0,
                pan_y: 0.0,
                zoom: 1.0,
            });
            let snap = gtk::Snapshot::new();
            WidgetImpl::snapshot(vista.imp(), &snap);
            let node = snap.to_node().expect("cuadro");
            let renderer = ventana.renderer().expect("renderizador");
            renderer
                .render_texture(
                    &node,
                    Some(&gtk::graphene::Rect::new(0.0, 0.0, 1280.0, 800.0)),
                )
                .save_to_png(png)
                .unwrap();
        }
        // Hover: buscar el nodo bajo el mouse recorre todos los nodos.
        let t = std::time::Instant::now();
        for k in 0..1000 {
            let _ = vista.nodo_en(f64::from(k % 1280), f64::from(k % 800));
        }
        println!(
            "GRAFO nodo_en: {:.1} µs por evento",
            t.elapsed().as_secs_f64() * 1e6 / 1000.0
        );
        ventana.destroy();
    }
}
