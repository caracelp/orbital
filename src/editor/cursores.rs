//! Multi-cursor (006): gestos, teclas y la capa que dibuja los cursores extra.

use super::*;

impl Editor {
    /// Multi-cursor (006): gestos con Alt, teclas en fase captura y el redibujado del overlay.
    /// Sin cursores extra, nada de esto cambia el comportamiento (FR-7).
    pub(super) fn setup_multi_cursor(&self) {
        let i = &self.0;

        // Redibujado: scroll del lienzo y cambios del texto.
        let this = self.clone();
        i.code_scroller
            .vadjustment()
            .connect_value_changed(move |_| {
                this.actualizar_carenes();
            });
        let this = self.clone();
        i.code_scroller
            .hadjustment()
            .connect_value_changed(move |_| {
                this.actualizar_carenes();
            });
        let this = self.clone();
        i.buffer.connect_changed(move |_| {
            this.actualizar_carenes();
        });

        // Alt+clic / Alt+arrastre con un GestureDrag (la distancia decide clic vs selección).
        let drag = gtk::GestureDrag::new();
        drag.set_button(1);
        drag.set_propagation_phase(gtk::PropagationPhase::Capture);
        let this = self.clone();
        drag.connect_drag_begin(move |g, _x, _y| {
            let mods = g.current_event_state();
            if !mods.contains(gtk::gdk::ModifierType::ALT_MASK) {
                return;
            }
            let inner = &this.0;
            // Alt+presionar no mueve el cursor principal: se guarda y se repone al soltar.
            let ins = inner.buffer.iter_at_mark(&inner.buffer.get_insert());
            let bound = inner.buffer.iter_at_mark(&inner.buffer.selection_bound());
            inner.extra_base.set(Some((ins.offset(), bound.offset())));
        });
        let this = self.clone();
        drag.connect_drag_end(move |g, dx, dy| {
            let inner = &this.0;
            let Some((ins_off, bound_off)) = inner.extra_base.take() else {
                return; // sin Alt: nada que hacer
            };
            g.set_state(gtk::EventSequenceState::Claimed);
            let (x0, y0) = {
                let (sx, sy) = g.start_point().unwrap_or((0.0, 0.0));
                (sx, sy)
            };
            let (x1, y1) = (x0 + dx, y0 + dy);
            let arrastre = ((x1 - x0) as i32).abs() + ((y1 - y0) as i32).abs() > 4;
            let mut extra = inner.extra.borrow_mut();
            if arrastre {
                let Some(a) = inner.view.iter_at_location(x0 as i32, y0 as i32) else {
                    return;
                };
                let Some(b) = inner.view.iter_at_location(x1 as i32, y1 as i32) else {
                    return;
                };
                extra.add_selection(&inner.buffer, &a, &b);
            } else {
                let Some(it) = inner.view.iter_at_location(x0 as i32, y0 as i32) else {
                    return;
                };
                if !extra.remove_at(&inner.buffer, &it) {
                    extra.add_cursor(&inner.buffer, &it);
                }
            }
            drop(extra);
            // El cursor principal queda donde estaba antes del Alt+clic.
            let ins = inner.buffer.iter_at_offset(ins_off);
            let bound = inner.buffer.iter_at_offset(bound_off);
            inner.buffer.select_range(&ins, &bound);
            this.actualizar_carenes();
        });
        i.view.add_controller(drag);

        // Teclas en captura: solo con cursores extra (FR-7). Esc, columnas y ediciones.
        let teclas = gtk::EventControllerKey::new();
        teclas.set_propagation_phase(gtk::PropagationPhase::Capture);
        let this = self.clone();
        teclas.connect_key_pressed(move |_, keyval, _code, state| {
            let inner = &this.0;
            if inner.extra.borrow().is_empty() {
                // Outliner (011): solo si el cursor está en una línea de lista y sin modificadores
                // ocupados. Alt+Arriba/Abajo mueve ítems (sin Ctrl/Shift).
                let ctrl = state.contains(gtk::gdk::ModifierType::CONTROL_MASK);
                let alt = state.contains(gtk::gdk::ModifierType::ALT_MASK);
                let shift = state.contains(gtk::gdk::ModifierType::SHIFT_MASK);
                // Plegado (028): Ctrl+Arriba/Abajo sobre un elemento plegable y Enter al final de
                // un elemento plegado; en cualquier otra línea, lo de siempre.
                if ctrl && !alt && !shift {
                    let plegar = match keyval {
                        gtk::gdk::Key::Up => Some(true),
                        gtk::gdk::Key::Down => Some(false),
                        _ => None,
                    };
                    if let Some(p) = plegar
                        && this.tecla_plegar(p)
                    {
                        return glib::Propagation::Stop;
                    }
                }
                // Enfoque (028): Alt+Derecha enfoca el elemento del cursor; Alt+Izquierda sube o
                // sale del enfoque.
                if alt && !ctrl && !shift {
                    let hecho = match keyval {
                        gtk::gdk::Key::Right => this.enfocar_en_cursor(),
                        gtk::gdk::Key::Left => this.subir_enfoque(),
                        _ => false,
                    };
                    if hecho {
                        return glib::Propagation::Stop;
                    }
                }
                if matches!(keyval, gtk::gdk::Key::Return | gtk::gdk::Key::KP_Enter)
                    && !ctrl
                    && !alt
                    && !shift
                    && this.enter_en_plegado()
                {
                    return glib::Propagation::Stop;
                }
                let en_lista = outliner::item_en_cursor(&inner.buffer).is_some();
                let intervenir = match (keyval, ctrl, alt, shift) {
                    (gtk::gdk::Key::Return | gtk::gdk::Key::KP_Enter, false, false, _)
                    | (gtk::gdk::Key::Tab, false, false, false) => en_lista,
                    (gtk::gdk::Key::ISO_Left_Tab, false, false, true) => en_lista,
                    (gtk::gdk::Key::Up | gtk::gdk::Key::Down, false, true, false) => en_lista,
                    _ => false,
                };
                if !intervenir {
                    return glib::Propagation::Proceed;
                }
                let hecho = match (keyval, shift) {
                    (gtk::gdk::Key::Return | gtk::gdk::Key::KP_Enter, _) => {
                        outliner::enter(&inner.buffer)
                    }
                    (gtk::gdk::Key::Tab, false) => this.indentar_plegado(true),
                    (gtk::gdk::Key::ISO_Left_Tab, true) => this.indentar_plegado(false),
                    (gtk::gdk::Key::Up, _) => this.mover_item_plegado(true),
                    (gtk::gdk::Key::Down, _) => this.mover_item_plegado(false),
                    _ => false,
                };
                return if hecho {
                    glib::Propagation::Stop
                } else {
                    glib::Propagation::Proceed
                };
            }
            let ctrl = state.contains(gtk::gdk::ModifierType::CONTROL_MASK);
            let alt = state.contains(gtk::gdk::ModifierType::ALT_MASK);
            let shift = state.contains(gtk::gdk::ModifierType::SHIFT_MASK);
            if ctrl && alt && !shift {
                let delta = match keyval {
                    gtk::gdk::Key::Up => -1,
                    gtk::gdk::Key::Down => 1,
                    _ => return glib::Propagation::Proceed,
                };
                this.cursor_en_columna(delta);
                return glib::Propagation::Stop;
            }
            if alt {
                return glib::Propagation::Proceed;
            }
            if keyval == gtk::gdk::Key::Escape {
                inner.extra.borrow_mut().clear(&inner.buffer);
                this.actualizar_carenes();
                return glib::Propagation::Stop;
            }
            // Con el popover de completado visible, sus teclas ganan (US3: solo cursor principal).
            if inner.completion.is_visible() {
                return glib::Propagation::Proceed;
            }
            let edit = if state
                .intersects(gtk::gdk::ModifierType::CONTROL_MASK | gtk::gdk::ModifierType::ALT_MASK)
            {
                return glib::Propagation::Proceed;
            } else {
                match keyval {
                    gtk::gdk::Key::BackSpace => Some(multi_cursor::EditKind::Backspace),
                    gtk::gdk::Key::Delete => Some(multi_cursor::EditKind::Delete),
                    gtk::gdk::Key::Return | gtk::gdk::Key::KP_Enter | gtk::gdk::Key::ISO_Enter => {
                        Some(multi_cursor::EditKind::Insert("\n".into()))
                    }
                    gtk::gdk::Key::Tab | gtk::gdk::Key::KP_Tab => {
                        Some(multi_cursor::EditKind::Insert("\t".into()))
                    }
                    _ => keyval
                        .to_unicode()
                        .filter(|c| !c.is_control())
                        .map(|c| multi_cursor::EditKind::Insert(c.to_string())),
                }
            };
            let Some(edit) = edit else {
                return glib::Propagation::Proceed;
            };
            let _ = multi_cursor::editar_tanda(&inner.buffer, &inner.extra.borrow(), &edit);
            this.actualizar_carenes();
            glib::Propagation::Stop
        });
        i.view.add_controller(teclas);
    }

    /// Ctrl+Alt+Arriba/Abajo: un cursor extra en la línea de arriba o de abajo (006). Un cursor
    /// extra no queda en texto oculto (028): lo que oculta esa línea se despliega antes.
    pub(super) fn cursor_en_columna(&self, delta: i32) -> bool {
        let inner = &self.0;
        let base = multi_cursor::base_columna(&inner.buffer, &inner.extra.borrow(), delta);
        let Some(it) = multi_cursor::iter_columna_adyacente(&inner.buffer, &base, delta) else {
            return false;
        };
        let offset = it.offset();
        if plegado::oculto(&it, &inner.tag_plegado) {
            self.revelar_linea(it.line() as u32);
        }
        let it = inner.buffer.iter_at_offset(offset);
        inner.extra.borrow_mut().add_cursor(&inner.buffer, &it);
        self.actualizar_carenes();
        true
    }

    /// Recalcula los rectángulos de los cursores extra para el overlay (research R2).
    pub(super) fn actualizar_carenes(&self) {
        let i = &self.0;
        if i.mode.get() != View::Codigo && i.mode.get() != View::Dividida {
            i.caren.limpiar();
            return;
        }
        let extra = i.extra.borrow();
        if extra.is_empty() {
            i.caren.limpiar();
            return;
        }
        let rects = std::rc::Rc::new(multi_cursor::rects_para_dibujar(&i.view, &i.buffer, &extra));
        i.caren.set_rects(&i.view, rects);
    }
}
