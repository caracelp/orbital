//! Notas: lectura, edición de propiedades por rangos de líneas y creación.
//!
//! Ninguna edición reserializa el bloque de propiedades: cambiar una propiedad reemplaza solo
//! las líneas que ocupa, y todo lo demás del archivo queda igual byte a byte (FR-016).

use std::fs;
use std::io::Read;
use std::ops::Range;

use crate::entry::{MAX_EDITABLE_BYTES, RelPath, Zone};
use crate::error::VaultError;
use crate::props::emit::{self, LineEnding};
use crate::props::{self, PropBlock, PropValue, Warning, html, markdown, yaml};
use crate::scan::zone_of;
use crate::vault::Vault;
use crate::write::{WriteCtx, WriteReceipt, hash, safe_write};

/// Formato de una nota.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Format {
    /// Nota atómica `.md`.
    Md,
    /// Nota de síntesis `.html`.
    Html,
}

impl Format {
    /// Formato según la extensión del archivo.
    pub fn from_name(name: &str) -> Option<Self> {
        let ext = name.rsplit_once('.')?.1;
        if ext.eq_ignore_ascii_case("md") {
            Some(Self::Md)
        } else if ext.eq_ignore_ascii_case("html") || ext.eq_ignore_ascii_case("htm") {
            Some(Self::Html)
        } else {
            None
        }
    }

    fn extension(self) -> &'static str {
        match self {
            Self::Md => "md",
            Self::Html => "html",
        }
    }

    fn default_type(self) -> &'static str {
        match self {
            Self::Md => "atomo",
            Self::Html => "sintesis",
        }
    }
}

/// Una nota cargada, con su contenido completo e intacto.
#[derive(Clone, Debug)]
pub struct Note {
    pub path: RelPath,
    pub format: Format,
    pub zone: Zone,
    pub source: String,
    pub line_ending: LineEnding,
    pub props: Option<PropBlock>,
    /// Hash de `source` al leerla; sirve para detectar cambios externos.
    pub hash: u64,
    /// `raw/` o más de 5 MB.
    pub read_only: bool,
    extra_blocks: usize,
}

impl Note {
    /// Valor de una propiedad (la primera, si está repetida).
    pub fn prop(&self, key: &str) -> Option<&PropValue> {
        self.props
            .as_ref()
            .and_then(|b| props::find(&b.entries, key))
            .map(|e| &e.value)
    }

    /// El tipo de la nota: `type` o, si no está, su alias `tipo` (gana `type` si están las dos).
    pub fn kind(&self) -> Option<&str> {
        self.props
            .as_ref()
            .and_then(|b| props::find_type(&b.entries))
            .and_then(|e| e.value.as_text())
    }

    /// La clave donde se escribe el tipo de esta nota (`tipo` si solo tiene el alias).
    pub fn type_key(&self) -> &'static str {
        self.props
            .as_ref()
            .map_or(props::TYPE_KEY, |b| props::type_key_for(&b.entries))
    }

    /// Avisos no bloqueantes sobre las propiedades.
    pub fn warnings(&self) -> Vec<Warning> {
        let entries = self.props.as_ref().map_or(&[][..], |b| &b.entries[..]);
        let error = self.props.as_ref().is_some_and(|b| b.error.is_some());
        let mut w = if error {
            Vec::new()
        } else {
            // `index.md` y `log.md` son archivos reservados de OKF: no llevan propiedades (§8 y §9).
            let reserved = matches!(self.path.file_name(), "index.md" | "log.md");
            props::validate(entries, self.zone == Zone::Wiki && !reserved)
        };
        if self.extra_blocks > 0 {
            w.push(Warning::ExtraBlocks(self.extra_blocks));
        }
        w
    }

    /// Contenido que tendría la nota con `key` cambiada a `value`, sin escribir nada.
    pub fn with_prop(&self, key: &str, value: &PropValue) -> Result<String, VaultError> {
        edit(self, key, Some(value))
    }

    /// Contenido que tendría la nota sin `key`, sin escribir nada.
    pub fn without_prop(&self, key: &str) -> Result<String, VaultError> {
        edit(self, key, None)
    }

    /// La misma nota (ruta, zona, formato) con otro texto: para editar las propiedades sobre el
    /// texto del editor en lugar del disco (research R7 de la 002).
    pub fn reparse(&self, source: String) -> Result<Note, VaultError> {
        parse(self.path.clone(), self.zone, source, self.read_only)
    }

    /// Analiza un texto como si fuera el contenido de `path` en `vault`.
    pub(crate) fn parse_for(vault: &Vault, path: &RelPath, source: &str) -> Option<Note> {
        parse(path.clone(), vault.zone(path), source.to_owned(), false).ok()
    }

    /// Huella de lo que muestra `index.md` de esta nota: si cambia al guardar, hay que ponerlo al día.
    pub(crate) fn index_key(&self) -> u64 {
        let mut text = String::new();
        for key in ["title", "description", "archived"] {
            if let Some(v) = self.prop(key) {
                text.push_str(&format!("{key}={v:?}\n"));
            }
        }
        if let Some(t) = self.kind() {
            text.push_str(&format!("type={t:?}\n"));
        }
        hash(text.as_bytes())
    }

    /// El texto de la nota sin el bloque de propiedades (lo que se muestra al leerla).
    pub fn body(&self) -> &str {
        match (self.format, &self.props) {
            (Format::Md, Some(b)) => {
                let after = &self.source[b.region.end..];
                let skip = after.find('\n').map_or(after.len(), |i| i + 1);
                &after[skip..]
            }
            _ => &self.source[markdown::bom_len(&self.source)..],
        }
    }
}

/// Arma la nota a partir del texto.
fn parse(path: RelPath, zone: Zone, source: String, read_only: bool) -> Result<Note, VaultError> {
    let format = Format::from_name(path.file_name())
        .ok_or_else(|| VaultError::InvalidPath(path.to_string()))?;
    let (region, extra_blocks) = match format {
        Format::Md => (markdown::locate(&source), 0),
        Format::Html => {
            let s = html::scan(&source);
            (s.block, s.extra_blocks)
        }
    };
    let props = region.map(|region| match yaml::parse_block(&source[region.clone()]) {
        Ok(entries) => PropBlock {
            region,
            entries,
            error: None,
        },
        Err(e) => PropBlock {
            region,
            entries: Vec::new(),
            error: Some(e),
        },
    });
    Ok(Note {
        hash: hash(source.as_bytes()),
        line_ending: LineEnding::detect(&source),
        path,
        format,
        zone,
        source,
        props,
        read_only,
        extra_blocks,
    })
}

/// Tamaño del primer tramo que lee `read_props`: alcanza para casi cualquier bloque.
const PROPS_PREFIX: u64 = 64 * 1024;

fn escape_html(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            c => out.push(c),
        }
    }
    out
}

impl Vault {
    fn check_path(&self, path: &RelPath) -> Result<(), VaultError> {
        if path.as_str().split('/').any(|c| c.starts_with('.')) {
            return Err(VaultError::Hidden(path.to_string()));
        }
        Ok(())
    }

    pub fn zone(&self, path: &RelPath) -> Zone {
        let l = self.layout();
        let structured = l.raw == crate::Presence::Present || l.wiki == crate::Presence::Present;
        zone_of(path, structured)
    }

    /// Lee una nota completa. Un archivo que no es UTF-8 devuelve `NotUtf8`.
    pub fn read_note(&self, path: &RelPath) -> Result<Note, VaultError> {
        self.check_path(path)?;
        let abs = path.to_path(self.root());
        let bytes = fs::read(&abs)?;
        let too_large = bytes.len() as u64 > MAX_EDITABLE_BYTES;
        let source = String::from_utf8(bytes).map_err(|_| VaultError::NotUtf8(path.clone()))?;
        let zone = self.zone(path);
        parse(path.clone(), zone, source, zone == Zone::Raw || too_large)
    }

    /// Lee solo las propiedades, sin cargar todo el archivo si el bloque está al principio.
    pub fn read_props(&self, path: &RelPath) -> Result<Option<PropBlock>, VaultError> {
        self.check_path(path)?;
        let abs = path.to_path(self.root());
        let mut file = fs::File::open(&abs)?;
        let mut buf = Vec::with_capacity(4096);
        (&mut file).take(PROPS_PREFIX).read_to_end(&mut buf)?;
        let complete = (buf.len() as u64) < PROPS_PREFIX;
        let text = match std::str::from_utf8(&buf) {
            Ok(t) => t,
            // Un carácter cortado al final del tramo no es un error de codificación.
            Err(e) if !complete && e.error_len().is_none() => {
                std::str::from_utf8(&buf[..e.valid_up_to()]).unwrap_or_default()
            }
            Err(_) => return Err(VaultError::NotUtf8(path.clone())),
        };
        let format = Format::from_name(path.file_name())
            .ok_or_else(|| VaultError::InvalidPath(path.to_string()))?;
        let region = match format {
            Format::Md => markdown::locate(text),
            Format::Html => html::scan(text).block,
        };
        // Si el bloque no entró en el tramo (o no se encontró y falta leer), se lee todo.
        if region.is_none() && !complete {
            return Ok(self.read_note(path)?.props);
        }
        Ok(
            region.map(|region| match yaml::parse_block(&text[region.clone()]) {
                Ok(entries) => PropBlock {
                    region,
                    entries,
                    error: None,
                },
                Err(e) => PropBlock {
                    region,
                    entries: Vec::new(),
                    error: Some(e),
                },
            }),
        )
    }

    /// Guarda un contenido nuevo (GuardadoSeguro + escritura atómica). Sin cambios, no escribe.
    pub fn save_note(
        &self,
        note: &Note,
        new_source: &str,
        ctx: &WriteCtx,
    ) -> Result<WriteReceipt, VaultError> {
        if note.read_only {
            return Err(VaultError::ReadOnly(note.path.clone()));
        }
        if new_source == note.source {
            return Ok(WriteReceipt::default());
        }
        safe_write(
            self.root(),
            &note.path,
            new_source.as_bytes(),
            Some(note.hash),
            ctx,
        )
    }

    /// Cambia (o agrega) una propiedad, tocando solo sus líneas.
    pub fn set_prop(
        &self,
        note: &Note,
        key: &str,
        value: &PropValue,
        ctx: &WriteCtx,
    ) -> Result<WriteReceipt, VaultError> {
        let new = edit(note, key, Some(value))?;
        let mut r = self.save_note(note, &new, ctx)?;
        self.after_prop_change(note, key, ctx, &mut r)?;
        Ok(r)
    }

    /// Quita una propiedad. Si no existe, no hace nada.
    pub fn remove_prop(
        &self,
        note: &Note,
        key: &str,
        ctx: &WriteCtx,
    ) -> Result<WriteReceipt, VaultError> {
        let new = edit(note, key, None)?;
        let mut r = self.save_note(note, &new, ctx)?;
        self.after_prop_change(note, key, ctx, &mut r)?;
        Ok(r)
    }

    /// Si cambió algo que se ve en el índice, pone al día su entrada (sin tocar el registro).
    fn after_prop_change(
        &self,
        note: &Note,
        key: &str,
        ctx: &WriteCtx,
        receipt: &mut WriteReceipt,
    ) -> Result<(), VaultError> {
        if !receipt.written.is_empty()
            && matches!(key, "title" | "description" | "type" | "tipo" | "archived")
        {
            receipt
                .written
                .extend(self.sync_index(&note.path, ctx)?.written);
        }
        Ok(())
    }

    /// Crea una nota nueva con las propiedades predeterminadas (data-model): `type`, `title`,
    /// `created` y `generated`. Falla si ya existe o si el nombre está reservado.
    pub fn create_note(
        &self,
        dir: &RelPath,
        name: &str,
        format: Format,
        ctx: &WriteCtx,
    ) -> Result<RelPath, VaultError> {
        let path = dir.join(&format!("{name}.{}", format.extension()))?;
        self.create_typed(&path, name, format, format.default_type(), ctx)?;
        Ok(path)
    }

    /// Valida que se pueda crear una nota en `path`.
    fn check_new(&self, path: &RelPath) -> Result<(), VaultError> {
        if self.zone(path) == Zone::Wiki && matches!(path.file_name(), "index.md" | "log.md") {
            return Err(VaultError::ReservedName(path.file_name().to_owned()));
        }
        if path.first_component() == "raw" {
            return Err(VaultError::ReadOnly(path.clone()));
        }
        self.check_path(path)
    }

    /// Crea una nota con `type`, `title`, `created` y `generated` (data-model).
    fn create_typed(
        &self,
        path: &RelPath,
        title: &str,
        format: Format,
        kind: &str,
        ctx: &WriteCtx,
    ) -> Result<(), VaultError> {
        self.check_new(path)?;
        let stamp = PropValue::DateTime(ctx.timestamp());
        let entries = [
            ("type", PropValue::Text(kind.to_owned())),
            ("title", PropValue::Text(title.to_owned())),
            ("created", stamp.clone()),
            (
                "generated",
                PropValue::Map(vec![
                    ("by".to_owned(), PropValue::Text(ctx.actor.0.clone())),
                    ("at".to_owned(), stamp),
                ]),
            ),
        ];
        let block = emit::document(&entries, LineEnding::Lf);
        let content = match format {
            Format::Md => format!("---\n{block}---\n# {title}\n"),
            Format::Html => {
                let t = escape_html(title);
                format!(
                    "<!doctype html>\n<html lang=\"es\">\n<head>\n  <meta charset=\"utf-8\">\n  <title>{t}</title>\n  <script type=\"application/yaml\">\n{block}  </script>\n</head>\n<body>\n<h1>{t}</h1>\n</body>\n</html>\n"
                )
            }
        };
        safe_write(self.root(), path, content.as_bytes(), None, ctx)?;
        self.after_create(path, ctx)?;
        Ok(())
    }

    /// Crea una nota desde una plantilla (FR-024): reemplaza las variables y cambia
    /// `type: plantilla` por `atomo` o `sintesis`. Devuelve la ruta y las variables desconocidas.
    pub fn create_from_template(
        &self,
        dir: &RelPath,
        name: &str,
        template: &RelPath,
        ctx: &WriteCtx,
    ) -> Result<(RelPath, Vec<String>), VaultError> {
        let tpl = self.read_note(template)?;
        let path = dir.join(&format!("{name}.{}", tpl.format.extension()))?;
        self.check_new(&path)?;
        let vars = crate::template::Vars {
            fecha: ctx.date.to_string(),
            hora: format!("{:02}:{:02}", ctx.time.hour, ctx.time.minute),
            titulo: name.to_owned(),
            carpeta: dir.to_string(),
        };
        let (rendered, unknown) = crate::template::render(&tpl.source, &vars);
        let note = parse(path.clone(), self.zone(&path), rendered, false)?;
        let keep_type = note.kind().is_some_and(|t| t != "plantilla");
        let content = if keep_type {
            note.source
        } else {
            edit(
                &note,
                note.type_key(),
                Some(&PropValue::Text(tpl.format.default_type().to_owned())),
            )?
        };
        safe_write(self.root(), &path, content.as_bytes(), None, ctx)?;
        self.after_create(&path, ctx)?;
        Ok((path, unknown))
    }

    /// Abre la nota diaria de hoy, creándola si no existe (FR-025). El nombre sale de
    /// `journal_name` (`AAAA`, `MM` y `DD` se reemplazan) y puede tener subcarpetas.
    pub fn open_journal(&self, ctx: &WriteCtx) -> Result<RelPath, VaultError> {
        let name = self
            .config()
            .journal_name
            .replace("AAAA", &format!("{:04}", ctx.date.year))
            .replace("MM", &format!("{:02}", ctx.date.month))
            .replace("DD", &format!("{:02}", ctx.date.day));
        let base = if self.root().join("wiki").is_dir() {
            format!("wiki/{name}.md")
        } else {
            format!("{name}.md")
        };
        let path = RelPath::new(&base)?;
        if path.to_path(self.root()).exists() {
            return Ok(path);
        }
        self.create_typed(&path, &ctx.date.to_string(), Format::Md, "diario", ctx)?;
        Ok(path)
    }

    /// Guarda un adjunto junto a la nota (FR-026) con un nombre que no pise nada, y lo enlaza al
    /// final de la nota (`![](…)` en `.md`, `<img>` antes de `</body>` en `.html`).
    pub fn attach(
        &self,
        note_path: &RelPath,
        name: &str,
        bytes: &[u8],
        ctx: &WriteCtx,
    ) -> Result<RelPath, VaultError> {
        let note = self.read_note(note_path)?;
        if note.read_only {
            return Err(VaultError::ReadOnly(note_path.clone()));
        }
        let clean = clean_file_name(name);
        let dir = note_path.as_str().rsplit_once('/').map(|(d, _)| d);
        let (stem, ext) = match clean.rsplit_once('.') {
            Some((s, e)) if !s.is_empty() => (s.to_owned(), format!(".{e}")),
            _ => (clean.clone(), String::new()),
        };
        let mut n = 0u32;
        let path = loop {
            let file = if n == 0 {
                clean.clone()
            } else {
                format!("{stem}-{n}{ext}")
            };
            let candidate = match dir {
                Some(d) => RelPath::new(&format!("{d}/{file}"))?,
                None => RelPath::new(&file)?,
            };
            if !candidate.to_path(self.root()).exists() {
                break candidate;
            }
            n += 1;
        };
        safe_write(self.root(), &path, bytes, None, ctx)?;
        let file = path.file_name();
        let href = encode_href(file);
        let src = &note.source;
        let eol = note.line_ending.as_str();
        let new = match note.format {
            Format::Md => {
                let mut out = src.clone();
                if !out.is_empty() && !out.ends_with('\n') {
                    out.push_str(eol);
                }
                if !out.is_empty() {
                    out.push_str(eol);
                }
                out.push_str(&format!("![{}]({href}){eol}", file.replace(']', "\\]")));
                out
            }
            Format::Html => {
                let tag = format!("<img src=\"{href}\" alt=\"{}\">{eol}", escape_html(file));
                match src.to_ascii_lowercase().rfind("</body") {
                    Some(at) => format!("{}{tag}{}", &src[..at], &src[at..]),
                    None => format!("{src}{tag}"),
                }
            }
        };
        let mut receipt = self.save_note(&note, &new, ctx)?;
        receipt.written.push((path.clone(), hash(bytes)));
        Ok(path)
    }

    /// Lee las propiedades de las notas de `wiki/` (en segundo plano): plantillas y archivadas.
    pub fn note_meta(&self) -> NoteMeta {
        let mut meta = NoteMeta::default();
        for e in self.entries() {
            if e.zone != Zone::Wiki
                || !matches!(
                    e.kind,
                    crate::EntryKind::AtomNote | crate::EntryKind::SynthesisNote
                )
            {
                continue;
            }
            let Ok(Some(block)) = self.read_props(&e.path) else {
                continue;
            };
            let entries = &block.entries;
            if props::find_type(entries).and_then(|p| p.value.as_text()) == Some("plantilla") {
                meta.templates.push(e.path.clone());
            }
            if props::find(entries, "archived").is_some_and(|p| p.value == PropValue::Bool(true)) {
                meta.archived.push(e.path.clone());
            }
        }
        meta
    }

    /// Plantillas de la bóveda (`type: plantilla` en `wiki/`).
    pub fn templates(&self) -> Vec<RelPath> {
        self.note_meta().templates
    }

    /// Si la nota tiene `archived: true`.
    pub fn is_archived(&self, path: &RelPath) -> Result<bool, VaultError> {
        Ok(self.read_props(path)?.is_some_and(|b| {
            props::find(&b.entries, "archived").is_some_and(|p| p.value == PropValue::Bool(true))
        }))
    }
}

/// Plantillas y notas archivadas de una bóveda.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct NoteMeta {
    pub templates: Vec<RelPath>,
    pub archived: Vec<RelPath>,
}

/// Nombre de archivo seguro: solo el último componente, sin puntos iniciales ni controles.
fn clean_file_name(name: &str) -> String {
    let last = name.rsplit(['/', '\\']).next().unwrap_or(name);
    let cleaned: String = last
        .trim()
        .trim_start_matches('.')
        .chars()
        .filter(|c| !c.is_control())
        .collect();
    if cleaned.is_empty() {
        "adjunto".to_owned()
    } else {
        cleaned
    }
}

/// Codifica un nombre de archivo para usarlo en `src`/`href`: los caracteres especiales de URL
/// van como `%XX`; las letras no ASCII quedan tal cual.
fn encode_href(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    for c in name.chars() {
        if c.is_ascii_alphanumeric() || "-_.~".contains(c) || !c.is_ascii() {
            out.push(c);
        } else {
            let mut buf = [0u8; 4];
            for b in c.encode_utf8(&mut buf).bytes() {
                out.push_str(&format!("%{b:02X}"));
            }
        }
    }
    out
}

/// Calcula el contenido nuevo de una nota al cambiar (`Some`) o quitar (`None`) una propiedad.
fn edit(note: &Note, key: &str, value: Option<&PropValue>) -> Result<String, VaultError> {
    if note.read_only {
        return Err(VaultError::ReadOnly(note.path.clone()));
    }
    let eol = note.line_ending;
    let src = &note.source;
    let Some(block) = &note.props else {
        let Some(value) = value else {
            return Ok(src.clone());
        };
        return insert_block(note, &emit::entry(key, value, eol));
    };
    if let Some(err) = &block.error {
        return Err(VaultError::InvalidProps {
            path: note.path.clone(),
            line: err.line,
            message: err.message.clone(),
        });
    }
    let region_text = &src[block.region.clone()];
    let table = yaml::line_table(region_text);
    let abs = |r: Range<usize>| block.region.start + r.start..block.region.start + r.end;
    let existing = props::find(&block.entries, key);
    let (range, replacement): (Range<usize>, String) = match (existing, value) {
        (Some(e), Some(v)) => {
            let r = abs(yaml::byte_range(region_text, &table, e.lines.clone()));
            // Si la última línea de la propiedad no terminaba en salto de línea, tampoco el reemplazo.
            let mut text = emit::entry(key, v, eol);
            if !src[r.clone()].ends_with('\n') {
                text.truncate(text.trim_end_matches(['\r', '\n']).len());
            }
            (r, text)
        }
        (Some(e), None) => (
            abs(yaml::byte_range(region_text, &table, e.lines.clone())),
            String::new(),
        ),
        (None, None) => return Ok(src.clone()),
        (None, Some(v)) => {
            let at = match block.entries.last() {
                Some(last) => abs(yaml::byte_range(region_text, &table, last.lines.clone())).end,
                // Bloque vacío: después del primer salto de línea del bloque (en HTML, el que
                // sigue a `<script ...>`), o al principio.
                None => {
                    block.region.start
                        + region_text
                            .find('\n')
                            .filter(|_| region_text.trim().is_empty())
                            .map_or(0, |i| i + 1)
                }
            };
            let mut text = emit::entry(key, v, eol);
            // Si lo anterior no terminaba en salto de línea, se agrega uno antes.
            if at > 0 && !src[..at].ends_with('\n') {
                text.insert_str(0, eol.as_str());
            }
            (at..at, text)
        }
    };
    let mut out = String::with_capacity(src.len() + replacement.len());
    out.push_str(&src[..range.start]);
    out.push_str(&replacement);
    out.push_str(&src[range.end..]);
    Ok(out)
}

/// Inserta un bloque de propiedades nuevo con una entrada.
fn insert_block(note: &Note, entry: &str) -> Result<String, VaultError> {
    let eol = note.line_ending.as_str();
    let src = &note.source;
    let (at, block) = match note.format {
        Format::Md => (markdown::bom_len(src), format!("---{eol}{entry}---{eol}")),
        Format::Html => {
            let at = html::scan(src)
                .insert_at
                .ok_or_else(|| VaultError::NoHead(note.path.clone()))?;
            let indent: String = src[at..]
                .chars()
                .take_while(|c| *c == ' ' || *c == '\t')
                .collect();
            (
                at,
                format!(
                    "{indent}<script type=\"application/yaml\">{eol}{entry}{indent}</script>{eol}"
                ),
            )
        }
    };
    let mut out = String::with_capacity(src.len() + block.len());
    out.push_str(&src[..at]);
    out.push_str(&block);
    out.push_str(&src[at..]);
    Ok(out)
}
