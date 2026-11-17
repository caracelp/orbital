use super::*;
use adw::prelude::*;
const TYPES: [&str; 8] = [
    "Automático",
    "Texto",
    "Número",
    "Booleano",
    "Fecha",
    "Fecha y hora",
    "Enlace",
    "Lista",
];
fn kind(index: u32) -> PropertyType {
    match index {
        1 => PropertyType::Text,
        2 => PropertyType::Number,
        3 => PropertyType::Bool,
        4 => PropertyType::Date,
        5 => PropertyType::DateTime,
        6 => PropertyType::Link,
        7 => PropertyType::List,
        _ => PropertyType::Auto,
    }
}
fn kind_index(value: PropertyType) -> u32 {
    match value {
        PropertyType::Auto => 0,
        PropertyType::Text => 1,
        PropertyType::Number => 2,
        PropertyType::Bool => 3,
        PropertyType::Date => 4,
        PropertyType::DateTime => 5,
        PropertyType::Link => 6,
        PropertyType::List => 7,
    }
}
fn entry(label: &str, value: &str) -> gtk::Entry {
    let entry = gtk::Entry::builder()
        .placeholder_text(label)
        .text(value)
        .hexpand(true)
        .build();
    entry.update_property(&[gtk::accessible::Property::Label(label)]);
    entry
}
fn dropdown(label: &str, values: &[&str], selected: u32) -> gtk::DropDown {
    let drop = gtk::DropDown::from_strings(values);
    drop.set_selected(selected);
    drop.update_property(&[gtk::accessible::Property::Label(label)]);
    drop
}
impl BasePanel {
    fn dialog(&self, title: &str) -> (adw::Window, gtk::Box) {
        let window = adw::Window::builder()
            .title(title)
            .modal(true)
            .default_width(640)
            .default_height(600)
            .build();
        if let Some(parent) = self.0.root.root().and_downcast::<gtk::Window>() {
            window.set_transient_for(Some(&parent));
        }
        let content = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(12)
            .margin_top(18)
            .margin_bottom(18)
            .margin_start(18)
            .margin_end(18)
            .build();
        let outer = gtk::Box::new(gtk::Orientation::Vertical, 6);
        let scroll = gtk::ScrolledWindow::builder()
            .vexpand(true)
            .child(&content)
            .build();
        outer.append(&scroll);
        let cancel = gtk::Button::with_label("Cancelar");
        let weak = window.downgrade();
        cancel.connect_clicked(move |_| {
            if let Some(window) = weak.upgrade() {
                window.close();
            }
        });
        outer.append(&cancel);
        window.set_content(Some(&outer));
        let keys = gtk::EventControllerKey::new();
        let weak = window.downgrade();
        keys.connect_key_pressed(move |_, key, _, _| {
            if key == gtk::gdk::Key::Escape {
                if let Some(window) = weak.upgrade() {
                    window.close();
                }
                return glib::Propagation::Stop;
            }
            glib::Propagation::Proceed
        });
        window.add_controller(keys);
        (window, content)
    }
    pub(super) fn edit_dialog(&self, row: &BaseRow, column: &BaseColumn) {
        if row.read_only || row.error.is_some() || column.property.starts_with("file.") {
            return;
        }
        let (window, content) = self.dialog(&format!("Editar {}", column.label));
        content.append(&gtk::Label::new(Some(&format!(
            "{} · {}",
            row.title, column.property
        ))));
        let input = gtk::TextView::builder()
            .wrap_mode(gtk::WrapMode::WordChar)
            .build();
        input.update_property(&[gtk::accessible::Property::Label("Valor de la propiedad")]);
        let value = row
            .value(&column.property)
            .filter(|v| !matches!(v, PropValue::Null))
            .map(|v| edit_value(column.kind, v))
            .unwrap_or_default();
        input.buffer().set_text(&value);
        content.append(
            &gtk::ScrolledWindow::builder()
                .min_content_height(100)
                .child(&input)
                .build(),
        );
        let error = gtk::Label::builder().wrap(true).xalign(0.0).build();
        error.add_css_class("error");
        content.append(&error);
        let buttons = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        for (label, mode) in [
            ("Guardar", 0),
            ("Establecer nulo", 1),
            ("Quitar propiedad", 2),
        ] {
            let button = gtk::Button::with_label(label);
            buttons.append(&button);
            let weak = Rc::downgrade(&self.0);
            let window = window.clone();
            let input = input.clone();
            let error = error.clone();
            let path = row.path.clone();
            let hash = row.hash;
            let key = column.property.clone();
            let kind = column.kind;
            button.connect_clicked(move |_| {
                let value = match mode {
                    1 => Some(PropValue::Null),
                    2 => None,
                    _ => match parse_value(
                        kind,
                        input
                            .buffer()
                            .text(
                                &input.buffer().start_iter(),
                                &input.buffer().end_iter(),
                                true,
                            )
                            .as_str(),
                    ) {
                        Ok(v) => Some(v),
                        Err(e) => {
                            error.set_text(&e);
                            return;
                        }
                    },
                };
                if let Some(inner) = weak.upgrade() {
                    for callback in inner.edit.borrow().iter() {
                        callback(path.clone(), hash, key.clone(), value.clone());
                    }
                }
                window.close();
            });
        }
        content.append(&buttons);
        window.present();
        input.grab_focus();
    }
    pub(super) fn rename_dialog(&self) {
        let (window, content) = self.dialog("Renombrar o fusionar propiedad");
        content.append(&gtk::Label::builder().label("Se mostrará una vista previa antes de modificar archivos. Los valores diferentes se conservan.").wrap(true).build());
        let old = entry("Propiedad de origen", "");
        let new = entry("Propiedad de destino", "");
        content.append(&old);
        content.append(&new);
        let error = gtk::Label::new(None);
        content.append(&error);
        let button = gtk::Button::with_label("Previsualizar renombre");
        content.append(&button);
        let weak = Rc::downgrade(&self.0);
        let close = window.clone();
        button.connect_clicked(move |_| {
            let old = old.text().trim().to_owned();
            let new = new.text().trim().to_owned();
            if old.is_empty() || new.is_empty() || old == new {
                error.set_text("Ingresá dos propiedades distintas y no vacías.");
                return;
            }
            if let Some(inner) = weak.upgrade() {
                for callback in inner.rename.borrow().iter() {
                    callback(old.clone(), new.clone());
                }
            }
            close.close();
        });
        window.present();
    }
    pub(super) fn configure_dialog(&self) {
        let definition = self.0.definition.borrow().clone();
        let revision = self.0.revision.get();
        let (window, content) = self.dialog("Configurar base");
        let view = dropdown(
            "Vista",
            &["Tabla", "Tarjetas"],
            u32::from(matches!(definition.view, BaseView::Cards)),
        );
        content.append(&view);
        content.append(&gtk::Label::new(Some(
            "Columnas (en orden): propiedad, etiqueta y tipo",
        )));
        let columns_box = gtk::Box::new(gtk::Orientation::Vertical, 6);
        let columns = Rc::new(RefCell::new(Vec::new()));
        let add_column = {
            let columns_box = columns_box.clone();
            let columns = columns.clone();
            move |column: BaseColumn| {
                let line = gtk::Box::new(gtk::Orientation::Vertical, 4);
                let key = entry("Propiedad de columna", &column.property);
                let label = entry("Etiqueta de columna", &column.label);
                let ty = dropdown("Tipo de columna", &TYPES, kind_index(column.kind));
                line.append(&key);
                line.append(&label);
                line.append(&ty);
                let remove = gtk::Button::with_label("Quitar columna");
                line.append(&remove);
                let weak = line.downgrade();
                remove.connect_clicked(move |_| {
                    if let Some(line) = weak.upgrade() {
                        line.set_visible(false);
                    }
                });
                columns_box.append(&line);
                columns.borrow_mut().push((line, key, label, ty));
            }
        };
        for column in &definition.columns {
            add_column(column.clone());
        }
        let columns_scroll = gtk::ScrolledWindow::builder()
            .min_content_height(120)
            .child(&columns_box)
            .build();
        content.append(&columns_scroll);
        let add = gtk::Button::with_label("Agregar columna");
        content.append(&add);
        add.connect_clicked(move |_| {
            add_column(BaseColumn {
                property: String::new(),
                label: String::new(),
                kind: PropertyType::Auto,
            })
        });
        content.append(&gtk::Label::new(Some(
            "Filtros: todas las condiciones deben cumplirse (Y)",
        )));
        let filters_box = gtk::Box::new(gtk::Orientation::Vertical, 6);
        let filters = Rc::new(RefCell::new(Vec::new()));
        let add_filter = {
            let filters_box = filters_box.clone();
            let filters = filters.clone();
            move |filter: BaseFilter| {
                let (index, key, value) = match filter {
                    BaseFilter::Folder(v) => (0, v, String::new()),
                    BaseFilter::Tag(v) => (1, v, String::new()),
                    BaseFilter::Property { key, value: None } => (2, key, String::new()),
                    BaseFilter::Property {
                        key,
                        value: Some(v),
                    } => (3, key, orbital_vault::props::emit::value(&v)),
                };
                let line = gtk::Box::new(gtk::Orientation::Vertical, 4);
                let ty = dropdown(
                    "Condición",
                    &[
                        "Carpeta",
                        "Etiqueta",
                        "Propiedad presente",
                        "Propiedad igual",
                    ],
                    index,
                );
                let key = entry("Carpeta, etiqueta o propiedad", &key);
                let value = entry("Valor del filtro", &value);
                let value_type = dropdown("Tipo del valor del filtro", &TYPES, 0);
                line.append(&ty);
                line.append(&key);
                line.append(&value);
                line.append(&value_type);
                let remove = gtk::Button::with_label("Quitar filtro");
                line.append(&remove);
                let weak = line.downgrade();
                remove.connect_clicked(move |_| {
                    if let Some(line) = weak.upgrade() {
                        line.set_visible(false);
                    }
                });
                filters_box.append(&line);
                filters
                    .borrow_mut()
                    .push((line, ty, key, value, value_type));
            }
        };
        for filter in &definition.filters {
            add_filter(filter.clone());
        }
        content.append(
            &gtk::ScrolledWindow::builder()
                .min_content_height(100)
                .child(&filters_box)
                .build(),
        );
        let add = gtk::Button::with_label("Agregar condición");
        content.append(&add);
        add.connect_clicked(move |_| add_filter(BaseFilter::Tag(String::new())));
        let sort = entry(
            "Ordenar por propiedad (vacío: ruta)",
            definition.sort.as_ref().map_or("", |s| s.property.as_str()),
        );
        content.append(&sort);
        let descending = gtk::CheckButton::with_label("Orden descendente");
        descending.set_active(definition.sort.as_ref().is_some_and(|s| s.descending));
        content.append(&descending);
        let error = gtk::Label::builder().wrap(true).build();
        error.add_css_class("error");
        content.append(&error);
        let save = gtk::Button::with_label("Guardar configuración");
        content.append(&save);
        let weak = Rc::downgrade(&self.0);
        let close = window.clone();
        save.connect_clicked(move |_| {
            if weak.upgrade().is_none_or(|i| i.revision.get() != revision) {
                error.set_text(
                    "La base cambió mientras la configurabas. Cerrá y volvé a configurar.",
                );
                return;
            }
            let mut result = definition.clone();
            result.view = if view.selected() == 1 {
                BaseView::Cards
            } else {
                BaseView::Table
            };
            result.columns.clear();
            result.filters.clear();
            for (line, key, label, ty) in columns.borrow().iter().filter(|r| r.0.is_visible()) {
                let _ = line;
                let key = key.text().trim().to_owned();
                if key.is_empty() || result.columns.iter().any(|c| c.property == key) {
                    error.set_text("Las columnas necesitan propiedades únicas y no vacías.");
                    return;
                }
                result.columns.push(BaseColumn {
                    property: key,
                    label: label.text().to_string(),
                    kind: kind(ty.selected()),
                });
            }
            if result.columns.is_empty() {
                error.set_text("Agregá al menos una columna.");
                return;
            }
            for (_, ty, key, value, value_type) in
                filters.borrow().iter().filter(|r| r.0.is_visible())
            {
                let key = key.text().trim().to_owned();
                if key.is_empty() {
                    error.set_text("Completá cada condición o quitá la fila.");
                    return;
                }
                result.filters.push(match ty.selected() {
                    0 => BaseFilter::Folder(key),
                    1 => BaseFilter::Tag(key),
                    2 => BaseFilter::Property { key, value: None },
                    _ => {
                        let value =
                            match parse_value(kind(value_type.selected()), value.text().as_str()) {
                                Ok(v) => v,
                                Err(e) => {
                                    error.set_text(&e);
                                    return;
                                }
                            };
                        BaseFilter::Property {
                            key,
                            value: Some(value),
                        }
                    }
                });
            }
            let property = sort.text().trim().to_owned();
            result.sort = if property.is_empty() {
                None
            } else {
                Some(BaseSort {
                    property,
                    descending: descending.is_active(),
                })
            };
            if let Err(message) = parse(&emit(&result)) {
                error.set_text(&message);
                return;
            }
            if let Some(inner) = weak.upgrade() {
                for callback in inner.configure.borrow().iter() {
                    callback(result.clone());
                }
            }
            close.close();
        });
        window.present();
    }
}

pub(super) fn edit_value(kind: PropertyType, value: &PropValue) -> String {
    if kind == PropertyType::Text {
        display_value(Some(value))
    } else {
        orbital_vault::props::emit::value(value)
    }
}
