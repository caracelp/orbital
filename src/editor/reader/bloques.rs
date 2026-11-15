//! Cómo se dibuja cada bloque: párrafos, transclusiones, tareas, callouts, tablas, texto en línea e imágenes.

use super::*;

impl Reader {
    pub(super) fn top_block(
        &self,
        s: &RenderState,
        end: &mut gtk::TextIter,
        block: &Block,
        hash: u64,
    ) -> BlockInfo {
        let mark = s.buffer.create_mark(None, end, true);
        let base = Rc::new(Cell::new(block.source.start));
        *s.block_base.borrow_mut() = base.clone();
        s.block_start.set(block.source.start);
        self.block(s, end, block, 0);
        let heading = match &block.kind {
            BlockKind::Heading { inlines, .. } => Some(orbital_vault::render::plain_text(inlines)),
            _ => None,
        };
        BlockInfo {
            hash,
            line: block.line,
            mark,
            base,
            tags: std::mem::take(&mut *s.tags.borrow_mut()),
            heading,
            links: std::mem::take(&mut *s.link_items.borrow_mut()),
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
                self.inlines(s, end, inlines, &tags);
                buf.insert(end, "\n");
            }
            BlockKind::Paragraph(inlines) => {
                self.inlines(s, end, inlines, &base);
                buf.insert(end, "\n");
            }
            BlockKind::List { start, items } => {
                for (n, item) in items.iter().enumerate() {
                    let tag = self.indent_tag((depth + 1) * 24);
                    let mut tags = base.clone();
                    tags.push(tag.clone());
                    let item_start = end.offset();
                    if let Some(task) = item.task {
                        self.task(s, end, item, task);
                    } else {
                        let bullet = match start {
                            Some(s) => format!("{}. ", s + n as u64),
                            None => "•  ".to_owned(),
                        };
                        let refs: Vec<&gtk::TextTag> = tags.iter().collect();
                        buf.insert_with_tags(end, &bullet, &refs);
                    }
                    for (k, sub) in item.blocks.iter().enumerate() {
                        match &sub.kind {
                            // El primer párrafo va en la misma línea que la viñeta.
                            BlockKind::Paragraph(p) if k == 0 => {
                                self.inlines(s, end, p, &tags);
                                buf.insert(end, "\n");
                            }
                            _ => self.block(s, end, sub, depth + 1),
                        }
                    }
                    if item.blocks.is_empty() {
                        buf.insert(end, "\n");
                    }
                    // La sangría cubre también la casilla anclada al principio de la línea.
                    let first = buf.iter_at_offset(item_start);
                    let mut line_end = first;
                    if !line_end.ends_line() {
                        line_end.forward_to_line_end();
                    }
                    buf.apply_tag(&tag, &first, &line_end);
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
            } => self.callout(s, end, kind, title, *folded, body, depth, &base),
            BlockKind::Code { text, .. } => {
                let mut tags = base.clone();
                tags.extend(table.lookup("codeblock"));
                let refs: Vec<&gtk::TextTag> = tags.iter().collect();
                let body = text.strip_suffix('\n').unwrap_or(text);
                buf.insert_with_tags(end, body, &refs);
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
                let mut tags = base.clone();
                tags.extend(table.lookup("code"));
                let refs: Vec<&gtk::TextTag> = tags.iter().collect();
                buf.insert_with_tags(end, text.trim_end(), &refs);
                buf.insert(end, "\n");
            }
            BlockKind::Transclusion(t) => self.transclusion(s, end, t, depth, &base),
        }
    }

    /// Tarjeta de una transclusión (US5 de la 003): un encabezado con la nota de origen (un enlace
    /// que la abre) y su contenido, con fondo y sangría. Lo incrustado no se edita desde acá: sus
    /// casillas quedan deshabilitadas.
    pub(super) fn transclusion(
        &self,
        s: &RenderState,
        end: &mut gtk::TextIter,
        t: &orbital_vault::render::Transclusion,
        depth: i32,
        base: &[gtk::TextTag],
    ) {
        use orbital_vault::render::Embed;
        let buf = &s.buffer;
        let table = buf.tag_table();
        let start = end.offset();
        let url = match &t.anchor {
            Some(a) => format!("[[{}#{a}]]", t.target),
            None => format!("[[{}]]", t.target),
        };
        let shown = match &t.anchor {
            Some(a) => format!("{} › {a}", t.target),
            None => t.target.clone(),
        };
        let link = |text: String| orbital_vault::render::Inline::Link {
            url: url.clone(),
            children: vec![orbital_vault::render::Inline::Text(text)],
        };
        let mut title: Vec<gtk::TextTag> = base.to_vec();
        title.extend(table.lookup("callout-title"));
        let mut dim: Vec<gtk::TextTag> = base.to_vec();
        dim.extend(table.lookup("dim"));
        let text = |end: &mut gtk::TextIter, tags: &[gtk::TextTag], t: &str| {
            buf.insert_with_tags(end, t, &tags.iter().collect::<Vec<_>>());
        };
        text(end, &title, "↪  ");
        match &t.content {
            Embed::Note { blocks, .. } => {
                self.inlines(s, end, &[link(shown)], &title);
                buf.insert(end, "\n");
                s.embedded.set(s.embedded.get() + 1);
                for b in blocks {
                    self.block(s, end, b, depth + 1);
                }
                s.embedded.set(s.embedded.get() - 1);
            }
            Embed::Missing => {
                text(end, &dim, "Nota inexistente: ");
                self.inlines(s, end, &[link(shown)], &title);
                text(end, &dim, " (creala desde el enlace)");
                buf.insert(end, "\n");
            }
            Embed::SectionMissing { .. } => {
                self.inlines(s, end, &[link(t.target.clone())], &title);
                let a = t.anchor.as_deref().unwrap_or("");
                text(end, &dim, &format!(" no tiene el encabezado «{a}»"));
                buf.insert(end, "\n");
            }
            Embed::Html { .. } => {
                self.inlines(s, end, &[link(shown)], &title);
                text(end, &dim, " es una nota HTML: abrila para verla");
                buf.insert(end, "\n");
            }
            Embed::Ambiguous(all) => {
                text(
                    end,
                    &dim,
                    &format!(
                        "Hay {} notas «{}»: escribí la carpeta, por ejemplo ![[{}]]",
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
                buf.insert(end, "\n");
            }
            Embed::Cycle { .. } => {
                text(end, &dim, "↻ Referencia circular a ");
                self.inlines(s, end, &[link(shown)], &title);
                text(end, &dim, ": ya se está mostrando más arriba");
                buf.insert(end, "\n");
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
                self.inlines(s, end, &[link(shown)], &title);
                buf.insert(end, "\n");
            }
            Embed::Pending => {
                text(end, &dim, &format!("![[{}]]", t.target));
                buf.insert(end, "\n");
            }
        }
        if let Some(bg) = table.lookup("embed") {
            buf.apply_tag(&bg, &buf.iter_at_offset(start), end);
        }
        // Un espacio corto sin fondo: dos tarjetas seguidas no se ven como una sola.
        let gap: Vec<gtk::TextTag> = table.lookup("embed-gap").into_iter().collect();
        buf.insert_with_tags(end, "\n", &gap.iter().collect::<Vec<_>>());
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
        base: &[gtk::TextTag],
    ) {
        let buf = &s.buffer;
        let table = buf.tag_table();
        let (name, glyph) = match kind {
            CalloutKind::Nota => ("callout-nota", "ⓘ"),
            CalloutKind::Consejo => ("callout-consejo", "✦"),
            CalloutKind::Importante => ("callout-importante", "✱"),
            CalloutKind::Advertencia => ("callout-advertencia", "⚠"),
            CalloutKind::Peligro => ("callout-peligro", "✖"),
            CalloutKind::Otro(_) => ("callout-otro", "ⓘ"),
        };
        let start = end.offset();
        let mut ttags: Vec<gtk::TextTag> = base.to_vec();
        ttags.extend(table.lookup("callout-title"));
        let trefs: Vec<&gtk::TextTag> = ttags.iter().collect();
        buf.insert_with_tags(end, &format!("{glyph}  {title}"), &trefs);
        // Plegable: un botón que oculta el cuerpo (el único widget del callout).
        let fold = folded.map(|f| {
            let tag = self.new_tag(s, gtk::TextTag::builder().invisible(f).build());
            let icon = |open: bool| {
                if open {
                    "pan-down-symbolic"
                } else {
                    "pan-end-symbolic"
                }
            };
            let button = gtk::ToggleButton::builder()
                .icon_name(icon(!f))
                .active(!f)
                .valign(gtk::Align::Center)
                .css_classes(["flat", "circular"])
                .build();
            button.update_property(&[gtk::accessible::Property::Label(&format!(
                "Mostrar u ocultar «{title}»"
            ))]);
            let t = tag.clone();
            button.connect_toggled(move |b| {
                t.set_invisible(!b.is_active());
                b.set_icon_name(icon(b.is_active()));
            });
            buf.insert(end, " ");
            self.anchor(s, end, button.upcast());
            tag
        });
        buf.insert(end, "\n");
        let body_start = end.offset();
        for sub in body {
            self.block(s, end, sub, depth + 1);
        }
        if let Some(bg) = table.lookup(name) {
            buf.apply_tag(&bg, &buf.iter_at_offset(start), end);
        }
        if let Some(tag) = &fold {
            buf.apply_tag(tag, &buf.iter_at_offset(body_start), end);
        }
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
        let line = |end: &mut gtk::TextIter, cells: &[String], bold: bool| {
            let mut l = String::new();
            for c in 0..cols {
                l.push('\t');
                l.push_str(cells.get(c).map(String::as_str).unwrap_or(""));
            }
            l.push('\n');
            let mut t = tags.clone();
            if bold {
                t.extend(table.lookup("table-head"));
            }
            buf.insert_with_tags(end, &l, &t.iter().collect::<Vec<_>>());
        };
        line(end, &head, true);
        for row in &body {
            line(end, row, false);
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
                    let refs: Vec<&gtk::TextTag> = tags.iter().collect();
                    buf.insert_with_tags(end, t, &refs);
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
                Inline::SoftBreak => buf.insert(end, " "),
                Inline::HardBreak => buf.insert(end, "\n"),
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
        let local = (!url.contains("://"))
            .then(|| s.base_dir.as_ref().map(|d| d.join(url)))
            .flatten()
            .filter(|p| p.is_file());
        match local {
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
}
