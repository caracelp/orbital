//! La galería de la nota (033) en la Lectura: un widget anclado del ancho de la columna
//! ([`crate::galeria`]) y, debajo, los avisos de lo que se ignoró como texto tenue (lo lee el
//! lector de pantalla y lo encuentra la búsqueda).

use super::*;

impl Reader {
    pub(super) fn galeria(
        &self,
        s: &RenderState,
        end: &mut gtk::TextIter,
        g: &orbital_vault::galeria::Galeria,
        base: &[gtk::TextTag],
    ) {
        let buf = &s.buffer;
        let refs: Vec<&gtk::TextTag> = base.iter().collect();
        // Los enlaces (no los hay en la galería, pero el visor del carrusel los acepta) se abren
        // como los de la nota.
        let debil = Rc::downgrade(&self.0);
        let abrir: crate::galeria::AbrirFn = Rc::new(move |url: &str| {
            if let Some(i) = debil.upgrade()
                && let Some(f) = i.on_link.borrow().as_ref()
            {
                f(url);
            }
        });
        let w = crate::galeria::widget(Rc::new(g.clone()), self.0.width.get(), abrir);
        self.track_wide(&w);
        self.anchor(s, end, w);
        buf.insert_with_tags(end, "\n", &refs);
        if !g.avisos.is_empty() {
            let mut dim = base.to_vec();
            dim.extend(buf.tag_table().lookup("dim"));
            let dim: Vec<&gtk::TextTag> = dim.iter().collect();
            for a in &g.avisos {
                buf.insert_with_tags(end, &format!("⚠ {a}\n"), &dim);
            }
        }
    }
}
