//! Lo que va detrás y alrededor de las series, en píxeles: la grilla (cartesiana o polar), los
//! ejes, los rótulos de las marcas, el título y la leyenda. Lo dibujan igual el widget de GTK y
//! el SVG.

use crate::bloque::{Grafico, Grilla};
use crate::ejes::{self, Vista};

/// Dónde se apoya un rótulo respecto de su punto.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ancla {
    /// Centrado, con el borde de arriba en el punto (marcas del eje x).
    Arriba,
    /// Centrado, con el borde de abajo en el punto.
    Abajo,
    /// A la izquierda del punto, centrado en alto (marcas del eje y).
    Derecha,
    /// A la derecha del punto, centrado en alto.
    Izquierda,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Rotulo {
    pub texto: String,
    pub x: f64,
    pub y: f64,
    pub ancla: Ancla,
}

/// Las guías de una vista.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Guias {
    /// Líneas de la grilla: (x0, y0, x1, y1).
    pub grilla: Vec<[f64; 4]>,
    /// Círculos de la grilla polar: (cx, cy, r).
    pub circulos: Vec<[f64; 3]>,
    /// Los ejes que se ven: (x0, y0, x1, y1).
    pub ejes: Vec<[f64; 4]>,
    pub rotulos: Vec<Rotulo>,
}

/// Separación buscada entre marcas, en píxeles.
const SEPARACION: f64 = 90.0;
/// Margen de los rótulos al borde.
const MARGEN: f64 = 4.0;

pub fn guias(g: &Grafico, v: &Vista) -> Guias {
    let mut out = Guias::default();
    let px = ejes::paso(v.x0, v.x1, v.ancho, SEPARACION);
    let py = ejes::paso(v.y0, v.y1, v.alto, SEPARACION * 0.7);
    let (ox, oy) = v.px(0.0, 0.0);
    match g.grilla {
        Grilla::No => {}
        Grilla::Cartesiana => {
            for x in ejes::marcas(v.x0, v.x1, px) {
                let (sx, _) = v.px(x, 0.0);
                out.grilla.push([sx, 0.0, sx, v.alto]);
            }
            for y in ejes::marcas(v.y0, v.y1, py) {
                let (_, sy) = v.px(0.0, y);
                out.grilla.push([0.0, sy, v.ancho, sy]);
            }
        }
        Grilla::Polar => {
            // Radios redondos hasta la esquina más lejana del origen.
            let lejos = [(v.x0, v.y0), (v.x0, v.y1), (v.x1, v.y0), (v.x1, v.y1)]
                .iter()
                .map(|(x, y)| x.hypot(*y))
                .fold(0.0f64, f64::max);
            let cerca = if (v.x0..=v.x1).contains(&0.0) && (v.y0..=v.y1).contains(&0.0) {
                0.0
            } else {
                [(v.x0, v.y0), (v.x0, v.y1), (v.x1, v.y0), (v.x1, v.y1)]
                    .iter()
                    .map(|(x, y)| x.hypot(*y))
                    .fold(f64::INFINITY, f64::min)
                    .min(v.x0.abs().min(v.x1.abs()).max(v.y0.abs().min(v.y1.abs())))
            };
            let paso_r = px;
            let (ux, _) = v.por_pixel();
            let mut k = (cerca / paso_r).floor().max(1.0);
            let mut n = 0;
            while k * paso_r <= lejos && n < 120 {
                out.circulos.push([ox, oy, k * paso_r / ux]);
                k += 1.0;
                n += 1;
            }
            let largo = (lejos / ux) * 1.5 + v.ancho + v.alto;
            for a in 0..12 {
                if a % 3 == 0 {
                    continue; // los ejes
                }
                let ang = f64::from(a) * std::f64::consts::PI / 6.0;
                out.grilla
                    .push([ox, oy, ox + largo * ang.cos(), oy - largo * ang.sin()]);
            }
            if !g.ejes {
                out.grilla.push([ox - largo, oy, ox + largo, oy]);
                out.grilla.push([ox, oy - largo, ox, oy + largo]);
            }
        }
    }
    if !g.ejes {
        return out;
    }
    // Ejes: si el origen está fuera de la vista, los rótulos van en el borde.
    let eje_x_visible = (0.0..=v.alto).contains(&oy);
    let eje_y_visible = (0.0..=v.ancho).contains(&ox);
    if eje_x_visible {
        out.ejes.push([0.0, oy, v.ancho, oy]);
    }
    if eje_y_visible {
        out.ejes.push([ox, 0.0, ox, v.alto]);
    }
    let fila_x = oy.clamp(MARGEN, v.alto - 18.0);
    let col_y = ox.clamp(36.0, v.ancho - MARGEN);
    for x in ejes::marcas(v.x0, v.x1, px) {
        if x.abs() < px * 1e-6 {
            continue;
        }
        let (sx, _) = v.px(x, 0.0);
        if sx < 12.0 || sx > v.ancho - 12.0 {
            continue;
        }
        out.rotulos.push(Rotulo {
            texto: ejes::rotulo(x, px),
            x: sx,
            y: fila_x + MARGEN,
            ancla: Ancla::Arriba,
        });
    }
    for y in ejes::marcas(v.y0, v.y1, py) {
        if y.abs() < py * 1e-6 {
            continue;
        }
        let (_, sy) = v.px(0.0, y);
        if sy < 10.0 || sy > v.alto - 10.0 {
            continue;
        }
        out.rotulos.push(Rotulo {
            texto: ejes::rotulo(y, py),
            x: col_y - MARGEN,
            y: sy,
            ancla: Ancla::Derecha,
        });
    }
    if eje_x_visible && eje_y_visible && oy < v.alto - 20.0 && ox > 20.0 {
        out.rotulos.push(Rotulo {
            texto: "0".into(),
            x: ox - MARGEN,
            y: oy + MARGEN + 6.0,
            ancla: Ancla::Derecha,
        });
    }
    out
}

/// El texto de la leyenda de una serie (su nombre o su expresión legible, acortada).
pub fn nombre_de_serie(s: &crate::bloque::Serie) -> String {
    let t = match &s.nombre {
        Some(n) => n.clone(),
        None => legible(&s.fuente),
    };
    if t.chars().count() > 40 {
        let corto: String = t.chars().take(39).collect();
        format!("{corto}…")
    } else {
        t
    }
}

fn superindice(c: char) -> Option<char> {
    Some(match c {
        '0' => '⁰',
        '1' => '¹',
        '2' => '²',
        '3' => '³',
        '4' => '⁴',
        '5' => '⁵',
        '6' => '⁶',
        '7' => '⁷',
        '8' => '⁸',
        '9' => '⁹',
        '-' | '−' => '⁻',
        '+' => '⁺',
        'n' => 'ⁿ',
        _ => return None,
    })
}

/// Lo que sigue: un grupo `{…}` o un solo carácter. Devuelve (contenido, resto).
fn grupo(s: &str) -> (&str, &str) {
    let s = s.trim_start();
    if let Some(r) = s.strip_prefix('{') {
        let mut prof = 1;
        for (k, c) in r.char_indices() {
            match c {
                '{' => prof += 1,
                '}' => {
                    prof -= 1;
                    if prof == 0 {
                        return (&r[..k], &r[k + 1..]);
                    }
                }
                _ => {}
            }
        }
        return (r, "");
    }
    match s.chars().next() {
        Some(c) => s.split_at(c.len_utf8()),
        None => ("", ""),
    }
}

/// Entre paréntesis si tiene más de un término.
fn agrupado(s: &str) -> String {
    let l = legible(s);
    if l.chars().count() > 1 && l.contains([' ', '+', '-', '−', '/', '·']) {
        format!("({l})")
    } else {
        l
    }
}

/// Una expresión escrita en LaTeX, legible como texto (para la leyenda): `\frac{1}{x}` → `1/x`,
/// `x^{2}` → `x²`, `\theta` → `θ`, `\cdot` → `·`. La notación simple queda casi igual.
pub fn legible(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut r = s;
    while let Some(c) = r.chars().next() {
        r = &r[c.len_utf8()..];
        match c {
            '\\' => {
                let fin = r
                    .char_indices()
                    .find(|(_, c)| !c.is_ascii_alphabetic())
                    .map_or(r.len(), |(k, _)| k);
                let (nombre, resto) = if fin == 0 {
                    r.split_at(r.chars().next().map_or(0, char::len_utf8))
                } else {
                    r.split_at(fin)
                };
                r = resto;
                match nombre {
                    "frac" | "dfrac" | "tfrac" => {
                        let (a, resto) = grupo(r);
                        let (b, resto) = grupo(resto);
                        r = resto;
                        out.push_str(&format!("{}/{}", agrupado(a), agrupado(b)));
                    }
                    "sqrt" => {
                        let (a, resto) = grupo(r);
                        r = resto;
                        out.push_str(&format!("√({})", legible(a)));
                    }
                    "cdot" | "times" => out.push('·'),
                    "div" => out.push('÷'),
                    "left" | "right" | "," | ";" | ":" | "!" | " " | "displaystyle" | "quad" => {}
                    "{" | "}" | "|" => out.push_str(nombre),
                    otro => {
                        let griega = [
                            ("alpha", 'α'),
                            ("beta", 'β'),
                            ("gamma", 'γ'),
                            ("delta", 'δ'),
                            ("theta", 'θ'),
                            ("lambda", 'λ'),
                            ("mu", 'μ'),
                            ("pi", 'π'),
                            ("rho", 'ρ'),
                            ("sigma", 'σ'),
                            ("tau", 'τ'),
                            ("phi", 'φ'),
                            ("omega", 'ω'),
                        ]
                        .iter()
                        .find(|(n, _)| *n == otro)
                        .map(|(_, c)| *c);
                        match griega {
                            Some(g) => out.push(g),
                            None => {
                                out.push_str(otro);
                                if r.starts_with(|c: char| c.is_alphanumeric()) {
                                    out.push(' ');
                                }
                            }
                        }
                    }
                }
            }
            '^' => {
                let (e, resto) = grupo(r);
                r = resto;
                let l = legible(e);
                match l.chars().map(superindice).collect::<Option<String>>() {
                    Some(sup) if !sup.is_empty() => out.push_str(&sup),
                    _ => out.push_str(&format!("^({l})")),
                }
            }
            '{' | '}' => {}
            c => out.push(c),
        }
    }
    out
}
