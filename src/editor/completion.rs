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
    /// Lo que se escribe dentro de `[[…]]`: el nombre, o `carpeta/nombre` si otra nota tiene el
    /// mismo nombre (si no, el enlace quedaría ambiguo).
    pub target: String,
    /// Nombre, título y alias en minúsculas, calculados una vez (no en cada tecla).
    keys: Keys,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Keys {
    stem: String,
    title: Option<String>,
    aliases: Vec<String>,
}

impl CompletionCandidate {
    pub fn new(path: RelPath, title: Option<String>, aliases: Vec<String>) -> Self {
        let name = path.file_name();
        let stem = name.rsplit_once('.').map_or(name, |(s, _)| s).to_owned();
        let keys = Keys {
            stem: orbital_vault::fold(&stem),
            title: title.as_deref().map(orbital_vault::fold),
            aliases: aliases.iter().map(|a| orbital_vault::fold(a)).collect(),
        };
        Self {
            target: stem.clone(),
            stem,
            title,
            aliases,
            path,
            keys,
        }
    }

    /// Retorna el alias que coincide con la consulta, si lo hay.
    pub fn matching_alias(&self, query: &str) -> Option<&str> {
        let q = orbital_vault::fold(query.trim());
        if q.is_empty() {
            return None;
        }
        self.keys
            .aliases
            .iter()
            .position(|a| a.contains(&q))
            .map(|n| self.aliases[n].as_str())
    }

    /// Retorna el texto del destino wikilink a insertar según la consulta.
    /// Si coincide con un alias, devuelve `destino|alias`; si tiene alias y no se buscó nada,
    /// devuelve `destino|primer_alias`; en caso contrario `destino`.
    pub fn link_target(&self, query: &str) -> String {
        if let Some(alias) = self.matching_alias(query) {
            format!("{}|{}", self.target, alias)
        } else if query.trim().is_empty() && !self.aliases.is_empty() {
            format!("{}|{}", self.target, self.aliases[0])
        } else {
            self.target.clone()
        }
    }
}

/// Los candidatos cuyo nombre se repite (sin distinguir mayúsculas) se escriben con su carpeta
/// dentro de `wiki/` (`[[fisica/energia]]`), como resuelve la bóveda.
pub fn qualify_duplicates(list: &mut [CompletionCandidate]) {
    let mut count: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();
    for c in list.iter() {
        *count.entry(c.keys.stem.as_str()).or_default() += 1;
    }
    let dup: std::collections::HashSet<String> = count
        .into_iter()
        .filter(|(_, n)| *n > 1)
        .map(|(k, _)| k.to_owned())
        .collect();
    for c in list.iter_mut() {
        if dup.contains(&c.keys.stem) {
            let in_wiki = c
                .path
                .as_str()
                .strip_prefix("wiki/")
                .unwrap_or(c.path.as_str());
            c.target = match in_wiki.rsplit_once('/') {
                Some((dir, _)) => format!("{dir}/{}", c.stem),
                None => c.stem.clone(),
            };
        }
    }
}

#[derive(Clone)]
struct Suggestion {
    label: String,
    detail: String,
    target: String,
    icon: Option<(String, Option<std::path::PathBuf>)>,
}

/// Nombres y claves plegadas preparados al cambiar el índice, nunca al escribir.
#[derive(Clone, Debug)]
pub struct TagCandidate {
    name: String,
    key: String,
    inline: bool,
}

impl TagCandidate {
    pub fn new(name: String) -> Self {
        let key = orbital_vault::fold(&name);
        // Propiedades pueden tener nombres que no se pueden escribir como #etiqueta.
        let inline = name
            .chars()
            .all(|c| c.is_alphanumeric() || "_-/".contains(c))
            && !name.ends_with('/')
            && name.chars().any(|c| !c.is_ascii_digit())
            && !(matches!(name.len(), 3 | 6 | 8) && name.bytes().all(|b| b.is_ascii_hexdigit()));
        Self { name, key, inline }
    }
}

pub fn filter_tags(candidates: &[TagCandidate], query: &str, limit: usize) -> Vec<String> {
    let query = orbital_vault::fold(query);
    candidates
        .iter()
        .filter(|c| c.inline && c.key.starts_with(&query))
        .take(limit)
        .map(|c| c.name.clone())
        .collect()
}

/// Clave de marcador `[%clave]` para autocompletar (US5 de la 005).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MarkerCandidate {
    name: String,
    key: String,
}

impl MarkerCandidate {
    pub fn new(name: String) -> Self {
        let key = orbital_vault::fold(&name);
        Self { name, key }
    }
}

/// Filtra claves de marcadores por prefijo, sin distinguir mayúsculas ni tildes.
pub fn filter_markers(candidates: &[MarkerCandidate], query: &str, limit: usize) -> Vec<String> {
    let query = orbital_vault::fold(query);
    candidates
        .iter()
        .filter(|c| c.key.starts_with(&query))
        .take(limit)
        .map(|c| c.name.clone())
        .collect()
}

/// Claves disponibles para autocompletar marcadores en la nota abierta: los campos inline
/// `clave:: valor` de toda la nota (son los que ganan al resolver) y las claves del frontmatter
/// (YAML Markdown o `<script>` de HTML). Una entrada por clave; inline primero.
/// Los bloques cuyo id empieza con `query` o cuyo texto la contiene, en orden de la nota.
pub fn filter_blocks<'a>(
    bloques: &'a [orbital_vault::bloques::BloqueConId],
    query: &str,
    limit: usize,
) -> Vec<&'a orbital_vault::bloques::BloqueConId> {
    let q = orbital_vault::fold(query.trim());
    bloques
        .iter()
        .filter(|b| {
            q.is_empty() || b.id.starts_with(&q) || orbital_vault::fold(&b.resumen).contains(&q)
        })
        .take(limit)
        .collect()
}

pub fn marker_candidates(text: &str, is_html: bool) -> Vec<MarkerCandidate> {
    let mut names: Vec<String> = Vec::new();
    let mut push = |key: &str| {
        if !key.is_empty() && !names.iter().any(|n| n == key) {
            names.push(key.to_owned());
        }
    };
    let inline = if is_html {
        orbital_vault::html_text::html_text(text)
    } else {
        text.to_owned()
    };
    for (key, _) in orbital_vault::tags::inline_fields(&inline) {
        push(&key);
    }
    let yaml = if is_html {
        orbital_vault::props::html::scan(text)
            .block
            .and_then(|region| orbital_vault::props::yaml::parse_block(&text[region]).ok())
    } else {
        orbital_vault::props::markdown::locate(text)
            .and_then(|region| orbital_vault::props::yaml::parse_block(&text[region]).ok())
    };
    for entry in yaml.unwrap_or_default() {
        push(&entry.key);
    }
    names.into_iter().map(MarkerCandidate::new).collect()
}

type SelectedFn = Box<dyn Fn(String)>;

struct Inner {
    popover: gtk::Popover,
    list: gtk::ListBox,
    candidates: RefCell<Vec<CompletionCandidate>>,
    filtered: RefCell<Vec<Suggestion>>,
    tags: RefCell<Vec<TagCandidate>>,
    markers: RefCell<Vec<MarkerCandidate>>,
    /// Tipos de relación (049): la lista base y los usados en la bóveda.
    relation_types: RefCell<Vec<String>>,
    /// SVGs locales en `.orbital/iconos/` para autocompletado `:nombre:` (062).
    local_icons: RefCell<Vec<orbital_vault::SvgLocal>>,
    on_selected: RefCell<Option<SelectedFn>>,
}

#[derive(Clone)]
pub struct LinkCompletionPopover(Rc<Inner>);

/// Filtra candidatos según la consulta (coincidencia insensible a mayúsculas en nombre, título o
/// alias): primero el nombre exacto, después los que empiezan con la consulta y por último los que
/// la contienen. Recorre por índices y copia solo los `limit` que se muestran (FR-009).
pub fn filter_candidates(
    candidates: &[CompletionCandidate],
    query: &str,
    limit: usize,
) -> Vec<CompletionCandidate> {
    let q = orbital_vault::fold(query.trim());
    if q.is_empty() {
        return candidates.iter().take(limit).cloned().collect();
    }
    let mut exact = Vec::new();
    let mut starts = Vec::new();
    let mut contains = Vec::new();
    for (n, c) in candidates.iter().enumerate() {
        let k = &c.keys;
        let title = k.title.as_deref();
        if k.stem == q {
            if exact.len() < limit {
                exact.push(n);
            }
        } else if k.stem.starts_with(&q) || title.is_some_and(|t| t.starts_with(&q)) {
            if starts.len() < limit {
                starts.push(n);
            }
        } else if contains.len() < limit
            && (k.stem.contains(&q)
                || title.is_some_and(|t| t.contains(&q))
                || k.aliases.iter().any(|a| a.contains(&q)))
        {
            contains.push(n);
        }
    }
    exact
        .into_iter()
        .chain(starts)
        .chain(contains)
        .take(limit)
        .map(|n| candidates[n].clone())
        .collect()
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
            tags: RefCell::new(Vec::new()),
            markers: RefCell::new(Vec::new()),
            relation_types: RefCell::new(base_relation_types(&[])),
            local_icons: RefCell::new(Vec::new()),
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
                    let target = c.target;
                    if let Some(f) = i.on_selected.borrow().as_ref() {
                        f(target);
                    }
                }
            }
        });

        this
    }

    pub fn set_candidates(&self, candidates: Vec<CompletionCandidate>) {
        *self.0.candidates.borrow_mut() = candidates;
    }

    pub fn connect_selected(&self, f: impl Fn(String) + 'static) {
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
        let candidates = self.0.candidates.borrow();
        let matches = filter_candidates(&candidates, query, 8);

        let suggestions = matches
            .into_iter()
            .map(|c| {
                let target = c.link_target(query);
                let label = c.title.clone().unwrap_or_else(|| c.stem.clone());
                let detail = if c.aliases.is_empty() {
                    c.stem
                } else {
                    format!("{} · alias: {}", c.stem, c.aliases.join(", "))
                };
                Suggestion {
                    label,
                    detail,
                    target,
                    icon: None,
                }
            })
            .collect();
        drop(candidates);
        self.show_suggestions(suggestions, "enlaces")
    }

    pub fn set_tags(&self, tags: Vec<TagCandidate>) {
        *self.0.tags.borrow_mut() = tags;
    }

    pub fn filter_tag_query(&self, query: &str) -> usize {
        let suggestions = filter_tags(&self.0.tags.borrow(), query, 8)
            .into_iter()
            .map(|name| Suggestion {
                label: format!("#{name}"),
                detail: String::new(),
                target: name,
                icon: None,
            })
            .collect();
        self.show_suggestions(suggestions, "etiquetas")
    }

    pub fn set_markers(&self, markers: Vec<MarkerCandidate>) {
        *self.0.markers.borrow_mut() = markers;
    }

    pub fn filter_marker_query(&self, query: &str) -> usize {
        let suggestions = filter_markers(&self.0.markers.borrow(), query, 8)
            .into_iter()
            .map(|key| Suggestion {
                label: format!("[%{key}]"),
                detail: String::new(),
                target: key,
                icon: None,
            })
            .collect();
        self.show_suggestions(suggestions, "marcadores")
    }

    /// Los tipos usados en la bóveda (049), después de la lista base.
    pub fn set_relation_types(&self, usados: &[String]) {
        *self.0.relation_types.borrow_mut() = base_relation_types(usados);
    }

    pub fn filter_relation_query(&self, query: &str) -> usize {
        let tipos = self.0.relation_types.borrow();
        let suggestions = orbital_vault::relaciones::filtrar_tipos(&tipos, query, 8)
            .into_iter()
            .map(|t| {
                let inversa = orbital_vault::relaciones::nombre_inverso(t);
                Suggestion {
                    label: t.to_owned(),
                    detail: if inversa.starts_with('←') {
                        "tipo usado en la bóveda".to_owned()
                    } else {
                        format!("en la otra nota: «{inversa}»")
                    },
                    target: t.to_owned(),
                    icon: None,
                }
            })
            .collect();
        drop(tipos);
        self.show_suggestions(suggestions, "tipos de relación")
    }

    /// Los bloques de `nota` cuyo id o texto contienen `query` (026).
    pub fn filter_block_query(
        &self,
        nota: &str,
        bloques: &[orbital_vault::bloques::BloqueConId],
        query: &str,
    ) -> usize {
        let suggestions = filter_blocks(bloques, query, 8)
            .into_iter()
            .map(|b| Suggestion {
                label: if b.resumen.is_empty() {
                    format!("^{}", b.id)
                } else {
                    b.resumen.clone()
                },
                detail: format!("^{} · línea {}", b.id, b.linea + 1),
                target: format!("{nota}#^{}", b.id),
                icon: None,
            })
            .collect();
        self.show_suggestions(suggestions, "bloques")
    }

    /// Registra los SVGs locales de `.orbital/iconos/` para autocompletar `:nombre:` (062).
    pub fn set_local_icons(&self, svgs: Vec<orbital_vault::SvgLocal>) {
        *self.0.local_icons.borrow_mut() = svgs;
    }

    /// Filtra íconos locales y del catálogo por nombre, etiqueta o palabras clave (062).
    pub fn filter_icon_query(&self, query: &str) -> usize {
        let q = orbital_vault::fold(query.trim());
        let mut starts = Vec::new();
        let mut contains = Vec::new();
        for sv in self.0.local_icons.borrow().iter() {
            let nf = orbital_vault::fold(&sv.nombre);
            let sug = || Suggestion {
                label: format!(":{}:", sv.nombre),
                detail: "SVG local".to_owned(),
                target: sv.nombre.clone(),
                icon: Some((sv.nombre.clone(), Some(sv.ruta.clone()))),
            };
            if nf.starts_with(&q) {
                starts.push(sug());
            } else if nf.contains(&q) {
                contains.push(sug());
            }
        }
        for ic in orbital_vault::iconos_catalogo::CATALOGO {
            let nf = orbital_vault::fold(ic.nombre);
            let ef = orbital_vault::fold(ic.etiqueta);
            let cf = orbital_vault::fold(ic.claves);
            let sug = || Suggestion {
                label: format!(":{}:", ic.nombre),
                detail: format!("{} · {}", ic.etiqueta, ic.categoria),
                target: ic.nombre.to_owned(),
                icon: Some((ic.nombre.to_owned(), None)),
            };
            if nf.starts_with(&q) || ef.starts_with(&q) {
                starts.push(sug());
            } else if nf.contains(&q) || ef.contains(&q) || cf.contains(&q) {
                contains.push(sug());
            }
        }
        let matches: Vec<Suggestion> = starts.into_iter().chain(contains).take(8).collect();
        self.show_suggestions(matches, "íconos")
    }

    fn show_suggestions(&self, matches: Vec<Suggestion>, kind: &str) -> usize {
        self.0
            .list
            .update_property(&[gtk::accessible::Property::Label(&format!(
                "Sugerencias de {kind}"
            ))]);
        self.0
            .popover
            .update_property(&[gtk::accessible::Property::Label(&format!(
                "Autocompletado de {kind}"
            ))]);
        while let Some(row) = self.0.list.first_child() {
            self.0.list.remove(&row);
        }
        for c in &matches {
            let outer = gtk::Box::new(gtk::Orientation::Horizontal, 8);
            outer.set_margin_top(6);
            outer.set_margin_bottom(6);
            outer.set_margin_start(10);
            outer.set_margin_end(10);
            if let Some((nombre, svg)) = &c.icon
                && let Some(img) =
                    crate::iconos_personalizados::imagen(nombre, None, 16, svg.as_deref())
            {
                outer.append(&img);
            }
            let row_box = gtk::Box::new(gtk::Orientation::Vertical, 2);
            row_box.append(
                &gtk::Label::builder()
                    .label(&c.label)
                    .xalign(0.0)
                    .css_classes(["heading"])
                    .build(),
            );
            if !c.detail.is_empty() {
                row_box.append(
                    &gtk::Label::builder()
                        .label(&c.detail)
                        .xalign(0.0)
                        .css_classes(["dim-label"])
                        .build(),
                );
            }
            outer.append(&row_box);
            let row_btn = gtk::Button::builder()
                .has_frame(false)
                .focusable(false)
                .css_classes(["flat"])
                .child(&outer)
                .build();
            row_btn.update_property(&[gtk::accessible::Property::Label(&format!(
                "Sugerencia: {}",
                c.label
            ))]);
            let weak = Rc::downgrade(&self.0);
            let target = c.target.clone();
            row_btn.connect_clicked(move |_| {
                let Some(i) = weak.upgrade() else { return };
                i.popover.popdown();
                if let Some(f) = i.on_selected.borrow().as_ref() {
                    f(target.clone());
                }
            });
            self.0.list.append(
                &gtk::ListBoxRow::builder()
                    .child(&row_btn)
                    .focusable(false)
                    .build(),
            );
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
                    let target = c.target;
                    if let Some(f) = self.0.on_selected.borrow().as_ref() {
                        f(target);
                    }
                    return true;
                }
            }
        }
        false
    }
}

/// Los tipos base (049) y, después, los usados en la bóveda que no son base.
pub fn base_relation_types(usados: &[String]) -> Vec<String> {
    let mut v: Vec<String> = orbital_vault::relaciones::TIPOS_BASE
        .iter()
        .map(|t| t.nombre.to_owned())
        .collect();
    v.extend(usados.iter().cloned());
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tag_suggestions_fold_prefixes_and_limit_results() {
        let tags: Vec<_> = [
            "Física",
            "Física/Termodinámica",
            "Química",
            "otra/física",
            "con espacios",
            "123",
            "ff00ff",
        ]
        .into_iter()
        .map(|name| TagCandidate::new(name.into()))
        .collect();
        assert_eq!(
            filter_tags(&tags, "fis", 8),
            ["Física", "Física/Termodinámica"]
        );
        assert_eq!(filter_tags(&tags, "FÍSICA/t", 8), ["Física/Termodinámica"]);
        assert_eq!(filter_tags(&tags, "", 1), ["Física"]);
        assert!(filter_tags(&tags, "ausente", 8).is_empty());
        assert_eq!(filter_tags(&tags, "", 8).len(), 4);
    }

    #[test]
    fn bloques_por_id_o_texto() {
        let texto = "La entropía mide. ^k3x9a1\n\n- Primer principio ^p2m8q4\n";
        let b = orbital_vault::bloques::listar(texto, false);
        let ids = |q: &str| -> Vec<String> {
            filter_blocks(&b, q, 8)
                .iter()
                .map(|x| x.id.clone())
                .collect()
        };
        assert_eq!(ids(""), ["k3x9a1", "p2m8q4"]);
        assert_eq!(ids("p2"), ["p2m8q4"]);
        assert_eq!(ids("ENTROPIA"), ["k3x9a1"]);
        assert!(ids("nada").is_empty());
    }

    #[test]
    fn marker_keys_from_inline_and_yaml_dedup_and_fold() {
        // Inline primero (es lo que gana al resolver); YAML después; una entrada por clave.
        let text = "---\nprioridad: alta\nrepetida: yaml\n---\n\n# Nota\n\nestado:: activo\n- repetida:: inline\n";
        let keys = marker_candidates(text, false);
        let names: Vec<_> = keys.iter().map(|c| c.name.clone()).collect();
        assert_eq!(names, ["estado", "repetida", "prioridad"]);

        // Prefijo sin mayúsculas ni tildes; límite.
        assert_eq!(filter_markers(&keys, "PRI", 8), ["prioridad"]);
        assert_eq!(filter_markers(&keys, "est", 8), ["estado"]);
        assert!(filter_markers(&keys, "ausente", 8).is_empty());
        assert_eq!(filter_markers(&keys, "", 2).len(), 2);
        assert_eq!(filter_markers(&keys, "", 8).len(), 3);
    }

    #[test]
    fn marker_keys_html_take_visible_text_and_script_yaml() {
        let text = "<head><script type=\"application/yaml\">prioridad: alta\n</script></head>\n<body><p>estado:: activo</p><pre>fence:: no</pre></body>";
        let keys = marker_candidates(text, true);
        let names: Vec<_> = keys.iter().map(|c| c.name.clone()).collect();
        assert_eq!(names, ["estado", "prioridad"]);
    }

    /// 049: dentro de la parte Conexiones, el tipo que se escribe al principio de un ítem se
    /// completa con la lista base y los tipos de la bóveda; afuera (u otra parte) no se sugiere.
    #[test]
    #[ignore = "requiere acceso GTK; tools/pruebas-gtk.sh"]
    fn tipos_de_relacion_se_completan_en_conexiones() {
        use sourceview5::prelude::*;
        gtk::init().expect("el proceso necesita conectarse a una sesión GTK");
        let (view, buffer) = super::super::code::new_view();
        // La vista va en una ventana, como en el editor: un popover hijo de una vista suelta
        // deja a GTK repitiendo un aviso sin fin al destruirla.
        let window = gtk::Window::builder().child(&view).build();
        window.present();
        let popover = LinkCompletionPopover::new(&view);
        popover.set_relation_types(&["refuta".to_owned(), "amplía".to_owned()]);
        let cabeza = "---\nid: AP-001\n---\n<!-- block 1 AP-001 -->\n- contra\n<!---->\n<!-- connections AP-001 -->\n- [[Cálculo]]\n";
        let q = super::super::code::relation_query_at_cursor;
        // En el cuerpo (un bloque), no.
        buffer.set_text(cabeza);
        let mut it = buffer.start_iter();
        it.forward_lines(4);
        it.forward_to_line_end();
        buffer.place_cursor(&it);
        assert!(q(&buffer).is_none());
        // En Conexiones, sí: «contra» → «contradice a», una sola vez aunque esté en la bóveda.
        let texto = format!("{cabeza}- contra");
        buffer.set_text(&texto);
        buffer.place_cursor(&buffer.end_iter());
        let (query, start) = q(&buffer).unwrap();
        assert_eq!(query, "contra");
        assert_eq!(start, cabeza.chars().count() as i32 + 2);
        assert_eq!(popover.filter_relation_query(&query), 1);
        assert_eq!(popover.filter_relation_query("ref"), 1);
        assert_eq!(popover.filter_relation_query("amp"), 1);
        let for_insert = buffer.clone();
        popover.connect_selected(move |t| {
            super::super::code::insert_relation_completion(&for_insert, start, &t)
        });
        popover.filter_relation_query(&query);
        assert!(popover.activate_selected());
        let escrito = buffer.text(&buffer.start_iter(), &buffer.end_iter(), true);
        assert_eq!(escrito, format!("{cabeza}- contradice a:: "));
        buffer.undo();
        assert_eq!(
            buffer.text(&buffer.start_iter(), &buffer.end_iter(), true),
            texto
        );
        // Corregir un tipo ya escrito reemplaza hasta su `::`.
        let texto = format!("{cabeza}- amp:: [[X]]");
        buffer.set_text(&texto);
        let mut it = buffer.end_iter();
        it.set_line_offset(5);
        buffer.place_cursor(&it);
        let (query, start) = q(&buffer).unwrap();
        assert_eq!(query, "amp");
        super::super::code::insert_relation_completion(&buffer, start, "amplía");
        assert_eq!(
            buffer.text(&buffer.start_iter(), &buffer.end_iter(), true),
            format!("{cabeza}- amplía:: [[X]]")
        );
        // Después del cierre de la parte, no.
        buffer.set_text(&format!("{cabeza}<!---->\n- amp"));
        buffer.place_cursor(&buffer.end_iter());
        assert!(q(&buffer).is_none());
    }

    #[test]
    #[ignore = "requiere acceso GTK; tools/verificar-005-us5.sh"]
    fn us5_marker_widgets_completion() {
        gtk::init().expect("el proceso necesita conectarse a una sesión GTK");
        let (view, buffer) = super::super::code::new_view();
        let window = gtk::Window::builder().child(&view).build();
        window.present();
        let context = gtk::glib::MainContext::default();
        context.block_on(gtk::glib::timeout_future(std::time::Duration::from_millis(
            50,
        )));
        view.grab_focus();
        let popover = LinkCompletionPopover::new(&view);
        let nota = "---\nprioridad: alta\nrepetida: yaml\n---\n\n# Nota\n\nestado:: activo\n- repetida:: inline\n";
        popover.set_markers(marker_candidates(nota, false));
        let texto = format!("{nota}😀 [%est");
        buffer.set_text(&texto);
        buffer.place_cursor(&buffer.end_iter());
        let (query, start) = super::super::code::marker_query_at_cursor(&buffer, false).unwrap();
        assert_eq!(query, "est");
        assert_eq!(start, nota.chars().count() as i32 + 2);
        let for_insert = buffer.clone();
        popover.connect_selected(move |target| {
            super::super::code::insert_marker_completion(&for_insert, start, &target)
        });
        // Solo «estado» coincide con «est»; las claves del YAML las cubre el test unitario
        // de filter_markers (refiltrar acá cambiaría la fila elegida).
        assert_eq!(popover.filter_marker_query(&query), 1);
        popover.popup_at(&gtk::gdk::Rectangle::new(0, 0, 1, 18));
        context.block_on(gtk::glib::timeout_future(std::time::Duration::from_millis(
            50,
        )));
        assert!(!popover.0.popover.is_autohide());
        assert!(!popover.0.popover.can_focus());
        // El foco dentro de la ventana no depende del compositor (que puede no activar una
        // ventana de prueba); el popover no enfocable no tiene que cambiarlo.
        assert!(
            view.is_focus(),
            "abrir sugerencias conserva foco en la nota"
        );
        if window.is_active() {
            assert!(view.has_focus());
        }
        popover.select_next();
        popover.select_prev();
        popover.select_next();
        assert!(popover.activate_selected());
        assert_eq!(
            buffer.text(&buffer.start_iter(), &buffer.end_iter(), true),
            format!("{nota}😀 [%estado]")
        );
        use sourceview5::prelude::*;
        buffer.undo();
        assert_eq!(
            buffer.text(&buffer.start_iter(), &buffer.end_iter(), true),
            texto
        );
        popover.0.popover.unparent();
        window.destroy();
    }

    #[test]
    #[ignore = "requiere acceso GTK; tools/verificar-004-us5.sh"]
    fn us5_tag_widgets_completion() {
        gtk::init().expect("el proceso necesita conectarse a una sesión GTK");
        let (view, buffer) = super::super::code::new_view();
        let window = gtk::Window::builder().child(&view).build();
        window.present();
        let context = gtk::glib::MainContext::default();
        context.block_on(gtk::glib::timeout_future(std::time::Duration::from_millis(
            50,
        )));
        view.grab_focus();
        let popover = LinkCompletionPopover::new(&view);
        popover.set_tags(
            ["Física", "Física/Termodinámica"]
                .into_iter()
                .map(|s| TagCandidate::new(s.into()))
                .collect(),
        );
        buffer.set_text("😀 #fis");
        buffer.place_cursor(&buffer.end_iter());
        let (query, start) = super::super::code::tag_query_at_cursor(&buffer, false).unwrap();
        assert_eq!(query, "fis");
        assert_eq!(start, 2);
        let for_insert = buffer.clone();
        popover.connect_selected(move |target| {
            super::super::code::insert_tag_completion(&for_insert, start, &target)
        });
        assert_eq!(popover.filter_tag_query(&query), 2);
        popover.popup_at(&gtk::gdk::Rectangle::new(0, 0, 1, 18));
        context.block_on(gtk::glib::timeout_future(std::time::Duration::from_millis(
            50,
        )));
        assert!(!popover.0.popover.is_autohide());
        assert!(!popover.0.popover.can_focus());
        // El foco dentro de la ventana no depende del compositor (que puede no activar una
        // ventana de prueba); el popover no enfocable no tiene que cambiarlo.
        assert!(
            view.is_focus(),
            "abrir sugerencias conserva foco en la nota"
        );
        if window.is_active() {
            assert!(view.has_focus());
        }
        popover.select_next();
        popover.select_prev();
        popover.select_next();
        assert!(popover.activate_selected());
        assert_eq!(
            buffer.text(&buffer.start_iter(), &buffer.end_iter(), true),
            "😀 #Física/Termodinámica"
        );
        use sourceview5::prelude::*;
        buffer.undo();
        assert_eq!(
            buffer.text(&buffer.start_iter(), &buffer.end_iter(), true),
            "😀 #fis"
        );
        popover.0.popover.unparent();
        window.destroy();
    }

    #[test]
    fn filter_candidates_matches_name_title_and_alias() {
        let rel = |p: &str| RelPath::new(p).unwrap();
        let mut candidates = vec![
            CompletionCandidate::new(
                rel("wiki/nota-01.md"),
                Some("Biología Marina".into()),
                vec!["Bio".into(), "Océanos".into()],
            ),
            CompletionCandidate::new(
                rel("wiki/nota-02.md"),
                Some("Física Cuántica".into()),
                vec!["Cuántica".into()],
            ),
            CompletionCandidate::new(rel("wiki/apuntes.md"), None, Vec::new()),
        ];
        qualify_duplicates(&mut candidates);

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

    #[test]
    fn duplicate_names_get_their_folder() {
        let rel = |p: &str| RelPath::new(p).unwrap();
        let mut list = vec![
            CompletionCandidate::new(rel("wiki/fisica/energia.md"), None, Vec::new()),
            CompletionCandidate::new(rel("wiki/quimica/Energia.html"), None, Vec::new()),
            CompletionCandidate::new(rel("wiki/otra.md"), None, vec!["Alias".into()]),
        ];
        qualify_duplicates(&mut list);
        assert_eq!(list[0].link_target("ener"), "fisica/energia");
        assert_eq!(list[1].link_target("ener"), "quimica/Energia");
        assert_eq!(list[2].link_target("alias"), "otra|Alias");
    }

    /// FR-009: filtrar 5000 notas en menos de 10 ms. Medir con
    /// `cargo test --release --bin orbital filter_latency -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn filter_latency() {
        let list: Vec<CompletionCandidate> = (0..5000)
            .map(|n| {
                CompletionCandidate::new(
                    RelPath::new(&format!("wiki/carpeta-{}/nota-{n:05}.md", n % 50)).unwrap(),
                    Some(format!(
                        "Título de la nota número {n} sobre física y química"
                    )),
                    vec![format!("alias-{n}"), "Energía".into()],
                )
            })
            .collect();
        let mut worst = std::time::Duration::ZERO;
        let mut times = Vec::new();
        for q in [
            "n", "no", "not", "nota-0", "nota-04", "física", "zzz", "energía", "alias-49",
        ] {
            let t = std::time::Instant::now();
            let r = filter_candidates(&list, q, 8);
            let e = t.elapsed();
            assert!(r.len() <= 8);
            worst = worst.max(e);
            times.push((q, e));
        }
        println!("filtrado de 5000 notas: {times:?}");
        println!("peor: {worst:?}");
        assert!(worst < std::time::Duration::from_millis(10));
    }
}
