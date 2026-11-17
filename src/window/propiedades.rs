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

    /// `sources` en las propiedades de arriba de la vista formateada: una fila con un botón por
    /// fuente, que la abre.
    fn source_fila(&self, note: &RelPath, e: &PropEntry) -> Option<gtk::Widget> {
        let PropValue::List(items) = &e.value else {
            return None;
        };
        if e.key != "sources" {
            return None;
        }
        let caja = gtk::Box::new(gtk::Orientation::Vertical, 0);
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
            let open = gtk::Button::builder()
                .label(title)
                .tooltip_text(resource)
                .halign(gtk::Align::Start)
                .css_classes(["flat", "orbital-prop-boton"])
                .build();
            open.update_property(&[gtk::accessible::Property::Label(&format!(
                "Abrir la fuente {title}"
            ))]);
            let (this, from, res) = (self.clone(), note.clone(), resource.to_owned());
            open.connect_clicked(move |_| this.open_source(&from, &res));
            caja.append(&open);
        }
        Some(editor::props::fila(&e.key, "prop-enlace", &caja, None))
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
        let custom = move |e: &PropEntry| this.source_fila(&note, e).map(|f| vec![f]);
        let props_widget = list(&h.entries, mode, actions, &custom);
        if path.as_str().ends_with(".md")
            && let Some(col) = props_widget.downcast_ref::<gtk::Box>()
        {
            let texto = self.0.code_editor.text();
            let id_sug = self.id_sugerido_para_nota_abierta();
            let diag = crate::grafo::notas_tres_partes::DiagnosticoTresPartes::analizar(
                &texto,
                Some(&id_sug),
            );
            let retroenlaces: Vec<String> = self
                .0
                .graph
                .borrow()
                .as_ref()
                .and_then(|g| g.lock().ok())
                .map(|g| {
                    let mut v: Vec<String> = g
                        .backlinks_for(path)
                        .iter()
                        .map(|b| {
                            let n = b.from.file_name();
                            n.rsplit_once('.').map_or(n, |(s, _)| s).to_owned()
                        })
                        .collect();
                    v.sort();
                    v.dedup();
                    v
                })
                .unwrap_or_default();
            let weak_ir = Rc::downgrade(&self.0);
            let weak_est = Rc::downgrade(&self.0);
            let weak_opc = Rc::downgrade(&self.0);
            let weak_lnk = Rc::downgrade(&self.0);
            let origen = path.clone();
            let acc_tres = Rc::new(crate::grafo::notas_tres_partes::AccionesTresPartes {
                ir_a_linea: Box::new(move |linea| {
                    if let Some(i) = weak_ir.upgrade() {
                        i.code_editor.reveal_line(linea);
                    }
                }),
                estructurar: Box::new(move || {
                    if let Some(i) = weak_est.upgrade() {
                        Window(i).estructurar_nota_tres_partes();
                    }
                }),
                insertar_opcional: Box::new(move |parte| {
                    if let Some(i) = weak_opc.upgrade() {
                        Window(i).insertar_parte_opcional_nota(parte);
                    }
                }),
                abrir_enlace: Box::new(move |url| {
                    if let Some(i) = weak_lnk.upgrade() {
                        Window(i).open_link(Some(&origen), url);
                    }
                }),
            });
            let barra = crate::grafo::notas_tres_partes::construir_barra_tres_partes(
                &diag,
                &retroenlaces,
                h.read_only,
                acc_tres,
            );
            col.append(&barra);
        }
        Some(props_widget)
    }

    fn id_sugerido_para_nota_abierta(&self) -> String {
        let note = self.0.current_note.borrow();
        let tipo = note
            .as_ref()
            .and_then(|n| n.prop("type"))
            .and_then(|v| v.as_text())
            .unwrap_or("atomo");
        let id_existente = note
            .as_ref()
            .and_then(|n| n.prop("id"))
            .and_then(|v| v.as_text())
            .map(str::to_owned);
        id_existente
            .unwrap_or_else(|| format!("{}-001", crate::grafo::ramas_libres::prefijo_de_tipo(tipo)))
    }

    /// Estructura la nota abierta en sus 3 partes obligatorias (`Propiedades YAML`, `Cuerpo` en
    /// bloques `<!-- block ... -->` y `Conexiones` al pie `<!-- connections ID -->`, 041).
    pub fn estructurar_nota_tres_partes(&self) {
        let i = &self.0;
        let Some(entry) = i.current_entry.borrow().clone() else {
            self.toast("Abrí una nota antes de estructurarla en 3 partes");
            return;
        };
        if Format::from_name(entry.path.file_name()) != Some(Format::Md) {
            self.toast("La estructura en 3 partes está disponible para notas Markdown (.md)");
            return;
        }
        if i.code_editor.current_path().as_ref() != Some(&entry.path) {
            return;
        }
        let texto = i.code_editor.text();
        let id_sug = self.id_sugerido_para_nota_abierta();
        let (nuevo, linea) =
            crate::grafo::notas_tres_partes::estructurar_en_tres_partes(&texto, Some(&id_sug));
        if nuevo != texto {
            i.code_editor.replace_text(&nuevo);
            i.code_editor.render_now();
        }
        i.code_editor.reveal_line(linea);
        self.toast("Nota estructurada en 3 partes (Propiedades · Cuerpo · Conexiones)");
    }

    /// Inserta una de las 2 partes opcionales (`Síntesis` o `Fuentes`) en la nota abierta (041).
    pub(super) fn insertar_parte_opcional_nota(
        &self,
        parte: crate::grafo::notas_tres_partes::ParteOpcional,
    ) {
        let i = &self.0;
        let Some(entry) = i.current_entry.borrow().clone() else {
            return;
        };
        if Format::from_name(entry.path.file_name()) != Some(Format::Md)
            || i.code_editor.current_path().as_ref() != Some(&entry.path)
        {
            return;
        }
        let texto = i.code_editor.text();
        let id_sug = self.id_sugerido_para_nota_abierta();
        let (nuevo, linea) =
            crate::grafo::notas_tres_partes::insertar_parte_opcional(&texto, parte, Some(&id_sug));
        if nuevo != texto {
            i.code_editor.replace_text(&nuevo);
            i.code_editor.render_now();
        }
        i.code_editor.reveal_line(linea);
        let nombre = match parte {
            crate::grafo::notas_tres_partes::ParteOpcional::Sintesis => "Síntesis (summary)",
            crate::grafo::notas_tres_partes::ParteOpcional::Fuentes => "Fuentes (sources)",
        };
        self.toast(&format!("Se agregó la sección opcional de {nombre}"));
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
