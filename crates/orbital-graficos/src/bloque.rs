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
}

impl Grafico {
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
        let mut s = String::from("Gráfico");
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
        || ["x", "y", "r", "t", "θ", "theta", "e", "π", "pi"].contains(&normal.as_str())
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
    for v in [Var::X, Var::Y, Var::T, Var::Theta] {
        if e.usa().var(v) && Some(v) != permitida {
            return err(
                renglon,
                format!("en {que} no vale la variable {}", v.nombre()),
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
        _ => {
            // `f(x)`, `g(x)`, `r(\theta)`, `r(θ)`.
            let (n, arg) = t.split_once('(')?;
            let arg = arg.strip_suffix(')')?;
            if n.is_empty() || !n.chars().all(|c| c.is_ascii_alphabetic()) {
                return None;
            }
            match arg {
                "x" => Some("y"),
                "θ" | "\\theta" | "theta" => Some("r"),
                _ => None,
            }
        }
    }
}

/// Una tupla `(a, b)` o varias `(a, b), (c, d)`.
fn tuplas(s: &str) -> Option<Vec<(&str, &str)>> {
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
        match dividir_nivel_cero(p, ',').as_slice() {
            [a, b] => out.push((*a, *b)),
            _ => return None,
        }
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
    };
    let mut grilla_pedida = false;
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
                    "x" => g.x = rango(valor, &nombres_ref, renglon)?,
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
                        let estilo = match normalizar(valor).as_str() {
                            "" | "puntos" | "dispersion" => Estilo::Puntos,
                            "lineas" | "linea" => Estilo::Linea,
                            "lineas y puntos" | "ambos" => Estilo::LineaYPuntos,
                            otro => {
                                return err(
                                    renglon,
                                    format!("«{otro}»: los datos van como puntos, líneas o ambos"),
                                );
                            }
                        };
                        k = tabla(&renglones, k, estilo, &mut g)?;
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
    if !grilla_pedida && g.series.iter().all(|s| matches!(s.tipo, Tipo::Polar(_))) {
        g.grilla = Grilla::Polar;
    }
    Ok(g)
}

fn serie(fuente: &str, tipo: Tipo) -> Serie {
    Serie {
        fuente: fuente.trim().to_owned(),
        tipo,
        color: None,
        estilo: Estilo::Linea,
        nombre: None,
        grosor: None,
    }
}

/// Una ecuación o expresión. `None`: era un parámetro (queda en `g.params`).
fn ecuacion(
    e: &str,
    params: &[&str],
    g: &mut Grafico,
    renglon: usize,
) -> Result<Option<Serie>, Error> {
    // Puntos o curva paramétrica: `(a, b)`.
    if let Some(ts) = tuplas(e) {
        let mut pares = Vec::new();
        for (a, b) in ts {
            pares.push((
                leer_expr(a, params, renglon)?,
                leer_expr(b, params, renglon)?,
            ));
        }
        let usa_t = pares
            .iter()
            .any(|(a, b)| a.usa().var(Var::T) || b.usa().var(Var::T));
        for (a, b) in &pares {
            for x in [a, b] {
                solo(
                    x,
                    usa_t.then_some(Var::T),
                    "un punto o una curva (x(t), y(t))",
                    renglon,
                )?;
            }
        }
        if usa_t {
            if pares.len() != 1 {
                return err(renglon, "una curva (x(t), y(t)) va sola en su renglón");
            }
            let (a, b) = pares.pop().expect("uno");
            return Ok(Some(serie(e, Tipo::Parametrica(a, b))));
        }
        let mut s = serie(e, Tipo::Puntos(pares));
        s.estilo = Estilo::Puntos;
        return Ok(Some(s));
    }
    // Paramétrica escrita como `x = cos t, y = sin t`.
    if let [a, b] = dividir_nivel_cero(e, ',').as_slice()
        && let (Some((xa, xe)), Some((ya, ye))) = (igual(a), igual(b))
        && lado_simple(xa) == Some("x")
        && lado_simple(ya) == Some("y")
    {
        let fx = leer_expr(xe, params, renglon)?;
        let fy = leer_expr(ye, params, renglon)?;
        for f in [&fx, &fy] {
            solo(f, Some(Var::T), "x = f(t), y = g(t)", renglon)?;
        }
        return Ok(Some(serie(e, Tipo::Parametrica(fx, fy))));
    }
    let Some((izq, der)) = igual(e) else {
        if e.contains(['<', '>', '≤', '≥']) || e.contains("\\le") || e.contains("\\ge") {
            return err(renglon, "las desigualdades todavía no se pueden graficar");
        }
        if dividir_nivel_cero(e, '=').len() > 2 {
            return err(renglon, "un renglón lleva un solo «=»");
        }
        // Una expresión sola: y = f(x), o r = f(θ) si usa θ.
        let f = leer_expr(e, params, renglon)?;
        if f.usa().var(Var::Theta) {
            solo(&f, Some(Var::Theta), "r = f(θ)", renglon)?;
            return Ok(Some(serie(e, Tipo::Polar(f))));
        }
        if f.usa().var(Var::Y) {
            return err(renglon, "falta el «=»: por ejemplo x^2 + y^2 = 1");
        }
        solo(&f, Some(Var::X), "y = f(x)", renglon)?;
        return Ok(Some(serie(e, Tipo::Funcion { f, dominio: None })));
    };
    if dividir_nivel_cero(der, ',').len() > 1 {
        return err(
            renglon,
            "sobra una coma (las curvas se escriben x = f(t), y = g(t))",
        );
    }
    // Un parámetro: `a = 1 (0..5)` o `a = 2`.
    if let Some((nombre, valor, rango_txt)) = parametro(izq, der) {
        {
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
    }
    match lado_simple(izq) {
        Some("y") => {
            let f = leer_expr(der, params, renglon)?;
            if !f.usa().var(Var::Y) {
                solo(&f, Some(Var::X), "y = f(x)", renglon)?;
                return Ok(Some(serie(e, Tipo::Funcion { f, dominio: None })));
            }
        }
        Some("r") => {
            let f = leer_expr(der, params, renglon)?;
            solo(&f, Some(Var::Theta), "r = f(θ)", renglon)?;
            return Ok(Some(serie(e, Tipo::Polar(f))));
        }
        _ => {}
    }
    // Implícita: `L = R` como `L - R = 0`.
    let l = nodo(izq, params, renglon)?;
    let r = nodo(der, params, renglon)?;
    let f = Expr::de_nodo(&Nodo::Bin(Bin::Menos, Box::new(l), Box::new(r))).map_err(|e| Error {
        renglon,
        mensaje: e.0,
    })?;
    if !f.usa().var(Var::X) && !f.usa().var(Var::Y) {
        return err(renglon, "la ecuación no tiene x ni y");
    }
    for v in [Var::T, Var::Theta] {
        if f.usa().var(v) {
            return err(
                renglon,
                format!(
                    "en una ecuación F(x, y) = 0 no vale la variable {}",
                    v.nombre()
                ),
            );
        }
    }
    Ok(Some(serie(e, Tipo::Implicita(f))))
}

/// Una tabla de datos desde el renglón `k` (después de `datos:`). Devuelve el renglón siguiente.
fn tabla(
    renglones: &[&str],
    mut k: usize,
    estilo: Estilo,
    g: &mut Grafico,
) -> Result<usize, Error> {
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
        });
    }
    Ok(k)
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
    fn solo_polares_lleva_grilla_polar() {
        assert_eq!(leer("r = 2").unwrap().grilla, Grilla::Polar);
        assert_eq!(leer("r = 2\ny = x").unwrap().grilla, Grilla::Cartesiana);
        let d = leer("título: A\ny = x^2").unwrap().descripcion();
        assert!(d.starts_with("Gráfico: A. y = x^2. x de −10 a 10"), "{d}");
    }
}
