//! Esqueleto del diseño de interfaz (025): barras plegables, modos de la barra izquierda, marca, cabecera única, riel de navegación y barra inferior.

use super::*;

/// Lo que arma [`construir_esqueleto`] y usa el resto de [`Window::new`].
pub(super) struct Esqueleto {
    pub(super) split: adw::OverlaySplitView,
    pub(super) status: crate::status_bar::StatusBar,
    pub(super) root_view: adw::ToolbarView,
}

pub(super) fn construir_esqueleto(
    backlinks_button: gtk::ToggleButton,
    centro: adw::OverlaySplitView,
    sidebar_stack: adw::ViewStack,
    sidebar_view: adw::ToolbarView,
) -> Esqueleto {
    // --- Esqueleto del diseño de interfaz (025) ---
    // Barra izquierda (260 px) y barra derecha (280 px) plegables; en ventanas angostas se
    // superponen al centro (breakpoint de abajo).
    let split = adw::OverlaySplitView::builder()
        .sidebar(&sidebar_view)
        .content(&centro)
        .min_sidebar_width(260.0)
        .max_sidebar_width(260.0)
        .sidebar_width_fraction(1.0)
        .build();
    split.add_css_class("orbital-split");
    let left_toggle = crate::iconos::boton_alternable("barra-izquierda", "Ocultar barra izquierda");
    // La barra manda el estado inicial al botón (no al revés).
    split
        .bind_property("show-sidebar", &left_toggle, "active")
        .bidirectional()
        .sync_create()
        .build();
    nombre_de_barra(&left_toggle, "barra izquierda", "Ctrl+[");

    // Modos de la barra izquierda, alineados sobre ella: Explorador, Búsqueda y Guardados (027).
    let modes = gtk::Box::builder()
        .spacing(8)
        .css_classes(["orbital-left-modes"])
        .build();
    modes.update_property(&[gtk::accessible::Property::Label("Modo del panel izquierdo")]);
    let mode_files = crate::iconos::boton_alternable("crear-carpeta", "Explorador de archivos");
    let mode_search = crate::iconos::boton_alternable("buscar", "Búsqueda");
    mode_search.set_group(Some(&mode_files));
    let mode_saved = crate::iconos::boton_alternable("marcador", "Guardados");
    mode_saved.set_group(Some(&mode_files));
    mode_files.set_active(true);
    for (b, page) in [
        (&mode_files, "archivos"),
        (&mode_search, "busqueda"),
        (&mode_saved, "guardados"),
    ] {
        let stack = sidebar_stack.clone();
        b.connect_toggled(move |b| {
            if b.is_active() {
                stack.set_visible_child_name(page);
            }
        });
    }
    {
        let (f, se, sa) = (mode_files.clone(), mode_search.clone(), mode_saved.clone());
        sidebar_stack.connect_visible_child_name_notify(move |st| {
            match st.visible_child_name().as_deref() {
                Some("busqueda") => se.set_active(true),
                Some("guardados") => sa.set_active(true),
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

    // Minimizar, maximizar y cerrar, aunque el sistema muestre solo cerrar (GNOME por defecto).
    let main_header = adw::HeaderBar::builder()
        .css_classes(["orbital-top"])
        .decoration_layout(":minimize,maximize,close")
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
        ("db", "Crear base de datos", Some("win.new-base")),
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
    let brujula_btn = crate::iconos::boton("brujula", "Brújula");
    brujula_btn.set_action_name(Some("win.ver-brujula"));
    rail.append(&brujula_btn);
    // Habilitar el botón Canva del riel (038).
    let mut hijo_riel = rail.first_child();
    while let Some(w) = hijo_riel {
        if let Some(b) = w.downcast_ref::<gtk::Button>()
            && b.tooltip_text()
                .is_some_and(|t| t.starts_with("Crear canva"))
        {
            b.set_sensitive(true);
            b.set_tooltip_text(Some("Crear canva"));
            b.update_property(&[gtk::accessible::Property::Label("Crear canva")]);
            b.set_action_name(Some("win.nuevo-canvas"));
        }
        hijo_riel = w.next_sibling();
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
    Esqueleto {
        split,
        status,
        root_view,
    }
}
