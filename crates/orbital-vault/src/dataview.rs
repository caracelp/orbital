//! Motor declarativo tipo Dataview en Rust puro, sin JavaScript (065).
//!
//! Soporta bloques ` ```dataview ` / ` ```dv ` y expresiones inline `` `= expr` `` sobre el
//! [`SearchIndex`] en memoria:
//! - Vistas: `TABLE [WITHOUT ID]`, `LIST`, `TASK` y `CALENDAR <expr>`.
//! - Cláusulas: `FROM`, `WHERE`, `FLATTEN`, `GROUP BY`, `SORT`, `LIMIT`.
//! - Campos implícitos `file.*`, acceso a la nota abierta `this.*`, campos inline `[k:: v]` /
//!   `(k:: v)` y >30 funciones nativas (incluyendo `filter`/`map` con lambdas `(x) => ...`).

use std::cmp::Ordering;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::entry::{RelPath, Zone};
use crate::fold::fold;
use crate::props::PropValue;
use crate::search::{Doc, SearchIndex, TaskDoc};

// ---------------------------------------------------------------------------
// Modelo de valores
// ---------------------------------------------------------------------------

/// Fecha calendario civil (AAAA-MM-DD).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Fecha {
    pub anio: i32,
    pub mes: u8,
    pub dia: u8,
}

impl Fecha {
    pub fn parse(s: &str) -> Option<Self> {
        let t = s.trim();
        let pref = t.get(..10)?;
        let bytes = pref.as_bytes();
        if bytes[4] != b'-' || bytes[7] != b'-' {
            return None;
        }
        let anio: i32 = pref[0..4].parse().ok()?;
        let mes: u8 = pref[5..7].parse().ok()?;
        let dia: u8 = pref[8..10].parse().ok()?;
        if !(1..=12).contains(&mes) || !(1..=31).contains(&dia) {
            return None;
        }
        let max_d = dias_del_mes(anio, mes);
        if dia > max_d {
            return None;
        }
        Some(Self { anio, mes, dia })
    }

    pub fn desde_system_time(t: SystemTime) -> Self {
        let secs = t
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        Self::desde_dias_epoch(secs.div_euclid(86_400))
    }

    pub fn hoy() -> Self {
        Self::desde_system_time(SystemTime::now())
    }

    /// Conversión civil desde días desde 1970-01-01 (algoritmo de Howard Hinnant).
    pub fn desde_dias_epoch(z: i64) -> Self {
        let z = z + 719_468;
        let era = z.div_euclid(146_097);
        let doe = z - era * 146_097;
        let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
        let y = yoe + era * 400;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let d = doy - (153 * mp + 2) / 5 + 1;
        let m = if mp < 10 { mp + 3 } else { mp - 9 };
        let anio = (y + i64::from(m <= 2)) as i32;
        Self {
            anio,
            mes: m as u8,
            dia: d as u8,
        }
    }

    pub fn a_dias_epoch(self) -> i64 {
        let y = i64::from(self.anio) - i64::from(self.mes <= 2);
        let era = y.div_euclid(400);
        let yoe = y - era * 400;
        let m = i64::from(self.mes);
        let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + i64::from(self.dia) - 1;
        let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
        era * 146_097 + doe - 719_468
    }

    pub fn sumar_dias(self, dias: i64) -> Self {
        Self::desde_dias_epoch(self.a_dias_epoch() + dias)
    }

    /// Día de la semana: 0 = Lunes .. 6 = Domingo.
    pub fn dia_semana_lunes(self) -> u8 {
        // 1970-01-01 fue jueves (3 en 0=lunes).
        (self.a_dias_epoch() + 3).rem_euclid(7) as u8
    }

    pub fn formato(self, fmt: &str) -> String {
        fmt.replace("YYYY", &format!("{:04}", self.anio))
            .replace("yyyy", &format!("{:04}", self.anio))
            .replace("MM", &format!("{:02}", self.mes))
            .replace("DD", &format!("{:02}", self.dia))
            .replace("dd", &format!("{:02}", self.dia))
    }
}

impl std::fmt::Display for Fecha {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:04}-{:02}-{:02}", self.anio, self.mes, self.dia)
    }
}

pub fn dias_del_mes(anio: i32, mes: u8) -> u8 {
    match mes {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => {
            if (anio % 4 == 0 && anio % 100 != 0) || (anio % 400 == 0) {
                29
            } else {
                28
            }
        }
        _ => 30,
    }
}

/// Valor dinámico en tiempo de ejecución del motor DQL.
#[derive(Clone, Debug, PartialEq)]
pub enum Valor {
    Null,
    Bool(bool),
    Numero(f64),
    Texto(String),
    Fecha(Fecha),
    /// Duración en días.
    Duracion(i64),
    Lista(Vec<Valor>),
    Enlace {
        ruta: RelPath,
        etiqueta: String,
    },
    Mapa(Vec<(String, Valor)>),
}

impl Valor {
    pub fn es_verdadero(&self) -> bool {
        match self {
            Self::Null => false,
            Self::Bool(b) => *b,
            Self::Numero(n) => *n != 0.0 && !n.is_nan(),
            Self::Texto(s) => !s.is_empty(),
            Self::Fecha(_) => true,
            Self::Duracion(d) => *d != 0,
            Self::Lista(l) => !l.is_empty(),
            Self::Enlace { .. } => true,
            Self::Mapa(m) => !m.is_empty(),
        }
    }

    pub fn a_numero(&self) -> Option<f64> {
        match self {
            Self::Numero(n) => Some(*n),
            Self::Bool(b) => Some(if *b { 1.0 } else { 0.0 }),
            Self::Texto(s) => s.trim().replace(',', ".").parse::<f64>().ok(),
            Self::Duracion(d) => Some(*d as f64),
            _ => None,
        }
    }

    pub fn a_fecha(&self) -> Option<Fecha> {
        match self {
            Self::Fecha(f) => Some(*f),
            Self::Texto(s) => Fecha::parse(s),
            _ => None,
        }
    }

    pub fn a_texto(&self) -> String {
        match self {
            Self::Null => String::new(),
            Self::Bool(b) => if *b { "sí" } else { "no" }.to_owned(),
            Self::Numero(n) => {
                if (*n - n.round()).abs() < 1e-9 {
                    format!("{}", n.round() as i64)
                } else {
                    let s = format!("{n:.4}");
                    s.trim_end_matches('0').trim_end_matches('.').to_owned()
                }
            }
            Self::Texto(s) => s.clone(),
            Self::Fecha(f) => f.to_string(),
            Self::Duracion(d) => {
                if *d == 1 {
                    "1 día".into()
                } else {
                    format!("{d} días")
                }
            }
            Self::Lista(l) => l
                .iter()
                .map(Self::a_texto)
                .filter(|s| !s.is_empty())
                .collect::<Vec<_>>()
                .join(", "),
            Self::Enlace { etiqueta, .. } => etiqueta.clone(),
            Self::Mapa(m) => m
                .iter()
                .map(|(k, v)| format!("{k}: {}", v.a_texto()))
                .collect::<Vec<_>>()
                .join(", "),
        }
    }

    pub fn desde_prop(p: &PropValue, index: &SearchIndex) -> Self {
        match p {
            PropValue::Null => Self::Null,
            PropValue::Bool(b) => Self::Bool(*b),
            PropValue::Number(s) => s
                .trim()
                .parse::<f64>()
                .map(Self::Numero)
                .unwrap_or_else(|_| Self::Texto(s.clone())),
            PropValue::Date(s) | PropValue::DateTime(s) => Fecha::parse(s)
                .map(Self::Fecha)
                .unwrap_or_else(|| Self::Texto(s.clone())),
            PropValue::Link(s) => resolver_enlace_valor(s, index),
            PropValue::List(l) => {
                Self::Lista(l.iter().map(|v| Self::desde_prop(v, index)).collect())
            }
            PropValue::Map(m) => Self::Mapa(
                m.iter()
                    .map(|(k, v)| (k.clone(), Self::desde_prop(v, index)))
                    .collect(),
            ),
            PropValue::Text(s) => {
                let t = s.trim();
                if t.starts_with("[[") && t.ends_with("]]") && !t[2..t.len() - 2].contains("]]") {
                    return resolver_enlace_valor(&t[2..t.len() - 2], index);
                }
                if let Some(f) = Fecha::parse(t)
                    && (t.len() == 10 || t.as_bytes().get(10) == Some(&b'T'))
                {
                    return Self::Fecha(f);
                }
                if let Ok(n) = t.parse::<f64>()
                    && n.is_finite()
                {
                    return Self::Numero(n);
                }
                if t.eq_ignore_ascii_case("true") {
                    return Self::Bool(true);
                }
                if t.eq_ignore_ascii_case("false") {
                    return Self::Bool(false);
                }
                Self::Texto(s.clone())
            }
        }
    }

    pub fn campo(&self, clave: &str) -> Self {
        let k = fold(clave);
        match self {
            Self::Mapa(m) => m
                .iter()
                .find(|(mk, _)| fold(mk) == k)
                .map(|(_, v)| v.clone())
                .unwrap_or(Self::Null),
            // Proyección sobre lista de mapas (`rows.puntaje` en GROUP BY).
            Self::Lista(items) => {
                let vals: Vec<Valor> = items.iter().map(|it| it.campo(clave)).collect();
                Self::Lista(vals)
            }
            Self::Fecha(f) => match k.as_str() {
                "year" | "anio" => Self::Numero(f64::from(f.anio)),
                "month" | "mes" => Self::Numero(f64::from(f.mes)),
                "day" | "dia" => Self::Numero(f64::from(f.dia)),
                _ => Self::Null,
            },
            _ => Self::Null,
        }
    }
}

fn comparar_valores(a: &Valor, b: &Valor) -> Ordering {
    match (a, b) {
        (Valor::Null, Valor::Null) => Ordering::Equal,
        (Valor::Null, _) => Ordering::Less,
        (_, Valor::Null) => Ordering::Greater,
        (Valor::Numero(x), Valor::Numero(y)) => x.partial_cmp(y).unwrap_or(Ordering::Equal),
        (Valor::Fecha(x), Valor::Fecha(y)) => x.cmp(y),
        (Valor::Duracion(x), Valor::Duracion(y)) => x.cmp(y),
        (Valor::Bool(x), Valor::Bool(y)) => x.cmp(y),
        (Valor::Fecha(x), Valor::Texto(s)) => {
            Fecha::parse(s).map_or_else(|| fold(&x.to_string()).cmp(&fold(s)), |y| x.cmp(&y))
        }
        (Valor::Texto(s), Valor::Fecha(y)) => {
            Fecha::parse(s).map_or_else(|| fold(s).cmp(&fold(&y.to_string())), |x| x.cmp(y))
        }
        (Valor::Numero(x), Valor::Texto(s)) => s.trim().parse::<f64>().map_or_else(
            |_| fold(&a.a_texto()).cmp(&fold(s)),
            |y| x.partial_cmp(&y).unwrap_or(Ordering::Equal),
        ),
        (Valor::Texto(s), Valor::Numero(y)) => s.trim().parse::<f64>().map_or_else(
            |_| fold(s).cmp(&fold(&b.a_texto())),
            |x| x.partial_cmp(y).unwrap_or(Ordering::Equal),
        ),
        _ => fold(&a.a_texto()).cmp(&fold(&b.a_texto())),
    }
}

fn iguales(a: &Valor, b: &Valor) -> bool {
    match (a, b) {
        (Valor::Null, Valor::Null) => true,
        (Valor::Bool(x), Valor::Bool(y)) => x == y,
        (Valor::Numero(x), Valor::Numero(y)) => (x - y).abs() < 1e-9,
        (Valor::Fecha(x), Valor::Fecha(y)) => x == y,
        (Valor::Duracion(x), Valor::Duracion(y)) => x == y,
        (Valor::Enlace { ruta: r1, .. }, Valor::Enlace { ruta: r2, .. }) => r1 == r2,
        (Valor::Fecha(x), Valor::Texto(s)) | (Valor::Texto(s), Valor::Fecha(x)) => {
            Fecha::parse(s).is_some_and(|y| y == *x)
        }
        (Valor::Numero(x), Valor::Texto(s)) | (Valor::Texto(s), Valor::Numero(x)) => {
            s.trim().parse::<f64>().is_ok_and(|y| (x - y).abs() < 1e-9)
        }
        (Valor::Texto(x), Valor::Texto(y)) => fold(x) == fold(y),
        (Valor::Enlace { ruta, etiqueta }, Valor::Texto(t))
        | (Valor::Texto(t), Valor::Enlace { ruta, etiqueta }) => {
            let tf = fold(t);
            fold(etiqueta) == tf || fold(ruta.as_str()) == tf || fold(stem_de(ruta)) == tf
        }
        _ => false,
    }
}

fn stem_de(path: &RelPath) -> &str {
    let f = path.file_name();
    f.rsplit_once('.').map_or(f, |(s, _)| s)
}

fn resolver_enlace_valor(raw: &str, index: &SearchIndex) -> Valor {
    let target = crate::links::parse_link_target(raw);
    let buscado = fold(
        target
            .path
            .rsplit('/')
            .next()
            .unwrap_or(&target.path)
            .strip_suffix(".md")
            .or_else(|| target.path.strip_suffix(".html"))
            .unwrap_or(&target.path),
    );
    let etiqueta = target.alias.unwrap_or_else(|| {
        target
            .path
            .rsplit('/')
            .next()
            .unwrap_or(&target.path)
            .to_owned()
    });
    if let Some((path, _)) = index
        .docs
        .iter()
        .find(|(p, d)| d.name == buscado || fold(p.as_str()) == fold(&target.path))
    {
        Valor::Enlace {
            ruta: path.clone(),
            etiqueta,
        }
    } else {
        Valor::Texto(etiqueta)
    }
}

// ---------------------------------------------------------------------------
// AST y Parser DQL
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
pub struct ColumnaTabla {
    pub expr: Expr,
    pub alias: String,
}

#[derive(Clone, Debug)]
pub enum TipoVista {
    Tabla {
        sin_id: bool,
        columnas: Vec<ColumnaTabla>,
    },
    Lista {
        expr: Option<Expr>,
    },
    Tareas,
    Calendario {
        expr: Expr,
    },
}

#[derive(Clone, Debug)]
pub enum Fuente {
    Todo,
    Etiqueta(String),
    Carpeta(String),
    Entrantes(String),
    Salientes(String),
    Negado(Box<Fuente>),
    And(Box<Fuente>, Box<Fuente>),
    Or(Box<Fuente>, Box<Fuente>),
}

#[derive(Clone, Debug)]
pub struct ClaveOrden {
    pub expr: Expr,
    pub descendente: bool,
}

#[derive(Clone, Debug)]
pub enum Clausula {
    Where(Expr),
    Sort(Vec<ClaveOrden>),
    GroupBy { expr: Expr, alias: Option<String> },
    Flatten { expr: Expr, alias: Option<String> },
    Limit(usize),
}

#[derive(Clone, Debug)]
pub struct ConsultaDv {
    pub vista: TipoVista,
    pub fuente: Fuente,
    pub clausulas: Vec<Clausula>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OpBin {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    And,
    Or,
}

#[derive(Clone, Debug)]
pub enum Expr {
    Null,
    Bool(bool),
    Num(f64),
    Str(String),
    Link(String),
    Ident(String),
    Lista(Vec<Expr>),
    Campo(Box<Expr>, String),
    Indice(Box<Expr>, Box<Expr>),
    Neg(Box<Expr>),
    Not(Box<Expr>),
    Bin(Box<Expr>, OpBin, Box<Expr>),
    Llamada(String, Vec<Expr>),
    Lambda(String, Box<Expr>),
}

// --- Lexer de expresiones ---

#[derive(Clone, Debug, PartialEq)]
enum Tok {
    Num(f64),
    Str(String),
    Link(String),
    Ident(String),
    Dot,
    Comma,
    LParen,
    RParen,
    LBracket,
    RBracket,
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    Bang,
    AndAnd,
    OrOr,
    Arrow,
}

fn tokenizar_expr(src: &str) -> Result<Vec<Tok>, String> {
    let mut out = Vec::new();
    let bytes = src.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if b.is_ascii_whitespace() {
            i += 1;
            continue;
        }
        if b == b'"' || b == b'\'' {
            let q = b;
            i += 1;
            let mut s = String::new();
            let mut cerrado = false;
            while i < bytes.len() {
                let c = src[i..].chars().next().unwrap();
                if c as u32 == u32::from(q) {
                    i += 1;
                    cerrado = true;
                    break;
                }
                if c == '\\' && i + 1 < bytes.len() {
                    i += 1;
                    let esc = src[i..].chars().next().unwrap();
                    s.push(match esc {
                        'n' => '\n',
                        't' => '\t',
                        other => other,
                    });
                    i += esc.len_utf8();
                } else {
                    s.push(c);
                    i += c.len_utf8();
                }
            }
            if !cerrado {
                return Err("Cadena sin comillas de cierre".into());
            }
            out.push(Tok::Str(s));
            continue;
        }
        if b == b'[' && i + 1 < bytes.len() && bytes[i + 1] == b'[' {
            let start = i + 2;
            let Some(rel) = src[start..].find("]]") else {
                return Err("Wikilink sin «]]» de cierre".into());
            };
            out.push(Tok::Link(src[start..start + rel].to_owned()));
            i = start + rel + 2;
            continue;
        }
        if b.is_ascii_digit() {
            let start = i;
            while i < bytes.len() && (bytes[i].is_ascii_digit() || bytes[i] == b'.') {
                if bytes[i] == b'.' && bytes.get(i + 1) == Some(&b'.') {
                    break;
                }
                i += 1;
            }
            let n: f64 = src[start..i]
                .parse()
                .map_err(|_| format!("Número inválido: «{}»", &src[start..i]))?;
            out.push(Tok::Num(n));
            continue;
        }
        // Operadores de dos caracteres
        if i + 1 < bytes.len() {
            match (b, bytes[i + 1]) {
                (b'=', b'>') => {
                    out.push(Tok::Arrow);
                    i += 2;
                    continue;
                }
                (b'=', b'=') => {
                    out.push(Tok::Eq);
                    i += 2;
                    continue;
                }
                (b'!', b'=') => {
                    out.push(Tok::Ne);
                    i += 2;
                    continue;
                }
                (b'<', b'=') => {
                    out.push(Tok::Le);
                    i += 2;
                    continue;
                }
                (b'>', b'=') => {
                    out.push(Tok::Ge);
                    i += 2;
                    continue;
                }
                (b'&', b'&') => {
                    out.push(Tok::AndAnd);
                    i += 2;
                    continue;
                }
                (b'|', b'|') => {
                    out.push(Tok::OrOr);
                    i += 2;
                    continue;
                }
                _ => {}
            }
        }
        let simple = match b {
            b'.' => Some(Tok::Dot),
            b',' => Some(Tok::Comma),
            b'(' => Some(Tok::LParen),
            b')' => Some(Tok::RParen),
            b'[' => Some(Tok::LBracket),
            b']' => Some(Tok::RBracket),
            b'+' => Some(Tok::Plus),
            b'-' => Some(Tok::Minus),
            b'*' => Some(Tok::Star),
            b'/' => Some(Tok::Slash),
            b'%' => Some(Tok::Percent),
            b'=' => Some(Tok::Eq),
            b'<' => Some(Tok::Lt),
            b'>' => Some(Tok::Gt),
            b'!' => Some(Tok::Bang),
            _ => None,
        };
        if let Some(t) = simple {
            out.push(t);
            i += 1;
            continue;
        }
        let ch = src[i..].chars().next().unwrap();
        if ch.is_alphabetic() || matches!(ch, '_' | '#') {
            let start = i;
            i += ch.len_utf8();
            while i < bytes.len() {
                let c = src[i..].chars().next().unwrap();
                if c.is_alphanumeric() || matches!(c, '_' | '-') {
                    // Un `-` seguido de espacio o dígito fuera de identificador es resta.
                    if c == '-'
                        && src[i + 1..]
                            .chars()
                            .next()
                            .is_none_or(|nx| !nx.is_alphabetic())
                    {
                        break;
                    }
                    i += c.len_utf8();
                } else {
                    break;
                }
            }
            out.push(Tok::Ident(src[start..i].to_owned()));
            continue;
        }
        return Err(format!("Carácter inesperado en expresión: «{ch}»"));
    }
    Ok(out)
}

struct ExprP {
    toks: Vec<Tok>,
    pos: usize,
}

pub fn parse_expr(src: &str) -> Result<Expr, String> {
    let toks = tokenizar_expr(src)?;
    if toks.is_empty() {
        return Err("Expresión vacía".into());
    }
    let mut p = ExprP { toks, pos: 0 };
    let e = p.parse_or()?;
    if p.pos < p.toks.len() {
        return Err(format!(
            "Token inesperado en expresión: {:?}",
            p.toks[p.pos]
        ));
    }
    Ok(e)
}

impl ExprP {
    fn peek(&self) -> Option<&Tok> {
        self.toks.get(self.pos)
    }

    fn is_kw(&self, kw: &str) -> bool {
        matches!(self.peek(), Some(Tok::Ident(s)) if s.eq_ignore_ascii_case(kw))
    }

    fn parse_or(&mut self) -> Result<Expr, String> {
        // Lambda: `x => expr` o `(x) => expr`
        if let Some(Tok::Ident(var)) = self.toks.get(self.pos).cloned()
            && self.toks.get(self.pos + 1) == Some(&Tok::Arrow)
        {
            self.pos += 2;
            let body = self.parse_or()?;
            return Ok(Expr::Lambda(var, Box::new(body)));
        }
        if self.toks.get(self.pos) == Some(&Tok::LParen)
            && let Some(Tok::Ident(var)) = self.toks.get(self.pos + 1).cloned()
            && self.toks.get(self.pos + 2) == Some(&Tok::RParen)
            && self.toks.get(self.pos + 3) == Some(&Tok::Arrow)
        {
            self.pos += 4;
            let body = self.parse_or()?;
            return Ok(Expr::Lambda(var, Box::new(body)));
        }
        let mut lhs = self.parse_and()?;
        while self.peek() == Some(&Tok::OrOr) || self.is_kw("OR") {
            self.pos += 1;
            let rhs = self.parse_and()?;
            lhs = Expr::Bin(Box::new(lhs), OpBin::Or, Box::new(rhs));
        }
        Ok(lhs)
    }

    fn parse_and(&mut self) -> Result<Expr, String> {
        let mut lhs = self.parse_cmp()?;
        while self.peek() == Some(&Tok::AndAnd) || self.is_kw("AND") {
            self.pos += 1;
            let rhs = self.parse_cmp()?;
            lhs = Expr::Bin(Box::new(lhs), OpBin::And, Box::new(rhs));
        }
        Ok(lhs)
    }

    fn parse_cmp(&mut self) -> Result<Expr, String> {
        let mut lhs = self.parse_add()?;
        loop {
            let op = match self.peek() {
                Some(Tok::Eq) => OpBin::Eq,
                Some(Tok::Ne) => OpBin::Ne,
                Some(Tok::Lt) => OpBin::Lt,
                Some(Tok::Le) => OpBin::Le,
                Some(Tok::Gt) => OpBin::Gt,
                Some(Tok::Ge) => OpBin::Ge,
                _ => break,
            };
            self.pos += 1;
            let rhs = self.parse_add()?;
            lhs = Expr::Bin(Box::new(lhs), op, Box::new(rhs));
        }
        Ok(lhs)
    }

    fn parse_add(&mut self) -> Result<Expr, String> {
        let mut lhs = self.parse_mul()?;
        loop {
            let op = match self.peek() {
                Some(Tok::Plus) => OpBin::Add,
                Some(Tok::Minus) => OpBin::Sub,
                _ => break,
            };
            self.pos += 1;
            let rhs = self.parse_mul()?;
            lhs = Expr::Bin(Box::new(lhs), op, Box::new(rhs));
        }
        Ok(lhs)
    }

    fn parse_mul(&mut self) -> Result<Expr, String> {
        let mut lhs = self.parse_unary()?;
        loop {
            let op = match self.peek() {
                Some(Tok::Star) => OpBin::Mul,
                Some(Tok::Slash) => OpBin::Div,
                Some(Tok::Percent) => OpBin::Mod,
                _ => break,
            };
            self.pos += 1;
            let rhs = self.parse_unary()?;
            lhs = Expr::Bin(Box::new(lhs), op, Box::new(rhs));
        }
        Ok(lhs)
    }

    fn parse_unary(&mut self) -> Result<Expr, String> {
        match self.peek() {
            Some(Tok::Bang) => {
                self.pos += 1;
                Ok(Expr::Not(Box::new(self.parse_unary()?)))
            }
            Some(Tok::Minus) => {
                self.pos += 1;
                Ok(Expr::Neg(Box::new(self.parse_unary()?)))
            }
            _ => self.parse_postfix(),
        }
    }

    fn parse_postfix(&mut self) -> Result<Expr, String> {
        let mut expr = self.parse_primary()?;
        loop {
            match self.peek() {
                Some(Tok::Dot) => {
                    self.pos += 1;
                    let Some(Tok::Ident(c)) = self.toks.get(self.pos).cloned() else {
                        return Err("Se esperaba un nombre de campo después de «.»".into());
                    };
                    self.pos += 1;
                    expr = Expr::Campo(Box::new(expr), c);
                }
                Some(Tok::LBracket) => {
                    self.pos += 1;
                    let idx = self.parse_or()?;
                    if self.peek() != Some(&Tok::RBracket) {
                        return Err("Falta «]» de cierre".into());
                    }
                    self.pos += 1;
                    expr = Expr::Indice(Box::new(expr), Box::new(idx));
                }
                _ => break,
            }
        }
        Ok(expr)
    }

    fn parse_primary(&mut self) -> Result<Expr, String> {
        let Some(tok) = self.toks.get(self.pos).cloned() else {
            return Err("Expresión incompleta".into());
        };
        self.pos += 1;
        match tok {
            Tok::Num(n) => Ok(Expr::Num(n)),
            Tok::Str(s) => Ok(Expr::Str(s)),
            Tok::Link(l) => Ok(Expr::Link(l)),
            Tok::LParen => {
                let e = self.parse_or()?;
                if self.peek() != Some(&Tok::RParen) {
                    return Err("Falta «)» de cierre".into());
                }
                self.pos += 1;
                Ok(e)
            }
            Tok::LBracket => {
                let mut items = Vec::new();
                if self.peek() != Some(&Tok::RBracket) {
                    loop {
                        items.push(self.parse_or()?);
                        if self.peek() == Some(&Tok::Comma) {
                            self.pos += 1;
                        } else {
                            break;
                        }
                    }
                }
                if self.peek() != Some(&Tok::RBracket) {
                    return Err("Falta «]» en la lista".into());
                }
                self.pos += 1;
                Ok(Expr::Lista(items))
            }
            Tok::Ident(s) => {
                if self.peek() == Some(&Tok::LParen) {
                    self.pos += 1;
                    let mut args = Vec::new();
                    if self.peek() != Some(&Tok::RParen) {
                        loop {
                            args.push(self.parse_or()?);
                            if self.peek() == Some(&Tok::Comma) {
                                self.pos += 1;
                            } else {
                                break;
                            }
                        }
                    }
                    if self.peek() != Some(&Tok::RParen) {
                        return Err(format!("Falta «)» al llamar a {s}()"));
                    }
                    self.pos += 1;
                    return Ok(Expr::Llamada(s.to_ascii_lowercase(), args));
                }
                if s.eq_ignore_ascii_case("true") {
                    Ok(Expr::Bool(true))
                } else if s.eq_ignore_ascii_case("false") {
                    Ok(Expr::Bool(false))
                } else if s.eq_ignore_ascii_case("null") {
                    Ok(Expr::Null)
                } else if s.starts_with('#') {
                    Ok(Expr::Str(s))
                } else {
                    Ok(Expr::Ident(s))
                }
            }
            other => Err(format!("Elemento inesperado en expresión: {other:?}")),
        }
    }
}

// --- Parser de consultas DQL ---

/// Divide una consulta DQL en sus secciones por palabras clave de cláusula al inicio de línea (o
/// en una sola línea), respetando comillas y paréntesis.
pub fn parse_consulta(source: &str) -> Result<ConsultaDv, String> {
    let bloques = dividir_clausulas(source);
    let Some((cab_kw, cab_resto)) = bloques.first() else {
        return Err("La consulta Dataview está vacía".into());
    };
    let vista = match cab_kw.as_str() {
        "TABLE" => parse_cabecera_tabla(cab_resto)?,
        "LIST" => {
            let r = cab_resto.trim();
            let expr = if r.is_empty() {
                None
            } else {
                Some(parse_expr(r)?)
            };
            TipoVista::Lista { expr }
        }
        "TASK" => TipoVista::Tareas,
        "CALENDAR" => {
            let r = cab_resto.trim();
            if r.is_empty() {
                return Err("CALENDAR requiere una expresión de fecha (ej. CALENDAR fecha)".into());
            }
            TipoVista::Calendario {
                expr: parse_expr(r)?,
            }
        }
        otra => {
            return Err(format!(
                "La consulta debe empezar con TABLE, LIST, TASK o CALENDAR (no «{otra}»)"
            ));
        }
    };

    let mut fuente = Fuente::Todo;
    let mut clausulas = Vec::new();
    for (kw, resto) in bloques.iter().skip(1) {
        let r = resto.trim();
        match kw.as_str() {
            "FROM" => fuente = parse_fuente(r)?,
            "WHERE" => clausulas.push(Clausula::Where(parse_expr(r)?)),
            "SORT" => clausulas.push(Clausula::Sort(parse_sort(r)?)),
            "GROUP BY" => {
                let (e_str, alias) = partir_as(r);
                clausulas.push(Clausula::GroupBy {
                    expr: parse_expr(e_str)?,
                    alias,
                });
            }
            "FLATTEN" => {
                let (e_str, alias) = partir_as(r);
                clausulas.push(Clausula::Flatten {
                    expr: parse_expr(e_str)?,
                    alias,
                });
            }
            "LIMIT" => {
                let n: usize = r
                    .parse()
                    .map_err(|_| format!("LIMIT espera un número entero positivo, no «{r}»"))?;
                clausulas.push(Clausula::Limit(n));
            }
            otra => return Err(format!("Cláusula desconocida: «{otra}»")),
        }
    }

    Ok(ConsultaDv {
        vista,
        fuente,
        clausulas,
    })
}

fn dividir_clausulas(source: &str) -> Vec<(String, String)> {
    const KWS: &[&str] = &[
        "TABLE", "LIST", "TASK", "CALENDAR", "FROM", "WHERE", "SORT", "GROUP BY", "FLATTEN",
        "LIMIT",
    ];
    let mut out: Vec<(String, String)> = Vec::new();
    for raw in source.lines() {
        let linea = raw.trim();
        if linea.is_empty() || linea.starts_with("//") {
            continue;
        }
        let upper = linea.to_ascii_uppercase();
        let matched = KWS.iter().find(|&&kw| {
            upper == kw
                || (upper.starts_with(kw) && upper[kw.len()..].starts_with(char::is_whitespace))
        });
        if let Some(&kw) = matched {
            let resto = linea[kw.len()..].trim().to_owned();
            out.push((kw.to_owned(), resto));
        } else if let Some((_, ultimo)) = out.last_mut() {
            if !ultimo.is_empty() {
                ultimo.push(' ');
            }
            ultimo.push_str(linea);
        } else {
            let first = linea.split_whitespace().next().unwrap_or(linea).to_owned();
            out.push((first, String::new()));
        }
    }
    out
}

fn parse_cabecera_tabla(resto: &str) -> Result<TipoVista, String> {
    let mut r = resto.trim();
    let mut sin_id = false;
    let upper = r.to_ascii_uppercase();
    if upper == "WITHOUT ID" {
        sin_id = true;
        r = "";
    } else if upper.starts_with("WITHOUT ID") && r[10..].starts_with(char::is_whitespace) {
        sin_id = true;
        r = r[10..].trim();
    }
    let mut columnas = Vec::new();
    if !r.is_empty() {
        for parte in dividir_por_coma(r) {
            let p = parte.trim();
            if p.is_empty() {
                continue;
            }
            let (e_str, alias_opt) = partir_as(p);
            let expr = parse_expr(e_str)?;
            let alias = alias_opt.unwrap_or_else(|| e_str.trim().to_owned());
            columnas.push(ColumnaTabla { expr, alias });
        }
    }
    Ok(TipoVista::Tabla { sin_id, columnas })
}

fn dividir_por_coma(s: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut inicio = 0;
    let mut par = 0usize;
    let mut cor = 0usize;
    let mut comilla: Option<u8> = None;
    let bytes = s.as_bytes();
    for (i, &b) in bytes.iter().enumerate() {
        if let Some(q) = comilla {
            if b == q && (i == 0 || bytes[i - 1] != b'\\') {
                comilla = None;
            }
            continue;
        }
        match b {
            b'"' | b'\'' => comilla = Some(b),
            b'(' => par += 1,
            b')' => par = par.saturating_sub(1),
            b'[' => cor += 1,
            b']' => cor = cor.saturating_sub(1),
            b',' if par == 0 && cor == 0 => {
                out.push(&s[inicio..i]);
                inicio = i + 1;
            }
            _ => {}
        }
    }
    out.push(&s[inicio..]);
    out
}

fn partir_as(s: &str) -> (&str, Option<String>) {
    // Busca ` AS ` en nivel superior (fuera de comillas/paréntesis).
    let bytes = s.as_bytes();
    let mut par = 0usize;
    let mut cor = 0usize;
    let mut comilla: Option<u8> = None;
    for i in 0..bytes.len() {
        let b = bytes[i];
        if let Some(q) = comilla {
            if b == q && (i == 0 || bytes[i - 1] != b'\\') {
                comilla = None;
            }
            continue;
        }
        match b {
            b'"' | b'\'' => comilla = Some(b),
            b'(' => par += 1,
            b')' => par = par.saturating_sub(1),
            b'[' => cor += 1,
            b']' => cor = cor.saturating_sub(1),
            _ => {}
        }
        if par == 0
            && cor == 0
            && i + 4 <= bytes.len()
            && bytes[i].is_ascii_whitespace()
            && bytes[i + 1..i + 3].eq_ignore_ascii_case(b"as")
            && bytes[i + 3].is_ascii_whitespace()
        {
            let expr = s[..i].trim();
            let raw_alias = s[i + 4..].trim();
            let alias = raw_alias
                .strip_prefix('"')
                .and_then(|x| x.strip_suffix('"'))
                .or_else(|| {
                    raw_alias
                        .strip_prefix('\'')
                        .and_then(|x| x.strip_suffix('\''))
                })
                .unwrap_or(raw_alias)
                .to_owned();
            return (expr, Some(alias));
        }
    }
    (s.trim(), None)
}

fn parse_sort(s: &str) -> Result<Vec<ClaveOrden>, String> {
    let mut out = Vec::new();
    for parte in dividir_por_coma(s) {
        let p = parte.trim();
        if p.is_empty() {
            continue;
        }
        let (e_str, desc) = if let Some((a, b)) = p.rsplit_once(char::is_whitespace) {
            if b.eq_ignore_ascii_case("DESC") || b.eq_ignore_ascii_case("DESCENDING") {
                (a.trim(), true)
            } else if b.eq_ignore_ascii_case("ASC") || b.eq_ignore_ascii_case("ASCENDING") {
                (a.trim(), false)
            } else {
                (p, false)
            }
        } else {
            (p, false)
        };
        out.push(ClaveOrden {
            expr: parse_expr(e_str)?,
            descendente: desc,
        });
    }
    if out.is_empty() {
        return Err("SORT necesita al menos un criterio".into());
    }
    Ok(out)
}

fn parse_fuente(s: &str) -> Result<Fuente, String> {
    let s = s.trim();
    if s.is_empty() || s == "\"\"" {
        return Ok(Fuente::Todo);
    }
    // Operador OR de menor precedencia.
    if let Some((a, b)) = dividir_operador_fuente(s, "OR") {
        return Ok(Fuente::Or(
            Box::new(parse_fuente(a)?),
            Box::new(parse_fuente(b)?),
        ));
    }
    if let Some((a, b)) = dividir_operador_fuente(s, "AND") {
        return Ok(Fuente::And(
            Box::new(parse_fuente(a)?),
            Box::new(parse_fuente(b)?),
        ));
    }
    if let Some(rest) = s.strip_prefix('-').or_else(|| s.strip_prefix('!')) {
        return Ok(Fuente::Negado(Box::new(parse_fuente(rest)?)));
    }
    if let Some(inner) = s.strip_prefix('(').and_then(|x| x.strip_suffix(')')) {
        return parse_fuente(inner);
    }
    if let Some(tag) = s.strip_prefix('#') {
        return Ok(Fuente::Etiqueta(fold(tag.trim())));
    }
    if let Some(dir) = s.strip_prefix('"').and_then(|x| x.strip_suffix('"')) {
        if dir.trim().is_empty() {
            return Ok(Fuente::Todo);
        }
        return Ok(Fuente::Carpeta(fold(dir.trim().trim_matches('/'))));
    }
    if let Some(inner) = s.strip_prefix("[[").and_then(|x| x.strip_suffix("]]")) {
        return Ok(Fuente::Entrantes(fold(inner.trim())));
    }
    let upper = s.to_ascii_uppercase();
    if upper.starts_with("OUTGOING(") && s.ends_with(')') {
        let inner = s[9..s.len() - 1]
            .trim()
            .trim_start_matches("[[")
            .trim_end_matches("]]");
        return Ok(Fuente::Salientes(fold(inner.trim())));
    }
    Err(format!("Origen FROM no reconocido: «{s}»"))
}

fn dividir_operador_fuente<'a>(s: &'a str, op: &str) -> Option<(&'a str, &'a str)> {
    let bytes = s.as_bytes();
    let mut par = 0usize;
    let mut en_comillas = false;
    let mut en_link = false;
    let op_len = op.len();
    for i in 0..bytes.len() {
        let b = bytes[i];
        if b == b'"' {
            en_comillas = !en_comillas;
            continue;
        }
        if en_comillas {
            continue;
        }
        if b == b'[' && bytes.get(i + 1) == Some(&b'[') {
            en_link = true;
            continue;
        }
        if b == b']' && bytes.get(i + 1) == Some(&b']') {
            en_link = false;
            continue;
        }
        if en_link {
            continue;
        }
        if b == b'(' {
            par += 1;
        } else if b == b')' {
            par = par.saturating_sub(1);
        } else if par == 0
            && i > 0
            && i + op_len < bytes.len()
            && bytes[i - 1].is_ascii_whitespace()
            && bytes[i + op_len].is_ascii_whitespace()
            && s[i..i + op_len].eq_ignore_ascii_case(op)
        {
            return Some((s[..i].trim(), s[i + op_len..].trim()));
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Evaluación y ejecución sobre SearchIndex
// ---------------------------------------------------------------------------

/// Un ítem de vista `LIST`: `(enlace_id, valor_opcional)`.
pub type ItemListaDv = (Valor, Option<Valor>);

/// Un grupo de vista `LIST`: `(clave_grupo_opcional, items)`.
pub type GrupoListaDv = (Option<Valor>, Vec<ItemListaDv>);

/// Resultado listo para pintar en Lectura, OnStage o exportar HTML.
#[derive(Clone, Debug, PartialEq)]
pub enum ResultadoDv {
    Tabla {
        sin_id: bool,
        encabezados: Vec<String>,
        filas: Vec<(Option<Valor>, Vec<Valor>)>,
    },
    Lista {
        grupos: Vec<GrupoListaDv>,
    },
    Tareas {
        grupos: Vec<(Option<Valor>, Vec<TareaResultado>)>,
    },
    Calendario {
        meses: Vec<MesCalendario>,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub struct TareaResultado {
    pub ruta: RelPath,
    pub texto: String,
    pub completed: bool,
    pub line: usize,
}

#[derive(Clone, Debug, PartialEq)]
pub struct MesCalendario {
    pub anio: i32,
    pub mes: u8,
    /// Días con notas: `dia (1..=31)` -> lista de enlaces a notas.
    pub dias: BTreeMap<u8, Vec<Valor>>,
}

struct EntornoNota<'a> {
    index: &'a SearchIndex,
    inlinks: &'a HashMap<String, Vec<RelPath>>,
    this_val: &'a Valor,
    vars: HashMap<String, Valor>,
}

fn construir_inlinks(index: &SearchIndex) -> HashMap<String, Vec<RelPath>> {
    let mut map: HashMap<String, Vec<RelPath>> = HashMap::new();
    for (path, doc) in &index.docs {
        if doc.zone == Zone::Raw {
            continue;
        }
        for out in &doc.outlinks {
            map.entry(out.clone()).or_default().push(path.clone());
        }
    }
    for v in map.values_mut() {
        v.sort();
        v.dedup();
    }
    map
}

fn valor_file(
    path: &RelPath,
    doc: &Doc,
    inlinks: &HashMap<String, Vec<RelPath>>,
    index: &SearchIndex,
) -> Valor {
    let stem = stem_de(path).to_owned();
    let folder = path
        .as_str()
        .rsplit_once('/')
        .map(|(d, _)| d.to_owned())
        .unwrap_or_default();
    let ext = path
        .file_name()
        .rsplit_once('.')
        .map(|(_, e)| e.to_owned())
        .unwrap_or_default();
    let ctime = doc
        .created
        .or(doc.modified)
        .map(Fecha::desde_system_time)
        .map(Valor::Fecha)
        .unwrap_or(Valor::Null);
    let mtime = doc
        .modified
        .map(Fecha::desde_system_time)
        .map(Valor::Fecha)
        .unwrap_or(Valor::Null);
    let tags = Valor::Lista(
        doc.raw_tags
            .iter()
            .map(|t| Valor::Texto(format!("#{t}")))
            .collect(),
    );
    let inl = inlinks
        .get(&doc.name)
        .map(|v| {
            v.iter()
                .map(|p| Valor::Enlace {
                    ruta: p.clone(),
                    etiqueta: stem_de(p).to_owned(),
                })
                .collect()
        })
        .unwrap_or_default();
    let outl: Vec<Valor> = doc
        .outlinks
        .iter()
        .map(|k| resolver_enlace_valor(k, index))
        .collect();
    let aliases = doc
        .values
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("aliases"))
        .map(|(_, v)| Valor::desde_prop(v, index))
        .unwrap_or_else(|| Valor::Lista(Vec::new()));
    let link = Valor::Enlace {
        ruta: path.clone(),
        etiqueta: stem.clone(),
    };
    Valor::Mapa(vec![
        ("name".into(), Valor::Texto(stem)),
        ("path".into(), Valor::Texto(path.as_str().to_owned())),
        ("folder".into(), Valor::Texto(folder)),
        ("ext".into(), Valor::Texto(ext)),
        ("ctime".into(), ctime),
        ("mtime".into(), mtime),
        ("tags".into(), tags.clone()),
        ("etags".into(), tags),
        ("inlinks".into(), Valor::Lista(inl)),
        ("outlinks".into(), Valor::Lista(outl.clone())),
        ("links".into(), Valor::Lista(outl)),
        ("aliases".into(), aliases),
        ("link".into(), link),
    ])
}

fn mapa_nota(
    path: &RelPath,
    doc: &Doc,
    inlinks: &HashMap<String, Vec<RelPath>>,
    index: &SearchIndex,
) -> Valor {
    let mut pares = Vec::with_capacity(doc.values.len() + 2);
    pares.push(("file".into(), valor_file(path, doc, inlinks, index)));
    for (k, v) in &doc.values {
        pares.push((k.clone(), Valor::desde_prop(v, index)));
    }
    Valor::Mapa(pares)
}

fn mapa_tarea(
    path: &RelPath,
    doc: &Doc,
    t: &TaskDoc,
    inlinks: &HashMap<String, Vec<RelPath>>,
    index: &SearchIndex,
) -> Valor {
    let mut pares = vec![
        ("file".into(), valor_file(path, doc, inlinks, index)),
        ("text".into(), Valor::Texto(t.text.clone())),
        ("completed".into(), Valor::Bool(t.completed)),
        ("checked".into(), Valor::Bool(t.completed)),
        ("status".into(), Valor::Texto(t.status.to_string())),
        ("line".into(), Valor::Numero(t.line as f64)),
        ("path".into(), Valor::Texto(path.as_str().to_owned())),
        (
            "link".into(),
            Valor::Enlace {
                ruta: path.clone(),
                etiqueta: stem_de(path).to_owned(),
            },
        ),
        (
            "tags".into(),
            Valor::Lista(
                t.tags
                    .iter()
                    .map(|tg| Valor::Texto(format!("#{tg}")))
                    .collect(),
            ),
        ),
    ];
    for (k, v) in &t.props {
        pares.push((k.clone(), Valor::desde_prop(v, index)));
    }
    Valor::Mapa(pares)
}

fn cumple_fuente(
    fuente: &Fuente,
    path: &RelPath,
    doc: &Doc,
    index: &SearchIndex,
    inlinks: &HashMap<String, Vec<RelPath>>,
) -> bool {
    match fuente {
        Fuente::Todo => true,
        Fuente::Etiqueta(tag) => doc
            .tags
            .iter()
            .any(|t| t == tag || t.starts_with(&format!("{tag}/"))),
        Fuente::Carpeta(dir) => {
            let p = fold(path.as_str());
            let sin_wiki = p.strip_prefix("wiki/").unwrap_or(&p);
            p.starts_with(&format!("{dir}/")) || sin_wiki.starts_with(&format!("{dir}/"))
        }
        Fuente::Entrantes(target) => doc.outlinks.iter().any(|o| o == target),
        Fuente::Salientes(origen) => {
            let _ = inlinks;
            index
                .docs
                .iter()
                .find(|(_, d)| d.name == *origen)
                .is_some_and(|(_, d)| d.outlinks.iter().any(|o| o == &doc.name))
        }
        Fuente::Negado(inner) => !cumple_fuente(inner, path, doc, index, inlinks),
        Fuente::And(a, b) => {
            cumple_fuente(a, path, doc, index, inlinks)
                && cumple_fuente(b, path, doc, index, inlinks)
        }
        Fuente::Or(a, b) => {
            cumple_fuente(a, path, doc, index, inlinks)
                || cumple_fuente(b, path, doc, index, inlinks)
        }
    }
}

fn es_nota(path: &RelPath) -> bool {
    crate::Format::from_name(path.file_name()).is_some()
}

/// Evalúa una expresión inline `` `= <expr>` `` en el contexto de `nota_actual`.
pub fn evaluar_inline(
    index: &SearchIndex,
    nota_actual: Option<&RelPath>,
    expr_src: &str,
) -> Result<Valor, String> {
    let expr = parse_expr(expr_src)?;
    let inlinks = construir_inlinks(index);
    let this_val = nota_actual
        .and_then(|p| index.docs.get(p).map(|d| mapa_nota(p, d, &inlinks, index)))
        .unwrap_or(Valor::Null);
    let env = EntornoNota {
        index,
        inlinks: &inlinks,
        this_val: &this_val,
        vars: HashMap::new(),
    };
    eval(&expr, &this_val, &env)
}

/// Ejecuta una consulta DQL completa sobre `index`.
pub fn ejecutar(
    index: &SearchIndex,
    nota_actual: Option<&RelPath>,
    consulta: &ConsultaDv,
) -> Result<ResultadoDv, String> {
    let inlinks = construir_inlinks(index);
    let this_val = nota_actual
        .and_then(|p| index.docs.get(p).map(|d| mapa_nota(p, d, &inlinks, index)))
        .unwrap_or(Valor::Null);

    // Filas de trabajo: cada elemento es (ctx_valor, tarea_opcional, ruta_origen).
    #[derive(Clone)]
    struct FilaTrabajo {
        ctx: Valor,
        enlace_id: Valor,
        tarea: Option<TareaResultado>,
    }

    let mut filas: Vec<FilaTrabajo> = Vec::new();
    let es_task = matches!(consulta.vista, TipoVista::Tareas);

    for (path, doc) in &index.docs {
        if doc.zone == Zone::Raw || !es_nota(path) {
            continue;
        }
        if !cumple_fuente(&consulta.fuente, path, doc, index, &inlinks) {
            continue;
        }
        let enlace_id = Valor::Enlace {
            ruta: path.clone(),
            etiqueta: stem_de(path).to_owned(),
        };
        if es_task {
            for t in &doc.tasks {
                filas.push(FilaTrabajo {
                    ctx: mapa_tarea(path, doc, t, &inlinks, index),
                    enlace_id: enlace_id.clone(),
                    tarea: Some(TareaResultado {
                        ruta: path.clone(),
                        texto: t.text.clone(),
                        completed: t.completed,
                        line: t.line,
                    }),
                });
            }
        } else {
            filas.push(FilaTrabajo {
                ctx: mapa_nota(path, doc, &inlinks, index),
                enlace_id,
                tarea: None,
            });
        }
    }
    // Orden determinista inicial por ruta.
    filas.sort_by(|a, b| comparar_valores(&a.enlace_id, &b.enlace_id));

    let env = EntornoNota {
        index,
        inlinks: &inlinks,
        this_val: &this_val,
        vars: HashMap::new(),
    };

    // Aplicamos las cláusulas en orden (WHERE, FLATTEN, GROUP BY, SORT, LIMIT).
    let mut agrupado: Option<Vec<(Valor, Vec<FilaTrabajo>)>> = None;

    for cl in &consulta.clausulas {
        match cl {
            Clausula::Where(expr) => {
                if let Some(grupos) = &mut agrupado {
                    let mut quedan = Vec::new();
                    for (k, grp) in grupos.drain(..) {
                        let gctx =
                            ctx_grupo(&k, &grp.iter().map(|f| f.ctx.clone()).collect::<Vec<_>>());
                        if eval(expr, &gctx, &env)?.es_verdadero() {
                            quedan.push((k, grp));
                        }
                    }
                    *grupos = quedan;
                } else {
                    let mut quedan = Vec::new();
                    for f in filas {
                        if eval(expr, &f.ctx, &env)?.es_verdadero() {
                            quedan.push(f);
                        }
                    }
                    filas = quedan;
                }
            }
            Clausula::Flatten { expr, alias } => {
                let campo_nombre = alias.clone().unwrap_or_else(|| "valor".into());
                let mut nuevas = Vec::new();
                for f in filas {
                    let v = eval(expr, &f.ctx, &env)?;
                    match v {
                        Valor::Lista(items) => {
                            for it in items {
                                let mut ctx = f.ctx.clone();
                                if let Valor::Mapa(ref mut m) = ctx {
                                    m.push((campo_nombre.clone(), it));
                                }
                                nuevas.push(FilaTrabajo {
                                    ctx,
                                    enlace_id: f.enlace_id.clone(),
                                    tarea: f.tarea.clone(),
                                });
                            }
                        }
                        otro => {
                            let mut ctx = f.ctx.clone();
                            if let Valor::Mapa(ref mut m) = ctx {
                                m.push((campo_nombre.clone(), otro));
                            }
                            nuevas.push(FilaTrabajo {
                                ctx,
                                enlace_id: f.enlace_id.clone(),
                                tarea: f.tarea.clone(),
                            });
                        }
                    }
                }
                filas = nuevas;
            }
            Clausula::GroupBy { expr, alias } => {
                let mut grupos: Vec<(Valor, Vec<FilaTrabajo>)> = Vec::new();
                for f in filas.drain(..) {
                    let clave = eval(expr, &f.ctx, &env)?;
                    if let Some((_, list)) = grupos.iter_mut().find(|(k, _)| iguales(k, &clave)) {
                        list.push(f);
                    } else {
                        grupos.push((clave, vec![f]));
                    }
                }
                grupos.sort_by(|a, b| comparar_valores(&a.0, &b.0));
                if let Some(nombre) = alias {
                    for (k, grp) in &mut grupos {
                        for f in grp.iter_mut() {
                            if let Valor::Mapa(ref mut m) = f.ctx {
                                m.push((nombre.clone(), k.clone()));
                            }
                        }
                    }
                }
                agrupado = Some(grupos);
            }
            Clausula::Sort(claves) => {
                if let Some(grupos) = &mut agrupado {
                    type GrupoTrabajo = (Valor, Vec<FilaTrabajo>);
                    let mut evaluados: Vec<(Vec<Valor>, GrupoTrabajo)> = Vec::new();
                    for (k, grp) in grupos.drain(..) {
                        let gctx =
                            ctx_grupo(&k, &grp.iter().map(|f| f.ctx.clone()).collect::<Vec<_>>());
                        let mut ks = Vec::with_capacity(claves.len());
                        for c in claves {
                            ks.push(eval(&c.expr, &gctx, &env)?);
                        }
                        evaluados.push((ks, (k, grp)));
                    }
                    evaluados.sort_by(|a, b| comparar_claves(&a.0, &b.0, claves));
                    *grupos = evaluados.into_iter().map(|(_, g)| g).collect();
                } else {
                    let mut evaluados: Vec<(Vec<Valor>, FilaTrabajo)> = Vec::new();
                    for f in filas {
                        let mut ks = Vec::with_capacity(claves.len());
                        for c in claves {
                            ks.push(eval(&c.expr, &f.ctx, &env)?);
                        }
                        evaluados.push((ks, f));
                    }
                    evaluados.sort_by(|a, b| comparar_claves(&a.0, &b.0, claves));
                    filas = evaluados.into_iter().map(|(_, f)| f).collect();
                }
            }
            Clausula::Limit(n) => {
                if let Some(grupos) = &mut agrupado {
                    grupos.truncate(*n);
                } else {
                    filas.truncate(*n);
                }
            }
        }
    }

    match &consulta.vista {
        TipoVista::Tabla { sin_id, columnas } => {
            let mut encabezados = Vec::new();
            if !sin_id {
                encabezados.push(if agrupado.is_some() {
                    "Grupo".into()
                } else {
                    "Nota".into()
                });
            }
            for c in columnas {
                encabezados.push(c.alias.clone());
            }
            let mut res_filas = Vec::new();
            if let Some(grupos) = agrupado {
                for (k, grp) in grupos {
                    let gctx =
                        ctx_grupo(&k, &grp.iter().map(|f| f.ctx.clone()).collect::<Vec<_>>());
                    let mut vals = Vec::with_capacity(columnas.len());
                    for c in columnas {
                        vals.push(eval(&c.expr, &gctx, &env)?);
                    }
                    res_filas.push(((!sin_id).then_some(k), vals));
                }
            } else {
                for f in filas {
                    let mut vals = Vec::with_capacity(columnas.len());
                    for c in columnas {
                        vals.push(eval(&c.expr, &f.ctx, &env)?);
                    }
                    res_filas.push(((!sin_id).then_some(f.enlace_id), vals));
                }
            }
            Ok(ResultadoDv::Tabla {
                sin_id: *sin_id,
                encabezados,
                filas: res_filas,
            })
        }
        TipoVista::Lista { expr } => {
            let mut grupos_out = Vec::new();
            if let Some(grupos) = agrupado {
                for (k, grp) in grupos {
                    let mut items = Vec::new();
                    for f in grp {
                        let extra = match expr {
                            Some(e) => Some(eval(e, &f.ctx, &env)?),
                            None => None,
                        };
                        items.push((f.enlace_id, extra));
                    }
                    grupos_out.push((Some(k), items));
                }
            } else {
                let mut items = Vec::new();
                for f in filas {
                    let extra = match expr {
                        Some(e) => Some(eval(e, &f.ctx, &env)?),
                        None => None,
                    };
                    items.push((f.enlace_id, extra));
                }
                grupos_out.push((None, items));
            }
            Ok(ResultadoDv::Lista { grupos: grupos_out })
        }
        TipoVista::Tareas => {
            let mut grupos_out = Vec::new();
            if let Some(grupos) = agrupado {
                for (k, grp) in grupos {
                    let ts: Vec<TareaResultado> = grp.into_iter().filter_map(|f| f.tarea).collect();
                    if !ts.is_empty() {
                        grupos_out.push((Some(k), ts));
                    }
                }
            } else {
                let ts: Vec<TareaResultado> = filas.into_iter().filter_map(|f| f.tarea).collect();
                if !ts.is_empty() {
                    grupos_out.push((None, ts));
                }
            }
            Ok(ResultadoDv::Tareas { grupos: grupos_out })
        }
        TipoVista::Calendario { expr } => {
            let mut por_mes: BTreeMap<(i32, u8), BTreeMap<u8, Vec<Valor>>> = BTreeMap::new();
            for f in filas {
                let v = eval(expr, &f.ctx, &env)?;
                if let Some(fecha) = v.a_fecha() {
                    por_mes
                        .entry((fecha.anio, fecha.mes))
                        .or_default()
                        .entry(fecha.dia)
                        .or_default()
                        .push(f.enlace_id);
                }
            }
            let meses = por_mes
                .into_iter()
                .map(|((anio, mes), dias)| MesCalendario { anio, mes, dias })
                .collect();
            Ok(ResultadoDv::Calendario { meses })
        }
    }
}

fn ctx_grupo(key: &Valor, rows: &[Valor]) -> Valor {
    Valor::Mapa(vec![
        ("key".into(), key.clone()),
        ("rows".into(), Valor::Lista(rows.to_vec())),
    ])
}

fn comparar_claves(a: &[Valor], b: &[Valor], claves: &[ClaveOrden]) -> Ordering {
    for ((va, vb), cl) in a.iter().zip(b.iter()).zip(claves.iter()) {
        let mut ord = comparar_valores(va, vb);
        if cl.descendente {
            ord = ord.reverse();
        }
        if ord != Ordering::Equal {
            return ord;
        }
    }
    Ordering::Equal
}

fn eval(expr: &Expr, ctx: &Valor, env: &EntornoNota<'_>) -> Result<Valor, String> {
    match expr {
        Expr::Null => Ok(Valor::Null),
        Expr::Bool(b) => Ok(Valor::Bool(*b)),
        Expr::Num(n) => Ok(Valor::Numero(*n)),
        Expr::Str(s) => Ok(Valor::Texto(s.clone())),
        Expr::Link(l) => Ok(resolver_enlace_valor(l, env.index)),
        Expr::Ident(nombre) => {
            if nombre.eq_ignore_ascii_case("this") {
                return Ok(env.this_val.clone());
            }
            if let Some(v) = env.vars.get(&fold(nombre)) {
                return Ok(v.clone());
            }
            Ok(ctx.campo(nombre))
        }
        Expr::Lista(items) => {
            let mut out = Vec::with_capacity(items.len());
            for it in items {
                out.push(eval(it, ctx, env)?);
            }
            Ok(Valor::Lista(out))
        }
        Expr::Campo(base, campo) => {
            let b = eval(base, ctx, env)?;
            Ok(b.campo(campo))
        }
        Expr::Indice(base, idx) => {
            let b = eval(base, ctx, env)?;
            let i = eval(idx, ctx, env)?;
            match (b, i) {
                (Valor::Lista(l), Valor::Numero(n)) => {
                    let k = n as isize;
                    let pos = if k < 0 {
                        l.len().checked_sub(k.unsigned_abs())
                    } else {
                        Some(k as usize)
                    };
                    Ok(pos.and_then(|p| l.get(p).cloned()).unwrap_or(Valor::Null))
                }
                (b @ (Valor::Mapa(_) | Valor::Lista(_)), Valor::Texto(k)) => Ok(b.campo(&k)),
                _ => Ok(Valor::Null),
            }
        }
        Expr::Neg(inner) => {
            let v = eval(inner, ctx, env)?;
            match v {
                Valor::Numero(n) => Ok(Valor::Numero(-n)),
                Valor::Duracion(d) => Ok(Valor::Duracion(-d)),
                _ => Ok(Valor::Null),
            }
        }
        Expr::Not(inner) => Ok(Valor::Bool(!eval(inner, ctx, env)?.es_verdadero())),
        Expr::Bin(a, op, b) => {
            // Cortocircuito lógico.
            if *op == OpBin::And {
                let va = eval(a, ctx, env)?;
                if !va.es_verdadero() {
                    return Ok(Valor::Bool(false));
                }
                return Ok(Valor::Bool(eval(b, ctx, env)?.es_verdadero()));
            }
            if *op == OpBin::Or {
                let va = eval(a, ctx, env)?;
                if va.es_verdadero() {
                    return Ok(Valor::Bool(true));
                }
                return Ok(Valor::Bool(eval(b, ctx, env)?.es_verdadero()));
            }
            let va = eval(a, ctx, env)?;
            let vb = eval(b, ctx, env)?;
            eval_bin(va, *op, vb)
        }
        Expr::Lambda(_, _) => Ok(Valor::Null),
        Expr::Llamada(nombre, args) => eval_llamada(nombre, args, ctx, env),
    }
}

fn eval_bin(a: Valor, op: OpBin, b: Valor) -> Result<Valor, String> {
    match op {
        OpBin::Eq => Ok(Valor::Bool(iguales(&a, &b))),
        OpBin::Ne => Ok(Valor::Bool(!iguales(&a, &b))),
        OpBin::Lt => Ok(Valor::Bool(comparar_valores(&a, &b) == Ordering::Less)),
        OpBin::Le => Ok(Valor::Bool(comparar_valores(&a, &b) != Ordering::Greater)),
        OpBin::Gt => Ok(Valor::Bool(comparar_valores(&a, &b) == Ordering::Greater)),
        OpBin::Ge => Ok(Valor::Bool(comparar_valores(&a, &b) != Ordering::Less)),
        OpBin::Add => match (a, b) {
            (Valor::Numero(x), Valor::Numero(y)) => Ok(Valor::Numero(x + y)),
            (Valor::Fecha(f), Valor::Duracion(d)) | (Valor::Duracion(d), Valor::Fecha(f)) => {
                Ok(Valor::Fecha(f.sumar_dias(d)))
            }
            (Valor::Duracion(x), Valor::Duracion(y)) => Ok(Valor::Duracion(x + y)),
            (Valor::Texto(x), y) => Ok(Valor::Texto(format!("{x}{}", y.a_texto()))),
            (x, Valor::Texto(y)) => Ok(Valor::Texto(format!("{}{y}", x.a_texto()))),
            _ => Ok(Valor::Null),
        },
        OpBin::Sub => match (a, b) {
            (Valor::Numero(x), Valor::Numero(y)) => Ok(Valor::Numero(x - y)),
            (Valor::Fecha(f), Valor::Duracion(d)) => Ok(Valor::Fecha(f.sumar_dias(-d))),
            (Valor::Fecha(f1), Valor::Fecha(f2)) => {
                Ok(Valor::Duracion(f1.a_dias_epoch() - f2.a_dias_epoch()))
            }
            (Valor::Duracion(x), Valor::Duracion(y)) => Ok(Valor::Duracion(x - y)),
            _ => Ok(Valor::Null),
        },
        OpBin::Mul => match (a.a_numero(), b.a_numero()) {
            (Some(x), Some(y)) => Ok(Valor::Numero(x * y)),
            _ => Ok(Valor::Null),
        },
        OpBin::Div => match (a.a_numero(), b.a_numero()) {
            (Some(x), Some(y)) if y.abs() > 1e-12 => Ok(Valor::Numero(x / y)),
            _ => Ok(Valor::Null),
        },
        OpBin::Mod => match (a.a_numero(), b.a_numero()) {
            (Some(x), Some(y)) if y.abs() > 1e-12 => Ok(Valor::Numero(x % y)),
            _ => Ok(Valor::Null),
        },
        OpBin::And | OpBin::Or => unreachable!(),
    }
}

fn eval_llamada(
    nombre: &str,
    args: &[Expr],
    ctx: &Valor,
    env: &EntornoNota<'_>,
) -> Result<Valor, String> {
    // Funciones de orden superior con lambda: filter, map, any, all, none.
    if matches!(nombre, "filter" | "map" | "any" | "all" | "none")
        && args.len() == 2
        && let Expr::Lambda(var, body) = &args[1]
    {
        let lista = match eval(&args[0], ctx, env)? {
            Valor::Lista(l) => l,
            Valor::Null => Vec::new(),
            otro => vec![otro],
        };
        let var_k = fold(var);
        let mut sub_env = EntornoNota {
            index: env.index,
            inlinks: env.inlinks,
            this_val: env.this_val,
            vars: env.vars.clone(),
        };
        let mut res = Vec::new();
        for item in lista {
            sub_env.vars.insert(var_k.clone(), item.clone());
            let r = eval(body, ctx, &sub_env)?;
            match nombre {
                "filter" if r.es_verdadero() => res.push(item),
                "map" => res.push(r),
                "any" if r.es_verdadero() => return Ok(Valor::Bool(true)),
                "all" if !r.es_verdadero() => return Ok(Valor::Bool(false)),
                "none" if r.es_verdadero() => return Ok(Valor::Bool(false)),
                _ => {}
            }
        }
        return Ok(match nombre {
            "any" => Valor::Bool(false),
            "all" | "none" => Valor::Bool(true),
            _ => Valor::Lista(res),
        });
    }

    // Evaluar argumentos estándar.
    let mut vals = Vec::with_capacity(args.len());
    for a in args {
        vals.push(eval(a, ctx, env)?);
    }
    let aplanar_nums = |vs: &[Valor]| -> Vec<f64> {
        let mut out = Vec::new();
        for v in vs {
            match v {
                Valor::Lista(l) => {
                    for x in l {
                        if let Some(n) = x.a_numero() {
                            out.push(n);
                        }
                    }
                }
                other => {
                    if let Some(n) = other.a_numero() {
                        out.push(n);
                    }
                }
            }
        }
        out
    };

    Ok(match nombre {
        "sum" | "suma" => Valor::Numero(aplanar_nums(&vals).into_iter().sum()),
        "avg" | "average" | "mean" | "promedio" => {
            let ns = aplanar_nums(&vals);
            if ns.is_empty() {
                Valor::Null
            } else {
                Valor::Numero(ns.iter().sum::<f64>() / ns.len() as f64)
            }
        }
        "min" => {
            let ns = aplanar_nums(&vals);
            ns.into_iter()
                .reduce(f64::min)
                .map(Valor::Numero)
                .unwrap_or(Valor::Null)
        }
        "max" => {
            let ns = aplanar_nums(&vals);
            ns.into_iter()
                .reduce(f64::max)
                .map(Valor::Numero)
                .unwrap_or(Valor::Null)
        }
        "round" | "redondear" => {
            let Some(n) = vals.first().and_then(Valor::a_numero) else {
                return Ok(Valor::Null);
            };
            let dec = vals
                .get(1)
                .and_then(Valor::a_numero)
                .map(|d| d.clamp(0.0, 6.0) as i32)
                .unwrap_or(0);
            let f = 10_f64.powi(dec);
            Valor::Numero((n * f).round() / f)
        }
        "floor" => vals
            .first()
            .and_then(Valor::a_numero)
            .map(|n| Valor::Numero(n.floor()))
            .unwrap_or(Valor::Null),
        "ceil" => vals
            .first()
            .and_then(Valor::a_numero)
            .map(|n| Valor::Numero(n.ceil()))
            .unwrap_or(Valor::Null),
        "abs" => vals
            .first()
            .and_then(Valor::a_numero)
            .map(|n| Valor::Numero(n.abs()))
            .unwrap_or(Valor::Null),
        "length" | "len" | "count" => match vals.first() {
            Some(Valor::Lista(l)) => Valor::Numero(l.len() as f64),
            Some(Valor::Texto(s)) => Valor::Numero(s.chars().count() as f64),
            Some(Valor::Mapa(m)) => Valor::Numero(m.len() as f64),
            _ => Valor::Numero(0.0),
        },
        "lower" => Valor::Texto(
            vals.first()
                .map(Valor::a_texto)
                .unwrap_or_default()
                .to_lowercase(),
        ),
        "upper" => Valor::Texto(
            vals.first()
                .map(Valor::a_texto)
                .unwrap_or_default()
                .to_uppercase(),
        ),
        "string" => Valor::Texto(vals.first().map(Valor::a_texto).unwrap_or_default()),
        "number" => vals
            .first()
            .and_then(Valor::a_numero)
            .map(Valor::Numero)
            .unwrap_or(Valor::Null),
        "replace" => {
            let s = vals.first().map(Valor::a_texto).unwrap_or_default();
            let a = vals.get(1).map(Valor::a_texto).unwrap_or_default();
            let b = vals.get(2).map(Valor::a_texto).unwrap_or_default();
            Valor::Texto(s.replace(&a, &b))
        }
        "split" => {
            let s = vals.first().map(Valor::a_texto).unwrap_or_default();
            let sep = vals
                .get(1)
                .map(Valor::a_texto)
                .unwrap_or_else(|| ",".into());
            Valor::Lista(
                s.split(&sep)
                    .map(|p| Valor::Texto(p.trim().to_owned()))
                    .collect(),
            )
        }
        "join" => {
            let sep = vals
                .get(1)
                .map(Valor::a_texto)
                .unwrap_or_else(|| ", ".into());
            match vals.first() {
                Some(Valor::Lista(l)) => {
                    Valor::Texto(l.iter().map(Valor::a_texto).collect::<Vec<_>>().join(&sep))
                }
                Some(v) => Valor::Texto(v.a_texto()),
                None => Valor::Texto(String::new()),
            }
        }
        "substring" => {
            let s = vals.first().map(Valor::a_texto).unwrap_or_default();
            let start = vals
                .get(1)
                .and_then(Valor::a_numero)
                .map(|n| n.max(0.0) as usize)
                .unwrap_or(0);
            let chars: Vec<char> = s.chars().collect();
            let end = vals
                .get(2)
                .and_then(Valor::a_numero)
                .map(|n| (n.max(0.0) as usize).min(chars.len()))
                .unwrap_or(chars.len());
            let start = start.min(end);
            Valor::Texto(chars[start..end].iter().collect())
        }
        "contains" | "icontains" => {
            let Some(hay) = vals.first() else {
                return Ok(Valor::Bool(false));
            };
            let Some(needle) = vals.get(1) else {
                return Ok(Valor::Bool(false));
            };
            match hay {
                Valor::Lista(l) => Valor::Bool(l.iter().any(|x| iguales(x, needle))),
                Valor::Texto(s) => Valor::Bool(fold(s).contains(&fold(&needle.a_texto()))),
                _ => Valor::Bool(false),
            }
        }
        "startswith" => {
            let s = vals.first().map(Valor::a_texto).unwrap_or_default();
            let p = vals.get(1).map(Valor::a_texto).unwrap_or_default();
            Valor::Bool(fold(&s).starts_with(&fold(&p)))
        }
        "endswith" => {
            let s = vals.first().map(Valor::a_texto).unwrap_or_default();
            let p = vals.get(1).map(Valor::a_texto).unwrap_or_default();
            Valor::Bool(fold(&s).ends_with(&fold(&p)))
        }
        "regexmatch" => {
            let pat = vals.first().map(Valor::a_texto).unwrap_or_default();
            let txt = vals.get(1).map(Valor::a_texto).unwrap_or_default();
            let ok = regex_lite::Regex::new(&pat)
                .map(|re| re.is_match(&txt))
                .unwrap_or(false);
            Valor::Bool(ok)
        }
        "regexreplace" => {
            let txt = vals.first().map(Valor::a_texto).unwrap_or_default();
            let pat = vals.get(1).map(Valor::a_texto).unwrap_or_default();
            let rep = vals.get(2).map(Valor::a_texto).unwrap_or_default();
            let res = regex_lite::Regex::new(&pat)
                .map(|re| re.replace_all(&txt, rep.as_str()).into_owned())
                .unwrap_or(txt);
            Valor::Texto(res)
        }
        "date" => {
            let raw = vals.first().map(Valor::a_texto).unwrap_or_default();
            let k = raw.trim().to_ascii_lowercase();
            match k.as_str() {
                "today" | "now" | "hoy" | "ahora" | "" => Valor::Fecha(Fecha::hoy()),
                "tomorrow" | "manana" | "mañana" => Valor::Fecha(Fecha::hoy().sumar_dias(1)),
                "yesterday" | "ayer" => Valor::Fecha(Fecha::hoy().sumar_dias(-1)),
                _ => Fecha::parse(&raw).map(Valor::Fecha).unwrap_or(Valor::Null),
            }
        }
        "dur" => {
            let raw = vals.first().map(Valor::a_texto).unwrap_or_default();
            parse_duracion(&raw)
                .map(Valor::Duracion)
                .unwrap_or(Valor::Null)
        }
        "dateformat" => {
            let f = vals.first().and_then(Valor::a_fecha);
            let fmt = vals
                .get(1)
                .map(Valor::a_texto)
                .unwrap_or_else(|| "YYYY-MM-DD".into());
            match f {
                Some(fecha) => Valor::Texto(fecha.formato(&fmt)),
                None => Valor::Null,
            }
        }
        "choice" | "if" => {
            let cond = vals.first().is_some_and(Valor::es_verdadero);
            if cond {
                vals.get(1).cloned().unwrap_or(Valor::Null)
            } else {
                vals.get(2).cloned().unwrap_or(Valor::Null)
            }
        }
        "default" => match vals.first() {
            Some(Valor::Null) | None => vals.get(1).cloned().unwrap_or(Valor::Null),
            Some(v) => v.clone(),
        },
        "nonnull" => Valor::Lista(
            vals.into_iter()
                .flat_map(|v| match v {
                    Valor::Lista(l) => l,
                    other => vec![other],
                })
                .filter(|v| !matches!(v, Valor::Null))
                .collect(),
        ),
        "flat" => Valor::Lista(
            vals.into_iter()
                .flat_map(|v| match v {
                    Valor::Lista(l) => l,
                    other => vec![other],
                })
                .collect(),
        ),
        "sort" => {
            let mut l = match vals.into_iter().next() {
                Some(Valor::Lista(l)) => l,
                _ => Vec::new(),
            };
            l.sort_by(comparar_valores);
            Valor::Lista(l)
        }
        "reverse" => {
            let mut l = match vals.into_iter().next() {
                Some(Valor::Lista(l)) => l,
                _ => Vec::new(),
            };
            l.reverse();
            Valor::Lista(l)
        }
        "unique" => {
            let l = match vals.into_iter().next() {
                Some(Valor::Lista(l)) => l,
                _ => Vec::new(),
            };
            let mut seen = HashSet::new();
            let mut out = Vec::new();
            for x in l {
                if seen.insert(fold(&x.a_texto())) {
                    out.push(x);
                }
            }
            Valor::Lista(out)
        }
        "list" => Valor::Lista(vals),
        "link" => {
            let target = vals.first().map(Valor::a_texto).unwrap_or_default();
            let display = vals.get(1).map(Valor::a_texto);
            let mut v = resolver_enlace_valor(&target, env.index);
            if let (Valor::Enlace { etiqueta, .. }, Some(d)) = (&mut v, display) {
                *etiqueta = d;
            }
            v
        }
        "typeof" => Valor::Texto(
            match vals.first().unwrap_or(&Valor::Null) {
                Valor::Null => "null",
                Valor::Bool(_) => "boolean",
                Valor::Numero(_) => "number",
                Valor::Texto(_) => "string",
                Valor::Fecha(_) => "date",
                Valor::Duracion(_) => "duration",
                Valor::Lista(_) => "array",
                Valor::Enlace { .. } => "link",
                Valor::Mapa(_) => "object",
            }
            .to_owned(),
        ),
        otra => return Err(format!("Función Dataview desconocida: «{otra}»")),
    })
}

fn parse_duracion(raw: &str) -> Option<i64> {
    let mut total = 0i64;
    let mut tokens = raw.split_whitespace().peekable();
    while let Some(tok) = tokens.next() {
        let num_len = tok
            .bytes()
            .take_while(|b| b.is_ascii_digit() || *b == b'-')
            .count();
        if num_len == 0 {
            continue;
        }
        let n: i64 = tok[..num_len].parse().ok()?;
        let unidad = if num_len < tok.len() {
            tok[num_len..].to_ascii_lowercase()
        } else {
            tokens
                .next()
                .unwrap_or("d")
                .trim_end_matches(',')
                .to_ascii_lowercase()
        };
        let factor = match unidad.as_str() {
            "d" | "day" | "days" | "dia" | "dias" | "día" | "días" => 1,
            "w" | "week" | "weeks" | "semana" | "semanas" => 7,
            "m" | "mo" | "month" | "months" | "mes" | "meses" => 30,
            "y" | "yr" | "year" | "years" | "anio" | "anios" | "año" | "años" => 365,
            _ => return None,
        };
        total += n * factor;
    }
    Some(total)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn crear_boveda_prueba() -> (std::path::PathBuf, SearchIndex) {
        let dir = std::env::temp_dir().join(format!(
            "orbital-065-dv-{}",
            std::time::SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(dir.join("wiki/proyectos")).unwrap();
        fs::write(
            dir.join("wiki/proyectos/alfa.md"),
            "---\ntags: [proyecto]\nestado: activo\npuntaje: 9\nfecha: 2026-11-10\n---\nEnlaza a [[beta]] con [autor:: Ada] (prioridad:: alta).\n- [ ] Escribir pruebas [due:: 2026-11-28]\n- [x] Diseñar motor\n",
        )
        .unwrap();
        fs::write(
            dir.join("wiki/proyectos/beta.md"),
            "---\ntags: [proyecto]\nestado: pausado\npuntaje: 6\nfecha: 2026-11-15\n---\n- [ ] Revisar docs\n",
        )
        .unwrap();
        let vault = crate::open(&dir).unwrap();
        let idx = SearchIndex::build(&vault);
        (dir, idx)
    }

    #[test]
    fn table_where_sort_group_y_funciones_sin_js() {
        let (dir, idx) = crear_boveda_prueba();
        let q = parse_consulta(
            "TABLE estado AS \"Estado\", round(puntaje * 10) AS \"Nota\", autor\nFROM #proyecto\nWHERE puntaje >= 7 AND estado != \"pausado\"\nSORT puntaje DESC\nLIMIT 5",
        )
        .expect("parsea TABLE");
        let ResultadoDv::Tabla {
            encabezados, filas, ..
        } = ejecutar(&idx, None, &q).expect("ejecuta TABLE")
        else {
            panic!("esperaba tabla");
        };
        assert_eq!(encabezados, vec!["Nota", "Estado", "Nota", "autor"]);
        assert_eq!(filas.len(), 1);
        assert_eq!(filas[0].1[0].a_texto(), "activo");
        assert_eq!(filas[0].1[1].a_texto(), "90");
        assert_eq!(filas[0].1[2].a_texto(), "Ada");

        // GROUP BY + sum(rows.puntaje)
        let qg =
            parse_consulta("TABLE sum(rows.puntaje) AS \"Suma\"\nFROM #proyecto\nGROUP BY estado")
                .expect("parsea GROUP BY");
        let ResultadoDv::Tabla { filas: gf, .. } = ejecutar(&idx, None, &qg).unwrap() else {
            panic!();
        };
        assert_eq!(gf.len(), 2);

        // TASK
        let qt = parse_consulta("TASK\nFROM #proyecto\nWHERE !completed").unwrap();
        let ResultadoDv::Tareas { grupos } = ejecutar(&idx, None, &qt).unwrap() else {
            panic!();
        };
        assert_eq!(grupos[0].1.len(), 2);

        // CALENDAR
        let qc = parse_consulta("CALENDAR fecha\nFROM \"proyectos\"").unwrap();
        let ResultadoDv::Calendario { meses } = ejecutar(&idx, None, &qc).unwrap() else {
            panic!();
        };
        assert_eq!(meses.len(), 1);
        assert_eq!(meses[0].dias.len(), 2);

        // Inline `= ...` con `this.file.inlinks` y lambda `filter`/`map`
        let beta = RelPath::new("wiki/proyectos/beta.md").unwrap();
        let inl = evaluar_inline(&idx, Some(&beta), "length(this.file.inlinks)").unwrap();
        assert_eq!(inl, Valor::Numero(1.0));
        let lam = evaluar_inline(
            &idx,
            None,
            "sum(map(filter([1, 2, 3, 4], (x) => x > 2), (x) => x * 10))",
        )
        .unwrap();
        assert_eq!(lam, Valor::Numero(70.0));

        let _ = fs::remove_dir_all(&dir);
    }
}
