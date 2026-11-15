//! Barra derecha del diseño de interfaz (025) con tres modos, como la maqueta:
//! - **Propiedades**: «Metadatos de la nota» (Nombre, Formato, Ubicación, Palabras) y «Etiquetas».
//! - **Contexto**: retroenlaces con fragmento (el panel de la 003), «Enlaces entrantes» y
//!   «Etiquetas».
//! - **Grafo**: la nota abierta y sus vecinos (decisión de la usuaria: grafo local real).
//!
//! Solo muestra: los datos los arma la ventana. El grafo local se pide solo con su modo a la vista.

use gtk::prelude::*;
use orbital_vault::RelPath;
use std::cell::RefCell;
use std::rc::Rc;

use crate::editor::right_sidebar::RightSidebar;
use crate::grafo::{GrafoDatos, VistaGrafo};

/// Datos de «Metadatos de la nota».
pub struct Metadatos {
    pub nombre: String,
    pub formato: String,
    pub ubicacion: String,
    pub palabras: Option<usize>,
    pub etiquetas: Vec<String>,
}

type ModoFn = Box<dyn Fn(&str)>;
type AbrirFn = Box<dyn Fn(&str)>;

struct Inner {
    root: gtk::Box,
    stack: gtk::Stack,
    valores: [gtk::Label; 4],
    chips: [gtk::FlowBox; 2],
    entrantes_titulo: gtk::Label,
    entrantes: gtk::Box,
    grafo: VistaGrafo,
    grafo_msg: gtk::Label,
    al_cambiar_modo: RefCell<Option<ModoFn>>,
    al_abrir: RefCell<Option<AbrirFn>>,
    /// Páginas que todavía no están en el `Stack` (se agregan al elegirlas).
    pendientes: RefCell<Vec<(&'static str, gtk::Box)>>,
}

#[derive(Clone)]
pub struct PanelDerecho(Rc<Inner>);

/// Título de bloque en mayúsculas (`.right-content h3` de la maqueta).
fn titulo(texto: &str) -> gtk::Label {
    gtk::Label::builder()
        .label(texto)
        .xalign(0.0)
        .css_classes(["orbital-right-heading"])
        .build()
}

fn chips() -> gtk::FlowBox {
    let f = gtk::FlowBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .column_spacing(7)
        .row_spacing(7)
        .max_children_per_line(8)
        .homogeneous(false)
        .build();
    f.update_property(&[gtk::accessible::Property::Label("Etiquetas de la nota")]);
    f
}

fn bloque(hijos: &[&gtk::Widget]) -> gtk::Box {
    let b = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(10)
        .css_classes(["orbital-right-block"])
        .build();
    for h in hijos {
        b.append(*h);
    }
    b
}

fn desplazable(contenido: &gtk::Box) -> gtk::ScrolledWindow {
    gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vexpand(true)
        .child(contenido)
        .build()
}

impl PanelDerecho {
    pub fn new(contexto: &RightSidebar) -> Self {
        // Cabecera: los tres modos.
        let cabecera = gtk::Box::builder()
            .spacing(5)
            .css_classes(["orbital-right-head"])
            .build();
        cabecera.update_property(&[gtk::accessible::Property::Label("Modo del panel derecho")]);
        let b_prop = crate::iconos::boton_alternable("archivo", "Propiedades YAML");
        let b_ctx = crate::iconos::boton_alternable("red", "Contexto");
        let b_graf = crate::iconos::boton_alternable("estrella", "Grafo de conexiones");
        b_ctx.set_group(Some(&b_prop));
        b_graf.set_group(Some(&b_prop));
        b_prop.set_active(true);
        cabecera.append(&b_prop);
        cabecera.append(&b_ctx);
        cabecera.append(&b_graf);

        // Propiedades: metadatos y etiquetas.
        let valores: [gtk::Label; 4] = std::array::from_fn(|_| {
            gtk::Label::builder()
                .xalign(1.0)
                .hexpand(true)
                .wrap(true)
                .wrap_mode(gtk::pango::WrapMode::WordChar)
                .selectable(true)
                .css_classes(["orbital-meta-value"])
                .build()
        });
        let meta = bloque(&[titulo("METADATOS DE LA NOTA").upcast_ref()]);
        for (rotulo, valor) in ["Nombre", "Formato", "Ubicación", "Palabras"]
            .iter()
            .zip(&valores)
        {
            let fila = gtk::Box::builder()
                .spacing(12)
                .css_classes(["orbital-meta-row"])
                .build();
            fila.append(
                &gtk::Label::builder()
                    .label(*rotulo)
                    .xalign(0.0)
                    .css_classes(["orbital-meta-label"])
                    .build(),
            );
            fila.append(valor);
            meta.append(&fila);
        }
        let chips: [gtk::FlowBox; 2] = std::array::from_fn(|_| chips());
        let propiedades = gtk::Box::new(gtk::Orientation::Vertical, 0);
        propiedades.append(&meta);
        propiedades.append(&bloque(&[
            titulo("ETIQUETAS").upcast_ref(),
            chips[0].upcast_ref(),
        ]));

        // Contexto: retroenlaces (el panel existente), entrantes y etiquetas.
        let entrantes_titulo = titulo("ENLACES ENTRANTES (0)");
        let entrantes = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(8)
            .build();
        entrantes.update_property(&[gtk::accessible::Property::Label("Enlaces entrantes")]);
        let ctx = gtk::Box::new(gtk::Orientation::Vertical, 0);
        let retro = contexto.widget();
        retro.set_vexpand(false);
        ctx.append(retro);
        ctx.append(&bloque(&[
            entrantes_titulo.upcast_ref(),
            entrantes.upcast_ref(),
        ]));
        ctx.append(&bloque(&[
            titulo("ETIQUETAS").upcast_ref(),
            chips[1].upcast_ref(),
        ]));

        // Grafo local.
        let grafo = VistaGrafo::new();
        grafo.set_height_request(260);
        grafo.update_property(&[gtk::accessible::Property::Label(
            "Grafo de conexiones de la nota: la nota en el centro y sus vecinas alrededor",
        )]);
        let grafo_msg = gtk::Label::builder()
            .wrap(true)
            .xalign(0.0)
            .css_classes(["orbital-context-empty"])
            .build();
        let graf = bloque(&[
            titulo("GRAFO DE CONEXIONES").upcast_ref(),
            grafo.upcast_ref(),
            grafo_msg.upcast_ref(),
        ]);

        // No homogéneo: el alto y el ancho son los de la página a la vista (las otras no se miden
        // en cada disposición). Contexto y Grafo se agregan la primera vez que se eligen.
        let stack = gtk::Stack::builder()
            .transition_type(gtk::StackTransitionType::Crossfade)
            .hhomogeneous(false)
            .vhomogeneous(false)
            .vexpand(true)
            .build();
        stack.add_named(&desplazable(&propiedades), Some("propiedades"));
        let pendientes = RefCell::new(vec![("contexto", ctx), ("grafo", graf)]);

        let root = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .css_classes(["orbital-right-panel"])
            .build();
        root.update_property(&[gtk::accessible::Property::Label("Panel de la nota")]);
        root.append(&cabecera);
        root.append(&stack);

        let this = Self(Rc::new(Inner {
            root,
            stack,
            valores,
            chips,
            entrantes_titulo,
            entrantes,
            grafo,
            grafo_msg,
            al_cambiar_modo: RefCell::new(None),
            al_abrir: RefCell::new(None),
            pendientes,
        }));
        for (b, modo) in [
            (b_prop, "propiedades"),
            (b_ctx, "contexto"),
            (b_graf, "grafo"),
        ] {
            let weak = Rc::downgrade(&this.0);
            b.connect_toggled(move |b| {
                let Some(i) = weak.upgrade() else { return };
                if b.is_active() {
                    let pagina = {
                        let mut p = i.pendientes.borrow_mut();
                        p.iter().position(|(m, _)| *m == modo).map(|n| p.remove(n))
                    };
                    if let Some((nombre, contenido)) = pagina {
                        i.stack.add_named(&desplazable(&contenido), Some(nombre));
                    }
                    i.stack.set_visible_child_name(modo);
                    if let Some(f) = i.al_cambiar_modo.borrow().as_ref() {
                        f(modo);
                    }
                }
            });
        }
        let weak = Rc::downgrade(&this.0);
        this.0.grafo.set_on_abrir(move |ruta| {
            if let Some(i) = weak.upgrade()
                && let Some(f) = i.al_abrir.borrow().as_ref()
            {
                f(ruta);
            }
        });
        this.set_nota(None);
        this
    }

    pub fn widget(&self) -> &gtk::Box {
        &self.0.root
    }

    /// Modo a la vista: `propiedades`, `contexto` o `grafo`.
    pub fn modo(&self) -> String {
        self.0
            .stack
            .visible_child_name()
            .map(|s| s.to_string())
            .unwrap_or_default()
    }

    /// Elige un modo como lo haría la usuaria (para las pruebas y capturas).
    #[cfg(test)]
    pub fn set_modo(&self, modo: &str) {
        let indice = ["propiedades", "contexto", "grafo"]
            .iter()
            .position(|m| *m == modo);
        let mut hijo = self.0.root.first_child().and_then(|c| c.first_child());
        let mut n = 0;
        while let Some(w) = hijo {
            if Some(n) == indice
                && let Some(b) = w.downcast_ref::<gtk::ToggleButton>()
            {
                b.set_active(true);
            }
            n += 1;
            hijo = w.next_sibling();
        }
    }

    pub fn connect_modo(&self, f: impl Fn(&str) + 'static) {
        *self.0.al_cambiar_modo.borrow_mut() = Some(Box::new(f));
    }

    /// Abrir una nota desde «Enlaces entrantes» o un nodo del grafo local.
    pub fn connect_abrir(&self, f: impl Fn(&str) + 'static) {
        *self.0.al_abrir.borrow_mut() = Some(Box::new(f));
    }

    /// Metadatos y etiquetas de la nota abierta (o nada).
    pub fn set_nota(&self, nota: Option<Metadatos>) {
        let i = &self.0;
        let (vals, etiquetas): ([String; 4], Vec<String>) = match nota {
            Some(m) => (
                [
                    m.nombre,
                    m.formato,
                    m.ubicacion,
                    m.palabras.map_or_else(|| "—".to_owned(), |p| p.to_string()),
                ],
                m.etiquetas,
            ),
            None => (std::array::from_fn(|_| "—".to_owned()), Vec::new()),
        };
        for (l, v) in i.valores.iter().zip(vals) {
            l.set_label(&v);
        }
        for f in &i.chips {
            f.remove_all();
            if etiquetas.is_empty() {
                f.append(
                    &gtk::Label::builder()
                        .label("Sin etiquetas")
                        .css_classes(["orbital-context-empty"])
                        .build(),
                );
            }
            for e in &etiquetas {
                f.append(
                    &gtk::Label::builder()
                        .label(format!("#{e}"))
                        .css_classes(["orbital-tag-chip"])
                        .build(),
                );
            }
        }
    }

    /// Cambia solo «Palabras» (mientras se escribe).
    pub fn set_palabras(&self, palabras: usize) {
        self.0.valores[3].set_label(&palabras.to_string());
    }

    /// Notas distintas que enlazan a la abierta.
    pub fn set_entrantes(&self, notas: &[RelPath]) {
        let i = &self.0;
        while let Some(c) = i.entrantes.first_child() {
            i.entrantes.remove(&c);
        }
        i.entrantes_titulo
            .set_label(&format!("ENLACES ENTRANTES ({})", notas.len()));
        if notas.is_empty() {
            i.entrantes.append(
                &gtk::Label::builder()
                    .label("Sin enlaces entrantes")
                    .xalign(0.0)
                    .css_classes(["orbital-context-empty"])
                    .build(),
            );
            return;
        }
        for ruta in notas {
            let nombre = ruta.file_name();
            let b = gtk::Button::builder()
                .child(
                    &gtk::Label::builder()
                        .label(format!(
                            "↗  {}",
                            nombre.rsplit_once('.').map_or(nombre, |(s, _)| s)
                        ))
                        .xalign(0.0)
                        .ellipsize(gtk::pango::EllipsizeMode::End)
                        .build(),
                )
                .tooltip_text(ruta.as_str())
                .css_classes(["orbital-context-card"])
                .build();
            b.update_property(&[gtk::accessible::Property::Label(&format!("Abrir {nombre}"))]);
            let weak = Rc::downgrade(&self.0);
            let r = ruta.to_string();
            b.connect_clicked(move |_| {
                if let Some(i) = weak.upgrade()
                    && let Some(f) = i.al_abrir.borrow().as_ref()
                {
                    f(&r);
                }
            });
            i.entrantes.append(&b);
        }
    }

    /// Datos del grafo local (`None`: sin nota o cargando).
    pub fn set_grafo(&self, datos: Option<Rc<GrafoDatos>>, mensaje: &str) {
        let i = &self.0;
        match datos {
            Some(d) => {
                i.grafo.set_visible(true);
                i.grafo.set_datos(d);
            }
            None => {
                i.grafo.limpiar();
                i.grafo.set_visible(false);
            }
        }
        i.grafo_msg.set_label(mensaje);
        i.grafo_msg.set_visible(!mensaje.is_empty());
    }
}
