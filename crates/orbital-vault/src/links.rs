//! Resolución, extracción y reescritura de destinos de enlaces (FR-001 a FR-006, contrato §6).

use crate::entry::{EntryKind, RelPath, Zone};
use crate::vault::Vault;

/// Tipo sintáctico del enlace.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum LinkKind {
    /// Enlace interno tipo wikilink: `[[destino]]` o `[[destino|alias]]`
    Wikilink,
    /// Enlace estándar de CommonMark / Markdown: `[texto](destino)`
    Markdown,
    /// Transclusión de contenido: `![[destino]]` o `![[destino#encabezado]]`
    Transclusion,
}

/// Destino desglosado de un enlace.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct LinkTarget {
    /// Ruta o identificador base (ej: "Entropía", "fisica/termodinamica.md", "/raw/libro.pdf")
    pub path: String,
    /// Encabezado o ancla opcional (ej: "Segunda ley", sin `#`)
    pub anchor: Option<String>,
    /// Texto de reemplazo / alias si está explícito (ej: "mi alias")
    pub alias: Option<String>,
}

/// Instancia de enlace encontrada en un documento.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LinkOccurrence {
    pub kind: LinkKind,
    pub target: LinkTarget,
    /// Número de línea en el archivo fuente (base 1)
    pub line: usize,
    /// Fragmento contextual: línea completa donde aparece el enlace
    pub snippet: String,
    /// Rango de bytes en el texto fuente
    pub byte_range: std::ops::Range<usize>,
}

/// Referencia inversa (retroenlace / backlink).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BacklinkRef {
    /// Archivo origen donde se encuentra el enlace
    pub from: RelPath,
    /// Destino especificado en el enlace
    pub target: LinkTarget,
    /// Línea en la nota origen (base 1)
    pub line: usize,
    /// Fragmento contextual
    pub snippet: String,
}

/// Resultado de resolver un enlace.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Resolution {
    Found(RelPath),
    /// Más de un archivo coincide (por ejemplo `idea.md` e `idea.html`).
    Ambiguous(Vec<RelPath>),
    /// No existe: no es un error, es una nota por escribir (OKF §6.1).
    Missing,
}

/// Parsea un destino crudo de enlace (como `[[Destino#ancla|alias]]` o `/wiki/nota.md#ancla`).
pub fn parse_link_target(raw: &str) -> LinkTarget {
    let s = raw.trim();
    let s = s
        .strip_prefix("![[")
        .and_then(|t| t.strip_suffix("]]"))
        .or_else(|| s.strip_prefix("[[").and_then(|t| t.strip_suffix("]]")))
        .unwrap_or(s);

    let (before_pipe, alias) = match s.split_once('|') {
        Some((b, a)) => (b.trim(), Some(a.trim().to_string())),
        None => (s.trim(), None),
    };

    let (path, anchor) = match before_pipe.split_once('#') {
        Some((p, a)) => (p.trim(), Some(a.trim().to_string())),
        None => (before_pipe, None),
    };

    LinkTarget {
        path: path.to_string(),
        anchor,
        alias,
    }
}

/// Decodifica `%XX` (rutas de enlaces Markdown).
pub fn percent_decode(s: &str) -> String {
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

    pub fn resolve_wikilink(&self, inner: &str) -> Resolution {
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

/// Extrae todos los enlaces (wikilinks, markdown y transclusiones) de un texto.
pub fn extract_links(text: &str, is_html: bool) -> Vec<LinkOccurrence> {
    if is_html {
        extract_links_html(text)
    } else {
        extract_links_markdown(text)
    }
}

/// Extrae enlaces de un documento Markdown, ignorando bloques de código.
fn extract_links_markdown(text: &str) -> Vec<LinkOccurrence> {
    let mut out = Vec::new();
    let mut in_fence = false;
    let mut byte_offset = 0;

    for (line_idx, line) in text.split('\n').enumerate() {
        let line_num = line_idx + 1;
        let line_len = line.len();
        let trimmed = line.trim();

        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            in_fence = !in_fence;
            byte_offset += line_len + 1;
            continue;
        }

        if !in_fence {
            // Máscara simple de código en línea: caracteres entre backticks en la misma línea
            let mut masked = line.to_string();
            let mut in_code = false;
            let mut mask_chars = Vec::new();
            for c in masked.chars() {
                if c == '`' {
                    in_code = !in_code;
                    mask_chars.push(' ');
                } else if in_code {
                    mask_chars.push(' ');
                } else {
                    mask_chars.push(c);
                }
            }
            masked = mask_chars.into_iter().collect();

            // 1. Transclusiones: ![[...]]
            let mut search_from = 0;
            while let Some(rel_start) = masked[search_from..].find("![[") {
                let start_idx = search_from + rel_start;
                if let Some(rel_end) = masked[start_idx + 3..].find("]]") {
                    let end_idx = start_idx + 3 + rel_end + 2;
                    let inner = &line[start_idx + 3..end_idx - 2];
                    out.push(LinkOccurrence {
                        kind: LinkKind::Transclusion,
                        target: parse_link_target(inner),
                        line: line_num,
                        snippet: line.trim().to_string(),
                        byte_range: (byte_offset + start_idx)..(byte_offset + end_idx),
                    });
                    search_from = end_idx;
                } else {
                    break;
                }
            }

            // 2. Wikilinks: [[...]] (cuidando de no tomar ![[)
            let mut search_from = 0;
            while let Some(rel_start) = masked[search_from..].find("[[") {
                let start_idx = search_from + rel_start;
                if start_idx > 0 && masked.as_bytes()[start_idx - 1] == b'!' {
                    search_from = start_idx + 2;
                    continue;
                }
                if let Some(rel_end) = masked[start_idx + 2..].find("]]") {
                    let end_idx = start_idx + 2 + rel_end + 2;
                    let inner = &line[start_idx + 2..end_idx - 2];
                    out.push(LinkOccurrence {
                        kind: LinkKind::Wikilink,
                        target: parse_link_target(inner),
                        line: line_num,
                        snippet: line.trim().to_string(),
                        byte_range: (byte_offset + start_idx)..(byte_offset + end_idx),
                    });
                    search_from = end_idx;
                } else {
                    break;
                }
            }

            // 3. Markdown: [texto](url) (cuidando de no tomar ![alt](img))
            let mut search_from = 0;
            while let Some(rel_start) = masked[search_from..].find('[') {
                let start_idx = search_from + rel_start;
                if (start_idx > 0 && masked.as_bytes()[start_idx - 1] == b'!')
                    || (start_idx + 1 < masked.len() && masked.as_bytes()[start_idx + 1] == b'[')
                {
                    search_from = start_idx + 1;
                    continue;
                }
                if let Some(rel_text_end) = masked[start_idx + 1..].find(']') {
                    let text_end = start_idx + 1 + rel_text_end;
                    if masked[text_end..].starts_with("](")
                        && let Some(rel_url_end) = masked[text_end + 2..].find(')')
                    {
                        let url_end = text_end + 2 + rel_url_end;
                        let text_content = &line[start_idx + 1..text_end];
                        let url_content = &line[text_end + 2..url_end];

                        let mut target = parse_link_target(url_content);
                        if target.alias.is_none() && !text_content.is_empty() {
                            target.alias = Some(text_content.to_string());
                        }

                        out.push(LinkOccurrence {
                            kind: LinkKind::Markdown,
                            target,
                            line: line_num,
                            snippet: line.trim().to_string(),
                            byte_range: (byte_offset + start_idx)..(byte_offset + url_end + 1),
                        });
                        search_from = url_end + 1;
                        continue;
                    }
                }
                search_from = start_idx + 1;
            }
        }

        byte_offset += line_len + 1;
    }

    out.sort_by_key(|o| o.byte_range.start);
    out
}

/// Extrae enlaces de un documento HTML (etiquetas `<a>` y wikilinks en texto).
fn extract_links_html(text: &str) -> Vec<LinkOccurrence> {
    let mut out = Vec::new();
    let mut byte_offset = 0;

    for (line_idx, line) in text.split('\n').enumerate() {
        let line_num = line_idx + 1;
        let line_len = line.len();

        // 1. Buscar <a ... href="..." ...> ... </a>
        let mut search_from = 0;
        let lower = line.to_lowercase();
        while let Some(tag_start) = lower[search_from..].find("<a ") {
            let start_idx = search_from + tag_start;
            if let Some(tag_end) = lower[start_idx..].find('>') {
                let tag_str = &line[start_idx..start_idx + tag_end];
                // Extraer href
                if let Some(href_idx) = tag_str.to_lowercase().find("href=") {
                    let rest = &tag_str[href_idx + 5..].trim_start();
                    let (quote, quote_len) = if rest.starts_with('"') {
                        ('"', 1)
                    } else if rest.starts_with('\'') {
                        ('\'', 1)
                    } else {
                        (' ', 0)
                    };
                    let href_val = if quote_len > 0 {
                        rest[1..].split(quote).next().unwrap_or("")
                    } else {
                        rest.split([' ', '>']).next().unwrap_or("")
                    };

                    // Extraer texto interior hasta </a>
                    let close_tag = "</a>";
                    let (text_content, end_idx) =
                        if let Some(close_idx) = lower[start_idx + tag_end + 1..].find(close_tag) {
                            (
                                &line[start_idx + tag_end + 1..start_idx + tag_end + 1 + close_idx],
                                start_idx + tag_end + 1 + close_idx + close_tag.len(),
                            )
                        } else {
                            ("", start_idx + tag_end + 1)
                        };

                    let mut target = parse_link_target(href_val);
                    if target.alias.is_none() && !text_content.is_empty() {
                        target.alias = Some(text_content.to_string());
                    }

                    out.push(LinkOccurrence {
                        kind: LinkKind::Markdown,
                        target,
                        line: line_num,
                        snippet: line.trim().to_string(),
                        byte_range: (byte_offset + start_idx)..(byte_offset + end_idx),
                    });

                    search_from = end_idx;
                    continue;
                }
            }
            search_from = start_idx + 3;
        }

        // 2. Transclusiones en HTML: ![[...]]
        let mut search_from = 0;
        while let Some(rel_start) = line[search_from..].find("![[") {
            let start_idx = search_from + rel_start;
            if let Some(rel_end) = line[start_idx + 3..].find("]]") {
                let end_idx = start_idx + 3 + rel_end + 2;
                let inner = &line[start_idx + 3..end_idx - 2];
                out.push(LinkOccurrence {
                    kind: LinkKind::Transclusion,
                    target: parse_link_target(inner),
                    line: line_num,
                    snippet: line.trim().to_string(),
                    byte_range: (byte_offset + start_idx)..(byte_offset + end_idx),
                });
                search_from = end_idx;
            } else {
                break;
            }
        }

        // 3. Wikilinks en HTML: [[...]]
        let mut search_from = 0;
        while let Some(rel_start) = line[search_from..].find("[[") {
            let start_idx = search_from + rel_start;
            if start_idx > 0 && line.as_bytes()[start_idx - 1] == b'!' {
                search_from = start_idx + 2;
                continue;
            }
            if let Some(rel_end) = line[start_idx + 2..].find("]]") {
                let end_idx = start_idx + 2 + rel_end + 2;
                let inner = &line[start_idx + 2..end_idx - 2];
                out.push(LinkOccurrence {
                    kind: LinkKind::Wikilink,
                    target: parse_link_target(inner),
                    line: line_num,
                    snippet: line.trim().to_string(),
                    byte_range: (byte_offset + start_idx)..(byte_offset + end_idx),
                });
                search_from = end_idx;
            } else {
                break;
            }
        }

        byte_offset += line_len + 1;
    }

    out.sort_by_key(|o| o.byte_range.start);
    out
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
