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
use orbital_vault::props::{PropValue, Warning};
use orbital_vault::{DeletePlan, Entry, EntryKind, Format, Note, RelPath, Vault};
use webkit6::prelude::*;

use crate::html_view;
use crate::open_dialog::{self, Answer};
use crate::state::AppState;
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
    html_page: gtk::Box,
    /// Vista web compartida; `None` hasta abrir la primera nota HTML.
    web: RefCell<Option<webkit6::WebView>>,
    vault: RefCell<Option<Arc<Vault>>>,
    tree: RefCell<Rc<Tree>>,
    /// Si es `false` (modo de captura), no se guarda el estado de la app.
    persist: Cell<bool>,
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

        let content_title = adw::WindowTitle::new("", "");
        let content_header = adw::HeaderBar::new();
        content_header.set_title_widget(Some(&content_title));
        let note_menu = gio::Menu::new();
        note_menu.append(Some("Renombrar o mover…"), Some("win.rename"));
        note_menu.append(Some("Mover a la papelera"), Some("win.trash"));
        let note_menu_button = gtk::MenuButton::builder()
            .icon_name("view-more-symbolic")
            .tooltip_text("Opciones de la nota")
            .menu_model(&note_menu)
            .build();
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
            content_stack,
            props_list,
            note_banner,
            type_banner,
            current_note: RefCell::new(None),
            current_entry: RefCell::new(None),
            html_props,
            html_page,
            web: RefCell::new(None),
            vault: RefCell::new(None),
            tree: RefCell::new(Rc::new(Tree::new())),
            persist: Cell::new(true),
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
            orbital_vault::apply_open_as(&for_apply, &plan, &ctx).map(|v| {
                let tree = build_tree(v.entries());
                (v, tree)
            })
        })
        .await;
        match result {
            Ok(Ok((vault, tree))) => {
                self.show_vault(vault, tree);
                self.remember(path);
            }
            Ok(Err(e)) => self.toast(&format!("No se pudo crear la estructura: {e}")),
            Err(_) => self.toast("No se pudo crear la estructura"),
        }
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
                self.remember(path);
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
        i.banner.set_visible(incomplete);
        for w in vault.warnings() {
            self.toast(w);
        }

        let tree = Rc::new(tree);
        *i.tree.borrow_mut() = tree.clone();
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
            } else if entry.kind.is_text() {
                let abs = entry.path.to_path(vault.root());
                gio::spawn_blocking(move || read_text(&abs))
                    .await
                    .unwrap_or_else(|_| Err("No se pudo leer el archivo".into()))
                    .map(|t| {
                        this.0.editor.buffer().set_text(&t);
                        None
                    })
            } else {
                Err("Este archivo no es una nota de texto".into())
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
            match loaded {
                Ok(Some(note)) => this.show_note(note),
                Ok(None) => i.content_stack.set_visible_child_name("nota"),
                Err(msg) => {
                    i.editor.buffer().set_text(&msg);
                    i.content_stack.set_visible_child_name("nota");
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
                fill_props(list, &block.entries);
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
        let result = gio::spawn_blocking(move || {
            orbital_vault::open(&root).map(|v| {
                let tree = build_tree(v.entries());
                (v, tree)
            })
        })
        .await;
        if let Ok(Ok((vault, tree))) = result {
            self.show_vault(vault, tree);
            if let Some(p) = show {
                self.show_path(p.as_str());
            }
        }
    }

    /// Avisa con un botón «Deshacer» que ejecuta `undo`.
    fn toast_undo(&self, msg: &str, undo: impl Fn() + 'static) {
        let toast = adw::Toast::builder()
            .title(msg)
            .button_label("Deshacer")
            .timeout(8)
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
        if let Some(e) = self.entry(rel) {
            self.show_entry(e);
        }
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

/// Llena una lista con las propiedades de una nota.
fn fill_props(list: &gtk::ListBox, entries: &[orbital_vault::props::PropEntry]) {
    for e in entries {
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
