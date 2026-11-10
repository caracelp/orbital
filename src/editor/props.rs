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

/// Arma la lista. `custom` puede dar una fila propia para una propiedad (por ejemplo, las
/// fuentes con su botón para abrirlas); si devuelve `None`, se usa la fila de su tipo.
pub fn list(
    entries: &[PropEntry],
    mode: Mode,
    actions: Rc<Actions>,
    custom: &dyn Fn(&PropEntry) -> Option<Vec<gtk::Widget>>,
) -> gtk::Widget {
    let list = gtk::ListBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .build();
    list.add_css_class("boxed-list");
    list.update_property(&[gtk::accessible::Property::Label("Propiedades de la nota")]);
    let editable = matches!(mode, Mode::Editable);
    if let Mode::Invalid(err) = &mode {
        let row = adw::ActionRow::builder()
            .title("Corregí las propiedades en la vista Código")
            .subtitle(glib::markup_escape_text(&format!(
                "Línea {}: {}",
                err.line, err.message
            )))
            .build();
        row.add_prefix(&gtk::Image::from_icon_name("dialog-warning-symbolic"));
        let go = gtk::Button::builder()
            .label("Ir a Código")
            .valign(gtk::Align::Center)
            .build();
        let a = actions.clone();
        go.connect_clicked(move |_| (a.go_code)());
        row.add_suffix(&go);
        list.append(&row);
    }
    for e in entries {
        if let Some(rows) = custom(e) {
            rows.iter().for_each(|r| list.append(r));
            continue;
        }
        let row = if editable {
            editable_row(e, &actions)
        } else {
            read_only_row(e)
        };
        list.append(&row);
    }
    let column = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(6)
        .margin_bottom(18)
        .build();
    if entries.is_empty() && !matches!(mode, Mode::Invalid(_)) {
        list.set_visible(false);
    }
    column.append(&list);
    if editable {
        column.append(&add_button(entries, &actions));
    }
    column.upcast()
}

fn read_only_row(e: &PropEntry) -> gtk::Widget {
    let row = adw::ActionRow::builder()
        .title(glib::markup_escape_text(&e.key))
        .subtitle(glib::markup_escape_text(&display(&e.value)))
        .subtitle_selectable(true)
        .build();
    row.add_css_class("property");
    row.upcast()
}

fn remove_button(key: &str, actions: &Rc<Actions>) -> gtk::Button {
    let b = gtk::Button::builder()
        .icon_name("user-trash-symbolic")
        .tooltip_text("Quitar la propiedad")
        .valign(gtk::Align::Center)
        .css_classes(["flat"])
        .build();
    b.update_property(&[gtk::accessible::Property::Label(&format!("Quitar «{key}»"))]);
    let (a, k) = (actions.clone(), key.to_owned());
    b.connect_clicked(move |_| (a.remove)(&k));
    b
}

fn editable_row(e: &PropEntry, actions: &Rc<Actions>) -> gtk::Widget {
    let key = e.key.clone();
    match &e.value {
        PropValue::Bool(on) => {
            let row = adw::SwitchRow::builder()
                .title(glib::markup_escape_text(&key))
                .active(*on)
                .build();
            let a = actions.clone();
            row.connect_active_notify(move |r| {
                (a.set)(&key, PropValue::Bool(r.is_active()), false)
            });
            row.add_suffix(&remove_button(&e.key, actions));
            row.upcast()
        }
        PropValue::Number(n) => number_row(&key, n, actions),
        PropValue::Date(d) => date_row(&key, d, actions),
        PropValue::List(items) if items.iter().all(is_scalar) => list_row(&key, items, actions),
        PropValue::List(_) | PropValue::Map(_) => {
            // Estructuras anidadas: se ven, pero se editan en la vista Código.
            let row = adw::ActionRow::builder()
                .title(glib::markup_escape_text(&key))
                .subtitle(glib::markup_escape_text(&display(&e.value)))
                .subtitle_selectable(true)
                .tooltip_text("Se edita en la vista Código")
                .build();
            row.add_suffix(&remove_button(&e.key, actions));
            row.upcast()
        }
        PropValue::Null | PropValue::Text(_) | PropValue::Link(_) | PropValue::DateTime(_) => {
            text_row(&key, &e.value, actions)
        }
    }
}

fn is_scalar(v: &PropValue) -> bool {
    !matches!(v, PropValue::List(_) | PropValue::Map(_))
}

/// Texto, enlace o fecha y hora: se aplica con Intro o el botón de aplicar.
fn text_row(key: &str, value: &PropValue, actions: &Rc<Actions>) -> gtk::Widget {
    let current = match value {
        PropValue::Null => String::new(),
        v => display(v),
    };
    let row = adw::EntryRow::builder()
        .title(glib::markup_escape_text(key))
        .text(&current)
        .show_apply_button(true)
        .build();
    // Se aplica con Intro, con el botón de aplicar o al salir del campo (para no perder lo
    // escrito si el usuario hace clic en otro lado); nunca dos veces el mismo valor.
    let applied = Rc::new(std::cell::RefCell::new(current.clone()));
    let commit = {
        let (a, k, kind, applied) = (actions.clone(), key.to_owned(), value.clone(), applied);
        move |r: &adw::EntryRow| {
            let text = r.text().to_string();
            if *applied.borrow() == text {
                return;
            }
            *applied.borrow_mut() = text.clone();
            (a.set)(&k, typed_like(&kind, text), false);
        }
    };
    let c = commit.clone();
    row.connect_apply(move |r| c(r));
    let focus = gtk::EventControllerFocus::new();
    let r = row.clone();
    focus.connect_leave(move |_| commit(&r));
    row.add_controller(focus);
    row.add_suffix(&remove_button(key, actions));
    row.upcast()
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

fn number_row(key: &str, n: &str, actions: &Rc<Actions>) -> gtk::Widget {
    // `adw::SpinRow` no aparece en el árbol de accesibilidad (GTK 4.24, libadwaita 1.8): se usa
    // una fila con un `gtk::SpinButton`, que sí.
    let digits = n.split_once('.').map_or(0, |(_, d)| d.len() as u32);
    let value: f64 = n.parse().unwrap_or(0.0);
    let step = 10f64.powi(-(digits as i32));
    let adj = gtk::Adjustment::new(value, -1e12, 1e12, step, step * 10.0, 0.0);
    let spin = gtk::SpinButton::builder()
        .adjustment(&adj)
        .digits(digits)
        .valign(gtk::Align::Center)
        .build();
    spin.update_property(&[gtk::accessible::Property::Label(key)]);
    let row = adw::ActionRow::builder()
        .title(glib::markup_escape_text(key))
        .build();
    let (a, k) = (actions.clone(), key.to_owned());
    spin.connect_value_changed(move |s| {
        let text = format!("{:.*}", digits as usize, s.value());
        (a.set)(&k, PropValue::Number(text), false);
    });
    row.add_suffix(&spin);
    row.add_suffix(&remove_button(key, actions));
    row.upcast()
}

fn date_row(key: &str, date: &str, actions: &Rc<Actions>) -> gtk::Widget {
    let row = adw::ActionRow::builder()
        .title(glib::markup_escape_text(key))
        .build();
    let calendar = gtk::Calendar::new();
    if let Some(d) = parse_date(date) {
        calendar.select_day(&d);
    }
    let button = gtk::MenuButton::builder()
        .label(date)
        .valign(gtk::Align::Center)
        .popover(&gtk::Popover::builder().child(&calendar).build())
        .build();
    button.update_property(&[gtk::accessible::Property::Label(&format!(
        "Elegir la fecha de «{key}»"
    ))]);
    let (a, k, b) = (actions.clone(), key.to_owned(), button.clone());
    calendar.connect_day_selected(move |c| {
        let d = c.date();
        let text = format!("{:04}-{:02}-{:02}", d.year(), d.month(), d.day_of_month());
        b.set_label(&text);
        if let Some(p) = b.popover() {
            p.popdown();
        }
        (a.set)(&k, PropValue::Date(text), false);
    });
    row.add_suffix(&button);
    row.set_activatable_widget(Some(&button));
    row.add_suffix(&remove_button(key, actions));
    row.upcast()
}

fn parse_date(s: &str) -> Option<glib::DateTime> {
    let mut parts = s.splitn(3, '-').map(|p| p.parse::<i32>().ok());
    let (y, m, d) = (parts.next()??, parts.next()??, parts.next()??);
    glib::DateTime::from_local(y, m, d, 12, 0, 0.0).ok()
}

/// Lista de valores simples: una fila por elemento (con quitar) y un campo para agregar.
fn list_row(key: &str, items: &[PropValue], actions: &Rc<Actions>) -> gtk::Widget {
    let row = adw::ExpanderRow::builder()
        .title(glib::markup_escape_text(key))
        .subtitle(glib::markup_escape_text(
            &items.iter().map(display).collect::<Vec<_>>().join(", "),
        ))
        .build();
    for (n, item) in items.iter().enumerate() {
        let r = adw::ActionRow::builder()
            .title(glib::markup_escape_text(&display(item)))
            .build();
        let quit = gtk::Button::builder()
            .icon_name("list-remove-symbolic")
            .valign(gtk::Align::Center)
            .css_classes(["flat"])
            .build();
        quit.update_property(&[gtk::accessible::Property::Label(&format!(
            "Quitar «{}» de «{key}»",
            display(item)
        ))]);
        let (a, k, all) = (actions.clone(), key.to_owned(), items.to_vec());
        quit.connect_clicked(move |_| {
            let mut rest = all.clone();
            rest.remove(n);
            (a.set)(&k, PropValue::List(rest), true);
        });
        r.add_suffix(&quit);
        row.add_row(&r);
    }
    let add = adw::EntryRow::builder()
        .title(format!("Agregar a «{}»", glib::markup_escape_text(key)))
        .show_apply_button(true)
        .build();
    let (a, k, all) = (actions.clone(), key.to_owned(), items.to_vec());
    add.connect_apply(move |r| {
        let text = r.text().trim().to_owned();
        if text.is_empty() {
            return;
        }
        let mut more = all.clone();
        more.push(PropValue::Text(text));
        (a.set)(&k, PropValue::List(more), true);
    });
    row.add_row(&add);
    row.add_suffix(&remove_button(key, actions));
    row.upcast()
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
        .css_classes(["flat"])
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
