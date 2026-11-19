//! Los gráficos (052) en la Lectura: un ` ```grafico ` es un widget anclado (el lienzo con su
//! barra y sus deslizadores) que ocupa el ancho de la columna. Con un error, el fuente del bloque
//! como código y el motivo como texto visible (lo lee el lector de pantalla); el archivo no
//! cambia.

use super::*;

impl Reader {
    /// Sin interacción (OnStage): solo el dibujo de la vista inicial.
    pub fn fijar_graficos_estaticos(&self, si: bool) {
        self.0.graficos_estaticos.set(si);
    }

    pub(super) fn grafico(
        &self,
        s: &RenderState,
        end: &mut gtk::TextIter,
        fuente: &str,
        base: &[gtk::TextTag],
    ) {
        let buf = &s.buffer;
        let table = buf.tag_table();
        let fuente = fuente.strip_suffix('\n').unwrap_or(fuente);
        let refs: Vec<&gtk::TextTag> = base.iter().collect();
        match crate::graficos::leer(fuente) {
            Ok(g) => {
                let vista =
                    crate::graficos::GraficoVista::new(g, fuente, !self.0.graficos_estaticos.get());
                let w: gtk::Widget = vista.widget().clone().upcast();
                w.set_margin_top(4);
                w.set_margin_bottom(4);
                self.track_wide(&w);
                self.anchor(s, end, w);
                buf.insert_with_tags(end, "\n", &refs);
            }
            Err(e) => {
                let mut tags = base.to_vec();
                tags.extend(table.lookup("codeblock"));
                let codigo: Vec<&gtk::TextTag> = tags.iter().collect();
                buf.insert_with_tags(end, fuente, &codigo);
                buf.insert(end, "\n");
                let mut aviso = base.to_vec();
                aviso.extend(table.lookup("grafico-aviso"));
                let aviso: Vec<&gtk::TextTag> = aviso.iter().collect();
                buf.insert_with_tags(end, &format!("⚠ {e}\n"), &aviso);
            }
        }
    }
}
