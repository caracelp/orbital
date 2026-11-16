//! Utilidades de la ventana: menús, secciones plegables, árbol de la bóveda, pantalla inicial y
//! nombres portables.

use super::*;

/// Lee un archivo de texto para mostrarlo.
pub(super) fn read_text(path: &Path) -> Result<String, String> {
    let meta = std::fs::metadata(path).map_err(|e| e.to_string())?;
    if meta.len() > orbital_vault::entry::MAX_EDITABLE_BYTES {
        return Err("El archivo es demasiado grande para mostrarlo".into());
    }
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    String::from_utf8(bytes).map_err(|_| "El archivo no está en UTF-8".into())
}

/// Agrupa las entradas por carpeta, con las carpetas primero.
pub(super) fn build_tree(entries: &[Entry]) -> Tree {
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

pub(super) fn children_model(tree: &Tree, parent: &str, reversed: bool) -> gio::ListStore {
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

pub(super) fn icon_for(kind: EntryKind) -> &'static str {
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
pub(super) fn seccion_plegable(
    titulo: &str,
    contenido: &impl IsA<gtk::Widget>,
    abierta: bool,
    expandir: bool,
) -> (gtk::Box, gtk::Button) {
    let chevron = crate::iconos::Icono::new("abajo", 14);
    // Recortado: el nombre de una colección (027) puede ser largo y no tiene que ensanchar la barra.
    let rotulo = gtk::Label::builder()
        .label(titulo.to_uppercase())
        .xalign(0.0)
        .hexpand(true)
        .ellipsize(gtk::pango::EllipsizeMode::End)
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
pub(super) fn nombre_de_barra(boton: &gtk::ToggleButton, barra: &'static str, atajo: &'static str) {
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
pub(super) fn menu_button(
    icon: &str,
    tooltip: &str,
    items: &[(&str, &str, &str)],
) -> gtk::MenuButton {
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
pub(super) struct MenuItem {
    pub(super) label: String,
    pub(super) action: String,
    pub(super) accel: String,
    pub(super) target: Option<String>,
}

impl MenuItem {
    pub(super) fn new(label: &str, action: &str, accel: &str) -> Self {
        Self {
            label: label.to_owned(),
            action: action.to_owned(),
            accel: accel.to_owned(),
            target: None,
        }
    }
}

/// (Re)arma el contenido de un menú.
pub(super) fn fill_menu(popover: &gtk::Popover, items: &[MenuItem]) {
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

pub(super) fn start_page() -> adw::StatusPage {
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
pub(super) fn save_state_opened(path: PathBuf) {
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
pub(super) async fn find_in_trash(original: &Path) -> Option<gio::File> {
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
pub(super) fn prepare(v: Vault) -> (Vault, Tree, NoteMeta) {
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
pub(super) enum NewKind {
    Md,
    Html,
    /// Ruta de la plantilla.
    Template(String),
}

/// Caracteres que funcionan en Linux pero rompen la sincronización con Windows y algunas nubes.
pub(super) const NOT_PORTABLE: &[char] = &[':', '?', '*', '<', '>', '|', '"', '\\'];

/// Avisa (sin impedirlo) si un nombre tiene caracteres que dan problemas en otros sistemas.
pub(super) fn warn_portable(window: &Window, name: &str) {
    let bad: String = name.chars().filter(|c| NOT_PORTABLE.contains(c)).collect();
    if !bad.is_empty() {
        window.toast(&format!(
            "«{name}» tiene {bad}, que puede dar problemas al sincronizar con Windows o con la nube"
        ));
    }
}

/// Respuesta al diálogo de actualizar enlaces al renombrar.
pub(super) enum LinkAnswer {
    Cancel,
    Skip,
    Update,
}
