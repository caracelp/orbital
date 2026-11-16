//! Diálogo «Guardar en…» (027): una casilla por colección y un campo para crear una nueva.
//!
//! Las casillas y el nombre nuevo se aplican al pulsar «Guardar» (o Intro en el campo);
//! «Cancelar» no cambia nada (decisión 10 del plan). Un nombre repetido o hecho solo de espacios
//! desactiva «Guardar» y lo explica dentro del diálogo, sin cerrarlo.

use std::rc::Rc;

use adw::prelude::*;

/// Lo que eligió la usuaria.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Eleccion {
    /// Cada colección (por nombre) con el estado de su casilla.
    pub casillas: Vec<(String, bool)>,
    /// Colección nueva que contendrá la nota.
    pub nueva: Option<String>,
}

/// El diálogo armado; la ventana lo presenta y conecta la respuesta.
pub struct GuardarEn {
    pub dialogo: adw::AlertDialog,
    casillas: Vec<(String, gtk::CheckButton)>,
    entrada: gtk::Entry,
}

fn clave(nombre: &str) -> String {
    nombre.trim().to_lowercase()
}

impl GuardarEn {
    /// `colecciones`: nombre y si ya contiene la nota.
    pub fn new(nota: &str, colecciones: &[(String, bool)]) -> Self {
        let dialogo = adw::AlertDialog::new(
            Some("Guardar en…"),
            Some(&format!("Colecciones de «{nota}»")),
        );
        let caja = gtk::Box::new(gtk::Orientation::Vertical, 12);
        let mut casillas = Vec::with_capacity(colecciones.len());
        if !colecciones.is_empty() {
            let list = gtk::ListBox::builder()
                .selection_mode(gtk::SelectionMode::None)
                .css_classes(["boxed-list"])
                .build();
            list.update_property(&[gtk::accessible::Property::Label("Colecciones")]);
            for (nombre, dentro) in colecciones {
                let casilla = gtk::CheckButton::builder()
                    .label(nombre)
                    .active(*dentro)
                    .margin_top(6)
                    .margin_bottom(6)
                    .margin_start(6)
                    .margin_end(6)
                    .build();
                casilla.update_property(&[gtk::accessible::Property::Label(nombre)]);
                list.append(&casilla);
                casillas.push((nombre.clone(), casilla));
            }
            let scroll = gtk::ScrolledWindow::builder()
                .hscrollbar_policy(gtk::PolicyType::Never)
                .propagate_natural_height(true)
                .max_content_height(280)
                .child(&list)
                .build();
            caja.append(&scroll);
        }
        let entrada = gtk::Entry::builder()
            .placeholder_text("Colección nueva")
            .activates_default(true)
            .build();
        entrada.update_property(&[gtk::accessible::Property::Label("Colección nueva")]);
        let aviso = gtk::Label::builder()
            .xalign(0.0)
            .wrap(true)
            .visible(false)
            .css_classes(["error"])
            .build();
        caja.append(&entrada);
        caja.append(&aviso);
        dialogo.set_extra_child(Some(&caja));
        dialogo.add_response("cancelar", "Cancelar");
        dialogo.add_response("guardar", "Guardar");
        dialogo.set_response_appearance("guardar", adw::ResponseAppearance::Suggested);
        dialogo.set_default_response(Some("guardar"));
        dialogo.set_close_response("cancelar");
        let existentes: Rc<Vec<String>> =
            Rc::new(colecciones.iter().map(|(n, _)| clave(n)).collect());
        let d = dialogo.clone();
        entrada.connect_changed(move |e| {
            let texto = e.text();
            let problema = if !texto.is_empty() && texto.trim().is_empty() {
                Some("El nombre no puede ser solo espacios")
            } else if !texto.trim().is_empty() && existentes.contains(&clave(&texto)) {
                Some("Ya existe una colección con ese nombre: marcá su casilla")
            } else {
                None
            };
            aviso.set_label(problema.unwrap_or(""));
            aviso.set_visible(problema.is_some());
            d.set_response_enabled("guardar", problema.is_none());
            if let Some(p) = problema {
                e.update_property(&[gtk::accessible::Property::Description(p)]);
            } else {
                e.reset_property(gtk::AccessibleProperty::Description);
            }
        });
        let f = entrada.clone();
        gtk::glib::idle_add_local_once(move || {
            f.grab_focus();
        });
        Self {
            dialogo,
            casillas,
            entrada,
        }
    }

    /// Lee lo elegido sin sostener el diálogo (para su señal `response`: los widgets de adentro
    /// no mantienen vivo al diálogo, así que no se forma un ciclo).
    pub fn lector(&self) -> impl Fn() -> Eleccion + 'static {
        let casillas = self.casillas.clone();
        let entrada = self.entrada.clone();
        move || {
            let nueva = entrada.text().trim().to_owned();
            Eleccion {
                casillas: casillas
                    .iter()
                    .map(|(n, c)| (n.clone(), c.is_active()))
                    .collect(),
                nueva: (!nueva.is_empty()).then_some(nueva),
            }
        }
    }

    /// Las casillas, en el orden de las colecciones (para las pruebas).
    #[cfg(test)]
    pub fn casillas(&self) -> impl Iterator<Item = &gtk::CheckButton> {
        self.casillas.iter().map(|(_, c)| c)
    }

    #[cfg(test)]
    pub fn entrada(&self) -> &gtk::Entry {
        &self.entrada
    }
}
