//! Vistas virtualizadas de bases; toda E/S pertenece a la integración de ventana.
use gtk::{gio, glib, prelude::*};
use orbital_vault::{RelPath, bases::*, props::PropValue};
use std::{cell::RefCell, rc::Rc};
mod dialogs;
#[cfg(test)]
mod tests;

type OpenHandler = Box<dyn Fn(RelPath)>;
type EditHandler = Box<dyn Fn(RelPath, u64, String, Option<PropValue>)>;
type ConfigureHandler = Box<dyn Fn(BaseDefinition)>;
type RenameHandler = Box<dyn Fn(String, String)>;
struct Inner {
    root: gtk::Box,
    toolbar: gtk::Box,
    status: gtk::Label,
    stack: gtk::Stack,
    table: gtk::ColumnView,
    store: gio::ListStore,
    definition: RefCell<BaseDefinition>,
    revision: std::cell::Cell<u64>,
    open: RefCell<Vec<OpenHandler>>,
    edit: RefCell<Vec<EditHandler>>,
    configure: RefCell<Vec<ConfigureHandler>>,
    rename: RefCell<Vec<RenameHandler>>,
}
#[derive(Clone)]
pub struct BasePanel(Rc<Inner>);
impl BasePanel {
    pub fn new() -> Self {
        let root = gtk::Box::new(gtk::Orientation::Vertical, 6);
        root.update_property(&[gtk::accessible::Property::Label("Base de notas")]);
        let toolbar = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        let configure = gtk::Button::with_label("Configurar");
        let rename = gtk::Button::with_label("Renombrar propiedad");
        toolbar.append(&configure);
        toolbar.append(&rename);
        root.append(&toolbar);
        let status = gtk::Label::builder().wrap(true).xalign(0.0).build();
        root.append(&status);
        let store = gio::ListStore::new::<glib::BoxedAnyObject>();
        let table = gtk::ColumnView::new(Some(gtk::NoSelection::new(Some(store.clone()))));
        table.update_property(&[gtk::accessible::Property::Label(
            "Tabla de notas de la base",
        )]);
        let stack = gtk::Stack::builder().vexpand(true).build();
        stack.add_named(
            &gtk::ScrolledWindow::builder().child(&table).build(),
            Some("table"),
        );
        root.append(&stack);
        let panel = Self(Rc::new(Inner {
            root,
            toolbar,
            status,
            stack,
            table,
            store,
            definition: RefCell::new(BaseDefinition::default()),
            revision: std::cell::Cell::new(0),
            open: RefCell::new(Vec::new()),
            edit: RefCell::new(Vec::new()),
            configure: RefCell::new(Vec::new()),
            rename: RefCell::new(Vec::new()),
        }));
        let factory = panel.factory(None);
        let cards = gtk::GridView::new(
            Some(gtk::NoSelection::new(Some(panel.0.store.clone()))),
            Some(factory),
        );
        cards.set_min_columns(1);
        cards.set_max_columns(5);
        cards.update_property(&[gtk::accessible::Property::Label(
            "Tarjetas de notas de la base",
        )]);
        panel.0.stack.add_named(
            &gtk::ScrolledWindow::builder().child(&cards).build(),
            Some("cards"),
        );
        let weak = Rc::downgrade(&panel.0);
        configure.connect_clicked(move |_| {
            if let Some(i) = weak.upgrade() {
                Self(i).configure_dialog();
            }
        });
        let weak = Rc::downgrade(&panel.0);
        rename.connect_clicked(move |_| {
            if let Some(i) = weak.upgrade() {
                Self(i).rename_dialog();
            }
        });
        panel
    }
    pub fn widget(&self) -> &gtk::Box {
        &self.0.root
    }
    pub fn set_data(&self, definition: BaseDefinition, snapshot: BaseSnapshot) {
        self.0.revision.set(self.0.revision.get().wrapping_add(1));
        self.0.store.remove_all();
        while let Some(column) = self.0.table.columns().item(0) {
            self.0
                .table
                .remove_column(&column.downcast::<gtk::ColumnViewColumn>().unwrap());
        }
        let title = gtk::ColumnViewColumn::new(Some("Nota"), Some(self.factory(None)));
        title.set_resizable(true);
        self.0.table.append_column(&title);
        for column in &definition.columns {
            let view_column = gtk::ColumnViewColumn::new(
                Some(&column.label),
                Some(self.factory(Some(column.clone()))),
            );
            view_column.set_resizable(true);
            self.0.table.append_column(&view_column);
        }
        self.0.stack.set_visible_child_name(match definition.view {
            BaseView::Table => "table",
            BaseView::Cards => "cards",
        });
        *self.0.definition.borrow_mut() = definition;
        let count = snapshot.rows.len();
        self.0
            .status
            .set_text(&format!("{count} notas · {} avisos", snapshot.errors.len()));
        self.0.status.set_tooltip_text(Some(
            &snapshot
                .errors
                .iter()
                .map(|(p, e)| format!("{}: {e}", p.as_str()))
                .collect::<Vec<_>>()
                .join("\n"),
        ));
        let objects: Vec<_> = snapshot
            .rows
            .into_iter()
            .map(glib::BoxedAnyObject::new)
            .collect();
        self.0.store.splice(0, 0, &objects);
    }
    fn factory(&self, column: Option<BaseColumn>) -> gtk::SignalListItemFactory {
        let factory = gtk::SignalListItemFactory::new();
        let weak = Rc::downgrade(&self.0);
        let edit_column = column.clone();
        factory.connect_setup(move |_, item| {
            let item = item.downcast_ref::<gtk::ListItem>().unwrap();
            let button = gtk::Button::builder()
                .has_frame(false)
                .hexpand(true)
                .build();
            let weak_item = item.downgrade();
            let weak = weak.clone();
            let column = edit_column.clone();
            button.connect_clicked(move |_| {
                let (Some(item), Some(inner)) = (weak_item.upgrade(), weak.upgrade()) else {
                    return;
                };
                let Some(object) = item.item().and_downcast::<glib::BoxedAnyObject>() else {
                    return;
                };
                let row = object.borrow::<BaseRow>();
                if let Some(column) = &column {
                    Self(inner).edit_dialog(&row, column);
                } else {
                    for callback in inner.open.borrow().iter() {
                        callback(row.path.clone());
                    }
                }
            });
            item.set_child(Some(&button));
        });
        let weak = Rc::downgrade(&self.0);
        factory.connect_bind(move |_, item| {
            let item = item.downcast_ref::<gtk::ListItem>().unwrap();
            let object = item.item().and_downcast::<glib::BoxedAnyObject>().unwrap();
            let row = object.borrow::<BaseRow>();
            let button = item.child().and_downcast::<gtk::Button>().unwrap();
            if let Some(column) = &column {
                let value = row.value(&column.property);
                let invalid = value.is_some_and(|v| !validate_value(column.kind, v));
                let text = display_value(value);
                button.set_label(&format!("{}{}", if invalid { "⚠ " } else { "" }, text));
                button.set_sensitive(
                    !row.read_only && row.error.is_none() && !column.property.starts_with("file."),
                );
                button.set_tooltip_text(Some(row.error.as_deref().unwrap_or(if row.read_only {
                    "Nota de solo lectura"
                } else if invalid {
                    "Valor incompatible con el tipo de columna; editar para corregir"
                } else {
                    "Editar propiedad"
                })));
                button.update_property(&[gtk::accessible::Property::Label(&format!(
                    "Editar {} de {}: {text}",
                    column.label, row.title
                ))]);
            } else {
                let mut text = row.title.clone();
                if let Some(inner) = weak.upgrade() {
                    let definition = inner.definition.borrow();
                    if matches!(definition.view, BaseView::Cards) {
                        for col in &definition.columns {
                            text.push_str(&format!(
                                "\n{}: {}",
                                col.label,
                                display_value(row.value(&col.property))
                            ));
                        }
                    }
                }
                button.set_label(&text);
                button.set_tooltip_text(Some(row.path.as_str()));
                button.update_property(&[gtk::accessible::Property::Label(&format!(
                    "Abrir nota {}",
                    row.title
                ))]);
            }
        });
        factory
    }
    pub fn connect_open(&self, callback: impl Fn(RelPath) + 'static) {
        self.0.open.borrow_mut().push(Box::new(callback));
    }
    pub fn connect_edit(
        &self,
        callback: impl Fn(RelPath, u64, String, Option<PropValue>) + 'static,
    ) {
        self.0.edit.borrow_mut().push(Box::new(callback));
    }
    pub fn connect_configure(&self, callback: impl Fn(BaseDefinition) + 'static) {
        self.0.configure.borrow_mut().push(Box::new(callback));
    }
    pub fn connect_rename(&self, callback: impl Fn(String, String) + 'static) {
        self.0.rename.borrow_mut().push(Box::new(callback));
    }
    pub fn set_busy(&self, busy: bool) {
        self.0.toolbar.set_sensitive(!busy);
        self.0.stack.set_sensitive(!busy);
        if busy {
            self.0.status.set_text("Cargando base…");
        }
    }
    pub fn set_error(&self, error: Option<&str>) {
        self.0.status.set_text(error.unwrap_or(""));
    }
}
fn display_value(value: Option<&PropValue>) -> String {
    match value {
        None => "(ausente)".into(),
        Some(PropValue::Null) => "(nulo)".into(),
        Some(
            PropValue::Text(v)
            | PropValue::Number(v)
            | PropValue::Date(v)
            | PropValue::DateTime(v)
            | PropValue::Link(v),
        ) => v.clone(),
        Some(PropValue::Bool(v)) => if *v { "true" } else { "false" }.into(),
        Some(PropValue::List(v)) => v
            .iter()
            .map(|v| display_value(Some(v)))
            .collect::<Vec<_>>()
            .join(", "),
        Some(PropValue::Map(v)) => v
            .iter()
            .map(|(k, v)| format!("{k}: {}", display_value(Some(v))))
            .collect::<Vec<_>>()
            .join(", "),
    }
}

impl Default for BasePanel {
    fn default() -> Self {
        Self::new()
    }
}
