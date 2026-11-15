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

use crate::editor::props::display;
use crate::editor::{self, Editor};
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
    split: adw::OverlaySplitView,
    status: crate::status_bar::StatusBar,
    /// Sección «Recientes» del explorador y sus rutas (las cinco últimas notas abiertas).
    recent_list: gtk::Box,
    panel: crate::panel_derecho::PanelDerecho,
    pestanas: crate::pestanas::Pestanas,
    /// Estado de vista para reponer cuando cargue la nota de esa pestaña.
    estado_pendiente: RefCell<Option<(RelPath, crate::pestanas::Estado)>>,
    recientes: RefCell<Vec<String>>,
    /// Lo que muestra la sección ahora: se reconstruye solo si cambia (reabrir la misma nota, o
    /// recargar la lista por un cambio en disco, no toca los widgets).
    recientes_vistos: RefCell<Option<Vec<String>>>,
    /// «Cambiar orden» del explorador: árbol en orden inverso.
    tree_reversed: Cell<bool>,
    sidebar_title: adw::WindowTitle,
    content_title: adw::WindowTitle,
    banner: adw::Banner,
    list_view: gtk::ListView,
    editor: gtk::TextView,
    content_stack: gtk::Stack,
    code_editor: Editor,
    /// Subtítulo de la nota sin el estado del guardado.
    subtitle_base: RefCell<String>,
    /// La ventana ya guardó lo pendiente y puede cerrarse.
    closing: Cell<bool>,
    /// Evita abrir más de un diálogo de conflicto a la vez.
    conflict_dialog_open: Cell<bool>,
    /// Escrituras propias recientes (ruta → hash), que el monitor no debe tomar como externas.
    own_writes: RefCell<HashMap<RelPath, u64>>,
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
    /// Panel lateral derecho con los retroenlaces (US4 de la 003).
    right: crate::editor::right_sidebar::RightSidebar,
    right_split: adw::OverlaySplitView,
    /// Grafo de enlaces de la bóveda; `None` mientras se arma en segundo plano.
    graph: RefCell<Option<Arc<std::sync::Mutex<orbital_vault::LinkGraph>>>>,
    /// Cada bóveda nueva sube la generación: un grafo viejo que termina tarde se descarta.
    graph_gen: Cell<u64>,
    /// Línea a mostrar cuando termine de abrirse la nota (al elegir un retroenlace).
    pending_line: Cell<Option<(RelPath, usize)>>,
    /// Panel lateral izquierdo: stack de páginas (Archivos, Búsqueda, Etiquetas).
    sidebar_stack: adw::ViewStack,
    /// Panel de búsqueda en toda la bóveda (US1 de la 004).
    search_panel: crate::search_panel::SearchPanel,
    /// Índice de búsqueda en memoria; `None` mientras se arma en segundo plano.
    search_index: RefCell<Option<Arc<std::sync::RwLock<orbital_vault::SearchIndex>>>>,
    /// Generación del índice para descartar construcciones viejas.
    search_gen: Cell<u64>,
    tag_panel: crate::tag_panel::TagPanel,
    search_updates: RefCell<HashMap<RelPath, bool>>,
    search_updating: Cell<bool>,
    tag_operation: Cell<bool>,
    vault_session: Cell<u64>,
    tag_disk_changes: RefCell<Vec<PathBuf>>,
    /// Candidatos para el selector rápido de notas (US2 de la 004).
    candidates: RefCell<Vec<crate::editor::completion::CompletionCandidate>>,
    /// Vistas WebKit de exportación a PDF en curso (007): mantenerlas vivas hasta terminar.
    exports_pendientes: RefCell<Vec<webkit6::WebView>>,
    /// Datos y vista del grafo de enlaces (008).
    grafo_datos: RefCell<Option<std::rc::Rc<crate::grafo::GrafoDatos>>>,
    grafo_vista: crate::grafo::VistaGrafo,
    grafo_cerrar: gtk::Button,
    /// Presentación OnStage de la nota abierta (014): se crea con la ventana, oculta.
    onstage: std::rc::Rc<crate::onstage::OnStage>,
    /// Archivos externos por URL (009): vista del archivo y webview de la URL asociada.
    externo_ruta: RefCell<Option<RelPath>>,
    asociar_url: gtk::Button,
    quitar_url: gtk::Button,
    /// Importar a Markdown (015): sensible solo para extensiones soportadas.
    importar_md: gtk::Button,
    /// Ver el PDF (016): visible solo para `.pdf` en la vista externa.
    ver_pdf: gtk::Button,
    /// Visor PDF embebido (016).
    visor: std::rc::Rc<crate::visor_pdf::VisorPdf>,
    /// El PDF mostrado en el visor (para la búsqueda de resaltados).
    visor_ruta: RefCell<Option<RelPath>>,
    /// Webview de la URL asociada (009): se crea al primer uso (crear un `WebView` cuesta
    /// cientos de milisegundos de arranque) y va en `url_page`, debajo de su cabecera.
    url_web: RefCell<Option<webkit6::WebView>>,
    url_page: gtk::Box,
    url_abrir: gtk::Button,
    url_ver_archivo: gtk::Button,
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
        // Editor de notas (002): Código, Lectura y Dividida, con guardado automático.
        let code_editor = Editor::new();
        code_editor.switcher().set_visible(false);
        content_stack.add_named(code_editor.widget(), Some("codigo"));
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
        let asociar_url = gtk::Button::builder()
            .label("Asociar URL…")
            .action_name("win.asociar-url")
            .css_classes(["pill", "suggested-action"])
            .halign(gtk::Align::Center)
            .build();
        let quitar_url = gtk::Button::builder()
            .label("Quitar URL")
            .action_name("win.quitar-url")
            .css_classes(["pill"])
            .halign(gtk::Align::Center)
            .visible(false)
            .build();
        let importar_md = gtk::Button::builder()
            .label("Importar a Markdown")
            .action_name("win.importar-md")
            .css_classes(["pill"])
            .halign(gtk::Align::Center)
            .tooltip_text("Crea una nota Markdown en wiki/ (PDF, DOCX o HTML)")
            .build();
        let ver_pdf = gtk::Button::builder()
            .label("Ver el PDF")
            .action_name("win.ver-pdf")
            .css_classes(["pill", "suggested-action"])
            .halign(gtk::Align::Center)
            .visible(false)
            .build();
        let externo_botones = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(12)
            .valign(gtk::Align::Center)
            .build();
        externo_botones.append(&open_external);
        externo_botones.append(&asociar_url);
        externo_botones.append(&quitar_url);
        externo_botones.append(&importar_md);
        externo_botones.append(&ver_pdf);
        let external_page = adw::StatusPage::builder()
            .icon_name("document-open-symbolic")
            .child(&externo_botones)
            .build();
        content_stack.add_named(&external_page, Some("externo"));

        // Webview embebido de la URL asociada (009), con botón para volver al archivo.
        let url_header = adw::HeaderBar::new();
        let url_ver_archivo = gtk::Button::builder()
            .label("Ver el archivo")
            .css_classes(["flat"])
            .build();
        let url_abrir = gtk::Button::builder()
            .icon_name("external-link-symbolic")
            .tooltip_text("Abrir en el navegador")
            .css_classes(["flat"])
            .build();
        url_header.pack_start(&url_ver_archivo);
        url_header.pack_end(&url_abrir);
        let url_page = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .build();
        url_page.append(&url_header);
        content_stack.add_named(&url_page, Some("url-webview"));

        // Grafo de enlaces (008): página a pantalla completa con el canvas propio.
        let grafo_vista = crate::grafo::VistaGrafo::new();
        let grafo_aviso = gtk::Label::builder()
            .label("")
            .xalign(0.0)
            .margin_start(12)
            .css_classes(["dim-label", "caption"])
            .build();
        let grafo_cerrar = gtk::Button::builder()
            .icon_name("window-close-symbolic")
            .tooltip_text("Cerrar el grafo (Esc)")
            .css_classes(["flat"])
            .build();
        let grafo_barra = adw::HeaderBar::new();
        grafo_barra.set_title_widget(Some(&adw::WindowTitle::new(
            "Grafo de enlaces de la bóveda",
            "",
        )));
        grafo_barra.pack_end(&grafo_cerrar);
        grafo_barra.pack_start(&grafo_aviso);
        let grafo_page = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .build();
        grafo_page.append(&grafo_barra);
        grafo_page.append(&grafo_vista);
        grafo_vista.set_vexpand(true);
        content_stack.add_named(&grafo_page, Some("grafo"));

        let content_title = adw::WindowTitle::new("", "");
        // Barra de la nota (025): debajo de las pestañas, sin controles de ventana (los lleva la
        // cabecera única de arriba).
        let content_header = adw::HeaderBar::builder()
            .show_start_title_buttons(false)
            .show_end_title_buttons(false)
            .css_classes(["flat", "orbital-note-toolbar"])
            .build();
        // Ruta de la nota a la izquierda (maqueta: `note-path`); el subtítulo de `content_title`
        // ya la arma con «solo lectura» y el estado del guardado.
        let note_path = gtk::Label::builder()
            .xalign(0.0)
            .hexpand(true)
            .ellipsize(gtk::pango::EllipsizeMode::Middle)
            .css_classes(["orbital-note-path"])
            .build();
        content_title
            .bind_property("subtitle", &note_path, "label")
            .sync_create()
            .build();
        content_title
            .bind_property("subtitle", &note_path, "tooltip-text")
            .sync_create()
            .build();
        content_header.set_title_widget(Some(&gtk::Box::new(gtk::Orientation::Horizontal, 0)));
        content_header.pack_start(&note_path);
        let note_menu_button = menu_button(
            "view-more-symbolic",
            "Más opciones de la nota",
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
        note_menu_button.set_child(Some(&crate::iconos::Icono::new("mas", 18)));
        note_menu_button.add_css_class("flat");
        note_menu_button.add_css_class("orbital-iconbtn");
        content_header.pack_end(&note_menu_button);
        // Botón de vista de la maqueta: muestra el modo actual y pasa al siguiente (Código →
        // Lectura → Dividida en `.md`; Código ↔ Visual en `.html`). El grupo del editor sigue
        // siendo el estado (y lo usan Ctrl+1/2/3), pero ya no se muestra.
        let view_icon = crate::iconos::Icono::new("codigo", 18);
        let view_button = gtk::Button::builder()
            .child(&view_icon)
            .css_classes(["flat", "orbital-iconbtn"])
            .build();
        {
            let group = code_editor.switcher().clone();
            view_button.connect_clicked(move |_| {
                let n = group.n_toggles();
                if n > 0 {
                    group.set_active((group.active() + 1) % n);
                }
            });
            let (icon, button) = (view_icon.clone(), view_button.clone());
            let poner = move |g: &adw::ToggleGroup| {
                let name = g.active_name();
                let (glifo, nombre) = match name.as_deref() {
                    Some("lectura") => ("archivo", "Lectura"),
                    Some("dividida") => ("barra-izquierda", "Pantalla dividida"),
                    Some("visual") => ("archivo", "Visual"),
                    _ => ("codigo", "Código"),
                };
                icon.set_nombre(glifo);
                button.set_tooltip_text(Some(&format!(
                    "Modo de vista: {nombre} (clic para cambiar)"
                )));
                button.update_property(&[gtk::accessible::Property::Label(&format!(
                    "Modo de vista: {nombre}. Cambiar al siguiente"
                ))]);
            };
            poner(code_editor.switcher());
            code_editor.switcher().connect_active_name_notify(poner);
            code_editor
                .switcher()
                .bind_property("visible", &view_button, "visible")
                .sync_create()
                .build();
        }
        content_header.pack_end(&view_button);
        let content_view = adw::ToolbarView::new();
        // Pestañas de notas (025) arriba de la barra de la nota.
        let pestanas = crate::pestanas::Pestanas::new();
        content_view.add_top_bar(pestanas.barra());
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
        // Panel derecho de retroenlaces, debajo de la cabecera de la nota (Ctrl+Mayús+B).
        let right = crate::editor::right_sidebar::RightSidebar::new();
        // Barra derecha de tres modos (025): Propiedades, Contexto (los retroenlaces) y Grafo.
        let panel = crate::panel_derecho::PanelDerecho::new(&right);
        let right_split = adw::OverlaySplitView::builder()
            .content(&content_stack)
            .sidebar(panel.widget())
            .sidebar_position(gtk::PackType::End)
            .show_sidebar(true)
            .min_sidebar_width(280.0)
            .max_sidebar_width(280.0)
            .sidebar_width_fraction(1.0)
            .build();
        right_split.add_css_class("orbital-right");
        let backlinks_button =
            crate::iconos::boton_alternable("barra-derecha", "Mostrar barra derecha");
        // La barra manda el estado inicial al botón (no al revés).
        right_split
            .bind_property("show-sidebar", &backlinks_button, "active")
            .bidirectional()
            .sync_create()
            .build();
        nombre_de_barra(&backlinks_button, "barra derecha", "Ctrl+]");
        content_view.set_content(Some(&right_split));

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

        // --- Panel de búsqueda (US1) ---
        let initial_search_raw = AppState::load().search_raw;
        let search_panel = crate::search_panel::SearchPanel::new(initial_search_raw);

        let tag_panel = crate::tag_panel::TagPanel::new();
        // --- Explorador (025): secciones plegables Explorador, Etiquetas, Recientes y Favoritos ---
        // El árbol sigue siendo un `ListView` virtual con su propio desplazamiento (con 5000 notas
        // no puede ir dentro de un scroll de altura natural); las otras secciones van debajo, en
        // un panel redimensionable.
        let (files_section, _) = seccion_plegable("Explorador", &sidebar_scroll, true, true);
        sidebar_scroll.set_vexpand(true);
        tag_panel.widget().set_height_request(150);
        let (tags_section, _) = seccion_plegable("Etiquetas", tag_panel.widget(), true, false);
        let recent_list = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .css_classes(["orbital-recent"])
            .build();
        recent_list.update_property(&[gtk::accessible::Property::Label("Notas recientes")]);
        let (recent_section, _) = seccion_plegable("Recientes", &recent_list, true, false);
        let favorites_empty = gtk::Label::builder()
            .label("Pendiente: todavía no se pueden marcar notas favoritas")
            .wrap(true)
            .xalign(0.0)
            .css_classes(["orbital-count"])
            .build();
        let (favorites_section, favorites_toggle) =
            seccion_plegable("Favoritos", &favorites_empty, false, false);
        favorites_toggle.set_sensitive(false);
        // Las secciones de abajo comparten un desplazamiento que puede encogerse: el alto
        // mínimo de la barra no depende de cuántas etiquetas o notas recientes haya (si no, la
        // ventana se pasa de alto y GTK la desacomoda).
        let lower = gtk::Box::new(gtk::Orientation::Vertical, 0);
        lower.append(&tags_section);
        lower.append(&recent_section);
        lower.append(&favorites_section);
        let lower_scroll = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .propagate_natural_height(true)
            .max_content_height(260)
            .child(&lower)
            .build();
        let explorer = gtk::Box::new(gtk::Orientation::Vertical, 0);
        files_section.set_vexpand(true);
        explorer.append(&files_section);
        explorer.append(&lower_scroll);

        let sidebar_stack = adw::ViewStack::new();
        sidebar_stack.add_named(&explorer, Some("archivos"));
        sidebar_stack.add_named(search_panel.widget(), Some("busqueda"));

        let banner = adw::Banner::builder()
            .title("Estructura incompleta")
            .button_label("Completar…")
            .action_name("win.complete-structure")
            .revealed(true)
            .visible(false)
            .build();

        let sidebar_title = adw::WindowTitle::new("Orbital", "");
        sidebar_title.set_halign(gtk::Align::Start);
        // Cabecera de acciones del explorador (maqueta: crear nota, crear carpeta, cambiar orden,
        // mostrar archivo actual, colapsar carpetas).
        let new_button = menu_button("list-add-symbolic", "Crear nota", &[]);
        new_button.set_child(Some(&crate::iconos::Icono::new("crear-nota", 18)));
        new_button.add_css_class("flat");
        new_button.add_css_class("orbital-iconbtn");
        let left_head = gtk::Box::builder()
            .css_classes(["orbital-left-head"])
            .homogeneous(true)
            .build();
        left_head.append(&new_button);
        for (icono, nombre, accion) in [
            ("crear-carpeta", "Crear carpeta", "win.new-folder"),
            ("ordenar", "Cambiar orden", "win.sort-tree"),
            ("ubicar", "Mostrar archivo actual", "win.reveal-current"),
            ("colapsar", "Colapsar carpetas", "win.collapse-folders"),
        ] {
            let b = crate::iconos::boton(icono, nombre);
            b.set_action_name(Some(accion));
            left_head.append(&b);
        }
        // Pie: la bóveda abierta y el menú para cambiarla.
        let vault_button = menu_button(
            "folder-open-symbolic",
            "Cambiar de bóveda",
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
        );
        vault_button.set_child(Some(&crate::iconos::Icono::new("cambiar", 18)));
        vault_button.add_css_class("flat");
        vault_button.add_css_class("orbital-iconbtn");
        vault_button.set_direction(gtk::ArrowType::Up);
        let vault_footer = gtk::Box::builder()
            .spacing(8)
            .css_classes(["orbital-vault-footer"])
            .build();
        let vault_mark = gtk::Label::builder()
            .label("◈")
            .css_classes(["orbital-pink"])
            .build();
        vault_mark.set_accessible_role(gtk::AccessibleRole::Presentation);
        sidebar_title.set_hexpand(true);
        vault_footer.append(&vault_mark);
        vault_footer.append(&sidebar_title);
        vault_footer.append(&vault_button);
        let sidebar_view = adw::ToolbarView::new();
        sidebar_view.add_css_class("orbital-left");
        sidebar_view.add_top_bar(&left_head);
        sidebar_view.add_top_bar(&banner);
        sidebar_view.add_bottom_bar(&vault_footer);
        let gone_banner = adw::Banner::builder()
            .title("La bóveda no está disponible. Orbital no va a escribir nada.")
            .revealed(true)
            .visible(false)
            .build();
        sidebar_view.add_top_bar(&gone_banner);
        sidebar_view.set_content(Some(&sidebar_stack));

        // --- Esqueleto de la interfaz v13 (025) ---
        // Barra izquierda (260 px) y barra derecha (280 px) plegables; en ventanas angostas se
        // superponen al centro (breakpoint de abajo).
        let split = adw::OverlaySplitView::builder()
            .sidebar(&sidebar_view)
            .content(&content_view)
            .min_sidebar_width(260.0)
            .max_sidebar_width(260.0)
            .sidebar_width_fraction(1.0)
            .build();
        split.add_css_class("orbital-split");
        let left_toggle =
            crate::iconos::boton_alternable("barra-izquierda", "Ocultar barra izquierda");
        // La barra manda el estado inicial al botón (no al revés).
        split
            .bind_property("show-sidebar", &left_toggle, "active")
            .bidirectional()
            .sync_create()
            .build();
        nombre_de_barra(&left_toggle, "barra izquierda", "Ctrl+[");

        // Modos de la barra izquierda, alineados sobre ella: Explorador, Búsqueda y Guardados
        // (pendiente: guardar notas todavía no existe).
        let modes = gtk::Box::builder()
            .spacing(8)
            .css_classes(["orbital-left-modes"])
            .build();
        modes.update_property(&[gtk::accessible::Property::Label("Modo del panel izquierdo")]);
        let mode_files = crate::iconos::boton_alternable("crear-carpeta", "Explorador de archivos");
        let mode_search = crate::iconos::boton_alternable("buscar", "Búsqueda");
        mode_search.set_group(Some(&mode_files));
        let mode_saved = crate::iconos::boton_alternable("marcador", "Guardados (pendiente)");
        mode_saved.set_sensitive(false);
        mode_files.set_active(true);
        for (b, page) in [(&mode_files, "archivos"), (&mode_search, "busqueda")] {
            let stack = sidebar_stack.clone();
            b.connect_toggled(move |b| {
                if b.is_active() {
                    stack.set_visible_child_name(page);
                }
            });
        }
        {
            let (f, se) = (mode_files.clone(), mode_search.clone());
            sidebar_stack.connect_visible_child_name_notify(move |st| {
                match st.visible_child_name().as_deref() {
                    Some("busqueda") => se.set_active(true),
                    _ => f.set_active(true),
                }
            });
        }
        modes.append(&mode_files);
        modes.append(&mode_search);
        modes.append(&mode_saved);
        split
            .bind_property("show-sidebar", &modes, "visible")
            .sync_create()
            .build();

        let brand = gtk::Box::builder()
            .spacing(9)
            .css_classes(["orbital-brand"])
            .build();
        let planeta = crate::iconos::Icono::new("planeta", 24);
        planeta.add_css_class("orbital-planet");
        brand.append(&planeta);
        brand.append(&gtk::Label::new(Some("orbital")));
        brand.update_property(&[gtk::accessible::Property::Label("Orbital")]);

        let main_header = adw::HeaderBar::builder()
            .css_classes(["orbital-top"])
            .build();
        let left_slot = gtk::Box::builder()
            .width_request(58)
            .halign(gtk::Align::Start)
            .build();
        left_toggle.set_halign(gtk::Align::Center);
        left_toggle.set_hexpand(true);
        left_slot.append(&left_toggle);
        main_header.pack_start(&left_slot);
        main_header.pack_start(&modes);
        main_header.set_title_widget(Some(&brand));
        main_header.pack_end(&backlinks_button);

        // Riel de navegación (58 px): acciones reales; Canva y DB desactivados hasta su spec.
        let rail = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(8)
            .css_classes(["orbital-rail"])
            .build();
        rail.update_property(&[gtk::accessible::Property::Label("Navegación principal")]);
        for (icono, nombre, accion) in [
            ("selector", "Selector rápido", Some("win.quick-open")),
            ("red", "Conexiones", Some("win.ver-grafo")),
            ("canva", "Crear canva (todavía no disponible)", None),
            ("db", "Crear base de datos (todavía no disponible)", None),
            (
                "comandos",
                "Paleta de comandos",
                Some("win.command-palette"),
            ),
        ] {
            let b = crate::iconos::boton(icono, nombre);
            match accion {
                Some(a) => b.set_action_name(Some(a)),
                None => b.set_sensitive(false),
            }
            rail.append(&b);
        }
        let settings = crate::iconos::boton("configuracion", "Configuración general");
        settings.set_action_name(Some("win.preferences"));
        settings.set_vexpand(true);
        settings.set_valign(gtk::Align::End);
        rail.append(&settings);

        // Barra inferior plegable: versión, totales de la bóveda y contadores de la nota.
        let status = crate::status_bar::StatusBar::new();

        let workspace = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        workspace.append(&rail);
        split.set_hexpand(true);
        workspace.append(&split);
        let root_view = adw::ToolbarView::builder()
            .top_bar_style(adw::ToolbarStyle::Flat)
            .bottom_bar_style(adw::ToolbarStyle::Flat)
            .build();
        root_view.add_top_bar(&main_header);
        let overlay = gtk::Overlay::builder().child(&workspace).build();
        overlay.add_overlay(status.tab());
        root_view.set_content(Some(&overlay));
        root_view.add_bottom_bar(status.widget());

        // --- Pantalla inicial ---
        let start = start_page();

        // Homogéneo a propósito: la primera disposición de la interfaz de la bóveda ocurre
        // mientras la bóveda se lee en otro hilo (sin homogéneo, se suma después: +60 ms).
        let stack = gtk::Stack::builder()
            .transition_type(gtk::StackTransitionType::Crossfade)
            .build();
        stack.add_named(&start, Some("inicio"));
        stack.add_named(&root_view, Some("boveda"));

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

        // Anchos de la maqueta: 260/280 px y, en ventanas más chicas, 220/220 y 190/170. La
        // maqueta lo hace a 900 y 680 px; acá el centro tiene un ancho mínimo real (la columna del
        // editor y las propiedades), así que los cortes van antes y, si no entra, la barra
        // derecha y después la izquierda se superponen al centro en vez de recortarlo.
        let ancho_maximo = |px: f64| {
            adw::Breakpoint::new(adw::BreakpointCondition::new_length(
                adw::BreakpointConditionLengthType::MaxWidth,
                px,
                adw::LengthUnit::Px,
            ))
        };
        for (px, izq, der) in [(1180.0, 220.0, 220.0), (1060.0, 190.0, 170.0)] {
            let bp = ancho_maximo(px);
            for (vista, ancho) in [(&split, izq), (&right_split, der)] {
                bp.add_setter(vista, "min-sidebar-width", Some(&ancho.to_value()));
                bp.add_setter(vista, "max-sidebar-width", Some(&ancho.to_value()));
            }
            window.add_breakpoint(bp);
        }
        let bp = ancho_maximo(1040.0);
        bp.add_setter(&right_split, "collapsed", Some(&true.to_value()));
        bp.add_setter(&split, "min-sidebar-width", Some(&190.0.to_value()));
        bp.add_setter(&split, "max-sidebar-width", Some(&190.0.to_value()));
        window.add_breakpoint(bp);
        let bp = ancho_maximo(760.0);
        bp.add_setter(&split, "collapsed", Some(&true.to_value()));
        bp.add_setter(&right_split, "collapsed", Some(&true.to_value()));
        window.add_breakpoint(bp);

        // OnStage (014): ventana separada, oculta hasta «Presentar con OnStage». El material
        // vivo (texto de la nota y bóveda) lo arma la ventana en cada refresco.
        let onstage = std::rc::Rc::new(crate::onstage::OnStage::new(&window));

        // Visor PDF (016): página del stack; el webview y el esquema se crean al primer uso.
        let visor = std::rc::Rc::new(crate::visor_pdf::VisorPdf::new());
        content_stack.add_named(visor.page(), Some("pdf"));

        let this = Self(Rc::new(Inner {
            window,
            toasts,
            stack,
            split,
            status,
            recent_list,
            panel,
            pestanas,
            estado_pendiente: RefCell::new(None),
            recientes: RefCell::new(Vec::new()),
            recientes_vistos: RefCell::new(None),
            tree_reversed: Cell::new(false),
            sidebar_title,
            content_title,
            banner,
            list_view,
            editor,
            content_stack,
            code_editor,
            subtitle_base: RefCell::new(String::new()),
            closing: Cell::new(false),
            conflict_dialog_open: Cell::new(false),
            own_writes: RefCell::new(HashMap::new()),
            props_list,
            note_banner,
            type_banner,
            current_note: RefCell::new(None),
            current_entry: RefCell::new(None),
            right,
            right_split,
            graph: RefCell::new(None),
            graph_gen: Cell::new(0),
            pending_line: Cell::new(None),
            sidebar_stack,
            search_panel: search_panel.clone(),
            search_index: RefCell::new(None),
            search_gen: Cell::new(0),
            tag_panel,
            search_updates: RefCell::new(HashMap::new()),
            search_updating: Cell::new(false),
            tag_operation: Cell::new(false),
            vault_session: Cell::new(0),
            tag_disk_changes: RefCell::new(Vec::new()),
            candidates: RefCell::new(Vec::new()),
            exports_pendientes: RefCell::new(Vec::new()),
            grafo_datos: RefCell::new(None),
            grafo_vista,
            grafo_cerrar,
            onstage,
            externo_ruta: RefCell::new(None),
            asociar_url,
            quitar_url,
            importar_md,
            ver_pdf,
            visor,
            visor_ruta: RefCell::new(None),
            url_web: RefCell::new(None),
            url_page: url_page.clone(),
            url_abrir,
            url_ver_archivo,
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

        let weak = Rc::downgrade(&this.0);
        this.0.tag_panel.connect_rename({
            let weak = weak.clone();
            move |tag| {
                if let Some(i) = weak.upgrade() {
                    Window(i).ask_tag_rename(tag.to_owned());
                }
            }
        });
        this.0.tag_panel.connect_selected(move |tag| {
            if let Some(i) = weak.upgrade() {
                Window(i).search_tag(tag);
            }
        });
        let weak = Rc::downgrade(&this.0);
        this.0.search_panel.connect_open_match(move |path, line| {
            let Some(i) = weak.upgrade() else { return };
            let w = Window(i);
            w.0.pending_line.set(Some((path.clone(), line)));
            w.show_path(path.as_str());
        });

        let weak = Rc::downgrade(&this.0);
        this.0.search_panel.connect_raw_toggled(move |active| {
            let Some(i) = weak.upgrade() else { return };
            if i.persist.get() {
                let mut s = AppState::load();
                s.search_raw = active;
                glib::spawn_future_local(async move {
                    let _ = gio::spawn_blocking(move || s.save()).await;
                });
            }
        });

        // Archivos externos por URL (009): asociar/quitar/abrir y volver al archivo.
        {
            let weak = Rc::downgrade(&this.0);
            this.0.asociar_url.connect_clicked(move |_| {
                let Some(i) = weak.upgrade() else { return };
                let Some(ruta) = i.externo_ruta.borrow().clone() else {
                    return;
                };
                let w = Window(i);
                w.asociar_url_dialog(&ruta);
            });
            let weak = Rc::downgrade(&this.0);
            this.0.quitar_url.connect_clicked(move |_| {
                let Some(i) = weak.upgrade() else { return };
                let Some(ruta) = i.externo_ruta.borrow().clone() else {
                    return;
                };
                let w = Window(i.clone());
                let Some(vault) = i.vault.borrow().clone() else {
                    return;
                };
                glib::spawn_future_local(async move {
                    let (v, r) = (vault, ruta.clone());
                    let resultado =
                        gio::spawn_blocking(move || v.quitar_url(&r, &write_ctx::now(None))).await;
                    match resultado {
                        Ok(Ok(())) => {
                            i.content_stack.set_visible_child_name("externo");
                        }
                        _ => w.toast("No se pudo quitar la URL"),
                    }
                });
            });
            let weak = Rc::downgrade(&this.0);
            this.0.url_ver_archivo.connect_clicked(move |_| {
                if let Some(i) = weak.upgrade() {
                    i.content_stack.set_visible_child_name("externo");
                }
            });
            let weak = Rc::downgrade(&this.0);
            this.0.url_abrir.connect_clicked(move |_| {
                let Some(i) = weak.upgrade() else { return };
                let Some(ruta) = i.externo_ruta.borrow().clone() else {
                    return;
                };
                let Some(vault) = i.vault.borrow().clone() else {
                    return;
                };
                glib::spawn_future_local(async move {
                    let url = gio::spawn_blocking(move || vault.url_de(&ruta))
                        .await
                        .ok()
                        .flatten();
                    if let Some(url) = url {
                        gtk::UriLauncher::new(&url).launch(
                            None::<&gtk::Window>,
                            None::<&gio::Cancellable>,
                            |_| {},
                        );
                    }
                });
            });
        }

        // Grafo (008): clic abre la nota (cierra el grafo), Esc/botón vuelve, aviso de recorte.
        {
            let weak = Rc::downgrade(&this.0);
            this.0.grafo_vista.set_on_abrir(move |ruta| {
                if let Some(i) = weak.upgrade() {
                    i.content_stack.set_visible_child_name("codigo");
                    Window(i).show_path(ruta);
                }
            });
            let weak_cerrar = Rc::downgrade(&this.0);
            this.0.grafo_vista.set_on_cerrar(move || {
                if let Some(i) = weak_cerrar.upgrade() {
                    i.content_stack.set_visible_child_name("codigo");
                }
            });
            let weak_boton = Rc::downgrade(&this.0);
            this.0.grafo_cerrar.connect_clicked(move |_| {
                if let Some(i) = weak_boton.upgrade() {
                    i.content_stack.set_visible_child_name("codigo");
                }
            });
        }

        this.setup_actions(app);
        let content = this.0.content_stack.clone();
        this.setup_drop(&content);
        this.setup_raw_drop();
        this.setup_editor();
        this.setup_list();
        this.setup_status_counts();
        this.setup_pestanas();
        {
            let weak = Rc::downgrade(&this.0);
            this.0.panel.connect_modo(move |_| {
                if let Some(i) = weak.upgrade() {
                    Window(i).refresh_panel();
                }
            });
            let weak = Rc::downgrade(&this.0);
            this.0.panel.connect_abrir(move |ruta| {
                if let Some(i) = weak.upgrade() {
                    Window(i).show_path(ruta);
                }
            });
        }
        this
    }

    pub fn present(&self) {
        self.0.window.present();
    }

    /// Medición del arranque (desarrollo): fases del próximo cuadro.
    pub fn fases_del_proximo_cuadro(&self) {
        crate::medir_arranque::fases_del_proximo_cuadro(&self.0.window);
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
            simple("view-code", |w| {
                w.0.code_editor.set_view(crate::editor::View::Codigo)
            }),
            simple("view-read", |w| {
                w.0.code_editor.set_view(crate::editor::View::Lectura)
            }),
            simple("view-split", |w| {
                w.0.code_editor.set_view(crate::editor::View::Dividida)
            }),
            simple("backlinks", |w| {
                let s = &w.0.right_split;
                s.set_show_sidebar(!s.shows_sidebar());
            }),
            simple("toggle-left", |w| {
                let s = &w.0.split;
                s.set_show_sidebar(!s.shows_sidebar());
            }),
            simple("toggle-right", |w| {
                let s = &w.0.right_split;
                s.set_show_sidebar(!s.shows_sidebar());
            }),
            simple("new-folder", Window::ask_new_folder),
            simple("sort-tree", Window::sort_tree),
            simple("reveal-current", |w| {
                if let Some(e) = w.0.current_entry.borrow().clone() {
                    w.reveal(e.path.as_str());
                }
            }),
            simple("collapse-folders", Window::collapse_folders),
            simple("search", Window::focus_search),
            simple("toggle-search-raw", Window::toggle_search_raw),
            simple("find", |w| w.0.code_editor.open_find(false)),
            simple("replace", |w| w.0.code_editor.open_find(true)),
            simple("undo", |w| {
                w.0.code_editor.undo();
            }),
            simple("quick-open", Window::open_switcher),
            simple("command-palette", Window::open_command_palette),
            simple("exportar-html", |w| {
                w.exportar(crate::export::Formato::Html)
            }),
            simple("exportar-pdf", |w| w.exportar(crate::export::Formato::Pdf)),
            simple("ver-grafo", Window::ver_grafo),
            simple("cerrar-grafo", Window::cerrar_grafo),
            simple("historial", Window::historial),
            simple("card-link", Window::card_link),
            simple("onstage", Window::presentar),
            simple("importar-md", Window::importar_md),
            simple("ver-pdf", Window::ver_pdf_accion),
            simple("test-pdf-seleccion", |w: &Window| {
                w.0.visor.seleccion_de_prueba(1, 0, 6, "Informe");
            }),
            simple("renumerar-notas-al-pie", |w| {
                if w.0.code_editor.current_path().is_none() {
                    w.toast("Abrí una nota antes de renumerar");
                    return;
                }
                let text = w.0.code_editor.text();
                let Some(nuevo) = crate::editor::notas_al_pie::renumerar(&text) else {
                    w.toast("Las notas al pie ya están en orden");
                    return;
                };
                w.0.code_editor.replace_text(&nuevo);
                w.toast("Notas al pie renumeradas (guardá para persistir)");
            }),
            simple("asociar-url", |w| {
                let Some(ruta) = w.0.externo_ruta.borrow().clone() else {
                    w.toast("Abrí un archivo de raw/ antes de asociar una URL");
                    return;
                };
                w.asociar_url_dialog(&ruta);
            }),
            simple("quitar-url", |w| {
                let Some(ruta) = w.0.externo_ruta.borrow().clone() else {
                    return;
                };
                let w = w.clone();
                let i = w.0.clone();
                let Some(v) = i.vault.borrow().clone() else {
                    return;
                };
                glib::spawn_future_local(async move {
                    let r = ruta.clone();
                    let resultado =
                        gio::spawn_blocking(move || v.quitar_url(&r, &write_ctx::now(None))).await;
                    match resultado {
                        Ok(Ok(())) => {
                            i.content_stack.set_visible_child_name("externo");
                        }
                        _ => w.toast("No se pudo quitar la URL"),
                    }
                });
            }),
            simple("new-note", |w| w.ask_new_note(NewKind::Md)),
            simple("new-html", |w| w.ask_new_note(NewKind::Html)),
            simple("journal", Window::open_journal),
            simple("archive", Window::archive_current),
            simple("paste-image", Window::paste_image),
            simple("open-missing", Window::test_open_missing),
            from_template,
        ]);
        app.set_accels_for_action("win.new-note", &["<Control>n"]);
        app.set_accels_for_action("win.preferences", &["<Control>comma"]);
        app.set_accels_for_action("win.view-code", &["<Control>1"]);
        app.set_accels_for_action("win.view-read", &["<Control>2"]);
        app.set_accels_for_action("win.view-split", &["<Control>3"]);
        app.set_accels_for_action("win.search", &["<Control><Shift>f"]);
        app.set_accels_for_action("win.find", &["<Control>f"]);
        app.set_accels_for_action("win.replace", &["<Control>h"]);
        app.set_accels_for_action("win.undo", &["<Control>z"]);
        app.set_accels_for_action("win.quick-open", &["<Control>p"]);
        app.set_accels_for_action("win.command-palette", &["<Control><Shift>p"]);
        app.set_accels_for_action("win.exportar-html", &["<Control><Shift>e"]);
        app.set_accels_for_action("win.backlinks", &["<Control><Shift>b"]);
        app.set_accels_for_action("win.toggle-left", &["<Control>bracketleft"]);
        app.set_accels_for_action("win.toggle-right", &["<Control>bracketright"]);
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
        section.add(adw::ShortcutsItem::new("Vista Código", "<Control>1"));
        section.add(adw::ShortcutsItem::new("Vista Lectura", "<Control>2"));
        section.add(adw::ShortcutsItem::new("Vista Dividida", "<Control>3"));
        section.add(adw::ShortcutsItem::new(
            "Buscar en toda la bóveda",
            "<Control><Shift>f",
        ));
        section.add(adw::ShortcutsItem::new(
            "Buscar en la nota abierta",
            "<Control>f",
        ));
        section.add(adw::ShortcutsItem::new(
            "Reemplazar en la nota abierta",
            "<Control>h",
        ));
        section.add(adw::ShortcutsItem::new(
            "Saltar a una nota (selector rápido)",
            "<Control>p",
        ));
        section.add(adw::ShortcutsItem::new(
            "Paleta de comandos",
            "<Control><Shift>p",
        ));
        section.add(adw::ShortcutsItem::new(
            "Cursor extra con Alt+clic o Alt+arrastre",
            "<Alt>click",
        ));
        section.add(adw::ShortcutsItem::new(
            "Cursor por columnas",
            "<Control><Alt>Up",
        ));
        section.add(adw::ShortcutsItem::new("Volver a un solo cursor", "Escape"));
        section.add(adw::ShortcutsItem::new(
            "Exportar la nota como HTML",
            "<Control><Shift>e",
        ));
        section.add(adw::ShortcutsItem::new(
            "Mostrar u ocultar los retroenlaces",
            "<Control><Shift>b",
        ));
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
                crate::medir_arranque::marca("bóveda leída");
                self.show_vault(vault, tree, meta);
                self.remember(path);
            }
            Ok(Err(e)) => self.toast(&format!("No se pudo abrir la bóveda: {e}")),
            Err(_) => self.toast("No se pudo abrir la bóveda"),
        }
    }

    fn show_vault(&self, vault: Vault, tree: Tree, meta: NoteMeta) {
        if self
            .0
            .vault
            .borrow()
            .as_ref()
            .is_none_or(|current| current.root() != vault.root())
        {
            self.0.vault_session.set(self.0.vault_session.get() + 1);
            self.0.pestanas.limpiar();
        }
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
        let arc_vault = Arc::new(vault);
        *i.vault.borrow_mut() = Some(arc_vault.clone());
        i.search_panel
            .set_vault(Some(arc_vault.clone()), i.vault_session.get());
        i.search_gen.set(i.search_gen.get() + 1);
        i.search_updates.borrow_mut().clear();
        i.search_updating.set(false);
        *i.search_index.borrow_mut() = None;
        i.search_panel.set_search_index(None);
        i.code_editor.set_search_index(None);
        i.tag_panel.set_tree(None);
        i.code_editor.set_tag_candidates(Vec::new());

        crate::medir_arranque::marca("show_vault: hasta candidatos");
        let baseline = Self::collect_candidates_baseline(&arc_vault);
        crate::medir_arranque::marca("show_vault: candidatos");
        *i.candidates.borrow_mut() = baseline;
        let vault_for_cand = arc_vault.clone();
        let this_cand = self.clone();
        glib::spawn_future_local(async move {
            let full = gio::spawn_blocking(move || Self::collect_candidates_full(&vault_for_cand))
                .await
                .ok();
            if let Some(full) = full {
                *this_cand.0.candidates.borrow_mut() = full;
            }
        });

        self.rebuild_graph();
        crate::medir_arranque::marca("show_vault: grafo pedido");
        let this = self.clone();
        let generation = i.search_gen.get();
        self.after_next_frame(move || {
            if this.0.search_gen.get() == generation {
                this.rebuild_search_index();
            }
        });

        // Totales de la barra inferior (maqueta: notas, carpetas y archivos).
        let (mut notas, mut carpetas, mut archivos) = (0, 0, 0);
        for e in arc_vault.entries() {
            match e.kind {
                EntryKind::Folder => carpetas += 1,
                EntryKind::AtomNote | EntryKind::SynthesisNote => {
                    notas += 1;
                    archivos += 1;
                }
                _ => archivos += 1,
            }
        }
        i.status.set_totales(notas, carpetas, archivos);
        crate::medir_arranque::marca("show_vault: totales");
        *i.recientes.borrow_mut() = AppState::load().recent_notes_for(arc_vault.root()).to_vec();
        self.refresh_recent();
        crate::medir_arranque::marca("show_vault: recientes");
        self.set_list_model(tree);
        crate::medir_arranque::marca("show_vault: lista");

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
        self.guardar_estado_pestana();
        let this = self.clone();
        glib::spawn_future_local(async move {
            // Antes de cambiar de nota, se guarda lo pendiente de la anterior (FR-002).
            let editor = this.0.code_editor.clone();
            if editor.current_path().is_some_and(|p| p != entry.path) {
                editor.flush().await;
                if editor.has_pending() {
                    return;
                }
                editor.close();
            }
            let is_note = Format::from_name(entry.path.file_name()).is_some();
            let loaded = if is_note {
                let (v, p) = (vault.clone(), entry.path.clone());
                match gio::spawn_blocking(move || v.read_note(&p)).await {
                    Ok(Ok(note)) => Ok(Some(note)),
                    Ok(Err(e)) => Err(e.to_string()),
                    Err(_) => Err("No se pudo leer la nota".to_owned()),
                }
            } else if entry.path.as_str().ends_with(".pdf") {
                // Un PDF va a la vista externa con «Ver el PDF» (016): nunca se muestra como
                // texto, aunque el archivo sea ASCII limpio.
                Err("pdf".to_owned())
            } else {
                // Cualquier otro archivo: si es texto UTF-8 se muestra; si no, se ofrece abrirlo afuera.
                // Los NUL interiores panean `set_text`: se recortan.
                let abs = entry.path.to_path(vault.root());
                gio::spawn_blocking(move || read_text(&abs).map(|t| t.replace('\0', "")))
                    .await
                    .unwrap_or_else(|_| Err("No se pudo leer el archivo".into()))
                    .map(|t| {
                        this.0.editor.buffer().set_text(&t);
                        None
                    })
            };
            let i = &this.0;
            *i.current_entry.borrow_mut() = Some(entry.clone());
            i.pestanas
                .abrir(entry.path.as_str(), entry.path.file_name());
            this.refresh_backlinks();
            i.content_title.set_title(entry.path.file_name());
            let mut subtitle = entry.path.to_string();
            if entry.read_only {
                subtitle.push_str(" · solo lectura");
            }
            i.content_title.set_subtitle(&subtitle);
            *i.subtitle_base.borrow_mut() = subtitle;
            this.clear_note_view();
            let raw = entry.zone == orbital_vault::Zone::Raw;
            if raw {
                // La importación (015) y la asociación de URL operan sobre el archivo de raw/
                // abierto, sea que se muestre en el editor o en la vista externa.
                *i.externo_ruta.borrow_mut() = Some(entry.path.clone());
            }
            match loaded {
                Ok(Some(note)) => {
                    {
                        let mut r = this.0.recientes.borrow_mut();
                        let rel = note.path.as_str();
                        r.retain(|p| p != rel);
                        r.insert(0, rel.to_owned());
                    }
                    this.refresh_recent();
                    if this.0.persist.get() {
                        let rel = note.path.as_str().to_string();
                        let vroot = vault.root().to_path_buf();
                        glib::spawn_future_local(async move {
                            let _ = gio::spawn_blocking(move || {
                                let mut s = AppState::load();
                                s.note_opened(&vroot, &rel);
                                s.save()
                            })
                            .await;
                        });
                    }
                    this.show_note(note);
                }
                Ok(None) => i.content_stack.set_visible_child_name("nota"),
                Err(_) => {
                    i.external_page.set_title(entry.path.file_name());
                    i.external_page.set_description(Some(if raw {
                        "Fuente original, solo lectura. Orbital no la modifica."
                    } else {
                        "Este archivo no es una nota de texto."
                    }));
                    let url = if raw {
                        let v = vault.clone();
                        let p = entry.path.clone();
                        gio::spawn_blocking(move || v.url_de(&p))
                            .await
                            .ok()
                            .flatten()
                    } else {
                        None
                    };
                    *i.externo_ruta.borrow_mut() = Some(entry.path.clone());
                    i.quitar_url.set_visible(url.is_some());
                    // Importar a Markdown solo se ofrece para PDF, DOCX y HTML.
                    let extension = entry
                        .path
                        .file_name()
                        .rsplit_once('.')
                        .map(|(_, e)| e.to_ascii_lowercase())
                        .unwrap_or_default();
                    i.importar_md
                        .set_visible(orbital_vault::importar::formato_de_ext(&extension).is_some());
                    i.ver_pdf.set_visible(extension == "pdf");
                    match url {
                        Some(url) => {
                            Window::url_web(i).load_uri(url.as_str());
                            i.content_stack.set_visible_child_name("url-webview");
                        }
                        None => i.content_stack.set_visible_child_name("externo"),
                    }
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
        });
    }

    fn clear_note_view(&self) {
        let i = &self.0;
        i.code_editor.switcher().set_visible(false);
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
            if note.format == Format::Md && !note.read_only {
                // Las propiedades inválidas se corrigen en la vista Código (FR-018).
                self.open_in_code(&note);
            } else {
                i.editor.buffer().set_text(&note.source);
                i.content_stack.set_visible_child_name("nota");
            }
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
                Format::Md if !note.read_only => self.open_in_code(&note),
                Format::Md => {
                    i.editor.buffer().set_text(note.body());
                    i.content_stack.set_visible_child_name("nota");
                }
                // Las notas `.html` editables van al editor (Visual y Código, US4); las de `raw/`
                // se muestran como página de solo lectura.
                Format::Html if !note.read_only => self.open_in_code(&note),
                Format::Html => {
                    self.load_html(&note);
                    i.content_stack.set_visible_child_name("html");
                }
            }
        }
        *i.current_note.borrow_mut() = Some(note);
        self.refresh_panel();
    }

    /// Abre una nota `.md` en la vista Código. Si ya está abierta (por ejemplo, al recargar la
    /// lista), no se recarga el texto: se conservan el cursor, la selección y el deshacer.
    fn open_in_code(&self, note: &Note) {
        let i = &self.0;
        if i.code_editor.current_path().as_ref() != Some(&note.path)
            && let Some(vault) = i.vault.borrow().clone()
        {
            i.code_editor.load(vault, note);
        }
        // Las propiedades se ven en el texto (Código) o arriba de la vista formateada.
        i.props_list.set_visible(false);
        i.code_editor.switcher().set_visible(true);
        i.content_stack.set_visible_child_name("codigo");
        if let Some((path, line)) = i.pending_line.take() {
            if path == note.path {
                i.code_editor.reveal_line(line);
            } else {
                i.pending_line.set(Some((path, line)));
            }
        }
        let pendiente = i.estado_pendiente.borrow_mut().take();
        if let Some((path, (cursor, scroll, vista))) = pendiente
            && path == note.path
        {
            i.code_editor
                .restaurar_vista(cursor, scroll, vista.as_deref());
        }
    }

    /// Arma el modelo del árbol de la bóveda (con el orden elegido) y conserva las carpetas que
    /// estaban abiertas.
    fn set_list_model(&self, tree: Rc<Tree>) {
        let i = &self.0;
        // Pestañas de notas que ya no existen (borradas, archivadas o renombradas afuera).
        {
            let this = self.clone();
            i.pestanas.quitar(move |clave| this.entry(clave).is_none());
        }
        let reversed = i.tree_reversed.get();
        let root = children_model(&tree, "", reversed);
        let tree_model = gtk::TreeListModel::new(root, false, false, move |item| {
            let path = item.downcast_ref::<gtk::StringObject>()?.string();
            tree.get(path.as_str())
                .map(|_| children_model(&tree, path.as_str(), reversed).upcast::<gio::ListModel>())
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
        // Las carpetas que estaban abiertas siguen abiertas después de recargar.
        let expanded: std::collections::HashSet<String> = i
            .list_view
            .model()
            .map(|old| {
                (0..old.n_items())
                    .filter_map(|n| old.item(n).and_downcast::<gtk::TreeListRow>())
                    .filter(|r| r.is_expanded())
                    .filter_map(|r| r.item().and_downcast::<gtk::StringObject>())
                    .map(|o| o.string().to_string())
                    .collect()
            })
            .unwrap_or_default();
        i.list_view.set_model(Some(&selection));
        let mut n = 0;
        while n < selection.n_items() {
            if let Some(row) = selection.item(n).and_downcast::<gtk::TreeListRow>()
                && let Some(o) = row.item().and_downcast::<gtk::StringObject>()
                && expanded.contains(o.string().as_str())
            {
                row.set_expanded(true);
            }
            n += 1;
        }
    }

    /// Contadores de la barra inferior: se recalculan 150 ms después del último cambio de texto o
    /// de selección (nunca en cada tecla sobre todo el texto: rompería el presupuesto por tecla).
    fn setup_status_counts(&self) {
        let generation = Rc::new(Cell::new(0u64));
        let schedule = {
            let weak = Rc::downgrade(&self.0);
            let generation = generation.clone();
            move || {
                let g = generation.get() + 1;
                generation.set(g);
                let weak = weak.clone();
                let generation = generation.clone();
                glib::timeout_add_local_once(std::time::Duration::from_millis(150), move || {
                    if generation.get() != g {
                        return;
                    }
                    let Some(i) = weak.upgrade() else { return };
                    let b = i.code_editor.buffer();
                    let (start, end) = b.bounds();
                    let total = crate::status_bar::contar(&b.text(&start, &end, false));
                    let seleccion = b
                        .selection_bounds()
                        .map(|(a, z)| crate::status_bar::contar(&b.text(&a, &z, false)));
                    i.status.set_contadores(total, seleccion);
                    let en_editor = i.current_entry.borrow().as_ref().map(|e| &e.path)
                        == i.code_editor.current_path().as_ref();
                    if en_editor {
                        i.panel.set_palabras(total.1);
                    }
                });
            }
        };
        let buffer = self.0.code_editor.buffer();
        let s = schedule.clone();
        buffer.connect_changed(move |_| s());
        let s = schedule.clone();
        buffer.connect_has_selection_notify(move |_| s());
        // Extender una selección mueve la marca sin cambiar `has-selection`.
        buffer.connect_mark_set(move |b, _, mark| {
            if b.has_selection() && (mark == &b.selection_bound() || mark == &b.get_insert()) {
                schedule();
            }
        });
    }

    /// Sección «Recientes» del explorador: las cinco últimas notas abiertas.
    fn refresh_recent(&self) {
        let top: Vec<String> = self.0.recientes.borrow().iter().take(5).cloned().collect();
        if self.0.recientes_vistos.borrow().as_ref() == Some(&top) {
            return;
        }
        *self.0.recientes_vistos.borrow_mut() = Some(top);
        let list = &self.0.recent_list;
        while let Some(c) = list.first_child() {
            list.remove(&c);
        }
        let recientes = self.0.recientes.borrow();
        if recientes.is_empty() {
            list.append(
                &gtk::Label::builder()
                    .label("Sin notas recientes")
                    .xalign(0.0)
                    .css_classes(["orbital-count"])
                    .build(),
            );
            return;
        }
        for ruta in recientes.iter().take(5) {
            let nombre = ruta.rsplit('/').next().unwrap_or(ruta);
            let b = gtk::Button::builder()
                .child(
                    &gtk::Label::builder()
                        .label(nombre)
                        .xalign(0.0)
                        .ellipsize(gtk::pango::EllipsizeMode::End)
                        .build(),
                )
                .tooltip_text(ruta.as_str())
                .css_classes(["flat", "orbital-list-item"])
                .build();
            b.update_property(&[gtk::accessible::Property::Label(&format!("Abrir {nombre}"))]);
            let weak = Rc::downgrade(&self.0);
            let ruta = ruta.clone();
            b.connect_clicked(move |_| {
                if let Some(i) = weak.upgrade() {
                    Window(i).show_path(&ruta);
                }
            });
            list.append(&b);
        }
    }

    /// «Cambiar orden» (maqueta): invierte el orden del árbol.
    fn sort_tree(&self) {
        self.0.tree_reversed.set(!self.0.tree_reversed.get());
        let tree = self.0.tree.borrow().clone();
        self.set_list_model(tree);
    }

    /// «Colapsar carpetas» (maqueta): si hay carpetas abiertas, las cierra todas; si no, abre
    /// todas.
    fn collapse_folders(&self) {
        let Some(model) = self.0.list_view.model() else {
            return;
        };
        let rows = || {
            (0..model.n_items()).filter_map(|n| model.item(n).and_downcast::<gtk::TreeListRow>())
        };
        if rows().any(|r| r.is_expanded()) {
            // De abajo hacia arriba: cerrar una carpeta saca a sus hijas de la lista.
            for n in (0..model.n_items()).rev() {
                if let Some(r) = model.item(n).and_downcast::<gtk::TreeListRow>() {
                    r.set_expanded(false);
                }
            }
        } else {
            let mut n = 0;
            while n < model.n_items() {
                if let Some(r) = model.item(n).and_downcast::<gtk::TreeListRow>()
                    && r.is_expandable()
                {
                    r.set_expanded(true);
                }
                n += 1;
            }
        }
    }

    /// «Crear carpeta»: pide el nombre y la crea donde se crean las notas nuevas.
    fn ask_new_folder(&self) {
        let Some(vault) = self.0.vault.borrow().clone() else {
            return;
        };
        let this = self.clone();
        glib::spawn_future_local(async move {
            let field = gtk::Entry::builder().activates_default(true).build();
            field.update_property(&[gtk::accessible::Property::Label("Nombre de la carpeta")]);
            let dialog = adw::AlertDialog::new(Some("Carpeta nueva"), Some("¿Cómo se llama?"));
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
            if name.is_empty() || name.contains('/') || name.starts_with('.') {
                return this.toast("El nombre no puede estar vacío, tener «/» ni empezar con «.»");
            }
            warn_portable(&this, &name);
            let dir = this
                .target_dir()
                .map_or_else(|| "wiki".to_owned(), |d| d.as_str().to_owned());
            let abs = vault.root().join(&dir).join(&name);
            let result = gio::spawn_blocking(move || std::fs::create_dir(abs)).await;
            match result {
                Ok(Ok(())) => this.toast(&format!("Carpeta «{name}» creada en {dir}/")),
                Ok(Err(e)) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                    this.toast(&format!("Ya existe «{name}» en {dir}/"))
                }
                _ => this.toast("No se pudo crear la carpeta"),
            }
        });
    }

    /// El webview de la URL asociada (009), creado al primer uso con una sesión de red efímera:
    /// nada de lo que cargue (cookies, caché, almacenamiento) queda en disco.
    fn url_web(i: &Inner) -> webkit6::WebView {
        if let Some(web) = i.url_web.borrow().as_ref() {
            return web.clone();
        }
        let session = webkit6::NetworkSession::new_ephemeral();
        let web = webkit6::WebView::builder()
            .network_session(&session)
            .vexpand(true)
            .hexpand(true)
            .build();
        i.url_page.append(&web);
        *i.url_web.borrow_mut() = Some(web.clone());
        web
    }

    /// Arma el grafo de enlaces de la bóveda en segundo plano (US4; 136 ms con 5000 notas).
    fn rebuild_graph(&self) {
        let i = &self.0;
        let Some(vault) = i.vault.borrow().clone() else {
            return;
        };
        let generation = i.graph_gen.get() + 1;
        i.graph_gen.set(generation);
        *i.graph.borrow_mut() = None;
        self.refresh_backlinks();
        let this = self.clone();
        glib::spawn_future_local(async move {
            let built = gio::spawn_blocking(move || orbital_vault::LinkGraph::build(&vault))
                .await
                .ok()
                .and_then(Result::ok);
            if this.0.graph_gen.get() != generation {
                return;
            }
            let nuevo = built.map(|g| Arc::new(std::sync::Mutex::new(g)));
            *this.0.graph.borrow_mut() = nuevo.clone();
            this.refresh_backlinks();
            this.reconstruir_grafo_visual(nuevo.as_ref());
        });
    }

    /// Pone al día en el grafo los enlaces de `path` (al guardarse o cambiar afuera), en otro hilo.
    fn update_graph(&self, path: RelPath, removed: bool) {
        let i = &self.0;
        // Las transclusiones de la nota abierta que incrustan a `path` se ponen al día (US5).
        i.code_editor.dependency_changed(&path);
        let (Some(graph), Some(vault)) = (i.graph.borrow().clone(), i.vault.borrow().clone())
        else {
            return;
        };
        let Some(format) = Format::from_name(path.file_name()) else {
            return;
        };
        if !path.as_str().starts_with("wiki/") {
            return;
        }
        let this = self.clone();
        glib::spawn_future_local(async move {
            let _ = gio::spawn_blocking(move || {
                let text = if removed {
                    None
                } else {
                    std::fs::read_to_string(path.to_path(vault.root())).ok()
                };
                let Ok(mut g) = graph.lock() else { return };
                match text {
                    Some(t) => g.update_file(&vault, &path, &t, format == Format::Html),
                    None => g.remove_file(&path),
                }
            })
            .await;
            this.refresh_backlinks();
        });
    }

    /// Construye el índice de búsqueda de la bóveda en segundo plano (US1 de la 004).
    fn rebuild_search_index(&self) {
        let i = &self.0;
        let Some(vault) = i.vault.borrow().clone() else {
            return;
        };
        let generation = i.search_gen.get() + 1;
        i.search_gen.set(generation);
        *i.search_index.borrow_mut() = None;
        i.search_panel.set_search_index(None);
        i.code_editor.set_search_index(None);

        let measure_ticks = std::env::var_os("ORBITAL_MEDIR_INDICE").is_some();
        let tick_state = if measure_ticks {
            let last_tick: Rc<Cell<Option<std::time::Instant>>> = Rc::new(Cell::new(None));
            let max_gap = Rc::new(Cell::new(std::time::Duration::ZERO));
            let running = Rc::new(Cell::new(true));
            let last = last_tick.clone();
            let max = max_gap.clone();
            let run = running.clone();
            self.0.window.add_tick_callback(move |w, _| {
                if !run.get() {
                    return glib::ControlFlow::Break;
                }
                let now = std::time::Instant::now();
                if let Some(prev) = last.get() {
                    let dt = now.duration_since(prev);
                    if dt > max.get() {
                        max.set(dt);
                    }
                }
                last.set(Some(now));
                w.queue_draw();
                glib::ControlFlow::Continue
            });
            Some((running, max_gap, std::time::Instant::now()))
        } else {
            None
        };

        let this = self.clone();
        glib::spawn_future_local(async move {
            let built = gio::spawn_blocking(move || {
                let index = orbital_vault::SearchIndex::build(&vault);
                let snapshot = crate::tag_panel::TagSnapshot::from_index(&index);
                (index, snapshot)
            })
            .await
            .ok();
            if let Some((run, max_gap, start)) = tick_state {
                run.set(false);
                let elapsed = start.elapsed();
                let worst_ms = max_gap.get().as_secs_f64() * 1000.0;
                println!(
                    "ORBITAL_MEDIR_INDICE: armado en {:.2} ms, peor intervalo entre cuadros: {:.2} ms",
                    elapsed.as_secs_f64() * 1000.0,
                    worst_ms
                );
            }
            if this.0.search_gen.get() != generation {
                return;
            }
            if let Some((index, snapshot)) = built {
                let arc_idx = Some(Arc::new(std::sync::RwLock::new(index)));
                *this.0.search_index.borrow_mut() = arc_idx.clone();
                this.0.search_panel.set_search_index(arc_idx.clone());
                this.0.code_editor.set_search_index(arc_idx);
                this.apply_tag_snapshot(snapshot);
                this.drain_search_updates();
            }
        });
    }

    /// Una sola tarea aplica cambios por vez: nunca publica una lectura vieja después de otra nueva.
    /// Los eventos recibidos durante la construcción inicial quedan pendientes.
    fn update_search_index(&self, path: RelPath, removed: bool) {
        self.0.search_updates.borrow_mut().insert(path, removed);
        self.drain_search_updates();
    }

    fn drain_search_updates(&self) {
        let i = &self.0;
        if i.search_updating.get() || i.search_updates.borrow().is_empty() {
            return;
        }
        let (Some(index), Some(vault)) =
            (i.search_index.borrow().clone(), i.vault.borrow().clone())
        else {
            return;
        };
        let updates = std::mem::take(&mut *i.search_updates.borrow_mut());
        let generation = i.search_gen.get();
        i.search_updating.set(true);
        let this = self.clone();
        glib::spawn_future_local(async move {
            let snapshot = gio::spawn_blocking(move || {
                let texts: Vec<_> = updates
                    .into_iter()
                    .map(|(path, removed)| {
                        let text = (!removed)
                            .then(|| std::fs::read_to_string(path.to_path(vault.root())).ok())
                            .flatten();
                        (path, text)
                    })
                    .collect();
                let mut idx = index.write().ok()?;
                for (path, text) in texts {
                    match text {
                        Some(t) => idx.update_file(&vault, &path, &t),
                        None => idx.remove_file(&path),
                    }
                }
                Some(crate::tag_panel::TagSnapshot::from_index(&idx))
            })
            .await
            .ok()
            .flatten();
            if this.0.search_gen.get() != generation {
                return;
            }
            this.0.search_updating.set(false);
            if let Some(snapshot) = snapshot {
                this.apply_tag_snapshot(snapshot);
            }
            this.0
                .search_panel
                .set_search_index(this.0.search_index.borrow().clone());
            this.0
                .code_editor
                .set_search_index(this.0.search_index.borrow().clone());
            this.drain_search_updates();
        });
    }

    fn apply_tag_snapshot(&self, snapshot: crate::tag_panel::TagSnapshot) {
        self.0.code_editor.set_tag_candidates(snapshot.names);
        self.0.tag_panel.set_tree(Some(snapshot.tree));
        // Los codeblocks `indice` se generan desde el árbol: si la nota en pantalla tiene
        // alguno, la vista se regenera con el snapshot nuevo (005, T016).
        if self.0.code_editor.has_generated() {
            self.0.code_editor.render_now();
        }
    }

    fn search_tag(&self, tag: &str) {
        self.0
            .search_panel
            .set_search_query(&orbital_vault::query::tag_query(tag));
        self.focus_search();
    }

    fn same_vault(&self, vault: &Arc<Vault>, session: u64) -> bool {
        self.0.vault.borrow().as_ref().is_some_and(|current| {
            current.root() == vault.root() && self.0.vault_session.get() == session
        })
    }

    fn ask_tag_rename(&self, old: String) {
        if self.0.tag_operation.get() {
            return;
        }
        let Some(vault) = self.0.vault.borrow().clone() else {
            return;
        };
        let session = self.0.vault_session.get();
        let this = self.clone();
        glib::spawn_future_local(async move {
            let field = gtk::Entry::builder()
                .text(&old)
                .activates_default(true)
                .build();
            field.update_property(&[gtk::accessible::Property::Label("Nombre nuevo de etiqueta")]);
            let dialog = adw::AlertDialog::new(
                Some("Renombrar etiqueta"),
                Some("Usá / para etiquetas anidadas. También se renombrarán las hijas."),
            );
            dialog.set_extra_child(Some(&field));
            dialog.add_response("cancel", "Cancelar");
            dialog.add_response("plan", "Continuar");
            dialog.set_default_response(Some("plan"));
            dialog.set_close_response("cancel");
            dialog.set_response_appearance("plan", adw::ResponseAppearance::Suggested);
            let weak = dialog.downgrade();
            field.connect_changed(move |entry| {
                if let Some(dialog) = weak.upgrade() {
                    dialog.set_response_enabled(
                        "plan",
                        orbital_vault::tags::valid_tag_name(
                            entry.text().trim().trim_start_matches('#'),
                        ),
                    );
                }
            });
            if dialog.choose_future(Some(&this.0.window)).await != "plan"
                || !this.same_vault(&vault, session)
            {
                return;
            }
            let new = field.text().trim().trim_start_matches('#').to_owned();
            if !orbital_vault::tags::valid_tag_name(&new) || new == old {
                return;
            }
            this.0.code_editor.flush().await;
            if !this.same_vault(&vault, session) || this.0.code_editor.has_pending() {
                return this
                    .toast("Guardá o resolvé los cambios pendientes antes de renombrar etiquetas");
            }
            let (v, from, to) = (vault.clone(), old.clone(), new.clone());
            let index = this.0.search_index.borrow().clone();
            let built = gio::spawn_blocking(move || {
                let sources: Vec<_> = v
                    .entries()
                    .iter()
                    .filter(|entry| {
                        entry.zone == orbital_vault::Zone::Wiki
                            && Format::from_name(entry.path.file_name()).is_some()
                    })
                    .map(|entry| entry.path.clone())
                    .collect();
                let plan = v.plan_tag_rename(&from, &to, &sources);
                let merging = orbital_vault::fold(&from) != orbital_vault::fold(&to)
                    && index
                        .as_ref()
                        .and_then(|index| index.read().ok())
                        .is_some_and(|index| {
                            index
                                .tag_names()
                                .iter()
                                .any(|name| orbital_vault::fold(name) == orbital_vault::fold(&to))
                        });
                (plan, merging)
            })
            .await;
            if !this.same_vault(&vault, session) {
                return;
            }
            let Ok((plan, merging)) = built else {
                return this.toast("No se pudo preparar el renombre");
            };
            if let Some((path, error)) = plan.errors.first() {
                return this.toast(&format!(
                    "No se aplicó el renombre: {} notas no pudieron revisarse. {path}: {error}",
                    plan.errors.len()
                ));
            }
            if plan.is_empty() {
                return this.toast("No se encontraron notas para renombrar");
            }
            let confirm = adw::AlertDialog::new(
                Some("¿Renombrar la etiqueta?"),
                Some(&format!(
                    "«#{old}» y sus hijas pasarán a «#{new}» en {} notas.{}",
                    plan.files(),
                    if merging {
                        " El destino ya existe: las etiquetas se fusionarán."
                    } else {
                        ""
                    }
                )),
            );
            confirm.add_response("cancel", "Cancelar");
            confirm.add_response("rename", "Renombrar");
            confirm.set_response_appearance("rename", adw::ResponseAppearance::Suggested);
            confirm.set_close_response("cancel");
            if confirm.choose_future(Some(&this.0.window)).await != "rename"
                || !this.same_vault(&vault, session)
            {
                return;
            }
            this.run_tag_edits(vault, session, Arc::new(plan), false)
                .await;
        });
    }

    async fn run_tag_edits(
        &self,
        vault: Arc<Vault>,
        session: u64,
        plan: Arc<orbital_vault::EditPlan>,
        undo: bool,
    ) {
        if !self.same_vault(&vault, session) || self.0.tag_operation.get() {
            return;
        }
        self.0.code_editor.flush().await;
        if !self.same_vault(&vault, session)
            || self.0.code_editor.has_pending()
            || self.0.tag_operation.get()
        {
            return self.toast("Hay cambios pendientes: no se modificaron las etiquetas");
        }
        self.0.tag_operation.set(true);
        self.0.window.set_sensitive(false);
        let (v, p) = (vault.clone(), plan.clone());
        let result = gio::spawn_blocking(move || {
            let ctx = write_ctx::now(Some(v.config()));
            if undo {
                v.revert_edits(&p, &ctx)
            } else {
                v.apply_edits(&p, &ctx)
            }
        })
        .await;
        if self.same_vault(&vault, session) {
            match result {
                Ok((receipt, failed)) => {
                    for (path, hash) in &receipt.written {
                        self.0.own_writes.borrow_mut().insert(path.clone(), *hash);
                        self.update_graph(path.clone(), false);
                        self.update_search_index(path.clone(), false);
                    }
                    if let Some(open) =
                        self.0.code_editor.current_path().filter(|path| {
                            receipt.written.iter().any(|(written, _)| written == path)
                        })
                    {
                        let (v, path) = (vault.clone(), open.clone());
                        let note = gio::spawn_blocking(move || v.read_note(&path))
                            .await
                            .ok()
                            .and_then(Result::ok);
                        if self.same_vault(&vault, session)
                            && self.0.code_editor.current_path().as_ref() == Some(&open)
                            && !self.0.code_editor.has_pending()
                            && let Some(note) = note
                        {
                            self.0.code_editor.load(vault.clone(), &note);
                        }
                    }
                    let message = format!(
                        "{} notas {}; {} sin modificar{}",
                        receipt.written.len(),
                        if undo { "restauradas" } else { "actualizadas" },
                        failed.len(),
                        if failed.is_empty() {
                            ""
                        } else {
                            " (cambios externos o error de escritura)"
                        }
                    );
                    let applied = Arc::new(plan.applied(&receipt));
                    if !undo && !applied.is_empty() {
                        let (this, captured) = (self.clone(), vault.clone());
                        self.toast_undo(&message, move || {
                            let (this, v, p) = (this.clone(), captured.clone(), applied.clone());
                            glib::spawn_future_local(async move {
                                this.run_tag_edits(v, session, p, true).await;
                            });
                        });
                    } else {
                        self.toast(&message);
                    }
                }
                Err(_) => self.toast("No se pudo completar la edición de etiquetas"),
            }
        }
        self.0.window.set_sensitive(true);
        self.0.tag_operation.set(false);
        let deferred = std::mem::take(&mut *self.0.tag_disk_changes.borrow_mut());
        if !deferred.is_empty() {
            self.on_disk_changes(deferred);
        }
    }

    pub fn focus_search(&self) {
        self.0.sidebar_stack.set_visible_child_name("busqueda");
        self.0.search_panel.grab_focus();
    }

    pub fn toggle_search_raw(&self) {
        self.0.search_panel.toggle_raw();
    }

    pub fn open_switcher(&self) {
        let Some(vault) = self.0.vault.borrow().clone() else {
            return;
        };
        let candidates = self.0.candidates.borrow().clone();
        let state = AppState::load();
        let recent = state.recent_notes_for(vault.root()).to_vec();
        let this = self.clone();
        crate::switcher::SwitcherDialog::present(&self.0.window, candidates, recent, move |path| {
            this.show_path(path.as_str());
        });
    }

    /// Registro de comandos de la paleta (US8): nombre visible, acción `win.*`
    /// que corre, atajo mostrado y si requiere una nota abierta. El botón de
    /// los banners («Completar tipo/estructura») y las acciones internas o con
    /// parámetro quedan fuera: aparecen cuando toca, no en la paleta.
    const COMMANDS: &[(&str, &str, Option<&str>, bool)] = &[
        ("Nota nueva", "new-note", Some("Ctrl+N"), false),
        ("Nota HTML nueva", "new-html", None, false),
        ("Diario de hoy", "journal", Some("Ctrl+Alt+D"), false),
        ("Archivar la nota", "archive", Some("Ctrl+Mayús+A"), true),
        (
            "Pegar imagen como adjunto",
            "paste-image",
            Some("Ctrl+Mayús+V"),
            true,
        ),
        ("Renombrar o mover la nota", "rename", Some("F2"), true),
        ("Mover la nota a la papelera", "trash", Some("Supr"), true),
        ("Abrir la nota con otra app", "open-external", None, true),
        ("Vista Código", "view-code", Some("Ctrl+1"), false),
        ("Vista Lectura", "view-read", Some("Ctrl+2"), false),
        ("Vista Dividida", "view-split", Some("Ctrl+3"), false),
        (
            "Buscar en toda la bóveda",
            "search",
            Some("Ctrl+Mayús+F"),
            false,
        ),
        (
            "Incluir u ocultar fuentes (raw/)",
            "toggle-search-raw",
            None,
            false,
        ),
        ("Buscar en la nota abierta", "find", Some("Ctrl+F"), true),
        (
            "Reemplazar en la nota abierta",
            "replace",
            Some("Ctrl+H"),
            true,
        ),
        ("Deshacer en la nota abierta", "undo", Some("Ctrl+Z"), true),
        (
            "Saltar a una nota (selector rápido)",
            "quick-open",
            Some("Ctrl+P"),
            false,
        ),
        (
            "Mostrar u ocultar los retroenlaces",
            "backlinks",
            Some("Ctrl+Mayús+B"),
            false,
        ),
        ("Abrir carpeta…", "open", Some("Ctrl+O"), false),
        (
            "Abrir como bóveda de Orbital…",
            "open-as",
            Some("Ctrl+Mayús+O"),
            false,
        ),
        (
            "Preferencias de la bóveda",
            "preferences",
            Some("Ctrl+,"),
            false,
        ),
        ("Atajos de teclado", "shortcuts", Some("Ctrl+?"), false),
        (
            "Exportar nota como HTML",
            "exportar-html",
            Some("Ctrl+Mayús+E"),
            true,
        ),
        ("Exportar nota como PDF", "exportar-pdf", None, true),
        ("Ver el grafo de enlaces", "ver-grafo", None, false),
        ("Historial de versiones…", "historial", None, true),
        (
            "Renumerar notas al pie",
            "renumerar-notas-al-pie",
            None,
            true,
        ),
        ("Card link desde el portapapeles", "card-link", None, true),
        ("Presentar con OnStage", "onstage", None, true),
        ("Importar a nota (Markdown)", "importar-md", None, false),
    ];

    /// Abre la paleta de comandos (US8). La lista se arma al abrir con el
    /// estado actual: los comandos que requieren nota abierta se muestran
    /// desactivados si no hay ninguna.
    pub fn open_command_palette(&self) {
        let has_note = self.0.current_entry.borrow().is_some();
        let commands: Vec<crate::switcher::CommandEntry> = Self::COMMANDS
            .iter()
            .map(|(name, action, accel, needs_note)| {
                crate::switcher::CommandEntry::new(
                    (*name).to_owned(),
                    (*action).to_owned(),
                    accel.map(|a| a.to_owned()),
                    !needs_note || has_note,
                )
            })
            .collect();
        let this = self.clone();
        crate::switcher::SwitcherDialog::present_commands(
            &self.0.window,
            commands,
            move |command| {
                gtk::prelude::ActionGroupExt::activate_action(
                    &this.0.window,
                    &command.action,
                    None,
                );
            },
        );
    }

    /// Pide la URL y la asocia al archivo (009, US1): precargada si ya tiene (FR-1.3).
    pub fn asociar_url_dialog(&self, ruta: &RelPath) {
        let i = &self.0;
        let Some(vault) = i.vault.borrow().clone() else {
            return;
        };
        let this = self.clone();
        let ruta = ruta.clone();
        glib::spawn_future_local(async move {
            let actual = gio::spawn_blocking({
                let ruta = ruta.clone();
                move || vault.url_de(&ruta)
            })
            .await
            .ok()
            .flatten();
            let campo = gtk::Entry::builder()
                .text(actual.unwrap_or_default())
                .activates_default(true)
                .build();
            campo.update_property(&[gtk::accessible::Property::Label("URL del archivo")]);
            let dialog = adw::AlertDialog::new(
                Some("Asociar URL al archivo"),
                Some("Solo http y https. Queda en .orbital/enlaces.yaml."),
            );
            dialog.set_extra_child(Some(&campo));
            dialog.add_response("cancel", "Cancelar");
            dialog.add_response("asociar", "Asociar");
            dialog.set_default_response(Some("asociar"));
            dialog.set_response_appearance("asociar", adw::ResponseAppearance::Suggested);
            dialog.set_close_response("cancel");
            let weak = dialog.downgrade();
            campo.connect_changed(move |e| {
                if let Some(d) = weak.upgrade() {
                    let ok = e.text().trim().starts_with("http://")
                        || e.text().trim().starts_with("https://");
                    d.set_response_enabled("asociar", ok);
                }
            });
            dialog.set_response_enabled("asociar", false);
            if dialog.choose_future(Some(&this.0.window)).await != "asociar" {
                return;
            }
            let url = campo.text().trim().to_owned();
            let Some(v) = this.0.vault.borrow().clone() else {
                return;
            };
            let r = ruta.clone();
            let resultado =
                gio::spawn_blocking(move || v.asociar_url(&r, &url, &write_ctx::now(None))).await;
            match resultado {
                Ok(Ok(())) => this.reload(None).await,
                Ok(Err(error)) => this.toast(&format!("No se pudo asociar la URL: {error}")),
                Err(_) => this.toast("No se pudo asociar la URL"),
            }
        });
    }

    /// Diálogo «Historial de versiones…» (010, US2): lista con fecha/hora, diff contra el
    /// contenido actual y «Restaurar» que carga la versión en el editor como edición.
    pub fn historial(&self) {
        let i = &self.0;
        let Some(path) = i.code_editor.current_path() else {
            self.toast("Abrí una nota antes de ver su historial");
            return;
        };
        let Some(vault) = i.vault.borrow().clone() else {
            return;
        };
        let this = self.clone();
        glib::spawn_future_local(async move {
            let (v, p) = (vault.clone(), path.clone());
            let versiones =
                match gio::spawn_blocking(move || orbital_vault::versiones_historial(&v, &p)).await
                {
                    Ok(Ok(v)) => v,
                    Ok(Err(error)) => {
                        this.toast(&format!("No se pudo leer el historial: {error}"));
                        Vec::new()
                    }
                    Err(_) => {
                        this.toast("No se pudo leer el historial");
                        Vec::new()
                    }
                };
            if versiones.is_empty() {
                this.toast("Esta nota todavía no tiene versiones en el historial");
                return;
            }
            let dialog = this.dialogo_historial(&path, &versiones);
            dialog.present(Some(&this.0.window));
        });
    }

    /// Construye el diálogo de historial (010, US2). Separado de `historial()` para poder
    /// probarlo dentro de un proceso GTK sin presentación.
    fn dialogo_historial(&self, path: &RelPath, versiones: &[(String, String)]) -> adw::Dialog {
        let dialog = adw::Dialog::builder()
            .title("Historial de versiones")
            .content_width(720)
            .content_height(560)
            .build();
        let root = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(0)
            .build();
        let header = adw::HeaderBar::new();
        header.set_title_widget(Some(&adw::WindowTitle::new(
            &format!("Historial: {}", path.file_name()),
            "",
        )));
        let restaurar = gtk::Button::builder()
            .label("Restaurar")
            .sensitive(false)
            .css_classes(["suggested-action"])
            .build();
        header.pack_end(&restaurar);
        root.append(&header);
        let lista = gtk::ListBox::builder()
            .selection_mode(gtk::SelectionMode::Single)
            .css_classes(["navigation-sidebar"])
            .build();
        lista.update_property(&[gtk::accessible::Property::Label("Versiones del historial")]);
        for (nombre, _contenido) in versiones {
            let fila = gtk::Box::builder()
                .orientation(gtk::Orientation::Horizontal)
                .spacing(8)
                .margin_start(12)
                .margin_end(12)
                .margin_top(6)
                .margin_bottom(6)
                .build();
            let etiqueta = gtk::Label::builder()
                .label(Self::nombre_version_legible(nombre))
                .xalign(0.0)
                .hexpand(true)
                .build();
            fila.append(&etiqueta);
            lista.append(&fila);
        }
        let scroll = gtk::ScrolledWindow::builder()
            .child(&lista)
            .vexpand(true)
            .height_request(200)
            .build();
        let diff_view = editor::find::vista_diff();
        let diff_scroll = gtk::ScrolledWindow::builder()
            .child(&diff_view)
            .vexpand(true)
            .build();
        let separador = gtk::Separator::new(gtk::Orientation::Horizontal);
        root.append(&separador);
        root.append(&scroll);
        root.append(&diff_scroll);
        dialog.set_child(Some(&root));
        self.cargar_diff_historial(path, &versiones[0].0, &diff_view);
        {
            let ventana = self.clone();
            let path = path.clone();
            let versiones = versiones.to_vec();
            let diff_view = diff_view.clone();
            let restaurar = restaurar.clone();
            lista.connect_row_selected(move |_, fila| {
                restaurar.set_sensitive(fila.is_some());
                let Some(fila) = fila else { return };
                let indice = fila.index();
                if indice < 0 {
                    return;
                }
                let (nombre, _contenido) = &versiones[indice as usize];
                ventana.cargar_diff_historial(&path, nombre, &diff_view);
            });
        }
        {
            let this = self.clone();
            let dialog = dialog.clone();
            let lista = lista.clone();
            let versiones = versiones.to_vec();
            restaurar.connect_clicked(move |_| {
                let Some(fila) = lista.selected_row() else {
                    return;
                };
                let indice = fila.index().max(0) as usize;
                if let Some((_nombre, contenido)) = versiones.get(indice) {
                    this.0.code_editor.replace_text(contenido);
                    this.toast(&format!(
                        "Versión restaurada como edición ({} caracteres); guardá normalmente",
                        contenido.len()
                    ));
                    dialog.close();
                }
            });
        }
        dialog
    }

    /// Card link (013): lee la URL del portapapeles, arma el bloque `cardlink` con los
    /// metadatos disponibles (host derivado de la URL; título/descr/image por red si gvfs
    /// lo resuelve; sin red, el bloque queda con la URL y el host) y lo inserta en la nota.
    pub fn card_link(&self) {
        let i = &self.0;
        if i.code_editor.current_path().is_none() {
            self.toast("Abrí una nota antes de insertar un card link");
            return;
        }
        let this = self.clone();
        glib::spawn_future_local(async move {
            let clip = this
                .0
                .window
                .clipboard()
                .read_text_future()
                .await
                .map(|t| t.map(String::from).unwrap_or_default())
                .unwrap_or_default();
            let url = clip.trim().to_owned();
            if !url.starts_with("http://") && !url.starts_with("https://") {
                this.toast("El portapapeles no tiene una URL http(s)");
                return;
            }
            let host = url
                .strip_prefix("https://")
                .or_else(|| url.strip_prefix("http://"))
                .and_then(|r| r.split('/').next())
                .unwrap_or("")
                .to_owned();
            let (title, description, image) =
                match gio::File::for_uri(&url).load_contents_future().await {
                    Ok((bytes, _)) => {
                        let bytes = bytes.to_vec();
                        let texto = String::from_utf8_lossy(&bytes).to_string();
                        let extraer = |tag: &str| -> Option<String> {
                            let marker = format!("{tag}\" content=\"");
                            let start = texto.find(&marker)? + marker.len();
                            let end = texto[start..].find('"')? + start;
                            Some(texto[start..end].to_owned())
                        };
                        let title = extraer("property=\"og:title\"")
                            .or_else(|| extraer("name=\"og:title\""))
                            .or_else(|| {
                                texto.find("<title>").and_then(|s| {
                                    texto[s + 7..]
                                        .find("</title>")
                                        .map(|e| texto[s + 7..s + 7 + e].to_owned())
                                })
                            });
                        let description = extraer("property=\"og:description\"")
                            .or_else(|| extraer("name=\"og:description\""));
                        let image = extraer("property=\"og:image\"")
                            .or_else(|| extraer("name=\"og:image\""));
                        (title, description, image)
                    }
                    Err(_) => (None, None, None),
                };
            let mut bloque = format!(
                "```cardlink\nurl: \"{url}\"\ntitle: \"{}\"\n",
                title.as_deref().unwrap_or(&url)
            );
            if let Some(d) = &description {
                bloque.push_str(&format!("description: \"{d}\"\n"));
            }
            bloque.push_str(&format!("host: \"{host}\"\n"));
            if let Some(img) = &image {
                bloque.push_str(&format!("image: \"{img}\"\n"));
            }
            bloque.push_str("```\n");
            let actual = this.0.code_editor.text();
            let nuevo = if actual.ends_with('\n') || actual.is_empty() {
                format!("{actual}\n{bloque}\n")
            } else {
                format!("{actual}\n\n{bloque}\n")
            };
            this.0.code_editor.replace_text(&nuevo);
            this.toast(&format!("Card link insertado: {host}"));
        });
    }

    /// Fecha legible desde el nombre de archivo `AAAA-MM-DD_HH-MM-SS-mmm.txt`.
    fn nombre_version_legible(nombre: &str) -> String {
        let base = nombre.trim_end_matches(".txt");
        let (fecha, hora) = base.split_once('_').unwrap_or((base, ""));
        let (h, _ms) = hora.split_once('-').unwrap_or((hora, ""));
        format!("{} a las {}", fecha.replace('-', "/"), h.replace('-', ":"))
    }

    /// Carga en la vista de diff el contenido de una versión contra el texto actual del editor.
    fn cargar_diff_historial(&self, path: &RelPath, nombre: &str, vista: &sourceview5::View) {
        let Some(vault) = self.0.vault.borrow().clone() else {
            return;
        };
        let text = self.0.code_editor.text();
        let (path, nombre, nombre_clon) = (path.clone(), nombre.to_owned(), nombre.to_owned());
        let vista = vista.clone();
        glib::spawn_future_local(async move {
            let contenido =
                gio::spawn_blocking(move || orbital_vault::leer_version(&vault, &path, &nombre))
                    .await
                    .unwrap_or_else(|_| {
                        Err(orbital_vault::VaultError::InvalidPath(
                            nombre_clon.to_owned(),
                        ))
                    })
                    .unwrap_or_default();
            let buffer = vista.buffer();
            buffer.set_text("");
            let lines = orbital_vault::diff::diff_lines(&contenido, &text);
            use orbital_vault::diff::DiffKind;
            let mut iter = buffer.start_iter();
            for line in lines {
                let texto = &line.text;
                match line.kind {
                    DiffKind::Same => buffer.insert(&mut iter, &format!("   {texto}\n")),
                    DiffKind::Removed => {
                        buffer.insert_with_tags_by_name(&mut iter, "−  ", &["diff-removed"]);
                        buffer.insert_with_tags_by_name(
                            &mut iter,
                            &format!("{texto}\n"),
                            &["diff-removed"],
                        );
                    }
                    DiffKind::Added => {
                        buffer.insert_with_tags_by_name(&mut iter, "+  ", &["diff-added"]);
                        buffer.insert_with_tags_by_name(
                            &mut iter,
                            &format!("{texto}\n"),
                            &["diff-added"],
                        );
                    }
                }
            }
        });
    }

    /// Exporta la nota abierta (007): HTML autónomo o PDF vía impresión de WebKit. Lee el texto
    /// del editor (lo que se ve, con ediciones sin guardar); sin nota abierta, aviso sin diálogo.
    pub fn exportar(&self, formato: crate::export::Formato) {
        let i = &self.0;
        let Some(entry) = i.current_entry.borrow().clone() else {
            self.toast("Abrí una nota antes de exportar");
            return;
        };
        let text = i.code_editor.text();
        let nombre = entry.path.file_name().to_owned();
        let stem = nombre
            .strip_suffix(".md")
            .or_else(|| nombre.strip_suffix(".html"))
            .unwrap_or(&nombre)
            .to_owned();
        let (titulo, defecto) = match formato {
            crate::export::Formato::Html => ("Exportar nota como HTML", format!("{stem}.html")),
            crate::export::Formato::Pdf => ("Exportar nota como PDF", format!("{stem}.pdf")),
        };
        let dialog = gtk::FileDialog::builder()
            .title(titulo)
            .initial_name(&defecto)
            .build();
        let this = self.clone();
        dialog.save(
            Some(&i.window),
            None::<&gio::Cancellable>,
            move |resultado| {
                let Ok(file) = resultado else {
                    return; // cancelado (SC-3: no se escribe nada)
                };
                let Some(destino) = file.path() else {
                    return;
                };
                let this = this.clone();
                let text = text.clone();
                let formato = formato;
                glib::spawn_future_local(async move {
                    match formato {
                        crate::export::Formato::Html => {
                            let es_html = text.trim_start().starts_with("<!doctype html>")
                                || text.trim_start().starts_with("<!DOCTYPE html>")
                                || text.trim_start().starts_with("<html");
                            let out = if es_html {
                                crate::export::html_passthrough(&text)
                            } else {
                                crate::export::html_md(&text, &destino.to_string_lossy())
                            };
                            let out_clon = out.clone();
                            let destino_clon = destino.clone();
                            let escritura = gio::spawn_blocking(move || {
                                std::fs::write(&destino_clon, out_clon)
                            })
                            .await;
                            match escritura {
                                Ok(Ok(())) => {
                                    this.toast(&format!("Nota exportada: {}", destino.display()))
                                }
                                _ => this.toast("No se pudo escribir el archivo exportado"),
                            }
                        }
                        crate::export::Formato::Pdf => this.exportar_pdf(&text, &destino),
                    }
                });
            },
        );
    }

    /// PDF vía impresión de WebKit del mismo HTML autónomo: el diálogo de impresión aparece
    /// preconfigurado con «Salida a archivo» y el destino elegido (WebKit no permite exportación
    /// silenciosa). La vista vive en `exports_pendientes` hasta que la impresión termina.
    fn exportar_pdf(&self, text: &str, destino: &std::path::Path) {
        let es_html = text.trim_start().starts_with("<!doctype html>")
            || text.trim_start().starts_with("<!DOCTYPE html>")
            || text.trim_start().starts_with("<html");
        let html = if es_html {
            crate::export::html_passthrough(text)
        } else {
            crate::export::html_md(text, &destino.to_string_lossy())
        };
        let web = webkit6::WebView::new();
        web.load_html(&html, None);
        let destino = destino.to_path_buf();
        web.connect_load_changed(move |web, ev| {
            if ev != webkit6::LoadEvent::Finished {
                return;
            }
            let op = webkit6::PrintOperation::new(web);
            let settings = gtk::PrintSettings::new();
            settings.set("output-uri", Some(&format!("file://{}", destino.display())));
            settings.set("output-file-format", Some("pdf"));
            op.set_print_settings(&settings);
            op.print();
        });
        self.0.exports_pendientes.borrow_mut().push(web);
    }

    /// Abre la página del grafo (008) y construye sus datos si no los tiene.
    pub fn ver_grafo(&self) {
        let i = &self.0;
        self.guardar_estado_pestana();
        i.pestanas.abrir(crate::pestanas::CONEXIONES, "Conexiones");
        if i.grafo_datos.borrow().is_none() {
            let graph = i.graph.borrow().clone();
            self.reconstruir_grafo_visual(graph.as_ref());
        }
        i.content_stack.set_visible_child_name("grafo");
    }

    /// Cierra el grafo y vuelve a la nota (Esc: SC-3, nada cambia).
    pub fn cerrar_grafo(&self) {
        // Cerrar el grafo es cerrar su pestaña: la barra elige la vecina (y la abre).
        self.0.pestanas.cerrar(crate::pestanas::CONEXIONES);
    }

    /// Guarda el estado de vista de la nota del editor en su pestaña (antes de dejarla).
    fn guardar_estado_pestana(&self) {
        let i = &self.0;
        if i.content_stack.visible_child_name().as_deref() != Some("codigo") {
            return;
        }
        if let Some(path) = i.code_editor.current_path() {
            i.pestanas
                .guardar_estado(path.as_str(), i.code_editor.estado_vista());
        }
    }

    /// Barra derecha (025): metadatos y etiquetas de la nota abierta, enlaces entrantes y, con
    /// su modo a la vista, el grafo local (armado en segundo plano).
    fn refresh_panel(&self) {
        use crate::panel_derecho::Metadatos;
        let i = &self.0;
        if !i.right_split.shows_sidebar() {
            return;
        }
        let Some(entry) = i.current_entry.borrow().clone() else {
            i.panel.set_nota(None);
            i.panel.set_entrantes(&[]);
            i.panel
                .set_grafo(None, "Abrí una nota para ver sus conexiones.");
            return;
        };
        let file = entry.path.file_name();
        let formato = Format::from_name(file);
        let meta = {
            let note = i.current_note.borrow();
            let note = note.as_ref().filter(|n| n.path == entry.path);
            let nombre = note
                .and_then(|n| n.prop("title"))
                .and_then(|v| v.as_text())
                .map(str::to_owned)
                .unwrap_or_else(|| file.rsplit_once('.').map_or(file, |(s, _)| s).to_owned());
            let palabras = if i.code_editor.current_path().as_ref() == Some(&entry.path) {
                let b = i.code_editor.buffer();
                Some(crate::status_bar::contar(&b.text(&b.start_iter(), &b.end_iter(), false)).1)
            } else {
                note.map(|n| crate::status_bar::contar(&n.source).1)
            };
            let mut etiquetas: Vec<String> = Vec::new();
            if let Some(n) = note {
                for t in orbital_vault::tags::extract_tags(&n.source, n.format == Format::Html) {
                    if !etiquetas.contains(&t.tag) {
                        etiquetas.push(t.tag);
                    }
                }
            }
            Metadatos {
                nombre,
                formato: match formato {
                    Some(Format::Md) => "Markdown".to_owned(),
                    Some(Format::Html) => "HTML".to_owned(),
                    None => file
                        .rsplit_once('.')
                        .map_or_else(|| "Archivo".to_owned(), |(_, e)| e.to_uppercase()),
                },
                ubicacion: entry
                    .path
                    .as_str()
                    .rsplit_once('/')
                    .map_or_else(|| "/".to_owned(), |(d, _)| d.to_owned()),
                palabras,
                etiquetas,
            }
        };
        i.panel.set_nota(Some(meta));
        let graph = i.graph.borrow().clone();
        let Some(graph) = graph else {
            i.panel.set_entrantes(&[]);
            i.panel.set_grafo(None, "Armando las conexiones…");
            return;
        };
        let mut entrantes: Vec<RelPath> = match graph.lock() {
            Ok(g) => g
                .backlinks_for(&entry.path)
                .iter()
                .map(|b| b.from.clone())
                .collect(),
            Err(_) => Vec::new(),
        };
        entrantes.sort();
        entrantes.dedup();
        i.panel.set_entrantes(&entrantes);
        if i.panel.modo() != "grafo" {
            return;
        }
        if formato.is_none() {
            i.panel
                .set_grafo(None, "Los archivos que no son notas no tienen enlaces.");
            return;
        }
        let this = self.clone();
        let centro = entry.path.clone();
        glib::spawn_future_local(async move {
            let c = centro.clone();
            let datos = gio::spawn_blocking(move || {
                let g = graph.lock().ok()?;
                let enlaces: Vec<(String, String)> = g
                    .enlaces_resueltos()
                    .into_iter()
                    .map(|(a, b)| (a.to_string(), b.to_string()))
                    .collect();
                let vecinos: Vec<String> = enlaces
                    .iter()
                    .filter_map(|(a, b)| {
                        if *a == c.as_str() {
                            Some(b.clone())
                        } else if *b == c.as_str() {
                            Some(a.clone())
                        } else {
                            None
                        }
                    })
                    .collect();
                Some(crate::grafo::GrafoDatos::local(
                    c.to_string(),
                    vecinos,
                    &enlaces,
                ))
            })
            .await
            .ok()
            .flatten();
            // La nota pudo cambiar mientras tanto.
            let sigue = this.0.current_entry.borrow().as_ref().map(|e| &e.path) == Some(&centro);
            if !sigue {
                return;
            }
            match datos {
                Some(d) if d.rutas.len() > 1 => this.0.panel.set_grafo(Some(Rc::new(d)), ""),
                Some(d) => this.0.panel.set_grafo(
                    Some(Rc::new(d)),
                    "Ninguna nota se conecta con esta todavía.",
                ),
                None => this
                    .0
                    .panel
                    .set_grafo(None, "No se pudieron leer las conexiones."),
            }
        });
    }

    /// Conecta las pestañas: elegir abre su nota con el estado guardado; cerrar la última deja
    /// el centro vacío (guardando antes lo pendiente).
    fn setup_pestanas(&self) {
        let weak = Rc::downgrade(&self.0);
        self.0.pestanas.connect_elegir(move |clave, estado| {
            let Some(i) = weak.upgrade() else { return };
            let w = Window(i);
            if clave == crate::pestanas::CONEXIONES {
                w.ver_grafo();
                return;
            }
            w.guardar_estado_pestana();
            if let (Some(estado), Ok(path)) = (estado, RelPath::new(clave)) {
                *w.0.estado_pendiente.borrow_mut() = Some((path, estado));
            }
            w.show_path(clave);
        });
        let weak = Rc::downgrade(&self.0);
        self.0.pestanas.connect_cerrar(move |clave, quedan| {
            let Some(i) = weak.upgrade() else { return };
            if quedan {
                return;
            }
            let w = Window(i);
            if clave == crate::pestanas::CONEXIONES && w.0.code_editor.current_path().is_some() {
                // Quedó solo la nota del editor sin pestaña: se vuelve a ella.
                if let Some(e) = w.0.current_entry.borrow().clone() {
                    w.0.pestanas.abrir(e.path.as_str(), e.path.file_name());
                }
                w.0.content_stack.set_visible_child_name("codigo");
                return;
            }
            glib::spawn_future_local(async move {
                let editor = w.0.code_editor.clone();
                editor.flush().await;
                if editor.has_pending() {
                    w.toast("La nota tiene cambios sin guardar: quedó abierta");
                    if let Some(e) = w.0.current_entry.borrow().clone() {
                        w.0.pestanas.abrir(e.path.as_str(), e.path.file_name());
                    }
                    return;
                }
                editor.close();
                *w.0.current_entry.borrow_mut() = None;
                *w.0.current_note.borrow_mut() = None;
                w.0.content_title.set_title("");
                w.0.content_title.set_subtitle("");
                if let Some(model) = w.0.list_view.model().and_downcast::<gtk::SingleSelection>() {
                    model.set_selected(gtk::INVALID_LIST_POSITION);
                }
                w.0.content_stack.set_visible_child_name("vacio");
                w.refresh_backlinks();
            });
        });
    }

    /// Presenta la nota abierta como diapositivas (OnStage, 014): solo notas `.md`.
    pub fn presentar(&self) {
        let Some(ruta) = self.0.code_editor.current_path() else {
            self.toast("Abrí una nota antes de presentar");
            return;
        };
        if ruta.as_str().ends_with(".html") {
            self.toast("OnStage presenta notas .md; esta nota es HTML");
            return;
        }
        self.0.onstage.presentar();
    }

    /// Abre el visor del PDF abierto de `raw/` (016, acción de ventana y botón).
    fn ver_pdf_accion(&self) {
        let Some(ruta) = self.0.externo_ruta.borrow().clone() else {
            self.toast("Abrí un PDF de raw/ antes de verlo");
            return;
        };
        if !ruta.as_str().ends_with(".pdf") {
            self.toast("Solo se ve así un PDF");
            return;
        }
        self.ver_pdf(&ruta, &crate::visor_pdf::Ancla::default());
    }

    /// Muestra el PDF en el visor embebido, con ancla si la hay (016).
    fn ver_pdf(&self, ruta: &RelPath, ancla: &crate::visor_pdf::Ancla) {
        let Some(vault) = self.0.vault.borrow().clone() else {
            return;
        };
        *self.0.visor_ruta.borrow_mut() = Some(ruta.clone());
        self.0.visor.abrir(vault, ruta.clone(), ancla);
        self.0.content_stack.set_visible_child_name("pdf");
    }

    /// Importa el archivo abierto de `raw/` a una nota Markdown nueva en `wiki/` (015).
    fn importar_md(&self) {
        let Some(vault) = self.0.vault.borrow().clone() else {
            return;
        };
        let Some(ruta) = self.0.externo_ruta.borrow().clone() else {
            self.toast("Abrí un archivo de raw/ antes de importar");
            return;
        };
        let extension = ruta
            .file_name()
            .rsplit_once('.')
            .map(|(_, e)| e.to_ascii_lowercase())
            .unwrap_or_default();
        if orbital_vault::importar::formato_de_ext(&extension).is_none() {
            self.toast("Solo se importan archivos PDF, DOCX o HTML");
            return;
        }
        let this = self.clone();
        glib::spawn_future_local(async move {
            let result = gio::spawn_blocking(move || {
                let ctx = write_ctx::now(Some(vault.config()));
                orbital_vault::importar::crear_nota_importada(&vault, &ruta, &ctx)
            })
            .await;
            match result {
                Ok(Ok(path)) => {
                    this.reload(Some(path.clone())).await;
                    this.toast(&format!("Nota creada: «{path}»"));
                }
                Ok(Err(e)) => this.toast(&e.to_string()),
                Err(_) => this.toast("No se pudo importar el archivo"),
            }
        });
    }

    /// Arma los `GrafoDatos` del grafo actual en segundo plano (SC-1).
    fn reconstruir_grafo_visual(
        &self,
        graph: Option<&Arc<std::sync::Mutex<orbital_vault::LinkGraph>>>,
    ) {
        let Some(graph) = graph else {
            return;
        };
        let graph = graph.clone();
        let this = self.clone();
        glib::spawn_future_local(async move {
            let built = gio::spawn_blocking(move || {
                let guard = graph.lock().ok();
                guard.map(|g| (g.notas(), g.enlaces_resueltos()))
            })
            .await
            .ok();
            let Some(Some((notas, enlaces))) = built else {
                return;
            };
            let datos = std::rc::Rc::new(crate::grafo::GrafoDatos::desde(
                notas.iter().map(|p| p.as_str().to_owned()).collect(),
                enlaces
                    .iter()
                    .map(|(a, b)| (a.as_str().to_owned(), b.as_str().to_owned()))
                    .collect(),
            ));
            *this.0.grafo_datos.borrow_mut() = Some(datos.clone());
            this.0.grafo_vista.set_datos(datos);
        });
    }

    fn collect_candidates_baseline(
        vault: &Vault,
    ) -> Vec<crate::editor::completion::CompletionCandidate> {
        let mut list: Vec<_> = vault
            .entries()
            .iter()
            .filter(|e| {
                e.zone == orbital_vault::Zone::Wiki
                    && matches!(
                        e.kind,
                        orbital_vault::EntryKind::AtomNote
                            | orbital_vault::EntryKind::SynthesisNote
                    )
            })
            .map(|e| {
                crate::editor::completion::CompletionCandidate::new(
                    e.path.clone(),
                    None,
                    Vec::new(),
                )
            })
            .collect();
        crate::editor::completion::qualify_duplicates(&mut list);
        list
    }

    fn collect_candidates_full(
        vault: &Vault,
    ) -> Vec<crate::editor::completion::CompletionCandidate> {
        let mut list: Vec<_> = vault
            .entries()
            .iter()
            .filter(|e| {
                e.zone == orbital_vault::Zone::Wiki
                    && matches!(
                        e.kind,
                        orbital_vault::EntryKind::AtomNote
                            | orbital_vault::EntryKind::SynthesisNote
                    )
            })
            .map(|e| {
                let props = vault.read_props(&e.path).ok().flatten();
                let entries = props.as_ref().map_or(&[][..], |p| &p.entries[..]);
                let title = orbital_vault::props::find(entries, "title")
                    .and_then(|v| v.value.as_text())
                    .map(str::to_owned);
                let aliases = orbital_vault::props::find(entries, "aliases")
                    .map(|v| match &v.value {
                        orbital_vault::props::PropValue::List(l) => l
                            .iter()
                            .filter_map(|x| x.as_text().map(str::to_owned))
                            .collect(),
                        orbital_vault::props::PropValue::Text(t) => vec![t.clone()],
                        _ => Vec::new(),
                    })
                    .unwrap_or_default();
                crate::editor::completion::CompletionCandidate::new(e.path.clone(), title, aliases)
            })
            .collect();
        crate::editor::completion::qualify_duplicates(&mut list);
        list
    }

    /// Muestra en el panel los retroenlaces de la nota abierta (solo si el panel está a la vista).
    fn refresh_backlinks(&self) {
        use crate::editor::right_sidebar::Backlinks;
        let i = &self.0;
        if !i.right_split.shows_sidebar() {
            return;
        }
        self.refresh_panel();
        let path = i
            .current_entry
            .borrow()
            .as_ref()
            .filter(|e| Format::from_name(e.path.file_name()).is_some())
            .map(|e| e.path.clone());
        let Some(path) = path else {
            i.right.set_backlinks(Backlinks::NoNote);
            return;
        };
        let graph = i.graph.borrow().clone();
        let Some(graph) = graph else {
            i.right.set_backlinks(Backlinks::Loading);
            return;
        };
        // Se copian y se suelta el candado antes de armar los widgets.
        let refs = match graph.lock() {
            Ok(g) => g.backlinks_for(&path).to_vec(),
            Err(_) => Vec::new(),
        };
        i.right.set_backlinks(Backlinks::Ready(&refs));
    }

    /// Conecta el editor con la ventana: estado del guardado, errores y cierre.
    fn setup_editor(&self) {
        // Retroenlaces: elegir uno abre la nota de origen en la línea del enlace (FR-013).
        let weak = Rc::downgrade(&self.0);
        self.0.right.connect_open(move |from, line| {
            let Some(i) = weak.upgrade() else { return };
            let w = Window(i);
            if w.0.code_editor.current_path().as_ref() == Some(from) {
                w.0.code_editor.reveal_line(line);
            } else {
                w.0.pending_line.set(Some((from.clone(), line)));
                w.show_path(from.as_str());
            }
        });
        let weak = Rc::downgrade(&self.0);
        self.0.right_split.connect_show_sidebar_notify(move |_| {
            if let Some(i) = weak.upgrade() {
                Window(i).refresh_backlinks();
            }
        });
        // Las propiedades arriba de la vista formateada (con los botones de fuentes).
        let weak = Rc::downgrade(&self.0);
        self.0.code_editor.set_on_header(move |path, header| {
            let i = weak.upgrade()?;
            Window(i).props_header(path, header)
        });
        // Enlaces: web afuera (con un clic, principio V); notas de la bóveda, adentro.
        let weak = Rc::downgrade(&self.0);
        self.0.code_editor.set_on_link(move |from, url| {
            let Some(i) = weak.upgrade() else { return };
            Window(i).open_link(Some(from), url);
        });
        // La vista elegida se recuerda (FR-013).
        let persist = self.0.persist.get();
        self.0.code_editor.set_on_view(move |format, v| {
            if !persist {
                return;
            }
            let name = v.name().to_owned();
            glib::spawn_future_local(async move {
                let _ = gio::spawn_blocking(move || {
                    let mut state = AppState::load();
                    match format {
                        Format::Md => state.md_view = Some(name),
                        Format::Html => state.html_view = Some(name),
                    }
                    state.save()
                })
                .await;
            });
        });
        let editor = self.0.code_editor.clone();
        glib::spawn_future_local(async move {
            let saved = gio::spawn_blocking(|| {
                let s = AppState::load();
                (s.md_view, s.html_view)
            })
            .await
            .unwrap_or_default();
            let view = |s: Option<String>, default| {
                s.as_deref()
                    .and_then(crate::editor::View::from_name)
                    .unwrap_or(default)
            };
            editor.set_initial_views(
                view(saved.0, crate::editor::View::Lectura),
                view(saved.1, crate::editor::View::Visual),
            );
        });
        let weak = Rc::downgrade(&self.0);
        self.0.code_editor.set_on_status(move |status| {
            let Some(i) = weak.upgrade() else { return };
            let base = i.subtitle_base.borrow().clone();
            let label = status.label();
            i.content_title.set_subtitle(&if label.is_empty() {
                base
            } else {
                format!("{base} · {label}")
            });
        });
        let weak = Rc::downgrade(&self.0);
        self.0.code_editor.set_on_saved(move |path, hash| {
            if let Some(i) = weak.upgrade() {
                i.own_writes.borrow_mut().insert(path.clone(), hash);
                Window(i.clone()).update_graph(path.clone(), false);
                Window(i).update_search_index(path.clone(), false);
            }
        });
        let weak = Rc::downgrade(&self.0);
        self.0.code_editor.set_on_error(move |msg| {
            if let Some(i) = weak.upgrade() {
                Window(i).toast(msg);
            }
        });
        // Visor PDF (016): copiar enlace al portapapeles, resaltados y volver.
        let visor = self.0.visor.clone();
        visor.set_on_copiar({
            let weak = Rc::downgrade(&self.0);
            move |texto| {
                let Some(i) = weak.upgrade() else {
                    return;
                };
                let bytes = gtk::glib::Bytes::from(texto.as_bytes());
                let contenido =
                    gtk::gdk::ContentProvider::for_bytes("text/plain;charset=utf-8", &bytes);
                match i.window.clipboard().set_content(Some(&contenido)) {
                    Ok(()) => Window(i).toast("Enlace copiado al portapapeles"),
                    _ => Window(i).toast("No se pudo copiar el enlace"),
                }
            }
        });
        visor.set_on_resaltados({
            let weak = Rc::downgrade(&self.0);
            move || {
                let Some(i) = weak.upgrade() else {
                    return;
                };
                let nombre = i
                    .visor_ruta
                    .borrow()
                    .as_ref()
                    .map(|r| r.file_name().to_owned())
                    .unwrap_or_default();
                let consulta = format!("\"raw/{nombre}\"");
                i.search_panel.set_search_query(&consulta);
                Window(i).focus_search();
            }
        });
        visor.set_on_volver({
            let weak = Rc::downgrade(&self.0);
            move || {
                let Some(i) = weak.upgrade() else {
                    return;
                };
                i.content_stack.set_visible_child_name("externo");
            }
        });
        // OnStage (014): material vivo, apertura de enlaces y aviso de cambios del editor.
        let onstage = self.0.onstage.clone();
        onstage.set_on_link({
            let weak = Rc::downgrade(&self.0);
            move |url| {
                let Some(i) = weak.upgrade() else {
                    return;
                };
                let from = i.code_editor.current_path();
                Window(i).open_link(from.as_ref(), url);
            }
        });
        onstage.set_proveedor({
            let weak = Rc::downgrade(&self.0);
            move || {
                let i = weak.upgrade()?;
                let ruta = i.code_editor.current_path()?;
                if ruta.as_str().ends_with(".html") {
                    return None;
                }
                Some(crate::onstage::Fuente {
                    texto: i.code_editor.text(),
                    ruta,
                    vault: i.vault.borrow().clone(),
                })
            }
        });
        {
            let onstage = self.0.onstage.clone();
            self.0
                .code_editor
                .set_on_text_changed(move || onstage.editar());
        }
        // Al cerrar la ventana, primero se guarda lo pendiente.
        let this = self.clone();
        self.0.window.connect_close_request(move |w| {
            if this.0.closing.get() || !this.0.code_editor.has_pending() {
                return glib::Propagation::Proceed;
            }
            let (this, w) = (this.clone(), w.clone());
            glib::spawn_future_local(async move {
                this.0.code_editor.flush().await;
                if this.0.code_editor.has_pending() {
                    return;
                }
                this.0.closing.set(true);
                w.close();
            });
            glib::Propagation::Stop
        });
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
                let weak_link = Rc::downgrade(i);
                let v = html_view::new_view(Some(Rc::new(move |url| {
                    if let Some(inner) = weak_link.upgrade() {
                        Window(inner).open_link(None, url);
                    }
                })));
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
    fn source_rows(&self, note: &RelPath, e: &PropEntry) -> Option<Vec<gtk::Widget>> {
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

    /// Lista editable de propiedades arriba de la vista formateada (US3 de la 002).
    fn props_header(&self, path: &RelPath, h: &editor::Header) -> Option<gtk::Widget> {
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
        let custom = move |e: &PropEntry| this.source_rows(&note, e);
        Some(list(&h.entries, mode, actions, &custom))
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
        self.0.code_editor.flush().await;
        if self.0.code_editor.has_pending() {
            return;
        }
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
        self.0.code_editor.flush().await;
        if self.0.code_editor.has_pending() {
            return;
        }
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
        if self.0.tag_operation.get() {
            self.0.tag_disk_changes.borrow_mut().extend(paths);
            return;
        }
        let Some(vault) = self.0.vault.borrow().clone() else {
            return;
        };
        let retry_paths = paths.clone();
        let this = self.clone();
        glib::spawn_future_local(async move {
            let v = vault.clone();
            let own: Vec<(RelPath, u64)> = this
                .0
                .own_writes
                .borrow()
                .iter()
                .map(|(p, h)| (p.clone(), *h))
                .collect();
            let (mut changes, echoes) = gio::spawn_blocking(move || {
                let changes = v.on_external_change(&paths);
                // Un cambio cuyo contenido es el que escribió el editor es un eco propio.
                let echoes: Vec<RelPath> = own
                    .into_iter()
                    .filter(|(p, h)| {
                        std::fs::read(p.to_path(v.root()))
                            .is_ok_and(|b| orbital_vault::write::hash(&b) == *h)
                    })
                    .map(|(p, _)| p)
                    .collect();
                (changes, echoes)
            })
            .await
            .unwrap_or_default();
            if this.0.tag_operation.get() {
                this.0.tag_disk_changes.borrow_mut().extend(retry_paths);
                return;
            }
            changes.retain(
                |c| !matches!(c, orbital_vault::ExternalChange::Modified(p) if echoes.contains(p)),
            );
            // El grafo de enlaces sigue los cambios de otros programas (FR-014).
            for c in &changes {
                match c {
                    orbital_vault::ExternalChange::Modified(p) => {
                        this.update_graph(p.clone(), false);
                        this.update_search_index(p.clone(), false);
                    }
                    orbital_vault::ExternalChange::Removed(p) => {
                        this.update_graph(p.clone(), true);
                        this.update_search_index(p.clone(), true);
                    }
                    _ => {}
                }
            }
            let current = this
                .0
                .current_entry
                .borrow()
                .as_ref()
                .map(|e| e.path.clone());
            // La nota abierta en el editor: el eco de un guardado propio se ignora, y con cambios
            // sin guardar no se pisa nada (FR-019).
            let editor = this.0.code_editor.clone();
            if let Some(open) = editor.current_path() {
                let touched = changes
                    .iter()
                    .any(|c| matches!(c, orbital_vault::ExternalChange::Modified(p) if *p == open));
                if touched {
                    let abs = open.to_path(vault.root());
                    let disk = gio::spawn_blocking(move || std::fs::read(abs).ok())
                        .await
                        .ok()
                        .flatten();
                    let disk_hash = disk.as_deref().map(orbital_vault::write::hash).unwrap_or(0);
                    match editor.external_change(disk_hash) {
                        orbital_vault::session::ExternalAction::Ignore => changes.retain(|c| {
                            !matches!(c, orbital_vault::ExternalChange::Modified(p) if *p == open)
                        }),
                        orbital_vault::session::ExternalAction::Reload => {
                            let (v, p) = (vault.clone(), open.clone());
                            let note = gio::spawn_blocking(move || v.read_note(&p))
                                .await
                                .ok()
                                .and_then(Result::ok);
                            if let Some(note) = note {
                                editor.load(vault.clone(), &note);
                            }
                            changes.retain(|c| {
                                !matches!(c, orbital_vault::ExternalChange::Modified(p) if *p == open)
                            });
                        }
                        orbital_vault::session::ExternalAction::AskUser => {
                            changes.retain(|c| {
                                !matches!(c, orbital_vault::ExternalChange::Modified(p) if *p == open)
                            });
                            if !this.0.conflict_dialog_open.get() {
                                this.0.conflict_dialog_open.set(true);
                                let disk_text = disk
                                    .and_then(|b| String::from_utf8(b).ok())
                                    .unwrap_or_default();
                                let mine_text = editor.text();
                                let name = open.file_name().to_string();
                                let format = editor.format();
                                let this_c = this.clone();
                                let v_c = vault.clone();
                                let ed = editor.clone();
                                crate::editor::conflict::show_conflict_dialog(
                                    &this.0.window,
                                    &name,
                                    mine_text,
                                    disk_text,
                                    format,
                                    move |choice| {
                                        this_c.0.conflict_dialog_open.set(false);
                                        let this_choice = this_c.clone();
                                        let v_choice = v_c.clone();
                                        let ed_choice = ed.clone();
                                        glib::spawn_future_local(async move {
                                            match choice {
                                                crate::editor::conflict::ConflictChoice::KeepMine => {
                                                    if let Err(e) = ed_choice.keep_mine(v_choice).await {
                                                        this_choice.toast(&format!("Error al conservar cambios: {e}"));
                                                    }
                                                }
                                                crate::editor::conflict::ConflictChoice::UseDisk => {
                                                    if let Err(e) = ed_choice.use_disk(v_choice).await {
                                                        this_choice.toast(&format!("Error al usar la versión del disco: {e}"));
                                                    }
                                                }
                                            }
                                        });
                                    },
                                );
                            }
                        }
                    }
                }
                let gone = changes
                    .iter()
                    .any(|c| matches!(c, orbital_vault::ExternalChange::Removed(p) if *p == open));
                if gone && editor.has_pending() {
                    if !this.0.conflict_dialog_open.get() {
                        this.0.conflict_dialog_open.set(true);
                        let name = open.file_name().to_string();
                        let this_c = this.clone();
                        let v_c = vault.clone();
                        let ed = editor.clone();
                        crate::editor::conflict::show_deleted_dialog(
                            &this.0.window,
                            &name,
                            move |choice| {
                                this_c.0.conflict_dialog_open.set(false);
                                let this_choice = this_c.clone();
                                let v_choice = v_c.clone();
                                let ed_choice = ed.clone();
                                glib::spawn_future_local(async move {
                                    match choice {
                                        crate::editor::conflict::DeletedChoice::SaveAgain => {
                                            if let Err(e) = ed_choice.keep_mine(v_choice).await {
                                                this_choice.toast(&format!(
                                                    "Error al guardar la nota: {e}"
                                                ));
                                            }
                                        }
                                        crate::editor::conflict::DeletedChoice::DiscardToBackup => {
                                            if let Err(e) =
                                                ed_choice.discard_to_backup(v_choice).await
                                            {
                                                this_choice.toast(&format!(
                                                    "Error al descartar a respaldo: {e}"
                                                ));
                                            }
                                        }
                                    }
                                });
                            },
                        );
                    }
                    return;
                }
            }
            if changes.is_empty() {
                return;
            }
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

    /// Renombra en segundo plano; si `undoable`, ofrece deshacer con el renombre inverso. Si la
    /// nota tiene enlaces entrantes, ofrece actualizarlos (US6 de la 003).
    async fn do_rename(&self, from: RelPath, to: RelPath, undoable: bool) {
        self.0.code_editor.flush().await;
        if self.0.code_editor.has_pending() {
            return;
        }
        let Some(vault) = self.0.vault.borrow().clone() else {
            return;
        };
        // Los enlaces que hoy apuntan a la nota (antes de renombrar: la resolución mira la bóveda
        // actual). Con el grafo armado se revisan solo sus retroenlaces; si no, toda la bóveda.
        let mut plan = None;
        if undoable && Format::from_name(from.file_name()).is_some() {
            let sources: Option<Vec<RelPath>> = self.0.graph.borrow().as_ref().and_then(|g| {
                g.lock().ok().map(|g| {
                    let mut v: Vec<RelPath> = g
                        .backlinks_for(&from)
                        .iter()
                        .map(|b| b.from.clone())
                        .collect();
                    v.sort();
                    v.dedup();
                    v
                })
            });
            let (v, f, t) = (vault.clone(), from.clone(), to.clone());
            let found =
                gio::spawn_blocking(move || v.plan_link_updates(&f, &t, sources.as_deref()))
                    .await
                    .unwrap_or_default();
            if !found.is_empty() {
                match self.ask_link_update(&from, &found).await {
                    LinkAnswer::Cancel => return,
                    LinkAnswer::Skip => {}
                    LinkAnswer::Update => plan = Some(Arc::new(found)),
                }
            }
        }
        let (f, t, p) = (from.clone(), to.clone(), plan.clone());
        let result = gio::spawn_blocking(move || {
            let ctx = write_ctx::now(Some(vault.config()));
            let renamed = vault.rename(&f, &t, &ctx)?;
            // Los enlaces se escriben después de renombrar (la nota puede enlazarse a sí misma).
            let (receipt, failed) = p.map_or_else(
                || (orbital_vault::WriteReceipt::default(), Vec::new()),
                |p| vault.apply_link_updates(&p, &ctx),
            );
            // La URL asociada migra con el archivo (009, FR-3); los errores van junto a los demás.
            let enlaces_err = vault.migrar_enlace(&f, &t, &ctx).err();
            Ok::<_, orbital_vault::VaultError>((renamed, receipt, failed, enlaces_err))
        })
        .await;
        match result {
            Ok(Ok((_, receipt, failed, enlaces_err))) => {
                let plan = plan.as_ref().map(|plan| Arc::new(plan.applied(&receipt)));
                // La ruta cambió: el editor vuelve a abrir la nota en su lugar nuevo.
                self.0.code_editor.close();
                self.reload(Some(to.clone())).await;
                if let Some(error) = enlaces_err {
                    self.toast(&format!("No se pudo migrar la URL asociada: {error}"));
                }
                if !failed.is_empty() {
                    let names: Vec<&str> = failed.iter().map(|(p, _)| p.file_name()).collect();
                    self.toast(&format!(
                        "No se actualizaron los enlaces de {} (cambiaron mientras tanto)",
                        names.join(", ")
                    ));
                }
                if undoable {
                    let this = self.clone();
                    let message = match &plan {
                        Some(p) => format!(
                            "«{}» ahora es «{to}»; {} {} actualizados",
                            from.file_name(),
                            p.links(),
                            if p.links() == 1 { "enlace" } else { "enlaces" }
                        ),
                        None => format!("«{}» ahora es «{to}»", from.file_name()),
                    };
                    self.toast_undo(&message, move || {
                        let (this, from, to, plan) =
                            (this.clone(), from.clone(), to.clone(), plan.clone());
                        glib::spawn_future_local(async move {
                            // Primero los enlaces, después el nombre: todo vuelve a ser igual.
                            let vault = this.0.vault.borrow().clone();
                            if let (Some(plan), Some(vault)) = (plan, vault) {
                                this.0.code_editor.flush().await;
                                let failed = gio::spawn_blocking(move || {
                                    let ctx = write_ctx::now(Some(vault.config()));
                                    vault.revert_link_updates(&plan, &ctx).1
                                })
                                .await
                                .unwrap_or_default();
                                if !failed.is_empty() {
                                    this.toast(
                                        "Algunos enlaces cambiaron después y no se revirtieron",
                                    );
                                }
                            }
                            this.do_rename(to, from, false).await;
                        });
                    });
                }
            }
            Ok(Err(e)) => self.toast(&e.to_string()),
            Err(_) => self.toast("No se pudo renombrar"),
        }
    }

    /// «Se encontraron N enlaces a esta nota en M archivos. ¿Querés actualizarlos?» (FR-018).
    async fn ask_link_update(
        &self,
        from: &RelPath,
        plan: &orbital_vault::LinkUpdatePlan,
    ) -> LinkAnswer {
        let (links, files) = (plan.links(), plan.files());
        let dialog = adw::AlertDialog::new(
            Some("¿Actualizar los enlaces?"),
            Some(&format!(
                "Se {} {links} {} a «{}» en {files} {}. ¿Querés actualizarlos para que sigan \
                 apuntando a la nota?",
                if links == 1 {
                    "encontró"
                } else {
                    "encontraron"
                },
                if links == 1 { "enlace" } else { "enlaces" },
                from.file_name(),
                if files == 1 { "archivo" } else { "archivos" },
            )),
        );
        dialog.add_response("cancel", "Cancelar");
        dialog.add_response("skip", "No actualizar");
        dialog.add_response("update", "Actualizar enlaces");
        dialog.set_response_appearance("update", adw::ResponseAppearance::Suggested);
        dialog.set_default_response(Some("update"));
        dialog.set_close_response("cancel");
        match dialog.choose_future(Some(&self.0.window)).await.as_str() {
            "update" => LinkAnswer::Update,
            "skip" => LinkAnswer::Skip,
            _ => LinkAnswer::Cancel,
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
        self.0.code_editor.flush().await;
        if self.0.code_editor.has_pending() {
            return;
        }
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
        self.0.code_editor.close();
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

    fn test_open_missing(&self) {
        let from = self
            .0
            .current_entry
            .borrow()
            .as_ref()
            .map(|e| e.path.clone());
        self.open_link(from.as_ref(), "[[nota-nueva-pendiente]]");
    }

    /// Muestra una nota por su ruta relativa (para el modo de captura).
    /// Abre un enlace externo o interno, resolviendo anclas de encabezado.
    pub fn open_link(&self, from: Option<&RelPath>, url: &str) {
        if let Some(tag) = url.strip_prefix("orbital-tag:") {
            self.search_tag(tag);
            return;
        }
        if url.starts_with("http://") || url.starts_with("https://") || url.starts_with("mailto:") {
            gtk::UriLauncher::new(url).launch(
                Some(&self.0.window),
                None::<&gio::Cancellable>,
                |_| {},
            );
            return;
        }

        let Some(vault) = self.0.vault.borrow().clone() else {
            return;
        };

        // Si viene de WebKit como file://, convertirlo a relativo a la bóveda
        let clean_url = if let Some(file_path) = url.strip_prefix("file://") {
            let decoded = orbital_vault::percent_decode(file_path);
            let p = std::path::Path::new(&decoded);
            if let Ok(rel) = p.strip_prefix(vault.root()) {
                rel.to_string_lossy().to_string()
            } else {
                decoded
            }
        } else {
            url.to_string()
        };

        let parsed = orbital_vault::parse_link_target(&clean_url);
        let target_str = if clean_url.starts_with("[[") {
            format!("[[{}]]", parsed.path)
        } else {
            parsed.path.clone()
        };

        let default_rel = RelPath::new("wiki/index.md").ok();
        let from_ref = from.or(default_rel.as_ref());
        let res = from_ref.map_or(orbital_vault::Resolution::Missing, |f| {
            vault.resolve_link(f, &target_str)
        });

        match res {
            orbital_vault::Resolution::Found(p) => {
                // Un PDF va al visor embebido, con su ancla de página/selección si la hay (016).
                if p.as_str().ends_with(".pdf") {
                    let ancla_txt = clean_url.split_once('#').map(|(_, a)| a).unwrap_or("");
                    let ancla = crate::visor_pdf::parse_ancla(ancla_txt);
                    self.ver_pdf(&p, &ancla);
                    return;
                }
                self.show_path(p.as_str());
                if let Some(anchor) = parsed.anchor {
                    let w_clone = self.clone();
                    glib::idle_add_local_once(move || {
                        w_clone.0.code_editor.scroll_to_heading(&anchor);
                    });
                }
            }
            orbital_vault::Resolution::Ambiguous(hits) => {
                if let Some(first) = hits.first() {
                    self.show_path(first.as_str());
                    self.toast(&format!("Enlace ambiguo: abriendo «{}»", first));
                }
            }
            orbital_vault::Resolution::Missing => {
                let note_name = parsed.path.trim().to_string();
                if note_name.is_empty() {
                    return;
                }
                let clean_name = note_name
                    .strip_suffix(".md")
                    .or_else(|| note_name.strip_suffix(".html"))
                    .unwrap_or(&note_name)
                    .to_string();

                let (dir_rel, file_name) = match clean_name.rsplit_once('/') {
                    Some((d, f)) => (format!("wiki/{d}"), f.to_string()),
                    None => ("wiki".to_string(), clean_name),
                };

                let Ok(dir) = RelPath::new(&dir_rel) else {
                    self.toast(&format!("Ruta inválida «{}»", dir_rel));
                    return;
                };

                let ask = vault.config().ask_create_missing_notes;
                if !ask {
                    self.create_missing_note(dir, file_name);
                    return;
                }

                let check = gtk::CheckButton::builder()
                    .label("No volver a preguntar")
                    .margin_top(12)
                    .build();

                let heading = format!("¿Crear la nota «{file_name}» en wiki/?");
                let dialog = adw::AlertDialog::builder()
                    .heading(&heading)
                    .body("El enlace apunta a una nota que todavía no existe en la bóveda.")
                    .extra_child(&check)
                    .default_response("create")
                    .close_response("cancel")
                    .build();
                dialog.add_response("cancel", "Cancelar");
                dialog.add_response("create", "Crear");
                dialog.set_response_appearance("create", adw::ResponseAppearance::Suggested);

                let this = self.clone();
                let v = vault.clone();
                dialog.choose(
                    Some(&self.0.window),
                    None::<&gio::Cancellable>,
                    move |response| {
                        if response == "create" {
                            if check.is_active() {
                                let mut cfg = v.config().clone();
                                cfg.ask_create_missing_notes = false;
                                let root = v.root().to_path_buf();
                                gio::spawn_blocking(move || {
                                    let _ = cfg.save(&root);
                                });
                            }
                            this.create_missing_note(dir, file_name);
                        }
                    },
                );
            }
        }
    }

    fn create_missing_note(&self, dir: RelPath, name: String) {
        let this = self.clone();
        let Some(vault) = self.0.vault.borrow().clone() else {
            return;
        };
        glib::spawn_future_local(async move {
            let n = name.clone();
            let v = vault.clone();
            let d = dir.clone();
            let result = gio::spawn_blocking(move || {
                let ctx = write_ctx::now(Some(v.config()));
                v.create_note(&d, &n, orbital_vault::Format::Md, &ctx)
            })
            .await;
            match result {
                Ok(Ok(path)) => {
                    this.reload(Some(path.clone())).await;
                    this.toast(&format!("Se creó «{path}»"));
                }
                Ok(Err(e)) => this.toast(&e.to_string()),
                Err(_) => this.toast("No se pudo crear la nota"),
            }
        });
    }

    pub fn show_path(&self, rel: &str) {
        // Elegir la fila la abre (ver `selection-changed`); si no está en la lista (por ejemplo,
        // archivada) o ya estaba elegida (otra coincidencia de la nota abierta, o volver desde el
        // grafo), se muestra directamente.
        if !self.reveal(rel)
            && let Some(e) = self.entry(rel)
        {
            self.show_entry(e);
        }
    }

    /// Expande las carpetas hasta `rel` y elige su fila. Devuelve si cambió la selección (y con
    /// eso `selection-changed` abre la nota).
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
                let changed = selection.selected() != pos;
                selection.set_selected(pos);
                self.0
                    .list_view
                    .scroll_to(pos, gtk::ListScrollFlags::NONE, None);
                changed
            }
            None => false,
        }
    }

    /// Desarrollo: latencia al escribir en la nota abierta (ver `Editor::measure_typing`).
    pub async fn measure_typing(&self, chars: usize) -> Vec<f64> {
        self.0.code_editor.measure_typing(chars).await
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

fn children_model(tree: &Tree, parent: &str, reversed: bool) -> gio::ListStore {
    let store = gio::ListStore::new::<gtk::StringObject>();
    if let Some(children) = tree.get(parent) {
        let mut items: Vec<gtk::StringObject> = children
            .iter()
            .map(|e| gtk::StringObject::new(e.path.as_str()))
            .collect();
        if reversed {
            items.reverse();
        }
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

/// Una sección plegable del explorador (maqueta: rótulo en mayúsculas con chevron). Devuelve la
/// sección y su botón de encabezado.
fn seccion_plegable(
    titulo: &str,
    contenido: &impl IsA<gtk::Widget>,
    abierta: bool,
    expandir: bool,
) -> (gtk::Box, gtk::Button) {
    let chevron = crate::iconos::Icono::new("abajo", 14);
    let rotulo = gtk::Label::builder()
        .label(titulo.to_uppercase())
        .xalign(0.0)
        .hexpand(true)
        .build();
    let fila = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    fila.append(&rotulo);
    fila.append(&chevron);
    let boton = gtk::Button::builder()
        .child(&fila)
        .css_classes(["flat", "orbital-section"])
        .build();
    boton.update_property(&[gtk::accessible::Property::Label(titulo)]);
    let revealer = gtk::Revealer::builder()
        .child(contenido)
        .reveal_child(abierta)
        .vexpand(expandir && abierta)
        .transition_type(gtk::RevealerTransitionType::SlideDown)
        .build();
    let marcar = {
        let chevron = chevron.clone();
        move |b: &gtk::Button, abierta: bool| {
            b.update_state(&[gtk::accessible::State::Expanded(Some(abierta))]);
            chevron.set_nombre(if abierta { "abajo" } else { "derecha" });
        }
    };
    marcar(&boton, abierta);
    let r = revealer.clone();
    boton.connect_clicked(move |b| {
        let abierta = !r.reveals_child();
        r.set_reveal_child(abierta);
        r.set_vexpand(expandir && abierta);
        marcar(b, abierta);
    });
    let seccion = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .css_classes(["orbital-explorer-section"])
        .build();
    seccion.append(&boton);
    seccion.append(&revealer);
    (seccion, boton)
}

/// Nombre accesible y tooltip de un botón de barra plegable, según esté abierta o no
/// («Ocultar barra izquierda (Ctrl+[)» / «Mostrar…»), como los `aria-label` de la maqueta.
fn nombre_de_barra(boton: &gtk::ToggleButton, barra: &'static str, atajo: &'static str) {
    let poner = move |b: &gtk::ToggleButton| {
        let verbo = if b.is_active() { "Ocultar" } else { "Mostrar" };
        let nombre = format!("{verbo} {barra}");
        b.set_tooltip_text(Some(&format!("{nombre} ({atajo})")));
        b.update_property(&[gtk::accessible::Property::Label(&nombre)]);
    };
    poner(boton);
    boton.connect_active_notify(poner);
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

/// Respuesta al diálogo de actualizar enlaces al renombrar.
enum LinkAnswer {
    Cancel,
    Skip,
    Update,
}

#[cfg(test)]
mod url_tests {
    use super::*;

    fn app_de_prueba(nombre: &str) -> adw::Application {
        adw::init().expect("se necesita acceso GTK");
        adw::Application::builder()
            .application_id(format!("io.github.caracelp.Orbital.UrlTest.{nombre}"))
            .flags(gio::ApplicationFlags::NON_UNIQUE)
            .build()
    }

    /// 009 US1/US2: abrir un archivo de raw/ asociado muestra el webview de la URL; sin
    /// asociación, la página de solo lectura con «Asociar URL…».
    #[test]
    #[ignore = "requiere GTK/DBus; los archivos de raw/ no son activables por AT-SPI"]
    fn us9_url_pages_for_raw_files() {
        let app = app_de_prueba("Url");
        app.register(None::<&gio::Cancellable>).unwrap();
        glib::MainContext::default().block_on(async {
            let root =
                std::env::temp_dir().join(format!("orbital-us9-{}", glib::uuid_string_random()));
            std::fs::create_dir_all(root.join("raw")).unwrap();
            // Un archivo binario (como un PDF real): read_note falla y va a la página externo.
            std::fs::write(root.join("raw/x.pdf"), b"%PDF-1.4\xff\xfe\x01binario").unwrap();
            std::fs::create_dir_all(root.join(".orbital")).unwrap();
            std::fs::write(
                root.join(".orbital/enlaces.yaml"),
                "- ruta: raw/x.pdf\n  url: https://ejemplo.local/x\n",
            )
            .unwrap();
            let vault = Arc::new(orbital_vault::open(&root).unwrap());
            let window = Window::new(&app);
            window.set_persist(false);
            window.show_vault(
                orbital_vault::open(&root).unwrap(),
                Tree::default(),
                orbital_vault::NoteMeta::default(),
            );
            // Abrir el archivo de raw/ por show_entry.
            let entry = Arc::new(
                vault
                    .entries()
                    .iter()
                    .find(|e| e.path.as_str() == "raw/x.pdf")
                    .cloned()
                    .expect("el archivo de raw/ existe"),
            );
            assert!(
                window.0.url_web.borrow().is_none(),
                "el webview de la URL no se crea al arrancar"
            );
            window.show_entry(entry);
            for _ in 0..100 {
                if window.0.content_stack.visible_child_name().as_deref() == Some("url-webview") {
                    break;
                }
                glib::timeout_future(std::time::Duration::from_millis(20)).await;
            }
            assert_eq!(
                window.0.content_stack.visible_child_name().as_deref(),
                Some("url-webview"),
                "el archivo asociado debe abrir el webview de la URL"
            );
            let web = window
                .0
                .url_web
                .borrow()
                .clone()
                .expect("webview creado al abrir");
            assert!(
                web.network_session().is_some_and(|s| s.is_ephemeral()),
                "la URL se carga con una sesión de red efímera"
            );
            // «Ver el archivo» vuelve a la vista de solo lectura.
            window.0.url_ver_archivo.emit_clicked();
            for _ in 0..50 {
                if window.0.content_stack.visible_child_name().as_deref() == Some("externo") {
                    break;
                }
                glib::timeout_future(std::time::Duration::from_millis(20)).await;
            }
            assert_eq!(
                window.0.content_stack.visible_child_name().as_deref(),
                Some("externo")
            );
            // Sin asociación: «Quitar URL» oculto y la página de solo lectura.
            *window.0.externo_ruta.borrow_mut() = None;
            window.0.quitar_url.set_visible(false);
            assert!(!window.0.quitar_url.is_visible());
            window.0.window.destroy();
            let _ = std::fs::remove_dir_all(root);
        });
    }
}

#[cfg(test)]
mod historial_tests {
    use super::*;

    /// Recorre el árbol de widgets del diálogo buscando la lista de versiones y el
    /// botón «Restaurar» (que cuelga de la cabecera, fuera del contenedor de la lista).
    fn encontrar_lista_y_boton(widget: &gtk::Widget) -> Option<(gtk::ListBox, gtk::Button)> {
        fn buscar(
            widget: &gtk::Widget,
            lista: &mut Option<gtk::ListBox>,
            boton: &mut Option<gtk::Button>,
        ) {
            if lista.is_none()
                && let Some(l) = widget.downcast_ref::<gtk::ListBox>()
            {
                *lista = Some(l.clone());
            }
            if boton.is_none()
                && let Some(b) = widget.downcast_ref::<gtk::Button>()
                && b.label().as_deref() == Some("Restaurar")
            {
                *boton = Some(b.clone());
            }
            let mut hijo = widget.first_child();
            while let Some(w) = hijo {
                buscar(&w, lista, boton);
                hijo = w.next_sibling();
            }
        }
        let mut lista = None;
        let mut boton = None;
        buscar(widget, &mut lista, &mut boton);
        Some((lista?, boton?))
    }

    #[test]
    #[ignore = "requiere GTK/DBus; tools/verificar-010-us1.sh"]
    fn us2_historial_dialogo() {
        adw::init().expect("se necesita acceso a GTK");
        let app = adw::Application::builder()
            .application_id("io.github.caracelp.Orbital.HistorialTest")
            .flags(gio::ApplicationFlags::NON_UNIQUE)
            .build();
        app.register(None::<&gio::Cancellable>).unwrap();
        glib::MainContext::default().block_on(async {
            let root = std::env::temp_dir()
                .join(format!("orbital-us2-010-{}", glib::uuid_string_random()));
            std::fs::create_dir(&root).unwrap();
            std::fs::create_dir(root.join("wiki")).unwrap();
            let path = RelPath::new("wiki/nota.md").unwrap();
            std::fs::write(path.to_path(&root), "Versión tres\n").unwrap();
            let vault = Arc::new(orbital_vault::open(&root).unwrap());
            let window = Window::new(&app);
            window.set_persist(false);
            *window.0.vault.borrow_mut() = Some(vault.clone());
            window.0.vault_session.set(1);
            let editor = window.0.code_editor.clone();
            editor.load(vault.clone(), &vault.read_note(&path).unwrap());

            // Dos versiones guardadas (la más reciente por último en disco).
            orbital_vault::historial::guardar_version(&vault, &path, "Versión inicial\n", "")
                .unwrap();
            glib::timeout_future(std::time::Duration::from_millis(5)).await;
            orbital_vault::historial::guardar_version(&vault, &path, "Versión dos\n", "").unwrap();
            let versiones = orbital_vault::versiones_historial(&vault, &path).unwrap();
            assert_eq!(versiones.len(), 2);
            assert_eq!(versiones[0].1, "Versión dos\n", "la más reciente primero");
            assert_eq!(versiones[1].1, "Versión inicial\n");

            let dialog = window.dialogo_historial(&path, &versiones);
            dialog.present(Some(&window.0.window));
            glib::timeout_future(std::time::Duration::from_millis(50)).await;
            let raiz = dialog.child().expect("el diálogo tiene contenido");
            let (lista, restaurar) =
                encontrar_lista_y_boton(&raiz).expect("el diálogo expone lista y botón Restaurar");
            assert!(
                !restaurar.is_sensitive(),
                "Restaurar sin selección está desactivado"
            );
            assert_eq!(lista.row_at_index(0).map(|r| r.index()), Some(0));
            assert_eq!(lista.observe_children().n_items(), 2);

            // Seleccionar la fila más vieja y restaurar: el texto del editor cambia, el
            // archivo en disco no (restaurar deja una edición pendiente, como al escribir).
            lista.select_row(Some(&lista.row_at_index(1).unwrap()));
            assert!(restaurar.is_sensitive());
            restaurar.emit_clicked();
            assert_eq!(editor.text(), "Versión inicial\n");
            assert_eq!(
                std::fs::read_to_string(path.to_path(&root)).unwrap(),
                "Versión tres\n"
            );

            window.0.window.destroy();
            std::fs::remove_dir_all(root).unwrap();
        });
    }
}

#[cfg(test)]
mod tag_rename_tests {
    use super::*;

    #[test]
    #[ignore = "requiere GTK/DBus; tools/verificar-004-us6.sh"]
    fn us6_tag_rename_buffers() {
        adw::init().expect("se necesita acceso a GTK");
        let app = adw::Application::builder()
            .application_id("io.github.caracelp.Orbital.TagRenameTest")
            .flags(gio::ApplicationFlags::NON_UNIQUE)
            .build();
        app.register(None::<&gio::Cancellable>).unwrap();
        glib::MainContext::default().block_on(async {
            let root =
                std::env::temp_dir().join(format!("orbital-us6-{}", glib::uuid_string_random()));
            std::fs::create_dir(&root).unwrap();
            std::fs::create_dir(root.join("wiki")).unwrap();
            let path = RelPath::new("wiki/nota.md").unwrap();
            std::fs::write(path.to_path(&root), "#vieja\n").unwrap();
            let vault = Arc::new(orbital_vault::open(&root).unwrap());
            let window = Window::new(&app);
            window.set_persist(false);
            *window.0.vault.borrow_mut() = Some(vault.clone());
            window.0.vault_session.set(1);
            let editor = window.0.code_editor.clone();
            editor.load(vault.clone(), &vault.read_note(&path).unwrap());
            let stale = vault.plan_tag_rename("vieja", "nueva", std::slice::from_ref(&path));
            editor.replace_text("#vieja con mis cambios\n");
            window
                .run_tag_edits(vault.clone(), 1, Arc::new(stale), false)
                .await;
            assert_eq!(
                std::fs::read_to_string(path.to_path(&root)).unwrap(),
                "#vieja con mis cambios\n"
            );
            assert_eq!(editor.text(), "#vieja con mis cambios\n");

            let plan =
                Arc::new(vault.plan_tag_rename("vieja", "nueva", std::slice::from_ref(&path)));
            window
                .run_tag_edits(vault.clone(), 1, plan.clone(), false)
                .await;
            assert_eq!(editor.text(), "#nueva con mis cambios\n");
            let receipt = orbital_vault::WriteReceipt {
                written: window
                    .0
                    .own_writes
                    .borrow()
                    .iter()
                    .map(|(path, hash)| (path.clone(), *hash))
                    .collect(),
            };
            let applied = Arc::new(plan.applied(&receipt));
            assert_eq!(applied.files(), 1);
            editor.replace_text("#nueva con otros cambios\n");
            window
                .run_tag_edits(vault.clone(), 1, applied.clone(), true)
                .await;
            assert_eq!(
                std::fs::read_to_string(path.to_path(&root)).unwrap(),
                "#nueva con otros cambios\n"
            );
            assert_eq!(editor.text(), "#nueva con otros cambios\n");
            // Una nueva sesión no puede consumir el Deshacer de la anterior.
            window.0.vault_session.set(2);
            window.run_tag_edits(vault, 1, applied, true).await;
            assert_eq!(
                std::fs::read_to_string(path.to_path(&root)).unwrap(),
                "#nueva con otros cambios\n"
            );
            window.0.window.destroy();
            std::fs::remove_dir_all(root).unwrap();
        });
    }
}

#[cfg(test)]
mod search_open_tests {
    use super::*;

    async fn esperar(cond: impl Fn() -> bool) {
        for _ in 0..150 {
            if cond() {
                return;
            }
            glib::timeout_future(std::time::Duration::from_millis(20)).await;
        }
    }

    /// 004 escenario 1.3: elegir una coincidencia abre la nota en su línea, también cuando la
    /// nota ya está elegida en la lista (otra coincidencia de la misma nota, o volver del grafo).
    #[test]
    #[ignore = "requiere GTK/DBus; tools/verificar-004-us1.sh"]
    fn us1_open_match_same_note() {
        adw::init().expect("se necesita acceso a GTK");
        let app = adw::Application::builder()
            .application_id("io.github.caracelp.Orbital.SearchOpenTest")
            .flags(gio::ApplicationFlags::NON_UNIQUE)
            .build();
        app.register(None::<&gio::Cancellable>).unwrap();
        glib::MainContext::default().block_on(async {
            let root = std::env::temp_dir()
                .join(format!("orbital-us1-004-{}", glib::uuid_string_random()));
            std::fs::create_dir_all(root.join("wiki")).unwrap();
            std::fs::create_dir_all(root.join("raw")).unwrap();
            let texto: String = (1..=80).map(|n| format!("línea {n}\n\n")).collect();
            std::fs::write(root.join("wiki/nota.md"), &texto).unwrap();
            let window = Window::new(&app);
            window.set_persist(false);
            let (vault, tree, meta) = {
                let v = orbital_vault::open(&root).unwrap();
                let entries: Vec<_> = v.entries().to_vec();
                let tree = build_tree(&entries);
                (v, tree, orbital_vault::NoteMeta::default())
            };
            window.show_vault(vault, tree, meta);
            // El índice de búsqueda se arma después del primer cuadro: la ventana tiene que verse.
            window.0.window.present();
            let abrir = |line: usize| {
                let path = RelPath::new("wiki/nota.md").unwrap();
                window.0.pending_line.set(Some((path.clone(), line)));
                window.show_path(path.as_str());
            };
            let linea_cursor = || window.0.code_editor.cursor_line();
            abrir(3);
            esperar(|| window.0.content_stack.visible_child_name().as_deref() == Some("codigo"))
                .await;
            esperar(|| linea_cursor() == 3).await;
            assert_eq!(linea_cursor(), 3, "la primera coincidencia va a su línea");

            // Otra coincidencia de la misma nota: la fila ya está elegida.
            abrir(101);
            esperar(|| linea_cursor() == 101).await;
            assert_eq!(
                linea_cursor(),
                101,
                "una coincidencia de la nota abierta va a su línea"
            );

            // Desde el grafo, la misma nota vuelve a verse.
            window.0.content_stack.set_visible_child_name("grafo");
            abrir(51);
            esperar(|| window.0.content_stack.visible_child_name().as_deref() == Some("codigo"))
                .await;
            assert_eq!(
                window.0.content_stack.visible_child_name().as_deref(),
                Some("codigo"),
                "elegir una coincidencia vuelve al editor"
            );
            esperar(|| linea_cursor() == 51).await;
            assert_eq!(linea_cursor(), 51);

            // El camino real: buscar en el panel y activar la fila (clic o Intro) abre la línea.
            window.0.search_panel.set_search_query("\"línea 70\"");
            let mut activada = false;
            for _ in 0..250 {
                if window.0.search_panel.activate_match("línea 70") {
                    activada = true;
                    break;
                }
                glib::timeout_future(std::time::Duration::from_millis(20)).await;
            }
            assert!(activada, "el panel muestra la coincidencia");
            esperar(|| linea_cursor() == 139).await;
            assert_eq!(
                linea_cursor(),
                139,
                "activar la fila va a la línea de la coincidencia"
            );
            window.0.window.destroy();
            std::fs::remove_dir_all(root).unwrap();
        });
    }
}

#[cfg(test)]
mod pestanas_tests {
    use super::*;
    use crate::pestanas::CONEXIONES;

    async fn esperar(cond: impl Fn() -> bool) {
        for _ in 0..150 {
            if cond() {
                return;
            }
            glib::timeout_future(std::time::Duration::from_millis(20)).await;
        }
    }

    /// 025 US3: abrir notas crea pestañas; elegir una repone su nota con cursor y vista;
    /// «Conexiones» es una pestaña; cerrar la última deja el centro vacío.
    #[test]
    #[ignore = "requiere GTK/DBus"]
    fn us3_pestanas_editor_compartido() {
        adw::init().expect("se necesita acceso a GTK");
        let app = adw::Application::builder()
            .application_id("io.github.caracelp.Orbital.PestanasTest")
            .flags(gio::ApplicationFlags::NON_UNIQUE)
            .build();
        app.register(None::<&gio::Cancellable>).unwrap();
        glib::MainContext::default().block_on(async {
            let root =
                std::env::temp_dir().join(format!("orbital-025-{}", glib::uuid_string_random()));
            std::fs::create_dir_all(root.join("wiki")).unwrap();
            std::fs::create_dir_all(root.join("raw")).unwrap();
            let texto: String = (1..=60).map(|n| format!("línea {n}\n")).collect();
            std::fs::write(root.join("wiki/a.md"), &texto).unwrap();
            std::fs::write(root.join("wiki/b.md"), &texto).unwrap();
            let window = Window::new(&app);
            window.set_persist(false);
            let v = orbital_vault::open(&root).unwrap();
            let tree = build_tree(v.entries());
            window.show_vault(v, tree, orbital_vault::NoteMeta::default());
            window.0.window.present();
            let ruta = || window.0.code_editor.current_path().map(|p| p.to_string());

            window.show_path("wiki/a.md");
            esperar(|| ruta().as_deref() == Some("wiki/a.md")).await;
            window.show_path("wiki/b.md");
            esperar(|| ruta().as_deref() == Some("wiki/b.md")).await;
            assert_eq!(window.0.pestanas.claves(), ["wiki/a.md", "wiki/b.md"]);
            assert_eq!(window.0.pestanas.actual().as_deref(), Some("wiki/b.md"));

            // Cursor en la línea 40 de b; ir a a y volver repone el cursor y la vista.
            window.0.code_editor.restaurar_vista(
                window
                    .0
                    .code_editor
                    .buffer()
                    .iter_at_line(39)
                    .unwrap()
                    .offset(),
                0.0,
                Some("dividida"),
            );
            window.0.pestanas.elegir("wiki/a.md");
            esperar(|| ruta().as_deref() == Some("wiki/a.md")).await;
            assert_eq!(ruta().as_deref(), Some("wiki/a.md"));
            window.0.pestanas.elegir("wiki/b.md");
            esperar(|| ruta().as_deref() == Some("wiki/b.md")).await;
            esperar(|| window.0.code_editor.cursor_line() == 40).await;
            assert_eq!(window.0.code_editor.cursor_line(), 40, "cursor repuesto");
            assert_eq!(
                window.0.code_editor.estado_vista().2.as_deref(),
                Some("dividida"),
                "vista repuesta"
            );

            // Conexiones como pestaña; cerrarla vuelve a la nota.
            window.ver_grafo();
            assert_eq!(
                window.0.pestanas.claves(),
                ["wiki/a.md", "wiki/b.md", CONEXIONES]
            );
            assert_eq!(
                window.0.content_stack.visible_child_name().as_deref(),
                Some("grafo")
            );
            window.cerrar_grafo();
            esperar(|| window.0.content_stack.visible_child_name().as_deref() == Some("codigo"))
                .await;
            assert_eq!(window.0.pestanas.claves(), ["wiki/a.md", "wiki/b.md"]);

            // Cerrar la pestaña abierta abre la vecina; cerrar la última deja el centro vacío.
            window.0.pestanas.cerrar("wiki/b.md");
            esperar(|| ruta().as_deref() == Some("wiki/a.md")).await;
            assert_eq!(ruta().as_deref(), Some("wiki/a.md"));
            window.0.pestanas.cerrar("wiki/a.md");
            esperar(|| window.0.content_stack.visible_child_name().as_deref() == Some("vacio"))
                .await;
            assert_eq!(
                window.0.content_stack.visible_child_name().as_deref(),
                Some("vacio")
            );
            assert!(ruta().is_none());

            // Una nota borrada afuera pierde su pestaña al recargar la lista.
            window.show_path("wiki/a.md");
            esperar(|| ruta().as_deref() == Some("wiki/a.md")).await;
            window.show_path("wiki/b.md");
            esperar(|| ruta().as_deref() == Some("wiki/b.md")).await;
            std::fs::remove_file(root.join("wiki/a.md")).unwrap();
            let v = orbital_vault::open(&root).unwrap();
            let tree = build_tree(v.entries());
            window.show_vault(v, tree, orbital_vault::NoteMeta::default());
            assert_eq!(window.0.pestanas.claves(), ["wiki/b.md"]);
            window.0.window.destroy();
            std::fs::remove_dir_all(root).unwrap();
        });
    }
}

#[cfg(test)]
mod medicion_pestanas {
    use super::*;

    /// SC-003 de la 025: cambiar de pestaña entre dos notas de 100 KB (hasta que el editor
    /// tiene la otra nota cargada y a la vista). En release:
    /// `cargo test --release --bin orbital sc3_cambio_pestana -- --ignored --nocapture`.
    #[test]
    #[ignore = "medición; requiere GTK"]
    fn sc3_cambio_pestana() {
        adw::init().expect("se necesita acceso a GTK");
        let app = adw::Application::builder()
            .application_id("io.github.caracelp.Orbital.MedirPestanas")
            .flags(gio::ApplicationFlags::NON_UNIQUE)
            .build();
        app.register(None::<&gio::Cancellable>).unwrap();
        glib::MainContext::default().block_on(async {
            let root =
                std::env::temp_dir().join(format!("orbital-sc3-{}", glib::uuid_string_random()));
            std::fs::create_dir_all(root.join("wiki")).unwrap();
            std::fs::create_dir_all(root.join("raw")).unwrap();
            let mut texto = String::from("---\ntype: atomo\n---\n# Grande\n\n");
            let mut n = 0;
            while texto.len() < 100 * 1024 {
                texto.push_str(&format!(
                    "Párrafo {n} con texto de prueba y una #etiqueta y [[otra]].\n\n"
                ));
                n += 1;
            }
            std::fs::write(root.join("wiki/a.md"), &texto).unwrap();
            std::fs::write(root.join("wiki/b.md"), &texto).unwrap();
            let window = Window::new(&app);
            window.set_persist(false);
            let v = orbital_vault::open(&root).unwrap();
            let tree = build_tree(v.entries());
            window.show_vault(v, tree, orbital_vault::NoteMeta::default());
            window.0.window.present();
            let ruta = || window.0.code_editor.current_path().map(|p| p.to_string());
            for nota in ["wiki/a.md", "wiki/b.md"] {
                window.show_path(nota);
                while ruta().as_deref() != Some(nota) {
                    glib::timeout_future(std::time::Duration::from_millis(5)).await;
                }
            }
            glib::timeout_future(std::time::Duration::from_millis(500)).await;
            let mut ms = Vec::new();
            for k in 0..10 {
                let nota = if k % 2 == 0 { "wiki/a.md" } else { "wiki/b.md" };
                let t = std::time::Instant::now();
                window.0.pestanas.elegir(nota);
                while ruta().as_deref() != Some(nota) {
                    glib::timeout_future(std::time::Duration::from_millis(1)).await;
                }
                ms.push(t.elapsed().as_secs_f64() * 1000.0);
                glib::timeout_future(std::time::Duration::from_millis(200)).await;
            }
            ms.sort_by(f64::total_cmp);
            println!(
                "PESTANA 100 KB: mediana {:.1} ms, máx {:.1} ms",
                ms[ms.len() / 2],
                ms[ms.len() - 1]
            );
            window.0.window.destroy();
            std::fs::remove_dir_all(root).unwrap();
        });
    }
}

#[cfg(test)]
mod captura_v13 {
    use super::*;

    /// Captura de la interfaz v13 a 1440×900 con una nota y cada modo de la barra derecha:
    /// `ORBITAL_CAPTURA_BOVEDA=<bóveda> ORBITAL_CAPTURA_NOTA=<ruta> ORBITAL_CAPTURA_DIR=<carpeta>`.
    #[test]
    #[ignore = "captura de desarrollo; requiere GTK"]
    fn captura_modos_panel() {
        let (Some(boveda), Some(nota), Some(dir)) = (
            std::env::var_os("ORBITAL_CAPTURA_BOVEDA"),
            std::env::var("ORBITAL_CAPTURA_NOTA").ok(),
            std::env::var_os("ORBITAL_CAPTURA_DIR"),
        ) else {
            return;
        };
        adw::init().expect("se necesita acceso a GTK");
        crate::style::apply();
        let app = adw::Application::builder()
            .application_id("io.github.caracelp.Orbital.CapturaV13")
            .flags(gio::ApplicationFlags::NON_UNIQUE)
            .build();
        app.register(None::<&gio::Cancellable>).unwrap();
        glib::MainContext::default().block_on(async {
            let window = Window::new(&app);
            window.set_persist(false);
            window.0.window.set_default_size(1440, 900);
            let v = orbital_vault::open(std::path::Path::new(&boveda)).unwrap();
            let tree = build_tree(v.entries());
            window.show_vault(v, tree, orbital_vault::NoteMeta::default());
            window.0.window.present();
            for _ in 0..100 {
                if window.0.graph.borrow().is_some() {
                    break;
                }
                glib::timeout_future(std::time::Duration::from_millis(30)).await;
            }
            window.show_path(&nota);
            glib::timeout_future(std::time::Duration::from_millis(900)).await;
            for modo in ["propiedades", "contexto", "grafo"] {
                window.0.panel.set_modo(modo);
                glib::timeout_future(std::time::Duration::from_millis(700)).await;
                let ancho = window.0.window.width();
                let alto = window.0.window.height();
                let pintable = gtk::WidgetPaintable::new(Some(&window.0.window));
                let snap = gtk::Snapshot::new();
                pintable.snapshot(&snap, f64::from(ancho), f64::from(alto));
                let node = snap.to_node().expect("cuadro");
                let png = std::path::Path::new(&dir).join(format!("v13-{modo}.png"));
                window
                    .0
                    .window
                    .renderer()
                    .unwrap()
                    .render_texture(&node, None)
                    .save_to_png(png)
                    .unwrap();
            }
            window.0.window.destroy();
        });
    }
}
