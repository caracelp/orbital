//! Pie de la barra izquierda de la interfaz v13 (025): la bóveda abierta en un desplegable con las
//! bóvedas recientes (elegir otra la abre) y el botón «Cambiar de bóveda» con el menú de abrir.

use gtk::prelude::*;
use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::rc::Rc;

type ElegirFn = Box<dyn Fn(PathBuf)>;

struct Inner {
    raiz: gtk::Box,
    desplegable: gtk::DropDown,
    nombres: gtk::StringList,
    rutas: RefCell<Vec<PathBuf>>,
    /// Cambios de selección hechos por código: no abren nada.
    sincronizando: Cell<bool>,
    al_elegir: RefCell<Option<ElegirFn>>,
}

#[derive(Clone)]
pub struct PieBoveda(Rc<Inner>);

/// Nombre visible de una bóveda: el de su carpeta.
fn nombre(ruta: &Path) -> String {
    ruta.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| ruta.display().to_string())
}

impl PieBoveda {
    /// `cambiar` es el botón con el menú de abrir (lo arma la ventana).
    pub fn new(cambiar: &gtk::MenuButton) -> Self {
        let marca = gtk::Label::builder()
            .label("◈")
            .css_classes(["orbital-pink"])
            .build();
        marca.set_accessible_role(gtk::AccessibleRole::Presentation);
        let nombres = gtk::StringList::new(&[]);
        let desplegable = gtk::DropDown::builder()
            .model(&nombres)
            .hexpand(true)
            .css_classes(["flat", "orbital-vault-select"])
            .build();
        desplegable.update_property(&[gtk::accessible::Property::Label("Bóveda actual")]);
        let raiz = gtk::Box::builder()
            .spacing(8)
            .css_classes(["orbital-vault-footer"])
            .build();
        raiz.append(&marca);
        raiz.append(&desplegable);
        raiz.append(cambiar);
        let this = Self(Rc::new(Inner {
            raiz,
            desplegable,
            nombres,
            rutas: RefCell::new(Vec::new()),
            sincronizando: Cell::new(false),
            al_elegir: RefCell::new(None),
        }));
        let weak = Rc::downgrade(&this.0);
        this.0.desplegable.connect_selected_notify(move |d| {
            let Some(i) = weak.upgrade() else { return };
            if i.sincronizando.get() {
                return;
            }
            let Some(ruta) = i.rutas.borrow().get(d.selected() as usize).cloned() else {
                return;
            };
            if let Some(f) = i.al_elegir.borrow().as_ref() {
                f(ruta);
            }
        });
        this
    }

    pub fn widget(&self) -> &gtk::Box {
        &self.0.raiz
    }

    /// Muestra `actual` elegida, seguida de las `recientes` (si alguna ya no existe, abrirla avisa
    /// con el error de siempre).
    pub fn set_boveda(&self, actual: &Path, recientes: &[PathBuf]) {
        let i = &self.0;
        let mut rutas = vec![actual.to_path_buf()];
        rutas.extend(recientes.iter().filter(|r| r.as_path() != actual).cloned());
        let textos: Vec<String> = rutas.iter().map(|r| nombre(r)).collect();
        let textos: Vec<&str> = textos.iter().map(String::as_str).collect();
        i.sincronizando.set(true);
        i.nombres.splice(0, i.nombres.n_items(), &textos);
        i.desplegable.set_selected(0);
        i.sincronizando.set(false);
        i.desplegable
            .set_tooltip_text(Some(&actual.display().to_string()));
        *i.rutas.borrow_mut() = rutas;
    }

    /// La usuaria eligió otra bóveda del desplegable.
    pub fn connect_elegir(&self, f: impl Fn(PathBuf) + 'static) {
        *self.0.al_elegir.borrow_mut() = Some(Box::new(f));
    }

    /// Bóvedas del desplegable, en orden (para las pruebas).
    #[cfg(test)]
    pub fn rutas(&self) -> Vec<PathBuf> {
        self.0.rutas.borrow().clone()
    }

    /// Elige una posición como lo haría la usuaria (para las pruebas).
    #[cfg(test)]
    pub fn elegir(&self, n: u32) {
        self.0.desplegable.set_selected(n);
    }
}
