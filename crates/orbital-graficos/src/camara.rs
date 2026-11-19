//! La cámara de los gráficos 3D (053): giro alrededor de z, elevación y acercamiento, con una
//! proyección paralela (la de los gráficos matemáticos: las rectas paralelas siguen paralelas).
//! También la caja: las caras del fondo con su grilla y los rótulos de las marcas.

use crate::ejes;
use crate::espacio::Caja;
use crate::guias::{Ancla, Rotulo};

/// Desde dónde se mira, en grados.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Camara {
    /// Giro de la cámara alrededor del eje z.
    pub giro: f64,
    /// Altura de la cámara sobre el plano xy (−90 a 90).
    pub elevacion: f64,
    /// 1: la caja entra entera en cualquier giro.
    pub zoom: f64,
}

/// La cámara si el bloque no pide otra (como la de casi todos los programas de gráficos).
pub const CAMARA: (f64, f64) = (-60.0, 28.0);

impl Camara {
    pub fn inicial(g: &crate::Grafico) -> Camara {
        let (giro, elevacion) = g.camara.unwrap_or(CAMARA);
        Camara {
            giro,
            elevacion,
            zoom: 1.0,
        }
    }

    /// Gira `dg` grados alrededor de z y sube `de` grados (con la elevación acotada).
    pub fn girar(&self, dg: f64, de: f64) -> Camara {
        let mut giro = (self.giro + dg) % 360.0;
        if giro > 180.0 {
            giro -= 360.0;
        } else if giro < -180.0 {
            giro += 360.0;
        }
        Camara {
            giro,
            elevacion: (self.elevacion + de).clamp(-89.0, 89.0),
            ..*self
        }
    }

    pub fn acercar(&self, factor: f64) -> Camara {
        Camara {
            zoom: (self.zoom * factor).clamp(0.25, 8.0),
            ..*self
        }
    }
}

/// La proyección de la caja normalizada (−1..1) a píxeles de un lienzo.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Proyeccion {
    /// Filas: derecha, arriba y hacia adentro de la pantalla (la profundidad).
    pub r: [[f32; 3]; 3],
    pub escala: f32,
    pub cx: f32,
    pub cy: f32,
    pub ancho: f32,
    pub alto: f32,
}

impl Proyeccion {
    pub fn new(c: &Camara, ancho: f32, alto: f32) -> Proyeccion {
        let (sg, cg) = (c.giro.to_radians() as f32).sin_cos();
        let (se, ce) = (c.elevacion.to_radians() as f32).sin_cos();
        // La cámara está en (cos e cos g, cos e sin g, sin e) y mira al centro.
        let derecha = [-sg, cg, 0.0];
        let arriba = [-cg * se, -sg * se, ce];
        let adentro = [-ce * cg, -ce * sg, -se];
        // La caja (de semidiagonal √3) entra entera en cualquier giro, con lugar para los
        // rótulos.
        let escala = (ancho / 3.9).min(alto / 3.9) * c.zoom as f32;
        Proyeccion {
            r: [derecha, arriba, adentro],
            escala,
            cx: ancho / 2.0,
            cy: alto / 2.0,
            ancho,
            alto,
        }
    }

    /// La misma proyección en un lienzo `f` veces más grande (para suavizar los bordes).
    pub fn por(&self, f: f32) -> Proyeccion {
        Proyeccion {
            escala: self.escala * f,
            cx: self.cx * f,
            cy: self.cy * f,
            ancho: self.ancho * f,
            alto: self.alto * f,
            ..*self
        }
    }

    /// (x, y) en píxeles y la profundidad (menos es más cerca).
    #[inline]
    pub fn px(&self, p: [f32; 3]) -> [f32; 3] {
        let r = &self.r;
        let x = r[0][0] * p[0] + r[0][1] * p[1] + r[0][2] * p[2];
        let y = r[1][0] * p[0] + r[1][1] * p[1] + r[1][2] * p[2];
        let z = r[2][0] * p[0] + r[2][1] * p[1] + r[2][2] * p[2];
        [self.cx + x * self.escala, self.cy - y * self.escala, z]
    }

    /// Un vector (una normal) en la vista: (derecha, arriba, adentro).
    #[inline]
    pub fn girar(&self, n: [f32; 3]) -> [f32; 3] {
        let r = &self.r;
        [
            r[0][0] * n[0] + r[0][1] * n[1] + r[0][2] * n[2],
            r[1][0] * n[0] + r[1][1] * n[1] + r[1][2] * n[2],
            r[2][0] * n[0] + r[2][1] * n[1] + r[2][2] * n[2],
        ]
    }

    /// Del píxel (x, y) con su profundidad a la caja normalizada.
    pub fn desde_px(&self, x: f32, y: f32, prof: f32) -> [f32; 3] {
        let v = [
            (x - self.cx) / self.escala,
            (self.cy - y) / self.escala,
            prof,
        ];
        // La matriz es ortogonal: la inversa es la traspuesta.
        let r = &self.r;
        [
            r[0][0] * v[0] + r[1][0] * v[1] + r[2][0] * v[2],
            r[0][1] * v[0] + r[1][1] * v[1] + r[2][1] * v[2],
            r[0][2] * v[0] + r[1][2] * v[1] + r[2][2] * v[2],
        ]
    }
}

/// La caja dibujada: las caras del fondo (borde y grilla) y los rótulos.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Guias3 {
    /// La grilla de las caras del fondo, en píxeles.
    pub grilla: Vec<[f32; 4]>,
    /// Los bordes de las caras del fondo.
    pub bordes: Vec<[f32; 4]>,
    /// Los números de las marcas.
    pub rotulos: Vec<Rotulo>,
    /// «x», «y», «z» junto a sus aristas.
    pub nombres: Vec<Rotulo>,
}

/// Las esquinas de la arista del eje `eje` en la posición (a, b) de los otros dos.
fn arista(eje: usize, a: f32, b: f32) -> ([f32; 3], [f32; 3]) {
    let mut p = [0.0f32; 3];
    let mut q = [0.0f32; 3];
    let (o1, o2) = otros(eje);
    p[eje] = -1.0;
    q[eje] = 1.0;
    p[o1] = a;
    q[o1] = a;
    p[o2] = b;
    q[o2] = b;
    (p, q)
}

fn otros(eje: usize) -> (usize, usize) {
    match eje {
        0 => (1, 2),
        1 => (0, 2),
        _ => (0, 1),
    }
}

/// Las marcas de un eje: (valor, posición en −1..1).
fn marcas_eje(rango: (f64, f64), pixeles: f64) -> (Vec<(f64, f32)>, f64) {
    let paso = ejes::paso(rango.0, rango.1, pixeles, 55.0);
    let m = ejes::marcas(rango.0, rango.1, paso)
        .into_iter()
        .map(|v| (v, ((v - rango.0) / (rango.1 - rango.0) * 2.0 - 1.0) as f32))
        .filter(|(_, n)| (-1.0001..=1.0001).contains(n))
        .collect();
    (m, paso)
}

pub fn guias3(caja: &Caja, p: &Proyeccion, ejes_visibles: bool) -> Guias3 {
    let mut g = Guias3::default();
    if !ejes_visibles {
        return g;
    }
    let seg = |a: [f32; 3], b: [f32; 3]| {
        let (pa, pb) = (p.px(a), p.px(b));
        [pa[0], pa[1], pb[0], pb[1]]
    };
    let largo_px = |eje: usize| {
        let (a, b) = arista(eje, -1.0, -1.0);
        let (pa, pb) = (p.px(a), p.px(b));
        f64::from((pa[0] - pb[0]).hypot(pa[1] - pb[1]))
    };
    let marcas: Vec<(Vec<(f64, f32)>, f64)> = (0..3)
        .map(|k| marcas_eje(caja.eje(k), largo_px(k).max(40.0)))
        .collect();
    // Las caras del fondo: las que miran hacia adentro de la pantalla.
    for eje in 0..3 {
        for lado in [-1.0f32, 1.0] {
            if lado * p.r[2][eje] <= 0.0 {
                continue;
            }
            let (o1, o2) = otros(eje);
            let punto = |a: f32, b: f32| {
                let mut q = [0.0f32; 3];
                q[eje] = lado;
                q[o1] = a;
                q[o2] = b;
                q
            };
            for (a, b, c, d) in [
                (-1.0, -1.0, 1.0, -1.0),
                (1.0, -1.0, 1.0, 1.0),
                (1.0, 1.0, -1.0, 1.0),
                (-1.0, 1.0, -1.0, -1.0),
            ] {
                g.bordes.push(seg(punto(a, b), punto(c, d)));
            }
            for (_, t) in &marcas[o1].0 {
                g.grilla.push(seg(punto(*t, -1.0), punto(*t, 1.0)));
            }
            for (_, t) in &marcas[o2].0 {
                g.grilla.push(seg(punto(-1.0, *t), punto(1.0, *t)));
            }
        }
    }
    // Los rótulos: x e y en la arista más baja de la pantalla; z en la de más a la izquierda.
    let centro = p.px([0.0, 0.0, 0.0]);
    for eje in 0..3 {
        let mut mejor: Option<((f32, f32), f32)> = None;
        for (a, b) in [(-1.0, -1.0), (1.0, -1.0), (-1.0, 1.0), (1.0, 1.0)] {
            let (pa, pb) = arista(eje, a, b);
            let (qa, qb) = (p.px(pa), p.px(pb));
            let medio = ((qa[0] + qb[0]) / 2.0, (qa[1] + qb[1]) / 2.0);
            let prof = (qa[2] + qb[2]) / 2.0;
            // Más abajo (o más a la izquierda); a igualdad, la de adelante.
            let puntaje = if eje == 2 { -medio.0 } else { medio.1 } - prof * 0.01;
            if mejor.is_none_or(|(_, m)| puntaje > m) {
                mejor = Some(((a, b), puntaje));
            }
        }
        let ((a, b), _) = mejor.expect("cuatro aristas");
        let (pa, pb) = arista(eje, a, b);
        let (qa, qb) = (p.px(pa), p.px(pb));
        let medio = ((qa[0] + qb[0]) / 2.0, (qa[1] + qb[1]) / 2.0);
        // Hacia afuera de la caja.
        let (mut dx, mut dy) = (medio.0 - centro[0], medio.1 - centro[1]);
        let l = dx.hypot(dy).max(1e-3);
        dx /= l;
        dy /= l;
        let ancla = |dx: f32, dy: f32| {
            if dx.abs() > dy.abs() * 1.4 {
                if dx > 0.0 {
                    Ancla::Izquierda
                } else {
                    Ancla::Derecha
                }
            } else if dy > 0.0 {
                Ancla::Arriba
            } else {
                Ancla::Abajo
            }
        };
        let (valores, paso) = &marcas[eje];
        for (v, t) in valores {
            let mut q = pa;
            q[eje] = *t;
            let s = p.px(q);
            g.rotulos.push(Rotulo {
                texto: ejes::rotulo(*v, *paso),
                x: f64::from(s[0] + dx * 10.0),
                y: f64::from(s[1] + dy * 10.0),
                ancla: ancla(dx, dy),
            });
        }
        g.nombres.push(Rotulo {
            texto: ["x", "y", "z"][eje].to_owned(),
            x: f64::from(medio.0 + dx * 34.0),
            y: f64::from(medio.1 + dy * 34.0),
            ancla: ancla(dx, dy),
        });
    }
    g
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn proyeccion_ida_y_vuelta_y_orientacion() {
        let c = Camara {
            giro: -60.0,
            elevacion: 28.0,
            zoom: 1.0,
        };
        let p = Proyeccion::new(&c, 640.0, 360.0);
        for q in [[0.3f32, -0.7, 0.9], [-1.0, 1.0, -1.0]] {
            let s = p.px(q);
            let v = p.desde_px(s[0], s[1], s[2]);
            for k in 0..3 {
                assert!((v[k] - q[k]).abs() < 1e-5);
            }
        }
        // z hacia arriba en la pantalla; desde un giro de −60°, x va a la derecha y y hacia
        // adentro (una vista sin espejar: derecha × arriba apunta a la cámara).
        assert!(p.px([0.0, 0.0, 1.0])[1] < p.px([0.0, 0.0, -1.0])[1]);
        assert!(p.px([1.0, 0.0, 0.0])[0] > p.px([-1.0, 0.0, 0.0])[0]);
        assert!(p.px([0.0, 1.0, 0.0])[2] > p.px([0.0, -1.0, 0.0])[2]);
        let [d, a, f] = p.r;
        let x = [
            d[1] * a[2] - d[2] * a[1],
            d[2] * a[0] - d[0] * a[2],
            d[0] * a[1] - d[1] * a[0],
        ];
        for k in 0..3 {
            assert!((x[k] + f[k]).abs() < 1e-5, "mano derecha");
        }
        // La caja entra entera.
        for q in [[1.0f32, 1.0, 1.0], [-1.0, -1.0, -1.0], [1.0, -1.0, 1.0]] {
            let s = p.px(q);
            assert!((0.0..=640.0).contains(&s[0]) && (0.0..=360.0).contains(&s[1]));
        }
    }

    #[test]
    fn caras_del_fondo_y_rotulos() {
        let c = Camara {
            giro: -60.0,
            elevacion: 28.0,
            zoom: 1.0,
        };
        let p = Proyeccion::new(&c, 640.0, 360.0);
        let caja = Caja {
            x: (-3.0, 3.0),
            y: (-3.0, 3.0),
            z: (0.0, 10.0),
        };
        let g = guias3(&caja, &p, true);
        // Tres caras del fondo, cuatro bordes cada una.
        assert_eq!(g.bordes.len(), 12);
        assert!(!g.grilla.is_empty());
        assert_eq!(g.nombres.len(), 3);
        assert!(g.rotulos.iter().any(|r| r.texto == "10"));
        assert!(g.rotulos.len() >= 8, "{:?}", g.rotulos);
        let g = guias3(&caja, &p, false);
        assert!(g.bordes.is_empty() && g.rotulos.is_empty());
        let c2 = c.girar(400.0, 100.0);
        assert!((c2.giro - (-20.0)).abs() < 1e-9 && c2.elevacion == 89.0);
    }
}
