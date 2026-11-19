//! Números de línea en Código (054).
//!
//! Un `GutterRendererText` propio en la posición −30 del canal izquierdo (a la izquierda de las
//! flechitas del plegado, que van en la 10), y no el de `show-line-numbers` de GtkSourceView: ese
//! no avisa por línea, y una línea plegada (028) o fuera del elemento enfocado es texto invisible
//! que queda a la altura de la siguiente que se ve; su número se dibujaría encima del de esa. Acá
//! esas líneas no llevan número, y la del cursor va resaltada.
//!
//! Por línea dibujada: el número escrito en un `String` que se reutiliza. Con algo plegado o
//! enfocado, lo oculto del tramo dibujado se calcula una vez por cuadro, como en el canal de las
//! flechitas. El ancho lo maneja `code::center_column`.

use super::*;
use std::fmt::Write as _;
use std::ops::Range;

/// Posición del renderer en el canal izquierdo (la de los números de GtkSourceView).
const POSICION: i32 = -30;

/// Lo oculto entre la primera y la última línea del cuadro que se dibuja.
struct Cuadro {
    /// Primera y última línea, generación del texto y versión de lo oculto.
    clave: (u32, u32, u64, u64),
    plegadas: Vec<Range<u32>>,
    fuera: Vec<Range<u32>>,
}

/// `linea` cae en alguno de los rangos (ordenados y sin solaparse).
fn adentro(rangos: &[Range<u32>], linea: u32) -> bool {
    let n = rangos.partition_point(|r| r.end <= linea);
    rangos.get(n).is_some_and(|r| r.start <= linea)
}

/// El renderer, todavía sin vista.
pub(super) fn nuevo() -> sourceview5::GutterRendererText {
    let r = sourceview5::GutterRendererText::new();
    // Pegados a la derecha (junto a las flechitas o al texto) y a la altura del primer renglón
    // de una línea que se envuelve, como las flechitas.
    r.set_xalign(1.0);
    r.set_xpad(code::RELLENO_NUMEROS);
    r.set_alignment_mode(sourceview5::GutterRendererAlignmentMode::First);
    // Atenuados como el resto de lo secundario; la línea del cursor, a pleno.
    r.set_opacity(0.55);
    r.upcast_ref::<gtk::Widget>()
        .update_property(&[gtk::accessible::Property::Label("Números de línea")]);
    r
}

impl Editor {
    pub(super) fn setup_numeros(&self) {
        let i = &self.0;
        let r = &i.numeros;
        sourceview5::prelude::ViewExt::gutter(&i.view, gtk::TextWindowType::Left)
            .insert(r, POSICION);
        let weak = Rc::downgrade(&self.0);
        let cuadro: RefCell<Option<Cuadro>> = RefCell::new(None);
        let texto = RefCell::new(String::with_capacity(16));
        r.connect_query_data(move |r, lineas, linea| {
            let Some(i) = weak.upgrade() else { return };
            let Some(lineas) = lineas.downcast_ref::<sourceview5::GutterLines>() else {
                return;
            };
            let enfocado = i.enfoque.borrow().is_some();
            if enfocado || !i.pliegues.borrow().is_empty() {
                let mut c = cuadro.borrow_mut();
                let clave = (
                    lineas.first(),
                    lineas.last(),
                    i.texto_gen.get(),
                    i.plegado_version.get(),
                );
                if c.as_ref().is_none_or(|c| c.clave != clave) {
                    let (desde, hasta) = (clave.0, clave.1);
                    *c = Some(Cuadro {
                        clave,
                        plegadas: plegado::rangos_entre(&i.buffer, &i.tag_plegado, desde, hasta),
                        fuera: if enfocado {
                            plegado::rangos_entre(&i.buffer, &i.tag_fuera, desde, hasta)
                        } else {
                            Vec::new()
                        },
                    });
                }
                let c = c.as_ref().expect("recién calculado");
                if adentro(&c.plegadas, linea) || adentro(&c.fuera, linea) {
                    r.set_text("");
                    return;
                }
            }
            let mut t = texto.borrow_mut();
            t.clear();
            if lineas.is_cursor(linea) {
                let _ = write!(
                    t,
                    "<span foreground=\"{}\" weight=\"bold\">{}</span>",
                    crate::style::SYNTAX_KEYWORD.hex(),
                    linea + 1
                );
                r.set_markup(&t);
            } else {
                let _ = write!(t, "{}", linea + 1);
                r.set_text(&t);
            }
        });
    }
}
