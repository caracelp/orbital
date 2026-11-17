//! Del punto del mouse a una posición del texto sin el aborto de GTK 4.24.
//!
//! `gtk_text_view_get_iter_at_location` (y los manejadores propios de `GtkTextView`: clic, arrastre
//! de selección) abortan el proceso cuando el punto cae **debajo del texto de un renglón que tiene
//! algo invisible**: en el espacio de `pixels-below-lines`. Para ese caso GTK toma la cantidad de
//! bytes del renglón *con* lo invisible y la usa como índice *visible*
//! (`gtk_text_iter_set_visible_line_index`), que se pasa al renglón siguiente y termina en
//! `g_error ("Byte index … is off the end of the line")`. La Lectura tiene espacio debajo de cada
//! párrafo y texto invisible en lo plegado, los callouts plegables y las respuestas de las tarjetas:
//! bastaba con pasar el mouse entre dos renglones.
//!
//! [`iter_en`] reemplaza a `iter_at_location` en los manejadores propios, y [`proteger`] le saca a
//! la vista los clics y arrastres que caen en esa franja antes de que los vea GTK.

use gtk::prelude::*;

/// Si el carácter en `it` está oculto: manda la etiqueta de mayor prioridad que fija `invisible`
/// (`tags()` viene de menor a mayor prioridad).
pub fn es_invisible(it: &gtk::TextIter) -> bool {
    it.tags()
        .iter()
        .rev()
        .find(|t| t.is_invisible_set())
        .is_some_and(|t| t.is_invisible())
}

/// Lo que hay en el punto `(bx, by)` del buffer.
pub enum Punto {
    /// Sobre el texto del renglón (lo que devolvería GTK).
    Texto(gtk::TextIter),
    /// Debajo del texto del renglón, en su margen inferior; `peligroso` si el renglón tiene algo
    /// invisible (ahí GTK aborta).
    Debajo { peligroso: bool },
    /// Fuera del texto (a la izquierda o la derecha del renglón, o después del final).
    Fuera,
}

/// Clasifica el punto `(bx, by)` (coordenadas del buffer) sin llamar a GTK en el caso que aborta.
pub fn punto(view: &gtk::TextView, bx: i32, by: i32) -> Punto {
    let (inicio, _) = view.line_at_y(by);
    let mut fin = inicio;
    if !fin.ends_line() {
        fin.forward_to_line_end();
    }
    // El final del texto dibujado del renglón: el cursor en su última posición visible.
    let ultimo = if es_invisible(&fin) {
        let mut v = fin;
        if !v.backward_visible_cursor_position() || v.line() != inicio.line() {
            // Todo el renglón está oculto: no hay texto en el punto.
            return Punto::Fuera;
        }
        Some(v)
    } else {
        None
    };
    // El cursor tiene la altura del renglón dibujado (el rectángulo del salto de línea mide 0).
    let (r, _) = view.cursor_locations(Some(ultimo.as_ref().unwrap_or(&fin)));
    if by >= r.y() + r.height() {
        // En el margen inferior. Solo es peligroso si el renglón tiene algo invisible.
        let peligroso = ultimo.is_some() || tiene_invisible(&inicio, &fin);
        return Punto::Debajo { peligroso };
    }
    match view.iter_at_location(bx, by) {
        Some(it) => Punto::Texto(it),
        None => Punto::Fuera,
    }
}

/// Como `iter_at_location`, sin el aborto: `None` fuera del texto o debajo de un renglón.
pub fn iter_en(view: &gtk::TextView, bx: i32, by: i32) -> Option<gtk::TextIter> {
    match punto(view, bx, by) {
        Punto::Texto(it) => Some(it),
        _ => None,
    }
}

/// El renglón (de `inicio` a `fin`) tiene algún tramo invisible.
fn tiene_invisible(inicio: &gtk::TextIter, fin: &gtk::TextIter) -> bool {
    if es_invisible(inicio) {
        return true;
    }
    let mut it = *inicio;
    // Solo cambia la visibilidad donde empieza o termina una etiqueta.
    // Corre solo con el mouse en el margen de abajo de un renglón.
    while it.forward_to_tag_toggle(None::<&gtk::TextTag>) && it < *fin {
        if es_invisible(&it) {
            return true;
        }
    }
    false
}

/// Pasa coordenadas de la vista (`Widget`) a las del buffer.
pub fn al_buffer(view: &gtk::TextView, x: f64, y: f64) -> (i32, i32) {
    view.window_to_buffer_coords(gtk::TextWindowType::Widget, x as i32, y as i32)
}

/// Le saca a la vista los clics y los arrastres de selección que caen debajo de un renglón con
/// algo invisible: ahí los manejadores de `GtkTextView` abortan. Un controlador en la fase de
/// captura frena el evento antes de que llegue a sus gestos.
pub fn proteger(view: &gtk::TextView) {
    let legado = gtk::EventControllerLegacy::new();
    legado.set_propagation_phase(gtk::PropagationPhase::Capture);
    legado.connect_event(|c, ev| {
        use gtk::gdk::EventType as T;
        let boton = match ev.event_type() {
            T::ButtonPress | T::TouchBegin => true,
            // El movimiento solo importa con un botón apretado (arrastre de selección).
            T::MotionNotify | T::TouchUpdate => ev.modifier_state().intersects(
                gtk::gdk::ModifierType::BUTTON1_MASK
                    | gtk::gdk::ModifierType::BUTTON2_MASK
                    | gtk::gdk::ModifierType::BUTTON3_MASK,
            ),
            _ => false,
        };
        if !boton {
            return gtk::glib::Propagation::Proceed;
        }
        let Some(view) = c.widget().and_downcast::<gtk::TextView>() else {
            return gtk::glib::Propagation::Proceed;
        };
        let Some((x, y)) = posicion_en(&view, ev) else {
            return gtk::glib::Propagation::Proceed;
        };
        let (bx, by) = al_buffer(&view, x, y);
        if matches!(punto(&view, bx, by), Punto::Debajo { peligroso: true }) {
            gtk::glib::Propagation::Stop
        } else {
            gtk::glib::Propagation::Proceed
        }
    });
    view.add_controller(legado);
}

/// La posición de un evento en coordenadas del widget (los eventos crudos vienen en las de la
/// superficie).
fn posicion_en(w: &gtk::TextView, ev: &gtk::gdk::Event) -> Option<(f64, f64)> {
    let (x, y) = ev.position()?;
    let native = w.native()?;
    let (sx, sy) = native.surface_transform();
    let p = native.compute_point(
        w,
        &gtk::graphene::Point::new((x - sx) as f32, (y - sy) as f32),
    )?;
    Some((f64::from(p.x()), f64::from(p.y())))
}

#[cfg(test)]
#[path = "posicion_tests.rs"]
mod posicion_tests;
