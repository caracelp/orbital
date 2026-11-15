//! Historial de versiones de la nota (010).

use super::*;

impl Window {
    /// Diálogo «Historial de versiones…» (010, US2): lista con fecha/hora, diff contra el
    /// contenido actual y «Restaurar» que carga la versión en el editor como edición.
    pub fn historial(&self) {
        let i = &self.0;
        let Some(path) = i.code_editor.current_path() else {
            self.toast("Abrí una nota antes de ver su historial");
            return;
        };
        let Some(vault) = i.vault.borrow().clone() else {
            return;
        };
        let this = self.clone();
        glib::spawn_future_local(async move {
            let (v, p) = (vault.clone(), path.clone());
            let versiones =
                match gio::spawn_blocking(move || orbital_vault::versiones_historial(&v, &p)).await
                {
                    Ok(Ok(v)) => v,
                    Ok(Err(error)) => {
                        this.toast(&format!("No se pudo leer el historial: {error}"));
                        Vec::new()
                    }
                    Err(_) => {
                        this.toast("No se pudo leer el historial");
                        Vec::new()
                    }
                };
            if versiones.is_empty() {
                this.toast("Esta nota todavía no tiene versiones en el historial");
                return;
            }
            let dialog = this.dialogo_historial(&path, &versiones);
            dialog.present(Some(&this.0.window));
        });
    }

    /// Construye el diálogo de historial (010, US2). Separado de `historial()` para poder
    /// probarlo dentro de un proceso GTK sin presentación.
    pub(super) fn dialogo_historial(
        &self,
        path: &RelPath,
        versiones: &[(String, String)],
    ) -> adw::Dialog {
        let dialog = adw::Dialog::builder()
            .title("Historial de versiones")
            .content_width(720)
            .content_height(560)
            .build();
        let root = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(0)
            .build();
        let header = adw::HeaderBar::new();
        header.set_title_widget(Some(&adw::WindowTitle::new(
            &format!("Historial: {}", path.file_name()),
            "",
        )));
        let restaurar = gtk::Button::builder()
            .label("Restaurar")
            .sensitive(false)
            .css_classes(["suggested-action"])
            .build();
        header.pack_end(&restaurar);
        root.append(&header);
        let lista = gtk::ListBox::builder()
            .selection_mode(gtk::SelectionMode::Single)
            .css_classes(["navigation-sidebar"])
            .build();
        lista.update_property(&[gtk::accessible::Property::Label("Versiones del historial")]);
        for (nombre, _contenido) in versiones {
            let fila = gtk::Box::builder()
                .orientation(gtk::Orientation::Horizontal)
                .spacing(8)
                .margin_start(12)
                .margin_end(12)
                .margin_top(6)
                .margin_bottom(6)
                .build();
            let etiqueta = gtk::Label::builder()
                .label(Self::nombre_version_legible(nombre))
                .xalign(0.0)
                .hexpand(true)
                .build();
            fila.append(&etiqueta);
            lista.append(&fila);
        }
        let scroll = gtk::ScrolledWindow::builder()
            .child(&lista)
            .vexpand(true)
            .height_request(200)
            .build();
        let diff_view = editor::find::vista_diff();
        let diff_scroll = gtk::ScrolledWindow::builder()
            .child(&diff_view)
            .vexpand(true)
            .build();
        let separador = gtk::Separator::new(gtk::Orientation::Horizontal);
        root.append(&separador);
        root.append(&scroll);
        root.append(&diff_scroll);
        dialog.set_child(Some(&root));
        self.cargar_diff_historial(path, &versiones[0].0, &diff_view);
        {
            let ventana = self.clone();
            let path = path.clone();
            let versiones = versiones.to_vec();
            let diff_view = diff_view.clone();
            let restaurar = restaurar.clone();
            lista.connect_row_selected(move |_, fila| {
                restaurar.set_sensitive(fila.is_some());
                let Some(fila) = fila else { return };
                let indice = fila.index();
                if indice < 0 {
                    return;
                }
                let (nombre, _contenido) = &versiones[indice as usize];
                ventana.cargar_diff_historial(&path, nombre, &diff_view);
            });
        }
        {
            let this = self.clone();
            let dialog = dialog.clone();
            let lista = lista.clone();
            let versiones = versiones.to_vec();
            restaurar.connect_clicked(move |_| {
                let Some(fila) = lista.selected_row() else {
                    return;
                };
                let indice = fila.index().max(0) as usize;
                if let Some((_nombre, contenido)) = versiones.get(indice) {
                    this.0.code_editor.replace_text(contenido);
                    this.toast(&format!(
                        "Versión restaurada como edición ({} caracteres); guardá normalmente",
                        contenido.len()
                    ));
                    dialog.close();
                }
            });
        }
        dialog
    }

    /// Fecha legible desde el nombre de archivo `AAAA-MM-DD_HH-MM-SS-mmm.txt`.
    pub(super) fn nombre_version_legible(nombre: &str) -> String {
        let base = nombre.trim_end_matches(".txt");
        let (fecha, hora) = base.split_once('_').unwrap_or((base, ""));
        let (h, _ms) = hora.split_once('-').unwrap_or((hora, ""));
        format!("{} a las {}", fecha.replace('-', "/"), h.replace('-', ":"))
    }

    /// Carga en la vista de diff el contenido de una versión contra el texto actual del editor.
    pub(super) fn cargar_diff_historial(
        &self,
        path: &RelPath,
        nombre: &str,
        vista: &sourceview5::View,
    ) {
        let Some(vault) = self.0.vault.borrow().clone() else {
            return;
        };
        let text = self.0.code_editor.text();
        let (path, nombre, nombre_clon) = (path.clone(), nombre.to_owned(), nombre.to_owned());
        let vista = vista.clone();
        glib::spawn_future_local(async move {
            let contenido =
                gio::spawn_blocking(move || orbital_vault::leer_version(&vault, &path, &nombre))
                    .await
                    .unwrap_or_else(|_| {
                        Err(orbital_vault::VaultError::InvalidPath(
                            nombre_clon.to_owned(),
                        ))
                    })
                    .unwrap_or_default();
            let buffer = vista.buffer();
            buffer.set_text("");
            let lines = orbital_vault::diff::diff_lines(&contenido, &text);
            use orbital_vault::diff::DiffKind;
            let mut iter = buffer.start_iter();
            for line in lines {
                let texto = &line.text;
                match line.kind {
                    DiffKind::Same => buffer.insert(&mut iter, &format!("   {texto}\n")),
                    DiffKind::Removed => {
                        buffer.insert_with_tags_by_name(&mut iter, "−  ", &["diff-removed"]);
                        buffer.insert_with_tags_by_name(
                            &mut iter,
                            &format!("{texto}\n"),
                            &["diff-removed"],
                        );
                    }
                    DiffKind::Added => {
                        buffer.insert_with_tags_by_name(&mut iter, "+  ", &["diff-added"]);
                        buffer.insert_with_tags_by_name(
                            &mut iter,
                            &format!("{texto}\n"),
                            &["diff-added"],
                        );
                    }
                }
            }
        });
    }
}
