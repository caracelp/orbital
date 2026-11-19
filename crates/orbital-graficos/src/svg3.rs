//! El gráfico 3D (053) como SVG estático (exportar y notas `.html`): la vista inicial de la
//! cámara, con el algoritmo del pintor (los triángulos, los tramos de las curvas y los puntos,
//! de atrás para adelante). Cada triángulo lleva un borde fino de su mismo color para que no se
//! vean costuras entre vecinos. Las mallas van más gruesas que en la app (el archivo no crece de
//! más) y el color es el de papel.

use std::fmt::Write;

use crate::camara::{Camara, Proyeccion, guias3};
use crate::espacio::{Opciones as OpcionesEscena, escena};
use crate::guias::{self, Ancla};
use crate::raster::{Relleno, color_altura, iluminar, rellenos};
use crate::svg::{Opciones, escapar};
use crate::{Estilo, Grafico};

/// Puntos por lado de las mallas en el SVG.
const MALLA_SVG: u32 = 32;
/// Celdas por lado de las implícitas en el SVG.
const IMPLICITAS_SVG: u32 = 24;

fn hex(c: [f32; 3]) -> String {
    let b = |v: f32| v.round().clamp(0.0, 255.0) as u8;
    format!("#{:02x}{:02x}{:02x}", b(c[0]), b(c[1]), b(c[2]))
}

/// Un número con un decimal.
fn n1(v: f32) -> String {
    let s = format!("{v:.1}");
    let s = s.strip_suffix(".0").unwrap_or(&s);
    if s == "-0" { "0".into() } else { s.into() }
}

enum Pieza {
    Tri([[f32; 2]; 3], String),
    Tramo([f32; 2], [f32; 2], usize),
    Punto([f32; 2], usize),
}

pub fn svg3(g: &Grafico, o: &Opciones) -> String {
    let ancho = o.ancho.max(80) as f32;
    let alto = o.alto.or(g.alto).unwrap_or(crate::ALTO_3D).max(80) as f32;
    let e = escena(
        g,
        &g.valores(),
        &OpcionesEscena {
            superficies: Some(g.resolucion.unwrap_or(MALLA_SVG).min(MALLA_SVG)),
            implicitas: IMPLICITAS_SVG,
            caja: None,
        },
    );
    let p = Proyeccion::new(&Camara::inicial(g), ancho, alto);
    let rell = rellenos(g, o.paleta);
    let mut piezas: Vec<(f32, Pieza)> = Vec::new();
    for (k, (pz, r)) in e.piezas.iter().zip(&rell).enumerate() {
        for t in &pz.triangulos {
            let s = t.map(|v| p.px(v.p));
            let c = t.map(|v| iluminar(&v, r, &p, o.paleta));
            let medio = [
                (c[0][0] + c[1][0] + c[2][0]) / 3.0,
                (c[0][1] + c[1][1] + c[2][1]) / 3.0,
                (c[0][2] + c[1][2] + c[2][2]) / 3.0,
            ];
            let prof = (s[0][2] + s[1][2] + s[2][2]) / 3.0;
            piezas.push((prof, Pieza::Tri(s.map(|q| [q[0], q[1]]), hex(medio))));
        }
        let serie = &g.series[k];
        if serie.estilo != Estilo::Puntos {
            for l in &pz.lineas {
                for w in l.windows(2) {
                    let (a, b) = (p.px(w[0]), p.px(w[1]));
                    piezas.push((
                        (a[2] + b[2]) / 2.0,
                        Pieza::Tramo([a[0], a[1]], [b[0], b[1]], k),
                    ));
                }
            }
        }
        if serie.estilo != Estilo::Linea || pz.lineas.is_empty() {
            for q in &pz.puntos {
                let s = p.px(*q);
                piezas.push((s[2], Pieza::Punto([s[0], s[1]], k)));
            }
        }
    }
    // De atrás (más profundidad) para adelante.
    piezas.sort_by(|a, b| b.0.total_cmp(&a.0));

    let color_serie = |k: usize| -> String {
        match rell[k] {
            Relleno::Color(c) => hex([f32::from(c[0]), f32::from(c[1]), f32::from(c[2])]),
            Relleno::Altura => hex(color_altura(0.5, o.paleta)),
        }
    };
    let desc = g.descripcion();
    let mut s = String::with_capacity(64 * 1024);
    let _ = write!(
        s,
        "<svg class=\"grafico\" role=\"img\" aria-label=\"{d}\" width=\"{w}\" height=\"{h}\" viewBox=\"0 0 {w} {h}\" font-family=\"sans-serif\" font-size=\"11\"><title>{d}</title>",
        d = escapar(&desc),
        w = n1(ancho),
        h = n1(alto)
    );
    let gu = guias3(&e.caja, &p, g.ejes);
    if !gu.grilla.is_empty() {
        s.push_str("<path fill=\"none\" stroke=\"currentColor\" stroke-opacity=\"0.13\" d=\"");
        for l in &gu.grilla {
            let _ = write!(s, "M{} {}L{} {}", n1(l[0]), n1(l[1]), n1(l[2]), n1(l[3]));
        }
        s.push_str("\"/>");
    }
    if !gu.bordes.is_empty() {
        s.push_str("<path fill=\"none\" stroke=\"currentColor\" stroke-opacity=\"0.4\" d=\"");
        for l in &gu.bordes {
            let _ = write!(s, "M{} {}L{} {}", n1(l[0]), n1(l[1]), n1(l[2]), n1(l[3]));
        }
        s.push_str("\"/>");
    }
    s.push_str("<g stroke-width=\"0.7\" stroke-linejoin=\"round\" stroke-linecap=\"round\">");
    let mut k = 0;
    while k < piezas.len() {
        match &piezas[k].1 {
            Pieza::Tri(t, c) => {
                let _ = write!(
                    s,
                    "<path fill=\"{c}\" stroke=\"{c}\" d=\"M{} {}L{} {}L{} {}Z\"/>",
                    n1(t[0][0]),
                    n1(t[0][1]),
                    n1(t[1][0]),
                    n1(t[1][1]),
                    n1(t[2][0]),
                    n1(t[2][1])
                );
                k += 1;
            }
            Pieza::Tramo(a, b, serie) => {
                // Los tramos seguidos de la misma serie van en un solo camino.
                let grosor = g.series[*serie].grosor.unwrap_or(2.0);
                let _ = write!(
                    s,
                    "<path fill=\"none\" stroke=\"{}\" stroke-width=\"{}\"{} d=\"M{} {}L{} {}",
                    color_serie(*serie),
                    n1(grosor as f32),
                    if g.series[*serie].estilo == Estilo::Discontinua {
                        " stroke-dasharray=\"7 5\""
                    } else {
                        ""
                    },
                    n1(a[0]),
                    n1(a[1]),
                    n1(b[0]),
                    n1(b[1])
                );
                let mut fin = *b;
                k += 1;
                while let Some((_, Pieza::Tramo(a2, b2, s2))) = piezas.get(k) {
                    if s2 != serie {
                        break;
                    }
                    if (a2[0] - fin[0]).abs() < 0.05 && (a2[1] - fin[1]).abs() < 0.05 {
                        let _ = write!(s, "L{} {}", n1(b2[0]), n1(b2[1]));
                    } else {
                        let _ = write!(
                            s,
                            "M{} {}L{} {}",
                            n1(a2[0]),
                            n1(a2[1]),
                            n1(b2[0]),
                            n1(b2[1])
                        );
                    }
                    fin = *b2;
                    k += 1;
                }
                s.push_str("\"/>");
            }
            Pieza::Punto(q, serie) => {
                let _ = write!(
                    s,
                    "<circle fill=\"{}\" stroke=\"none\" cx=\"{}\" cy=\"{}\" r=\"3\"/>",
                    color_serie(*serie),
                    n1(q[0]),
                    n1(q[1])
                );
                k += 1;
            }
        }
    }
    s.push_str("</g>");
    let rotulos = gu.rotulos.iter().chain(&gu.nombres);
    s.push_str("<g fill=\"currentColor\" fill-opacity=\"0.75\">");
    for r in rotulos {
        let (ancla, dy) = match r.ancla {
            Ancla::Arriba => ("middle", "0.9em"),
            Ancla::Abajo => ("middle", "-0.2em"),
            Ancla::Derecha => ("end", "0.35em"),
            Ancla::Izquierda => ("start", "0.35em"),
        };
        let _ = write!(
            s,
            "<text x=\"{}\" y=\"{}\" dy=\"{dy}\" text-anchor=\"{ancla}\">{}</text>",
            n1(r.x as f32),
            n1(r.y as f32),
            escapar(&r.texto)
        );
    }
    s.push_str("</g>");
    if let Some(t) = &g.titulo {
        let _ = write!(
            s,
            "<text x=\"10\" y=\"20\" font-size=\"14\" font-weight=\"bold\" fill=\"currentColor\">{}</text>",
            escapar(t)
        );
    }
    if g.con_leyenda() {
        let mut y = 18.0;
        for (k, serie) in g.series.iter().enumerate() {
            let _ = write!(
                s,
                "<rect x=\"{}\" y=\"{}\" width=\"14\" height=\"3\" fill=\"{}\"/><text x=\"{}\" y=\"{}\" text-anchor=\"end\" fill=\"currentColor\">{}</text>",
                n1(ancho - 24.0),
                n1(y - 4.0),
                color_serie(k),
                n1(ancho - 30.0),
                n1(y),
                escapar(&guias::nombre_de_serie(serie))
            );
            y += 16.0;
        }
    }
    s.push_str("</svg>");
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bloque::leer;

    #[test]
    fn svg_3d_accesible_y_ordenado() {
        let g = leer("título: Silla\nz = x^2 - y^2\n(cos(t), sin(t), 3) ; t: 0..2π\n(0, 0, 5)")
            .unwrap();
        let s = crate::svg::svg(&g, &Opciones::default());
        assert!(
            s.starts_with("<svg class=\"grafico\" role=\"img\" aria-label=\"Gráfico 3D: Silla."),
            "{}",
            &s[..200]
        );
        assert!(
            s.contains("<title>Gráfico 3D: Silla. z = x^2 - y^2; (cos(t), sin(t), 3); (0, 0, 5).")
        );
        assert_eq!(
            s.matches("Z\"/>").count(),
            31 * 31 * 2,
            "un triángulo por mitad de cuadro"
        );
        assert!(s.contains("<circle"), "el punto");
        assert!(s.contains(">z</text>"), "los nombres de los ejes");
        assert!(!s.contains("NaN") && !s.contains("inf"));
        assert!(s.ends_with("</svg>"));
        // Tamaño razonable para exportar.
        assert!(s.len() < 250_000, "{} bytes", s.len());
    }
}
