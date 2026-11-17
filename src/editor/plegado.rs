//! Outliner plegable en el editor (028): el modelo de pliegues de la nota abierta, las reglas que
//! nunca esconden lo que se edita o se busca y la reconciliación diferida.
//!
//! Cada pliegue es una marca al comienzo de la línea de su elemento en el buffer de Código (la
//! verdad para Código y Lectura). Lo plegado se oculta con una sola etiqueta `invisible`, aplicada
//! sobre la unión de los rangos y tocando solo los tramos que cambian (nunca se quita de todo el
//! buffer: GTK volvería a disponer el documento entero). Sin pliegues, lo único que corre por tecla
//! son salidas tempranas: nada de esto recorre el texto.
//!
//! Las marcas tienen gravedad derecha: un Enter al comienzo de la línea del elemento (o un `^id`
//! que el guardado inserta al comienzo de la línea) deja la marca con su elemento.

use super::*;
use orbital_vault::Plegable;
use orbital_vault::plegado::{self as nucleo, Clave};
use std::ops::Range;

/// Etiqueta de lo plegado en el buffer de Código.
pub(super) const TAG_PLEGADO: &str = "plegado";
/// Etiqueta de lo que queda fuera del enfoque (tanda B).
pub(super) const TAG_FUERA: &str = "fuera-de-enfoque";
/// Pausa después de la última tecla antes de reconciliar los pliegues con el texto (research R6).
const RECONCILIAR: Duration = Duration::from_millis(300);

pub(super) type PlegadoFn = Box<dyn Fn(&RelPath, Vec<Clave>)>;

/// Árbol de plegables calculado para una generación del texto.
pub(super) struct Arbol {
    pub(super) generacion: u64,
    pub(super) texto: Rc<str>,
    pub(super) plegables: Rc<Vec<Plegable>>,
}

/// Qué hacer con un cursor que quedaría dentro de lo que se pliega.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Cursor {
    /// Plegar a pedido: el cursor pasa al final de la línea del elemento (decisión 7).
    Mover,
    /// Reconciliar o restaurar: el elemento que contiene un cursor no se pliega.
    Respetar,
}

/// Las dos etiquetas invisibles del buffer de Código.
pub(super) fn crear_etiquetas(buffer: &sourceview5::Buffer) -> (gtk::TextTag, gtk::TextTag) {
    let tabla = buffer.tag_table();
    let nueva = |nombre: &str| {
        let t = gtk::TextTag::builder().name(nombre).invisible(true).build();
        tabla.add(&t);
        t
    };
    (nueva(TAG_PLEGADO), nueva(TAG_FUERA))
}

/// El iterador está en texto oculto por `tag`. El comienzo del tramo cuenta como oculto si es
/// comienzo de línea; si no, es el salto de la línea del elemento (un tramo que llega al final
/// del texto sin salto) y el cursor ahí está al final de una línea visible.
pub(super) fn oculto(it: &gtk::TextIter, tag: &gtk::TextTag) -> bool {
    if it.has_tag(tag) {
        return !it.starts_tag(Some(tag)) || it.starts_line();
    }
    if it.is_end() {
        let mut antes = *it;
        return antes.backward_char() && antes.has_tag(tag);
    }
    false
}

/// Línea (desde 0) de un borde de tramo: los tramos van de comienzo de línea a comienzo de
/// línea, salvo el que empieza en el salto de la línea del elemento o termina al final del texto.
fn linea_inicio(it: &gtk::TextIter) -> u32 {
    if it.starts_line() {
        it.line() as u32
    } else {
        it.line() as u32 + 1
    }
}

fn linea_fin(it: &gtk::TextIter) -> u32 {
    if it.starts_line() && !it.is_end() {
        it.line() as u32
    } else {
        it.line() as u32 + 1
    }
}

/// Los rangos de líneas que hoy tienen la etiqueta (O(cambios de la etiqueta)).
pub(super) fn rangos_actuales(
    buf: &impl IsA<gtk::TextBuffer>,
    tag: &gtk::TextTag,
) -> Vec<Range<u32>> {
    let mut out: Vec<Range<u32>> = Vec::new();
    let mut it = buf.start_iter();
    if !it.starts_tag(Some(tag)) && !it.forward_to_tag_toggle(Some(tag)) {
        return out;
    }
    loop {
        if !it.has_tag(tag) {
            // Un cierre suelto (no debería pasar): seguir.
            if !it.forward_to_tag_toggle(Some(tag)) {
                break;
            }
            continue;
        }
        let a = linea_inicio(&it);
        if !it.forward_to_tag_toggle(Some(tag)) {
            it = buf.end_iter();
        }
        let b = linea_fin(&it);
        if a < b {
            match out.last_mut() {
                Some(u) if a <= u.end => u.end = u.end.max(b),
                _ => out.push(a..b),
            }
        }
        if it.is_end() || !it.forward_to_tag_toggle(Some(tag)) {
            break;
        }
    }
    out
}

/// Los rangos de líneas con la etiqueta entre las líneas `desde` y `hasta` (O(cambios de la
/// etiqueta en ese tramo)).
pub(super) fn rangos_entre(
    buf: &impl IsA<gtk::TextBuffer>,
    tag: &gtk::TextTag,
    desde: u32,
    hasta: u32,
) -> Vec<Range<u32>> {
    let mut out: Vec<Range<u32>> = Vec::new();
    let Some(mut it) = buf.iter_at_line(desde as i32) else {
        return out;
    };
    let mut abierto = it.has_tag(tag).then_some(desde);
    loop {
        if !it.forward_to_tag_toggle(Some(tag)) {
            it = buf.end_iter();
        }
        match abierto.take() {
            Some(a) => {
                let b = linea_fin(&it);
                if a < b {
                    out.push(a..b);
                }
            }
            None if it.starts_tag(Some(tag)) => abierto = Some(linea_inicio(&it)),
            None => {}
        }
        if it.is_end() || it.line() as u32 > hasta {
            if let Some(a) = abierto {
                out.push(a..hasta + 1);
            }
            break;
        }
    }
    out
}

/// Los iteradores de un rango de líneas `a..b` tal como lo cubre la etiqueta.
fn iters_de(
    buf: &impl IsA<gtk::TextBuffer>,
    r: &Range<u32>,
) -> Option<(gtk::TextIter, gtk::TextIter)> {
    let total = buf.line_count() as u32;
    let mut desde = buf.iter_at_line(r.start as i32)?;
    let hasta = if r.end < total {
        buf.iter_at_line(r.end as i32)?
    } else {
        // Hasta el final del texto: si la última línea no tiene salto, desde el salto de la línea
        // del elemento (si no, quedaría una línea vacía visible debajo).
        let fin = buf.end_iter();
        if !fin.starts_line() {
            desde.backward_char();
        }
        fin
    };
    Some((desde, hasta))
}

/// Deja `tag` exactamente sobre las líneas `deseado` (ordenadas y disjuntas), tocando solo lo que
/// cambia (research R3).
pub(super) fn aplicar_ocultas(
    buf: &impl IsA<gtk::TextBuffer>,
    tag: &gtk::TextTag,
    deseado: &[Range<u32>],
) {
    let actual = rangos_actuales(buf, tag);
    let (agregar, quitar) = nucleo::diferencia(&actual, deseado);
    for r in &quitar {
        if let Some((a, b)) = iters_de(buf, r) {
            buf.remove_tag(tag, &a, &b);
        }
    }
    for r in &agregar {
        if let Some((a, b)) = iters_de(buf, r) {
            buf.apply_tag(tag, &a, &b);
        }
    }
    // Un tramo que no empieza ni termina en un borde de línea (una edición lo corrió) se rehace.
    if quitar.is_empty() && agregar.is_empty() && !deseado.is_empty() && !bordes_limpios(buf, tag) {
        buf.remove_tag(tag, &buf.start_iter(), &buf.end_iter());
        for r in deseado {
            if let Some((a, b)) = iters_de(buf, r) {
                buf.apply_tag(tag, &a, &b);
            }
        }
    }
}

/// Cada tramo de la etiqueta empieza en un comienzo de línea (o en un salto antes del final) y
/// termina en un comienzo de línea o al final.
fn bordes_limpios(buf: &impl IsA<gtk::TextBuffer>, tag: &gtk::TextTag) -> bool {
    let mut it = buf.start_iter();
    loop {
        if it.starts_tag(Some(tag)) {
            let ok = it.starts_line() || it.ends_line();
            if !ok {
                return false;
            }
        } else if it.ends_tag(Some(tag)) && !(it.starts_line() || it.is_end()) {
            return false;
        }
        if !it.forward_to_tag_toggle(Some(tag)) {
            return true;
        }
    }
}

impl Editor {
    /// Conecta las señales del plegado (una vez, desde `Editor::new`).
    pub(super) fn setup_plegado(&self) {
        let i = &self.0;
        // Regla 1: el cursor nunca queda en texto oculto (ni plegado ni fuera del enfoque).
        let weak = Rc::downgrade(&self.0);
        i.buffer.connect_mark_set(move |buf, it, mark| {
            let Some(i) = weak.upgrade() else { return };
            if (i.pliegues.borrow().is_empty() && i.enfoque.borrow().is_none())
                || i.sin_desplegar.get()
                || i.loading.get()
            {
                return;
            }
            if mark != &buf.get_insert() {
                return;
            }
            if oculto(it, &i.tag_fuera) {
                Editor(i.clone()).salir_enfoque();
            }
            if oculto(it, &i.tag_plegado) {
                Editor(i).revelar_linea(it.line() as u32);
            }
        });
        // Regla 2: una edición que toca texto oculto lo despliega antes.
        let weak = Rc::downgrade(&self.0);
        i.buffer.connect_insert_text(move |_, it, _| {
            let Some(i) = weak.upgrade() else { return };
            if i.pliegues.borrow().is_empty() || i.sin_desplegar.get() || i.loading.get() {
                return;
            }
            if oculto(it, &i.tag_plegado) {
                Editor(i).revelar_linea(it.line() as u32);
            }
        });
        let weak = Rc::downgrade(&self.0);
        i.buffer.connect_delete_range(move |buf, a, b| {
            let Some(i) = weak.upgrade() else { return };
            if (i.pliegues.borrow().is_empty() && i.enfoque.borrow().is_none()) || i.loading.get() {
                return;
            }
            let editor = Editor(i.clone());
            editor.olvidar_marcas_borradas(buf, a, b);
            editor.olvidar_enfoque_borrado(buf, a, b);
            if i.sin_desplegar.get() {
                return;
            }
            let (a, b) = (a.offset(), b.offset());
            let lineas = editor.lineas_ocultas_tocadas(a, b);
            if !lineas.is_empty() {
                editor.desplegar_lineas(&lineas);
            }
        });
        // Copiar y cortar (decisión 14): GTK arma lo copiado sin el texto invisible y cortar
        // perdería lo plegado. Con la selección sobre algo plegado, se copia el texto entero.
        let weak = Rc::downgrade(&self.0);
        i.view.connect_copy_clipboard(move |v| {
            if let Some(i) = weak.upgrade()
                && Editor(i).copiar_con_lo_oculto(false)
            {
                v.stop_signal_emission_by_name("copy-clipboard");
            }
        });
        let weak = Rc::downgrade(&self.0);
        i.view.connect_cut_clipboard(move |v| {
            if let Some(i) = weak.upgrade()
                && Editor(i).copiar_con_lo_oculto(true)
            {
                v.stop_signal_emission_by_name("cut-clipboard");
            }
        });
        // Arrastrar una selección la mueve con el mismo contenido sin lo invisible: si el arrastre
        // empieza dentro de una selección que toca algo plegado, se despliega antes.
        let arrastre = gtk::GestureDrag::new();
        arrastre.set_button(1);
        arrastre.set_propagation_phase(gtk::PropagationPhase::Capture);
        let weak = Rc::downgrade(&self.0);
        arrastre.connect_drag_begin(move |_, x, y| {
            let Some(i) = weak.upgrade() else { return };
            if i.pliegues.borrow().is_empty() {
                return;
            }
            let Some((a, z)) = i.buffer.selection_bounds() else {
                return;
            };
            let (bx, by) = super::posicion::al_buffer(i.view.upcast_ref(), x, y);
            let Some(it) = super::posicion::iter_en(i.view.upcast_ref(), bx, by) else {
                return;
            };
            if it.offset() >= a.offset() && it.offset() <= z.offset() {
                let e = Editor(i.clone());
                let lineas = e.lineas_ocultas_tocadas(a.offset(), z.offset());
                if !lineas.is_empty() {
                    e.desplegar_lineas(&lineas);
                }
            }
        });
        i.view.add_controller(arrastre);
        let weak = Rc::downgrade(&self.0);
        i.reader.set_on_enfocar(move |linea| {
            if let Some(i) = weak.upgrade() {
                let e = Editor(i);
                match linea {
                    Some(l) => {
                        e.enfocar(l);
                    }
                    None => {
                        e.enfocar_en_lectura();
                    }
                }
            }
        });
        let weak = Rc::downgrade(&self.0);
        i.reader.set_on_subir_enfoque(move || {
            if let Some(i) = weak.upgrade() {
                Editor(i).subir_enfoque();
            }
        });
        self.setup_flechas();
    }

    /// Si la selección toca algo plegado, pone en el portapapeles su texto con lo oculto (y, al
    /// cortar, la borra: el borrado despliega antes). Devuelve si lo hizo.
    fn copiar_con_lo_oculto(&self, cortar: bool) -> bool {
        let i = &self.0;
        let b = &i.buffer;
        if !self.tiene_pliegues() {
            return false;
        }
        let Some((mut a, mut z)) = b.selection_bounds() else {
            return false;
        };
        if self
            .lineas_ocultas_tocadas(a.offset(), z.offset())
            .is_empty()
        {
            return false;
        }
        let texto = b.text(&a, &z, true);
        i.view.clipboard().set_text(&texto);
        if cortar && i.view.is_editable() {
            b.begin_user_action();
            b.delete(&mut a, &mut z);
            b.end_user_action();
        }
        true
    }

    /// El árbol de plegables del texto actual (calculado acá si cambió desde la última pasada).
    pub(super) fn arbol(&self) -> (Rc<str>, Rc<Vec<Plegable>>) {
        let i = &self.0;
        let generacion = i.texto_gen.get();
        if let Some(a) = i.arbol.borrow().as_ref()
            && a.generacion == generacion
        {
            return (a.texto.clone(), a.plegables.clone());
        }
        let texto: Rc<str> = self.text().into();
        let plegables = Rc::new(if self.plegado_disponible() {
            nucleo::plegables(&texto)
        } else {
            Vec::new()
        });
        *i.arbol.borrow_mut() = Some(Arbol {
            generacion,
            texto: texto.clone(),
            plegables: plegables.clone(),
        });
        (texto, plegables)
    }

    /// El árbol de la última pasada si sigue al día con el texto (para el canal, sin calcular).
    pub(super) fn arbol_al_dia(&self) -> Option<Rc<Vec<Plegable>>> {
        let i = &self.0;
        i.arbol
            .borrow()
            .as_ref()
            .filter(|a| a.generacion == i.texto_gen.get())
            .map(|a| a.plegables.clone())
    }

    /// Se pliega en las notas `.md` (FR-017: las `.html` quedan fuera).
    pub fn plegado_disponible(&self) -> bool {
        self.0.format.get() == Format::Md && self.0.session.borrow().is_some()
    }

    /// Hay algo plegado en la nota abierta.
    pub fn tiene_pliegues(&self) -> bool {
        !self.0.pliegues.borrow().is_empty()
    }

    /// Líneas (desde 0) de los elementos plegados, ordenadas y sin repetir.
    pub fn lineas_plegadas(&self) -> Vec<u32> {
        let b = &self.0.buffer;
        let mut v: Vec<u32> = self
            .0
            .pliegues
            .borrow()
            .iter()
            .map(|m| b.iter_at_mark(m).line() as u32)
            .collect();
        v.sort_unstable();
        v.dedup();
        v
    }

    /// La línea `linea` es la de un elemento plegado.
    pub fn esta_plegado(&self, linea: u32) -> bool {
        self.lineas_plegadas().binary_search(&linea).is_ok()
    }

    /// Pliega el elemento de la línea `linea` (desde 0). Devuelve si cambió algo.
    pub fn plegar(&self, linea: u32) -> bool {
        let (_, ps) = self.arbol();
        if nucleo::en_linea(&ps, linea).is_none() || self.esta_plegado(linea) {
            return false;
        }
        let mut lineas = self.lineas_plegadas();
        lineas.push(linea);
        self.poner_pliegues(lineas, Cursor::Mover);
        true
    }

    /// Despliega el elemento de la línea `linea` (sus elementos internos quedan como estaban).
    pub fn desplegar(&self, linea: u32) -> bool {
        let lineas = self.lineas_plegadas();
        if lineas.binary_search(&linea).is_err() {
            return false;
        }
        self.desplegar_lineas(&[linea]);
        true
    }

    /// Clic en la flechita, Enter en Lectura o `win.alternar-plegado`.
    pub fn alternar_plegado(&self, linea: u32) -> bool {
        if self.esta_plegado(linea) {
            self.desplegar(linea)
        } else {
            self.plegar(linea)
        }
    }

    /// `win.alternar-plegado`: el elemento de la línea del cursor o, si la línea no es de un
    /// elemento, el más interno que la contiene.
    /// En Lectura el cursor no se ve: `win.alternar-plegado` pliega la flechita con el foco o el
    /// elemento bajo el mouse; si no hay ninguno, lo del cursor de Código.
    pub fn alternar_plegado_en_lectura(&self) -> bool {
        match self.0.reader.linea_para_plegar() {
            Some(l) => self.alternar_plegado(l),
            None => self.alternar_plegado_en_cursor(),
        }
    }

    pub fn alternar_plegado_en_cursor(&self) -> bool {
        let b = &self.0.buffer;
        let linea = b.iter_at_mark(&b.get_insert()).line() as u32;
        let (_, ps) = self.arbol();
        if nucleo::en_linea(&ps, linea).is_some() {
            return self.alternar_plegado(linea);
        }
        match nucleo::contienen(&ps, linea).last() {
            Some(&n) => self.plegar(ps[n as usize].linea),
            None => false,
        }
    }

    /// Pliega todos los elementos plegables, en todos los niveles (FR-015). Con enfoque, solo
    /// sus descendientes.
    pub fn plegar_todo(&self) {
        let (_, ps) = self.arbol();
        let lineas = match self.rango_enfocado() {
            Some(r) => {
                let mut v = self.lineas_plegadas();
                for p in ps.iter() {
                    if p.linea > r.start && p.linea < r.end {
                        v.push(p.linea);
                    }
                }
                v
            }
            None => ps.iter().map(|p| p.linea).collect(),
        };
        self.poner_pliegues(lineas, Cursor::Mover);
    }

    /// Despliega todo (con enfoque, solo sus descendientes).
    pub fn desplegar_todo(&self) {
        let lineas = match self.rango_enfocado() {
            Some(r) => self
                .lineas_plegadas()
                .into_iter()
                .filter(|&l| l <= r.start || l >= r.end)
                .collect(),
            None => Vec::new(),
        };
        self.poner_pliegues(lineas, Cursor::Mover);
    }

    /// Despliega los elementos que ocultan la línea `linea` (y los que los contienen) para que se
    /// vea, y sale del enfoque si queda afuera. Devuelve si cambió algo. Lo usa toda navegación
    /// (FR-011, FR-014).
    pub fn revelar_linea(&self, linea: u32) -> bool {
        let mut cambio = false;
        if let Some(r) = self.rango_enfocado()
            && !r.contains(&linea)
        {
            cambio |= self.salir_enfoque();
        }
        if !self.tiene_pliegues() {
            return cambio;
        }
        let (_, ps) = self.arbol();
        let contienen: Vec<u32> = nucleo::contienen(&ps, linea)
            .into_iter()
            .map(|n| ps[n as usize].linea)
            .collect();
        let lineas = self.lineas_plegadas();
        if !contienen.iter().any(|l| lineas.binary_search(l).is_ok()) {
            return cambio;
        }
        self.desplegar_lineas(&contienen);
        true
    }

    /// Como [`Editor::revelar_linea`] para varias líneas, con un solo cambio.
    pub(super) fn revelar_lineas(&self, lineas: &[u32]) -> bool {
        let mut cambio = false;
        if let Some(r) = self.rango_enfocado()
            && lineas.iter().any(|l| !r.contains(l))
        {
            cambio |= self.salir_enfoque();
        }
        if !self.tiene_pliegues() {
            return cambio;
        }
        let (_, ps) = self.arbol();
        let mut quitar: Vec<u32> = lineas
            .iter()
            .flat_map(|&l| nucleo::contienen(&ps, l))
            .map(|n| ps[n as usize].linea)
            .collect();
        quitar.sort_unstable();
        quitar.dedup();
        let plegadas = self.lineas_plegadas();
        if !quitar.iter().any(|l| plegadas.binary_search(l).is_ok()) {
            return cambio;
        }
        self.desplegar_lineas(&quitar);
        true
    }

    pub(super) fn desplegar_lineas(&self, quitar: &[u32]) {
        let lineas: Vec<u32> = self
            .lineas_plegadas()
            .into_iter()
            .filter(|l| !quitar.contains(l))
            .collect();
        self.poner_pliegues(lineas, Cursor::Mover);
    }

    /// Las líneas de los elementos plegados que ocultan algo de `a..b` (offsets de caracteres) o
    /// que una edición en ese tramo uniría a una línea visible.
    fn lineas_ocultas_tocadas(&self, a: i32, b: i32) -> Vec<u32> {
        let i = &self.0;
        let buf = &i.buffer;
        let tag = &i.tag_plegado;
        let (ia, ib) = (buf.iter_at_offset(a), buf.iter_at_offset(b));
        let mut toca = oculto(&ia, tag) || ib.starts_tag(Some(tag));
        if !toca {
            let mut it = ia;
            toca = it.forward_to_tag_toggle(Some(tag)) && it.offset() < b;
        }
        if !toca {
            return Vec::new();
        }
        let (_, ps) = self.arbol();
        let plegadas = self.lineas_plegadas();
        // Los elementos plegados cuyo contenido (o el salto que lo separa) cae en el tramo.
        let desde = ia.line() as u32;
        let hasta = ib.line() as u32 + 1;
        plegadas
            .into_iter()
            .filter(|&l| match nucleo::en_linea(&ps, l) {
                Some(n) => {
                    let p = &ps[n as usize];
                    p.linea <= hasta && p.ultima + 1 >= desde
                }
                None => true,
            })
            .collect()
    }

    /// Antes de un borrado: la marca de un elemento cuya línea pierde su comienzo (el borrado
    /// cruza el comienzo de la línea) ya no es de ese elemento y se borra; si no, la marca caería
    /// en otra línea y podría plegar algo que la usuaria no plegó.
    fn olvidar_marcas_borradas(
        &self,
        buf: &sourceview5::Buffer,
        a: &gtk::TextIter,
        b: &gtk::TextIter,
    ) {
        let (a_off, b_off) = (a.offset(), b.offset());
        let limpio = a.starts_line();
        let mut borrar = Vec::new();
        self.0.pliegues.borrow_mut().retain(|m| {
            let p = buf.iter_at_mark(m).offset();
            let fuera = a_off < p && (p < b_off || (p == b_off && !limpio));
            if fuera {
                borrar.push(m.clone());
            }
            !fuera
        });
        for m in &borrar {
            buf.delete_mark(m);
        }
    }

    /// Deja plegados exactamente los elementos de `lineas` y aplica lo oculto en Código y en
    /// Lectura. Las líneas que no son de un elemento plegable se descartan.
    fn poner_pliegues(&self, mut lineas: Vec<u32>, cursor: Cursor) {
        let i = &self.0;
        let buf = &i.buffer;
        let (texto, ps) = self.arbol();
        lineas.sort_unstable();
        lineas.dedup();
        let mut indices: Vec<u32> = lineas
            .iter()
            .filter_map(|&l| nucleo::en_linea(&ps, l))
            .collect();
        let linea_cursor = buf.iter_at_mark(&buf.get_insert()).line() as u32;
        if cursor == Cursor::Respetar {
            let extra: Vec<u32> = i
                .extra
                .borrow()
                .inserciones(buf)
                .map(|it| it.line() as u32)
                .collect();
            indices.retain(|&n| {
                let r = ps[n as usize].oculta();
                !r.contains(&linea_cursor) && !extra.iter().any(|l| r.contains(l))
            });
        }
        let ocultas = nucleo::ocultas(&ps, &indices);
        let dentro = |l: u32| ocultas.iter().find(|r| r.contains(&l)).cloned();
        if cursor == Cursor::Mover {
            if let Some(r) = dentro(linea_cursor)
                && let Some(mut fin) = buf.iter_at_line(r.start as i32 - 1)
            {
                if !fin.ends_line() {
                    fin.forward_to_line_end();
                }
                buf.place_cursor(&fin);
            }
            // Los cursores extra que quedan adentro se quitan.
            let ocultos: Vec<gtk::TextIter> = i
                .extra
                .borrow()
                .inserciones(buf)
                .filter(|it| dentro(it.line() as u32).is_some())
                .collect();
            if !ocultos.is_empty() {
                let mut extra = i.extra.borrow_mut();
                for it in &ocultos {
                    extra.remove_at(buf, it);
                }
                drop(extra);
                self.actualizar_carenes();
            }
        }
        // Las marcas: una por elemento plegado, al comienzo de su línea. Crear o borrar una marca
        // emite `mark-set`: nada de esto pasa con `pliegues` prestado.
        let viejas = std::mem::take(&mut *i.pliegues.borrow_mut());
        for m in &viejas {
            buf.delete_mark(m);
        }
        let nuevas: Vec<gtk::TextMark> = indices
            .iter()
            .filter_map(|&n| buf.iter_at_line(ps[n as usize].linea as i32))
            .map(|it| buf.create_mark(None, &it, false))
            .collect();
        *i.pliegues.borrow_mut() = nuevas;
        aplicar_ocultas(buf, &i.tag_plegado, &ocultas);
        i.plegado_version.set(i.plegado_version.get() + 1);
        let plegadas: Rc<Vec<u32>> =
            Rc::new(indices.iter().map(|&n| ps[n as usize].linea).collect());
        i.reader.set_plegado(plegadas);
        i.flechas.queue_draw();
        self.avisar_claves(&texto, &ps, &indices);
    }

    /// Avisa a la ventana las claves de lo plegado si cambiaron (research R2).
    fn avisar_claves(&self, texto: &str, ps: &[Plegable], indices: &[u32]) {
        let i = &self.0;
        let claves = nucleo::claves(texto, ps, indices);
        if *i.plegado_claves.borrow() == claves {
            return;
        }
        *i.plegado_claves.borrow_mut() = claves.clone();
        if let (Some(f), Some(ruta)) = (i.on_plegado.borrow().as_ref(), self.current_path()) {
            f(&ruta, claves);
        }
    }

    /// Las claves de lo plegado ahora.
    pub fn claves_plegadas(&self) -> Vec<Clave> {
        self.0.plegado_claves.borrow().clone()
    }

    /// Avisa las claves de lo plegado cada vez que cambian (para guardarlas en la bóveda).
    pub fn set_on_plegado(&self, f: impl Fn(&RelPath, Vec<Clave>) + 'static) {
        *self.0.on_plegado.borrow_mut() = Some(Box::new(f));
    }

    /// Pliega lo guardado para la nota abierta (al cargarla, o cuando llega el estado de la
    /// bóveda). Las claves que no se encuentran no pliegan nada; un elemento que contiene un
    /// cursor no se pliega. No avisa: lo resuelto pasa a ser lo conocido.
    pub fn aplicar_claves(&self, claves: &[Clave]) {
        if !self.plegado_disponible() {
            return;
        }
        let (texto, ps) = self.arbol();
        let indices = nucleo::resolver(&texto, &ps, claves);
        let lineas: Vec<u32> = indices.iter().map(|&n| ps[n as usize].linea).collect();
        // Lo resuelto queda como lo ya avisado: claves perdidas se descartan en el próximo cambio.
        *self.0.plegado_claves.borrow_mut() = nucleo::claves(&texto, &ps, &indices);
        self.poner_pliegues(lineas, Cursor::Respetar);
    }

    /// Al cargar una nota u otro texto entero: sin pliegues.
    pub(super) fn reiniciar_plegado(&self) {
        let i = &self.0;
        let viejas = std::mem::take(&mut *i.pliegues.borrow_mut());
        for m in &viejas {
            i.buffer.delete_mark(m);
        }
        *i.arbol.borrow_mut() = None;
        i.plegado_claves.borrow_mut().clear();
        i.plegado_gen.set(i.plegado_gen.get() + 1);
        i.buffer
            .remove_tag(&i.tag_plegado, &i.buffer.start_iter(), &i.buffer.end_iter());
        i.reader.set_plegado(Rc::new(Vec::new()));
        self.salir_enfoque();
        i.flechas.set_visible(i.format.get() == Format::Md);
    }

    /// Después de una tecla (y al cargar): cuando la usuaria hace una pausa, el árbol se vuelve a
    /// calcular en otro hilo (las flechitas dejan la regla local) y, si hay pliegues, se
    /// reconcilian con el texto. Nada de esto corre por tecla.
    pub(super) fn reconciliar_pronto(&self) {
        self.reconciliar_en(RECONCILIAR);
    }

    pub(super) fn reconciliar_en(&self, pausa: Duration) {
        let i = &self.0;
        if !self.plegado_disponible() {
            return;
        }
        let generacion = i.plegado_gen.get() + 1;
        i.plegado_gen.set(generacion);
        let this = self.clone();
        glib::timeout_add_local_once(pausa, move || {
            if this.0.plegado_gen.get() != generacion {
                return;
            }
            let texto = this.text();
            let gen_texto = this.0.texto_gen.get();
            let that = this.clone();
            glib::spawn_future_local(async move {
                let Ok((texto, ps)) = gio::spawn_blocking(move || {
                    let ps = nucleo::plegables(&texto);
                    (texto, ps)
                })
                .await
                else {
                    return;
                };
                let i = &that.0;
                if i.texto_gen.get() != gen_texto || i.plegado_gen.get() != generacion {
                    return;
                }
                *i.arbol.borrow_mut() = Some(Arbol {
                    generacion: gen_texto,
                    texto: texto.into(),
                    plegables: Rc::new(ps),
                });
                i.flechas.queue_draw();
                that.reconciliar();
            });
        });
    }

    /// Reconciliación inmediata: cada marca con el elemento de su línea; lo que ya no es
    /// plegable o contiene un cursor se despliega.
    pub(super) fn reconciliar(&self) {
        if self.esta_enfocado() {
            self.reconciliar_enfoque();
        }
        if !self.tiene_pliegues() {
            return;
        }
        let lineas = self.lineas_plegadas();
        self.poner_pliegues(lineas, Cursor::Respetar);
    }

    /// Ctrl+Arriba (plegar) y Ctrl+Abajo (desplegar) sobre la línea de un elemento plegable
    /// (decisión 1). En cualquier otra línea no hace nada y la tecla sigue su camino.
    pub(super) fn tecla_plegar(&self, plegar: bool) -> bool {
        if !self.plegado_disponible() {
            return false;
        }
        let b = &self.0.buffer;
        let linea = b.iter_at_mark(&b.get_insert()).line() as u32;
        let (_, ps) = self.arbol();
        if nucleo::en_linea(&ps, linea).is_none() {
            return false;
        }
        if plegar {
            self.plegar(linea);
        } else {
            self.desplegar(linea);
        }
        true
    }

    /// Enter al final de la línea de un elemento plegado (decisión 5): la línea nueva va después
    /// de todo lo oculto, no entre el elemento y su contenido. En un ítem, con el marcador del
    /// hermano siguiente.
    pub(super) fn enter_en_plegado(&self) -> bool {
        let i = &self.0;
        let b = &i.buffer;
        let cursor = b.iter_at_mark(&b.get_insert());
        if !self.tiene_pliegues() || b.has_selection() || !cursor.ends_line() {
            return false;
        }
        let linea = cursor.line() as u32;
        if !self.esta_plegado(linea) {
            return false;
        }
        let (_, ps) = self.arbol();
        let Some(n) = nucleo::en_linea(&ps, linea) else {
            return false;
        };
        let p = &ps[n as usize];
        let texto_linea = {
            let inicio = b.iter_at_line(linea as i32).unwrap_or(cursor);
            b.text(&inicio, &cursor, true).to_string()
        };
        let nuevo = match outliner::item_de_linea(&texto_linea, 0, 0) {
            Some(item) if p.es_item() => {
                format!(
                    "{}{}",
                    " ".repeat(item.indent),
                    outliner::marcador_siguiente(&item)
                )
            }
            _ => String::new(),
        };
        let plegadas = self.lineas_plegadas();
        self.sin_desplegar(|| {
            b.begin_user_action();
            // Al comienzo de la línea que sigue a lo oculto (el borde de la etiqueta: el texto
            // insertado no la hereda) o, al final del texto, después de un salto.
            match b.iter_at_line(p.ultima as i32 + 1) {
                Some(mut en) if en.line() as u32 == p.ultima + 1 => {
                    let marca = b.create_mark(None, &en, true);
                    b.insert(&mut en, &format!("{nuevo}\n"));
                    let mut fin = b.iter_at_mark(&marca);
                    fin.forward_to_line_end();
                    b.place_cursor(&fin);
                    b.delete_mark(&marca);
                }
                _ => {
                    let mut en = b.end_iter();
                    b.insert(&mut en, &format!("\n{nuevo}"));
                    b.place_cursor(&b.end_iter());
                }
            }
            b.end_user_action();
        });
        self.poner_pliegues(plegadas, Cursor::Respetar);
        true
    }

    /// Alt+Arriba/Abajo del outliner con pliegues: el ítem se mueve con sus subítems y lo
    /// plegado de los dos bloques que se intercambian sigue plegado en su lugar nuevo.
    pub(super) fn mover_item_plegado(&self, hacia_arriba: bool) -> bool {
        let b = &self.0.buffer;
        if !self.tiene_pliegues() {
            return outliner::mover_item(b, hacia_arriba);
        }
        let Some(((p1, u1), (h1, hu))) = outliner::plan_mover(b, hacia_arriba) else {
            return false;
        };
        let (largo, largo_h) = (u1 - p1 + 1, hu - h1 + 1);
        let plegadas = self.lineas_plegadas();
        let antes = b.iter_at_mark(&b.get_insert());
        let (linea_cursor, columna) = (antes.line(), antes.line_offset());
        let movido = self.sin_desplegar(|| outliner::mover_item(b, hacia_arriba));
        if !movido {
            return false;
        }
        // El cursor sigue al ítem (si no, queda al final de lo insertado: dentro de lo oculto).
        let destino = linea_cursor + if hacia_arriba { -largo_h } else { largo_h };
        if let Some(mut it) = b.iter_at_line(destino) {
            let mut fin = it;
            if !fin.ends_line() {
                fin.forward_to_line_end();
            }
            it.set_line_offset(columna.min(fin.line_offset()));
            self.sin_desplegar(|| b.place_cursor(&it));
        }
        let nuevas = plegadas
            .into_iter()
            .map(|l| {
                let l = l as i32;
                let d = if (p1..=u1).contains(&l) {
                    if hacia_arriba { -largo_h } else { largo_h }
                } else if (h1..=hu).contains(&l) {
                    if hacia_arriba { largo } else { -largo }
                } else {
                    0
                };
                (l + d) as u32
            })
            .collect();
        self.poner_pliegues(nuevas, Cursor::Respetar);
        true
    }

    /// Tab y Mayús+Tab del outliner con pliegues: la sangría de todo el bloque cambia y lo
    /// plegado sigue plegado (las líneas no se mueven).
    pub(super) fn indentar_plegado(&self, hacia_adentro: bool) -> bool {
        let b = &self.0.buffer;
        if !self.tiene_pliegues() {
            return outliner::indentar(b, hacia_adentro);
        }
        let plegadas = self.lineas_plegadas();
        let hecho = self.sin_desplegar(|| outliner::indentar(b, hacia_adentro));
        if hecho {
            self.poner_pliegues(plegadas, Cursor::Respetar);
        }
        hecho
    }

    /// Corre `f` sin desplegar por las ediciones que hace (ids del guardado, outliner).
    pub(super) fn sin_desplegar<T>(&self, f: impl FnOnce() -> T) -> T {
        let i = &self.0;
        let antes = i.sin_desplegar.replace(true);
        let r = f();
        i.sin_desplegar.set(antes);
        r
    }
}

#[cfg(test)]
#[path = "plegado_tests.rs"]
mod plegado_tests;
