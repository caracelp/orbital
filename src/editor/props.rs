//! Lista editable de propiedades (US3 de la 002, research R7).
//!
//! Cada fila usa el control de su tipo. Los cambios no van al disco: se aplican al texto del
//! editor (con `Note::with_prop`, que edita por líneas) y los guarda el autoguardado. Así no se
//! pisa lo que el usuario está escribiendo.

use std::rc::Rc;

use adw::prelude::*;
use gtk::glib;
use orbital_vault::props::{PropEntry, PropError, PropValue};

/// Qué se puede hacer con la lista.
pub enum Mode {
    Editable,
    /// `raw/` o una nota demasiado grande.
    ReadOnly,
    /// El YAML tiene un error: no se edita desde la lista (FR-018).
    Invalid(PropError),
}

type SetFn = Box<dyn Fn(&str, PropValue, bool)>;

/// Lo que la lista le pide al editor.
pub struct Actions {
    /// Cambia el valor. `rebuild`: la lista se vuelve a armar (agregar o quitar filas); si es
    /// `false`, la fila ya muestra el valor nuevo y no hace falta (el foco no se pierde).
    pub set: SetFn,
    /// Quita la propiedad (con Deshacer).
    pub remove: Box<dyn Fn(&str)>,
    /// Pasa a la vista Código (para corregir un error).
    pub go_code: Box<dyn Fn()>,
}

/// Valor legible de una propiedad, en una línea.
pub fn display(v: &PropValue) -> String {
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

/// Arma las propiedades como en Obsidian: un título «Propiedades» que las pliega y una fila baja
/// por propiedad (ícono del tipo, clave en gris y el valor, editable en el sitio). `custom` puede
/// dar filas propias para una propiedad (por ejemplo, las fuentes con su botón para abrirlas); si
/// devuelve `None`, se usa la fila de su tipo.
pub fn list(
    entries: &[PropEntry],
    mode: Mode,
    actions: Rc<Actions>,
    custom: &dyn Fn(&PropEntry) -> Option<Vec<gtk::Widget>>,
) -> gtk::Widget {
    let column = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .build();
    column.add_css_class("orbital-props");
    let filas = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(1)
        .build();
    filas.update_property(&[gtk::accessible::Property::Label("Propiedades de la nota")]);
    let editable = matches!(mode, Mode::Editable);
    let invalido = matches!(mode, Mode::Invalid(_));
    if !entries.is_empty() || invalido {
        let flecha = crate::iconos::Icono::new("abajo", 14);
        let titulo = gtk::Button::builder()
            .child(&{
                let b = gtk::Box::new(gtk::Orientation::Horizontal, 6);
                b.append(&gtk::Label::new(Some("Propiedades")));
                b.append(&flecha);
                b
            })
            .halign(gtk::Align::Start)
            .css_classes(["flat", "orbital-props-titulo"])
            .build();
        titulo.update_property(&[gtk::accessible::Property::Label(
            "Mostrar u ocultar las propiedades",
        )]);
        let f = filas.clone();
        titulo.connect_clicked(move |_| {
            let ver = !f.is_visible();
            f.set_visible(ver);
            flecha.set_nombre(if ver { "abajo" } else { "derecha" });
        });
        column.append(&titulo);
    }
    if let Mode::Invalid(err) = &mode {
        let caja = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        caja.add_css_class("orbital-props-error");
        caja.append(&gtk::Image::from_icon_name("dialog-warning-symbolic"));
        let texto = gtk::Label::builder()
            .label(format!(
                "Corregí las propiedades en la vista Código (línea {}: {})",
                err.line, err.message
            ))
            .wrap(true)
            .xalign(0.0)
            .hexpand(true)
            .build();
        caja.append(&texto);
        let go = gtk::Button::builder()
            .label("Ir a Código")
            .valign(gtk::Align::Center)
            .build();
        let a = actions.clone();
        go.connect_clicked(move |_| (a.go_code)());
        caja.append(&go);
        filas.append(&caja);
    }
    for e in entries {
        if let Some(rows) = custom(e) {
            rows.iter().for_each(|r| filas.append(r));
            continue;
        }
        filas.append(&row(e, editable, &actions));
    }
    if editable {
        filas.append(&add_button(entries, &actions));
    }
    column.append(&filas);
    if !entries.is_empty() || invalido {
        let sep = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        sep.add_css_class("orbital-props-sep");
        column.append(&sep);
    }
    column.upcast()
}

/// El ícono del tipo de una propiedad.
fn icono(key: &str, v: &PropValue) -> &'static str {
    match v {
        PropValue::Bool(_) => "prop-casilla",
        PropValue::Number(_) => "prop-numero",
        PropValue::Date(_) | PropValue::DateTime(_) => "prop-fecha",
        PropValue::Link(_) => "prop-enlace",
        PropValue::List(_) if matches!(key, "tags" | "etiquetas") => "etiqueta",
        PropValue::List(_) | PropValue::Map(_) => "callout-lista",
        PropValue::Null | PropValue::Text(_) => "prop-texto",
    }
}

/// Una fila: ícono, clave (en gris, de ancho fijo) y el valor; `quitar`, si se puede quitar (se
/// ve al pasar el mouse o con el foco).
pub fn fila(
    key: &str,
    icono: &'static str,
    valor: &impl IsA<gtk::Widget>,
    quitar: Option<gtk::Button>,
) -> gtk::Widget {
    let fila = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    fila.add_css_class("orbital-prop");
    fila.append(&crate::iconos::Icono::new(icono, 15));
    let clave = gtk::Label::builder()
        .label(key)
        .xalign(0.0)
        .width_chars(14)
        .max_width_chars(14)
        .ellipsize(gtk::pango::EllipsizeMode::End)
        .tooltip_text(key)
        .css_classes(["orbital-prop-clave"])
        .build();
    fila.append(&clave);
    valor.set_hexpand(true);
    valor.set_valign(gtk::Align::Center);
    fila.append(valor);
    if let Some(q) = quitar {
        fila.append(&q);
    }
    fila.upcast()
}

fn row(e: &PropEntry, editable: bool, actions: &Rc<Actions>) -> gtk::Widget {
    let ic = icono(&e.key, &e.value);
    if !editable {
        let valor: gtk::Widget = match &e.value {
            PropValue::List(items) if items.iter().all(is_scalar) => pastillas(items, None),
            v => texto(&display(v)).upcast(),
        };
        return fila(&e.key, ic, &valor, None);
    }
    let key = e.key.as_str();
    let valor: gtk::Widget = match &e.value {
        PropValue::Bool(on) => {
            let c = gtk::CheckButton::builder()
                .active(*on)
                .halign(gtk::Align::Start)
                .build();
            c.update_property(&[gtk::accessible::Property::Label(key)]);
            let (a, k) = (actions.clone(), key.to_owned());
            c.connect_toggled(move |c| (a.set)(&k, PropValue::Bool(c.is_active()), false));
            c.upcast()
        }
        PropValue::Number(n) => number_entry(key, n, actions).upcast(),
        PropValue::Date(d) => date_button(key, d, actions).upcast(),
        PropValue::List(items) if items.iter().all(is_scalar) => {
            pastillas(items, Some((key, actions)))
        }
        // Estructuras anidadas: se ven, pero se editan en la vista Código.
        PropValue::List(_) | PropValue::Map(_) => {
            let l = texto(&display(&e.value));
            l.set_tooltip_text(Some("Se edita en la vista Código"));
            l.upcast()
        }
        PropValue::Null | PropValue::Text(_) | PropValue::Link(_) | PropValue::DateTime(_) => {
            text_entry(key, &e.value, actions).upcast()
        }
    };
    fila(key, ic, &valor, Some(remove_button(key, actions)))
}

/// Un valor de solo lectura (seleccionable).
fn texto(t: &str) -> gtk::Label {
    gtk::Label::builder()
        .label(t)
        .xalign(0.0)
        .wrap(true)
        .wrap_mode(gtk::pango::WrapMode::WordChar)
        .selectable(true)
        .css_classes(["orbital-prop-texto"])
        .build()
}

fn is_scalar(v: &PropValue) -> bool {
    !matches!(v, PropValue::List(_) | PropValue::Map(_))
}

fn remove_button(key: &str, actions: &Rc<Actions>) -> gtk::Button {
    let b = gtk::Button::builder()
        .icon_name("user-trash-symbolic")
        .tooltip_text("Quitar la propiedad")
        .valign(gtk::Align::Center)
        .css_classes(["flat", "orbital-prop-quitar"])
        .build();
    b.update_property(&[gtk::accessible::Property::Label(&format!("Quitar «{key}»"))]);
    let (a, k) = (actions.clone(), key.to_owned());
    b.connect_clicked(move |_| (a.remove)(&k));
    b
}

/// Un campo sin borde que aplica lo escrito con Intro o al salir (nunca dos veces lo mismo).
/// `convertir` devuelve `None` si el texto no vale (y se vuelve al valor anterior).
fn campo(
    key: &str,
    actual: String,
    actions: &Rc<Actions>,
    convertir: impl Fn(String) -> Option<PropValue> + 'static,
) -> gtk::Entry {
    let entry = gtk::Entry::builder()
        .text(&actual)
        .placeholder_text("Vacío")
        .css_classes(["orbital-prop-valor"])
        .build();
    entry.update_property(&[gtk::accessible::Property::Label(key)]);
    let applied = Rc::new(std::cell::RefCell::new(actual));
    let commit = Rc::new({
        let (a, k) = (actions.clone(), key.to_owned());
        move |e: &gtk::Entry| {
            let text = e.text().to_string();
            if *applied.borrow() == text {
                return;
            }
            match convertir(text.clone()) {
                Some(v) => {
                    *applied.borrow_mut() = text;
                    (a.set)(&k, v, false);
                }
                None => e.set_text(&applied.borrow()),
            }
        }
    });
    let c = commit.clone();
    entry.connect_activate(move |e| c(e));
    let focus = gtk::EventControllerFocus::new();
    let e = entry.clone();
    focus.connect_leave(move |_| commit(&e));
    entry.add_controller(focus);
    entry
}

/// Texto, enlace o fecha y hora.
fn text_entry(key: &str, value: &PropValue, actions: &Rc<Actions>) -> gtk::Entry {
    let current = match value {
        PropValue::Null => String::new(),
        v => display(v),
    };
    let kind = value.clone();
    campo(key, current, actions, move |t| Some(typed_like(&kind, t)))
}

/// El texto escrito con el tipo que tenía la propiedad (un enlace sigue siendo enlace).
fn typed_like(kind: &PropValue, text: String) -> PropValue {
    if text.is_empty() {
        return PropValue::Null;
    }
    match kind {
        PropValue::Link(_) => PropValue::Link(text),
        PropValue::DateTime(_) => PropValue::DateTime(text),
        _ => PropValue::Text(text),
    }
}

/// Número: se escribe como texto; si no es un número, vuelve al anterior.
fn number_entry(key: &str, n: &str, actions: &Rc<Actions>) -> gtk::Entry {
    let e = campo(key, n.to_owned(), actions, |t| {
        let t = t.trim().to_owned();
        t.parse::<f64>()
            .ok()
            .filter(|f| f.is_finite())
            .map(|_| PropValue::Number(t))
    });
    e.set_input_purpose(gtk::InputPurpose::Number);
    e
}

fn date_button(key: &str, date: &str, actions: &Rc<Actions>) -> gtk::MenuButton {
    let calendar = gtk::Calendar::new();
    if let Some(d) = parse_date(date) {
        calendar.select_day(&d);
    }
    // Con un rótulo propio (y no `label`), el botón no muestra la flecha de menú.
    let rotulo = gtk::Label::new(Some(date));
    let button = gtk::MenuButton::builder()
        .child(&rotulo)
        .halign(gtk::Align::Start)
        .css_classes(["flat", "orbital-prop-boton"])
        .popover(&gtk::Popover::builder().child(&calendar).build())
        .build();
    button.update_property(&[gtk::accessible::Property::Label(&format!(
        "Elegir la fecha de «{key}»"
    ))]);
    let (a, k, b) = (actions.clone(), key.to_owned(), button.clone());
    calendar.connect_day_selected(move |c| {
        let d = c.date();
        let text = format!("{:04}-{:02}-{:02}", d.year(), d.month(), d.day_of_month());
        rotulo.set_label(&text);
        if let Some(p) = b.popover() {
            p.popdown();
        }
        (a.set)(&k, PropValue::Date(text), false);
    });
    button
}

fn parse_date(s: &str) -> Option<glib::DateTime> {
    let mut parts = s.splitn(3, '-').map(|p| p.parse::<i32>().ok());
    let (y, m, d) = (parts.next()??, parts.next()??, parts.next()??);
    glib::DateTime::from_local(y, m, d, 12, 0, 0.0).ok()
}

/// Lista de valores simples como pastillas (con su ✕) y un campo para agregar al final. Sin
/// `editar`, solo las pastillas.
fn pastillas(items: &[PropValue], editar: Option<(&str, &Rc<Actions>)>) -> gtk::Widget {
    let caja = gtk::FlowBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .column_spacing(4)
        .row_spacing(4)
        .max_children_per_line(64)
        .homogeneous(false)
        .build();
    for (n, item) in items.iter().enumerate() {
        let texto = display(item);
        let p = gtk::Box::new(gtk::Orientation::Horizontal, 2);
        p.add_css_class("orbital-pastilla");
        p.append(&gtk::Label::new(Some(&texto)));
        match editar {
            Some((key, actions)) => {
                let quit = gtk::Button::builder()
                    .child(&crate::iconos::Icono::new("callout-cruz", 12))
                    .valign(gtk::Align::Center)
                    .css_classes(["flat", "circular"])
                    .build();
                quit.update_property(&[gtk::accessible::Property::Label(&format!(
                    "Quitar «{texto}» de «{key}»"
                ))]);
                let (a, k, all) = (actions.clone(), key.to_owned(), items.to_vec());
                quit.connect_clicked(move |_| {
                    let mut rest = all.clone();
                    rest.remove(n);
                    (a.set)(&k, PropValue::List(rest), true);
                });
                p.append(&quit);
            }
            None => p.add_css_class("solo"),
        }
        let hijo = gtk::FlowBoxChild::builder()
            .child(&p)
            .focusable(false)
            .halign(gtk::Align::Start)
            .build();
        caja.append(&hijo);
    }
    if let Some((key, actions)) = editar {
        let add = gtk::Entry::builder()
            .placeholder_text("Agregar…")
            .width_chars(10)
            .css_classes(["orbital-prop-valor"])
            .build();
        add.update_property(&[gtk::accessible::Property::Label(&format!(
            "Agregar a «{key}»"
        ))]);
        let (a, k, all) = (actions.clone(), key.to_owned(), items.to_vec());
        add.connect_activate(move |r| {
            let text = r.text().trim().to_owned();
            if text.is_empty() {
                return;
            }
            let mut more = all.clone();
            more.push(PropValue::Text(text));
            (a.set)(&k, PropValue::List(more), true);
        });
        let hijo = gtk::FlowBoxChild::builder()
            .child(&add)
            .focusable(false)
            .build();
        caja.append(&hijo);
    }
    caja.upcast()
}

/// Tipos que se pueden elegir al agregar una propiedad, con su valor inicial.
const KINDS: [&str; 5] = ["Texto", "Número", "Casilla", "Fecha", "Lista"];

fn initial(kind: u32) -> PropValue {
    match kind {
        1 => PropValue::Number("0".into()),
        2 => PropValue::Bool(false),
        3 => {
            let now = glib::DateTime::now_local().ok();
            PropValue::Date(now.map_or_else(
                || "2026-01-01".to_owned(),
                |d| format!("{:04}-{:02}-{:02}", d.year(), d.month(), d.day_of_month()),
            ))
        }
        4 => PropValue::List(Vec::new()),
        _ => PropValue::Null,
    }
}

/// Un nombre de propiedad válido: sin espacios al borde ni caracteres que rompan el YAML.
pub fn valid_key(key: &str) -> bool {
    !key.is_empty()
        && key.trim() == key
        && !key.starts_with(['-', '#', '&', '*', '!', '|', '>', '\'', '"', '%', '@', '`'])
        && !key.contains([':', '\n', '{', '}', '[', ']', ','])
}

/// «Agregar propiedad»: un diálogo pide nombre y tipo; va al final del bloque.
fn add_button(entries: &[PropEntry], actions: &Rc<Actions>) -> gtk::Widget {
    let button = gtk::Button::builder()
        .child(
            &adw::ButtonContent::builder()
                .icon_name("list-add-symbolic")
                .label("Agregar propiedad")
                .build(),
        )
        .halign(gtk::Align::Start)
        .css_classes(["flat", "orbital-props-agregar"])
        .build();
    let taken: Rc<Vec<String>> = Rc::new(entries.iter().map(|e| e.key.clone()).collect());
    let a = actions.clone();
    button.connect_clicked(move |b| add_dialog(b.upcast_ref(), taken.clone(), a.clone()));
    button.upcast()
}

fn add_dialog(parent: &gtk::Widget, taken: Rc<Vec<String>>, actions: Rc<Actions>) {
    let dialog = adw::AlertDialog::builder()
        .heading("Agregar propiedad")
        .default_response("add")
        .close_response("cancel")
        .build();
    dialog.add_responses(&[("cancel", "Cancelar"), ("add", "Agregar")]);
    dialog.set_response_appearance("add", adw::ResponseAppearance::Suggested);
    dialog.set_response_enabled("add", false);
    let fields = gtk::ListBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .build();
    fields.add_css_class("boxed-list");
    let name = adw::EntryRow::builder()
        .title("Nombre")
        .activates_default(true)
        .build();
    let kind = adw::ComboRow::builder()
        .title("Tipo")
        .model(&gtk::StringList::new(&KINDS))
        .build();
    fields.append(&name);
    fields.append(&kind);
    dialog.set_extra_child(Some(&fields));
    let d = dialog.clone();
    name.connect_changed(move |n| {
        let key = n.text();
        let ok = valid_key(&key) && !taken.iter().any(|t| t == key.as_str());
        d.set_response_enabled("add", ok);
        if !key.is_empty() && !ok {
            n.add_css_class("error");
        } else {
            n.remove_css_class("error");
        }
    });
    dialog.connect_response(Some("add"), move |_, _| {
        let key = name.text().to_string();
        (actions.set)(&key, initial(kind.selected()), true);
    });
    dialog.present(Some(parent));
}

#[cfg(test)]
mod tests {
    use super::valid_key;

    #[test]
    fn keys() {
        assert!(valid_key("autor"));
        assert!(valid_key("fecha de lectura"));
        assert!(!valid_key(""));
        assert!(!valid_key(" x"));
        assert!(!valid_key("a:b"));
        assert!(!valid_key("- x"));
        assert!(!valid_key("[x]"));
    }
}
