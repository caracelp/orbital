//! Autocompletado de enlaces `[[` (US3, FR-008, FR-009, FR-020).
//!
//! Al tipear `[[`, despliega un `gtk::Popover` anclado a la posición del cursor de texto
//! con las notas y alias de la bóveda filtradas en tiempo real.

use std::cell::RefCell;
use std::rc::Rc;

use gtk::gdk;
use gtk::prelude::*;
use orbital_vault::RelPath;

/// Candidato para autocompletar un enlace.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompletionCandidate {
    pub stem: String,
    pub title: Option<String>,
    pub aliases: Vec<String>,
    pub path: RelPath,
}

impl CompletionCandidate {
    /// Retorna el alias que coincide con la consulta, si lo hay.
    pub fn matching_alias(&self, query: &str) -> Option<&str> {
        let q = query.trim().to_lowercase();
        if q.is_empty() {
            return None;
        }
        self.aliases
            .iter()
            .find(|a| a.to_lowercase().contains(&q))
            .map(String::as_str)
    }

    /// Retorna el texto del destino wikilink a insertar según la consulta.
    /// Si coincide con un alias, devuelve `stem|alias`; si tiene alias y no se buscó stem exacto,
    /// devuelve `stem|primer_alias`; en caso contrario `stem`.
    pub fn link_target(&self, query: &str) -> String {
        if let Some(alias) = self.matching_alias(query) {
            format!("{}|{}", self.stem, alias)
        } else if query.trim().is_empty() && !self.aliases.is_empty() {
            format!("{}|{}", self.stem, self.aliases[0])
        } else {
            self.stem.clone()
        }
    }
}

type SelectedFn = Box<dyn Fn(CompletionCandidate, String)>;

struct Inner {
    popover: gtk::Popover,
    list: gtk::ListBox,
    candidates: RefCell<Vec<CompletionCandidate>>,
    filtered: RefCell<Vec<CompletionCandidate>>,
    query: RefCell<String>,
    on_selected: RefCell<Option<SelectedFn>>,
}

#[derive(Clone)]
pub struct LinkCompletionPopover(Rc<Inner>);

/// Filtra candidatos según la consulta (coincidencia insensible a mayúsculas en nombre, título o alias).
pub fn filter_candidates(
    candidates: &[CompletionCandidate],
    query: &str,
    limit: usize,
) -> Vec<CompletionCandidate> {
    let q = query.trim().to_lowercase();
    let mut exact = Vec::new();
    let mut starts = Vec::new();
    let mut contains = Vec::new();

    for c in candidates.iter() {
        if q.is_empty() {
            exact.push(c.clone());
            if exact.len() >= limit {
                break;
            }
            continue;
        }
        let stem_lower = c.stem.to_lowercase();
        let title_lower = c.title.as_deref().map(str::to_lowercase);
        let alias_match = c.aliases.iter().any(|a| a.to_lowercase().contains(&q));

        if stem_lower == q {
            exact.push(c.clone());
        } else if stem_lower.starts_with(&q)
            || title_lower.as_ref().is_some_and(|t| t.starts_with(&q))
        {
            starts.push(c.clone());
        } else if stem_lower.contains(&q)
            || title_lower.as_ref().is_some_and(|t| t.contains(&q))
            || alias_match
        {
            contains.push(c.clone());
        }
    }
    let mut results = exact;
    results.extend(starts);
    results.extend(contains);
    results.truncate(limit);
    results
}

impl LinkCompletionPopover {
    pub fn new(parent: &impl IsA<gtk::Widget>) -> Self {
        let list = gtk::ListBox::builder()
            .selection_mode(gtk::SelectionMode::Single)
            .css_classes(["orbital-completion-list"])
            .focusable(false)
            .build();
        list.update_property(&[gtk::accessible::Property::Label("Sugerencias de enlaces")]);

        let scroller = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vscrollbar_policy(gtk::PolicyType::Automatic)
            .propagate_natural_height(true)
            .max_content_height(280)
            .min_content_width(260)
            .child(&list)
            .build();

        let popover = gtk::Popover::builder()
            .has_arrow(false)
            // Sin autohide y sin foco: un popover con autohide toma el teclado al abrirse y las
            // teclas dejarían de llegar a la nota. Las flechas, Intro, Tab y Escape las maneja el
            // controlador de teclas de la vista de código.
            .autohide(false)
            .can_focus(false)
            .position(gtk::PositionType::Bottom)
            .css_classes(["orbital-completion-popover"])
            .child(&scroller)
            .build();
        popover.update_property(&[gtk::accessible::Property::Label(
            "Autocompletado de enlaces",
        )]);
        popover.set_parent(parent);

        let this = Self(Rc::new(Inner {
            popover,
            list,
            candidates: RefCell::new(Vec::new()),
            filtered: RefCell::new(Vec::new()),
            query: RefCell::new(String::new()),
            on_selected: RefCell::new(None),
        }));

        // Sin autohide, se cierra cuando la nota pierde el foco.
        let focus = gtk::EventControllerFocus::new();
        let weak = Rc::downgrade(&this.0);
        focus.connect_leave(move |_| {
            if let Some(i) = weak.upgrade() {
                i.popover.popdown();
            }
        });
        parent.add_controller(focus);

        let weak = Rc::downgrade(&this.0);
        this.0.list.connect_row_activated(move |_, row| {
            let Some(i) = weak.upgrade() else { return };
            let idx = row.index();
            if idx >= 0 {
                let candidate = i.filtered.borrow().get(idx as usize).cloned();
                if let Some(c) = candidate {
                    i.popover.popdown();
                    let q = i.query.borrow().clone();
                    let target = c.link_target(&q);
                    if let Some(f) = i.on_selected.borrow().as_ref() {
                        f(c, target);
                    }
                }
            }
        });

        this
    }

    pub fn set_candidates(&self, candidates: Vec<CompletionCandidate>) {
        *self.0.candidates.borrow_mut() = candidates;
    }

    pub fn connect_selected(&self, f: impl Fn(CompletionCandidate, String) + 'static) {
        *self.0.on_selected.borrow_mut() = Some(Box::new(f));
    }

    pub fn popup_at(&self, rect: &gdk::Rectangle) {
        self.0.popover.set_pointing_to(Some(rect));
        self.0.popover.popup();
    }

    pub fn popdown(&self) {
        self.0.popover.popdown();
    }

    pub fn is_visible(&self) -> bool {
        self.0.popover.is_visible()
    }

    /// Filtra candidatos según la consulta y actualiza la lista.
    /// Retorna la cantidad de coincidencias encontradas.
    pub fn filter(&self, query: &str) -> usize {
        *self.0.query.borrow_mut() = query.to_string();
        let candidates = self.0.candidates.borrow();
        let matches = filter_candidates(&candidates, query, 8);

        // Limpiar lista
        while let Some(row) = self.0.list.first_child() {
            self.0.list.remove(&row);
        }

        for c in &matches {
            let row_box = gtk::Box::new(gtk::Orientation::Vertical, 2);
            row_box.set_margin_top(6);
            row_box.set_margin_bottom(6);
            row_box.set_margin_start(10);
            row_box.set_margin_end(10);

            let title_text = c.title.as_deref().unwrap_or(&c.stem);
            let title_lbl = gtk::Label::builder()
                .label(title_text)
                .xalign(0.0)
                .css_classes(["heading"])
                .build();
            row_box.append(&title_lbl);

            let mut subtitle_parts = Vec::new();
            if c.title.is_some() && c.title.as_deref() != Some(&c.stem) {
                subtitle_parts.push(c.stem.clone());
            }
            if !c.aliases.is_empty() {
                subtitle_parts.push(format!("alias: {}", c.aliases.join(", ")));
            }

            if !subtitle_parts.is_empty() {
                let sub_lbl = gtk::Label::builder()
                    .label(subtitle_parts.join(" · "))
                    .xalign(0.0)
                    .css_classes(["dim-label"])
                    .build();
                row_box.append(&sub_lbl);
            }

            let row_btn = gtk::Button::builder()
                .has_frame(false)
                .focusable(false)
                .css_classes(["flat"])
                .child(&row_box)
                .build();
            row_btn.update_property(&[gtk::accessible::Property::Label(&format!(
                "Sugerencia: {}",
                c.stem
            ))]);

            let weak_btn = Rc::downgrade(&self.0);
            let cand_clone = c.clone();
            row_btn.connect_clicked(move |_| {
                let Some(i) = weak_btn.upgrade() else { return };
                i.popover.popdown();
                let q = i.query.borrow().clone();
                let target = cand_clone.link_target(&q);
                if let Some(f) = i.on_selected.borrow().as_ref() {
                    f(cand_clone.clone(), target);
                }
            });

            let row = gtk::ListBoxRow::builder()
                .child(&row_btn)
                .focusable(false)
                .build();
            self.0.list.append(&row);
        }

        let count = matches.len();
        *self.0.filtered.borrow_mut() = matches;

        if count > 0
            && let Some(first) = self.0.list.row_at_index(0)
        {
            self.0.list.select_row(Some(&first));
        }

        count
    }

    pub fn select_next(&self) {
        let count = self.0.filtered.borrow().len();
        if count == 0 {
            return;
        }
        let cur = self
            .0
            .list
            .selected_row()
            .map_or(0, |r| (r.index() + 1) as usize % count);
        if let Some(row) = self.0.list.row_at_index(cur as i32) {
            self.0.list.select_row(Some(&row));
        }
    }

    pub fn select_prev(&self) {
        let count = self.0.filtered.borrow().len();
        if count == 0 {
            return;
        }
        let cur = self.0.list.selected_row().map_or(0, |r| {
            if r.index() <= 0 {
                count - 1
            } else {
                (r.index() - 1) as usize
            }
        });
        if let Some(row) = self.0.list.row_at_index(cur as i32) {
            self.0.list.select_row(Some(&row));
        }
    }

    pub fn activate_selected(&self) -> bool {
        if let Some(row) = self.0.list.selected_row() {
            let idx = row.index();
            if idx >= 0 {
                let candidate = self.0.filtered.borrow().get(idx as usize).cloned();
                if let Some(c) = candidate {
                    self.popdown();
                    let q = self.0.query.borrow().clone();
                    let target = c.link_target(&q);
                    if let Some(f) = self.0.on_selected.borrow().as_ref() {
                        f(c, target);
                    }
                    return true;
                }
            }
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filter_candidates_matches_name_title_and_alias() {
        let candidates = vec![
            CompletionCandidate {
                stem: "nota-01".into(),
                title: Some("Biología Marina".into()),
                aliases: vec!["Bio".into(), "Océanos".into()],
                path: RelPath::new("wiki/nota-01.md").unwrap(),
            },
            CompletionCandidate {
                stem: "nota-02".into(),
                title: Some("Física Cuántica".into()),
                aliases: vec!["Cuántica".into()],
                path: RelPath::new("wiki/nota-02.md").unwrap(),
            },
            CompletionCandidate {
                stem: "apuntes".into(),
                title: None,
                aliases: Vec::new(),
                path: RelPath::new("wiki/apuntes.md").unwrap(),
            },
        ];

        // Coincidencia por nombre de archivo / stem
        let res = filter_candidates(&candidates, "apuntes", 8);
        assert_eq!(res.len(), 1);
        assert_eq!(res[0].stem, "apuntes");

        // Coincidencia por título
        let res = filter_candidates(&candidates, "marina", 8);
        assert_eq!(res.len(), 1);
        assert_eq!(res[0].stem, "nota-01");

        // Coincidencia por alias
        let res = filter_candidates(&candidates, "cuántica", 8);
        assert_eq!(res.len(), 1);
        assert_eq!(res[0].stem, "nota-02");

        // Coincidencia vacía devuelve todos hasta el límite
        let res = filter_candidates(&candidates, "", 2);
        assert_eq!(res.len(), 2);

        // link_target con alias y sin alias
        assert_eq!(candidates[0].link_target("bio"), "nota-01|Bio");
        assert_eq!(candidates[2].link_target("apuntes"), "apuntes");
    }
}
