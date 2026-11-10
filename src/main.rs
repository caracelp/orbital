use adw::prelude::*;
use gtk::glib;

const APP_ID: &str = "io.github.caracelp.Orbital";

fn main() -> glib::ExitCode {
    let app = adw::Application::builder().application_id(APP_ID).build();
    app.connect_activate(build_ui);
    app.run()
}

fn build_ui(app: &adw::Application) {
    // --- Editor (contenido) ---
    let editor = gtk::TextView::builder()
        .wrap_mode(gtk::WrapMode::WordChar)
        .monospace(true)
        .top_margin(24)
        .bottom_margin(24)
        .left_margin(32)
        .right_margin(32)
        .vexpand(true)
        .build();

    let editor_scroll = gtk::ScrolledWindow::builder().child(&editor).build();

    let content_header = adw::HeaderBar::new();
    let content_title = adw::WindowTitle::new("Orbital", "");
    content_header.set_title_widget(Some(&content_title));

    let content_view = adw::ToolbarView::new();
    content_view.add_top_bar(&content_header);
    content_view.set_content(Some(&editor_scroll));

    let content_page = adw::NavigationPage::builder()
        .title("Nota")
        .child(&content_view)
        .build();

    // --- Lista de notas (barra lateral) ---
    let notes = [
        (
            "Bienvenida",
            "# Bienvenida a Orbital\n\nTu espacio de notas en **Markdown**.\n",
        ),
        (
            "Ideas",
            "# Ideas\n\n- Enlaces entre notas `[[...]]`\n- Vista de grafo\n- Tareas\n",
        ),
        (
            "Tareas",
            "# Tareas\n\n- [ ] Guardar notas en disco\n- [ ] Vista previa Markdown\n",
        ),
    ];

    let list = gtk::ListBox::new();
    list.add_css_class("navigation-sidebar");
    for (title, _) in notes {
        let row = adw::ActionRow::builder()
            .title(title)
            .activatable(true)
            .build();
        list.append(&row);
    }

    let sidebar_scroll = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .child(&list)
        .vexpand(true)
        .build();

    let sidebar_header = adw::HeaderBar::new();
    let new_button = gtk::Button::from_icon_name("list-add-symbolic");
    new_button.set_tooltip_text(Some("Nueva nota"));
    sidebar_header.pack_start(&new_button);

    let sidebar_view = adw::ToolbarView::new();
    sidebar_view.add_top_bar(&sidebar_header);
    sidebar_view.set_content(Some(&sidebar_scroll));

    let sidebar_page = adw::NavigationPage::builder()
        .title("Notas")
        .child(&sidebar_view)
        .build();

    // --- Vista dividida adaptable ---
    let split = adw::NavigationSplitView::builder()
        .sidebar(&sidebar_page)
        .content(&content_page)
        .build();

    // Al seleccionar una nota, mostrar su contenido en el editor
    {
        let editor = editor.clone();
        let content_title = content_title.clone();
        let split = split.clone();
        list.connect_row_activated(move |_, row| {
            let (title, body) = notes[row.index() as usize];
            editor.buffer().set_text(body);
            content_title.set_title(title);
            split.set_show_content(true);
        });
    }

    let window = adw::ApplicationWindow::builder()
        .application(app)
        .title("Orbital")
        .default_width(1000)
        .default_height(680)
        .width_request(360)
        .height_request(300)
        .content(&split)
        .build();

    // En pantallas estrechas, la barra lateral y el editor se apilan
    let breakpoint = adw::Breakpoint::new(adw::BreakpointCondition::new_length(
        adw::BreakpointConditionLengthType::MaxWidth,
        600.0,
        adw::LengthUnit::Sp,
    ));
    breakpoint.add_setter(&split, "collapsed", Some(&true.to_value()));
    window.add_breakpoint(breakpoint);

    // Abrir la primera nota al inicio
    if let Some(first) = list.row_at_index(0) {
        list.select_row(Some(&first));
        first.emit_activate();
    }

    window.present();
}
