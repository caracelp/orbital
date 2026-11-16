//! Flechitas del plegado en el canal izquierdo de Código (028, research R4).
//!
//! Un `GutterRendererPixbuf` de GtkSourceView: se dibuja solo para las líneas visibles, tiene clic
//! propio y no agrega widgets a la vista ni capas dentro del `ScrolledWindow`. Su ancho toma el
//! lugar del margen izquierdo de la columna centrada (`code::center_column`), así la flechita queda
//! al lado del texto. Va en la posición 10 del canal: un renderer de números de línea (posición
//! −30 de GtkSourceView) quedaría a su izquierda.
//!
//! Qué líneas tienen flechita: la tabla del último árbol de plegables si sigue al día con el texto;
//! si no (se está escribiendo), una regla local sobre la línea y las siguientes. Nada global por
//! tecla.

use super::*;
use orbital_vault::plegado as nucleo;

/// Posición del renderer en el canal izquierdo.
const POSICION: i32 = 10;
/// Líneas que la regla local mira hacia abajo, como mucho.
const MIRAR: u32 = 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Flecha {
    Ninguna,
    Abierta,
    Cerrada,
}

/// El renderer, todavía sin vista.
pub(super) fn nuevo() -> sourceview5::GutterRendererPixbuf {
    let r = sourceview5::GutterRendererPixbuf::new();
    r.set_xalign(1.0);
    r.set_xpad(2);
    r.set_alignment_mode(sourceview5::GutterRendererAlignmentMode::First);
    // Un ancho fijo desde el principio: si el canal se midiera por el ícono de la última línea
    // dibujada, cambiaría de ancho mientras la vista se dispone (GTK aborta con
    // `onscreen_validated`). Después lo ajusta `code::center_column`.
    r.set_size_request(code::CANAL_MIN, -1);
    r
}

/// Sangría en columnas (un tab cuenta 4).
fn sangria(t: &str) -> usize {
    t.bytes()
        .take_while(|b| *b == b' ' || *b == b'\t')
        .map(|b| if b == b'\t' { 4 } else { 1 })
        .sum()
}

/// Nivel si la línea es un encabezado ATX.
fn nivel(t: &str) -> Option<usize> {
    let s = t.trim_start();
    if t.len() - s.len() > 3 {
        return None;
    }
    let n = s.bytes().take_while(|b| *b == b'#').count();
    ((1..=6).contains(&n) && (s.len() == n || s.as_bytes()[n] == b' ')).then_some(n)
}

impl Editor {
    pub(super) fn setup_flechas(&self) {
        let i = &self.0;
        let r = &i.flechas;
        sourceview5::prelude::ViewExt::gutter(&i.view, gtk::TextWindowType::Left)
            .insert(r, POSICION);
        let ultima = Rc::new(Cell::new(Flecha::Ninguna));
        let weak = Rc::downgrade(&self.0);
        r.connect_query_data(move |r, _, linea| {
            let Some(i) = weak.upgrade() else { return };
            let f = Editor(i).flecha(linea);
            // El renderer es uno para todas las líneas: se cambia el ícono solo si cambia.
            if f != ultima.get() {
                ultima.set(f);
                r.set_icon_name(match f {
                    Flecha::Ninguna => None,
                    Flecha::Abierta => Some("pan-down-symbolic"),
                    Flecha::Cerrada => Some("pan-end-symbolic"),
                });
            }
        });
        let weak = Rc::downgrade(&self.0);
        r.connect_query_activatable(move |_, it, _| {
            weak.upgrade()
                .is_some_and(|i| Editor(i).flecha(it.line() as u32) != Flecha::Ninguna)
        });
        let weak = Rc::downgrade(&self.0);
        r.connect_activate(move |_, it, _, boton, _, _| {
            if boton != 1 {
                return;
            }
            if let Some(i) = weak.upgrade() {
                Editor(i).alternar_plegado(it.line() as u32);
            }
        });
    }

    pub(super) fn flecha(&self, linea: u32) -> Flecha {
        if !self.plegado_disponible() {
            return Flecha::Ninguna;
        }
        let plegable = match self.arbol_al_dia() {
            Some(ps) => nucleo::en_linea(&ps, linea).is_some(),
            None => self.plegable_local(linea),
        };
        if !plegable {
            Flecha::Ninguna
        } else if self.tiene_pliegues() && self.siguiente_oculta(linea) {
            Flecha::Cerrada
        } else {
            Flecha::Abierta
        }
    }

    /// El comienzo de la línea siguiente está plegado.
    fn siguiente_oculta(&self, linea: u32) -> bool {
        let i = &self.0;
        match i.buffer.iter_at_line(linea as i32 + 1) {
            Some(it) if it.line() as u32 == linea + 1 => plegado::oculto(&it, &i.tag_plegado),
            // Última línea: el tramo que llega al final empieza en su salto.
            _ => {
                let mut it = match i.buffer.iter_at_line(linea as i32) {
                    Some(it) => it,
                    None => return false,
                };
                if !it.ends_line() {
                    it.forward_to_line_end();
                }
                it.has_tag(&i.tag_plegado)
            }
        }
    }

    fn texto_linea(&self, linea: u32) -> Option<String> {
        let b = &self.0.buffer;
        let inicio = b.iter_at_line(linea as i32)?;
        if inicio.line() as u32 != linea {
            return None;
        }
        let mut fin = inicio;
        if !fin.ends_line() {
            fin.forward_to_line_end();
        }
        Some(b.text(&inicio, &fin, true).to_string())
    }

    /// Mientras se escribe (el árbol no está al día): encabezado con algo debajo antes del próximo
    /// de nivel ≤, o ítem con un ítem de más sangría debajo (saltando líneas vacías).
    fn plegable_local(&self, linea: u32) -> bool {
        let Some(t) = self.texto_linea(linea) else {
            return false;
        };
        if let Some(n) = nivel(&t) {
            for k in 1..=MIRAR {
                let Some(s) = self.texto_linea(linea + k) else {
                    return false;
                };
                if s.trim().is_empty() {
                    continue;
                }
                return nivel(&s).is_none_or(|m| m > n);
            }
            return false;
        }
        if outliner::item_de_linea(&t, 0, 0).is_none() {
            return false;
        }
        let propia = sangria(&t);
        for k in 1..=MIRAR {
            let Some(s) = self.texto_linea(linea + k) else {
                return false;
            };
            if s.trim().is_empty() {
                continue;
            }
            return sangria(&s) > propia && outliner::item_de_linea(&s, 0, 0).is_some();
        }
        false
    }
}
