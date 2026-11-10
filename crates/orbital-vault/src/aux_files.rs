//! Archivos auxiliares `.base` y `.canvas` (FR-028, FR-029).
//!
//! Su contenido se define en sus propias especificaciones; aquí solo se exige que sean texto
//! legible (YAML o JSON, que también es YAML válido) con `format_version`, y que un archivo con
//! errores no se sobrescriba sin confirmación.

use std::fs;

use crate::entry::RelPath;
use crate::error::VaultError;
use crate::props::{PropError, PropValue, find, yaml};
use crate::vault::Vault;
use crate::write::{WriteCtx, WriteReceipt, hash, safe_write};

/// Un archivo auxiliar leído y validado.
#[derive(Clone, Debug)]
pub struct AuxFile {
    pub path: RelPath,
    pub source: String,
    pub hash: u64,
    pub format_version: Option<u32>,
    /// Error de sintaxis o falta de `format_version`.
    pub error: Option<PropError>,
}

impl Vault {
    /// Lee y valida un `.base` o `.canvas`.
    pub fn read_aux(&self, path: &RelPath) -> Result<AuxFile, VaultError> {
        let bytes = fs::read(path.to_path(self.root()))?;
        let source = String::from_utf8(bytes).map_err(|_| VaultError::NotUtf8(path.clone()))?;
        let (format_version, error) = match yaml::parse_block(&source) {
            Ok(entries) => match find(&entries, "format_version").map(|e| &e.value) {
                Some(PropValue::Number(n)) => match n.parse::<u32>() {
                    Ok(v) => (Some(v), None),
                    Err(_) => (
                        None,
                        Some(PropError {
                            line: 1,
                            message: format!("«format_version: {n}» no es un número entero"),
                        }),
                    ),
                },
                _ => (
                    None,
                    Some(PropError {
                        line: 1,
                        message: "falta «format_version» (un número entero, por ejemplo 1)".into(),
                    }),
                ),
            },
            Err(e) => (None, Some(e)),
        };
        Ok(AuxFile {
            path: path.clone(),
            hash: hash(source.as_bytes()),
            source,
            format_version,
            error,
        })
    }

    /// Guarda un archivo auxiliar. Si tenía errores, solo con `confirm = true` (el usuario lo
    /// pidió de forma explícita); si no, devuelve el error y no escribe nada.
    pub fn save_aux(
        &self,
        aux: &AuxFile,
        new_source: &str,
        confirm: bool,
        ctx: &WriteCtx,
    ) -> Result<WriteReceipt, VaultError> {
        if let (Some(err), false) = (&aux.error, confirm) {
            return Err(VaultError::InvalidProps {
                path: aux.path.clone(),
                line: err.line,
                message: err.message.clone(),
            });
        }
        safe_write(
            self.root(),
            &aux.path,
            new_source.as_bytes(),
            Some(aux.hash),
            ctx,
        )
    }
}
