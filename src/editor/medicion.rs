//! Medición de la latencia al escribir (`orbital --medir-escritura`).

use super::*;

impl Editor {
    /// Desarrollo: inserta `chars` caracteres de a uno al final del texto y devuelve, en ms, el
    /// trabajo de cada tecla: la inserción más el cálculo y dibujo de su cuadro (sin contar la
    /// espera de la sincronización vertical), separado en `(inserción, cuadro)`: la inserción es lo
    /// que corre al cambiar el texto (los manejadores de `changed`); el cuadro, disposición y dibujo.
    /// Para comprobar FR-022 (< 16 ms).
    pub async fn measure_typing(&self, chars: usize) -> Vec<(f64, f64)> {
        use std::time::Instant;
        let view = self.0.view.clone();
        // La medición es de la vista Código: con una configuración nueva la nota abre en
        // Lectura y la vista de código no tiene reloj de cuadros (no está a la vista).
        if view.frame_clock().is_none() {
            self.set_view(View::Codigo);
            for _ in 0..50 {
                if view.frame_clock().is_some() {
                    break;
                }
                glib::timeout_future(Duration::from_millis(20)).await;
            }
        }
        let Some(clock) = view.frame_clock() else {
            return Vec::new();
        };
        // Marcas del primer cuadro que empieza después de la tecla (t0).
        let t0: Rc<Cell<Instant>> = Rc::new(Cell::new(Instant::now()));
        let update_at: Rc<Cell<Option<Instant>>> = Rc::default();
        let painted: Rc<Cell<Option<Instant>>> = Rc::default();
        let (u, t) = (update_at.clone(), t0.clone());
        let h1 = clock.connect_update(move |_| {
            let now = Instant::now();
            if u.get().is_none() && now > t.get() {
                u.set(Some(now));
            }
        });
        let (pa, u2) = (painted.clone(), update_at.clone());
        let h2 = clock.connect_after_paint(move |_| {
            if pa.get().is_none() && u2.get().is_some() {
                pa.set(Some(Instant::now()));
            }
        });
        let buffer = self.0.buffer.clone();
        buffer.place_cursor(&buffer.end_iter());
        let mut out = Vec::with_capacity(chars);
        for k in 0..chars {
            update_at.set(None);
            painted.set(None);
            let start = Instant::now();
            t0.set(start);
            buffer.insert_at_cursor(if k % 60 == 59 { "\n" } else { "a" });
            let insert = start.elapsed();
            view.queue_draw();
            // Si la ventana está tapada, GNOME puede frenar los cuadros: no se espera más de 1 s.
            while painted.get().is_none() && start.elapsed() < Duration::from_secs(1) {
                glib::timeout_future(Duration::from_millis(1)).await;
            }
            let frame = match (update_at.get(), painted.get()) {
                (Some(u), Some(p)) => p.saturating_duration_since(u),
                _ => continue,
            };
            out.push((insert.as_secs_f64() * 1000.0, frame.as_secs_f64() * 1000.0));
        }
        clock.disconnect(h1);
        clock.disconnect(h2);
        out
    }
}
