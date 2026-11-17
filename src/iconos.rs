//! Íconos de trazo del diseño de interfaz (025): los mismos trazos SVG de 24×24 de la maqueta,
//! dibujados con `gsk::Path` en el color del texto del widget (como `currentColor`). Sin archivos
//! ni recursos: cada trazo se interpreta una sola vez por hilo y se reutiliza.

use gtk::{gdk, glib, graphene, gsk, prelude::*, subclass::prelude::*};
use std::cell::RefCell;
use std::collections::HashMap;

// Generado desde `orbital_estructura_base_v13.html` (trazos de 24×24 de la maqueta).
pub(crate) const TRAZOS: &[(&str, &str)] = &[
    (
        "barra-izquierda",
        "M5 4h14a2 2 0 0 1 2 2v12a2 2 0 0 1 -2 2h-14a2 2 0 0 1 -2 -2v-12a2 2 0 0 1 2 -2zM9 4v16",
    ),
    (
        "barra-derecha",
        "M5 4h14a2 2 0 0 1 2 2v12a2 2 0 0 1 -2 2h-14a2 2 0 0 1 -2 -2v-12a2 2 0 0 1 2 -2zM15 4v16",
    ),
    (
        "crear-nota",
        "M12 3H5a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h14a2 2 0 0 0 2-2v-7M16 3h5m-2.5-2.5v5M7 10h6M7 14h9",
    ),
    (
        "crear-carpeta",
        "M3 7a2 2 0 0 1 2-2h5l2 2h7a2 2 0 0 1 2 2v10H3zM12 12v6m-3-3h6",
    ),
    ("ordenar", "M4 7h14M4 12h10M4 17h6M17 14l3 3 3-3"),
    (
        "ubicar",
        "M5 12a7 7 0 1 0 14 0a7 7 0 1 0 -14 0zM12 2v5m0 10v5M2 12h5m10 0h5",
    ),
    ("colapsar", "M5 7h14M5 17h14m-10-7 3 3 3-3"),
    ("cambiar", "M4 7h15l-3-3m3 3-3 3M20 17H5l3-3m-3 3 3 3"),
    ("archivo", "M6 3h8l5 5v13H6zM14 3v5h5M9 13h7M9 17h7"),
    (
        "red",
        "M10 4a2 2 0 1 0 4 0a2 2 0 1 0 -4 0zM3 19a2 2 0 1 0 4 0a2 2 0 1 0 -4 0zM17 19a2 2 0 1 0 4 0a2 2 0 1 0 -4 0zM11 6l-5 11m7-11 5 11M7 19h10",
    ),
    (
        "estrella",
        "M12 2.5l2.7 5.5 6.1.9-4.4 4.3 1 6.1-5.4-2.9-5.4 2.9 1-6.1-4.4-4.3 6.1-.9z",
    ),
    (
        "configuracion",
        "M9 12a3 3 0 1 0 6 0a3 3 0 1 0 -6 0zM12 2v3m0 14v3M2 12h3m14 0h3M4.9 4.9 7 7m10 10 2.1 2.1m0-14.2L17 7M7 17l-2.1 2.1",
    ),
    ("abajo", "M6 9l6 6 6-6"),
    ("arriba", "M6 15l6-6 6 6"),
    (
        "planeta",
        "M3 12a9 9 0 1 0 18 0a9 9 0 1 0 -18 0zM8 12a4 4 0 1 0 8 0a4 4 0 1 0 -8 0z",
    ),
    ("derecha", "M9 6l6 6-6 6"),
    ("codigo", "M8 8l-4 4 4 4m8-8 4 4-4 4M14 4l-4 16"),
    (
        "mas",
        "M12 5a1.2 1.2 0 1 0 0.01 0M12 12a1.2 1.2 0 1 0 0.01 0M12 19a1.2 1.2 0 1 0 0.01 0",
    ),
    (
        "buscar",
        "M4 10.5a6.5 6.5 0 1 0 13 0a6.5 6.5 0 1 0 -13 0zM16 16l5 5",
    ),
    ("marcador", "M6 3h12v18l-6-4-6 4z"),
    ("selector", "M4 5h16v14H4zM8 9h8m-8 4h5M15 15l2 2 4-4"),
    (
        "canva",
        "M5 4h14a2 2 0 0 1 2 2v12a2 2 0 0 1 -2 2h-14a2 2 0 0 1 -2 -2v-12a2 2 0 0 1 2 -2zM7 8h5v5H7zM12 10h5m-7 3v4",
    ),
    (
        "db",
        "M4 5a8 3 0 1 0 16 0a8 3 0 1 0 -16 0zM4 5v14c0 3 16 3 16 0V5M4 12c0 3 16 3 16 0",
    ),
    (
        "comandos",
        "M9 7V5a3 3 0 1 0-3 3h12a3 3 0 1 0-3-3v14a3 3 0 1 0 3-3H6a3 3 0 1 0 3 3z",
    ),
    ("tipo-carpeta", "M3 6h7l2 2h9v11H3z"),
    ("tipo-md", "M6 2h9l4 4v16H6zM15 2v5h4M9 14l2 2 2-2m-2 2v-5"),
    ("tipo-pdf", "M6 2h9l4 4v16H6zM15 2v5h4M9 17h7"),
    (
        "tipo-imagen",
        "M5 4h14a2 2 0 0 1 2 2v12a2 2 0 0 1 -2 2h-14a2 2 0 0 1 -2 -2v-12a2 2 0 0 1 2 -2zM7 9a1 1 0 1 0 2 0a1 1 0 1 0 -2 0zM4 18l6-6 4 4 3-3 4 5",
    ),
    ("tipo-html", "M6 2h9l4 4v16H6zM11 11l-2 3 2 3m4-6 2 3-2 3"),
    (
        "tipo-json",
        "M6 2h9l4 4v16H6zM11 11c-2 0-2 1-2 3s0 3-2 3m8-6c2 0 2 1 2 3s0 3 2 3",
    ),
    ("tipo-codigo", "M6 2h9l4 4v16H6zM9 14l2-2 2 2 2-2"),
    ("tipo-texto", "M6 2h9l4 4v16H6zM15 2v5h4M9 12h7m-7 3h7"),
    (
        "brujula",
        "M3 12a9 9 0 1 0 18 0a9 9 0 1 0 -18 0zM16.2 7.8l-2.6 5.8-5.8 2.6 2.6-5.8z",
    ),
    (
        "etiqueta",
        "M20.59 13.41l-7.17 7.17a2 2 0 0 1-2.83 0L2 12V2h10l8.59 8.59a2 2 0 0 1 0 2.82zM7 7h.01",
    ),
    ("reloj", "M12 4a8 8 0 1 0 8 8 8 8 0 0 0-8-8zm0 4v4l2.5 2.5"),
    // Callouts de la vista formateada.
    ("callout-lapiz", "M4 20h4L19 9l-4-4L4 16zM13.5 6.5l4 4"),
    (
        "callout-info",
        "M12 3a9 9 0 1 0 0 18a9 9 0 1 0 0-18zM12 11v6M12 7.5v.5",
    ),
    (
        "callout-llama",
        "M12 3c1 3 5 5 5 10a5 5 0 0 1-10 0c0-2 1-3.5 2-4.5c0 2 1 3 2 3c0-3-1-5 1-8.5z",
    ),
    ("callout-alerta", "M12 4L2.5 20h19zM12 10v4.5M12 17.5v.5"),
    ("callout-rayo", "M13 2L4 14h7l-1 8 9-12h-7z"),
    ("callout-check", "M5 12.5l4.5 4.5L19 7"),
    (
        "callout-pregunta",
        "M12 3a9 9 0 1 0 0 18a9 9 0 1 0 0-18zM9.5 9.5a2.5 2.5 0 1 1 3.5 2.3c-.7.3-1 .9-1 1.7M12 16.5v.5",
    ),
    ("callout-cruz", "M6 6l12 12M18 6L6 18"),
    (
        "callout-lista",
        "M9 6h11M9 12h11M9 18h11M4.5 6h.5M4.5 12h.5M4.5 18h.5",
    ),
    (
        "callout-cita",
        "M5 17c2-1 3-3 3-6V7H4v4h4M15 17c2-1 3-3 3-6V7h-4v4h4",
    ),
    (
        "callout-tarea",
        "M12 3a9 9 0 1 0 0 18a9 9 0 1 0 0-18zM8 12l3 3 5-5",
    ),
    // Partes especiales de la nota en Lectura (041).
    (
        "seccion-chispa",
        "M12 3l1.9 5.1L19 10l-5.1 1.9L12 17l-1.9-5.1L5 10l5.1-1.9zM18.5 15.5l.8 2.2 2.2.8-2.2.8-.8 2.2-.8-2.2-2.2-.8 2.2-.8z",
    ),
    (
        "seccion-libro",
        "M5 4.5A1.5 1.5 0 0 1 6.5 3H19v15H6.5A1.5 1.5 0 0 0 5 19.5zM5 19.5A1.5 1.5 0 0 0 6.5 21H19M9 7h6",
    ),
    // Tipos de propiedad.
    ("prop-texto", "M4 7h16M4 12h16M4 17h10"),
    ("prop-numero", "M10 4L8 20M16 4l-2 16M5 9h15M4 15h15"),
    (
        "prop-casilla",
        "M5 4h14a1 1 0 0 1 1 1v14a1 1 0 0 1-1 1H5a1 1 0 0 1-1-1V5a1 1 0 0 1 1-1zM8 12l3 3 5-6",
    ),
    (
        "prop-fecha",
        "M5 5h14a1 1 0 0 1 1 1v13a1 1 0 0 1-1 1H5a1 1 0 0 1-1-1V6a1 1 0 0 1 1-1zM4 10h16M8 3v4M16 3v4",
    ),
    (
        "prop-enlace",
        "M10 14a4 4 0 0 0 5.7 0l3-3a4 4 0 0 0-5.7-5.7l-1 1M14 10a4 4 0 0 0-5.7 0l-3 3a4 4 0 0 0 5.7 5.7l1-1",
    ),
];

thread_local! {
    static CACHE: RefCell<HashMap<&'static str, gsk::Path>> = RefCell::new(HashMap::new());
}

/// El trazo interpretado de un ícono (o `None` si el nombre no existe).
fn trazo(nombre: &str) -> Option<gsk::Path> {
    let (clave, datos) = TRAZOS.iter().find(|(n, _)| *n == nombre)?;
    CACHE.with(|c| {
        let mut c = c.borrow_mut();
        if let Some(p) = c.get(clave) {
            return Some(p.clone());
        }
        let p = gsk::Path::parse(datos).ok()?;
        c.insert(clave, p.clone());
        Some(p)
    })
}

/// Dibuja el ícono `nombre` en un cuadrado de `lado` px con esquina en (`x`, `y`), con trazo
/// del `color` (para capas dibujadas a mano, como la vista formateada).
pub(crate) fn dibujar(
    snapshot: &gtk::Snapshot,
    nombre: &str,
    x: f32,
    y: f32,
    lado: f32,
    color: &gdk::RGBA,
) {
    let Some(path) = trazo(nombre) else { return };
    let escala = lado / 24.0;
    snapshot.save();
    snapshot.translate(&graphene::Point::new(x, y));
    snapshot.scale(escala, escala);
    let stroke = gsk::Stroke::new(if lado <= 16.0 { 1.6 } else { 1.8 });
    stroke.set_line_cap(gsk::LineCap::Round);
    stroke.set_line_join(gsk::LineJoin::Round);
    snapshot.append_stroke(&path, &stroke, color);
    snapshot.restore();
}

mod imp {
    use super::*;

    #[derive(Default)]
    pub struct Icono {
        pub nombre: RefCell<&'static str>,
        pub tamano: std::cell::Cell<i32>,
        pub grosor: std::cell::Cell<f32>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for Icono {
        const NAME: &'static str = "OrbitalIcono";
        type Type = super::Icono;
        type ParentType = gtk::Widget;

        fn class_init(klass: &mut Self::Class) {
            klass.set_css_name("orbital-icono");
            klass.set_accessible_role(gtk::AccessibleRole::Presentation);
        }
    }

    impl ObjectImpl for Icono {}

    impl WidgetImpl for Icono {
        fn measure(&self, _: gtk::Orientation, _: i32) -> (i32, i32, i32, i32) {
            let t = self.tamano.get();
            (t, t, -1, -1)
        }

        fn snapshot(&self, snapshot: &gtk::Snapshot) {
            let Some(path) = trazo(*self.nombre.borrow()) else {
                return;
            };
            let w = self.obj();
            let (ancho, alto) = (w.width() as f32, w.height() as f32);
            let lado = ancho.min(alto);
            let escala = lado / 24.0;
            snapshot.save();
            snapshot.translate(&graphene::Point::new(
                (ancho - lado) / 2.0,
                (alto - lado) / 2.0,
            ));
            snapshot.scale(escala, escala);
            let stroke = gsk::Stroke::new(self.grosor.get());
            stroke.set_line_cap(gsk::LineCap::Round);
            stroke.set_line_join(gsk::LineJoin::Round);
            let color: gdk::RGBA = w.color();
            snapshot.append_stroke(&path, &stroke, &color);
            snapshot.restore();
        }
    }
}

glib::wrapper! {
    /// Un ícono de trazo de la maqueta v13, del tamaño pedido y en el color del texto.
    pub struct Icono(ObjectSubclass<imp::Icono>) @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl Icono {
    /// `nombre` es una clave de [`TRAZOS`]; `tamano` en píxeles (la maqueta usa 18 en botones y 16
    /// en el árbol).
    pub fn new(nombre: &'static str, tamano: i32) -> Self {
        debug_assert!(TRAZOS.iter().any(|(n, _)| *n == nombre), "ícono «{nombre}»");
        let this: Self = glib::Object::new();
        let imp = this.imp();
        *imp.nombre.borrow_mut() = nombre;
        imp.tamano.set(tamano);
        imp.grosor.set(if tamano <= 16 { 1.25 } else { 1.35 });
        this.set_valign(gtk::Align::Center);
        this.set_halign(gtk::Align::Center);
        this
    }

    /// Cambia el trazo (por ejemplo, el botón de vista según el modo actual).
    pub fn set_nombre(&self, nombre: &'static str) {
        *self.imp().nombre.borrow_mut() = nombre;
        self.queue_draw();
    }
}

/// Botón plano de solo ícono, con nombre accesible y tooltip (como los `.iconbtn` de la maqueta).
pub fn boton(nombre: &'static str, etiqueta: &str) -> gtk::Button {
    let b = gtk::Button::builder()
        .child(&Icono::new(nombre, 18))
        .tooltip_text(etiqueta)
        .css_classes(["flat", "orbital-iconbtn"])
        .build();
    b.update_property(&[gtk::accessible::Property::Label(etiqueta)]);
    b
}

/// Como [`boton`], pero que queda presionado (modos y barras).
pub fn boton_alternable(nombre: &'static str, etiqueta: &str) -> gtk::ToggleButton {
    let b = gtk::ToggleButton::builder()
        .child(&Icono::new(nombre, 18))
        .tooltip_text(etiqueta)
        .css_classes(["flat", "orbital-iconbtn"])
        .build();
    b.update_property(&[gtk::accessible::Property::Label(etiqueta)]);
    b
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Todos los trazos de la maqueta se interpretan.
    #[test]
    fn trazos_validos() {
        for (nombre, datos) in TRAZOS {
            assert!(gsk::Path::parse(datos).is_ok(), "«{nombre}»: {datos}");
        }
    }
}
