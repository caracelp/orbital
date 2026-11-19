//! El bloque ` ```grafico ` (052): una expresión u opción por renglón.
//!
//! ````text
//! ```grafico
//! título: Ondas
//! x: -2π..2π
//! y: -3..3
//! a = 1 (0..3)
//! y = a sin(x)
//! y = \frac{1}{x} ; color: azul ; nombre: inversa
//! r = 1 + \cos\theta
//! (\cos t, \sin 2t)
//! x^2 + y^2 = 4
//! (1, 2), (2, 3)
//! datos: líneas
//! 0  0
//! 1  1.5
//! ```
//! ````

use crate::expr::{self, Bin, Expr, Nodo, Var};

/// Un error del bloque, con el renglón (desde 1) donde está.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Error {
    pub renglon: usize,
    pub mensaje: String,
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Error en el gráfico (renglón {}): {}",
            self.renglon, self.mensaje
        )
    }
}

/// Un parámetro con deslizador: `a = 1 (0..5)`.
#[derive(Clone, Debug, PartialEq)]
pub struct Parametro {
    pub nombre: String,
    pub valor: f64,
    /// Rango del deslizador; `None`: constante, sin deslizador.
    pub rango: Option<(f64, f64)>,
    pub paso: Option<f64>,
}

/// Cómo se dibuja una serie.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Estilo {
    #[default]
    Linea,
    Discontinua,
    Puntos,
    /// Puntos unidos por líneas.
    LineaYPuntos,
}

/// Un color pedido: uno de la paleta (por nombre) o uno propio.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Color {
    Paleta(usize),
    Propio([u8; 3]),
}

/// Lo que dibuja un renglón.
#[derive(Clone, Debug, PartialEq)]
pub enum Tipo {
    /// `y = f(x)`, con un dominio opcional.
    Funcion {
        f: Expr,
        dominio: Option<(f64, f64)>,
    },
    /// `r = f(θ)`.
    Polar(Expr),
    /// `(x(t), y(t))`.
    Parametrica(Expr, Expr),
    /// `F(x, y) = 0`.
    Implicita(Expr),
    /// Puntos sueltos (pueden depender de los parámetros).
    Puntos(Vec<(Expr, Expr)>),
    /// Una columna de una tabla de datos.
    Datos(Vec<(f64, f64)>),
    /// `z = f(x, y)` (053): una superficie coloreada por altura.
    Superficie(Expr),
    /// `(x(t), y(t), z(t))`: una curva en el espacio, con su rango de t si lo pide.
    Curva3 {
        x: Expr,
        y: Expr,
        z: Expr,
        t: Option<(f64, f64)>,
    },
    /// `(x(u, v), y(u, v), z(u, v))`: una superficie paramétrica (esfera, toro, Möbius).
    SuperficieParametrica {
        x: Expr,
        y: Expr,
        z: Expr,
        u: Option<(f64, f64)>,
        v: Option<(f64, f64)>,
    },
    /// `F(x, y, z) = 0`: una superficie implícita.
    Implicita3(Expr),
    /// Puntos sueltos en el espacio.
    Puntos3(Vec<[Expr; 3]>),
    /// Una nube de puntos de una tabla de datos (x y z).
    Datos3(Vec<[f64; 3]>),
}

impl Tipo {
    /// ¿Es una serie del espacio (053)?
    pub fn es_3d(&self) -> bool {
        matches!(
            self,
            Tipo::Superficie(_)
                | Tipo::Curva3 { .. }
                | Tipo::SuperficieParametrica { .. }
                | Tipo::Implicita3(_)
                | Tipo::Puntos3(_)
                | Tipo::Datos3(_)
        )
    }
}

/// Una serie: lo que dibuja un renglón (o una columna de datos).
#[derive(Clone, Debug, PartialEq)]
pub struct Serie {
    /// El renglón tal como está escrito (sin los atributos): para la leyenda y la accesibilidad.
    pub fuente: String,
    pub tipo: Tipo,
    pub color: Option<Color>,
    pub estilo: Estilo,
    pub nombre: Option<String>,
    pub grosor: Option<f64>,
    /// El renglón del bloque donde está (desde 1).
    pub renglon: usize,
}

/// La cuadrícula del fondo.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Grilla {
    No,
    Cartesiana,
    Polar,
}

/// Un bloque leído.
#[derive(Clone, Debug, PartialEq)]
pub struct Grafico {
    pub titulo: Option<String>,
    pub x: (f64, f64),
    /// `None`: la misma escala que x, centrada en 0.
    pub y: Option<(f64, f64)>,
    pub t: (f64, f64),
    pub theta: (f64, f64),
    pub ejes: bool,
    pub grilla: Grilla,
    pub leyenda: Option<bool>,
    /// Alto pedido para el gráfico, en píxeles.
    pub alto: Option<u32>,
    pub params: Vec<Parametro>,
    pub series: Vec<Serie>,
    /// ¿Se pidió el rango de x (`x:`)? En 3D, sin él la caja se ajusta a lo que se dibuja o va
    /// de −3 a 3.
    pub x_pedida: bool,
    /// Rango de z (053); `None`: según lo que se dibuja.
    pub z: Option<(f64, f64)>,
    /// Rangos de u y de v de las superficies paramétricas (0..2π si no se piden).
    pub u: (f64, f64),
    pub v: (f64, f64),
    /// Ángulo inicial de la cámara, en grados: (giro alrededor de z, elevación).
    pub camara: Option<(f64, f64)>,
    /// Puntos por lado de las mallas (superficies) y de la grilla de las implícitas.
    pub resolucion: Option<u32>,
}

impl Grafico {
    /// ¿Es un gráfico 3D (alguna serie del espacio)?
    pub fn es_3d(&self) -> bool {
        self.series.iter().any(|s| s.tipo.es_3d())
    }

    /// Los valores iniciales de los parámetros.
    pub fn valores(&self) -> Vec<f64> {
        self.params.iter().map(|p| p.valor).collect()
    }

    /// ¿Se muestra la leyenda? Si no se pidió: con más de una serie con nombre o varias series.
    pub fn con_leyenda(&self) -> bool {
        self.leyenda.unwrap_or_else(|| {
            self.series.len() > 1 || self.series.iter().any(|s| s.nombre.is_some())
        })
    }

    /// Lo que dice el lector de pantalla: el título, las expresiones y el rango.
    pub fn descripcion(&self) -> String {
        let mut s = String::from(if self.es_3d() {
            "Gráfico 3D"
        } else {
            "Gráfico"
        });
        if let Some(t) = &self.titulo {
            s.push_str(": ");
            s.push_str(t);
        }
        s.push_str(". ");
        let series: Vec<&str> = self.series.iter().map(|s| s.fuente.as_str()).collect();
        s.push_str(&series.join("; "));
        s.push_str(&format!(
            ". x de {} a {}",
            numero(self.x.0),
            numero(self.x.1)
        ));
        if let Some((a, b)) = self.y {
            s.push_str(&format!(", y de {} a {}", numero(a), numero(b)));
        }
        if let Some((a, b)) = self.z {
            s.push_str(&format!(", z de {} a {}", numero(a), numero(b)));
        }
        s.push('.');
        s
    }
}

fn numero(v: f64) -> String {
    crate::ejes::rotulo(v, 1e-3)
}

fn err<T>(renglon: usize, m: impl Into<String>) -> Result<T, Error> {
    Err(Error {
        renglon,
        mensaje: m.into(),
    })
}

/// Saca acentos y pasa a minúsculas (las claves de opciones y nombres de colores).
fn normalizar(s: &str) -> String {
    s.trim()
        .chars()
        .map(|c| match c {
            'á' | 'Á' => 'a',
            'é' | 'É' => 'e',
            'í' | 'Í' => 'i',
            'ó' | 'Ó' => 'o',
            'ú' | 'Ú' | 'ü' => 'u',
            c => c.to_ascii_lowercase(),
        })
        .collect()
}

/// Divide en el primer separador `sep` de nivel cero (fuera de paréntesis y llaves).
fn dividir_nivel_cero(s: &str, sep: char) -> Vec<&str> {
    let mut out = Vec::new();
    let mut prof = 0i32;
    let mut ini = 0;
    let mut previo = '\0';
    for (k, c) in s.char_indices() {
        match c {
            '(' | '[' | '{' => prof += 1,
            ')' | ']' | '}' => prof -= 1,
            _ if c == sep && prof == 0 && previo != '\\' => {
                out.push(&s[ini..k]);
                ini = k + c.len_utf8();
            }
            _ => {}
        }
        previo = c;
    }
    out.push(&s[ini..]);
    out
}

/// El `=` de nivel cero de una ecuación (no `<=`, `>=`, `==`).
fn igual(s: &str) -> Option<(&str, &str)> {
    let partes = dividir_nivel_cero(s, '=');
    match partes.as_slice() {
        [a, b] => Some((a, b)),
        _ => None,
    }
}

/// Lee un valor: una expresión constante (`2π`, `\frac{\pi}{2}`, `-3`).
fn constante(s: &str, params: &[&str], renglon: usize) -> Result<f64, Error> {
    let e = Expr::leer(s, params).map_err(|e| Error {
        renglon,
        mensaje: e.0,
    })?;
    if e.usa().alguna_var() || e.usa().params() {
        return err(renglon, format!("«{}» tiene que ser un número", s.trim()));
    }
    let v = e.eval(&[0.0; 4], &[]);
    if !v.is_finite() {
        return err(renglon, format!("«{}» no es un número finito", s.trim()));
    }
    Ok(v)
}

/// Un rango `a..b` (también `a, b` o `[a, b]`).
fn rango(s: &str, params: &[&str], renglon: usize) -> Result<(f64, f64), Error> {
    let s = s.trim();
    let s = s
        .strip_prefix('[')
        .and_then(|r| r.strip_suffix(']'))
        .unwrap_or(s);
    let (a, b) = if let Some(k) = s.find("..") {
        let b = s[k + 2..].trim_start_matches('.');
        (&s[..k], b)
    } else {
        match dividir_nivel_cero(s, ',').as_slice() {
            [a, b] => (*a, *b),
            _ => {
                return err(
                    renglon,
                    format!("«{s}» no es un rango: escribilo como -5..5"),
                );
            }
        }
    };
    let (a, b) = (
        constante(a, params, renglon)?,
        constante(b, params, renglon)?,
    );
    if a >= b {
        return err(renglon, "el rango tiene que ir de menor a mayor");
    }
    Ok((a, b))
}

fn booleano(v: &str, renglon: usize) -> Result<bool, Error> {
    match normalizar(v).as_str() {
        "si" | "s" | "true" | "1" | "mostrar" => Ok(true),
        "no" | "n" | "false" | "0" | "ocultar" => Ok(false),
        otro => err(renglon, format!("«{otro}» tiene que ser sí o no")),
    }
}

/// Colores por nombre: índice en la paleta de las series.
pub const NOMBRES_COLORES: &[(&str, usize)] = &[
    ("rosa", 0),
    ("celeste", 1),
    ("cian", 1),
    ("amarillo", 2),
    ("verde", 3),
    ("violeta", 4),
    ("morado", 4),
    ("naranja", 5),
    ("azul", 6),
    ("rojo", 7),
    ("gris", 8),
];

fn color(v: &str, renglon: usize) -> Result<Color, Error> {
    let n = normalizar(v);
    if let Some(hex) = n.strip_prefix('#') {
        let h = |k: usize| u8::from_str_radix(&hex[k..k + 2], 16).ok();
        if hex.len() == 6
            && let (Some(r), Some(g), Some(b)) = (h(0), h(2), h(4))
        {
            return Ok(Color::Propio([r, g, b]));
        }
        return err(renglon, format!("«{v}» no es un color: usá #rrggbb"));
    }
    NOMBRES_COLORES
        .iter()
        .find(|(nombre, _)| *nombre == n)
        .map(|(_, k)| Color::Paleta(*k))
        .ok_or_else(|| Error {
            renglon,
            mensaje: format!(
                "«{v}» no es un color: rosa, celeste, amarillo, verde, violeta, naranja, azul, rojo, gris o #rrggbb"
            ),
        })
}

/// Las claves de las opciones del bloque (normalizadas).
fn es_opcion(clave: &str) -> bool {
    matches!(
        clave,
        "x" | "y"
            | "t"
            | "θ"
            | "theta"
            | "tita"
            | "\\theta"
            | "titulo"
            | "ejes"
            | "grilla"
            | "cuadricula"
            | "leyenda"
            | "alto"
            | "datos"
            | "z"
            | "u"
            | "v"
            | "camara"
            | "resolucion"
    )
}

/// Un renglón de datos: números separados por espacios, comas, `;`, tabulaciones o `|`.
fn fila(l: &str) -> Option<Vec<f64>> {
    let l = l.trim().trim_matches('|');
    let celdas: Vec<&str> = l
        .split(|c: char| c.is_whitespace() || c == ',' || c == ';' || c == '|')
        .filter(|c| !c.is_empty())
        .collect();
    if celdas.is_empty() {
        return None;
    }
    celdas
        .iter()
        .map(|c| c.replace('−', "-").parse::<f64>().ok())
        .collect()
}

fn encabezado(l: &str) -> Option<Vec<String>> {
    let t = l.trim();
    if t.is_empty() || fila(t).is_some() {
        return None;
    }
    let celdas: Vec<String> = t
        .trim_matches('|')
        .split(['|', '\t', ';', ','])
        .flat_map(|c| {
            if t.contains(['|', '\t', ';', ',']) {
                vec![c.trim().to_owned()]
            } else {
                c.split_whitespace().map(str::to_owned).collect()
            }
        })
        .filter(|c| !c.is_empty())
        .collect();
    (celdas.len() >= 2).then_some(celdas)
}

fn separador_de_tabla(l: &str) -> bool {
    let t = l.trim();
    t.contains('-') && t.chars().all(|c| matches!(c, '|' | '-' | ':' | ' '))
}

/// Separa los atributos de una serie: `y = x^2 ; color: azul ; nombre: f`.
fn atributos(l: &str) -> (&str, Vec<(String, &str)>) {
    let partes = dividir_nivel_cero(l, ';');
    let expr = partes[0];
    let mut out = Vec::new();
    for p in &partes[1..] {
        if let Some((k, v)) = p.split_once(':') {
            out.push((normalizar(k), v.trim()));
        } else if !p.trim().is_empty() {
            out.push((String::new(), p.trim()));
        }
    }
    (expr, out)
}

/// ¿Es `a = 1 (0..5)` (un parámetro)? Devuelve (nombre, valor, resto con el rango).
fn parametro(izq: &str, der: &str) -> Option<(String, String, Option<String>)> {
    let nombre = izq.trim();
    // El nombre: una palabra (letras, griegas, `\alpha`, `a_1`) que no sea una variable.
    let normal = match nombre.strip_prefix('\\') {
        Some(cmd) => griega(cmd)?.to_string(),
        None => nombre.to_owned(),
    };
    if normal.is_empty()
        || ["x", "y", "z", "r", "t", "θ", "theta", "e", "π", "pi"].contains(&normal.as_str())
        || !normal.chars().next().is_some_and(|c| c.is_alphabetic())
        || !normal.chars().all(|c| c.is_alphanumeric() || c == '_')
    {
        return None;
    }
    let der = der.trim();
    // El rango: `(a..b)`, `[a..b]` o `(a..b, paso)` al final.
    if let Some(abre) = der.rfind(['(', '['])
        && der.ends_with([')', ']'])
        && der[abre..].contains("..")
    {
        let valor = der[..abre].trim().to_owned();
        let rango = der[abre + 1..der.len() - 1].to_owned();
        return Some((normal, valor, Some(rango)));
    }
    // Sin rango, solo si el valor es un número (`a = x^2` es una ecuación).
    let es_numero = Expr::leer(der, &[]).is_ok_and(|v| !v.usa().alguna_var() && !v.usa().params());
    es_numero.then(|| (normal, der.to_owned(), None))
}

fn griega(cmd: &str) -> Option<char> {
    const G: &[(&str, char)] = &[
        ("alpha", 'α'),
        ("beta", 'β'),
        ("gamma", 'γ'),
        ("delta", 'δ'),
        ("epsilon", 'ε'),
        ("lambda", 'λ'),
        ("mu", 'μ'),
        ("omega", 'ω'),
        ("phi", 'φ'),
        ("varphi", 'φ'),
        ("sigma", 'σ'),
        ("rho", 'ρ'),
        ("kappa", 'κ'),
        ("psi", 'ψ'),
        ("Delta", 'Δ'),
        ("Omega", 'Ω'),
    ];
    G.iter().find(|(n, _)| *n == cmd).map(|(_, c)| *c)
}

fn leer_expr(s: &str, params: &[&str], renglon: usize) -> Result<Expr, Error> {
    Expr::leer(s, params).map_err(|e| Error {
        renglon,
        mensaje: e.0,
    })
}

fn nodo(s: &str, params: &[&str], renglon: usize) -> Result<Nodo, Error> {
    expr::analizar(s, params).map_err(|e| Error {
        renglon,
        mensaje: e.0,
    })
}

fn solo(e: &Expr, permitida: Option<Var>, que: &str, renglon: usize) -> Result<(), Error> {
    solo_vars(e, permitida.as_slice(), que, renglon)
}

/// Que `e` use solo las variables `permitidas`.
fn solo_vars(e: &Expr, permitidas: &[Var], que: &str, renglon: usize) -> Result<(), Error> {
    for v in Var::TODAS {
        if e.usa().var(v) && !permitidas.contains(&v) {
            let n = v.nombre();
            return err(
                renglon,
                match v {
                    Var::U | Var::V => format!(
                        "en {que} no vale la variable {n}: u y v son de las superficies (x(u, v), y(u, v), z(u, v)); si es un parámetro, declaralo, por ejemplo {n} = 1 (0..5)"
                    ),
                    _ => format!("en {que} no vale la variable {n}"),
                },
            );
        }
    }
    Ok(())
}

/// ¿El lado izquierdo es `y`, `r`, `f(x)`…?
fn lado_simple(izq: &str) -> Option<&'static str> {
    let t: String = izq.chars().filter(|c| !c.is_whitespace()).collect();
    match t.as_str() {
        "y" => Some("y"),
        "r" => Some("r"),
        "x" => Some("x"),
        "z" => Some("z"),
        _ => {
            // `f(x)`, `g(x)`, `r(\theta)`, `r(θ)`, `f(x, y)`.
            let (n, arg) = t.split_once('(')?;
            let arg = arg.strip_suffix(')')?;
            if n.is_empty() || !n.chars().all(|c| c.is_ascii_alphabetic()) {
                return None;
            }
            match arg {
                "x" => Some("y"),
                "x,y" => Some("z"),
                "θ" | "\\theta" | "theta" => Some("r"),
                _ => None,
            }
        }
    }
}

/// Una tupla `(a, b)` o varias `(a, b), (c, d)`; también de tres: `(a, b, c)`.
fn tuplas(s: &str) -> Option<Vec<Vec<&str>>> {
    let s = s.trim();
    if !s.starts_with('(') && !s.starts_with("\\left(") {
        return None;
    }
    let mut out = Vec::new();
    for parte in dividir_nivel_cero(s, ',') {
        let p = parte.trim();
        if p.is_empty() {
            continue;
        }
        let p = p.strip_prefix("\\left(").or_else(|| p.strip_prefix('('))?;
        let p = p.strip_suffix("\\right)").or_else(|| p.strip_suffix(')'))?;
        let partes = dividir_nivel_cero(p, ',');
        if !(2..=3).contains(&partes.len()) {
            return None;
        }
        out.push(partes);
    }
    (!out.is_empty()).then_some(out)
}

const X_DEFECTO: (f64, f64) = (-10.0, 10.0);

/// Lee un bloque ` ```grafico `.
pub fn leer(texto: &str) -> Result<Grafico, Error> {
    let mut g = Grafico {
        titulo: None,
        x: X_DEFECTO,
        y: None,
        t: (0.0, std::f64::consts::TAU),
        theta: (0.0, std::f64::consts::TAU),
        ejes: true,
        grilla: Grilla::Cartesiana,
        leyenda: None,
        alto: None,
        params: Vec::new(),
        series: Vec::new(),
        x_pedida: false,
        z: None,
        u: (0.0, std::f64::consts::TAU),
        v: (0.0, std::f64::consts::TAU),
        camara: None,
        resolucion: None,
    };
    let mut grilla_pedida = false;
    let mut tablas: Vec<TablaCruda> = Vec::new();
    let renglones: Vec<&str> = texto.lines().collect();

    // Primera vuelta: los parámetros (se pueden usar antes de declararlos).
    let mut nombres: Vec<String> = Vec::new();
    for l in &renglones {
        let (e, _) = atributos(l);
        if let Some((izq, der)) = igual(e)
            && let Some((n, _, _)) = parametro(izq, der)
            && !nombres.contains(&n)
        {
            nombres.push(n);
        }
    }
    // Un nombre de una letra que es la variable de otra cosa (`a` en `y = a x`) es parámetro;
    // los demás `n = …` que no son número (por ejemplo `x = y^2`) no lo son: se ve abajo.
    let nombres_ref: Vec<&str> = nombres.iter().map(String::as_str).collect();

    let mut k = 0;
    while k < renglones.len() {
        let renglon = k + 1;
        let l = renglones[k].trim();
        k += 1;
        if l.is_empty() || l.starts_with('%') || l.starts_with("//") {
            continue;
        }
        // Opciones: `clave: valor`.
        if let Some((clave, valor)) = l.split_once(':') {
            let c = normalizar(clave);
            if es_opcion(&c) && !clave.contains('=') {
                match c.as_str() {
                    "x" => {
                        g.x = rango(valor, &nombres_ref, renglon)?;
                        g.x_pedida = true;
                    }
                    "z" => {
                        g.z = match normalizar(valor).as_str() {
                            "auto" | "automatico" => None,
                            _ => Some(rango(valor, &nombres_ref, renglon)?),
                        }
                    }
                    "u" => g.u = rango(valor, &nombres_ref, renglon)?,
                    "v" => g.v = rango(valor, &nombres_ref, renglon)?,
                    "camara" => g.camara = Some(camara(valor, renglon)?),
                    "resolucion" => {
                        let v = constante(valor, &[], renglon)?;
                        if !(8.0..=MAX_RESOLUCION as f64).contains(&v) {
                            return err(
                                renglon,
                                format!("la resolución va de 8 a {MAX_RESOLUCION}"),
                            );
                        }
                        g.resolucion = Some(v.round() as u32);
                    }
                    "y" => {
                        g.y = match normalizar(valor).as_str() {
                            "auto" | "automatico" => None,
                            _ => Some(rango(valor, &nombres_ref, renglon)?),
                        }
                    }
                    "t" => g.t = rango(valor, &nombres_ref, renglon)?,
                    "θ" | "theta" | "tita" | "\\theta" => {
                        g.theta = rango(valor, &nombres_ref, renglon)?
                    }
                    "titulo" => g.titulo = Some(valor.trim().to_owned()).filter(|s| !s.is_empty()),
                    "ejes" => g.ejes = booleano(valor, renglon)?,
                    "grilla" | "cuadricula" => {
                        grilla_pedida = true;
                        g.grilla = match normalizar(valor).as_str() {
                            "polar" => Grilla::Polar,
                            "cartesiana" | "si" | "s" | "true" | "1" => Grilla::Cartesiana,
                            "no" | "n" | "false" | "0" => Grilla::No,
                            otro => {
                                return err(
                                    renglon,
                                    format!("«{otro}»: la grilla es sí, no o polar"),
                                );
                            }
                        }
                    }
                    "leyenda" => g.leyenda = Some(booleano(valor, renglon)?),
                    "alto" => {
                        let v = constante(valor, &[], renglon)?;
                        if !(80.0..=1200.0).contains(&v) {
                            return err(renglon, "el alto va de 80 a 1200 píxeles");
                        }
                        g.alto = Some(v.round() as u32);
                    }
                    "datos" => {
                        let pedido = normalizar(valor);
                        // `datos: nube` (o `3d`): una nube de puntos x y z.
                        let nube = matches!(pedido.as_str(), "nube" | "3d" | "nube de puntos");
                        let estilo = match pedido.as_str() {
                            "" | "puntos" | "dispersion" | "nube" | "3d" | "nube de puntos" => {
                                Estilo::Puntos
                            }
                            "lineas" | "linea" => Estilo::Linea,
                            "lineas y puntos" | "ambos" => Estilo::LineaYPuntos,
                            otro => {
                                return err(
                                    renglon,
                                    format!("«{otro}»: los datos van como puntos, líneas o ambos"),
                                );
                            }
                        };
                        let primera = g.series.len();
                        let (siguiente, cruda) = tabla(&renglones, k, estilo, &mut g)?;
                        // Con encabezado x y z, también es una nube.
                        let con_z = cruda
                            .nombres
                            .as_ref()
                            .is_some_and(|n| n.get(2).is_some_and(|z| normalizar(z) == "z"));
                        tablas.push(TablaCruda {
                            primera,
                            cuantas: g.series.len() - primera,
                            nube: nube || con_z,
                            ..cruda
                        });
                        k = siguiente;
                    }
                    _ => unreachable!(),
                }
                continue;
            }
        }
        let (e, attrs) = atributos(l);
        let mut serie = match ecuacion(e, &nombres_ref, &mut g, renglon)? {
            Some(s) => s,
            None => continue,
        };
        for (clave, valor) in attrs {
            match clave.as_str() {
                "color" => serie.color = Some(color(valor, renglon)?),
                "nombre" | "rotulo" => serie.nombre = Some(valor.to_owned()),
                "estilo" => {
                    serie.estilo = match normalizar(valor).as_str() {
                        "linea" | "lineas" | "continua" => Estilo::Linea,
                        "discontinua" | "punteada" | "guiones" => Estilo::Discontinua,
                        "puntos" | "dispersion" => Estilo::Puntos,
                        "lineas y puntos" | "ambos" => Estilo::LineaYPuntos,
                        otro => {
                            return err(
                                renglon,
                                format!(
                                    "«{otro}»: el estilo es línea, discontinua, puntos o ambos"
                                ),
                            );
                        }
                    }
                }
                "grosor" => {
                    let v = constante(valor, &[], renglon)?;
                    if !(0.5..=8.0).contains(&v) {
                        return err(renglon, "el grosor va de 0.5 a 8");
                    }
                    serie.grosor = Some(v);
                }
                "dominio" => {
                    let d = rango(valor, &nombres_ref, renglon)?;
                    match &mut serie.tipo {
                        Tipo::Funcion { dominio, .. } => *dominio = Some(d),
                        _ => return err(renglon, "el dominio es para las funciones y = f(x)"),
                    }
                }
                "t" => {
                    let d = rango(valor, &nombres_ref, renglon)?;
                    match &mut serie.tipo {
                        Tipo::Curva3 { t, .. } => *t = Some(d),
                        _ => {
                            return err(
                                renglon,
                                "el rango t: de una serie es para las curvas (x(t), y(t), z(t)); para las del plano, usá la opción t: del gráfico",
                            );
                        }
                    }
                }
                "u" | "v" => {
                    let d = rango(valor, &nombres_ref, renglon)?;
                    match &mut serie.tipo {
                        Tipo::SuperficieParametrica { u, v, .. } => {
                            if clave == "u" {
                                *u = Some(d);
                            } else {
                                *v = Some(d);
                            }
                        }
                        _ => {
                            return err(
                                renglon,
                                format!(
                                    "el rango {clave}: es para las superficies (x(u, v), y(u, v), z(u, v))"
                                ),
                            );
                        }
                    }
                }
                "" => {
                    // Un color suelto: `y = x ; azul`.
                    serie.color = Some(color(valor, renglon)?);
                }
                otro => return err(renglon, format!("«{otro}» no es un atributo de una serie")),
            }
        }
        g.series.push(serie);
    }
    // Las expresiones nombran los parámetros por su lugar en la primera vuelta.
    g.params
        .sort_by_key(|p| nombres.iter().position(|n| *n == p.nombre));
    if g.series.is_empty() {
        return err(
            renglones.len().max(1),
            "el gráfico no tiene nada que dibujar: escribí una expresión, como y = x^2",
        );
    }
    if g.es_3d() || g.z.is_some() || g.camara.is_some() || tablas.iter().any(|t| t.nube) {
        a_3d(&mut g, tablas)?;
    } else if g.resolucion.is_some() {
        return err(
            renglones.len().max(1),
            "la resolución es para los gráficos 3D (superficies e implícitas)",
        );
    }
    if !grilla_pedida && g.series.iter().all(|s| matches!(s.tipo, Tipo::Polar(_))) {
        g.grilla = Grilla::Polar;
    }
    Ok(g)
}

/// Lo más fino que se puede pedir con `resolución:`.
pub const MAX_RESOLUCION: u32 = 160;

/// `cámara: 30, 20` (grados: giro alrededor de z y elevación).
fn camara(v: &str, renglon: usize) -> Result<(f64, f64), Error> {
    let v = v.replace('°', "");
    match dividir_nivel_cero(&v, ',').as_slice() {
        [a, b] => {
            let (a, b) = (constante(a, &[], renglon)?, constante(b, &[], renglon)?);
            if !(-90.0..=90.0).contains(&b) {
                return err(renglon, "la elevación de la cámara va de -90 a 90 grados");
            }
            Ok((a, b))
        }
        _ => err(
            renglon,
            "la cámara lleva dos ángulos en grados: giro y elevación, por ejemplo cámara: 30, 20",
        ),
    }
}

/// Una tabla tal como se leyó (para volver a leerla en 3D).
struct TablaCruda {
    primera: usize,
    cuantas: usize,
    filas: Vec<Vec<f64>>,
    nombres: Option<Vec<String>>,
    estilo: Estilo,
    renglon: usize,
    /// `datos: nube` o encabezado x y z: el gráfico es 3D.
    nube: bool,
}

/// Pasa el gráfico a 3D: las ecuaciones de x e y son superficies (x² + y² = 1 es un cilindro),
/// una expresión sola es la altura z = f(x, y) y las tablas son nubes x y z.
fn a_3d(g: &mut Grafico, tablas: Vec<TablaCruda>) -> Result<(), Error> {
    let mut series = std::mem::take(&mut g.series);
    // Las tablas, de atrás para adelante para no correr los índices.
    for t in tablas.into_iter().rev() {
        let columnas = t.filas[0].len();
        if columnas < 3 {
            return err(
                t.renglon,
                "en un gráfico 3D, cada fila de datos lleva tres números: x y z",
            );
        }
        let nuevas: Vec<Serie> = (2..columnas)
            .map(|c| {
                let puntos: Vec<[f64; 3]> = t.filas.iter().map(|f| [f[0], f[1], f[c]]).collect();
                let nombre = t.nombres.as_ref().and_then(|n| n.get(c)).cloned();
                let fuente = match &nombre {
                    Some(n) => format!("datos: {n} ({} puntos)", puntos.len()),
                    None => format!("datos ({} puntos)", puntos.len()),
                };
                Serie {
                    fuente,
                    tipo: Tipo::Datos3(puntos),
                    color: None,
                    estilo: t.estilo,
                    nombre,
                    grosor: None,
                    renglon: t.renglon,
                }
            })
            .collect();
        series.splice(t.primera..t.primera + t.cuantas, nuevas);
    }
    let nombres: Vec<String> = g.params.iter().map(|p| p.nombre.clone()).collect();
    let nombres: Vec<&str> = nombres.iter().map(String::as_str).collect();
    for s in &mut series {
        let r = s.renglon;
        let tipo = std::mem::replace(&mut s.tipo, Tipo::Datos(Vec::new()));
        s.tipo = match tipo {
            Tipo::Funcion { f, dominio } => {
                if dominio.is_some() {
                    return err(r, "en un gráfico 3D no vale el dominio: usá x: e y:");
                }
                if let Some((_, der)) = igual(&s.fuente) {
                    // `y = f(x)` en el espacio: la superficie y − f(x) = 0.
                    let n = nodo(der, &nombres, r)?;
                    let e = Expr::de_nodo(&Nodo::Bin(
                        Bin::Menos,
                        Box::new(Nodo::Var(Var::Y)),
                        Box::new(n),
                    ))
                    .map_err(|e| Error {
                        renglon: r,
                        mensaje: e.0,
                    })?;
                    Tipo::Implicita3(e)
                } else {
                    Tipo::Superficie(f)
                }
            }
            Tipo::Implicita(f) => Tipo::Implicita3(f),
            Tipo::Polar(_) => {
                return err(r, "en un gráfico 3D no van las curvas polares r = f(θ)");
            }
            Tipo::Parametrica(..) => {
                return err(
                    r,
                    "en un gráfico 3D, las curvas llevan tres coordenadas: (x(t), y(t), z(t))",
                );
            }
            Tipo::Puntos(_) => {
                return err(
                    r,
                    "en un gráfico 3D, los puntos llevan tres coordenadas: (x, y, z)",
                );
            }
            otro => otro,
        };
    }
    g.series = series;
    if !g.x_pedida && g.series.iter().any(necesita_caja) {
        g.x = X_DEFECTO_3D;
    }
    Ok(())
}

/// ¿La serie necesita un rango de x e y para dibujarse (superficies z = f(x, y) e implícitas)?
pub fn necesita_caja(s: &Serie) -> bool {
    matches!(s.tipo, Tipo::Superficie(_) | Tipo::Implicita3(_))
}

/// x (e y) de un gráfico 3D si no se piden y hay superficies o implícitas.
pub const X_DEFECTO_3D: (f64, f64) = (-3.0, 3.0);

fn serie(fuente: &str, tipo: Tipo, renglon: usize) -> Serie {
    Serie {
        fuente: fuente.trim().to_owned(),
        tipo,
        color: None,
        estilo: Estilo::Linea,
        nombre: None,
        grosor: None,
        renglon,
    }
}

/// Una ecuación o expresión. `None`: era un parámetro (queda en `g.params`).
fn ecuacion(
    e: &str,
    params: &[&str],
    g: &mut Grafico,
    renglon: usize,
) -> Result<Option<Serie>, Error> {
    // Puntos o curva paramétrica: `(a, b)`, `(a, b, c)`.
    if let Some(ts) = tuplas(e) {
        let dim = ts[0].len();
        if ts.iter().any(|t| t.len() != dim) {
            return err(
                renglon,
                "todos los puntos de un renglón llevan la misma cantidad de coordenadas",
            );
        }
        let mut tuplas_e: Vec<Vec<Expr>> = Vec::with_capacity(ts.len());
        for t in &ts {
            let mut v = Vec::with_capacity(dim);
            for c in t {
                v.push(leer_expr(c, params, renglon)?);
            }
            tuplas_e.push(v);
        }
        let usa = |var: Var| tuplas_e.iter().flatten().any(|x| x.usa().var(var));
        if dim == 3 {
            let (t, uv) = (usa(Var::T), usa(Var::U) || usa(Var::V));
            return tupla_3d(e, tuplas_e, t, uv, renglon).map(Some);
        }
        let usa_t = usa(Var::T);
        for x in tuplas_e.iter().flatten() {
            solo(
                x,
                usa_t.then_some(Var::T),
                "un punto o una curva (x(t), y(t))",
                renglon,
            )?;
        }
        if usa_t {
            if tuplas_e.len() != 1 {
                return err(renglon, "una curva (x(t), y(t)) va sola en su renglón");
            }
            let mut t = tuplas_e.pop().expect("una");
            let b = t.pop().expect("dos");
            let a = t.pop().expect("dos");
            return Ok(Some(serie(e, Tipo::Parametrica(a, b), renglon)));
        }
        let pares = tuplas_e
            .into_iter()
            .map(|mut t| {
                let b = t.pop().expect("dos");
                let a = t.pop().expect("dos");
                (a, b)
            })
            .collect();
        let mut s = serie(e, Tipo::Puntos(pares), renglon);
        s.estilo = Estilo::Puntos;
        return Ok(Some(s));
    }
    // Paramétrica escrita como `x = cos t, y = sin t` (y `, z = t` en el espacio).
    let partes = dividir_nivel_cero(e, ',');
    if (2..=3).contains(&partes.len()) {
        let lados: Vec<Option<(&str, &str)>> = partes.iter().map(|p| igual(p)).collect();
        let esperado = ["x", "y", "z"];
        if lados
            .iter()
            .enumerate()
            .all(|(k, l)| l.is_some_and(|(izq, _)| lado_simple(izq) == Some(esperado[k])))
        {
            let mut fs = Vec::with_capacity(3);
            for (_, der) in lados.into_iter().flatten() {
                fs.push(leer_expr(der, params, renglon)?);
            }
            if fs.len() == 3 {
                let usa = |var: Var| fs.iter().any(|x| x.usa().var(var));
                let (t, uv) = (usa(Var::T), usa(Var::U) || usa(Var::V));
                return tupla_3d(e, vec![fs], t, uv, renglon).map(Some);
            }
            for f in &fs {
                solo(f, Some(Var::T), "x = f(t), y = g(t)", renglon)?;
            }
            let fy = fs.pop().expect("dos");
            let fx = fs.pop().expect("dos");
            return Ok(Some(serie(e, Tipo::Parametrica(fx, fy), renglon)));
        }
    }
    let Some((izq, der)) = igual(e) else {
        if e.contains(['<', '>', '≤', '≥']) || e.contains("\\le") || e.contains("\\ge") {
            return err(renglon, "las desigualdades todavía no se pueden graficar");
        }
        if dividir_nivel_cero(e, '=').len() > 2 {
            return err(renglon, "un renglón lleva un solo «=»");
        }
        // Una expresión sola: y = f(x), r = f(θ) si usa θ, o la altura z = f(x, y) si usa y.
        let f = leer_expr(e, params, renglon)?;
        if f.usa().var(Var::Theta) {
            solo(&f, Some(Var::Theta), "r = f(θ)", renglon)?;
            return Ok(Some(serie(e, Tipo::Polar(f), renglon)));
        }
        if f.usa().var(Var::Z) {
            return err(renglon, "falta el «=»: por ejemplo x^2 + y^2 + z^2 = 1");
        }
        if f.usa().var(Var::Y) {
            solo_vars(&f, &[Var::X, Var::Y], "z = f(x, y)", renglon)?;
            return Ok(Some(serie(e, Tipo::Superficie(f), renglon)));
        }
        solo(&f, Some(Var::X), "y = f(x)", renglon)?;
        return Ok(Some(serie(e, Tipo::Funcion { f, dominio: None }, renglon)));
    };
    if dividir_nivel_cero(der, ',').len() > 1 {
        return err(
            renglon,
            "sobra una coma (las curvas se escriben x = f(t), y = g(t))",
        );
    }
    // Un parámetro: `a = 1 (0..5)` o `a = 2`.
    if let Some((nombre, valor, rango_txt)) = parametro(izq, der) {
        if g.params.iter().any(|p| p.nombre == nombre) {
            return err(renglon, format!("el parámetro {nombre} ya está declarado"));
        }
        let valor = constante(&valor, &[], renglon)?;
        let (rango, paso) = match rango_txt {
            Some(r) => {
                let partes = dividir_nivel_cero(&r, ',');
                let rg = rango(partes[0], &[], renglon)?;
                let paso = match partes.get(1) {
                    Some(p) => {
                        let p = p.trim();
                        let p = p.strip_prefix("paso").unwrap_or(p);
                        let v = constante(p.trim_start_matches([':', ' ']), &[], renglon)?;
                        if v <= 0.0 {
                            return err(renglon, "el paso tiene que ser positivo");
                        }
                        Some(v)
                    }
                    None => None,
                };
                (Some(rg), paso)
            }
            None => (None, None),
        };
        if let Some((a, b)) = rango
            && !(a..=b).contains(&valor)
        {
            return err(
                renglon,
                format!("el valor de {nombre} está fuera de su rango"),
            );
        }
        g.params.push(Parametro {
            nombre,
            valor,
            rango,
            paso,
        });
        return Ok(None);
    }
    match lado_simple(izq) {
        Some("y") => {
            let f = leer_expr(der, params, renglon)?;
            if !f.usa().var(Var::Y) && !f.usa().var(Var::Z) {
                solo(&f, Some(Var::X), "y = f(x)", renglon)?;
                return Ok(Some(serie(e, Tipo::Funcion { f, dominio: None }, renglon)));
            }
        }
        Some("r") => {
            let f = leer_expr(der, params, renglon)?;
            solo(&f, Some(Var::Theta), "r = f(θ)", renglon)?;
            return Ok(Some(serie(e, Tipo::Polar(f), renglon)));
        }
        Some("z") => {
            let f = leer_expr(der, params, renglon)?;
            if !f.usa().var(Var::Z) {
                solo_vars(&f, &[Var::X, Var::Y], "z = f(x, y)", renglon)?;
                return Ok(Some(serie(e, Tipo::Superficie(f), renglon)));
            }
        }
        _ => {}
    }
    // Implícita: `L = R` como `L - R = 0` (en el espacio si usa z).
    let l = nodo(izq, params, renglon)?;
    let r = nodo(der, params, renglon)?;
    let f = Expr::de_nodo(&Nodo::Bin(Bin::Menos, Box::new(l), Box::new(r))).map_err(|e| Error {
        renglon,
        mensaje: e.0,
    })?;
    if !f.usa().var(Var::X) && !f.usa().var(Var::Y) && !f.usa().var(Var::Z) {
        return err(renglon, "la ecuación no tiene x, y ni z");
    }
    if f.usa().var(Var::Z) {
        solo_vars(
            &f,
            &[Var::X, Var::Y, Var::Z],
            "una ecuación F(x, y, z) = 0",
            renglon,
        )?;
        return Ok(Some(serie(e, Tipo::Implicita3(f), renglon)));
    }
    solo_vars(&f, &[Var::X, Var::Y], "una ecuación F(x, y) = 0", renglon)?;
    Ok(Some(serie(e, Tipo::Implicita(f), renglon)))
}

/// Una tupla de tres: una curva si usa t, una superficie si usa u o v, y si no, puntos.
fn tupla_3d(
    e: &str,
    mut tuplas: Vec<Vec<Expr>>,
    usa_t: bool,
    usa_uv: bool,
    renglon: usize,
) -> Result<Serie, Error> {
    if usa_t && usa_uv {
        return err(
            renglon,
            "una curva usa t y una superficie usa u y v: no las dos en el mismo renglón",
        );
    }
    if (usa_t || usa_uv) && tuplas.len() != 1 {
        return err(renglon, "una curva o una superficie va sola en su renglón");
    }
    let permitidas: &[Var] = if usa_t {
        &[Var::T]
    } else if usa_uv {
        &[Var::U, Var::V]
    } else {
        &[]
    };
    let que = if usa_t {
        "una curva (x(t), y(t), z(t))"
    } else if usa_uv {
        "una superficie (x(u, v), y(u, v), z(u, v))"
    } else {
        "un punto (x, y, z)"
    };
    for x in tuplas.iter().flatten() {
        solo_vars(x, permitidas, que, renglon)?;
    }
    let tres = |mut t: Vec<Expr>| -> [Expr; 3] {
        let z = t.pop().expect("tres");
        let y = t.pop().expect("tres");
        let x = t.pop().expect("tres");
        [x, y, z]
    };
    if usa_t || usa_uv {
        let [x, y, z] = tres(tuplas.pop().expect("una"));
        let tipo = if usa_t {
            Tipo::Curva3 { x, y, z, t: None }
        } else {
            Tipo::SuperficieParametrica {
                x,
                y,
                z,
                u: None,
                v: None,
            }
        };
        return Ok(serie(e, tipo, renglon));
    }
    let mut s = serie(
        e,
        Tipo::Puntos3(tuplas.into_iter().map(tres).collect()),
        renglon,
    );
    s.estilo = Estilo::Puntos;
    Ok(s)
}

/// Una tabla de datos desde el renglón `k` (después de `datos:`). Devuelve el renglón siguiente.
fn tabla(
    renglones: &[&str],
    mut k: usize,
    estilo: Estilo,
    g: &mut Grafico,
) -> Result<(usize, TablaCruda), Error> {
    let inicio = k;
    let mut nombres: Option<Vec<String>> = None;
    let mut filas: Vec<Vec<f64>> = Vec::new();
    while k < renglones.len() {
        let l = renglones[k];
        if l.trim().is_empty() {
            break;
        }
        if filas.is_empty()
            && nombres.is_none()
            && let Some(n) = encabezado(l)
        {
            nombres = Some(n);
            k += 1;
            continue;
        }
        if separador_de_tabla(l) {
            k += 1;
            continue;
        }
        let Some(f) = fila(l) else { break };
        if f.len() < 2 {
            return err(k + 1, "cada fila de datos lleva al menos dos números: x y");
        }
        if let Some(primera) = filas.first()
            && primera.len() != f.len()
        {
            return err(
                k + 1,
                "todas las filas de datos llevan la misma cantidad de números",
            );
        }
        filas.push(f);
        k += 1;
    }
    if filas.is_empty() {
        return err(
            inicio,
            "después de «datos:» van las filas: x y (un par de números por renglón)",
        );
    }
    let columnas = filas[0].len();
    for c in 1..columnas {
        let puntos: Vec<(f64, f64)> = filas.iter().map(|f| (f[0], f[c])).collect();
        let nombre = nombres.as_ref().and_then(|n| n.get(c)).cloned();
        let fuente = match &nombre {
            Some(n) => format!("datos: {n} ({} puntos)", puntos.len()),
            None => format!("datos ({} puntos)", puntos.len()),
        };
        g.series.push(Serie {
            fuente,
            tipo: Tipo::Datos(puntos),
            color: None,
            estilo,
            nombre,
            grosor: None,
            renglon: inicio,
        });
    }
    Ok((
        k,
        TablaCruda {
            primera: 0,
            cuantas: 0,
            filas,
            nombres,
            estilo,
            renglon: inicio,
            nube: false,
        },
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tipos(g: &Grafico) -> Vec<&'static str> {
        g.series
            .iter()
            .map(|s| match s.tipo {
                Tipo::Funcion { .. } => "funcion",
                Tipo::Polar(_) => "polar",
                Tipo::Parametrica(..) => "parametrica",
                Tipo::Implicita(_) => "implicita",
                Tipo::Puntos(_) => "puntos",
                Tipo::Datos(_) => "datos",
                Tipo::Superficie(_) => "superficie",
                Tipo::Curva3 { .. } => "curva3",
                Tipo::SuperficieParametrica { .. } => "parametrica3",
                Tipo::Implicita3(_) => "implicita3",
                Tipo::Puntos3(_) => "puntos3",
                Tipo::Datos3(_) => "datos3",
            })
            .collect()
    }

    #[test]
    fn cada_tipo_en_las_dos_notaciones() {
        let g = leer(
            "y = x^2\nsin(x)/x\ny = \\frac{1}{x}\nf(x) = \\sqrt{x}\nr = 1 + \\cos\\theta\nr = 1 + cos(θ)\n1 + \\cos(2\\theta)\n(\\cos t, \\sin t)\nx = cos(t), y = sin(2t)\nx^2 + y^2 = 1\n\\frac{x^2}{4} + y^2 = 1\ny = x + y^2\n(1, 2), (3, 4)\n",
        )
        .unwrap();
        assert_eq!(
            tipos(&g),
            [
                "funcion",
                "funcion",
                "funcion",
                "funcion",
                "polar",
                "polar",
                "polar",
                "parametrica",
                "parametrica",
                "implicita",
                "implicita",
                "implicita",
                "puntos"
            ]
        );
        assert_eq!(g.series[0].fuente, "y = x^2");
    }

    #[test]
    fn opciones_y_parametros() {
        let g = leer(
            "título: Ondas\nx: -2π..2π\ny: -3, 3\nejes: no\ngrilla: polar\nleyenda: sí\nalto: 400\na = 1 (0..5)\nb = 2 (0..10, paso 0.5)\nc = \\frac{\\pi}{2}\ny = a sin(b x + c) ; color: azul ; nombre: onda ; estilo: discontinua ; grosor: 3\ny = x ; dominio: 0..1 ; rojo",
        )
        .unwrap();
        assert_eq!(g.titulo.as_deref(), Some("Ondas"));
        assert!((g.x.0 + std::f64::consts::TAU).abs() < 1e-12);
        assert_eq!(g.y, Some((-3.0, 3.0)));
        assert!(!g.ejes);
        assert_eq!(g.grilla, Grilla::Polar);
        assert_eq!(g.alto, Some(400));
        assert_eq!(g.params.len(), 3);
        assert_eq!(g.params[0].rango, Some((0.0, 5.0)));
        assert_eq!(g.params[1].paso, Some(0.5));
        assert_eq!(g.params[2].rango, None);
        let s = &g.series[0];
        assert_eq!(s.color, Some(Color::Paleta(6)));
        assert_eq!(s.nombre.as_deref(), Some("onda"));
        assert_eq!(s.estilo, Estilo::Discontinua);
        assert_eq!(s.grosor, Some(3.0));
        assert!(matches!(
            g.series[1].tipo,
            Tipo::Funcion {
                dominio: Some((0.0, 1.0)),
                ..
            }
        ));
        assert_eq!(g.series[1].color, Some(Color::Paleta(7)));
    }

    #[test]
    fn parametros_declarados_despues_y_griegos() {
        let g = leer("y = k x\nk = 2 (0..4)\n\\alpha = 1 (0..2)\nr = \\alpha\\theta").unwrap();
        assert_eq!(g.params.len(), 2);
        assert_eq!(g.params[0].nombre, "k");
        assert_eq!(g.params[1].nombre, "α");
        assert_eq!(tipos(&g), ["funcion", "polar"]);
    }

    #[test]
    fn tabla_de_datos() {
        let g = leer("datos: líneas\n| t | a | b |\n|---|---|---|\n| 0 | 1 | 2 |\n| 1 | 3 | 4 |\n\ndatos:\n0, 0\n1, 2.5\n2 -1\ny = x").unwrap();
        assert_eq!(tipos(&g), ["datos", "datos", "datos", "funcion"]);
        assert_eq!(g.series[0].estilo, Estilo::Linea);
        assert_eq!(g.series[0].nombre.as_deref(), Some("a"));
        assert_eq!(g.series[1].tipo, Tipo::Datos(vec![(0.0, 2.0), (1.0, 4.0)]));
        assert_eq!(g.series[2].estilo, Estilo::Puntos);
        assert_eq!(
            g.series[2].tipo,
            Tipo::Datos(vec![(0.0, 0.0), (1.0, 2.5), (2.0, -1.0)])
        );
    }

    #[test]
    fn errores_con_renglon() {
        let e = leer("y = x\ny = k x").unwrap_err();
        assert_eq!(e.renglon, 2);
        assert!(e.mensaje.contains("«k» no está definido"), "{e}");
        assert!(leer("").is_err());
        assert!(
            leer("x: 5..1\ny = x")
                .unwrap_err()
                .mensaje
                .contains("menor a mayor")
        );
        assert!(leer("y = x ; color: fucsia").is_err());
        assert!(leer("y = t").is_err());
        assert!(leer("x^2 + y^2 < 1").is_err());
        assert!(leer("r = x").is_err());
        assert!(leer("a = 7 (0..5)\ny = a").is_err());
        assert!(leer("datos:\nhola").is_err());
        assert_eq!(
            leer("y = x\n\n% comentario\nalto: 10").unwrap_err().renglon,
            4
        );
        let msg = leer("y = (x").unwrap_err().to_string();
        assert!(msg.starts_with("Error en el gráfico (renglón 1):"), "{msg}");
    }

    #[test]
    fn tres_d_en_las_dos_notaciones() {
        let g = leer(
            "z = x^2 - y^2\nf(x, y) = \\sin x \\cos y\nx y\n(\\cos t, \\sin t, t/5)\nx = cos(t), y = sin(t), z = t\n(\\cos u \\sin v, \\sin u \\sin v, \\cos v) ; v: 0..π\nx = u, y = v, z = u v\nx^2 + y^2 + z^2 = 1\n\\frac{x^2}{4} + y^2 - z = 0\n(1, 2, 3), (4, 5, 6)\n",
        )
        .unwrap();
        assert!(g.es_3d());
        assert_eq!(
            tipos(&g),
            [
                "superficie",
                "superficie",
                "superficie",
                "curva3",
                "curva3",
                "parametrica3",
                "parametrica3",
                "implicita3",
                "implicita3",
                "puntos3"
            ]
        );
        assert!(matches!(
            g.series[5].tipo,
            Tipo::SuperficieParametrica {
                v: Some((0.0, _)),
                u: None,
                ..
            }
        ));
        assert!(g.descripcion().starts_with("Gráfico 3D. z = x^2 - y^2;"));
        // Sin x pedida y con superficies, la caja va de −3 a 3.
        assert_eq!(g.x, X_DEFECTO_3D);
    }

    #[test]
    fn deteccion_2d_y_3d() {
        // Del plano: nada cambia.
        let g = leer("y = x^2\nx^2 + y^2 = 1\nr = 2\n(1, 2)").unwrap();
        assert!(!g.es_3d());
        assert_eq!(g.x, (-10.0, 10.0));
        // Con una serie del espacio, todo el gráfico es 3D: x² + y² = 1 es un cilindro, una
        // expresión sola de x es la altura y y = f(x) es una superficie.
        let g = leer("x^2 + y^2 = 1\nsin(x)\ny = x^2\nz = 1").unwrap();
        assert_eq!(
            tipos(&g),
            ["implicita3", "superficie", "implicita3", "superficie"]
        );
        // Las opciones del espacio también lo hacen 3D.
        assert!(leer("cámara: 30, 20\nx^2 + y^2 = 1").unwrap().es_3d());
        assert!(leer("z: 0..1\nx^2 + y^2 = 1").unwrap().es_3d());
        // Tablas: con encabezado x y z o `datos: nube`, una nube de puntos.
        let g = leer("datos:\nx y z\n0 0 0\n1 2 3").unwrap();
        assert_eq!(tipos(&g), ["datos3"]);
        let g = leer("datos: nube\n0 0 0 1\n1 2 3 4").unwrap();
        assert_eq!(tipos(&g), ["datos3", "datos3"]);
        assert_eq!(
            g.series[1].tipo,
            Tipo::Datos3(vec![[0.0, 0.0, 1.0], [1.0, 2.0, 4.0]])
        );
        let g = leer("datos:\n0 0 0\n1 2 3").unwrap();
        assert_eq!(
            tipos(&g),
            ["datos", "datos"],
            "sin pistas, dos series del plano"
        );
        // Opciones.
        let g =
            leer("z: -1..1\nu: 0..π\nv: 0..2π\ncámara: -45, 30°\nresolución: 80\nz = x").unwrap();
        assert_eq!(g.z, Some((-1.0, 1.0)));
        assert_eq!(g.camara, Some((-45.0, 30.0)));
        assert_eq!(g.resolucion, Some(80));
        assert!((g.u.1 - std::f64::consts::PI).abs() < 1e-12);
    }

    #[test]
    fn errores_3d() {
        let e = |t: &str| leer(t).unwrap_err().to_string();
        assert!(e("z = k x").contains("«k» no está definido"));
        assert!(e("x^2 + y^2 + z^2").contains("falta el «=»"));
        assert!(e("z = x\nr = 2").contains("polares"));
        assert!(e("z = x\n(1, 2)").contains("tres coordenadas"));
        assert!(e("z = x\n(cos t, sin t)").contains("tres coordenadas"));
        assert!(e("(cos t, sin u, 1)").contains("no las dos"));
        assert!(e("(1, 2, 3), (cos t, 1, 1)").contains("sola en su renglón"));
        assert!(e("(1, 2, 3), (1, 2)").contains("misma cantidad"));
        assert!(e("z = x\ndatos:\n0 0\n1 1").contains("tres números"));
        assert!(e("cámara: 30").contains("dos ángulos"));
        assert!(e("cámara: 0, 120\nz = x").contains("elevación"));
        assert!(e("resolución: 4\nz = x").contains("resolución va de 8"));
        assert!(e("resolución: 40\ny = x").contains("3D"));
        assert!(e("y = x ; u: 0..1").contains("superficies"));
        assert!(e("y = u x").contains("u y v son de las superficies"));
        // Con u declarado, es un parámetro.
        assert!(leer("u = 1 (0..2)\ny = u x").is_ok());
        // `z = 2` es un plano, no un parámetro.
        assert!(leer("z = 2").unwrap().es_3d());
    }

    #[test]
    fn solo_polares_lleva_grilla_polar() {
        assert_eq!(leer("r = 2").unwrap().grilla, Grilla::Polar);
        assert_eq!(leer("r = 2\ny = x").unwrap().grilla, Grilla::Cartesiana);
        let d = leer("título: A\ny = x^2").unwrap().descripcion();
        assert!(d.starts_with("Gráfico: A. y = x^2. x de −10 a 10"), "{d}");
    }
}
