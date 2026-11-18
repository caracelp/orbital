use super::*;

impl OnStage {
    pub(super) fn conectar_navegacion(&self) {
        let this = self.clone();
        self.0.atras.connect_clicked(move |_| this.retroceder());
        let this = self.clone();
        self.0.adelante.connect_clicked(move |_| this.avanzar());
        let this = self.clone();
        let teclas = gtk::EventControllerKey::new();
        teclas.set_propagation_phase(gtk::PropagationPhase::Capture);
        teclas.connect_key_pressed(move |_, keyval, _, _| {
            use gtk::gdk::Key;
            if keyval == Key::Escape {
                this.0.window.set_visible(false);
                return glib::Propagation::Stop;
            }
            if keyval == Key::F11 {
                this.alternar_calm();
                return glib::Propagation::Stop;
            }
            // Con el índice visible, las teclas van a sus botones (teclado nativo).
            if this
                .0
                .stack
                .visible_child_name()
                .is_some_and(|n| n == "indice")
            {
                return glib::Propagation::Proceed;
            }
            match keyval {
                Key::Right
                | Key::Down
                | Key::Page_Down
                | Key::Return
                | Key::KP_Enter
                | Key::space => {
                    this.avanzar();
                    glib::Propagation::Stop
                }
                Key::Left | Key::Up | Key::Page_Up | Key::BackSpace => {
                    this.retroceder();
                    glib::Propagation::Stop
                }
                Key::Home => {
                    this.ir_a(0);
                    glib::Propagation::Stop
                }
                Key::End => {
                    this.ir_a(usize::MAX);
                    glib::Propagation::Stop
                }
                _ => glib::Propagation::Proceed,
            }
        });
        self.0.window.add_controller(teclas);
    }

    pub(super) fn conectar_toggles(&self) {
        let this = self.clone();
        self.0.toggle_portada.connect_toggled(move |_| {
            this.render_actual();
        });
        let this = self.clone();
        self.0.toggle_toc.connect_toggled(move |_| {
            this.rearmar_toc();
            this.render_actual();
        });
        let this = self.clone();
        self.0.toggle_calm.connect_toggled(move |t| {
            if t.is_active() {
                this.0.window.fullscreen();
            } else {
                this.0.window.unfullscreen();
            }
            this.0.contador_flotante.set_visible(false);
        });
        // El gestor de ventanas puede salir de pantalla completa por su cuenta: sincroniza.
        let this = self.clone();
        self.0
            .window
            .connect_notify_local(Some("fullscreened"), move |_, _| {
                if !this.0.window.is_fullscreen() {
                    this.0.toggle_calm.set_active(false);
                    this.0.contador_flotante.set_visible(false);
                }
            });
        let this = self.clone();
        self.0.window.connect_close_request(move |_| {
            if let Some(id) = this.0.refresco_pendiente.borrow_mut().take() {
                id.remove();
            }
            glib::Propagation::Proceed
        });
    }

    /// El contador flotante solo vive en modo calm y reaparece al mover el mouse.
    pub(super) fn conectar_contador_flotante(&self) {
        let this = self.clone();
        let motion = gtk::EventControllerMotion::new();
        motion.connect_motion(move |_, _, _| {
            if !this.0.toggle_calm.is_active() {
                return;
            }
            this.0.contador_flotante.set_visible(true);
            if let Some(previo) = this.0.contador_pendiente.borrow_mut().take() {
                previo.remove();
            }
            let este = this.clone();
            let id = glib::timeout_add_local_once(CONTADOR_MUESTRA, move || {
                *este.0.contador_pendiente.borrow_mut() = None;
                este.0.contador_flotante.set_visible(false);
            });
            *this.0.contador_pendiente.borrow_mut() = Some(id);
        });
        self.0.stack.add_controller(motion);
    }

    pub(super) fn alternar_calm(&self) {
        self.0
            .toggle_calm
            .set_active(!self.0.toggle_calm.is_active());
    }

    /// Portada activada y con datos (título del frontmatter).
    pub(super) fn portada_visible(&self) -> bool {
        self.0.toggle_portada.is_active() && self.0.portada_datos.borrow().is_some()
    }

    /// Índice activado y con diapositivas.
    pub(super) fn toc_visible(&self) -> bool {
        self.0.toggle_toc.is_active() && !self.0.diapositivas.borrow().is_empty()
    }

    pub(super) fn prefijo(&self) -> usize {
        usize::from(self.portada_visible()) + usize::from(self.toc_visible())
    }

    pub(super) fn total(&self) -> usize {
        self.prefijo() + self.0.diapositivas.borrow().len()
    }

    pub(super) fn avanzar(&self) {
        self.ir_a(self.0.indice.get() + 1);
    }

    pub(super) fn retroceder(&self) {
        self.ir_a(self.0.indice.get().saturating_sub(1));
    }

    pub(super) fn ir_a(&self, destino: usize) {
        let total = self.total();
        let indice = destino.min(total.saturating_sub(1));
        if self.0.indice.get() == indice {
            return;
        }
        self.0.indice.set(indice);
        self.render_actual();
    }

    /// Contador y sensibilidad de los botones de navegación según el índice vigente.
    pub(super) fn actualizar_estado(&self) {
        let total = self.total();
        let indice = self.0.indice.get().min(total.saturating_sub(1));
        self.0.indice.set(indice);
        let texto = format!("Diapositiva {} de {}", indice + 1, total.max(1));
        let corto = format!("{} de {}", indice + 1, total);
        self.0.contador_barra.set_label(&corto);
        self.0.contador_barra.set_tooltip_text(Some(&texto));
        self.0.contador_flotante.set_label(&corto);
        self.0.atras.set_sensitive(indice > 0);
        self.0.adelante.set_sensitive(indice + 1 < total);
    }
}
