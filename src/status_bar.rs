//! Barra inferior del diseño de interfaz (025): versión, totales de la bóveda y contadores de la nota
//! (y de la selección). Plegable: plegada queda una pestañita con los contadores y el botón para
//! volver a mostrarla, como en la maqueta.

use gtk::prelude::*;
use std::rc::Rc;

/// Líneas, palabras y caracteres de un texto (como `getStats` de la maqueta).
pub fn contar(texto: &str) -> (usize, usize, usize) {
    if texto.is_empty() {
        return (0, 0, 0);
    }
    let lineas = texto.split('\n').count();
    let palabras = texto.split_whitespace().count();
    let caracteres = texto.chars().count();
    (lineas, palabras, caracteres)
}

struct Inner {
    bar: gtk::Box,
    tab: gtk::Box,
    totales: gtk::Label,
    seleccion: gtk::Label,
    contadores: gtk::Label,
    contadores_tab: gtk::Label,
}

#[derive(Clone)]
pub struct StatusBar(Rc<Inner>);

impl StatusBar {
    pub fn new() -> Self {
        let version = gtk::Label::new(Some(&format!("Orbital · v{}", env!("CARGO_PKG_VERSION"))));
        let totales = gtk::Label::new(None);
        let izquierda = gtk::Box::builder().spacing(22).hexpand(true).build();
        izquierda.append(&version);
        izquierda.append(&totales);
        let seleccion = gtk::Label::builder()
            .css_classes(["orbital-pink"])
            .visible(false)
            .margin_end(20)
            .build();
        let contadores = gtk::Label::new(None);
        let ocultar = crate::iconos::boton("abajo", "Ocultar barra inferior");
        let bar = gtk::Box::builder()
            .spacing(10)
            .css_classes(["orbital-status"])
            .build();
        bar.append(&izquierda);
        bar.append(&seleccion);
        bar.append(&contadores);
        bar.append(&ocultar);

        let contadores_tab = gtk::Label::new(None);
        let mostrar = crate::iconos::boton("arriba", "Mostrar barra inferior");
        let tab = gtk::Box::builder()
            .spacing(6)
            .halign(gtk::Align::End)
            .valign(gtk::Align::End)
            .visible(false)
            .css_classes(["orbital-status-tab"])
            .build();
        tab.append(&contadores_tab);
        tab.append(&mostrar);

        let this = Self(Rc::new(Inner {
            bar,
            tab,
            totales,
            seleccion,
            contadores,
            contadores_tab,
        }));
        let t = this.clone();
        ocultar.connect_clicked(move |_| t.set_abierta(false));
        let t = this.clone();
        mostrar.connect_clicked(move |_| t.set_abierta(true));
        this.set_totales(0, 0, 0);
        this.set_contadores((0, 0, 0), None);
        this
    }

    /// La barra completa (barra inferior de la ventana).
    pub fn widget(&self) -> &gtk::Box {
        &self.0.bar
    }

    /// La pestañita que queda con la barra plegada (va en un `gtk::Overlay` sobre el contenido).
    pub fn tab(&self) -> &gtk::Box {
        &self.0.tab
    }

    pub fn set_abierta(&self, abierta: bool) {
        self.0.bar.set_visible(abierta);
        self.0.tab.set_visible(!abierta);
    }

    pub fn set_totales(&self, notas: usize, carpetas: usize, archivos: usize) {
        self.0.totales.set_text(&format!(
            "Notas: {notas} · Carpetas: {carpetas} · Archivos: {archivos}"
        ));
    }

    /// Contadores de la nota y, si hay, de la selección.
    pub fn set_contadores(
        &self,
        (l, p, c): (usize, usize, usize),
        seleccion: Option<(usize, usize, usize)>,
    ) {
        let texto = format!("Líneas: {l}    Palabras: {p}    Caracteres: {c}");
        self.0.contadores.set_text(&texto);
        self.0.contadores_tab.set_text(&texto);
        match seleccion {
            Some((l, p, c)) => {
                self.0.seleccion.set_text(&format!(
                    "Selección · Líneas: {l}    Palabras: {p}    Caracteres: {c}"
                ));
                self.0.seleccion.set_visible(true);
            }
            None => self.0.seleccion.set_visible(false),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::contar;

    #[test]
    fn cuenta_como_la_maqueta() {
        assert_eq!(contar(""), (0, 0, 0));
        assert_eq!(contar("hola"), (1, 1, 4));
        assert_eq!(contar("una línea\n\notra  más\n"), (4, 4, 21));
    }
}
