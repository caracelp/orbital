//! El dibujo de los gráficos 3D (053) en la CPU: los triángulos de la [`Escena`] se proyectan y
//! se pintan con un búfer de profundidad (cada píxel queda con lo más cercano, sin errores de
//! orden ni agujeros), con la luz calculada por vértice e interpolada (Gouraud). La imagen sale
//! con fondo transparente, lista para una `GdkMemoryTexture`; con `factor` 2 se pinta al doble
//! y se promedia (bordes suaves) para la imagen quieta. La comparación con dibujar cada triángulo
//! con GSK y con OpenGL está en `specs/053-graficos-3d/research.md`.

use crate::bloque::{Grafico, Tipo};
use crate::camara::Proyeccion;
use crate::espacio::{Escena, Vertice};
use crate::{Paleta, color_de_serie};

/// Cómo se pinta una serie.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Relleno {
    /// Un degradé según la altura (las superficies z = f(x, y) sin color pedido).
    Altura,
    Color([u8; 3]),
}

/// El relleno de cada serie: las superficies z = f(x, y) sin color van por altura (la primera; si
/// hay varias, las demás toman los colores de las series para distinguirse).
pub fn rellenos(g: &Grafico, paleta: Paleta) -> Vec<Relleno> {
    let mut alguna = false;
    g.series
        .iter()
        .enumerate()
        .map(|(k, s)| {
            if matches!(s.tipo, Tipo::Superficie(_)) && s.color.is_none() && !alguna {
                alguna = true;
                Relleno::Altura
            } else {
                Relleno::Color(color_de_serie(g, k, paleta))
            }
        })
        .collect()
}

/// ¿La serie tiene un color propio (no el degradé)? Para la leyenda.
pub fn con_color(r: &Relleno) -> Option<[u8; 3]> {
    match r {
        Relleno::Color(c) => Some(*c),
        Relleno::Altura => None,
    }
}

/// El degradé de la altura: violeta abajo, rosa al medio y ámbar arriba.
const ALTURA_OSCURA: [[f32; 3]; 3] = [
    [0x6a as f32, 0x6e as f32, 0xff as f32],
    [255.0, 0x6f as f32, 0xb5 as f32],
    [255.0, 0xd3 as f32, 0x6b as f32],
];
const ALTURA_PAPEL: [[f32; 3]; 3] = [
    [0x3d as f32, 0x4f as f32, 0xd6 as f32],
    [0xd6 as f32, 0x33 as f32, 0x6c as f32],
    [0xe0 as f32, 0x9a as f32, 0x1a as f32],
];

/// El color de la altura `h` (0 abajo, 1 arriba), de 0 a 255.
pub fn color_altura(h: f32, paleta: Paleta) -> [f32; 3] {
    let t = match paleta {
        Paleta::Oscura => &ALTURA_OSCURA,
        Paleta::Papel => &ALTURA_PAPEL,
    };
    let h = h.clamp(0.0, 1.0) * 2.0;
    let (a, b, s) = if h < 1.0 {
        (t[0], t[1], h)
    } else {
        (t[1], t[2], h - 1.0)
    };
    [
        a[0] + (b[0] - a[0]) * s,
        a[1] + (b[1] - a[1]) * s,
        a[2] + (b[2] - a[2]) * s,
    ]
}

/// La luz viene de arriba a la izquierda, del lado de la cámara (en la vista: derecha, arriba,
/// adentro).
const LUZ: [f32; 3] = [-0.36, 0.56, -0.746];

/// El color iluminado de un vértice, de 0 a 255. Las dos caras se iluminan igual.
#[inline]
pub fn iluminar(v: &Vertice, r: &Relleno, p: &Proyeccion, paleta: Paleta) -> [f32; 3] {
    let base = match r {
        Relleno::Altura => color_altura(v.h, paleta),
        Relleno::Color(c) => [f32::from(c[0]), f32::from(c[1]), f32::from(c[2])],
    };
    let n = p.girar(v.n);
    let d = (n[0] * LUZ[0] + n[1] * LUZ[1] + n[2] * LUZ[2]).abs();
    let luz = 0.40 + 0.60 * d;
    let brillo = 30.0 * d.powi(32);
    [
        (base[0] * luz + brillo).min(255.0),
        (base[1] * luz + brillo).min(255.0),
        (base[2] * luz + brillo).min(255.0),
    ]
}

/// Una imagen con su búfer de profundidad.
#[derive(Clone, Debug, Default)]
pub struct Imagen {
    /// Tamaño final (lo que se muestra), en píxeles.
    pub ancho: usize,
    pub alto: usize,
    /// Cuántas veces más grande se pinta (1 o 2).
    pub factor: usize,
    /// 0xAARRGGBB, premultiplicado; 0 es transparente.
    color: Vec<u32>,
    prof: Vec<f32>,
}

impl Imagen {
    pub fn new() -> Imagen {
        Imagen::default()
    }

    fn preparar(&mut self, ancho: usize, alto: usize, factor: usize) {
        self.ancho = ancho;
        self.alto = alto;
        self.factor = factor.max(1);
        let n = ancho * alto * self.factor * self.factor;
        self.color.clear();
        self.color.resize(n, 0);
        self.prof.clear();
        self.prof.resize(n, f32::INFINITY);
    }

    /// La profundidad de lo pintado en el píxel (x, y) del tamaño final, si hay algo.
    pub fn profundidad(&self, x: f32, y: f32) -> Option<f32> {
        if x < 0.0 || y < 0.0 {
            return None;
        }
        let f = self.factor;
        let (xi, yi) = ((x * f as f32) as usize, (y * f as f32) as usize);
        let w = self.ancho * f;
        if xi >= w || yi >= self.alto * f {
            return None;
        }
        let d = self.prof[yi * w + xi];
        d.is_finite().then_some(d)
    }

    /// ¿Se pintó algo?
    pub fn vacia(&self) -> bool {
        self.color.iter().all(|c| *c == 0)
    }

    /// Los píxeles del tamaño final en BGRA premultiplicado (el orden de
    /// `GDK_MEMORY_B8G8R8A8_PREMULTIPLIED`), promediando si se pintó más grande.
    pub fn bytes(&self) -> Vec<u8> {
        let (w, h, f) = (self.ancho, self.alto, self.factor);
        let mut out = vec![0u8; w * h * 4];
        if f == 1 {
            for (o, c) in out.as_chunks_mut::<4>().0.iter_mut().zip(&self.color) {
                o.copy_from_slice(&c.to_le_bytes());
            }
            return out;
        }
        let sw = w * f;
        let n = (f * f) as u32;
        for y in 0..h {
            for x in 0..w {
                let (mut a, mut r, mut g, mut b) = (0u32, 0u32, 0u32, 0u32);
                for dy in 0..f {
                    let fila = (y * f + dy) * sw + x * f;
                    for c in &self.color[fila..fila + f] {
                        a += c >> 24;
                        r += (c >> 16) & 255;
                        g += (c >> 8) & 255;
                        b += c & 255;
                    }
                }
                let k = (y * w + x) * 4;
                out[k] = (b / n) as u8;
                out[k + 1] = (g / n) as u8;
                out[k + 2] = (r / n) as u8;
                out[k + 3] = (a / n) as u8;
            }
        }
        out
    }
}

/// Un cuadro pintado, listo para mostrar: los píxeles (BGRA premultiplicado) y la profundidad de
/// cada uno (para saber qué hay bajo el puntero y qué tapa a las líneas).
#[derive(Clone, Debug, Default)]
pub struct Cuadro {
    pub ancho: usize,
    pub alto: usize,
    pub bytes: Vec<u8>,
    prof: Vec<f32>,
}

impl Cuadro {
    /// La profundidad de lo pintado en el píxel (x, y), si hay algo.
    pub fn profundidad(&self, x: f32, y: f32) -> Option<f32> {
        if !(x >= 0.0 && y >= 0.0) {
            return None;
        }
        let (xi, yi) = (x as usize, y as usize);
        if xi >= self.ancho || yi >= self.alto {
            return None;
        }
        let d = self.prof[yi * self.ancho + xi];
        d.is_finite().then_some(d)
    }

    /// ¿Lo que está en (x, y) con profundidad `prof` queda a la vista (nada pintado delante)?
    #[inline]
    pub fn visible(&self, x: f32, y: f32, prof: f32) -> bool {
        self.profundidad(x, y).is_none_or(|d| prof <= d + 0.02)
    }
}

/// Pinta la escena (en un hilo cualquiera) y devuelve el cuadro del tamaño de `p`.
pub fn pintar(
    e: &Escena,
    rellenos: &[Relleno],
    p: &Proyeccion,
    paleta: Paleta,
    factor: usize,
) -> Cuadro {
    let mut img = Imagen::new();
    rasterizar(e, rellenos, p, paleta, factor, &mut img);
    let bytes = img.bytes();
    let (w, h, f) = (img.ancho, img.alto, img.factor);
    let prof = if f == 1 {
        img.prof
    } else {
        // Lo más cercano de cada grupo de f × f.
        let sw = w * f;
        let mut out = vec![f32::INFINITY; w * h];
        for (k, o) in out.iter_mut().enumerate() {
            let (x, y) = (k % w, k / w);
            for dy in 0..f {
                let fila = (y * f + dy) * sw + x * f;
                for d in &img.prof[fila..fila + f] {
                    *o = o.min(*d);
                }
            }
        }
        out
    };
    Cuadro {
        ancho: w,
        alto: h,
        bytes,
        prof,
    }
}

/// Pinta los triángulos de la escena con la proyección `p` (del tamaño final) en `img`, `factor`
/// veces más grande.
pub fn rasterizar(
    e: &Escena,
    rellenos: &[Relleno],
    p: &Proyeccion,
    paleta: Paleta,
    factor: usize,
    img: &mut Imagen,
) {
    let (ancho, alto) = (
        p.ancho.round().max(1.0) as usize,
        p.alto.round().max(1.0) as usize,
    );
    img.preparar(ancho, alto, factor);
    let q = p.por(img.factor as f32);
    let (w, h) = (ancho * img.factor, alto * img.factor);
    for (pieza, relleno) in e.piezas.iter().zip(rellenos) {
        for t in &pieza.triangulos {
            let s = [q.px(t[0].p), q.px(t[1].p), q.px(t[2].p)];
            let c = [
                iluminar(&t[0], relleno, p, paleta),
                iluminar(&t[1], relleno, p, paleta),
                iluminar(&t[2], relleno, p, paleta),
            ];
            triangulo(&s, &c, w, h, &mut img.color, &mut img.prof);
        }
    }
}

/// Un triángulo con funciones de borde y pasos incrementales por píxel.
#[inline]
fn triangulo(
    s: &[[f32; 3]; 3],
    c: &[[f32; 3]; 3],
    w: usize,
    h: usize,
    color: &mut [u32],
    prof: &mut [f32],
) {
    let [a, b, d] = *s;
    let area = (b[0] - a[0]) * (d[1] - a[1]) - (b[1] - a[1]) * (d[0] - a[0]);
    if !area.is_finite() || area.abs() < 1e-8 {
        return;
    }
    let xmin = a[0].min(b[0]).min(d[0]).floor().max(0.0);
    let xmax = a[0].max(b[0]).max(d[0]).ceil().min(w as f32 - 1.0);
    let ymin = a[1].min(b[1]).min(d[1]).floor().max(0.0);
    let ymax = a[1].max(b[1]).max(d[1]).ceil().min(h as f32 - 1.0);
    if xmin > xmax || ymin > ymax {
        return;
    }
    let inv = 1.0 / area;
    // e_k(x, y) = A_k x + B_k y + C_k, normalizadas: e0 + e1 + e2 = 1 (baricéntricas).
    let borde = |p: [f32; 3], q: [f32; 3]| {
        let aa = (p[1] - q[1]) * inv;
        let bb = (q[0] - p[0]) * inv;
        let cc = (p[0] * q[1] - p[1] * q[0]) * inv;
        (aa, bb, cc)
    };
    let e0 = borde(b, d);
    let e1 = borde(d, a);
    let e2 = borde(a, b);
    // Lo que avanza por píxel en x: profundidad y color.
    let interp = |v0: f32, v1: f32, v2: f32| {
        (
            e0.0 * v0 + e1.0 * v1 + e2.0 * v2,
            e0.1 * v0 + e1.1 * v1 + e2.1 * v2,
            e0.2 * v0 + e1.2 * v1 + e2.2 * v2,
        )
    };
    let z = interp(a[2], b[2], d[2]);
    let r = interp(c[0][0], c[1][0], c[2][0]);
    let g = interp(c[0][1], c[1][1], c[2][1]);
    let bl = interp(c[0][2], c[1][2], c[2][2]);
    const EPS: f32 = -1e-5;
    let (x0, x1) = (xmin as usize, xmax as usize);
    let px0 = x0 as f32 + 0.5;
    for y in ymin as usize..=ymax as usize {
        let py = y as f32 + 0.5;
        let mut w0 = e0.0 * px0 + e0.1 * py + e0.2;
        let mut w1 = e1.0 * px0 + e1.1 * py + e1.2;
        let mut w2 = e2.0 * px0 + e2.1 * py + e2.2;
        let mut zz = z.0 * px0 + z.1 * py + z.2;
        let mut rr = r.0 * px0 + r.1 * py + r.2;
        let mut gg = g.0 * px0 + g.1 * py + g.2;
        let mut bb = bl.0 * px0 + bl.1 * py + bl.2;
        let fila = y * w;
        let mut dentro = false;
        let (a, b) = (fila + x0, fila + x1 + 1);
        for (pz, pc) in prof[a..b].iter_mut().zip(&mut color[a..b]) {
            if w0 >= EPS && w1 >= EPS && w2 >= EPS {
                dentro = true;
                if zz < *pz {
                    *pz = zz;
                    let cr = rr.clamp(0.0, 255.0) as u32;
                    let cg = gg.clamp(0.0, 255.0) as u32;
                    let cb = bb.clamp(0.0, 255.0) as u32;
                    *pc = 0xff00_0000 | (cr << 16) | (cg << 8) | cb;
                }
            } else if dentro {
                // Un triángulo es convexo: después de salir no se vuelve a entrar en la fila.
                break;
            }
            w0 += e0.0;
            w1 += e1.0;
            w2 += e2.0;
            zz += z.0;
            rr += r.0;
            gg += g.0;
            bb += bl.0;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bloque::leer;
    use crate::camara::Camara;
    use crate::espacio::{Opciones, escena};

    fn imagen(fuente: &str, factor: usize) -> (Imagen, Proyeccion, Escena) {
        let g = leer(fuente).unwrap();
        let e = escena(
            &g,
            &g.valores(),
            &Opciones {
                superficies: None,
                implicitas: 24,
                caja: None,
            },
        );
        let p = Proyeccion::new(&Camara::inicial(&g), 320.0, 200.0);
        let mut img = Imagen::new();
        rasterizar(
            &e,
            &rellenos(&g, Paleta::Oscura),
            &p,
            Paleta::Oscura,
            factor,
            &mut img,
        );
        (img, p, e)
    }

    #[test]
    fn la_profundidad_deja_lo_mas_cercano() {
        // Dos planos: z = 1 tapa a z = −1 visto desde arriba.
        let (img, p, e) = imagen(
            "cámara: 0, 89\nz: -1..1\nz = 0.9 ; rojo\nz = -0.9 ; azul",
            1,
        );
        let c = img.profundidad(160.0, 100.0).expect("pintado");
        let punto = e.caja.mundo(p.desde_px(160.0, 100.0, c));
        assert!((punto[2] - 0.9).abs() < 0.05, "{punto:?}");
        let b = img.bytes();
        let k = (100 * 320 + 160) * 4;
        // Rojo (BGRA): más rojo que azul, opaco.
        assert!(b[k + 2] > b[k] && b[k + 3] == 255, "{:?}", &b[k..k + 4]);
        // Fuera de la caja, transparente.
        assert_eq!(&b[0..4], &[0, 0, 0, 0]);
        assert!(img.profundidad(0.0, 0.0).is_none());
    }

    #[test]
    fn sin_agujeros_entre_triangulos() {
        // Una superficie vista desde arriba cubre el cuadrado de la caja sin huecos.
        let (img, p, _) = imagen(
            "cámara: 0, 89\nx: -1..1\ny: -1..1\nz: -1..1\nz = 0.3 sin(3x) cos(2y)",
            1,
        );
        let (a, b) = (p.px([-0.9, -0.9, 0.0]), p.px([0.9, 0.9, 0.0]));
        let (x0, x1) = (a[0].min(b[0]) as usize, a[0].max(b[0]) as usize);
        let (y0, y1) = (a[1].min(b[1]) as usize, a[1].max(b[1]) as usize);
        for y in y0..=y1 {
            for x in x0..=x1 {
                assert!(
                    img.profundidad(x as f32 + 0.5, y as f32 + 0.5).is_some(),
                    "agujero en {x}, {y}"
                );
            }
        }
    }

    #[test]
    fn suavizado_al_doble_y_degrade() {
        let (img, _, _) = imagen("x^2 + y^2 + z^2 = 4", 2);
        assert_eq!(img.factor, 2);
        let b = img.bytes();
        assert_eq!(b.len(), 320 * 200 * 4);
        // Algún píxel del borde queda a medias (suavizado).
        assert!(b.as_chunks::<4>().0.iter().any(|p| p[3] > 0 && p[3] < 255));
        assert!(!img.vacia());
        let bajo = color_altura(0.0, Paleta::Oscura);
        let alto = color_altura(1.0, Paleta::Oscura);
        assert!(bajo[2] > bajo[0] && alto[0] > alto[2]);
    }
}
