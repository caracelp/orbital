//! `wiki/index.md` y `wiki/log.md` (OKF v0.2 §8 y §9), editados línea por línea (research R11).
//!
//! Orbital solo agrega, cambia o quita sus propias líneas (`* [Título](ruta) - descripción` que
//! apuntan a notas, y entradas del registro); todo lo demás, incluido lo escrito a mano o por un
//! agente, queda igual.

use crate::entry::RelPath;
use crate::props::emit::LineEnding;

/// Una entrada del índice.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct IndexEntry {
    pub title: String,
    /// Ruta relativa a la raíz de `wiki/` que empieza con `/` (forma recomendada por OKF §6.1).
    pub link: String,
    pub description: String,
    /// Sección del índice: el `type` de la nota.
    pub section: String,
}

/// Verbo de una entrada del registro.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Verb {
    Creation,
    Update,
    Rename,
    Move,
    Archive,
    Deletion,
}

impl Verb {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Creation => "Creación",
            Self::Update => "Actualización",
            Self::Rename => "Renombre",
            Self::Move => "Movimiento",
            Self::Archive => "Archivado",
            Self::Deletion => "Borrado",
        }
    }
}

/// Enlace del índice para una ruta de `wiki/`: `wiki/a/b.md` → `/a/b.md`.
pub fn index_link(path: &RelPath) -> Option<String> {
    path.as_str()
        .strip_prefix("wiki/")
        .map(|rest| format!("/{rest}"))
}

/// Enlace en Markdown: entre `<>` si tiene espacios o paréntesis (CommonMark).
fn md_link(link: &str) -> String {
    if link.contains([' ', '(', ')']) {
        format!("<{link}>")
    } else {
        link.to_owned()
    }
}

/// `[Título](enlace)` para el registro y el índice.
pub fn md_ref(title: &str, link: &str) -> String {
    format!("[{}]({})", title.replace(']', "\\]"), md_link(link))
}

fn entry_line(e: &IndexEntry) -> String {
    let mut line = format!("* {}", md_ref(&e.title, &e.link));
    if !e.description.is_empty() {
        line.push_str(" - ");
        line.push_str(&e.description);
    }
    line
}

/// Enlace de una línea `* [Título](enlace)…`, si la línea tiene esa forma.
pub fn line_link(line: &str) -> Option<&str> {
    let rest = line.trim_end().strip_prefix("* [")?;
    // Fin del título: el primer `](` que no esté escapado.
    let mut i = 0;
    let bytes = rest.as_bytes();
    let close = loop {
        let p = rest[i..].find("](")? + i;
        if p == 0 || bytes[p - 1] != b'\\' {
            break p;
        }
        i = p + 2;
    };
    let after = &rest[close + 2..];
    if let Some(inner) = after.strip_prefix('<') {
        return inner.find('>').map(|e| &inner[..e]);
    }
    after.find(')').map(|e| &after[..e])
}

fn split_lines(text: &str) -> Vec<&str> {
    // Cada elemento sin su final de línea.
    text.split_inclusive('\n')
        .map(|l| l.trim_end_matches(['\n', '\r']))
        .collect()
}

fn join(lines: &[String], eol: LineEnding) -> String {
    let mut out = String::with_capacity(lines.iter().map(|l| l.len() + 2).sum());
    for l in lines {
        out.push_str(l);
        out.push_str(eol.as_str());
    }
    out
}

fn is_heading1(line: &str) -> Option<&str> {
    line.strip_prefix("# ").map(str::trim)
}

/// Agrega o actualiza la entrada de `entry.link` en la sección `entry.section`.
pub fn index_upsert(text: &str, entry: &IndexEntry) -> String {
    index_upsert_from(text, &entry.link, entry)
}

/// Como [`index_upsert`], pero reemplaza la línea de `old_link` (un renombre): si sigue en la
/// misma sección, la entrada queda en el mismo lugar.
pub fn index_upsert_from(text: &str, old_link: &str, entry: &IndexEntry) -> String {
    index_put_line(text, old_link, &entry.section, entry_line(entry))
}

/// Sección y línea exacta de la entrada de `link`, si está en el índice.
pub fn index_find(text: &str, link: &str) -> Option<(String, String)> {
    let mut section = String::new();
    for l in split_lines(text) {
        if let Some(h) = is_heading1(l) {
            section = h.to_owned();
        } else if line_link(l) == Some(link) {
            return Some((section, l.to_owned()));
        }
    }
    None
}

/// Pone `new_line` como la entrada de `old_link` en la sección `section` (en el lugar si ya
/// estaba en esa sección; si no, al final de la sección).
pub fn index_put_line(text: &str, old_link: &str, section_name: &str, new_line: String) -> String {
    let eol = LineEnding::detect(text);
    let mut lines: Vec<String> = split_lines(text).into_iter().map(str::to_owned).collect();
    let entry_section = section_name;

    // ¿Ya existe? Se actualiza en el lugar si está en la sección correcta.
    let mut section: Option<String> = None;
    let mut existing: Option<(usize, Option<String>)> = None;
    for (i, l) in lines.iter().enumerate() {
        if let Some(h) = is_heading1(l) {
            section = Some(h.to_owned());
        } else if line_link(l) == Some(old_link) {
            existing = Some((i, section.clone()));
            break;
        }
    }
    if let Some((i, sec)) = existing {
        if sec.as_deref() == Some(entry_section) {
            lines[i] = new_line;
            return join(&lines, eol);
        }
        lines.remove(i);
    }

    // Al final de la sección (después de su última línea no vacía), o una sección nueva al final.
    let start = lines
        .iter()
        .position(|l| is_heading1(l) == Some(entry_section));
    match start {
        Some(h) => {
            let end = lines[h + 1..]
                .iter()
                .position(|l| is_heading1(l).is_some())
                .map_or(lines.len(), |p| h + 1 + p);
            let mut at = end;
            while at > h + 1 && lines[at - 1].trim().is_empty() {
                at -= 1;
            }
            if at == h + 1 {
                // Sección sin entradas: una línea en blanco después del título (sin duplicarla).
                if lines.get(h + 1).is_none_or(|l| !l.trim().is_empty()) {
                    lines.insert(h + 1, String::new());
                }
                at = h + 2;
            }
            lines.insert(at, new_line);
        }
        None => {
            while lines.last().is_some_and(|l| l.trim().is_empty()) {
                lines.pop();
            }
            if !lines.is_empty() {
                lines.push(String::new());
            }
            lines.push(format!("# {entry_section}"));
            lines.push(String::new());
            lines.push(new_line);
        }
    }
    join(&lines, eol)
}

/// Quita la entrada de `link`, si está.
pub fn index_remove(text: &str, link: &str) -> String {
    let eol = LineEnding::detect(text);
    let lines: Vec<String> = split_lines(text)
        .into_iter()
        .filter(|l| line_link(l) != Some(link))
        .map(str::to_owned)
        .collect();
    if lines.len() == split_lines(text).len() {
        return text.to_owned();
    }
    join(&lines, eol)
}

/// Cambia el prefijo de los enlaces del índice (al mover una carpeta).
pub fn index_relink_prefix(text: &str, old: &str, new: &str) -> String {
    let eol = LineEnding::detect(text);
    let mut changed = false;
    let lines: Vec<String> = split_lines(text)
        .into_iter()
        .map(|l| match line_link(l) {
            Some(link) if link.starts_with(old) => {
                changed = true;
                let new_link = format!("{new}{}", &link[old.len()..]);
                l.replacen(&md_link(link), &md_link(&new_link), 1)
            }
            _ => l.to_owned(),
        })
        .collect();
    if changed {
        join(&lines, eol)
    } else {
        text.to_owned()
    }
}

/// Agrega `* **verbo**: detalle` bajo `## fecha`, con el día más reciente primero.
pub fn log_add(text: &str, date: &str, verb: Verb, detail: &str) -> String {
    let eol = LineEnding::detect(text);
    let mut lines: Vec<String> = split_lines(text).into_iter().map(str::to_owned).collect();
    let item = format!("* **{}**: {detail}", verb.as_str());
    let heading = format!("## {date}");
    let first_day = lines.iter().position(|l| l.starts_with("## "));
    match first_day {
        Some(h) if lines[h].trim_end() == heading => {
            // Mismo día: al final de su bloque.
            let end = lines[h + 1..]
                .iter()
                .position(|l| l.starts_with("## ") || l.starts_with("# "))
                .map_or(lines.len(), |p| h + 1 + p);
            let mut at = end;
            while at > h + 1 && lines[at - 1].trim().is_empty() {
                at -= 1;
            }
            lines.insert(at, item);
        }
        Some(h) => {
            lines.splice(h..h, [heading, item, String::new()]);
        }
        None => {
            while lines.last().is_some_and(|l| l.trim().is_empty()) {
                lines.pop();
            }
            if !lines.is_empty() {
                lines.push(String::new());
            }
            lines.push(heading);
            lines.push(item);
        }
    }
    join(&lines, eol)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_links() {
        assert_eq!(line_link("* [A](/a.md) - x"), Some("/a.md"));
        assert_eq!(line_link("* [A \\](b)](/a.md)"), Some("/a.md"));
        assert_eq!(line_link("* [A](</a b.md>) - x"), Some("/a b.md"));
        assert_eq!(line_link("- [A](/a.md)"), None);
        assert_eq!(line_link("texto"), None);
    }

    #[test]
    fn link_of_path() {
        assert_eq!(
            index_link(&RelPath::new("wiki/a/b.md").unwrap()).as_deref(),
            Some("/a/b.md")
        );
        assert_eq!(index_link(&RelPath::new("raw/a.md").unwrap()), None);
    }
}
