//! Utilidades compartidas por los tests de integración.
#![allow(dead_code)]

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::SystemTime;

use orbital_vault::{Actor, Date, Offset, Time, WriteCtx};

/// Carpeta de fixtures.
pub fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(name)
}

/// Carpeta temporal única que se borra al soltarla.
pub struct TempDir(pub PathBuf);

impl TempDir {
    pub fn new(tag: &str) -> Self {
        static N: AtomicU32 = AtomicU32::new(0);
        let dir = std::env::temp_dir().join(format!(
            "orbital-test-{tag}-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }

    pub fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// Copia recursiva (incluidos los elementos ocultos).
pub fn copy_tree(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    for e in fs::read_dir(from).unwrap() {
        let e = e.unwrap();
        let dest = to.join(e.file_name());
        if e.file_type().unwrap().is_dir() {
            copy_tree(&e.path(), &dest);
        } else {
            fs::copy(e.path(), dest).unwrap();
        }
    }
}

/// Instantánea de cada archivo y carpeta: contenido (para archivos) y fecha de modificación.
pub fn snapshot(root: &Path) -> BTreeMap<PathBuf, (Option<Vec<u8>>, SystemTime)> {
    let mut out = BTreeMap::new();
    fn walk(root: &Path, dir: &Path, out: &mut BTreeMap<PathBuf, (Option<Vec<u8>>, SystemTime)>) {
        for e in fs::read_dir(dir).unwrap() {
            let e = e.unwrap();
            let p = e.path();
            let meta = fs::symlink_metadata(&p).unwrap();
            let rel = p.strip_prefix(root).unwrap().to_path_buf();
            if meta.is_dir() {
                out.insert(rel, (None, meta.modified().unwrap()));
                walk(root, &p, out);
            } else {
                out.insert(rel, (Some(fs::read(&p).unwrap()), meta.modified().unwrap()));
            }
        }
    }
    walk(root, root, &mut out);
    out
}

/// Contexto de escritura fijo para los tests.
pub fn ctx() -> WriteCtx {
    WriteCtx {
        date: Date {
            year: 2026,
            month: 11,
            day: 9,
        },
        time: Time {
            hour: 18,
            minute: 30,
            second: 5,
        },
        utc_offset: Offset { minutes: -180 },
        actor: Actor::human("prueba"),
    }
}
