//! La bóveda abierta: estructura, entradas y configuración.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::config::VaultConfig;
use crate::entry::{Entry, RelPath};
use crate::error::VaultError;
use crate::scan;
use crate::write::{WriteCtx, safe_write};

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
    /// Notas de `wiki/` por nombre sin extensión en minúsculas (índices en `entries`). Se arma la
    /// primera vez que se resuelve un wikilink: sin él, resolver era recorrer toda la bóveda por
    /// enlace (el grafo de 5000 notas tardaba 15 s).
    names: std::sync::OnceLock<std::collections::HashMap<String, Vec<usize>>>,
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
        names: std::sync::OnceLock::new(),
    })
}

impl Vault {
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Las notas de `wiki/` cuyo nombre sin extensión es `stem` (en minúsculas).
    pub(crate) fn notes_named(&self, stem: &str) -> impl Iterator<Item = &Entry> {
        let names = self.names.get_or_init(|| {
            let mut m: std::collections::HashMap<String, Vec<usize>> =
                std::collections::HashMap::new();
            for (n, e) in self.entries.iter().enumerate() {
                if e.zone == crate::entry::Zone::Wiki
                    && matches!(
                        e.kind,
                        crate::entry::EntryKind::AtomNote | crate::entry::EntryKind::SynthesisNote
                    )
                {
                    let name = e.path.file_name().to_lowercase();
                    let key = name.rsplit_once('.').map_or(name.as_str(), |(s, _)| s);
                    m.entry(key.to_owned()).or_default().push(n);
                }
            }
            m
        });
        names
            .get(stem)
            .map_or(&[][..], Vec::as_slice)
            .iter()
            .map(|&n| &self.entries[n])
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

/// Qué se crea en un paso de «Abrir como».
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CreationKind {
    Folder,
    File,
}

/// Un elemento que falta de la estructura de referencia.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Creation {
    pub path: RelPath,
    pub kind: CreationKind,
}

/// Lo que «Abrir como bóveda de Orbital» va a crear, y lo que no puede tocar.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct OpenAsPlan {
    /// En orden de creación: carpetas antes que sus archivos.
    pub creations: Vec<Creation>,
    /// Elementos que existen con otro tipo (por ejemplo, un archivo llamado `wiki`).
    pub conflicts: Vec<RelPath>,
}

/// Contenido inicial de `wiki/log.md` (OKF §9).
fn initial_log(ctx: &WriteCtx) -> String {
    format!(
        "# Registro de la wiki\n\n## {}\n* **Creación**: Estructura de la bóveda\n",
        ctx.date
    )
}

fn rel(s: &str) -> RelPath {
    RelPath::new(s).expect("ruta fija válida")
}

/// Calcula lo que falta de la estructura de referencia. **No escribe nada.**
pub fn plan_open_as(root: &Path) -> Result<OpenAsPlan, VaultError> {
    if !fs::metadata(root)?.is_dir() {
        return Err(VaultError::Io(io::Error::new(
            io::ErrorKind::NotADirectory,
            format!("«{}» no es una carpeta", root.display()),
        )));
    }
    let layout = Layout::detect(root);
    let mut plan = OpenAsPlan::default();
    let mut add = |presence: Presence, path: &str, kind: CreationKind| match presence {
        Presence::Missing => plan.creations.push(Creation {
            path: rel(path),
            kind,
        }),
        Presence::Conflict => plan.conflicts.push(rel(path)),
        Presence::Present => {}
    };
    add(layout.raw, "raw", CreationKind::Folder);
    add(layout.wiki, "wiki", CreationKind::Folder);
    if layout.wiki != Presence::Conflict {
        add(layout.index, "wiki/index.md", CreationKind::File);
        add(layout.log, "wiki/log.md", CreationKind::File);
    }
    Ok(plan)
}

/// Crea solo lo planificado y abre la bóveda. Lo que haya aparecido mientras tanto no se toca.
pub fn apply_open_as(root: &Path, plan: &OpenAsPlan, ctx: &WriteCtx) -> Result<Vault, VaultError> {
    for c in &plan.creations {
        let path = c.path.to_path(root);
        match c.kind {
            CreationKind::Folder => match fs::create_dir(&path) {
                Ok(()) => {}
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
                Err(e) => return Err(e.into()),
            },
            CreationKind::File => {
                let content = match c.path.as_str() {
                    "wiki/log.md" => initial_log(ctx),
                    // index.md vacío: en OKF §8 cada `#` abre una sección.
                    _ => String::new(),
                };
                match safe_write(root, &c.path, content.as_bytes(), None, ctx) {
                    Ok(_) | Err(VaultError::Conflict { .. }) => {}
                    Err(e) => return Err(e),
                }
            }
        }
    }
    open(root)
}

/// Un cambio hecho por otro programa (FR-030).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ExternalChange {
    Added(RelPath),
    Modified(RelPath),
    Removed(RelPath),
}

impl Vault {
    /// Compara `paths` (rutas absolutas avisadas por el monitor de archivos) con lo que se leyó
    /// al abrir la bóveda. Ignora los elementos ocultos (`.orbital/`, temporales) y lo que no
    /// cambió: así, las escrituras propias no se reportan una vez que la bóveda se volvió a leer.
    pub fn on_external_change(&self, paths: &[PathBuf]) -> Vec<ExternalChange> {
        let mut out = Vec::new();
        for abs in paths {
            let Ok(rel_path) = abs.strip_prefix(&self.root) else {
                continue;
            };
            let Ok(rel) = RelPath::from_path(rel_path) else {
                continue;
            };
            if rel.as_str().split('/').any(|c| c.starts_with('.')) {
                continue;
            }
            let known = self.entries.iter().find(|e| e.path == rel);
            let change = match (fs::metadata(abs), known) {
                (Err(_), Some(_)) => Some(ExternalChange::Removed(rel)),
                (Err(_), None) => None,
                (Ok(_), None) => Some(ExternalChange::Added(rel)),
                (Ok(m), Some(e)) if m.is_dir() => {
                    (e.kind != crate::EntryKind::Folder).then_some(ExternalChange::Modified(rel))
                }
                (Ok(m), Some(e)) => (m.len() != e.size || m.modified().ok() != e.mtime)
                    .then_some(ExternalChange::Modified(rel)),
            };
            if let Some(c) = change
                && !out.contains(&c)
            {
                out.push(c);
            }
        }
        out
    }
}
