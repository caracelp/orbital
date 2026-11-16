//! Reparto del alto de la página «Archivos» de la barra izquierda (027): Favoritos, Explorador,
//! Etiquetas y Recientes, en ese orden, en una ventana que puede ser baja (1000×680 por defecto).
//!
//! Un `GtkBox` reparte lo que sobra según el alto natural de cada hijo, sin prioridades: con
//! Favoritos arriba, el árbol quedaba en tres filas, Favoritos cortaba una fila por la mitad y
//! Recientes desaparecía. [`Reparto`] es el administrador de disposición de esa columna y da el
//! alto por orden de importancia:
//!
//! 1. a cada sección su mínimo (los encabezados siempre se ven; el mínimo de la ventana no cambia);
//! 2. al árbol, unas [`FILAS_ARBOL`] filas (sigue siendo un `ListView` virtual con su propio
//!    desplazamiento: nunca se mide entero);
//! 3. a Favoritos, su alto natural hasta [`FILAS_FAVORITOS`] filas enteras (si no entran todas,
//!    las que entren enteras; el resto se desplaza dentro de la sección);
//! 4. a Etiquetas y Recientes, por partes iguales y hasta su objetivo, solo si alcanza para una
//!    fila (nada de tiras de pocos píxeles);
//! 5. lo que sobra, al árbol; si el árbol está plegado, a las demás secciones hasta su alto natural.
//!
//! Plegar una sección baja su alto natural al del encabezado, y lo que deja libre pasa a las demás.
//! El cálculo es aritmética pura ([`repartir`]) y se prueba sin GTK.

use std::cell::RefCell;

use gtk::subclass::prelude::*;
use gtk::{glib, graphene, gsk, prelude::*};

/// Alto de una fila de lista con el estilo `navigation-sidebar` (árbol, Favoritos, Etiquetas).
pub const ALTO_FILA: i32 = 38;
/// Filas del árbol que se conservan antes de dar lugar a lo demás.
pub const FILAS_ARBOL: i32 = 4;
/// Filas de Favoritos con alto natural; con más, la sección se desplaza.
pub const FILAS_FAVORITOS: usize = 4;
/// Alto mínimo del contenido del árbol (el que la ventana exige siempre).
pub const ARBOL_MINIMO: i32 = 110;
/// Alto del contenido de Etiquetas abierta cuando hay lugar (su lista es virtual: no tiene alto
/// natural propio).
pub const ETIQUETAS_OBJETIVO: i32 = 150;

/// Mínimo y natural medidos de una sección.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Medida {
    pub min: i32,
    pub nat: i32,
}

/// Lo que [`repartir`] necesita saber de las cuatro secciones.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Pedido {
    pub favoritos: Medida,
    pub favoritos_abierta: bool,
    /// Filas de Favoritos y alto de cada una (0 si no hay filas).
    pub favoritos_filas: usize,
    pub alto_fila: i32,
    pub arbol: Medida,
    pub arbol_abierto: bool,
    pub etiquetas: Medida,
    pub etiquetas_abierta: bool,
    pub recientes: Medida,
    pub recientes_abierta: bool,
    /// Mínimo del contenido de cada sección que puede encogerse hasta su encabezado
    /// ([`Encogible`]): por debajo de eso no se muestra, así que no vale darle menos.
    pub contenido_min: [i32; 4],
}

const F: usize = 0;
const E: usize = 1;
const T: usize = 2;
const R: usize = 3;

impl Pedido {
    fn medidas(&self) -> [Medida; 4] {
        [self.favoritos, self.arbol, self.etiquetas, self.recientes]
    }

    /// Alto de Favoritos sin filas (encabezado y márgenes de la lista) y alto de una fila.
    fn favoritos_partes(&self) -> Option<(i32, i32)> {
        (self.favoritos_abierta && self.favoritos_filas > 0 && self.alto_fila > 0).then(|| {
            let filas = self.favoritos_filas as i32 * self.alto_fila;
            (self.favoritos.nat - filas, self.alto_fila)
        })
    }

    /// Alto objetivo de cada sección (lo que pide antes de que el árbol se lleve lo que sobra).
    fn objetivos(&self) -> [i32; 4] {
        let favoritos = match self.favoritos_partes() {
            Some((base, fila)) => base + self.favoritos_filas.min(FILAS_FAVORITOS) as i32 * fila,
            None => self.favoritos.nat,
        };
        let arbol = if self.arbol_abierto {
            self.arbol.min - ARBOL_MINIMO + FILAS_ARBOL * ALTO_FILA + 12
        } else {
            self.arbol.nat
        };
        let etiquetas = if self.etiquetas_abierta {
            self.etiquetas
                .nat
                .max(self.etiquetas.min + ETIQUETAS_OBJETIVO)
        } else {
            self.etiquetas.nat
        };
        [
            favoritos.max(self.favoritos.min),
            arbol.max(self.arbol.min),
            etiquetas.max(self.etiquetas.min),
            self.recientes.nat.max(self.recientes.min),
        ]
    }

    /// Alto natural de la columna entera.
    pub fn natural(&self) -> i32 {
        self.objetivos().iter().sum()
    }

    pub fn minimo(&self) -> i32 {
        self.medidas().iter().map(|m| m.min).sum()
    }
}

/// Alto de cada sección (Favoritos, Explorador, Etiquetas, Recientes) para un alto total.
pub fn repartir(alto: i32, p: &Pedido) -> [i32; 4] {
    let medidas = p.medidas();
    let objetivo = p.objetivos();
    let mut h = medidas.map(|m| m.min);
    let mut libre = alto - p.minimo();
    if libre <= 0 {
        return h;
    }
    let crecer = |h: &mut [i32; 4], i: usize, hasta: i32, libre: &mut i32| {
        let d = (hasta - h[i]).clamp(0, *libre);
        h[i] += d;
        *libre -= d;
    };
    // 2. El árbol, unas cuatro filas.
    crecer(&mut h, E, objetivo[E], &mut libre);
    // 3. Favoritos, en filas enteras.
    match p.favoritos_partes() {
        Some((base, fila)) if h[F] + libre < objetivo[F] => {
            let filas = ((h[F] + libre - base) / fila).max(0);
            crecer(&mut h, F, base + filas * fila, &mut libre);
        }
        _ => crecer(&mut h, F, objetivo[F], &mut libre),
    }
    // 4. Etiquetas y Recientes, por partes iguales, sin tiras que no muestren nada: si no
    //    alcanza para las dos, primero Recientes y si no, Etiquetas.
    for grupo in [&[T, R][..], &[R], &[T]] {
        let pedidos: Vec<(i32, i32)> = grupo
            .iter()
            .map(|&i| {
                let falta = (objetivo[i] - h[i]).max(0);
                (falta, falta.min(p.contenido_min[i].max(ALTO_FILA)))
            })
            .collect();
        if let Some(dado) = llenar(libre, &pedidos) {
            for (&i, d) in grupo.iter().zip(dado) {
                h[i] += d;
                libre -= d;
            }
            break;
        }
    }
    // 5. Lo que sobra: al árbol, o si está plegado, a las demás hasta su natural.
    if p.arbol_abierto {
        h[E] += libre;
    } else {
        for i in [F, T, R] {
            crecer(&mut h, i, medidas[i].nat.max(objetivo[i]), &mut libre);
        }
    }
    h
}

/// Reparte `libre` por partes iguales entre secciones que piden `(falta, umbral)`, sin pasarse
/// de lo que falta a cada una. `None` si alguna recibiría menos que su umbral.
fn llenar(libre: i32, pedidos: &[(i32, i32)]) -> Option<Vec<i32>> {
    let mut dado = vec![0; pedidos.len()];
    let mut orden: Vec<usize> = (0..pedidos.len()).collect();
    orden.sort_by_key(|&k| pedidos[k].0);
    let mut resto = libre;
    for (n, &k) in orden.iter().enumerate() {
        let parte = resto / (pedidos.len() - n) as i32;
        dado[k] = pedidos[k].0.min(parte);
        resto -= dado[k];
    }
    pedidos
        .iter()
        .zip(&dado)
        .all(|(&(_, umbral), &d)| d >= umbral)
        .then_some(dado)
}

/// Lo que el reparto pregunta a Favoritos: cuántas filas tiene y el alto de una.
type FilasFavoritos = Box<dyn Fn() -> (usize, i32)>;

mod imp {
    use super::*;

    #[derive(Default)]
    pub struct Reparto {
        pub favoritos: RefCell<Option<FilasFavoritos>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for Reparto {
        const NAME: &'static str = "OrbitalRepartoIzquierda";
        type Type = super::Reparto;
        type ParentType = gtk::LayoutManager;
    }

    impl ObjectImpl for Reparto {}

    impl LayoutManagerImpl for Reparto {
        fn request_mode(&self, _widget: &gtk::Widget) -> gtk::SizeRequestMode {
            gtk::SizeRequestMode::HeightForWidth
        }

        fn measure(
            &self,
            widget: &gtk::Widget,
            orientation: gtk::Orientation,
            for_size: i32,
        ) -> (i32, i32, i32, i32) {
            if orientation == gtk::Orientation::Horizontal {
                let (mut min, mut nat) = (0, 0);
                for hijo in hijos(widget) {
                    let (m, n, _, _) = hijo.measure(orientation, -1);
                    min = min.max(m);
                    nat = nat.max(n);
                }
                return (min, nat, -1, -1);
            }
            let p = self.pedido(widget, for_size);
            (p.minimo(), p.natural(), -1, -1)
        }

        fn allocate(&self, widget: &gtk::Widget, width: i32, height: i32, _baseline: i32) {
            let p = self.pedido(widget, width);
            let altos = repartir(height, &p);
            let mut y = 0;
            for (hijo, alto) in hijos(widget).zip(altos) {
                let t = gsk::Transform::new().translate(&graphene::Point::new(0.0, y as f32));
                hijo.allocate(width, alto, -1, Some(t));
                y += alto;
            }
        }
    }

    impl Reparto {
        fn pedido(&self, widget: &gtk::Widget, ancho: i32) -> Pedido {
            let mut m = [Medida::default(); 4];
            let mut abierta = [false; 4];
            let mut contenido_min = [0; 4];
            for (i, hijo) in hijos(widget).enumerate().take(4) {
                let (min, nat, _, _) = hijo.measure(gtk::Orientation::Vertical, ancho);
                m[i] = Medida { min, nat };
                let revealer = hijo.last_child().and_downcast::<gtk::Revealer>();
                abierta[i] = revealer.as_ref().is_some_and(|r| r.reveals_child());
                if let Some(e) = revealer
                    .and_then(|r| r.child())
                    .and_downcast::<super::Encogible>()
                    && let Some(c) = e.first_child()
                {
                    contenido_min[i] = c.measure(gtk::Orientation::Vertical, ancho).0;
                }
            }
            let (filas, alto_fila) = self.favoritos.borrow().as_ref().map_or((0, 0), |f| f());
            Pedido {
                favoritos: m[F],
                favoritos_abierta: abierta[F],
                favoritos_filas: filas,
                alto_fila,
                arbol: m[E],
                arbol_abierto: abierta[E],
                etiquetas: m[T],
                etiquetas_abierta: abierta[T],
                recientes: m[R],
                recientes_abierta: abierta[R],
                contenido_min,
            }
        }
    }

    fn hijos(widget: &gtk::Widget) -> impl Iterator<Item = gtk::Widget> {
        std::iter::successors(widget.first_child(), |w| w.next_sibling())
            .filter(|w| w.should_layout())
    }
}

glib::wrapper! {
    /// Administrador de disposición de la columna del explorador (ver el módulo).
    pub struct Reparto(ObjectSubclass<imp::Reparto>) @extends gtk::LayoutManager;
}

impl Reparto {
    /// `favoritos` devuelve cuántas filas tiene Favoritos y el alto de una (0 si no hay).
    pub fn new(favoritos: impl Fn() -> (usize, i32) + 'static) -> Self {
        let this: Self = glib::Object::new();
        *this.imp().favoritos.borrow_mut() = Some(Box::new(favoritos));
        this
    }
}

mod imp_encogible {
    use super::*;

    #[derive(Default)]
    pub struct Encogible;

    #[glib::object_subclass]
    impl ObjectSubclass for Encogible {
        const NAME: &'static str = "OrbitalEncogible";
        type Type = super::Encogible;
        type ParentType = gtk::Widget;
    }

    impl ObjectImpl for Encogible {
        fn dispose(&self) {
            while let Some(hijo) = self.obj().first_child() {
                hijo.unparent();
            }
        }
    }

    impl WidgetImpl for Encogible {
        fn measure(&self, orientation: gtk::Orientation, for_size: i32) -> (i32, i32, i32, i32) {
            let Some(hijo) = self.obj().first_child() else {
                return (0, 0, -1, -1);
            };
            let (min, nat, _, _) = hijo.measure(orientation, for_size);
            match orientation {
                gtk::Orientation::Horizontal => (min, nat, -1, -1),
                _ => (0, nat, -1, -1),
            }
        }

        fn size_allocate(&self, width: i32, height: i32, baseline: i32) {
            let Some(hijo) = self.obj().first_child() else {
                return;
            };
            // Como `GtkPaned`: si no entra ni su mínimo, el contenido no se muestra.
            let entra = height >= hijo.measure(gtk::Orientation::Vertical, width).0;
            hijo.set_child_visible(entra);
            if entra {
                hijo.allocate(width, height, baseline, None);
            }
        }
    }
}

glib::wrapper! {
    /// Envoltorio de un contenido con desplazamiento propio (Etiquetas, Recientes) que puede
    /// encogerse hasta cero: su mínimo es 0 y, con menos alto que el mínimo del contenido, el
    /// contenido no se muestra. Así la sección cede hasta su encabezado sin subir el mínimo de la
    /// ventana (el de un `ScrolledWindow` es el de su barra, unos 58 px).
    pub struct Encogible(ObjectSubclass<imp_encogible::Encogible>) @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl Encogible {
    pub fn new(hijo: &impl IsA<gtk::Widget>) -> Self {
        let this: Self = glib::Object::new();
        hijo.set_parent(&this);
        this
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ENCABEZADO: i32 = 40;

    /// Medidas reales de la ventana por defecto (1000×680: 474 px para la columna): encabezado de
    /// 40, Favoritos con 10 de márgenes y el mínimo de su `ScrolledWindow`, árbol con su mínimo,
    /// Etiquetas y Recientes encogibles (su contenido pide 58) y cinco notas recientes.
    const ALTO_680: i32 = 474;
    const FAV_BASE: i32 = ENCABEZADO + 10;

    fn pedido(favoritos: usize) -> Pedido {
        Pedido {
            favoritos: Medida {
                min: ENCABEZADO + 58,
                nat: (FAV_BASE + favoritos as i32 * ALTO_FILA).max(ENCABEZADO + 58),
            },
            favoritos_abierta: true,
            favoritos_filas: favoritos,
            alto_fila: ALTO_FILA,
            arbol: Medida {
                min: ENCABEZADO + ARBOL_MINIMO,
                nat: ENCABEZADO + ARBOL_MINIMO,
            },
            arbol_abierto: true,
            etiquetas: Medida {
                min: ENCABEZADO,
                nat: ENCABEZADO,
            },
            etiquetas_abierta: true,
            recientes: Medida {
                min: ENCABEZADO,
                nat: ENCABEZADO + 180,
            },
            recientes_abierta: true,
            contenido_min: [0, 0, 58, 58],
        }
    }

    fn filas_favoritos(alto: i32) -> f64 {
        f64::from(alto - FAV_BASE) / f64::from(ALTO_FILA)
    }

    #[test]
    fn ventana_por_defecto_con_tres_y_ocho_favoritos() {
        // Con ocho, cuatro filas no entran junto con cuatro del árbol y los dos encabezados de
        // abajo: quedan tres enteras y el resto se desplaza.
        for (n, filas) in [(3, 3.0), (8, 3.0)] {
            let p = pedido(n);
            let h = repartir(ALTO_680, &p);
            assert_eq!(h, [164, 230, 40, 40], "{n} favoritos");
            assert_eq!(filas_favoritos(h[F]), filas, "{n} favoritos: {h:?}");
            assert!(h[E] - ENCABEZADO >= FILAS_ARBOL * ALTO_FILA, "{h:?}");
            assert_eq!(h[R], ENCABEZADO, "el encabezado de Recientes se ve: {h:?}");
        }
        // Un poco más alta, Favoritos llega a cuatro filas.
        let h = repartir(ALTO_680 + 40, &pedido(8));
        assert_eq!(filas_favoritos(h[F]), 4.0, "{h:?}");
    }

    #[test]
    fn favoritos_en_filas_enteras_si_falta_lugar() {
        let p = pedido(8);
        for alto in 330..900 {
            let h = repartir(alto, &p);
            let filas = filas_favoritos(h[F]);
            assert!(
                h[F] == p.favoritos.min || filas.fract() == 0.0,
                "{alto}: {filas} filas"
            );
            assert!(filas <= FILAS_FAVORITOS as f64);
        }
    }

    #[test]
    fn sin_tiras_de_pocos_pixeles() {
        let p = pedido(3);
        for alto in 300..900 {
            let h = repartir(alto, &p);
            for i in [T, R] {
                let extra = h[i] - p.medidas()[i].min;
                assert!(
                    extra == 0 || extra >= ALTO_FILA.min(p.objetivos()[i] - p.medidas()[i].min)
                );
            }
        }
    }

    #[test]
    fn ventana_alta_lleva_lo_que_sobra_al_arbol() {
        let p = pedido(3);
        let h = repartir(1200, &p);
        let o = p.objetivos();
        assert_eq!([h[F], h[T], h[R]], [o[F], o[T], o[R]]);
        assert_eq!(h.iter().sum::<i32>(), 1200);
    }

    #[test]
    fn plegar_devuelve_el_espacio() {
        let mut p = pedido(8);
        let antes = repartir(ALTO_680, &p);
        // Explorador plegado: Favoritos muestra todas sus filas.
        p.arbol_abierto = false;
        p.arbol = Medida {
            min: ENCABEZADO,
            nat: ENCABEZADO,
        };
        let h = repartir(ALTO_680, &p);
        assert_eq!(h[E], ENCABEZADO);
        assert_eq!(filas_favoritos(h[F]), 4.0, "{h:?}");
        assert!(h[T] > ENCABEZADO && h[R] > ENCABEZADO, "{h:?}");
        assert_eq!(h.iter().sum::<i32>(), ALTO_680);
        // Favoritos plegada: su lugar vuelve a mostrar Etiquetas y Recientes.
        let mut p = pedido(8);
        p.favoritos_abierta = false;
        p.favoritos = Medida {
            min: ENCABEZADO,
            nat: ENCABEZADO,
        };
        let h = repartir(ALTO_680, &p);
        assert_eq!(h[F], ENCABEZADO);
        assert!(h[E] - ENCABEZADO >= FILAS_ARBOL * ALTO_FILA, "{h:?}");
        assert!(h[T] > antes[T] && h[R] > antes[R], "{h:?}");
        assert_eq!(h.iter().sum::<i32>(), ALTO_680);
    }

    #[test]
    fn mas_chica_que_el_minimo_da_los_minimos() {
        let p = pedido(3);
        assert_eq!(repartir(100, &p), p.medidas().map(|m| m.min));
    }
}
