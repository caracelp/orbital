//! Escritura segura: hash de contenido, escritura atómica, GuardadoSeguro y contexto de escritura.

use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use crate::entry::RelPath;
use crate::error::VaultError;
use crate::props::PropValue;
use crate::props::emit::{self, LineEnding};

/// Hash de contenido (XXH3 de 64 bits). Solo detecta cambios; no es criptográfico (research R7).
pub fn hash(bytes: &[u8]) -> u64 {
    xxhash_rust::xxh3::xxh3_64(bytes)
}

/// Fecha local.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Date {
    pub year: i32,
    pub month: u8,
    pub day: u8,
}

impl fmt::Display for Date {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:04}-{:02}-{:02}", self.year, self.month, self.day)
    }
}

/// Hora local.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Time {
    pub hour: u8,
    pub minute: u8,
    pub second: u8,
}

impl fmt::Display for Time {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:02}:{:02}:{:02}", self.hour, self.minute, self.second)
    }
}

/// Desplazamiento respecto de UTC, en minutos.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Offset {
    pub minutes: i32,
}

impl fmt::Display for Offset {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.minutes == 0 {
            return f.write_str("Z");
        }
        let sign = if self.minutes < 0 { '-' } else { '+' };
        let m = self.minutes.unsigned_abs();
        write!(f, "{sign}{:02}:{:02}", m / 60, m % 60)
    }
}

/// Actor según la convención de OKF §7 (por ejemplo `human:ejemplo`).
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Actor(pub String);

impl Actor {
    /// Actor humano: `human:<id>`.
    pub fn human(id: &str) -> Self {
        Self(format!("human:{id}"))
    }
}

/// Todo lo que una escritura necesita saber del mundo. El núcleo nunca lee el reloj (research R10).
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct WriteCtx {
    pub date: Date,
    pub time: Time,
    pub utc_offset: Offset,
    pub actor: Actor,
}

impl WriteCtx {
    /// Fecha y hora ISO 8601 con desplazamiento, como exige OKF §5.
    pub fn timestamp(&self) -> String {
        format!("{}T{}{}", self.date, self.time, self.utc_offset)
    }
}

/// Archivos que una operación escribió, con el hash de lo escrito. La app los usa para ignorar
/// sus propios cambios en el monitor de archivos (research R5).
#[derive(Clone, Default, PartialEq, Eq, Debug)]
pub struct WriteReceipt {
    pub written: Vec<(RelPath, u64)>,
}

/// Resultado de una escritura atómica.
#[derive(Clone, Default, PartialEq, Eq, Debug)]
pub struct AtomicOutcome {
    /// Temporal huérfano de una escritura interrumpida que se encontró (y no se borró).
    pub orphan: Option<PathBuf>,
}

/// Escribe `bytes` en `path` de forma atómica (research R6): temporal oculto en la misma carpeta,
/// `sync_all`, permisos del original, `rename` y `sync_all` de la carpeta.
pub fn atomic_write(path: &Path, bytes: &[u8]) -> io::Result<AtomicOutcome> {
    let dir = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "nombre de archivo inválido"))?;
    let base_tmp = dir.join(format!(".{name}.orbital-tmp"));
    let mut outcome = AtomicOutcome::default();

    let (tmp_path, mut file) = match OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&base_tmp)
    {
        Ok(f) => (base_tmp, f),
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {
            // Un temporal huérfano de un corte anterior: se informa y se usa otro nombre.
            outcome.orphan = Some(base_tmp);
            let mut n = 1u32;
            loop {
                let candidate = dir.join(format!(".{name}.orbital-tmp-{}-{n}", std::process::id()));
                match OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(&candidate)
                {
                    Ok(f) => break (candidate, f),
                    Err(e) if e.kind() == io::ErrorKind::AlreadyExists && n < 1000 => n += 1,
                    Err(e) => return Err(e),
                }
            }
        }
        Err(e) => return Err(e),
    };

    let result = (|| {
        file.write_all(bytes)?;
        file.sync_all()?;
        if let Ok(meta) = fs::metadata(path) {
            fs::set_permissions(&tmp_path, meta.permissions())?;
        }
        drop(file);
        fs::rename(&tmp_path, path)?;
        File::open(dir)?.sync_all()
    })();
    if result.is_err() {
        let _ = fs::remove_file(&tmp_path);
    }
    result.map(|()| outcome)
}

/// Carpeta propia de Orbital dentro de la bóveda.
pub const ORBITAL_DIR: &str = ".orbital";

/// Guarda una copia del contenido anterior en `.orbital/respaldo/` (GuardadoSeguro, research R8).
/// Crea `.orbital/` si no existía. Devuelve la ruta de la copia.
pub fn backup(
    root: &Path,
    rel: &RelPath,
    old: &[u8],
    new_hash: u64,
    ctx: &WriteCtx,
) -> io::Result<PathBuf> {
    let day_dir = root
        .join(ORBITAL_DIR)
        .join("respaldo")
        .join(ctx.date.to_string());
    let rel_path = rel.to_path(&day_dir);
    let parent = rel_path.parent().unwrap_or(&day_dir).to_path_buf();
    fs::create_dir_all(&parent)?;

    let name = rel.file_name();
    let (stem, ext) = match name.rsplit_once('.') {
        Some((s, e)) if !s.is_empty() => (s, Some(e)),
        _ => (name, None),
    };
    let stamp = format!(
        "{:02}{:02}{:02}",
        ctx.time.hour, ctx.time.minute, ctx.time.second
    );
    let mut n = 0u32;
    let copy = loop {
        let suffix = if n == 0 {
            stamp.clone()
        } else {
            format!("{stamp}-{n}")
        };
        let file_name = match ext {
            Some(e) => format!("{stem}.{suffix}.{e}"),
            None => format!("{stem}.{suffix}"),
        };
        let candidate = parent.join(file_name);
        if !candidate.exists() {
            break candidate;
        }
        n += 1;
    };
    atomic_write(&copy, old)?;

    let meta = emit::document(
        &[
            ("original_path", PropValue::Text(rel.to_string())),
            ("saved_at", PropValue::DateTime(ctx.timestamp())),
            (
                "hash_before",
                PropValue::Text(format!("{:016x}", hash(old))),
            ),
            ("hash_after", PropValue::Text(format!("{new_hash:016x}"))),
        ],
        LineEnding::Lf,
    );
    let mut meta_path = copy.clone().into_os_string();
    meta_path.push(".yaml");
    atomic_write(Path::new(&meta_path), meta.as_bytes())?;
    Ok(copy)
}

/// Escribe un archivo de la bóveda con todas las garantías del principio VI:
///
/// - dentro de `raw/` o en elementos ocultos devuelve error;
/// - si el archivo cambió en disco desde que se leyó (`expected_hash` no coincide) devuelve
///   `Conflict` y no escribe; `expected_hash = None` significa «archivo nuevo» y falla si ya existe;
/// - si el contenido es el mismo no escribe nada;
/// - si cambió, primero guarda un respaldo y después escribe de forma atómica.
pub fn safe_write(
    root: &Path,
    rel: &RelPath,
    new_bytes: &[u8],
    expected_hash: Option<u64>,
    ctx: &WriteCtx,
) -> Result<WriteReceipt, VaultError> {
    if rel.first_component() == "raw" {
        return Err(VaultError::ReadOnly(rel.clone()));
    }
    if rel.as_str().split('/').any(|c| c.starts_with('.')) {
        return Err(VaultError::Hidden(rel.to_string()));
    }
    let path = rel.to_path(root);
    let new_hash = hash(new_bytes);
    match fs::read(&path) {
        Ok(current) => {
            let current_hash = hash(&current);
            if expected_hash != Some(current_hash) {
                return Err(VaultError::Conflict { path: rel.clone() });
            }
            if current_hash == new_hash && current == new_bytes {
                return Ok(WriteReceipt::default());
            }
            backup(root, rel, &current, new_hash, ctx)?;
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            if expected_hash.is_some() {
                return Err(VaultError::Conflict { path: rel.clone() });
            }
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)?;
            }
        }
        Err(e) => return Err(e.into()),
    }
    atomic_write(&path, new_bytes)?;
    Ok(WriteReceipt {
        written: vec![(rel.clone(), new_hash)],
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn offset_display() {
        assert_eq!(Offset { minutes: 0 }.to_string(), "Z");
        assert_eq!(Offset { minutes: -180 }.to_string(), "-03:00");
        assert_eq!(Offset { minutes: 330 }.to_string(), "+05:30");
    }
}
