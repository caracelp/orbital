//! Búsquedas guardadas: ruta fija, snapshot de contenido y escritura atómica con respaldo.
use crate::props::{PropValue, emit, yaml};
use crate::write::{atomic_write, backup, hash};
use crate::{RelPath, Vault, VaultError, WriteCtx};
use saphyr_parser::{Event, Parser, ScalarStyle};
use std::{
    collections::HashSet,
    fs, io,
    path::{Path, PathBuf},
};

const SAVED_PATH: &str = ".orbital/busquedas.yaml";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SavedSearch {
    pub name: String,
    pub query: String,
}

/// El hash pertenece al archivo que se mostró al usuario, no a una relectura al pulsar Guardar.
#[derive(Clone, Debug)]
pub struct SavedSearchSnapshot {
    pub searches: Vec<SavedSearch>,
    pub hash: Option<u64>,
    pub warning: Option<String>,
    root: Option<PathBuf>,
    writable: bool,
}

fn invalid(message: impl Into<String>) -> VaultError {
    VaultError::InvalidProps {
        path: RelPath::from_static(SAVED_PATH),
        line: 1,
        message: message.into(),
    }
}

fn validate(searches: &[SavedSearch]) -> Result<(), VaultError> {
    let mut names = HashSet::new();
    for search in searches {
        if search.name.trim().is_empty() || search.query.trim().is_empty() {
            return Err(invalid("Cada búsqueda necesita nombre y consulta"));
        }
        if !names.insert(search.name.as_str()) {
            return Err(invalid("Ya existe una búsqueda con ese nombre"));
        }
    }
    Ok(())
}

fn parse(text: &str) -> Result<Vec<SavedSearch>, String> {
    let mut parser = Parser::new_from_str(text);
    let mut searches = Vec::new();
    let (mut document, mut sequence, mut complete, mut mapping) = (false, false, false, false);
    let mut key: Option<String> = None;
    let (mut name, mut query) = (None, None);
    while let Some(event) = parser.next_event() {
        let (event, span) = event.map_err(|error| {
            format!(
                "Línea {}: {}",
                error.marker().line(),
                yaml::translate(error.info())
            )
        })?;
        let error = || {
            format!(
                "Formato de búsquedas inválido en línea {}: se espera una lista de pares name/query",
                span.start.line()
            )
        };
        match event {
            Event::StreamStart | Event::Nothing => {}
            Event::DocumentStart(_) if !document => document = true,
            Event::SequenceStart(0, None) if document && !sequence && !complete && !mapping => {
                sequence = true
            }
            Event::MappingStart(0, None) if sequence && !mapping => {
                mapping = true;
                name = None;
                query = None;
            }
            Event::Scalar(value, style, 0, None) if mapping => {
                if let Some(field) = key.take() {
                    if style == ScalarStyle::Plain
                        && !matches!(
                            yaml::scalar_value(&value, style),
                            PropValue::Text(_) | PropValue::Link(_)
                        )
                    {
                        return Err(error());
                    }
                    let target = match field.as_str() {
                        "name" => &mut name,
                        "query" => &mut query,
                        _ => return Err(error()),
                    };
                    if target.is_some() {
                        return Err(error());
                    }
                    *target = Some(value.into_owned());
                } else {
                    if !matches!(value.as_ref(), "name" | "query") {
                        return Err(error());
                    }
                    key = Some(value.into_owned());
                }
            }
            Event::MappingEnd if mapping && key.is_none() => {
                searches.push(SavedSearch {
                    name: name.take().ok_or_else(error)?,
                    query: query.take().ok_or_else(error)?,
                });
                mapping = false;
            }
            Event::SequenceEnd if sequence && !mapping => {
                sequence = false;
                complete = true;
            }
            Event::DocumentEnd if complete => {}
            Event::StreamEnd if complete => break,
            _ => return Err(error()),
        }
    }
    if !complete {
        return Err("El archivo de búsquedas debe contener una lista YAML".into());
    }
    validate(&searches).map_err(|error| error.to_string())?;
    Ok(searches)
}

fn serialize(searches: &[SavedSearch]) -> String {
    if searches.is_empty() {
        return "[]\n".into();
    }
    let mut output = String::new();
    for search in searches {
        output.push_str(&format!(
            "- name: {}\n  query: {}\n",
            emit::scalar(&search.name),
            emit::scalar(&search.query)
        ));
    }
    output
}

/// Solo para esta ruta de configuración. No relaja la prohibición genérica de escribir ocultos.
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

fn read_config(root: &Path) -> Result<Option<Vec<u8>>, VaultError> {
    check_directories(root, &[".orbital"])?;
    let path = root.join(SAVED_PATH);
    match fs::symlink_metadata(&path) {
        Ok(meta) if meta.file_type().is_symlink() || !meta.is_file() => {
            return Err(VaultError::InvalidPath(SAVED_PATH.into()));
        }
        Ok(meta) if meta.len() > crate::entry::MAX_EDITABLE_BYTES => {
            return Err(VaultError::TooLarge(RelPath::from_static(SAVED_PATH)));
        }
        Ok(_) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    }
    fs::read(path).map(Some).map_err(Into::into)
}

impl Vault {
    pub fn saved_searches_snapshot(&self) -> SavedSearchSnapshot {
        let root = self.root().canonicalize().ok();
        let mut snapshot = SavedSearchSnapshot {
            searches: vec![],
            hash: None,
            warning: None,
            root,
            writable: false,
        };
        let loaded = read_config(self.root()).and_then(|bytes| {
            if let Some(bytes) = bytes {
                snapshot.hash = Some(hash(&bytes));
                let text = std::str::from_utf8(&bytes)
                    .map_err(|_| VaultError::NotUtf8(RelPath::from_static(SAVED_PATH)))?;
                snapshot.searches = parse(text).map_err(invalid)?;
            }
            Ok(())
        });
        match loaded {
            Ok(()) => snapshot.writable = snapshot.root.is_some(),
            Err(error) => {
                snapshot.warning = Some(format!(
                    "No se cargaron las búsquedas guardadas: {error}. El archivo no se sobreescribirá."
                ))
            }
        }
        snapshot
    }

    pub fn saved_searches(&self) -> Vec<SavedSearch> {
        self.saved_searches_snapshot().searches
    }

    pub fn save_searches(&self, all: &[SavedSearch], ctx: &WriteCtx) -> Result<(), VaultError> {
        self.save_searches_checked(all, &self.saved_searches_snapshot(), ctx)
            .map(|_| ())
    }

    pub fn save_searches_checked(
        &self,
        all: &[SavedSearch],
        baseline: &SavedSearchSnapshot,
        ctx: &WriteCtx,
    ) -> Result<SavedSearchSnapshot, VaultError> {
        let root = self.root().canonicalize()?;
        if !baseline.writable || baseline.root.as_deref() != Some(&root) {
            return Err(invalid(
                "La lista no tiene un estado de lectura válido para esta bóveda",
            ));
        }
        validate(all)?;
        let before = read_config(&root)?;
        if before.as_deref().map(hash) != baseline.hash {
            return Err(VaultError::Conflict {
                path: RelPath::from_static(SAVED_PATH),
            });
        }
        let output = serialize(all);
        if output.len() as u64 > crate::entry::MAX_EDITABLE_BYTES {
            return Err(VaultError::TooLarge(RelPath::from_static(SAVED_PATH)));
        }
        if before.as_deref() != Some(output.as_bytes()) {
            check_directories(&root, &[".orbital"])?;
            fs::create_dir_all(root.join(".orbital"))?;
            if let Some(bytes) = before {
                let date = ctx.date.to_string();
                check_directories(&root, &[".orbital", "respaldo", &date, ".orbital"])?;
                backup(
                    &root,
                    &RelPath::from_static(SAVED_PATH),
                    &bytes,
                    hash(output.as_bytes()),
                    ctx,
                )?;
            }
            atomic_write(&root.join(SAVED_PATH), output.as_bytes())?;
        }
        Ok(SavedSearchSnapshot {
            searches: all.to_vec(),
            hash: Some(hash(output.as_bytes())),
            warning: None,
            root: Some(root),
            writable: true,
        })
    }
}
