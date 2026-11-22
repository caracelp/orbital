//! Vista previa de fórmulas en Código (050): con el cursor dentro de una fórmula (`$…$`,
//! `$$…$$`, ` ```math `), un globito encima muestra la fórmula dibujada, o el motivo si tiene un
//! error. No toma el foco ni las teclas (como el autocompletado) y se arma recién con la primera
//! fórmula. Dentro de un ` ```grafico ` (052) muestra el gráfico estático (o el error); dentro de
//! un ` ```mermaid ` (058), el diagrama achicado (o el error).

use super::*;
use crate::formulas::{self, Clave, Formula, Modo};

/// Pausa después del último movimiento del cursor: escribir no dibuja en cada tecla.
const PAUSA: Duration = Duration::from_millis(160);
/// Renglones alrededor del cursor que se miran (una fórmula o un ` ```math ` más largo no se ve).
const RENGLONES: i32 = 200;
/// Las fórmulas del globito, un poco más grandes que el texto de Código.
const AUMENTO: f32 = 1.3;

/// Una fórmula del buffer, en caracteres.
pub(super) struct FormulaEnBuffer {
    /// Comienzo de la fórmula con sus delimitadores.
    pub(super) inicio: i32,
    /// Rango del LaTeX, sin delimitadores.
    pub(super) contenido: std::ops::Range<i32>,
    pub(super) tex: String,
    pub(super) modo: Modo,
}

/// Un ` ```grafico ` del buffer, en caracteres (052).
pub(super) struct GraficoEnBuffer {
    /// Comienzo de la cerca de apertura.
    pub(super) inicio: i32,
    /// Los renglones de adentro (sin las cercas).
    pub(super) contenido: std::ops::Range<i32>,
    pub(super) fuente: String,
}

/// Lo más grande que se ve un diagrama en el globito (se achica sin deformarse).
const DIAGRAMA_ANCHO: i32 = 480;
const DIAGRAMA_ALTO: i32 = 360;

/// Ancho y alto del gráfico del globito.
const GRAFICO_ANCHO: i32 = 380;
const GRAFICO_ALTO: i32 = 220;

/// El globito.
pub(super) struct VistaPrevia {
    pub(super) popover: gtk::Popover,
    pub(super) imagen: gtk::Picture,
    pub(super) error: gtk::Label,
    /// Donde va el gráfico (052).
    pub(super) grafico: gtk::Box,
    /// La fórmula que se está mostrando (LaTeX, modo) y el inicio en el buffer.
    actual: RefCell<Option<(String, Modo, i32)>>,
    /// El fuente del gráfico que se está mostrando.
    actual_grafico: RefCell<Option<String>>,
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
        let grafico = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .margin_top(6)
            .margin_bottom(6)
            .margin_start(6)
            .margin_end(6)
            .visible(false)
            .build();
        let caja = gtk::Box::new(gtk::Orientation::Vertical, 0);
        caja.append(&imagen);
        caja.append(&grafico);
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
            grafico,
            actual: RefCell::new(None),
            actual_grafico: RefCell::new(None),
        }
    }

    fn ocultar(&self) {
        self.actual.replace(None);
        self.actual_grafico.replace(None);
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
        let b = &self.0.buffer;
        let f = self.formula_en_offset(b.cursor_position())?;
        if f.tex.trim().is_empty() {
            return None;
        }
        Some((f.inicio, f.tex, f.modo))
    }

    /// La fórmula que contiene el carácter `offset` del buffer, si hay (busca en ±200 renglones).
    pub(super) fn formula_en_offset(&self, offset: i32) -> Option<FormulaEnBuffer> {
        let i = &self.0;
        let b = &i.buffer;
        let cursor = b.iter_at_offset(offset);
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
        let byte = antes.len();
        let (rango, contenido, tex, modo) = match i.format.get() {
            Format::Html => {
                let (rango, tex, modo) = orbital_formulas::html::formula_en(&ventana, byte)?;
                (rango.clone(), rango, tex, modo)
            }
            _ => {
                let f = orbital_vault::formulas::formula_en(&ventana, byte)?;
                let modo = if f.bloque {
                    Modo::Bloque
                } else {
                    Modo::EnLinea
                };
                (f.rango, f.contenido, f.tex, modo)
            }
        };
        let base = desde.offset();
        let a_offset = |byte: usize| base + ventana[..byte].chars().count() as i32;
        Some(FormulaEnBuffer {
            inicio: a_offset(rango.start),
            contenido: a_offset(contenido.start)..a_offset(contenido.end),
            tex,
            modo,
        })
    }

    /// El ` ```grafico ` que contiene el carácter `offset` del buffer (busca en ±200 renglones).
    pub(super) fn grafico_en_offset(&self, offset: i32) -> Option<GraficoEnBuffer> {
        self.cerca_en_offset(offset, "grafico")
    }

    /// El bloque cercado con lenguaje `lang` que contiene el carácter `offset` (±200 renglones).
    pub(super) fn cerca_en_offset(&self, offset: i32, lang: &str) -> Option<GraficoEnBuffer> {
        let i = &self.0;
        if i.format.get() != Format::Md {
            return None;
        }
        let b = &i.buffer;
        let cursor = b.iter_at_offset(offset);
        let linea = cursor.line();
        let desde = b.iter_at_line((linea - RENGLONES).max(0))?;
        let mut hasta = b
            .iter_at_line(linea + RENGLONES)
            .unwrap_or_else(|| b.end_iter());
        if !hasta.ends_line() {
            hasta.forward_to_line_end();
        }
        let ventana = b.text(&desde, &hasta, true);
        if !ventana.contains(lang) {
            return None;
        }
        let byte = b.text(&desde, &cursor, true).len();
        let c = orbital_vault::formulas::cerca_en(&ventana, byte)?;
        if c.lang != lang {
            return None;
        }
        let base = desde.offset();
        let a_offset = |byte: usize| base + ventana[..byte].chars().count() as i32;
        Some(GraficoEnBuffer {
            inicio: a_offset(c.rango.start),
            contenido: a_offset(c.contenido.start)..a_offset(c.contenido.end),
            fuente: ventana[c.contenido].to_owned(),
        })
    }

    /// Lleva el globito sobre el carácter `inicio` del buffer.
    fn apuntar_globito(&self, v: &VistaPrevia, inicio: i32) {
        let i = &self.0;
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
    }

    pub(super) fn actualizar_vista_previa(&self) {
        let i = &self.0;
        let activa = i.view.is_focus()
            && matches!(i.mode.get(), View::Codigo | View::Dividida)
            && !i.loading.get();
        let hallada = activa.then(|| self.formula_en_cursor()).flatten();
        let grafico = (activa && hallada.is_none())
            .then(|| self.grafico_en_offset(i.buffer.cursor_position()))
            .flatten();
        if activa && i.format.get() == Format::Md {
            self.rotular_menu(match (&hallada, &grafico) {
                (Some(_), _) => super::ecuacion::Sobre::Formula,
                (None, Some(_)) => super::ecuacion::Sobre::Grafico,
                _ => super::ecuacion::Sobre::Nada,
            });
        }
        if let Some(g) = grafico {
            self.mostrar_grafico(g);
            return;
        }
        let diagrama = (activa && hallada.is_none())
            .then(|| self.cerca_en_offset(i.buffer.cursor_position(), "mermaid"))
            .flatten();
        if let Some(d) = diagrama {
            self.mostrar_diagrama(d);
            return;
        }
        let indice = (activa && hallada.is_none())
            .then(|| self.cerca_en_offset(i.buffer.cursor_position(), "indice"))
            .flatten();
        if let Some(d) = indice {
            self.mostrar_indice(d);
            return;
        }
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
        self.apuntar_globito(v, inicio);
        if v.actual_grafico.replace(None).is_some() {
            v.popover
                .update_property(&[gtk::accessible::Property::Label(
                    "Vista previa de la fórmula",
                )]);
        }
        v.grafico.set_visible(false);
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

    /// El gráfico bajo el cursor (052), estático; con error, el motivo.
    fn mostrar_grafico(&self, g: GraficoEnBuffer) {
        let i = &self.0;
        if g.fuente.trim().is_empty() {
            self.ocultar_vista_previa();
            return;
        }
        if i.vista_previa.borrow().is_none() {
            i.vista_previa.replace(Some(VistaPrevia::new(&i.view)));
        }
        let prestamo = i.vista_previa.borrow();
        let Some(v) = prestamo.as_ref() else { return };
        self.apuntar_globito(v, g.inicio);
        let mismo = v.actual_grafico.borrow().as_deref() == Some(g.fuente.as_str());
        if mismo && v.popover.is_visible() {
            return;
        }
        v.actual.replace(None);
        v.imagen.set_paintable(None::<&gtk::gdk::Paintable>);
        v.imagen.set_visible(false);
        while let Some(h) = v.grafico.first_child() {
            v.grafico.remove(&h);
        }
        match crate::graficos::leer(&g.fuente) {
            Ok(grafico) => {
                let vista = crate::graficos::GraficoVista::new(grafico, &g.fuente, false);
                vista.lienzo().fijar_alto(GRAFICO_ALTO);
                vista.widget().set_size_request(GRAFICO_ANCHO, -1);
                v.grafico.append(vista.widget());
                v.grafico.set_visible(true);
                v.error.set_visible(false);
            }
            Err(e) => {
                v.grafico.set_visible(false);
                v.error.set_label(&e.to_string());
                v.error.set_visible(true);
            }
        }
        v.actual_grafico.replace(Some(g.fuente));
        v.popover
            .update_property(&[gtk::accessible::Property::Label("Vista previa del gráfico")]);
        v.popover.popup();
    }

    /// El diagrama Mermaid bajo el cursor (058), achicado para el globito; con error, el motivo.
    /// El SVG sale de la caché (o se arma: décimas de milisegundo) y la imagen se dibuja en el
    /// pool de `gio`.
    fn mostrar_diagrama(&self, g: GraficoEnBuffer) {
        let i = &self.0;
        if g.fuente.trim().is_empty() {
            self.ocultar_vista_previa();
            return;
        }
        if i.vista_previa.borrow().is_none() {
            i.vista_previa.replace(Some(VistaPrevia::new(&i.view)));
        }
        let prestamo = i.vista_previa.borrow();
        let Some(v) = prestamo.as_ref() else { return };
        self.apuntar_globito(v, g.inicio);
        let mismo = v.actual_grafico.borrow().as_deref() == Some(g.fuente.as_str());
        if mismo && v.popover.is_visible() {
            return;
        }
        v.actual.replace(None);
        v.imagen.set_paintable(None::<&gtk::gdk::Paintable>);
        v.imagen.set_visible(false);
        while let Some(h) = v.grafico.first_child() {
            v.grafico.remove(&h);
        }
        let mut estilo = crate::diagramas::Estilo::de(&i.view);
        // Código es oscuro como la Lectura.
        estilo.tema = crate::diagramas::Tema::Oscuro;
        match crate::diagramas::svg(&g.fuente, estilo.tema) {
            Ok(d) => {
                let pic = crate::diagramas::picture(
                    crate::diagramas::Clave::new(&g.fuente, estilo),
                    &d.nombre(),
                );
                let k = (DIAGRAMA_ANCHO as f32 / d.ancho)
                    .min(DIAGRAMA_ALTO as f32 / d.alto)
                    .min(1.0);
                pic.set_size_request(
                    (d.ancho * k).round().max(1.0) as i32,
                    (d.alto * k).round().max(1.0) as i32,
                );
                v.grafico.append(&pic);
                v.grafico.set_visible(true);
                v.error.set_visible(false);
            }
            Err(e) => {
                v.grafico.set_visible(false);
                v.error.set_label(&e.to_string());
                v.error.set_visible(true);
            }
        }
        v.actual_grafico.replace(Some(g.fuente));
        v.popover
            .update_property(&[gtk::accessible::Property::Label(
                "Vista previa del diagrama",
            )]);
        v.popover.popup();
    }

    /// El índice por etiqueta bajo el cursor (036): cuántas notas lista y las primeras, del
    /// índice en memoria (una pasada por las notas; sin leer el disco); con error, el motivo.
    fn mostrar_indice(&self, g: GraficoEnBuffer) {
        let i = &self.0;
        let texto = match orbital_vault::indice::parse(&g.fuente, &reader::indice_defaults()) {
            Err(e) => e.to_string(),
            Ok(o) => {
                let idx = i.search_index.borrow().clone();
                let Some(idx) = idx else {
                    self.ocultar_vista_previa();
                    return;
                };
                let Ok(idx) = idx.read() else { return };
                resumen_indice(&idx.indice_etiqueta(&o), &o)
            }
        };
        if i.vista_previa.borrow().is_none() {
            i.vista_previa.replace(Some(VistaPrevia::new(&i.view)));
        }
        let prestamo = i.vista_previa.borrow();
        let Some(v) = prestamo.as_ref() else { return };
        self.apuntar_globito(v, g.inicio);
        let clave = format!("indice\n{}", g.fuente);
        if v.actual_grafico.borrow().as_deref() == Some(clave.as_str())
            && v.popover.is_visible()
            && v.error.label() == texto
        {
            return;
        }
        v.actual.replace(None);
        v.imagen.set_paintable(None::<&gtk::gdk::Paintable>);
        v.imagen.set_visible(false);
        while let Some(h) = v.grafico.first_child() {
            v.grafico.remove(&h);
        }
        v.grafico.set_visible(false);
        v.error.set_label(&texto);
        v.error.set_visible(true);
        v.actual_grafico.replace(Some(clave));
        v.popover
            .update_property(&[gtk::accessible::Property::Label("Vista previa del índice")]);
        v.popover.popup();
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

/// El texto del globito de un índice por etiqueta (036): el total, las sub-etiquetas y las
/// primeras notas.
pub(super) fn resumen_indice(
    ind: &orbital_vault::indice::Indice,
    o: &orbital_vault::indice::IndiceOpciones,
) -> String {
    let r = &ind.raiz;
    if r.notas.is_empty() && r.hijas.is_empty() {
        return orbital_vault::indice::vacio(ind, o);
    }
    let total = r.total();
    let mut s = if o.meta {
        format!(
            "Índice de índices: {total} {}",
            if total == 1 { "nota" } else { "notas" }
        )
    } else {
        format!(
            "Índice de #{}: {total} {}",
            o.etiqueta,
            if total == 1 { "nota" } else { "notas" }
        )
    };
    if !r.hijas.is_empty() {
        let n = r.hijas.len();
        s.push_str(&format!(
            " en {n} {}",
            if n == 1 {
                "sub-etiqueta"
            } else {
                "sub-etiquetas"
            }
        ));
    }
    for e in r.notas.iter().take(5) {
        s.push_str(&format!("\n• {}", e.nombre));
    }
    if r.notas.len() > 5 {
        s.push_str(&format!("\n… y {} más", r.notas.len() - 5));
    }
    s
}
