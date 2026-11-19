//! El gráfico como SVG autónomo y accesible (`role="img"`, `aria-label`, `<title>`): para exportar
//! (papel blanco) y para la Visual de las notas `.html`. Estático: la vista inicial con los
//! valores iniciales de los parámetros.

use std::fmt::Write;

use crate::bloque::{Estilo, Grafico};
use crate::ejes::Vista;
use crate::guias::{self, Ancla};
use crate::{Paleta, color_de_serie, muestreo};

/// Tamaño y colores del SVG.
#[derive(Clone, Copy, Debug)]
pub struct Opciones {
    pub ancho: u32,
    /// `None`: el del bloque (`alto:`) o el de por defecto.
    pub alto: Option<u32>,
    pub paleta: Paleta,
}

impl Default for Opciones {
    fn default() -> Self {
        Self {
            ancho: 640,
            alto: None,
            paleta: Paleta::Papel,
        }
    }
}

pub fn escapar(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            c => out.push(c),
        }
    }
    out
}

fn hex(c: [u8; 3]) -> String {
    format!("#{:02x}{:02x}{:02x}", c[0], c[1], c[2])
}

/// Un número corto para el SVG (dos decimales).
fn n(v: f64) -> String {
    let s = format!("{v:.2}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s == "-0" { "0".into() } else { s.into() }
}

/// El SVG del gráfico.
pub fn svg(g: &Grafico, o: &Opciones) -> String {
    let ancho = f64::from(o.ancho.max(80));
    let alto = f64::from(o.alto.or(g.alto).unwrap_or(crate::ALTO).max(80));
    let v = Vista::nueva(g.x, g.y, ancho, alto);
    let trazos = muestreo::muestrear(g, &v, &g.valores());
    let desc = g.descripcion();
    let mut s = String::with_capacity(16 * 1024);
    let _ = write!(
        s,
        "<svg class=\"grafico\" role=\"img\" aria-label=\"{d}\" width=\"{w}\" height=\"{h}\" viewBox=\"0 0 {w} {h}\" font-family=\"sans-serif\" font-size=\"11\"><title>{d}</title>",
        d = escapar(&desc),
        w = n(ancho),
        h = n(alto)
    );
    let _ = write!(
        s,
        "<defs><clipPath id=\"c\"><rect width=\"{}\" height=\"{}\"/></clipPath></defs><g clip-path=\"url(#c)\">",
        n(ancho),
        n(alto)
    );
    let gu = guias::guias(g, &v);
    if !gu.grilla.is_empty() || !gu.circulos.is_empty() {
        s.push_str("<g stroke=\"currentColor\" stroke-opacity=\"0.13\" fill=\"none\">");
        if !gu.grilla.is_empty() {
            s.push_str("<path d=\"");
            for l in &gu.grilla {
                let _ = write!(s, "M{} {}L{} {}", n(l[0]), n(l[1]), n(l[2]), n(l[3]));
            }
            s.push_str("\"/>");
        }
        for c in &gu.circulos {
            let _ = write!(
                s,
                "<circle cx=\"{}\" cy=\"{}\" r=\"{}\"/>",
                n(c[0]),
                n(c[1]),
                n(c[2])
            );
        }
        s.push_str("</g>");
    }
    if !gu.ejes.is_empty() {
        s.push_str("<path stroke=\"currentColor\" stroke-opacity=\"0.6\" d=\"");
        for l in &gu.ejes {
            let _ = write!(s, "M{} {}L{} {}", n(l[0]), n(l[1]), n(l[2]), n(l[3]));
        }
        s.push_str("\"/>");
    }
    for (k, (serie, t)) in g.series.iter().zip(&trazos).enumerate() {
        let color = hex(color_de_serie(g, k, o.paleta));
        let grosor = serie.grosor.unwrap_or(2.0);
        if serie.estilo != Estilo::Puntos && !t.lineas.is_empty() {
            let _ = write!(
                s,
                "<path fill=\"none\" stroke=\"{color}\" stroke-width=\"{}\" stroke-linejoin=\"round\" stroke-linecap=\"round\"{} d=\"",
                n(grosor),
                if serie.estilo == Estilo::Discontinua {
                    " stroke-dasharray=\"7 5\""
                } else {
                    ""
                }
            );
            for l in muestreo::a_pixeles(&t.lineas, &v) {
                for (i, p) in l.iter().enumerate() {
                    let c = if i == 0 { 'M' } else { 'L' };
                    let _ = write!(s, "{c}{} {}", n(f64::from(p[0])), n(f64::from(p[1])));
                }
            }
            s.push_str("\"/>");
        }
        if matches!(serie.estilo, Estilo::Puntos | Estilo::LineaYPuntos) && !t.puntos.is_empty() {
            let _ = write!(s, "<g fill=\"{color}\">");
            for p in &t.puntos {
                let (x, y) = v.px(p[0], p[1]);
                if (-10.0..=ancho + 10.0).contains(&x) && (-10.0..=alto + 10.0).contains(&y) {
                    let _ = write!(s, "<circle cx=\"{}\" cy=\"{}\" r=\"3.5\"/>", n(x), n(y));
                }
            }
            s.push_str("</g>");
        }
    }
    s.push_str("</g>");
    if !gu.rotulos.is_empty() {
        s.push_str("<g fill=\"currentColor\" fill-opacity=\"0.75\">");
        for r in &gu.rotulos {
            let (ancla, dy) = match r.ancla {
                Ancla::Arriba => ("middle", "0.9em"),
                Ancla::Abajo => ("middle", "-0.2em"),
                Ancla::Derecha => ("end", "0.35em"),
                Ancla::Izquierda => ("start", "0.35em"),
            };
            let _ = write!(
                s,
                "<text x=\"{}\" y=\"{}\" dy=\"{dy}\" text-anchor=\"{ancla}\">{}</text>",
                n(r.x),
                n(r.y),
                escapar(&r.texto)
            );
        }
        s.push_str("</g>");
    }
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
            let color = hex(color_de_serie(g, k, o.paleta));
            let _ = write!(
                s,
                "<rect x=\"{}\" y=\"{}\" width=\"14\" height=\"3\" fill=\"{color}\"/><text x=\"{}\" y=\"{}\" text-anchor=\"end\" fill=\"currentColor\">{}</text>",
                n(ancho - 24.0),
                n(y - 4.0),
                n(ancho - 30.0),
                n(y),
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
    fn svg_accesible_con_cada_serie() {
        let g = leer("título: Prueba <1>\ny = x^2\nr = 2\n(1, 2)\nx^2 + y^2 = 4").unwrap();
        let s = svg(&g, &Opciones::default());
        assert!(
            s.starts_with(
                "<svg class=\"grafico\" role=\"img\" aria-label=\"Gráfico: Prueba &lt;1&gt;."
            ),
            "{s}"
        );
        assert!(
            s.contains("<title>Gráfico: Prueba &lt;1&gt;. y = x^2; r = 2; (1, 2); x^2 + y^2 = 4.")
        );
        assert_eq!(s.matches("stroke-linejoin").count(), 3);
        assert!(
            s.contains("<circle cx=\"352\" cy=\"86\" r=\"3.5\"/>"),
            "{s}"
        );
        assert!(s.contains("#d6336c"), "colores para papel");
        assert!(!s.contains("http"), "sin xmlns ni recursos");
        assert!(s.ends_with("</svg>"));
    }
}
