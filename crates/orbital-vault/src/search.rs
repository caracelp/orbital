//! Índice de búsqueda de la bóveda (R1, R3, spec 004).

use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::Read;
use std::ops::Range;

use memchr::memmem;

use crate::entry::{RelPath, Zone};
use crate::fold::fold;
use crate::html_text::html_text;
use crate::props::{self, PropValue, html, markdown, yaml};
use crate::query::{Atom, Query};
use crate::tags::{TagNode, TagTree, extract_tags, inline_fields};
use crate::vault::Vault;

const MAX_INDEXABLE_BYTES: usize = 5 * 1024 * 1024; // 5 MB

/// Resultado de una consulta de búsqueda.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SearchResults {
    pub notes: Vec<NoteHit>,
    pub total_notes: usize,
    pub total_matches: usize,
}

/// Coincidencia en una nota.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NoteHit {
    pub path: RelPath,
    pub zone: Zone,
    pub score: u32,
    pub matches: Vec<Match>,
    pub more: usize,
}

/// Coincidencia puntual en una línea.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Match {
    /// Número de línea en el archivo fuente (base 1).
    pub line: usize,
    /// Fragmento contextual (la línea original).
    pub snippet: String,
    /// Rangos de caracteres a resaltar en `snippet`.
    pub highlights: Vec<Range<usize>>,
}

/// Opciones de búsqueda.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SearchOptions {
    pub include_raw: bool,
    pub max_notes: usize,
    pub max_matches: usize,
}

impl Default for SearchOptions {
    fn default() -> Self {
        Self {
            include_raw: false,
            max_notes: 200,
            max_matches: 20,
        }
    }
}

/// Documento indexado en memoria (R1).
#[derive(Clone, Debug)]
pub struct Doc {
    pub zone: Zone,
    pub folded: String,
    pub line_starts: Vec<u32>,
    pub headings: Vec<(u32, u8)>,
    pub name: String,
    pub title: Option<String>,
    pub tags: Vec<String>,
    pub raw_tags: Vec<String>,
    pub props: Vec<(String, String)>,
}

/// Índice de búsqueda en memoria para toda la bóveda.
#[derive(Clone, Debug, Default)]
pub struct SearchIndex {
    docs: HashMap<RelPath, Doc>,
}

impl SearchIndex {
    pub fn new() -> Self {
        Self {
            docs: HashMap::new(),
        }
    }

    pub fn len(&self) -> usize {
        self.docs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.docs.is_empty()
    }

    pub fn contains(&self, path: &RelPath) -> bool {
        self.docs.contains_key(path)
    }

    /// Tamaño estimado en bytes de la memoria utilizada por el índice (SC-006).
    pub fn memory_bytes(&self) -> usize {
        let mut total = std::mem::size_of::<Self>();
        total += self.docs.capacity()
            * (std::mem::size_of::<RelPath>() + std::mem::size_of::<Doc>() + 16);

        for (path, doc) in &self.docs {
            total += path.as_str().len();
            total += doc.folded.capacity();
            total += doc.line_starts.capacity() * std::mem::size_of::<u32>();
            total += doc.headings.capacity() * std::mem::size_of::<(u32, u8)>();
            total += doc.name.capacity();
            if let Some(t) = &doc.title {
                total += t.capacity();
            }
            total += doc.tags.capacity() * std::mem::size_of::<String>();
            for tag in &doc.tags {
                total += tag.capacity();
            }
            total += doc.raw_tags.capacity() * std::mem::size_of::<String>();
            for tag in &doc.raw_tags {
                total += tag.capacity();
            }
            total += doc.props.capacity() * std::mem::size_of::<(String, String)>();
            for (k, v) in &doc.props {
                total += k.capacity() + v.capacity();
            }
        }

        total
    }

    /// Construye el índice recorriendo la bóveda en paralelo con `std::thread::scope` (R3).
    pub fn build(vault: &Vault) -> Self {
        let entries: Vec<RelPath> = vault
            .entries()
            .iter()
            .filter(|e| {
                if e.kind.is_text() {
                    true
                } else if e.zone == Zone::Raw {
                    is_text_file(&e.path)
                } else {
                    false
                }
            })
            .map(|e| e.path.clone())
            .collect();

        if entries.is_empty() {
            return Self::new();
        }

        let num_threads = std::thread::available_parallelism()
            .map(|n| (n.get().saturating_sub(1)).max(1))
            .unwrap_or(3)
            .min(entries.len())
            .max(1);

        let chunk_size = entries.len().div_ceil(num_threads);
        let root = vault.root();

        let partial_maps = std::thread::scope(|s| {
            let mut handles = Vec::new();
            for chunk in entries.chunks(chunk_size) {
                let chunk = chunk.to_vec();
                handles.push(s.spawn(move || {
                    let mut local_docs = HashMap::with_capacity(chunk.len());
                    for path in chunk {
                        let full_path = path.to_path(root);
                        if let Ok(file) = fs::File::open(&full_path)
                            && let Ok(text) = read_file_up_to(file, MAX_INDEXABLE_BYTES)
                        {
                            let zone = if path.as_str().starts_with("raw/") {
                                Zone::Raw
                            } else {
                                Zone::Wiki
                            };
                            let doc = build_doc(&path, zone, &text);
                            local_docs.insert(path, doc);
                        }
                    }
                    local_docs
                }));
            }

            let mut all = Vec::with_capacity(handles.len());
            for h in handles {
                if let Ok(res) = h.join() {
                    all.push(res);
                }
            }
            all
        });

        let mut docs = HashMap::with_capacity(entries.len());
        for part in partial_maps {
            docs.extend(part);
        }

        Self { docs }
    }

    /// Actualiza el índice cuando se modifica o crea un archivo.
    pub fn update_file(&mut self, vault: &Vault, path: &RelPath, text: &str) {
        let is_note = path.file_name().ends_with(".md")
            || path.file_name().ends_with(".html")
            || path.file_name().ends_with(".htm");
        let is_raw_text = path.as_str().starts_with("raw/") && is_text_file(path);

        if !is_note && !is_raw_text {
            self.docs.remove(path);
            return;
        }

        let valid_text = if text.len() > MAX_INDEXABLE_BYTES {
            let limit = MAX_INDEXABLE_BYTES;
            let valid = match std::str::from_utf8(&text.as_bytes()[..limit]) {
                Ok(_) => limit,
                Err(e) => e.valid_up_to(),
            };
            &text[..valid]
        } else {
            text
        };

        let zone = vault.zone(path);
        let doc = build_doc(path, zone, valid_text);
        self.docs.insert(path.clone(), doc);
    }

    /// Elimina una nota del índice.
    pub fn remove_file(&mut self, path: &RelPath) {
        self.docs.remove(path);
    }

    /// Ejecuta una consulta sobre el índice.
    pub fn search(&self, vault: &Vault, query: &Query, opts: &SearchOptions) -> SearchResults {
        let mut candidates = Vec::new();
        let positive_terms = extract_positive_terms(query);

        for (path, doc) in &self.docs {
            if !opts.include_raw && doc.zone == Zone::Raw {
                continue;
            }

            if !matches_query(doc, path, query) {
                continue;
            }

            let score = calculate_score(doc, &positive_terms);
            candidates.push((path.clone(), doc, score));
        }

        let total_notes = candidates.len();

        // Ordenar por score descendente, y desempate por ruta ascendente
        candidates.sort_by(|a, b| b.2.cmp(&a.2).then_with(|| a.0.cmp(&b.0)));

        let mut notes = Vec::new();
        let mut total_matches = 0;

        for (path, doc, score) in candidates {
            let full_path = path.to_path(vault.root());
            let source_text = fs::read_to_string(&full_path).unwrap_or_default();

            let all_matches = if positive_terms.is_empty() {
                // Si la consulta no tiene términos de texto (ej: solo tag: o path:),
                // no listamos todas las líneas, sólo marcamos una coincidencia testimonial o vacía
                Vec::new()
            } else {
                find_matches_in_file(doc, &source_text, &positive_terms)
            };

            let note_matches_count = all_matches.len();
            total_matches += note_matches_count;

            if notes.len() < opts.max_notes {
                let matches: Vec<Match> = all_matches.into_iter().take(opts.max_matches).collect();
                let more = note_matches_count.saturating_sub(opts.max_matches);
                notes.push(NoteHit {
                    path,
                    zone: doc.zone,
                    score,
                    matches,
                    more,
                });
            }
        }

        SearchResults {
            notes,
            total_notes,
            total_matches,
        }
    }

    /// Devuelve todos los nombres de etiquetas existentes mostrando la forma más usada.
    pub fn tag_names(&self) -> Vec<String> {
        let tree = self.tags();
        let mut out = Vec::new();
        fn collect(nodes: &[TagNode], prefix: &str, out: &mut Vec<String>) {
            for node in nodes {
                let full = if prefix.is_empty() {
                    node.name.clone()
                } else {
                    format!("{prefix}/{}", node.name)
                };
                out.push(full.clone());
                collect(&node.children, &full, out);
            }
        }
        collect(&tree.roots, "", &mut out);
        out
    }

    /// Devuelve el árbol de etiquetas para el panel.
    pub fn tags(&self) -> TagTree {
        let mut counts: HashMap<String, usize> = HashMap::new();
        let mut segment_forms: HashMap<String, HashMap<String, usize>> = HashMap::new();

        for doc in self.docs.values() {
            let mut doc_tags = std::collections::HashSet::new();
            for tag in &doc.tags {
                doc_tags.insert(tag.clone());
                // Contar también para los padres
                let mut current = tag.as_str();
                while let Some((parent, _)) = current.rsplit_once('/') {
                    doc_tags.insert(parent.to_string());
                    current = parent;
                }
            }
            for t in doc_tags {
                *counts.entry(t).or_insert(0) += 1;
            }

            for raw in &doc.raw_tags {
                let mut norm_acc = String::new();
                for seg in raw.split('/') {
                    let seg_fold = fold(seg);
                    if norm_acc.is_empty() {
                        norm_acc = seg_fold;
                    } else {
                        norm_acc.push('/');
                        norm_acc.push_str(&seg_fold);
                    }
                    *segment_forms
                        .entry(norm_acc.clone())
                        .or_default()
                        .entry(seg.to_string())
                        .or_insert(0) += 1;
                }
            }
        }

        // Construir árbol
        let mut all_paths: Vec<String> = counts.keys().cloned().collect();
        // Ordenar por tramos mantiene juntas las hijas aunque exista «tema-otra».
        all_paths.sort_by(|a, b| a.split('/').cmp(b.split('/')));

        TagTree {
            roots: build_tag_forest(&all_paths, &counts, &segment_forms, ""),
        }
    }
}

fn build_tag_forest(
    paths: &[String],
    counts: &HashMap<String, usize>,
    segment_forms: &HashMap<String, HashMap<String, usize>>,
    prefix: &str,
) -> Vec<TagNode> {
    let mut nodes = Vec::new();
    let mut i = 0;

    while i < paths.len() {
        let path = &paths[i];
        let rel = if prefix.is_empty() {
            path.as_str()
        } else if let Some(stripped) = path.strip_prefix(prefix) {
            let s = stripped.strip_prefix('/').unwrap_or(stripped);
            if s.is_empty() {
                i += 1;
                continue;
            }
            s
        } else {
            i += 1;
            continue;
        };

        let segment = rel.split('/').next().unwrap_or(rel);
        let current_path = if prefix.is_empty() {
            segment.to_string()
        } else {
            format!("{prefix}/{segment}")
        };

        // Encontrar todas las rutas que caen bajo este prefijo
        let mut j = i + 1;
        let child_prefix = format!("{current_path}/");
        while j < paths.len() && paths[j].starts_with(&child_prefix) {
            j += 1;
        }

        let children = build_tag_forest(&paths[i + 1..j], counts, segment_forms, &current_path);
        let notes = *counts.get(&current_path).unwrap_or(&0);

        let display_name = segment_forms
            .get(&current_path)
            .and_then(|forms| {
                forms
                    .iter()
                    .max_by(|(s1, c1), (s2, c2)| c1.cmp(c2).then_with(|| s1.cmp(s2)))
                    .map(|(s, _)| s.clone())
            })
            .unwrap_or_else(|| segment.to_string());

        nodes.push(TagNode {
            name: display_name,
            path: current_path,
            notes,
            children,
        });

        i = j;
    }

    nodes
}

fn read_file_up_to(file: fs::File, limit: usize) -> std::io::Result<String> {
    let mut buf = Vec::new();
    let mut reader = file.take(limit as u64);
    reader.read_to_end(&mut buf)?;
    let valid_len = match std::str::from_utf8(&buf) {
        Ok(_) => buf.len(),
        Err(e) => e.valid_up_to(),
    };
    Ok(String::from_utf8_lossy(&buf[..valid_len]).into_owned())
}

fn is_text_file(path: &RelPath) -> bool {
    let s = path.as_str().to_lowercase();
    let text_exts = [
        ".txt", ".csv", ".tsv", ".json", ".yaml", ".yml", ".xml", ".log", ".md", ".html", ".htm",
        ".rs", ".py", ".js", ".css", ".sh",
    ];
    if text_exts.iter().any(|ext| s.ends_with(ext)) {
        return true;
    }
    let bin_exts = [
        ".png", ".jpg", ".jpeg", ".gif", ".webp", ".pdf", ".zip", ".tar", ".gz", ".mp3", ".mp4",
        ".wav", ".exe", ".bin", ".so", ".dylib", ".ico",
    ];
    if bin_exts.iter().any(|ext| s.ends_with(ext)) {
        return false;
    }
    false
}

fn prop_values_as_strings(val: &PropValue) -> Vec<String> {
    match val {
        PropValue::Text(s) | PropValue::Link(s) => vec![s.clone()],
        PropValue::Number(s) => vec![s.clone()],
        PropValue::Bool(b) => vec![b.to_string()],
        PropValue::Date(s) | PropValue::DateTime(s) => vec![s.clone()],
        PropValue::List(items) => items.iter().flat_map(prop_values_as_strings).collect(),
        PropValue::Null => vec![String::new()],
        PropValue::Map(_) => vec![],
    }
}

fn build_doc(path: &RelPath, zone: Zone, text: &str) -> Doc {
    let file_name = path.file_name();
    let is_html = file_name.ends_with(".html") || file_name.ends_with(".htm");

    let stem = file_name.rsplit_once('.').map_or(file_name, |(s, _)| s);
    let name = fold(stem);

    let content = if is_html {
        html_text(text)
    } else {
        text.to_string()
    };

    let folded = fold(&content);

    // Calcular line_starts en folded
    let mut line_starts = vec![0u32];
    for (i, b) in folded.bytes().enumerate() {
        if b == b'\n' && i + 1 < folded.len() {
            line_starts.push((i + 1) as u32);
        }
    }

    // Encabezados
    let mut headings = Vec::new();
    if is_html {
        for (line_idx, line) in text.lines().enumerate() {
            let lower = line.to_lowercase();
            for level in 1..=6 {
                let tag = format!("<h{level}");
                if lower.contains(&tag) {
                    headings.push(((line_idx + 1) as u32, level as u8));
                    break;
                }
            }
        }
    } else {
        for (line_idx, line) in text.lines().enumerate() {
            let trimmed = line.trim_start();
            if trimmed.starts_with('#') {
                let count = trimmed.bytes().take_while(|&b| b == b'#').count();
                if count <= 6 && trimmed[count..].starts_with(' ') {
                    headings.push(((line_idx + 1) as u32, count as u8));
                }
            }
        }
    }

    // Título
    let title = extract_title(text, is_html);

    // Etiquetas
    let raw_tags_occ = extract_tags(text, is_html);
    let mut tags_set = std::collections::BTreeSet::new();
    let mut raw_tags_set = std::collections::BTreeSet::new();
    for occ in raw_tags_occ {
        tags_set.insert(fold(&occ.tag));
        raw_tags_set.insert(occ.tag);
    }
    let tags: Vec<String> = tags_set.into_iter().collect();
    let raw_tags: Vec<String> = raw_tags_set.into_iter().collect();

    // Propiedades
    // 1. Campos inline (sobre el texto visible, para HTML extrae sin etiquetas)
    let mut inline_props = Vec::new();
    for (k, v) in inline_fields(&content) {
        inline_props.push((fold(&k), fold(&v)));
    }
    // 2. Frontmatter YAML (el bloque manda si la clave se repite, T027)
    let mut yaml_props = Vec::new();
    let mut yaml_keys = HashSet::new();
    let region = if is_html {
        html::scan(text).block
    } else {
        markdown::locate(text)
    };
    if let Some(r) = region
        && let Ok(entries) = yaml::parse_block(&text[r])
    {
        for entry in entries {
            let k_fold = fold(&entry.key);
            yaml_keys.insert(k_fold.clone());
            let vals = prop_values_as_strings(&entry.value);
            for v in vals {
                yaml_props.push((k_fold.clone(), fold(&v)));
            }
        }
    }

    let mut props = yaml_props;
    for (k, v) in inline_props {
        if !yaml_keys.contains(&k) {
            props.push((k, v));
        }
    }

    Doc {
        zone,
        folded,
        line_starts,
        headings,
        name,
        title,
        tags,
        raw_tags,
        props,
    }
}

fn extract_title(text: &str, is_html: bool) -> Option<String> {
    if is_html {
        let region = html::scan(text).block;
        if let Some(r) = region
            && let Ok(entries) = yaml::parse_block(&text[r])
            && let Some(entry) = props::find(&entries, "title")
            && let Some(t) = entry.value.as_text()
        {
            return Some(fold(t));
        }

        let lower = text.to_lowercase();
        if let Some(start) = lower.find("<title>")
            && let Some(end) = lower[start + 7..].find("</title>")
        {
            let raw_title = &text[start + 7..start + 7 + end];
            return Some(fold(raw_title.trim()));
        }
    } else {
        let region = markdown::locate(text);
        if let Some(r) = region
            && let Ok(entries) = yaml::parse_block(&text[r])
            && let Some(entry) = props::find(&entries, "title")
            && let Some(t) = entry.value.as_text()
        {
            return Some(fold(t));
        }
    }
    None
}

fn extract_positive_terms(query: &Query) -> Vec<String> {
    let mut terms = Vec::new();
    for and_group in &query.any_of {
        for clause in and_group {
            if !clause.negated {
                collect_terms(&clause.atom, &mut terms);
            }
        }
    }
    terms.sort_by_key(|a| std::cmp::Reverse(a.len()));
    terms.dedup();
    terms
}

fn collect_terms(atom: &Atom, out: &mut Vec<String>) {
    match atom {
        Atom::Word(w) => out.push(fold(w)),
        Atom::Phrase(p) => out.push(fold(p)),
        Atom::Group(inner) | Atom::Line(inner) | Atom::Section(inner) => {
            for and_g in &inner.any_of {
                for c in and_g {
                    if !c.negated {
                        collect_terms(&c.atom, out);
                    }
                }
            }
        }
        _ => {}
    }
}

fn matches_query(doc: &Doc, path: &RelPath, query: &Query) -> bool {
    query.any_of.iter().any(|and_group| {
        and_group.iter().all(|clause| {
            let matched = matches_atom(doc, path, &clause.atom);
            if clause.negated { !matched } else { matched }
        })
    })
}

fn matches_atom(doc: &Doc, path: &RelPath, atom: &Atom) -> bool {
    match atom {
        Atom::Word(w) => {
            let term = fold(w);
            doc.name.contains(&term)
                || doc.title.as_ref().is_some_and(|t| t.contains(&term))
                || doc.tags.iter().any(|t| t.contains(&term))
                || doc
                    .props
                    .iter()
                    .any(|(k, v)| k.contains(&term) || v.contains(&term))
                || memmem::find(doc.folded.as_bytes(), term.as_bytes()).is_some()
        }
        Atom::Phrase(p) => {
            let phrase = fold(p);
            memmem::find(doc.folded.as_bytes(), phrase.as_bytes()).is_some()
        }
        Atom::Tag(t) => {
            let target = fold(t.trim_start_matches('#'));
            doc.tags
                .iter()
                .any(|dt| dt == &target || dt.starts_with(&format!("{target}/")))
        }
        Atom::Path(p) => {
            let p_fold = fold(p);
            path.as_str().to_lowercase().contains(&p_fold)
        }
        Atom::File(f) => {
            let f_fold = fold(f);
            doc.name.contains(&f_fold)
        }
        Atom::Prop { key, value } => {
            let k_fold = fold(key);
            match value {
                None => doc.props.iter().any(|(pk, _)| pk == &k_fold),
                Some(v) => {
                    let v_fold = fold(v);
                    doc.props
                        .iter()
                        .any(|(pk, pv)| pk == &k_fold && pv.contains(&v_fold))
                }
            }
        }
        Atom::Line(sub) => {
            for line in doc.folded.lines() {
                if matches_text_slice(line, sub) {
                    return true;
                }
            }
            false
        }
        Atom::Section(sub) => {
            // Dividir por secciones según doc.headings
            let lines: Vec<&str> = doc.folded.lines().collect();
            if doc.headings.is_empty() {
                return matches_text_slice(&doc.folded, sub);
            }
            if let Some(&(first_line, _)) = doc.headings.first() {
                let end_lead = (first_line.saturating_sub(1)) as usize;
                if end_lead > 0 && end_lead <= lines.len() {
                    let lead_text = lines[..end_lead].join("\n");
                    if matches_text_slice(&lead_text, sub) {
                        return true;
                    }
                }
            }
            for (idx, &(line_num, _)) in doc.headings.iter().enumerate() {
                let start_idx = (line_num.saturating_sub(1)) as usize;
                let end_idx = if idx + 1 < doc.headings.len() {
                    (doc.headings[idx + 1].0.saturating_sub(1)) as usize
                } else {
                    lines.len()
                };
                if start_idx < lines.len() {
                    let end = end_idx.min(lines.len());
                    let section_text = lines[start_idx..end].join("\n");
                    if matches_text_slice(&section_text, sub) {
                        return true;
                    }
                }
            }
            false
        }
        Atom::Group(inner) => matches_query(doc, path, inner),
    }
}

fn matches_text_slice(text: &str, query: &Query) -> bool {
    query.any_of.iter().any(|and_group| {
        and_group.iter().all(|clause| {
            let matched = match &clause.atom {
                Atom::Word(w) => {
                    let term = fold(w);
                    memmem::find(text.as_bytes(), term.as_bytes()).is_some()
                }
                Atom::Phrase(p) => {
                    let phrase = fold(p);
                    memmem::find(text.as_bytes(), phrase.as_bytes()).is_some()
                }
                Atom::Group(inner) => matches_text_slice(text, inner),
                _ => true,
            };
            if clause.negated { !matched } else { matched }
        })
    })
}

fn calculate_score(doc: &Doc, positive_terms: &[String]) -> u32 {
    if positive_terms.is_empty() {
        return 1;
    }

    let mut score = 0u32;
    for term in positive_terms {
        // En el nombre x10
        if doc.name.contains(term) {
            score += 10;
        }

        // En el título x8
        if let Some(title) = &doc.title
            && title.contains(term)
        {
            score += 8;
        }

        // Encabezados x3
        for &(line_num, _) in &doc.headings {
            let line_idx = line_num.saturating_sub(1) as usize;
            if let Some(line) = doc.folded.lines().nth(line_idx) {
                let count = memmem::find_iter(line.as_bytes(), term.as_bytes()).count();
                score += (count as u32) * 3;
            }
        }

        // En el texto x1
        let total_text_matches = memmem::find_iter(doc.folded.as_bytes(), term.as_bytes()).count();
        score += total_text_matches as u32;
    }

    score.max(1)
}

fn find_matches_in_file(doc: &Doc, source_text: &str, positive_terms: &[String]) -> Vec<Match> {
    let mut matches = Vec::new();
    let source_lines: Vec<&str> = source_text.lines().collect();

    for (line_idx, folded_line) in doc.folded.lines().enumerate() {
        let mut line_highlights = Vec::new();

        for term in positive_terms {
            for byte_pos in memmem::find_iter(folded_line.as_bytes(), term.as_bytes()) {
                // Convertir byte_pos a índice de caracteres
                let char_start = folded_line[..byte_pos].chars().count();
                let char_len = term.chars().count();
                line_highlights.push(char_start..(char_start + char_len));
            }
        }

        if !line_highlights.is_empty() {
            line_highlights.sort_by_key(|r| r.start);
            // Fusionar solapados
            let mut merged: Vec<Range<usize>> = Vec::new();
            for r in line_highlights {
                if let Some(last) = merged.last_mut()
                    && r.start <= last.end
                {
                    last.end = last.end.max(r.end);
                    continue;
                }
                merged.push(r);
            }

            let snippet = if line_idx < source_lines.len() {
                source_lines[line_idx].trim().to_string()
            } else {
                folded_line.trim().to_string()
            };

            matches.push(Match {
                line: line_idx + 1,
                snippet,
                highlights: merged,
            });
        }
    }

    matches
}
