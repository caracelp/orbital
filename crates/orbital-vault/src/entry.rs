//! Rutas relativas y entradas de la bóveda.

use std::fmt;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use crate::error::VaultError;

/// Tamaño máximo de un archivo de texto editable (5 MB).
pub const MAX_EDITABLE_BYTES: u64 = 5 * 1024 * 1024;

/// Ruta relativa a la raíz de la bóveda, siempre con `/` y sin `.`, `..` ni componentes vacíos.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct RelPath(String);

impl RelPath {
    /// Valida y crea una ruta relativa. Acepta nombres no ASCII.
    pub fn new(s: &str) -> Result<Self, VaultError> {
        if s.is_empty() || s.starts_with('/') || s.contains('\\') || s.contains('\0') {
            return Err(VaultError::InvalidPath(s.to_owned()));
        }
        if s.split('/').any(|c| c.is_empty() || c == "." || c == "..") {
            return Err(VaultError::InvalidPath(s.to_owned()));
        }
        Ok(Self(s.to_owned()))
    }

    /// Crea una ruta a partir de un camino relativo del sistema de archivos.
    pub fn from_path(p: &Path) -> Result<Self, VaultError> {
        let mut s = String::new();
        for c in p.components() {
            let std::path::Component::Normal(os) = c else {
                return Err(VaultError::InvalidPath(p.display().to_string()));
            };
            let part = os
                .to_str()
                .ok_or_else(|| VaultError::InvalidPath(p.display().to_string()))?;
            if !s.is_empty() {
                s.push('/');
            }
            s.push_str(part);
        }
        Self::new(&s)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Último componente (nombre del archivo o carpeta).
    pub fn file_name(&self) -> &str {
        self.0.rsplit('/').next().unwrap_or(&self.0)
    }

    /// Primer componente de la ruta.
    pub fn first_component(&self) -> &str {
        self.0.split('/').next().unwrap_or(&self.0)
    }

    /// Ruta absoluta dentro de `root`.
    pub fn to_path(&self, root: &Path) -> PathBuf {
        let mut p = root.to_path_buf();
        for c in self.0.split('/') {
            p.push(c);
        }
        p
    }

    /// Ruta relativa de un hijo.
    pub fn join(&self, name: &str) -> Result<Self, VaultError> {
        Self::new(&format!("{}/{name}", self.0))
    }
}

impl fmt::Display for RelPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Zona de la bóveda donde está una entrada.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Zone {
    /// Dentro de `raw/`: fuentes inmutables.
    Raw,
    /// Dentro de `wiki/`.
    Wiki,
    /// En la raíz de una bóveda con estructura (por ejemplo `AGENTS.md`).
    Root,
    /// En una carpeta sin la estructura de referencia.
    Outside,
}

/// Archivos reservados de OKF.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ReservedKind {
    Index,
    Log,
}

/// Clase de entrada.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EntryKind {
    Folder,
    /// Nota atómica `.md`.
    AtomNote,
    /// Nota de síntesis `.html`.
    SynthesisNote,
    Base,
    Canvas,
    Reserved(ReservedKind),
    /// `AGENTS.md` en la raíz.
    Agents,
    /// Adjuntos y cualquier otro archivo.
    Other,
}

impl EntryKind {
    /// Clasifica un archivo según su nombre y su zona.
    pub fn classify(name: &str, zone: Zone, at_root: bool) -> Self {
        if zone == Zone::Wiki {
            match name {
                "index.md" => return Self::Reserved(ReservedKind::Index),
                "log.md" => return Self::Reserved(ReservedKind::Log),
                _ => {}
            }
        }
        if at_root && zone == Zone::Root && name == "AGENTS.md" {
            return Self::Agents;
        }
        match extension(name) {
            Some(e) if e.eq_ignore_ascii_case("md") => Self::AtomNote,
            Some(e) if e.eq_ignore_ascii_case("html") || e.eq_ignore_ascii_case("htm") => {
                Self::SynthesisNote
            }
            Some(e) if e.eq_ignore_ascii_case("base") => Self::Base,
            Some(e) if e.eq_ignore_ascii_case("canvas") => Self::Canvas,
            _ => Self::Other,
        }
    }

    /// Verdadero si es una nota (`.md` o `.html`), incluidos los reservados y `AGENTS.md`.
    pub fn is_text(self) -> bool {
        matches!(
            self,
            Self::AtomNote
                | Self::SynthesisNote
                | Self::Base
                | Self::Canvas
                | Self::Reserved(_)
                | Self::Agents
        )
    }
}

fn extension(name: &str) -> Option<&str> {
    let (stem, ext) = name.rsplit_once('.')?;
    (!stem.is_empty()).then_some(ext)
}

/// Una entrada del recorrido de la bóveda.
#[derive(Clone, Debug)]
pub struct Entry {
    pub path: RelPath,
    pub kind: EntryKind,
    pub zone: Zone,
    /// En `raw/` siempre es `true`. Para textos de más de 5 MB también. El recorrido no abre
    /// archivos, así que la validez de UTF-8 se comprueba al leer la nota.
    pub read_only: bool,
    pub size: u64,
    pub mtime: Option<SystemTime>,
}

impl Entry {
    /// Calcula `read_only` a partir de la zona y el tamaño.
    pub fn compute_read_only(kind: EntryKind, zone: Zone, size: u64) -> bool {
        zone == Zone::Raw || (kind.is_text() && size > MAX_EDITABLE_BYTES)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rel_path_validation() {
        assert!(RelPath::new("wiki/nota.md").is_ok());
        assert!(RelPath::new("Bóveda/canción.md").is_ok());
        for bad in ["", "/abs", "a//b", "a/../b", "./a", "a/", "a\\b"] {
            assert!(RelPath::new(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn classify() {
        assert_eq!(
            EntryKind::classify("index.md", Zone::Wiki, false),
            EntryKind::Reserved(ReservedKind::Index)
        );
        assert_eq!(
            EntryKind::classify("index.md", Zone::Outside, false),
            EntryKind::AtomNote
        );
        assert_eq!(
            EntryKind::classify("AGENTS.md", Zone::Root, true),
            EntryKind::Agents
        );
        assert_eq!(
            EntryKind::classify("x.HTML", Zone::Wiki, false),
            EntryKind::SynthesisNote
        );
        assert_eq!(
            EntryKind::classify("foto.png", Zone::Wiki, false),
            EntryKind::Other
        );
        assert_eq!(
            EntryKind::classify("md", Zone::Wiki, false),
            EntryKind::Other
        );
    }
}
