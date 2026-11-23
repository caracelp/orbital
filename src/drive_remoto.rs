//! Lo «solo en Drive» visto desde la Lectura (061b): un embed (`![[foto.png]]`, `![](foto.png)`)
//! de un archivo que no está en este equipo muestra un marcador de lugar y le pide la copia a la
//! ventana (que se la pide al motor de Drive). Cuando llega, la ventana vuelve a dibujar la nota.
//! Sin conexión, el marcador explica por qué no se ve.

use std::cell::{Cell, RefCell};
use std::path::Path;
use std::rc::Rc;

use gtk::prelude::*;

/// Lo que hace la ventana con un pedido (la ruta relativa del archivo).
type Pedir = Rc<dyn Fn(String)>;

thread_local! {
    static PEDIR: RefCell<Option<Pedir>> = const { RefCell::new(None) };
    static EN_LINEA: Cell<bool> = const { Cell::new(false) };
}

/// La ventana se anota para recibir los pedidos (la ruta relativa del archivo).
pub fn configurar(pedir: Pedir) {
    PEDIR.with(|p| *p.borrow_mut() = Some(pedir));
}

/// Si hay conexión con Drive (lo decide la ventana con el resumen del motor).
pub fn poner_en_linea(en_linea: bool) {
    EN_LINEA.with(|e| e.set(en_linea));
}

/// Si `ruta` es la copia (todavía sin bajar o ya bajada) de algo «solo en Drive».
pub fn es_remoto(ruta: &Path) -> bool {
    orbital_vault::ubicacion::origen(ruta).is_some()
}

/// El marcador de lugar de un archivo «solo en Drive» sin copia, y el pedido de la copia.
pub fn marcador_de_lugar(ruta: &Path) -> gtk::Widget {
    let nombre = ruta
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let en_linea = EN_LINEA.with(Cell::get);
    let fila = gtk::Box::builder()
        .spacing(8)
        .margin_top(6)
        .margin_bottom(6)
        .css_classes(["dim-label"])
        .build();
    let texto = if en_linea {
        if let Some((_, rel)) = orbital_vault::ubicacion::origen(ruta)
            && let Some(p) = PEDIR.with(|p| p.borrow().clone())
        {
            p(rel);
        }
        fila.append(&adw::Spinner::new());
        format!("Bajando de Drive: «{nombre}»…")
    } else {
        fila.append(&gtk::Image::from_icon_name("system-lock-screen-symbolic"));
        format!("«{nombre}» está solo en Drive y no hay conexión: se va a ver cuando vuelva la red")
    };
    let l = gtk::Label::builder()
        .label(&texto)
        .wrap(true)
        .xalign(0.0)
        .build();
    fila.append(&l);
    fila.update_property(&[gtk::accessible::Property::Label(&texto)]);
    fila.upcast()
}
