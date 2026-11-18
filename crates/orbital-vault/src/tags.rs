//! Extracción y renombrado de etiquetas (R7, spec 004).

use std::ops::Range;

use crate::props::{self, PropValue, html, markdown, yaml};
use crate::{EditPlan, FileEdit, RelPath, Vault, VaultError};

/// Ocurrencia de una etiqueta en un documento.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TagOccurrence {
    /// Nombre de la etiqueta sin '#' (ej: "fisica/mecanica").
    pub tag: String,
    /// Forma original encontrada.
    pub original: String,
    /// Rango de bytes en el texto fuente (0..0 para etiquetas provenientes de frontmatter).
    pub byte_range: Range<usize>,
    /// Si proviene de las propiedades/frontmatter.
    pub from_prop: bool,
}

/// Árbol jerárquico de etiquetas para el panel lateral.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct TagTree {
    pub roots: Vec<TagNode>,
}

/// Nodo de una etiqueta en el árbol.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TagNode {
    /// El tramo del nombre («Física»).
    pub name: String,
    /// Ruta completa normalizada («fisica/termo»).
    pub path: String,
    /// Cantidad de notas con esta etiqueta o alguna hija.
    pub notes: usize,
    /// Nodos hijos.
    pub children: Vec<TagNode>,
}

/// Nombres nuevos de etiquetas inline: todos los tramos deben poder escribirse tras `#`.
pub fn valid_tag_name(name: &str) -> bool {
    is_valid_tag(name)
        && name.chars().all(is_valid_tag_char)
        && name.split('/').all(|part| !part.is_empty())
}

fn renamed_tag(tag: &str, old: &str, new: &str) -> Option<String> {
    let folded = crate::fold(tag);
    if folded == old {
        return Some(new.to_owned());
    }
    if folded
        .strip_prefix(old)
        .is_some_and(|rest| rest.starts_with('/'))
    {
        // fold conserva caracteres, pero no bytes: calcular el sufijo sobre separadores originales.
        let boundary = tag.match_indices('/').nth(old.split('/').count() - 1)?.0;
        return Some(format!("{new}{}", &tag[boundary..]));
    }
    None
}

fn rename_prop_tags(value: &PropValue, old: &str, new: &str) -> (PropValue, usize) {
    fn merge(values: Vec<(PropValue, bool)>) -> (Vec<PropValue>, usize) {
        let affected: std::collections::HashSet<_> = values
            .iter()
            .filter(|(_, changed)| *changed)
            .filter_map(|(value, _)| {
                value
                    .as_text()
                    .map(|text| crate::fold(text.trim_start_matches('#')))
            })
            .collect();
        let count = values.iter().filter(|(_, changed)| *changed).count();
        let mut seen = std::collections::HashSet::new();
        let values = values
            .into_iter()
            .filter(|(value, _)| {
                value.as_text().is_none_or(|text| {
                    let key = crate::fold(text.trim_start_matches('#'));
                    !affected.contains(&key) || seen.insert(key)
                })
            })
            .map(|(value, _)| value)
            .collect();
        (values, count)
    }
    let rename = |value: &PropValue| {
        let changed = value.as_text().and_then(|text| {
            let raw = text.trim().trim_start_matches('#').trim();
            renamed_tag(raw, old, new).map(|name| {
                if text.trim().starts_with('#') {
                    format!("#{name}")
                } else {
                    name
                }
            })
        });
        match changed {
            Some(name) => (PropValue::Text(name), true),
            None => (value.clone(), false),
        }
    };
    match value {
        PropValue::Text(text) => {
            let (values, count) = merge(
                text.split([',', ';'])
                    .map(|part| rename(&PropValue::Text(part.trim().to_owned())))
                    .collect(),
            );
            (
                PropValue::Text(
                    values
                        .iter()
                        .filter_map(PropValue::as_text)
                        .collect::<Vec<_>>()
                        .join(", "),
                ),
                count,
            )
        }
        PropValue::List(values) => {
            let (values, count) = merge(values.iter().map(rename).collect());
            (PropValue::List(values), count)
        }
        other => (other.clone(), 0),
    }
}

/// Rangos del texto donde una etiqueta o un marcador no cuentan: código (cercas, en línea,
/// bloques), enlaces e imágenes Markdown, y en HTML los elementos `a`, `code`, `pre`, `script`,
/// `style`, `textarea` y `title`. Ordenados y sin solapamientos.
pub(crate) fn excluded_ranges(text: &str, is_html: bool) -> Vec<Range<usize>> {
    let mut excluded: Vec<Range<usize>> = Vec::new();
    if is_html {
        let mut open: Vec<(String, usize)> = Vec::new();
        for tag in crate::html_edit::tags(text, 0) {
            if matches!(
                tag.name.as_str(),
                "a" | "code" | "pre" | "script" | "style" | "textarea" | "title"
            ) {
                open.push((tag.name, tag.range.start));
            } else if let Some(name) = tag.name.strip_prefix('/')
                && let Some(position) = open.iter().rposition(|(current, _)| current == name)
            {
                let (_, start) = open.remove(position);
                excluded.push(start..tag.range.end);
            }
        }
        excluded.extend(open.into_iter().map(|(_, start)| start..text.len()));
    } else {
        use pulldown_cmark::{Event, Parser, Tag};
        excluded.extend(
            Parser::new(text)
                .into_offset_iter()
                .filter_map(|(event, range)| {
                    matches!(
                        event,
                        Event::Start(Tag::Link { .. } | Tag::Image { .. } | Tag::CodeBlock(_))
                            | Event::Code(_)
                    )
                    .then_some(range)
                }),
        );
    }
    excluded.sort_unstable_by_key(|range| range.start);
    let mut merged: Vec<Range<usize>> = Vec::new();
    for range in excluded {
        if let Some(last) = merged.last_mut()
            && range.start <= last.end
        {
            last.end = last.end.max(range.end);
        } else {
            merged.push(range);
        }
    }
    merged
}

/// Verdadero si el byte en `start` está escapado por una cantidad impar de barras.
pub(crate) fn escaped(text: &str, start: usize) -> bool {
    text[..start]
        .bytes()
        .rev()
        .take_while(|byte| *byte == b'\\')
        .count()
        % 2
        == 1
}

fn editable_body_tags(text: &str, is_html: bool) -> Vec<TagOccurrence> {
    let mut tags: Vec<_> = extract_tags(text, is_html)
        .into_iter()
        .filter(|tag| !tag.from_prop)
        .collect();
    let merged = excluded_ranges(text, is_html);
    let mut index = 0;
    tags.retain(|tag| {
        while index < merged.len() && merged[index].end <= tag.byte_range.start {
            index += 1;
        }
        (index == merged.len() || merged[index].start >= tag.byte_range.end)
            && !escaped(text, tag.byte_range.start)
    });
    tags
}

impl Vault {
    /// Planifica sin escribir; cada fuente debe ser una nota wiki conocida y contenida en la bóveda.
    /// Las omisiones se exponen en `errors`; la interfaz debe mostrarlas antes de pedir confirmación.
    pub fn plan_tag_rename(&self, old: &str, new: &str, sources: &[RelPath]) -> EditPlan {
        let old = crate::fold(old.trim().trim_start_matches('#'));
        let new = new.trim().trim_start_matches('#');
        let mut plan = EditPlan {
            root: self.root().canonicalize().ok(),
            ..Default::default()
        };
        if old.is_empty() || !valid_tag_name(new) {
            plan.errors.push((
                sources
                    .first()
                    .cloned()
                    .unwrap_or_else(|| RelPath::from_static("wiki")),
                "El nombre de etiqueta no es válido".into(),
            ));
            return plan;
        }
        let entries: std::collections::HashMap<_, _> = self
            .entries()
            .iter()
            .map(|entry| (&entry.path, entry))
            .collect();
        let mut seen = std::collections::HashSet::new();
        for path in sources {
            if !seen.insert(path) {
                continue;
            }
            let change = (|| -> Result<Option<FileEdit>, VaultError> {
                let entry = entries
                    .get(path)
                    .ok_or_else(|| VaultError::InvalidPath(path.to_string()))?;
                if entry.zone != crate::Zone::Wiki
                    || entry.read_only
                    || crate::Format::from_name(path.file_name()).is_none()
                {
                    return Err(VaultError::ReadOnly(path.clone()));
                }
                crate::edit_plan::checked_path(self.root(), path)?;
                let note = self.read_note(path)?;
                if note.read_only {
                    return Err(VaultError::ReadOnly(path.clone()));
                }
                if let Some(error) = note.props.as_ref().and_then(|props| props.error.as_ref()) {
                    return Err(VaultError::InvalidProps {
                        path: path.clone(),
                        line: error.line,
                        message: error.message.clone(),
                    });
                }
                let mut count = 0;
                let mut updated = String::with_capacity(note.source.len());
                let mut at = 0;
                for tag in editable_body_tags(&note.source, note.format == crate::Format::Html) {
                    if let Some(name) = renamed_tag(&tag.tag, &old, new) {
                        updated.push_str(&note.source[at..tag.byte_range.start]);
                        updated.push('#');
                        updated.push_str(&name);
                        at = tag.byte_range.end;
                        count += 1;
                    }
                }
                updated.push_str(&note.source[at..]);
                if let Some(tags) = note.prop("tags") {
                    let (value, changed) = rename_prop_tags(tags, &old, new);
                    if changed > 0 {
                        updated = note.reparse(updated)?.with_prop("tags", &value)?;
                        count += changed;
                    }
                }
                Ok((count > 0 && updated != note.source).then_some(FileEdit {
                    path: path.clone(),
                    count,
                    original: note.source,
                    updated,
                }))
            })();
            match change {
                Ok(Some(change)) => plan.changes.push(change),
                Ok(None) => {}
                Err(error) => plan.errors.push((path.clone(), error.to_string())),
            }
        }
        plan.changes.sort_by(|a, b| a.path.cmp(&b.path));
        plan
    }
}

/// Extrae todas las etiquetas de un documento (.md o .html).
pub fn extract_tags(text: &str, is_html: bool) -> Vec<TagOccurrence> {
    let mut out = extract_frontmatter_tags(text, is_html);
    if is_html {
        out.extend(extract_html_body_tags(text));
    } else {
        out.extend(extract_markdown_body_tags(text));
    }
    out
}

/// Prefijo de etiqueta bajo el cursor (bytes). El análisis sintáctico descarta código,
/// encabezados y enlaces, incluso incompletos. Se llama solo al encontrar `#` en la línea actual.
pub fn completion_query(text: &str, cursor: usize, is_html: bool) -> Option<(&str, usize)> {
    let prefix = text.get(..cursor)?;
    let line_start = prefix.rfind('\n').map_or(0, |p| p + 1);
    let line = &prefix[line_start..];
    let hash = line.rfind('#')?;
    let query = &line[hash + 1..];
    if !query.chars().all(is_valid_tag_char) {
        return None;
    }
    if line[..hash]
        .chars()
        .next_back()
        .is_some_and(|c| c.is_alphanumeric() || "_/:.&=?#\\".contains(c))
    {
        return None;
    }
    // No sugerir dentro de enlaces todavía sin cerrar.
    if line[..hash]
        .rfind('[')
        .is_some_and(|open| !line[open..hash].contains(']'))
        || line[..hash]
            .rfind("](")
            .is_some_and(|open| !line[open + 2..hash].contains(')'))
    {
        return None;
    }
    let start = line_start + hash;
    if is_html {
        // El centinela permite analizar también un `#` sin letras, con el mismo escáner del índice.
        let sample = format!("{prefix}z</body>");
        return extract_tags(&sample, true)
            .iter()
            .any(|t| !t.from_prop && t.byte_range.start == start)
            .then_some((query, start));
    }
    if markdown::locate(text).is_some_and(|range| start <= range.end) {
        return None;
    }
    // Mientras se escribe, un delimitador sin cerrar expresa intención de código aunque
    // CommonMark todavía lo interprete como texto literal.
    let before = &line[..hash];
    let mut open_ticks = 0;
    let mut bytes = before.bytes().peekable();
    while let Some(byte) = bytes.next() {
        if byte == b'\\' {
            bytes.next();
        } else if byte == b'`' {
            let mut width = 1;
            while bytes.peek() == Some(&b'`') {
                bytes.next();
                width += 1;
            }
            if open_ticks == 0 {
                open_ticks = width;
            } else if open_ticks == width {
                open_ticks = 0;
            }
        }
    }
    if open_ticks != 0 {
        return None;
    }
    use pulldown_cmark::{Event, Parser, Tag};
    let mut excluded = 0;
    for (event, range) in Parser::new(text).into_offset_iter() {
        match event {
            Event::Start(
                Tag::CodeBlock(_) | Tag::Heading { .. } | Tag::Link { .. } | Tag::Image { .. },
            ) => excluded += 1,
            Event::End(
                pulldown_cmark::TagEnd::CodeBlock
                | pulldown_cmark::TagEnd::Heading(_)
                | pulldown_cmark::TagEnd::Link
                | pulldown_cmark::TagEnd::Image,
            ) => excluded -= 1,
            Event::Text(_) if excluded == 0 && range.start <= start && cursor <= range.end => {
                return Some((query, start));
            }
            _ => {}
        }
        if range.start > cursor {
            break;
        }
    }
    None
}

fn extract_frontmatter_tags(text: &str, is_html: bool) -> Vec<TagOccurrence> {
    let mut out = Vec::new();
    let region = if is_html {
        html::scan(text).block
    } else {
        markdown::locate(text)
    };
    let Some(region) = region else {
        return out;
    };

    if let Ok(entries) = yaml::parse_block(&text[region])
        && let Some(entry) = props::find(&entries, "tags")
    {
        match &entry.value {
            PropValue::Text(s) => {
                for piece in s.split([',', ';']) {
                    let t = piece.trim().trim_start_matches('#').trim();
                    if !t.is_empty() {
                        out.push(TagOccurrence {
                            tag: t.to_string(),
                            original: t.to_string(),
                            byte_range: 0..0,
                            from_prop: true,
                        });
                    }
                }
            }
            PropValue::List(items) => {
                for item in items {
                    if let Some(s) = item.as_text() {
                        let t = s.trim().trim_start_matches('#').trim();
                        if !t.is_empty() {
                            out.push(TagOccurrence {
                                tag: t.to_string(),
                                original: t.to_string(),
                                byte_range: 0..0,
                                from_prop: true,
                            });
                        }
                    }
                }
            }
            _ => {}
        }
    }
    out
}

fn is_valid_tag_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_' || c == '-' || c == '/'
}

fn is_valid_tag(tag: &str) -> bool {
    if tag.is_empty() {
        return false;
    }
    // Debe tener al menos un carácter que no sea dígito
    if !tag.chars().any(|c| !c.is_ascii_digit()) {
        return false;
    }
    // No debe ser un color hex de 3, 6 u 8 dígitos
    let len = tag.len();
    !((len == 3 || len == 6 || len == 8) && tag.chars().all(|c| c.is_ascii_hexdigit()))
}

fn extract_markdown_body_tags(text: &str) -> Vec<TagOccurrence> {
    let mut out = Vec::new();
    let mut in_fence = false;
    let mut byte_offset = 0;

    let body_start = if let Some(fm) = markdown::locate(text) {
        text[fm.end..]
            .find('\n')
            .map_or(text.len(), |i| fm.end + i + 1)
    } else {
        0
    };

    for line in text.split('\n') {
        let line_len = line.len();
        let line_start = byte_offset;
        let line_end = byte_offset + line_len;
        byte_offset += line_len + 1; // +1 for '\n'

        if line_end <= body_start {
            continue;
        }

        let trimmed = line.trim();
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            in_fence = !in_fence;
            continue;
        }
        if in_fence {
            continue;
        }

        // Máscara para ignorar código en línea, wikilinks y enlaces markdown
        let masked = mask_markdown_line(line);

        let mut idx = 0;
        let bytes = masked.as_bytes();
        while idx < bytes.len() {
            if bytes[idx] == b'#' {
                // Verificar caracter previo
                let prev_ok = if idx == 0 {
                    true
                } else {
                    let prev = bytes[idx - 1] as char;
                    !prev.is_alphanumeric()
                        && prev != '/'
                        && prev != ':'
                        && prev != '.'
                        && prev != '&'
                        && prev != '='
                        && prev != '?'
                        && prev != '#'
                };

                if prev_ok {
                    // Extraer candidatos desde line[idx + 1..]
                    let tag_content = &line[idx + 1..];
                    let mut tag_len = 0;
                    for c in tag_content.chars() {
                        if is_valid_tag_char(c) {
                            tag_len += c.len_utf8();
                        } else {
                            break;
                        }
                    }

                    let raw_tag = &tag_content[..tag_len];
                    // Quitar posibles / o - finales
                    let trimmed_tag = raw_tag.trim_end_matches('/');
                    if is_valid_tag(trimmed_tag) {
                        let final_tag_len = trimmed_tag.len();
                        let start_byte = line_start + idx;
                        let end_byte = start_byte + 1 + final_tag_len;
                        out.push(TagOccurrence {
                            tag: trimmed_tag.to_string(),
                            original: trimmed_tag.to_string(),
                            byte_range: start_byte..end_byte,
                            from_prop: false,
                        });
                        idx += 1 + tag_len;
                        continue;
                    }
                }
            }
            idx += 1;
        }
    }

    out
}

pub(crate) fn mask_markdown_line(line: &str) -> String {
    // Cada byte enmascarado ocupa un espacio: los rangos siguen apuntando al fuente UTF-8.
    let mut bytes = line.as_bytes().to_vec();
    let mut in_code = false;
    for byte in &mut bytes {
        if *byte == b'`' {
            in_code = !in_code;
            *byte = b' ';
        } else if in_code {
            *byte = b' ';
        }
    }
    let mut masked = String::from_utf8(bytes).expect("la máscara conserva UTF-8 fuera del código");

    // 2. Enmascarar enlaces wikilink [[...]]
    while let Some(start) = masked.find("[[") {
        if let Some(end) = masked[start + 2..].find("]]") {
            let full_end = start + 2 + end + 2;
            masked.replace_range(start..full_end, &" ".repeat(full_end - start));
        } else {
            break;
        }
    }

    // 3. Enmascarar enlaces markdown [texto](url)
    let mut search_from = 0;
    while let Some(rel_start) = masked[search_from..].find('[') {
        let start = search_from + rel_start;
        if let Some(rel_mid) = masked[start + 1..].find("](") {
            let mid = start + 1 + rel_mid;
            if let Some(rel_end) = masked[mid + 2..].find(')') {
                let end = mid + 2 + rel_end + 1;
                masked.replace_range(start..end, &" ".repeat(end - start));
                search_from = end;
                continue;
            }
        }
        search_from = start + 1;
    }

    // 4. Enmascarar URLs comunes https:// o http://
    for proto in &["http://", "https://"] {
        let mut s_from = 0;
        while let Some(rel_s) = masked[s_from..].find(proto) {
            let start = s_from + rel_s;
            let end = masked[start..]
                .find(|c: char| c.is_whitespace() || c == ')' || c == ']' || c == '>')
                .map_or(masked.len(), |i| start + i);
            masked.replace_range(start..end, &" ".repeat(end - start));
            s_from = end;
        }
    }

    masked
}

fn extract_html_body_tags(text: &str) -> Vec<TagOccurrence> {
    let mut out = Vec::new();
    let lower = text.to_ascii_lowercase();
    let body_start = lower.find("<body").unwrap_or(0);
    let body_end = lower.rfind("</body>").unwrap_or(text.len());

    let mut in_code = false;
    let mut in_tag = false;
    let mut in_comment = false;
    let mut in_script_or_style = false;

    let bytes = text.as_bytes();
    let mut i = body_start;

    while i < body_end {
        if !text.is_char_boundary(i) {
            i += 1;
            continue;
        }

        if !in_comment && !in_tag && text[i..].starts_with("<!--") {
            in_comment = true;
            i += 4;
            continue;
        }
        if in_comment {
            if text[i..].starts_with("-->") {
                in_comment = false;
                i += 3;
            } else {
                i += 1;
            }
            continue;
        }

        if !in_tag && bytes[i] == b'<' {
            in_tag = true;
            let rest = &lower[i..];
            if rest.starts_with("<script") || rest.starts_with("<style") {
                in_script_or_style = true;
            } else if rest.starts_with("</script>") || rest.starts_with("</style>") {
                in_script_or_style = false;
            } else if rest.starts_with("<code") || rest.starts_with("<pre") {
                in_code = true;
            } else if rest.starts_with("</code>") || rest.starts_with("</pre>") {
                in_code = false;
            }
            i += 1;
            continue;
        }

        if in_tag {
            if bytes[i] == b'>' {
                in_tag = false;
            }
            i += 1;
            continue;
        }

        if in_script_or_style || in_code {
            i += 1;
            continue;
        }

        if bytes[i] == b'#' {
            let prev_ok = if i == 0 {
                true
            } else {
                let prev = bytes[i - 1] as char;
                !prev.is_alphanumeric()
                    && prev != '/'
                    && prev != ':'
                    && prev != '.'
                    && prev != '&'
                    && prev != '='
                    && prev != '?'
                    && prev != '#'
            };

            if prev_ok {
                let tag_content = &text[i + 1..body_end];
                let mut tag_len = 0;
                for c in tag_content.chars() {
                    if is_valid_tag_char(c) {
                        tag_len += c.len_utf8();
                    } else {
                        break;
                    }
                }

                let raw_tag = &tag_content[..tag_len];
                let trimmed_tag = raw_tag.trim_end_matches('/');
                if is_valid_tag(trimmed_tag) {
                    let final_tag_len = trimmed_tag.len();
                    out.push(TagOccurrence {
                        tag: trimmed_tag.to_string(),
                        original: trimmed_tag.to_string(),
                        byte_range: i..(i + 1 + final_tag_len),
                        from_prop: false,
                    });
                    i += 1 + tag_len;
                    continue;
                }
            }
        }

        i += 1;
    }

    out
}

/// Extrae campos en línea `clave:: valor` de un documento Markdown. Los renglones de la parte
/// Conexiones (`<!-- connections ID -->`, 041) no son campos: ahí `tipo:: [[nota]]` declara una
/// relación con tipo (049), que se busca con `rel:`.
pub fn inline_fields(text: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut in_fence = false;
    let mut conexiones =
        crate::relaciones::tiene_conexiones(text).then(crate::relaciones::Rastreador::default);

    for line in text.lines() {
        if let Some(r) = conexiones.as_mut()
            && r.paso(line)
        {
            continue;
        }
        let trimmed = line.trim();
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            in_fence = !in_fence;
            continue;
        }
        if in_fence {
            continue;
        }

        let trimmed_start = line.trim_start();
        if trimmed_start.starts_with('`') {
            continue;
        }

        // Quitar posible marcador de lista: '-', '*', '+', o numerado '1.', '1)'
        let mut rest = trimmed_start;
        if let Some(after) = rest
            .strip_prefix("- ")
            .or_else(|| rest.strip_prefix("* "))
            .or_else(|| rest.strip_prefix("+ "))
        {
            rest = after.trim_start();
        } else {
            let num_prefix_len = rest.bytes().take_while(|b| b.is_ascii_digit()).count();
            if num_prefix_len > 0 && num_prefix_len < rest.len() {
                let after_num = &rest[num_prefix_len..];
                if after_num.starts_with(". ") || after_num.starts_with(") ") {
                    rest = after_num[2..].trim_start();
                }
            }
        }

        if let Some((raw_key, raw_val)) = rest.split_once("::") {
            let key = raw_key.trim();
            if !key.is_empty()
                && !raw_key.ends_with(' ')
                && !raw_key.starts_with(' ')
                && !key.contains(':')
                && !key.contains('`')
                && !key.chars().any(char::is_whitespace)
            {
                let val = raw_val.trim();
                out.push((key.to_string(), val.to_string()));
            }
        }
    }

    out
}
