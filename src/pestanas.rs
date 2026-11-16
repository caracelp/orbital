//! Pestañas de notas del diseño de interfaz (025), con **un editor compartido** (decisión de la
//! usuaria): las páginas del `adw::TabView` son marcadores livianos y el contenido real sigue
//! siendo el centro de la ventana. Cada pestaña recuerda el estado de vista de su nota (cursor,
//! desplazamiento y vista) para reponerlo al volver.
//!
//! La clave de cada pestaña es la ruta relativa de la nota, o [`CONEXIONES`] para el grafo
//! global.

use adw::prelude::*;
use gtk::glib;
use std::cell::{Cell, RefCell};
use std::rc::Rc;

/// Clave de la pestaña del grafo global («Conexiones» en la maqueta).
pub const CONEXIONES: &str = "orbital:conexiones";
/// Clave de la pestaña de la Brújula de la nota (017).
pub const BRUJULA: &str = "orbital:brujula";
/// Clave de la pestaña del Mapa mental de la nota (039).
pub const MAPA_MENTAL: &str = "orbital:mapa-mental";

/// Estado de vista de una nota: offset del cursor, desplazamiento de Código y vista.
pub type Estado = (i32, f64, Option<String>);

type ElegirFn = Box<dyn Fn(&str, Option<Estado>)>;
type CerrarFn = Box<dyn Fn(&str, bool)>;

struct Pestana {
    pagina: adw::TabPage,
    clave: String,
    estado: Option<Estado>,
}

struct Inner {
    vista: adw::TabView,
    barra: adw::TabBar,
    pestanas: RefCell<Vec<Pestana>>,
    /// Cambios de selección hechos por código (no por la usuaria): no disparan `elegir`.
    sincronizando: Cell<bool>,
    al_elegir: RefCell<Option<ElegirFn>>,
    al_cerrar: RefCell<Option<CerrarFn>>,
}

#[derive(Clone)]
pub struct Pestanas(Rc<Inner>);

impl Pestanas {
    pub fn new() -> Self {
        let vista = adw::TabView::new();
        let barra = adw::TabBar::builder()
            .view(&vista)
            .autohide(false)
            .expand_tabs(false)
            .css_classes(["orbital-tabs"])
            .build();
        barra
            .upcast_ref::<gtk::Widget>()
            .update_property(&[gtk::accessible::Property::Label("Notas abiertas")]);
        // `adw::TabBar` tiene dos listas internas (pestañas fijadas y comunes) sin nombre
        // accesible: se nombran recorriendo sus hijos.
        let mut pila = vec![barra.clone().upcast::<gtk::Widget>()];
        let mut n = 0;
        while let Some(w) = pila.pop() {
            if w.accessible_role() == gtk::AccessibleRole::TabList {
                let nombre = if n == 0 {
                    "Notas abiertas"
                } else {
                    "Notas fijadas"
                };
                w.update_property(&[gtk::accessible::Property::Label(nombre)]);
                n += 1;
            }
            let mut hijo = w.last_child();
            while let Some(h) = hijo {
                hijo = h.prev_sibling();
                pila.push(h);
            }
        }
        let this = Self(Rc::new(Inner {
            vista,
            barra,
            pestanas: RefCell::new(Vec::new()),
            sincronizando: Cell::new(false),
            al_elegir: RefCell::new(None),
            al_cerrar: RefCell::new(None),
        }));
        let weak = Rc::downgrade(&this.0);
        this.0.vista.connect_selected_page_notify(move |v| {
            let Some(i) = weak.upgrade() else { return };
            if i.sincronizando.get() {
                return;
            }
            let Some(pagina) = v.selected_page() else {
                return;
            };
            let (clave, estado) = {
                let p = i.pestanas.borrow();
                let Some(t) = p.iter().find(|t| t.pagina == pagina) else {
                    return;
                };
                (t.clave.clone(), t.estado.clone())
            };
            if let Some(f) = i.al_elegir.borrow().as_ref() {
                f(&clave, estado);
            }
        });
        let weak = Rc::downgrade(&this.0);
        this.0.vista.connect_close_page(move |v, pagina| {
            if let Some(i) = weak.upgrade() {
                let clave = {
                    let mut p = i.pestanas.borrow_mut();
                    let pos = p.iter().position(|t| t.pagina == *pagina);
                    pos.map(|n| p.remove(n).clave)
                };
                v.close_page_finish(pagina, true);
                if let Some(clave) = clave {
                    let quedan = !i.pestanas.borrow().is_empty();
                    if let Some(f) = i.al_cerrar.borrow().as_ref() {
                        f(&clave, quedan);
                    }
                }
            } else {
                v.close_page_finish(pagina, true);
            }
            glib::Propagation::Stop
        });
        // Reordenar arrastrando lo hace `adw::TabBar`; el orden propio se toma de la vista.
        this
    }

    /// La barra de pestañas (va arriba del centro).
    pub fn barra(&self) -> &adw::TabBar {
        &self.0.barra
    }

    /// Clave de la pestaña elegida.
    #[cfg(test)]
    pub fn actual(&self) -> Option<String> {
        let pagina = self.0.vista.selected_page()?;
        self.0
            .pestanas
            .borrow()
            .iter()
            .find(|t| t.pagina == pagina)
            .map(|t| t.clave.clone())
    }

    /// Muestra la pestaña de `clave` (la crea si no existe) sin disparar `elegir`.
    pub fn abrir(&self, clave: &str, titulo: &str) {
        let i = &self.0;
        let existente = i
            .pestanas
            .borrow()
            .iter()
            .find(|t| t.clave == clave)
            .map(|t| t.pagina.clone());
        i.sincronizando.set(true);
        let pagina = match existente {
            Some(p) => p,
            None => {
                let marcador = gtk::Box::new(gtk::Orientation::Vertical, 0);
                let p = i.vista.append(&marcador);
                p.set_title(titulo);
                p.set_tooltip(if clave == CONEXIONES { titulo } else { clave });
                i.pestanas.borrow_mut().push(Pestana {
                    pagina: p.clone(),
                    clave: clave.to_owned(),
                    estado: None,
                });
                p
            }
        };
        i.vista.set_selected_page(&pagina);
        i.sincronizando.set(false);
    }

    /// Guarda el estado de vista de la nota de `clave` (antes de dejarla).
    pub fn guardar_estado(&self, clave: &str, estado: Estado) {
        if let Some(t) = self
            .0
            .pestanas
            .borrow_mut()
            .iter_mut()
            .find(|t| t.clave == clave)
        {
            t.estado = Some(estado);
        }
    }

    /// Cierra sin avisar las pestañas cuya clave cumple `quitar` (notas que ya no existen).
    pub fn quitar(&self, quitar: impl Fn(&str) -> bool) {
        let i = &self.0;
        let paginas: Vec<adw::TabPage> = {
            let mut p = i.pestanas.borrow_mut();
            let (fuera, quedan): (Vec<Pestana>, Vec<Pestana>) = p
                .drain(..)
                .partition(|t| t.clave != CONEXIONES && quitar(&t.clave));
            *p = quedan;
            fuera.into_iter().map(|t| t.pagina).collect()
        };
        i.sincronizando.set(true);
        for pagina in paginas {
            i.vista.close_page(&pagina);
        }
        i.sincronizando.set(false);
    }

    /// Cierra todas (otra bóveda).
    pub fn limpiar(&self) {
        self.quitar(|_| true);
        let conexiones = self
            .0
            .pestanas
            .borrow()
            .iter()
            .find(|t| t.clave == CONEXIONES)
            .map(|t| t.pagina.clone());
        if let Some(p) = conexiones {
            self.0.pestanas.borrow_mut().clear();
            self.0.sincronizando.set(true);
            self.0.vista.close_page(&p);
            self.0.sincronizando.set(false);
        }
    }

    /// La usuaria eligió otra pestaña: `f(clave, estado guardado)`.
    pub fn connect_elegir(&self, f: impl Fn(&str, Option<Estado>) + 'static) {
        *self.0.al_elegir.borrow_mut() = Some(Box::new(f));
    }

    /// Se cerró una pestaña: `f(clave, quedan otras)`.
    pub fn connect_cerrar(&self, f: impl Fn(&str, bool) + 'static) {
        *self.0.al_cerrar.borrow_mut() = Some(Box::new(f));
    }

    /// Claves en el orden de la barra (para las pruebas).
    #[cfg(test)]
    pub fn claves(&self) -> Vec<String> {
        let i = &self.0;
        (0..i.vista.n_pages())
            .filter_map(|n| {
                let p = i.vista.nth_page(n);
                i.pestanas
                    .borrow()
                    .iter()
                    .find(|t| t.pagina == p)
                    .map(|t| t.clave.clone())
            })
            .collect()
    }

    /// Elige una pestaña como lo haría la usuaria (para las pruebas).
    #[cfg(test)]
    pub fn elegir(&self, clave: &str) {
        let p = self
            .0
            .pestanas
            .borrow()
            .iter()
            .find(|t| t.clave == clave)
            .map(|t| t.pagina.clone());
        if let Some(p) = p {
            self.0.vista.set_selected_page(&p);
        }
    }

    /// Cierra una pestaña como lo haría la usuaria (avisa con `cerrar`).
    pub fn cerrar(&self, clave: &str) {
        let p = self
            .0
            .pestanas
            .borrow()
            .iter()
            .find(|t| t.clave == clave)
            .map(|t| t.pagina.clone());
        if let Some(p) = p {
            self.0.vista.close_page(&p);
        }
    }
}
