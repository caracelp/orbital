//! Panel lateral derecho con la sección «Retroenlaces» (US4 de la 003, research R3).
//!
//! Solo muestra: el grafo de enlaces lo mantiene la ventana en segundo plano. Cada entrada es un
//! botón (nombre accesible, se activa con el teclado y por AT-SPI) que abre la nota de origen en la
//! línea del enlace. La sección se condensa con su botón de cabecera.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gtk::prelude::*;
use orbital_vault::{BacklinkRef, RelPath};

/// Largo máximo del fragmento de contexto, en caracteres.
const SNIPPET: usize = 160;

/// Qué mostrar en la sección.
pub enum Backlinks<'a> {
    /// No hay nota abierta.
    NoNote,
    /// El grafo todavía se está armando.
    Loading,
    Ready(&'a [BacklinkRef]),
}

type OpenFn = Box<dyn Fn(&RelPath, usize)>;

struct Inner {
    root: gtk::Box,
    header: gtk::Button,
    arrow: gtk::Image,
    count: gtk::Label,
    revealer: gtk::Revealer,
    list: gtk::ListBox,
    message: gtk::Label,
    expanded: Cell<bool>,
    on_open: RefCell<Option<OpenFn>>,
}

#[derive(Clone)]
pub struct RightSidebar(Rc<Inner>);

impl Default for RightSidebar {
    fn default() -> Self {
        Self::new()
    }
}

impl RightSidebar {
    pub fn new() -> Self {
        let root = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(6)
            .margin_top(12)
            .margin_bottom(12)
            .margin_start(12)
            .margin_end(12)
            .build();
        root.update_property(&[gtk::accessible::Property::Label("Panel de la nota")]);

        let arrow = gtk::Image::from_icon_name("pan-down-symbolic");
        let title = gtk::Label::builder()
            .label("Retroenlaces")
            .xalign(0.0)
            .hexpand(true)
            .css_classes(["heading"])
            .build();
        let count = gtk::Label::builder()
            .css_classes(["dim-label", "numeric"])
            .build();
        let head = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        head.append(&arrow);
        head.append(&title);
        head.append(&count);
        let header = gtk::Button::builder()
            .child(&head)
            .css_classes(["flat"])
            .tooltip_text("Condensar o expandir la sección")
            .build();

        let list = gtk::ListBox::builder()
            .selection_mode(gtk::SelectionMode::None)
            .css_classes(["boxed-list"])
            .build();
        list.update_property(&[gtk::accessible::Property::Label("Notas que enlazan a esta")]);
        let message = gtk::Label::builder()
            .wrap(true)
            .xalign(0.0)
            .margin_start(6)
            .css_classes(["dim-label"])
            .build();
        let body = gtk::Box::new(gtk::Orientation::Vertical, 6);
        body.append(&message);
        body.append(&list);
        let revealer = gtk::Revealer::builder()
            .child(&body)
            .reveal_child(true)
            .transition_type(gtk::RevealerTransitionType::SlideDown)
            .build();

        let content = gtk::Box::new(gtk::Orientation::Vertical, 6);
        content.append(&header);
        content.append(&revealer);
        // Va dentro del modo Contexto de la barra derecha (025), que ya desplaza: toma su alto
        // natural en vez de encogerse a cero.
        let scroller = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vscrollbar_policy(gtk::PolicyType::Never)
            .propagate_natural_height(true)
            .child(&content)
            .build();
        root.append(&scroller);

        let this = Self(Rc::new(Inner {
            root,
            header,
            arrow,
            count,
            revealer,
            list,
            message,
            expanded: Cell::new(true),
            on_open: RefCell::new(None),
        }));
        // Condensada, la lista se oculta del todo al terminar la animación (si no, un lector de
        // pantalla la seguiría leyendo).
        this.0.revealer.connect_child_revealed_notify(|r| {
            if let Some(c) = r.child() {
                c.set_visible(r.reveals_child() || r.is_child_revealed());
            }
        });
        let weak = Rc::downgrade(&this.0);
        this.0.header.connect_clicked(move |_| {
            if let Some(i) = weak.upgrade() {
                let on = !i.expanded.get();
                RightSidebar(i).set_expanded(on);
            }
        });
        this.set_backlinks(Backlinks::NoNote);
        this
    }

    pub fn widget(&self) -> &gtk::Box {
        &self.0.root
    }

    /// Al elegir una entrada: la nota de origen y la línea (desde 1).
    pub fn connect_open(&self, f: impl Fn(&RelPath, usize) + 'static) {
        *self.0.on_open.borrow_mut() = Some(Box::new(f));
    }

    fn set_expanded(&self, on: bool) {
        let i = &self.0;
        i.expanded.set(on);
        if on && let Some(c) = i.revealer.child() {
            c.set_visible(true);
        }
        i.revealer.set_reveal_child(on);
        i.arrow.set_icon_name(Some(if on {
            "pan-down-symbolic"
        } else {
            "pan-end-symbolic"
        }));
        i.header
            .update_state(&[gtk::accessible::State::Expanded(Some(on))]);
        self.label_header();
    }

    fn label_header(&self) {
        let i = &self.0;
        let count = i.count.label();
        let state = if i.expanded.get() {
            "expandida"
        } else {
            "condensada"
        };
        let label = if count.is_empty() {
            format!("Retroenlaces, sección {state}")
        } else {
            format!("Retroenlaces: {count}, sección {state}")
        };
        i.header
            .update_property(&[gtk::accessible::Property::Label(&label)]);
    }

    /// Muestra los retroenlaces de la nota abierta.
    pub fn set_backlinks(&self, backlinks: Backlinks) {
        let i = &self.0;
        i.list.remove_all();
        let (count, message) = match backlinks {
            Backlinks::NoNote => (
                String::new(),
                Some("Abrí una nota para ver qué notas la enlazan."),
            ),
            Backlinks::Loading => (String::new(), Some("Buscando retroenlaces…")),
            Backlinks::Ready([]) => (
                "0".to_owned(),
                Some("Sin retroenlaces: ninguna nota enlaza a esta."),
            ),
            Backlinks::Ready(refs) => {
                let mut notes: Vec<&RelPath> = refs.iter().map(|b| &b.from).collect();
                notes.sort();
                notes.dedup();
                // Primero las relaciones con tipo (049), agrupadas por su nombre inverso; los
                // enlaces sin tipo, después y como siempre.
                let (grupos, sin_tipo) = agrupar_por_relacion(refs);
                for (nombre, refs_grupo) in &grupos {
                    i.list.append(&encabezado(nombre, refs_grupo.len()));
                    for b in refs_grupo {
                        i.list.append(&self.row(b));
                    }
                }
                if !grupos.is_empty() && !sin_tipo.is_empty() {
                    i.list
                        .append(&encabezado("Enlaces sin tipo", sin_tipo.len()));
                }
                for b in sin_tipo {
                    i.list.append(&self.row(b));
                }
                let n = notes.len();
                (n.to_string(), None)
            }
        };
        i.count.set_label(&count);
        i.message.set_label(message.unwrap_or(""));
        i.message.set_visible(message.is_some());
        i.list.set_visible(message.is_none());
        self.label_header();
    }

    fn row(&self, b: &BacklinkRef) -> gtk::Widget {
        let file = b.from.file_name();
        let name = file.rsplit_once('.').map_or(file, |(s, _)| s);
        let folder = b
            .from
            .as_str()
            .strip_prefix("wiki/")
            .unwrap_or(b.from.as_str())
            .rsplit_once('/')
            .map_or("", |(d, _)| d);
        let snippet = shorten(&b.snippet, SNIPPET);

        let column = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(2)
            .margin_top(6)
            .margin_bottom(6)
            .build();
        let top = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        top.append(
            &gtk::Label::builder()
                .label(name)
                .xalign(0.0)
                .hexpand(true)
                .ellipsize(gtk::pango::EllipsizeMode::End)
                .css_classes(["heading"])
                .build(),
        );
        top.append(
            &gtk::Label::builder()
                .label(format!("línea {}", b.line))
                .css_classes(["dim-label", "caption", "numeric"])
                .build(),
        );
        column.append(&top);
        if !folder.is_empty() {
            column.append(
                &gtk::Label::builder()
                    .label(folder)
                    .xalign(0.0)
                    .ellipsize(gtk::pango::EllipsizeMode::Middle)
                    .css_classes(["dim-label", "caption"])
                    .build(),
            );
        }
        let motivo = b.relacion.as_ref().and_then(|r| r.motivo.as_deref());
        if let Some(m) = motivo {
            column.append(
                &gtk::Label::builder()
                    .label(format!("Motivo: {m}"))
                    .xalign(0.0)
                    .wrap(true)
                    .wrap_mode(gtk::pango::WrapMode::WordChar)
                    .build(),
            );
        }
        // Con tipo, el renglón fuente (`- amplía:: [[…]] — …`) repetiría lo que ya dicen el grupo
        // y el motivo: solo los enlaces sin tipo llevan su fragmento de contexto.
        if b.relacion.is_none() {
            column.append(
                &gtk::Label::builder()
                    .label(&snippet)
                    .xalign(0.0)
                    .wrap(true)
                    .wrap_mode(gtk::pango::WrapMode::WordChar)
                    .lines(3)
                    .ellipsize(gtk::pango::EllipsizeMode::End)
                    .css_classes(["dim-label"])
                    .build(),
            );
        }
        let button = gtk::Button::builder()
            .child(&column)
            .css_classes(["flat"])
            .build();
        let rotulo = match &b.relacion {
            Some(r) => {
                use orbital_vault::relaciones as rel;
                // Un tipo base se lee con su inverso («ampliada por Derivadas»); uno libre, en
                // el sentido en que se escribió («Derivadas refuta esta nota»).
                let relacion = match rel::tipo_base(&r.tipo) {
                    Some(_) => format!("{} {name}", rel::nombre_inverso(&r.tipo)),
                    None => format!("{name} {} esta nota", rel::nombre_canonico(&r.tipo)),
                };
                match motivo {
                    Some(m) => format!("Abrir {name}: {relacion}; motivo: {m}; línea {}", b.line),
                    None => format!("Abrir {name}: {relacion}; línea {}", b.line),
                }
            }
            None => format!("Abrir {name}, línea {}: {snippet}", b.line),
        };
        button.update_property(&[gtk::accessible::Property::Label(&rotulo)]);
        let (weak, from, line) = (Rc::downgrade(&self.0), b.from.clone(), b.line);
        button.connect_clicked(move |_| {
            if let Some(i) = weak.upgrade()
                && let Some(f) = i.on_open.borrow().as_ref()
            {
                f(&from, line);
            }
        });
        button.upcast()
    }
}

/// Separa los retroenlaces con tipo, agrupados por el nombre inverso del tipo en el orden en
/// que aparece cada uno (sin distinguir mayúsculas ni tildes), de los que no tienen tipo.
pub(crate) fn agrupar_por_relacion(
    refs: &[BacklinkRef],
) -> (Vec<(String, Vec<&BacklinkRef>)>, Vec<&BacklinkRef>) {
    use orbital_vault::relaciones::{clave, nombre_inverso};
    let mut grupos: Vec<(String, String, Vec<&BacklinkRef>)> = Vec::new();
    let mut sin_tipo = Vec::new();
    for b in refs {
        let Some(r) = &b.relacion else {
            sin_tipo.push(b);
            continue;
        };
        let k = clave(&r.tipo);
        match grupos.iter_mut().find(|g| g.0 == k) {
            Some(g) => g.2.push(b),
            None => grupos.push((k, nombre_inverso(&r.tipo), vec![b])),
        }
    }
    let grupos = grupos.into_iter().map(|(_, n, v)| (n, v)).collect();
    (grupos, sin_tipo)
}

/// Fila de encabezado de un grupo: no se elige ni se activa; el lector de pantalla la lee.
fn encabezado(nombre: &str, n: usize) -> gtk::ListBoxRow {
    let mut titulo = String::with_capacity(nombre.len());
    let mut chars = nombre.chars();
    if let Some(c) = chars.next() {
        titulo.extend(c.to_uppercase());
        titulo.push_str(chars.as_str());
    }
    let label = gtk::Label::builder()
        .label(&titulo)
        .xalign(0.0)
        .margin_top(8)
        .margin_bottom(4)
        .margin_start(8)
        .css_classes(["caption-heading", "accent"])
        .build();
    let row = gtk::ListBoxRow::builder()
        .child(&label)
        .activatable(false)
        .selectable(false)
        .focusable(false)
        .build();
    let notas = if n == 1 { "nota" } else { "notas" };
    let accesible = match titulo.strip_prefix("← ") {
        Some(tipo) => format!("Relación entrante «{tipo}»: {n} {notas}"),
        None => format!("{titulo}: {n} {notas}"),
    };
    row.update_property(&[gtk::accessible::Property::Label(&accesible)]);
    row
}

/// Corta `s` a `max` caracteres con «…».
fn shorten(s: &str, max: usize) -> String {
    match s.char_indices().nth(max) {
        Some((at, _)) => format!("{}…", s[..at].trim_end()),
        None => s.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::{agrupar_por_relacion, shorten};
    use orbital_vault::{BacklinkRef, LinkTarget, RelPath, RelacionInfo};
    use std::sync::Arc;

    fn r(from: &str, tipo: Option<&str>) -> BacklinkRef {
        BacklinkRef {
            from: RelPath::new(from).unwrap(),
            target: LinkTarget {
                path: "x".into(),
                anchor: None,
                alias: None,
            },
            line: 1,
            snippet: String::new(),
            relacion: tipo.map(|t| {
                Arc::new(RelacionInfo {
                    tipo: t.into(),
                    motivo: None,
                })
            }),
        }
    }

    /// Los textos de las etiquetas que hay debajo de `w`, en orden.
    fn textos(w: &gtk::Widget) -> Vec<String> {
        use gtk::prelude::*;
        let mut out = Vec::new();
        if let Some(l) = w.downcast_ref::<gtk::Label>() {
            out.push(l.label().to_string());
        }
        let mut hijo = w.first_child();
        while let Some(h) = hijo {
            out.extend(textos(&h));
            hijo = h.next_sibling();
        }
        out
    }

    /// 049: un retroenlace con tipo muestra la nota, la línea y el motivo, sin repetir el
    /// renglón fuente; uno sin tipo conserva su fragmento de contexto.
    #[test]
    #[ignore = "requiere acceso GTK; tools/pruebas-gtk.sh"]
    fn retroenlace_con_tipo_sin_renglon_fuente() {
        use gtk::prelude::*;
        gtk::init().expect("el proceso necesita conectarse a una sesión GTK");
        let mut con_tipo = r("wiki/Regla.md", Some("amplía"));
        con_tipo.snippet = "- amplía:: [[Derivadas]] — agrega la regla".into();
        con_tipo.relacion = Some(Arc::new(RelacionInfo {
            tipo: "amplía".into(),
            motivo: Some("agrega la regla".into()),
        }));
        let mut sin_tipo = r("wiki/Otra.md", None);
        sin_tipo.snippet = "Texto que cita a [[Derivadas]].".into();
        let panel = super::RightSidebar::new();
        panel.set_backlinks(super::Backlinks::Ready(&[con_tipo, sin_tipo]));
        let todo = textos(panel.widget().upcast_ref()).join("\n");
        assert!(todo.contains("Ampliada por"), "{todo}");
        assert!(todo.contains("Motivo: agrega la regla"), "{todo}");
        assert!(!todo.contains("amplía::"), "sin el renglón fuente: {todo}");
        assert!(todo.contains("Texto que cita a [[Derivadas]]."), "{todo}");
        assert!(todo.contains("Enlaces sin tipo"), "{todo}");
    }

    #[test]
    fn agrupa_por_nombre_inverso() {
        let refs = [
            r("wiki/a.md", Some("amplía")),
            r("wiki/b.md", None),
            r("wiki/c.md", Some("se opone a")),
            r("wiki/d.md", Some("AMPLIA")),
            r("wiki/e.md", Some("refuta")),
        ];
        let (grupos, sin_tipo) = agrupar_por_relacion(&refs);
        let nombres: Vec<(&str, usize)> =
            grupos.iter().map(|(n, v)| (n.as_str(), v.len())).collect();
        assert_eq!(
            nombres,
            [("ampliada por", 2), ("se opone a", 1), ("← refuta", 1)]
        );
        assert_eq!(sin_tipo.len(), 1);
    }

    #[test]
    fn shorten_keeps_char_boundaries() {
        assert_eq!(shorten("energía", 10), "energía");
        assert_eq!(shorten("áéíóú xyz", 5), "áéíóú…");
    }
}
