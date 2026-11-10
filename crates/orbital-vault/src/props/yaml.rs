//! Lectura de bloques de propiedades YAML con `saphyr-parser`, en modo eventos.
//!
//! No se arma un DOM ni se reserializa nada: cada propiedad de primer nivel conserva el rango de
//! líneas que ocupa, para que editar una sola propiedad cambie solo esas líneas (research R2).
//! `saphyr-parser` cuenta posiciones en caracteres, así que aquí se usan solo los números de línea.

use saphyr_parser::{Event, Parser, ScalarStyle};

use super::{PropEntry, PropError, PropValue};

/// Lee un bloque de propiedades (el texto entre `---` o el contenido del `<script>`).
///
/// Un bloque vacío o con solo comentarios devuelve una lista vacía.
pub fn parse_block(text: &str) -> Result<Vec<PropEntry>, PropError> {
    let mut parser = Parser::new_from_str(text);
    let mut builder = Builder::default();
    let mut top: Vec<(String, PropValue, usize)> = Vec::new();
    // Profundidad: 0 = fuera del documento, 1 = mapeo de primer nivel.
    let mut depth = 0usize;
    let mut pending_key: Option<(String, usize)> = None;
    let mut seen_root = false;

    while let Some(item) = parser.next_event() {
        let (event, span) = item.map_err(|e| PropError {
            line: e.marker().line().max(1),
            message: translate(e.info()),
        })?;
        let line = span.start.line().saturating_sub(1);
        match event {
            Event::StreamStart | Event::Nothing => {}
            Event::DocumentStart(_) => {}
            Event::DocumentEnd | Event::StreamEnd => break,
            _ if depth == 0 => {
                // Raíz del documento: tiene que ser un mapeo (o un nulo implícito).
                if seen_root {
                    break;
                }
                seen_root = true;
                match event {
                    Event::MappingStart(..) => depth = 1,
                    Event::Scalar(ref v, ScalarStyle::Plain, ..) if is_null(v) => {}
                    _ => {
                        return Err(PropError {
                            line: line + 1,
                            message: "las propiedades tienen que ser pares «clave: valor»".into(),
                        });
                    }
                }
            }
            Event::MappingEnd if depth == 1 && !builder.active() => depth = 0,
            _ if depth == 1 && pending_key.is_none() && !builder.active() => {
                // Clave de primer nivel.
                match event {
                    Event::Scalar(k, ..) => pending_key = Some((k.into_owned(), line)),
                    _ => {
                        return Err(PropError {
                            line: line + 1,
                            message: "las claves de las propiedades tienen que ser texto".into(),
                        });
                    }
                }
            }
            ev => {
                if let Some(value) = builder.push(ev) {
                    let Some((key, key_line)) = pending_key.take() else {
                        return Err(PropError {
                            line: line + 1,
                            message: "hay un valor sin clave".into(),
                        });
                    };
                    top.push((key, value, key_line));
                }
            }
        }
    }

    let lines = line_table(text);
    let n_lines = lines.len();
    let starts: Vec<usize> = top.iter().map(|t| t.2).collect();
    let mut out = Vec::with_capacity(top.len());
    for (i, (key, value, start)) in top.into_iter().enumerate() {
        let mut end = starts.get(i + 1).copied().unwrap_or(n_lines);
        // Las líneas en blanco y los comentarios en la columna 0 al final del rango pertenecen a
        // lo que sigue, no a esta propiedad.
        while end > start + 1 && is_trailing_filler(line_text(text, &lines, end - 1)) {
            end -= 1;
        }
        out.push(PropEntry {
            key,
            value,
            lines: start..end,
        });
    }
    Ok(out)
}

/// Traduce los mensajes de `saphyr-parser` que aparecen con YAML escrito a mano.
/// Si el mensaje no es conocido, se muestra genérico con el original entre paréntesis.
pub fn translate(msg: &str) -> String {
    const TABLE: [(&str, &str); 16] = [
        (
            "illegal placement of ':' indicator",
            "hay un «:» donde no corresponde; si el valor tiene «:», ponelo entre comillas",
        ),
        (
            "mapping values are not allowed in this context",
            "hay un «clave: valor» donde no corresponde; revisá la sangría o poné el valor entre comillas",
        ),
        (
            "flow sequence, expected ',' or ']'",
            "falta cerrar una lista con «]» o separar sus elementos con «,»",
        ),
        (
            "flow mapping, did not find expected ',' or '}'",
            "falta cerrar un mapeo con «}» o separar sus elementos con «,»",
        ),
        (
            "block mapping, did not find expected key",
            "falta una clave; revisá la sangría de esta línea",
        ),
        (
            "block collection, did not find expected '-'",
            "falta un «-» en una lista; revisá la sangría",
        ),
        ("did not find expected node content", "falta un valor"),
        ("invalid indentation", "la sangría no es válida"),
        (
            "tabs disallowed",
            "YAML no admite tabulaciones para la sangría; usá espacios",
        ),
        (
            "found a tab",
            "YAML no admite tabulaciones aquí; usá espacios",
        ),
        (
            "quoted scalar, found unexpected end of stream",
            "falta cerrar unas comillas",
        ),
        (
            "found unknown escape character",
            "hay un escape «\\» desconocido dentro de comillas dobles",
        ),
        (
            "found unknown anchor",
            "se usa un alias «*» que no fue definido",
        ),
        ("found duplicated anchor", "hay un ancla «&» repetida"),
        (
            "unexpected end of plain scalar",
            "el texto termina de forma inesperada; probá ponerlo entre comillas",
        ),
        (
            "block sequence entries are not allowed",
            "hay un elemento de lista «-» donde no corresponde",
        ),
    ];
    TABLE.iter().find(|(en, _)| msg.contains(en)).map_or_else(
        || format!("error de sintaxis YAML ({msg})"),
        |(_, es)| (*es).to_owned(),
    )
}

/// Inicios de línea en bytes. La línea `i` va de `table[i]` hasta `table[i + 1]` (o el final).
pub fn line_table(text: &str) -> Vec<usize> {
    let mut table = Vec::with_capacity(text.len() / 32 + 1);
    table.push(0);
    for (i, b) in text.bytes().enumerate() {
        if b == b'\n' && i + 1 < text.len() {
            table.push(i + 1);
        }
    }
    if text.is_empty() {
        table.clear();
    }
    table
}

/// Rango de bytes que ocupan las líneas `range` (incluidos sus finales de línea).
pub fn byte_range(
    text: &str,
    table: &[usize],
    range: std::ops::Range<usize>,
) -> std::ops::Range<usize> {
    let start = table.get(range.start).copied().unwrap_or(text.len());
    let end = table.get(range.end).copied().unwrap_or(text.len());
    start..end
}

fn line_text<'a>(text: &'a str, table: &[usize], i: usize) -> &'a str {
    let r = byte_range(text, table, i..i + 1);
    &text[r]
}

fn is_trailing_filler(line: &str) -> bool {
    let t = line.trim_end_matches(['\n', '\r']);
    t.trim().is_empty() || t.starts_with('#')
}

/// Arma valores anidados a partir de eventos.
#[derive(Default)]
struct Builder {
    stack: Vec<Frame>,
}

enum Frame {
    Seq(Vec<PropValue>),
    Map(Vec<(String, PropValue)>, Option<String>),
}

impl Builder {
    fn active(&self) -> bool {
        !self.stack.is_empty()
    }

    /// Procesa un evento; devuelve el valor terminado cuando se cierra el de primer nivel.
    fn push(&mut self, ev: Event<'_>) -> Option<PropValue> {
        let finished = match ev {
            Event::SequenceStart(..) => {
                self.stack.push(Frame::Seq(Vec::new()));
                return None;
            }
            Event::MappingStart(..) => {
                self.stack.push(Frame::Map(Vec::new(), None));
                return None;
            }
            Event::SequenceEnd | Event::MappingEnd => match self.stack.pop()? {
                Frame::Seq(items) => PropValue::List(items),
                Frame::Map(items, _) => PropValue::Map(items),
            },
            Event::Scalar(v, style, ..) => {
                // Una clave dentro de un mapeo anidado.
                if let Some(Frame::Map(_, key @ None)) = self.stack.last_mut() {
                    *key = Some(v.into_owned());
                    return None;
                }
                scalar_value(&v, style)
            }
            Event::Alias(_) => PropValue::Null,
            _ => return None,
        };
        match self.stack.last_mut() {
            None => Some(finished),
            Some(Frame::Seq(items)) => {
                items.push(finished);
                None
            }
            Some(Frame::Map(items, key)) => {
                let k = key.take().unwrap_or_default();
                items.push((k, finished));
                None
            }
        }
    }
}

fn is_null(v: &str) -> bool {
    matches!(v, "" | "~" | "null" | "Null" | "NULL")
}

/// Infiere el tipo de un escalar (data-model, tabla de `PropValue`).
pub fn scalar_value(v: &str, style: ScalarStyle) -> PropValue {
    if style == ScalarStyle::Plain {
        if is_null(v) {
            return PropValue::Null;
        }
        match v {
            "true" | "True" | "TRUE" => return PropValue::Bool(true),
            "false" | "False" | "FALSE" => return PropValue::Bool(false),
            _ => {}
        }
        if is_number(v) {
            return PropValue::Number(v.to_owned());
        }
        if is_date(v) {
            return PropValue::Date(v.to_owned());
        }
        if is_datetime(v) {
            return PropValue::DateTime(v.to_owned());
        }
    }
    if is_link(v) {
        return PropValue::Link(v.to_owned());
    }
    PropValue::Text(v.to_owned())
}

/// Entero o decimal del esquema core de YAML 1.2.
pub fn is_number(v: &str) -> bool {
    let s = v.strip_prefix(['-', '+']).unwrap_or(v);
    if s.is_empty() {
        return false;
    }
    if let Some(h) = s.strip_prefix("0x") {
        return !h.is_empty() && h.bytes().all(|b| b.is_ascii_hexdigit());
    }
    if let Some(o) = s.strip_prefix("0o") {
        return !o.is_empty() && o.bytes().all(|b| (b'0'..=b'7').contains(&b));
    }
    if matches!(s, ".inf" | ".Inf" | ".INF") || matches!(v, ".nan" | ".NaN" | ".NAN") {
        return true;
    }
    let (mantissa, exp) = match s.find(['e', 'E']) {
        Some(i) => (&s[..i], Some(&s[i + 1..])),
        None => (s, None),
    };
    let mut parts = mantissa.splitn(2, '.');
    let int = parts.next().unwrap_or("");
    let frac = parts.next();
    let digits = |x: &str| x.bytes().all(|b| b.is_ascii_digit());
    let mantissa_ok = match frac {
        None => !int.is_empty() && digits(int),
        Some(f) => (!int.is_empty() || !f.is_empty()) && digits(int) && digits(f),
    };
    let exp_ok = exp.is_none_or(|e| {
        let e = e.strip_prefix(['-', '+']).unwrap_or(e);
        !e.is_empty() && digits(e)
    });
    mantissa_ok && exp_ok
}

/// `AAAA-MM-DD`.
pub fn is_date(v: &str) -> bool {
    let b = v.as_bytes();
    b.len() == 10
        && b[4] == b'-'
        && b[7] == b'-'
        && b.iter()
            .enumerate()
            .all(|(i, c)| i == 4 || i == 7 || c.is_ascii_digit())
        && (1..=12).contains(&((b[5] - b'0') * 10 + (b[6] - b'0')))
        && (1..=31).contains(&((b[8] - b'0') * 10 + (b[9] - b'0')))
}

/// `AAAA-MM-DDTHH:MM[:SS[.f]](Z|±HH:MM)`.
pub fn is_datetime(v: &str) -> bool {
    let Some((date, rest)) = v.split_once(['T', 't']) else {
        return false;
    };
    if !is_date(date) {
        return false;
    }
    let (time, off_ok) = if let Some(t) = rest.strip_suffix(['Z', 'z']) {
        (t, true)
    } else if let Some(i) = rest.rfind(['+', '-']) {
        let off = &rest[i + 1..];
        let ob = off.as_bytes();
        (
            &rest[..i],
            ob.len() == 5
                && ob[2] == b':'
                && ob
                    .iter()
                    .enumerate()
                    .all(|(j, c)| j == 2 || c.is_ascii_digit()),
        )
    } else {
        return false;
    };
    let tb = time.as_bytes();
    let hm_ok = tb.len() >= 5
        && tb[2] == b':'
        && tb[..5]
            .iter()
            .enumerate()
            .all(|(j, c)| j == 2 || c.is_ascii_digit());
    let secs_ok = match tb.len() {
        5 => true,
        n if n >= 8 => {
            tb[5] == b':'
                && tb[6].is_ascii_digit()
                && tb[7].is_ascii_digit()
                && (n == 8 || (tb[8] == b'.' && n > 9 && tb[9..].iter().all(u8::is_ascii_digit)))
        }
        _ => false,
    };
    off_ok && hm_ok && secs_ok
}

/// `[[destino]]`, o ruta que empieza con `/`, `./` o `../` y termina en `.md` o `.html`.
pub fn is_link(v: &str) -> bool {
    if v.len() > 4
        && v.starts_with("[[")
        && v.ends_with("]]")
        && !v[2..v.len() - 2].contains(['[', ']'])
    {
        return true;
    }
    (v.starts_with('/') || v.starts_with("./") || v.starts_with("../"))
        && (v.ends_with(".md") || v.ends_with(".html"))
        && !v.contains(char::is_whitespace)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn errors_in_spanish() {
        let e = parse_block("type: atomo\ntags: [a, b\ntitle: roto\n").unwrap_err();
        assert!(
            !e.message.is_ascii() || e.message.starts_with("error de sintaxis"),
            "{}",
            e.message
        );
        assert!(translate("illegal placement of ':' indicator").starts_with("hay un «:»"));
        assert_eq!(translate("algo raro"), "error de sintaxis YAML (algo raro)");
    }

    #[test]
    fn numbers() {
        for ok in [
            "0", "-3", "+4", "3.14", ".5", "1e10", "2.5E-3", "0x1F", "0o17", ".inf", "-.inf",
            ".nan",
        ] {
            assert!(is_number(ok), "{ok}");
        }
        for bad in ["", "-", "1.2.3", "12a", "e5", "0x", "1e"] {
            assert!(!is_number(bad), "{bad}");
        }
    }

    #[test]
    fn dates() {
        assert!(is_date("2026-11-09"));
        assert!(!is_date("2026-13-09"));
        assert!(!is_date("2026-1-09"));
        assert!(is_datetime("2026-06-30T14:00:00Z"));
        assert!(is_datetime("2026-11-09T18:30:00-03:00"));
        assert!(is_datetime("2026-11-09T18:30-03:00"));
        assert!(is_datetime("2026-11-09T18:30:00.123+01:00"));
        assert!(!is_datetime("2026-11-09T18:30:00"));
        assert!(!is_datetime("2026-11-09"));
    }

    #[test]
    fn links() {
        assert!(is_link("[[Entropía]]"));
        assert!(is_link("/wiki/a.md"));
        assert!(is_link("../b.html"));
        assert!(!is_link("[[]]"));
        assert!(!is_link("nota.md"));
        assert!(!is_link("/raw/x.pdf"));
    }
}
