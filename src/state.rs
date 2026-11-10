//! Estado de la app entre sesiones: `$XDG_CONFIG_HOME/orbital/estado.yaml` (research R9).
//!
//! Se lee y se escribe siempre fuera del hilo principal (`gio::spawn_blocking`).

use std::fs;
use std::path::{Path, PathBuf};

use gtk::glib;
use orbital_vault::props::emit::{self, LineEnding};
use orbital_vault::props::yaml::parse_block;
use orbital_vault::props::{PropValue, find};
use orbital_vault::write::atomic_write;

const MAX_RECENT: usize = 10;

/// Bóvedas recientes y la última abierta.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AppState {
    pub last_vault: Option<PathBuf>,
    pub recent: Vec<PathBuf>,
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
        Self { last_vault, recent }
    }

    /// Registra que se abrió `vault`.
    pub fn opened(&mut self, vault: &Path) {
        self.last_vault = Some(vault.to_path_buf());
        self.recent.retain(|p| p != vault);
        self.recent.insert(0, vault.to_path_buf());
        self.recent.truncate(MAX_RECENT);
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
        let mut entries = Vec::with_capacity(2);
        if let Some(v) = &self.last_vault {
            entries.push(("last_vault", text(v)));
        }
        entries.push((
            "recent",
            PropValue::List(self.recent.iter().map(|p| text(p)).collect()),
        ));
        atomic_write(path, emit::document(&entries, LineEnding::Lf).as_bytes()).map(|_| ())
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
        assert_eq!(
            s.recent,
            vec![
                PathBuf::from("/tmp/a"),
                PathBuf::from("/home/ejemplo/Documentos/Bóveda")
            ]
        );
        s.save_to(&file).unwrap();
        assert_eq!(AppState::load_from(&file), s);
        let _ = fs::remove_dir_all(dir);
    }
}
