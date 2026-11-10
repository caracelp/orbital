//! Recorrido de la bóveda (research R4).
//!
//! Solo lee entradas de directorio: nunca abre archivos. Los elementos ocultos (nombres que
//! empiezan con `.`) no se listan ni se recorren (FR-008). Los enlaces simbólicos a carpetas no se
//! siguen, para evitar ciclos.

use std::fs;
use std::io;
use std::path::Path;

use crate::entry::{Entry, EntryKind, RelPath, Zone};

/// Resultado del recorrido.
#[derive(Clone, Debug, Default)]
pub struct ScanResult {
    /// Entradas ordenadas por ruta.
    pub entries: Vec<Entry>,
    /// Nombres que no son UTF-8 válido y no se pudieron representar.
    pub skipped_non_utf8: usize,
}

/// Recorre `root` completo.
pub fn scan(root: &Path) -> io::Result<ScanResult> {
    let structured = root.join("raw").is_dir() || root.join("wiki").is_dir();
    let mut out = ScanResult::default();
    walk(root, None, structured, &mut out)?;
    out.entries.sort_unstable_by(|a, b| a.path.cmp(&b.path));
    Ok(out)
}

fn walk(
    dir: &Path,
    rel_dir: Option<&RelPath>,
    structured: bool,
    out: &mut ScanResult,
) -> io::Result<()> {
    for item in fs::read_dir(dir)? {
        let item = item?;
        let os_name = item.file_name();
        let Some(name) = os_name.to_str() else {
            out.skipped_non_utf8 += 1;
            continue;
        };
        if name.starts_with('.') {
            continue;
        }
        let Ok(rel) = (match rel_dir {
            Some(parent) => parent.join(name),
            None => RelPath::new(name),
        }) else {
            continue;
        };
        let zone = zone_of(&rel, structured);
        let file_type = item.file_type()?;
        if file_type.is_dir() {
            out.entries.push(Entry {
                path: rel.clone(),
                kind: EntryKind::Folder,
                zone,
                read_only: zone == Zone::Raw,
                size: 0,
                mtime: None,
            });
            walk(&item.path(), Some(&rel), structured, out)?;
            continue;
        }
        // Archivos comunes y enlaces simbólicos a archivos (se siguen para leer su tamaño).
        let Ok(meta) = fs::metadata(item.path()) else {
            continue;
        };
        if meta.is_dir() {
            // Enlace simbólico a una carpeta: no se recorre.
            continue;
        }
        let at_root = rel_dir.is_none();
        let kind = EntryKind::classify(name, zone, at_root);
        let size = meta.len();
        out.entries.push(Entry {
            read_only: Entry::compute_read_only(kind, zone, size),
            path: rel,
            kind,
            zone,
            size,
            mtime: meta.modified().ok(),
        });
    }
    Ok(())
}

/// Zona de una ruta según su primer componente.
pub fn zone_of(rel: &RelPath, structured: bool) -> Zone {
    if !structured {
        return Zone::Outside;
    }
    match rel.first_component() {
        "raw" => Zone::Raw,
        "wiki" => Zone::Wiki,
        _ => Zone::Root,
    }
}
