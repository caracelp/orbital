//! Árbol virtual de etiquetas. Los datos llegan ya calculados desde el índice en otro hilo.

use std::{cell::RefCell, rc::Rc};

use gtk::{gio, glib, prelude::*};
use orbital_vault::tags::{TagNode, TagTree};

#[derive(Default)]
pub struct TagSnapshot {
    pub tree: TagTree,
    pub names: Vec<crate::editor::completion::TagCandidate>,
}

impl TagSnapshot {
    pub fn from_index(index: &orbital_vault::SearchIndex) -> Self {
        fn collect(
            nodes: &[TagNode],
            prefix: &str,
            names: &mut Vec<crate::editor::completion::TagCandidate>,
        ) {
            for node in nodes {
                let name = if prefix.is_empty() {
                    node.name.clone()
                } else {
                    format!("{prefix}/{}", node.name)
                };
                names.push(crate::editor::completion::TagCandidate::new(name.clone()));
                collect(&node.children, &name, names);
            }
        }
        let tree = index.tags();
        let mut names = Vec::new();
        collect(&tree.roots, "", &mut names);
        Self { tree, names }
    }
}

type SelectedFn = Box<dyn Fn(&str)>;

struct Inner {
    root: gtk::Box,
    status: gtk::Label,
    list: gtk::ListView,
    on_selected: RefCell<Option<SelectedFn>>,
    on_rename: Rc<RefCell<Option<SelectedFn>>>,
}

#[derive(Clone)]
pub struct TagPanel(Rc<Inner>);

fn nodes_model(nodes: Vec<TagNode>) -> gio::ListStore {
    let store = gio::ListStore::new::<glib::BoxedAnyObject>();
    for node in nodes {
        store.append(&glib::BoxedAnyObject::new(node));
    }
    store
}

impl TagPanel {
    pub fn new() -> Self {
        let root = gtk::Box::new(gtk::Orientation::Vertical, 6);
        root.update_property(&[gtk::accessible::Property::Label("Panel de etiquetas")]);
        let status = gtk::Label::builder()
            .label("Indexando…")
            .wrap(true)
            .margin_top(8)
            .margin_bottom(8)
            .build();
        root.append(&status);
        let on_rename: Rc<RefCell<Option<SelectedFn>>> = Rc::new(RefCell::new(None));
        let rename_callback = on_rename.clone();
        let factory = gtk::SignalListItemFactory::new();
        factory.connect_setup(move |_, item| {
            let item = item.downcast_ref::<gtk::ListItem>().unwrap();
            let label = gtk::Label::builder().xalign(0.0).hexpand(true).build();
            let expander = gtk::TreeExpander::builder().child(&label).build();
            let row = gtk::Box::new(gtk::Orientation::Horizontal, 4);
            expander.set_hexpand(true);
            row.append(&expander);
            let rename = gtk::Button::builder()
                .icon_name("document-edit-symbolic")
                .tooltip_text("Renombrar etiqueta…")
                .css_classes(["flat"])
                .build();
            let weak_item = item.downgrade();
            let callback = rename_callback.clone();
            rename.connect_clicked(move |_| {
                let Some(item) = weak_item.upgrade() else {
                    return;
                };
                let Some(row) = item.item().and_downcast::<gtk::TreeListRow>() else {
                    return;
                };
                let Some(object) = row.item().and_downcast::<glib::BoxedAnyObject>() else {
                    return;
                };
                if let Some(f) = callback.borrow().as_ref() {
                    f(&object.borrow::<TagNode>().path);
                }
            });
            row.append(&rename);
            item.set_child(Some(&row));
        });
        factory.connect_bind(|_, item| {
            let item = item.downcast_ref::<gtk::ListItem>().unwrap();
            let row = item.item().and_downcast::<gtk::TreeListRow>().unwrap();
            let object = row.item().and_downcast::<glib::BoxedAnyObject>().unwrap();
            let node = object.borrow::<TagNode>();
            let row_box = item.child().and_downcast::<gtk::Box>().unwrap();
            let expander = row_box
                .first_child()
                .and_downcast::<gtk::TreeExpander>()
                .unwrap();
            row_box
                .last_child()
                .unwrap()
                .update_property(&[gtk::accessible::Property::Label(&format!(
                    "Renombrar #{}",
                    node.path
                ))]);
            expander.set_list_row(Some(&row));
            let label = expander.child().and_downcast::<gtk::Label>().unwrap();
            label.set_text(&format!("#{} ({})", node.name, node.notes));
            expander.update_property(&[gtk::accessible::Property::Label(&format!(
                "Etiqueta #{}: {} notas",
                node.path, node.notes
            ))]);
        });
        factory.connect_unbind(|_, item| {
            if let Some(expander) = item
                .downcast_ref::<gtk::ListItem>()
                .and_then(|item| item.child())
                .and_then(|row| row.first_child())
                .and_downcast::<gtk::TreeExpander>()
            {
                expander.set_list_row(None::<&gtk::TreeListRow>);
            }
        });
        let list = gtk::ListView::builder()
            .factory(&factory)
            .single_click_activate(true)
            .vexpand(true)
            .build();
        list.update_property(&[gtk::accessible::Property::Label("Árbol de etiquetas")]);
        root.append(
            &gtk::ScrolledWindow::builder()
                .child(&list)
                .hscrollbar_policy(gtk::PolicyType::Never)
                .vexpand(true)
                .build(),
        );
        let this = Self(Rc::new(Inner {
            root,
            status,
            list,
            on_selected: RefCell::new(None),
            on_rename,
        }));
        let weak = Rc::downgrade(&this.0);
        this.0.list.connect_activate(move |list, pos| {
            let Some(i) = weak.upgrade() else { return };
            let Some(row) = list
                .model()
                .and_then(|m| m.item(pos))
                .and_downcast::<gtk::TreeListRow>()
            else {
                return;
            };
            let Some(object) = row.item().and_downcast::<glib::BoxedAnyObject>() else {
                return;
            };
            if let Some(f) = i.on_selected.borrow().as_ref() {
                f(&object.borrow::<TagNode>().path);
            }
        });
        this
    }

    pub fn widget(&self) -> &gtk::Box {
        &self.0.root
    }

    pub fn connect_selected(&self, f: impl Fn(&str) + 'static) {
        *self.0.on_selected.borrow_mut() = Some(Box::new(f));
    }

    pub fn connect_rename(&self, f: impl Fn(&str) + 'static) {
        *self.0.on_rename.borrow_mut() = Some(Box::new(f));
    }

    pub fn set_tree(&self, tree: Option<TagTree>) {
        let mut expanded = std::collections::HashSet::new();
        let mut selected = None;
        if let Some(selection) = self.0.list.model().and_downcast::<gtk::SingleSelection>() {
            if let Some(row) = selection.selected_item().and_downcast::<gtk::TreeListRow>()
                && let Some(object) = row.item().and_downcast::<glib::BoxedAnyObject>()
            {
                selected = Some(object.borrow::<TagNode>().path.clone());
            }
            if let Some(model) = selection.model().and_downcast::<gtk::TreeListModel>() {
                for position in 0..model.n_items() {
                    if let Some(row) = model.row(position)
                        && row.is_expanded()
                        && let Some(object) = row.item().and_downcast::<glib::BoxedAnyObject>()
                    {
                        expanded.insert(object.borrow::<TagNode>().path.clone());
                    }
                }
            }
        }
        let Some(tree) = tree else {
            self.0.status.set_text("Indexando…");
            self.0.status.set_visible(true);
            self.0.list.set_model(None::<&gtk::SelectionModel>);
            return;
        };
        self.0.status.set_text("No hay etiquetas en esta bóveda");
        self.0.status.set_visible(tree.roots.is_empty());
        let model = gtk::TreeListModel::new(nodes_model(tree.roots), false, false, |item| {
            let object = item.downcast_ref::<glib::BoxedAnyObject>()?;
            let node = object.borrow::<TagNode>();
            (!node.children.is_empty()).then(|| nodes_model(node.children.clone()).upcast())
        });
        let selection = gtk::SingleSelection::builder()
            .model(&model)
            .autoselect(false)
            .build();
        let mut position = 0;
        while position < model.n_items() {
            if let Some(row) = model.row(position)
                && let Some(object) = row.item().and_downcast::<glib::BoxedAnyObject>()
            {
                let node = object.borrow::<TagNode>();
                if expanded.contains(&node.path) {
                    row.set_expanded(true);
                }
                if selected.as_deref() == Some(node.path.as_str()) {
                    selection.set_selected(position);
                }
            }
            position += 1;
        }
        self.0.list.set_model(Some(&selection));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "requiere acceso GTK; tools/verificar-004-us5.sh"]
    fn us5_tag_widgets_panel() {
        gtk::init().expect("el proceso necesita conectarse a una sesión GTK");
        let panel = TagPanel::new();
        let tree = TagTree {
            roots: vec![TagNode {
                name: "Física".into(),
                path: "fisica".into(),
                notes: 2,
                children: vec![TagNode {
                    name: "Termodinámica".into(),
                    path: "fisica/termodinamica".into(),
                    notes: 1,
                    children: vec![],
                }],
            }],
        };
        panel.set_tree(Some(tree.clone()));
        let selection = panel
            .0
            .list
            .model()
            .and_downcast::<gtk::SingleSelection>()
            .unwrap();
        let model = selection
            .model()
            .and_downcast::<gtk::TreeListModel>()
            .unwrap();
        model.row(0).unwrap().set_expanded(true);
        assert_eq!(model.n_items(), 2);
        selection.set_selected(1);
        let chosen = Rc::new(RefCell::new(String::new()));
        let got = chosen.clone();
        panel.connect_selected(move |path| {
            *got.borrow_mut() = orbital_vault::query::tag_query(path)
        });
        panel.0.list.emit_by_name::<()>("activate", &[&1u32]);
        assert_eq!(*chosen.borrow(), "tag:#fisica/termodinamica");
        panel.set_tree(Some(tree));
        let selection = panel
            .0
            .list
            .model()
            .and_downcast::<gtk::SingleSelection>()
            .unwrap();
        assert_eq!(selection.selected(), 1);
        assert_eq!(selection.n_items(), 2);
        panel.set_tree(None);
        assert!(panel.0.list.model().is_none());
    }
}
