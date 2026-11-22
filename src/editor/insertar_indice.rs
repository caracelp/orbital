//! «Insertar índice de etiqueta…» (036): un diálogo para elegir la etiqueta (de las de la
//! bóveda, del índice en memoria), la etiqueta de prioridad, si van los títulos o si es un índice
//! de índices. Al aceptar, el bloque ` ```indice ` queda en el cursor, en su propio párrafo y como
//! una sola acción de deshacer; solo escribe lo que difiere de Preferencias. Solo en notas `.md`.
//!
//! También «Nota nueva con esta etiqueta» desde Código: [`Editor::etiqueta_del_indice`] dice qué
//! etiqueta tiene el bloque ` ```indice ` donde está el cursor.

use super::*;
use adw::prelude::*;

impl Editor {
    /// Abre el diálogo. Devuelve el diálogo (las pruebas lo accionan) o `None` si no se puede.
    pub fn insertar_indice(&self) -> Option<adw::Dialog> {
        let i = &self.0;
        if self.current_path().is_none() {
            self.error("Abrí una nota para insertar un índice de etiqueta");
            return None;
        }
        if i.format.get() == Format::Html {
            self.error("«Insertar índice de etiqueta» es para las notas .md");
            return None;
        }
        let nombres: Vec<String> = i
            .search_index
            .borrow()
            .as_ref()
            .and_then(|x| x.read().ok().map(|x| x.tag_names()))
            .unwrap_or_default();
        if nombres.is_empty() {
            self.error("La bóveda no tiene etiquetas para un índice");
            return None;
        }
        let refs: Vec<&str> = nombres.iter().map(String::as_str).collect();
        let etiqueta = adw::ComboRow::builder()
            .title("Etiqueta")
            .model(&gtk::StringList::new(&refs))
            .enable_search(true)
            .build();
        etiqueta.set_expression(Some(gtk::PropertyExpression::new(
            gtk::StringObject::static_type(),
            gtk::Expression::NONE,
            "string",
        )));
        let base = reader::indice_defaults();
        let prioridad = adw::EntryRow::builder()
            .title("Etiqueta de prioridad (vacío: ninguna)")
            .text(&base.prioridad)
            .build();
        let titulos = adw::SwitchRow::builder()
            .title("Título de cada nota")
            .active(base.titulo)
            .build();
        let meta = adw::SwitchRow::builder()
            .title("Índice de índices")
            .subtitle("Listar las notas que tienen un índice, en lugar de una etiqueta")
            .build();
        meta.bind_property("active", &etiqueta, "sensitive")
            .invert_boolean()
            .sync_create()
            .build();
        let grupo = adw::PreferencesGroup::new();
        grupo.add(&etiqueta);
        grupo.add(&prioridad);
        grupo.add(&titulos);
        grupo.add(&meta);
        let insertar = gtk::Button::builder()
            .label("Insertar")
            .css_classes(["pill", "suggested-action"])
            .halign(gtk::Align::Center)
            .build();
        let caja = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(18)
            .margin_top(12)
            .margin_bottom(18)
            .margin_start(18)
            .margin_end(18)
            .build();
        caja.append(&grupo);
        caja.append(&insertar);
        let barra = adw::ToolbarView::new();
        barra.add_top_bar(&adw::HeaderBar::new());
        barra.set_content(Some(&caja));
        let dialogo = adw::Dialog::builder()
            .title("Insertar índice de etiqueta")
            .content_width(440)
            .child(&barra)
            .default_widget(&insertar)
            .build();
        let b = &i.buffer;
        let marca = b.create_mark(None, &b.iter_at_offset(b.cursor_position()), true);
        {
            let (e, d, m) = (self.clone(), dialogo.downgrade(), marca.clone());
            insertar.connect_clicked(move |_| {
                let elegida = etiqueta
                    .selected_item()
                    .and_downcast::<gtk::StringObject>()
                    .map(|s| s.string().to_string())
                    .unwrap_or_default();
                let bloque = orbital_vault::indice::bloque(
                    &elegida,
                    prioridad.text().trim(),
                    titulos.is_active(),
                    meta.is_active(),
                    &base,
                );
                let b = &e.0.buffer;
                let a = b.iter_at_mark(&m);
                let texto = ecuacion::en_su_parrafo(b.upcast_ref(), &a, &a, &bloque);
                b.begin_user_action();
                let mut en = b.iter_at_mark(&m);
                b.insert(&mut en, &texto);
                b.end_user_action();
                b.place_cursor(&en);
                if let Some(d) = d.upgrade() {
                    d.close();
                }
            });
        }
        {
            let e = self.clone();
            dialogo.connect_closed(move |_| {
                let b = &e.0.buffer;
                if !marca.is_deleted() {
                    b.delete_mark(&marca);
                }
            });
        }
        dialogo.present(Some(&i.view));
        Some(dialogo)
    }

    /// El texto que muestra la Lectura (pruebas de la ventana).
    #[cfg(test)]
    pub(crate) fn texto_lectura(&self) -> String {
        let b = self.0.reader.text_view().buffer();
        b.text(&b.start_iter(), &b.end_iter(), true).to_string()
    }

    /// La etiqueta del bloque ` ```indice ` donde está el cursor (con los valores por defecto de
    /// Preferencias), o `None` si el cursor no está en uno o es un índice de índices.
    pub fn etiqueta_del_indice(&self) -> Option<String> {
        let b = &self.0.buffer;
        let cursor = b.iter_at_offset(b.cursor_position()).line();
        let linea = |n: i32| -> Option<String> {
            let a = b.iter_at_line(n)?;
            let mut z = a;
            if !z.ends_line() {
                z.forward_to_line_end();
            }
            Some(b.text(&a, &z, true).to_string())
        };
        // La cerca de apertura: hacia arriba, sin cruzar otra cerca.
        let mut inicio = None;
        for n in (0..=cursor).rev() {
            let l = linea(n)?;
            let t = l.trim_start();
            if t.starts_with("```") {
                if t.trim_start_matches('`').trim() == "indice" {
                    inicio = Some(n);
                }
                break;
            }
        }
        let inicio = inicio?;
        let mut cuerpo = String::new();
        let mut n = inicio + 1;
        loop {
            let l = linea(n)?;
            if l.trim_start().starts_with("```") {
                break;
            }
            if n > cursor + 200 {
                return None;
            }
            cuerpo.push_str(&l);
            cuerpo.push('\n');
            n += 1;
        }
        if cursor > n {
            return None;
        }
        orbital_vault::indice::parse(&cuerpo, &reader::indice_defaults())
            .ok()
            .filter(|o| !o.meta)
            .map(|o| o.etiqueta)
    }
}
