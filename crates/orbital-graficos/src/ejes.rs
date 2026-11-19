//! La vista (qué parte del plano se ve y en cuántos píxeles), las marcas de los ejes y sus
//! rótulos, y la grilla.

/// Una ventana del plano en un rectángulo de píxeles (el origen arriba a la izquierda).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Vista {
    pub x0: f64,
    pub x1: f64,
    pub y0: f64,
    pub y1: f64,
    pub ancho: f64,
    pub alto: f64,
}

impl Vista {
    /// La vista de `x` con `y` dado o, si no, con la misma escala que x y centrada en 0.
    pub fn nueva(x: (f64, f64), y: Option<(f64, f64)>, ancho: f64, alto: f64) -> Vista {
        let ancho = ancho.max(1.0);
        let alto = alto.max(1.0);
        let (y0, y1) = y.unwrap_or_else(|| {
            let medio = (x.1 - x.0) * alto / ancho / 2.0;
            (-medio, medio)
        });
        Vista {
            x0: x.0,
            x1: x.1,
            y0,
            y1,
            ancho,
            alto,
        }
    }

    #[inline]
    pub fn px(&self, x: f64, y: f64) -> (f64, f64) {
        (
            (x - self.x0) / (self.x1 - self.x0) * self.ancho,
            (self.y1 - y) / (self.y1 - self.y0) * self.alto,
        )
    }

    #[inline]
    pub fn mundo(&self, px: f64, py: f64) -> (f64, f64) {
        (
            self.x0 + px / self.ancho * (self.x1 - self.x0),
            self.y1 - py / self.alto * (self.y1 - self.y0),
        )
    }

    /// Unidades por píxel en x y en y.
    pub fn por_pixel(&self) -> (f64, f64) {
        (
            (self.x1 - self.x0) / self.ancho,
            (self.y1 - self.y0) / self.alto,
        )
    }

    /// Mueve la vista `dx`, `dy` píxeles (arrastrar a la derecha muestra lo de la izquierda).
    pub fn mover(&self, dx: f64, dy: f64) -> Vista {
        let (ux, uy) = self.por_pixel();
        Vista {
            x0: self.x0 - dx * ux,
            x1: self.x1 - dx * ux,
            y0: self.y0 + dy * uy,
            y1: self.y1 + dy * uy,
            ..*self
        }
    }

    /// Acerca (`factor` < 1) o aleja (> 1) alrededor del píxel (`px`, `py`). Con límites para
    /// que los números no pierdan precisión ni se vayan al infinito.
    pub fn zoom(&self, factor: f64, px: f64, py: f64) -> Vista {
        let (cx, cy) = self.mundo(px, py);
        let ancho = (self.x1 - self.x0) * factor;
        let alto = (self.y1 - self.y0) * factor;
        let escala = cx.abs().max(cy.abs()).max(1.0);
        if !(1e-9 * escala..=1e12).contains(&ancho) || !(1e-9 * escala..=1e12).contains(&alto) {
            return *self;
        }
        let fx = px / self.ancho;
        let fy = py / self.alto;
        Vista {
            x0: cx - fx * ancho,
            x1: cx + (1.0 - fx) * ancho,
            y0: cy - (1.0 - fy) * alto,
            y1: cy + fy * alto,
            ..*self
        }
    }

    /// La misma ventana en otro tamaño de píxeles: x se mantiene y y conserva la escala.
    pub fn con_tamano(&self, ancho: f64, alto: f64) -> Vista {
        let (_, uy) = self.por_pixel();
        let medio = (self.y0 + self.y1) / 2.0;
        let ny = uy * alto.max(1.0) / 2.0;
        Vista {
            y0: medio - ny,
            y1: medio + ny,
            ancho: ancho.max(1.0),
            alto: alto.max(1.0),
            ..*self
        }
    }
}

/// Un paso «redondo» (1, 2 o 5 por una potencia de 10) para marcas cada ~`objetivo` píxeles.
pub fn paso(desde: f64, hasta: f64, pixeles: f64, objetivo: f64) -> f64 {
    let crudo = (hasta - desde).abs() * objetivo / pixeles.max(1.0);
    if !crudo.is_finite() || crudo <= 0.0 {
        return 1.0;
    }
    let p = 10f64.powf(crudo.log10().floor());
    let m = crudo / p;
    let f = if m < 1.5 {
        1.0
    } else if m < 3.5 {
        2.0
    } else if m < 7.5 {
        5.0
    } else {
        10.0
    };
    f * p
}

/// Las marcas múltiplo de `paso` dentro de [`desde`, `hasta`] (a lo sumo 200).
pub fn marcas(desde: f64, hasta: f64, paso: f64) -> Vec<f64> {
    if paso <= 0.0 || !paso.is_finite() {
        return Vec::new();
    }
    let primero = (desde / paso).ceil() as i64;
    let ultimo = (hasta / paso).floor() as i64;
    if ultimo < primero || ultimo - primero > 200 {
        return Vec::new();
    }
    (primero..=ultimo).map(|k| k as f64 * paso).collect()
}

/// El rótulo de un valor con los decimales que pide el paso; signo menos tipográfico.
pub fn rotulo(v: f64, paso: f64) -> String {
    let decimales = if paso >= 1.0 || paso <= 0.0 {
        0
    } else {
        (-paso.log10().floor()) as usize
    };
    let v = if v.abs() < paso * 1e-6 { 0.0 } else { v };
    let a = v.abs();
    let s = if a != 0.0 && (a >= 1e7 || a < 1e-4) {
        let s = format!("{a:e}");
        // `1e7` → `1×10⁷`.
        match s.split_once('e') {
            Some((m, e)) => {
                let m = m.trim_end_matches('0').trim_end_matches('.');
                let m: String = m.chars().take(5).collect();
                format!("{m}×10{}", superindice(e))
            }
            None => s,
        }
    } else {
        let s = format!("{a:.decimales$}");
        if s.contains('.') {
            s.trim_end_matches('0').trim_end_matches('.').to_owned()
        } else {
            s
        }
    };
    if v < 0.0 { format!("−{s}") } else { s }
}

fn superindice(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            '-' => '⁻',
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
            c => c,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pasos_redondos_y_rotulos() {
        assert_eq!(paso(-10.0, 10.0, 800.0, 80.0), 2.0);
        assert_eq!(paso(0.0, 1.0, 500.0, 80.0), 0.2);
        assert_eq!(marcas(-1.0, 1.0, 0.5), vec![-1.0, -0.5, 0.0, 0.5, 1.0]);
        assert_eq!(rotulo(-2.0, 1.0), "−2");
        assert_eq!(rotulo(0.5, 0.1), "0.5");
        assert_eq!(rotulo(1e-17, 0.5), "0");
        assert_eq!(rotulo(2e8, 1e8), "2×10⁸");
    }

    #[test]
    fn vista_ida_y_vuelta() {
        let v = Vista::nueva((-10.0, 10.0), None, 400.0, 200.0);
        assert_eq!((v.y0, v.y1), (-5.0, 5.0));
        let (px, py) = v.px(2.0, 3.0);
        let (x, y) = v.mundo(px, py);
        assert!((x - 2.0).abs() < 1e-12 && (y - 3.0).abs() < 1e-12);
        let z = v.zoom(0.5, px, py);
        let (x2, y2) = z.mundo(px, py);
        assert!(
            (x2 - 2.0).abs() < 1e-9 && (y2 - 3.0).abs() < 1e-9,
            "el punto bajo el puntero queda"
        );
        assert!(((z.x1 - z.x0) - 10.0).abs() < 1e-9);
        let m = v.mover(40.0, 0.0);
        assert!((m.x0 + 12.0).abs() < 1e-12);
    }
}
