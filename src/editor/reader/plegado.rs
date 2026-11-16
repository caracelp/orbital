//! Plegado en la vista formateada (028, research R7).
//!
//! Sin widgets nuevos: cada elemento plegable lleva dos glifos de texto, `▾` y `▸`, con la
//! etiqueta compartida `flecha` (uno de los dos oculto con `flecha-oculta`). Lo plegado se oculta
//! con la etiqueta compartida `plegado`, sobre la unión de los rangos y tocando solo los tramos
//! que cambian. Las etiquetas son de la tabla (creadas en `create_tags`), nunca de un bloque: lo
//! de `new_tag` se borra de la tabla con su bloque. El estado lo decide el editor (las marcas de
//! Código); la vista solo lo muestra y avisa los clics.

use super::*;
use orbital_vault::plegado as nucleo;
use std::ops::Range;

/// Prefijo de los «enlaces» de las flechitas en la lista que recorre Tab.
pub(super) const URL_PLEGAR: &str = "orbital-plegar:";

/// Un elemento plegable dibujado.
pub(super) struct PlegableLector {
    /// Línea fuente del elemento.
    pub(super) linea: u32,
    /// Comienzo de los dos glifos (`▾▸`).
    pub(super) glifo: gtk::TextMark,
    /// Contenido de un ítem en la vista (lo que va después de su primera línea). Una sección no
    /// tiene: llega hasta el próximo encabezado de nivel menor o igual.
    pub(super) contenido: Option<(gtk::TextMark, gtk::TextMark)>,
}

impl RenderState {
    /// La línea fuente es la de un elemento plegable y no es contenido incrustado.
    pub(super) fn es_plegable(&self, linea: usize) -> bool {
        self.embedded.get() == 0 && self.plegables.binary_search(&(linea as u32)).is_ok()
    }

    /// El contenido del ítem plegable `indice` del bloque en curso.
    pub(super) fn cerrar_item(&self, indice: usize, desde: gtk::TextMark, hasta: gtk::TextMark) {
        if let Some(p) = self.plegables_lector.borrow_mut().get_mut(indice) {
            p.contenido = Some((desde, hasta));
        }
    }
}

/// Los tramos (en caracteres) que hoy tienen la etiqueta.
fn tramos(buf: &gtk::TextBuffer, tag: &gtk::TextTag) -> Vec<Range<u32>> {
    let mut out = Vec::new();
    let mut it = buf.start_iter();
    if !it.starts_tag(Some(tag)) && !it.forward_to_tag_toggle(Some(tag)) {
        return out;
    }
    while it.has_tag(tag) || it.starts_tag(Some(tag)) {
        let a = it.offset() as u32;
        if !it.forward_to_tag_toggle(Some(tag)) {
            it = buf.end_iter();
        }
        out.push(a..it.offset() as u32);
        if it.is_end() || !it.forward_to_tag_toggle(Some(tag)) {
            break;
        }
    }
    out
}

impl Reader {
    /// Los dos glifos de un elemento plegable, con `despues` (espacio hasta el texto).
    pub(super) fn glifos(
        &self,
        s: &RenderState,
        end: &mut gtk::TextIter,
        linea: u32,
        base: &[gtk::TextTag],
        despues: &str,
    ) {
        let buf = &s.buffer;
        let table = buf.tag_table();
        let inicio = buf.create_mark(None, end, true);
        let mut tags: Vec<gtk::TextTag> = base.to_vec();
        tags.extend(table.lookup("flecha"));
        buf.insert_with_tags(end, "▾", &tags.iter().collect::<Vec<_>>());
        tags.extend(table.lookup("flecha-oculta"));
        buf.insert_with_tags(end, "▸", &tags.iter().collect::<Vec<_>>());
        let fin = buf.create_mark(None, end, false);
        buf.insert_with_tags(end, despues, &base.iter().collect::<Vec<_>>());
        // Tab llega a la flechita como a un enlace; Enter o Espacio la alternan.
        s.link_items.borrow_mut().push(LinkItem {
            url: format!("{URL_PLEGAR}{linea}"),
            start: inicio.clone(),
            end: fin,
        });
        s.plegables_lector.borrow_mut().push(PlegableLector {
            linea,
            glifo: inicio,
            contenido: None,
        });
    }

    /// Clic o Enter en una flechita: avisa la línea fuente del elemento.
    pub fn set_on_plegar(&self, f: impl Fn(u32) + 'static) {
        *self.0.on_plegar.borrow_mut() = Some(Box::new(f));
    }

    pub(super) fn plegar(&self, linea: u32) {
        if let Some(f) = self.0.on_plegar.borrow().as_ref() {
            f(linea);
        }
    }

    /// La flechita bajo un iterador con la etiqueta `flecha`.
    pub(super) fn flecha_en(&self, it: &gtk::TextIter) -> Option<u32> {
        let buf = self.buffer();
        let off = it.offset();
        let blocks = self.0.blocks.borrow();
        let n = blocks
            .partition_point(|b| buf.iter_at_mark(&b.mark).offset() <= off)
            .checked_sub(1)?;
        blocks[n]
            .plegables
            .iter()
            .find(|p| {
                let g = buf.iter_at_mark(&p.glifo).offset();
                (g..g + 2).contains(&off)
            })
            .map(|p| p.linea)
    }

    /// Las líneas fuente de los elementos plegados (las da el editor desde sus marcas).
    pub fn set_plegado(&self, lineas: Rc<Vec<u32>>) {
        *self.0.plegado.borrow_mut() = lineas;
        self.aplicar_plegado();
    }

    /// Oculta lo plegado y pone cada flechita en su estado, tocando solo lo que cambia. Corre
    /// también al final de `swap_in` y de `update` (lo que la actualización en el sitio inserta no
    /// tiene etiqueta).
    pub(super) fn aplicar_plegado(&self) {
        let i = &self.0;
        let buf = self.buffer();
        let table = buf.tag_table();
        let (Some(tag), Some(oculta)) = (table.lookup("plegado"), table.lookup("flecha-oculta"))
        else {
            return;
        };
        let plegadas = i.plegado.borrow().clone();
        let blocks = i.blocks.borrow();
        let off = |m: &gtk::TextMark| buf.iter_at_mark(m).offset() as u32;
        let mut deseado: Vec<Range<u32>> = Vec::new();
        for (n, b) in blocks.iter().enumerate() {
            for p in &b.plegables {
                let plegado = plegadas.binary_search(&p.linea).is_ok();
                // El glifo visible: `▾` desplegado, `▸` plegado.
                let g = buf.iter_at_mark(&p.glifo);
                if g.has_tag(&oculta) != plegado {
                    let mut g2 = g;
                    g2.forward_chars(2);
                    buf.remove_tag(&oculta, &g, &g2);
                    let mut a = g;
                    if !plegado {
                        a.forward_char();
                    }
                    let mut z = a;
                    z.forward_char();
                    buf.apply_tag(&oculta, &a, &z);
                }
                if !plegado {
                    continue;
                }
                match &p.contenido {
                    Some((d, h)) => deseado.push(off(d)..off(h)),
                    None => {
                        let nivel = b.nivel.unwrap_or(1);
                        let Some(sig) = blocks.get(n + 1) else {
                            continue;
                        };
                        let hasta = blocks[n + 1..]
                            .iter()
                            .find(|x| x.nivel.is_some_and(|m| m <= nivel))
                            .map_or(buf.end_iter().offset() as u32, |x| off(&x.mark));
                        deseado.push(off(&sig.mark)..hasta);
                    }
                }
            }
        }
        drop(blocks);
        nucleo::unir(&mut deseado);
        let actual = tramos(&buf, &tag);
        let (agregar, quitar) = nucleo::diferencia(&actual, &deseado);
        for r in &quitar {
            buf.remove_tag(
                &tag,
                &buf.iter_at_offset(r.start as i32),
                &buf.iter_at_offset(r.end as i32),
            );
        }
        for r in &agregar {
            buf.apply_tag(
                &tag,
                &buf.iter_at_offset(r.start as i32),
                &buf.iter_at_offset(r.end as i32),
            );
        }
    }

    /// Las líneas fuente de los elementos plegados cuyo contenido incluye el carácter `off` (para
    /// desplegarlos antes de mostrar una coincidencia de la búsqueda, FR-011).
    pub fn plegados_en(&self, off: i32) -> Vec<u32> {
        let buf = self.buffer();
        let Some(tag) = buf.tag_table().lookup("plegado") else {
            return Vec::new();
        };
        if !buf.iter_at_offset(off).has_tag(&tag) {
            return Vec::new();
        }
        let plegadas = self.0.plegado.borrow().clone();
        let blocks = self.0.blocks.borrow();
        let o = |m: &gtk::TextMark| buf.iter_at_mark(m).offset();
        let mut out = Vec::new();
        for (n, b) in blocks.iter().enumerate() {
            for p in &b.plegables {
                if plegadas.binary_search(&p.linea).is_err() {
                    continue;
                }
                let rango = match &p.contenido {
                    Some((d, h)) => o(d)..o(h),
                    None => {
                        let nivel = b.nivel.unwrap_or(1);
                        let Some(sig) = blocks.get(n + 1) else {
                            continue;
                        };
                        let hasta = blocks[n + 1..]
                            .iter()
                            .find(|x| x.nivel.is_some_and(|m| m <= nivel))
                            .map_or(buf.end_iter().offset(), |x| o(&x.mark));
                        o(&sig.mark)..hasta
                    }
                };
                if rango.contains(&off) {
                    out.push(p.linea);
                }
            }
        }
        out
    }

    /// Para las pruebas: la flechita bajo el carácter `off` y el clic sobre ella.
    #[cfg(test)]
    pub fn clic_en(&self, off: i32) -> Option<u32> {
        let it = self.buffer().iter_at_offset(off);
        let linea = self.flecha_en(&it)?;
        self.plegar(linea);
        Some(linea)
    }

    /// Para las pruebas: las URL de lo que recorre Tab, y activar la `n`.
    #[cfg(test)]
    pub fn urls_tab(&self) -> Vec<String> {
        self.all_link_items().into_iter().map(|l| l.url).collect()
    }

    #[cfg(test)]
    pub fn activar_tab(&self, n: usize) -> bool {
        self.set_focused_link(Some(n));
        self.activate_focused_link()
    }

    /// Para las pruebas: las líneas fuente con flechita, en orden.
    #[cfg(test)]
    pub fn lineas_con_flecha(&self) -> Vec<u32> {
        self.0
            .blocks
            .borrow()
            .iter()
            .flat_map(|b| b.plegables.iter().map(|p| p.linea))
            .collect()
    }
}
