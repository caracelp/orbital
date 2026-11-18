//! Vista previa de fórmulas en Código (050): con el cursor dentro de una fórmula (`$…$`,
//! `$$…$$`, ` ```math `), un globito encima muestra la fórmula dibujada, o el motivo si tiene un
//! error. No toma el foco ni las teclas (como el autocompletado) y se arma recién con la primera
//! fórmula.

use super::*;
use crate::formulas::{self, Clave, Formula, Modo};

/// Pausa después del último movimiento del cursor: escribir no dibuja en cada tecla.
const PAUSA: Duration = Duration::from_millis(160);
/// Renglones alrededor del cursor que se miran (una fórmula o un ` ```math ` más largo no se ve).
const RENGLONES: i32 = 200;
/// Las fórmulas del globito, un poco más grandes que el texto de Código.
const AUMENTO: f32 = 1.3;

/// El globito.
pub(super) struct VistaPrevia {
    popover: gtk::Popover,
    imagen: gtk::Picture,
    error: gtk::Label,
    /// La fórmula que se está mostrando (LaTeX, modo) y el inicio en el buffer.
    actual: RefCell<Option<(String, Modo, i32)>>,
}

impl VistaPrevia {
    fn new(parent: &sourceview5::View) -> Self {
        let imagen = gtk::Picture::builder()
            .can_shrink(false)
            .margin_top(6)
            .margin_bottom(6)
            .margin_start(8)
            .margin_end(8)
            .build();
        let error = gtk::Label::builder()
            .wrap(true)
            .max_width_chars(48)
            .xalign(0.0)
            .margin_top(6)
            .margin_bottom(6)
            .margin_start(8)
            .margin_end(8)
            .css_classes(["dim-label"])
            .build();
        let caja = gtk::Box::new(gtk::Orientation::Vertical, 0);
        caja.append(&imagen);
        caja.append(&error);
        let popover = gtk::Popover::builder()
            .has_arrow(true)
            // Sin autohide y sin foco: si no, el globito se queda con el teclado.
            .autohide(false)
            .can_focus(false)
            .position(gtk::PositionType::Top)
            .css_classes(["orbital-formula-popover"])
            .child(&caja)
            .build();
        popover.update_property(&[gtk::accessible::Property::Label(
            "Vista previa de la fórmula",
        )]);
        popover.set_parent(parent);
        Self {
            popover,
            imagen,
            error,
            actual: RefCell::new(None),
        }
    }

    fn ocultar(&self) {
        self.actual.replace(None);
        if self.popover.is_visible() {
            self.popover.popdown();
        }
    }
}

impl Drop for VistaPrevia {
    fn drop(&mut self) {
        self.popover.unparent();
    }
}

impl Editor {
    /// El cursor se movió: la vista previa se revisa después de una pausa.
    pub(super) fn programar_vista_previa(&self) {
        let i = &self.0;
        let generacion = i.vista_previa_gen.get() + 1;
        i.vista_previa_gen.set(generacion);
        let this = self.clone();
        glib::timeout_add_local_once(PAUSA, move || {
            if this.0.vista_previa_gen.get() == generacion {
                this.actualizar_vista_previa();
            }
        });
    }

    /// Esconde el globito (cambio de vista o de nota).
    pub(super) fn ocultar_vista_previa(&self) {
        if let Some(v) = self.0.vista_previa.borrow().as_ref() {
            v.ocultar();
        }
    }

    /// La fórmula bajo el cursor, si hay: (inicio en el buffer, LaTeX, modo).
    fn formula_en_cursor(&self) -> Option<(i32, String, Modo)> {
        let i = &self.0;
        let b = &i.buffer;
        let cursor = b.iter_at_offset(b.cursor_position());
        let linea = cursor.line();
        let desde = b.iter_at_line((linea - RENGLONES).max(0))?;
        let mut hasta = b
            .iter_at_line(linea + RENGLONES)
            .unwrap_or_else(|| b.end_iter());
        if !hasta.ends_line() {
            hasta.forward_to_line_end();
        }
        let ventana = b.text(&desde, &hasta, true);
        // Sin `$` ni cercas no hay nada que buscar (lo común al escribir).
        if !ventana.contains('$') && !ventana.contains("math") {
            return None;
        }
        let antes = b.text(&desde, &cursor, true);
        let offset = antes.len();
        let (rango, tex, modo) = match i.format.get() {
            Format::Html => orbital_formulas::html::formula_en(&ventana, offset)?,
            _ => {
                let f = orbital_vault::formulas::formula_en(&ventana, offset)?;
                let modo = if f.bloque {
                    Modo::Bloque
                } else {
                    Modo::EnLinea
                };
                (f.rango, f.tex, modo)
            }
        };
        if tex.trim().is_empty() {
            return None;
        }
        let inicio = desde.offset() + ventana[..rango.start].chars().count() as i32;
        Some((inicio, tex, modo))
    }

    fn actualizar_vista_previa(&self) {
        let i = &self.0;
        let activa = i.view.has_focus()
            && matches!(i.mode.get(), View::Codigo | View::Dividida)
            && !i.loading.get();
        let hallada = activa.then(|| self.formula_en_cursor()).flatten();
        let Some((inicio, tex, modo)) = hallada else {
            self.ocultar_vista_previa();
            return;
        };
        if i.vista_previa.borrow().is_none() {
            i.vista_previa.replace(Some(VistaPrevia::new(&i.view)));
        }
        let prestamo = i.vista_previa.borrow();
        let Some(v) = prestamo.as_ref() else { return };
        // El globito, sobre el comienzo de la fórmula.
        let it = i.buffer.iter_at_offset(inicio);
        let loc = i.view.iter_location(&it);
        let (wx, wy) =
            i.view
                .buffer_to_window_coords(gtk::TextWindowType::Widget, loc.x(), loc.y());
        v.popover.set_pointing_to(Some(&gtk::gdk::Rectangle::new(
            wx,
            wy,
            1,
            loc.height().max(1),
        )));
        let misma = v
            .actual
            .borrow()
            .as_ref()
            .is_some_and(|(t, m, _)| *t == tex && *m == modo);
        if misma && v.popover.is_visible() {
            return;
        }
        v.actual.replace(Some((tex.clone(), modo, inicio)));
        let mut estilo = formulas::Estilo::de(&i.view).por(AUMENTO);
        let c = crate::style::TEXT_FG;
        estilo.color = [c.0, c.1, c.2, 255];
        let clave = Clave::new(&tex, modo, &estilo);
        match formulas::en_cache(&clave) {
            Some(r) => self.mostrar_vista_previa(&tex, modo, r),
            None => {
                let this = self.clone();
                glib::spawn_future_local(async move {
                    let Ok(r) = gio::spawn_blocking(move || formulas::dibujar(&clave)).await else {
                        return;
                    };
                    this.mostrar_vista_previa(&tex, modo, r);
                });
            }
        }
    }

    fn mostrar_vista_previa(&self, tex: &str, modo: Modo, r: formulas::Resultado) {
        let prestamo = self.0.vista_previa.borrow();
        let Some(v) = prestamo.as_ref() else { return };
        // El cursor ya se fue a otra fórmula mientras se dibujaba.
        if !v
            .actual
            .borrow()
            .as_ref()
            .is_some_and(|(t, m, _)| t == tex && *m == modo)
        {
            return;
        }
        match r {
            Ok(d) => {
                v.imagen.set_paintable(Some(&Formula::lista(&d)));
                v.imagen
                    .set_alternative_text(Some(&orbital_formulas::lectura(tex)));
                v.imagen.set_visible(true);
                v.error.set_visible(false);
            }
            Err(e) => {
                v.imagen.set_paintable(None::<&gtk::gdk::Paintable>);
                v.imagen.set_visible(false);
                v.error.set_label(&e.mensaje);
                v.error.set_visible(true);
            }
        }
        v.popover.popup();
    }
}
