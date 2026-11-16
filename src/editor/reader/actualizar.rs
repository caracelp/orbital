//! Render de la vista formateada y actualización en el sitio de los bloques que cambiaron; widgets anclados por tandas cerca de lo visible.

use super::*;

impl Reader {
    /// Anclar durante la disposición rompe una aserción de GTK: se hace en el próximo reposo.
    pub(super) fn schedule_attach(&self) {
        let i = &self.0;
        if i.attach_scheduled.get() || i.pending.borrow().is_empty() {
            return;
        }
        i.attach_scheduled.set(true);
        let this = self.clone();
        glib::idle_add_local_once(move || {
            this.0.attach_scheduled.set(false);
            this.attach_visible();
        });
    }

    /// Ancla los widgets pendientes que están a menos de una pantalla de lo visible.
    pub(super) fn attach_visible(&self) {
        let i = &self.0;
        if i.pending.borrow().is_empty() {
            return;
        }
        let rect = i.view.visible_rect();
        let h = rect.height().max(400);
        let top = i.view.iter_at_location(0, (rect.y() - h).max(0));
        let bottom = i.view.iter_at_location(0, rect.y() + 2 * h);
        // Antes de calcular la disposición, GTK estima alturas y todo parece cerca: la ventana
        // también se acota en caracteres.
        let from = top.map_or(0, |t| t.offset());
        let to = bottom
            .map_or(i32::MAX, |b| {
                let mut b = b;
                b.forward_line();
                b.offset()
            })
            .min(from + WINDOW_CHARS);
        let buffer = self.buffer();
        let mut ready = Vec::new();
        i.pending.borrow_mut().retain(|(w, a)| {
            if a.is_deleted() {
                return false;
            }
            if ready.len() >= ATTACH_BATCH {
                return true;
            }
            let at = buffer.iter_at_child_anchor(a).offset();
            if (from..=to).contains(&at) {
                ready.push((w.clone(), a.clone()));
                false
            } else {
                true
            }
        });
        let more = ready.len() >= ATTACH_BATCH;
        for (w, a) in ready {
            i.view.add_child_at_anchor(&w, &a);
        }
        // Por tandas: cada widget nuevo se mide y se dibuja en el cuadro siguiente.
        if more {
            self.schedule_attach();
        }
    }

    /// Vuelca un documento. `header` arma las propiedades (solo se llama si cambiaron);
    /// `base_dir` resuelve las imágenes.
    pub fn render(
        &self,
        prepared: Prepared,
        header: impl FnOnce() -> Option<gtk::Widget>,
        base_dir: Option<PathBuf>,
    ) {
        let i = &self.0;
        let generation = i.generation.get() + 1;
        i.generation.set(generation);
        let Prepared {
            doc,
            hashes,
            header: header_hash,
            plegables,
            ..
        } = prepared;
        let plegables = Rc::new(plegables);
        let in_place = {
            let old = i.blocks.borrow();
            let p = old
                .iter()
                .zip(&hashes)
                .take_while(|(b, h)| b.hash == **h)
                .count();
            let max_s = old.len().min(hashes.len()) - p;
            let s = old
                .iter()
                .rev()
                .zip(hashes.iter().rev())
                .take(max_s)
                .take_while(|(b, h)| b.hash == **h)
                .count();
            let changed = hashes.len() - p - s;
            (!old.is_empty() && changed <= MAX_IN_PLACE).then_some((p, s))
        };
        match in_place {
            Some((p, s)) => {
                self.update(doc, hashes, header_hash, header, base_dir, plegables, p, s)
            }
            None => self.rebuild(
                doc,
                hashes,
                header_hash,
                header,
                base_dir,
                plegables,
                generation,
            ),
        }
    }

    /// Arma todo en un buffer nuevo por tramos y lo pone al final: la vista no parpadea.
    pub(super) fn rebuild(
        &self,
        doc: RenderDoc,
        hashes: Vec<u64>,
        header_hash: u64,
        header: impl FnOnce() -> Option<gtk::Widget>,
        base_dir: Option<PathBuf>,
        plegables: Rc<Vec<u32>>,
        generation: u64,
    ) {
        let table = self.buffer().tag_table();
        let fresh = gtk::TextBuffer::new(Some(&table));
        let state = Rc::new(RenderState {
            buffer: fresh,
            attach_now: false,
            children: RefCell::new(Vec::new()),
            base_dir,
            tags: RefCell::new(Vec::new()),
            links: RefCell::new(Vec::new()),
            link_items: RefCell::new(Vec::new()),
            block_base: RefCell::new(Rc::new(Cell::new(0))),
            block_start: Cell::new(0),
            embedded: Cell::new(0),
            plegables,
            plegables_lector: RefCell::new(Vec::new()),
        });
        if let Some(h) = header() {
            let mut end = state.buffer.end_iter();
            self.anchor(&state, &mut end, h.clone());
            self.track_wide(&h);
            state.buffer.insert(&mut end, "\n");
        }
        let built: Rc<RefCell<Vec<BlockInfo>>> = Rc::default();
        let blocks = RefCell::new(doc.blocks.into_iter().zip(hashes));
        let this = self.clone();
        glib::idle_add_local(move || {
            if this.0.generation.get() != generation {
                // Cancelado: las etiquetas creadas no llegaron a la vista.
                let table = state.buffer.tag_table();
                for b in built.borrow_mut().drain(..) {
                    b.tags.iter().for_each(|t| table.remove(t));
                }
                state.tags.borrow().iter().for_each(|t| table.remove(t));
                return glib::ControlFlow::Break;
            }
            let start = Instant::now();
            loop {
                let next = blocks.borrow_mut().next();
                let Some((block, hash)) = next else {
                    this.swap_in(
                        &state,
                        std::mem::take(&mut *built.borrow_mut()),
                        header_hash,
                    );
                    return glib::ControlFlow::Break;
                };
                let mut end = state.buffer.end_iter();
                let info = this.top_block(&state, &mut end, &block, hash);
                built.borrow_mut().push(info);
                if start.elapsed() >= SLICE {
                    return glib::ControlFlow::Continue;
                }
            }
        });
    }

    /// Pone en la vista el buffer armado por `rebuild` y suelta lo anterior.
    pub(super) fn swap_in(&self, s: &RenderState, blocks: Vec<BlockInfo>, header_hash: u64) {
        let i = &self.0;
        i.focused_link.set(None);
        let table = s.buffer.tag_table();
        let mut links = i.links.borrow_mut();
        for b in i.blocks.borrow_mut().drain(..) {
            for t in &b.tags {
                links.remove(t);
                table.remove(t);
            }
        }
        links.extend(s.links.borrow_mut().drain(..));
        drop(links);
        let top = s.buffer.start_iter();
        i.view.set_buffer(Some(&s.buffer));
        s.buffer.place_cursor(&top);
        // Los widgets se anclan después de poner el buffer en la vista (si se anclan antes, el
        // cambio de buffer los descarta), y solo los cercanos a lo visible.
        *i.pending.borrow_mut() = std::mem::take(&mut *s.children.borrow_mut());
        *i.blocks.borrow_mut() = blocks;
        i.header.set(Some(header_hash));
        // Lo plegado (028) antes del primer cuadro del buffer nuevo.
        self.aplicar_plegado();
        // La disposición todavía no está calculada: se ancla lo visible en el próximo reposo.
        self.schedule_attach();
        if let Some(line) = i.pending_line.take() {
            self.scroll_to_line(line);
        }
        if let Some(line) = i.pending_resaltar.take() {
            self.resaltar_linea(line);
        }
        if let Some(f) = i.on_rendered.borrow().as_ref() {
            f();
        }
    }

    /// Reemplaza en el sitio los bloques que cambiaron; el prefijo `p` y el sufijo `s` quedan.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn update(
        &self,
        doc: RenderDoc,
        hashes: Vec<u64>,
        header_hash: u64,
        header: impl FnOnce() -> Option<gtk::Widget>,
        base_dir: Option<PathBuf>,
        plegables: Rc<Vec<u32>>,
        p: usize,
        s: usize,
    ) {
        let i = &self.0;
        let buffer = self.buffer();
        let table = buffer.tag_table();
        let state = RenderState {
            buffer: buffer.clone(),
            attach_now: true,
            children: RefCell::new(Vec::new()),
            base_dir,
            tags: RefCell::new(Vec::new()),
            links: RefCell::new(Vec::new()),
            link_items: RefCell::new(Vec::new()),
            block_base: RefCell::new(Rc::new(Cell::new(0))),
            block_start: Cell::new(0),
            embedded: Cell::new(0),
            plegables,
            plegables_lector: RefCell::new(Vec::new()),
        };
        i.focused_link.set(None);
        let mut old = std::mem::take(&mut *i.blocks.borrow_mut());
        let suffix = old.split_off(old.len() - s);
        let removed = old.split_off(p);
        let prefix = old;

        let start_at = |b: Option<&BlockInfo>| match b {
            Some(b) => buffer.iter_at_mark(&b.mark),
            None => buffer.end_iter(),
        };
        let mut from = start_at(removed.first().or(suffix.first()));
        let mut to = start_at(suffix.first());
        {
            let mut links = i.links.borrow_mut();
            for b in &removed {
                for t in &b.tags {
                    links.remove(t);
                    table.remove(t);
                }
                for item in &b.links {
                    buffer.delete_mark(&item.start);
                    buffer.delete_mark(&item.end);
                }
                for p in &b.plegables {
                    if let Some((d, h)) = &p.contenido {
                        buffer.delete_mark(d);
                        buffer.delete_mark(h);
                    }
                }
                buffer.delete_mark(&b.mark);
            }
        }
        buffer.delete(&mut from, &mut to);

        let new_len = doc.blocks.len();
        let mut end = from;
        let mut middle = Vec::with_capacity(new_len - p - s);
        for (block, hash) in doc.blocks[p..new_len - s]
            .iter()
            .zip(&hashes[p..new_len - s])
        {
            middle.push(self.top_block(&state, &mut end, block, *hash));
        }
        // La marca del primer bloque que queda tiene gravedad izquierda: quedó antes de lo insertado.
        if let Some(b) = suffix.first() {
            buffer.move_mark(&b.mark, &end);
        }
        i.links
            .borrow_mut()
            .extend(state.links.borrow_mut().drain(..));

        let mut blocks = prefix;
        blocks.extend(middle);
        blocks.extend(suffix);
        for (info, block) in blocks.iter_mut().zip(&doc.blocks) {
            info.line = block.line;
            info.base.set(block.source.start);
        }

        if i.header.get() != Some(header_hash) {
            let mut start = buffer.start_iter();
            let mut first = start_at(blocks.first());
            buffer.delete(&mut start, &mut first);
            let mut at = buffer.start_iter();
            if let Some(h) = header() {
                self.anchor(&state, &mut at, h.clone());
                self.track_wide(&h);
                buffer.insert(&mut at, "\n");
            }
            if let Some(b) = blocks.first() {
                buffer.move_mark(&b.mark, &at);
            }
            i.header.set(Some(header_hash));
        }
        *i.blocks.borrow_mut() = blocks;
        // Lo que se insertó en el sitio no tiene la etiqueta de lo plegado (028).
        self.aplicar_plegado();
        if let Some(f) = i.on_rendered.borrow().as_ref() {
            f();
        }
    }
}
