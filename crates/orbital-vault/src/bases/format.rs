use super::*;
use crate::props::emit::LineEnding;
use crate::props::{emit as yaml_emit, yaml};
fn single_document(source: &str) -> Result<(), String> {
    let mut parser = saphyr_parser::Parser::new_from_str(source.trim_start_matches('\u{feff}'));
    let mut documents = 0;
    while let Some(event) = parser.next_event() {
        let (event, _) = event.map_err(|e| e.to_string())?;
        if matches!(event, saphyr_parser::Event::DocumentStart(_)) {
            documents += 1;
            if documents > 1 {
                return Err("La base debe contener un único documento YAML".into());
            }
        }
    }
    Ok(())
}
fn text(v: &PropValue) -> Result<String, String> {
    match v {
        PropValue::Text(s) | PropValue::Link(s) => Ok(s.clone()),
        _ => Err("Se esperaba texto".into()),
    }
}
fn map(v: &PropValue) -> Result<&[(String, PropValue)], String> {
    if let PropValue::Map(m) = v {
        Ok(m)
    } else {
        Err("Se esperaba un mapa".into())
    }
}
fn allowed(m: &[(String, PropValue)], keys: &[&str]) -> Result<(), String> {
    let mut seen = std::collections::HashSet::new();
    for (k, _) in m {
        if !keys.contains(&k.as_str()) {
            return Err(format!("Campo desconocido: {k}"));
        }
        if !seen.insert(k) {
            return Err(format!("Campo duplicado: {k}"));
        }
    }
    Ok(())
}
fn get<'a>(m: &'a [(String, PropValue)], k: &str) -> Option<&'a PropValue> {
    m.iter().find(|(key, _)| key == k).map(|(_, v)| v)
}
fn required(m: &[(String, PropValue)], k: &str) -> Result<String, String> {
    text(get(m, k).ok_or_else(|| format!("Falta {k}"))?)
}
fn list(v: &PropValue) -> Result<&[PropValue], String> {
    if let PropValue::List(l) = v {
        Ok(l)
    } else {
        Err("Se esperaba una lista".into())
    }
}
fn boolean(v: &PropValue) -> Result<bool, String> {
    if let PropValue::Bool(b) = v {
        Ok(*b)
    } else {
        Err("Se esperaba verdadero o falso".into())
    }
}
impl PropertyType {
    pub fn name(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Text => "text",
            Self::Number => "number",
            Self::Bool => "bool",
            Self::Date => "date",
            Self::DateTime => "datetime",
            Self::Link => "link",
            Self::List => "list",
        }
    }
    pub fn from_name(s: &str) -> Option<Self> {
        Some(match s {
            "auto" => Self::Auto,
            "text" => Self::Text,
            "number" => Self::Number,
            "bool" => Self::Bool,
            "date" => Self::Date,
            "datetime" => Self::DateTime,
            "link" => Self::Link,
            "list" => Self::List,
            _ => return None,
        })
    }
}
pub fn parse(source: &str) -> Result<BaseDefinition, String> {
    single_document(source)?;
    let entries = yaml::parse_block(source.trim_start_matches('\u{feff}'))
        .map_err(|e| format!("Línea {}: {}", e.line, e.message))?;
    let m: Vec<_> = entries.into_iter().map(|e| (e.key, e.value)).collect();
    allowed(
        &m,
        &["format_version", "view", "columns", "filters", "sort"],
    )?;
    if get(&m, "format_version") != Some(&PropValue::Number("1".into())) {
        return Err("Versión de base desconocida; se requiere format_version: 1".into());
    }
    let mut d = BaseDefinition::default();
    if let Some(v) = get(&m, "view") {
        d.view = match text(v)?.as_str() {
            "table" => BaseView::Table,
            "cards" => BaseView::Cards,
            _ => return Err("Vista desconocida".into()),
        };
    }
    if let Some(v) = get(&m, "columns") {
        d.columns.clear();
        for v in list(v)? {
            let c = map(v)?;
            allowed(c, &["property", "label", "type"])?;
            let property = required(c, "property")?;
            if property.trim().is_empty() || property.chars().any(char::is_control) {
                return Err("La propiedad no puede estar vacía".into());
            }
            let label = get(c, "label")
                .map(text)
                .transpose()?
                .unwrap_or_else(|| property.clone());
            let kind = get(c, "type")
                .map(text)
                .transpose()?
                .unwrap_or_else(|| "auto".into());
            d.columns.push(BaseColumn {
                property,
                label,
                kind: PropertyType::from_name(&kind).ok_or("Tipo desconocido")?,
            });
        }
    }
    if d.columns.is_empty() {
        return Err("La base necesita al menos una columna".into());
    }
    let mut seen = std::collections::HashSet::new();
    if d.columns.iter().any(|c| !seen.insert(&c.property)) {
        return Err("La base tiene columnas duplicadas".into());
    }
    if let Some(v) = get(&m, "filters") {
        for v in list(v)? {
            let f = map(v)?;
            allowed(f, &["folder", "tag", "property", "value"])?;
            let discriminants = ["folder", "tag", "property"]
                .iter()
                .filter(|k| get(f, k).is_some())
                .count();
            if discriminants != 1 {
                return Err("Cada filtro debe tener una carpeta, etiqueta o propiedad".into());
            }
            if let Some(v) = get(f, "folder") {
                let s = text(v)?;
                if !s.is_empty() {
                    RelPath::new(&s).map_err(|e| e.to_string())?;
                }
                if get(f, "value").is_some() {
                    return Err("Una carpeta no admite value".into());
                }
                d.filters.push(BaseFilter::Folder(s));
            } else if let Some(v) = get(f, "tag") {
                if get(f, "value").is_some() {
                    return Err("Una etiqueta no admite value".into());
                }
                d.filters.push(BaseFilter::Tag(text(v)?));
            } else {
                d.filters.push(BaseFilter::Property {
                    key: required(f, "property")?,
                    value: get(f, "value").cloned(),
                });
            }
        }
    }
    if let Some(v) = get(&m, "sort") {
        let s = map(v)?;
        allowed(s, &["property", "descending"])?;
        d.sort = Some(BaseSort {
            property: required(s, "property")?,
            descending: get(s, "descending")
                .map(boolean)
                .transpose()?
                .unwrap_or(false),
        });
    }
    Ok(d)
}
fn s(v: &str) -> PropValue {
    PropValue::Text(v.into())
}
pub fn emit(d: &BaseDefinition) -> String {
    let columns = d
        .columns
        .iter()
        .map(|c| {
            PropValue::Map(vec![
                ("property".into(), s(&c.property)),
                ("label".into(), s(&c.label)),
                ("type".into(), s(c.kind.name())),
            ])
        })
        .collect();
    let filters = d
        .filters
        .iter()
        .map(|f| {
            PropValue::Map(match f {
                BaseFilter::Folder(p) => vec![("folder".into(), s(p))],
                BaseFilter::Tag(t) => vec![("tag".into(), s(t))],
                BaseFilter::Property { key, value } => {
                    let mut m = vec![("property".into(), s(key))];
                    if let Some(v) = value {
                        m.push(("value".into(), v.clone()));
                    }
                    m
                }
            })
        })
        .collect();
    let mut fields = vec![
        ("format_version", PropValue::Number(d.version.to_string())),
        (
            "view",
            s(if d.view == BaseView::Table {
                "table"
            } else {
                "cards"
            }),
        ),
        ("columns", PropValue::List(columns)),
        ("filters", PropValue::List(filters)),
    ];
    if let Some(sort) = &d.sort {
        fields.push((
            "sort",
            PropValue::Map(vec![
                ("property".into(), s(&sort.property)),
                ("descending".into(), PropValue::Bool(sort.descending)),
            ]),
        ));
    }
    yaml_emit::document(&fields, LineEnding::Lf)
}
fn valid_date(s: &str) -> bool {
    if !yaml::is_date(s) {
        return false;
    }
    let Ok(y) = s[0..4].parse::<u32>() else {
        return false;
    };
    let Ok(m) = s[5..7].parse::<usize>() else {
        return false;
    };
    let Ok(d) = s[8..10].parse::<u32>() else {
        return false;
    };
    let days = [
        31,
        if y % 4 == 0 && (y % 100 != 0 || y % 400 == 0) {
            29
        } else {
            28
        },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];
    (1..=12).contains(&m) && d > 0 && d <= days[m - 1]
}
pub fn validate_value(kind: PropertyType, v: &PropValue) -> bool {
    if matches!(v, PropValue::Null) {
        return true;
    }
    match (kind, v) {
        (PropertyType::Auto, _) => true,
        (PropertyType::Text, PropValue::Text(_)) => true,
        (PropertyType::Number, PropValue::Number(s)) => number(s).is_some(),
        (PropertyType::Bool, PropValue::Bool(_)) => true,
        (PropertyType::Date, PropValue::Date(s)) => valid_date(s),
        (PropertyType::DateTime, PropValue::DateTime(s)) => datetime(s).is_some(),
        (PropertyType::Link, PropValue::Link(s)) => yaml::is_link(s),
        (PropertyType::List, PropValue::List(_)) => true,
        _ => false,
    }
}
/// Forma decimal exacta, sin redondeo de coma flotante.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Numeric {
    negative: bool,
    magnitude: i64,
    digits: String,
}
impl Ord for Numeric {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        if self.negative != other.negative {
            return other.negative.cmp(&self.negative);
        }
        let order = self.magnitude.cmp(&other.magnitude).then_with(|| {
            let n = self.digits.len().max(other.digits.len());
            self.digits
                .bytes()
                .chain(std::iter::repeat(b'0'))
                .take(n)
                .cmp(other.digits.bytes().chain(std::iter::repeat(b'0')).take(n))
        });
        if self.negative {
            order.reverse()
        } else {
            order
        }
    }
}
impl PartialOrd for Numeric {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}
pub(crate) fn number(s: &str) -> Option<Numeric> {
    if !yaml::is_number(s) {
        return None;
    }
    let clean = s.replace('_', "");
    let negative = clean.starts_with('-');
    let raw = clean.trim_start_matches(['+', '-']);
    let radix = if raw.starts_with("0x") {
        Some(16)
    } else if raw.starts_with("0o") {
        Some(8)
    } else if raw.starts_with("0b") {
        Some(2)
    } else {
        None
    };
    let owned;
    let raw = if let Some(radix) = radix {
        let mut decimal = vec![0u8];
        for c in raw[2..].chars() {
            let mut carry = c.to_digit(radix)?;
            for d in decimal.iter_mut().rev() {
                let value = *d as u32 * radix + carry;
                *d = (value % 10) as u8;
                carry = value / 10;
            }
            while carry > 0 {
                decimal.insert(0, (carry % 10) as u8);
                carry /= 10;
            }
        }
        owned = decimal
            .into_iter()
            .map(|d| (b'0' + d) as char)
            .collect::<String>();
        owned.as_str()
    } else {
        raw
    };
    let (mantissa, exp) = if let Some((m, e)) = raw.split_once(['e', 'E']) {
        (m, e.parse::<i64>().ok()?)
    } else {
        (raw, 0)
    };
    let before = mantissa.find('.').unwrap_or(mantissa.len());
    let all = mantissa.replace('.', "");
    if all.is_empty() || !all.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let leading = all.bytes().take_while(|b| *b == b'0').count();
    let digits = all[leading..].trim_end_matches('0').to_owned();
    if digits.is_empty() {
        return Some(Numeric {
            negative: false,
            magnitude: i64::MIN,
            digits: "0".into(),
        });
    }
    Some(Numeric {
        negative,
        magnitude: (before as i64)
            .checked_sub(leading as i64)?
            .checked_add(exp)?,
        digits,
    })
}
/// Segundos desde un origen común y fracción decimal; respeta el huso horario.
pub(crate) fn datetime(s: &str) -> Option<(i64, String)> {
    if !yaml::is_datetime(s) || !valid_date(&s[..10]) {
        return None;
    }
    let year = s[..4].parse::<i64>().ok()?;
    let month = s[5..7].parse::<usize>().ok()?;
    let day = s[8..10].parse::<i64>().ok()?;
    let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let months = [
        31,
        if leap { 29 } else { 28 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];
    let previous = year - 1;
    let days = 365 * year + previous.div_euclid(4) - previous.div_euclid(100)
        + previous.div_euclid(400)
        + months[..month - 1].iter().sum::<i64>()
        + day;
    let rest = &s[11..];
    let (time, offset) = if let Some(t) = rest.strip_suffix(['Z', 'z']) {
        (t, 0)
    } else {
        let i = rest.rfind(['+', '-'])?;
        let h = rest[i + 1..i + 3].parse::<i64>().ok()?;
        let m = rest[i + 4..].parse::<i64>().ok()?;
        if h > 23 || m > 59 {
            return None;
        }
        (
            &rest[..i],
            (h * 3600 + m * 60) * if rest.as_bytes()[i] == b'-' { -1 } else { 1 },
        )
    };
    let h = time[..2].parse::<i64>().ok()?;
    let m = time[3..5].parse::<i64>().ok()?;
    let secs = if time.len() > 5 {
        time[6..8].parse::<i64>().ok()?
    } else {
        0
    };
    if h > 23 || m > 59 || secs > 59 {
        return None;
    }
    let fraction = time
        .split_once('.')
        .map_or("", |(_, f)| f)
        .trim_end_matches('0')
        .to_owned();
    Some((days * 86400 + h * 3600 + m * 60 + secs - offset, fraction))
}
pub fn parse_value(kind: PropertyType, input: &str) -> Result<PropValue, String> {
    let value = if kind == PropertyType::Link && yaml::is_link(input) {
        PropValue::Link(input.into())
    } else if kind == PropertyType::Text {
        PropValue::Text(input.into())
    } else {
        let doc = format!("value: {input}");
        single_document(&doc)?;
        let entries = yaml::parse_block(&doc).map_err(|e| e.message)?;
        if entries.len() != 1 {
            return Err("Escribí un solo valor".into());
        }
        entries[0].value.clone()
    };
    if validate_value(kind, &value) {
        Ok(value)
    } else {
        Err(format!("El valor no es de tipo {}", kind.name()))
    }
}
