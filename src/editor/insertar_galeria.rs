//! «Insertar galería…» (033): un diálogo para elegir la carpeta de la bóveda, la disposición y
//! si entran las subcarpetas; al aceptar, el bloque ` ```galeria ` queda en el cursor, en su
//! propio párrafo y como una sola acción de deshacer. Solo en notas `.md`. La lista de carpetas
//! sale del índice de la bóveda (no se lee el disco).

use super::*;
use adw::prelude::*;
use orbital_vault::galeria::Layout;

impl Editor {
    /// Abre el diálogo. Devuelve el diálogo (las pruebas lo accionan) o `None` si no se puede.
    pub fn insertar_galeria(&self) -> Option<adw::Dialog> {
        let i = &self.0;
        if self.current_path().is_none() {
            self.error("Abrí una nota para insertar una galería");
            return None;
        }
        if i.format.get() == Format::Html {
            self.error("«Insertar galería» es para las notas .md");
            return None;
        }
        let vault = i.vault.borrow().clone()?;
        let mut carpetas: Vec<&str> = vault
            .entries()
            .iter()
            .filter(|e| e.kind == orbital_vault::EntryKind::Folder)
            .map(|e| e.path.as_str())
            .filter(|p| !p.split('/').any(|c| c.starts_with('.')))
            .collect();
        if carpetas.is_empty() {
            self.error("La bóveda no tiene carpetas para mostrar en una galería");
            return None;
        }
        carpetas.sort_by_cached_key(|p| p.to_lowercase());
        let lista = gtk::StringList::new(&carpetas);
        let carpeta = adw::ComboRow::builder()
            .title("Carpeta")
            .model(&lista)
            .enable_search(true)
            .build();
        carpeta.set_expression(Some(gtk::PropertyExpression::new(
            gtk::StringObject::static_type(),
            gtk::Expression::NONE,
            "string",
        )));
        let nombres = [
            "Miniaturas",
            "Masonry (columnas)",
            "Justificadas (filas)",
            "Carrusel",
        ];
        let disposicion = adw::ComboRow::builder()
            .title("Disposición")
            .model(&gtk::StringList::new(&nombres))
            .build();
        let subcarpetas = adw::SwitchRow::builder()
            .title("Incluir las subcarpetas")
            .build();
        let grupo = adw::PreferencesGroup::new();
        grupo.add(&carpeta);
        grupo.add(&disposicion);
        grupo.add(&subcarpetas);
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
            .title("Insertar galería")
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
                let elegida = carpeta
                    .selected_item()
                    .and_downcast::<gtk::StringObject>()
                    .map(|s| s.string().to_string())
                    .unwrap_or_default();
                let layout = Layout::TODOS
                    .get(disposicion.selected() as usize)
                    .copied()
                    .unwrap_or_default();
                let bloque =
                    orbital_vault::galeria::bloque(&elegida, layout, subcarpetas.is_active());
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
