use super::*;

impl OnStage {
    /// Ajuste de texto: el paso de fuente más grande que entra en la ventana. La altura se mide
    /// con height-for-width al paso actual y se estima el del resto por linealidad; con el mismo
    /// contenido y la misma ventana la decisión es siempre la misma, así que no oscila.
    pub(super) fn ajustar(&self) {
        let i = &self.0;
        if i.stack
            .visible_child_name()
            .is_none_or(|n| n != "diapositiva")
        {
            return;
        }
        let view = i.reader.text_view();
        let scroller = i.reader.widget();
        let ancho = view.width();
        let disponible = scroller.height();
        if ancho < 120 || disponible < 120 {
            return;
        }
        let (_, natural, _, _) = view.measure(gtk::Orientation::Vertical, ancho);
        if natural <= 0 {
            return;
        }
        let px = i.px.get();
        // Altura estimada al paso base (30): los tamaños escalan casi linealmente.
        let en_base = natural as f64 * (ESCALAS_PX[0] as f64) / (px as f64);
        if en_base <= (disponible + TOLERANCIA_PX) as f64 {
            self.set_px(ESCALAS_PX[0]);
            return;
        }
        let escala = (ESCALAS_PX[0] as f64) * (disponible as f64) / en_base;
        let destino = ESCALAS_PX
            .iter()
            .copied()
            .find(|p| (*p as f64) <= escala)
            .unwrap_or(*ESCALAS_PX.last().unwrap());
        self.set_px(destino);
    }

    /// Programa el ajuste en un idle: nunca dentro de la disposición.
    pub(super) fn ajustar_en_reposo(&self) {
        if self.0.ajuste_programado.get() {
            return;
        }
        self.0.ajuste_programado.set(true);
        let this = self.clone();
        glib::idle_add_local_once(move || {
            this.0.ajuste_programado.set(false);
            this.ajustar();
        });
    }

    /// Escala la fuente de la diapositiva por el proveedor CSS propio (solo si cambia).
    pub(super) fn set_px(&self, px: i32) {
        let i = &self.0;
        if i.px.get() == px {
            return;
        }
        i.px.set(px);
        // El cambio de fuente dispara el señal del adjustment: se consume una vez para no
        // reajustar en cascada (el próximo cambio genuino de contenido o ventana reevalúa).
        self.0.ajuste_propio.set(true);
        i.proveedor_css.load_from_string(&format!(
            ".onstage-escala textview {{ font-size: {px}px; }}"
        ));
    }

    pub(super) fn conectar_ajuste(&self) {
        // El volcado terminó: la diapositiva nueva puede necesitar otro paso.
        let this = self.clone();
        self.0.reader.set_on_rendered(move || {
            this.ajustar_en_reposo();
        });
        // La ventana o el contenido cambiaron de alto: reevalúa. El propio cambio de fuente
        // dispara este señal una vez; se consume para no reajustar en cascada.
        let this = self.clone();
        let vadj = self.0.reader.widget().vadjustment();
        vadj.connect_changed(move |_| {
            if this.0.ajuste_propio.get() {
                this.0.ajuste_propio.set(false);
                return;
            }
            this.ajustar_en_reposo();
        });
        // Un enlace de la diapositiva se abre en la ventana principal.
        let this = self.clone();
        self.0.reader.set_on_link(move |url| {
            if let Some(f) = this.0.abrir.borrow().as_ref() {
                f(url);
            }
        });
    }
}
