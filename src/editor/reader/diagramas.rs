//! Los diagramas Mermaid (058) en la Lectura: un ` ```mermaid ` es una imagen anclada de su tamaño
//! natural (achicada si no entra en la columna), con la imagen de la caché de
//! `crate::diagramas` o pedida al pool de `gio`. Con un error, el fuente del bloque como código y
//! el motivo como texto visible (lo lee el lector de pantalla y lo encuentra la búsqueda); el
//! archivo no cambia.

use super::*;
use crate::diagramas::{self, Clave, Estilo};

impl Reader {
    /// El estilo de los diagramas de esta vista (claro u oscuro, escala del monitor).
    pub fn estilo_diagramas(&self) -> Estilo {
        Estilo::de(&self.0.view)
    }

    /// Las claves de los diagramas de un documento, para dibujarlos en el hilo de fondo antes de
    /// volcar ([`diagramas::precalentar`]).
    pub fn claves_diagramas(doc: &RenderDoc, estilo: Estilo) -> Vec<Clave> {
        fn bloques(out: &mut Vec<Clave>, blocks: &[Block], e: Estilo) {
            for b in blocks {
                match &b.kind {
                    BlockKind::Code { lang, text } if lang == "mermaid" => {
                        out.push(Clave::new(text, e));
                    }
                    BlockKind::List { items, .. } => {
                        for it in items {
                            bloques(out, &it.blocks, e);
                        }
                    }
                    BlockKind::Quote(body)
                    | BlockKind::Callout { body, .. }
                    | BlockKind::Group(body)
                    | BlockKind::Seccion { body, .. } => bloques(out, body, e),
                    BlockKind::Flashcard(f) => {
                        bloques(out, &f.question, e);
                        bloques(out, &f.answer, e);
                    }
                    BlockKind::Transclusion(t) => {
                        if let orbital_vault::render::Embed::Note { blocks, .. } = &t.content {
                            bloques(out, blocks, e);
                        }
                    }
                    _ => {}
                }
            }
        }
        let mut out = Vec::new();
        bloques(&mut out, &doc.blocks, estilo);
        out
    }

    pub(super) fn diagrama(
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
        let estilo = self.estilo_diagramas();
        // El SVG (décimas de milisegundo, en caché) dice si hay error y el tamaño; la imagen se
        // dibuja aparte.
        match diagramas::svg(fuente, estilo.tema) {
            Ok(d) => {
                let pic = diagramas::picture(Clave::new(fuente, estilo), &d.nombre());
                // Ampliar y copiar (059): clic, teclado y menú contextual.
                crate::diagrama_ampliado::interactivo(&pic, fuente, estilo);
                let (ancho, alto) = diagramas::medida(d.ancho, d.alto, self.0.width.get());
                pic.set_size_request(ancho, alto);
                pic.set_margin_top(6);
                pic.set_margin_bottom(6);
                let w: gtk::Widget = pic.upcast();
                self.0
                    .diagramas
                    .borrow_mut()
                    .push((w.downgrade(), d.ancho, d.alto));
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
