//! Búsquedas guardadas: lista virtual y escrituras con snapshot; sin E/S en GTK.
use adw::prelude::*;
use gtk::{gio, glib};
use orbital_vault::{SavedSearch, SavedSearchSnapshot, Vault};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    sync::Arc,
};

#[derive(Clone, Copy)]
enum Action {
    Save,
    Rename(u32),
    Delete(u32),
}

struct Inner {
    root: gtk::Box,
    entry: gtk::SearchEntry,
    save: gtk::Button,
    warning: adw::Banner,
    store: gio::ListStore,
    scroll: gtk::ScrolledWindow,
    vault: RefCell<Option<Arc<Vault>>>,
    snapshot: RefCell<Option<SavedSearchSnapshot>>,
    generation: Cell<u64>,
    session: Cell<u64>,
    writing: Cell<bool>,
}

#[derive(Clone)]
pub struct SavedPanel(Rc<Inner>);

impl SavedPanel {
    pub fn new(entry: &gtk::SearchEntry) -> Self {
        let root = gtk::Box::new(gtk::Orientation::Vertical, 4);
        root.update_property(&[gtk::accessible::Property::Label("Búsquedas guardadas")]);
        let heading = gtk::Box::builder()
            .spacing(6)
            .margin_start(12)
            .margin_end(12)
            .build();
        heading.append(
            &gtk::Label::builder()
                .label("Búsquedas guardadas")
                .xalign(0.0)
                .hexpand(true)
                .css_classes(["heading"])
                .build(),
        );
        let save = gtk::Button::builder()
            .icon_name("document-save-symbolic")
            .tooltip_text("Guardar búsqueda…")
            .sensitive(false)
            .css_classes(["flat"])
            .build();
        save.update_property(&[gtk::accessible::Property::Label("Guardar búsqueda")]);
        heading.append(&save);
        root.append(&heading);
        let warning = adw::Banner::builder()
            .title("")
            .button_label("Recargar")
            .revealed(false)
            .build();
        warning
            .upcast_ref::<gtk::Widget>()
            .update_property(&[gtk::accessible::Property::Label(
                "Aviso de búsquedas guardadas",
            )]);
        root.append(&warning);
        let store = gio::ListStore::new::<glib::BoxedAnyObject>();
        let scroll = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .propagate_natural_height(true)
            .max_content_height(150)
            .visible(false)
            .build();
        root.append(&scroll);
        let panel = Self(Rc::new(Inner {
            root,
            entry: entry.clone(),
            save,
            warning,
            store,
            scroll,
            vault: RefCell::new(None),
            snapshot: RefCell::new(None),
            generation: Cell::new(0),
            session: Cell::new(0),
            writing: Cell::new(false),
        }));
        let factory = gtk::SignalListItemFactory::new();
        let weak = Rc::downgrade(&panel.0);
        factory.connect_setup(move |_, item| {
            let item = item.downcast_ref::<gtk::ListItem>().unwrap();
            let row = gtk::Box::new(gtk::Orientation::Horizontal, 2);
            let execute = gtk::Button::builder()
                .has_frame(false)
                .hexpand(true)
                .css_classes(["flat"])
                .build();
            let weak_item = item.downgrade();
            let weak_panel = weak.clone();
            execute.connect_clicked(move |_| {
                let (Some(item), Some(i)) = (weak_item.upgrade(), weak_panel.upgrade()) else {
                    return;
                };
                let Some(object) = item.item().and_downcast::<glib::BoxedAnyObject>() else {
                    return;
                };
                i.entry.set_text(&object.borrow::<SavedSearch>().query);
                i.entry.grab_focus();
            });
            row.append(&execute);
            for (icon, action) in [
                ("document-edit-symbolic", true),
                ("user-trash-symbolic", false),
            ] {
                let button = gtk::Button::builder()
                    .icon_name(icon)
                    .css_classes(["flat"])
                    .build();
                let weak_item = item.downgrade();
                let weak_panel = weak.clone();
                button.connect_clicked(move |_| {
                    let (Some(item), Some(i)) = (weak_item.upgrade(), weak_panel.upgrade()) else {
                        return;
                    };
                    SavedPanel(i).ask(if action {
                        Action::Rename(item.position())
                    } else {
                        Action::Delete(item.position())
                    });
                });
                row.append(&button);
            }
            item.set_child(Some(&row));
        });
        factory.connect_bind(|_, item| {
            let item = item.downcast_ref::<gtk::ListItem>().unwrap();
            let object = item.item().and_downcast::<glib::BoxedAnyObject>().unwrap();
            let search = object.borrow::<SavedSearch>();
            let row = item.child().unwrap();
            let execute = row.first_child().and_downcast::<gtk::Button>().unwrap();
            execute.set_label(&search.name);
            execute.set_tooltip_text(Some(&search.query));
            execute.update_property(&[gtk::accessible::Property::Label(&format!(
                "Ejecutar búsqueda {}",
                search.name
            ))]);
            let rename = execute.next_sibling().unwrap();
            rename.update_property(&[gtk::accessible::Property::Label(&format!(
                "Renombrar búsqueda {}",
                search.name
            ))]);
            rename.set_tooltip_text(Some("Renombrar búsqueda…"));
            let delete = rename.next_sibling().unwrap();
            delete.update_property(&[gtk::accessible::Property::Label(&format!(
                "Borrar búsqueda {}",
                search.name
            ))]);
            delete.set_tooltip_text(Some("Borrar búsqueda…"));
        });
        let list = gtk::ListView::builder()
            .model(&gtk::NoSelection::new(Some(panel.0.store.clone())))
            .factory(&factory)
            .build();
        list.update_property(&[gtk::accessible::Property::Label(
            "Lista de búsquedas guardadas",
        )]);
        panel.0.scroll.set_child(Some(&list));
        let weak = Rc::downgrade(&panel.0);
        panel.0.save.connect_clicked(move |_| {
            if let Some(i) = weak.upgrade() {
                SavedPanel(i).ask(Action::Save);
            }
        });
        let weak = Rc::downgrade(&panel.0);
        panel.0.entry.connect_changed(move |_| {
            if let Some(i) = weak.upgrade() {
                SavedPanel(i).update_sensitive();
            }
        });
        let weak = Rc::downgrade(&panel.0);
        panel.0.warning.connect_button_clicked(move |_| {
            if let Some(i) = weak.upgrade() {
                SavedPanel(i).reload();
            }
        });
        panel
    }

    pub fn widget(&self) -> &gtk::Box {
        &self.0.root
    }

    fn update_sensitive(&self) {
        self.0.save.set_sensitive(
            !self.0.writing.get()
                && !self.0.entry.text().trim().is_empty()
                && self
                    .0
                    .snapshot
                    .borrow()
                    .as_ref()
                    .is_some_and(|snapshot| snapshot.warning.is_none()),
        );
    }

    pub fn set_vault(&self, vault: Option<Arc<Vault>>, session: u64) {
        let same = self.0.session.get() == session
            && self
                .0
                .vault
                .borrow()
                .as_ref()
                .zip(vault.as_ref())
                .is_some_and(|(old, new)| old.root() == new.root());
        *self.0.vault.borrow_mut() = vault;
        self.0.session.set(session);
        if !same {
            // Invalida el callback del worker anterior aun si sigue escribiendo su raíz capturada.
            self.0.writing.set(false);
            self.reload();
        }
    }

    fn reload(&self) {
        if self.0.writing.get() {
            return;
        }
        let generation = self.0.generation.get() + 1;
        self.0.generation.set(generation);
        self.0.writing.set(false);
        self.0.root.set_sensitive(true);
        self.0.store.remove_all();
        self.0.scroll.set_visible(false);
        *self.0.snapshot.borrow_mut() = None;
        self.0.warning.set_revealed(false);
        self.update_sensitive();
        let Some(vault) = self.0.vault.borrow().clone() else {
            return;
        };
        let this = self.clone();
        glib::spawn_future_local(async move {
            let loaded = gio::spawn_blocking(move || vault.saved_searches_snapshot()).await;
            if this.0.generation.get() != generation {
                return;
            }
            match loaded {
                Ok(snapshot) => this.show_snapshot(snapshot),
                Err(_) => this.warn("No se pudieron cargar las búsquedas guardadas"),
            }
        });
    }

    fn warn(&self, message: &str) {
        self.0.warning.set_title(message);
        self.0.warning.set_revealed(true);
    }

    fn show_snapshot(&self, snapshot: SavedSearchSnapshot) {
        self.0.store.remove_all();
        for search in &snapshot.searches {
            self.0
                .store
                .append(&glib::BoxedAnyObject::new(search.clone()));
        }
        self.0.scroll.set_visible(!snapshot.searches.is_empty());
        self.0.warning.set_revealed(false);
        if let Some(message) = &snapshot.warning {
            self.warn(message);
        }
        *self.0.snapshot.borrow_mut() = Some(snapshot);
        self.update_sensitive();
    }

    fn ask(&self, action: Action) {
        if self.0.writing.get() {
            return;
        }
        let (Some(vault), Some(baseline)) = (
            self.0.vault.borrow().clone(),
            self.0.snapshot.borrow().clone(),
        ) else {
            return;
        };
        if baseline.warning.is_some() {
            return;
        }
        let generation = self.0.generation.get();
        let query = self.0.entry.text().to_string();
        let mut searches = baseline.searches.clone();
        let selected = match action {
            Action::Save => None,
            Action::Rename(index) | Action::Delete(index) => searches.get(index as usize).cloned(),
        };
        if !matches!(action, Action::Save) && selected.is_none() {
            return;
        }
        let this = self.clone();
        glib::spawn_future_local(async move {
            let (title, response, label) = match action {
                Action::Save => ("Guardar búsqueda", "save", "Guardar"),
                Action::Rename(_) => ("Renombrar búsqueda", "save", "Renombrar"),
                Action::Delete(_) => ("Borrar búsqueda", "delete", "Borrar"),
            };
            let field = gtk::Entry::builder()
                .text(selected.as_ref().map_or("", |s| &s.name))
                .activates_default(true)
                .build();
            field.update_property(&[gtk::accessible::Property::Label(
                "Nombre de búsqueda guardada",
            )]);
            let dialog = adw::AlertDialog::new(
                Some(title),
                selected
                    .as_ref()
                    .filter(|_| matches!(action, Action::Delete(_)))
                    .map(|s| format!("¿Borrar «{}»?", s.name))
                    .as_deref(),
            );
            dialog.add_response("cancel", "Cancelar");
            dialog.add_response(response, label);
            dialog.set_close_response("cancel");
            dialog.set_response_appearance(
                response,
                if matches!(action, Action::Delete(_)) {
                    adw::ResponseAppearance::Destructive
                } else {
                    adw::ResponseAppearance::Suggested
                },
            );
            if !matches!(action, Action::Delete(_)) {
                dialog.set_extra_child(Some(&field));
                dialog.set_default_response(Some(response));
                dialog.set_response_enabled(response, !field.text().trim().is_empty());
                let weak = dialog.downgrade();
                field.connect_changed(move |field| {
                    if let Some(dialog) = weak.upgrade() {
                        dialog.set_response_enabled("save", !field.text().trim().is_empty());
                    }
                });
            }
            if dialog
                .choose_future(this.0.root.root().and_downcast::<gtk::Window>().as_ref())
                .await
                != response
                || this.0.generation.get() != generation
            {
                return;
            }
            match action {
                Action::Save => searches.push(SavedSearch {
                    name: field.text().trim().to_owned(),
                    query,
                }),
                Action::Rename(index) => {
                    searches[index as usize].name = field.text().trim().to_owned()
                }
                Action::Delete(index) => {
                    searches.remove(index as usize);
                }
            }
            this.persist(vault, generation, baseline, searches).await;
        });
    }

    async fn persist(
        &self,
        vault: Arc<Vault>,
        generation: u64,
        baseline: SavedSearchSnapshot,
        searches: Vec<SavedSearch>,
    ) {
        if self.0.generation.get() != generation || self.0.writing.get() {
            return;
        }
        self.0.writing.set(true);
        self.0.root.set_sensitive(false);
        let result = gio::spawn_blocking(move || {
            let ctx = crate::write_ctx::now(Some(vault.config()));
            vault.save_searches_checked(&searches, &baseline, &ctx)
        })
        .await;
        if self.0.generation.get() != generation {
            return;
        }
        self.0.writing.set(false);
        self.0.root.set_sensitive(true);
        match result {
            Ok(Ok(snapshot)) => self.show_snapshot(snapshot),
            Ok(Err(error)) => self.warn(&format!(
                "No se guardaron las búsquedas: {error}. Recargá para revisar el archivo actual."
            )),
            Err(_) => self.warn("No se pudieron guardar las búsquedas"),
        }
        self.update_sensitive();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "requiere GTK; tools/verificar-004-us7.sh"]
    fn us7_saved_widgets_persistence_and_conflict() {
        gtk::init().expect("se necesita acceso GTK");
        glib::MainContext::default().block_on(async {
            let root =
                std::env::temp_dir().join(format!("orbital-us7-{}", glib::uuid_string_random()));
            std::fs::create_dir(&root).unwrap();
            let vault = Arc::new(orbital_vault::open(&root).unwrap());
            let entry = gtk::SearchEntry::new();
            entry.set_text("tag:#física");
            let panel = SavedPanel::new(&entry);
            panel.set_vault(Some(vault.clone()), 1);
            for _ in 0..100 {
                if panel.0.snapshot.borrow().is_some() {
                    break;
                }
                glib::timeout_future(std::time::Duration::from_millis(10)).await;
            }
            let baseline = panel.0.snapshot.borrow().clone().unwrap();
            let generation = panel.0.generation.get();
            panel
                .persist(
                    vault.clone(),
                    generation,
                    baseline,
                    vec![SavedSearch {
                        name: "Física".into(),
                        query: entry.text().to_string(),
                    }],
                )
                .await;
            assert_eq!(panel.0.store.n_items(), 1);
            assert_eq!(vault.saved_searches()[0].name, "Física");
            assert_eq!(entry.text(), "tag:#física");
            let baseline = panel.0.snapshot.borrow().clone().unwrap();
            let external = "- name: Afuera\n  query: otro\n";
            std::fs::write(root.join(".orbital/busquedas.yaml"), external).unwrap();
            panel.persist(vault, generation, baseline, vec![]).await;
            assert!(panel.0.warning.is_revealed());
            assert_eq!(
                panel.0.store.n_items(),
                1,
                "el fallo no publica la lista optimista"
            );
            assert_eq!(entry.text(), "tag:#física");
            assert_eq!(
                std::fs::read_to_string(root.join(".orbital/busquedas.yaml")).unwrap(),
                external
            );
            std::fs::remove_dir_all(root).unwrap();
        });
    }
}
