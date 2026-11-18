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
        // Por renglón (`line_at_y`): `iter_at_location` aborta debajo de un renglón con algo
        // invisible (`editor::posicion`).
        let (top, _) = i.view.line_at_y((rect.y() - h).max(0));
        let (mut bottom, _) = i.view.line_at_y(rect.y() + 2 * h);
        // Antes de calcular la disposición, GTK estima alturas y todo parece cerca: la ventana
        // también se acota en caracteres.
        let from = top.offset();
        bottom.forward_line();
        let to = bottom.offset().min(from + WINDOW_CHARS);
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
        // Cada bloque se guarda: lo que se pliega sale del buffer y se vuelve a volcar al
        // desplegar (046).
        let fuentes: Vec<Rc<Block>> = doc.blocks.into_iter().map(Rc::new).collect();
        match in_place {
            Some((p, s)) => self.update(
                fuentes,
                hashes,
                header_hash,
                header,
                base_dir,
                plegables,
                p,
                s,
            ),
            None => self.rebuild(
                fuentes,
                hashes,
                header_hash,
                header,
                base_dir,
                plegables,
                generation,
            ),
        }
    }

    /// Un estado de volcado sobre `buffer`.
    pub(super) fn estado(
        &self,
        buffer: gtk::TextBuffer,
        attach_now: bool,
        base_dir: Option<PathBuf>,
        plegables: Rc<Vec<u32>>,
    ) -> RenderState {
        RenderState {
            buffer,
            attach_now,
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
            capas: RefCell::new(Vec::new()),
            foco: Cell::new(None),
            plegadas: RefCell::new(Vec::new()),
            provisorio: Cell::new(false),
            lugares: RefCell::new(Vec::new()),
        }
    }

    /// Arma todo en un buffer nuevo por tramos y lo pone al final: la vista no parpadea.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn rebuild(
        &self,
        fuentes: Vec<Rc<Block>>,
        hashes: Vec<u64>,
        header_hash: u64,
        header: impl FnOnce() -> Option<gtk::Widget>,
        base_dir: Option<PathBuf>,
        plegables: Rc<Vec<u32>>,
        generation: u64,
    ) {
        let table = self.buffer().tag_table();
        let fresh = gtk::TextBuffer::new(Some(&table));
        let firmas = self.firmas_de(&fuentes, &plegables);
        let state = Rc::new(self.estado(fresh, false, base_dir, plegables));
        if let Some(h) = header() {
            let mut end = state.buffer.end_iter();
            self.anchor(&state, &mut end, h.clone());
            self.track_wide(&h);
            state.buffer.insert(&mut end, "\n");
        }
        let built: Rc<RefCell<Vec<BlockInfo>>> = Rc::default();
        let blocks = RefCell::new(fuentes.into_iter().zip(hashes).zip(firmas));
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
                let Some(((fuente, hash), firma)) = next else {
                    this.swap_in(
                        &state,
                        std::mem::take(&mut *built.borrow_mut()),
                        header_hash,
                    );
                    return glib::ControlFlow::Break;
                };
                let mut end = state.buffer.end_iter();
                let info = this.top_block(&state, &mut end, fuente, hash, firma);
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
        *i.plegables.borrow_mut() = s.plegables.clone();
        *i.base_dir.borrow_mut() = s.base_dir.clone();
        i.header.set(Some(header_hash));
        // Si el plegado cambió mientras se armaba, se vuelca lo que haga falta (028/046).
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
        fuentes: Vec<Rc<Block>>,
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
        *i.plegables.borrow_mut() = plegables.clone();
        *i.base_dir.borrow_mut() = base_dir.clone();
        let firmas = self.firmas_de(&fuentes, &plegables);
        let state = self.estado(buffer.clone(), true, base_dir, plegables);
        i.focused_link.set(None);
        let new_len = fuentes.len();
        let nuevos: Vec<(Rc<Block>, u64, plegado::Firma)> = (p..new_len - s)
            .map(|n| (fuentes[n].clone(), hashes[n], firmas[n].clone()))
            .collect();
        let mut blocks = std::mem::take(&mut *i.blocks.borrow_mut());
        let quedan = blocks.len() - s;
        self.reemplazar(
            &mut blocks,
            p..quedan,
            nuevos,
            i.base_dir.borrow().clone(),
            i.plegables.borrow().clone(),
        );
        for (info, fuente) in blocks.iter_mut().zip(&fuentes) {
            info.line = fuente.line;
            info.base.set(fuente.source.start);
            info.fuente = fuente.clone();
        }

        if i.header.get() != Some(header_hash) {
            let start_at = |b: Option<&BlockInfo>| match b {
                Some(b) => buffer.iter_at_mark(&b.mark),
                None => buffer.end_iter(),
            };
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
        // Lo de antes y lo de después pueden haber cambiado de plegado (un encabezado nuevo, por
        // ejemplo): se vuelven a volcar los que hagan falta (028/046).
        self.aplicar_plegado();
        if let Some(f) = i.on_rendered.borrow().as_ref() {
            f();
        }
    }

    /// Reemplaza en el buffer de la vista los bloques `rango` de `blocks` por `nuevos`, en el
    /// mismo lugar. Lo de antes y lo de después no se toca.
    ///
    /// Los bloques se vuelcan en un buffer suelto (misma tabla de etiquetas) y se trasplantan de
    /// una vez: volcar directo en el buffer de la vista costaba 3–4 veces más (cada inserción
    /// invalida la disposición y avisa a la accesibilidad), unos 10 ms por transclusión al
    /// desplegar una sección (046). Los widgets se vuelcan como `U+FFFC` y se anclan al
    /// trasplantar; las marcas se vuelven a crear en el buffer de la vista.
    pub(super) fn reemplazar(
        &self,
        blocks: &mut Vec<BlockInfo>,
        rango: std::ops::Range<usize>,
        nuevos: Vec<(Rc<Block>, u64, plegado::Firma)>,
        base_dir: Option<PathBuf>,
        plegables: Rc<Vec<u32>>,
    ) {
        let i = &self.0;
        let buffer = self.buffer();
        let table = buffer.tag_table();
        // 1. Lo nuevo, en un buffer suelto.
        let suelto = gtk::TextBuffer::new(Some(&table));
        let state = self.estado(suelto.clone(), false, base_dir, plegables);
        state.provisorio.set(true);
        let mut fin_suelto = suelto.end_iter();
        let mut middle: Vec<BlockInfo> = nuevos
            .into_iter()
            .map(|(fuente, hash, firma)| {
                self.top_block(&state, &mut fin_suelto, fuente, hash, firma)
            })
            .collect();

        // 2. Lo viejo, fuera.
        let marca_en = |n: usize, blocks: &[BlockInfo]| match blocks.get(n) {
            Some(b) => buffer.iter_at_mark(&b.mark),
            None => buffer.end_iter(),
        };
        let mut from = marca_en(rango.start, blocks);
        let mut to = marca_en(rango.end, blocks);
        let removed: Vec<BlockInfo> = blocks.drain(rango.clone()).collect();
        {
            // Una misma marca puede estar en un enlace y en una capa: se borra una vez.
            let borrar = |m: &gtk::TextMark| {
                if !m.is_deleted() {
                    buffer.delete_mark(m);
                }
            };
            let mut links = i.links.borrow_mut();
            for b in &removed {
                for t in &b.tags {
                    links.remove(t);
                    if let Some(n) = t.name() {
                        i.clave_de.borrow_mut().remove(n.as_str());
                    }
                    table.remove(t);
                }
                for item in &b.links {
                    borrar(&item.start);
                    borrar(&item.end);
                }
                for p in &b.plegables {
                    borrar(&p.glifo);
                    if let Some((d, h)) = &p.contenido {
                        borrar(d);
                        borrar(h);
                    }
                }
                b.capas.iter().flat_map(|c| c.marcas()).for_each(borrar);
                borrar(&b.mark);
            }
        }
        buffer.delete(&mut from, &mut to);
        let desde = from.offset();

        // 3. El texto de una vez y sus etiquetas tramo por tramo. Una etiqueta que cubría lo de
        // antes y lo de después (se unieron al borrar) no tiene que cubrir lo nuevo.
        let cruzan: Vec<gtk::TextTag> = from
            .tags()
            .into_iter()
            .filter(|t| !from.starts_tag(Some(t)))
            .collect();
        let texto = suelto.slice(&suelto.start_iter(), &suelto.end_iter(), true);
        let largo = suelto.char_count();
        buffer.insert(&mut from, &texto);
        let en = |off: i32| buffer.iter_at_offset(desde + off);
        for t in &cruzan {
            buffer.remove_tag(t, &en(0), &en(largo));
        }
        let mut it = suelto.start_iter();
        loop {
            for t in it.toggled_tags(true) {
                let mut hasta = it;
                if !hasta.forward_to_tag_toggle(Some(&t)) {
                    hasta = suelto.end_iter();
                }
                buffer.apply_tag(&t, &en(it.offset()), &en(hasta.offset()));
            }
            if !it.forward_to_tag_toggle(None::<&gtk::TextTag>) || it.is_end() {
                break;
            }
        }

        // 4. Los widgets, en el lugar de su `U+FFFC` y con sus etiquetas.
        for (w, off) in state.lugares.borrow_mut().drain(..) {
            let mut a = en(off);
            let mut z = a;
            z.forward_char();
            let etiquetas = a.tags();
            buffer.delete(&mut a, &mut z);
            let anchor = buffer.create_child_anchor(&mut a);
            let mut b = a;
            b.backward_char();
            for t in &etiquetas {
                buffer.apply_tag(t, &b, &a);
            }
            i.pending.borrow_mut().push((w, anchor));
        }

        // 5. Las marcas, en el buffer de la vista (una marca compartida sigue compartida).
        let mut nuevas: HashMap<gtk::TextMark, gtk::TextMark> = HashMap::new();
        for b in &mut middle {
            for m in b.marcas_mut() {
                let nueva = nuevas.entry(m.clone()).or_insert_with(|| {
                    let off = suelto.iter_at_mark(m).offset();
                    buffer.create_mark(None, &en(off), m.is_left_gravity())
                });
                *m = nueva.clone();
            }
        }

        // 6. Lo que sigue empezaba justo donde se insertó: sus marcas de gravedad izquierda
        // quedaron antes de lo insertado.
        if largo > 0 {
            let end = en(largo);
            for b in &blocks[rango.start..] {
                if buffer.iter_at_mark(&b.mark).offset() != desde {
                    break;
                }
                let correr = |m: &gtk::TextMark| {
                    if buffer.iter_at_mark(m).offset() == desde {
                        buffer.move_mark(m, &end);
                    }
                };
                correr(&b.mark);
                b.plegables.iter().for_each(|p| correr(&p.glifo));
                b.links.iter().for_each(|l| correr(&l.start));
                b.capas.iter().flat_map(|c| c.marcas()).for_each(correr);
            }
        }
        i.links
            .borrow_mut()
            .extend(state.links.borrow_mut().drain(..));
        blocks.splice(rango.start..rango.start, middle);
        self.schedule_attach();
    }
}
