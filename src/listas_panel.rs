//! Favoritos y Guardados en la barra izquierda (027): la sección «Favoritos» del explorador y la
//! página «guardados» (una sección plegable por colección).
//!
//! Solo widgets: el modelo vive en la ventana (`src/window/listas.rs`), que llama a
//! [`ListasPanel::mostrar_favoritos`] y [`ListasPanel::mostrar_guardados`] y recibe los clics por
//! [`ListasPanel::connect_abrir`]. La página Guardados se arma recién la primera vez que se muestra.

use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::rc::Rc;

use adw::prelude::*;
use orbital_vault::{Coleccion, Lista, RelPath};

/// Qué hacer al activar una fila: la ruta, de qué lista y si falta el archivo.
type Abrir = Rc<dyn Fn(&RelPath, Lista, bool)>;

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

fn fila(ruta: &RelPath, falta: bool) -> gtk::ListBoxRow {
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
    row
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
        }));
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
        for (ruta, falta) in &filas {
            i.favoritos.append(&fila(ruta, *falta));
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
                for (ruta, falta) in filas.iter() {
                    list.append(&fila(ruta, *falta));
                }
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
            let (seccion, _) = (i.plegable)(&c.nombre, contenido.upcast_ref(), true, false);
            i.guardados_lista.append(&seccion);
        }
    }
}
