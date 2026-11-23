//! Lo del disco: sumas, recorrido de la bóveda, papelera y reemplazo atómico.

use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use gio::prelude::*;

use md5::{Digest, Md5};

/// MD5 en hexadecimal (el `md5Checksum` de Drive), leyendo por tramos.
pub fn md5_archivo(ruta: &Path) -> io::Result<String> {
    let mut f = File::open(ruta)?;
    let mut h = Md5::new();
    let mut buf = vec![0u8; 256 * 1024];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(hex(&h.finalize()))
}

pub fn md5_bytes(b: &[u8]) -> String {
    hex(&Md5::digest(b))
}

fn hex(b: &[u8]) -> String {
    use std::fmt::Write;
    b.iter()
        .fold(String::with_capacity(b.len() * 2), |mut s, x| {
            let _ = write!(s, "{x:02x}");
            s
        })
}

/// Tamaño y fecha de modificación (ns) de un archivo.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Firma {
    pub tam: u64,
    pub mtime: i128,
}

pub fn firma(m: &fs::Metadata) -> Firma {
    let mtime = m
        .modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_nanos() as i128);
    Firma {
        tam: m.len(),
        mtime,
    }
}

/// Lo que hay en disco en una ruta.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EnDisco {
    Nada,
    Carpeta,
    Archivo(Firma),
}

pub fn en_disco(ruta: &Path) -> EnDisco {
    match fs::symlink_metadata(ruta) {
        Ok(m) if m.is_dir() => EnDisco::Carpeta,
        Ok(m) if m.is_file() => EnDisco::Archivo(firma(&m)),
        _ => EnDisco::Nada,
    }
}

/// Recorre la bóveda: rutas relativas (con `/`) de lo que se sincroniza, con lo que hay.
/// Salta lo oculto (salvo la configuración compartida de `.orbital/`) y lo que `excluida` dice
/// (lo «solo en este equipo»).
pub fn recorrer(root: &Path, excluida: &dyn Fn(&str) -> bool) -> BTreeMap<String, EnDisco> {
    let mut out = BTreeMap::new();
    let mut pila = vec![(root.to_path_buf(), String::new())];
    while let Some((dir, rel)) = pila.pop() {
        let Ok(rd) = fs::read_dir(&dir) else { continue };
        for e in rd.flatten() {
            let Some(nombre) = e.file_name().to_str().map(str::to_owned) else {
                continue;
            };
            let r = if rel.is_empty() {
                nombre.clone()
            } else {
                format!("{rel}/{nombre}")
            };
            let Ok(tipo) = e.file_type() else { continue };
            if nombre == ".orbital" && rel.is_empty() && tipo.is_dir() {
                for n in crate::reglas::ORBITAL_COMPARTIDOS {
                    let rr = format!(".orbital/{n}");
                    if let EnDisco::Archivo(f) = en_disco(&root.join(&rr))
                        && !excluida(&rr)
                    {
                        out.insert(rr, EnDisco::Archivo(f));
                    }
                }
                continue;
            }
            if !crate::reglas::sincronizable(&r) || excluida(&r) {
                continue;
            }
            if tipo.is_dir() {
                out.insert(r.clone(), EnDisco::Carpeta);
                pila.push((e.path(), r));
            } else if tipo.is_file()
                && let Ok(m) = e.metadata()
            {
                out.insert(r, EnDisco::Archivo(firma(&m)));
            }
        }
    }
    out
}

/// Dónde van los archivos que el motor saca de la bóveda.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Papelera {
    /// La papelera del sistema (y, si el disco no tiene, la de la bóveda).
    Sistema,
    /// Siempre `.orbital/papelera/<fecha>/<ruta>` (las pruebas: nunca la papelera de la usuaria).
    Boveda,
}

/// Fecha y hora local `AAAA-MM-DD_HH-MM-SS` (para nombres de carpetas y copias).
pub fn marca_de_tiempo() -> String {
    gio::glib::DateTime::now_local()
        .and_then(|d| d.format("%Y-%m-%d_%H-%M-%S"))
        .map(|s| s.to_string())
        .unwrap_or_else(|_| "sin-fecha".into())
}

/// Saca `rel` de la bóveda sin borrarlo.
pub fn a_papelera(root: &Path, rel: &str, papelera: &Papelera) -> io::Result<()> {
    let origen = root.join(rel);
    if *papelera == Papelera::Sistema
        && gio::File::for_path(&origen)
            .trash(gio::Cancellable::NONE)
            .is_ok()
    {
        return Ok(());
    }
    let destino = libre(
        &root
            .join(".orbital/papelera")
            .join(marca_de_tiempo())
            .join(rel),
    );
    if let Some(p) = destino.parent() {
        fs::create_dir_all(p)?;
    }
    fs::rename(&origen, &destino)
}

/// Un id de Drive como nombre de archivo (lo que no es letra, número, `-` o `_` pasa a `_`).
pub fn id_limpio(id: &str) -> String {
    id.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

/// Una ruta que no existe todavía (agrega ` (2)`, ` (3)`… antes de la extensión).
pub fn libre(ruta: &Path) -> PathBuf {
    if fs::symlink_metadata(ruta).is_err() {
        return ruta.to_path_buf();
    }
    let padre = ruta.parent().unwrap_or(Path::new(""));
    let tallo = ruta
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let ext = ruta
        .extension()
        .map(|e| format!(".{}", e.to_string_lossy()))
        .unwrap_or_default();
    (2..)
        .map(|n| padre.join(format!("{tallo} ({n}){ext}")))
        .find(|p| fs::symlink_metadata(p).is_err())
        .expect("hay una ruta libre")
}

/// Mueve `de` a `a` creando las carpetas que falten. Entre discos distintos (la caché de lo
/// «solo en Drive» puede estar en otro), copia a un temporal junto al destino, lo renombra y
/// recién entonces borra el original.
pub fn mover(de: &Path, a: &Path) -> io::Result<()> {
    if let Some(p) = a.parent() {
        fs::create_dir_all(p)?;
    }
    match fs::rename(de, a) {
        Err(e) if e.raw_os_error() == Some(18) => {
            // EXDEV
            let tmp = a.with_extension("orbital-copiando");
            fs::copy(de, &tmp)?;
            File::open(&tmp)?.sync_all()?;
            fs::rename(&tmp, a)?;
            fs::remove_file(de)
        }
        r => r,
    }
}

/// Copia `de` a `a` (creando las carpetas) pasando por un temporal: `a` nunca queda a medias.
pub fn copiar(de: &Path, a: &Path) -> io::Result<()> {
    if let Some(p) = a.parent() {
        fs::create_dir_all(p)?;
    }
    let tmp = a.with_extension("orbital-copiando");
    fs::copy(de, &tmp)?;
    File::open(&tmp)?.sync_all()?;
    fs::rename(&tmp, a)
}

/// Si `rel` es una nota de texto que se puede fusionar y guardar en el historial.
pub fn es_texto(rel: &str) -> bool {
    let r = rel.to_ascii_lowercase();
    r.ends_with(".md") || r.ends_with(".html") || r.ends_with(".htm")
}

/// Limpia un nombre de Drive para el disco (`/` y NUL no se pueden).
pub fn nombre_local(nombre: &str) -> String {
    let n: String = nombre
        .chars()
        .map(|c| if c == '/' || c == '\0' { '∕' } else { c })
        .collect();
    match n.as_str() {
        "" | "." | ".." => "sin nombre".into(),
        _ => n,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn md5_como_drive() {
        assert_eq!(md5_bytes(b""), "d41d8cd98f00b204e9800998ecf8427e");
        assert_eq!(md5_bytes(b"hola"), "4d186321c1a7f0f354b297e8914ab240");
    }

    #[test]
    fn nombres_imposibles() {
        assert_eq!(nombre_local("a/b"), "a∕b");
        assert_eq!(nombre_local(".."), "sin nombre");
    }

    #[test]
    fn ruta_libre_agrega_numero() {
        let d = std::env::temp_dir().join(format!("orbital-drive-libre-{}", std::process::id()));
        fs::create_dir_all(&d).unwrap();
        fs::write(d.join("a.md"), "x").unwrap();
        assert_eq!(libre(&d.join("a.md")), d.join("a (2).md"));
        assert_eq!(libre(&d.join("b.md")), d.join("b.md"));
        fs::remove_dir_all(&d).unwrap();
    }
}
