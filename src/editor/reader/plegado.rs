//! Plegado y enfoque en la vista formateada (028, research R7; rediseño de la 046).
//!
//! Sin widgets nuevos: cada elemento plegable lleva un glifo de texto con la etiqueta compartida
//! `flecha`, `▾` desplegado o `▸` plegado. El glifo es transparente y cuelga en el margen: la
//! flechita que se ve la dibuja `capas`, siempre si está plegado y, desplegado, solo con el mouse
//! sobre la sección.
//!
//! Lo plegado **no está en el buffer**: un bloque de primer nivel dentro de una sección plegada
//! (o fuera del enfoque) queda solo con su marca, y un ítem plegado se vuelca sin lo que va
//! debajo de su primer renglón. Cada bloque guarda su fuente analizada y la [`Firma`] con la que
//! se volcó; al cambiar el plegado o el enfoque se vuelven a volcar en el sitio solo los bloques
//! cuya firma cambió. Así lo oculto no cuesta nada al dibujar: con texto invisible, GTK recorre
//! cada renglón oculto que cae en pantalla en cada cuadro y su caché de renglones (tres pantallas)
//! no alcanza (046). La búsqueda mira lo oculto en las fuentes ([`Reader::texto_oculto`]).
//!
//! El estado lo decide el editor (las marcas de Código); la vista solo lo muestra y avisa los
//! clics.

use super::*;
use orbital_vault::render::Embed;

const ABIERTA: &str = "▾";
const CERRADA: &str = "▸";

/// Bloques que se vuelven a volcar de una vez antes de mirar el reloj.
const TANDA_REPLEGADO: usize = 8;

/// El glifo bajo `it` es el de un elemento plegado.
pub(super) fn es_cerrada(it: &gtk::TextIter) -> bool {
    it.char() == '▸'
}

/// Prefijo de los «enlaces» de las flechitas en la lista que recorre Tab.
pub(super) const URL_PLEGAR: &str = "orbital-plegar:";

/// Un elemento plegable dibujado.
pub(super) struct PlegableLector {
    /// Línea fuente del elemento.
    pub(super) linea: u32,
    /// El glifo (`▾` o `▸`).
    pub(super) glifo: gtk::TextMark,
    /// Contenido de un ítem en la vista (lo que va después de su primera línea; vacío si está
    /// plegado). Una sección no tiene: llega hasta el próximo encabezado de nivel menor o igual.
    pub(super) contenido: Option<(gtk::TextMark, gtk::TextMark)>,
}

/// Cómo se volcó un bloque de primer nivel respecto del plegado y el enfoque: si cambia, el
/// bloque se vuelve a volcar.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct Firma {
    /// Dentro de una sección plegada o fuera del enfoque: no se vuelca.
    pub(super) oculto: bool,
    /// Líneas plegadas dentro del bloque (su encabezado o sus ítems).
    pub(super) plegadas: Vec<u32>,
    /// El bloque contiene el ítem enfocado (línea): de su lista se vuelca solo ese ítem.
    pub(super) foco: Option<u32>,
}

/// La firma de cada bloque de primer nivel. `esquema`: línea fuente y nivel (si es encabezado)
/// de cada bloque, en orden; `plegables` y `plegadas`, líneas ordenadas; `enfoque`: el rango
/// `linea..=ultima` del elemento enfocado.
pub(super) fn firmas(
    esquema: &[(u32, Option<u8>)],
    plegables: &[u32],
    plegadas: &[u32],
    enfoque: Option<(u32, u32)>,
) -> Vec<Firma> {
    let efectivas: Vec<u32> = plegadas
        .iter()
        .copied()
        .filter(|l| plegables.binary_search(l).is_ok())
        .collect();
    let mut out = Vec::with_capacity(esquema.len());
    // Nivel del encabezado plegado cuya sección se está recorriendo.
    let mut cubre: Option<u8> = None;
    for (n, &(linea, nivel)) in esquema.iter().enumerate() {
        let siguiente = esquema.get(n + 1).map(|x| x.0);
        if let (Some(c), Some(l)) = (cubre, nivel)
            && l <= c
        {
            cubre = None;
        }
        let mut oculto = cubre.is_some();
        let mut foco = None;
        if let Some((a, z)) = enfoque {
            if linea > z || (linea < a && siguiente.is_some_and(|x| x <= a)) {
                oculto = true;
            } else if linea < a {
                foco = Some(a);
            }
        }
        if cubre.is_none() && nivel.is_some() && efectivas.binary_search(&linea).is_ok() {
            cubre = nivel;
        }
        let firma = if oculto {
            Firma {
                oculto,
                ..Firma::default()
            }
        } else {
            let hasta = siguiente.unwrap_or(u32::MAX);
            let i0 = efectivas.partition_point(|l| *l < linea);
            let i1 = efectivas.partition_point(|l| *l < hasta);
            Firma {
                oculto,
                plegadas: efectivas[i0..i1].to_vec(),
                foco,
            }
        };
        out.push(firma);
    }
    out
}

/// Un trozo de lo que el plegado o el enfoque dejó fuera de la vista, para buscar en él.
pub struct Oculto {
    /// Dónde iría en el buffer (para ordenar las coincidencias).
    pub offset: i32,
    /// Su línea fuente: revelarla lo despliega (en una nota incrustada, la de la transclusión).
    pub linea: u32,
    /// Su texto, aproximado al que muestra la vista: un párrafo, un encabezado, un título.
    pub texto: String,
}

/// Los trozos de texto de un bloque con su línea fuente, aproximados a lo que muestra la vista
/// (para buscar en lo plegado). `fija`: la línea de la transclusión que lo incrusta.
fn hojas(b: &Block, fija: Option<u32>, out: &mut Vec<(u32, String)>) {
    let linea = fija.unwrap_or(b.line as u32);
    let mut hoja = |texto: String| out.push((linea, texto));
    match &b.kind {
        BlockKind::Heading { inlines, .. } | BlockKind::Paragraph(inlines) => {
            hoja(plain_text(inlines))
        }
        BlockKind::Code { text, .. } | BlockKind::Html(text) => hoja(text.clone()),
        BlockKind::Table { header, rows, .. } => {
            let mut texto = String::new();
            for fila in std::iter::once(header).chain(rows) {
                for celda in fila {
                    texto.push_str(&plain_text(celda));
                    texto.push('\t');
                }
                texto.push('\n');
            }
            hoja(texto);
        }
        BlockKind::Callout { title, body, .. } => {
            hoja(title.clone());
            body.iter().for_each(|b| hojas(b, fija, out));
        }
        BlockKind::List { items, .. } => items
            .iter()
            .flat_map(|i| &i.blocks)
            .for_each(|b| hojas(b, fija, out)),
        BlockKind::Quote(body) | BlockKind::Seccion { body, .. } | BlockKind::Group(body) => {
            body.iter().for_each(|b| hojas(b, fija, out))
        }
        BlockKind::Transclusion(t) => {
            hoja(t.target.clone());
            if let Embed::Note { blocks, .. } = &t.content {
                blocks.iter().for_each(|b| hojas(b, Some(linea), out));
            }
        }
        BlockKind::Flashcard(f) => f
            .question
            .iter()
            .chain(&f.answer)
            .for_each(|b| hojas(b, fija, out)),
        BlockKind::Generated(_) | BlockKind::Rule | BlockKind::FootnoteDefinition { .. } => {}
    }
}

/// Lo plegado dentro de los ítems de `items` (y de sus sublistas desplegadas): (línea del ítem,
/// trozos de lo que no se volcó).
fn items_plegados(items: &[ListItem], plegadas: &[u32], out: &mut Vec<(u32, Vec<(u32, String)>)>) {
    for item in items {
        let Some(primero) = item.blocks.first() else {
            continue;
        };
        let linea = primero.line as u32;
        if plegadas.binary_search(&linea).is_ok() {
            let mut trozos = Vec::new();
            item.blocks[1..]
                .iter()
                .for_each(|b| hojas(b, None, &mut trozos));
            out.push((linea, trozos));
            continue;
        }
        for sub in &item.blocks {
            if let BlockKind::List { items, .. } = &sub.kind {
                items_plegados(items, plegadas, out);
            }
        }
    }
}

impl RenderState {
    /// La línea fuente es la de un elemento plegable y no es contenido incrustado.
    pub(super) fn es_plegable(&self, linea: usize) -> bool {
        self.embedded.get() == 0 && self.plegables.binary_search(&(linea as u32)).is_ok()
    }

    /// La línea fuente `linea` es la de un elemento plegado del bloque en curso.
    pub(super) fn plegada(&self, linea: usize) -> bool {
        self.plegadas
            .borrow()
            .binary_search(&(linea as u32))
            .is_ok()
    }

    /// El contenido del ítem plegable `indice` del bloque en curso.
    pub(super) fn cerrar_item(&self, indice: usize, desde: gtk::TextMark, hasta: gtk::TextMark) {
        if let Some(p) = self.plegables_lector.borrow_mut().get_mut(indice) {
            p.contenido = Some((desde, hasta));
        }
    }
}

impl Reader {
    /// El glifo de un elemento plegable, con `despues` (espacio hasta el texto). `sangria`: la
    /// del renglón (el glifo cuelga a su izquierda).
    #[allow(clippy::too_many_arguments)]
    pub(super) fn glifos(
        &self,
        s: &RenderState,
        end: &mut gtk::TextIter,
        linea: u32,
        base: &[gtk::TextTag],
        despues: &str,
        sangria: i32,
        cerrada: bool,
    ) {
        let buf = &s.buffer;
        let table = buf.tag_table();
        let inicio = buf.create_mark(None, end, true);
        let mut tags: Vec<gtk::TextTag> = base.to_vec();
        tags.extend(table.lookup("flecha"));
        // Cuelga a la izquierda del texto (sangría negativa del renglón): el texto queda alineado
        // con el resto y la flechita, en el margen.
        tags.push(self.colgar_tag(sangria));
        let glifo = if cerrada { CERRADA } else { ABIERTA };
        buf.insert_with_tags(end, glifo, &tags.iter().collect::<Vec<_>>());
        let fin = buf.create_mark(None, end, false);
        let mut sep: Vec<gtk::TextTag> = base.to_vec();
        sep.extend(table.lookup("flecha-sep"));
        buf.insert_with_tags(end, despues, &sep.iter().collect::<Vec<_>>());
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

    pub fn set_on_enfocar(&self, f: impl Fn(Option<u32>) + 'static) {
        *self.0.on_enfocar.borrow_mut() = Some(Box::new(f));
    }

    pub fn set_on_subir_enfoque(&self, f: impl Fn() + 'static) {
        *self.0.on_subir_enfoque.borrow_mut() = Some(Box::new(f));
    }

    pub(super) fn plegar(&self, linea: u32) {
        if let Some(f) = self.0.on_plegar.borrow().as_ref() {
            f(linea);
        }
    }

    /// La flechita bajo un iterador con la etiqueta `flecha`.
    #[cfg(test)]
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
                g == off
            })
            .map(|p| p.linea)
    }

    /// Las líneas fuente de los elementos plegados (las da el editor desde sus marcas).
    pub fn set_plegado(&self, lineas: Rc<Vec<u32>>) {
        *self.0.plegado.borrow_mut() = lineas;
        self.aplicar_plegado();
    }

    /// Rango `linea..=ultima` del elemento enfocado (o `None` sin enfoque).
    pub fn set_enfoque(&self, rango: Option<(u32, u32)>) {
        *self.0.enfoque.borrow_mut() = rango;
        self.aplicar_plegado();
    }

    /// Las firmas que corresponden hoy a `fuentes` (bloques de primer nivel en orden).
    pub(super) fn firmas_de(&self, fuentes: &[Rc<Block>], plegables: &[u32]) -> Vec<Firma> {
        let esquema: Vec<(u32, Option<u8>)> = fuentes.iter().map(|b| esquema_de(b)).collect();
        firmas(
            &esquema,
            plegables,
            &self.0.plegado.borrow(),
            *self.0.enfoque.borrow(),
        )
    }

    /// Vuelve a volcar en el sitio los bloques cuya firma cambió (plegado, enfoque) y oculta el
    /// encabezado de propiedades fuera del enfoque. Corre también al final de `swap_in` y de
    /// `update`. Si hay mucho que volcar (desplegar todo en una nota grande), sigue por tramos de
    /// unos 8 ms; lo que cambia de a una sección se vuelca enseguida.
    pub(super) fn aplicar_plegado(&self) {
        let i = &self.0;
        let generacion = i.replegado.get() + 1;
        i.replegado.set(generacion);
        self.ocultar_encabezado();
        self.replegar(generacion, true);
    }

    /// Termina ya de volcar lo que el plegado dejó pendiente (antes de mostrar algo que se acaba
    /// de revelar).
    pub fn completar_plegado(&self) {
        self.replegar(self.0.replegado.get(), false);
    }

    fn replegar(&self, generacion: u64, por_tramos: bool) {
        let i = &self.0;
        if i.replegado.get() != generacion {
            return;
        }
        let inicio = Instant::now();
        loop {
            // La próxima tanda de bloques contiguos con la firma cambiada.
            let tanda = {
                let blocks = i.blocks.borrow();
                if blocks.is_empty() {
                    return;
                }
                let fuentes: Vec<Rc<Block>> = blocks.iter().map(|b| b.fuente.clone()).collect();
                let deseadas = self.firmas_de(&fuentes, &i.plegables.borrow());
                let Some(desde) = (0..blocks.len()).find(|&n| blocks[n].firma != deseadas[n])
                else {
                    return;
                };
                let mut hasta = desde + 1;
                while hasta < blocks.len()
                    && hasta - desde < TANDA_REPLEGADO
                    && blocks[hasta].firma != deseadas[hasta]
                {
                    hasta += 1;
                }
                let nuevos: Vec<(Rc<Block>, u64, Firma)> = (desde..hasta)
                    .map(|n| (fuentes[n].clone(), blocks[n].hash, deseadas[n].clone()))
                    .collect();
                (desde..hasta, nuevos)
            };
            let (rango, nuevos) = tanda;
            i.focused_link.set(None);
            let mut blocks = std::mem::take(&mut *i.blocks.borrow_mut());
            self.reemplazar(
                &mut blocks,
                rango,
                nuevos,
                i.base_dir.borrow().clone(),
                i.plegables.borrow().clone(),
            );
            *i.blocks.borrow_mut() = blocks;
            if por_tramos && inicio.elapsed() >= SLICE {
                let this = self.clone();
                glib::idle_add_local_once(move || this.replegar(generacion, true));
                return;
            }
        }
    }

    /// Fuera del enfoque, el encabezado de propiedades (lo de antes del primer bloque) no se ve:
    /// es un widget anclado en su renglón, que se oculta con la etiqueta `fuera-de-enfoque`.
    fn ocultar_encabezado(&self) {
        let buf = self.buffer();
        let Some(tag) = buf.tag_table().lookup("fuera-de-enfoque") else {
            return;
        };
        let primero = self
            .0
            .blocks
            .borrow()
            .first()
            .map(|b| buf.iter_at_mark(&b.mark));
        buf.remove_tag(&tag, &buf.start_iter(), &buf.end_iter());
        if self.0.enfoque.borrow().is_some()
            && let Some(z) = primero
        {
            buf.apply_tag(&tag, &buf.start_iter(), &z);
        }
    }

    /// Lo que el plegado o el enfoque dejó fuera del buffer, en orden: los bloques ocultos y el
    /// contenido de los ítems plegados, por trozos. Para buscar en Lectura (FR-011).
    pub fn texto_oculto(&self) -> Vec<Oculto> {
        let buf = self.buffer();
        let blocks = self.0.blocks.borrow();
        let mut out = Vec::new();
        for b in blocks.iter() {
            let offset = buf.iter_at_mark(&b.mark).offset();
            if b.firma.oculto {
                let mut trozos = Vec::new();
                hojas(&b.fuente, None, &mut trozos);
                out.extend(trozos.into_iter().map(|(linea, texto)| Oculto {
                    offset,
                    linea,
                    texto,
                }));
                continue;
            }
            let BlockKind::List { items, .. } = &b.fuente.kind else {
                continue;
            };
            if b.firma.plegadas.is_empty() {
                continue;
            }
            let mut plegados = Vec::new();
            items_plegados(items, &b.firma.plegadas, &mut plegados);
            for (item, trozos) in plegados {
                let offset = b
                    .plegables
                    .iter()
                    .find(|p| p.linea == item)
                    .and_then(|p| p.contenido.as_ref())
                    .map_or(offset, |(d, _)| buf.iter_at_mark(d).offset());
                out.extend(trozos.into_iter().map(|(linea, texto)| Oculto {
                    offset,
                    linea,
                    texto,
                }));
            }
        }
        out
    }

    /// Dónde quedó en el buffer un trozo de lo oculto después de revelarlo: el primer renglón de
    /// su texto, buscado desde el bloque de primer nivel que contiene su línea (o el comienzo de
    /// ese bloque si no se encuentra).
    pub fn inicio_revelado(&self, linea: u32, texto: &str) -> Option<i32> {
        self.completar_plegado();
        let buf = self.buffer();
        let blocks = self.0.blocks.borrow();
        let n = blocks
            .partition_point(|b| b.line as u32 <= linea)
            .checked_sub(1)?;
        let desde = buf.iter_at_mark(&blocks[n].mark);
        let renglon = texto.lines().find(|l| !l.trim().is_empty()).unwrap_or("");
        if renglon.is_empty() {
            return Some(desde.offset());
        }
        let hasta = blocks.get(n + 1).map(|b| buf.iter_at_mark(&b.mark));
        Some(
            desde
                .forward_search(
                    renglon.trim(),
                    gtk::TextSearchFlags::empty(),
                    hasta.as_ref(),
                )
                .map_or(desde.offset(), |(a, _)| a.offset()),
        )
    }

    /// Línea fuente para «Enfocar» en Lectura: la flechita con el foco o, si no hay, la del
    /// primer bloque visible (decisión 11 del plan).
    pub fn linea_para_enfocar(&self) -> Option<u32> {
        if let Some(idx) = self.0.focused_link.get()
            && let Some(item) = self.all_link_items().get(idx)
            && let Some(l) = item
                .url
                .strip_prefix(URL_PLEGAR)
                .and_then(|s| s.parse().ok())
        {
            return Some(l);
        }
        let rect = self.0.view.visible_rect();
        let (it, _) = self.0.view.line_at_y(rect.y());
        let off = it.offset();
        let buf = self.buffer();
        let blocks = self.0.blocks.borrow();
        let n = blocks
            .partition_point(|b| buf.iter_at_mark(&b.mark).offset() <= off)
            .saturating_sub(1);
        blocks.get(n).map(|b| b.line as u32)
    }

    /// Línea fuente para «Plegar o desplegar» en Lectura: la flechita con el foco o, si no hay, el
    /// elemento bajo el mouse.
    pub fn linea_para_plegar(&self) -> Option<u32> {
        self.0
            .focused_link
            .get()
            .and_then(|idx| self.all_link_items().get(idx).cloned())
            .and_then(|item| item.url.strip_prefix(URL_PLEGAR)?.parse().ok())
            .or(self.0.hover_plegable.get())
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

/// Línea y nivel de encabezado de un bloque de primer nivel.
fn esquema_de(b: &Block) -> (u32, Option<u8>) {
    let nivel = match &b.kind {
        BlockKind::Heading { level, .. } => Some(*level),
        _ => None,
    };
    (b.line as u32, nivel)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn f(oculto: bool, plegadas: &[u32], foco: Option<u32>) -> Firma {
        Firma {
            oculto,
            plegadas: plegadas.to_vec(),
            foco,
        }
    }

    /// Una sección plegada oculta hasta el próximo encabezado de nivel menor o igual; lo plegado
    /// adentro de lo oculto no cuenta; un ítem plegado queda en la firma de su bloque.
    #[test]
    fn firmas_de_secciones_e_items() {
        // 0 # A, 2 ## B, 4 lista (ítems 4 y 6), 8 ### C, 10 párrafo, 12 ## D
        let esquema = [
            (0, Some(1)),
            (2, Some(2)),
            (4, None),
            (8, Some(3)),
            (10, None),
            (12, Some(2)),
        ];
        let plegables = [0, 2, 4, 8, 12];
        assert_eq!(
            firmas(&esquema, &plegables, &[2, 8], None),
            vec![
                f(false, &[], None),
                f(false, &[2], None),
                f(true, &[], None),
                f(true, &[], None),
                f(true, &[], None),
                f(false, &[], None),
            ]
        );
        assert_eq!(
            firmas(&esquema, &plegables, &[4, 8], None),
            vec![
                f(false, &[], None),
                f(false, &[], None),
                f(false, &[4], None),
                f(false, &[8], None),
                f(true, &[], None),
                f(false, &[], None),
            ]
        );
        // Lo que no es plegable no pliega nada.
        assert_eq!(
            firmas(&esquema, &plegables, &[10], None),
            vec![f(false, &[], None); 6]
        );
    }

    /// Enfoque: lo de afuera se oculta; el bloque que contiene el ítem enfocado lo lleva en su
    /// firma.
    #[test]
    fn firmas_con_enfoque() {
        let esquema = [(0, Some(1)), (2, None), (4, None), (9, Some(2))];
        assert_eq!(
            firmas(&esquema, &[0, 5], &[], Some((5, 7))),
            vec![
                f(true, &[], None),
                f(true, &[], None),
                f(false, &[], Some(5)),
                f(true, &[], None),
            ]
        );
        assert_eq!(
            firmas(&esquema, &[0, 9], &[], Some((0, 8))),
            vec![
                f(false, &[], None),
                f(false, &[], None),
                f(false, &[], None),
                f(true, &[], None),
            ]
        );
    }
}
