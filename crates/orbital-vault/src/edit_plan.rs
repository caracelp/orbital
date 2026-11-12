//! Plan declarativo de cambios multiarchivo, compartido por enlaces y etiquetas.
use crate::write::{hash, safe_write};
use crate::{RelPath, Vault, VaultError, WriteCtx, WriteReceipt};
use std::collections::HashMap;
use std::path::Path;

/// Los cambios de un archivo.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileEdit {
    /// El archivo, con su ruta **antes** del renombre.
    pub path: RelPath,
    /// Ocurrencias reescritas.
    pub count: usize,
    pub(crate) original: String,
    pub(crate) updated: String,
}

impl FileEdit {
    pub fn original(&self) -> &str {
        &self.original
    }
    pub fn updated(&self) -> &str {
        &self.updated
    }
}

/// Lo que habría que cambiar al renombrar `old` a `new`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct EditPlan {
    pub old: Option<RelPath>,
    pub new: Option<RelPath>,
    pub changes: Vec<FileEdit>,
    /// Archivos que no se pudieron inspeccionar al planificar. Nunca ocultar estas omisiones.
    pub errors: Vec<(RelPath, String)>,
    pub(crate) root: Option<std::path::PathBuf>,
}

impl EditPlan {
    /// Total de ocurrencias; nombre conservado por compatibilidad con los planes de enlaces.
    pub fn links(&self) -> usize {
        self.changes.iter().map(|c| c.count).sum()
    }

    /// Archivos que cambian.
    pub fn files(&self) -> usize {
        self.changes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.changes.is_empty()
    }

    /// Dónde está el archivo después del renombre (la nota renombrada puede enlazarse a sí misma).
    pub(crate) fn current(&self, path: &RelPath) -> RelPath {
        match (&self.old, &self.new) {
            (Some(old), Some(new)) if path == old => new.clone(),
            _ => path.clone(),
        }
    }
}

impl EditPlan {
    /// Subplan de lo que ESTA aplicación escribió. Usarlo siempre para Deshacer tras fallos parciales.
    pub fn applied(&self, receipt: &WriteReceipt) -> Self {
        let written: HashMap<_, _> = receipt
            .written
            .iter()
            .map(|(path, hash)| (path, *hash))
            .collect();
        let changes = self
            .changes
            .iter()
            .filter(|change| {
                written.get(&self.current(&change.path)) == Some(&hash(change.updated.as_bytes()))
            })
            .cloned()
            .collect();
        Self {
            changes,
            errors: vec![],
            ..self.clone()
        }
    }
}

/// Comprueba contención incluso si apareció un symlink después de planificar.
pub(crate) fn checked_path(root: &Path, path: &RelPath) -> Result<(), VaultError> {
    let root = root.canonicalize()?;
    let resolved = path.to_path(&root).canonicalize()?;
    if !resolved.starts_with(&root) || !resolved.is_file() {
        return Err(VaultError::InvalidPath(path.to_string()));
    }
    Ok(())
}

impl Vault {
    pub fn apply_edits(
        &self,
        plan: &EditPlan,
        ctx: &WriteCtx,
    ) -> (WriteReceipt, Vec<(RelPath, VaultError)>) {
        self.write_plan(plan, ctx, |change| (&change.original, &change.updated))
    }

    /// Requiere un subplan `plan.applied(receipt)` si apply tuvo fallos parciales.
    pub fn revert_edits(
        &self,
        plan: &EditPlan,
        ctx: &WriteCtx,
    ) -> (WriteReceipt, Vec<(RelPath, VaultError)>) {
        self.write_plan(plan, ctx, |change| (&change.updated, &change.original))
    }
    fn write_plan(
        &self,
        plan: &EditPlan,
        ctx: &WriteCtx,
        pick: impl Fn(&FileEdit) -> (&String, &String),
    ) -> (WriteReceipt, Vec<(RelPath, VaultError)>) {
        let mut receipt = WriteReceipt::default();
        let mut failed = Vec::new();
        for c in &plan.changes {
            let (expected, text) = pick(c);
            let path = plan.current(&c.path);
            let checked =
                checked_path(self.root(), &path).and_then(|()| {
                    if plan.root.as_deref().is_some_and(|root| {
                        self.root().canonicalize().ok().as_deref() != Some(root)
                    }) {
                        return Err(VaultError::InvalidPath(path.to_string()));
                    }
                    safe_write(
                        self.root(),
                        &path,
                        text.as_bytes(),
                        Some(hash(expected.as_bytes())),
                        ctx,
                    )
                });
            match checked {
                Ok(r) => receipt.written.extend(r.written),
                Err(e) => failed.push((path, e)),
            }
        }
        (receipt, failed)
    }
}
