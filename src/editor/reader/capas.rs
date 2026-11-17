//! Lo que la vista formateada dibuja a mano debajo y encima del texto: el fondo redondeado y el
//! ícono de los callouts, las tarjetas de las transclusiones y de estudio, las guías de las
//! listas anidadas y las flechitas del plegado.
//!
//! Nada de esto son widgets ni etiquetas que cambien la disposición: el texto solo deja el lugar
//! (márgenes y espacios) y cada cuadro se dibuja lo visible, ubicado con `line_yrange` e
//! `iter_location`. Mover el mouse cambia una celda y pide un cuadro, sin tocar el buffer.

use super::*;
use crate::style::Rgb;
use gtk::subclass::prelude::*;
use gtk::{graphene, gsk};

/// Espacio entre un callout y lo que lo rodea (afuera del fondo).
pub(super) const CALLOUT_SEPARACION: i32 = 8;
/// Relleno arriba del título, dentro del fondo.
pub(super) const CALLOUT_RELLENO: i32 = 10;
/// Relleno izquierdo: el cuerpo (sangría de 24 por nivel) queda alineado con el ícono.
const CALLOUT_ICONO_X: f32 = 24.0;
/// Lado del ícono del título.
const CALLOUT_ICONO: f32 = 17.0;
/// Sangría del título: relleno, ícono y espacio hasta el texto.
pub(super) const CALLOUT_TITULO: i32 = 24 + 17 + 8;
/// Radio de las esquinas de un callout.
const RADIO: f32 = 6.0;
/// Cuánto se mete hacia adentro, por la derecha, cada nivel de transclusión: lo incrustado
/// (callouts y tarjetas incluidos) queda dentro del borde de la incrustación.
pub(super) const INCRUSTADO_DER: i32 = 16;
/// Niveles de transclusión con margen derecho propio (`der-1`…).
pub(super) const NIVELES_DER: i32 = orbital_vault::transclusion::MAX_DEPTH as i32;
/// Lado de una flechita del plegado.
const FLECHA: f32 = 14.0;

/// Algo que se dibuja sobre un tramo del texto.
pub(super) enum Capa {
    Callout {
        /// Comienzo del título.
        inicio: gtk::TextMark,
        /// Después del renglón de relleno del final.
        fin: gtk::TextMark,
        /// Final del texto del título (ahí va la flechita de un callout plegable).
        titulo_fin: gtk::TextMark,
        color: Rgb,
        icono: &'static str,
        /// La etiqueta que oculta el cuerpo, si se puede plegar.
        cuerpo: Option<gtk::TextTag>,
        profundidad: i32,
        /// Margen derecho extra (dentro de transclusiones).
        der: i32,
    },
    /// Una transclusión `![[nota]]`: tarjeta con borde y una franja de encabezado con el nombre.
    Incrustacion {
        inicio: gtk::TextMark,
        fin: gtk::TextMark,
        /// Comienzo del contenido (después del renglón del encabezado), si tiene.
        cuerpo: Option<gtk::TextMark>,
        profundidad: i32,
        der: i32,
    },
    /// Una tarjeta de estudio (024): fondo, borde, ícono, una línea sobre el botón y la píldora
    /// de «Mostrar/Ocultar respuesta».
    Tarjeta {
        inicio: gtk::TextMark,
        fin: gtk::TextMark,
        /// Texto de «Mostrar respuesta» y de «Ocultar respuesta» (uno de los dos está oculto).
        mostrar: (gtk::TextMark, gtk::TextMark),
        ocultar: (gtk::TextMark, gtk::TextMark),
        /// La etiqueta que oculta la respuesta.
        respuesta: gtk::TextTag,
        profundidad: i32,
        der: i32,
    },
    /// Una parte especial de la nota (041): la tarjeta de la Síntesis o de las Fuentes, o el
    /// bloque final de las Conexiones con sus píldoras.
    Seccion {
        inicio: gtk::TextMark,
        fin: gtk::TextMark,
        kind: SeccionKind,
        /// Comienzo y final del texto de cada píldora.
        chips: Vec<(gtk::TextMark, gtk::TextMark)>,
        profundidad: i32,
        der: i32,
    },
    /// Línea vertical de una lista anidada, a la altura de la viñeta de su ítem.
    Guia {
        inicio: gtk::TextMark,
        fin: gtk::TextMark,
        profundidad: i32,
    },
}

/// Color e ícono de un tipo de callout, con los alias de Obsidian.
pub(super) fn estilo_callout(kind: &CalloutKind) -> (Rgb, &'static str) {
    use crate::style::*;
    match kind {
        CalloutKind::Nota => (CALLOUT_AZUL, "callout-lapiz"),
        CalloutKind::Consejo | CalloutKind::Importante => (CALLOUT_CIAN, "callout-llama"),
        CalloutKind::Advertencia => (CALLOUT_NARANJA, "callout-alerta"),
        CalloutKind::Peligro => (CALLOUT_ROJO, "callout-rayo"),
        CalloutKind::Otro(nombre) => match nombre.as_str() {
            "info" => (CALLOUT_AZUL, "callout-info"),
            "todo" | "pendiente" => (CALLOUT_AZUL, "callout-tarea"),
            "abstract" | "summary" | "tldr" | "resumen" => (CALLOUT_CIAN, "callout-lista"),
            "success" | "check" | "done" | "exito" | "éxito" | "hecho" => {
                (CALLOUT_VERDE, "callout-check")
            }
            "question" | "help" | "faq" | "pregunta" | "ayuda" => {
                (CALLOUT_NARANJA, "callout-pregunta")
            }
            "attention" | "atencion" | "atención" => (CALLOUT_NARANJA, "callout-alerta"),
            "failure" | "fail" | "missing" | "fallo" | "error" => (CALLOUT_ROJO, "callout-cruz"),
            "bug" => (CALLOUT_ROJO, "callout-cruz"),
            "example" | "ejemplo" => (CALLOUT_VIOLETA, "callout-lista"),
            "quote" | "cite" | "cita" => (CALLOUT_GRIS, "callout-cita"),
            _ => (CALLOUT_AZUL, "callout-lapiz"),
        },
    }
}

mod imp {
    use super::*;

    type Dibujo = Box<dyn Fn(gtk::TextViewLayer, &gtk::Snapshot)>;

    #[derive(Default)]
    pub struct VistaLector {
        pub dibujo: RefCell<Option<Dibujo>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for VistaLector {
        const NAME: &'static str = "OrbitalVistaLector";
        type Type = super::VistaLector;
        type ParentType = gtk::TextView;
    }

    impl ObjectImpl for VistaLector {}
    impl WidgetImpl for VistaLector {}

    impl TextViewImpl for VistaLector {
        fn snapshot_layer(&self, layer: gtk::TextViewLayer, snapshot: gtk::Snapshot) {
            self.parent_snapshot_layer(layer, snapshot.clone());
            if let Some(f) = self.dibujo.borrow().as_ref() {
                f(layer, &snapshot);
            }
        }
    }
}

glib::wrapper! {
    /// La vista de texto del lector, con capas propias debajo y encima del texto.
    pub struct VistaLector(ObjectSubclass<imp::VistaLector>)
        @extends gtk::TextView, gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget, gtk::Scrollable;
}

impl VistaLector {
    pub(super) fn new() -> Self {
        glib::Object::new()
    }
}

impl Reader {
    /// Conecta las capas a la vista (una sola vez, al crearla).
    pub(super) fn setup_capas(&self) {
        let Some(vista) = self.0.view.downcast_ref::<VistaLector>() else {
            return;
        };
        let weak = Rc::downgrade(&self.0);
        *vista.imp().dibujo.borrow_mut() = Some(Box::new(move |capa, snapshot| {
            let Some(i) = weak.upgrade() else { return };
            let r = Reader(i);
            match capa {
                gtk::TextViewLayer::BelowText => r.dibujar_debajo(snapshot),
                _ => r.dibujar_flechas(snapshot),
            }
        }));
        self.setup_flechas_hover();
    }

    /// Los bloques de primer nivel que tocan lo visible, y el tramo visible en caracteres.
    fn visibles(&self) -> Option<(std::ops::Range<usize>, i32, i32)> {
        let i = &self.0;
        let blocks = i.blocks.borrow();
        if blocks.is_empty() {
            return None;
        }
        let buf = self.buffer();
        let vis = i.view.visible_rect();
        let (desde, _) = i.view.line_at_y(vis.y());
        let (mut hasta, _) = i.view.line_at_y(vis.y() + vis.height());
        if !hasta.ends_line() {
            hasta.forward_to_line_end();
        }
        let (a, z) = (desde.offset(), hasta.offset());
        let off = |m: &gtk::TextMark| buf.iter_at_mark(m).offset();
        let n0 = blocks
            .partition_point(|b| off(&b.mark) <= a)
            .saturating_sub(1);
        let n1 = blocks.partition_point(|b| off(&b.mark) <= z);
        Some((n0..n1.max(n0), a, z))
    }

    /// Fondos de las transclusiones, los callouts y las tarjetas, y guías de las listas. Primero
    /// lo de afuera: una transclusión debajo de lo que incrusta, un callout anidado encima del que
    /// lo contiene.
    fn dibujar_debajo(&self, snapshot: &gtk::Snapshot) {
        let Some((rango, a, z)) = self.visibles() else {
            return;
        };
        let i = &self.0;
        let view = &i.view;
        let buf = self.buffer();
        let side = i.side.get() as f32;
        let ancho = i.width.get() as f32;
        let blocks = i.blocks.borrow();
        let mut visibles: Vec<&Capa> = Vec::new();
        for b in &blocks[rango] {
            for c in &b.capas {
                let (inicio, fin) = match c {
                    Capa::Callout { inicio, fin, .. }
                    | Capa::Guia { inicio, fin, .. }
                    | Capa::Seccion { inicio, fin, .. }
                    | Capa::Incrustacion { inicio, fin, .. }
                    | Capa::Tarjeta { inicio, fin, .. } => (inicio, fin),
                };
                if buf.iter_at_mark(fin).offset() < a || buf.iter_at_mark(inicio).offset() > z {
                    continue;
                }
                visibles.push(c);
            }
        }
        // Las transclusiones de afuera hacia adentro, después el resto en el orden del texto y al
        // final los callouts, también de afuera hacia adentro (el orden es estable).
        visibles.sort_by_key(|c| match c {
            Capa::Incrustacion { profundidad, .. } => (0, *profundidad),
            Capa::Callout { profundidad, .. } => (2, *profundidad),
            _ => (1, 0),
        });
        let x0 = |profundidad: &i32| side + (24 * profundidad) as f32;
        let x1 = |der: &i32| side + ancho - *der as f32;
        for c in visibles {
            match c {
                Capa::Incrustacion {
                    inicio,
                    fin,
                    cuerpo,
                    profundidad,
                    der,
                } => self.dibujar_incrustacion(
                    snapshot,
                    (inicio, fin),
                    cuerpo.as_ref(),
                    x0(profundidad),
                    x1(der),
                ),
                Capa::Tarjeta {
                    inicio,
                    fin,
                    mostrar,
                    ocultar,
                    respuesta,
                    profundidad,
                    der,
                } => self.dibujar_tarjeta(
                    snapshot,
                    (inicio, fin),
                    if respuesta.is_invisible() {
                        mostrar
                    } else {
                        ocultar
                    },
                    x0(profundidad),
                    x1(der),
                ),
                Capa::Seccion {
                    inicio,
                    fin,
                    kind,
                    chips,
                    profundidad,
                    der,
                } => self.dibujar_seccion(
                    snapshot,
                    (inicio, fin),
                    *kind,
                    chips,
                    x0(profundidad),
                    x1(der),
                ),
                Capa::Guia {
                    inicio,
                    fin,
                    profundidad,
                } => {
                    let (y0, _) = view.line_yrange(&buf.iter_at_mark(inicio));
                    let mut f = buf.iter_at_mark(fin);
                    f.backward_char();
                    let (y1, h1) = view.line_yrange(&f);
                    let x = x0(profundidad) + 3.0;
                    let alto = (y1 + h1 - y0 - 4) as f32;
                    if alto > 0.0 {
                        snapshot.append_color(
                            &gtk::gdk::RGBA::new(1.0, 1.0, 1.0, 0.12),
                            &graphene::Rect::new(x, y0 as f32 + 2.0, 1.0, alto),
                        );
                    }
                }
                Capa::Callout {
                    inicio,
                    fin,
                    titulo_fin,
                    color,
                    icono,
                    cuerpo,
                    profundidad,
                    der,
                } => {
                    let ini = buf.iter_at_mark(inicio);
                    let Some(rect) = self.recuadro(&ini, fin, x0(profundidad), x1(der)) else {
                        continue;
                    };
                    snapshot.push_rounded_clip(&gsk::RoundedRect::from_rect(rect, RADIO));
                    snapshot.append_color(&color.rgba(crate::style::CALLOUT_ALFA as f32), &rect);
                    snapshot.pop();
                    let tinta = color.rgba(1.0);
                    self.icono_cabecera(snapshot, &ini, icono, x0(profundidad), &tinta);
                    if let Some(tag) = cuerpo {
                        let r = view.iter_location(&buf.iter_at_mark(titulo_fin));
                        let nombre = if tag.is_invisible() {
                            "derecha"
                        } else {
                            "abajo"
                        };
                        crate::iconos::dibujar(
                            snapshot,
                            nombre,
                            r.x() as f32 + 6.0,
                            r.y() as f32 + (r.height() as f32 - FLECHA) / 2.0,
                            FLECHA,
                            &tinta,
                        );
                    }
                }
            }
        }
    }

    /// El rectángulo de un bloque dibujado entre `x0` y `x1`: del renglón de `ini` al de `fin`
    /// (el renglón de relleno del final), sin la separación de afuera.
    fn recuadro(
        &self,
        ini: &gtk::TextIter,
        fin: &gtk::TextMark,
        x0: f32,
        x1: f32,
    ) -> Option<graphene::Rect> {
        let view = &self.0.view;
        let (y0, _) = view.line_yrange(ini);
        let mut f = self.buffer().iter_at_mark(fin);
        f.backward_char();
        let (y1, h1) = view.line_yrange(&f);
        let arriba = (y0 + CALLOUT_SEPARACION) as f32;
        let abajo = (y1 + h1 - CALLOUT_SEPARACION) as f32;
        (abajo > arriba)
            .then(|| graphene::Rect::new(x0, arriba, (x1 - x0).max(1.0), abajo - arriba))
    }

    /// El ícono del encabezado de un callout, una parte especial, una transclusión o una
    /// tarjeta, centrado en su primer renglón.
    fn icono_cabecera(
        &self,
        snapshot: &gtk::Snapshot,
        ini: &gtk::TextIter,
        icono: &str,
        x0: f32,
        tinta: &gtk::gdk::RGBA,
    ) {
        let view = &self.0.view;
        let (y0, h0) = view.line_yrange(ini);
        let renglon = view.iter_location(ini);
        let centro = if renglon.height() > 0 && renglon.y() >= y0 {
            renglon.y() as f32 + renglon.height() as f32 / 2.0
        } else {
            y0 as f32 + (CALLOUT_SEPARACION + CALLOUT_RELLENO) as f32 + h0 as f32 / 4.0
        };
        crate::iconos::dibujar(
            snapshot,
            icono,
            x0 + CALLOUT_ICONO_X,
            centro - CALLOUT_ICONO / 2.0,
            CALLOUT_ICONO,
            tinta,
        );
    }

    /// Una transclusión: borde fino rosado, la franja del encabezado con el ícono de nota y una
    /// línea que la separa del contenido. El contenido va sobre el fondo de la vista: sus callouts
    /// y tarjetas se ven como en la nota de origen.
    fn dibujar_incrustacion(
        &self,
        snapshot: &gtk::Snapshot,
        (inicio, fin): (&gtk::TextMark, &gtk::TextMark),
        cuerpo: Option<&gtk::TextMark>,
        x0: f32,
        x1: f32,
    ) {
        use crate::style::*;
        let view = &self.0.view;
        let buf = self.buffer();
        let ini = buf.iter_at_mark(inicio);
        let Some(rect) = self.recuadro(&ini, fin, x0, x1) else {
            return;
        };
        let redondo = gsk::RoundedRect::from_rect(rect, RADIO);
        // La franja del encabezado: hasta el comienzo del contenido (o todo, si no tiene).
        let alto_cab = cuerpo
            .map(|c| view.line_yrange(&buf.iter_at_mark(c)).0 as f32 - rect.y())
            .filter(|h| *h > 0.0 && *h < rect.height())
            .unwrap_or(rect.height());
        snapshot.push_rounded_clip(&redondo);
        snapshot.append_color(&EMBED_BG.rgba(1.0), &rect);
        snapshot.append_color(
            &VIEW_BG.rgba(0.55),
            &graphene::Rect::new(
                rect.x(),
                rect.y() + alto_cab,
                rect.width(),
                rect.height() - alto_cab,
            ),
        );
        snapshot.pop();
        let borde = CHIP_BORDER.rgba(1.0);
        if alto_cab < rect.height() {
            snapshot.append_color(
                &CHIP_BORDER.rgba(0.6),
                &graphene::Rect::new(rect.x(), rect.y() + alto_cab, rect.width(), 1.0),
            );
        }
        snapshot.append_border(&redondo, &[1.0; 4], &[borde, borde, borde, borde]);
        self.icono_cabecera(snapshot, &ini, "tipo-md", x0, &ACCENT_TEXT.rgba(1.0));
    }

    /// Una tarjeta de estudio (024): fondo, borde, el ícono de tarjetas junto al rótulo, una línea
    /// punteada sobre el botón y la píldora del botón visible (`boton`: su texto).
    fn dibujar_tarjeta(
        &self,
        snapshot: &gtk::Snapshot,
        (inicio, fin): (&gtk::TextMark, &gtk::TextMark),
        (ba, bz): &(gtk::TextMark, gtk::TextMark),
        x0: f32,
        x1: f32,
    ) {
        use crate::style::*;
        let view = &self.0.view;
        let buf = self.buffer();
        let ini = buf.iter_at_mark(inicio);
        let Some(rect) = self.recuadro(&ini, fin, x0, x1) else {
            return;
        };
        let redondo = gsk::RoundedRect::from_rect(rect, RADIO + 2.0);
        snapshot.push_rounded_clip(&redondo);
        snapshot.append_color(&CARD_CTX_BG.rgba(1.0), &rect);
        snapshot.append_color(
            &ACCENT_TEXT.rgba(0.85),
            &graphene::Rect::new(rect.x(), rect.y(), 3.0, rect.height()),
        );
        snapshot.pop();
        let borde = CARD_CTX_BORDER.rgba(1.0);
        snapshot.append_border(&redondo, &[1.0; 4], &[borde, borde, borde, borde]);
        self.icono_cabecera(snapshot, &ini, "tarjetas", x0, &ACCENT_TEXT.rgba(1.0));
        // El botón: la línea que separa la pregunta de la respuesta y una píldora detrás del
        // texto visible. Sin respuesta, no hay botón.
        let a = buf.iter_at_mark(ba);
        let mut ultimo = buf.iter_at_mark(bz);
        if !ultimo.backward_char() || ultimo.offset() < a.offset() {
            return;
        }
        let ra = view.iter_location(&a);
        let rz = view.iter_location(&ultimo);
        if ra.height() == 0 {
            return;
        }
        let (yl, _) = view.line_yrange(&a);
        let raya = gtk::gdk::RGBA::new(1.0, 1.0, 1.0, 0.14);
        let mut x = x0 + 24.0;
        while x < x1 - 24.0 {
            snapshot.append_color(&raya, &graphene::Rect::new(x, yl as f32 + 2.0, 4.0, 1.0));
            x += 8.0;
        }
        let r = graphene::Rect::new(
            ra.x() as f32 - 12.0,
            ra.y() as f32 - 3.0,
            (rz.x() + rz.width() - ra.x()) as f32 + 24.0,
            ra.height() as f32 + 6.0,
        );
        let pill = gsk::RoundedRect::from_rect(r, r.height() / 2.0);
        snapshot.push_rounded_clip(&pill);
        snapshot.append_color(&CHIP_BG.rgba(1.0), &r);
        snapshot.pop();
        let b = CHIP_BORDER.rgba(1.0);
        snapshot.append_border(&pill, &[1.0; 4], &[b, b, b, b]);
    }

    /// La tarjeta de una parte especial (041) entre `x0` y `x1`: la Síntesis con fondo rosa suave
    /// y borde de acento a la izquierda; las Fuentes con borde tenue; las Conexiones, una línea
    /// que las separa del cuerpo y una píldora redondeada detrás de cada enlace.
    fn dibujar_seccion(
        &self,
        snapshot: &gtk::Snapshot,
        (inicio, fin): (&gtk::TextMark, &gtk::TextMark),
        kind: SeccionKind,
        chips: &[(gtk::TextMark, gtk::TextMark)],
        x0: f32,
        x1: f32,
    ) {
        use crate::style::*;
        let view = &self.0.view;
        let buf = self.buffer();
        let ini = buf.iter_at_mark(inicio);
        let (y0, h0) = view.line_yrange(&ini);
        let mut f = buf.iter_at_mark(fin);
        f.backward_char();
        let (y1, h1) = view.line_yrange(&f);
        let arriba = (y0 + CALLOUT_SEPARACION) as f32;
        let abajo = (y1 + h1 - CALLOUT_SEPARACION) as f32;
        if abajo <= arriba {
            return;
        }
        let rect = graphene::Rect::new(x0, arriba, (x1 - x0).max(1.0), abajo - arriba);
        let redondo = gsk::RoundedRect::from_rect(rect, RADIO);
        let (icono, tinta) = match kind {
            SeccionKind::Sintesis => {
                snapshot.push_rounded_clip(&redondo);
                snapshot.append_color(&ACCENT_TEXT.rgba(0.07), &rect);
                snapshot.append_color(
                    &ACCENT_TEXT.rgba(0.85),
                    &graphene::Rect::new(x0, arriba, 3.0, abajo - arriba),
                );
                snapshot.pop();
                ("seccion-chispa", ACCENT_TEXT.rgba(1.0))
            }
            SeccionKind::Fuentes => {
                snapshot.push_rounded_clip(&redondo);
                snapshot.append_color(&gtk::gdk::RGBA::new(1.0, 1.0, 1.0, 0.025), &rect);
                snapshot.pop();
                let borde = gtk::gdk::RGBA::new(1.0, 1.0, 1.0, 0.10);
                snapshot.append_border(&redondo, &[1.0; 4], &[borde, borde, borde, borde]);
                ("seccion-libro", MUTED_FG.rgba(1.0))
            }
            SeccionKind::Conexiones => {
                snapshot.append_color(
                    &gtk::gdk::RGBA::new(1.0, 1.0, 1.0, 0.12),
                    &graphene::Rect::new(x0, arriba, (x1 - x0).max(1.0), 1.0),
                );
                ("red", MUTED_FG.rgba(1.0))
            }
        };
        // El ícono, centrado en el renglón del encabezado.
        let renglon = view.iter_location(&ini);
        let centro = if renglon.height() > 0 && renglon.y() >= y0 {
            renglon.y() as f32 + renglon.height() as f32 / 2.0
        } else {
            y0 as f32 + (CALLOUT_SEPARACION + CALLOUT_RELLENO) as f32 + h0 as f32 / 4.0
        };
        crate::iconos::dibujar(
            snapshot,
            icono,
            x0 + CALLOUT_ICONO_X,
            centro - CALLOUT_ICONO / 2.0,
            CALLOUT_ICONO,
            &tinta,
        );
        let fondo = CHIP_BG.rgba(1.0);
        let borde = CHIP_BORDER.rgba(1.0);
        for (a, z) in chips {
            let a = buf.iter_at_mark(a);
            let mut ultimo = buf.iter_at_mark(z);
            if !ultimo.backward_char() || ultimo.offset() < a.offset() {
                continue;
            }
            let ra = view.iter_location(&a);
            let rz = view.iter_location(&ultimo);
            if ra.height() == 0 {
                continue;
            }
            // Una píldora cortada por el ajuste de línea: un tramo por renglón.
            let tramos = if ra.y() == rz.y() {
                vec![(ra.x(), rz.x() + rz.width(), ra.y(), ra.height())]
            } else {
                vec![
                    (ra.x(), (x1 - 8.0) as i32, ra.y(), ra.height()),
                    ((x0 + 24.0) as i32, rz.x() + rz.width(), rz.y(), rz.height()),
                ]
            };
            for (xa, xz, y, h) in tramos {
                let r = graphene::Rect::new(
                    xa as f32 - 8.0,
                    y as f32 - 2.0,
                    (xz - xa) as f32 + 16.0,
                    h as f32 + 4.0,
                );
                let pill = gsk::RoundedRect::from_rect(r, r.height() / 2.0);
                snapshot.push_rounded_clip(&pill);
                snapshot.append_color(&fondo, &r);
                snapshot.pop();
                snapshot.append_border(&pill, &[1.0; 4], &[borde, borde, borde, borde]);
            }
        }
    }

    /// Las flechitas del plegado: las plegadas siempre; la de la sección bajo el mouse o con el
    /// foco del teclado, también desplegada (como sugerencia, en un gris suave).
    fn dibujar_flechas(&self, snapshot: &gtk::Snapshot) {
        let Some((rango, a, z)) = self.visibles() else {
            return;
        };
        let i = &self.0;
        let buf = self.buffer();
        let hover = i.hover_plegable.get();
        let enfocada = i
            .focused_link
            .get()
            .and_then(|n| self.all_link_items().get(n).map(|l| l.url.clone()))
            .and_then(|u| {
                u.strip_prefix(plegado::URL_PLEGAR)
                    .and_then(|l| l.parse::<u32>().ok())
            });
        let blocks = i.blocks.borrow();
        for b in &blocks[rango] {
            for p in &b.plegables {
                let g = buf.iter_at_mark(&p.glifo);
                let off = g.offset();
                if off < a || off > z {
                    continue;
                }
                let cerrada = plegado::es_cerrada(&g);
                let activa = hover == Some(p.linea) || enfocada == Some(p.linea);
                if !cerrada && !activa {
                    continue;
                }
                let r = i.view.iter_location(&g);
                if r.height() == 0 {
                    continue;
                }
                let alfa = if activa { 0.75 } else { 0.45 };
                crate::iconos::dibujar(
                    snapshot,
                    if cerrada { "derecha" } else { "abajo" },
                    r.x() as f32 + (r.width() as f32 - FLECHA) / 2.0,
                    r.y() as f32 + (r.height() as f32 - FLECHA) / 2.0,
                    FLECHA,
                    &gtk::gdk::RGBA::new(1.0, 1.0, 1.0, alfa),
                );
            }
        }
    }

    /// El elemento plegable más interno bajo el carácter `off`: un ítem con subítems o, si no,
    /// la sección del encabezado más cercano hacia arriba.
    pub(super) fn plegable_en(&self, off: i32) -> Option<u32> {
        let buf = self.buffer();
        let blocks = self.0.blocks.borrow();
        let o = |m: &gtk::TextMark| buf.iter_at_mark(m).offset();
        let n = blocks
            .partition_point(|b| o(&b.mark) <= off)
            .checked_sub(1)?;
        let item = blocks[n]
            .plegables
            .iter()
            .filter_map(|p| {
                let (_, hasta) = p.contenido.as_ref()?;
                let g = o(&p.glifo);
                (g <= off && off < o(hasta)).then_some((g, p.linea))
            })
            .max_by_key(|(g, _)| *g);
        if let Some((_, l)) = item {
            return Some(l);
        }
        // El encabezado más cercano hacia arriba contiene a `off`; si no se puede plegar (no tiene
        // nada debajo), su sección madre.
        let mut limite = u8::MAX;
        for b in blocks[..=n].iter().rev() {
            let Some(nivel) = b.nivel.filter(|l| *l < limite) else {
                continue;
            };
            if let Some(p) = b.plegables.iter().find(|p| p.contenido.is_none()) {
                return Some(p.linea);
            }
            limite = nivel;
            if nivel == 1 {
                break;
            }
        }
        None
    }

    /// El mouse sobre la vista: la flechita de la sección de abajo aparece como sugerencia.
    fn setup_flechas_hover(&self) {
        let motion = gtk::EventControllerMotion::new();
        let weak = Rc::downgrade(&self.0);
        motion.connect_motion(move |_, x, y| {
            let Some(i) = weak.upgrade() else { return };
            let (bx, by) =
                i.view
                    .window_to_buffer_coords(gtk::TextWindowType::Widget, x as i32, y as i32);
            let reader = Reader(i.clone());
            let nueva = i
                .view
                .iter_at_location(bx, by)
                .and_then(|it| reader.plegable_en(it.offset()));
            if i.hover_plegable.replace(nueva) != nueva {
                i.view.queue_draw();
            }
        });
        let weak = Rc::downgrade(&self.0);
        motion.connect_leave(move |_| {
            if let Some(i) = weak.upgrade()
                && i.hover_plegable.take().is_some()
            {
                i.view.queue_draw();
            }
        });
        self.0.view.add_controller(motion);
    }
}
