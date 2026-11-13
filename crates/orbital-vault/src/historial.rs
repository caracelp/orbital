//! Historial de versiones (010): snapshot automático al guardar en `.orbital/historial/<ruta>/`.
//!
//! Las versiones viven fuera de `wiki/`: nunca son notas ni entran en la búsqueda o el grafo.
//! Límite de 20 versiones por nota (se descartan las más viejas); un guardado que no cambia el
//! contenido no crea versión duplicada.

use crate::write::{atomic_write, hash};
use crate::{RelPath, Vault};
use std::fs;
use std::path::PathBuf;

const MAX_VERSIONES: usize = 20;
const RAIZ: &str = ".orbital/historial";

/// Carpeta del historial de una ruta (mismas subcarpetas que el archivo).
fn carpeta(root: &std::path::Path, ruta: &RelPath) -> PathBuf {
    root.join(RAIZ).join(ruta.as_str())
}

/// Guarda el contenido **previo** al guardado como versión nueva. Si coincide byte a byte con la
/// última versión, no crea duplicado. Devuelve si se escribió una versión.
pub fn guardar_version(
    vault: &Vault,
    ruta: &RelPath,
    texto_previo: &str,
    actor: &str,
) -> Result<bool, crate::VaultError> {
    let root = vault.root();
    let dir = carpeta(root, ruta);
    fs::create_dir_all(&dir)?;
    let contenido = texto_previo.as_bytes();
    // No duplicar: si la última versión es igual, no se crea otra.
    if let Some(ultima) = ultima_version(&dir)?
        && hash(&fs::read(&ultima)?) == hash(contenido)
    {
        return Ok(false);
    }
    // El nombre lleva milisegundos; si aun así colisiona con contenido distinto (dos guardados
    // en el mismo milisegundo), se agrega un sufijo incremental en lugar de pisar la versión.
    let mut destino = dir.join(nombre_version(actor));
    let mut sufijo = 1;
    while let Ok(existente) = fs::read(&destino) {
        if hash(&existente) == hash(contenido) {
            return Ok(false);
        }
        let base = destino
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_owned();
        destino = dir.join(format!("{base}-{sufijo}.txt"));
        sufijo += 1;
    }
    atomic_write(&destino, contenido)?;
    recortar(&dir)?;
    Ok(true)
}

/// Nombre de archivo por marca de tiempo: `AAAA-MM-DD_HH-MM-SS.txt`.
fn nombre_version(actor: &str) -> String {
    let ahora = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let (dias, resto) = (ahora / 86_400_000, ahora % 86_400_000);
    let segs_dia = resto / 1000;
    let millis = resto % 1000;
    // Días desde epoch → fecha civil (algoritmo de Howard Hinnant, dominio público).
    let z = dias as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    let (h, mi, s) = (segs_dia / 3600, (segs_dia % 3600) / 60, segs_dia % 60);
    let actor = if actor.is_empty() {
        String::new()
    } else {
        format!("-{}", actor.replace(['/', '\\', ':'], "_"))
    };
    format!("{y:04}-{m:02}-{d:02}_{h:02}-{mi:02}-{s:02}-{millis:03}{actor}.txt")
}

/// La versión más reciente por nombre (las fechas se ordenan lexicográficamente).
fn ultima_version(dir: &std::path::Path) -> Result<Option<PathBuf>, std::io::Error> {
    Ok(versiones_en(dir)?.last().cloned())
}

/// Lista de archivos de versión en una carpeta, ordenados por fecha de modificación (de viejo a
/// nuevo). El nombre lleva fecha, pero el sufijo anti-colisión puede romper el orden
/// lexicográfico: el mtime es la fuente de verdad.
fn versiones_en(dir: &std::path::Path) -> Result<Vec<PathBuf>, std::io::Error> {
    let mut con_hora: Vec<(std::time::SystemTime, PathBuf)> = Vec::new();
    match fs::read_dir(dir) {
        Ok(entradas) => {
            for e in entradas {
                let e = e?;
                if e.file_type()?.is_file() {
                    con_hora.push((e.metadata()?.modified()?, e.path()));
                }
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e),
    }
    con_hora.sort_by_key(|(hora, _)| *hora);
    Ok(con_hora.into_iter().map(|(_, camino)| camino).collect())
}

/// Descarta las versiones más viejas dejando `MAX_VERSIONES`.
fn recortar(dir: &std::path::Path) -> Result<(), std::io::Error> {
    let mut versiones = versiones_en(dir)?;
    while versiones.len() > MAX_VERSIONES {
        let mas_vieja = versiones.remove(0);
        let _ = fs::remove_file(&mas_vieja);
    }
    Ok(())
}

/// Las versiones de una nota, más recientes primero: `(nombre de archivo, contenido)`.
pub fn versiones(
    vault: &Vault,
    ruta: &RelPath,
) -> Result<Vec<(String, String)>, crate::VaultError> {
    let dir = carpeta(vault.root(), ruta);
    let mut out = Vec::new();
    for camino in versiones_en(&dir)? {
        let nombre = camino
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        let contenido =
            fs::read_to_string(&camino).map_err(|_| crate::VaultError::NotUtf8(ruta.clone()))?;
        out.push((nombre, contenido));
    }
    out.reverse();
    Ok(out)
}

/// Lee una versión por nombre de archivo.
pub fn leer_version(
    vault: &Vault,
    ruta: &RelPath,
    nombre: &str,
) -> Result<String, crate::VaultError> {
    if nombre.contains("..") || nombre.contains('/') || nombre.contains('\\') {
        return Err(crate::VaultError::InvalidPath(nombre.to_owned()));
    }
    let camino = carpeta(vault.root(), ruta).join(nombre);
    fs::read_to_string(&camino).map_err(|_| crate::VaultError::InvalidPath(nombre.to_owned()))
}
