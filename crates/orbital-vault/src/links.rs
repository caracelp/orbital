//! Resolución de destinos de enlaces (FR-017 a FR-019, contrato §6).
//!
//! Solo resuelve un destino a una ruta. Extraer los enlaces del texto y los retroenlaces es parte
//! del núcleo de notas (bloque A).

use crate::entry::{EntryKind, RelPath, Zone};
use crate::vault::Vault;

/// Resultado de resolver un enlace.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Resolution {
    Found(RelPath),
    /// Más de un archivo coincide (por ejemplo `idea.md` e `idea.html`).
    Ambiguous(Vec<RelPath>),
    /// No existe: no es un error, es una nota por escribir (OKF §6.1).
    Missing,
}

/// Decodifica `%XX` (rutas de enlaces Markdown).
fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let Ok(b) = u8::from_str_radix(
                std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap_or("zz"),
                16,
            )
        {
            out.push(b);
            i += 3;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8(out).unwrap_or_else(|_| s.to_owned())
}

/// Une `base` (carpeta, relativa a la bóveda) con `rel`, resolviendo `.` y `..`. `None` si se
/// sale de la bóveda.
fn join_normalized(base: &str, rel: &str) -> Option<String> {
    let mut parts: Vec<&str> = base.split('/').filter(|p| !p.is_empty()).collect();
    for p in rel.split('/') {
        match p {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            other => parts.push(other),
        }
    }
    (!parts.is_empty()).then(|| parts.join("/"))
}

fn stem(name: &str) -> &str {
    name.rsplit_once('.').map_or(name, |(s, _)| s)
}

impl Vault {
    /// Resuelve `target` visto desde la nota `from`.
    ///
    /// - `[[nombre]]`, `[[nombre|alias]]`, `[[nombre#encabezado]]`, `[[carpeta/nombre]]`: por
    ///   nombre de archivo dentro de `wiki/`, sin distinguir mayúsculas; con extensión, exacto.
    /// - `/ruta`: relativa a la raíz de `wiki/`; `/raw/...` va a las fuentes.
    /// - `./ruta`, `../ruta` o `ruta`: relativa a la carpeta de la nota.
    pub fn resolve_link(&self, from: &RelPath, target: &str) -> Resolution {
        let target = target.trim();
        if let Some(inner) = target.strip_prefix("[[").and_then(|t| t.strip_suffix("]]")) {
            return self.resolve_wikilink(inner);
        }
        let path = target.split(['#', '?']).next().unwrap_or("");
        let path = percent_decode(path.trim_start_matches('<').trim_end_matches('>'));
        let joined = if let Some(rest) = path.strip_prefix("/raw/") {
            join_normalized("raw", rest)
        } else if let Some(rest) = path.strip_prefix('/') {
            join_normalized("wiki", rest)
        } else {
            let dir = from.as_str().rsplit_once('/').map_or("", |(d, _)| d);
            join_normalized(dir, &path)
        };
        let Some(joined) = joined else {
            return Resolution::Missing;
        };
        match RelPath::new(&joined) {
            Ok(rel)
                if !rel.as_str().split('/').any(|c| c.starts_with('.'))
                    && rel.to_path(self.root()).is_file() =>
            {
                Resolution::Found(rel)
            }
            _ => Resolution::Missing,
        }
    }

    fn resolve_wikilink(&self, inner: &str) -> Resolution {
        let name = inner.split('|').next().unwrap_or("");
        let name = name.split(['#', '^']).next().unwrap_or("").trim();
        if name.is_empty() {
            return Resolution::Missing;
        }
        let (folder, file) = match name.rsplit_once('/') {
            Some((f, n)) => (Some(f.to_lowercase()), n),
            None => (None, name),
        };
        let has_ext =
            file.contains('.') && matches!(file.rsplit('.').next(), Some("md" | "html" | "htm"));
        let wanted = file.to_lowercase();
        let mut hits: Vec<RelPath> = self
            .entries()
            .iter()
            .filter(|e| {
                e.zone == Zone::Wiki
                    && matches!(e.kind, EntryKind::AtomNote | EntryKind::SynthesisNote)
            })
            .filter(|e| {
                let fname = e.path.file_name().to_lowercase();
                let name_ok = if has_ext {
                    fname == wanted
                } else {
                    stem(&fname) == wanted
                };
                let folder_ok = folder.as_ref().is_none_or(|f| {
                    let dir = e
                        .path
                        .as_str()
                        .rsplit_once('/')
                        .map_or("", |(d, _)| d)
                        .to_lowercase();
                    dir.ends_with(f.as_str())
                });
                name_ok && folder_ok
            })
            .map(|e| e.path.clone())
            .collect();
        match hits.len() {
            0 => Resolution::Missing,
            1 => Resolution::Found(hits.remove(0)),
            _ => Resolution::Ambiguous(hits),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn helpers() {
        assert_eq!(percent_decode("a%20b%C3%B3"), "a bó");
        assert_eq!(percent_decode("100%"), "100%");
        assert_eq!(
            join_normalized("wiki/a", "../b.md").as_deref(),
            Some("wiki/b.md")
        );
        assert_eq!(join_normalized("wiki", "../../x"), None);
    }
}
