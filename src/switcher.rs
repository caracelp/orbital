//! Selector rápido de notas (US2 de la 004, FR-012).
//!
//! Permite saltar a cualquier nota por su nombre, título o alias con Ctrl+P,
//! mostrando las notas recientes cuando el campo está vacío.

use std::cell::RefCell;
use std::rc::Rc;

use adw::prelude::*;
use gtk::gdk::Key;
use gtk::glib::subclass::prelude::ObjectSubclassIsExt;
use gtk::{gio, glib};
use orbital_vault::RelPath;

use crate::editor::completion::{CompletionCandidate, filter_candidates};

mod imp {
    use super::*;
    use glib::subclass::prelude::*;

    #[derive(Default)]
    pub struct SwitcherItem {
        pub candidate: RefCell<Option<CompletionCandidate>>,
        pub is_recent: RefCell<bool>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for SwitcherItem {
        const NAME: &'static str = "OrbitalSwitcherItem";
        type Type = super::SwitcherItem;
    }

    impl ObjectImpl for SwitcherItem {}
}

glib::wrapper! {
    pub struct SwitcherItem(ObjectSubclass<imp::SwitcherItem>);
}

impl SwitcherItem {
    pub fn new(candidate: CompletionCandidate, is_recent: bool) -> Self {
        let obj: Self = glib::Object::builder().build();
        *obj.imp().candidate.borrow_mut() = Some(candidate);
        *obj.imp().is_recent.borrow_mut() = is_recent;
        obj
    }

    pub fn candidate(&self) -> Option<CompletionCandidate> {
        self.imp().candidate.borrow().clone()
    }

    pub fn is_recent(&self) -> bool {
        *self.imp().is_recent.borrow()
    }
}

pub struct SwitcherDialog;

impl SwitcherDialog {
    /// Presenta el selector rápido de notas como un diálogo modal.
    pub fn present<F: Fn(RelPath) + 'static>(
        parent: &impl IsA<gtk::Widget>,
        candidates: Vec<CompletionCandidate>,
        recent_paths: Vec<String>,
        on_chosen: F,
    ) {
        let dialog = adw::Dialog::builder()
            .title("Saltar a nota")
            .content_width(580)
            .content_height(440)
            .build();

        let root = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(0)
            .build();

        let header = adw::HeaderBar::builder()
            .show_title(true)
            .title_widget(&adw::WindowTitle::new("Saltar a nota", ""))
            .build();
        root.append(&header);

        let search_entry = gtk::SearchEntry::builder()
            .placeholder_text("Buscar por nombre, título o alias…")
            .margin_start(16)
            .margin_end(16)
            .margin_top(8)
            .margin_bottom(8)
            .build();
        search_entry.update_property(&[gtk::accessible::Property::Label("Buscar nota")]);
        root.append(&search_entry);

        let list_store = gio::ListStore::new::<SwitcherItem>();
        let selection = gtk::SingleSelection::builder()
            .model(&list_store)
            .autoselect(true)
            .can_unselect(false)
            .build();

        let factory = gtk::SignalListItemFactory::new();
        factory.connect_setup(|_, item| {
            let item = item.downcast_ref::<gtk::ListItem>().expect("ListItem");
            let container = gtk::Box::new(gtk::Orientation::Horizontal, 10);
            container.set_margin_start(16);
            container.set_margin_end(16);
            container.set_margin_top(6);
            container.set_margin_bottom(6);

            let icon = gtk::Image::from_icon_name("text-html-symbolic");
            container.append(&icon);

            let text_box = gtk::Box::new(gtk::Orientation::Vertical, 2);
            text_box.set_hexpand(true);

            let name_label = gtk::Label::builder()
                .xalign(0.0)
                .ellipsize(gtk::pango::EllipsizeMode::End)
                .css_classes(["heading"])
                .build();
            text_box.append(&name_label);

            let subtitle_label = gtk::Label::builder()
                .xalign(0.0)
                .ellipsize(gtk::pango::EllipsizeMode::End)
                .css_classes(["dim-label", "caption"])
                .build();
            text_box.append(&subtitle_label);

            container.append(&text_box);

            let badge = gtk::Label::builder()
                .css_classes(["dim-label", "caption", "tag"])
                .visible(false)
                .build();
            container.append(&badge);

            item.set_child(Some(&container));
        });

        factory.connect_bind(|_, item| {
            let item = item.downcast_ref::<gtk::ListItem>().expect("ListItem");
            let Some(switcher_item) = item.item().and_downcast::<SwitcherItem>() else {
                return;
            };
            let Some(candidate) = switcher_item.candidate() else {
                return;
            };
            let is_recent = switcher_item.is_recent();

            let container = item.child().and_downcast::<gtk::Box>().expect("Box");
            let mut children = Vec::new();
            let mut next = container.first_child();
            while let Some(c) = next {
                next = c.next_sibling();
                children.push(c);
            }
            if children.len() < 3 {
                return;
            }

            let text_box = children[1].downcast_ref::<gtk::Box>().expect("Box");
            let name_label = text_box
                .first_child()
                .and_downcast::<gtk::Label>()
                .expect("Label");
            let subtitle_label = name_label
                .next_sibling()
                .and_downcast::<gtk::Label>()
                .expect("Label");
            let badge = children[2].downcast_ref::<gtk::Label>().expect("Label");

            name_label.set_text(&candidate.stem);

            // Subtítulo: carpeta y título/alias si difiere
            let full_str = candidate.path.as_str();
            let folder = match full_str.rsplit_once('/') {
                Some((dir, _)) => dir.strip_prefix("wiki/").unwrap_or(dir),
                None => "",
            };

            let mut subtitle_parts = Vec::new();
            if !folder.is_empty() {
                subtitle_parts.push(folder.to_string());
            }
            if let Some(title) = &candidate.title
                && title != &candidate.stem
            {
                subtitle_parts.push(title.clone());
            }
            subtitle_label.set_text(&subtitle_parts.join(" · "));

            if is_recent {
                badge.set_text("Reciente");
                badge.set_visible(true);
            } else {
                badge.set_visible(false);
            }

            container.update_property(&[gtk::accessible::Property::Label(&format!(
                "Nota {}, {}",
                candidate.stem,
                if folder.is_empty() { "raíz" } else { folder }
            ))]);
        });

        let list_view = gtk::ListView::builder()
            .model(&selection)
            .factory(&factory)
            .single_click_activate(true)
            .build();
        list_view.update_property(&[gtk::accessible::Property::Label("Resultados del selector")]);

        let scroll = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .child(&list_view)
            .vexpand(true)
            .build();
        root.append(&scroll);

        dialog.set_child(Some(&root));

        // Preparar lista de candidatos recientes
        let all_candidates = Rc::new(candidates);
        let recent_candidates = {
            let mut list = Vec::new();
            for rpath in &recent_paths {
                if let Some(cand) = all_candidates.iter().find(|c| c.path.as_str() == rpath) {
                    list.push(cand.clone());
                }
            }
            Rc::new(list)
        };

        // Función para actualizar el modelo según la consulta
        let update_results = {
            let list_store = list_store.clone();
            let all = all_candidates.clone();
            let rec = recent_candidates.clone();
            let selection = selection.clone();
            move |query: &str| {
                list_store.remove_all();
                let q = query.trim();
                if q.is_empty() {
                    // Mostrar recientes si hay; de lo contrario las primeras 30
                    if !rec.is_empty() {
                        for c in rec.iter().take(20) {
                            list_store.append(&SwitcherItem::new(c.clone(), true));
                        }
                    } else {
                        for c in all.iter().take(30) {
                            list_store.append(&SwitcherItem::new(c.clone(), false));
                        }
                    }
                } else {
                    let filtered = filter_candidates(&all, q, 40);
                    for c in filtered {
                        list_store.append(&SwitcherItem::new(c, false));
                    }
                }
                if list_store.n_items() > 0 {
                    selection.set_selected(0);
                }
            }
        };

        // Población inicial (recientes)
        update_results("");

        // Conectar búsqueda en tiempo real
        let update_for_search = update_results.clone();
        search_entry.connect_search_changed(move |entry| {
            update_for_search(&entry.text());
        });

        // Acción compartida para elegir el elemento seleccionado
        let on_chosen = Rc::new(on_chosen);
        let choose = {
            let selection = selection.clone();
            let list_store = list_store.clone();
            let dialog = dialog.clone();
            let on_chosen = on_chosen.clone();
            Rc::new(move || {
                let pos = selection.selected();
                if let Some(item) = list_store.item(pos).and_downcast::<SwitcherItem>()
                    && let Some(cand) = item.candidate()
                {
                    dialog.close();
                    on_chosen(cand.path);
                }
            })
        };

        // Exponer acción accesible en el diálogo
        let action_group = gio::SimpleActionGroup::new();
        let choose_act = gio::SimpleAction::new("choose", None);
        {
            let choose = choose.clone();
            choose_act.connect_activate(move |_, _| {
                choose();
            });
        }
        action_group.add_action(&choose_act);
        dialog.insert_action_group("switcher", Some(&action_group));

        // Activación de fila en la lista
        {
            let choose = choose.clone();
            list_view.connect_activate(move |_, _| {
                choose();
            });
        }

        // Activación desde el campo de búsqueda (Intro)
        {
            let choose = choose.clone();
            search_entry.connect_activate(move |_| {
                choose();
            });
        }

        // Navegación por teclado en el diálogo (Flechas, Intro, Escape)
        {
            let key_controller = gtk::EventControllerKey::new();
            key_controller.set_propagation_phase(gtk::PropagationPhase::Capture);
            let selection = selection.clone();
            let list_store = list_store.clone();
            let dialog = dialog.clone();
            let list_view = list_view.clone();
            let choose = choose.clone();

            key_controller.connect_key_pressed(move |_, keyval, _, _| match keyval {
                Key::Down => {
                    let total = list_store.n_items();
                    if total > 0 {
                        let curr = selection.selected();
                        let next = if curr + 1 < total { curr + 1 } else { curr };
                        selection.set_selected(next);
                        list_view.scroll_to(next, gtk::ListScrollFlags::FOCUS, None);
                    }
                    glib::Propagation::Stop
                }
                Key::Up => {
                    let total = list_store.n_items();
                    if total > 0 {
                        let curr = selection.selected();
                        let prev = curr.saturating_sub(1);
                        selection.set_selected(prev);
                        list_view.scroll_to(prev, gtk::ListScrollFlags::FOCUS, None);
                    }
                    glib::Propagation::Stop
                }
                Key::Return | Key::KP_Enter => {
                    choose();
                    glib::Propagation::Stop
                }
                Key::Escape => {
                    dialog.close();
                    glib::Propagation::Stop
                }
                _ => glib::Propagation::Proceed,
            });
            root.add_controller(key_controller);
        }

        dialog.present(Some(parent));
        search_entry.grab_focus();
    }
}
