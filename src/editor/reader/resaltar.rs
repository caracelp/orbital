//! Resaltado en reposo de los bloques de código de la vista formateada (055).
//!
//! El volcado pone el código sin color si no está en la caché de `crate::resaltado` y anota el
//! bloque de primer nivel. Acá, en reposo y de a tramos de [`PASO`], se resaltan primero los
//! bloques más cercanos a lo visible; al terminar uno, se le ponen las etiquetas en su lugar
//! (sin volver a volcarlo: no cambia el texto, ni la selección, ni el desplazamiento).

use super::*;

/// Tiempo máximo de cada vuelta en reposo (por debajo de un cuadro, como el volcado).
const PASO: Duration = Duration::from_millis(6);

/// Un bloque de primer nivel con bloques de código por resaltar.
pub(super) struct Pendiente {
    /// El bloque analizado: identifica su `BlockInfo` (si se reemplazó, ya no está).
    pub(super) fuente: Rc<Block>,
    /// (lenguaje, texto) de cada bloque de código, en el orden del volcado.
    pub(super) codigo: Vec<(String, String)>,
}

impl Reader {
    /// Programa el resaltado en reposo si hay algo pendiente y no está programado.
    pub(super) fn programar_resaltado(&self) {
        let i = &self.0;
        if i.resaltar_programado.get() || i.resaltar.borrow().is_empty() {
            return;
        }
        i.resaltar_programado.set(true);
        let debil = Rc::downgrade(&self.0);
        glib::idle_add_local(move || {
            let Some(inner) = debil.upgrade() else {
                return glib::ControlFlow::Break;
            };
            let this = Reader(inner);
            if this.resaltar_un_paso() {
                glib::ControlFlow::Continue
            } else {
                this.0.resaltar_programado.set(false);
                glib::ControlFlow::Break
            }
        });
    }

    /// Una vuelta de resaltado; `true` si queda algo.
    fn resaltar_un_paso(&self) -> bool {
        let i = &self.0;
        let limite = Instant::now() + PASO;
        loop {
            let Some(n) = self.proximo_pendiente() else {
                return false;
            };
            // Se resalta lo que falte de este bloque; si se acaba el tiempo, sigue en la
            // próxima vuelta con el mismo (el que está a medias es el primero de la lista).
            let terminado = {
                let pendientes = i.resaltar.borrow();
                pendientes[n]
                    .codigo
                    .iter()
                    .all(|(lang, texto)| crate::resaltado::avanzar(lang, texto, limite))
            };
            if !terminado {
                let p = i.resaltar.borrow_mut().remove(n);
                i.resaltar.borrow_mut().insert(0, p);
                return true;
            }
            let p = i.resaltar.borrow_mut().remove(n);
            self.pintar_pendiente(&p);
            if Instant::now() >= limite {
                return !i.resaltar.borrow().is_empty();
            }
        }
    }

    /// El pendiente a resaltar ahora: el que quedó a medias, o el más cercano a lo visible. Los
    /// que ya no están en la vista (se reemplazaron o se rearmó todo) se descartan.
    fn proximo_pendiente(&self) -> Option<usize> {
        let i = &self.0;
        let blocks = i.blocks.borrow();
        let buffer = self.buffer();
        let posicion = |p: &Pendiente| {
            blocks
                .iter()
                .find(|b| Rc::ptr_eq(&b.fuente, &p.fuente))
                .map(|b| buffer.iter_at_mark(&b.mark).offset())
        };
        i.resaltar
            .borrow_mut()
            .retain(|p| posicion(p).is_some() && !p.codigo.is_empty());
        let pendientes = i.resaltar.borrow();
        if pendientes.is_empty() {
            return None;
        }
        let rect = i.view.visible_rect();
        let (arriba, _) = i.view.line_at_y(rect.y().max(0));
        let desde = arriba.offset();
        // El que contiene lo de arriba de la pantalla está antes: se cuenta desde su comienzo
        // con ventaja sobre los que están más arriba todavía.
        pendientes
            .iter()
            .enumerate()
            .min_by_key(|(_, p)| {
                let off = posicion(p).unwrap_or(0);
                if off >= desde {
                    (off - desde) as i64
                } else {
                    2 * (desde - off) as i64
                }
            })
            .map(|(n, _)| n)
    }

    /// Pone las etiquetas de un pendiente ya resaltado. Cada bloque de código es un tramo seguido
    /// con la etiqueta `codeblock` dentro del bloque de primer nivel; se emparejan en orden por su
    /// texto.
    fn pintar_pendiente(&self, p: &Pendiente) {
        let i = &self.0;
        let blocks = i.blocks.borrow();
        let Some(n) = blocks.iter().position(|b| Rc::ptr_eq(&b.fuente, &p.fuente)) else {
            return;
        };
        let buffer = self.buffer();
        let Some(codeblock) = buffer.tag_table().lookup("codeblock") else {
            return;
        };
        let mut it = buffer.iter_at_mark(&blocks[n].mark);
        let fin = blocks
            .get(n + 1)
            .map_or(buffer.end_iter(), |b| buffer.iter_at_mark(&b.mark));
        let mut codigo = p.codigo.iter();
        let mut actual = codigo.next();
        while let Some((lang, texto)) = actual {
            if !it.has_tag(&codeblock) && !it.forward_to_tag_toggle(Some(&codeblock)) {
                return;
            }
            if it >= fin {
                return;
            }
            let inicio = it;
            if !it.forward_to_tag_toggle(Some(&codeblock)) {
                it = buffer.end_iter();
            }
            if buffer.slice(&inicio, &it, true) == texto.as_str() {
                if let Some(Some(tramos)) = crate::resaltado::en_cache(lang, texto) {
                    pintar_codigo(&buffer, inicio.offset(), &tramos);
                }
                actual = codigo.next();
            }
        }
    }
}
