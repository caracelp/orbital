//! Propiedades de la nota: filas editables, encabezado del bloque, vista del código fuente y «Completar» el tipo.

use super::*;

impl Window {
    /// Llena una lista con las propiedades de una nota. Las citas de `sources` tienen un botón
    /// que abre la fuente (FR-006, historia 6).
    pub(super) fn fill_props(&self, list: &gtk::ListBox, note: &RelPath, entries: &[PropEntry]) {
        for e in entries {
            if let Some(rows) = self.source_rows(note, e) {
                rows.iter().for_each(|r| list.append(r));
                continue;
            }
            let row = adw::ActionRow::builder()
                .title(&e.key)
                .subtitle(glib::markup_escape_text(&display(&e.value)).as_str())
                .subtitle_selectable(true)
                .build();
            row.add_css_class("property");
            list.append(&row);
        }
        list.set_visible(!entries.is_empty());
    }

    /// Filas de `sources` (OKF): una por fuente, con un botón para abrirla.
    pub(super) fn source_rows(&self, note: &RelPath, e: &PropEntry) -> Option<Vec<gtk::Widget>> {
        let PropValue::List(items) = &e.value else {
            return None;
        };
        if e.key != "sources" {
            return None;
        }
        let mut rows = Vec::new();
        for item in items {
            let PropValue::Map(fields) = item else {
                continue;
            };
            let get = |k: &str| {
                fields
                    .iter()
                    .find(|(key, _)| key == k)
                    .and_then(|(_, v)| v.as_text())
            };
            let Some(resource) = get("resource") else {
                continue;
            };
            let title = get("title").or(get("id")).unwrap_or("fuente");
            let row = adw::ActionRow::builder()
                .title(format!("sources · {}", glib::markup_escape_text(title)))
                .subtitle(glib::markup_escape_text(resource).as_str())
                .subtitle_selectable(true)
                .build();
            let open = gtk::Button::builder()
                .icon_name("document-open-symbolic")
                .tooltip_text("Abrir la fuente")
                .valign(gtk::Align::Center)
                .css_classes(["flat"])
                .build();
            open.update_property(&[gtk::accessible::Property::Label(&format!(
                "Abrir la fuente {title}"
            ))]);
            let (this, from, res) = (self.clone(), note.clone(), resource.to_owned());
            open.connect_clicked(move |_| this.open_source(&from, &res));
            row.add_suffix(&open);
            row.set_activatable_widget(Some(&open));
            rows.push(row.upcast());
        }
        Some(rows)
    }

    /// Lista editable de propiedades arriba de la vista formateada (US3 de la 002).
    pub(super) fn props_header(&self, path: &RelPath, h: &editor::Header) -> Option<gtk::Widget> {
        use editor::props::{Actions, Mode, list};
        let mode = match (&h.error, h.read_only) {
            (Some(e), _) => Mode::Invalid(e.clone()),
            (None, true) => Mode::ReadOnly,
            (None, false) => Mode::Editable,
        };
        if h.entries.is_empty() && matches!(mode, Mode::ReadOnly) {
            return None;
        }
        let weak = Rc::downgrade(&self.0);
        let set = {
            let weak = weak.clone();
            move |key: &str, value: PropValue, rebuild: bool| {
                let Some(i) = weak.upgrade() else { return };
                let w = Window(i);
                if let Err(e) = w.0.code_editor.edit_prop(key, Some(&value), rebuild) {
                    w.toast(&format!("No se pudo cambiar «{key}»: {e}"));
                }
            }
        };
        let remove = {
            let weak = weak.clone();
            let old: Vec<(String, PropValue)> = h
                .entries
                .iter()
                .map(|e| (e.key.clone(), e.value.clone()))
                .collect();
            move |key: &str| {
                let Some(i) = weak.upgrade() else { return };
                let w = Window(i);
                let editor = w.0.code_editor.clone();
                if let Err(e) = editor.edit_prop(key, None, true) {
                    w.toast(&format!("No se pudo quitar «{key}»: {e}"));
                    return;
                }
                let edits = editor.edits();
                let value = old.iter().find(|(k, _)| k == key).map(|(_, v)| v.clone());
                let k = key.to_owned();
                w.toast_undo(&format!("Se quitó «{key}»"), move || {
                    // Si no se escribió nada después, se deshace en el sitio; si no, se agrega
                    // de nuevo al final.
                    if !editor.undo_if_last(edits)
                        && let Some(v) = &value
                    {
                        let _ = editor.edit_prop(&k, Some(v), true);
                    }
                });
            }
        };
        let go_code = {
            let weak = weak.clone();
            let line = h.error.as_ref().map_or(0, |e| e.line as i32);
            move || {
                if let Some(i) = weak.upgrade() {
                    // El bloque empieza después de la línea `---` (y los errores cuentan desde 1).
                    i.code_editor.show_code_line(line);
                }
            }
        };
        let actions = Rc::new(Actions {
            set: Box::new(set),
            remove: Box::new(remove),
            go_code: Box::new(go_code),
        });
        let this = self.clone();
        let note = path.clone();
        let custom = move |e: &PropEntry| this.source_rows(&note, e);
        Some(list(&h.entries, mode, actions, &custom))
    }

    /// «Completar»: agrega `type` a la nota abierta, solo porque el usuario lo pidió.
    pub(super) fn complete_type(&self) {
        let Some(vault) = self.0.vault.borrow().clone() else {
            return;
        };
        let Some(note) = self.0.current_note.borrow().clone() else {
            return;
        };
        let this = self.clone();
        glib::spawn_future_local(async move {
            let value = PropValue::Text(
                match note.format {
                    Format::Md => "atomo",
                    Format::Html => "sintesis",
                }
                .to_owned(),
            );
            let path = note.path.clone();
            let v = vault.clone();
            let result = gio::spawn_blocking(move || {
                let ctx = write_ctx::now(Some(v.config()));
                v.set_prop(&note, "type", &value, &ctx)?;
                v.read_note(&note.path)
            })
            .await;
            match result {
                Ok(Ok(fresh)) => {
                    this.clear_note_view();
                    this.show_note(fresh);
                    this.toast(&format!("Se agregó «type» a {path}"));
                }
                Ok(Err(e)) => this.toast(&e.to_string()),
                Err(_) => this.toast("No se pudo guardar la nota"),
            }
        });
    }

    /// Abre una fuente citada con la aplicación del sistema, si existe en la bóveda o es una URL.
    pub(super) fn open_source(&self, from: &RelPath, resource: &str) {
        let Some(vault) = self.0.vault.borrow().clone() else {
            return;
        };
        if resource.starts_with("http://") || resource.starts_with("https://") {
            gtk::UriLauncher::new(resource).launch(
                Some(&self.0.window),
                None::<&gio::Cancellable>,
                |_| {},
            );
            return;
        }
        match vault.resolve_link(from, resource) {
            orbital_vault::Resolution::Found(p) => self.launch_file(&p.to_path(vault.root())),
            _ => self.toast(&format!("No encontré la fuente «{resource}»")),
        }
    }
}
