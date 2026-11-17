use super::*;
use crate::{EditPlan, FileEdit, props::PropValue};
#[derive(Clone, Debug, Default)]
pub struct PropertyRenamePlan {
    pub edits: EditPlan,
    pub conflicts: Vec<(RelPath, String)>,
}
fn reserved(key: &str) -> bool {
    matches!(
        key,
        "type"
            | "title"
            | "description"
            | "resource"
            | "tags"
            | "aliases"
            | "status"
            | "sources"
            | "generated"
            | "verified"
            | "stale_after"
            | "created"
            | "updated"
            | "archived"
    ) || key.starts_with("file.")
}
fn check_change(
    note: &crate::Note,
    updated: &str,
    expected: &[(String, PropValue)],
) -> Result<(), String> {
    let parsed = note
        .reparse(updated.to_owned())
        .map_err(|e| e.to_string())?;
    if parsed.props.as_ref().is_some_and(|p| p.error.is_some()) {
        return Err("El cambio produciría propiedades inválidas; no se guardó".into());
    }
    let actual: Vec<_> = parsed
        .props
        .as_ref()
        .map(|p| {
            p.entries
                .iter()
                .map(|e| (e.key.clone(), e.value.clone()))
                .collect()
        })
        .unwrap_or_default();
    if actual != expected {
        return Err("Este formato de propiedades requiere edición en Código para conservar todos los valores".into());
    }
    if let (Some(before), Some(after)) = (&note.props, &parsed.props) {
        if note.source[..before.region.start] != parsed.source[..after.region.start]
            || note.source[before.region.end..] != parsed.source[after.region.end..]
        {
            return Err(
                "El cambio alteraría contenido ajeno a las propiedades; no se guardó".into(),
            );
        }
    } else if note.format == crate::Format::Md && note.body() != parsed.body() {
        return Err("El cambio alteraría el cuerpo de la nota; no se guardó".into());
    }
    Ok(())
}
fn entries(note: &crate::Note) -> Vec<(String, PropValue)> {
    note.props
        .as_ref()
        .map(|p| {
            p.entries
                .iter()
                .map(|e| (e.key.clone(), e.value.clone()))
                .collect()
        })
        .unwrap_or_default()
}
pub fn edit_cell(
    vault: &Vault,
    path: &RelPath,
    expected_hash: u64,
    key: &str,
    value: Option<&PropValue>,
    ctx: &WriteCtx,
) -> Result<WriteReceipt, VaultError> {
    contained(vault, path)?;
    if key.starts_with("file.") {
        return Err(VaultError::ReadOnly(path.clone()));
    }
    let note = vault.read_note(path)?;
    if note.hash != expected_hash {
        return Err(VaultError::Conflict { path: path.clone() });
    }
    let updated = match value {
        Some(v) => note.with_prop(key, v)?,
        None => note.without_prop(key)?,
    };
    let mut expected = entries(&note);
    match value {
        Some(v) => {
            if let Some((_, old)) = expected.iter_mut().find(|(k, _)| k == key) {
                *old = v.clone()
            } else {
                expected.push((key.into(), v.clone()));
            }
        }
        None => expected.retain(|(k, _)| k != key),
    }
    check_change(&note, &updated, &expected).map_err(|e| invalid(path, e))?;
    match value {
        Some(value) => vault.set_prop(&note, key, value, ctx),
        None => vault.remove_prop(&note, key, ctx),
    }
}
/// Renombra solo el token de la clave; conserva valor, comentarios y finales de línea.
fn rename_source(note: &crate::Note, old: &str, new: &str) -> Result<String, String> {
    let block = note.props.as_ref().ok_or("Falta bloque de propiedades")?;
    let entries = &block.entries;
    let entry = entries
        .iter()
        .find(|e| e.key == old)
        .ok_or("Falta la propiedad")?;
    let yaml = &note.source[block.region.clone()];
    let table = crate::props::yaml::line_table(yaml);
    let start = table[entry.lines.start];
    let line = yaml[start..].lines().next().unwrap_or("");
    let mut quote = None;
    let mut escaped = false;
    let mut colon = None;
    for (i, c) in line.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        if quote == Some('"') && c == '\\' {
            escaped = true;
            continue;
        }
        if let Some(q) = quote {
            if c == q {
                quote = None;
            }
        } else if c == '\'' || c == '"' {
            quote = Some(c);
        } else if c == ':' {
            colon = Some(i);
            break;
        }
    }
    let colon = colon.ok_or("No se puede renombrar esta clave multilínea; corregila en Código")?;
    let mut out = note.source.clone();
    out.replace_range(
        block.region.start + start..block.region.start + start + colon,
        &crate::props::emit::value(&PropValue::Text(new.into())),
    );
    let reparsed = note.reparse(out.clone()).map_err(|e| e.to_string())?;
    let expected: Vec<_> = entries
        .iter()
        .map(|e| (if e.key == old { new } else { &e.key }, &e.value))
        .collect();
    let actual: Vec<_> = reparsed
        .props
        .as_ref()
        .ok_or("Se perdió el bloque de propiedades")?
        .entries
        .iter()
        .map(|e| (e.key.as_str(), &e.value))
        .collect();
    if actual != expected || reparsed.props.as_ref().is_some_and(|p| p.error.is_some()) {
        return Err("Este formato de clave requiere edición en Código; no se modificó".into());
    }
    Ok(out)
}
pub fn plan_property_rename(
    vault: &Vault,
    old: &str,
    new: &str,
) -> Result<PropertyRenamePlan, String> {
    if old.trim().is_empty() || new.trim().is_empty() || old == new {
        return Err("Elegí dos nombres de propiedad diferentes y no vacíos".into());
    }
    if reserved(old) || reserved(new) {
        return Err("Las propiedades reservadas de OKF no se pueden renombrar".into());
    }
    let mut plan = PropertyRenamePlan {
        edits: EditPlan {
            root: vault.root().canonicalize().ok(),
            ..Default::default()
        },
        ..Default::default()
    };
    for entry in vault.entries() {
        let p = &entry.path;
        if crate::Format::from_name(p.file_name()).is_some() {
            let change = (|| -> Result<Option<FileEdit>, String> {
                contained(vault, p).map_err(|e| e.to_string())?;
                let note = vault.read_note(p).map_err(|e| e.to_string())?;
                if let Some(error) = note.props.as_ref().and_then(|b| b.error.as_ref()) {
                    return Err(error.message.clone());
                }
                let body = if note.format == crate::Format::Html {
                    crate::html_text(note.body())
                } else {
                    note.body().to_owned()
                };
                let inline = crate::inline_fields(&body);
                if inline.iter().any(|(k, _)| k == old) {
                    return Err(
                        "Propiedad de origen inline: renombrala en Código; la nota quedó intacta"
                            .into(),
                    );
                }
                let Some(value) = note.prop(old) else {
                    return Ok(None);
                };
                let inline_destination = inline
                    .iter()
                    .find(|(k, _)| k == new)
                    .map(|(_, v)| PropValue::Text(v.clone()));
                let destination = note.prop(new).or(inline_destination.as_ref());
                if note.read_only {
                    return Err("Nota de solo lectura".into());
                }
                if note.props.as_ref().is_some_and(|b| {
                    b.entries
                        .iter()
                        .filter(|e| e.key == old || e.key == new)
                        .count()
                        > if note.prop(new).is_some() { 2 } else { 1 }
                }) {
                    return Err(
                        "Propiedades duplicadas: corregí el bloque antes de renombrar".into(),
                    );
                }
                if destination.is_some_and(|dest| dest != value) {
                    plan.conflicts.push((
                        p.clone(),
                        format!("«{old}» y «{new}» tienen valores diferentes"),
                    ));
                    return Ok(None);
                }
                let updated = if note.prop(new).is_some() {
                    note.without_prop(old).map_err(|e| e.to_string())?
                } else {
                    rename_source(&note, old, new)?
                };
                let mut expected = entries(&note);
                if note.prop(new).is_some() {
                    expected.retain(|(k, _)| k != old);
                } else {
                    for (k, _) in &mut expected {
                        if k == old {
                            *k = new.into();
                        }
                    }
                }
                check_change(&note, &updated, &expected)?;
                Ok(Some(FileEdit {
                    path: p.clone(),
                    count: 1,
                    original: note.source,
                    updated,
                }))
            })();
            match change {
                Ok(Some(c)) => plan.edits.changes.push(c),
                Ok(None) => {}
                Err(e) => plan.edits.errors.push((p.clone(), e)),
            }
        } else if entry.kind == crate::EntryKind::Base {
            let change = (|| -> Result<Option<FileEdit>, String> {
                let doc = read(vault, p).map_err(|e| e.to_string())?;
                let mut d = doc.definition;
                if d.columns.iter().any(|c| c.property == old)
                    && d.columns.iter().any(|c| c.property == new)
                {
                    plan.conflicts.push((
                        p.clone(),
                        "Ambas columnas ya existen; la base quedó intacta".into(),
                    ));
                    return Ok(None);
                }
                let mut count = 0;
                for c in &mut d.columns {
                    if c.property == old {
                        c.property = new.into();
                        count += 1;
                    }
                }
                for f in &mut d.filters {
                    if let BaseFilter::Property { key, .. } = f
                        && key == old
                    {
                        *key = new.into();
                        count += 1;
                    }
                }
                if let Some(s) = &mut d.sort
                    && s.property == old
                {
                    s.property = new.into();
                    count += 1;
                }
                if count == 0 {
                    return Ok(None);
                }
                if entry.read_only {
                    return Err("Base de solo lectura".into());
                }
                let original =
                    std::fs::read_to_string(p.to_path(vault.root())).map_err(|e| e.to_string())?;
                if crate::write::hash(original.as_bytes()) != doc.hash {
                    return Err("La base cambió durante la planificación".into());
                }
                let updated = emit(&d);
                parse(&updated)?;
                Ok(Some(FileEdit {
                    path: p.clone(),
                    count,
                    original,
                    updated,
                }))
            })();
            match change {
                Ok(Some(c)) => plan.edits.changes.push(c),
                Ok(None) => {}
                Err(e) => plan.edits.errors.push((p.clone(), e)),
            }
        }
    }
    Ok(plan)
}
