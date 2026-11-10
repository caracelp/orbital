//! Ventana principal: pantalla inicial, lista de la bóveda y visor de notas.
//!
//! Toda la E/S (abrir la bóveda, leer notas, guardar el estado) corre con `gio::spawn_blocking`;
//! el hilo principal solo arma widgets.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;

use adw::prelude::*;
use gtk::{gio, glib};
use orbital_vault::{Entry, EntryKind, Vault};

use crate::state::AppState;

/// Hijos de cada carpeta (`""` = raíz), ordenados: primero carpetas, después archivos.
type Tree = HashMap<String, Vec<Arc<Entry>>>;

struct Inner {
    window: adw::ApplicationWindow,
    toasts: adw::ToastOverlay,
    stack: gtk::Stack,
    split: adw::NavigationSplitView,
    sidebar_title: adw::WindowTitle,
    content_title: adw::WindowTitle,
    banner: adw::Banner,
    list_view: gtk::ListView,
    editor: gtk::TextView,
    vault: RefCell<Option<Rc<Vault>>>,
    tree: RefCell<Rc<Tree>>,
}

/// Ventana de Orbital.
#[derive(Clone)]
pub struct Window(Rc<Inner>);

impl Window {
    pub fn new(app: &adw::Application) -> Self {
        // --- Visor de notas (contenido) ---
        let editor = gtk::TextView::builder()
            .wrap_mode(gtk::WrapMode::WordChar)
            .monospace(true)
            .editable(false)
            .top_margin(24)
            .bottom_margin(24)
            .left_margin(32)
            .right_margin(32)
            .vexpand(true)
            .build();
        editor.update_property(&[gtk::accessible::Property::Label("Contenido de la nota")]);
        let editor_scroll = gtk::ScrolledWindow::builder().child(&editor).build();

        let content_title = adw::WindowTitle::new("", "");
        let content_header = adw::HeaderBar::new();
        content_header.set_title_widget(Some(&content_title));
        let content_view = adw::ToolbarView::new();
        content_view.add_top_bar(&content_header);
        content_view.set_content(Some(&editor_scroll));
        let content_page = adw::NavigationPage::builder()
            .title("Nota")
            .child(&content_view)
            .build();

        // --- Lista de la bóveda (barra lateral) ---
        let list_view = gtk::ListView::builder()
            .single_click_activate(false)
            .build();
        list_view.add_css_class("navigation-sidebar");
        list_view.update_property(&[gtk::accessible::Property::Label("Contenido de la bóveda")]);
        let sidebar_scroll = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .child(&list_view)
            .vexpand(true)
            .build();

        let banner = adw::Banner::builder()
            .title("La estructura de la bóveda está incompleta")
            .revealed(false)
            .build();

        let sidebar_title = adw::WindowTitle::new("Orbital", "");
        let sidebar_header = adw::HeaderBar::new();
        sidebar_header.set_title_widget(Some(&sidebar_title));
        sidebar_header.pack_start(&open_menu_button());
        let sidebar_view = adw::ToolbarView::new();
        sidebar_view.add_top_bar(&sidebar_header);
        sidebar_view.add_top_bar(&banner);
        sidebar_view.set_content(Some(&sidebar_scroll));
        let sidebar_page = adw::NavigationPage::builder()
            .title("Bóveda")
            .child(&sidebar_view)
            .build();

        let split = adw::NavigationSplitView::builder()
            .sidebar(&sidebar_page)
            .content(&content_page)
            .build();

        // --- Pantalla inicial ---
        let start = start_page();

        let stack = gtk::Stack::builder()
            .transition_type(gtk::StackTransitionType::Crossfade)
            .build();
        stack.add_named(&start, Some("inicio"));
        stack.add_named(&split, Some("boveda"));

        let toasts = adw::ToastOverlay::new();
        toasts.set_child(Some(&stack));

        let window = adw::ApplicationWindow::builder()
            .application(app)
            .title("Orbital")
            .default_width(1000)
            .default_height(680)
            .width_request(360)
            .height_request(300)
            .content(&toasts)
            .build();

        // En pantallas estrechas, la barra lateral y el visor se apilan.
        let breakpoint = adw::Breakpoint::new(adw::BreakpointCondition::new_length(
            adw::BreakpointConditionLengthType::MaxWidth,
            600.0,
            adw::LengthUnit::Sp,
        ));
        breakpoint.add_setter(&split, "collapsed", Some(&true.to_value()));
        window.add_breakpoint(breakpoint);

        let this = Self(Rc::new(Inner {
            window,
            toasts,
            stack,
            split,
            sidebar_title,
            content_title,
            banner,
            list_view,
            editor,
            vault: RefCell::new(None),
            tree: RefCell::new(Rc::new(Tree::new())),
        }));
        this.setup_actions(app);
        this.setup_list();
        this
    }

    pub fn present(&self) {
        self.0.window.present();
    }

    fn setup_actions(&self, app: &adw::Application) {
        let open = gio::ActionEntry::builder("open")
            .activate({
                let this = self.clone();
                move |_: &adw::ApplicationWindow, _, _| this.choose_and_open()
            })
            .build();
        // «Abrir como bóveda de Orbital» llega con la historia US2.
        let open_as = gio::SimpleAction::new("open-as", None);
        open_as.set_enabled(false);
        let shortcuts = gio::ActionEntry::builder("shortcuts")
            .activate({
                let this = self.clone();
                move |_: &adw::ApplicationWindow, _, _| this.show_shortcuts()
            })
            .build();
        self.0.window.add_action_entries([open, shortcuts]);
        self.0.window.add_action(&open_as);
        app.set_accels_for_action("win.open", &["<Control>o"]);
        app.set_accels_for_action("win.open-as", &["<Control><Shift>o"]);
        app.set_accels_for_action("win.shortcuts", &["<Control>question"]);
    }

    fn show_shortcuts(&self) {
        let dialog = adw::ShortcutsDialog::new();
        let section = adw::ShortcutsSection::new(Some("Bóveda"));
        section.add(adw::ShortcutsItem::new("Abrir una carpeta", "<Control>o"));
        section.add(adw::ShortcutsItem::new(
            "Abrir como bóveda de Orbital",
            "<Control><Shift>o",
        ));
        section.add(adw::ShortcutsItem::new(
            "Atajos de teclado",
            "<Control>question",
        ));
        dialog.add(section);
        dialog.present(Some(&self.0.window));
    }

    fn setup_list(&self) {
        let factory = gtk::SignalListItemFactory::new();
        factory.connect_setup(|_, item| {
            let item = item.downcast_ref::<gtk::ListItem>().expect("ListItem");
            let icon = gtk::Image::new();
            let label = gtk::Label::builder()
                .xalign(0.0)
                .ellipsize(gtk::pango::EllipsizeMode::End)
                .build();
            let row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
            row.append(&icon);
            row.append(&label);
            let expander = gtk::TreeExpander::new();
            expander.set_child(Some(&row));
            item.set_child(Some(&expander));
        });
        let tree_for_bind = self.clone();
        factory.connect_bind(move |_, item| {
            let item = item.downcast_ref::<gtk::ListItem>().expect("ListItem");
            let Some(row) = item.item().and_downcast::<gtk::TreeListRow>() else {
                return;
            };
            let Some(path) = row.item().and_downcast::<gtk::StringObject>() else {
                return;
            };
            let Some(expander) = item.child().and_downcast::<gtk::TreeExpander>() else {
                return;
            };
            expander.set_list_row(Some(&row));
            let Some(entry) = tree_for_bind.entry(path.string().as_str()) else {
                return;
            };
            let name = entry.path.file_name().to_owned();
            let content = expander.child().and_downcast::<gtk::Box>().expect("Box");
            let icon = content
                .first_child()
                .and_downcast::<gtk::Image>()
                .expect("Image");
            let label = content
                .last_child()
                .and_downcast::<gtk::Label>()
                .expect("Label");
            icon.set_icon_name(Some(icon_for(entry.kind)));
            label.set_label(&name);
            let what = match entry.kind {
                EntryKind::Folder => "Carpeta",
                EntryKind::AtomNote | EntryKind::Agents | EntryKind::Reserved(_) => "Nota",
                EntryKind::SynthesisNote => "Nota HTML",
                _ => "Archivo",
            };
            let ro = if entry.read_only {
                ", solo lectura"
            } else {
                ""
            };
            item.set_accessible_label(&format!("{what} {name}{ro}"));
        });
        self.0.list_view.set_factory(Some(&factory));

        let this = self.clone();
        self.0.list_view.connect_activate(move |view, pos| {
            let Some(model) = view.model() else { return };
            let Some(row) = model.item(pos).and_downcast::<gtk::TreeListRow>() else {
                return;
            };
            let Some(path) = row.item().and_downcast::<gtk::StringObject>() else {
                return;
            };
            match this.entry(path.string().as_str()) {
                Some(e) if e.kind == EntryKind::Folder => row.set_expanded(!row.is_expanded()),
                Some(e) => this.show_entry(e),
                None => {}
            }
        });
    }

    /// Busca una entrada por ruta en el árbol cargado.
    fn entry(&self, path: &str) -> Option<Arc<Entry>> {
        let parent = path.rsplit_once('/').map_or("", |(p, _)| p);
        let tree = self.0.tree.borrow();
        tree.get(parent)?
            .iter()
            .find(|e| e.path.as_str() == path)
            .cloned()
    }

    /// Elige una carpeta con el diálogo del sistema y la abre.
    pub fn choose_and_open(&self) {
        let this = self.clone();
        glib::spawn_future_local(async move {
            let dialog = gtk::FileDialog::builder()
                .title("Abrir una carpeta")
                .modal(true)
                .build();
            match dialog.select_folder_future(Some(&this.0.window)).await {
                Ok(folder) => {
                    if let Some(path) = folder.path() {
                        this.open_path(path).await;
                    }
                }
                Err(e) if e.matches(gtk::DialogError::Dismissed) => {}
                Err(e) if e.matches(gtk::DialogError::Cancelled) => {}
                Err(e) => this.toast(&format!("No se pudo elegir la carpeta: {e}")),
            }
        });
    }

    /// Abre `path` con «Abrir»: no escribe nada en la bóveda.
    pub async fn open_path(&self, path: PathBuf) {
        let for_thread = path.clone();
        let result = gio::spawn_blocking(move || {
            orbital_vault::open(&for_thread).map(|v| {
                let tree = build_tree(v.entries());
                (v, tree)
            })
        })
        .await;
        match result {
            Ok(Ok((vault, tree))) => {
                self.show_vault(vault, tree);
                save_state_opened(path);
            }
            Ok(Err(e)) => self.toast(&format!("No se pudo abrir la bóveda: {e}")),
            Err(_) => self.toast("No se pudo abrir la bóveda"),
        }
    }

    fn show_vault(&self, vault: Vault, tree: Tree) {
        let i = &self.0;
        let name = vault
            .root()
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| vault.root().display().to_string());
        i.sidebar_title.set_title(&name);
        i.sidebar_title
            .set_subtitle(&format!("{} elementos", vault.entries().len()));
        i.window.set_title(Some(&format!("{name} — Orbital")));

        let incomplete = !vault.layout().is_complete();
        i.banner.set_revealed(incomplete);
        for w in vault.warnings() {
            self.toast(w);
        }

        let tree = Rc::new(tree);
        *i.tree.borrow_mut() = tree.clone();
        *i.vault.borrow_mut() = Some(Rc::new(vault));

        let root = children_model(&tree, "");
        let tree_model = gtk::TreeListModel::new(root, false, false, move |item| {
            let path = item.downcast_ref::<gtk::StringObject>()?.string();
            tree.get(path.as_str())
                .map(|_| children_model(&tree, path.as_str()).upcast::<gio::ListModel>())
        });
        let selection = gtk::SingleSelection::builder()
            .model(&tree_model)
            .autoselect(false)
            .build();
        i.list_view.set_model(Some(&selection));

        i.editor.buffer().set_text("");
        i.content_title.set_title("");
        i.content_title.set_subtitle("");
        i.stack.set_visible_child_name("boveda");
        i.list_view.grab_focus();
        if incomplete {
            i.list_view.announce(
                "La estructura de la bóveda está incompleta",
                gtk::AccessibleAnnouncementPriority::Medium,
            );
        }
    }

    /// Muestra una nota o un archivo de texto (solo lectura hasta la historia US3).
    fn show_entry(&self, entry: Arc<Entry>) {
        let Some(vault) = self.0.vault.borrow().clone() else {
            return;
        };
        let this = self.clone();
        let abs = entry.path.to_path(vault.root());
        glib::spawn_future_local(async move {
            let text = if entry.kind.is_text() {
                gio::spawn_blocking(move || read_text(&abs))
                    .await
                    .unwrap_or_else(|_| Err("No se pudo leer el archivo".into()))
            } else {
                Err("Este archivo no es una nota de texto".into())
            };
            let i = &this.0;
            i.content_title.set_title(entry.path.file_name());
            let mut subtitle = entry.path.to_string();
            if entry.read_only {
                subtitle.push_str(" · solo lectura");
            }
            i.content_title.set_subtitle(&subtitle);
            match text {
                Ok(t) => i.editor.buffer().set_text(&t),
                Err(msg) => i.editor.buffer().set_text(&msg),
            }
            i.split.set_show_content(true);
        });
    }

    fn toast(&self, msg: &str) {
        self.0.toasts.add_toast(adw::Toast::new(msg));
    }
}

/// Lee un archivo de texto para mostrarlo.
fn read_text(path: &Path) -> Result<String, String> {
    let meta = std::fs::metadata(path).map_err(|e| e.to_string())?;
    if meta.len() > orbital_vault::entry::MAX_EDITABLE_BYTES {
        return Err("El archivo es demasiado grande para mostrarlo".into());
    }
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    String::from_utf8(bytes).map_err(|_| "El archivo no está en UTF-8".into())
}

/// Agrupa las entradas por carpeta, con las carpetas primero.
fn build_tree(entries: &[Entry]) -> Tree {
    let mut tree: Tree = HashMap::new();
    for e in entries {
        let parent = e.path.as_str().rsplit_once('/').map_or("", |(p, _)| p);
        tree.entry(parent.to_owned())
            .or_default()
            .push(Arc::new(e.clone()));
    }
    for children in tree.values_mut() {
        children.sort_by(|a, b| {
            (a.kind != EntryKind::Folder)
                .cmp(&(b.kind != EntryKind::Folder))
                .then_with(|| {
                    a.path
                        .file_name()
                        .to_lowercase()
                        .cmp(&b.path.file_name().to_lowercase())
                })
        });
    }
    tree
}

fn children_model(tree: &Tree, parent: &str) -> gio::ListStore {
    let store = gio::ListStore::new::<gtk::StringObject>();
    if let Some(children) = tree.get(parent) {
        let items: Vec<gtk::StringObject> = children
            .iter()
            .map(|e| gtk::StringObject::new(e.path.as_str()))
            .collect();
        store.extend_from_slice(&items);
    }
    store
}

fn icon_for(kind: EntryKind) -> &'static str {
    match kind {
        EntryKind::Folder => "folder-symbolic",
        EntryKind::AtomNote | EntryKind::Agents => "text-x-generic-symbolic",
        EntryKind::SynthesisNote => "text-html-symbolic",
        EntryKind::Reserved(_) => "view-list-symbolic",
        EntryKind::Base => "x-office-spreadsheet-symbolic",
        EntryKind::Canvas => "applications-graphics-symbolic",
        EntryKind::Other => "mail-attachment-symbolic",
    }
}

fn open_menu_button() -> gtk::MenuButton {
    let menu = gio::Menu::new();
    menu.append(Some("Abrir una carpeta…"), Some("win.open"));
    menu.append(Some("Abrir como bóveda de Orbital…"), Some("win.open-as"));
    menu.append(Some("Atajos de teclado"), Some("win.shortcuts"));
    gtk::MenuButton::builder()
        .icon_name("folder-open-symbolic")
        .tooltip_text("Abrir")
        .menu_model(&menu)
        .build()
}

fn start_page() -> adw::StatusPage {
    let open = gtk::Button::builder()
        .label("Abrir una carpeta")
        .action_name("win.open")
        .css_classes(["suggested-action", "pill"])
        .build();
    let open_as = gtk::Button::builder()
        .label("Abrir como bóveda de Orbital")
        .action_name("win.open-as")
        .css_classes(["pill"])
        .build();
    let buttons = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(12)
        .halign(gtk::Align::Center)
        .build();
    buttons.append(&open);
    buttons.append(&open_as);
    adw::StatusPage::builder()
        .icon_name("folder-open-symbolic")
        .title("Orbital")
        .description("Abrí una carpeta para ver tus notas. «Abrir» no cambia nada en la carpeta.")
        .child(&buttons)
        .build()
}

/// Guarda en segundo plano que se abrió `path`.
fn save_state_opened(path: PathBuf) {
    glib::spawn_future_local(async move {
        let _ = gio::spawn_blocking(move || {
            let mut state = AppState::load();
            state.opened(&path);
            state.save()
        })
        .await;
    });
}
