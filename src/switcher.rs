//! Selector rápido de notas (US2) y paleta de comandos (US8) de la 004, FR-012.
//!
//! El mismo diálogo sirve dos fuentes: con Ctrl+P salta a una nota por su
//! nombre, título o alias (con recientes si el campo está vacío) y con
//! Ctrl+Mayús+P ejecuta un comando por su nombre, con su atajo y con los que
//! no aplican en ese momento desactivados.

use std::cell::RefCell;
use std::rc::Rc;

use adw::prelude::*;
use gtk::gdk::Key;
use gtk::glib::subclass::prelude::ObjectSubclassIsExt;
use gtk::{gio, glib};
use orbital_vault::RelPath;

use crate::editor::completion::{CompletionCandidate, filter_candidates};

/// Un comando de la paleta (US8): nombre visible, acción `win.*` que corre,
/// atajo si tiene y si aplica en este momento. Lo arma la ventana a partir de
/// sus acciones. La clave doblada se precalcula para no repetir `fold` en
/// cada tecla (igual que `CompletionCandidate`).
#[derive(Clone)]
pub struct CommandEntry {
    pub name: String,
    pub action: String,
    pub accel: Option<String>,
    pub enabled: bool,
    /// Nombre doblado (sin mayúsculas ni tildes), para filtrar.
    pub key: String,
}

impl CommandEntry {
    pub fn new(name: String, action: String, accel: Option<String>, enabled: bool) -> Self {
        let key = orbital_vault::fold(&name);
        Self {
            name,
            action,
            accel,
            enabled,
            key,
        }
    }
}

#[derive(Clone)]
pub(crate) enum SwitcherRow {
    Note {
        candidate: CompletionCandidate,
        is_recent: bool,
    },
    Command {
        command: CommandEntry,
    },
}

/// Lo que el diálogo le devuelve a la ventana al elegir una fila.
pub enum Pick {
    Note(RelPath),
    Command(CommandEntry),
}

enum Source {
    Notes {
        candidates: Rc<Vec<CompletionCandidate>>,
        recent_paths: Vec<String>,
    },
    Commands(Rc<Vec<CommandEntry>>),
}

mod imp {
    use super::*;
    use glib::subclass::prelude::*;

    #[derive(Default)]
    pub struct SwitcherItem {
        pub(crate) row: RefCell<Option<SwitcherRow>>,
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
    fn new(row: SwitcherRow) -> Self {
        let obj: Self = glib::Object::builder().build();
        *obj.imp().row.borrow_mut() = Some(row);
        obj
    }

    fn row(&self) -> Option<SwitcherRow> {
        self.imp().row.borrow().clone()
    }
}

/// Filtra comandos por su nombre (insensible a mayúsculas y tildes): primero
/// los que empiezan con la consulta y por último los que la contienen. Igual
/// que `filter_candidates`, recorre por índices y copia solo los `limit`.
pub fn filter_commands(commands: &[CommandEntry], query: &str, limit: usize) -> Vec<usize> {
    let q = orbital_vault::fold(query.trim());
    if q.is_empty() {
        return (0..commands.len().min(limit)).collect();
    }
    let mut starts = Vec::new();
    let mut contains = Vec::new();
    for (n, c) in commands.iter().enumerate() {
        if c.key.starts_with(&q) {
            if starts.len() < limit {
                starts.push(n);
            }
        } else if contains.len() < limit && c.key.contains(&q) {
            contains.push(n);
        }
    }
    starts.into_iter().chain(contains).take(limit).collect()
}

pub struct SwitcherDialog;

/// Resolvedor opcional de íconos personalizados para el selector rápido (062).
pub type IconResolver = Rc<dyn Fn(&str) -> Option<gtk::gdk::Paintable>>;

impl SwitcherDialog {
    /// Presenta el selector rápido de notas como un diálogo modal.
    pub fn present<F: Fn(RelPath) + 'static>(
        parent: &impl IsA<gtk::Widget>,
        candidates: Vec<CompletionCandidate>,
        recent_paths: Vec<String>,
        on_chosen: F,
    ) {
        Self::present_with_icons(parent, candidates, recent_paths, None, on_chosen);
    }

    /// Como [`Self::present`], mostrando íconos personalizados en cada nota (062).
    pub fn present_with_icons<F: Fn(RelPath) + 'static>(
        parent: &impl IsA<gtk::Widget>,
        candidates: Vec<CompletionCandidate>,
        recent_paths: Vec<String>,
        icon_resolver: Option<IconResolver>,
        on_chosen: F,
    ) {
        let on_pick = move |pick: Pick| {
            if let Pick::Note(path) = pick {
                on_chosen(path);
            }
        };
        Self::present_impl(
            parent,
            "Saltar a nota",
            "Buscar por nombre, título o alias…",
            "Buscar nota",
            Source::Notes {
                candidates: Rc::new(candidates),
                recent_paths,
            },
            icon_resolver,
            Rc::new(on_pick),
        );
    }

    /// Presenta la paleta de comandos como el mismo diálogo modal (US8).
    pub fn present_commands<F: Fn(CommandEntry) + 'static>(
        parent: &impl IsA<gtk::Widget>,
        commands: Vec<CommandEntry>,
        on_run: F,
    ) {
        let on_pick = move |pick: Pick| {
            if let Pick::Command(command) = pick {
                on_run(command);
            }
        };
        Self::present_impl(
            parent,
            "Paleta de comandos",
            "Escribir parte de un comando…",
            "Buscar comando",
            Source::Commands(Rc::new(commands)),
            None,
            Rc::new(on_pick),
        );
    }

    /// Arma y presenta el diálogo compartido; devuelve el diálogo y el campo
    /// de búsqueda para las pruebas en proceso.
    fn present_impl(
        parent: &impl IsA<gtk::Widget>,
        title: &str,
        placeholder: &str,
        entry_label: &str,
        source: Source,
        icon_resolver: Option<IconResolver>,
        on_pick: Rc<dyn Fn(Pick)>,
    ) -> (adw::Dialog, gtk::SearchEntry) {
        let dialog = adw::Dialog::builder()
            .title(title)
            .content_width(580)
            .content_height(440)
            .build();

        let root = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(0)
            .build();

        let header = adw::HeaderBar::builder()
            .show_title(true)
            .title_widget(&adw::WindowTitle::new(title, ""))
            .build();
        root.append(&header);

        let search_entry = gtk::SearchEntry::builder()
            .placeholder_text(placeholder)
            .margin_start(16)
            .margin_end(16)
            .margin_top(8)
            .margin_bottom(8)
            .build();
        search_entry.update_property(&[gtk::accessible::Property::Label(entry_label)]);
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

        let resolver_bind = icon_resolver.clone();
        factory.connect_bind(move |_, item| {
            let item = item.downcast_ref::<gtk::ListItem>().expect("ListItem");
            let Some(switcher_item) = item.item().and_downcast::<SwitcherItem>() else {
                return;
            };
            let Some(row) = switcher_item.row() else {
                return;
            };

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

            let icon_img = children[0].downcast_ref::<gtk::Image>().expect("Image");
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

            match row {
                SwitcherRow::Note {
                    candidate,
                    is_recent,
                } => {
                    if let Some(p) = resolver_bind
                        .as_ref()
                        .and_then(|f| f(candidate.path.as_str()))
                    {
                        icon_img.set_paintable(Some(&p));
                        icon_img.set_pixel_size(16);
                    } else {
                        icon_img.set_icon_name(Some("text-x-generic-symbolic"));
                    }
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

                    container.set_sensitive(true);
                    container.set_opacity(1.0);
                    container.update_property(&[gtk::accessible::Property::Label(&format!(
                        "Nota {}, {}",
                        candidate.stem,
                        if folder.is_empty() { "raíz" } else { folder }
                    ))]);
                }
                SwitcherRow::Command { command } => {
                    name_label.set_text(&command.name);
                    subtitle_label.set_text(command.accel.as_deref().unwrap_or(""));

                    if command.enabled {
                        badge.set_visible(false);
                        container.set_sensitive(true);
                        container.set_opacity(1.0);
                    } else {
                        badge.set_text("No disponible");
                        badge.set_visible(true);
                        container.set_sensitive(false);
                        container.set_opacity(0.5);
                    }

                    let estado = if command.enabled {
                        String::new()
                    } else {
                        ", no disponible".to_owned()
                    };
                    let atajo = command
                        .accel
                        .as_deref()
                        .map(|a| format!(", atajo {a}"))
                        .unwrap_or_default();
                    container.update_property(&[gtk::accessible::Property::Label(&format!(
                        "Comando {}{}{}",
                        command.name, atajo, estado
                    ))]);
                }
            }
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

        // Datos para poblar y filtrar según la fuente
        let (recent_candidates, all_candidates, all_commands) = match &source {
            Source::Notes {
                candidates,
                recent_paths,
            } => {
                let recent = recent_paths
                    .iter()
                    .filter_map(|rpath| {
                        candidates
                            .iter()
                            .find(|c| c.path.as_str() == rpath)
                            .cloned()
                    })
                    .collect::<Vec<_>>();
                (Some(Rc::new(recent)), Some(candidates.clone()), None)
            }
            Source::Commands(commands) => (None, None, Some(commands.clone())),
        };

        // Función para actualizar el modelo según la consulta
        let update_results = {
            let list_store = list_store.clone();
            let all = all_candidates;
            let rec = recent_candidates;
            let commands = all_commands;
            let selection = selection.clone();
            move |query: &str| {
                list_store.remove_all();
                let q = query.trim();
                if let Some(all) = all.as_ref() {
                    if q.is_empty() {
                        // Mostrar recientes si hay; de lo contrario las primeras 30
                        if let Some(rec) = rec.as_ref()
                            && !rec.is_empty()
                        {
                            for c in rec.iter().take(20) {
                                list_store.append(&SwitcherItem::new(SwitcherRow::Note {
                                    candidate: c.clone(),
                                    is_recent: true,
                                }));
                            }
                        } else {
                            for c in all.iter().take(30) {
                                list_store.append(&SwitcherItem::new(SwitcherRow::Note {
                                    candidate: c.clone(),
                                    is_recent: false,
                                }));
                            }
                        }
                    } else {
                        for c in filter_candidates(all, q, 40) {
                            list_store.append(&SwitcherItem::new(SwitcherRow::Note {
                                candidate: c,
                                is_recent: false,
                            }));
                        }
                    }
                } else if let Some(commands) = commands.as_ref() {
                    let indices = filter_commands(commands, q, 40);
                    for n in indices {
                        list_store.append(&SwitcherItem::new(SwitcherRow::Command {
                            command: commands[n].clone(),
                        }));
                    }
                }
                if list_store.n_items() > 0 {
                    selection.set_selected(0);
                }
            }
        };

        // Población inicial (recientes o todos los comandos)
        update_results("");

        // Conectar búsqueda en tiempo real
        let update_for_search = update_results.clone();
        search_entry.connect_search_changed(move |entry| {
            update_for_search(&entry.text());
        });

        // Acción compartida para elegir el elemento seleccionado
        let choose = {
            let selection = selection.clone();
            let list_store = list_store.clone();
            let dialog = dialog.clone();
            let on_pick = on_pick.clone();
            Rc::new(move || {
                let pos = selection.selected();
                let Some(item) = list_store.item(pos).and_downcast::<SwitcherItem>() else {
                    return;
                };
                let Some(row) = item.row() else {
                    return;
                };
                match row {
                    SwitcherRow::Note { candidate, .. } => {
                        dialog.close();
                        on_pick(Pick::Note(candidate.path));
                    }
                    SwitcherRow::Command { command } => {
                        // Un comando que no aplica ahora se muestra pero no corre.
                        if command.enabled {
                            dialog.close();
                            on_pick(Pick::Command(command));
                        }
                    }
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
        (dialog, search_entry)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn comandos(n: usize) -> Vec<CommandEntry> {
        (0..n)
            .map(|i| {
                CommandEntry::new(
                    format!("Comando de prueba número {i} sobre física y química"),
                    format!("accion-{i}"),
                    Some("Ctrl+X".into()),
                    i % 2 == 0,
                )
            })
            .collect()
    }

    #[test]
    fn filter_commands_matches_starts_and_contains() {
        let cmds = vec![
            CommandEntry::new("Archivar la nota".into(), "archive".into(), None, true),
            CommandEntry::new(
                "Buscar en toda la bóveda".into(),
                "search".into(),
                None,
                true,
            ),
            CommandEntry::new("Vista Código".into(), "view-code".into(), None, true),
        ];
        // Sin tildes ni mayúsculas
        assert_eq!(filter_commands(&cmds, "ARCHIVAR", 40), [0]);
        // Prefijo gana sobre contiene
        assert_eq!(filter_commands(&cmds, "b", 40), [1]);
        assert_eq!(filter_commands(&cmds, "vista", 40), [2]);
        assert!(filter_commands(&cmds, "ausente", 40).is_empty());
        // Vacío: todas, hasta el límite
        assert_eq!(filter_commands(&cmds, "", 2).len(), 2);
    }

    /// SC-004: filtrar 5000 entradas en menos de 16 ms por tecla. Medir con
    /// `cargo test --release --bin orbital command_filter_latency -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn command_filter_latency() {
        let cmds = comandos(5000);
        let mut worst = std::time::Duration::ZERO;
        let mut times = Vec::new();
        for q in ["c", "co", "com", "número", "química", "zzz", "de prueba"] {
            let t = std::time::Instant::now();
            let r = filter_commands(&cmds, q, 40);
            let e = t.elapsed();
            assert!(r.len() <= 40);
            worst = worst.max(e);
            times.push((q, e));
        }
        println!("filtrado de 5000 comandos: {times:?}");
        println!("peor: {worst:?}");
        assert!(worst.as_millis() < 16, "peor caso {worst:?} supera 16 ms");
    }

    /// Lo que AT-SPI no puede accionar en GTK 4.24: correr un comando desde la
    /// paleta (activación de fila e Intro). Cubre elegir un comando habilitado
    /// y que uno desactivado no se ejecute.
    #[test]
    #[ignore]
    fn us8_palette_widgets_choose_and_disabled() {
        gtk::init().expect("se necesita acceso GTK");
        glib::MainContext::default().block_on(async {
            let parent = gtk::Window::new();
            parent.present();
            let corridos: std::rc::Rc<RefCell<Vec<String>>> = Default::default();
            let captura = corridos.clone();
            let (dialog, entry) = SwitcherDialog::present_impl(
                &parent,
                "Paleta de comandos",
                "Escribir parte de un comando…",
                "Buscar comando",
                Source::Commands(Rc::new(vec![
                    CommandEntry::new(
                        "Vista Código".into(),
                        "view-code".into(),
                        Some("Ctrl+1".into()),
                        true,
                    ),
                    CommandEntry::new(
                        "Archivar la nota".into(),
                        "archive".into(),
                        Some("Ctrl+Mayús+A".into()),
                        false,
                    ),
                ])),
                None,
                Rc::new(move |pick: Pick| {
                    if let Pick::Command(command) = pick {
                        corridos.borrow_mut().push(command.action);
                    }
                }),
            );
            // Elegir con la lista inicial: corre el único comando habilitado.
            let _ = dialog.activate_action("switcher.choose", None::<&glib::Variant>);
            assert_eq!(
                captura.borrow().as_slice(),
                ["view-code"],
                "debe correr el comando habilitado"
            );
            // Filtrar por el comando desactivado: elegir no lo debe correr.
            entry.set_text("archi");
            for _ in 0..50 {
                glib::timeout_future(std::time::Duration::from_millis(10)).await;
            }
            let _ = dialog.activate_action("switcher.choose", None::<&glib::Variant>);
            assert_eq!(
                captura.borrow().as_slice(),
                ["view-code"],
                "el comando desactivado no se ejecuta"
            );
            parent.close();
        });
    }
}
