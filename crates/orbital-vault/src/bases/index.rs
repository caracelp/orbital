use super::*;
use crate::props::PropValue;
use std::collections::BTreeMap;
#[derive(Clone, Debug)]
pub struct BaseRow {
    pub path: RelPath,
    pub title: String,
    pub hash: u64,
    pub read_only: bool,
    pub properties: Vec<(String, PropValue)>,
    pub error: Option<String>,
}
impl BaseRow {
    /// Valor de una propiedad. `type` lee también su alias `tipo` (gana `type` si están las dos).
    pub fn value(&self, key: &str) -> Option<&PropValue> {
        let get = |key: &str| {
            self.properties
                .iter()
                .find(|(k, _)| k == key)
                .map(|(_, v)| v)
        };
        match get(key) {
            None | Some(PropValue::Null) if key == crate::props::TYPE_KEY => {
                get(crate::props::TYPE_ALIAS).or(get(key))
            }
            found => found,
        }
    }
}
#[derive(Clone, Debug, Default)]
pub struct BaseSnapshot {
    pub rows: Vec<BaseRow>,
    pub errors: Vec<(RelPath, String)>,
}
#[derive(Clone, Debug, Default)]
pub struct BaseIndex {
    rows: BTreeMap<RelPath, (BaseRow, Vec<String>)>,
    errors: BTreeMap<RelPath, String>,
    /// Identificadores de nota (propiedad `id` y delimitadores `<!-- block … ID -->`) de cada
    /// nota, y las notas que usan cada uno: dan ids nuevos únicos y avisan de los repetidos.
    ids: BTreeMap<RelPath, Vec<String>>,
    by_id: std::collections::HashMap<String, Vec<RelPath>>,
}
impl BaseIndex {
    pub fn build(vault: &Vault) -> Self {
        let mut index = Self::default();
        for e in vault.entries() {
            if crate::Format::from_name(e.path.file_name()).is_some() {
                index.update(vault, &e.path);
            }
        }
        index
    }
    pub fn update(&mut self, vault: &Vault, path: &RelPath) {
        self.rows.remove(path);
        self.errors.remove(path);
        for id in self.ids.remove(path).unwrap_or_default() {
            if let Some(paths) = self.by_id.get_mut(&id) {
                paths.retain(|p| p != path);
                if paths.is_empty() {
                    self.by_id.remove(&id);
                }
            }
        }
        if crate::Format::from_name(path.file_name()).is_none()
            || !path.to_path(vault.root()).exists()
        {
            return;
        }
        let note = match contained(vault, path).and_then(|()| vault.read_note(path)) {
            Ok(n) => n,
            Err(e) => {
                self.errors.insert(path.clone(), e.to_string());
                return;
            }
        };
        let error = note
            .props
            .as_ref()
            .and_then(|p| p.error.as_ref())
            .map(|e| format!("Línea {}: {}", e.line, e.message));
        let mut properties: Vec<_> = note
            .props
            .as_ref()
            .map(|p| {
                p.entries
                    .iter()
                    .map(|e| (e.key.clone(), e.value.clone()))
                    .collect()
            })
            .unwrap_or_default();
        let inline_body = if note.format == crate::Format::Html {
            crate::html_text(note.body())
        } else {
            note.body().to_owned()
        };
        for (k, v) in crate::inline_fields(&inline_body) {
            if !properties.iter().any(|(key, _)| key == &k) {
                properties.push((k, PropValue::Text(v)));
            }
        }
        let title = note
            .prop("title")
            .and_then(PropValue::as_text)
            .unwrap_or(path.file_name())
            .to_owned();
        properties.retain(|(k, _)| k != "file.path" && k != "file.name");
        properties.push(("file.path".into(), PropValue::Text(path.to_string())));
        properties.push(("file.name".into(), PropValue::Text(path.file_name().into())));
        let tags = crate::extract_tags(&note.source, note.format == crate::Format::Html)
            .into_iter()
            .map(|t| t.tag)
            .collect();
        let ids =
            crate::note_ids::ids_of(note.prop("id").and_then(PropValue::as_text), note.body());
        for id in &ids {
            self.by_id.entry(id.clone()).or_default().push(path.clone());
        }
        if !ids.is_empty() {
            self.ids.insert(path.clone(), ids);
        }
        let row = BaseRow {
            path: path.clone(),
            title,
            hash: note.hash,
            read_only: note.read_only || error.is_some(),
            properties,
            error,
        };
        self.rows.insert(path.clone(), (row, tags));
    }
    /// Todos los identificadores de nota usados en la bóveda (normalizados).
    pub fn note_ids(&self) -> impl Iterator<Item = &str> {
        self.by_id.keys().map(String::as_str)
    }
    /// Las notas que usan `id` (propiedad o delimitadores), en orden.
    pub fn notes_with_id(&self, id: &str) -> Vec<RelPath> {
        let mut v = self
            .by_id
            .get(&crate::note_ids::normalize(id))
            .cloned()
            .unwrap_or_default();
        v.sort();
        v
    }
    /// Identificadores que usan dos notas o más, con sus notas, ordenados.
    pub fn duplicate_ids(&self) -> Vec<(String, Vec<RelPath>)> {
        let mut v: Vec<_> = self
            .by_id
            .iter()
            .filter(|(_, paths)| paths.len() > 1)
            .map(|(id, paths)| {
                let mut paths = paths.clone();
                paths.sort();
                (id.clone(), paths)
            })
            .collect();
        v.sort();
        v
    }
    pub fn query(&self, definition: &BaseDefinition) -> BaseSnapshot {
        let mut rows: Vec<_> = self
            .rows
            .values()
            .filter(|(row, tags)| {
                definition.filters.iter().all(|f| match f {
                    BaseFilter::Folder(folder) => {
                        folder.is_empty()
                            || row
                                .path
                                .as_str()
                                .strip_prefix(folder)
                                .is_some_and(|tail| tail.starts_with('/'))
                    }
                    BaseFilter::Tag(tag) => tags.contains(tag),
                    BaseFilter::Property { key, value } => match value {
                        None => row.value(key).is_some(),
                        Some(v) => row.value(key).is_some_and(|actual| match (actual, v) {
                            (PropValue::Number(_), PropValue::Number(_))
                            | (PropValue::DateTime(_), PropValue::DateTime(_)) => {
                                compare(Some(actual), Some(v)).is_eq()
                            }
                            _ => actual == v,
                        }),
                    },
                })
            })
            .map(|(row, _)| row.clone())
            .collect();
        if let Some(sort) = &definition.sort {
            rows.sort_by(|a, b| {
                let order = compare(a.value(&sort.property), b.value(&sort.property));
                let order = if sort.descending {
                    order.reverse()
                } else {
                    order
                };
                order.then_with(|| a.path.cmp(&b.path))
            });
        }
        BaseSnapshot {
            rows,
            errors: self
                .errors
                .iter()
                .map(|(p, e)| (p.clone(), e.clone()))
                .chain(self.rows.values().filter_map(|(row, _)| {
                    row.error.as_ref().map(|e| (row.path.clone(), e.clone()))
                }))
                .collect(),
        }
    }
}
fn compare(a: Option<&PropValue>, b: Option<&PropValue>) -> std::cmp::Ordering {
    use PropValue::*;
    fn rank(v: Option<&PropValue>) -> u8 {
        match v {
            None => 0,
            Some(Null) => 1,
            Some(Bool(_)) => 2,
            Some(Number(_)) => 3,
            Some(Date(_)) => 4,
            Some(DateTime(_)) => 5,
            Some(Text(_)) => 6,
            Some(Link(_)) => 7,
            Some(List(_)) => 8,
            Some(Map(_)) => 9,
        }
    }
    let r = rank(a).cmp(&rank(b));
    if !r.is_eq() {
        return r;
    }
    match (a, b) {
        (Some(Number(a)), Some(Number(b))) => {
            match (super::format::number(a), super::format::number(b)) {
                (Some(a), Some(b)) => a.cmp(&b),
                (Some(_), None) => std::cmp::Ordering::Less,
                (None, Some(_)) => std::cmp::Ordering::Greater,
                (None, None) => a.cmp(b),
            }
        }
        (Some(DateTime(a)), Some(DateTime(b))) => {
            match (super::format::datetime(a), super::format::datetime(b)) {
                (Some(a), Some(b)) => a.cmp(&b),
                (Some(_), None) => std::cmp::Ordering::Less,
                (None, Some(_)) => std::cmp::Ordering::Greater,
                (None, None) => a.cmp(b),
            }
        }
        (Some(Bool(a)), Some(Bool(b))) => a.cmp(b),
        (Some(Text(a) | Date(a) | Link(a)), Some(Text(b) | Date(b) | Link(b))) => a.cmp(b),
        (Some(a), Some(b)) => crate::props::emit::value(a).cmp(&crate::props::emit::value(b)),
        _ => std::cmp::Ordering::Equal,
    }
}
