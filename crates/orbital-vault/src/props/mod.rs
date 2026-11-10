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
