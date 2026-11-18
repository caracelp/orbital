//! Definiciones de bases e índice tipado, sin dependencias de interfaz.
mod format;
mod index;
mod properties;
use crate::{RelPath, Vault, VaultError, WriteCtx, WriteReceipt, props::PropValue};
pub use format::{emit, parse, parse_value, validate_value};
pub(crate) use index::compare;
pub use index::{BaseIndex, BaseRow, BaseSnapshot};
pub use properties::{PropertyRenamePlan, edit_cell, plan_property_rename};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum BaseView {
    #[default]
    Table,
    Cards,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PropertyType {
    #[default]
    Auto,
    Text,
    Number,
    Bool,
    Date,
    DateTime,
    Link,
    List,
}
#[derive(Clone, Debug, PartialEq)]
pub struct BaseColumn {
    pub property: String,
    pub label: String,
    pub kind: PropertyType,
}
#[derive(Clone, Debug, PartialEq)]
pub enum BaseFilter {
    Folder(String),
    Tag(String),
    Property {
        key: String,
        value: Option<PropValue>,
    },
}
#[derive(Clone, Debug, PartialEq)]
pub struct BaseSort {
    pub property: String,
    pub descending: bool,
}
#[derive(Clone, Debug, PartialEq)]
pub struct BaseDefinition {
    pub version: u32,
    pub view: BaseView,
    pub columns: Vec<BaseColumn>,
    pub filters: Vec<BaseFilter>,
    pub sort: Option<BaseSort>,
}
impl Default for BaseDefinition {
    fn default() -> Self {
        Self {
            version: 1,
            view: BaseView::Table,
            columns: vec![BaseColumn {
                property: "title".into(),
                label: "Título".into(),
                kind: PropertyType::Auto,
            }],
            filters: vec![],
            sort: None,
        }
    }
}
#[derive(Clone, Debug)]
pub struct BaseDocument {
    pub definition: BaseDefinition,
    pub hash: u64,
}
fn invalid(path: &RelPath, message: String) -> VaultError {
    VaultError::InvalidProps {
        path: path.clone(),
        line: 1,
        message,
    }
}
/// Comprueba también enlaces simbólicos de todos los padres existentes.
pub(crate) fn contained(vault: &Vault, path: &RelPath) -> Result<(), VaultError> {
    let root = vault.root().canonicalize()?;
    let mut current = path.to_path(&root);
    while std::fs::symlink_metadata(&current)
        .is_err_and(|e| e.kind() == std::io::ErrorKind::NotFound)
    {
        if !current.pop() {
            return Err(VaultError::InvalidPath(path.to_string()));
        }
    }
    if !current.canonicalize()?.starts_with(&root) {
        return Err(VaultError::InvalidPath(path.to_string()));
    }
    Ok(())
}
pub fn read(vault: &Vault, path: &RelPath) -> Result<BaseDocument, VaultError> {
    contained(vault, path)?;
    check_extension(path)?;
    use std::io::Read;
    let file = std::fs::File::open(path.to_path(vault.root()))?;
    let mut bytes = Vec::new();
    file.take(crate::entry::MAX_EDITABLE_BYTES + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > crate::entry::MAX_EDITABLE_BYTES {
        return Err(VaultError::TooLarge(path.clone()));
    }
    let text = std::str::from_utf8(&bytes).map_err(|_| VaultError::NotUtf8(path.clone()))?;
    Ok(BaseDocument {
        definition: parse(text).map_err(|e| invalid(path, e))?,
        hash: crate::write::hash(&bytes),
    })
}
pub fn save(
    vault: &Vault,
    path: &RelPath,
    definition: &BaseDefinition,
    expected: Option<u64>,
    ctx: &WriteCtx,
) -> Result<WriteReceipt, VaultError> {
    contained(vault, path)?;
    check_extension(path)?;
    let text = emit(definition);
    parse(&text).map_err(|e| invalid(path, e))?;
    if expected.is_some() {
        read(vault, path)?;
    }
    crate::write::safe_write(vault.root(), path, text.as_bytes(), expected, ctx)
}

fn check_extension(path: &RelPath) -> Result<(), VaultError> {
    if !path.file_name().to_ascii_lowercase().ends_with(".base") {
        Err(VaultError::InvalidPath(path.to_string()))
    } else {
        Ok(())
    }
}

/// URL interna inequívoca: incluso `%`, espacios, `#` y Unicode se conservan.
pub fn path_url(path: &RelPath) -> String {
    use std::fmt::Write;
    let mut url = String::from("orbital-path:");
    for b in path.as_str().bytes() {
        if b.is_ascii_alphanumeric() || b"/-._~".contains(&b) {
            url.push(b as char);
        } else {
            let _ = write!(url, "%{b:02X}");
        }
    }
    url
}
