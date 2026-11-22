//! Cómo se dibuja cada bloque: párrafos, transclusiones, tareas, callouts, tablas, texto en línea e imágenes.

use super::*;

/// Prefijo del «enlace» que abre y cierra un callout plegable (el resto es su etiqueta).
pub(super) const URL_CALLOUT: &str = "orbital-callout:";

impl Reader {
    /// Un bloque de primer nivel. Si su `firma` dice que está oculto (dentro de una sección
    /// plegada o fuera del enfoque), no se vuelca nada: queda solo su marca (028/046).
    pub(super) fn top_block(
        &self,
        s: &RenderState,
        end: &mut gtk::TextIter,
        fuente: Rc<Block>,
        hash: u64,
        firma: super::plegado::Firma,
    ) -> BlockInfo {
        let block = &*fuente;
        let mark = s.buffer.create_mark(None, end, true);
        let base = Rc::new(Cell::new(block.source.start));
        s.plegables_lector.borrow_mut().clear();
        s.capas.borrow_mut().clear();
        *s.block_base.borrow_mut() = base.clone();
        s.block_start.set(block.source.start);
        if !firma.oculto {
            s.foco.set(firma.foco);
            s.plegadas.borrow_mut().clone_from(&firma.plegadas);
            self.block(s, end, block, 0);
            s.foco.set(None);
            let codigo = std::mem::take(&mut *s.codigo.borrow_mut());
            if !codigo.is_empty() {
                s.resaltar.borrow_mut().push(super::resaltar::Pendiente {
                    fuente: fuente.clone(),
                    codigo,
                });
            }
        }
        let (heading, nivel) = match &block.kind {
            BlockKind::Heading { inlines, level } => (
                Some(orbital_vault::render::plain_text(inlines)),
                Some(*level),
            ),
            _ => (None, None),
        };
        BlockInfo {
            hash,
            line: block.line,
            mark,
            base,
            tags: std::mem::take(&mut *s.tags.borrow_mut()),
            heading,
            links: std::mem::take(&mut *s.link_items.borrow_mut()),
            nivel,
            plegables: std::mem::take(&mut *s.plegables_lector.borrow_mut()),
            capas: std::mem::take(&mut *s.capas.borrow_mut()),
            fuente,
            firma,
        }
    }

    pub(super) fn block(&self, s: &RenderState, end: &mut gtk::TextIter, b: &Block, depth: i32) {
        let buf = &s.buffer;
        let table = buf.tag_table();
        // US1 (012): las definiciones de notas al pie no se muestran como párrafo; su contenido
        // queda en el Reader para el tooltip de la referencia.
        if let BlockKind::FootnoteDefinition { nombre, inlines } = &b.kind {
            self.0
                .fn_defs
                .borrow_mut()
                .insert(nombre.clone(), plain_text(inlines));
            return;
        }
        let indent = (depth > 0).then(|| self.indent_tag(depth * 24));
        let base: Vec<gtk::TextTag> = indent.into_iter().collect();
        match &b.kind {
            // FootnoteDefinition nunca llega acá (el early return de arriba la procesa).
            BlockKind::FootnoteDefinition { .. } => {}
            BlockKind::Heading { level, inlines } => {
                let mut tags = base.clone();
                tags.extend(table.lookup(&format!("h{level}")));
                // Un encabezado de primer nivel del documento con contenido: flechita (028).
                if depth == 0 && s.es_plegable(b.line) {
                    let mut g = tags.clone();
                    g.extend(table.lookup(&format!("flecha-h{level}")));
                    let cerrada = s.plegada(b.line);
                    self.glifos(s, end, b.line as u32, &g, " ", 0, cerrada);
                }
                self.inlines(s, end, inlines, &tags);
                buf.insert(end, "\n");
            }
            BlockKind::Paragraph(inlines) => {
                self.inlines(s, end, inlines, &base);
                buf.insert(end, "\n");
            }
            BlockKind::List { start, items } => {
                for (n, item) in items.iter().enumerate() {
                    // Enfocado un ítem de esta lista (028): solo él con lo suyo; de los que lo
                    // contienen, nada más que la sublista que lleva hasta él.
                    let foco = s.foco.get();
                    if let Some(a) = foco {
                        let linea = item.blocks.first().map(|b| b.line as u32);
                        let siguiente = items
                            .get(n + 1)
                            .and_then(|i| i.blocks.first())
                            .map(|b| b.line as u32);
                        match linea {
                            Some(l) if l == a => {}
                            Some(l) if l < a && siguiente.is_none_or(|x| x > a) => {
                                for sub in &item.blocks {
                                    if matches!(sub.kind, BlockKind::List { .. }) {
                                        self.block(s, end, sub, depth + 1);
                                    }
                                }
                                continue;
                            }
                            _ => continue,
                        }
                        s.foco.set(None);
                    }
                    let tag = self.indent_tag((depth + 1) * 24);
                    let mut tags = base.clone();
                    tags.push(tag.clone());
                    let item_start = end.offset();
                    // Un ítem con subítems: la flechita reemplaza a la viñeta (028).
                    let plegable = item
                        .blocks
                        .first()
                        .map(|b| b.line)
                        .filter(|l| s.es_plegable(*l));
                    let indice = s.plegables_lector.borrow().len();
                    // Plegado, lo de debajo de su primer renglón no se vuelca (046).
                    let cerrado = plegable.is_some_and(|l| s.plegada(l));
                    // Un ítem con subítems: la flechita cuelga a la izquierda de la viñeta (028).
                    if let Some(l) = plegable {
                        self.glifos(s, end, l as u32, &tags, " ", (depth + 1) * 24, cerrado);
                    }
                    let mut desde: Option<gtk::TextMark> = None;
                    if let Some(task) = item.task {
                        self.task(s, end, item, task);
                    } else {
                        let refs: Vec<&gtk::TextTag> = tags.iter().collect();
                        match start {
                            Some(s) => {
                                buf.insert_with_tags(end, &format!("{}. ", s + n as u64), &refs)
                            }
                            None => {
                                let mut v = tags.clone();
                                v.extend(table.lookup("vineta"));
                                buf.insert_with_tags(end, "•", &v.iter().collect::<Vec<_>>());
                                buf.insert_with_tags(end, "  ", &refs);
                            }
                        }
                    }
                    for (k, sub) in item.blocks.iter().enumerate() {
                        if cerrado && k > 0 {
                            break;
                        }
                        match &sub.kind {
                            // El primer párrafo va en la misma línea que la viñeta.
                            BlockKind::Paragraph(p) if k == 0 => {
                                self.inlines(s, end, p, &tags);
                                buf.insert(end, "\n");
                            }
                            // Una sublista lleva una guía vertical bajo la viñeta del ítem.
                            BlockKind::List { .. } => {
                                let inicio = buf.create_mark(None, end, true);
                                self.block(s, end, sub, depth + 1);
                                s.capas.borrow_mut().push(super::capas::Capa::Guia {
                                    inicio,
                                    fin: buf.create_mark(None, end, true),
                                    profundidad: depth + 1,
                                });
                            }
                            _ => self.block(s, end, sub, depth + 1),
                        }
                        if k == 0 && plegable.is_some() {
                            desde = Some(buf.create_mark(None, end, true));
                        }
                    }
                    if item.blocks.is_empty() {
                        buf.insert(end, "\n");
                    }
                    if let Some(d) = desde {
                        s.cerrar_item(indice, d, buf.create_mark(None, end, true));
                    }
                    // La sangría cubre también la casilla anclada al principio de la línea.
                    let first = buf.iter_at_offset(item_start);
                    let mut line_end = first;
                    if !line_end.ends_line() {
                        line_end.forward_to_line_end();
                    }
                    buf.apply_tag(&tag, &first, &line_end);
                    s.foco.set(foco);
                }
            }
            BlockKind::Quote(body) => {
                for sub in body {
                    let start = end.offset();
                    self.block(s, end, sub, depth + 1);
                    if let Some(dim) = table.lookup("dim") {
                        buf.apply_tag(&dim, &buf.iter_at_offset(start), end);
                    }
                }
            }
            BlockKind::Callout {
                kind,
                title,
                folded,
                body,
            } => self.callout(s, end, kind, title, *folded, body, depth, b.source.start),
            BlockKind::Code { lang, text } if lang == "grafico" => {
                self.grafico(s, end, text, &base)
            }
            BlockKind::Code { lang, text } if lang == "mermaid" => {
                self.diagrama(s, end, text, &base)
            }
            BlockKind::Carrusel(c) => self.carrusel(s, end, c, &base),
            BlockKind::Galeria(g) => self.galeria(s, end, g, &base),
            BlockKind::Code { lang, text } => {
                let mut tags = base.clone();
                tags.extend(table.lookup("codeblock"));
                let refs: Vec<&gtk::TextTag> = tags.iter().collect();
                let body = text.strip_suffix('\n').unwrap_or(text);
                let inicio = end.offset();
                buf.insert_with_tags(end, body, &refs);
                // Los colores de Código (055) van como etiquetas sobre el texto ya puesto: copiar
                // y buscar siguen viendo el mismo texto plano. Si el bloque no está en la caché,
                // se resalta después, en reposo (`resaltar`).
                let hecho = if s.provisorio.get() {
                    crate::resaltado::en_el_acto(lang, body)
                } else {
                    crate::resaltado::en_cache(lang, body)
                };
                match hecho {
                    Some(Some(tramos)) => pintar_codigo(buf, inicio, &tramos),
                    Some(None) => {}
                    None => s.codigo.borrow_mut().push((lang.clone(), body.to_owned())),
                }
                buf.insert(end, "\n");
            }
            // Hasta que la vista complete el contenido (T015/T019), el bloque generado se ve
            // como su codeblock fuente.
            BlockKind::Generated(g) => self.block(
                s,
                end,
                &Block {
                    kind: BlockKind::Code {
                        lang: String::new(),
                        text: g.raw.clone(),
                    },
                    source: g.source.clone(),
                    line: b.line,
                    id: None,
                },
                depth,
            ),
            BlockKind::Table {
                align,
                header,
                rows,
            } => self.table(s, end, align, header, rows, &base),
            BlockKind::Rule => {
                let space: Vec<gtk::TextTag> = table.lookup("rule-space").into_iter().collect();
                let rule: Vec<gtk::TextTag> = table.lookup("rule").into_iter().collect();
                buf.insert_with_tags(end, "\n", &space.iter().collect::<Vec<_>>());
                buf.insert_with_tags(end, "\u{200b}\n", &rule.iter().collect::<Vec<_>>());
                buf.insert_with_tags(end, "\n", &space.iter().collect::<Vec<_>>());
            }
            BlockKind::Html(text) => {
                // Ocultar en Lectura los delimitadores de bloque `<!-- block ... -->` y `<!---->` (040),
                // y mostrar etiqueta de sección para `summary`, `connections` y `sources` (041).
                let mut resto_html: Vec<&str> = Vec::new();
                for l in text.lines() {
                    if crate::grafo::ramas_libres::es_delimitador_bloque_comentario(l) {
                        continue;
                    }
                    if let Some(etq) =
                        crate::grafo::notas_tres_partes::etiqueta_apertura_seccion_lectura(l)
                    {
                        let mut tags_sec = base.clone();
                        tags_sec.extend(table.lookup("callout-title"));
                        tags_sec.extend(table.lookup("dim"));
                        let refs_sec: Vec<&gtk::TextTag> = tags_sec.iter().collect();
                        buf.insert_with_tags(end, etq, &refs_sec);
                        buf.insert(end, "\n");
                    } else {
                        resto_html.push(l);
                    }
                }
                if !resto_html.is_empty() {
                    let limpio = resto_html.join("\n");
                    if !limpio.trim().is_empty() {
                        let mut tags = base.clone();
                        tags.extend(table.lookup("code"));
                        let refs: Vec<&gtk::TextTag> = tags.iter().collect();
                        buf.insert_with_tags(end, limpio.trim_end(), &refs);
                        buf.insert(end, "\n");
                    }
                }
            }
            BlockKind::Transclusion(t) => self.transclusion(s, end, t, depth, &base),
            BlockKind::Group(body) => {
                for sub in body {
                    self.block(s, end, sub, depth);
                }
            }
            BlockKind::Seccion { kind, body } => self.seccion(s, end, *kind, body, depth),
            BlockKind::Flashcard(f) => self.flashcard(s, end, f, depth, b.source.start),
            BlockKind::Math { tex } => self.formula_bloque(s, end, tex, &base),
        }
        self.vaciar_medios(s, end, &base);
    }

    /// Los medios que quedaron de los párrafos del bloque, cada uno en su renglón (057).
    fn vaciar_medios(&self, s: &RenderState, end: &mut gtk::TextIter, base: &[gtk::TextTag]) {
        if s.medios_en_linea.borrow().is_empty() {
            return;
        }
        let medios = std::mem::take(&mut *s.medios_en_linea.borrow_mut());
        let mut tags = base.to_vec();
        tags.extend(s.buffer.tag_table().lookup("medio"));
        let refs: Vec<&gtk::TextTag> = tags.iter().collect();
        for (path, tipo, fragmento) in medios {
            self.medio(s, end, &path, tipo, fragmento.as_deref());
            // La etiqueta del renglón (aire arriba y abajo) también sobre el ancla.
            let mut a = *end;
            a.backward_char();
            for t in &tags {
                s.buffer.apply_tag(t, &a, end);
            }
            s.buffer.insert_with_tags(end, "\n", &refs);
        }
    }

    /// Una parte especial de la nota (041) como estructura propia, igual que un callout: el texto
    /// deja el lugar (márgenes y renglones de relleno) y `capas` dibuja la tarjeta, el ícono y las
    /// píldoras. Sin widgets: las píldoras de las Conexiones son enlaces de texto.
    pub(super) fn seccion(
        &self,
        s: &RenderState,
        end: &mut gtk::TextIter,
        kind: SeccionKind,
        body: &[Block],
        depth: i32,
    ) {
        use super::capas::{CALLOUT_TITULO, Capa};
        let buf = &s.buffer;
        let table = buf.tag_table();
        let inicio = buf.create_mark(None, end, true);
        let start = end.offset();
        // Encabezado chico con lugar para el ícono.
        let mut ttags: Vec<gtk::TextTag> = vec![self.indent_tag(depth * 24 + CALLOUT_TITULO)];
        ttags.extend(table.lookup("callout-title"));
        ttags.extend(table.lookup("callout-cab"));
        ttags.extend(table.lookup(match kind {
            SeccionKind::Sintesis => "seccion-cab-acento",
            _ => "seccion-cab",
        }));
        buf.insert_with_tags(end, kind.label(), &ttags.iter().collect::<Vec<_>>());
        buf.insert_with_tags(end, "\n", &ttags.iter().collect::<Vec<_>>());
        // Un encabezado al principio que repite el nombre de la parte («## Síntesis») ya lo
        // dice la tarjeta.
        let body = match body.first().map(|b| &b.kind) {
            Some(BlockKind::Heading { inlines, .. })
                if repite_nombre(kind, &plain_text(inlines)) =>
            {
                &body[1..]
            }
            _ => body,
        };
        let body_start = end.offset();
        let mut chips: Vec<(gtk::TextMark, gtk::TextMark)> = Vec::new();
        match kind {
            SeccionKind::Conexiones => self.conexiones(s, end, body, depth + 1, &mut chips),
            SeccionKind::Fuentes => {
                for sub in body {
                    // La bibliografía va numerada.
                    if let BlockKind::List { start: None, items } = &sub.kind {
                        let numerada = Block {
                            kind: BlockKind::List {
                                start: Some(1),
                                items: items.clone(),
                            },
                            source: sub.source.clone(),
                            line: sub.line,
                            id: None,
                        };
                        self.block(s, end, &numerada, depth + 1);
                    } else {
                        self.block(s, end, sub, depth + 1);
                    }
                }
            }
            SeccionKind::Sintesis => {
                for sub in body {
                    self.block(s, end, sub, depth + 1);
                }
            }
        }
        let cuerpo = match kind {
            SeccionKind::Sintesis => "seccion-sintesis",
            SeccionKind::Fuentes => "seccion-fuentes",
            SeccionKind::Conexiones => "",
        };
        if let Some(tag) = table.lookup(cuerpo) {
            buf.apply_tag(&tag, &buf.iter_at_offset(body_start), end);
        }
        let pie: Vec<gtk::TextTag> = table.lookup("callout-pie").into_iter().collect();
        buf.insert_with_tags(end, "\n", &pie.iter().collect::<Vec<_>>());
        if let Some(der) = table.lookup("callout-der") {
            buf.apply_tag(&der, &buf.iter_at_offset(start), end);
        }
        s.capas.borrow_mut().push(Capa::Seccion {
            inicio,
            fin: buf.create_mark(None, end, true),
            kind,
            chips,
            profundidad: depth,
            der: s.embedded.get() as i32 * super::capas::INCRUSTADO_DER,
        });
    }

    /// Las Conexiones. Primero las relaciones con tipo (049), agrupadas por tipo en el orden en
    /// que aparece cada uno: el tipo como rótulo rosa y, debajo, un renglón por ítem con sus
    /// píldoras y el motivo en texto tenue. Después, lo demás como antes de la 049.
    fn conexiones(
        &self,
        s: &RenderState,
        end: &mut gtk::TextIter,
        body: &[Block],
        depth: i32,
        chips: &mut Vec<(gtk::TextMark, gtk::TextMark)>,
    ) {
        let (grupos, resto) = agrupar_tipadas(body);
        if grupos.is_empty() {
            return self.conexiones_sin_tipo(s, end, body, depth, chips);
        }
        let buf = &s.buffer;
        let table = buf.tag_table();
        let tags = |extra: &[&str], indent: i32| -> Vec<gtk::TextTag> {
            let mut v = vec![self.indent_tag(indent)];
            v.extend(table.lookup("seccion-chips"));
            v.extend(extra.iter().filter_map(|n| table.lookup(n)));
            v
        };
        let titulo = tags(&["seccion-cab-acento"], depth * 24);
        let fila = tags(&[], (depth + 1) * 24);
        let motivo = tags(&["seccion-rotulo"], (depth + 1) * 24);
        let pildora = tags(&["chip"], (depth + 1) * 24);
        fn r(t: &[gtk::TextTag]) -> Vec<&gtk::TextTag> {
            t.iter().collect()
        }
        for g in &grupos {
            buf.insert_with_tags(end, &g.nombre, &r(&titulo));
            buf.insert_with_tags(end, "\n", &r(&titulo));
            for (enlaces, mot) in &g.items {
                for (n, i) in enlaces.iter().enumerate() {
                    if n > 0 {
                        buf.insert_with_tags(end, "\u{2003}\u{2002}", &r(&fila));
                    }
                    let a = buf.create_mark(None, end, true);
                    self.inlines(s, end, std::slice::from_ref(*i), &pildora);
                    chips.push((a, buf.create_mark(None, end, true)));
                }
                if let Some(m) = mot {
                    buf.insert_with_tags(end, "\u{2002}\u{2002}— ", &r(&motivo));
                    buf.insert_with_tags(end, m, &r(&motivo));
                }
                buf.insert_with_tags(end, "\n", &r(&fila));
            }
        }
        if !resto.is_empty() {
            self.conexiones_sin_tipo(s, end, &resto, depth, chips);
        }
    }

    /// Las Conexiones sin tipo: cada enlace es una píldora (texto enlazado; el fondo lo dibuja
    /// `capas`) y las píldoras corren seguidas, con el texto de cada ítem («Prerrequisito:») como
    /// rótulo tenue antes de las suyas. Lo que no tiene enlaces se muestra como siempre.
    fn conexiones_sin_tipo(
        &self,
        s: &RenderState,
        end: &mut gtk::TextIter,
        body: &[Block],
        depth: i32,
        chips: &mut Vec<(gtk::TextMark, gtk::TextMark)>,
    ) {
        let buf = &s.buffer;
        let table = buf.tag_table();
        let mut fila: Vec<gtk::TextTag> = vec![self.indent_tag(depth * 24)];
        fila.extend(table.lookup("seccion-chips"));
        let mut rotulo = fila.clone();
        rotulo.extend(table.lookup("seccion-rotulo"));
        let mut pildora = fila.clone();
        pildora.extend(table.lookup("chip"));
        let refs = |t: &[gtk::TextTag]| -> Vec<gtk::TextTag> { t.to_vec() };
        // Los párrafos con enlaces (sueltos o de ítems, a cualquier profundidad), en orden.
        fn parrafos<'b>(blocks: &'b [Block], out: &mut Vec<&'b [Inline]>) -> bool {
            let mut todos = true;
            for b in blocks {
                match &b.kind {
                    BlockKind::Paragraph(p)
                        if p.iter().any(|i| matches!(i, Inline::Link { .. })) =>
                    {
                        out.push(p)
                    }
                    BlockKind::List { items, .. } => {
                        for it in items {
                            todos &= parrafos(&it.blocks, out);
                        }
                    }
                    _ => todos = false,
                }
            }
            todos
        }
        let mut grupos: Vec<&[Inline]> = Vec::new();
        let mut abierta = false;
        for b in body {
            grupos.clear();
            let solo_enlaces = parrafos(std::slice::from_ref(b), &mut grupos);
            if grupos.is_empty() || !solo_enlaces {
                if abierta {
                    buf.insert_with_tags(end, "\n", &fila.iter().collect::<Vec<_>>());
                    abierta = false;
                }
                self.block(s, end, b, depth);
                continue;
            }
            for p in &grupos {
                let texto: String = p
                    .iter()
                    .filter(|i| !matches!(i, Inline::Link { .. }))
                    .map(|i| plain_text(std::slice::from_ref(i)))
                    .collect();
                let texto = texto
                    .trim()
                    .trim_end_matches([':', '-', '—', '–', ','])
                    .trim();
                if abierta {
                    buf.insert_with_tags(end, "\u{2003}\u{2003}", &fila.iter().collect::<Vec<_>>());
                }
                abierta = true;
                if !texto.is_empty() {
                    buf.insert_with_tags(end, texto, &rotulo.iter().collect::<Vec<_>>());
                    buf.insert_with_tags(end, "\u{2002}\u{2002}", &fila.iter().collect::<Vec<_>>());
                }
                let mut primera = true;
                for i in p.iter().filter(|i| matches!(i, Inline::Link { .. })) {
                    if !primera {
                        buf.insert_with_tags(
                            end,
                            "\u{2002}\u{2002}",
                            &fila.iter().collect::<Vec<_>>(),
                        );
                    }
                    primera = false;
                    let a = buf.create_mark(None, end, true);
                    self.inlines(s, end, std::slice::from_ref(i), &refs(&pildora));
                    chips.push((a, buf.create_mark(None, end, true)));
                }
            }
        }
        if abierta {
            buf.insert_with_tags(end, "\n", &fila.iter().collect::<Vec<_>>());
        }
    }

    /// Tarjeta de una transclusión (US5 de la 003): una franja de encabezado con el nombre de la
    /// nota de origen (un enlace que la abre) y, debajo, su contenido con el mismo formato que en
    /// la nota (callouts, tarjetas, partes especiales…), más adentro. El borde y la franja los
    /// dibuja `capas`; el texto solo deja el lugar. Lo incrustado no se edita desde acá: sus
    /// casillas quedan deshabilitadas.
    pub(super) fn transclusion(
        &self,
        s: &RenderState,
        end: &mut gtk::TextIter,
        t: &orbital_vault::render::Transclusion,
        depth: i32,
        _base: &[gtk::TextTag],
    ) {
        use super::capas::{CALLOUT_TITULO, Capa, INCRUSTADO_DER, NIVELES_DER};
        use orbital_vault::render::Embed;
        let buf = &s.buffer;
        let table = buf.tag_table();
        // Imágenes, audio y video (057): el archivo en el lugar, sin tarjeta.
        match &t.content {
            Embed::Archivo { abs, tipo, .. } => {
                let mut tags = _base.to_vec();
                tags.extend(table.lookup("medio"));
                self.medio(s, end, abs, *tipo, t.anchor.as_deref());
                buf.insert_with_tags(end, "\n", &tags.iter().collect::<Vec<_>>());
                return;
            }
            Embed::Missing if orbital_vault::medios::tipo_de(&t.target).is_some() => {
                self.archivo_inexistente(s, end, &t.target, _base);
                buf.insert(end, "\n");
                return;
            }
            _ => {}
        }
        let nivel = s.embedded.get() as i32;
        let inicio = buf.create_mark(None, end, true);
        let start = end.offset();
        let url = match &t.content {
            Embed::Note { path, .. }
                if path.file_name().to_ascii_lowercase().ends_with(".base") =>
            {
                orbital_vault::bases::path_url(path)
            }
            _ => match &t.anchor {
                Some(a) => format!("[[{}#{a}]]", t.target),
                None => format!("[[{}]]", t.target),
            },
        };
        // El nombre de la nota sin carpeta ni extensión; el ancla va aparte, tenue.
        let nombre = match &t.content {
            Embed::Note { path, .. }
            | Embed::Html { path }
            | Embed::Cycle { path }
            | Embed::SectionMissing { path }
            | Embed::BlockMissing { path } => {
                let f = path.file_name();
                f.rsplit_once('.').map_or(f, |(n, _)| n).to_owned()
            }
            _ => t.target.rsplit('/').next().unwrap_or(&t.target).to_owned(),
        };
        let ancla = t.anchor.as_deref().map(|a| match a.strip_prefix('^') {
            Some(id) => format!("  ·  bloque {id}"),
            None => format!("  ›  {a}"),
        });
        let link = |text: String| orbital_vault::render::Inline::Link {
            url: url.clone(),
            children: vec![orbital_vault::render::Inline::Text(text)],
        };
        // El renglón del encabezado: sangría con lugar para el ícono.
        let mut cab: Vec<gtk::TextTag> = vec![self.indent_tag(depth * 24 + CALLOUT_TITULO)];
        cab.extend(table.lookup("embed-cab"));
        let mut title = cab.clone();
        title.extend(table.lookup("callout-title"));
        let mut dim = cab.clone();
        dim.extend(table.lookup("dim"));
        let text = |end: &mut gtk::TextIter, tags: &[gtk::TextTag], t: &str| {
            buf.insert_with_tags(end, t, &tags.iter().collect::<Vec<_>>());
        };
        let mut cuerpo: Option<gtk::TextMark> = None;
        match &t.content {
            Embed::Note { blocks, .. } => {
                self.inlines(s, end, &[link(nombre)], &title);
                if let Some(a) = &ancla {
                    text(end, &dim, a);
                }
                text(end, &cab, "\n");
                cuerpo = Some(buf.create_mark(None, end, true));
                s.embedded.set(s.embedded.get() + 1);
                for b in blocks {
                    self.block(s, end, b, depth + 1);
                }
                s.embedded.set(s.embedded.get() - 1);
            }
            Embed::Missing => {
                text(end, &dim, "Nota inexistente: ");
                self.inlines(s, end, &[link(t.target.clone())], &title);
                text(end, &dim, " (creala desde el enlace)\n");
            }
            Embed::BlockMissing { .. } => {
                self.inlines(s, end, &[link(nombre)], &title);
                let a = t.anchor.as_deref().unwrap_or("");
                text(end, &dim, &format!(" no tiene el bloque «{a}»\n"));
            }
            Embed::SectionMissing { .. } => {
                self.inlines(s, end, &[link(nombre)], &title);
                let a = t.anchor.as_deref().unwrap_or("");
                text(end, &dim, &format!(" no tiene el encabezado «{a}»\n"));
            }
            Embed::Html { .. } => {
                self.inlines(s, end, &[link(nombre)], &title);
                text(end, &dim, " es una nota HTML: abrila para verla\n");
            }
            Embed::Ambiguous(all) => {
                text(
                    end,
                    &dim,
                    &format!(
                        "Hay {} notas «{}»: escribí la carpeta, por ejemplo ![[{}]]\n",
                        all.len(),
                        t.target,
                        all[0]
                            .as_str()
                            .strip_prefix("wiki/")
                            .unwrap_or(all[0].as_str())
                            .rsplit_once('.')
                            .map_or("", |(p, _)| p),
                    ),
                );
            }
            Embed::Cycle { .. } => {
                text(end, &dim, "↻ Referencia circular a ");
                self.inlines(s, end, &[link(nombre)], &title);
                text(end, &dim, ": ya se está mostrando más arriba\n");
            }
            Embed::TooDeep => {
                text(
                    end,
                    &dim,
                    &format!(
                        "Demasiados niveles de transclusión (máximo {}): ",
                        orbital_vault::transclusion::MAX_DEPTH
                    ),
                );
                self.inlines(s, end, &[link(nombre)], &title);
                text(end, &cab, "\n");
            }
            Embed::Pending => {
                text(end, &dim, &format!("![[{}]]\n", t.target));
            }
            // Ya se mostró arriba, sin tarjeta.
            Embed::Archivo { .. } => {}
        }
        // Relleno de abajo (dentro del borde) y el margen derecho de este nivel. Sin contenido
        // (un aviso), el relleno es el del propio renglón: queda centrado en el recuadro.
        let cuerpo = cuerpo.filter(|c| buf.iter_at_mark(c).offset() < end.offset());
        if cuerpo.is_some() {
            let pie: Vec<gtk::TextTag> = table.lookup("callout-pie").into_iter().collect();
            buf.insert_with_tags(end, "\n", &pie.iter().collect::<Vec<_>>());
        } else if let Some(solo) = table.lookup("embed-solo") {
            buf.apply_tag(&solo, &buf.iter_at_offset(start), end);
        }
        let der = table
            .lookup(&format!("der-{}", (nivel + 1).min(NIVELES_DER)))
            .or_else(|| table.lookup("callout-der"));
        if let Some(der) = der {
            buf.apply_tag(&der, &buf.iter_at_offset(start), end);
        }
        s.capas.borrow_mut().push(Capa::Incrustacion {
            inicio,
            fin: buf.create_mark(None, end, true),
            cuerpo,
            profundidad: depth,
            der: nivel * INCRUSTADO_DER,
        });
    }

    /// Una tarjeta de estudio (024) como tarjeta propia: un rótulo con su número e id, la
    /// pregunta destacada y la respuesta oculta hasta un clic (o Intro) en «Mostrar respuesta»,
    /// con el mismo plegado barato de los callouts: una etiqueta invisible y un enlace interno.
    /// El fondo, el borde, la línea y la píldora del botón los dibuja `capas`.
    pub(super) fn flashcard(
        &self,
        s: &RenderState,
        end: &mut gtk::TextIter,
        f: &orbital_vault::render::Flashcard,
        depth: i32,
        origen: usize,
    ) {
        use super::capas::{CALLOUT_TITULO, Capa, INCRUSTADO_DER};
        let buf = &s.buffer;
        let table = buf.tag_table();
        let inicio = buf.create_mark(None, end, true);
        let start = end.offset();
        let tags = |extra: &[&str], mut base: Vec<gtk::TextTag>| -> Vec<gtk::TextTag> {
            base.extend(extra.iter().filter_map(|n| table.lookup(n)));
            base
        };
        let insertar = |end: &mut gtk::TextIter, t: &str, tags: &[gtk::TextTag]| {
            buf.insert_with_tags(end, t, &tags.iter().collect::<Vec<_>>());
        };
        // El rótulo: «Tarjeta 3 de 16 · fc-003».
        let mut rotulo = format!("Tarjeta {} de {}", f.numero, f.total);
        if let Some(id) = &f.id {
            rotulo.push_str("  ·  ");
            rotulo.push_str(id);
        }
        let rtags = tags(
            &["callout-cab", "tarjeta-rotulo"],
            vec![self.indent_tag(depth * 24 + CALLOUT_TITULO)],
        );
        insertar(end, &rotulo, &rtags);
        insertar(end, "\n", &rtags);
        let pregunta = end.offset();
        for b in &f.question {
            self.block(s, end, b, depth + 1);
        }
        if let Some(tag) = table.lookup("tarjeta-pregunta") {
            buf.apply_tag(&tag, &buf.iter_at_offset(pregunta), end);
        }
        let n = self.0.callouts.get() + 1;
        self.0.callouts.set(n);
        let nombre = format!("tarjeta-{n}");
        // Abierta antes de volver a volcarse (al desplegar su sección), sigue abierta (046).
        let abierta = self.alternado(s, &nombre, origen);
        let respuesta = self.new_tag(
            s,
            gtk::TextTag::builder()
                .name(&nombre)
                .invisible(!abierta)
                .build(),
        );
        let marca = |end: &gtk::TextIter| buf.create_mark(None, end, true);
        let (mostrar, ocultar) = if f.answer.is_empty() {
            // Sin respuesta no hay botón: marcas vacías, la capa no dibuja la píldora.
            let m = marca(end);
            ((m.clone(), m.clone()), (m.clone(), m))
        } else {
            let url = format!("{URL_CALLOUT}{nombre}");
            let enlace = self.link_tag(s, &url);
            let mut ver = gtk::TextTag::builder().name(format!("{nombre}-mostrar"));
            if abierta {
                ver = ver.invisible(true);
            }
            let ver = self.new_tag(s, ver.build());
            let esconder = self.new_tag(
                s,
                gtk::TextTag::builder()
                    .name(format!("{nombre}-ocultar"))
                    .invisible(!abierta)
                    .build(),
            );
            let linea = tags(
                &["tarjeta-boton"],
                vec![self.indent_tag((depth + 1) * 24 + 12)],
            );
            let mut boton = tags(&["chip", "tarjeta-boton-texto"], linea.clone());
            boton.push(enlace);
            let a = marca(end);
            insertar(
                end,
                "Mostrar respuesta",
                &[boton.clone(), vec![ver]].concat(),
            );
            let b = marca(end);
            insertar(end, "Ocultar respuesta", &[boton, vec![esconder]].concat());
            let c = marca(end);
            insertar(end, "\n", &linea);
            // Con el teclado: Tab llega al botón e Intro lo acciona.
            s.link_items.borrow_mut().push(LinkItem {
                url,
                start: a.clone(),
                end: c.clone(),
            });
            ((a, b.clone()), (b, c))
        };
        let resp = end.offset();
        for b in &f.answer {
            self.block(s, end, b, depth + 1);
        }
        buf.apply_tag(&respuesta, &buf.iter_at_offset(resp), end);
        let pie: Vec<gtk::TextTag> = table.lookup("callout-pie").into_iter().collect();
        insertar(end, "\n", &pie);
        if let Some(der) = table.lookup("callout-der") {
            buf.apply_tag(&der, &buf.iter_at_offset(start), end);
        }
        s.capas.borrow_mut().push(Capa::Tarjeta {
            inicio,
            fin: buf.create_mark(None, end, true),
            mostrar,
            ocultar,
            respuesta,
            profundidad: depth,
            der: s.embedded.get() as i32 * INCRUSTADO_DER,
        });
    }

    /// Casilla de una tarea: un `CheckButton` que cambia un solo byte del texto fuente.
    pub(super) fn task(
        &self,
        s: &RenderState,
        end: &mut gtk::TextIter,
        item: &orbital_vault::render::ListItem,
        task: orbital_vault::render::TaskMark,
    ) {
        let check = gtk::CheckButton::builder()
            .active(task.checked)
            .valign(gtk::Align::Center)
            .build();
        if s.embedded.get() == 0 {
            check.set_cursor_from_name(Some("pointer"));
        }
        // Una casilla de una nota incrustada no es de esta nota: no se marca desde acá.
        if s.embedded.get() > 0 {
            check.set_sensitive(false);
            check.update_property(&[gtk::accessible::Property::Label(&format!(
                "Tarea de otra nota: {}",
                item.blocks
                    .first()
                    .map(|b| match &b.kind {
                        BlockKind::Paragraph(p) => plain_text(p),
                        _ => String::new(),
                    })
                    .unwrap_or_default()
            ))]);
            self.anchor(s, end, check.upcast());
            s.buffer.insert(end, " ");
            return;
        }
        let label = item
            .blocks
            .first()
            .map(|b| match &b.kind {
                BlockKind::Paragraph(p) => plain_text(p),
                _ => String::new(),
            })
            .unwrap_or_default();
        check.update_property(&[gtk::accessible::Property::Label(&format!("Tarea: {label}"))]);
        // Relativo al bloque: si se edita algo antes, el bloque se conserva y su base cambia.
        let rel = task.offset - s.block_start.get();
        let base = s.block_base.borrow().clone();
        let weak = Rc::downgrade(&self.0);
        check.connect_toggled(move |_| {
            if let Some(i) = weak.upgrade()
                && let Some(f) = i.on_toggle.borrow().as_ref()
            {
                f(base.get() + rel);
            }
        });
        self.anchor(s, end, check.upcast());
        s.buffer.insert(end, " ");
    }

    /// Callout (caja de aviso con tipo): el texto deja el lugar (márgenes, rellenos y un renglón bajo
    /// al final) y `capas` dibuja el fondo redondeado, el ícono y la flechita. Uno plegable se
    /// abre y cierra con un clic en el título (un «enlace» interno), sin widgets.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn callout(
        &self,
        s: &RenderState,
        end: &mut gtk::TextIter,
        kind: &CalloutKind,
        title: &str,
        folded: Option<bool>,
        body: &[Block],
        depth: i32,
        origen: usize,
    ) {
        use super::capas::{CALLOUT_TITULO, Capa, estilo_callout};
        let buf = &s.buffer;
        let table = buf.tag_table();
        let (color, icono) = estilo_callout(kind);
        let inicio = buf.create_mark(None, end, true);
        let start = end.offset();
        // El título: en el color del tipo, con lugar a la izquierda para el ícono.
        let mut ttags: Vec<gtk::TextTag> = vec![self.indent_tag(depth * 24 + CALLOUT_TITULO)];
        ttags.extend(table.lookup("callout-title"));
        ttags.extend(table.lookup("callout-cab"));
        ttags.push(self.color_tag(color));
        let cuerpo = folded.map(|f| {
            let n = self.0.callouts.get() + 1;
            self.0.callouts.set(n);
            let nombre = format!("callout-cuerpo-{n}");
            // Abierto o cerrado a mano antes de volver a volcarse, queda como estaba (046).
            let f = f != self.alternado(s, &nombre, origen);
            let tag = self.new_tag(
                s,
                gtk::TextTag::builder().name(&nombre).invisible(f).build(),
            );
            let url = format!("{URL_CALLOUT}{nombre}");
            ttags.push(self.link_tag(s, &url));
            (tag, url)
        });
        let trefs: Vec<&gtk::TextTag> = ttags.iter().collect();
        let titulo_inicio = buf.create_mark(None, end, true);
        buf.insert_with_tags(end, title, &trefs);
        let titulo_fin = buf.create_mark(None, end, true);
        if let Some((_, url)) = &cuerpo {
            // Lugar para la flechita, clicable como el título; Tab llega al título.
            buf.insert_with_tags(end, "\u{2003}\u{2003}", &trefs);
            s.link_items.borrow_mut().push(LinkItem {
                url: url.clone(),
                start: titulo_inicio,
                end: titulo_fin.clone(),
            });
        }
        buf.insert_with_tags(end, "\n", &trefs);
        let body_start = end.offset();
        for sub in body {
            self.block(s, end, sub, depth + 1);
        }
        if let Some((tag, _)) = &cuerpo {
            buf.apply_tag(tag, &buf.iter_at_offset(body_start), end);
        }
        // Relleno de abajo (dentro del fondo) y separación con lo que sigue.
        let pie: Vec<gtk::TextTag> = table.lookup("callout-pie").into_iter().collect();
        buf.insert_with_tags(end, "\n", &pie.iter().collect::<Vec<_>>());
        if let Some(der) = table.lookup("callout-der") {
            buf.apply_tag(&der, &buf.iter_at_offset(start), end);
        }
        s.capas.borrow_mut().push(Capa::Callout {
            inicio,
            fin: buf.create_mark(None, end, true),
            titulo_fin,
            color,
            icono,
            cuerpo: cuerpo.map(|(t, _)| t),
            profundidad: depth,
            der: s.embedded.get() as i32 * super::capas::INCRUSTADO_DER,
        });
    }

    /// Etiqueta compartida de texto de un color (se crea una vez por color).
    fn color_tag(&self, c: crate::style::Rgb) -> gtk::TextTag {
        let table = self.buffer().tag_table();
        let nombre = format!("color-{}", c.hex());
        if let Some(t) = table.lookup(&nombre) {
            return t;
        }
        let t = gtk::TextTag::builder()
            .name(&nombre)
            .foreground(c.hex())
            .build();
        table.add(&t);
        t
    }

    /// Tabla como texto con tabulaciones: sin widgets, seleccionable y barata de dibujar.
    pub(super) fn table(
        &self,
        s: &RenderState,
        end: &mut gtk::TextIter,
        align: &[Align],
        header: &[Vec<Inline>],
        rows: &[Vec<Vec<Inline>>],
        base: &[gtk::TextTag],
    ) {
        const PAD: i32 = 12;
        const GAP: i32 = 28;
        let buf = &s.buffer;
        let table = buf.tag_table();
        let text = |cells: &[Vec<Inline>]| -> Vec<String> {
            cells
                .iter()
                .map(|c| plain_text(c).replace(['\t', '\n'], " "))
                .collect()
        };
        let head = text(header);
        let body: Vec<Vec<String>> = rows.iter().map(|r| text(r)).collect();
        let cols = head.len().max(body.iter().map(Vec::len).max().unwrap_or(0));
        let mut widths = vec![0; cols];
        let layout = self.0.view.create_pango_layout(None);
        let measure = |t: &str, bold: bool| {
            if bold {
                layout.set_markup(&format!("<b>{}</b>", glib::markup_escape_text(t)));
            } else {
                layout.set_text(t);
            }
            layout.pixel_size().0
        };
        for (c, t) in head.iter().enumerate() {
            widths[c] = widths[c].max(measure(t, true));
        }
        for row in &body {
            for (c, t) in row.iter().enumerate() {
                widths[c] = widths[c].max(measure(t, false));
            }
        }
        let mut tabs = pango::TabArray::new(cols as i32, true);
        let mut x = PAD;
        for (c, w) in widths.iter().enumerate() {
            let (a, pos) = match align.get(c) {
                Some(Align::Right) => (pango::TabAlign::Right, x + w),
                Some(Align::Center) => (pango::TabAlign::Center, x + w / 2),
                _ => (pango::TabAlign::Left, x),
            };
            tabs.set_tab(c as i32, a, pos);
            x += w + GAP;
        }
        let tab_tag = self.new_tag(s, gtk::TextTag::builder().tabs(&tabs).build());
        let mut tags: Vec<gtk::TextTag> = base.to_vec();
        tags.extend(table.lookup("table"));
        tags.push(tab_tag);
        let line =
            |end: &mut gtk::TextIter, cells: &[String], inlines: &[Vec<Inline>], bold: bool| {
                let mut t = tags.clone();
                if bold {
                    t.extend(table.lookup("table-head"));
                }
                // Una fila de texto solo va de una vez; con enlaces u otro formato (las tablas de
                // una consulta, 048) cada celda se vuelca con sus etiquetas.
                if inlines
                    .iter()
                    .all(|c| c.iter().all(|i| matches!(i, Inline::Text(_))))
                {
                    let mut l = String::new();
                    for c in 0..cols {
                        l.push('\t');
                        l.push_str(cells.get(c).map(String::as_str).unwrap_or(""));
                    }
                    l.push('\n');
                    buf.insert_with_tags(end, &l, &t.iter().collect::<Vec<_>>());
                    return;
                }
                let refs: Vec<&gtk::TextTag> = t.iter().collect();
                for c in 0..cols {
                    buf.insert_with_tags(end, "\t", &refs);
                    if let Some(cell) = inlines.get(c) {
                        self.inlines(s, end, &celda(cell), &t);
                    }
                }
                buf.insert_with_tags(end, "\n", &refs);
            };
        line(end, &head, header, true);
        for (row, cells) in body.iter().zip(rows) {
            line(end, row, cells, false);
        }
    }

    pub(super) fn inlines(
        &self,
        s: &RenderState,
        end: &mut gtk::TextIter,
        inlines: &[Inline],
        tags: &[gtk::TextTag],
    ) {
        let buf = &s.buffer;
        let table = buf.tag_table();
        for inl in inlines {
            match inl {
                Inline::Text(t) => {
                    if !crate::grafo::ramas_libres::es_delimitador_bloque_comentario(t) {
                        let refs: Vec<&gtk::TextTag> = tags.iter().collect();
                        buf.insert_with_tags(end, t, &refs);
                    }
                }
                Inline::FootnoteRef(nombre) => {
                    // Superíndice con tooltip del contenido (012, US1).
                    let tag_name = format!("fnref-{nombre}");
                    if table.lookup(&tag_name).is_none() {
                        let tag = gtk::TextTag::builder()
                            .name(&tag_name)
                            .rise(-8192)
                            .scale(0.75)
                            .foreground(crate::style::LINK_FG.hex())
                            .build();
                        table.add(&tag);
                    }
                    let mut more = tags.to_vec();
                    if let Some(tag) = table.lookup(&tag_name) {
                        more.push(tag);
                    }
                    let refs: Vec<&gtk::TextTag> = more.iter().collect();
                    buf.insert_with_tags(end, &format!("[^{nombre}]"), &refs);
                }
                Inline::Code(t) => {
                    let mut more = tags.to_vec();
                    more.extend(table.lookup("code"));
                    let refs: Vec<&gtk::TextTag> = more.iter().collect();
                    buf.insert_with_tags(end, t, &refs);
                }
                Inline::Strong(c) | Inline::Emph(c) | Inline::Strike(c) => {
                    let name = match inl {
                        Inline::Strong(_) => "strong",
                        Inline::Emph(_) => "emph",
                        _ => "strike",
                    };
                    let mut more = tags.to_vec();
                    more.extend(table.lookup(name));
                    self.inlines(s, end, c, &more);
                }
                Inline::Link { url, children } => {
                    let is_missing = self.0.is_missing.borrow().as_ref().is_some_and(|f| f(url));
                    let tag_name = if is_missing { "link-missing" } else { "link" };
                    let start_mark = buf.create_mark(None, end, true);
                    let tag = self.link_tag(s, url);
                    let mut more = tags.to_vec();
                    more.extend(table.lookup(tag_name));
                    more.push(tag);
                    self.inlines(s, end, children, &more);
                    let end_mark = buf.create_mark(None, end, false);
                    s.link_items.borrow_mut().push(LinkItem {
                        url: url.clone(),
                        start: start_mark,
                        end: end_mark,
                    });
                }
                Inline::Image { url, alt } => self.image(s, end, url, alt, tags),
                Inline::Math { tex, display } => {
                    let modo = if *display {
                        crate::formulas::Modo::Bloque
                    } else {
                        crate::formulas::Modo::EnLinea
                    };
                    self.formula(s, end, tex, modo, tags);
                }
                // Un salto simple dentro del párrafo es un salto de línea. Va con
                // las etiquetas del contexto, así la línea siguiente conserva la sangría del
                // callout, la cita o el ítem.
                Inline::SoftBreak | Inline::HardBreak => {
                    let refs: Vec<&gtk::TextTag> = tags.iter().collect();
                    buf.insert_with_tags(end, "\n", &refs);
                }
            }
        }
    }

    pub(super) fn image(
        &self,
        s: &RenderState,
        end: &mut gtk::TextIter,
        url: &str,
        alt: &str,
        tags: &[gtk::TextTag],
    ) {
        let buf = &s.buffer;
        // Audio y video con la sintaxis de imagen (`![](clip.mp4#t=30)`, 057).
        if let Some(tipo) = orbital_vault::medios::tipo_de(url)
            .filter(|t| *t != orbital_vault::medios::Tipo::Imagen)
            && !url.contains("://")
        {
            let (ruta, fragmento) = url
                .split_once('#')
                .map_or((url, None), |(r, f)| (r, Some(f)));
            let ruta = orbital_vault::links::percent_decode(ruta);
            match s
                .base_dir
                .as_ref()
                .map(|d| d.join(&*ruta))
                .map(|p| orbital_vault::ubicacion::redirigir(&p).unwrap_or(p))
                .filter(|p| p.is_file() || crate::drive_remoto::es_remoto(p))
            {
                Some(path) => {
                    s.medios_en_linea
                        .borrow_mut()
                        .push((path, tipo, fragmento.map(str::to_owned)))
                }
                None => self.archivo_inexistente(s, end, &ruta, tags),
            }
            return;
        }
        // Como un archivo en disco: `fotos%20ñ/foto-0.png` es «fotos ñ/foto-0.png», sin `#…`.
        let local = (!url.contains("://"))
            .then(|| {
                let ruta = url.split('#').next().unwrap_or(url);
                let ruta = orbital_vault::links::percent_decode(ruta);
                s.base_dir.as_ref().map(|d| d.join(&*ruta))
            })
            .flatten()
            .map(|p| orbital_vault::ubicacion::redirigir(&p).unwrap_or(p))
            .filter(|p| p.is_file() || crate::drive_remoto::es_remoto(p));
        match local {
            Some(path) if !path.is_file() => {
                // «Solo en Drive» sin copia todavía (061b): un marcador mientras baja.
                let w = crate::drive_remoto::marcador_de_lugar(&path);
                self.anchor(s, end, w);
            }
            Some(path) => {
                let pic = picture(&path, alt, self.0.width.get());
                self.anchor(s, end, pic);
            }
            // Las imágenes remotas no se cargan (principio V): se muestran como enlace.
            None => {
                let tag = self.link_tag(s, url);
                let mut more = tags.to_vec();
                more.extend(buf.tag_table().lookup("link"));
                more.push(tag);
                let refs: Vec<&gtk::TextTag> = more.iter().collect();
                let label = if alt.is_empty() { url } else { alt };
                buf.insert_with_tags(end, &format!("🖼 {label}"), &refs);
            }
        }
    }
}

impl Reader {
    /// Un archivo incrustado que existe (057): la imagen, o el reproductor de audio o video
    /// (que no abre nada hasta que se pide reproducir). `fragmento`: el `t=` de inicio.
    pub(super) fn medio(
        &self,
        s: &RenderState,
        end: &mut gtk::TextIter,
        path: &Path,
        tipo: orbital_vault::medios::Tipo,
        fragmento: Option<&str>,
    ) {
        if !path.is_file() && crate::drive_remoto::es_remoto(path) {
            // «Solo en Drive» sin copia todavía (061b): un marcador mientras baja, o que explica
            // por qué no se ve sin conexión.
            let w = crate::drive_remoto::marcador_de_lugar(path);
            self.anchor(s, end, w);
            return;
        }
        let ancho = self.0.width.get();
        let w = match tipo {
            orbital_vault::medios::Tipo::Imagen => {
                let alt = path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                picture(path, &alt, ancho)
            }
            _ => {
                let w = crate::medios::reproductor(
                    path,
                    tipo,
                    fragmento.and_then(orbital_vault::medios::inicio),
                );
                self.track_wide(&w);
                w
            }
        };
        self.anchor(s, end, w);
    }

    /// «Archivo inexistente: «nombre»», tenue (057).
    pub(super) fn archivo_inexistente(
        &self,
        s: &RenderState,
        end: &mut gtk::TextIter,
        nombre: &str,
        tags: &[gtk::TextTag],
    ) {
        let mut dim = tags.to_vec();
        dim.extend(s.buffer.tag_table().lookup("dim"));
        s.buffer.insert_with_tags(
            end,
            &format!("Archivo inexistente: «{nombre}»"),
            &dim.iter().collect::<Vec<_>>(),
        );
    }
}

/// Una celda de tabla en una sola línea: sin saltos ni tabulaciones (rompen las columnas) y con
/// las imágenes como su texto alternativo, igual que lo que mide el ancho.
fn celda(inlines: &[Inline]) -> Vec<Inline> {
    inlines
        .iter()
        .map(|i| match i {
            Inline::Text(t) => Inline::Text(t.replace(['\t', '\n'], " ")),
            Inline::SoftBreak | Inline::HardBreak => Inline::Text(" ".into()),
            Inline::Image { alt, .. } => Inline::Text(alt.clone()),
            Inline::Strong(c) => Inline::Strong(celda(c)),
            Inline::Emph(c) => Inline::Emph(celda(c)),
            Inline::Strike(c) => Inline::Strike(celda(c)),
            Inline::Link { url, children } => Inline::Link {
                url: url.clone(),
                children: celda(children),
            },
            otro => otro.clone(),
        })
        .collect()
}

/// Un grupo de relaciones con el mismo tipo (049): el nombre que se muestra y, por ítem, sus
/// enlaces y su motivo.
pub(super) struct GrupoTipado<'b> {
    pub(super) nombre: String,
    clave: String,
    pub(super) items: Vec<(Vec<&'b Inline>, Option<String>)>,
}

/// El tipo, los enlaces y el motivo de un párrafo `tipo:: [[A]] — motivo`, con el mismo
/// criterio que el índice (`orbital_vault::relaciones`).
pub(super) fn relacion_de_parrafo(p: &[Inline]) -> Option<(String, Vec<&Inline>, Option<String>)> {
    use orbital_vault::relaciones as rel;
    let es_enlace = |i: &Inline| matches!(i, Inline::Link { .. });
    let primero = p.iter().position(es_enlace)?;
    let ultimo = p.iter().rposition(es_enlace)?;
    let antes = plain_text(&p[..primero]);
    let tipo = rel::tipo_de_prefijo(&antes)?;
    let despues = plain_text(&p[ultimo + 1..]);
    let motivo = rel::motivo_de_sufijo(&despues).map(str::to_owned);
    let enlaces = p[primero..=ultimo]
        .iter()
        .filter(|i| es_enlace(i))
        .collect();
    Some((rel::nombre_canonico(tipo), enlaces, motivo))
}

/// Separa de las Conexiones los ítems de lista con tipo (de las listas de primer nivel, ítems de
/// un solo párrafo) y los junta por tipo; devuelve también lo demás, sin esos ítems.
pub(super) fn agrupar_tipadas(body: &[Block]) -> (Vec<GrupoTipado<'_>>, Vec<Block>) {
    let mut grupos: Vec<GrupoTipado> = Vec::new();
    let mut resto = Vec::new();
    for b in body {
        let BlockKind::List { start, items } = &b.kind else {
            resto.push(b.clone());
            continue;
        };
        let mut quedan = Vec::new();
        for it in items {
            let tipada = match it.blocks.as_slice() {
                [
                    Block {
                        kind: BlockKind::Paragraph(p),
                        ..
                    },
                ] if it.task.is_none() => relacion_de_parrafo(p),
                _ => None,
            };
            let Some((nombre, enlaces, motivo)) = tipada else {
                quedan.push(it.clone());
                continue;
            };
            let clave = orbital_vault::relaciones::clave(&nombre);
            match grupos.iter_mut().find(|g| g.clave == clave) {
                Some(g) => g.items.push((enlaces, motivo)),
                None => grupos.push(GrupoTipado {
                    nombre,
                    clave,
                    items: vec![(enlaces, motivo)],
                }),
            }
        }
        if !quedan.is_empty() {
            resto.push(Block {
                kind: BlockKind::List {
                    start: *start,
                    items: quedan,
                },
                source: b.source.clone(),
                line: b.line,
                id: b.id.clone(),
            });
        }
    }
    (grupos, resto)
}

/// Si el texto de un encabezado es el nombre de la parte (o un sinónimo): «Síntesis», «Resumen»,
/// «Fuentes», «Bibliografía», «Conexiones»…
fn repite_nombre(kind: SeccionKind, texto: &str) -> bool {
    let t = texto.trim().to_lowercase();
    let nombres: &[&str] = match kind {
        SeccionKind::Sintesis => &["síntesis", "sintesis", "resumen", "summary"],
        SeccionKind::Fuentes => &[
            "fuentes",
            "bibliografía",
            "bibliografia",
            "referencias",
            "sources",
        ],
        SeccionKind::Conexiones => &["conexiones", "connections", "enlaces"],
    };
    nombres.contains(&t.as_str())
}

/// Imagen local reducida al ancho de la columna. La vista de texto da a los widgets anclados su
/// tamaño mínimo, así que se pide el tamaño explícito (leído de la cabecera, sin decodificar).
pub(super) fn picture(path: &Path, alt: &str, max_width: i32) -> gtk::Widget {
    let pic = gtk::Picture::for_filename(path);
    pic.set_can_shrink(true);
    pic.set_content_fit(gtk::ContentFit::Contain);
    pic.set_alternative_text(Some(alt));
    if let Some((_, w, h)) = gtk::gdk_pixbuf::Pixbuf::file_info(path)
        && w > 0
        && h > 0
    {
        let width = w.min(max_width.max(120));
        pic.set_size_request(width, h * width / w);
    }
    pic.upcast()
}

/// Pone las etiquetas de resaltado (055) sobre el texto de un bloque de código que empieza en el
/// carácter `inicio` del buffer.
pub(super) fn pintar_codigo(
    buf: &gtk::TextBuffer,
    inicio: i32,
    tramos: &[crate::resaltado::Tramo],
) {
    let table = buf.tag_table();
    // Cada etiqueta se busca por nombre una vez por bloque.
    let mut etiquetas: [Option<Option<gtk::TextTag>>; 20] = Default::default();
    let mut a = buf.iter_at_offset(inicio);
    let mut b = a;
    for t in tramos {
        let n = t.rol as usize * 4 + usize::from(t.negrita) + 2 * usize::from(t.cursiva);
        let Some(tag) = etiquetas[n]
            .get_or_insert_with(|| {
                table.lookup(&super::etiquetas::nombre_sintaxis(
                    t.rol, t.negrita, t.cursiva,
                ))
            })
            .as_ref()
        else {
            continue;
        };
        a.set_offset(inicio + t.inicio as i32);
        b.set_offset(inicio + t.fin as i32);
        buf.apply_tag(tag, &a, &b);
    }
}

pub(super) struct RenderState {
    pub(super) buffer: gtk::TextBuffer,
    /// En el sitio (el buffer ya está en la vista) los widgets se anclan enseguida.
    pub(super) attach_now: bool,
    pub(super) children: RefCell<Vec<(gtk::Widget, gtk::TextChildAnchor)>>,
    pub(super) base_dir: Option<PathBuf>,
    /// Etiquetas propias del bloque de primer nivel en curso.
    pub(super) tags: RefCell<Vec<gtk::TextTag>>,
    pub(super) links: RefCell<Vec<(gtk::TextTag, String)>>,
    pub(super) link_items: RefCell<Vec<LinkItem>>,
    pub(super) block_base: RefCell<Rc<Cell<usize>>>,
    pub(super) block_start: Cell<usize>,
    /// Profundidad dentro de transclusiones (lo incrustado no se edita desde acá).
    pub(super) embedded: Cell<u32>,
    /// Líneas fuente de los elementos plegables (028).
    pub(super) plegables: Rc<Vec<u32>>,
    /// Elementos plegables del bloque de primer nivel en curso.
    pub(super) plegables_lector: RefCell<Vec<super::plegado::PlegableLector>>,
    /// Lo que se dibuja a mano sobre el bloque de primer nivel en curso.
    pub(super) capas: RefCell<Vec<super::capas::Capa>>,
    /// Línea del ítem enfocado si el bloque en curso lo contiene (028): de su lista se vuelca
    /// solo ese ítem.
    pub(super) foco: Cell<Option<u32>>,
    /// Líneas plegadas del bloque de primer nivel en curso (de su firma).
    pub(super) plegadas: RefCell<Vec<u32>>,
    /// Se vuelca en un buffer suelto para trasplantar (`Reader::reemplazar`): los widgets van como
    /// `U+FFFC` y se anotan en `lugares` con su posición.
    pub(super) provisorio: Cell<bool>,
    pub(super) lugares: RefCell<Vec<(gtk::Widget, i32)>>,
    /// Lo mismo para los pintables de las fórmulas (050): un `U+FFFC` copiado como texto pierde
    /// el pintable.
    pub(super) pintables: RefCell<Vec<(gtk::gdk::Paintable, i32)>>,
    /// Letra, color y escala de las fórmulas de la vista (050).
    pub(super) estilo_formulas: Cell<crate::formulas::Estilo>,
    /// Bloques de código del bloque de primer nivel en curso que faltan resaltar (055).
    pub(super) codigo: RefCell<Vec<(String, String)>>,
    /// Bloques de primer nivel con código por resaltar (055).
    pub(super) resaltar: RefCell<Vec<super::resaltar::Pendiente>>,
    /// Medios escritos en medio de un párrafo (`![](clip.mp4)`, 057): van en su propio renglón
    /// al terminar el bloque, como los `![[…]]`.
    pub(super) medios_en_linea:
        RefCell<Vec<(PathBuf, orbital_vault::medios::Tipo, Option<String>)>>,
}
