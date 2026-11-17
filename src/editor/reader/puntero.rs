//! El mouse sobre la vista formateada: qué hay bajo el puntero, la forma del puntero y los clics
//! que pliegan.
//!
//! Plegar con el mouse no pide puntería: la franja de la flechita (del borde de la columna hasta
//! el texto) cuenta a la altura de todo el renglón, y en un callout plegable todo el renglón del
//! título. Esos clics se toman al apretar y en la fase de captura, antes que los gestos de
//! `GtkTextView`: no mueven el cursor ni empiezan una selección.
//!
//! El puntero es una mano sobre lo que se acciona (enlaces, etiquetas, píldoras, «Mostrar
//! respuesta», flechitas, títulos plegables), la barra de texto sobre el texto y la flecha en lo
//! demás. Se cambia solo cuando cambia la zona.
//!
//! Todas las posiciones pasan por `editor::posicion`: con `iter_at_location`, GTK 4.24 aborta
//! debajo de un renglón con algo invisible (el botón de una tarjeta, lo plegado).

use super::*;
use crate::editor::posicion::{self, Punto};

/// Cuánto se extiende la zona de la flechita a su izquierda (hacia el borde de la columna).
const FRANJA_IZQ: i32 = 22;
/// Y a su derecha, hasta el comienzo del texto.
const FRANJA_DER: i32 = 6;

/// La forma del puntero.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Zona {
    /// Flecha normal: márgenes, espacio entre renglones, widgets anclados.
    Nada,
    /// Barra de texto: texto seleccionable.
    Texto,
    /// Mano: algo que se acciona con un clic.
    Mano,
}

impl Zona {
    fn cursor(self) -> &'static str {
        match self {
            Zona::Nada => "default",
            Zona::Texto => "text",
            Zona::Mano => "pointer",
        }
    }
}

/// Lo que hace un clic en un lugar de la vista, si es de plegado.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Accion {
    /// La flechita de un elemento plegable (su línea fuente).
    Plegar(u32),
    /// El título de un callout plegable (la URL interna que lo alterna).
    Callout(String),
}

impl Reader {
    /// Conecta el movimiento y el clic de plegado (una vez, al crear la vista).
    pub(super) fn setup_puntero(&self) {
        let view = &self.0.view;
        let motion = gtk::EventControllerMotion::new();
        let weak = Rc::downgrade(&self.0);
        motion.connect_motion(move |_, x, y| {
            if let Some(i) = weak.upgrade() {
                Reader(i).mover_mouse(x, y);
            }
        });
        let weak = Rc::downgrade(&self.0);
        motion.connect_leave(move |_| {
            if let Some(i) = weak.upgrade() {
                if i.hover_plegable.take().is_some() {
                    i.view.queue_draw();
                }
                i.zona.set(None);
            }
        });
        view.add_controller(motion);

        let clic = gtk::GestureClick::new();
        clic.set_button(gtk::gdk::BUTTON_PRIMARY);
        clic.set_propagation_phase(gtk::PropagationPhase::Capture);
        let weak = Rc::downgrade(&self.0);
        clic.connect_pressed(move |g, _, x, y| {
            let Some(i) = weak.upgrade() else { return };
            if Reader(i).clic_de_plegado(x, y) {
                g.set_state(gtk::EventSequenceState::Claimed);
            }
        });
        view.add_controller(clic);
        posicion::proteger(view);
    }

    /// El mouse en `(x, y)` (coordenadas de la vista): la flechita de lo que está debajo como
    /// sugerencia y la forma del puntero.
    pub(crate) fn mover_mouse(&self, x: f64, y: f64) {
        let i = &self.0;
        let (bx, by) = posicion::al_buffer(&i.view, x, y);
        let punto = posicion::punto(&i.view, bx, by);
        let (linea, _) = i.view.line_at_y(by);
        let bajo = match &punto {
            Punto::Texto(it) => Some(*it),
            _ => None,
        };
        // Fuera del texto (el margen de la flechita, el espacio entre renglones) vale el renglón.
        let off = bajo.map_or(linea.offset(), |it| it.offset());
        let nueva = self.plegable_en(off);
        if i.hover_plegable.replace(nueva) != nueva {
            i.view.queue_draw();
        }
        let zona = if self.accion_en(bx, by).is_some() || bajo.is_some_and(|it| self.es_enlace(&it))
        {
            Zona::Mano
        } else if bajo.is_some_and(|it| it.char() != '\u{fffc}') {
            Zona::Texto
        } else {
            Zona::Nada
        };
        if i.zona.replace(Some(zona)) != Some(zona) {
            i.view.set_cursor_from_name(Some(zona.cursor()));
        }
    }

    /// La zona del puntero ahora (para las pruebas).
    #[cfg(test)]
    pub(crate) fn zona(&self) -> Option<Zona> {
        self.0.zona.get()
    }

    /// Un clic en `(x, y)` (coordenadas de la vista): si cae en una flechita o en el título de
    /// un callout plegable, lo alterna y devuelve `true`.
    pub(crate) fn clic_de_plegado(&self, x: f64, y: f64) -> bool {
        let (bx, by) = posicion::al_buffer(&self.0.view, x, y);
        match self.accion_en(bx, by) {
            Some(Accion::Plegar(linea)) => {
                self.set_focused_link(None);
                self.plegar(linea);
                true
            }
            Some(Accion::Callout(url)) => {
                self.set_focused_link(None);
                self.alternar_callout(&url)
            }
            None => false,
        }
    }

    /// El tag de un enlace (o de una etiqueta, una píldora, «Mostrar respuesta»…) bajo `it`.
    fn es_enlace(&self, it: &gtk::TextIter) -> bool {
        let links = self.0.links.borrow();
        !links.is_empty() && it.tags().iter().any(|t| links.contains_key(t))
    }

    /// Qué haría un clic de plegado en `(bx, by)` (coordenadas del buffer).
    pub(crate) fn accion_en(&self, bx: i32, by: i32) -> Option<Accion> {
        let (inicio, _) = self.0.view.line_at_y(by);
        if let Some((linea, r)) = self.flecha_del_renglon(&inicio)
            && bx >= r.x() - FRANJA_IZQ
            && bx < r.x() + r.width() + FRANJA_DER
        {
            return Some(Accion::Plegar(linea));
        }
        // El renglón del título de un callout plegable: su etiqueta de enlace interno.
        let table = self.buffer().tag_table();
        let titulo = table.lookup("callout-title")?;
        if !inicio.has_tag(&titulo) {
            return None;
        }
        let links = self.0.links.borrow();
        inicio
            .tags()
            .iter()
            .filter_map(|t| links.get(t))
            .find(|u| u.starts_with(super::bloques::URL_CALLOUT))
            .cloned()
            .map(Accion::Callout)
    }

    /// La flechita del plegado que hay en el renglón de `inicio`, con su rectángulo.
    fn flecha_del_renglon(&self, inicio: &gtk::TextIter) -> Option<(u32, gtk::gdk::Rectangle)> {
        let buf = self.buffer();
        let blocks = self.0.blocks.borrow();
        let off = inicio.offset();
        let o = |m: &gtk::TextMark| buf.iter_at_mark(m).offset();
        let n = blocks
            .partition_point(|b| o(&b.mark) <= off)
            .checked_sub(1)?;
        let renglon = inicio.line();
        // El glifo va al comienzo del renglón, en este bloque o en el que empieza en él. Los
        // plegables de un bloque están en el orden del texto: el primero desde `off`.
        for b in blocks[n..].iter().take(2) {
            let k = b.plegables.partition_point(|p| o(&p.glifo) < off);
            if let Some(p) = b.plegables.get(k) {
                let g = buf.iter_at_mark(&p.glifo);
                if g.line() == renglon && !posicion::es_invisible(&g) {
                    return Some((p.linea, self.0.view.iter_location(&g)));
                }
            }
        }
        None
    }
}

#[cfg(test)]
#[path = "puntero_tests.rs"]
mod puntero_tests;
