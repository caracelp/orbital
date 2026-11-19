//! Expresiones de los gráficos (052): un solo analizador para las dos notaciones, la simple
//! (`sin(x)/x`, `2x^2`, `sqrt(x)`, `|x|`) y la de LaTeX (`\sin x`, `\frac{1}{x}`, `\sqrt[3]{x}`,
//! `\left| x \right|`), mezclables en la misma expresión. El árbol se pliega (constantes) y se
//! compila a un código de pila que se evalúa sin asignar memoria.

use std::fmt;

/// Las variables que puede usar una expresión.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Var {
    X = 0,
    Y = 1,
    T = 2,
    Theta = 3,
    /// La tercera coordenada de los gráficos 3D (053).
    Z = 4,
    /// Los parámetros de las superficies (x(u, v), y(u, v), z(u, v)) (053).
    U = 5,
    V = 6,
}

impl Var {
    pub const TODAS: [Var; 7] = [Var::X, Var::Y, Var::T, Var::Theta, Var::Z, Var::U, Var::V];
}

impl Var {
    pub fn nombre(self) -> &'static str {
        match self {
            Var::X => "x",
            Var::Y => "y",
            Var::T => "t",
            Var::Theta => "θ",
            Var::Z => "z",
            Var::U => "u",
            Var::V => "v",
        }
    }
}

/// Un error al leer una expresión.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Error(pub String);

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

fn error<T>(m: impl Into<String>) -> Result<T, Error> {
    Err(Error(m.into()))
}

/// Funciones de un argumento.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum F1 {
    Sin,
    Cos,
    Tan,
    Cot,
    Sec,
    Csc,
    Asin,
    Acos,
    Atan,
    Sinh,
    Cosh,
    Tanh,
    Asinh,
    Acosh,
    Atanh,
    Exp,
    Ln,
    Log10,
    Log2,
    Sqrt,
    Cbrt,
    Abs,
    Floor,
    Ceil,
    Round,
    Sign,
}

/// Funciones de dos argumentos.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum F2 {
    Min,
    Max,
    Atan2,
    Mod,
    Hypot,
    /// `log_b(x)`: (base, x).
    LogBase,
    /// `\sqrt[n]{x}`: (índice, x).
    Raiz,
}

impl F1 {
    fn aplicar(self, v: f64) -> f64 {
        match self {
            F1::Sin => v.sin(),
            F1::Cos => v.cos(),
            F1::Tan => v.tan(),
            F1::Cot => 1.0 / v.tan(),
            F1::Sec => 1.0 / v.cos(),
            F1::Csc => 1.0 / v.sin(),
            F1::Asin => v.asin(),
            F1::Acos => v.acos(),
            F1::Atan => v.atan(),
            F1::Sinh => v.sinh(),
            F1::Cosh => v.cosh(),
            F1::Tanh => v.tanh(),
            F1::Asinh => v.asinh(),
            F1::Acosh => v.acosh(),
            F1::Atanh => v.atanh(),
            F1::Exp => v.exp(),
            F1::Ln => v.ln(),
            F1::Log10 => v.log10(),
            F1::Log2 => v.log2(),
            F1::Sqrt => v.sqrt(),
            F1::Cbrt => v.cbrt(),
            F1::Abs => v.abs(),
            F1::Floor => v.floor(),
            F1::Ceil => v.ceil(),
            F1::Round => v.round(),
            F1::Sign => {
                if v > 0.0 {
                    1.0
                } else if v < 0.0 {
                    -1.0
                } else {
                    v
                }
            }
        }
    }

    /// La inversa de una trigonométrica (`\sin^{-1}`).
    fn inversa(self) -> Option<F1> {
        Some(match self {
            F1::Sin => F1::Asin,
            F1::Cos => F1::Acos,
            F1::Tan => F1::Atan,
            F1::Sinh => F1::Asinh,
            F1::Cosh => F1::Acosh,
            F1::Tanh => F1::Atanh,
            _ => return None,
        })
    }
}

impl F2 {
    fn aplicar(self, a: f64, b: f64) -> f64 {
        match self {
            F2::Min => a.min(b),
            F2::Max => a.max(b),
            F2::Atan2 => a.atan2(b),
            F2::Mod => a - b * (a / b).floor(),
            F2::Hypot => a.hypot(b),
            F2::LogBase => b.ln() / a.ln(),
            F2::Raiz => {
                if a == 2.0 {
                    b.sqrt()
                } else if a == 3.0 {
                    b.cbrt()
                } else {
                    potencia(b, 1.0 / a)
                }
            }
        }
    }
}

/// `a^b` con raíces reales de números negativos cuando el exponente es una fracción de
/// denominador impar (`(-8)^(1/3) = -2`), como se espera al graficar.
pub fn potencia(a: f64, b: f64) -> f64 {
    if a >= 0.0 || b.fract() == 0.0 || !b.is_finite() {
        return a.powf(b);
    }
    for q in [3.0f64, 5.0, 7.0, 9.0, 11.0, 13.0, 15.0] {
        let p = b * q;
        if (p - p.round()).abs() < 1e-9 {
            let m = (-a).powf(b);
            return if (p.round() as i64) % 2 == 0 { m } else { -m };
        }
    }
    f64::NAN
}

/// Nombres de las funciones (notación simple y comandos de LaTeX sin la barra).
const FUNCIONES1: &[(&str, F1)] = &[
    ("sin", F1::Sin),
    ("sen", F1::Sin),
    ("cos", F1::Cos),
    ("tan", F1::Tan),
    ("tg", F1::Tan),
    ("cot", F1::Cot),
    ("cotan", F1::Cot),
    ("sec", F1::Sec),
    ("csc", F1::Csc),
    ("cosec", F1::Csc),
    ("asin", F1::Asin),
    ("arcsin", F1::Asin),
    ("arcsen", F1::Asin),
    ("acos", F1::Acos),
    ("arccos", F1::Acos),
    ("atan", F1::Atan),
    ("arctan", F1::Atan),
    ("arctg", F1::Atan),
    ("sinh", F1::Sinh),
    ("senh", F1::Sinh),
    ("cosh", F1::Cosh),
    ("tanh", F1::Tanh),
    ("asinh", F1::Asinh),
    ("arsinh", F1::Asinh),
    ("acosh", F1::Acosh),
    ("arcosh", F1::Acosh),
    ("atanh", F1::Atanh),
    ("artanh", F1::Atanh),
    ("exp", F1::Exp),
    ("ln", F1::Ln),
    ("log", F1::Log10),
    ("lg", F1::Log10),
    ("log10", F1::Log10),
    ("log2", F1::Log2),
    ("sqrt", F1::Sqrt),
    ("raiz", F1::Sqrt),
    ("cbrt", F1::Cbrt),
    ("abs", F1::Abs),
    ("floor", F1::Floor),
    ("piso", F1::Floor),
    ("ceil", F1::Ceil),
    ("techo", F1::Ceil),
    ("round", F1::Round),
    ("redondeo", F1::Round),
    ("sign", F1::Sign),
    ("sgn", F1::Sign),
    ("signo", F1::Sign),
];

const FUNCIONES2: &[(&str, F2)] = &[
    ("min", F2::Min),
    ("max", F2::Max),
    ("atan2", F2::Atan2),
    ("mod", F2::Mod),
    ("hypot", F2::Hypot),
];

/// Letras griegas de LaTeX que se pueden usar como nombres de parámetros.
const GRIEGAS: &[(&str, char)] = &[
    ("alpha", 'α'),
    ("beta", 'β'),
    ("gamma", 'γ'),
    ("delta", 'δ'),
    ("epsilon", 'ε'),
    ("varepsilon", 'ε'),
    ("zeta", 'ζ'),
    ("eta", 'η'),
    ("theta", 'θ'),
    ("vartheta", 'θ'),
    ("iota", 'ι'),
    ("kappa", 'κ'),
    ("lambda", 'λ'),
    ("mu", 'μ'),
    ("nu", 'ν'),
    ("xi", 'ξ'),
    ("pi", 'π'),
    ("rho", 'ρ'),
    ("sigma", 'σ'),
    ("tau", 'τ'),
    ("upsilon", 'υ'),
    ("phi", 'φ'),
    ("varphi", 'φ'),
    ("chi", 'χ'),
    ("psi", 'ψ'),
    ("omega", 'ω'),
    ("Delta", 'Δ'),
    ("Gamma", 'Γ'),
    ("Lambda", 'Λ'),
    ("Omega", 'Ω'),
    ("Phi", 'Φ'),
    ("Psi", 'Ψ'),
    ("Sigma", 'Σ'),
    ("Theta", 'Θ'),
];

#[derive(Clone, Debug, PartialEq)]
enum Tok {
    Num(f64),
    /// Un nombre (letra, letra griega, `a_1`): variable, constante o parámetro.
    Id(String),
    F1(F1),
    F2(F2),
    /// `log` con base: `\log_2 x`, `log_{10}(x)`.
    Log,
    Frac,
    Sqrt,
    Mas,
    Menos,
    Por,
    Entre,
    Pot,
    Sub,
    Abre,
    Cierra,
    Coma,
    Barra,
    PisoAbre,
    PisoCierra,
    TechoAbre,
    TechoCierra,
    Factorial,
}

fn es_letra(c: char) -> bool {
    c.is_ascii_alphabetic() || ('\u{0391}'..='\u{03c9}').contains(&c)
}

/// Separa el texto en piezas. `nombres` son los parámetros declarados (pueden tener varias
/// letras); `ajenos` dice si un nombre desconocido de varias letras se parte en letras sueltas
/// (`xy` = `x·y`).
fn piezas(texto: &str, nombres: &[&str]) -> Result<Vec<Tok>, Error> {
    let cs: Vec<char> = texto.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < cs.len() {
        let c = cs[i];
        match c {
            ' ' | '\t' | '\n' | '\r' | '&' => i += 1,
            '0'..='9' | '.' => {
                let ini = i;
                while i < cs.len() && cs[i].is_ascii_digit() {
                    i += 1;
                }
                if i < cs.len() && cs[i] == '.' && cs.get(i + 1) != Some(&'.') {
                    i += 1;
                    while i < cs.len() && cs[i].is_ascii_digit() {
                        i += 1;
                    }
                }
                // Exponente científico: `1e-3`, `2E5` (si sigue un dígito; si no, `2e` es 2·e).
                if i < cs.len() && (cs[i] == 'e' || cs[i] == 'E') {
                    let mut j = i + 1;
                    if j < cs.len() && (cs[j] == '+' || cs[j] == '-') {
                        j += 1;
                    }
                    if j < cs.len() && cs[j].is_ascii_digit() {
                        i = j;
                        while i < cs.len() && cs[i].is_ascii_digit() {
                            i += 1;
                        }
                    }
                }
                let s: String = cs[ini..i].iter().collect();
                if s == "." {
                    return error("un punto suelto no es un número");
                }
                let v: f64 = s
                    .parse()
                    .map_err(|_| Error(format!("«{s}» no es un número")))?;
                out.push(Tok::Num(v));
            }
            '+' => {
                out.push(Tok::Mas);
                i += 1;
            }
            '-' | '−' | '–' => {
                out.push(Tok::Menos);
                i += 1;
            }
            '*' if cs.get(i + 1) == Some(&'*') => {
                out.push(Tok::Pot);
                i += 2;
            }
            '*' | '·' | '×' | '⋅' => {
                out.push(Tok::Por);
                i += 1;
            }
            '/' | '÷' => {
                out.push(Tok::Entre);
                i += 1;
            }
            '^' => {
                out.push(Tok::Pot);
                i += 1;
            }
            '²' | '³' => {
                out.push(Tok::Pot);
                out.push(Tok::Num(if c == '²' { 2.0 } else { 3.0 }));
                i += 1;
            }
            '_' => {
                out.push(Tok::Sub);
                i += 1;
            }
            '(' | '[' | '{' => {
                out.push(Tok::Abre);
                i += 1;
            }
            ')' | ']' | '}' => {
                out.push(Tok::Cierra);
                i += 1;
            }
            ',' => {
                out.push(Tok::Coma);
                i += 1;
            }
            '|' => {
                out.push(Tok::Barra);
                i += 1;
            }
            '!' => {
                out.push(Tok::Factorial);
                i += 1;
            }
            '√' => {
                out.push(Tok::Sqrt);
                i += 1;
            }
            '⌊' => {
                out.push(Tok::PisoAbre);
                i += 1;
            }
            '⌋' => {
                out.push(Tok::PisoCierra);
                i += 1;
            }
            '⌈' => {
                out.push(Tok::TechoAbre);
                i += 1;
            }
            '⌉' => {
                out.push(Tok::TechoCierra);
                i += 1;
            }
            '\\' => {
                i += 1;
                let Some(&n) = cs.get(i) else {
                    return error("una barra «\\» suelta al final");
                };
                if !n.is_ascii_alphabetic() {
                    i += 1;
                    match n {
                        // Espacios de LaTeX.
                        ',' | ';' | ':' | '!' | ' ' | '>' => {}
                        '{' => out.push(Tok::Abre),
                        '}' => out.push(Tok::Cierra),
                        '|' => out.push(Tok::Barra),
                        _ => return error(format!("«\\{n}» no se entiende en un gráfico")),
                    }
                    continue;
                }
                let ini = i;
                while i < cs.len() && cs[i].is_ascii_alphabetic() {
                    i += 1;
                }
                let nombre: String = cs[ini..i].iter().collect();
                comando(&nombre, &cs, &mut i, &mut out)?;
            }
            c if es_letra(c) => {
                let ini = i;
                while i < cs.len() && es_letra(cs[i]) {
                    i += 1;
                }
                let mut palabra: String = cs[ini..i].iter().collect();
                // `log10`, `log2`, `atan2`: los únicos nombres con dígitos.
                for (fin, digitos) in [("log", "10"), ("log", "2"), ("atan", "2")] {
                    let d: Vec<char> = digitos.chars().collect();
                    if palabra.ends_with(fin) && cs[i..].starts_with(&d) {
                        palabra.push_str(digitos);
                        i += d.len();
                        break;
                    }
                }
                partir_palabra(&palabra, nombres, &mut out)?;
            }
            otro => return error(format!("«{otro}» no se entiende en un gráfico")),
        }
    }
    Ok(out)
}

/// Un comando de LaTeX (sin la barra).
fn comando(nombre: &str, cs: &[char], i: &mut usize, out: &mut Vec<Tok>) -> Result<(), Error> {
    match nombre {
        "left" | "right" | "big" | "Big" | "bigg" | "Bigg" | "bigl" | "bigr" | "Bigl" | "Bigr"
        | "displaystyle" | "textstyle" => {
            // `\left.` no abre nada.
            if cs.get(*i) == Some(&'.') {
                *i += 1;
            }
        }
        "quad" | "qquad" | "space" => {}
        "cdot" | "times" | "ast" => out.push(Tok::Por),
        "div" => out.push(Tok::Entre),
        "frac" | "dfrac" | "tfrac" | "cfrac" => out.push(Tok::Frac),
        "sqrt" => out.push(Tok::Sqrt),
        "lvert" | "rvert" | "vert" | "mid" | "lVert" | "rVert" => out.push(Tok::Barra),
        "lfloor" => out.push(Tok::PisoAbre),
        "rfloor" => out.push(Tok::PisoCierra),
        "lceil" => out.push(Tok::TechoAbre),
        "rceil" => out.push(Tok::TechoCierra),
        "lbrace" | "langle" | "lparen" => out.push(Tok::Abre),
        "rbrace" | "rangle" | "rparen" => out.push(Tok::Cierra),
        "log" => out.push(Tok::Log),
        "infty" => return error("∞ no se puede graficar como valor"),
        "operatorname" | "mathrm" | "text" | "textrm" | "mathit" | "mathbf" | "rm" => {
            // `\operatorname{sgn}`, `\mathrm{e}`: el nombre entre llaves.
            let mut j = *i;
            while j < cs.len() && cs[j] == ' ' {
                j += 1;
            }
            if cs.get(j) != Some(&'{') {
                return Ok(());
            }
            let ini = j + 1;
            let Some(fin) = cs[ini..].iter().position(|c| *c == '}') else {
                return error(format!("falta cerrar la llave de \\{nombre}"));
            };
            let palabra: String = cs[ini..ini + fin].iter().collect();
            *i = ini + fin + 1;
            let palabra = palabra.trim();
            if let Some((_, f)) = FUNCIONES1.iter().find(|(n, _)| *n == palabra) {
                out.push(Tok::F1(*f));
            } else if let Some((_, f)) = FUNCIONES2.iter().find(|(n, _)| *n == palabra) {
                out.push(Tok::F2(*f));
            } else if !palabra.is_empty() {
                out.push(Tok::Id(palabra.to_owned()));
            }
        }
        _ => {
            if let Some((_, f)) = FUNCIONES1.iter().find(|(n, _)| *n == nombre) {
                out.push(Tok::F1(*f));
            } else if let Some((_, f)) = FUNCIONES2.iter().find(|(n, _)| *n == nombre) {
                out.push(Tok::F2(*f));
            } else if let Some((_, c)) = GRIEGAS.iter().find(|(n, _)| *n == nombre) {
                out.push(Tok::Id(c.to_string()));
            } else {
                return error(format!("el comando \\{nombre} no se puede graficar"));
            }
        }
    }
    Ok(())
}

/// Nombres de varias letras que no se parten.
const PALABRAS: &[(&str, &str)] = &[("pi", "π"), ("tau", "τ"), ("theta", "θ"), ("tita", "θ")];

/// Una palabra de letras: funciones, constantes y parámetros declarados por el nombre más largo;
/// lo demás, letra por letra (`2xy` = `2·x·y`, `sinx` = `sin x`).
fn partir_palabra(palabra: &str, nombres: &[&str], out: &mut Vec<Tok>) -> Result<(), Error> {
    let mut resto = palabra;
    while !resto.is_empty() {
        let mut mejor: Option<(usize, Tok)> = None;
        let mut probar = |n: &str, t: Tok| {
            if resto.starts_with(n) && mejor.as_ref().is_none_or(|(l, _)| n.len() > *l) {
                mejor = Some((n.len(), t));
            }
        };
        for (n, f) in FUNCIONES1 {
            probar(n, Tok::F1(*f));
        }
        for (n, f) in FUNCIONES2 {
            probar(n, Tok::F2(*f));
        }
        for (n, v) in PALABRAS {
            probar(n, Tok::Id((*v).to_owned()));
        }
        for n in nombres {
            probar(n, Tok::Id((*n).to_owned()));
        }
        // `log` sin más es el de base 10, pero acepta una base con `_`.
        if resto.starts_with("log") && mejor.as_ref().is_some_and(|(l, _)| *l == 3) {
            mejor = Some((3, Tok::Log));
        }
        let (largo, tok) = mejor.unwrap_or_else(|| {
            let c = resto.chars().next().expect("no vacío");
            (c.len_utf8(), Tok::Id(c.to_string()))
        });
        out.push(tok);
        resto = &resto[largo..];
    }
    Ok(())
}

/// El árbol de una expresión.
#[derive(Clone, Debug, PartialEq)]
pub enum Nodo {
    Num(f64),
    Var(Var),
    Param(usize),
    Neg(Box<Nodo>),
    Bin(Bin, Box<Nodo>, Box<Nodo>),
    F1(F1, Box<Nodo>),
    F2(F2, Box<Nodo>, Box<Nodo>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Bin {
    Mas,
    Menos,
    Por,
    Entre,
    Pot,
}

impl Bin {
    fn aplicar(self, a: f64, b: f64) -> f64 {
        match self {
            Bin::Mas => a + b,
            Bin::Menos => a - b,
            Bin::Por => a * b,
            Bin::Entre => a / b,
            Bin::Pot => potencia(a, b),
        }
    }
}

struct Analizador<'a> {
    toks: Vec<Tok>,
    pos: usize,
    params: &'a [&'a str],
    /// Barras de valor absoluto abiertas: una barra en posición de operador cierra.
    barras: usize,
}

impl Analizador<'_> {
    fn mirar(&self) -> Option<&Tok> {
        self.toks.get(self.pos)
    }

    fn tomar(&mut self) -> Option<Tok> {
        let t = self.toks.get(self.pos).cloned();
        self.pos += 1;
        t
    }

    fn esperar(&mut self, t: Tok, falta: &str) -> Result<(), Error> {
        if self.mirar() == Some(&t) {
            self.pos += 1;
            Ok(())
        } else {
            error(falta.to_owned())
        }
    }

    fn suma(&mut self) -> Result<Nodo, Error> {
        let mut izq = self.producto()?;
        loop {
            let op = match self.mirar() {
                Some(Tok::Mas) => Bin::Mas,
                Some(Tok::Menos) => Bin::Menos,
                _ => return Ok(izq),
            };
            self.pos += 1;
            let der = self.producto()?;
            izq = Nodo::Bin(op, Box::new(izq), Box::new(der));
        }
    }

    /// ¿La próxima pieza empieza un factor (para la multiplicación implícita)?
    fn empieza_factor(&self) -> bool {
        match self.mirar() {
            Some(
                Tok::Num(_)
                | Tok::Id(_)
                | Tok::F1(_)
                | Tok::F2(_)
                | Tok::Log
                | Tok::Frac
                | Tok::Sqrt
                | Tok::Abre
                | Tok::PisoAbre
                | Tok::TechoAbre,
            ) => true,
            Some(Tok::Barra) => self.barras == 0,
            _ => false,
        }
    }

    fn producto(&mut self) -> Result<Nodo, Error> {
        let mut izq = self.unario()?;
        loop {
            let op = match self.mirar() {
                Some(Tok::Por) => {
                    self.pos += 1;
                    Bin::Por
                }
                Some(Tok::Entre) => {
                    self.pos += 1;
                    Bin::Entre
                }
                _ if self.empieza_factor() => Bin::Por,
                _ => return Ok(izq),
            };
            let der = self.unario()?;
            izq = Nodo::Bin(op, Box::new(izq), Box::new(der));
        }
    }

    fn unario(&mut self) -> Result<Nodo, Error> {
        match self.mirar() {
            Some(Tok::Menos) => {
                self.pos += 1;
                Ok(Nodo::Neg(Box::new(self.unario()?)))
            }
            Some(Tok::Mas) => {
                self.pos += 1;
                self.unario()
            }
            _ => self.potencia(),
        }
    }

    fn potencia(&mut self) -> Result<Nodo, Error> {
        let base = self.postfijo()?;
        if self.mirar() == Some(&Tok::Pot) {
            self.pos += 1;
            let exp = self.unario()?;
            return Ok(Nodo::Bin(Bin::Pot, Box::new(base), Box::new(exp)));
        }
        Ok(base)
    }

    fn postfijo(&mut self) -> Result<Nodo, Error> {
        let p = self.primario()?;
        if self.mirar() == Some(&Tok::Factorial) {
            return error("el factorial «!» no se puede graficar");
        }
        Ok(p)
    }

    /// Un grupo entre paréntesis o llaves (`{…}` de LaTeX), o un solo factor.
    fn grupo(&mut self) -> Result<Nodo, Error> {
        if self.mirar() == Some(&Tok::Abre) {
            self.pos += 1;
            let barras = std::mem::take(&mut self.barras);
            let e = self.suma()?;
            self.barras = barras;
            self.esperar(Tok::Cierra, "falta cerrar un paréntesis o una llave")?;
            Ok(e)
        } else {
            self.potencia_sin_signo()
        }
    }

    fn potencia_sin_signo(&mut self) -> Result<Nodo, Error> {
        match self.mirar() {
            Some(Tok::Menos) => {
                self.pos += 1;
                Ok(Nodo::Neg(Box::new(self.potencia()?)))
            }
            _ => self.potencia(),
        }
    }

    /// El argumento de una función sin paréntesis (`\sin 2x`): factores seguidos hasta un
    /// operador o una función (`\sin x \cos x` = `sin(x)·cos(x)`).
    fn argumento(&mut self) -> Result<Nodo, Error> {
        if self.mirar() == Some(&Tok::Abre) {
            let g = self.grupo()?;
            // `sin(x)^2` eleva el resultado: lo hace quien llama.
            return Ok(g);
        }
        let mut a = self.potencia_sin_signo()?;
        while self.empieza_factor()
            && !matches!(
                self.mirar(),
                Some(Tok::F1(_) | Tok::F2(_) | Tok::Log | Tok::Abre)
            )
        {
            let b = self.potencia()?;
            a = Nodo::Bin(Bin::Por, Box::new(a), Box::new(b));
        }
        Ok(a)
    }

    /// `^2` o `^{-1}` entre el nombre de la función y su argumento.
    fn exponente_de_funcion(&mut self) -> Result<Option<Nodo>, Error> {
        if self.mirar() != Some(&Tok::Pot) {
            return Ok(None);
        }
        self.pos += 1;
        Ok(Some(self.grupo()?))
    }

    fn dos_argumentos(&mut self, nombre: &str) -> Result<(Nodo, Nodo), Error> {
        self.esperar(
            Tok::Abre,
            &format!("{nombre} lleva dos valores entre paréntesis: {nombre}(a, b)"),
        )?;
        let barras = std::mem::take(&mut self.barras);
        let a = self.suma()?;
        self.esperar(
            Tok::Coma,
            &format!("{nombre} lleva dos valores: {nombre}(a, b)"),
        )?;
        let b = self.suma()?;
        self.barras = barras;
        self.esperar(Tok::Cierra, "falta cerrar un paréntesis")?;
        Ok((a, b))
    }

    fn primario(&mut self) -> Result<Nodo, Error> {
        let Some(t) = self.tomar() else {
            return error("la expresión termina antes de tiempo");
        };
        match t {
            Tok::Num(v) => Ok(Nodo::Num(v)),
            Tok::Id(nombre) => self.nombre(&nombre),
            Tok::Abre => {
                let barras = std::mem::take(&mut self.barras);
                let e = self.suma()?;
                self.barras = barras;
                if self.mirar() == Some(&Tok::Coma) {
                    return error("una lista de valores entre paréntesis va sola en su renglón");
                }
                self.esperar(Tok::Cierra, "falta cerrar un paréntesis")?;
                Ok(e)
            }
            Tok::Barra => {
                self.barras += 1;
                let e = self.suma()?;
                self.barras -= 1;
                self.esperar(Tok::Barra, "falta cerrar el valor absoluto «|»")?;
                Ok(Nodo::F1(F1::Abs, Box::new(e)))
            }
            Tok::PisoAbre => {
                let e = self.suma()?;
                self.esperar(Tok::PisoCierra, "falta cerrar \\lfloor con \\rfloor")?;
                Ok(Nodo::F1(F1::Floor, Box::new(e)))
            }
            Tok::TechoAbre => {
                let e = self.suma()?;
                self.esperar(Tok::TechoCierra, "falta cerrar \\lceil con \\rceil")?;
                Ok(Nodo::F1(F1::Ceil, Box::new(e)))
            }
            Tok::Frac => {
                let a = self.grupo()?;
                let b = self.grupo()?;
                Ok(Nodo::Bin(Bin::Entre, Box::new(a), Box::new(b)))
            }
            Tok::Sqrt => {
                // `\sqrt[n]{x}`: el corchete se lee como paréntesis.
                let indice = if self.mirar() == Some(&Tok::Abre)
                    && self.toks.get(self.pos + 1).is_some()
                    && self.indice_de_raiz()
                {
                    Some(self.grupo()?)
                } else {
                    None
                };
                let x = self.argumento()?;
                Ok(match indice {
                    Some(n) => Nodo::F2(F2::Raiz, Box::new(n), Box::new(x)),
                    None => Nodo::F1(F1::Sqrt, Box::new(x)),
                })
            }
            Tok::F1(f) => {
                let exp = self.exponente_de_funcion()?;
                let (f, exp) = match exp {
                    Some(Nodo::Neg(uno)) if *uno == Nodo::Num(1.0) => match f.inversa() {
                        Some(inv) => (inv, None),
                        None => (f, Some(Nodo::Neg(uno))),
                    },
                    otro => (f, otro),
                };
                let arg = self.argumento()?;
                let v = Nodo::F1(f, Box::new(arg));
                Ok(match exp {
                    Some(e) => Nodo::Bin(Bin::Pot, Box::new(v), Box::new(e)),
                    None => v,
                })
            }
            Tok::F2(f) => {
                let nombre = FUNCIONES2
                    .iter()
                    .find(|(_, g)| *g == f)
                    .map_or("", |(n, _)| *n);
                let (a, b) = self.dos_argumentos(nombre)?;
                Ok(Nodo::F2(f, Box::new(a), Box::new(b)))
            }
            Tok::Log => {
                // `\log_2 x`, `log_{b}(x)`, `log(x)` (base 10).
                let base = if self.mirar() == Some(&Tok::Sub) {
                    self.pos += 1;
                    Some(self.grupo_o_numero()?)
                } else {
                    None
                };
                let exp = self.exponente_de_funcion()?;
                let arg = self.argumento()?;
                let v = match base {
                    Some(b) => Nodo::F2(F2::LogBase, Box::new(b), Box::new(arg)),
                    None => Nodo::F1(F1::Log10, Box::new(arg)),
                };
                Ok(match exp {
                    Some(e) => Nodo::Bin(Bin::Pot, Box::new(v), Box::new(e)),
                    None => v,
                })
            }
            Tok::Cierra => error("sobra un paréntesis o una llave que cierra"),
            Tok::Coma => error("sobra una coma"),
            Tok::Mas | Tok::Por | Tok::Entre | Tok::Pot => {
                error("falta un valor antes de un operador")
            }
            Tok::Menos => unreachable!("lo toma unario"),
            Tok::Sub => error("un «_» suelto: los subíndices van pegados a un nombre (a_1)"),
            Tok::PisoCierra | Tok::TechoCierra => error("sobra un \\rfloor o \\rceil"),
            Tok::Factorial => error("el factorial «!» no se puede graficar"),
        }
    }

    /// Después de `\sqrt`, ¿lo que sigue entre corchetes es el índice? Solo si después del
    /// grupo viene otro grupo (`\sqrt[3]{x}`); `sqrt(x)` es la raíz cuadrada.
    fn indice_de_raiz(&self) -> bool {
        let mut prof = 0;
        for (k, t) in self.toks[self.pos..].iter().enumerate() {
            match t {
                Tok::Abre => prof += 1,
                Tok::Cierra => {
                    prof -= 1;
                    if prof == 0 {
                        return self.toks.get(self.pos + k + 1) == Some(&Tok::Abre);
                    }
                }
                _ => {}
            }
        }
        false
    }

    fn grupo_o_numero(&mut self) -> Result<Nodo, Error> {
        match self.tomar() {
            Some(Tok::Num(v)) => Ok(Nodo::Num(v)),
            Some(Tok::Abre) => {
                let e = self.suma()?;
                self.esperar(Tok::Cierra, "falta cerrar la llave del subíndice")?;
                Ok(e)
            }
            Some(Tok::Id(n)) => self.nombre(&n),
            _ => error("falta la base del logaritmo"),
        }
    }

    fn nombre(&mut self, nombre: &str) -> Result<Nodo, Error> {
        // Subíndice pegado a un nombre: `a_1`, `a_{12}`.
        let mut nombre = nombre.to_owned();
        if self.mirar() == Some(&Tok::Sub) {
            self.pos += 1;
            let sub = match self.tomar() {
                Some(Tok::Num(v)) => format!("{v}"),
                Some(Tok::Id(s)) => s,
                Some(Tok::Abre) => {
                    let mut s = String::new();
                    while let Some(t) = self.tomar() {
                        match t {
                            Tok::Cierra => break,
                            Tok::Num(v) => s.push_str(&format!("{v}")),
                            Tok::Id(i) => s.push_str(&i),
                            _ => return error("un subíndice lleva letras o números"),
                        }
                    }
                    s
                }
                _ => return error("falta el subíndice después de «_»"),
            };
            nombre = format!("{nombre}_{sub}");
        }
        if let Some(k) = self.params.iter().position(|p| *p == nombre) {
            return Ok(Nodo::Param(k));
        }
        Ok(match nombre.as_str() {
            "x" => Nodo::Var(Var::X),
            "y" => Nodo::Var(Var::Y),
            "t" => Nodo::Var(Var::T),
            "θ" => Nodo::Var(Var::Theta),
            "z" => Nodo::Var(Var::Z),
            "u" => Nodo::Var(Var::U),
            "v" => Nodo::Var(Var::V),
            "π" => Nodo::Num(std::f64::consts::PI),
            "τ" => Nodo::Num(std::f64::consts::TAU),
            "e" => Nodo::Num(std::f64::consts::E),
            otro => {
                return error(format!(
                    "«{otro}» no está definido: declaralo como parámetro, por ejemplo {otro} = 1 (0..5)"
                ));
            }
        })
    }
}

impl Nodo {
    /// Pliega las constantes (`2π`, `\frac{1}{2}`).
    fn plegar(self) -> Nodo {
        match self {
            Nodo::Neg(a) => match a.plegar() {
                Nodo::Num(v) => Nodo::Num(-v),
                a => Nodo::Neg(Box::new(a)),
            },
            Nodo::Bin(op, a, b) => match (a.plegar(), b.plegar()) {
                (Nodo::Num(x), Nodo::Num(y)) => Nodo::Num(op.aplicar(x, y)),
                (a, b) => Nodo::Bin(op, Box::new(a), Box::new(b)),
            },
            Nodo::F1(f, a) => match a.plegar() {
                Nodo::Num(x) => Nodo::Num(f.aplicar(x)),
                a => Nodo::F1(f, Box::new(a)),
            },
            Nodo::F2(f, a, b) => match (a.plegar(), b.plegar()) {
                (Nodo::Num(x), Nodo::Num(y)) => Nodo::Num(f.aplicar(x, y)),
                (a, b) => Nodo::F2(f, Box::new(a), Box::new(b)),
            },
            n => n,
        }
    }

    fn usa(&self, u: &mut Usa) {
        match self {
            Nodo::Num(_) => {}
            Nodo::Var(v) => u.vars |= 1 << (*v as u8),
            Nodo::Param(k) => u.params = u.params.max(*k + 1),
            Nodo::Neg(a) | Nodo::F1(_, a) => a.usa(u),
            Nodo::Bin(_, a, b) | Nodo::F2(_, a, b) => {
                a.usa(u);
                b.usa(u);
            }
        }
    }

    fn compilar(&self, out: &mut Vec<Op>, prof: usize, max: &mut usize) {
        *max = (*max).max(prof + 1);
        match self {
            Nodo::Num(v) => out.push(Op::Num(*v)),
            Nodo::Var(v) => out.push(Op::Var(*v as u8)),
            Nodo::Param(k) => out.push(Op::Param(*k as u16)),
            Nodo::Neg(a) => {
                a.compilar(out, prof, max);
                out.push(Op::Neg);
            }
            Nodo::Bin(op, a, b) => {
                a.compilar(out, prof, max);
                // `x^2` y `x^3`, sin `powf`.
                if *op == Bin::Pot
                    && let Nodo::Num(n) = **b
                    && (n == 2.0 || n == 3.0)
                {
                    out.push(if n == 2.0 { Op::Cuadrado } else { Op::Cubo });
                    return;
                }
                b.compilar(out, prof + 1, max);
                out.push(Op::Bin(*op));
            }
            Nodo::F1(f, a) => {
                a.compilar(out, prof, max);
                out.push(Op::F1(*f));
            }
            Nodo::F2(f, a, b) => {
                a.compilar(out, prof, max);
                b.compilar(out, prof + 1, max);
                out.push(Op::F2(*f));
            }
        }
    }
}

/// Qué usa una expresión.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Usa {
    vars: u8,
    /// Uno más que el índice del parámetro más alto usado.
    params: usize,
}

impl Usa {
    pub fn var(&self, v: Var) -> bool {
        self.vars & (1 << (v as u8)) != 0
    }
    pub fn alguna_var(&self) -> bool {
        self.vars != 0
    }
    pub fn params(&self) -> bool {
        self.params > 0
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Op {
    Num(f64),
    Var(u8),
    Param(u16),
    Neg,
    Cuadrado,
    Cubo,
    Bin(Bin),
    F1(F1),
    F2(F2),
}

/// Lo más hondo que puede ir la pila de evaluación.
const PILA: usize = 48;

/// Una expresión compilada: se evalúa sin asignar memoria y desde cualquier hilo.
#[derive(Clone, Debug, PartialEq)]
pub struct Expr {
    codigo: Vec<Op>,
    usa: Usa,
}

impl Expr {
    /// Lee `texto` en cualquiera de las dos notaciones. `params` son los nombres de los
    /// parámetros declarados en el bloque (su valor se pasa al evaluar, en ese orden).
    pub fn leer(texto: &str, params: &[&str]) -> Result<Expr, Error> {
        let nodo = analizar(texto, params)?;
        Expr::de_nodo(&nodo)
    }

    pub fn de_nodo(nodo: &Nodo) -> Result<Expr, Error> {
        let nodo = nodo.clone().plegar();
        let mut codigo = Vec::new();
        let mut max = 0;
        nodo.compilar(&mut codigo, 0, &mut max);
        if max > PILA {
            return error("la expresión está demasiado anidada");
        }
        let mut usa = Usa::default();
        nodo.usa(&mut usa);
        Ok(Expr { codigo, usa })
    }

    pub fn usa(&self) -> Usa {
        self.usa
    }

    /// Si es una constante, su valor.
    pub fn constante(&self) -> Option<f64> {
        match self.codigo.as_slice() {
            [Op::Num(v)] => Some(*v),
            _ => None,
        }
    }

    /// Evalúa con `vars` = (x, y, t, θ) —o (x, y, t, θ, z, u, v) en 3D— y los valores de los
    /// parámetros. Una variable que no está en `vars` vale 0.
    #[inline]
    pub fn eval<const N: usize>(&self, vars: &[f64; N], params: &[f64]) -> f64 {
        let mut pila = [0.0f64; PILA];
        let mut n = 0usize;
        for op in &self.codigo {
            match *op {
                Op::Num(v) => {
                    pila[n] = v;
                    n += 1;
                }
                Op::Var(k) => {
                    pila[n] = vars.get(k as usize).copied().unwrap_or(0.0);
                    n += 1;
                }
                Op::Param(k) => {
                    pila[n] = params.get(k as usize).copied().unwrap_or(f64::NAN);
                    n += 1;
                }
                Op::Neg => pila[n - 1] = -pila[n - 1],
                Op::Cuadrado => pila[n - 1] *= pila[n - 1],
                Op::Cubo => pila[n - 1] = pila[n - 1] * pila[n - 1] * pila[n - 1],
                Op::Bin(b) => {
                    n -= 1;
                    pila[n - 1] = b.aplicar(pila[n - 1], pila[n]);
                }
                Op::F1(f) => pila[n - 1] = f.aplicar(pila[n - 1]),
                Op::F2(f) => {
                    n -= 1;
                    pila[n - 1] = f.aplicar(pila[n - 1], pila[n]);
                }
            }
        }
        if n == 0 { f64::NAN } else { pila[0] }
    }
}

/// El árbol de `texto` (para quien necesite combinar expresiones, como `L - R`).
pub fn analizar(texto: &str, params: &[&str]) -> Result<Nodo, Error> {
    let toks = piezas(texto, params)?;
    if toks.is_empty() {
        return error("falta la expresión");
    }
    let mut a = Analizador {
        toks,
        pos: 0,
        params,
        barras: 0,
    };
    let nodo = a.suma()?;
    if let Some(t) = a.mirar() {
        return error(match t {
            Tok::Cierra => "sobra un paréntesis o una llave que cierra".to_owned(),
            Tok::Coma => "sobra una coma".to_owned(),
            Tok::Barra => "sobra una barra «|»".to_owned(),
            _ => "la expresión sigue después de terminar".to_owned(),
        });
    }
    Ok(nodo)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(texto: &str, x: f64) -> f64 {
        Expr::leer(texto, &[])
            .unwrap()
            .eval(&[x, 0.0, 0.0, 0.0], &[])
    }

    fn cerca(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9 * (1.0 + b.abs())
    }

    #[test]
    fn notacion_simple() {
        assert!(cerca(v("x^2 + 2x + 1", 3.0), 16.0));
        assert!(cerca(v("sin(x)/x", 2.0), 2f64.sin() / 2.0));
        assert!(cerca(v("2^-1", 0.0), 0.5));
        assert!(cerca(v("-x^2", 3.0), -9.0));
        assert!(cerca(v("2^3^2", 0.0), 512.0));
        assert!(cerca(v("|x - 5|", 2.0), 3.0));
        assert!(cerca(v("sqrt(x)", 9.0), 3.0));
        assert!(cerca(v("x**2", 4.0), 16.0));
        assert!(cerca(v("2pi", 0.0), std::f64::consts::TAU));
        assert!(cerca(v("1e-3x", 2.0), 0.002));
        assert!(cerca(v("2e", 0.0), 2.0 * std::f64::consts::E));
        assert!(cerca(v("x²", 5.0), 25.0));
        assert!(cerca(v("sin x cos x", 1.0), 1f64.sin() * 1f64.cos()));
        assert!(cerca(v("sin 2x", 1.0), 2f64.sin()));
        assert!(cerca(v("sinx", 1.0), 1f64.sin()));
        assert!(cerca(v("max(x, 1)", 0.0), 1.0));
        assert!(cerca(v("mod(x, 3)", 7.0), 1.0));
        assert!(cerca(v("log(x)", 100.0), 2.0));
        assert!(cerca(v("ln(e^x)", 2.0), 2.0));
        assert!(cerca(v("x^(1/3)", -8.0), -2.0));
        assert!(cerca(v("|x||x|", -2.0), 4.0));
        assert!(cerca(v("3(x+1)", 1.0), 6.0));
        assert!(cerca(v("(x+1)(x-1)", 3.0), 8.0));
        assert!(cerca(v("sen(x)", 1.0), 1f64.sin()));
    }

    #[test]
    fn notacion_latex() {
        assert!(cerca(v(r"\sin x", 1.0), 1f64.sin()));
        assert!(cerca(v(r"\frac{1}{x}", 4.0), 0.25));
        assert!(cerca(v(r"\frac{x^{2}}{2}", 4.0), 8.0));
        assert!(cerca(v(r"\sqrt{x}", 16.0), 4.0));
        assert!(cerca(v(r"\sqrt[3]{x}", -27.0), -3.0));
        assert!(cerca(v(r"\left| x \right|", -3.0), 3.0));
        assert!(cerca(v(r"2\pi x", 1.0), std::f64::consts::TAU));
        assert!(cerca(v(r"x \cdot 3", 2.0), 6.0));
        assert!(cerca(v(r"\sin^2 x + \cos^2 x", 0.7), 1.0));
        assert!(cerca(v(r"\sin^{-1} x", 0.5), 0.5f64.asin()));
        assert!(cerca(v(r"\log_2 x", 8.0), 3.0));
        assert!(cerca(v(r"\log_{10}(x)", 1000.0), 3.0));
        assert!(cerca(v(r"e^{-x^2}", 1.0), (-1f64).exp()));
        assert!(cerca(v(r"\lfloor x \rfloor", 2.7), 2.0));
        assert!(cerca(v(r"\operatorname{sgn}(x)", -2.0), -1.0));
        assert!(cerca(v(r"\mathrm{e}^{x}", 1.0), std::f64::consts::E));
        assert!(cerca(v(r"\left(x+1\right)^{2}", 1.0), 4.0));
        assert!(cerca(v(r"\dfrac{\sin x}{x}", 2.0), 2f64.sin() / 2.0));
        assert!(cerca(v(r"3\,x", 2.0), 6.0));
    }

    #[test]
    fn mezcla_de_notaciones() {
        assert!(cerca(
            v(r"\frac{1}{2} sin(x) + sqrt(x)", 4.0),
            4f64.sin() / 2.0 + 2.0
        ));
        assert!(cerca(v(r"\sqrt{x}*2 + |x|", 4.0), 8.0));
    }

    #[test]
    fn parametros_y_errores() {
        let e = Expr::leer("a sin(b x)", &["a", "b"]).unwrap();
        assert!(cerca(
            e.eval(&[1.0, 0.0, 0.0, 0.0], &[2.0, 3.0]),
            2.0 * 3f64.sin()
        ));
        assert!(e.usa().params());
        let e = Expr::leer(r"\alpha x", &["α"]).unwrap();
        assert!(cerca(e.eval(&[2.0, 0.0, 0.0, 0.0], &[3.0]), 6.0));
        let e = Expr::leer("a_1 x", &["a_1"]).unwrap();
        assert!(cerca(e.eval(&[2.0, 0.0, 0.0, 0.0], &[3.0]), 6.0));
        let err = Expr::leer("k x", &[]).unwrap_err().0;
        assert!(err.contains("«k» no está definido"), "{err}");
        assert!(Expr::leer("(x", &[]).is_err());
        assert!(Expr::leer("x)", &[]).is_err());
        assert!(Expr::leer("x +", &[]).is_err());
        assert!(Expr::leer(r"\foo x", &[]).is_err());
        assert!(Expr::leer("", &[]).is_err());
        assert!(Expr::leer("|x", &[]).is_err());
    }

    #[test]
    fn variables_y_constantes() {
        let e = Expr::leer(r"x^2 + y^2", &[]).unwrap();
        assert!(e.usa().var(Var::X) && e.usa().var(Var::Y) && !e.usa().var(Var::T));
        let e = Expr::leer(r"1 + \cos\theta", &[]).unwrap();
        assert!(e.usa().var(Var::Theta));
        let e = Expr::leer("1 + cos(θ)", &[]).unwrap();
        assert!(e.usa().var(Var::Theta));
        assert_eq!(
            Expr::leer(r"2\pi", &[]).unwrap().constante(),
            Some(std::f64::consts::TAU)
        );
        assert!(Expr::leer("x", &[]).unwrap().constante().is_none());
    }
}
