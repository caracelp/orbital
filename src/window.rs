//! Ventana principal: pantalla inicial, lista de la bóveda y visor de notas.
//!
//! Toda la E/S (abrir la bóveda, leer notas, guardar el estado) corre con `gio::spawn_blocking`;
//! el hilo principal solo arma widgets.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;

use adw::prelude::*;
use gtk::{gio, glib};
use orbital_vault::props::{PropEntry, PropValue, Warning};
use orbital_vault::{DeletePlan, Entry, EntryKind, Format, Note, NoteMeta, RelPath, Vault};
use webkit6::prelude::*;

use crate::html_view;
use crate::open_dialog::{self, Answer};
use crate::state::AppState;
use crate::watcher::{Batch, Watcher};
use crate::write_ctx;

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
    content_stack: gtk::Stack,
    props_list: gtk::ListBox,
    note_banner: adw::Banner,
    type_banner: adw::Banner,
    current_note: RefCell<Option<Note>>,
    /// Archivo que se está mostrando (nota o no).
    current_entry: RefCell<Option<Arc<Entry>>>,
    html_props: gtk::ListBox,
    new_button: gtk::MenuButton,
    external_page: adw::StatusPage,
    html_page: gtk::Box,
    /// Vista web compartida; `None` hasta abrir la primera nota HTML.
    web: RefCell<Option<webkit6::WebView>>,
    vault: RefCell<Option<Arc<Vault>>>,
    tree: RefCell<Rc<Tree>>,
    /// Si es `false` (modo de captura), no se guarda el estado de la app.
    persist: Cell<bool>,
    watcher: RefCell<Option<Watcher>>,
    /// La carpeta de la bóveda dejó de estar disponible (disco desconectado).
    gone: Cell<bool>,
    gone_banner: adw::Banner,
}

/// Ventana de Orbital.
#[derive(Clone)]
pub struct Window(Rc<Inner>);

impl Window {
    pub fn new(app: &adw::Application) -> Self {
        // --- Visor de notas (contenido) ---
        let editor = gtk::TextView::builder()
            .wrap_mode(gtk::WrapMode::WordChar)
            .editable(false)
            .top_margin(32)
            .bottom_margin(48)
            .left_margin(12)
            .right_margin(12)
            .pixels_inside_wrap(4)
            .pixels_below_lines(6)
            .vexpand(true)
            .build();
        editor.add_css_class("orbital-reader");
        editor.update_property(&[gtk::accessible::Property::Label("Contenido de la nota")]);
        // Ancho de lectura cómodo: la columna de texto no pasa de ~70 caracteres.
        // Propiedades arriba del texto, como una lista de filas.
        let props_list = gtk::ListBox::builder()
            .selection_mode(gtk::SelectionMode::None)
            .margin_top(24)
            .margin_start(12)
            .margin_end(12)
            .visible(false)
            .build();
        props_list.add_css_class("boxed-list");
        props_list.update_property(&[gtk::accessible::Property::Label("Propiedades de la nota")]);
        let column = gtk::Box::new(gtk::Orientation::Vertical, 0);
        column.append(&props_list);
        column.append(&editor);
        let reader = adw::Clamp::builder()
            .maximum_size(720)
            .tightening_threshold(560)
            .child(&column)
            .build();
        let editor_scroll = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .child(&reader)
            .build();

        // Sin nota elegida, el panel muestra qué hacer en vez de quedar vacío.
        let empty = adw::StatusPage::builder()
            .icon_name("text-x-generic-symbolic")
            .title("Ninguna nota abierta")
            .description("Elegí una nota de la lista para leerla.")
            .build();
        let content_stack = gtk::Stack::builder()
            .transition_type(gtk::StackTransitionType::Crossfade)
            .build();
        content_stack.add_named(&empty, Some("vacio"));
        content_stack.add_named(&editor_scroll, Some("nota"));
        // Página de notas HTML: propiedades arriba y la vista web (que se crea al usarla) abajo.
        let html_props = gtk::ListBox::builder()
            .selection_mode(gtk::SelectionMode::None)
            .margin_top(12)
            .margin_bottom(12)
            .margin_start(12)
            .margin_end(12)
            .visible(false)
            .build();
        html_props.add_css_class("boxed-list");
        html_props.update_property(&[gtk::accessible::Property::Label("Propiedades de la nota")]);
        let html_page = gtk::Box::new(gtk::Orientation::Vertical, 0);
        html_page.append(
            &adw::Clamp::builder()
                .maximum_size(720)
                .child(&html_props)
                .build(),
        );
        content_stack.add_named(&html_page, Some("html"));
        // Archivos que no son texto (PDF, imágenes…): se abren con otra aplicación.
        let open_external = gtk::Button::builder()
            .label("Abrir con otra aplicación")
            .action_name("win.open-external")
            .css_classes(["pill"])
            .halign(gtk::Align::Center)
            .build();
        let external_page = adw::StatusPage::builder()
            .icon_name("document-open-symbolic")
            .child(&open_external)
            .build();
        content_stack.add_named(&external_page, Some("externo"));

        let content_title = adw::WindowTitle::new("", "");
        let content_header = adw::HeaderBar::new();
        content_header.set_title_widget(Some(&content_title));
        let note_menu_button = menu_button(
            "view-more-symbolic",
            "Opciones de la nota",
            &[
                ("Renombrar o mover…", "win.rename", "F2"),
                ("Archivar", "win.archive", "Ctrl+Mayús+A"),
                (
                    "Pegar imagen como adjunto",
                    "win.paste-image",
                    "Ctrl+Mayús+V",
                ),
                ("Mover a la papelera", "win.trash", "Supr"),
            ],
        );
        content_header.pack_end(&note_menu_button);
        let content_view = adw::ToolbarView::new();
        content_view.add_top_bar(&content_header);
        // Avisos de la nota abierta: falta `type` (con «Completar») o propiedades inválidas.
        // Dos avisos fijos: AT-SPI (y Orca) no ven un botón que se agrega después de crear el aviso.
        let type_banner = adw::Banner::builder()
            .title(Warning::MissingType.message())
            .button_label("Completar")
            .action_name("win.complete-type")
            .revealed(true)
            .visible(false)
            .build();
        let note_banner = adw::Banner::builder().revealed(true).visible(false).build();
        content_view.add_top_bar(&type_banner);
        content_view.add_top_bar(&note_banner);
        content_view.set_content(Some(&content_stack));
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
            .title("Estructura incompleta")
            .button_label("Completar…")
            .action_name("win.complete-structure")
            .revealed(true)
            .visible(false)
            .build();

        let sidebar_title = adw::WindowTitle::new("Orbital", "");
        let sidebar_header = adw::HeaderBar::new();
        sidebar_header.set_title_widget(Some(&sidebar_title));
        sidebar_header.pack_start(&open_menu_button());
        let new_button = menu_button("list-add-symbolic", "Nueva nota", &[]);
        sidebar_header.pack_end(&new_button);
        let sidebar_view = adw::ToolbarView::new();
        sidebar_view.add_top_bar(&sidebar_header);
        sidebar_view.add_top_bar(&banner);
        let gone_banner = adw::Banner::builder()
            .title("La bóveda no está disponible. Orbital no va a escribir nada.")
            .revealed(true)
            .visible(false)
            .build();
        sidebar_view.add_top_bar(&gone_banner);
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
            content_stack,
            props_list,
            note_banner,
            type_banner,
            current_note: RefCell::new(None),
            current_entry: RefCell::new(None),
            html_props,
            new_button,
            external_page,
            html_page,
            web: RefCell::new(None),
            vault: RefCell::new(None),
            tree: RefCell::new(Rc::new(Tree::new())),
            persist: Cell::new(true),
            watcher: RefCell::new(None),
            gone: Cell::new(false),
            gone_banner,
        }));
        this.setup_actions(app);
        let content = this.0.content_stack.clone();
        this.setup_drop(&content);
        this.setup_raw_drop();
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
        let open_as = gio::ActionEntry::builder("open-as")
            .activate({
                let this = self.clone();
                move |_: &adw::ApplicationWindow, _, _| this.choose_and_open_as()
            })
            .build();
        let complete = gio::ActionEntry::builder("complete-structure")
            .activate({
                let this = self.clone();
                move |_: &adw::ApplicationWindow, _, _| {
                    let root = this
                        .0
                        .vault
                        .borrow()
                        .as_ref()
                        .map(|v| v.root().to_path_buf());
                    if let Some(root) = root {
                        let this = this.clone();
                        glib::spawn_future_local(async move { this.open_as_path(root).await });
                    }
                }
            })
            .build();
        let complete_type = gio::ActionEntry::builder("complete-type")
            .activate({
                let this = self.clone();
                move |_: &adw::ApplicationWindow, _, _| this.complete_type()
            })
            .build();
        let shortcuts = gio::ActionEntry::builder("shortcuts")
            .activate({
                let this = self.clone();
                move |_: &adw::ApplicationWindow, _, _| this.show_shortcuts()
            })
            .build();
        self.0
            .window
            .add_action_entries([open, open_as, complete, complete_type, shortcuts]);
        let rename = gio::ActionEntry::builder("rename")
            .activate({
                let this = self.clone();
                move |_: &adw::ApplicationWindow, _, _| this.ask_rename()
            })
            .build();
        let trash = gio::ActionEntry::builder("trash")
            .activate({
                let this = self.clone();
                move |_: &adw::ApplicationWindow, _, _| this.ask_trash()
            })
            .build();
        self.0.window.add_action_entries([rename, trash]);
        let simple = |name: &str, f: fn(&Window)| {
            let this = self.clone();
            gio::ActionEntry::builder(name)
                .activate(move |_: &adw::ApplicationWindow, _, _| f(&this))
                .build()
        };
        let from_template = gio::ActionEntry::builder("new-from-template")
            .parameter_type(Some(glib::VariantTy::STRING))
            .activate({
                let this = self.clone();
                move |_: &adw::ApplicationWindow, _, param| {
                    if let Some(p) = param.and_then(|v| v.get::<String>()) {
                        this.ask_new_note(NewKind::Template(p));
                    }
                }
            })
            .build();
        self.0.window.add_action_entries([
            simple("open-external", |w| {
                let entry = w.0.current_entry.borrow().clone();
                let root = w.0.vault.borrow().as_ref().map(|v| v.root().to_path_buf());
                if let (Some(e), Some(root)) = (entry, root) {
                    w.launch_file(&e.path.to_path(&root));
                }
            }),
            simple("preferences", Window::show_preferences),
            simple("new-note", |w| w.ask_new_note(NewKind::Md)),
            simple("new-html", |w| w.ask_new_note(NewKind::Html)),
            simple("journal", Window::open_journal),
            simple("archive", Window::archive_current),
            simple("paste-image", Window::paste_image),
            from_template,
        ]);
        app.set_accels_for_action("win.new-note", &["<Control>n"]);
        app.set_accels_for_action("win.preferences", &["<Control>comma"]);
        app.set_accels_for_action("win.journal", &["<Control><Alt>d"]);
        app.set_accels_for_action("win.archive", &["<Control><Shift>a"]);
        app.set_accels_for_action("win.paste-image", &["<Control><Shift>v"]);
        app.set_accels_for_action("win.rename", &["F2"]);
        app.set_accels_for_action("win.trash", &["Delete"]);
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
            "Preferencias de la bóveda",
            "<Control>comma",
        ));
        section.add(adw::ShortcutsItem::new("Nota nueva", "<Control>n"));
        section.add(adw::ShortcutsItem::new("Diario de hoy", "<Control><Alt>d"));
        section.add(adw::ShortcutsItem::new(
            "Archivar la nota",
            "<Control><Shift>a",
        ));
        section.add(adw::ShortcutsItem::new(
            "Pegar imagen como adjunto",
            "<Control><Shift>v",
        ));
        section.add(adw::ShortcutsItem::new("Renombrar o mover la nota", "F2"));
        section.add(adw::ShortcutsItem::new(
            "Mover la nota a la papelera",
            "Delete",
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
            // Las notas se abren al elegirlas (ver `show_vault`); activar solo expande carpetas.
            if this
                .entry(path.string().as_str())
                .is_some_and(|e| e.kind == EntryKind::Folder)
            {
                row.set_expanded(!row.is_expanded());
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

    /// Diálogo del sistema para elegir una carpeta.
    async fn choose_folder(&self, title: &str) -> Option<PathBuf> {
        let dialog = gtk::FileDialog::builder().title(title).modal(true).build();
        match dialog.select_folder_future(Some(&self.0.window)).await {
            Ok(folder) => folder.path(),
            Err(e)
                if e.matches(gtk::DialogError::Dismissed)
                    || e.matches(gtk::DialogError::Cancelled) =>
            {
                None
            }
            Err(e) => {
                self.toast(&format!("No se pudo elegir la carpeta: {e}"));
                None
            }
        }
    }

    /// Elige una carpeta y la abre con «Abrir».
    pub fn choose_and_open(&self) {
        let this = self.clone();
        glib::spawn_future_local(async move {
            if let Some(path) = this.choose_folder("Abrir una carpeta").await {
                this.open_path(path).await;
            }
        });
    }

    /// Elige una carpeta y la abre con «Abrir como bóveda de Orbital».
    pub fn choose_and_open_as(&self) {
        let this = self.clone();
        glib::spawn_future_local(async move {
            if let Some(path) = this.choose_folder("Abrir como bóveda de Orbital").await {
                this.open_as_path(path).await;
            }
        });
    }

    /// «Abrir como»: muestra lo que falta, pide confirmación y crea solo eso.
    pub async fn open_as_path(&self, path: PathBuf) {
        let for_plan = path.clone();
        let plan = match gio::spawn_blocking(move || orbital_vault::plan_open_as(&for_plan)).await {
            Ok(Ok(plan)) => plan,
            Ok(Err(e)) => return self.toast(&format!("No se pudo revisar la carpeta: {e}")),
            Err(_) => return self.toast("No se pudo revisar la carpeta"),
        };
        if open_dialog::confirm(&self.0.window, &plan).await == Answer::Cancel {
            return;
        }
        let for_apply = path.clone();
        let result = gio::spawn_blocking(move || {
            let (config, _) = orbital_vault::VaultConfig::load(&for_apply);
            let ctx = write_ctx::now(Some(&config));
            orbital_vault::apply_open_as(&for_apply, &plan, &ctx).map(prepare)
        })
        .await;
        match result {
            Ok(Ok((vault, tree, meta))) => {
                self.show_vault(vault, tree, meta);
                self.remember(path);
            }
            Ok(Err(e)) => self.toast(&format!("No se pudo crear la estructura: {e}")),
            Err(_) => self.toast("No se pudo crear la estructura"),
        }
    }

    /// Abre `path` con «Abrir»: no escribe nada en la bóveda.
    pub async fn open_path(&self, path: PathBuf) {
        let for_thread = path.clone();
        let result =
            gio::spawn_blocking(move || orbital_vault::open(&for_thread).map(prepare)).await;
        match result {
            Ok(Ok((vault, tree, meta))) => {
                self.show_vault(vault, tree, meta);
                self.remember(path);
            }
            Ok(Err(e)) => self.toast(&format!("No se pudo abrir la bóveda: {e}")),
            Err(_) => self.toast("No se pudo abrir la bóveda"),
        }
    }

    fn show_vault(&self, vault: Vault, tree: Tree, meta: NoteMeta) {
        let i = &self.0;
        self.fill_new_menu(&meta.templates);
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
        i.banner.set_visible(incomplete);
        for w in vault.warnings() {
            self.toast(w);
        }

        let tree = Rc::new(tree);
        *i.tree.borrow_mut() = tree.clone();
        let folders: Vec<PathBuf> = vault
            .entries()
            .iter()
            .filter(|e| e.kind == EntryKind::Folder)
            .map(|e| e.path.to_path(vault.root()))
            .collect();
        let this = self.clone();
        *i.watcher.borrow_mut() =
            Some(Watcher::new(
                vault.root(),
                folders.into_iter(),
                move |b| match b {
                    Batch::Changed(paths) => this.on_disk_changes(paths),
                    Batch::Gone => this.vault_gone(),
                },
            ));
        self.set_gone(false);
        *i.vault.borrow_mut() = Some(Arc::new(vault));

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
        // Elegir un archivo lo abre (clic, flechas o AT-SPI), como en las barras laterales de GNOME.
        let this = self.clone();
        selection.connect_selection_changed(move |sel, _, _| {
            let Some(row) = sel.selected_item().and_downcast::<gtk::TreeListRow>() else {
                return;
            };
            let Some(path) = row.item().and_downcast::<gtk::StringObject>() else {
                return;
            };
            if let Some(e) = this
                .entry(path.string().as_str())
                .filter(|e| e.kind != EntryKind::Folder)
            {
                this.show_entry(e);
            }
        });
        i.list_view.set_model(Some(&selection));

        i.editor.buffer().set_text("");
        i.content_stack.set_visible_child_name("vacio");
        // Bóveda vacía: decir qué hacer en vez de mostrar una lista en blanco.
        if let Some(empty) = i
            .content_stack
            .child_by_name("vacio")
            .and_downcast::<adw::StatusPage>()
        {
            let vault_empty = i
                .vault
                .borrow()
                .as_ref()
                .is_some_and(|v| v.entries().iter().all(|e| e.kind == EntryKind::Folder));
            if vault_empty {
                empty.set_title("La bóveda todavía no tiene notas");
                empty.set_description(Some(
                    "Creá una con Ctrl+N o con el botón + de la barra lateral.",
                ));
            } else {
                empty.set_title("Ninguna nota abierta");
                empty.set_description(Some("Elegí una nota de la lista para leerla."));
            }
        }
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

    /// Muestra una nota (propiedades + texto) o un archivo de texto.
    fn show_entry(&self, entry: Arc<Entry>) {
        let Some(vault) = self.0.vault.borrow().clone() else {
            return;
        };
        let this = self.clone();
        glib::spawn_future_local(async move {
            let is_note = Format::from_name(entry.path.file_name()).is_some();
            let loaded = if is_note {
                let (v, p) = (vault.clone(), entry.path.clone());
                match gio::spawn_blocking(move || v.read_note(&p)).await {
                    Ok(Ok(note)) => Ok(Some(note)),
                    Ok(Err(e)) => Err(e.to_string()),
                    Err(_) => Err("No se pudo leer la nota".to_owned()),
                }
            } else {
                // Cualquier otro archivo: si es texto UTF-8 se muestra; si no, se ofrece abrirlo afuera.
                let abs = entry.path.to_path(vault.root());
                gio::spawn_blocking(move || read_text(&abs))
                    .await
                    .unwrap_or_else(|_| Err("No se pudo leer el archivo".into()))
                    .map(|t| {
                        this.0.editor.buffer().set_text(&t);
                        None
                    })
            };
            let i = &this.0;
            *i.current_entry.borrow_mut() = Some(entry.clone());
            i.content_title.set_title(entry.path.file_name());
            let mut subtitle = entry.path.to_string();
            if entry.read_only {
                subtitle.push_str(" · solo lectura");
            }
            i.content_title.set_subtitle(&subtitle);
            this.clear_note_view();
            let raw = entry.zone == orbital_vault::Zone::Raw;
            match loaded {
                Ok(Some(note)) => this.show_note(note),
                Ok(None) => i.content_stack.set_visible_child_name("nota"),
                Err(_) => {
                    i.external_page.set_title(entry.path.file_name());
                    i.external_page.set_description(Some(if raw {
                        "Fuente original, solo lectura. Orbital no la modifica."
                    } else {
                        "Este archivo no es una nota de texto."
                    }));
                    i.content_stack.set_visible_child_name("externo");
                }
            }
            if raw {
                i.note_banner.set_title("Fuente original, solo lectura");
                i.note_banner.set_visible(true);
            }
            // En las fuentes de raw/ no se puede renombrar, archivar, adjuntar ni borrar.
            let is_note = Format::from_name(entry.path.file_name()).is_some();
            for (name, enabled) in [
                ("rename", !raw),
                ("trash", !raw),
                ("archive", !raw && is_note),
                ("paste-image", !raw && is_note),
            ] {
                if let Some(action) = i
                    .window
                    .lookup_action(name)
                    .and_downcast::<gio::SimpleAction>()
                {
                    action.set_enabled(enabled);
                }
            }
            i.split.set_show_content(true);
        });
    }

    fn clear_note_view(&self) {
        let i = &self.0;
        i.props_list.remove_all();
        i.props_list.set_visible(false);
        i.html_props.remove_all();
        i.html_props.set_visible(false);
        i.note_banner.set_visible(false);
        i.type_banner.set_visible(false);
        *i.current_note.borrow_mut() = None;
    }

    /// Dibuja las propiedades, los avisos y el texto de una nota.
    fn show_note(&self, note: Note) {
        let i = &self.0;
        let error = note.props.as_ref().and_then(|b| b.error.clone());
        if let Some(err) = &error {
            // Propiedades inválidas: se muestra todo el archivo tal cual y no se edita nada.
            i.note_banner.set_title(&format!(
                "Las propiedades tienen un error en la línea {}: {}",
                err.line, err.message
            ));
            i.note_banner.set_visible(true);
            i.editor.buffer().set_text(&note.source);
            i.content_stack.set_visible_child_name("nota");
        } else {
            let list = match note.format {
                Format::Md => &i.props_list,
                Format::Html => &i.html_props,
            };
            if let Some(block) = &note.props {
                self.fill_props(list, &note.path, &block.entries);
            }
            let warnings = note.warnings();
            if warnings.contains(&Warning::MissingType) && !note.read_only {
                i.type_banner.set_visible(true);
            }
            if let Some(w) = warnings.iter().find(|w| **w != Warning::MissingType) {
                i.note_banner.set_title(&w.message());
                i.note_banner.set_visible(true);
            }
            match note.format {
                Format::Md => {
                    i.editor.buffer().set_text(note.body());
                    i.content_stack.set_visible_child_name("nota");
                }
                Format::Html => {
                    self.load_html(&note);
                    i.content_stack.set_visible_child_name("html");
                }
            }
        }
        *i.current_note.borrow_mut() = Some(note);
    }

    /// Muestra una nota HTML como página, creando la vista web la primera vez.
    fn load_html(&self, note: &Note) {
        let i = &self.0;
        let Some(vault) = i.vault.borrow().clone() else {
            return;
        };
        let view = i
            .web
            .borrow_mut()
            .get_or_insert_with(|| {
                let v = html_view::new_view();
                v.update_property(&[gtk::accessible::Property::Label("Página de la nota")]);
                i.html_page.append(&v);
                v
            })
            .clone();
        // Las rutas relativas (imágenes, CSS) se resuelven desde la carpeta de la nota.
        let dir = note
            .path
            .to_path(vault.root())
            .parent()
            .map(|d| gio::File::for_path(d).uri().to_string() + "/");
        view.load_html(&html_view::with_csp(&note.source), dir.as_deref());
    }

    /// Llena una lista con las propiedades de una nota. Las citas de `sources` tienen un botón
    /// que abre la fuente (FR-006, historia 6).
    fn fill_props(&self, list: &gtk::ListBox, note: &RelPath, entries: &[PropEntry]) {
        for e in entries {
            if e.key == "sources"
                && let PropValue::List(items) = &e.value
            {
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
                    list.append(&row);
                }
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

    /// Abre una fuente citada con la aplicación del sistema, si existe en la bóveda o es una URL.
    fn open_source(&self, from: &RelPath, resource: &str) {
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

    /// Abre un archivo con la aplicación predeterminada del sistema.
    fn launch_file(&self, path: &Path) {
        let launcher = gtk::FileLauncher::new(Some(&gio::File::for_path(path)));
        let this = self.clone();
        launcher.launch(Some(&self.0.window), None::<&gio::Cancellable>, move |r| {
            if let Err(e) = r
                && !e.matches(gtk::DialogError::Dismissed)
            {
                this.toast(&format!("No se pudo abrir: {e}"));
            }
        });
    }

    /// Copia archivos soltados sobre la lista a `raw/` (la única forma de agregar fuentes).
    fn setup_raw_drop(&self) {
        let drop = gtk::DropTarget::new(
            gtk::gdk::FileList::static_type(),
            gtk::gdk::DragAction::COPY,
        );
        let this = self.clone();
        drop.connect_drop(move |_, value, _, _| {
            let Ok(list) = value.get::<gtk::gdk::FileList>() else {
                return false;
            };
            let Some(vault) = this.0.vault.borrow().clone() else {
                return false;
            };
            let paths: Vec<PathBuf> = list.files().iter().filter_map(|f| f.path()).collect();
            let this = this.clone();
            glib::spawn_future_local(async move {
                let result = gio::spawn_blocking(move || {
                    let ctx = write_ctx::now(Some(vault.config()));
                    paths
                        .iter()
                        .map(|p| vault.copy_into_raw(p, &ctx))
                        .collect::<Result<Vec<_>, _>>()
                })
                .await;
                match result {
                    Ok(Ok(copied)) => {
                        let first = copied.first().cloned();
                        this.reload(first).await;
                        this.toast(&format!("{} fuente(s) copiada(s) a raw/", copied.len()));
                    }
                    Ok(Err(e)) => this.toast(&format!("No se pudo copiar a raw/: {e}")),
                    Err(_) => this.toast("No se pudo copiar a raw/"),
                }
            });
            true
        });
        self.0.list_view.add_controller(drop);
    }

    /// «Completar»: agrega `type` a la nota abierta, solo porque el usuario lo pidió.
    fn complete_type(&self) {
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

    fn remember(&self, path: PathBuf) {
        if self.0.persist.get() {
            save_state_opened(path);
        }
    }

    /// Modo de captura: no guarda el estado de la app.
    pub fn set_persist(&self, persist: bool) {
        self.0.persist.set(persist);
    }

    /// Vuelve a leer la bóveda (después de renombrar o borrar) y muestra `show`, si se indica.
    async fn reload(&self, show: Option<RelPath>) {
        let Some(root) = self
            .0
            .vault
            .borrow()
            .as_ref()
            .map(|v| v.root().to_path_buf())
        else {
            return;
        };
        let result = gio::spawn_blocking(move || orbital_vault::open(&root).map(prepare)).await;
        if let Ok(Ok((vault, tree, meta))) = result {
            self.show_vault(vault, tree, meta);
            if let Some(p) = show {
                self.show_path(p.as_str());
            }
        }
    }

    /// Arma el menú «Nueva nota» con las plantillas de la bóveda.
    fn fill_new_menu(&self, templates: &[RelPath]) {
        let Some(popover) = self.0.new_button.popover() else {
            return;
        };
        let mut items = vec![
            MenuItem::new("Nota", "win.new-note", "Ctrl+N"),
            MenuItem::new("Nota HTML", "win.new-html", ""),
            MenuItem::new("Diario de hoy", "win.journal", "Ctrl+Alt+D"),
        ];
        for t in templates {
            let name = t.file_name();
            let stem = name.rsplit_once('.').map_or(name, |(s, _)| s);
            let mut item = MenuItem::new(
                &format!("Desde la plantilla «{stem}»"),
                "win.new-from-template",
                "",
            );
            item.target = Some(t.to_string());
            items.push(item);
        }
        fill_menu(&popover, &items);
    }

    /// Carpeta donde se crean las notas nuevas: la de la nota abierta, o `wiki/`.
    fn target_dir(&self) -> Option<RelPath> {
        let vault = self.0.vault.borrow().clone()?;
        let from_current = self.0.current_entry.borrow().as_ref().and_then(|e| {
            let s = e.path.as_str();
            s.rsplit_once('/')
                .filter(|(d, _)| d.starts_with("wiki"))
                .and_then(|(d, _)| RelPath::new(d).ok())
        });
        from_current.or_else(|| {
            vault
                .root()
                .join("wiki")
                .is_dir()
                .then(|| RelPath::new("wiki").ok())
                .flatten()
        })
    }

    /// Pide el nombre de una nota nueva y la crea.
    fn ask_new_note(&self, kind: NewKind) {
        let Some(vault) = self.0.vault.borrow().clone() else {
            return;
        };
        let this = self.clone();
        glib::spawn_future_local(async move {
            let field = gtk::Entry::builder().activates_default(true).build();
            field.update_property(&[gtk::accessible::Property::Label("Nombre de la nota")]);
            let dialog = adw::AlertDialog::new(Some("Nota nueva"), Some("¿Cómo se llama?"));
            dialog.set_extra_child(Some(&field));
            dialog.add_response("cancel", "Cancelar");
            dialog.add_response("create", "Crear");
            dialog.set_response_appearance("create", adw::ResponseAppearance::Suggested);
            dialog.set_default_response(Some("create"));
            dialog.set_close_response("cancel");
            let f = field.clone();
            glib::idle_add_local_once(move || {
                f.grab_focus();
            });
            if dialog.choose_future(Some(&this.0.window)).await != "create" {
                return;
            }
            let name = field.text().trim().to_owned();
            if name.is_empty() || name.contains('/') {
                return this.toast("El nombre no puede estar vacío ni tener «/»");
            }
            warn_portable(&this, &name);
            let dir = this.target_dir();
            let result = gio::spawn_blocking(move || {
                let ctx = write_ctx::now(Some(vault.config()));
                let dir = match dir {
                    Some(d) => d,
                    None => return Err(orbital_vault::VaultError::InvalidPath(name)),
                };
                match kind {
                    NewKind::Md => vault
                        .create_note(&dir, &name, Format::Md, &ctx)
                        .map(|p| (p, Vec::new())),
                    NewKind::Html => vault
                        .create_note(&dir, &name, Format::Html, &ctx)
                        .map(|p| (p, Vec::new())),
                    NewKind::Template(t) => {
                        let t = RelPath::new(&t)?;
                        vault.create_from_template(&dir, &name, &t, &ctx)
                    }
                }
            })
            .await;
            match result {
                Ok(Ok((path, unknown))) => {
                    this.reload(Some(path.clone())).await;
                    if unknown.is_empty() {
                        this.toast(&format!("Se creó «{path}»"));
                    } else {
                        this.toast(&format!(
                            "Se creó «{path}»; la plantilla usa variables desconocidas: {}",
                            unknown.join(", ")
                        ));
                    }
                }
                Ok(Err(e)) => this.toast(&e.to_string()),
                Err(_) => this.toast("No se pudo crear la nota"),
            }
        });
    }

    /// «Diario de hoy»: lo abre, creándolo si no existe.
    fn open_journal(&self) {
        let Some(vault) = self.0.vault.borrow().clone() else {
            return;
        };
        let this = self.clone();
        glib::spawn_future_local(async move {
            let result = gio::spawn_blocking(move || {
                let ctx = write_ctx::now(Some(vault.config()));
                vault.open_journal(&ctx)
            })
            .await;
            match result {
                Ok(Ok(path)) => this.reload(Some(path)).await,
                Ok(Err(e)) => this.toast(&e.to_string()),
                Err(_) => this.toast("No se pudo abrir el diario"),
            }
        });
    }

    /// «Archivar» la nota abierta (o desarchivar, al deshacer).
    fn archive_current(&self) {
        let Some(entry) = self.0.current_entry.borrow().clone() else {
            return;
        };
        let this = self.clone();
        glib::spawn_future_local(async move { this.set_archived(entry.path.clone(), true).await });
    }

    async fn set_archived(&self, path: RelPath, archived: bool) {
        let Some(vault) = self.0.vault.borrow().clone() else {
            return;
        };
        let p = path.clone();
        let result = gio::spawn_blocking(move || {
            let ctx = write_ctx::now(Some(vault.config()));
            vault.archive(&p, archived, &ctx)
        })
        .await;
        match result {
            Ok(Ok(_)) if archived => {
                self.reload(None).await;
                let this = self.clone();
                self.toast_undo("Nota archivada", move || {
                    let (this, path) = (this.clone(), path.clone());
                    glib::spawn_future_local(async move { this.set_archived(path, false).await });
                });
            }
            Ok(Ok(_)) => self.reload(Some(path)).await,
            Ok(Err(e)) => self.toast(&e.to_string()),
            Err(_) => self.toast("No se pudo archivar"),
        }
    }

    /// Guarda archivos como adjuntos de la nota abierta y la vuelve a mostrar.
    async fn attach_files(&self, files: Vec<(String, Vec<u8>)>) {
        let Some(vault) = self.0.vault.borrow().clone() else {
            return;
        };
        let Some(note) = self
            .0
            .current_entry
            .borrow()
            .as_ref()
            .map(|e| e.path.clone())
        else {
            return self.toast("Abrí una nota antes de agregarle adjuntos");
        };
        let n = note.clone();
        let result = gio::spawn_blocking(move || {
            let ctx = write_ctx::now(Some(vault.config()));
            files
                .iter()
                .map(|(name, bytes)| vault.attach(&n, name, bytes, &ctx))
                .collect::<Result<Vec<_>, _>>()
        })
        .await;
        match result {
            Ok(Ok(saved)) => {
                self.reload(Some(note)).await;
                let names: Vec<&str> = saved.iter().map(RelPath::file_name).collect();
                self.toast(&format!("Adjunto guardado: {}", names.join(", ")));
            }
            Ok(Err(e)) => self.toast(&e.to_string()),
            Err(_) => self.toast("No se pudo guardar el adjunto"),
        }
    }

    /// «Pegar imagen como adjunto»: la imagen del portapapeles se guarda como PNG.
    fn paste_image(&self) {
        let this = self.clone();
        glib::spawn_future_local(async move {
            let clipboard = this.0.window.clipboard();
            let texture = match clipboard.read_texture_future().await {
                Ok(Some(t)) => t,
                _ => return this.toast("El portapapeles no tiene una imagen"),
            };
            let bytes = texture.save_to_png_bytes().to_vec();
            let ctx = write_ctx::now(None);
            let name = format!(
                "pegado-{}-{:02}{:02}{:02}.png",
                ctx.date, ctx.time.hour, ctx.time.minute, ctx.time.second
            );
            this.attach_files(vec![(name, bytes)]).await;
        });
    }

    /// Soltar archivos sobre la nota los guarda como adjuntos.
    fn setup_drop(&self, target: &impl IsA<gtk::Widget>) {
        let drop = gtk::DropTarget::new(
            gtk::gdk::FileList::static_type(),
            gtk::gdk::DragAction::COPY,
        );
        let this = self.clone();
        drop.connect_drop(move |_, value, _, _| {
            let Ok(list) = value.get::<gtk::gdk::FileList>() else {
                return false;
            };
            let paths: Vec<PathBuf> = list.files().iter().filter_map(|f| f.path()).collect();
            let this = this.clone();
            glib::spawn_future_local(async move {
                let read = gio::spawn_blocking(move || {
                    paths
                        .iter()
                        .filter_map(|p| {
                            let name = p.file_name()?.to_string_lossy().into_owned();
                            std::fs::read(p).ok().map(|b| (name, b))
                        })
                        .collect::<Vec<_>>()
                })
                .await
                .unwrap_or_default();
                if !read.is_empty() {
                    this.attach_files(read).await;
                }
            });
            true
        });
        target.add_controller(drop);
    }

    /// Otro programa cambió archivos: si de verdad cambió algo, vuelve a leer la bóveda y deja
    /// abierta la misma nota (FR-030). Como todavía no hay editor, nunca hay cambios locales que
    /// se puedan perder.
    fn on_disk_changes(&self, paths: Vec<PathBuf>) {
        let Some(vault) = self.0.vault.borrow().clone() else {
            return;
        };
        let this = self.clone();
        glib::spawn_future_local(async move {
            let changes = gio::spawn_blocking(move || vault.on_external_change(&paths))
                .await
                .unwrap_or_default();
            if changes.is_empty() {
                return;
            }
            let current = this
                .0
                .current_entry
                .borrow()
                .as_ref()
                .map(|e| e.path.clone());
            let removed = current.as_ref().is_some_and(|c| {
                changes
                    .iter()
                    .any(|ch| matches!(ch, orbital_vault::ExternalChange::Removed(p) if p == c))
            });
            if removed {
                this.reload(None).await;
                this.toast("La nota abierta se borró o se movió desde otro programa");
            } else {
                this.reload(current).await;
            }
        });
    }

    /// La carpeta de la bóveda desapareció: solo lectura hasta que vuelva.
    fn vault_gone(&self) {
        if self.0.gone.get() {
            return;
        }
        self.set_gone(true);
        let Some(root) = self
            .0
            .vault
            .borrow()
            .as_ref()
            .map(|v| v.root().to_path_buf())
        else {
            return;
        };
        let this = self.clone();
        glib::spawn_future_local(async move {
            // Revisa cada 2 s (fuera del hilo principal: un disco de red puede tardar) si volvió.
            loop {
                glib::timeout_future_seconds(2).await;
                let r = root.clone();
                if gio::spawn_blocking(move || r.is_dir())
                    .await
                    .unwrap_or(false)
                {
                    let current = this
                        .0
                        .current_entry
                        .borrow()
                        .as_ref()
                        .map(|e| e.path.clone());
                    this.reload(current).await;
                    this.toast("La bóveda volvió a estar disponible");
                    return;
                }
                if !this.0.gone.get() {
                    return;
                }
            }
        });
    }

    /// Activa o desactiva el modo «bóveda no disponible».
    fn set_gone(&self, gone: bool) {
        self.0.gone.set(gone);
        self.0.gone_banner.set_visible(gone);
        for name in [
            "rename",
            "trash",
            "archive",
            "paste-image",
            "new-note",
            "new-html",
            "journal",
            "new-from-template",
            "complete-type",
            "complete-structure",
        ] {
            if let Some(a) = self
                .0
                .window
                .lookup_action(name)
                .and_downcast::<gio::SimpleAction>()
            {
                a.set_enabled(!gone);
            }
        }
        if gone {
            self.0.list_view.announce(
                "La bóveda no está disponible",
                gtk::AccessibleAnnouncementPriority::High,
            );
        }
    }

    /// «Preferencias de la bóveda».
    fn show_preferences(&self) {
        let Some(vault) = self.0.vault.borrow().clone() else {
            return;
        };
        let this = self.clone();
        let current = self
            .0
            .current_entry
            .borrow()
            .as_ref()
            .map(|e| e.path.clone());
        crate::preferences::show(
            &self.0.window,
            vault.root().to_path_buf(),
            vault.config().clone(),
            move |r| {
                let (this, current) = (this.clone(), current.clone());
                glib::spawn_future_local(async move {
                    match r {
                        Ok(()) => {
                            this.reload(current).await;
                            this.toast("Preferencias guardadas");
                        }
                        Err(e) => {
                            this.toast(&format!("No se pudieron guardar las preferencias: {e}"))
                        }
                    }
                });
            },
        );
    }

    /// Avisa con un botón «Deshacer» que ejecuta `undo`.
    fn toast_undo(&self, msg: &str, undo: impl Fn() + 'static) {
        // Sin tiempo límite (WCAG 2.2.1): queda hasta que se descarta o lo reemplaza otro aviso.
        let toast = adw::Toast::builder()
            .title(msg)
            .button_label("Deshacer")
            .timeout(0)
            .priority(adw::ToastPriority::High)
            .build();
        toast.connect_button_clicked(move |_| undo());
        self.0.toasts.add_toast(toast);
    }

    /// «Renombrar o mover»: pide la ruta nueva (relativa a la bóveda).
    fn ask_rename(&self) {
        let Some(entry) = self.0.current_entry.borrow().clone() else {
            return;
        };
        let this = self.clone();
        glib::spawn_future_local(async move {
            let field = gtk::Entry::builder()
                .text(entry.path.as_str())
                .activates_default(true)
                .build();
            field.update_property(&[gtk::accessible::Property::Label("Ruta nueva")]);
            let dialog = adw::AlertDialog::new(
                Some("Renombrar o mover"),
                Some("Ruta dentro de la bóveda. Cambiá el nombre o la carpeta."),
            );
            dialog.set_extra_child(Some(&field));
            dialog.add_response("cancel", "Cancelar");
            dialog.add_response("rename", "Renombrar");
            dialog.set_response_appearance("rename", adw::ResponseAppearance::Suggested);
            dialog.set_default_response(Some("rename"));
            dialog.set_close_response("cancel");
            // Selecciona el nombre sin la extensión, que es lo que casi siempre se cambia.
            let path = entry.path.as_str().to_owned();
            let start = path.rfind('/').map_or(0, |p| p + 1);
            let end = path.rfind('.').filter(|e| *e > start).unwrap_or(path.len());
            let f = field.clone();
            glib::idle_add_local_once(move || {
                f.grab_focus();
                f.select_region(start as i32, end as i32);
            });
            if dialog.choose_future(Some(&this.0.window)).await != "rename" {
                return;
            }
            let Ok(to) = RelPath::new(field.text().trim()) else {
                return this.toast("La ruta nueva no es válida");
            };
            warn_portable(&this, to.file_name());
            if to == entry.path {
                return;
            }
            this.do_rename(entry.path.clone(), to, true).await;
        });
    }

    /// Renombra en segundo plano; si `undoable`, ofrece deshacer con el renombre inverso.
    async fn do_rename(&self, from: RelPath, to: RelPath, undoable: bool) {
        let Some(vault) = self.0.vault.borrow().clone() else {
            return;
        };
        let (f, t) = (from.clone(), to.clone());
        let result = gio::spawn_blocking(move || {
            let ctx = write_ctx::now(Some(vault.config()));
            vault.rename(&f, &t, &ctx)
        })
        .await;
        match result {
            Ok(Ok(_)) => {
                self.reload(Some(to.clone())).await;
                if undoable {
                    let this = self.clone();
                    self.toast_undo(
                        &format!("«{}» ahora es «{to}»", from.file_name()),
                        move || {
                            let (this, from, to) = (this.clone(), from.clone(), to.clone());
                            glib::spawn_future_local(async move {
                                this.do_rename(to, from, false).await;
                            });
                        },
                    );
                }
            }
            Ok(Err(e)) => self.toast(&e.to_string()),
            Err(_) => self.toast("No se pudo renombrar"),
        }
    }

    /// «Mover a la papelera», con confirmación.
    fn ask_trash(&self) {
        let Some(entry) = self.0.current_entry.borrow().clone() else {
            return;
        };
        let this = self.clone();
        glib::spawn_future_local(async move {
            let dialog = adw::AlertDialog::new(
                Some(&format!(
                    "¿Mover «{}» a la papelera?",
                    entry.path.file_name()
                )),
                Some("Vas a poder recuperarla con «Deshacer» o desde la papelera de GNOME."),
            );
            dialog.add_response("cancel", "Cancelar");
            dialog.add_response("trash", "Mover a la papelera");
            dialog.set_response_appearance("trash", adw::ResponseAppearance::Destructive);
            dialog.set_close_response("cancel");
            if dialog.choose_future(Some(&this.0.window)).await == "trash" {
                this.do_trash(entry.path.clone()).await;
            }
        });
    }

    async fn do_trash(&self, path: RelPath) {
        let Some(vault) = self.0.vault.borrow().clone() else {
            return;
        };
        let abs = path.to_path(vault.root());
        let (v, p) = (vault.clone(), path.clone());
        let plan = match gio::spawn_blocking(move || v.delete(&p)).await {
            Ok(Ok(plan)) => plan,
            Ok(Err(e)) => return self.toast(&e.to_string()),
            Err(_) => return self.toast("No se pudo borrar"),
        };
        // Papelera del sistema; si ese disco no tiene (tmpfs, FAT…), la papelera de la bóveda.
        let in_vault: Option<PathBuf> = match gio::File::for_path(&abs)
            .trash_future(glib::Priority::DEFAULT)
            .await
        {
            Ok(()) => None,
            Err(_) => {
                let (v, p) = (vault.clone(), path.clone());
                let moved = gio::spawn_blocking(move || {
                    let ctx = write_ctx::now(Some(v.config()));
                    v.move_to_vault_trash(&p, &ctx)
                })
                .await;
                match moved {
                    Ok(Ok(dest)) => Some(dest),
                    Ok(Err(e)) => return self.toast(&format!("No se pudo borrar: {e}")),
                    Err(_) => return self.toast("No se pudo borrar"),
                }
            }
        };
        let v = vault.clone();
        let for_log = plan.clone();
        let _ = gio::spawn_blocking(move || {
            let ctx = write_ctx::now(Some(v.config()));
            v.after_delete(&for_log, &ctx)
        })
        .await;
        self.reload(None).await;
        let msg = match &in_vault {
            None => format!("«{}» se movió a la papelera", path.file_name()),
            Some(_) => format!(
                "«{}» se movió a la papelera de la bóveda (.orbital/papelera)",
                path.file_name()
            ),
        };
        let this = self.clone();
        self.toast_undo(&msg, move || {
            let (this, plan, abs, in_vault) =
                (this.clone(), plan.clone(), abs.clone(), in_vault.clone());
            glib::spawn_future_local(async move {
                match in_vault {
                    None => this.restore(plan, abs).await,
                    Some(trashed) => this.restore_vault_trash(plan, trashed).await,
                }
            });
        });
    }

    /// Deshacer un borrado que fue a la papelera de la bóveda.
    async fn restore_vault_trash(&self, plan: DeletePlan, trashed: PathBuf) {
        let Some(vault) = self.0.vault.borrow().clone() else {
            return;
        };
        let path = plan.path.clone();
        let result = gio::spawn_blocking(move || {
            vault.restore_from(&trashed, &plan.path)?;
            let ctx = write_ctx::now(Some(vault.config()));
            vault.after_restore(&plan, &ctx)
        })
        .await;
        match result {
            Ok(Ok(_)) => self.reload(Some(path)).await,
            Ok(Err(e)) => self.toast(&format!("No se pudo recuperar: {e}")),
            Err(_) => self.toast("No se pudo recuperar"),
        }
    }

    /// Deshacer un borrado: busca el archivo en la papelera por su ruta original y lo devuelve.
    async fn restore(&self, plan: DeletePlan, abs: PathBuf) {
        let path = plan.path.clone();
        let Some(vault) = self.0.vault.borrow().clone() else {
            return;
        };
        match find_in_trash(&abs).await {
            Some(trashed) => {
                let dest = gio::File::for_path(&abs);
                // Mover desde la papelera es local y rápido, pero igual va fuera del hilo principal.
                let moved = gio::spawn_blocking(move || {
                    trashed.move_(
                        &dest,
                        gio::FileCopyFlags::NONE,
                        None::<&gio::Cancellable>,
                        None,
                    )
                })
                .await;
                match moved {
                    Ok(Ok(())) => {}
                    Ok(Err(e)) => return self.toast(&format!("No se pudo recuperar: {e}")),
                    Err(_) => return self.toast("No se pudo recuperar"),
                }
                let v = vault.clone();
                let _ = gio::spawn_blocking(move || {
                    let ctx = write_ctx::now(Some(v.config()));
                    v.after_restore(&plan, &ctx)
                })
                .await;
                self.reload(Some(path)).await;
            }
            None => self.toast("No encontré el archivo en la papelera"),
        }
    }

    /// Muestra una nota por su ruta relativa (para el modo de captura).
    pub fn show_path(&self, rel: &str) {
        // Elegir la fila la abre (ver `selection-changed`); si no está en la lista (por ejemplo,
        // archivada), se muestra igual.
        if !self.reveal(rel)
            && let Some(e) = self.entry(rel)
        {
            self.show_entry(e);
        }
    }

    /// Expande las carpetas hasta `rel` y elige su fila. Devuelve si la encontró.
    fn reveal(&self, rel: &str) -> bool {
        let Some(selection) = self
            .0
            .list_view
            .model()
            .and_downcast::<gtk::SingleSelection>()
        else {
            return false;
        };
        let find = |path: &str| -> Option<(u32, gtk::TreeListRow)> {
            (0..selection.n_items()).find_map(|i| {
                let row = selection.item(i).and_downcast::<gtk::TreeListRow>()?;
                let item = row.item().and_downcast::<gtk::StringObject>()?;
                (item.string() == path).then_some((i, row))
            })
        };
        // Cada carpeta antepasada, de afuera hacia adentro.
        let mut prefix = String::new();
        let parts: Vec<&str> = rel.split('/').collect();
        for part in &parts[..parts.len().saturating_sub(1)] {
            if !prefix.is_empty() {
                prefix.push('/');
            }
            prefix.push_str(part);
            if let Some((_, row)) = find(&prefix) {
                row.set_expanded(true);
            }
        }
        match find(rel) {
            Some((pos, _)) => {
                selection.set_selected(pos);
                self.0
                    .list_view
                    .scroll_to(pos, gtk::ListScrollFlags::NONE, None);
                true
            }
            None => false,
        }
    }

    /// Ejecuta `f` cuando termina de dibujarse el próximo cuadro de la ventana.
    pub fn after_next_frame(&self, f: impl FnOnce() + 'static) {
        let f = std::cell::Cell::new(Some(f));
        self.0.window.add_tick_callback(move |_, _| {
            if let Some(f) = f.take() {
                glib::idle_add_local_once(f);
            }
            glib::ControlFlow::Break
        });
    }

    /// Dibuja la ventana en un PNG, sin depender de capturas del compositor.
    pub fn capture(&self, out: &Path) -> Result<(), String> {
        let w = &self.0.window;
        let (width, height) = (w.width(), w.height());
        let paintable = gtk::WidgetPaintable::new(Some(w));
        let snapshot = gtk::Snapshot::new();
        paintable.snapshot(&snapshot, f64::from(width), f64::from(height));
        let node = snapshot.to_node().ok_or("la ventana no dibujó nada")?;
        let renderer = w.renderer().ok_or("la ventana no tiene renderizador")?;
        let bounds = gtk::graphene::Rect::new(0.0, 0.0, width as f32, height as f32);
        let texture = renderer.render_texture(node, Some(&bounds));
        texture.save_to_png(out).map_err(|e| e.to_string())
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
    menu_button(
        "folder-open-symbolic",
        "Abrir",
        &[
            ("Abrir una carpeta…", "win.open", "Ctrl+O"),
            (
                "Abrir como bóveda de Orbital…",
                "win.open-as",
                "Ctrl+Mayús+O",
            ),
            ("Preferencias de la bóveda", "win.preferences", "Ctrl+,"),
            ("Atajos de teclado", "win.shortcuts", "Ctrl+?"),
        ],
    )
}

/// Botón con un menú hecho de botones planos.
///
/// Los menús de `gio::Menu` en GTK 4.24 no tienen nombre accesible en sus ítems (Orca no puede
/// leerlos); con botones comunes, cada opción se anuncia con su texto. El atajo se muestra a la
/// derecha y se agrega a la descripción accesible.
fn menu_button(icon: &str, tooltip: &str, items: &[(&str, &str, &str)]) -> gtk::MenuButton {
    let popover = gtk::Popover::builder().has_arrow(true).build();
    popover.add_css_class("menu");
    let items: Vec<MenuItem> = items
        .iter()
        .map(|&(l, a, k)| MenuItem::new(l, a, k))
        .collect();
    fill_menu(&popover, &items);
    let button = gtk::MenuButton::builder()
        .icon_name(icon)
        .tooltip_text(tooltip)
        .popover(&popover)
        .build();
    // Sin esto, el nombre accesible se arma con el texto de las opciones del menú.
    button.update_property(&[gtk::accessible::Property::Label(tooltip)]);
    button
}

/// Una opción de menú: texto, acción, atajo y, si hace falta, el parámetro de la acción.
struct MenuItem {
    label: String,
    action: String,
    accel: String,
    target: Option<String>,
}

impl MenuItem {
    fn new(label: &str, action: &str, accel: &str) -> Self {
        Self {
            label: label.to_owned(),
            action: action.to_owned(),
            accel: accel.to_owned(),
            target: None,
        }
    }
}

/// (Re)arma el contenido de un menú.
fn fill_menu(popover: &gtk::Popover, items: &[MenuItem]) {
    let column = gtk::Box::new(gtk::Orientation::Vertical, 0);
    column.add_css_class("menu");
    for item in items {
        let text = gtk::Label::builder()
            .label(&item.label)
            .xalign(0.0)
            .hexpand(true)
            .build();
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 24);
        row.append(&text);
        if !item.accel.is_empty() {
            let shortcut = gtk::Label::new(Some(&item.accel));
            shortcut.add_css_class("dim-label");
            row.append(&shortcut);
        }
        let button = gtk::Button::builder()
            .child(&row)
            .action_name(&item.action)
            .css_classes(["flat"])
            .build();
        if let Some(t) = &item.target {
            button.set_action_target_value(Some(&t.to_variant()));
        }
        button.update_property(&[gtk::accessible::Property::Label(&item.label)]);
        if !item.accel.is_empty() {
            button.update_property(&[gtk::accessible::Property::KeyShortcuts(&item.accel)]);
        }
        let p = popover.clone();
        button.connect_clicked(move |_| p.popdown());
        column.append(&button);
    }
    popover.set_child(Some(&column));
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

/// Texto legible de un valor de propiedad.
fn display(v: &PropValue) -> String {
    match v {
        PropValue::Null => "—".to_owned(),
        PropValue::Text(s)
        | PropValue::Link(s)
        | PropValue::Number(s)
        | PropValue::Date(s)
        | PropValue::DateTime(s) => s.clone(),
        PropValue::Bool(true) => "Sí".to_owned(),
        PropValue::Bool(false) => "No".to_owned(),
        PropValue::List(items) => items.iter().map(display).collect::<Vec<_>>().join(", "),
        PropValue::Map(items) => items
            .iter()
            .map(|(k, v)| format!("{k}: {}", display(v)))
            .collect::<Vec<_>>()
            .join(" · "),
    }
}

/// El archivo más reciente de la papelera cuya ruta original es `original`.
async fn find_in_trash(original: &Path) -> Option<gio::File> {
    let trash = gio::File::for_uri("trash:///");
    let enumerator = trash
        .enumerate_children_future(
            "standard::name,trash::orig-path,trash::deletion-date",
            gio::FileQueryInfoFlags::NONE,
            glib::Priority::DEFAULT,
        )
        .await
        .ok()?;
    let mut best: Option<(String, gio::File)> = None;
    loop {
        let infos = enumerator
            .next_files_future(64, glib::Priority::DEFAULT)
            .await
            .ok()?;
        if infos.is_empty() {
            break;
        }
        for info in infos {
            let orig = info.attribute_byte_string("trash::orig-path");
            if orig.as_deref().map(Path::new) != Some(original) {
                continue;
            }
            let date = info
                .attribute_string("trash::deletion-date")
                .map(|d| d.to_string())
                .unwrap_or_default();
            if best.as_ref().is_none_or(|(d, _)| date > *d) {
                best = Some((date, trash.child(info.name())));
            }
        }
    }
    best.map(|(_, f)| f)
}

/// Corre en segundo plano: árbol de la lista (sin las notas archivadas) y plantillas.
fn prepare(v: Vault) -> (Vault, Tree, NoteMeta) {
    let meta = v.note_meta();
    let archived: std::collections::HashSet<&RelPath> = meta.archived.iter().collect();
    let visible: Vec<Entry> = v
        .entries()
        .iter()
        .filter(|e| !archived.contains(&e.path))
        .cloned()
        .collect();
    let tree = build_tree(&visible);
    (v, tree, meta)
}

/// Qué clase de nota se crea.
enum NewKind {
    Md,
    Html,
    /// Ruta de la plantilla.
    Template(String),
}

/// Caracteres que funcionan en Linux pero rompen la sincronización con Windows y algunas nubes.
const NOT_PORTABLE: &[char] = &[':', '?', '*', '<', '>', '|', '"', '\\'];

/// Avisa (sin impedirlo) si un nombre tiene caracteres que dan problemas en otros sistemas.
fn warn_portable(window: &Window, name: &str) {
    let bad: String = name.chars().filter(|c| NOT_PORTABLE.contains(c)).collect();
    if !bad.is_empty() {
        window.toast(&format!(
            "«{name}» tiene {bad}, que puede dar problemas al sincronizar con Windows o con la nube"
        ));
    }
}
