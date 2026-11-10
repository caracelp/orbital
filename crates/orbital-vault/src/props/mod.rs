//! Modelo de propiedades, común a las notas `.md` y `.html`.

pub mod emit;
pub mod html;
pub mod markdown;
pub mod yaml;

use std::ops::Range;

/// Valor de una propiedad, con su tipo inferido del escalar YAML.
#[derive(Clone, PartialEq, Debug)]
pub enum PropValue {
    /// Valor vacío (`clave:` sin nada, `null` o `~`).
    Null,
    Text(String),
    /// Número YAML 1.2; se guarda el texto original para no alterarlo.
    Number(String),
    Bool(bool),
    /// Fecha `AAAA-MM-DD`.
    Date(String),
    /// Fecha y hora ISO 8601 con desplazamiento, por ejemplo `2026-06-30T14:00:00Z`.
    DateTime(String),
    /// `[[destino]]` o ruta que empieza con `/`, `./` o `../` y termina en `.md`/`.html`.
    Link(String),
    List(Vec<PropValue>),
    /// Mapeo, en el orden del archivo.
    Map(Vec<(String, PropValue)>),
}

impl PropValue {
    /// Texto del valor si es un escalar de texto (o enlace).
    pub fn as_text(&self) -> Option<&str> {
        match self {
            Self::Text(s) | Self::Link(s) => Some(s),
            _ => None,
        }
    }
}

/// Una propiedad de primer nivel y las líneas que ocupa dentro del bloque.
#[derive(Clone, PartialEq, Debug)]
pub struct PropEntry {
    pub key: String,
    pub value: PropValue,
    /// Líneas del bloque (índices desde 0, fin exclusivo): desde la clave hasta la última línea
    /// de su valor, incluidos los comentarios intermedios.
    pub lines: Range<usize>,
}

/// Error de sintaxis en un bloque de propiedades.
#[derive(Clone, PartialEq, Debug)]
pub struct PropError {
    /// Línea dentro del bloque, desde 1.
    pub line: usize,
    pub message: String,
}

/// Busca una propiedad por clave.
pub fn find<'a>(entries: &'a [PropEntry], key: &str) -> Option<&'a PropEntry> {
    entries.iter().find(|e| e.key == key)
}

/// Bloque de propiedades de una nota.
#[derive(Clone, PartialEq, Debug)]
pub struct PropBlock {
    /// Rango en bytes del YAML dentro del archivo: entre las líneas `---` en `.md`, o el
    /// contenido de `<script type="application/yaml">` en `.html`.
    pub region: Range<usize>,
    /// Propiedades en el orden del archivo. Vacío si hay un error.
    pub entries: Vec<PropEntry>,
    /// Error de sintaxis; mientras exista, las propiedades no se pueden editar (FR-029).
    pub error: Option<PropError>,
}

/// Aviso no bloqueante sobre las propiedades (nunca impide editar).
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Warning {
    /// Nota de `wiki/` sin `type` (OKF §4.1).
    MissingType,
    /// Una propiedad conocida tiene un tipo distinto del esperado.
    UnexpectedType { key: String },
    /// `status` fuera de `draft | stable | deprecated` (OKF §5.4).
    InvalidStatus(String),
    /// La misma clave aparece más de una vez; vale la primera.
    DuplicateKey(String),
    /// La nota HTML tiene más de un bloque de propiedades; vale el primero.
    ExtraBlocks(usize),
}

impl Warning {
    /// Mensaje en español para la interfaz.
    pub fn message(&self) -> String {
        match self {
            Self::MissingType => "Esta nota no tiene «type» (OKF)".into(),
            Self::UnexpectedType { key } => {
                format!("La propiedad «{key}» tiene un tipo inesperado")
            }
            Self::InvalidStatus(v) => {
                format!("«status: {v}» no es válido; se usa draft, stable o deprecated")
            }
            Self::DuplicateKey(k) => format!("La propiedad «{k}» está repetida; vale la primera"),
            Self::ExtraBlocks(n) => {
                format!("Hay {n} bloques de propiedades de más; vale el primero")
            }
        }
    }
}

fn is_text(v: &PropValue) -> bool {
    matches!(v, PropValue::Text(_) | PropValue::Link(_))
}

fn is_text_list(v: &PropValue) -> bool {
    matches!(v, PropValue::List(items) if items.iter().all(is_text))
}

fn is_event(v: &PropValue) -> bool {
    matches!(v, PropValue::Map(_))
}

/// Avisos sobre las propiedades conocidas (data-model, tabla de propiedades).
pub fn validate(entries: &[PropEntry], in_wiki: bool) -> Vec<Warning> {
    let mut out = Vec::new();
    let mut seen: Vec<&str> = Vec::with_capacity(entries.len());
    for e in entries {
        if seen.contains(&e.key.as_str()) {
            out.push(Warning::DuplicateKey(e.key.clone()));
            continue;
        }
        seen.push(&e.key);
        let v = &e.value;
        let ok = match e.key.as_str() {
            "type" | "title" | "description" | "resource" => is_text(v),
            "tags" | "aliases" => is_text_list(v),
            "status" => match v.as_text() {
                Some("draft" | "stable" | "deprecated") => true,
                Some(other) => {
                    out.push(Warning::InvalidStatus(other.to_owned()));
                    continue;
                }
                None => false,
            },
            "sources" => matches!(v, PropValue::List(items) if items.iter().all(is_event)),
            "generated" => is_event(v),
            // Un mapeo suelto vale como lista de uno (OKF §5.2).
            "verified" => {
                is_event(v) || matches!(v, PropValue::List(items) if items.iter().all(is_event))
            }
            "stale_after" | "created" | "updated" => matches!(v, PropValue::DateTime(_)),
            "archived" => matches!(v, PropValue::Bool(_)),
            _ => true,
        };
        if !ok && *v != PropValue::Null {
            out.push(Warning::UnexpectedType { key: e.key.clone() });
        }
    }
    if in_wiki
        && !entries
            .iter()
            .any(|e| e.key == "type" && e.value != PropValue::Null)
    {
        out.push(Warning::MissingType);
    }
    out
}
