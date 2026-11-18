//! Fórmulas LaTeX en la Lectura (050): cada una es un pintable dentro del texto (sin widgets
//! anclados), con la imagen de la caché de `crate::formulas`. Detrás va su LaTeX como texto casi
//! invisible y diminuto: lo lee el lector de pantalla, lo encuentra la búsqueda y se copia con la
//! selección. Una fórmula con error se ve como su fuente, subrayada, con el motivo en el tooltip.

use super::*;
use crate::formulas::{self, Clave, Formula, Modo};

/// Escala del LaTeX que acompaña a cada fórmula: ocupa casi nada.
pub(super) const ESCALA_FUENTE: f64 = 0.02;

impl Reader {
    /// El estilo de las fórmulas de esta vista (letra, color y escala del monitor).
    pub fn estilo_formulas(&self) -> formulas::Estilo {
        let e = formulas::Estilo::de(&self.0.view);
        match self.0.tamano_formulas.get() {
            Some(tamano) => formulas::Estilo { tamano, ..e },
            None => e,
        }
    }

    /// Fija el tamaño de letra de las fórmulas (OnStage cambia la letra por CSS y la vista se
    /// entera recién al dibujar).
    pub fn fijar_tamano_formulas(&self, px: Option<f32>) {
        self.0.tamano_formulas.set(px);
    }

    /// Lo volcado tiene alguna fórmula.
    pub fn tiene_formulas(&self) -> bool {
        self.0.hay_formulas.get()
    }

    /// Las claves de las fórmulas de un documento con el estilo dado, para dibujarlas en el hilo
    /// de fondo antes de volcar ([`formulas::precalentar`]). Los encabezados van con su escala.
    pub fn claves_formulas(doc: &RenderDoc, estilo: &formulas::Estilo) -> Vec<Clave> {
        fn inl(out: &mut Vec<Clave>, inlines: &[Inline], e: &formulas::Estilo) {
            for i in inlines {
                match i {
                    Inline::Math { tex, display } => {
                        let modo = if *display {
                            Modo::Bloque
                        } else {
                            Modo::EnLinea
                        };
                        out.push(Clave::new(tex, modo, e));
                    }
                    Inline::Strong(c) | Inline::Emph(c) | Inline::Strike(c) => inl(out, c, e),
                    Inline::Link { children, .. } => inl(out, children, e),
                    _ => {}
                }
            }
        }
        fn bloques(out: &mut Vec<Clave>, blocks: &[Block], e: &formulas::Estilo) {
            for b in blocks {
                match &b.kind {
                    BlockKind::Math { tex } => out.push(Clave::new(tex, Modo::Bloque, e)),
                    BlockKind::Paragraph(i) => inl(out, i, e),
                    BlockKind::Heading { level, inlines } => {
                        let escala = ESCALAS_TITULO[usize::from(level.clamp(&1, &6) - 1)];
                        inl(out, inlines, &e.por(escala as f32));
                    }
                    BlockKind::List { items, .. } => {
                        for it in items {
                            bloques(out, &it.blocks, e);
                        }
                    }
                    BlockKind::Quote(body)
                    | BlockKind::Callout { body, .. }
                    | BlockKind::Group(body)
                    | BlockKind::Seccion { body, .. } => bloques(out, body, e),
                    BlockKind::Table { header, rows, .. } => {
                        for c in header.iter().chain(rows.iter().flatten()) {
                            inl(out, c, e);
                        }
                    }
                    BlockKind::Flashcard(f) => {
                        bloques(out, &f.question, e);
                        bloques(out, &f.answer, e);
                    }
                    BlockKind::Transclusion(t) => {
                        if let orbital_vault::render::Embed::Note { blocks, .. } = &t.content {
                            bloques(out, blocks, e);
                        }
                    }
                    _ => {}
                }
            }
        }
        let mut out = Vec::new();
        bloques(&mut out, &doc.blocks, estilo);
        out
    }

    /// Una fórmula en `end`, con las etiquetas del contexto.
    pub(super) fn formula(
        &self,
        s: &RenderState,
        end: &mut gtk::TextIter,
        tex: &str,
        modo: Modo,
        tags: &[gtk::TextTag],
    ) {
        self.0.hay_formulas.set(true);
        let buf = &s.buffer;
        let table = buf.tag_table();
        // El tamaño de la letra del contexto (un encabezado agranda la fórmula).
        let escala: f64 = tags
            .iter()
            .filter(|t| t.is_scale_set())
            .map(|t| t.scale())
            .product();
        let estilo = s.estilo_formulas.get().por(escala as f32);
        let clave = Clave::new(tex, modo, &estilo);
        let (pintable, profundidad) = match formulas::en_cache(&clave) {
            Some(Ok(d)) => (Formula::lista(&d), d.profundidad),
            Some(Err(e)) => return self.formula_con_error(s, end, tex, modo, tags, &e.mensaje),
            None => match formulas::medir(&clave) {
                Ok((ancho, alto, prof)) => {
                    let f = Formula::esperando(ancho, alto);
                    f.pedir(clave);
                    (f, prof)
                }
                Err(e) => return self.formula_con_error(s, end, tex, modo, tags, &e.mensaje),
            },
        };
        let desde = end.offset();
        if s.provisorio.get() {
            // En un buffer suelto que se trasplanta (`reemplazar`): el pintable se pone al
            // trasplantar, en el lugar de su `U+FFFC`.
            s.pintables
                .borrow_mut()
                .push((pintable.upcast(), end.offset()));
            buf.insert(end, "\u{fffc}");
        } else {
            buf.insert_paintable(end, &pintable);
        }
        let mut propias: Vec<gtk::TextTag> = tags.to_vec();
        // En el renglón, la línea base de la fórmula va con la del texto: baja lo que su
        // profundidad (la vista apoya los pintables sobre la línea base).
        let baja = profundidad.round() as i32;
        if modo == Modo::EnLinea && baja > 0 {
            propias.push(self.tag_bajar(baja));
        }
        let inicio = buf.iter_at_offset(desde);
        for t in &propias {
            buf.apply_tag(t, &inicio, end);
        }
        let mut fuente = tags.to_vec();
        fuente.extend(table.lookup("formula-fuente"));
        let refs: Vec<&gtk::TextTag> = fuente.iter().collect();
        // En un solo renglón: un salto del LaTeX sería un renglón (diminuto) más en la vista.
        let delim = if modo == Modo::Bloque { "$$" } else { "$" };
        let mut texto = String::with_capacity(tex.len() + 4);
        texto.push_str(delim);
        texto.extend(tex.split_whitespace().flat_map(|p| [" ", p]).skip(1));
        texto.push_str(delim);
        buf.insert_with_tags(end, &texto, &refs);
    }

    /// El fuente de una fórmula que no se puede dibujar, subrayado como error.
    fn formula_con_error(
        &self,
        s: &RenderState,
        end: &mut gtk::TextIter,
        tex: &str,
        modo: Modo,
        tags: &[gtk::TextTag],
        _mensaje: &str,
    ) {
        self.0.hay_formulas.set(true);
        let table = s.buffer.tag_table();
        let mut more = tags.to_vec();
        more.extend(table.lookup("formula-error"));
        let refs: Vec<&gtk::TextTag> = more.iter().collect();
        let delim = if modo == Modo::Bloque { "$$" } else { "$" };
        s.buffer
            .insert_with_tags(end, &format!("{delim}{tex}{delim}"), &refs);
    }

    /// Una fórmula en bloque: su propio renglón, centrada.
    pub(super) fn formula_bloque(
        &self,
        s: &RenderState,
        end: &mut gtk::TextIter,
        tex: &str,
        base: &[gtk::TextTag],
    ) {
        let mut tags = base.to_vec();
        tags.extend(s.buffer.tag_table().lookup("formula-bloque"));
        self.formula(s, end, tex, Modo::Bloque, &tags);
        let refs: Vec<&gtk::TextTag> = tags.iter().collect();
        s.buffer.insert_with_tags(end, "\n", &refs);
    }

    /// Etiqueta que baja `px` píxeles (compartida, una por valor).
    fn tag_bajar(&self, px: i32) -> gtk::TextTag {
        let table = self.buffer().tag_table();
        let nombre = format!("formula-baja-{px}");
        if let Some(t) = table.lookup(&nombre) {
            return t;
        }
        let t = gtk::TextTag::builder()
            .name(&nombre)
            .rise(-px * pango::SCALE)
            .build();
        table.add(&t);
        t
    }

    /// El motivo del error de la fórmula con error bajo `iter`, para el tooltip.
    pub(super) fn error_de_formula_en(&self, iter: &gtk::TextIter) -> Option<String> {
        let tag = self.buffer().tag_table().lookup("formula-error")?;
        if !iter.has_tag(&tag) {
            return None;
        }
        let mut a = *iter;
        if !a.starts_tag(Some(&tag)) {
            a.backward_to_tag_toggle(Some(&tag));
        }
        let mut z = *iter;
        z.forward_to_tag_toggle(Some(&tag));
        let fuente = a.slice(&z);
        let tex = fuente
            .strip_prefix("$$")
            .and_then(|t| t.strip_suffix("$$"))
            .or_else(|| fuente.strip_prefix('$').and_then(|t| t.strip_suffix('$')))
            .unwrap_or(&fuente);
        Some(match orbital_formulas::validar(tex) {
            Err(e) => e.mensaje,
            Ok(()) => "No se pudo dibujar la fórmula".into(),
        })
    }
}

/// La escala de cada nivel de encabezado (la misma que sus etiquetas).
pub(super) const ESCALAS_TITULO: [f64; 6] = [1.9, 1.55, 1.3, 1.15, 1.05, 1.0];
