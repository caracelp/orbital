//! Ids de bloque al guardar (026): cada bloque sin `^id` (o sin atributo `id` en HTML) recibe uno,
//! salvo los bloques donde hay un cursor (el que se está escribiendo).

use super::*;
use orbital_vault::bloques;

impl Editor {
    /// Agrega al buffer los ids que faltan y devuelve si cambió algo. `cursores`: excluir los
    /// bloques con el cursor principal o un cursor extra (autoguardado); sin eso (al dejar la
    /// nota), todos.
    pub(super) fn asignar_ids(&self, texto: &str, cursores: bool) -> bool {
        let i = &self.0;
        // Solo lectura (`raw/`) no se toca; en Visual el DOM es la fuente y pone sus propios ids.
        if !i.view.is_editable() || i.mode.get() == View::Visual {
            return false;
        }
        // Si la nota usa bloques delimitados `<!-- block <jerarquía> <ID> --> ... <!---->` (040),
        // no inyectar `^xxxxxx` al final de los párrafos.
        if i.format.get() != Format::Html
            && crate::grafo::ramas_libres::usa_bloques_delimitados(texto)
        {
            return false;
        }
        let lineas = comienzos_de_linea(texto);
        let excluir: Vec<std::ops::Range<usize>> = if cursores {
            let b = &i.buffer;
            let mut iters = vec![b.iter_at_mark(&b.get_insert())];
            iters.extend(i.extra.borrow().inserciones(b));
            iters
                .iter()
                .map(|it| {
                    let o = byte_de(&lineas, it);
                    o..o
                })
                .collect()
        } else {
            Vec::new()
        };
        let ediciones = if i.format.get() == Format::Html {
            bloques::inserciones_html(texto, &excluir)
        } else {
            bloques::inserciones(texto, &excluir)
        };
        if ediciones.is_empty() {
            return false;
        }

        // Desplazamiento (FR-005): si algo cae antes de lo visible, el texto de arriba no se mueve.
        let b = &i.buffer;
        let rect = i.view.visible_rect();
        let arriba = i
            .view
            .iter_at_location(rect.x(), rect.y())
            .map(|it| (byte_de(&lineas, &it), b.create_mark(None, &it, true)));

        i.asignando_ids.set(true);
        b.begin_user_action();
        for e in &ediciones {
            let mut desde = iter_de(b, &lineas, e.rango.start);
            if e.rango.is_empty() {
                b.insert(&mut desde, e.texto.as_str());
            } else {
                let mut hasta = iter_de(b, &lineas, e.rango.end);
                b.delete(&mut desde, &mut hasta);
                b.insert(&mut desde, e.texto.as_str());
            }
        }
        b.end_user_action();
        i.asignando_ids.set(false);

        if let Some((byte, mark)) = arriba {
            if ediciones.iter().any(|e| e.rango.start < byte) {
                i.view.scroll_to_mark(&mark, 0.0, true, 0.0, 0.0);
            }
            b.delete_mark(&mark);
        }
        true
    }
}

/// El byte donde empieza cada línea.
fn comienzos_de_linea(texto: &str) -> Vec<usize> {
    let mut v = Vec::with_capacity(texto.len() / 32 + 1);
    v.push(0);
    v.extend(
        texto
            .bytes()
            .enumerate()
            .filter_map(|(n, c)| (c == b'\n').then_some(n + 1)),
    );
    v
}

fn byte_de(lineas: &[usize], it: &gtk::TextIter) -> usize {
    lineas.get(it.line() as usize).copied().unwrap_or(0) + it.line_index() as usize
}

fn iter_de(b: &impl IsA<gtk::TextBuffer>, lineas: &[usize], byte: usize) -> gtk::TextIter {
    let linea = lineas.partition_point(|&c| c <= byte) - 1;
    b.iter_at_line_index(linea as i32, (byte - lineas[linea]) as i32)
        .unwrap_or_else(|| b.end_iter())
}

#[cfg(test)]
#[path = "ids_bloque_tests.rs"]
mod ids_bloque_tests;
