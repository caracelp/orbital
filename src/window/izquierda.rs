//! Barra izquierda: árbol de la bóveda, búsqueda, etiquetas, secciones del explorador, cabecera de acciones y pie de bóveda.

use super::*;

/// Lo que arma [`construir_izquierda`] y usa el resto de [`Window::new`].
pub(super) struct Izquierda {
    pub(super) list_view: gtk::ListView,
    pub(super) search_panel: crate::search_panel::SearchPanel,
    pub(super) tag_panel: crate::tag_panel::TagPanel,
    pub(super) recent_list: gtk::Box,
    pub(super) sidebar_stack: adw::ViewStack,
    pub(super) banner: adw::Banner,
    pub(super) new_button: gtk::MenuButton,
    pub(super) pie_boveda: crate::pie_boveda::PieBoveda,
    pub(super) sidebar_view: adw::ToolbarView,
    pub(super) gone_banner: adw::Banner,
    pub(super) listas_panel: crate::listas_panel::ListasPanel,
}

pub(super) fn construir_izquierda() -> Izquierda {
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
    // --- Explorador (025): secciones plegables Favoritos, Explorador, Etiquetas y Recientes ---
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
    // Favoritos (027) arriba de Explorador; su contenido y la página Guardados son de
    // `ListasPanel`, con el mismo estilo de sección.
    let listas_panel = crate::listas_panel::ListasPanel::new(seccion_plegable);
    let (favorites_section, _) =
        seccion_plegable("Favoritos", listas_panel.favoritos(), true, false);
    // Las secciones de abajo comparten un desplazamiento que puede encogerse: el alto
    // mínimo de la barra no depende de cuántas etiquetas o notas recientes haya (si no, la
    // ventana se pasa de alto y GTK la desacomoda).
    let lower = gtk::Box::new(gtk::Orientation::Vertical, 0);
    lower.append(&tags_section);
    lower.append(&recent_section);
    let lower_scroll = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .propagate_natural_height(true)
        .max_content_height(260)
        .child(&lower)
        .build();
    let explorer = gtk::Box::new(gtk::Orientation::Vertical, 0);
    files_section.set_vexpand(true);
    explorer.append(&favorites_section);
    explorer.append(&files_section);
    explorer.append(&lower_scroll);

    let sidebar_stack = adw::ViewStack::new();
    sidebar_stack.add_named(&explorer, Some("archivos"));
    sidebar_stack.add_named(search_panel.widget(), Some("busqueda"));
    // Guardados (027): la página existe vacía; su contenido se arma al mostrarla por primera vez.
    sidebar_stack.add_named(listas_panel.guardados(), Some("guardados"));

    let banner = adw::Banner::builder()
        .title("Estructura incompleta")
        .button_label("Completar…")
        .action_name("win.complete-structure")
        .revealed(true)
        .visible(false)
        .build();

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
    let pie_boveda = crate::pie_boveda::PieBoveda::new(&vault_button);
    let sidebar_view = adw::ToolbarView::new();
    sidebar_view.add_css_class("orbital-left");
    sidebar_view.add_top_bar(&left_head);
    sidebar_view.add_top_bar(&banner);
    sidebar_view.add_bottom_bar(pie_boveda.widget());
    let gone_banner = adw::Banner::builder()
        .title("La bóveda no está disponible. Orbital no va a escribir nada.")
        .revealed(true)
        .visible(false)
        .build();
    sidebar_view.add_top_bar(&gone_banner);
    sidebar_view.set_content(Some(&sidebar_stack));
    Izquierda {
        list_view,
        search_panel,
        tag_panel,
        recent_list,
        sidebar_stack,
        banner,
        new_button,
        pie_boveda,
        sidebar_view,
        gone_banner,
        listas_panel,
    }
}
