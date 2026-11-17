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

/// Lo que importa de una línea para la regla local, leído del buffer sin copiar el texto.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Linea {
    Vacia,
    Encabezado(u8),
    Item(u32),
    Otra(u32),
}

/// Mira el comienzo de la línea `n` con un iterador (sin asignar memoria: corre por línea visible
/// en cada cuadro mientras se escribe).
fn mirar(b: &sourceview5::Buffer, n: u32) -> Option<Linea> {
    let mut it = b.iter_at_line(n as i32)?;
    if it.line() as u32 != n {
        return None;
    }
    let mut sangria = 0u32;
    while !it.ends_line() {
        match it.char() {
            ' ' => sangria += 1,
            '\t' => sangria += 4,
            _ => break,
        }
        it.forward_char();
    }
    if it.ends_line() {
        return Some(Linea::Vacia);
    }
    let sigue_espacio = |mut x: gtk::TextIter| x.forward_char() && x.char() == ' ';
    match it.char() {
        '#' if sangria <= 3 => {
            let mut k = 0u8;
            while it.char() == '#' && k < 7 {
                k += 1;
                it.forward_char();
            }
            Some(
                if (1..=6).contains(&k) && (it.ends_line() || it.char() == ' ') {
                    Linea::Encabezado(k)
                } else {
                    Linea::Otra(sangria)
                },
            )
        }
        '-' | '*' | '+' if sigue_espacio(it) => Some(Linea::Item(sangria)),
        c if c.is_ascii_digit() => {
            while it.char().is_ascii_digit() {
                it.forward_char();
            }
            Some(if matches!(it.char(), '.' | ')') && sigue_espacio(it) {
                Linea::Item(sangria)
            } else {
                Linea::Otra(sangria)
            })
        }
        _ => Some(Linea::Otra(sangria)),
    }
}

/// Lo oculto entre la primera y la última línea del cuadro que se dibuja.
struct Cuadro {
    /// Primera y última línea, generación del texto y versión de lo plegado.
    clave: (u32, u32, u64, u64),
    ocultas: Vec<std::ops::Range<u32>>,
}

impl Cuadro {
    fn oculta(&self, linea: u32) -> bool {
        let n = self.ocultas.partition_point(|r| r.end <= linea);
        self.ocultas.get(n).is_some_and(|r| r.start <= linea)
    }
}

/// Las dos imágenes, cargadas una vez: cambiar el nombre del ícono rehace la imagen en cada línea.
fn imagenes(widget: &impl IsA<gtk::Widget>) -> (gtk::IconPaintable, gtk::IconPaintable) {
    let tema = gtk::IconTheme::for_display(&widget.display());
    let escala = widget.scale_factor();
    let cargar = |nombre: &str| {
        tema.lookup_icon(
            nombre,
            &[],
            16,
            escala,
            gtk::TextDirection::None,
            gtk::IconLookupFlags::empty(),
        )
    };
    (cargar("pan-down-symbolic"), cargar("pan-end-symbolic"))
}

/// La línea del elemento plegable más interno que contiene `linea` (la suya o su contenido).
fn plegable_bajo(ps: &[nucleo::Plegable], linea: u32) -> Option<u32> {
    let n = ps.partition_point(|p| p.linea <= linea).checked_sub(1)?;
    let mut p = &ps[n];
    loop {
        if p.ultima >= linea {
            return Some(p.linea);
        }
        p = &ps[p.padre? as usize];
    }
}

impl Editor {
    pub(super) fn setup_flechas(&self) {
        let i = &self.0;
        let r = &i.flechas;
        sourceview5::prelude::ViewExt::gutter(&i.view, gtk::TextWindowType::Left)
            .insert(r, POSICION);
        let ultima = Rc::new(Cell::new(Flecha::Ninguna));
        let cargadas: Rc<RefCell<Option<(gtk::IconPaintable, gtk::IconPaintable)>>> = Rc::default();
        // Lo oculto del tramo dibujado, una vez por cuadro: con mucho plegado, lo visible abarca
        // miles de líneas ocultas y GtkSourceView consulta el canal por cada una.
        let cuadro: Rc<RefCell<Option<Cuadro>>> = Rc::default();
        let weak = Rc::downgrade(&self.0);
        // La flechita de un elemento desplegado aparece solo con el mouse sobre él (su línea o
        // su contenido), como sugerencia; la de uno plegado se ve siempre. Se calcula una vez por
        // movimiento (el elemento más interno bajo el mouse), no por línea dibujada.
        let hover: Rc<Cell<Option<u32>>> = Rc::default();
        let motion = gtk::EventControllerMotion::new();
        let weak_motion = Rc::downgrade(&self.0);
        let h = hover.clone();
        motion.connect_motion(move |_, x, y| {
            let Some(inner) = weak_motion.upgrade() else {
                return;
            };
            let (bx, by) =
                inner
                    .view
                    .window_to_buffer_coords(gtk::TextWindowType::Widget, x as i32, y as i32);
            let nuevo = inner.view.iter_at_location(bx, by).and_then(|it| {
                let arbol = inner.arbol.borrow();
                plegable_bajo(&arbol.as_ref()?.plegables, it.line() as u32)
            });
            if h.replace(nuevo) != nuevo {
                inner.flechas.queue_draw();
            }
        });
        let weak_leave = Rc::downgrade(&self.0);
        let h = hover.clone();
        motion.connect_leave(move |_| {
            if h.take().is_some()
                && let Some(inner) = weak_leave.upgrade()
            {
                inner.flechas.queue_draw();
            }
        });
        i.view.add_controller(motion);
        // Gris suave, como el resto de lo secundario.
        r.set_opacity(0.55);

        r.connect_query_data(move |r, lineas, linea| {
            let Some(i) = weak.upgrade() else { return };
            let e = Editor(i);
            let mut f = match lineas.downcast_ref::<sourceview5::GutterLines>() {
                Some(l) if e.tiene_pliegues() => {
                    let mut c = cuadro.borrow_mut();
                    let clave = (
                        l.first(),
                        l.last(),
                        e.0.texto_gen.get(),
                        e.0.plegado_version.get(),
                    );
                    if c.as_ref().is_none_or(|c| c.clave != clave) {
                        *c = Some(Cuadro {
                            clave,
                            ocultas: plegado::rangos_entre(
                                &e.0.buffer,
                                &e.0.tag_plegado,
                                l.first(),
                                l.last(),
                            ),
                        });
                    }
                    let c = c.as_ref().expect("recién calculado");
                    if c.oculta(linea) {
                        Flecha::Ninguna
                    } else if l.last() > linea {
                        e.flecha_con(linea, Some(c.oculta(linea + 1)))
                    } else {
                        e.flecha(linea)
                    }
                }
                _ => e.flecha(linea),
            };
            if f == Flecha::Abierta && hover.get() != Some(linea) {
                f = Flecha::Ninguna;
            }
            // El renderer es uno para todas las líneas: se cambia la imagen solo si cambia.
            if f != ultima.get() {
                ultima.set(f);
                let mut c = cargadas.borrow_mut();
                let (abierta, cerrada) = c.get_or_insert_with(|| imagenes(r));
                match f {
                    Flecha::Ninguna => r.set_paintable(None::<&gtk::IconPaintable>),
                    Flecha::Abierta => r.set_paintable(Some(&*abierta)),
                    Flecha::Cerrada => r.set_paintable(Some(&*cerrada)),
                }
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
        self.flecha_con(linea, None)
    }

    /// Con `siguiente`, ya se sabe si la línea siguiente está oculta.
    fn flecha_con(&self, linea: u32, siguiente: Option<bool>) -> Flecha {
        if !self.plegado_disponible() {
            return Flecha::Ninguna;
        }
        let plegable = match self.arbol_al_dia() {
            Some(ps) => nucleo::en_linea(&ps, linea).is_some(),
            None => self.plegable_local(linea),
        };
        if !plegable {
            Flecha::Ninguna
        } else if self.tiene_pliegues() && siguiente.unwrap_or_else(|| self.siguiente_oculta(linea))
        {
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

    /// Mientras se escribe (el árbol no está al día): encabezado con algo debajo antes del próximo
    /// de nivel ≤, o ítem con un ítem de más sangría debajo (saltando líneas vacías).
    fn plegable_local(&self, linea: u32) -> bool {
        let b = &self.0.buffer;
        let propia = match mirar(b, linea) {
            Some(l @ (Linea::Encabezado(_) | Linea::Item(_))) => l,
            _ => return false,
        };
        for k in 1..=MIRAR {
            match (propia, mirar(b, linea + k)) {
                (_, None) => return false,
                (_, Some(Linea::Vacia)) => continue,
                (Linea::Encabezado(n), Some(Linea::Encabezado(m))) => return m > n,
                (Linea::Encabezado(_), Some(_)) => return true,
                (Linea::Item(s), Some(Linea::Item(t))) => return t > s,
                _ => return false,
            }
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plegable_bajo_el_mouse_es_el_mas_interno() {
        let texto = "# A\n\ntexto\n\n## B\n\n- uno\n  - dos\n- tres\n\n# C\n";
        let ps = nucleo::plegables(texto);
        // En el texto de A, A; en B o su lista, B; en el ítem con subítem, el ítem.
        assert_eq!(plegable_bajo(&ps, 2), Some(0));
        assert_eq!(plegable_bajo(&ps, 4), Some(4));
        assert_eq!(plegable_bajo(&ps, 6), Some(6));
        assert_eq!(plegable_bajo(&ps, 7), Some(6));
        assert_eq!(plegable_bajo(&ps, 8), Some(4));
        // C no tiene nada abajo: no es plegable ni está dentro de otro.
        assert_eq!(plegable_bajo(&ps, 10), None);
    }
}
