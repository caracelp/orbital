//! Grafo de enlaces de la bóveda (008): canvas con nodos por nota y aristas por enlace resuelto.
//!
//! Layout en cuadrícula determinista calculado en segundo plano; zoom y paneo del lienzo; clic
//! abre la nota; arrastre de nodos (posiciones efímeras) y vecinos resaltados al pasar el mouse.

use std::cell::Cell;
use std::cell::RefCell;
use std::rc::Rc;

use gtk::glib;
use gtk::prelude::*;
use gtk::subclass::prelude::*;

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
        }
    }

    /// Posición inicial en la cuadrícula (columnas de ~70 nodos por fila).
    pub fn posicion(&self, n: usize) -> (f64, f64) {
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
            let v = self.vista.get();
            let width = self.obj().width() as f64;
            let height = self.obj().height() as f64;
            let rosa = gtk::gdk::RGBA::parse("#ff8cc6").expect("rosa de paleta");
            let gris = gtk::gdk::RGBA::parse("#8a8a92").expect("gris de paleta");
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
                let _ = &mut posiciones;
                (base_x + dx, base_y + dy)
            };
            let a_pantalla = |wx: f64, wy: f64| (wx * v.zoom - v.pan_x, wy * v.zoom - v.pan_y);
            // Aristas.
            for (a, b) in &datos.aristas {
                let resaltar = hovered.is_some_and(|h| *a == h || *b == h);
                if hovered.is_some() && !resaltar {
                    continue; // con hover, el resto se atenúa (no se dibuja)
                }
                let (ax, ay) = pos(*a);
                let (bx, by) = pos(*b);
                let (x0, y0) = a_pantalla(ax, ay);
                let (x1, y1) = a_pantalla(bx, by);
                if (x1 - x0).abs() > width * 2.0 || (y1 - y0).abs() > height * 2.0 {
                    continue;
                }
                snapshot.append_color(
                    &if resaltar { rosa } else { gris },
                    &gtk::graphene::Rect::new(
                        x0.min(x1) as f32,
                        y0.min(y1) as f32,
                        (x1 - x0).abs().max(1.0) as f32,
                        (y1 - y0).abs().max(1.0) as f32,
                    ),
                );
            }
            // Nodos.
            for n in 0..datos.rutas.len() {
                let (wx, wy) = pos(n);
                let (x, y) = a_pantalla(wx, wy);
                if x < -RADIO * v.zoom || y < -RADIO * v.zoom || x > width || y > height {
                    continue;
                }
                let resaltar = hovered.is_some_and(|h| h == n || vecinos.contains(&n));
                let mut color = gris;
                if hovered.is_some() && !resaltar {
                    continue; // atenuar el resto
                }
                if resaltar {
                    color = rosa;
                }
                let r = (RADIO * v.zoom).max(2.0);
                snapshot.append_color(
                    &color,
                    &gtk::graphene::Rect::new(
                        (x - r) as f32,
                        (y - r) as f32,
                        (r * 2.0) as f32,
                        (r * 2.0) as f32,
                    ),
                );
                // Etiqueta: con zoom suficiente o en el nodo/hover.
                if (v.zoom > 0.5 || hovered == Some(n))
                    && let Some(nombre) = datos.rutas.get(n)
                {
                    let etiqueta = nombre.rsplit_once('/').map(|(_, f)| f).unwrap_or(nombre);
                    let widget = self.obj();
                    let layout = pango_label(widget.upcast_ref::<gtk::Widget>(), etiqueta);
                    snapshot.append_layout(&layout.2, &color);
                }
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
                v.pan_x = mx - wx * nuevo;
                v.pan_y = my - wy * nuevo;
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
        let pintable = gtk::WidgetPaintable::new(Some(&vista));
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
                    let snap = gtk::Snapshot::new();
                    let t = std::time::Instant::now();
                    pintable.snapshot(&snap, 1280.0, 800.0);
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
