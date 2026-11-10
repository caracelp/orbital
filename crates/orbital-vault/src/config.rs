//! Configuración de la bóveda (`.orbital/config.yaml`, research R9).

use std::fs;
use std::path::Path;

use crate::props::emit::{self, LineEnding};
use crate::props::{PropValue, find, yaml};
use crate::write::ORBITAL_DIR;

/// Tipo de enlace que Orbital escribe al autocompletar (FR-018).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum LinkStyle {
    #[default]
    Wikilink,
    Markdown,
}

impl LinkStyle {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Wikilink => "wikilink",
            Self::Markdown => "markdown",
        }
    }
}

/// Configuración de una bóveda. Sin archivo, valen los valores predeterminados.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct VaultConfig {
    pub format_version: u32,
    pub link_style: LinkStyle,
    /// Formato del nombre de la nota diaria; `AAAA`, `MM` y `DD` se reemplazan.
    pub journal_name: String,
    /// Actor `human:<id>`; `None` = el usuario del sistema.
    pub actor: Option<String>,
    /// Si debe preguntar antes de crear notas pendientes al hacer clic en un enlace inexistente.
    pub ask_create_missing_notes: bool,
}

impl Default for VaultConfig {
    fn default() -> Self {
        Self {
            format_version: 1,
            link_style: LinkStyle::Wikilink,
            journal_name: "AAAA-MM-DD".to_owned(),
            actor: None,
            ask_create_missing_notes: true,
        }
    }
}

/// Ruta relativa del archivo de configuración.
pub const CONFIG_FILE: &str = "config.yaml";

impl VaultConfig {
    /// Lee `.orbital/config.yaml`. No crea nada. Si falta o no se puede leer, devuelve los
    /// valores predeterminados (y el motivo, si hubo un error).
    pub fn load(root: &Path) -> (Self, Option<String>) {
        let path = root.join(ORBITAL_DIR).join(CONFIG_FILE);
        let Ok(text) = fs::read_to_string(&path) else {
            return (Self::default(), None);
        };
        match yaml::parse_block(&text) {
            Ok(entries) => (Self::from_entries(&entries), None),
            Err(e) => (
                Self::default(),
                Some(format!(
                    "La configuración de la bóveda tiene un error en la línea {}: {}",
                    e.line, e.message
                )),
            ),
        }
    }

    fn from_entries(entries: &[crate::props::PropEntry]) -> Self {
        let mut c = Self::default();
        if let Some(PropValue::Number(n)) = find(entries, "format_version").map(|e| &e.value) {
            c.format_version = n.parse().unwrap_or(1);
        }
        if let Some(s) = find(entries, "link_style").and_then(|e| e.value.as_text()) {
            c.link_style = match s {
                "markdown" => LinkStyle::Markdown,
                _ => LinkStyle::Wikilink,
            };
        }
        if let Some(s) = find(entries, "journal_name").and_then(|e| e.value.as_text()) {
            c.journal_name = s.to_owned();
        }
        c.actor = find(entries, "actor")
            .and_then(|e| e.value.as_text())
            .map(str::to_owned);
        if let Some(val) = find(entries, "ask_create_missing_notes").map(|e| &e.value) {
            match val {
                PropValue::Bool(b) => c.ask_create_missing_notes = *b,
                PropValue::Text(s) => c.ask_create_missing_notes = s != "false",
                _ => {}
            }
        }
        c
    }

    /// Guarda la configuración en `.orbital/config.yaml` (creando `.orbital/` si hace falta), de
    /// forma atómica. Es la única escritura que Orbital hace en `.orbital/` fuera de los respaldos.
    pub fn save(&self, root: &Path) -> std::io::Result<()> {
        let dir = root.join(ORBITAL_DIR);
        fs::create_dir_all(&dir)?;
        crate::write::atomic_write(&dir.join(CONFIG_FILE), self.to_yaml().as_bytes()).map(|_| ())
    }

    /// Representación YAML completa.
    pub fn to_yaml(&self) -> String {
        let mut entries = vec![
            (
                "format_version",
                PropValue::Number(self.format_version.to_string()),
            ),
            (
                "link_style",
                PropValue::Text(self.link_style.as_str().to_owned()),
            ),
            ("journal_name", PropValue::Text(self.journal_name.clone())),
            (
                "ask_create_missing_notes",
                PropValue::Bool(self.ask_create_missing_notes),
            ),
        ];
        if let Some(a) = &self.actor {
            entries.push(("actor", PropValue::Text(a.clone())));
        }
        emit::document(&entries, LineEnding::Lf)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        let c = VaultConfig {
            link_style: LinkStyle::Markdown,
            actor: Some("human:ejemplo".into()),
            ..VaultConfig::default()
        };
        let entries = yaml::parse_block(&c.to_yaml()).unwrap();
        assert_eq!(VaultConfig::from_entries(&entries), c);
    }
}
