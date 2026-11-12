//! Actualización de enlaces al renombrar o mover una nota (US6 de la 003).
//!
//! Un enlace se reescribe solo si **hoy resuelve** a la nota renombrada, con la misma resolución que
//! usa la navegación ([`Vault::resolve_link`], [`Vault::resolve_wikilink`]); nunca por coincidencia
//! de nombre (dos notas `x.md` en carpetas distintas, o una URL que termine en `/x`, no se tocan).
//! De cada enlace cambia solo la ruta, en el mismo estilo que tenía (con o sin carpeta, con o sin
//! extensión, absoluta o relativa); alias, ancla, título y el resto del archivo quedan igual.
//!
//! Uso: [`Vault::plan_link_updates`] **antes** de renombrar (la resolución mira la bóveda actual),
//! después renombrar, y [`Vault::apply_link_updates`]. [`Vault::revert_link_updates`] deshace.

use std::fs;

use crate::entry::{EntryKind, RelPath, Zone};
use crate::error::VaultError;
use crate::links::{LinkKind, LinkOccurrence, Resolution, extract_links};
use crate::vault::Vault;
use crate::write::{WriteCtx, WriteReceipt};

pub use crate::edit_plan::{EditPlan as LinkUpdatePlan, FileEdit as LinkChange};

impl Vault {
    /// Reescribe en `text` (el contenido de `from`) los enlaces que resuelven a `old` para que
    /// apunten a `new`. Devuelve el texto y cuántos enlaces cambió. Llamar antes de renombrar.
    pub fn rewrite_links(
        &self,
        from: &RelPath,
        text: &str,
        is_html: bool,
        old: &RelPath,
        new: &RelPath,
    ) -> (String, usize) {
        let mut out = String::with_capacity(text.len());
        let mut last = 0;
        let mut count = 0;
        for occ in extract_links(text, is_html) {
            if occ.byte_range.start < last || !self.points_to(from, &occ, old) {
                continue;
            }
            let raw = &text[occ.byte_range.clone()];
            let Some(replaced) = self.rewrite_one(from, &occ, raw, is_html, old, new) else {
                continue;
            };
            out.push_str(&text[last..occ.byte_range.start]);
            out.push_str(&replaced);
            last = occ.byte_range.end;
            count += 1;
        }
        if count == 0 {
            return (text.to_owned(), 0);
        }
        out.push_str(&text[last..]);
        (out, count)
    }

    /// Calcula los cambios en `sources` (las notas que enlazan a `old`; por ejemplo, los
    /// retroenlaces del grafo). Si `sources` es `None`, revisa todas las notas de `wiki/`.
    pub fn plan_link_updates(
        &self,
        old: &RelPath,
        new: &RelPath,
        sources: Option<&[RelPath]>,
    ) -> LinkUpdatePlan {
        let all: Vec<RelPath>;
        let sources = match sources {
            Some(s) => s,
            None => {
                all = self
                    .entries()
                    .iter()
                    .filter(|e| is_note(e.zone, e.kind))
                    .map(|e| e.path.clone())
                    .collect();
                &all
            }
        };
        let mut seen = std::collections::HashSet::new();
        let mut changes = Vec::new();
        for from in sources {
            if !seen.insert(from.clone()) {
                continue;
            }
            let Ok(text) = fs::read_to_string(from.to_path(self.root())) else {
                continue;
            };
            let is_html = matches!(from.file_name().rsplit('.').next(), Some("html" | "htm"));
            let (updated, count) = self.rewrite_links(from, &text, is_html, old, new);
            if count > 0 {
                changes.push(LinkChange {
                    path: from.clone(),
                    count,
                    original: text,
                    updated,
                });
            }
        }
        changes.sort_by(|a, b| a.path.as_str().cmp(b.path.as_str()));
        LinkUpdatePlan {
            old: Some(old.clone()),
            new: Some(new.clone()),
            changes,
            ..Default::default()
        }
    }

    /// Escribe los cambios del plan (después de renombrar), con GuardadoSeguro. Un archivo que
    /// cambió desde que se hizo el plan no se pisa: da `Conflict` y se sigue con los demás.
    pub fn apply_link_updates(
        &self,
        plan: &LinkUpdatePlan,
        ctx: &WriteCtx,
    ) -> (WriteReceipt, Vec<(RelPath, VaultError)>) {
        self.apply_edits(plan, ctx)
    }

    /// Deshace [`Vault::apply_link_updates`] (antes de volver a renombrar al nombre viejo).
    pub fn revert_link_updates(
        &self,
        plan: &LinkUpdatePlan,
        ctx: &WriteCtx,
    ) -> (WriteReceipt, Vec<(RelPath, VaultError)>) {
        self.revert_edits(plan, ctx)
    }

    /// El enlace resuelve hoy a `old`.
    fn points_to(&self, from: &RelPath, occ: &LinkOccurrence, old: &RelPath) -> bool {
        let path = occ.target.path.trim();
        if path.is_empty() || is_external(path) {
            return false;
        }
        let r = match occ.kind {
            LinkKind::Wikilink | LinkKind::Transclusion => self.resolve_wikilink(path),
            LinkKind::Markdown => self.resolve_link(from, path),
        };
        r == Resolution::Found(old.clone())
    }

    fn rewrite_one(
        &self,
        from: &RelPath,
        occ: &LinkOccurrence,
        raw: &str,
        is_html: bool,
        old: &RelPath,
        new: &RelPath,
    ) -> Option<String> {
        match occ.kind {
            LinkKind::Wikilink | LinkKind::Transclusion => {
                let open = if occ.kind == LinkKind::Transclusion {
                    3
                } else {
                    2
                };
                let inner = &raw[open..raw.len() - 2];
                let range = wiki_path_range(inner);
                let target = self.wiki_target(&inner[range.clone()], old, new);
                Some(replace(raw, open + range.start..open + range.end, &target))
            }
            LinkKind::Markdown if is_html => {
                let range = href_value_range(raw)?;
                let value = &raw[range.clone()];
                let path_end = url_path_end(value);
                let target = url_target(from, &value[..path_end], new, true)?;
                Some(replace(raw, range.start..range.start + path_end, &target))
            }
            LinkKind::Markdown => {
                let open = raw.find("](")? + 2;
                let url = &raw[open..raw.len() - 1];
                let lead = url.len() - url.trim_start().len();
                let url = url.trim_start();
                let (start, path) = match url.strip_prefix('<') {
                    Some(rest) => (open + lead + 1, &rest[..rest.find('>')?]),
                    None => (open + lead, &url[..url_path_end(url)]),
                };
                let path_end = start + path.len();
                // Dentro de `<…>` los espacios se pueden escribir tal cual.
                let encode = !url.starts_with('<');
                let target = url_target(from, path, new, encode)?;
                Some(replace(raw, start..path_end, &target))
            }
        }
    }

    /// El destino nuevo de un wikilink, en el mismo estilo que `old_text`.
    fn wiki_target(&self, old_text: &str, old: &RelPath, new: &RelPath) -> String {
        let with_ext =
            old_text.ends_with(".md") || old_text.ends_with(".html") || old_text.ends_with(".htm");
        let name = if with_ext {
            new.file_name()
        } else {
            stem(new.file_name())
        };
        // Con carpeta si el original la tenía o si el nombre solo quedaría ambiguo.
        if old_text.contains('/') || self.stem_taken(new, old) {
            let in_wiki = new.as_str().strip_prefix("wiki/").unwrap_or(new.as_str());
            let dir = in_wiki.rsplit_once('/').map_or("", |(d, _)| d);
            if dir.is_empty() {
                name.to_owned()
            } else {
                format!("{dir}/{name}")
            }
        } else {
            name.to_owned()
        }
    }

    /// Otra nota (que no sea `old`) ya tiene el nombre de `new`.
    fn stem_taken(&self, new: &RelPath, old: &RelPath) -> bool {
        let wanted = stem(new.file_name()).to_lowercase();
        self.entries().iter().any(|e| {
            is_note(e.zone, e.kind)
                && e.path != *old
                && e.path != *new
                && stem(e.path.file_name()).to_lowercase() == wanted
        })
    }
}

fn is_note(zone: Zone, kind: EntryKind) -> bool {
    zone == Zone::Wiki && matches!(kind, EntryKind::AtomNote | EntryKind::SynthesisNote)
}

fn is_external(path: &str) -> bool {
    path.contains("://") || path.starts_with("mailto:") || path.starts_with("data:")
}

fn stem(name: &str) -> &str {
    name.rsplit_once('.').map_or(name, |(s, _)| s)
}

fn replace(raw: &str, range: std::ops::Range<usize>, with: &str) -> String {
    let mut out = String::with_capacity(raw.len() + with.len());
    out.push_str(&raw[..range.start]);
    out.push_str(with);
    out.push_str(&raw[range.end..]);
    out
}

/// Rango de la ruta dentro de `destino#ancla|alias` (sin los espacios de los bordes).
fn wiki_path_range(inner: &str) -> std::ops::Range<usize> {
    let end = inner.find(['#', '|', '^']).unwrap_or(inner.len());
    let path = &inner[..end];
    let start = path.len() - path.trim_start().len();
    start..start + path.trim().len()
}

/// Fin de la ruta en una URL: antes de `#`, `?`, un espacio (título) o el final.
fn url_path_end(url: &str) -> usize {
    url.find(['#', '?', ' ', '\t']).unwrap_or(url.len())
}

/// Rango del valor de `href` en `<a …>` (sin las comillas).
fn href_value_range(raw: &str) -> Option<std::ops::Range<usize>> {
    let tag_end = raw.find('>')?;
    let lower = raw[..tag_end].to_ascii_lowercase();
    let mut at = 0;
    let attr = loop {
        let i = at + lower[at..].find("href")?;
        let before_ok = i == 0 || lower.as_bytes()[i - 1].is_ascii_whitespace();
        let rest = lower[i + 4..].trim_start();
        if before_ok && rest.starts_with('=') {
            break i;
        }
        at = i + 4;
    };
    let eq = attr + 4 + lower[attr + 4..].find('=')? + 1;
    let ws = raw[eq..tag_end].len() - raw[eq..tag_end].trim_start().len();
    let v = eq + ws;
    match raw.as_bytes().get(v) {
        Some(&q @ (b'"' | b'\'')) => {
            let close = raw[v + 1..tag_end].find(q as char)?;
            Some(v + 1..v + 1 + close)
        }
        _ => {
            let len = raw[v..tag_end]
                .find(|c: char| c.is_ascii_whitespace())
                .unwrap_or(tag_end - v);
            Some(v..v + len)
        }
    }
}

/// La ruta nueva de un enlace de URL, en el estilo de `old_path`: `/…` relativa a `wiki/`, o
/// relativa a la carpeta de la nota (con `./` si la tenía). `None` para `/raw/…`.
fn url_target(from: &RelPath, old_path: &str, new: &RelPath, encode: bool) -> Option<String> {
    if old_path.starts_with("/raw/") {
        return None;
    }
    let in_wiki = new.as_str().strip_prefix("wiki/")?;
    let path = if old_path.starts_with('/') {
        format!("/{in_wiki}")
    } else {
        let dir = from.as_str().rsplit_once('/').map_or("", |(d, _)| d);
        let rel = relative(dir, new.as_str());
        if old_path.starts_with("./") && !rel.starts_with("../") {
            format!("./{rel}")
        } else {
            rel
        }
    };
    Some(if encode || old_path.contains("%20") {
        path.replace(' ', "%20")
    } else {
        path
    })
}

/// Ruta de `target` relativa a la carpeta `dir` (ambas relativas a la bóveda).
fn relative(dir: &str, target: &str) -> String {
    let a: Vec<&str> = dir.split('/').filter(|p| !p.is_empty()).collect();
    let b: Vec<&str> = target.split('/').collect();
    let common = a
        .iter()
        .zip(&b[..b.len() - 1])
        .take_while(|(x, y)| x == y)
        .count();
    let mut parts: Vec<&str> = vec![".."; a.len() - common];
    parts.extend(&b[common..]);
    parts.join("/")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn helpers() {
        assert_eq!(
            relative("wiki/fisica", "wiki/quimica/x.md"),
            "../quimica/x.md"
        );
        assert_eq!(relative("wiki", "wiki/x.md"), "x.md");
        assert_eq!(relative("wiki/a", "wiki/a/b/x.md"), "b/x.md");
        assert_eq!(wiki_path_range(" viejo #sec|A"), 1..6);
        let raw = "<a class=\"x\" HREF = 'b.md#s'>t</a>";
        assert_eq!(&raw[href_value_range(raw).unwrap()], "b.md#s");
        assert_eq!(url_path_end("x.md \"t\""), 4);
    }
}
