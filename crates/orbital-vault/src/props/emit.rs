//! Emisor mínimo de YAML para propiedades nuevas o cambiadas, y para los archivos de
//! configuración de Orbital.
//!
//! Siempre escribe en estilo de flujo en una sola línea (`clave: valor`, `[a, b]`, `{ k: v }`).
//! Un texto que se leería como otro tipo, o que tiene caracteres especiales, va entre comillas
//! dobles. `</` se escribe siempre como `<\/` para que nunca cierre un `<script>` (research R3).

use super::PropValue;
use super::yaml::{is_date, is_datetime, is_number};

/// Final de línea a usar al escribir.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LineEnding {
    Lf,
    CrLf,
}

impl LineEnding {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Lf => "\n",
            Self::CrLf => "\r\n",
        }
    }

    /// Detecta el final de línea dominante de un texto (`Lf` si no hay ninguno).
    pub fn detect(text: &str) -> Self {
        match text.find('\n') {
            Some(i) if i > 0 && text.as_bytes()[i - 1] == b'\r' => Self::CrLf,
            _ => Self::Lf,
        }
    }
}

/// `clave: valor` + final de línea.
pub fn entry(key: &str, value: &PropValue, eol: LineEnding) -> String {
    let mut out = String::with_capacity(key.len() + 16);
    out.push_str(&scalar(key));
    out.push(':');
    if *value != PropValue::Null {
        out.push(' ');
        write_value(&mut out, value);
    }
    out.push_str(eol.as_str());
    out
}

/// Documento completo con varias claves, en el orden dado.
pub fn document(entries: &[(&str, PropValue)], eol: LineEnding) -> String {
    let mut out = String::new();
    for (k, v) in entries {
        out.push_str(&entry(k, v, eol));
    }
    out
}

/// Valor en estilo de flujo.
pub fn value(v: &PropValue) -> String {
    let mut out = String::new();
    write_value(&mut out, v);
    out
}

fn write_value(out: &mut String, v: &PropValue) {
    match v {
        PropValue::Null => out.push_str("null"),
        PropValue::Text(s) => out.push_str(&scalar(s)),
        // Un enlace `[[x]]` empieza con `[`, así que siempre lleva comillas.
        PropValue::Link(s) => out.push_str(&scalar(s)),
        PropValue::Number(n) => out.push_str(n),
        PropValue::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        PropValue::Date(d) | PropValue::DateTime(d) => out.push_str(d),
        PropValue::List(items) => {
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                write_value(out, item);
            }
            out.push(']');
        }
        PropValue::Map(items) => {
            if items.is_empty() {
                out.push_str("{}");
                return;
            }
            out.push_str("{ ");
            for (i, (k, item)) in items.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                out.push_str(&scalar(k));
                out.push_str(": ");
                write_value(out, item);
            }
            out.push_str(" }");
        }
    }
}

/// Escalar de texto: sin comillas si es seguro, si no entre comillas dobles.
pub fn scalar(s: &str) -> String {
    if is_plain_safe(s) {
        return s.to_owned();
    }
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    let mut prev_lt = false;
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '/' if prev_lt => out.push_str("\\/"),
            c if c.is_control() => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
        prev_lt = c == '<';
    }
    out.push('"');
    out
}

fn is_plain_safe(s: &str) -> bool {
    if s.is_empty() || s.trim() != s || s.contains("</") {
        return false;
    }
    let first = s.chars().next().unwrap_or(' ');
    if "-?:,[]{}#&*!|>'\"%@`".contains(first) {
        return false;
    }
    if s.contains(": ") || s.contains(" #") || s.ends_with(':') {
        return false;
    }
    if s.chars().any(|c| c.is_control() || ",[]{}".contains(c)) {
        return false;
    }
    // Un texto que se leería como otro tipo tiene que ir entre comillas para seguir siendo texto.
    let looks_typed = matches!(
        s,
        "~" | "null" | "Null" | "NULL" | "true" | "True" | "TRUE" | "false" | "False" | "FALSE"
    ) || is_number(s)
        || is_date(s)
        || is_datetime(s);
    !looks_typed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scalars() {
        assert_eq!(scalar("hola"), "hola");
        assert_eq!(scalar("canción"), "canción");
        assert_eq!(scalar("true"), "\"true\"");
        assert_eq!(scalar("2026-11-09"), "\"2026-11-09\"");
        assert_eq!(scalar("a: b"), "\"a: b\"");
        assert_eq!(scalar("</script>"), "\"<\\/script>\"");
        assert_eq!(scalar("[[x]]"), "\"[[x]]\"");
        assert_eq!(scalar(""), "\"\"");
    }

    #[test]
    fn line_endings() {
        assert_eq!(LineEnding::detect("a\r\nb"), LineEnding::CrLf);
        assert_eq!(LineEnding::detect("a\nb"), LineEnding::Lf);
        assert_eq!(LineEnding::detect("sin salto"), LineEnding::Lf);
    }
}
