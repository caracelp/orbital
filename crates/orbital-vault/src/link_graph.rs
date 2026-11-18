//! Grafo de enlaces y retroenlaces bidireccional en memoria (especificación 003).

use std::collections::HashMap;

use crate::entry::{EntryKind, RelPath, Zone};
use crate::error::VaultError;
use crate::links::{BacklinkRef, LinkKind, LinkOccurrence, Resolution, extract_links};
use crate::vault::Vault;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LinkGraph {
    /// Enlaces salientes por archivo origen
    outgoing: HashMap<RelPath, Vec<LinkOccurrence>>,
    /// Retroenlaces hacia archivos destino existentes en la bóveda
    backlinks: HashMap<RelPath, Vec<BacklinkRef>>,
    /// Enlaces hacia destinos pendientes (nombre base en minúsculas -> lista de referencias)
    unresolved: HashMap<String, Vec<BacklinkRef>>,
}

fn stem(name: &str) -> &str {
    name.rsplit_once('.').map_or(name, |(s, _)| s)
}

impl LinkGraph {
    pub fn new() -> Self {
        Self::default()
    }

    /// Construye el grafo de enlaces a partir de las notas existentes en la bóveda.
    pub fn build(vault: &Vault) -> Result<Self, VaultError> {
        let mut graph = Self::new();
        for entry in vault.entries() {
            if entry.zone == Zone::Wiki
                && matches!(entry.kind, EntryKind::AtomNote | EntryKind::SynthesisNote)
            {
                let full_path = entry.path.to_path(vault.root());
                if let Ok(text) = std::fs::read_to_string(&full_path) {
                    let is_html = entry.kind == EntryKind::SynthesisNote;
                    graph.update_file(vault, &entry.path, &text, is_html);
                }
            }
        }
        Ok(graph)
    }

    /// Devuelve los retroenlaces hacia una nota dada.
    pub fn backlinks_for(&self, path: &RelPath) -> &[BacklinkRef] {
        self.backlinks
            .get(path)
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }

    /// Devuelve los enlaces salientes de una nota dada.
    pub fn outgoing_for(&self, path: &RelPath) -> &[LinkOccurrence] {
        self.outgoing.get(path).map(|v| v.as_slice()).unwrap_or(&[])
    }

    /// Indica si un destino dado no está resuelto todavía.
    pub fn is_unresolved(&self, target_name: &str) -> bool {
        let key = stem(target_name.trim()).to_lowercase();
        self.unresolved.contains_key(&key)
    }

    /// Cantidad de destinos distintos pendientes de resolución.
    /// Pares de enlaces resueltos `(origen, destino)` sin duplicados, para el grafo (008).
    /// El orden es determinista: origen y destino ordenados por ruta. Solo enlaces que
    /// resolvieron a una nota existente (las claves de `backlinks`).
    pub fn enlaces_resueltos(&self) -> Vec<(RelPath, RelPath)> {
        let mut out: Vec<(RelPath, RelPath)> = self
            .backlinks
            .iter()
            .flat_map(|(destino, refs)| refs.iter().map(|r| (r.from.clone(), destino.clone())))
            .collect();
        out.sort();
        out.dedup();
        out
    }

    /// Como [`Self::enlaces_resueltos`], con el tipo de la relación (049) en su nombre canónico:
    /// si un par tiene enlaces con y sin tipo, gana el primer tipo (en orden de ruta y línea).
    pub fn enlaces_resueltos_con_tipo(&self) -> Vec<(RelPath, RelPath, Option<String>)> {
        let mut out: Vec<(&RelPath, &RelPath, usize, Option<&str>)> = self
            .backlinks
            .iter()
            .flat_map(|(destino, refs)| {
                refs.iter().map(move |r| {
                    (
                        &r.from,
                        destino,
                        r.line,
                        r.relacion.as_deref().map(|i| i.tipo.as_str()),
                    )
                })
            })
            .collect();
        // Por par; dentro del par, primero los que tienen tipo y después por línea.
        out.sort_by(|a, b| (a.0, a.1, a.3.is_none(), a.2).cmp(&(b.0, b.1, b.3.is_none(), b.2)));
        out.dedup_by(|b, a| a.0 == b.0 && a.1 == b.1);
        // Pocos tipos distintos: el nombre canónico se calcula una vez por cada uno.
        let mut nombres: HashMap<&str, String> = HashMap::new();
        out.into_iter()
            .map(|(a, b, _, t)| {
                let t = t.map(|t| {
                    nombres
                        .entry(t)
                        .or_insert_with(|| crate::relaciones::nombre_canonico(t))
                        .clone()
                });
                (a.clone(), b.clone(), t)
            })
            .collect()
    }

    /// Los tipos de relación usados en la bóveda (049), en su nombre canónico y sin repetir
    /// claves, del más usado al menos usado (empate: alfabético).
    pub fn tipos_usados(&self) -> Vec<String> {
        // Primero por tipo tal como se escribió (pocos distintos), después por clave.
        let mut escritos: HashMap<&str, usize> = HashMap::new();
        for r in self
            .backlinks
            .values()
            .chain(self.unresolved.values())
            .flatten()
        {
            if let Some(info) = &r.relacion {
                *escritos.entry(info.tipo.as_str()).or_default() += 1;
            }
        }
        let mut cuenta: HashMap<String, (String, usize)> = HashMap::new();
        for (tipo, n) in escritos {
            let e = cuenta
                .entry(crate::relaciones::clave(tipo))
                .or_insert_with(|| (crate::relaciones::nombre_canonico(tipo), 0));
            e.1 += n;
        }
        let mut v: Vec<(String, usize)> = cuenta.into_values().collect();
        v.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        v.into_iter().map(|(n, _)| n).collect()
    }

    /// Todas las notas del grafo, ordenadas por ruta (008).
    pub fn notas(&self) -> Vec<RelPath> {
        let mut out: Vec<RelPath> = self.outgoing.keys().cloned().collect();
        for destinos in self.backlinks.keys() {
            out.push(destinos.clone());
        }
        out.sort();
        out.dedup();
        out
    }

    pub fn unresolved_count(&self) -> usize {
        self.unresolved.len()
    }

    /// Actualiza los enlaces salientes de un archivo en el grafo y recalcula sus retroenlaces.
    pub fn update_file(&mut self, vault: &Vault, path: &RelPath, text: &str, is_html: bool) {
        // 1. Quitar referencias salientes anteriores originadas en `path` (solo si ya estaba:
        //    limpiar recorre todo el grafo, y al construirlo desde cero sería cuadrático).
        if self.outgoing.contains_key(path) {
            self.clean_outgoing_from(path);
        }

        // 2. Extraer los enlaces nuevos y, en Markdown, los tipos de la parte Conexiones (049).
        let occurrences = extract_links(text, is_html);
        self.outgoing.insert(path.clone(), occurrences.clone());
        let relaciones = if is_html {
            Vec::new()
        } else {
            tipos_por_enlace(text, &occurrences)
        };

        // 3. Registrar los retroenlaces
        for occ in occurrences {
            // Ignorar enlaces web externos en el grafo de la bóveda
            if occ.target.path.starts_with("http://")
                || occ.target.path.starts_with("https://")
                || occ.target.path.starts_with("mailto:")
            {
                continue;
            }

            let relacion = relaciones
                .iter()
                .find(|(r, _)| es_del_renglon(r, &occ))
                .and_then(|(_, info)| info.clone());
            let backlink = BacklinkRef {
                from: path.clone(),
                target: occ.target.clone(),
                line: occ.line,
                snippet: occ.snippet.clone(),
                relacion,
            };

            let resolution = match occ.kind {
                LinkKind::Wikilink | LinkKind::Transclusion => {
                    vault.resolve_wikilink(&occ.target.path)
                }
                LinkKind::Markdown => vault.resolve_link(path, &occ.target.path),
            };

            match resolution {
                Resolution::Found(dest) => {
                    self.backlinks.entry(dest).or_default().push(backlink);
                }
                Resolution::Missing | Resolution::Ambiguous(_) => {
                    let key = stem(&occ.target.path).to_lowercase();
                    self.unresolved.entry(key).or_default().push(backlink);
                }
            }
        }

        // 4. Si `path` resuelve algún enlace que antes figuraba como pendiente, moverlo a backlinks
        let my_key = stem(path.file_name()).to_lowercase();
        if let Some(pending_refs) = self.unresolved.remove(&my_key) {
            self.backlinks
                .entry(path.clone())
                .or_default()
                .extend(pending_refs);
        }
    }

    /// Elimina una nota del grafo (por borrado).
    pub fn remove_file(&mut self, path: &RelPath) {
        self.clean_outgoing_from(path);
        self.outgoing.remove(path);
        self.backlinks.remove(path);
    }

    /// Limpia cualquier retroenlace que haya sido originado desde `from`.
    fn clean_outgoing_from(&mut self, from: &RelPath) {
        for list in self.backlinks.values_mut() {
            list.retain(|b| b.from != *from);
        }
        for list in self.unresolved.values_mut() {
            list.retain(|b| b.from != *from);
        }
        self.unresolved.retain(|_, list| !list.is_empty());
    }
}

/// El enlace va en el valor de ese renglón con tipo (049).
fn es_del_renglon(r: &crate::relaciones::RenglonConTipo, occ: &LinkOccurrence) -> bool {
    occ.line == r.linea && occ.byte_range.start >= r.valor && occ.kind != LinkKind::Transclusion
}

/// Los renglones con tipo de la nota con su tipo y motivo, a partir de los enlaces ya extraídos
/// (el motivo es lo que sigue al último enlace del renglón). Sin enlaces, el renglón no declara.
fn tipos_por_enlace<'t>(
    text: &'t str,
    occurrences: &[LinkOccurrence],
) -> Vec<(
    crate::relaciones::RenglonConTipo<'t>,
    Option<std::sync::Arc<crate::relaciones::RelacionInfo>>,
)> {
    crate::relaciones::renglones_con_tipo(text)
        .into_iter()
        .map(|r| {
            let fin = occurrences
                .iter()
                .filter(|o| es_del_renglon(&r, o))
                .map(|o| o.byte_range.end)
                .max();
            let info = fin.map(|e| {
                std::sync::Arc::new(crate::relaciones::RelacionInfo {
                    tipo: r.tipo.to_owned(),
                    motivo: crate::relaciones::motivo_de_sufijo(&text[e.min(r.fin)..r.fin])
                        .map(str::to_owned),
                })
            });
            (r, info)
        })
        .collect()
}
