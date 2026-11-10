//! Vista Visual de las notas `.html` (US4 de la 002, research R6): WebKit editable con una barra
//! de formato.
//!
//! El texto del editor (la vista Código) sigue siendo el único original: lo que se edita acá se
//! lleva al texto (solo el `<body>` y el `<style data-orbital>`) y de ahí lo guarda el
//! autoguardado. Colores, alineación, columnas y estilos son clases `o-…` con sus reglas en un
//! único `<style data-orbital>` (FR-014b): nunca `style=` en línea.
//!
//! WebKit se crea recién al abrir la primera nota `.html` en Visual.

use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::rc::Rc;

use adw::prelude::*;
use gtk::{gio, glib};
use webkit6::prelude::*;

/// Nombre del *world* aislado donde corre el script de Orbital.
const WORLD: &str = "orbital";
const SCRIPT: &str = include_str!("visual.js");

/// Colores de la paleta: (clave, nombre, color de texto, RGB para el fondo).
const COLORS: [(&str, &str, &str, &str); 8] = [
    ("rosa", "Rosa", "#ff8cc6", "255,140,198"),
    ("rojo", "Rojo", "#ff6b6b", "255,107,107"),
    ("naranja", "Naranja", "#ffa94d", "255,169,77"),
    ("amarillo", "Amarillo", "#ffd43b", "255,212,59"),
    ("verde", "Verde", "#69db7c", "105,219,124"),
    ("azul", "Azul", "#74c0fc", "116,192,252"),
    ("violeta", "Violeta", "#b197fc", "177,151,252"),
    ("gris", "Gris", "#adb5bd", "173,181,189"),
];

/// Reglas de todas las clases de Orbital, como objeto de JavaScript.
fn rules() -> String {
    let mut r: Vec<(String, String)> = Vec::new();
    for (key, _, fg, rgb) in COLORS {
        r.push((format!("o-color-{key}"), format!("color: {fg}")));
        r.push((
            format!("o-fondo-{key}"),
            format!("background: rgba({rgb}, 0.28); border-radius: 3px"),
        ));
    }
    for (key, value) in [
        ("izquierda", "left"),
        ("centro", "center"),
        ("derecha", "right"),
        ("justificado", "justify"),
    ] {
        r.push((format!("o-alinear-{key}"), format!("text-align: {value}")));
    }
    for n in [2, 3] {
        r.push((
            format!("o-columnas-{n}"),
            format!("column-count: {n}; column-gap: 2em"),
        ));
    }
    r.push((
        "o-estilo-destacado".into(),
        "font-size: 1.15em; font-weight: 600; border-left: 4px solid #ff8cc6; padding-left: 0.75em"
            .into(),
    ));
    r.push((
        "o-estilo-nota".into(),
        "background: rgba(116,192,252,0.12); border-left: 4px solid #74c0fc; padding: 0.5em 0.75em; border-radius: 6px".into(),
    ));
    r.push((
        "o-estilo-advertencia".into(),
        "background: rgba(255,212,59,0.12); border-left: 4px solid #ffd43b; padding: 0.5em 0.75em; border-radius: 6px".into(),
    ));
    let body: Vec<String> = r
        .iter()
        .map(|(k, v)| format!("{}: {}", js_string(k), js_string(v)))
        .collect();
    format!("{{{}}}", body.join(", "))
}

/// Literal de cadena de JavaScript.
fn js_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '<' => out.push_str("\\u003c"),
            c if (c as u32) < 0x20 || c == '\u{2028}' || c == '\u{2029}' => {
                out.push_str(&format!("\\u{:04x}", c as u32));
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

type Callback = Box<dyn Fn()>;
type ErrorFn = Box<dyn Fn(&str)>;

struct Inner {
    root: gtk::Box,
    /// Lugar de las propiedades (las arma la ventana).
    header: gtk::Box,
    web: RefCell<Option<webkit6::WebView>>,
    /// Carpeta de la nota y raíz de la bóveda (imágenes).
    dirs: RefCell<Option<(PathBuf, PathBuf)>>,
    /// Hash del texto cargado en la página (para no recargar si no cambió).
    loaded: Cell<Option<u64>>,
    on_input: RefCell<Option<Callback>>,
    on_error: RefCell<Option<ErrorFn>>,
}

#[derive(Clone)]
pub struct Visual(Rc<Inner>);

impl Default for Visual {
    fn default() -> Self {
        Self::new()
    }
}

impl Visual {
    pub fn new() -> Self {
        let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
        let header = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .margin_start(12)
            .margin_end(12)
            .margin_top(12)
            .build();
        root.append(&header);
        Self(Rc::new(Inner {
            root,
            header,
            web: RefCell::new(None),
            dirs: RefCell::new(None),
            loaded: Cell::new(None),
            on_input: RefCell::new(None),
            on_error: RefCell::new(None),
        }))
    }

    pub fn widget(&self) -> &gtk::Box {
        &self.0.root
    }

    /// El usuario cambió algo en la página.
    pub fn set_on_input(&self, f: impl Fn() + 'static) {
        *self.0.on_input.borrow_mut() = Some(Box::new(f));
    }

    pub fn set_on_error(&self, f: impl Fn(&str) + 'static) {
        *self.0.on_error.borrow_mut() = Some(Box::new(f));
    }

    fn error(&self, msg: &str) {
        if let Some(f) = self.0.on_error.borrow().as_ref() {
            f(msg);
        }
    }

    /// Pone las propiedades arriba de la barra.
    pub fn set_header(&self, w: Option<gtk::Widget>) {
        let h = &self.0.header;
        while let Some(c) = h.first_child() {
            h.remove(&c);
        }
        if let Some(w) = w {
            h.append(&w);
        }
        h.set_visible(h.first_child().is_some());
    }

    /// Crea la vista web y la barra la primera vez.
    fn web(&self) -> webkit6::WebView {
        if let Some(w) = self.0.web.borrow().as_ref() {
            return w.clone();
        }
        let content = webkit6::UserContentManager::new();
        content.add_script(&webkit6::UserScript::for_world(
            &SCRIPT.replace("__RULES__", &rules()),
            webkit6::UserContentInjectedFrames::TopFrame,
            webkit6::UserScriptInjectionTime::End,
            WORLD,
            &[],
            &[],
        ));
        content.register_script_message_handler("orbital", Some(WORLD));
        let weak = Rc::downgrade(&self.0);
        content.connect_script_message_received(Some("orbital"), move |_, _| {
            if let Some(i) = weak.upgrade()
                && let Some(f) = i.on_input.borrow().as_ref()
            {
                f();
            }
        });
        let web = crate::html_view::new_editable_view(&content);
        web.update_property(&[gtk::accessible::Property::Label(
            "Página editable de la nota",
        )]);
        self.0.root.append(&self.toolbar());
        self.0.root.append(&web);
        *self.0.web.borrow_mut() = Some(web.clone());
        web
    }

    /// Muestra `html` en la página, si no es lo que ya está cargado.
    pub fn load(&self, html: &str, note_dir: &Path, vault_root: &Path) {
        let h = orbital_vault::write::hash(html.as_bytes());
        *self.0.dirs.borrow_mut() = Some((note_dir.to_path_buf(), vault_root.to_path_buf()));
        if self.0.loaded.get() == Some(h) {
            return;
        }
        self.0.loaded.set(Some(h));
        let base = gio::File::for_path(note_dir).uri().to_string() + "/";
        self.web()
            .load_html(&crate::html_view::with_csp(html), Some(&base));
    }

    /// El texto que muestra la página ya es `html` (lo escribió la propia página).
    pub fn mark_loaded(&self, html: &str) {
        self.0
            .loaded
            .set(Some(orbital_vault::write::hash(html.as_bytes())));
    }

    /// Olvida la página cargada (al cambiar de nota).
    pub fn forget(&self) {
        self.0.loaded.set(None);
    }

    /// El contenido de `<body>` y las reglas de las clases usadas.
    pub async fn contents(&self) -> Option<(String, String)> {
        let web = self.0.web.borrow().clone()?;
        // Una sola evaluación: cuerpo y estilos pertenecen a la misma edición.
        let snapshot = web
            .evaluate_javascript_future(
                "[document.body.innerHTML, window.orbital.css()]",
                Some(WORLD),
                None,
            )
            .await
            .ok()?;
        let body = snapshot
            .object_get_property_at_index(0)?
            .to_str()
            .to_string();
        let css = snapshot
            .object_get_property_at_index(1)?
            .to_str()
            .to_string();
        Some((body, css))
    }

    fn js(&self, script: String) {
        let Some(web) = self.0.web.borrow().clone() else {
            return;
        };
        let weak = Rc::downgrade(&self.0);
        glib::spawn_future_local(async move {
            let remembered = web
                .evaluate_javascript_future("window.orbital.rememberSelection()", Some(WORLD), None)
                .await;
            web.grab_focus();
            let script = format!("window.orbital.restoreSelection();{script}");
            if (remembered.is_err()
                || web
                    .evaluate_javascript_future(&script, Some(WORLD), None)
                    .await
                    .is_err())
                && let Some(inner) = weak.upgrade()
            {
                Visual(inner)
                    .error("No se pudo aplicar el formato; esperá a que termine de cargar la nota");
            }
        });
    }

    fn exec(&self, command: &str, arg: Option<&str>) {
        let script = match arg {
            Some(a) => format!(
                "document.execCommand({}, false, {})",
                js_string(command),
                js_string(a)
            ),
            None => format!("document.execCommand({})", js_string(command)),
        };
        self.js(script);
    }

    fn run(&self, cmd: &str, arg: &str) {
        self.js(format!(
            "window.orbital.run({}, {})",
            js_string(cmd),
            js_string(arg)
        ));
    }

    /// Elige una imagen de la bóveda y la inserta con una ruta relativa a la nota.
    fn pick_image(&self, parent: &gtk::Widget) {
        let Some((note_dir, root)) = self.0.dirs.borrow().clone() else {
            return;
        };
        let filter = gtk::FileFilter::new();
        filter.set_name(Some("Imágenes"));
        filter.add_mime_type("image/*");
        let filters = gio::ListStore::new::<gtk::FileFilter>();
        filters.append(&filter);
        let dialog = gtk::FileDialog::builder()
            .title("Insertar una imagen de la bóveda")
            .initial_folder(&gio::File::for_path(&note_dir))
            .filters(&filters)
            .modal(true)
            .build();
        let window = parent.root().and_downcast::<gtk::Window>();
        let this = self.clone();
        dialog.open(window.as_ref(), None::<&gio::Cancellable>, move |r| {
            let Ok(file) = r else { return };
            let Some(path) = file.path() else { return };
            if !path.starts_with(&root) {
                this.error("Elegí una imagen que esté dentro de la bóveda");
                return;
            }
            let rel = relative(&note_dir, &path);
            let rel = glib::uri_escape_string(&rel, Some("/"), false);
            let alt = path
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default();
            this.js(format!(
                "window.orbital.run('imagen', [{}, {}])",
                js_string(&rel),
                js_string(&alt)
            ));
        });
    }

    fn toolbar(&self) -> gtk::Widget {
        let bar = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(2)
            .margin_start(8)
            .margin_end(8)
            .margin_top(6)
            .margin_bottom(6)
            .build();
        bar.update_property(&[gtk::accessible::Property::Label("Barra de formato")]);
        let button = |icon: &str, name: &str, f: Box<dyn Fn(&Visual)>| {
            let b = gtk::Button::builder()
                .icon_name(icon)
                .focus_on_click(false)
                .tooltip_text(name)
                .css_classes(["flat"])
                .build();
            b.update_property(&[gtk::accessible::Property::Label(name)]);
            let weak = Rc::downgrade(&self.0);
            b.connect_clicked(move |_| {
                if let Some(i) = weak.upgrade() {
                    f(&Visual(i));
                }
            });
            b
        };
        for (icon, name, cmd) in [
            ("edit-undo-symbolic", "Deshacer (Ctrl+Z)", "undo"),
            ("edit-redo-symbolic", "Rehacer (Ctrl+Mayús+Z)", "redo"),
            ("format-text-bold-symbolic", "Negrita (Ctrl+B)", "bold"),
            ("format-text-italic-symbolic", "Cursiva (Ctrl+I)", "italic"),
            (
                "format-text-underline-symbolic",
                "Subrayado (Ctrl+U)",
                "underline",
            ),
            (
                "format-text-strikethrough-symbolic",
                "Tachado",
                "strikeThrough",
            ),
        ] {
            bar.append(&button(icon, name, Box::new(move |v| v.exec(cmd, None))));
        }
        bar.append(&separator());
        bar.append(&self.menu(
            None,
            Some("Párrafo"),
            "Tipo de bloque",
            &[
                ("Párrafo", "formatBlock", "<p>"),
                ("Título 1", "formatBlock", "<h1>"),
                ("Título 2", "formatBlock", "<h2>"),
                ("Título 3", "formatBlock", "<h3>"),
                ("Cita", "formatBlock", "<blockquote>"),
                ("Bloque de código", "formatBlock", "<pre>"),
            ],
            |v, cmd, arg| v.exec(cmd, Some(arg)),
        ));
        bar.append(&button(
            "view-list-bullet-symbolic",
            "Lista con viñetas",
            Box::new(|v| v.exec("insertUnorderedList", None)),
        ));
        bar.append(&button(
            "view-list-ordered-symbolic",
            "Lista numerada",
            Box::new(|v| v.exec("insertOrderedList", None)),
        ));
        bar.append(&separator());
        bar.append(&self.link_button());
        bar.append(&self.menu(
            Some("view-grid-symbolic"),
            None,
            "Tabla",
            &[
                ("Insertar tabla", "tabla", "insert"),
                ("Agregar fila", "tabla", "row-add"),
                ("Agregar columna", "tabla", "col-add"),
                ("Quitar fila", "tabla", "row-del"),
                ("Quitar columna", "tabla", "col-del"),
            ],
            |v, cmd, arg| v.run(cmd, arg),
        ));
        let weak = Rc::downgrade(&self.0);
        let img = gtk::Button::builder()
            .icon_name("insert-image-symbolic")
            .tooltip_text("Imagen de la bóveda")
            .css_classes(["flat"])
            .build();
        img.update_property(&[gtk::accessible::Property::Label("Imagen de la bóveda")]);
        img.connect_clicked(move |b| {
            if let Some(i) = weak.upgrade() {
                Visual(i).pick_image(b.upcast_ref());
            }
        });
        bar.append(&img);
        bar.append(&button(
            "view-more-horizontal-symbolic",
            "Separador",
            Box::new(|v| v.exec("insertHorizontalRule", None)),
        ));
        bar.append(&separator());
        bar.append(&self.color_menu());
        bar.append(&self.menu(
            Some("format-justify-left-symbolic"),
            None,
            "Alineación",
            &[
                ("Izquierda", "alinear", "o-alinear-izquierda"),
                ("Centro", "alinear", "o-alinear-centro"),
                ("Derecha", "alinear", "o-alinear-derecha"),
                ("Justificado", "alinear", "o-alinear-justificado"),
                ("Sin alineación", "alinear", ""),
            ],
            |v, cmd, arg| v.run(cmd, arg),
        ));
        bar.append(&self.menu(
            Some("view-dual-symbolic"),
            None,
            "Columnas",
            &[
                ("Una columna", "columnas", ""),
                ("Dos columnas", "columnas", "o-columnas-2"),
                ("Tres columnas", "columnas", "o-columnas-3"),
            ],
            |v, cmd, arg| v.run(cmd, arg),
        ));
        bar.append(&self.menu(
            None,
            Some("Estilo"),
            "Estilo del bloque",
            &[
                ("Normal", "estilo", ""),
                ("Destacado", "estilo", "o-estilo-destacado"),
                ("Nota", "estilo", "o-estilo-nota"),
                ("Advertencia", "estilo", "o-estilo-advertencia"),
            ],
            |v, cmd, arg| v.run(cmd, arg),
        ));
        let scroller = gtk::ScrolledWindow::builder()
            .vscrollbar_policy(gtk::PolicyType::Never)
            .hscrollbar_policy(gtk::PolicyType::Automatic)
            .child(&bar)
            .build();
        scroller.add_css_class("toolbar");
        scroller.upcast()
    }

    /// Un botón con un menú de acciones (botones en un popover: con nombre accesible cada uno).
    #[allow(clippy::type_complexity)]
    fn menu(
        &self,
        icon: Option<&str>,
        label: Option<&str>,
        name: &str,
        items: &[(&'static str, &'static str, &'static str)],
        act: fn(&Visual, &str, &str),
    ) -> gtk::Widget {
        let column = gtk::Box::new(gtk::Orientation::Vertical, 0);
        let popover = gtk::Popover::builder().child(&column).build();
        for &(text, cmd, arg) in items {
            let b = gtk::Button::builder()
                .child(&gtk::Label::builder().label(text).xalign(0.0).build())
                .css_classes(["flat"])
                .build();
            let (weak, p) = (Rc::downgrade(&self.0), popover.clone());
            b.connect_clicked(move |_| {
                p.popdown();
                if let Some(i) = weak.upgrade() {
                    act(&Visual(i), cmd, arg);
                }
            });
            column.append(&b);
        }
        popover_button(&popover, icon, label, name).upcast()
    }

    fn color_menu(&self) -> gtk::Widget {
        let column = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(6)
            .build();
        let popover = gtk::Popover::builder().child(&column).build();
        for (title, cmd, prefix) in [
            ("Color del texto", "color", "o-color-"),
            ("Color de fondo", "fondo", "o-fondo-"),
        ] {
            column.append(
                &gtk::Label::builder()
                    .label(title)
                    .xalign(0.0)
                    .css_classes(["heading"])
                    .build(),
            );
            let grid = gtk::FlowBox::builder()
                .selection_mode(gtk::SelectionMode::None)
                .max_children_per_line(4)
                .min_children_per_line(4)
                .build();
            let mut entries: Vec<(String, String)> = COLORS
                .iter()
                .map(|(k, n, fg, _)| {
                    (
                        format!("{prefix}{k}"),
                        format!("<span foreground=\"{fg}\">●</span> {n}"),
                    )
                })
                .collect();
            entries.push((String::new(), "Sin color".to_owned()));
            for (class, markup) in entries {
                let label = gtk::Label::builder()
                    .use_markup(true)
                    .label(&markup)
                    .build();
                let b = gtk::Button::builder()
                    .child(&label)
                    .css_classes(["flat"])
                    .build();
                let plain = markup
                    .rsplit("</span> ")
                    .next()
                    .unwrap_or(&markup)
                    .to_owned();
                b.update_property(&[gtk::accessible::Property::Label(&format!(
                    "{title}: {plain}"
                ))]);
                let (weak, p) = (Rc::downgrade(&self.0), popover.clone());
                b.connect_clicked(move |_| {
                    p.popdown();
                    if let Some(i) = weak.upgrade() {
                        Visual(i).run(cmd, &class);
                    }
                });
                grid.append(&b);
            }
            column.append(&grid);
        }
        popover_button(&popover, Some("color-select-symbolic"), None, "Colores").upcast()
    }

    fn link_button(&self) -> gtk::Widget {
        let entry = adw::EntryRow::builder()
            .title("Dirección del enlace")
            .show_apply_button(true)
            .build();
        let list = gtk::ListBox::builder()
            .selection_mode(gtk::SelectionMode::None)
            .width_request(320)
            .build();
        list.add_css_class("boxed-list");
        list.append(&entry);
        let popover = gtk::Popover::builder().child(&list).build();
        let (weak, p) = (Rc::downgrade(&self.0), popover.clone());
        entry.connect_apply(move |e| {
            let url = e.text().trim().to_owned();
            p.popdown();
            if !url.is_empty()
                && let Some(i) = weak.upgrade()
            {
                Visual(i).exec("createLink", Some(&url));
            }
            e.set_text("");
        });
        popover_button(&popover, Some("insert-link-symbolic"), None, "Enlace").upcast()
    }
}

/// GtkMenuButton no expone acciones AT-SPI en GTK 4.24. El botón conserva la
/// activación nativa por teclado/accesibilidad y abre el mismo popover.
fn popover_button(
    popover: &gtk::Popover,
    icon: Option<&str>,
    label: Option<&str>,
    name: &str,
) -> gtk::Button {
    let button = gtk::Button::builder()
        .tooltip_text(name)
        .css_classes(["flat"])
        .build();
    if let Some(icon) = icon {
        button.set_icon_name(icon);
    }
    if let Some(label) = label {
        button.set_label(label);
    }
    button.update_property(&[
        gtk::accessible::Property::Label(name),
        gtk::accessible::Property::HasPopup(true),
    ]);
    popover.set_parent(&button);
    let popup = popover.clone();
    button.connect_clicked(move |_| popup.popup());
    let popup = popover.clone();
    button.connect_destroy(move |_| popup.unparent());
    button
}

fn separator() -> gtk::Separator {
    let s = gtk::Separator::new(gtk::Orientation::Vertical);
    s.set_margin_start(4);
    s.set_margin_end(4);
    s
}

/// Ruta de `target` relativa a la carpeta `from`, con `/`.
fn relative(from: &Path, target: &Path) -> String {
    let a: Vec<_> = from.components().collect();
    let b: Vec<_> = target.components().collect();
    let common = a.iter().zip(&b).take_while(|(x, y)| x == y).count();
    let mut parts: Vec<String> = vec!["..".to_owned(); a.len() - common];
    parts.extend(
        b[common..]
            .iter()
            .map(|c| c.as_os_str().to_string_lossy().into_owned()),
    );
    parts.join("/")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relative_paths() {
        let r = relative(Path::new("/b/wiki/fisica"), Path::new("/b/wiki/img/a.png"));
        assert_eq!(r, "../img/a.png");
        assert_eq!(
            relative(Path::new("/b/wiki"), Path::new("/b/wiki/a.png")),
            "a.png"
        );
    }

    #[test]
    fn js_strings_are_safe() {
        assert_eq!(js_string("a\"b</script>\n"), "\"a\\\"b\\u003c/script>\\n\"");
        assert!(rules().contains("\"o-color-rosa\": \"color: #ff8cc6\""));
    }
}

#[cfg(test)]
#[path = "html_tests.rs"]
mod webkit_tests;
