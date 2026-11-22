//! Estado de la app entre sesiones: `$XDG_CONFIG_HOME/orbital/estado.yaml` (research R9).
//!
//! Se lee y se escribe siempre fuera del hilo principal (`gio::spawn_blocking`).

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use gtk::glib;
use orbital_vault::props::emit::{self, LineEnding};
use orbital_vault::props::yaml::parse_block;
use orbital_vault::props::{PropValue, find};
use orbital_vault::write::atomic_write;

const MAX_RECENT: usize = 10;
const MAX_RECENT_NOTES_PER_VAULT: usize = 20;
/// Tamaño de la ventana sin maximizar la primera vez, y el mínimo (056).
const DEFAULT_WIDTH: i32 = 1100;
const DEFAULT_HEIGHT: i32 = 720;
pub const MIN_WIDTH: i32 = 640;
pub const MIN_HEIGHT: i32 = 450;

/// Bóvedas recientes y la última abierta.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AppState {
    pub last_vault: Option<PathBuf>,
    pub recent: Vec<PathBuf>,
    /// Últimas notas abiertas por bóveda (hasta 20 por bóveda, T019 de la 004).
    pub recent_notes: HashMap<PathBuf, Vec<String>>,
    /// Última vista usada en notas `.md` (FR-013 de la 002).
    pub md_view: Option<String>,
    /// Última vista usada en notas `.html`.
    pub html_view: Option<String>,
    /// Si la búsqueda incluye archivos de `raw/` (T017 de la 004).
    pub search_raw: bool,
    /// Ancho de la ventana sin maximizar (al que vuelve si se desmaximiza; 056).
    pub window_width: Option<i32>,
    /// Alto de la ventana sin maximizar.
    pub window_height: Option<i32>,
    /// Números de línea apagados en la vista Código (054; por defecto se ven).
    pub hide_line_numbers: bool,
    /// Valores por defecto de la tabla de contenidos (035): los toma un ` ```toc ` que no
    /// escribe esa opción.
    pub toc: orbital_vault::toc::TocDefaults,
}

fn state_file() -> PathBuf {
    glib::user_config_dir().join("orbital").join("estado.yaml")
}

impl AppState {
    /// Lee el estado. Si falta o no se entiende, devuelve un estado vacío.
    pub fn load() -> Self {
        Self::load_from(&state_file())
    }

    fn load_from(path: &Path) -> Self {
        let Ok(text) = fs::read_to_string(path) else {
            return Self::default();
        };
        let Ok(entries) = parse_block(&text) else {
            return Self::default();
        };
        let last_vault = find(&entries, "last_vault")
            .and_then(|e| e.value.as_text())
            .map(PathBuf::from);
        let recent = match find(&entries, "recent").map(|e| &e.value) {
            Some(PropValue::List(items)) => items
                .iter()
                .filter_map(PropValue::as_text)
                .map(PathBuf::from)
                .collect(),
            _ => Vec::new(),
        };
        let recent_notes = match find(&entries, "recent_notes").map(|e| &e.value) {
            Some(PropValue::Map(pairs)) => {
                let mut map = HashMap::new();
                for (vault_str, val) in pairs {
                    if let PropValue::List(items) = val {
                        let notes: Vec<String> = items
                            .iter()
                            .filter_map(PropValue::as_text)
                            .map(str::to_owned)
                            .collect();
                        map.insert(PathBuf::from(vault_str), notes);
                    }
                }
                map
            }
            _ => HashMap::new(),
        };
        let md_view = find(&entries, "md_view")
            .and_then(|e| e.value.as_text())
            .map(str::to_owned);
        let html_view = find(&entries, "html_view")
            .and_then(|e| e.value.as_text())
            .map(str::to_owned);
        let search_raw = find(&entries, "search_raw")
            .and_then(|e| match &e.value {
                PropValue::Bool(b) => Some(*b),
                PropValue::Text(s) => Some(s == "true"),
                _ => None,
            })
            .unwrap_or(false);
        let window_width = find(&entries, "window_width").and_then(|e| match &e.value {
            PropValue::Number(s) | PropValue::Text(s) => s.parse().ok(),
            _ => None,
        });
        let window_height = find(&entries, "window_height").and_then(|e| match &e.value {
            PropValue::Number(s) | PropValue::Text(s) => s.parse().ok(),
            _ => None,
        });
        let hide_line_numbers = find(&entries, "line_numbers").is_some_and(|e| match &e.value {
            PropValue::Bool(b) => !*b,
            PropValue::Text(s) => s == "false",
            _ => false,
        });
        let toc = toc_from(&entries);
        Self {
            last_vault,
            recent,
            recent_notes,
            md_view,
            html_view,
            search_raw,
            window_width,
            window_height,
            hide_line_numbers,
            toc,
        }
    }

    /// Tamaño de la ventana sin maximizar (056): el guardado, o 1100×720 la primera vez, nunca
    /// menos que el mínimo de la ventana (640×450). La ventana abre siempre maximizada; este es
    /// el tamaño al que vuelve si se desmaximiza.
    pub fn restore_size(&self) -> (i32, i32) {
        (
            self.window_width.unwrap_or(DEFAULT_WIDTH).max(MIN_WIDTH),
            self.window_height.unwrap_or(DEFAULT_HEIGHT).max(MIN_HEIGHT),
        )
    }

    /// Registra que se abrió `vault`.
    pub fn opened(&mut self, vault: &Path) {
        self.last_vault = Some(vault.to_path_buf());
        self.recent.retain(|p| p != vault);
        self.recent.insert(0, vault.to_path_buf());
        self.recent.truncate(MAX_RECENT);
    }

    /// Registra que se abrió la nota `note` (ruta relativa) en `vault` (T019).
    pub fn note_opened(&mut self, vault: &Path, note: &str) {
        let list = self.recent_notes.entry(vault.to_path_buf()).or_default();
        list.retain(|p| p != note);
        list.insert(0, note.to_owned());
        list.truncate(MAX_RECENT_NOTES_PER_VAULT);
    }

    /// Retorna las notas recientes para `vault` (hasta 20).
    pub fn recent_notes_for(&self, vault: &Path) -> &[String] {
        self.recent_notes
            .get(vault)
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }

    /// Guarda el estado de forma atómica.
    pub fn save(&self) -> std::io::Result<()> {
        self.save_to(&state_file())
    }

    fn save_to(&self, path: &Path) -> std::io::Result<()> {
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        let text = |p: &Path| PropValue::Text(p.to_string_lossy().into_owned());
        let mut entries = Vec::with_capacity(3);
        if let Some(v) = &self.last_vault {
            entries.push(("last_vault", text(v)));
        }
        entries.push((
            "recent",
            PropValue::List(self.recent.iter().map(|p| text(p)).collect()),
        ));
        if !self.recent_notes.is_empty() {
            let mut pairs = Vec::with_capacity(self.recent_notes.len());
            for (vault, notes) in &self.recent_notes {
                let note_values = notes.iter().map(|n| PropValue::Text(n.clone())).collect();
                pairs.push((
                    vault.to_string_lossy().into_owned(),
                    PropValue::List(note_values),
                ));
            }
            pairs.sort_by(|a, b| a.0.cmp(&b.0));
            entries.push(("recent_notes", PropValue::Map(pairs)));
        }
        if let Some(v) = &self.html_view {
            entries.push(("html_view", PropValue::Text(v.clone())));
        }
        if let Some(v) = &self.md_view {
            entries.push(("md_view", PropValue::Text(v.clone())));
        }
        if self.search_raw {
            entries.push(("search_raw", PropValue::Bool(true)));
        }
        if let Some(w) = self.window_width {
            entries.push(("window_width", PropValue::Number(w.to_string())));
        }
        if let Some(h) = self.window_height {
            entries.push(("window_height", PropValue::Number(h.to_string())));
        }
        if self.hide_line_numbers {
            entries.push(("line_numbers", PropValue::Bool(false)));
        }
        toc_entries(&self.toc, &mut entries);
        atomic_write(path, emit::document(&entries, LineEnding::Lf).as_bytes()).map(|_| ())
    }
}

/// Los valores por defecto de la tabla de contenidos guardados (035): `toc_title`, `toc_style`,
/// `toc_min_level`, `toc_max_level`, `toc_links` y `toc_hide_empty`. Lo que falta o no se
/// entiende queda con el valor de fábrica.
fn toc_from(entries: &[orbital_vault::props::PropEntry]) -> orbital_vault::toc::TocDefaults {
    use orbital_vault::toc::{TocDefaults, TocStyle};
    let mut d = TocDefaults::default();
    let texto = |k: &str| {
        find(entries, k).and_then(|e| match &e.value {
            PropValue::Text(s) | PropValue::Number(s) => Some(s.clone()),
            PropValue::Bool(b) => Some(b.to_string()),
            _ => None,
        })
    };
    if let Some(t) = texto("toc_title") {
        d.title = t;
    }
    if let Some(s) = texto("toc_style").and_then(|s| TocStyle::from_name(&s)) {
        d.style = s;
    }
    if let Some(n) = texto("toc_min_level").and_then(|s| s.parse::<u8>().ok()) {
        d.min_level = n.min(6);
    }
    if let Some(n) = texto("toc_max_level").and_then(|s| s.parse::<u8>().ok()) {
        d.max_level = n.min(6);
    }
    if let Some(b) = texto("toc_links").and_then(|s| s.parse::<bool>().ok()) {
        d.include_links = b;
    }
    if let Some(b) = texto("toc_hide_empty").and_then(|s| s.parse::<bool>().ok()) {
        d.hide_when_empty = b;
    }
    d
}

/// Escribe solo lo que difiere del valor de fábrica.
fn toc_entries(
    toc: &orbital_vault::toc::TocDefaults,
    entries: &mut Vec<(&'static str, PropValue)>,
) {
    let fabrica = orbital_vault::toc::TocDefaults::default();
    if toc.title != fabrica.title {
        entries.push(("toc_title", PropValue::Text(toc.title.clone())));
    }
    if toc.style != fabrica.style {
        entries.push(("toc_style", PropValue::Text(toc.style.alias().to_owned())));
    }
    if toc.min_level != fabrica.min_level {
        entries.push((
            "toc_min_level",
            PropValue::Number(toc.min_level.to_string()),
        ));
    }
    if toc.max_level != fabrica.max_level {
        entries.push((
            "toc_max_level",
            PropValue::Number(toc.max_level.to_string()),
        ));
    }
    if toc.include_links != fabrica.include_links {
        entries.push(("toc_links", PropValue::Bool(toc.include_links)));
    }
    if toc.hide_when_empty != fabrica.hide_when_empty {
        entries.push(("toc_hide_empty", PropValue::Bool(toc.hide_when_empty)));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_and_recent_order() {
        let dir = std::env::temp_dir().join(format!("orbital-state-{}", std::process::id()));
        let file = dir.join("estado.yaml");
        let mut s = AppState::default();
        s.opened(Path::new("/tmp/a"));
        s.opened(Path::new("/home/ejemplo/Documentos/Bóveda"));
        s.opened(Path::new("/tmp/a"));
        s.window_width = Some(1200);
        s.window_height = Some(800);
        s.hide_line_numbers = true;
        assert_eq!(
            s.recent,
            vec![
                PathBuf::from("/tmp/a"),
                PathBuf::from("/home/ejemplo/Documentos/Bóveda")
            ]
        );
        s.save_to(&file).unwrap();
        assert_eq!(AppState::load_from(&file), s);
        assert!(
            fs::read_to_string(&file)
                .unwrap()
                .contains("line_numbers: false")
        );
        let _ = fs::remove_dir_all(dir);
    }

    /// Los valores por defecto de la tabla de contenidos (035) van y vuelven; los de fábrica no
    /// se escriben.
    #[test]
    fn toc_defaults_roundtrip() {
        use orbital_vault::toc::{TocDefaults, TocStyle};
        let dir = std::env::temp_dir().join(format!("orbital-state-toc-{}", std::process::id()));
        let file = dir.join("estado.yaml");
        AppState::default().save_to(&file).unwrap();
        assert!(!fs::read_to_string(&file).unwrap().contains("toc_"));
        let mut s = AppState::default();
        s.toc = TocDefaults {
            title: "En esta nota: índice".into(),
            style: TocStyle::NestedOrderedList,
            min_level: 2,
            max_level: 3,
            include_links: false,
            hide_when_empty: true,
        };
        s.save_to(&file).unwrap();
        assert_eq!(AppState::load_from(&file).toc, s.toc);
        // Un valor que no se entiende queda con el de fábrica.
        fs::write(&file, "toc_style: arbol\ntoc_max_level: x\n").unwrap();
        assert_eq!(AppState::load_from(&file).toc, TocDefaults::default());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn line_numbers_default_on() {
        let dir = std::env::temp_dir().join(format!("orbital-lineas-{}", std::process::id()));
        let file = dir.join("estado.yaml");
        AppState::default().save_to(&file).unwrap();
        assert!(!fs::read_to_string(&file).unwrap().contains("line_numbers"));
        assert!(!AppState::load_from(&file).hide_line_numbers);
        fs::write(&file, "line_numbers: false\n").unwrap();
        assert!(AppState::load_from(&file).hide_line_numbers);
        fs::write(&file, "line_numbers: true\n").unwrap();
        assert!(!AppState::load_from(&file).hide_line_numbers);
        let _ = fs::remove_dir_all(dir);
    }

    /// 056: un `estado.yaml` de antes (con `window_maximized`) sigue cargando; la clave ya no se
    /// lee ni se escribe, y el tamaño guardado queda como tamaño sin maximizar.
    #[test]
    fn old_state_loads_and_restore_size() {
        let dir = std::env::temp_dir().join(format!("orbital-ventana-{}", std::process::id()));
        let file = dir.join("estado.yaml");
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            &file,
            "last_vault: /tmp/a\nrecent:\n  - /tmp/a\nwindow_width: 1200\nwindow_height: 800\n\
             window_maximized: true\nline_numbers: false\n",
        )
        .unwrap();
        let s = AppState::load_from(&file);
        assert_eq!(s.last_vault.as_deref(), Some(Path::new("/tmp/a")));
        assert!(s.hide_line_numbers);
        assert_eq!(s.restore_size(), (1200, 800));
        s.save_to(&file).unwrap();
        let text = fs::read_to_string(&file).unwrap();
        assert!(!text.contains("window_maximized"), "{text}");
        assert_eq!(AppState::load_from(&file), s);

        // Sin estado: 1100×720; un tamaño menor que el mínimo sube al mínimo.
        assert_eq!(AppState::default().restore_size(), (1100, 720));
        let chico = AppState {
            window_width: Some(100),
            window_height: Some(-1),
            ..AppState::default()
        };
        assert_eq!(chico.restore_size(), (MIN_WIDTH, MIN_HEIGHT));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn recent_notes_per_vault_and_limit() {
        let dir = std::env::temp_dir().join(format!("orbital-recent-notes-{}", std::process::id()));
        let file = dir.join("estado.yaml");
        let mut s = AppState::default();
        let v1 = Path::new("/tmp/vault1");
        let v2 = Path::new("/tmp/vault2");

        // Insertar más de 20 notas en v1
        for i in 0..25 {
            s.note_opened(v1, &format!("wiki/nota-{i:02}.md"));
        }
        s.note_opened(v2, "wiki/otra.md");

        // Verificar límite de 20 y orden más reciente primero
        let rec1 = s.recent_notes_for(v1);
        assert_eq!(rec1.len(), 20);
        assert_eq!(rec1[0], "wiki/nota-24.md");
        assert_eq!(rec1[19], "wiki/nota-05.md");

        // Reabrir nota 10: pasa al principio sin duplicar
        s.note_opened(v1, "wiki/nota-10.md");
        let rec1 = s.recent_notes_for(v1);
        assert_eq!(rec1.len(), 20);
        assert_eq!(rec1[0], "wiki/nota-10.md");

        // v2 es independiente
        let rec2 = s.recent_notes_for(v2);
        assert_eq!(rec2, &["wiki/otra.md"]);

        // Guardar y recuperar
        s.save_to(&file).unwrap();
        let loaded = AppState::load_from(&file);
        assert_eq!(loaded.recent_notes_for(v1), s.recent_notes_for(v1));
        assert_eq!(loaded.recent_notes_for(v2), s.recent_notes_for(v2));
        let _ = fs::remove_dir_all(dir);
    }
}
