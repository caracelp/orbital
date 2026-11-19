//! Unidades (051): la tabla de unidades y prefijos, el análisis de expresiones de unidades en las
//! dos formas de siunitx (literal, `km/h`, `kg m s^-2`, y con comandos, `\kilo\metre\per\hour`) y
//! la conversión entre unidades del mismo tipo.
//!
//! La tabla es propia (ver `specs/051-visual-math-editor/research.md`, R3): unas 80 unidades y los
//! 24 prefijos del SI. Cada unidad sabe su dimensión (exponentes de las 7 magnitudes base del SI),
//! su factor a la unidad coherente del SI y, las temperaturas absolutas, su desplazamiento.

/// Exponentes de metro, kilogramo, segundo, ampere, kelvin, mol y candela.
pub type Dimension = [i8; 7];

const UNO: Dimension = [0; 7];
const LONGITUD: Dimension = [1, 0, 0, 0, 0, 0, 0];
const MASA: Dimension = [0, 1, 0, 0, 0, 0, 0];
const TIEMPO: Dimension = [0, 0, 1, 0, 0, 0, 0];
const CORRIENTE: Dimension = [0, 0, 0, 1, 0, 0, 0];
const TEMPERATURA: Dimension = [0, 0, 0, 0, 1, 0, 0];
const CANTIDAD: Dimension = [0, 0, 0, 0, 0, 1, 0];
const LUMINOSIDAD: Dimension = [0, 0, 0, 0, 0, 0, 1];
const FRECUENCIA: Dimension = [0, 0, -1, 0, 0, 0, 0];
const FUERZA: Dimension = [1, 1, -2, 0, 0, 0, 0];
const PRESION: Dimension = [-1, 1, -2, 0, 0, 0, 0];
const ENERGIA: Dimension = [2, 1, -2, 0, 0, 0, 0];
const POTENCIA: Dimension = [2, 1, -3, 0, 0, 0, 0];
const CARGA: Dimension = [0, 0, 1, 1, 0, 0, 0];
const TENSION: Dimension = [2, 1, -3, -1, 0, 0, 0];
const CAPACIDAD: Dimension = [-2, -1, 4, 2, 0, 0, 0];
const RESISTENCIA: Dimension = [2, 1, -3, -2, 0, 0, 0];
const CONDUCTANCIA: Dimension = [-2, -1, 3, 2, 0, 0, 0];
const FLUJO: Dimension = [2, 1, -2, -1, 0, 0, 0];
const INDUCCION: Dimension = [0, 1, -2, -1, 0, 0, 0];
const INDUCTANCIA: Dimension = [2, 1, -2, -2, 0, 0, 0];
const ILUMINANCIA: Dimension = [-2, 0, 0, 0, 0, 0, 1];
const DOSIS: Dimension = [2, 0, -2, 0, 0, 0, 0];
const CATALITICA: Dimension = [0, 0, -1, 0, 0, 1, 0];
const AREA: Dimension = [2, 0, 0, 0, 0, 0, 0];
const VOLUMEN: Dimension = [3, 0, 0, 0, 0, 0, 0];
const VELOCIDAD: Dimension = [1, 0, -1, 0, 0, 0, 0];

/// Una unidad de la tabla.
#[derive(Debug, PartialEq)]
pub struct Unidad {
    /// Cómo se escribe en la forma literal (`m`, `Pa`, `°C`).
    pub simbolo: &'static str,
    /// Otras formas literales que se aceptan (`ohm`, `degC`, `l`).
    pub alias: &'static [&'static str],
    /// El comando de siunitx sin la barra (`metre`), o vacío.
    pub comando: &'static str,
    /// Cómo se dibuja; vacío: `\mathrm{símbolo}`.
    pub tex: &'static str,
    pub nombre: &'static str,
    pub dimension: Dimension,
    /// Cuánto vale en la unidad coherente del SI.
    pub factor: f64,
    /// Para temperaturas absolutas: kelvin = valor × factor + desplazamiento.
    pub desplazamiento: f64,
    /// Admite prefijos del SI.
    pub prefijable: bool,
    /// Va pegada al número, sin espacio (`30°`).
    pub pegada: bool,
}

macro_rules! u {
    ($s:literal, [$($a:literal),*], $c:literal, $t:literal, $n:literal, $d:expr, $f:expr, $p:literal) => {
        Unidad {
            simbolo: $s,
            alias: &[$($a),*],
            comando: $c,
            tex: $t,
            nombre: $n,
            dimension: $d,
            factor: $f,
            desplazamiento: 0.0,
            prefijable: $p,
            pegada: false,
        }
    };
}

const PI: f64 = std::f64::consts::PI;
const LIBRA: f64 = 0.453_592_37;
const PIE: f64 = 0.3048;
const PULGADA: f64 = 0.0254;
const GALON: f64 = 3.785_411_784e-3;
const CALORIA: f64 = 4.184;
const ATMOSFERA: f64 = 101_325.0;

/// La tabla. El orden importa solo para mostrarla: la búsqueda prueba primero el símbolo entero y
/// después prefijo + símbolo.
pub static UNIDADES: &[Unidad] = &[
    // Base del SI.
    u!("m", [], "metre", "", "metro", LONGITUD, 1.0, true),
    u!("g", [], "gram", "", "gramo", MASA, 1e-3, true),
    u!("s", [], "second", "", "segundo", TIEMPO, 1.0, true),
    u!("A", [], "ampere", "", "ampere", CORRIENTE, 1.0, true),
    u!("K", [], "kelvin", "", "kelvin", TEMPERATURA, 1.0, true),
    u!("mol", [], "mole", "", "mol", CANTIDAD, 1.0, true),
    u!("cd", [], "candela", "", "candela", LUMINOSIDAD, 1.0, true),
    // Derivadas con nombre propio.
    u!("Hz", [], "hertz", "", "hertz", FRECUENCIA, 1.0, true),
    u!("N", [], "newton", "", "newton", FUERZA, 1.0, true),
    u!("Pa", [], "pascal", "", "pascal", PRESION, 1.0, true),
    u!("J", [], "joule", "", "joule", ENERGIA, 1.0, true),
    u!("W", [], "watt", "", "watt", POTENCIA, 1.0, true),
    u!("C", [], "coulomb", "", "coulomb", CARGA, 1.0, true),
    u!("V", [], "volt", "", "volt", TENSION, 1.0, true),
    u!("F", [], "farad", "", "farad", CAPACIDAD, 1.0, true),
    u!(
        "Ω",
        ["ohm", "Ohm"],
        "ohm",
        r"\Omega",
        "ohm",
        RESISTENCIA,
        1.0,
        true
    ),
    u!("S", [], "siemens", "", "siemens", CONDUCTANCIA, 1.0, true),
    u!("Wb", [], "weber", "", "weber", FLUJO, 1.0, true),
    u!("T", [], "tesla", "", "tesla", INDUCCION, 1.0, true),
    u!("H", [], "henry", "", "henry", INDUCTANCIA, 1.0, true),
    u!("lm", [], "lumen", "", "lumen", LUMINOSIDAD, 1.0, true),
    u!("lx", [], "lux", "", "lux", ILUMINANCIA, 1.0, true),
    u!(
        "Bq",
        [],
        "becquerel",
        "",
        "becquerel",
        FRECUENCIA,
        1.0,
        true
    ),
    u!("Gy", [], "gray", "", "gray", DOSIS, 1.0, true),
    u!("Sv", [], "sievert", "", "sievert", DOSIS, 1.0, true),
    u!("kat", [], "katal", "", "katal", CATALITICA, 1.0, true),
    u!("rad", [], "radian", "", "radián", UNO, 1.0, true),
    u!("sr", [], "steradian", "", "estereorradián", UNO, 1.0, true),
    Unidad {
        simbolo: "°C",
        alias: &["degC", "ºC", "℃"],
        comando: "degreeCelsius",
        tex: r"{}^{\circ}\mathrm{C}",
        nombre: "grado Celsius",
        dimension: TEMPERATURA,
        factor: 1.0,
        desplazamiento: 273.15,
        prefijable: false,
        pegada: false,
    },
    // Aceptadas junto al SI.
    u!("min", [], "minute", "", "minuto", TIEMPO, 60.0, false),
    u!("h", [], "hour", "", "hora", TIEMPO, 3600.0, false),
    u!("d", [], "day", "", "día", TIEMPO, 86_400.0, false),
    u!("L", ["l"], "litre", "", "litro", VOLUMEN, 1e-3, true),
    u!("t", [], "tonne", "", "tonelada", MASA, 1e3, true),
    u!("ha", [], "hectare", "", "hectárea", AREA, 1e4, false),
    u!(
        "au",
        [],
        "astronomicalunit",
        "",
        "unidad astronómica",
        LONGITUD,
        1.495_978_707e11,
        false
    ),
    u!(
        "eV",
        [],
        "electronvolt",
        "",
        "electronvolt",
        ENERGIA,
        1.602_176_634e-19,
        true
    ),
    u!(
        "Da",
        [],
        "dalton",
        "",
        "dalton",
        MASA,
        1.660_539_066_6e-27,
        true
    ),
    u!("bar", [], "bar", "", "bar", PRESION, 1e5, true),
    u!(
        "Å",
        ["angstrom"],
        "angstrom",
        r"\mathring{\mathrm{A}}",
        "ångström",
        LONGITUD,
        1e-10,
        false
    ),
    Unidad {
        simbolo: "°",
        alias: &["deg", "º"],
        comando: "degree",
        tex: r"^{\circ}",
        nombre: "grado",
        dimension: UNO,
        factor: PI / 180.0,
        desplazamiento: 0.0,
        prefijable: false,
        pegada: true,
    },
    Unidad {
        simbolo: "'",
        alias: &["′", "arcmin"],
        comando: "arcminute",
        tex: r"^{\prime}",
        nombre: "minuto de arco",
        dimension: UNO,
        factor: PI / 10_800.0,
        desplazamiento: 0.0,
        prefijable: false,
        pegada: true,
    },
    Unidad {
        simbolo: "\"",
        alias: &["″", "arcsec"],
        comando: "arcsecond",
        tex: r"^{\prime\prime}",
        nombre: "segundo de arco",
        dimension: UNO,
        factor: PI / 648_000.0,
        desplazamiento: 0.0,
        prefijable: false,
        pegada: true,
    },
    u!("%", [], "percent", r"\%", "por ciento", UNO, 0.01, false),
    u!("Wh", [], "", "", "watt hora", ENERGIA, 3600.0, true),
    u!("cal", [], "", "", "caloría", ENERGIA, CALORIA, true),
    u!("atm", [], "", "", "atmósfera", PRESION, ATMOSFERA, false),
    u!(
        "mmHg",
        [],
        "",
        "",
        "milímetro de mercurio",
        PRESION,
        133.322_387_415,
        false
    ),
    u!(
        "Torr",
        [],
        "",
        "",
        "torr",
        PRESION,
        ATMOSFERA / 760.0,
        false
    ),
    u!(
        "rpm",
        [],
        "",
        "",
        "revoluciones por minuto",
        FRECUENCIA,
        1.0 / 60.0,
        false
    ),
    // Anglosajonas y otras comunes.
    u!("in", [], "", "", "pulgada", LONGITUD, PULGADA, false),
    u!("ft", [], "", "", "pie", LONGITUD, PIE, false),
    u!("yd", [], "", "", "yarda", LONGITUD, 3.0 * PIE, false),
    u!("mi", [], "", "", "milla", LONGITUD, 1609.344, false),
    u!("nmi", [], "", "", "milla náutica", LONGITUD, 1852.0, false),
    u!("lb", [], "", "", "libra", MASA, LIBRA, false),
    u!("oz", [], "", "", "onza", MASA, LIBRA / 16.0, false),
    u!(
        "mph",
        [],
        "",
        "",
        "milla por hora",
        VELOCIDAD,
        1609.344 / 3600.0,
        false
    ),
    u!(
        "kn",
        ["kt"],
        "",
        "",
        "nudo",
        VELOCIDAD,
        1852.0 / 3600.0,
        false
    ),
    u!(
        "psi",
        [],
        "",
        "",
        "libra por pulgada cuadrada",
        PRESION,
        LIBRA * 9.806_65 / (PULGADA * PULGADA),
        false
    ),
    u!("gal", [], "", "", "galón (EE. UU.)", VOLUMEN, GALON, false),
    u!("acre", [], "", "", "acre", AREA, 4_046.856_422_4, false),
    u!("BTU", [], "", "", "BTU", ENERGIA, 1_055.055_852_62, false),
    u!(
        "hp",
        [],
        "",
        "",
        "caballo de fuerza",
        POTENCIA,
        745.699_871_582,
        false
    ),
    u!(
        "cv",
        [],
        "",
        "",
        "caballo de vapor",
        POTENCIA,
        735.498_75,
        false
    ),
    u!(
        "kgf",
        [],
        "",
        "",
        "kilogramo fuerza",
        FUERZA,
        9.806_65,
        false
    ),
    u!("dyn", [], "", "", "dina", FUERZA, 1e-5, false),
    u!("erg", [], "", "", "ergio", ENERGIA, 1e-7, false),
    u!(
        "ly",
        [],
        "",
        "",
        "año luz",
        LONGITUD,
        9.460_730_472_580_8e15,
        false
    ),
    u!(
        "pc",
        [],
        "",
        "",
        "pársec",
        LONGITUD,
        3.085_677_581_491_367e16,
        true
    ),
    Unidad {
        simbolo: "°F",
        alias: &["degF", "ºF", "℉"],
        comando: "",
        tex: r"{}^{\circ}\mathrm{F}",
        nombre: "grado Fahrenheit",
        dimension: TEMPERATURA,
        factor: 5.0 / 9.0,
        desplazamiento: 459.67 * 5.0 / 9.0,
        prefijable: false,
        pegada: false,
    },
    // siunitx también trae estas.
    u!("B", [], "bel", "", "bel", UNO, 1.0, true),
    u!("Np", [], "neper", "", "neper", UNO, 1.0, true),
];

/// Un prefijo del SI.
#[derive(Debug, PartialEq)]
pub struct Prefijo {
    pub simbolo: &'static str,
    pub comando: &'static str,
    /// Cómo se dibuja; vacío: el símbolo en `\mathrm`.
    pub tex: &'static str,
    pub exponente: i8,
}

macro_rules! p {
    ($s:literal, $c:literal, $e:literal) => {
        Prefijo {
            simbolo: $s,
            comando: $c,
            tex: "",
            exponente: $e,
        }
    };
}

pub static PREFIJOS: &[Prefijo] = &[
    p!("Q", "quetta", 30),
    p!("R", "ronna", 27),
    p!("Y", "yotta", 24),
    p!("Z", "zetta", 21),
    p!("E", "exa", 18),
    p!("P", "peta", 15),
    p!("T", "tera", 12),
    p!("G", "giga", 9),
    p!("M", "mega", 6),
    p!("k", "kilo", 3),
    p!("h", "hecto", 2),
    p!("da", "deca", 1),
    p!("d", "deci", -1),
    p!("c", "centi", -2),
    p!("m", "milli", -3),
    Prefijo {
        simbolo: "µ",
        comando: "micro",
        tex: r"\text{µ}",
        exponente: -6,
    },
    p!("n", "nano", -9),
    p!("p", "pico", -12),
    p!("f", "femto", -15),
    p!("a", "atto", -18),
    p!("z", "zepto", -21),
    p!("y", "yocto", -24),
    p!("r", "ronto", -27),
    p!("q", "quecto", -30),
];

/// `deka` es otra grafía de `deca`; `u` y `μ` (mu griega) valen por `µ`.
fn prefijo_literal(s: &str) -> Option<&'static Prefijo> {
    let s = match s {
        "u" | "μ" => "µ",
        otro => otro,
    };
    PREFIJOS.iter().find(|p| p.simbolo == s)
}

fn prefijo_comando(c: &str) -> Option<&'static Prefijo> {
    let c = if c == "deka" { "deca" } else { c };
    PREFIJOS.iter().find(|p| p.comando == c)
}

fn unidad_literal(s: &str) -> Option<&'static Unidad> {
    UNIDADES
        .iter()
        .find(|u| u.simbolo == s || u.alias.contains(&s))
}

fn unidad_comando(c: &str) -> Option<&'static Unidad> {
    let c = match c {
        "meter" => "metre",
        "liter" => "litre",
        "celsius" => "degreeCelsius",
        otro => otro,
    };
    UNIDADES
        .iter()
        .find(|u| !u.comando.is_empty() && u.comando == c)
}

/// Un símbolo literal (`km`, `Pa`, `µs`): la unidad entera primero, después prefijo + unidad.
pub fn buscar_simbolo(s: &str) -> Option<(Option<&'static Prefijo>, &'static Unidad)> {
    if let Some(u) = unidad_literal(s) {
        return Some((None, u));
    }
    let mut cortes: Vec<usize> = s.char_indices().map(|(i, _)| i).skip(1).collect();
    // `da` antes que `d`: el prefijo más largo primero.
    cortes.sort_unstable_by(|a, b| b.cmp(a));
    cortes.into_iter().find_map(|i| {
        let (p, resto) = s.split_at(i);
        let p = prefijo_literal(p)?;
        let u = unidad_literal(resto).filter(|u| u.prefijable)?;
        Some((Some(p), u))
    })
}

/// Un factor de una expresión de unidades: prefijo, unidad y exponente.
#[derive(Clone, Debug, PartialEq)]
pub struct Factor {
    pub prefijo: Option<&'static Prefijo>,
    pub unidad: Option<&'static Unidad>,
    /// El texto tal como vino, si no es una unidad conocida.
    pub crudo: String,
    pub exponente: i32,
}

/// Una pieza de una expresión, para dibujarla como se escribió.
#[derive(Clone, Debug, PartialEq)]
pub enum Pieza {
    Factor(Factor),
    /// Producto entre unidades (espacio, `.`, `*`, `·`, `~`).
    Producto,
    /// `/` literal.
    Barra,
    Abre,
    Cierra,
}

/// Una unidad desconocida o una expresión mal formada.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ErrorUnidad(pub String);

impl std::fmt::Display for ErrorUnidad {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// Lee un argumento entre llaves (`{-1}`) o un número suelto (`-1`, `2`) desde `i`.
fn leer_entero(s: &str, i: &mut usize) -> Option<i32> {
    let b = s.as_bytes();
    while *i < b.len() && b[*i] == b' ' {
        *i += 1;
    }
    let llave = b.get(*i) == Some(&b'{');
    if llave {
        *i += 1;
    }
    let inicio = *i;
    if matches!(b.get(*i), Some(b'-' | b'+')) {
        *i += 1;
    }
    while *i < b.len() && b[*i].is_ascii_digit() {
        *i += 1;
    }
    let n = s[inicio..*i].parse().ok();
    if llave {
        while *i < b.len() && b[*i] == b' ' {
            *i += 1;
        }
        if b.get(*i) == Some(&b'}') {
            *i += 1;
        } else {
            return None;
        }
    }
    n
}

/// Superíndices Unicode (`m²`, `s⁻¹`) como exponente.
fn superindice(s: &str, i: &mut usize) -> Option<i32> {
    let mut n = String::new();
    for c in s[*i..].chars() {
        let d = match c {
            '⁻' => '-',
            '⁰' => '0',
            '¹' => '1',
            '²' => '2',
            '³' => '3',
            '⁴' => '4',
            '⁵' => '5',
            '⁶' => '6',
            '⁷' => '7',
            '⁸' => '8',
            '⁹' => '9',
            _ => break,
        };
        n.push(d);
        *i += c.len_utf8();
    }
    n.parse().ok()
}

fn es_letra_unidad(c: char) -> bool {
    c.is_alphabetic()
        || matches!(
            c,
            '°' | 'º' | '%' | '\'' | '"' | '′' | '″' | 'Å' | '℃' | '℉'
        )
}

/// Analiza una expresión de unidades, literal o con comandos de siunitx (se pueden mezclar).
/// Las unidades desconocidas quedan como texto (`Factor::crudo`); un comando que no es de
/// unidades es un error.
pub fn piezas(s: &str) -> Result<Vec<Pieza>, ErrorUnidad> {
    let mut out: Vec<Pieza> = Vec::new();
    let b = s.as_bytes();
    let mut i = 0;
    // Estado de los comandos que modifican la unidad siguiente.
    let mut prefijo: Option<&'static Prefijo> = None;
    let mut por = false;
    let mut potencia: Option<i32> = None;
    let mut producto_pendiente = false;
    fn empujar(out: &mut Vec<Pieza>, f: Factor, producto: &mut bool) {
        if *producto && matches!(out.last(), Some(Pieza::Factor(_) | Pieza::Cierra)) {
            out.push(Pieza::Producto);
        }
        *producto = false;
        out.push(Pieza::Factor(f));
    }
    fn ultimo_factor(out: &mut [Pieza]) -> Option<&mut Factor> {
        match out.last_mut() {
            Some(Pieza::Factor(f)) => Some(f),
            _ => None,
        }
    }
    while i < b.len() {
        let c = s[i..].chars().next().unwrap_or(' ');
        match c {
            ' ' | '.' | '*' | '·' | '~' | '\t' | '\n' => {
                producto_pendiente = true;
                i += c.len_utf8();
            }
            '/' => {
                out.push(Pieza::Barra);
                producto_pendiente = false;
                i += 1;
            }
            '(' => {
                if producto_pendiente
                    && matches!(out.last(), Some(Pieza::Factor(_) | Pieza::Cierra))
                {
                    out.push(Pieza::Producto);
                }
                producto_pendiente = false;
                out.push(Pieza::Abre);
                i += 1;
            }
            ')' => {
                out.push(Pieza::Cierra);
                i += 1;
            }
            '^' => {
                i += 1;
                let n = leer_entero(s, &mut i)
                    .ok_or_else(|| ErrorUnidad("falta el número del exponente".into()))?;
                match out.last_mut() {
                    Some(Pieza::Factor(f)) => f.exponente *= n,
                    Some(Pieza::Cierra) => {
                        return Err(ErrorUnidad(
                            "un exponente sobre paréntesis no se puede convertir".into(),
                        ));
                    }
                    _ => return Err(ErrorUnidad("exponente sin unidad".into())),
                }
            }
            '⁻' | '⁰' | '¹' | '²' | '³' | '⁴' | '⁵' | '⁶' | '⁷' | '⁸' | '⁹' => {
                let n = superindice(s, &mut i)
                    .ok_or_else(|| ErrorUnidad("exponente mal escrito".into()))?;
                if let Some(f) = ultimo_factor(&mut out) {
                    f.exponente *= n;
                }
            }
            '\\' => {
                let inicio = i + 1;
                let mut fin = inicio;
                while fin < b.len() && b[fin].is_ascii_alphabetic() {
                    fin += 1;
                }
                let cmd = &s[inicio..fin];
                i = fin;
                if cmd.is_empty() {
                    // `\,`, `\ `, `\;`: espacios.
                    if i < b.len() {
                        i += s[i..].chars().next().map_or(1, char::len_utf8);
                    }
                    producto_pendiente = true;
                    continue;
                }
                match cmd {
                    "per" => por = true,
                    "square" => potencia = Some(2),
                    "cubic" => potencia = Some(3),
                    "raiseto" => {
                        potencia =
                            Some(leer_entero(s, &mut i).ok_or_else(|| {
                                ErrorUnidad("falta el exponente de \\raiseto".into())
                            })?)
                    }
                    "squared" | "cubed" | "tothe" => {
                        let n = match cmd {
                            "squared" => 2,
                            "cubed" => 3,
                            _ => leer_entero(s, &mut i).ok_or_else(|| {
                                ErrorUnidad("falta el exponente de \\tothe".into())
                            })?,
                        };
                        if let Some(f) = ultimo_factor(&mut out) {
                            f.exponente *= n;
                        }
                    }
                    "cdot" | "times" => producto_pendiente = true,
                    "of" => {
                        // `\of{…}`: un calificador que no cambia la unidad; se saltea.
                        let mut nivel = 0;
                        while i < b.len() {
                            match b[i] {
                                b'{' => nivel += 1,
                                b'}' => {
                                    nivel -= 1;
                                    if nivel <= 0 {
                                        i += 1;
                                        break;
                                    }
                                }
                                _ => {}
                            }
                            i += 1;
                        }
                    }
                    _ => {
                        if let Some(p) = prefijo_comando(cmd) {
                            prefijo = Some(p);
                        } else if let Some(u) = unidad_comando(cmd) {
                            let mut exp = potencia.take().unwrap_or(1);
                            if std::mem::take(&mut por) {
                                exp = -exp;
                            }
                            producto_pendiente = true;
                            empujar(
                                &mut out,
                                Factor {
                                    prefijo: prefijo.take(),
                                    unidad: Some(u),
                                    crudo: String::new(),
                                    exponente: exp,
                                },
                                &mut producto_pendiente,
                            );
                            producto_pendiente = true;
                        } else {
                            return Err(ErrorUnidad(format!("\\{cmd} no es una unidad")));
                        }
                    }
                }
            }
            c if es_letra_unidad(c) => {
                let inicio = i;
                // `°C`: el grado va pegado a la letra; `'` y `"` (arco) van solos.
                let solo = matches!(c, '\'' | '"' | '′' | '″' | '%');
                i += c.len_utf8();
                if !solo {
                    for c2 in s[i..].chars() {
                        if c2.is_alphabetic()
                            || (c == '°' && i == inicio + c.len_utf8() && c2.is_alphabetic())
                        {
                            i += c2.len_utf8();
                        } else {
                            break;
                        }
                    }
                }
                let palabra = &s[inicio..i];
                // `m2`, `s-1`: un entero pegado es el exponente.
                let mut exp = 1;
                let resto = &s[i..];
                let digitos = resto
                    .char_indices()
                    .take_while(|(k, ch)| ch.is_ascii_digit() || (*k == 0 && *ch == '-'))
                    .last()
                    .map(|(k, ch)| k + ch.len_utf8());
                if let Some(n) = digitos
                    && let Ok(v) = resto[..n].parse::<i32>()
                {
                    exp = v;
                    i += n;
                }
                let (p, u) = match buscar_simbolo(palabra) {
                    Some((p, u)) => (p, Some(u)),
                    None => (None, None),
                };
                let mut f = Factor {
                    prefijo: p,
                    unidad: u,
                    crudo: if u.is_none() {
                        palabra.to_owned()
                    } else {
                        String::new()
                    },
                    exponente: exp,
                };
                if let Some(pr) = prefijo.take()
                    && f.prefijo.is_none()
                {
                    f.prefijo = Some(pr);
                }
                if std::mem::take(&mut por) {
                    f.exponente = -f.exponente;
                }
                if let Some(n) = potencia.take() {
                    f.exponente *= n;
                }
                empujar(&mut out, f, &mut producto_pendiente);
            }
            _ => {
                return Err(ErrorUnidad(format!("«{c}» no va en una unidad")));
            }
        }
    }
    Ok(out)
}

/// Una expresión de unidades reducida: factor y dimensión (y desplazamiento, si es una
/// temperatura absoluta sola).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Compuesta {
    pub factor: f64,
    pub dimension: Dimension,
    pub desplazamiento: f64,
}

/// Factor y dimensión de una expresión. `*` y `/` encadenan de izquierda a derecha; el producto
/// por espacio liga más fuerte (`J/mol K` es `J/(mol K)`, como en la notación habitual).
pub fn analizar(s: &str) -> Result<Compuesta, ErrorUnidad> {
    let ps = piezas(s)?;
    if ps.is_empty() {
        return Err(ErrorUnidad("falta la unidad".into()));
    }
    let mut i = 0;
    let c = expresion(&ps, &mut i)?;
    if i != ps.len() {
        return Err(ErrorUnidad("sobra un paréntesis".into()));
    }
    // Una temperatura absoluta sola (°C, °F) se convierte con su desplazamiento; dentro de un
    // compuesto (`J/°C`) cuenta como diferencia.
    let solo = match ps.as_slice() {
        [Pieza::Factor(f)] if f.exponente == 1 && f.prefijo.is_none() => f.unidad,
        _ => None,
    };
    Ok(Compuesta {
        desplazamiento: solo.map_or(0.0, |u| u.desplazamiento),
        ..c
    })
}

fn combinar(a: Compuesta, b: Compuesta, signo: i8) -> Compuesta {
    let mut d = a.dimension;
    for (x, y) in d.iter_mut().zip(b.dimension) {
        *x += signo * y;
    }
    Compuesta {
        factor: if signo > 0 {
            a.factor * b.factor
        } else {
            a.factor / b.factor
        },
        dimension: d,
        desplazamiento: 0.0,
    }
}

const NEUTRA: Compuesta = Compuesta {
    factor: 1.0,
    dimension: UNO,
    desplazamiento: 0.0,
};

fn expresion(ps: &[Pieza], i: &mut usize) -> Result<Compuesta, ErrorUnidad> {
    let mut acc = termino(ps, i)?;
    while let Some(Pieza::Barra) = ps.get(*i) {
        *i += 1;
        let t = termino(ps, i)?;
        acc = combinar(acc, t, -1);
    }
    Ok(acc)
}

fn termino(ps: &[Pieza], i: &mut usize) -> Result<Compuesta, ErrorUnidad> {
    let mut acc = NEUTRA;
    let mut alguno = false;
    loop {
        match ps.get(*i) {
            Some(Pieza::Factor(f)) => {
                *i += 1;
                let Some(u) = f.unidad else {
                    return Err(ErrorUnidad(format!("no conozco la unidad «{}»", f.crudo)));
                };
                let base = u.factor * 10f64.powi(f.prefijo.map_or(0, |p| i32::from(p.exponente)));
                let mut d = UNO;
                for (x, y) in d.iter_mut().zip(u.dimension) {
                    *x = y * f.exponente as i8;
                }
                acc = combinar(
                    acc,
                    Compuesta {
                        factor: base.powi(f.exponente),
                        dimension: d,
                        desplazamiento: 0.0,
                    },
                    1,
                );
                alguno = true;
            }
            Some(Pieza::Producto) => *i += 1,
            Some(Pieza::Abre) => {
                *i += 1;
                let dentro = expresion(ps, i)?;
                if ps.get(*i) != Some(&Pieza::Cierra) {
                    return Err(ErrorUnidad("falta cerrar un paréntesis".into()));
                }
                *i += 1;
                acc = combinar(acc, dentro, 1);
                alguno = true;
            }
            _ => break,
        }
    }
    if alguno {
        Ok(acc)
    } else {
        Err(ErrorUnidad("falta una unidad".into()))
    }
}

/// Convierte `valor` de la unidad `desde` a la unidad `hacia`.
pub fn convertir(valor: f64, desde: &str, hacia: &str) -> Result<f64, ErrorUnidad> {
    let a = analizar(desde)?;
    let b = analizar(hacia)?;
    if a.dimension != b.dimension {
        return Err(ErrorUnidad(format!(
            "{} y {} no miden lo mismo",
            desde.trim(),
            hacia.trim()
        )));
    }
    let si = valor * a.factor + a.desplazamiento;
    Ok((si - b.desplazamiento) / b.factor)
}

/// Unidades destino frecuentes; [`sugerencias`] da las del mismo tipo que una unidad.
const FRECUENTES: &[&str] = &[
    "m/s", "km/h", "mph", "kn", "m", "km", "cm", "mm", "µm", "nm", "in", "ft", "yd", "mi", "nmi",
    "au", "ly", "kg", "g", "mg", "t", "lb", "oz", "s", "ms", "min", "h", "d", "K", "°C", "°F",
    "Pa", "kPa", "MPa", "hPa", "bar", "atm", "psi", "mmHg", "J", "kJ", "MJ", "cal", "kcal", "Wh",
    "kWh", "eV", "BTU", "W", "kW", "MW", "hp", "cv", "N", "kN", "kgf", "m^2", "cm^2", "km^2", "ha",
    "acre", "L", "mL", "m^3", "cm^3", "gal", "kg/m^3", "g/cm^3", "g/L", "Hz", "kHz", "rpm", "rad",
    "°", "m/s^2", "N m", "A", "mA", "V", "kV", "Ω", "kΩ", "C", "mol", "mol/L",
];

/// Las unidades frecuentes que miden lo mismo que `desde` (sin ella misma).
pub fn sugerencias(desde: &str) -> Vec<&'static str> {
    let Ok(a) = analizar(desde) else {
        return Vec::new();
    };
    let propia = desde.trim();
    FRECUENTES
        .iter()
        .copied()
        .filter(|s| *s != propia)
        .filter(|s| analizar(s).is_ok_and(|b| b.dimension == a.dimension))
        .collect()
}

/// Un número para `\qty`/`\num`: 6 cifras significativas, sin ceros de más, con exponente si es
/// muy grande o muy chico (`1.5e-7`).
pub fn formatear(v: f64) -> String {
    if !v.is_finite() {
        return "0".into();
    }
    if v == 0.0 {
        return "0".into();
    }
    let a = v.abs();
    if !(1e-4..1e7).contains(&a) {
        let s = format!("{v:.5e}");
        let (m, e) = s.split_once('e').unwrap_or((&s, "0"));
        let m = recortar(m);
        return format!("{m}e{e}");
    }
    let cifras = 6 - (a.log10().floor() as i32 + 1);
    let dec = cifras.clamp(0, 10) as usize;
    recortar(&format!("{v:.dec$}")).to_owned()
}

fn recortar(s: &str) -> &str {
    if s.contains('.') {
        s.trim_end_matches('0').trim_end_matches('.')
    } else {
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cerca(a: f64, b: f64) -> bool {
        (a - b).abs() <= 1e-9 * a.abs().max(b.abs()).max(1.0)
    }

    #[test]
    fn convierte_compuestas() {
        assert!(cerca(convertir(90.0, "km/h", "m/s").unwrap(), 25.0));
        assert!(cerca(
            convertir(90.0, r"\kilo\metre\per\hour", "m s^-1").unwrap(),
            25.0
        ));
        assert!(cerca(convertir(1.0, "kWh", "MJ").unwrap(), 3.6));
        assert!(cerca(convertir(1.0, "atm", "kPa").unwrap(), 101.325));
        assert!(cerca(convertir(1.0, "N", "kg m/s^2").unwrap(), 1.0));
        assert!(cerca(convertir(1.0, "J/(mol K)", "J/mol K").unwrap(), 1.0));
        assert!(cerca(convertir(1.0, "g/cm^3", "kg/m^3").unwrap(), 1000.0));
        assert!(cerca(convertir(2.0, "m²", "cm^2").unwrap(), 20_000.0));
        assert!(cerca(convertir(1.0, "mi", "km").unwrap(), 1.609_344));
        assert!(cerca(convertir(1.0, "lb", "g").unwrap(), 453.592_37));
        assert!(cerca(convertir(180.0, "°", "rad").unwrap(), PI));
        assert!(cerca(convertir(1.0, "µs", "ns").unwrap(), 1000.0));
        assert!(cerca(convertir(1.0, "us", "ns").unwrap(), 1000.0));
        assert!(cerca(convertir(1.0, "L", "cm^3").unwrap(), 1000.0));
        assert!(cerca(convertir(1.0, "kΩ", "ohm").unwrap(), 1000.0));
        assert!(cerca(convertir(60.0, "rpm", "Hz").unwrap(), 1.0));
        assert!(cerca(convertir(1.0, "m s-1", "km/h").unwrap(), 3.6));
        assert!(cerca(
            convertir(1.0, r"\kilo\gram\metre\per\square\second", "N").unwrap(),
            1.0
        ));
    }

    #[test]
    fn temperaturas_con_desplazamiento() {
        assert!(cerca(convertir(100.0, "°C", "°F").unwrap(), 212.0));
        assert!(cerca(convertir(0.0, "°C", "K").unwrap(), 273.15));
        assert!(cerca(convertir(32.0, "degF", "degC").unwrap(), 0.0));
        assert!(cerca(
            convertir(25.0, r"\degreeCelsius", r"\kelvin").unwrap(),
            298.15
        ));
        // Dentro de un compuesto es una diferencia.
        assert!(cerca(convertir(1.0, "J/°C", "J/K").unwrap(), 1.0));
    }

    #[test]
    fn errores_claros() {
        assert!(
            convertir(1.0, "m", "s")
                .unwrap_err()
                .0
                .contains("no miden lo mismo")
        );
        assert!(
            convertir(1.0, "furlong", "m")
                .unwrap_err()
                .0
                .contains("furlong")
        );
        assert!(analizar(r"\frac").is_err());
        assert!(analizar("(m").is_err());
        assert!(analizar("").is_err());
    }

    #[test]
    fn simbolos_ambiguos() {
        // El símbolo entero gana: `min` es minuto, `ft` es pie, `cd` candela, `Pa` pascal.
        for (s, u) in [
            ("min", "min"),
            ("ft", "ft"),
            ("cd", "cd"),
            ("Pa", "Pa"),
            ("h", "h"),
        ] {
            assert_eq!(buscar_simbolo(s).unwrap().1.simbolo, u);
        }
        let (p, u) = buscar_simbolo("dam").unwrap();
        assert_eq!((p.unwrap().simbolo, u.simbolo), ("da", "m"));
        let (p, u) = buscar_simbolo("mm").unwrap();
        assert_eq!((p.unwrap().simbolo, u.simbolo), ("m", "m"));
        // Las que no admiten prefijo.
        assert!(buscar_simbolo("kmin").is_none());
    }

    #[test]
    fn sugiere_del_mismo_tipo() {
        let s = sugerencias("km/h");
        assert!(s.contains(&"m/s") && s.contains(&"mph") && !s.contains(&"km/h"));
        assert!(!s.contains(&"m"));
        assert!(sugerencias("°C").contains(&"°F"));
        assert!(sugerencias("nada").is_empty());
    }

    #[test]
    fn formatea_numeros() {
        assert_eq!(formatear(25.0), "25");
        assert_eq!(formatear(3.6), "3.6");
        assert_eq!(formatear(1.0 / 3.0), "0.333333");
        assert_eq!(formatear(1609.344), "1609.34");
        assert_eq!(formatear(6.022e23), "6.022e23");
        assert_eq!(formatear(1.5e-7), "1.5e-7");
        assert_eq!(formatear(-40.0), "-40");
        assert_eq!(formatear(0.0), "0");
    }
}
