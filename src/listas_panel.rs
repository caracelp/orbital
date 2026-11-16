//! Favoritos y Guardados en la barra izquierda (027): la sección «Favoritos» del explorador y la
//! página «guardados» (una sección plegable por colección).
//!
//! Solo widgets: el modelo vive en la ventana (`src/window/listas.rs`), que llama a
//! [`ListasPanel::mostrar_favoritos`] y [`ListasPanel::mostrar_guardados`] y recibe los clics por
//! [`ListasPanel::connect_abrir`] y los pedidos de los menús y del arrastre por
//! [`ListasPanel::connect_orden`]. La página Guardados se arma recién la primera vez que se muestra.
//!
//! Cada fila tiene un menú «Opciones de <nombre>» («Subir», «Bajar», «Quitar…»; también con
//! Mayús+F10 o la tecla de menú sobre la fila) y se puede arrastrar dentro de su lista. Cada
//! colección tiene su menú «Opciones de la colección <nombre>». Los menús se arman al abrirse.

use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::rc::{Rc, Weak};

use adw::prelude::*;
use gtk::glib;
use orbital_vault::{Coleccion, Lista, RelPath};

/// Qué hacer al activar una fila: la ruta, de qué lista y si falta el archivo.
type Abrir = Rc<dyn Fn(&RelPath, Lista, bool)>;

/// Lo que piden los menús y el arrastre; los índices son los de lo que se ve.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Orden {
    /// Arrastrar, «Subir» o «Bajar».
    Mover {
        lista: Lista,
        desde: usize,
        hasta: usize,
    },
    /// «Quitar de favoritos» / «Quitar de esta colección».
    Quitar {
        lista: Lista,
        indice: usize,
    },
    RenombrarColeccion(usize),
    BorrarColeccion(usize),
}

type Ordenar = Rc<dyn Fn(Orden)>;

/// Una sección plegable con el estilo del explorador (la arma la ventana).
pub type Plegable = fn(&str, &gtk::Widget, bool, bool) -> (gtk::Box, gtk::Button);

struct Inner {
    favoritos: gtk::ListBox,
    favoritos_vacio: gtk::Label,
    favoritos_scroll: gtk::ScrolledWindow,
    favoritos_caja: gtk::Box,
    /// Lo que muestra cada fila de Favoritos: ruta y si falta.
    favoritos_filas: RefCell<Vec<(RelPath, bool)>>,
    guardados: gtk::Box,
    guardados_lista: gtk::Box,
    guardados_construida: Cell<bool>,
    plegable: Plegable,
    abrir: RefCell<Option<Abrir>>,
    orden: RefCell<Option<Ordenar>>,
}

/// Widgets de Favoritos y Guardados.
#[derive(Clone)]
pub struct ListasPanel(Rc<Inner>);

/// Nombre que se muestra de una ruta: el del archivo.
pub fn nombre_de(ruta: &RelPath) -> &str {
    ruta.file_name()
}

/// Nombre accesible de una fila (contrato de la 027).
pub fn nombre_accesible(ruta: &RelPath, falta: bool) -> String {
    if falta {
        format!("{} (falta el archivo)", nombre_de(ruta))
    } else {
        format!("Abrir {}", nombre_de(ruta))
    }
}

/// Identificador de una lista para el arrastre (solo se suelta dentro de la misma).
fn id_lista(lista: Lista) -> String {
    match lista {
        Lista::Favoritos => "favoritos".to_owned(),
        Lista::Coleccion(n) => format!("coleccion:{n}"),
    }
}

/// Un menú que se arma al abrirse: botones planos que piden una [`Orden`].
fn menu_de(
    nombre: &str,
    panel: &Weak<Inner>,
    opciones: impl Fn() -> Vec<(&'static str, bool, Orden)> + 'static,
) -> gtk::MenuButton {
    let boton = gtk::MenuButton::builder()
        .icon_name("view-more-symbolic")
        .tooltip_text(nombre)
        .css_classes(["flat"])
        .valign(gtk::Align::Center)
        .build();
    boton.update_property(&[gtk::accessible::Property::Label(nombre)]);
    let panel = panel.clone();
    boton.set_create_popup_func(move |b| {
        let popover = gtk::Popover::builder().has_arrow(true).build();
        popover.add_css_class("menu");
        let columna = gtk::Box::new(gtk::Orientation::Vertical, 0);
        columna.add_css_class("menu");
        for (texto, activa, orden) in opciones() {
            let item = gtk::Button::builder()
                .child(&gtk::Label::builder().label(texto).xalign(0.0).build())
                .css_classes(["flat"])
                .sensitive(activa)
                .build();
            item.update_property(&[gtk::accessible::Property::Label(texto)]);
            let (panel, p) = (panel.clone(), popover.clone());
            item.connect_clicked(move |_| {
                p.popdown();
                if let Some(f) = panel.upgrade().and_then(|i| i.orden.borrow().clone()) {
                    f(orden);
                }
            });
            columna.append(&item);
        }
        popover.set_child(Some(&columna));
        b.set_popover(Some(&popover));
    });
    boton
}

fn fila(
    panel: &Weak<Inner>,
    ruta: &RelPath,
    falta: bool,
    lista: Lista,
    total: usize,
) -> gtk::ListBoxRow {
    let caja = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    let nombre = gtk::Label::builder()
        .label(nombre_de(ruta))
        .xalign(0.0)
        .hexpand(true)
        .ellipsize(gtk::pango::EllipsizeMode::End)
        .build();
    if falta {
        nombre.add_css_class("dim-label");
    }
    caja.append(&nombre);
    if falta {
        let icono = gtk::Image::from_icon_name("dialog-warning-symbolic");
        icono.set_tooltip_text(Some("Falta el archivo"));
        caja.append(&icono);
    }
    let row = gtk::ListBoxRow::builder()
        .child(&caja)
        .activatable(true)
        .tooltip_text(if falta {
            format!("{} (falta el archivo)", ruta.as_str())
        } else {
            ruta.as_str().to_owned()
        })
        .build();
    row.update_property(&[gtk::accessible::Property::Label(&nombre_accesible(
        ruta, falta,
    ))]);
    // El índice se lee al abrir el menú: es el de la fila en ese momento.
    let quitar = match lista {
        Lista::Favoritos => "Quitar de favoritos",
        Lista::Coleccion(_) => "Quitar de esta colección",
    };
    let r = row.downgrade();
    let menu = menu_de(
        &format!("Opciones de {}", nombre_de(ruta)),
        panel,
        move || {
            let n = r
                .upgrade()
                .and_then(|r| usize::try_from(r.index()).ok())
                .unwrap_or(0);
            vec![
                (
                    "Subir",
                    n > 0,
                    Orden::Mover {
                        lista,
                        desde: n,
                        hasta: n.saturating_sub(1),
                    },
                ),
                (
                    "Bajar",
                    n + 1 < total,
                    Orden::Mover {
                        lista,
                        desde: n,
                        hasta: n + 1,
                    },
                ),
                (quitar, true, Orden::Quitar { lista, indice: n }),
            ]
        },
    );
    caja.append(&menu);
    // Teclado: Mayús+F10 o la tecla de menú sobre la fila abren su menú.
    let atajos = gtk::ShortcutController::new();
    let m = menu.downgrade();
    atajos.add_shortcut(gtk::Shortcut::new(
        gtk::ShortcutTrigger::parse_string("<Shift>F10|Menu"),
        Some(gtk::CallbackAction::new(move |_, _| {
            if let Some(m) = m.upgrade() {
                m.popup();
            }
            glib::Propagation::Stop
        })),
    ));
    row.add_controller(atajos);
    // Arrastrar dentro de la lista: lleva «<lista>\n<índice>».
    let origen = gtk::DragSource::builder()
        .actions(gtk::gdk::DragAction::MOVE)
        .build();
    let id = id_lista(lista);
    let r = row.downgrade();
    origen.connect_prepare(move |_, _, _| {
        let n = r.upgrade()?.index();
        Some(gtk::gdk::ContentProvider::for_value(
            &format!("{id}\n{n}").to_value(),
        ))
    });
    let r = row.downgrade();
    origen.connect_drag_begin(move |o, _| {
        if let Some(r) = r.upgrade() {
            o.set_icon(Some(&gtk::WidgetPaintable::new(Some(&r))), 0, 0);
        }
    });
    row.add_controller(origen);
    row
}

/// Soltar una fila arrastrada sobre otra de la misma lista la mueve a ese lugar.
fn arrastre_en(list: &gtk::ListBox, lista: Lista, panel: &Weak<Inner>) {
    let destino = gtk::DropTarget::new(glib::Type::STRING, gtk::gdk::DragAction::MOVE);
    let id = id_lista(lista);
    let panel = panel.clone();
    destino.connect_drop(move |t, valor, _, y| {
        let Some(list) = t.widget().and_downcast::<gtk::ListBox>() else {
            return false;
        };
        let Ok(texto) = valor.get::<String>() else {
            return false;
        };
        let Some(desde) = texto
            .strip_prefix(id.as_str())
            .and_then(|r| r.strip_prefix('\n'))
            .and_then(|n| n.parse::<usize>().ok())
        else {
            return false;
        };
        let hasta = match list.row_at_y(y as i32) {
            Some(r) => usize::try_from(r.index()).unwrap_or(desde),
            None => {
                // Debajo de la última fila: al final.
                let mut n = 0;
                while list.row_at_index(n + 1).is_some() {
                    n += 1;
                }
                n as usize
            }
        };
        if desde == hasta {
            return false;
        }
        if let Some(f) = panel.upgrade().and_then(|i| i.orden.borrow().clone()) {
            f(Orden::Mover {
                lista,
                desde,
                hasta,
            });
        }
        true
    });
    list.add_controller(destino);
}

fn lista_de_filas(nombre: &str) -> gtk::ListBox {
    let list = gtk::ListBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .css_classes(["navigation-sidebar"])
        .build();
    list.update_property(&[gtk::accessible::Property::Label(nombre)]);
    list
}

fn vaciar(caja: &impl IsA<gtk::Widget>) {
    while let Some(hijo) = caja.as_ref().first_child() {
        hijo.unparent();
    }
}

fn texto_tenue(texto: &str) -> gtk::Label {
    gtk::Label::builder()
        .label(texto)
        .wrap(true)
        .xalign(0.0)
        .css_classes(["orbital-count"])
        .build()
}

impl ListasPanel {
    pub fn new(plegable: Plegable) -> Self {
        let favoritos = lista_de_filas("Notas favoritas");
        let favoritos_vacio = texto_tenue("Agregá notas desde el menú ⋯ de la nota");
        // Hasta que terminen de cargarse las listas, la sección queda vacía y sin texto.
        favoritos_vacio.set_visible(false);
        // El desplazamiento solo envuelve las filas: vacía, la sección mide su encabezado y el
        // texto de ayuda (el mínimo de un `ScrolledWindow` es el de su barra, unos 58 px).
        let favoritos_scroll = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .propagate_natural_height(true)
            .child(&favoritos)
            .visible(false)
            .build();
        let favoritos_caja = gtk::Box::new(gtk::Orientation::Vertical, 0);
        favoritos_caja.append(&favoritos_scroll);
        favoritos_caja.append(&favoritos_vacio);
        let guardados = gtk::Box::new(gtk::Orientation::Vertical, 0);
        guardados.update_property(&[gtk::accessible::Property::Label("Guardados")]);
        let guardados_lista = gtk::Box::new(gtk::Orientation::Vertical, 0);
        let this = Self(Rc::new(Inner {
            favoritos,
            favoritos_vacio,
            favoritos_scroll,
            favoritos_caja,
            favoritos_filas: RefCell::new(Vec::new()),
            guardados,
            guardados_lista,
            guardados_construida: Cell::new(false),
            plegable,
            abrir: RefCell::new(None),
            orden: RefCell::new(None),
        }));
        arrastre_en(&this.0.favoritos, Lista::Favoritos, &Rc::downgrade(&this.0));
        let weak = Rc::downgrade(&this.0);
        this.0.favoritos.connect_row_activated(move |_, row| {
            let Some(i) = weak.upgrade() else { return };
            let Ok(n) = usize::try_from(row.index()) else {
                return;
            };
            let elegido = i.favoritos_filas.borrow().get(n).cloned();
            let abrir = i.abrir.borrow().clone();
            if let (Some((ruta, falta)), Some(abrir)) = (elegido, abrir) {
                abrir(&ruta, Lista::Favoritos, falta);
            }
        });
        this
    }

    /// Contenido de la sección «Favoritos» (la ventana lo envuelve en su sección plegable).
    pub fn favoritos(&self) -> &gtk::Box {
        &self.0.favoritos_caja
    }

    /// Cuántas filas tiene Favoritos y el alto de una (para el reparto de la barra, `reparto.rs`).
    pub fn filas_favoritos(&self) -> (usize, i32) {
        let i = &self.0;
        let filas = i.favoritos_filas.borrow().len();
        let alto = match i.favoritos.row_at_index(0) {
            Some(r) if filas > 0 => r.measure(gtk::Orientation::Vertical, -1).1,
            _ => 0,
        };
        (filas, alto)
    }

    /// La lista de filas de Favoritos.
    #[cfg(test)]
    pub fn lista_favoritos(&self) -> &gtk::ListBox {
        &self.0.favoritos
    }

    /// Página «guardados» de la barra izquierda: vacía hasta [`Self::construir_guardados`].
    pub fn guardados(&self) -> &gtk::Box {
        &self.0.guardados
    }

    pub fn connect_abrir(&self, f: impl Fn(&RelPath, Lista, bool) + 'static) {
        *self.0.abrir.borrow_mut() = Some(Rc::new(f));
    }

    pub fn connect_orden(&self, f: impl Fn(Orden) + 'static) {
        *self.0.orden.borrow_mut() = Some(Rc::new(f));
    }

    /// Redibuja Favoritos.
    pub fn mostrar_favoritos(&self, rutas: &[RelPath], faltantes: &HashSet<RelPath>) {
        let i = &self.0;
        let filas: Vec<(RelPath, bool)> = rutas
            .iter()
            .map(|r| (r.clone(), faltantes.contains(r)))
            .collect();
        if *i.favoritos_filas.borrow() == filas
            && i.favoritos_vacio.is_visible() == filas.is_empty()
        {
            return;
        }
        i.favoritos.remove_all();
        let panel = Rc::downgrade(&self.0);
        for (ruta, falta) in &filas {
            i.favoritos
                .append(&fila(&panel, ruta, *falta, Lista::Favoritos, filas.len()));
        }
        i.favoritos_scroll.set_visible(!filas.is_empty());
        i.favoritos_vacio.set_visible(filas.is_empty());
        *i.favoritos_filas.borrow_mut() = filas;
    }

    /// Arma la página Guardados (la primera vez que se muestra). Devuelve si recién se armó.
    pub fn construir_guardados(&self) -> bool {
        let i = &self.0;
        if i.guardados_construida.replace(true) {
            return false;
        }
        let scroll = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vexpand(true)
            .child(&i.guardados_lista)
            .build();
        i.guardados.append(&scroll);
        true
    }

    #[cfg(test)]
    pub fn guardados_construida(&self) -> bool {
        self.0.guardados_construida.get()
    }

    /// Redibuja Guardados, solo si la página ya se armó.
    pub fn mostrar_guardados(&self, colecciones: &[Coleccion], faltantes: &HashSet<RelPath>) {
        let i = &self.0;
        if !i.guardados_construida.get() {
            return;
        }
        vaciar(&i.guardados_lista);
        if colecciones.is_empty() {
            let texto =
                texto_tenue("Guardá notas en colecciones con «Guardar en…» del menú ⋯ de la nota");
            texto.set_margin_start(12);
            texto.set_margin_end(12);
            texto.set_margin_top(12);
            i.guardados_lista.append(&texto);
            return;
        }
        for (n, c) in colecciones.iter().enumerate() {
            let contenido = gtk::Box::new(gtk::Orientation::Vertical, 0);
            if c.notas.is_empty() {
                contenido.append(&texto_tenue("Sin notas"));
            } else {
                let list = lista_de_filas(&format!("Notas de {}", c.nombre));
                let filas: Rc<Vec<(RelPath, bool)>> = Rc::new(
                    c.notas
                        .iter()
                        .map(|r| (r.clone(), faltantes.contains(r)))
                        .collect(),
                );
                let panel = Rc::downgrade(&self.0);
                for (ruta, falta) in filas.iter() {
                    list.append(&fila(
                        &panel,
                        ruta,
                        *falta,
                        Lista::Coleccion(n),
                        filas.len(),
                    ));
                }
                arrastre_en(&list, Lista::Coleccion(n), &panel);
                let weak = Rc::downgrade(&self.0);
                list.connect_row_activated(move |_, row| {
                    let Some(i) = weak.upgrade() else { return };
                    let Ok(k) = usize::try_from(row.index()) else {
                        return;
                    };
                    let abrir = i.abrir.borrow().clone();
                    if let (Some((ruta, falta)), Some(abrir)) = (filas.get(k), abrir) {
                        abrir(ruta, Lista::Coleccion(n), *falta);
                    }
                });
                contenido.append(&list);
            }
            let (seccion, boton) = (i.plegable)(&c.nombre, contenido.upcast_ref(), true, false);
            // El menú de la colección va al lado de su encabezado.
            let menu = menu_de(
                &format!("Opciones de la colección {}", c.nombre),
                &Rc::downgrade(&self.0),
                move || {
                    vec![
                        ("Renombrar…", true, Orden::RenombrarColeccion(n)),
                        ("Borrar colección…", true, Orden::BorrarColeccion(n)),
                    ]
                },
            );
            let encabezado = gtk::Box::new(gtk::Orientation::Horizontal, 0);
            seccion.remove(&boton);
            boton.set_hexpand(true);
            encabezado.append(&boton);
            encabezado.append(&menu);
            seccion.prepend(&encabezado);
            i.guardados_lista.append(&seccion);
        }
    }
}
