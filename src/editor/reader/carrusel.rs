//! El carrusel de medios (032) en la Lectura: un widget anclado del ancho de la columna
//! ([`crate::carrusel`]) y, debajo, los avisos de lo que se ignoró como texto tenue (lo lee el
//! lector de pantalla y lo encuentra la búsqueda).

use super::*;

impl Reader {
    pub(super) fn carrusel(
        &self,
        s: &RenderState,
        end: &mut gtk::TextIter,
        c: &orbital_vault::carrusel::Carrusel,
        base: &[gtk::TextTag],
    ) {
        let buf = &s.buffer;
        let refs: Vec<&gtk::TextTag> = base.iter().collect();
        if !c.diapositivas.is_empty() {
            // Los enlaces (un PDF) se abren como los de la nota.
            let debil = Rc::downgrade(&self.0);
            let abrir: crate::carrusel::AbrirFn = Rc::new(move |url: &str| {
                if let Some(i) = debil.upgrade()
                    && let Some(f) = i.on_link.borrow().as_ref()
                {
                    f(url);
                }
            });
            let w = crate::carrusel::widget(Rc::new(c.clone()), self.0.width.get(), abrir);
            self.track_wide(&w);
            self.anchor(s, end, w);
            buf.insert_with_tags(end, "\n", &refs);
        }
        if !c.avisos.is_empty() {
            let mut dim = base.to_vec();
            dim.extend(buf.tag_table().lookup("dim"));
            let dim: Vec<&gtk::TextTag> = dim.iter().collect();
            for a in &c.avisos {
                buf.insert_with_tags(end, &format!("⚠ {a}\n"), &dim);
            }
        }
    }
}
