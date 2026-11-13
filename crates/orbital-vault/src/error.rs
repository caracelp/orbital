//! Errores del núcleo, con mensajes en español listos para la interfaz.

use std::fmt;
use std::io;

use crate::entry::RelPath;

/// Error de cualquier operación sobre la bóveda.
#[derive(Debug)]
pub enum VaultError {
    /// Error de entrada/salida del sistema.
    Io(io::Error),
    /// El archivo no está en UTF-8; solo se puede ver.
    NotUtf8(RelPath),
    /// El archivo de texto supera el tamaño máximo para editarlo.
    TooLarge(RelPath),
    /// El archivo es de solo lectura (por ejemplo, está en `raw/`).
    ReadOnly(RelPath),
    /// El archivo cambió en disco desde que se leyó.
    Conflict { path: RelPath },
    /// Las propiedades de la nota no son YAML válido.
    InvalidProps {
        path: RelPath,
        line: usize,
        message: String,
    },
    /// `index.md` y `log.md` son nombres reservados (OKF §3.1).
    ReservedName(String),
    /// La nota HTML no tiene `<head>`, así que no hay dónde guardar las propiedades.
    NoHead(RelPath),
    /// La ruta es un elemento oculto (empieza con `.`) y Orbital no la maneja.
    Hidden(String),
    /// La ruta no es válida dentro de la bóveda.
    InvalidPath(String),
    /// La conversión del archivo a Markdown falló (015).
    Importar(String),
}

impl fmt::Display for VaultError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => write!(f, "Error de lectura o escritura: {e}"),
            Self::NotUtf8(p) => write!(f, "«{p}» no está en UTF-8; se abre en solo lectura"),
            Self::TooLarge(p) => {
                write!(
                    f,
                    "«{p}» es demasiado grande para editarse; se abre en solo lectura"
                )
            }
            Self::ReadOnly(p) => write!(f, "«{p}» es de solo lectura"),
            Self::Conflict { path } => write!(
                f,
                "«{path}» cambió en el disco desde que se abrió; no se guardó para no pisar el cambio"
            ),
            Self::InvalidProps {
                path,
                line,
                message,
            } => write!(
                f,
                "Las propiedades de «{path}» tienen un error en la línea {line}: {message}"
            ),
            Self::ReservedName(n) => write!(f, "«{n}» es un nombre reservado de la wiki"),
            Self::NoHead(p) => write!(
                f,
                "«{p}» no tiene <head>; no hay dónde guardar las propiedades"
            ),
            Self::Hidden(p) => write!(f, "«{p}» es un elemento oculto"),
            Self::InvalidPath(p) => write!(f, "«{p}» no es una ruta válida dentro de la bóveda"),
            Self::Importar(d) => write!(f, "No se pudo importar a Markdown: {d}"),
        }
    }
}

impl std::error::Error for VaultError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(e) => Some(e),
            _ => None,
        }
    }
}

impl From<io::Error> for VaultError {
    fn from(e: io::Error) -> Self {
        Self::Io(e)
    }
}
