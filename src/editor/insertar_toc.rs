//! «Insertar índice…» (035): un diálogo chico con las opciones de la tabla de contenidos (estilo,
//! niveles, título, enlaces y qué hacer sin encabezados), que parte de los valores por defecto de
//! Preferencias. Al aceptar, el bloque ` ```toc ` queda en el cursor, en su propio párrafo y como
//! una sola acción de deshacer; solo escribe las opciones que difieren de Preferencias, así el
//! bloque las sigue. Solo en notas `.md`.

use super::*;
use adw::prelude::*;
use orbital_vault::toc::{TocDefaults, TocStyle};

impl Editor {
    /// Abre el diálogo. Devuelve el diálogo (las pruebas lo accionan) o `None` si no se puede.
    pub fn insertar_toc(&self) -> Option<adw::Dialog> {
        let i = &self.0;
        if self.current_path().is_none() {
            self.error("Abrí una nota para insertar un índice");
            return None;
        }
        if i.format.get() == Format::Html {
            self.error("«Insertar índice» es para las notas .md");
            return None;
        }
        let base = reader::toc_defaults();
        let estilo = adw::ComboRow::builder()
            .title("Estilo")
            .model(&gtk::StringList::new(&[
                "Lista con viñetas",
                "Lista numerada",
                "Primer nivel en una línea",
            ]))
            .selected(
                TocStyle::TODOS
                    .iter()
                    .position(|s| *s == base.style)
                    .unwrap_or(0) as u32,
            )
            .build();
        let nivel = |titulo: &str, subtitulo: &str, valor: u8| {
            adw::SpinRow::builder()
                .title(titulo)
                .subtitle(subtitulo)
                .adjustment(&gtk::Adjustment::new(valor as f64, 0.0, 6.0, 1.0, 1.0, 0.0))
                .digits(0)
                .numeric(true)
                .build()
        };
        let minimo = nivel("Nivel mínimo", "0: el menor de la nota", base.min_level);
        let maximo = nivel("Nivel máximo", "0: sin límite", base.max_level);
        let titulo = adw::EntryRow::builder()
            .title("Título (vacío: sin título)")
            .text(&base.title)
            .build();
        let enlaces = adw::SwitchRow::builder()
            .title("Enlaces a los encabezados")
            .active(base.include_links)
            .build();
        let ocultar = adw::SwitchRow::builder()
            .title("Ocultar si no hay encabezados")
            .active(base.hide_when_empty)
            .build();
        let grupo = adw::PreferencesGroup::new();
        grupo.add(&estilo);
        grupo.add(&minimo);
        grupo.add(&maximo);
        grupo.add(&titulo);
        grupo.add(&enlaces);
        grupo.add(&ocultar);
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
            .title("Insertar índice")
            .content_width(440)
            .child(&barra)
            .default_widget(&insertar)
            .build();
        // Dónde va el bloque: el cursor al abrir el diálogo.
        let b = &i.buffer;
        let marca = b.create_mark(None, &b.iter_at_offset(b.cursor_position()), true);
        {
            let (e, d, m) = (self.clone(), dialogo.downgrade(), marca.clone());
            insertar.connect_clicked(move |_| {
                let nivel = |r: &adw::SpinRow| r.value().round().clamp(0.0, 6.0) as u8;
                let elegidas = TocDefaults {
                    title: titulo.text().trim().to_owned(),
                    style: TocStyle::TODOS
                        .get(estilo.selected() as usize)
                        .copied()
                        .unwrap_or(TocStyle::NestedList),
                    min_level: nivel(&minimo),
                    max_level: nivel(&maximo),
                    include_links: enlaces.is_active(),
                    hide_when_empty: ocultar.is_active(),
                };
                let bloque = orbital_vault::toc::bloque(&elegidas, &base);
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
}
