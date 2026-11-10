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

    /// Avisos no bloqueantes sobre las propiedades.
    pub fn warnings(&self) -> Vec<Warning> {
        let entries = self.props.as_ref().map_or(&[][..], |b| &b.entries[..]);
        let error = self.props.as_ref().is_some_and(|b| b.error.is_some());
        let mut w = if error {
            Vec::new()
        } else {
            props::validate(entries, self.zone == Zone::Wiki)
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

    fn zone(&self, path: &RelPath) -> Zone {
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
            && matches!(key, "title" | "description" | "type" | "archived")
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
        let file = format!("{name}.{}", format.extension());
        let path = dir.join(&file)?;
        if self.zone(&path) == Zone::Wiki && matches!(file.as_str(), "index.md" | "log.md") {
            return Err(VaultError::ReservedName(file));
        }
        self.check_path(&path)?;
        let stamp = PropValue::DateTime(ctx.timestamp());
        let entries = [
            ("type", PropValue::Text(format.default_type().to_owned())),
            ("title", PropValue::Text(name.to_owned())),
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
            Format::Md => format!("---\n{block}---\n# {name}\n"),
            Format::Html => {
                let title = escape_html(name);
                format!(
                    "<!doctype html>\n<html lang=\"es\">\n<head>\n  <meta charset=\"utf-8\">\n  <title>{title}</title>\n  <script type=\"application/yaml\">\n{block}  </script>\n</head>\n<body>\n<h1>{title}</h1>\n</body>\n</html>\n"
                )
            }
        };
        safe_write(self.root(), &path, content.as_bytes(), None, ctx)?;
        self.after_create(&path, ctx)?;
        Ok(path)
    }
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
