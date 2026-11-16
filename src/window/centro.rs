//! Centro de la ventana: visor y editor de notas, páginas de HTML, archivos externos, URL y grafo, barra de la nota, pestañas, avisos y barra derecha.

use super::*;

/// Lo que arma [`construir_centro`] y usa el resto de [`Window::new`].
pub(super) struct Centro {
    pub(super) editor: gtk::TextView,
    pub(super) props_list: gtk::ListBox,
    pub(super) content_stack: gtk::Stack,
    pub(super) code_editor: Editor,
    pub(super) html_props: gtk::ListBox,
    pub(super) html_page: gtk::Box,
    pub(super) asociar_url: gtk::Button,
    pub(super) quitar_url: gtk::Button,
    pub(super) importar_md: gtk::Button,
    pub(super) ver_pdf: gtk::Button,
    pub(super) external_page: adw::StatusPage,
    pub(super) url_ver_archivo: gtk::Button,
    pub(super) url_abrir: gtk::Button,
    pub(super) url_page: gtk::Box,
    pub(super) grafo_vista: crate::grafo::VistaGrafo,
    pub(super) grafo_cerrar: gtk::Button,
    pub(super) content_title: adw::WindowTitle,
    pub(super) pestanas: crate::pestanas::Pestanas,
    pub(super) type_banner: adw::Banner,
    pub(super) note_banner: adw::Banner,
    pub(super) right: crate::editor::right_sidebar::RightSidebar,
    pub(super) panel: crate::panel_derecho::PanelDerecho,
    pub(super) right_split: adw::OverlaySplitView,
    pub(super) backlinks_button: gtk::ToggleButton,
    pub(super) note_menu: gtk::Popover,
}

pub(super) fn construir_centro() -> Centro {
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

    // Grafo de enlaces (008, 030): página a pantalla completa con el canvas propio y controles
    // discretos de simulación física (Pausar/Reanudar y Recalcular).
    let grafo_vista = crate::grafo::VistaGrafo::new();
    let grafo_aviso = gtk::Label::builder()
        .label("")
        .xalign(0.0)
        .margin_start(12)
        .css_classes(["dim-label", "caption"])
        .build();
    let grafo_pausar = gtk::Button::builder()
        .icon_name("media-playback-pause-symbolic")
        .tooltip_text("Pausar la simulación")
        .css_classes(["flat"])
        .build();
    grafo_pausar.update_property(&[gtk::accessible::Property::Label("Pausar la simulación")]);
    let grafo_recalcular = gtk::Button::builder()
        .icon_name("view-refresh-symbolic")
        .tooltip_text("Recalcular disposición")
        .css_classes(["flat"])
        .build();
    grafo_recalcular.update_property(&[gtk::accessible::Property::Label("Recalcular disposición")]);
    {
        let b_pausar = grafo_pausar.clone();
        grafo_vista.set_on_estado_fisica(move |pausado| {
            let (icono, rotulo) = if pausado {
                ("media-playback-start-symbolic", "Reanudar la simulación")
            } else {
                ("media-playback-pause-symbolic", "Pausar la simulación")
            };
            b_pausar.set_icon_name(icono);
            b_pausar.set_tooltip_text(Some(rotulo));
            b_pausar.update_property(&[gtk::accessible::Property::Label(rotulo)]);
        });
        let vista_p = grafo_vista.clone();
        grafo_pausar.connect_clicked(move |_| {
            vista_p.pausar_reanudar();
        });
        let vista_r = grafo_vista.clone();
        grafo_recalcular.connect_clicked(move |_| {
            vista_r.recalcular();
        });
    }
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
    grafo_barra.pack_end(&grafo_recalcular);
    grafo_barra.pack_end(&grafo_pausar);
    grafo_barra.pack_start(&grafo_aviso);
    let grafo_page = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .build();
    grafo_page.append(&grafo_barra);
    grafo_page.append(&grafo_vista);
    grafo_vista.set_vexpand(true);
    content_stack.add_named(&grafo_page, Some("grafo"));

    // Brújula de la nota abierta (017): página en el centro activada desde el riel izquierdo.
    let brujula_vista = crate::grafo::brujula::VistaBrujula::new();
    content_stack.add_named(brujula_vista.widget(), Some("brujula"));

    // Editor de Canvas (038): página en el centro para archivos `.canvas`.
    let canvas_vista = crate::grafo::canvas::VistaCanvas::new();
    content_stack.add_named(canvas_vista.widget(), Some("canvas"));

    // Mapa mental de la nota abierta (039).
    let mapa_mental_vista = crate::grafo::mapa_mental::VistaMapaMental::new();
    content_stack.add_named(mapa_mental_vista.widget(), Some("mapa-mental"));

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
            ("Agregar a favoritos", "win.alternar-favorito", ""),
            ("Guardar en…", "win.guardar-en", ""),
            ("Ver como mapa mental", "win.ver-mapa-mental", ""),
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
    // `listas.rs` lo rearma al abrirse, con el estado de Favoritos y Guardados (027).
    let note_menu = note_menu_button
        .popover()
        .expect("el menú de la nota tiene popover");
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
    // La barra derecha va al lado de toda la columna central (pestañas y barra de la nota
    // incluidas): llega hasta la barra superior.
    content_view.set_content(Some(&content_stack));
    let right_split = adw::OverlaySplitView::builder()
        .content(&content_view)
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
    Centro {
        editor,
        props_list,
        content_stack,
        code_editor,
        html_props,
        html_page,
        asociar_url,
        quitar_url,
        importar_md,
        ver_pdf,
        external_page,
        url_ver_archivo,
        url_abrir,
        url_page,
        grafo_vista,
        grafo_cerrar,
        content_title,
        pestanas,
        type_banner,
        note_banner,
        right,
        panel,
        right_split,
        backlinks_button,
        note_menu,
    }
}
