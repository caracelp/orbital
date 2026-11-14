//! Panel de búsqueda en toda la bóveda (US1 de la 004, research R9).

mod saved;

use std::cell::{Cell, RefCell};
use std::ops::Range;
use std::rc::Rc;
use std::sync::{Arc, RwLock};

use adw::prelude::*;
use gtk::glib::subclass::prelude::ObjectSubclassIsExt;
use gtk::{gio, glib};
use orbital_vault::entry::{RelPath, Zone};
use orbital_vault::search::{SearchIndex, SearchOptions, SearchResults};
use orbital_vault::vault::Vault;

#[derive(Clone, Debug)]
pub enum SearchRowData {
    NoteHeader {
        path: RelPath,
        name: String,
        folder: String,
        match_count: usize,
        is_raw: bool,
    },
    Match {
        path: RelPath,
        line: usize,
        snippet: String,
        markup: String,
    },
}

mod imp {
    use super::*;
    use glib::subclass::prelude::*;

    #[derive(Default)]
    pub struct SearchRowItem {
        pub data: RefCell<Option<SearchRowData>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for SearchRowItem {
        const NAME: &'static str = "OrbitalSearchRowItem";
        type Type = super::SearchRowItem;
    }

    impl ObjectImpl for SearchRowItem {}
}

glib::wrapper! {
    pub struct SearchRowItem(ObjectSubclass<imp::SearchRowItem>);
}

impl SearchRowItem {
    pub fn new(data: SearchRowData) -> Self {
        let obj: Self = glib::Object::builder().build();
        *obj.imp().data.borrow_mut() = Some(data);
        obj
    }

    pub fn data(&self) -> Option<SearchRowData> {
        self.imp().data.borrow().clone()
    }
}

pub type OpenMatchFn = Box<dyn Fn(&RelPath, usize)>;
pub type RawToggledFn = Box<dyn Fn(bool)>;

struct Inner {
    root: gtk::Box,
    saved: saved::SavedPanel,
    search_entry: gtk::SearchEntry,
    raw_check: gtk::CheckButton,
    status_stack: gtk::Stack,
    error_banner: adw::Banner,
    empty_page: adw::StatusPage,
    summary_label: gtk::Label,
    list_store: gio::ListStore,
    list_view: gtk::ListView,
    search_gen: Cell<u64>,
    vault: RefCell<Option<Arc<Vault>>>,
    search_index: RefCell<Option<Arc<RwLock<SearchIndex>>>>,
    on_open_match: RefCell<Option<OpenMatchFn>>,
    on_raw_toggled: RefCell<Option<RawToggledFn>>,
}

#[derive(Clone)]
pub struct SearchPanel(Rc<Inner>);

impl SearchPanel {
    pub fn new(search_raw_initial: bool) -> Self {
        let root = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(6)
            .hexpand(true)
            .vexpand(true)
            .build();
        root.update_property(&[gtk::accessible::Property::Label("Panel de búsqueda")]);

        // Cabecera del panel: entrada de búsqueda y ayuda con operadores
        let header_box = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(4)
            .margin_start(12)
            .margin_end(12)
            .margin_top(8)
            .margin_bottom(4)
            .build();

        let search_entry = gtk::SearchEntry::builder()
            .placeholder_text("Buscar en toda la bóveda…")
            .hexpand(true)
            .build();
        search_entry.update_property(&[gtk::accessible::Property::Label("Campo de búsqueda")]);

        let help_btn = gtk::MenuButton::builder()
            .label("?")
            .tooltip_text("Operadores de búsqueda")
            .valign(gtk::Align::Center)
            .css_classes(["flat"])
            .build();
        help_btn.update_property(&[
            gtk::accessible::Property::Label("Ayuda con operadores de búsqueda"),
            gtk::accessible::Property::Description(
                "Muestra los operadores de búsqueda disponibles",
            ),
        ]);

        let popover = gtk::Popover::builder()
            .position(gtk::PositionType::Bottom)
            .autohide(true)
            .build();
        popover.update_property(&[gtk::accessible::Property::Label(
            "Operadores de búsqueda disponibles",
        )]);

        let help_content = gtk::Box::new(gtk::Orientation::Vertical, 8);
        help_content.set_margin_start(12);
        help_content.set_margin_end(12);
        help_content.set_margin_top(12);
        help_content.set_margin_bottom(12);
        help_content.set_size_request(300, -1);

        let help_title = gtk::Label::builder()
            .label("Operadores de búsqueda")
            .css_classes(["heading"])
            .xalign(0.0)
            .build();
        help_content.append(&help_title);

        let grid = gtk::Grid::builder()
            .column_spacing(10)
            .row_spacing(6)
            .build();

        let operators = [
            ("palabra1 palabra2", "Contiene ambas palabras"),
            ("\"frase exacta\"", "Palabras contiguas en ese orden"),
            ("-palabra", "Excluye notas con el término"),
            ("a OR b", "Contiene al menos uno"),
            ("tag:#etiqueta", "Etiqueta (incluye subetiquetas)"),
            (
                "tag:\"#con espacios\"",
                "Etiqueta literal; \\\" y \\\\ para comilla y barra",
            ),
            ("path:carpeta", "Ruta de la nota"),
            ("file:nombre", "Nombre de archivo"),
            ("[clave]", "Propiedad existente"),
            ("[clave:valor]", "Propiedad con ese valor"),
            ("line:(a b)", "Términos en la misma línea"),
            ("section:(a b)", "Términos en la misma sección"),
        ];

        for (row, (syntax, desc)) in operators.iter().enumerate() {
            let syn_label = gtk::Label::builder()
                .label(*syntax)
                .xalign(0.0)
                .css_classes(["monospace", "dim-label"])
                .build();
            let desc_label = gtk::Label::builder()
                .label(*desc)
                .xalign(0.0)
                .wrap(true)
                .build();
            grid.attach(&syn_label, 0, row as i32, 1, 1);
            grid.attach(&desc_label, 1, row as i32, 1, 1);
        }

        help_content.append(&grid);
        popover.set_child(Some(&help_content));
        help_btn.set_popover(Some(&popover));

        header_box.append(&search_entry);
        header_box.append(&help_btn);

        // Filtro: Incluir fuentes (raw/)
        let raw_check = gtk::CheckButton::builder()
            .label("Incluir fuentes (raw/)")
            .active(search_raw_initial)
            .margin_start(12)
            .margin_end(12)
            .margin_bottom(6)
            .build();
        raw_check.update_property(&[gtk::accessible::Property::Label("Incluir fuentes (raw/)")]);

        let error_banner = adw::Banner::builder().title("").revealed(false).build();
        error_banner.add_css_class("error");

        // Stack de estados
        let status_stack = gtk::Stack::builder()
            .transition_type(gtk::StackTransitionType::Crossfade)
            .vexpand(true)
            .build();

        // 1. Estado inicial vacío (placeholder)
        let placeholder_page = adw::StatusPage::builder()
            .icon_name("system-search-symbolic")
            .title("Buscar en la bóveda")
            .description(
                "Escribí palabras, \"frases exactas\", -excluir, OR, o filtros como tag:#etiqueta.",
            )
            .vexpand(true)
            .build();
        status_stack.add_named(&placeholder_page, Some("placeholder"));

        // 2. Estado "Indexando..."
        let indexing_page = adw::StatusPage::builder()
            .icon_name("view-refresh-symbolic")
            .title("Indexando…")
            .description("Indexando la bóveda en segundo plano.")
            .vexpand(true)
            .build();
        status_stack.add_named(&indexing_page, Some("indexing"));

        // 3. Estado "Sin resultados"
        let empty_page = adw::StatusPage::builder()
            .icon_name("edit-find-symbolic")
            .title("Sin resultados")
            .description("No se encontraron notas que coincidan.")
            .vexpand(true)
            .build();
        status_stack.add_named(&empty_page, Some("empty"));

        // 4. Estado de resultados: resumen + lista
        let results_box = gtk::Box::new(gtk::Orientation::Vertical, 4);
        let summary_label = gtk::Label::builder()
            .xalign(0.0)
            .margin_start(14)
            .margin_end(14)
            .margin_top(4)
            .margin_bottom(4)
            .css_classes(["dim-label", "caption"])
            .build();
        results_box.append(&summary_label);

        let list_store = gio::ListStore::new::<SearchRowItem>();
        let selection = gtk::SingleSelection::new(Some(list_store.clone()));

        let factory = gtk::SignalListItemFactory::new();
        factory.connect_setup(|_, item| {
            let item = item.downcast_ref::<gtk::ListItem>().expect("ListItem");
            let container = gtk::Box::new(gtk::Orientation::Vertical, 2);
            item.set_child(Some(&container));
        });

        factory.connect_bind(|_, item| {
            let item = item.downcast_ref::<gtk::ListItem>().expect("ListItem");
            let Some(row_item) = item.item().and_downcast::<SearchRowItem>() else {
                return;
            };
            let Some(data) = row_item.data() else {
                return;
            };
            let container = item.child().and_downcast::<gtk::Box>().expect("Box");
            // Limpiar hijos previos
            while let Some(child) = container.first_child() {
                container.remove(&child);
            }

            match data {
                SearchRowData::NoteHeader {
                    name,
                    folder,
                    match_count,
                    is_raw,
                    ..
                } => {
                    item.set_selectable(true);
                    item.set_activatable(true);

                    let header_box = gtk::Box::new(gtk::Orientation::Horizontal, 8);
                    header_box.set_margin_top(8);
                    header_box.set_margin_bottom(2);
                    header_box.set_margin_start(10);
                    header_box.set_margin_end(10);

                    let icon = gtk::Image::from_icon_name(if is_raw {
                        "text-x-generic-symbolic"
                    } else {
                        "text-html-symbolic"
                    });
                    header_box.append(&icon);

                    let titles_box = gtk::Box::new(gtk::Orientation::Vertical, 0);
                    titles_box.set_hexpand(true);

                    let title_label = gtk::Label::builder()
                        .label(&name)
                        .xalign(0.0)
                        .ellipsize(gtk::pango::EllipsizeMode::End)
                        .css_classes(["heading"])
                        .build();
                    titles_box.append(&title_label);

                    if !folder.is_empty() {
                        let folder_label = gtk::Label::builder()
                            .label(&folder)
                            .xalign(0.0)
                            .ellipsize(gtk::pango::EllipsizeMode::End)
                            .css_classes(["dim-label", "caption"])
                            .build();
                        titles_box.append(&folder_label);
                    }
                    header_box.append(&titles_box);

                    if is_raw {
                        let raw_badge = gtk::Label::builder()
                            .label("fuente")
                            .css_classes(["tag", "dim-label"])
                            .build();
                        header_box.append(&raw_badge);
                    }

                    if match_count > 0 {
                        let count_badge = gtk::Label::builder()
                            .label(match_count.to_string())
                            .css_classes(["numeric", "dim-label"])
                            .build();
                        header_box.append(&count_badge);
                    }

                    container.append(&header_box);
                    container.update_property(&[gtk::accessible::Property::Label(&format!(
                        "Nota {name}, {folder}, {match_count} coincidencias"
                    ))]);
                }
                SearchRowData::Match {
                    line,
                    snippet,
                    markup,
                    ..
                } => {
                    item.set_selectable(true);
                    item.set_activatable(true);

                    let match_box = gtk::Box::new(gtk::Orientation::Horizontal, 8);
                    match_box.set_margin_top(2);
                    match_box.set_margin_bottom(2);
                    match_box.set_margin_start(28);
                    match_box.set_margin_end(10);

                    let line_label = gtk::Label::builder()
                        .label(line.to_string())
                        .xalign(1.0)
                        .width_chars(3)
                        .css_classes(["dim-label", "numeric", "caption"])
                        .build();
                    match_box.append(&line_label);

                    let snippet_label = gtk::Label::builder()
                        .use_markup(true)
                        .label(&markup)
                        .xalign(0.0)
                        .hexpand(true)
                        .ellipsize(gtk::pango::EllipsizeMode::End)
                        .build();
                    match_box.append(&snippet_label);

                    container.append(&match_box);
                    container.update_property(&[gtk::accessible::Property::Label(&format!(
                        "Línea {line}: {snippet}"
                    ))]);
                }
            }
        });

        let list_view = gtk::ListView::builder()
            .model(&selection)
            .factory(&factory)
            .single_click_activate(true)
            .build();
        list_view.update_property(&[gtk::accessible::Property::Label("Resultados de búsqueda")]);

        let scroll = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .child(&list_view)
            .vexpand(true)
            .build();
        results_box.append(&scroll);

        status_stack.add_named(&results_box, Some("results"));

        let saved = saved::SavedPanel::new(&search_entry);
        root.append(&header_box);
        root.append(&raw_check);
        root.append(saved.widget());
        root.append(&error_banner);
        root.append(&status_stack);

        let inner = Rc::new(Inner {
            root,
            saved,
            search_entry,
            raw_check,
            status_stack,
            error_banner,
            empty_page,
            summary_label,
            list_store,
            list_view,
            search_gen: Cell::new(0),
            vault: RefCell::new(None),
            search_index: RefCell::new(None),
            on_open_match: RefCell::new(None),
            on_raw_toggled: RefCell::new(None),
        });

        let panel = SearchPanel(inner);

        // Conectar búsqueda al escribir con debounce de 120 ms
        let p = panel.clone();
        panel.0.search_entry.connect_search_changed(move |_| {
            p.queue_search();
        });

        // Conectar alternancia de raw/
        let p = panel.clone();
        panel.0.raw_check.connect_toggled(move |btn| {
            let active = btn.is_active();
            if let Some(cb) = p.0.on_raw_toggled.borrow().as_ref() {
                cb(active);
            }
            p.queue_search();
        });

        // Conectar activación de fila en ListView
        let p = panel.clone();
        panel.0.list_view.connect_activate(move |lv, pos| {
            let Some(model) = lv.model().and_downcast::<gtk::SingleSelection>() else {
                return;
            };
            let Some(item) = model.item(pos).and_downcast::<SearchRowItem>() else {
                return;
            };
            if let Some(data) = item.data() {
                match data {
                    SearchRowData::Match { path, line, .. } => {
                        if let Some(cb) = p.0.on_open_match.borrow().as_ref() {
                            cb(&path, line);
                        }
                    }
                    SearchRowData::NoteHeader { path, .. } => {
                        if let Some(cb) = p.0.on_open_match.borrow().as_ref() {
                            cb(&path, 1);
                        }
                    }
                }
            }
        });

        panel
    }

    pub fn widget(&self) -> &gtk::Box {
        &self.0.root
    }

    #[allow(dead_code)]
    pub fn search_entry(&self) -> &gtk::SearchEntry {
        &self.0.search_entry
    }

    pub fn grab_focus(&self) {
        self.0.search_entry.grab_focus();
        self.0.search_entry.select_region(0, -1);
    }

    pub fn connect_open_match<F: Fn(&RelPath, usize) + 'static>(&self, f: F) {
        *self.0.on_open_match.borrow_mut() = Some(Box::new(f));
    }

    pub fn connect_raw_toggled<F: Fn(bool) + 'static>(&self, f: F) {
        *self.0.on_raw_toggled.borrow_mut() = Some(Box::new(f));
    }

    pub fn set_vault(&self, vault: Option<Arc<Vault>>, session: u64) {
        self.0.saved.set_vault(vault.clone(), session);
        *self.0.vault.borrow_mut() = vault;
    }

    pub fn set_search_index(&self, index: Option<Arc<RwLock<SearchIndex>>>) {
        *self.0.search_index.borrow_mut() = index;
        self.0.search_gen.set(self.0.search_gen.get() + 1);
        if self.0.search_index.borrow().is_none() {
            self.0.list_store.remove_all();
            self.0.summary_label.set_text("");
            self.0.status_stack.set_visible_child_name("indexing");
        }
        if self.0.search_index.borrow().is_some() {
            // El índice terminó de construirse
            if !self.0.search_entry.text().trim().is_empty() {
                self.queue_search();
            }
        }
    }

    #[allow(dead_code)]
    /// Activa la primera fila de coincidencia (no de nota) cuyo fragmento contiene `text`, como
    /// un clic o Intro. Devuelve si la encontró. Para las pruebas: AT-SPI no expone la activación
    /// de las filas de un `GtkListView` (solo `listitem.scroll-to`).
    #[cfg(test)]
    pub fn activate_match(&self, text: &str) -> bool {
        let store = &self.0.list_store;
        for pos in 0..store.n_items() {
            let Some(item) = store.item(pos).and_downcast::<SearchRowItem>() else {
                continue;
            };
            if let Some(SearchRowData::Match { snippet, .. }) = item.data()
                && snippet.contains(text)
            {
                self.0.list_view.emit_by_name::<()>("activate", &[&pos]);
                return true;
            }
        }
        false
    }

    pub fn set_search_query(&self, text: &str) {
        self.0.search_entry.set_text(text);
        self.0.search_entry.set_position(-1);
    }

    pub fn toggle_raw(&self) {
        self.0.raw_check.set_active(!self.0.raw_check.is_active());
    }

    fn queue_search(&self) {
        let generation = self.0.search_gen.get() + 1;
        self.0.search_gen.set(generation);
        let this = self.clone();
        glib::timeout_add_local_once(std::time::Duration::from_millis(120), move || {
            if this.0.search_gen.get() == generation {
                this.perform_search(generation);
            }
        });
    }

    fn perform_search(&self, generation: u64) {
        let text = self.0.search_entry.text();
        let trimmed = text.trim();

        if trimmed.is_empty() {
            self.0.error_banner.set_revealed(false);
            self.0.status_stack.set_visible_child_name("placeholder");
            self.0.list_store.remove_all();
            self.0.summary_label.set_text("");
            return;
        }

        // Parsear consulta
        let query = match orbital_vault::parse_query(trimmed) {
            Ok(q) => {
                self.0.error_banner.set_revealed(false);
                q
            }
            Err(err) => {
                // Error de consulta (FR-011): mostrar aviso y NO borrar el texto
                self.0.error_banner.set_title(&format!(
                    "Error en columna {}: {}",
                    err.at + 1,
                    err.message
                ));
                self.0.error_banner.set_revealed(true);
                return;
            }
        };

        let Some(index) = self.0.search_index.borrow().clone() else {
            // Índice aún construyéndose
            self.0.status_stack.set_visible_child_name("indexing");
            return;
        };

        let Some(vault) = self.0.vault.borrow().clone() else {
            return;
        };

        let include_raw = self.0.raw_check.is_active();
        let opts = SearchOptions {
            include_raw,
            max_notes: 200,
            max_matches: 20,
        };

        let this = self.clone();
        let trimmed_str = trimmed.to_string();
        let measure_search = std::env::var_os("ORBITAL_MEDIR_BUSQUEDA").is_some();
        let start_time = if measure_search {
            Some(std::time::Instant::now())
        } else {
            None
        };

        glib::spawn_future_local(async move {
            let res: Option<SearchResults> = gio::spawn_blocking(move || {
                let idx = index.read().ok()?;
                Some(idx.search(&vault, &query, &opts))
            })
            .await
            .ok()
            .flatten();

            if let Some(start) = start_time {
                let elapsed = start.elapsed();
                println!(
                    "ORBITAL_MEDIR_BUSQUEDA: búsqueda de «{}» completada en {:.2} ms",
                    trimmed_str,
                    elapsed.as_secs_f64() * 1000.0
                );
            }

            if this.0.search_gen.get() != generation {
                return; // Descartar resultado viejo
            }

            let Some(results) = res else {
                return;
            };

            if results.total_notes == 0 {
                this.0.empty_page.set_title("Sin resultados");
                this.0.empty_page.set_description(Some(&format!(
                    "No se encontraron notas que coincidan con «{trimmed_str}»"
                )));
                this.0.status_stack.set_visible_child_name("empty");
                this.0.list_store.remove_all();
                this.0.summary_label.set_text("0 notas");
                return;
            }

            this.0.list_store.remove_all();
            for hit in results.notes {
                let full_str = hit.path.as_str();
                let (dir, file_name) = match full_str.rsplit_once('/') {
                    Some((d, f)) => (d, f),
                    None => ("", full_str),
                };
                let stem = file_name.rsplit_once('.').map_or(file_name, |(s, _)| s);
                let is_raw = hit.zone == Zone::Raw;
                let total_hits = hit.matches.len() + hit.more;

                // Fila de encabezado de nota
                this.0
                    .list_store
                    .append(&SearchRowItem::new(SearchRowData::NoteHeader {
                        path: hit.path.clone(),
                        name: stem.to_string(),
                        folder: dir.to_string(),
                        match_count: total_hits,
                        is_raw,
                    }));

                // Filas de coincidencias
                for m in hit.matches {
                    let markup = pango_highlight(&m.snippet, &m.highlights);
                    this.0
                        .list_store
                        .append(&SearchRowItem::new(SearchRowData::Match {
                            path: hit.path.clone(),
                            line: m.line,
                            snippet: m.snippet,
                            markup,
                        }));
                }
            }

            this.0.summary_label.set_text(&format!(
                "{} notas ({} coincidencias)",
                results.total_notes, results.total_matches
            ));
            this.0.status_stack.set_visible_child_name("results");
        });
    }
}

pub fn pango_highlight(snippet: &str, highlights: &[Range<usize>]) -> String {
    let mut out = String::with_capacity(snippet.len() + highlights.len() * 16);
    let chars: Vec<char> = snippet.chars().collect();
    let n = chars.len();

    let mut current_idx = 0;
    for r in highlights {
        let start = r.start.min(n);
        let end = r.end.min(n);
        if start > current_idx {
            escape_and_append(&chars[current_idx..start], &mut out);
        }
        if end > start {
            out.push_str("<b>");
            escape_and_append(&chars[start..end], &mut out);
            out.push_str("</b>");
            current_idx = end;
        }
    }
    if current_idx < n {
        escape_and_append(&chars[current_idx..n], &mut out);
    }
    out
}

fn escape_and_append(chars: &[char], out: &mut String) {
    for &c in chars {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            other => out.push(other),
        }
    }
}
