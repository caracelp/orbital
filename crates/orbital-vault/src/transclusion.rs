//! Transclusiones `![[nota]]` y `![[nota#encabezado]]` (US5 de la 003, research R6).
//!
//! [`crate::render::render_markdown`] deja cada transclusión sin expandir; [`Vault::expand_transclusions`]
//! lee las notas incrustadas (en el hilo de fondo donde se analiza el Markdown) y las reemplaza
//! por su contenido, con un conjunto de notas visitadas contra los ciclos y un máximo de
//! [`MAX_DEPTH`] niveles.

use crate::entry::RelPath;
use crate::links::Resolution;
use crate::render::{Block, BlockKind, Embed, RenderDoc, plain_text, render_markdown};
use crate::vault::Vault;
use crate::write::hash;

/// Niveles de transclusión anidada como máximo (FR-017).
pub const MAX_DEPTH: usize = 3;

impl Vault {
    /// Expande las transclusiones de `doc` (el contenido de `from`). Devuelve las notas
    /// incrustadas, a cualquier profundidad: si una de ellas cambia, hay que volver a dibujar.
    pub fn expand_transclusions(&self, from: &RelPath, doc: &mut RenderDoc) -> Vec<RelPath> {
        let mut deps = Vec::new();
        let mut visited = vec![from.clone()];
        self.expand(&mut doc.blocks, &mut visited, 1, &mut deps);
        deps.sort();
        deps.dedup();
        deps
    }

    fn expand(
        &self,
        blocks: &mut [Block],
        visited: &mut Vec<RelPath>,
        depth: usize,
        deps: &mut Vec<RelPath>,
    ) {
        for b in blocks {
            match &mut b.kind {
                BlockKind::Transclusion(t) if t.content == Embed::Pending => {
                    t.content = self.embed(&t.target, t.anchor.as_deref(), visited, depth, deps);
                }
                BlockKind::Quote(body) | BlockKind::Callout { body, .. } => {
                    self.expand(body, visited, depth, deps)
                }
                BlockKind::List { items, .. } => {
                    for item in items {
                        self.expand(&mut item.blocks, visited, depth, deps);
                    }
                }
                _ => {}
            }
        }
    }

    fn embed(
        &self,
        target: &str,
        anchor: Option<&str>,
        visited: &mut Vec<RelPath>,
        depth: usize,
        deps: &mut Vec<RelPath>,
    ) -> Embed {
        let path = match self.resolve_wikilink(target) {
            Resolution::Found(p) => p,
            Resolution::Missing => return Embed::Missing,
            Resolution::Ambiguous(all) => return Embed::Ambiguous(all),
        };
        if visited.contains(&path) {
            return Embed::Cycle { path };
        }
        if depth > MAX_DEPTH {
            return Embed::TooDeep;
        }
        deps.push(path.clone());
        if matches!(path.file_name().rsplit('.').next(), Some("html" | "htm")) {
            return Embed::Html { path };
        }
        let Ok(text) = std::fs::read_to_string(path.to_path(self.root())) else {
            return Embed::Missing;
        };
        let mut blocks = render_markdown(&text).blocks;
        if let Some(a) = anchor {
            match section(blocks, a) {
                Some(s) => blocks = s,
                None => return Embed::SectionMissing { path },
            }
        }
        visited.push(path.clone());
        self.expand(&mut blocks, visited, depth + 1, deps);
        visited.pop();
        Embed::Note {
            path,
            hash: hash(text.as_bytes()),
            blocks,
        }
    }
}

/// El encabezado `anchor` y lo que sigue hasta el próximo encabezado de igual o mayor nivel.
fn section(blocks: Vec<Block>, anchor: &str) -> Option<Vec<Block>> {
    let wanted = normalize(anchor);
    let start = blocks.iter().position(|b| {
        matches!(&b.kind, BlockKind::Heading { inlines, .. } if normalize(&plain_text(inlines)) == wanted)
    })?;
    let BlockKind::Heading { level, .. } = blocks[start].kind else {
        return None;
    };
    let end = blocks[start + 1..]
        .iter()
        .position(|b| matches!(b.kind, BlockKind::Heading { level: l, .. } if l <= level))
        .map_or(blocks.len(), |n| start + 1 + n);
    Some(blocks.into_iter().take(end).skip(start).collect())
}

/// Encabezados comparados sin mayúsculas, sin `#` al principio y con los espacios colapsados.
fn normalize(s: &str) -> String {
    s.trim()
        .trim_start_matches('#')
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

/// Huella de lo incrustado en un bloque (a cualquier profundidad): cambia si cambia una nota
/// incrustada, aunque el texto del bloque sea el mismo. 0 si no incrusta nada.
pub fn fingerprint(block: &Block) -> u64 {
    fn walk(blocks: &[Block], acc: &mut Vec<u8>) {
        for b in blocks {
            match &b.kind {
                BlockKind::Transclusion(t) => {
                    match &t.content {
                        Embed::Note { path, hash, blocks } => {
                            acc.extend_from_slice(path.as_str().as_bytes());
                            acc.extend_from_slice(&hash.to_le_bytes());
                            walk(blocks, acc);
                        }
                        other => acc.extend_from_slice(format!("{other:?}").as_bytes()),
                    }
                    acc.push(0);
                }
                BlockKind::Quote(body) | BlockKind::Callout { body, .. } => walk(body, acc),
                BlockKind::List { items, .. } => {
                    for i in items {
                        walk(&i.blocks, acc);
                    }
                }
                _ => {}
            }
        }
    }
    let mut acc = Vec::new();
    walk(std::slice::from_ref(block), &mut acc);
    if acc.is_empty() { 0 } else { hash(&acc) }
}
