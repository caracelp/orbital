//! Lo «solo en Drive» visto desde la Lectura (061b): un embed (`![[foto.png]]`, `![](foto.png)`)
//! de un archivo que no está en este equipo muestra un marcador de lugar y le pide la copia a la
//! ventana (que se la pide al motor de Drive). Cuando llega, la ventana vuelve a dibujar la nota.
//! Sin conexión, el marcador explica por qué no se ve.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::path::Path;
use std::rc::Rc;

use gtk::prelude::*;

/// Lo que hace la ventana con un pedido (la ruta relativa del archivo).
type Pedir = Rc<dyn Fn(String)>;

thread_local! {
    static PEDIR: RefCell<Option<Pedir>> = const { RefCell::new(None) };
    static EN_LINEA: Cell<bool> = const { Cell::new(false) };
    /// Los marcadores de lugar que esperan cada archivo (para mostrar cuánto falta).
    static ESPERAN: RefCell<HashMap<String, Vec<Espera>>> = RefCell::new(HashMap::new());
}

/// Un marcador de lugar que espera su copia.
struct Espera {
    fila: gtk::glib::WeakRef<gtk::Box>,
    texto: gtk::glib::WeakRef<gtk::Label>,
    nombre: String,
    /// El último cuarto que se dijo en el nombre accesible (no se cambia en cada tramo).
    cuarto: Cell<u8>,
}

/// Cuánto lleva bajado un archivo que esperan embeds: el porcentaje en el texto, y en el nombre
/// accesible solo cada 25 %.
pub fn progreso(rel: &str, bajado: u64, total: u64) {
    ESPERAN.with(|e| {
        let mut e = e.borrow_mut();
        let Some(v) = e.get_mut(rel) else {
            return;
        };
        v.retain(|x| x.texto.upgrade().is_some());
        let pct = (bajado.min(total) * 100).checked_div(total).unwrap_or(100);
        for x in v.iter() {
            let texto = format!("Bajando de Drive: «{}»… {pct} %", x.nombre);
            if let Some(l) = x.texto.upgrade() {
                l.set_label(&texto);
            }
            let cuarto = (pct / 25).min(4) as u8;
            if cuarto != x.cuarto.get()
                && let Some(f) = x.fila.upgrade()
            {
                x.cuarto.set(cuarto);
                f.update_property(&[gtk::accessible::Property::Label(&texto)]);
            }
        }
    });
}

/// Llegó la copia: los marcadores de lugar de ese archivo ya no esperan (la nota se redibuja).
pub fn terminado(rel: &str) {
    ESPERAN.with(|e| e.borrow_mut().remove(rel));
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
    let mut espera = None;
    let texto = if en_linea {
        if let Some((_, rel)) = orbital_vault::ubicacion::origen(ruta) {
            if let Some(p) = PEDIR.with(|p| p.borrow().clone()) {
                p(rel.clone());
            }
            espera = Some(rel);
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
    if let Some(rel) = espera {
        ESPERAN.with(|e| {
            e.borrow_mut().entry(rel).or_default().push(Espera {
                fila: fila.downgrade(),
                texto: l.downgrade(),
                nombre,
                cuarto: Cell::new(0),
            })
        });
    }
    fila.upcast()
}
