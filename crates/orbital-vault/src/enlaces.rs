//! Archivos externos por URL (009): asociación `ruta → url` en `.orbital/enlaces.yaml`.
//!
//! Mismo trato que `busquedas.yaml`: legible, escritura atómica con respaldo, snapshot con hash
//! para detectar cambios externos, y migración de la clave al renombrar el archivo.

use crate::props::emit;
use std::{
    fs, io,
    path::{Path, PathBuf},
};

use crate::write::{atomic_write, backup, hash};
use crate::{RelPath, Vault, VaultError, WriteCtx};
use saphyr_parser::{Event, Parser, ScalarStyle};

const ENLACES_PATH: &str = ".orbital/enlaces.yaml";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Enlace {
    pub ruta: String,
    pub url: String,
}

fn invalid(message: impl Into<String>) -> VaultError {
    VaultError::InvalidProps {
        path: RelPath::from_static(ENLACES_PATH),
        line: 1,
        message: message.into(),
    }
}

fn parse(text: &str) -> Result<Vec<Enlace>, VaultError> {
    // El YAML es una lista de mapas de dos escalares (ruta, url): los escalares en orden dan los
    // pares, igual que el serializador los escribe.
    let mut parser = Parser::new_from_str(text);
    let mut escalares: Vec<String> = Vec::new();
    let mut error = None;
    while let Some(evento) = parser.next_event() {
        match evento {
            Ok((Event::Scalar(v, style, _anchor, _tag), _span)) => {
                let v = match style {
                    ScalarStyle::SingleQuoted | ScalarStyle::DoubleQuoted => v.replace("''", "'"),
                    _ => v.to_string(),
                };
                escalares.push(v);
            }
            Ok(_) => {}
            Err(e) => {
                error = Some(invalid(format!(
                    "Línea {}: {}",
                    e.marker().line(),
                    e.info()
                )));
                break;
            }
        }
    }
    if let Some(e) = error {
        return Err(e);
    }
    let mut out = Vec::new();
    let mut i = 0;
    while i + 3 < escalares.len() {
        if escalares[i].trim() == "ruta"
            && escalares[i + 2].trim() == "url"
            && !escalares[i + 1].trim().is_empty()
            && !escalares[i + 3].trim().is_empty()
        {
            out.push(Enlace {
                ruta: escalares[i + 1].trim().to_owned(),
                url: escalares[i + 3].trim().to_owned(),
            });
            i += 4;
        } else {
            i += 1;
        }
    }
    for e in &out {
        if !e.ruta.starts_with("raw/") && !e.ruta.starts_with("wiki/") {
            return Err(invalid(format!(
                "La ruta «{}» no es un archivo de la bóveda",
                e.ruta
            )));
        }
        if !e.url.starts_with("http://") && !e.url.starts_with("https://") {
            return Err(invalid(format!("La URL «{}» no es http ni https", e.url)));
        }
    }
    Ok(out)
}

fn serialize(enlaces: &[Enlace]) -> String {
    let mut out = String::new();
    for e in enlaces {
        let ruta = emit::scalar(&e.ruta);
        let url = emit::scalar(&e.url);
        out.push_str(&format!("- ruta: {ruta}\n  url: {url}\n"));
    }
    out
}

fn validate(enlaces: &[Enlace]) -> Result<(), VaultError> {
    let mut rutas = std::collections::HashSet::new();
    for e in enlaces {
        if !rutas.insert(e.ruta.clone()) {
            return Err(invalid(format!("La ruta «{}» está repetida", e.ruta)));
        }
        if !e.url.starts_with("http://") && !e.url.starts_with("https://") {
            return Err(invalid(format!("La URL «{}» no es http ni https", e.url)));
        }
    }
    Ok(())
}

fn read_config(root: &Path) -> Result<Option<Vec<u8>>, io::Error> {
    match fs::read(root.join(ENLACES_PATH)) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e),
    }
}

/// Snapshot de lectura: mapa, hash del archivo mostrado y aviso si el YAML está corrupto.
#[derive(Clone, Debug)]
pub struct EnlacesSnapshot {
    pub enlaces: Vec<Enlace>,
    pub hash: Option<u64>,
    pub warning: Option<String>,
    root: Option<PathBuf>,
    writable: bool,
}

impl Vault {
    pub fn enlaces_snapshot(&self) -> EnlacesSnapshot {
        let root = self.root().canonicalize().ok();
        let mut snapshot = EnlacesSnapshot {
            enlaces: vec![],
            hash: None,
            warning: None,
            root: root.clone(),
            writable: false,
        };
        let carga = (|| -> Result<(), VaultError> {
            if let Some(bytes) = read_config(&root.unwrap_or_default())? {
                snapshot.hash = Some(hash(&bytes));
                let text = std::str::from_utf8(&bytes)
                    .map_err(|_| VaultError::NotUtf8(RelPath::from_static(ENLACES_PATH)))?;
                snapshot.enlaces = parse(text)?;
            }
            Ok(())
        })();
        match carga {
            Ok(()) => snapshot.writable = snapshot.root.is_some(),
            Err(error) => {
                snapshot.warning = Some(format!(
                    "No se cargaron las URL asociadas: {error}. El archivo no se sobreescribirá."
                ))
            }
        }
        snapshot
    }

    /// La URL asociada a una ruta, si la hay.
    pub fn url_de(&self, ruta: &RelPath) -> Option<String> {
        self.enlaces_snapshot()
            .enlaces
            .iter()
            .find(|e| e.ruta == ruta.as_str())
            .map(|e| e.url.clone())
    }

    /// Asocia o reemplaza la URL de una ruta (FR-2: solo http/https).
    pub fn asociar_url(&self, ruta: &RelPath, url: &str, ctx: &WriteCtx) -> Result<(), VaultError> {
        if !url.starts_with("http://") && !url.starts_with("https://") {
            return Err(invalid("La URL debe ser http o https"));
        }
        let baseline = self.enlaces_snapshot();
        let mut enlaces = baseline.enlaces.clone();
        enlaces.retain(|e| e.ruta != ruta.as_str());
        enlaces.push(Enlace {
            ruta: ruta.as_str().to_owned(),
            url: url.to_owned(),
        });
        enlaces.sort_by(|a, b| a.ruta.cmp(&b.ruta));
        self.guardar_enlaces(enlaces, &baseline, ctx)
    }

    /// Quita la asociación de una ruta; si no estaba, no hace nada.
    pub fn quitar_url(&self, ruta: &RelPath, ctx: &WriteCtx) -> Result<(), VaultError> {
        let baseline = self.enlaces_snapshot();
        let mut enlaces = baseline.enlaces.clone();
        enlaces.retain(|e| e.ruta != ruta.as_str());
        if enlaces.len() == baseline.enlaces.len() {
            return Ok(());
        }
        self.guardar_enlaces(enlaces, &baseline, ctx)
    }

    /// Migra la asociación al renombrar/mover el archivo (FR-3).
    pub fn migrar_enlace(
        &self,
        vieja: &RelPath,
        nueva: &RelPath,
        ctx: &WriteCtx,
    ) -> Result<bool, VaultError> {
        let baseline = self.enlaces_snapshot();
        let Some(url) = baseline
            .enlaces
            .iter()
            .find(|e| e.ruta == vieja.as_str())
            .map(|e| e.url.clone())
        else {
            return Ok(false);
        };
        let mut enlaces = baseline.enlaces.clone();
        enlaces.retain(|e| e.ruta != vieja.as_str());
        enlaces.push(Enlace {
            ruta: nueva.as_str().to_owned(),
            url,
        });
        enlaces.sort_by(|a, b| a.ruta.cmp(&b.ruta));
        self.guardar_enlaces(enlaces, &baseline, ctx)?;
        Ok(true)
    }

    fn guardar_enlaces(
        &self,
        enlaces: Vec<Enlace>,
        baseline: &EnlacesSnapshot,
        ctx: &WriteCtx,
    ) -> Result<(), VaultError> {
        let root = self.root().canonicalize()?;
        if !baseline.writable || baseline.root.as_deref() != Some(&root) {
            return Err(invalid("El snapshot de URL no es válido para esta bóveda"));
        }
        validate(&enlaces)?;
        let before = read_config(&root)?;
        if before.as_deref().map(hash) != baseline.hash {
            return Err(VaultError::Conflict {
                path: RelPath::from_static(ENLACES_PATH),
            });
        }
        let output = serialize(&enlaces);
        if output.len() as u64 > crate::entry::MAX_EDITABLE_BYTES {
            return Err(VaultError::TooLarge(RelPath::from_static(ENLACES_PATH)));
        }
        if before.as_deref() != Some(output.as_bytes()) {
            check_directories(&root, &[".orbital"])?;
            fs::create_dir_all(root.join(".orbital"))?;
            if let Some(bytes) = before {
                let date = ctx.date.to_string();
                check_directories(&root, &[".orbital", "respaldo", &date, ".orbital"])?;
                backup(
                    &root,
                    &RelPath::from_static(ENLACES_PATH),
                    &bytes,
                    hash(output.as_bytes()),
                    ctx,
                )?;
            }
            atomic_write(&root.join(ENLACES_PATH), output.as_bytes())?;
        }
        Ok(())
    }
}

fn check_directories(root: &Path, parts: &[&str]) -> Result<(), VaultError> {
    let mut path = root.to_path_buf();
    for part in parts {
        path.push(part);
        match fs::symlink_metadata(&path) {
            Ok(meta) if !meta.file_type().is_symlink() && meta.is_dir() => {}
            Ok(_) => return Err(VaultError::InvalidPath(path.display().to_string())),
            Err(error) if error.kind() == io::ErrorKind::NotFound => break,
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_roundtrip() {
        let enlaces = vec![
            Enlace {
                ruta: "raw/libro.pdf".into(),
                url: "https://ejemplo.com/x".into(),
            },
            Enlace {
                ruta: "raw/f.x".into(),
                url: "http://otro.org/".into(),
            },
        ];
        let texto = serialize(&enlaces);
        assert_eq!(parse(&texto).unwrap(), enlaces);
        // Comillas y caracteres especiales sobreviven el roundtrip.
        let raras = vec![Enlace {
            ruta: "raw/con 'comilla'.pdf".into(),
            url: "https://ejemplo.com/a=b&c#d".into(),
        }];
        assert_eq!(parse(&serialize(&raras)).unwrap(), raras);
    }

    #[test]
    fn rechaza_url_sin_esquema() {
        let texto = serialize(&[Enlace {
            ruta: "raw/x".into(),
            url: "ftp://nada".into(),
        }]);
        assert!(parse(&texto).is_err());
    }
}
