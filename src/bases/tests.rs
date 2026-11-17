use super::*;

#[test]
fn missing_null_false_and_zero_remain_distinct() {
    assert_eq!(display_value(None), "(ausente)");
    assert_eq!(display_value(Some(&PropValue::Null)), "(nulo)");
    assert_eq!(display_value(Some(&PropValue::Bool(false))), "false");
    assert_eq!(display_value(Some(&PropValue::Number("0".into()))), "0");
    assert_eq!(display_value(Some(&PropValue::Text(String::new()))), "");
}

#[test]
#[ignore = "Requiere GTK con servidor gráfico; no abre bóvedas ni archivos"]
fn panel_uses_virtual_models_for_both_views() {
    gtk::init().unwrap();
    let panel = BasePanel::new();
    let definition = BaseDefinition::default();
    let snapshot = BaseSnapshot {
        rows: (0..5000)
            .map(|index| BaseRow {
                path: RelPath::new(&format!("wiki/nota-{index}.md")).unwrap(),
                title: format!("Nota {index}"),
                hash: 1,
                read_only: false,
                properties: vec![],
                error: None,
            })
            .collect(),
        errors: vec![],
    };
    panel.set_data(definition.clone(), snapshot.clone());
    let window = gtk::Window::builder()
        .default_width(800)
        .default_height(600)
        .child(panel.widget())
        .build();
    window.present();
    settle_frames();
    assert_virtualized(&panel);
    assert_eq!(panel.0.store.n_items(), 5000);
    assert_eq!(panel.0.stack.visible_child_name().as_deref(), Some("table"));
    assert_eq!(
        panel.0.table.columns().n_items(),
        definition.columns.len() as u32 + 1
    );
    panel.set_busy(true);
    assert!(!panel.0.stack.is_sensitive());
    panel.set_busy(false);
    assert!(panel.0.stack.is_sensitive());
    panel.set_error(Some("Conflicto externo"));
    assert_eq!(panel.0.status.text(), "Conflicto externo");
    let mut cards = definition;
    cards.view = BaseView::Cards;
    panel.set_data(cards, snapshot);
    settle_frames();
    assert_virtualized(&panel);
    assert_eq!(panel.0.stack.visible_child_name().as_deref(), Some("cards"));
    window.close();
}

#[test]
fn editor_preserves_typed_values_without_implicit_conversion() {
    for value in [
        PropValue::Text("true".into()),
        PropValue::Bool(false),
        PropValue::Number("12.5".into()),
        PropValue::List(vec![
            PropValue::Text("uno".into()),
            PropValue::Number("2".into()),
        ]),
        PropValue::Null,
    ] {
        let input = dialogs::edit_value(PropertyType::Auto, &value);
        assert_eq!(parse_value(PropertyType::Auto, &input).unwrap(), value);
    }
}

fn descendants(widget: &gtk::Widget) -> Vec<gtk::Widget> {
    let mut result = vec![widget.clone()];
    let mut child = widget.first_child();
    while let Some(current) = child {
        result.extend(descendants(&current));
        child = current.next_sibling();
    }
    result
}
fn dialog(title: &str) -> gtk::Window {
    gtk::Window::list_toplevels()
        .into_iter()
        .filter_map(|w| w.downcast::<gtk::Window>().ok())
        .find(|w| w.title().as_deref() == Some(title))
        .unwrap()
}
fn click(window: &gtk::Window, label: &str) {
    descendants(window.upcast_ref())
        .into_iter()
        .filter_map(|w| w.downcast::<gtk::Button>().ok())
        .find(|b| b.label().as_deref() == Some(label))
        .unwrap()
        .emit_clicked();
}

#[test]
#[ignore = "Requiere GTK; ejecutar con --test-threads=1; usa únicamente datos en memoria"]
fn dialogs_validate_and_emit_only_explicit_valid_changes() {
    gtk::init().unwrap();
    let panel = BasePanel::new();
    let edits = Rc::new(RefCell::new(Vec::new()));
    let capture = edits.clone();
    panel.connect_edit(move |path, hash, key, value| {
        capture.borrow_mut().push((path, hash, key, value))
    });
    let mut row = BaseRow {
        path: RelPath::new("wiki/prueba.md").unwrap(),
        title: "Prueba".into(),
        hash: 42,
        read_only: false,
        properties: vec![("precio".into(), PropValue::Number("3".into()))],
        error: None,
    };
    let column = BaseColumn {
        property: "precio".into(),
        label: "Precio".into(),
        kind: PropertyType::Number,
    };
    panel.edit_dialog(&row, &column);
    let window = dialog("Editar Precio");
    let input = descendants(window.upcast_ref())
        .into_iter()
        .find_map(|w| w.downcast::<gtk::TextView>().ok())
        .unwrap();
    input.buffer().set_text("no es un número");
    click(&window, "Guardar");
    assert!(edits.borrow().is_empty());
    assert!(window.is_visible());
    input.buffer().set_text("12.5");
    click(&window, "Guardar");
    assert_eq!(edits.borrow()[0].1, 42);
    assert_eq!(edits.borrow()[0].3, Some(PropValue::Number("12.5".into())));
    panel.edit_dialog(&row, &column);
    click(&dialog("Editar Precio"), "Establecer nulo");
    assert_eq!(edits.borrow()[1].3, Some(PropValue::Null));
    panel.edit_dialog(&row, &column);
    click(&dialog("Editar Precio"), "Quitar propiedad");
    assert_eq!(edits.borrow()[2].3, None);
    row.read_only = true;
    panel.edit_dialog(&row, &column);
    assert_eq!(edits.borrow().len(), 3);
    let configs = Rc::new(RefCell::new(Vec::new()));
    let capture = configs.clone();
    panel.connect_configure(move |value| capture.borrow_mut().push(value));
    panel.configure_dialog();
    let window = dialog("Configurar base");
    let key = descendants(window.upcast_ref())
        .into_iter()
        .filter_map(|w| w.downcast::<gtk::Entry>().ok())
        .find(|e| e.placeholder_text().as_deref() == Some("Propiedad de columna"))
        .unwrap();
    key.set_text("");
    click(&window, "Guardar configuración");
    assert!(configs.borrow().is_empty());
    assert!(window.is_visible());
    key.set_text("estado");
    click(&window, "Guardar configuración");
    assert_eq!(configs.borrow()[0].columns[0].property, "estado");
    panel.configure_dialog();
    click(&dialog("Configurar base"), "Cancelar");
    assert_eq!(configs.borrow().len(), 1);
}

fn settle_frames() {
    for _ in 0..20 {
        while glib::MainContext::default().pending() {
            glib::MainContext::default().iteration(false);
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}
fn assert_virtualized(panel: &BasePanel) {
    let buttons = descendants(panel.widget().upcast_ref())
        .iter()
        .filter(|w| w.is::<gtk::Button>())
        .count();
    assert!(buttons > 2, "La vista debe materializar filas visibles");
    assert!(
        buttons < 1000,
        "5000 filas no deben crear 5000 widgets: {buttons}"
    );
}
fn input(window: &gtk::Window, label: &str) -> gtk::Entry {
    descendants(window.upcast_ref())
        .into_iter()
        .filter_map(|w| w.downcast::<gtk::Entry>().ok())
        .find(|e| e.placeholder_text().as_deref() == Some(label))
        .unwrap()
}
#[test]
#[ignore = "Requiere GTK; ejecutar con --test-threads=1; sin archivos"]
fn configuration_preserves_typed_filters_sort_view_and_rejects_stale_snapshot() {
    gtk::init().unwrap();
    let panel = BasePanel::new();
    let definition = BaseDefinition {
        view: BaseView::Cards,
        filters: vec![
            BaseFilter::Folder("wiki/proyectos".into()),
            BaseFilter::Tag("activo".into()),
            BaseFilter::Property {
                key: "precio".into(),
                value: Some(PropValue::Number("12".into())),
            },
        ],
        sort: Some(BaseSort {
            property: "precio".into(),
            descending: true,
        }),
        ..BaseDefinition::default()
    };
    panel.set_data(definition.clone(), BaseSnapshot::default());
    let configs = Rc::new(RefCell::new(Vec::new()));
    let capture = configs.clone();
    panel.connect_configure(move |v| capture.borrow_mut().push(v));
    panel.configure_dialog();
    let window = dialog("Configurar base");
    click(&window, "Guardar configuración");
    assert_eq!(configs.borrow()[0], definition);
    panel.configure_dialog();
    let window = dialog("Configurar base");
    input(&window, "Ordenar por propiedad (vacío: ruta)").set_text("estado");
    let view = descendants(window.upcast_ref())
        .into_iter()
        .find_map(|w| w.downcast::<gtk::DropDown>().ok())
        .unwrap();
    view.set_selected(0);
    click(&window, "Guardar configuración");
    assert_eq!(configs.borrow()[1].view, BaseView::Table);
    assert_eq!(
        configs.borrow()[1].sort.as_ref().unwrap().property,
        "estado"
    );
    panel.configure_dialog();
    let window = dialog("Configurar base");
    panel.set_data(BaseDefinition::default(), BaseSnapshot::default());
    click(&window, "Guardar configuración");
    assert_eq!(configs.borrow().len(), 2);
    assert!(window.is_visible());
    click(&window, "Cancelar");
}
