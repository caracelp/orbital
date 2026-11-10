//! La bóveda abierta: estructura, entradas y configuración.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::config::VaultConfig;
use crate::entry::Entry;
use crate::error::VaultError;
use crate::scan;

/// Si existe una parte de la estructura de referencia.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Presence {
    Present,
    Missing,
    /// Existe, pero con otro tipo (por ejemplo, un archivo llamado `wiki`).
    Conflict,
}

/// Qué partes de la estructura de referencia existen.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Layout {
    pub raw: Presence,
    pub wiki: Presence,
    pub index: Presence,
    pub log: Presence,
    pub agents: Presence,
}

impl Layout {
    /// Lee la estructura de `root` sin escribir nada.
    pub fn detect(root: &Path) -> Self {
        Self {
            raw: presence(&root.join("raw"), true),
            wiki: presence(&root.join("wiki"), true),
            index: presence(&root.join("wiki").join("index.md"), false),
            log: presence(&root.join("wiki").join("log.md"), false),
            agents: presence(&root.join("AGENTS.md"), false),
        }
    }

    /// Verdadero si `raw/`, `wiki/`, `wiki/index.md` y `wiki/log.md` existen con su tipo.
    pub fn is_complete(&self) -> bool {
        [self.raw, self.wiki, self.index, self.log]
            .iter()
            .all(|p| *p == Presence::Present)
    }
}

fn presence(path: &Path, want_dir: bool) -> Presence {
    match fs::metadata(path) {
        Ok(m) if m.is_dir() == want_dir => Presence::Present,
        Ok(_) => Presence::Conflict,
        Err(_) => Presence::Missing,
    }
}

/// Una bóveda abierta.
#[derive(Clone, Debug)]
pub struct Vault {
    root: PathBuf,
    layout: Layout,
    entries: Vec<Entry>,
    config: VaultConfig,
    warnings: Vec<String>,
}

/// Abre `root` como bóveda. **No escribe nada** (FR-002): ni `.orbital/` ni ningún otro archivo.
pub fn open(root: &Path) -> Result<Vault, VaultError> {
    let meta = fs::metadata(root)?;
    if !meta.is_dir() {
        return Err(VaultError::Io(io::Error::new(
            io::ErrorKind::NotADirectory,
            format!("«{}» no es una carpeta", root.display()),
        )));
    }
    let scanned = scan::scan(root)?;
    let (config, config_warning) = VaultConfig::load(root);
    let mut warnings: Vec<String> = config_warning.into_iter().collect();
    if scanned.skipped_non_utf8 > 0 {
        warnings.push(format!(
            "{} archivos tienen nombres que no son UTF-8 y no se muestran",
            scanned.skipped_non_utf8
        ));
    }
    Ok(Vault {
        root: root.to_path_buf(),
        layout: Layout::detect(root),
        entries: scanned.entries,
        config,
        warnings,
    })
}

impl Vault {
    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    pub fn layout(&self) -> &Layout {
        &self.layout
    }

    pub fn config(&self) -> &VaultConfig {
        &self.config
    }

    /// Avisos no bloqueantes de la apertura, en español.
    pub fn warnings(&self) -> &[String] {
        &self.warnings
    }
}
