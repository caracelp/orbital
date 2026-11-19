//! Muestreo de los gráficos 3D (053): de las series del espacio a triángulos, líneas y puntos
//! dentro de la caja del gráfico, en coordenadas normalizadas (la caja va de −1 a 1 en cada eje).
//!
//! - `z = f(x, y)` y las superficies paramétricas: una malla de n × n. En un cuadro con alguna
//!   esquina sin definir (`sqrt(1 − x² − y²)`), el borde se busca por bisección y se dibuja solo
//!   la parte definida; un salto (`tan(x + y)`, el centro del cuadro fuera de lo que dan las
//!   esquinas) no se une. Todo se recorta contra la caja: lo que se sale queda cortado al ras.
//! - Implícitas `F(x, y, z) = 0`: *surface nets* sobre una grilla de m³ celdas (un vértice por
//!   celda que cruza la superficie, en el promedio de los cruces de sus aristas; un cuadrilátero
//!   por arista con cambio de signo). Sin tablas de casos y con la mitad de triángulos que
//!   marching cubes (comparación en `specs/053-graficos-3d/research.md`). Un cambio de signo
//!   donde |F| crece (un polo, `z − 1/x = 0`) no es superficie.
//! - Curvas `(x(t), y(t), z(t))`: muestreo parejo con cortes donde no está definida o salta, y
//!   recorte contra la caja. Puntos y nubes de datos: los de adentro de la caja.
//!
//! Todo es puro y va fuera del hilo de GTK.

use crate::bloque::{self, Grafico, Serie, Tipo};
use crate::expr::Expr;

/// La caja del gráfico, en las unidades de la usuaria.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Caja {
    pub x: (f64, f64),
    pub y: (f64, f64),
    pub z: (f64, f64),
}

impl Caja {
    pub fn eje(&self, k: usize) -> (f64, f64) {
        match k {
            0 => self.x,
            1 => self.y,
            _ => self.z,
        }
    }

    /// De las unidades del gráfico a la caja normalizada (−1..1).
    #[inline]
    pub fn normalizar(&self, p: [f64; 3]) -> [f64; 3] {
        let n = |v: f64, (a, b): (f64, f64)| (v - a) / (b - a) * 2.0 - 1.0;
        [n(p[0], self.x), n(p[1], self.y), n(p[2], self.z)]
    }

    /// De la caja normalizada a las unidades del gráfico.
    #[inline]
    pub fn mundo(&self, p: [f32; 3]) -> [f64; 3] {
        let m = |v: f32, (a, b): (f64, f64)| a + (f64::from(v) + 1.0) / 2.0 * (b - a);
        [m(p[0], self.x), m(p[1], self.y), m(p[2], self.z)]
    }
}

/// Un vértice de una malla: posición y normal en la caja normalizada, y la altura (0 abajo, 1
/// arriba) para el color.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Vertice {
    pub p: [f32; 3],
    pub n: [f32; 3],
    pub h: f32,
}

/// Lo que se dibuja de una serie del espacio, en la caja normalizada.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Pieza {
    pub triangulos: Vec<[Vertice; 3]>,
    pub lineas: Vec<Vec<[f32; 3]>>,
    pub puntos: Vec<[f32; 3]>,
}

impl Pieza {
    pub fn cantidad(&self) -> usize {
        self.triangulos.len() + self.lineas.iter().map(Vec::len).sum::<usize>() + self.puntos.len()
    }
}

/// Un gráfico 3D muestreado.
#[derive(Clone, Debug, PartialEq)]
pub struct Escena {
    pub caja: Caja,
    /// Una por serie, en el orden del bloque.
    pub piezas: Vec<Pieza>,
    /// Resolución con que se muestrearon las implícitas (0 si no hay).
    pub resolucion_implicitas: u32,
}

/// Cómo muestrear.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Opciones {
    /// Puntos por lado de las mallas (`None`: los del bloque o [`RESOLUCION`]).
    pub superficies: Option<u32>,
    /// Celdas por lado de la grilla de las implícitas.
    pub implicitas: u32,
    /// La caja de un muestreo anterior: al mover un deslizador, la caja no salta.
    pub caja: Option<Caja>,
}

/// Puntos por lado de una malla si el bloque no pide otra cosa.
pub const RESOLUCION: u32 = 60;
/// Celdas por lado de las implícitas, ya refinadas, si el bloque no pide otra cosa.
pub const RESOLUCION_IMPLICITAS: u32 = 48;
/// Lo más fino para una implícita (64³ ya son 260 000 evaluaciones).
pub const MAX_IMPLICITAS: u32 = 96;
/// Los pasos del refinamiento de las implícitas: primero una grilla gruesa (el primer cuadro
/// sale enseguida) y después la fina.
pub fn niveles_implicitas(g: &Grafico) -> Vec<u32> {
    let fin = g
        .resolucion
        .unwrap_or(RESOLUCION_IMPLICITAS)
        .clamp(8, MAX_IMPLICITAS);
    let mut v: Vec<u32> = [16, 32].into_iter().filter(|n| *n < fin).collect();
    v.push(fin);
    v
}

/// ¿Hay implícitas (que conviene refinar de a poco)?
pub fn tiene_implicitas(g: &Grafico) -> bool {
    g.series
        .iter()
        .any(|s| matches!(s.tipo, Tipo::Implicita3(_)))
}

/// Puntos de una curva.
const PUNTOS_CURVA: usize = 1200;
/// Un tramo (o una arista de una malla) más largo que esto en la caja normalizada es un salto.
const SALTO: f64 = 0.6;
/// Lo más lejos de la caja que se acota un valor (en anchos de la caja).
const ACOTAR: f64 = 4.0;

#[inline]
fn vars(x: f64, y: f64, z: f64, t: f64, u: f64, v: f64) -> [f64; 7] {
    [x, y, t, 0.0, z, u, v]
}

/// Muestrea las series de un gráfico 3D.
pub fn escena(g: &Grafico, params: &[f64], o: &Opciones) -> Escena {
    let n = o
        .superficies
        .or(g.resolucion)
        .unwrap_or(RESOLUCION)
        .clamp(8, bloque::MAX_RESOLUCION) as usize;
    let gx = g.x;
    let gy = g.y.unwrap_or(g.x);
    // Primera pasada: lo que no necesita la caja entera (todo menos las implícitas), en
    // unidades del gráfico.
    let crudas: Vec<Cruda> = g
        .series
        .iter()
        .map(|s| cruda(g, s, params, n, gx, gy))
        .collect();
    let caja = o.caja.unwrap_or_else(|| caja_de(g, &crudas, gx, gy));
    let mut implicitas = 0;
    let piezas = g
        .series
        .iter()
        .zip(crudas)
        .map(|(s, c)| match (&s.tipo, c) {
            (Tipo::Implicita3(f), _) => {
                let m = o.implicitas.clamp(4, MAX_IMPLICITAS);
                implicitas = m;
                implicita(f, params, &caja, m as usize)
            }
            (_, c) => c.a_pieza(&caja),
        })
        .collect();
    Escena {
        caja,
        piezas,
        resolucion_implicitas: implicitas,
    }
}

/// Una serie antes de conocer la caja.
enum Cruda {
    Nada,
    /// Una malla de n × n (posiciones; `None`: sin definir) con los centros de cada cuadro para
    /// ver los saltos, y cómo evaluar un punto del dominio (para los bordes).
    Malla {
        n: usize,
        p: Vec<Option<[f64; 3]>>,
        centros: Vec<Option<[f64; 3]>>,
        bordes: Vec<Borde>,
        altura: bool,
    },
    Curva(Vec<Vec<[f64; 3]>>),
    Puntos(Vec<[f64; 3]>, bool),
}

/// Un punto del borde del dominio entre una esquina definida y otra que no.
struct Borde {
    /// Índices de las dos esquinas (definida, sin definir).
    a: usize,
    b: usize,
    p: [f64; 3],
}

fn cruda(
    g: &Grafico,
    s: &Serie,
    params: &[f64],
    n: usize,
    gx: (f64, f64),
    gy: (f64, f64),
) -> Cruda {
    match &s.tipo {
        Tipo::Superficie(f) => {
            let ev = |x: f64, y: f64| {
                let z = f.eval(&vars(x, y, 0.0, 0.0, 0.0, 0.0), params);
                (!z.is_nan()).then_some([x, y, z])
            };
            malla(n, gx, gy, ev, true)
        }
        Tipo::SuperficieParametrica { x, y, z, u, v } => {
            let ev = |a: f64, b: f64| {
                let vs = vars(0.0, 0.0, 0.0, 0.0, a, b);
                let p = [
                    x.eval(&vs, params),
                    y.eval(&vs, params),
                    z.eval(&vs, params),
                ];
                p.iter().all(|c| !c.is_nan()).then_some(p)
            };
            malla(n, u.unwrap_or(g.u), v.unwrap_or(g.v), ev, false)
        }
        Tipo::Curva3 { x, y, z, t } => Cruda::Curva(curva([x, y, z], t.unwrap_or(g.t), params)),
        Tipo::Puntos3(ps) => Cruda::Puntos(
            ps.iter()
                .map(|[a, b, c]| {
                    let v = [0.0; 7];
                    [a.eval(&v, params), b.eval(&v, params), c.eval(&v, params)]
                })
                .filter(|p| p.iter().all(|c| c.is_finite()))
                .collect(),
            s.estilo != crate::Estilo::Puntos,
        ),
        Tipo::Datos3(ps) => Cruda::Puntos(
            ps.iter()
                .copied()
                .filter(|p| p.iter().all(|c| c.is_finite()))
                .collect(),
            matches!(s.estilo, crate::Estilo::Linea | crate::Estilo::LineaYPuntos),
        ),
        _ => Cruda::Nada,
    }
}

/// Una malla de n × n sobre (a, b), con los centros y los bordes del dominio.
fn malla(
    n: usize,
    ra: (f64, f64),
    rb: (f64, f64),
    ev: impl Fn(f64, f64) -> Option<[f64; 3]>,
    altura: bool,
) -> Cruda {
    let n = n.max(2);
    let paso_a = (ra.1 - ra.0) / (n - 1) as f64;
    let paso_b = (rb.1 - rb.0) / (n - 1) as f64;
    let mut p = Vec::with_capacity(n * n);
    for j in 0..n {
        for i in 0..n {
            p.push(ev(ra.0 + i as f64 * paso_a, rb.0 + j as f64 * paso_b));
        }
    }
    let mut centros = Vec::with_capacity((n - 1) * (n - 1));
    for j in 0..n - 1 {
        for i in 0..n - 1 {
            centros.push(ev(
                ra.0 + (i as f64 + 0.5) * paso_a,
                rb.0 + (j as f64 + 0.5) * paso_b,
            ));
        }
    }
    // Bordes: en cada arista de la grilla entre una esquina definida y otra que no, el último
    // punto definido (bisección).
    let coord = |k: usize| {
        (
            ra.0 + (k % n) as f64 * paso_a,
            rb.0 + (k / n) as f64 * paso_b,
        )
    };
    let mut bordes = Vec::new();
    let mut arista = |a: usize, b: usize| {
        let (da, db) = (p[a].is_some(), p[b].is_some());
        if da == db {
            return;
        }
        let (bien, mal) = if da { (a, b) } else { (b, a) };
        let (mut x0, mut y0) = coord(bien);
        let (mut x1, mut y1) = coord(mal);
        let mut mejor = p[bien].expect("definida");
        for _ in 0..12 {
            let (xm, ym) = ((x0 + x1) / 2.0, (y0 + y1) / 2.0);
            match ev(xm, ym) {
                Some(q) => {
                    mejor = q;
                    (x0, y0) = (xm, ym);
                }
                None => (x1, y1) = (xm, ym),
            }
        }
        bordes.push(Borde {
            a: bien,
            b: mal,
            p: mejor,
        });
    };
    for j in 0..n {
        for i in 0..n {
            let k = j * n + i;
            if i + 1 < n {
                arista(k, k + 1);
            }
            if j + 1 < n {
                arista(k, k + n);
            }
        }
    }
    Cruda::Malla {
        n,
        p,
        centros,
        bordes,
        altura,
    }
}

/// Una curva pareja en t, cortada donde no está definida.
fn curva(f: [&Expr; 3], (t0, t1): (f64, f64), params: &[f64]) -> Vec<Vec<[f64; 3]>> {
    let mut out = Vec::new();
    let mut actual: Vec<[f64; 3]> = Vec::new();
    for k in 0..=PUNTOS_CURVA {
        let t = t0 + (t1 - t0) * k as f64 / PUNTOS_CURVA as f64;
        let v = vars(0.0, 0.0, 0.0, t, 0.0, 0.0);
        let p = [
            f[0].eval(&v, params),
            f[1].eval(&v, params),
            f[2].eval(&v, params),
        ];
        if p.iter().all(|c| c.is_finite()) {
            actual.push(p);
        } else if !actual.is_empty() {
            out.push(std::mem::take(&mut actual));
        }
    }
    if !actual.is_empty() {
        out.push(actual);
    }
    out
}

/// La caja: la pedida o la que abarca lo que se dibuja.
fn caja_de(g: &Grafico, crudas: &[Cruda], gx: (f64, f64), gy: (f64, f64)) -> Caja {
    let pedida_xy = g.x_pedida || g.series.iter().any(bloque::necesita_caja);
    // Lo que abarcan los puntos de cada eje.
    let mut valores: [Vec<f64>; 3] = [Vec::new(), Vec::new(), Vec::new()];
    for c in crudas {
        let mut sumar = |p: &[f64; 3]| {
            for k in 0..3 {
                if p[k].is_finite() {
                    valores[k].push(p[k]);
                }
            }
        };
        match c {
            Cruda::Malla { p, .. } => p.iter().flatten().for_each(&mut sumar),
            Cruda::Curva(ls) => ls.iter().flatten().for_each(&mut sumar),
            Cruda::Puntos(ps, _) => ps.iter().for_each(&mut sumar),
            Cruda::Nada => {}
        }
    }
    let ajustado = |v: &mut Vec<f64>, defecto: (f64, f64)| -> (f64, f64) {
        if v.is_empty() {
            return defecto;
        }
        v.sort_unstable_by(f64::total_cmp);
        let (min, max) = (v[0], v[v.len() - 1]);
        // Sin dejar que un polo (1/(x² + y²)) achate todo lo demás: si los extremos están muy
        // lejos de lo que abarca el 98 % de los valores, la caja se queda con ese 98 %.
        let q = |f: f64| v[((v.len() - 1) as f64 * f).round() as usize];
        let (lo, hi) = (q(0.01), q(0.99));
        let (a, b) = if max - min <= 3.0 * (hi - lo) {
            (min, max)
        } else {
            let m = 0.25 * (hi - lo);
            (lo - m, hi + m)
        };
        if b - a < 1e-9 * a.abs().max(1.0) {
            let m = a.abs().max(1.0) * 0.5;
            (a - m, b + m)
        } else {
            let m = (b - a) * 0.04;
            (a - m, b + m)
        }
    };
    let [vx, vy, vz] = &mut valores;
    let (x, y) = if pedida_xy {
        (gx, gy)
    } else {
        (ajustado(vx, gx), ajustado(vy, gy))
    };
    let z = g.z.unwrap_or_else(|| {
        // Con superficies, z se ajusta a ellas; si solo hay implícitas, como x.
        if vz.is_empty() { x } else { ajustado(vz, x) }
    });
    if !pedida_xy && g.z.is_none() && g.y.is_none() {
        // Una caja ajustada a curvas, superficies paramétricas o puntos va con la misma escala
        // en los tres ejes: un toro no se ve como un barril.
        let lado = (x.1 - x.0).max(y.1 - y.0).max(z.1 - z.0);
        let centrado = |(a, b): (f64, f64)| {
            let m = (a + b) / 2.0;
            (m - lado / 2.0, m + lado / 2.0)
        };
        return Caja {
            x: centrado(x),
            y: centrado(y),
            z: centrado(z),
        };
    }
    Caja { x, y, z }
}

impl Cruda {
    fn a_pieza(self, caja: &Caja) -> Pieza {
        match self {
            Cruda::Nada => Pieza::default(),
            Cruda::Malla {
                n,
                p,
                centros,
                bordes,
                altura,
            } => malla_a_triangulos(n, &p, &centros, &bordes, altura, caja),
            Cruda::Curva(ls) => Pieza {
                lineas: recortar_lineas(&ls, caja),
                ..Pieza::default()
            },
            Cruda::Puntos(ps, unidos) => {
                let dentro: Vec<[f32; 3]> = ps
                    .iter()
                    .map(|p| caja.normalizar(*p))
                    .filter(|q| q.iter().all(|c| (-1.0001..=1.0001).contains(c)))
                    .map(f32x3)
                    .collect();
                let lineas = if unidos {
                    recortar_lineas(&[ps], caja)
                } else {
                    Vec::new()
                };
                Pieza {
                    triangulos: Vec::new(),
                    lineas,
                    puntos: dentro,
                }
            }
        }
    }
}

#[inline]
fn f32x3(p: [f64; 3]) -> [f32; 3] {
    [p[0] as f32, p[1] as f32, p[2] as f32]
}

#[inline]
fn acotar(p: [f64; 3]) -> [f64; 3] {
    p.map(|c| c.clamp(-1.0 - 2.0 * ACOTAR, 1.0 + 2.0 * ACOTAR))
}

fn resta(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn cruz(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn largo(a: [f64; 3]) -> f64 {
    (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt()
}

fn unitario(a: [f64; 3]) -> [f32; 3] {
    let l = largo(a);
    if l > 1e-12 && l.is_finite() {
        f32x3([a[0] / l, a[1] / l, a[2] / l])
    } else {
        [0.0, 0.0, 1.0]
    }
}

/// Un vértice (en la caja normalizada) con su normal y su altura.
#[derive(Clone, Copy, Debug)]
struct V {
    p: [f64; 3],
    n: [f64; 3],
}

fn malla_a_triangulos(
    n: usize,
    p: &[Option<[f64; 3]>],
    centros: &[Option<[f64; 3]>],
    bordes: &[Borde],
    altura: bool,
    caja: &Caja,
) -> Pieza {
    let q: Vec<Option<[f64; 3]>> = p
        .iter()
        .map(|v| v.map(|v| acotar(caja.normalizar(v))))
        .collect();
    // Normales por vértice: diferencias con los vecinos (en la caja normalizada, que es donde
    // se ilumina).
    let normal = |i: usize, j: usize| -> [f64; 3] {
        let en = |i: isize, j: isize| -> Option<[f64; 3]> {
            if i < 0 || j < 0 || i >= n as isize || j >= n as isize {
                return None;
            }
            q[j as usize * n + i as usize]
        };
        let (i, j) = (i as isize, j as isize);
        let c = en(i, j).expect("definido");
        let da = match (en(i + 1, j), en(i - 1, j)) {
            (Some(a), Some(b)) => resta(a, b),
            (Some(a), None) => resta(a, c),
            (None, Some(b)) => resta(c, b),
            _ => [1.0, 0.0, 0.0],
        };
        let db = match (en(i, j + 1), en(i, j - 1)) {
            (Some(a), Some(b)) => resta(a, b),
            (Some(a), None) => resta(a, c),
            (None, Some(b)) => resta(c, b),
            _ => [0.0, 1.0, 0.0],
        };
        cruz(da, db)
    };
    let nz = |p: [f64; 3]| p[2];
    let zspan = 2.0;
    let mut tri: Vec<[Vertice; 3]> = Vec::with_capacity((n - 1) * (n - 1) * 2);
    let mut poligono: Vec<V> = Vec::with_capacity(8);
    let borde = |a: usize, b: usize| -> Option<[f64; 3]> {
        bordes
            .binary_search_by(|e| (e.a.min(e.b), e.a.max(e.b)).cmp(&(a.min(b), a.max(b))))
            .ok()
            .map(|k| acotar(caja.normalizar(bordes[k].p)))
    };
    // Normales de los vértices definidos (se calculan una vez).
    let normales: Vec<[f64; 3]> = (0..n * n)
        .map(|k| {
            if q[k].is_some() {
                normal(k % n, k / n)
            } else {
                [0.0, 0.0, 1.0]
            }
        })
        .collect();
    for j in 0..n - 1 {
        for i in 0..n - 1 {
            let esquinas = [
                j * n + i,
                j * n + i + 1,
                (j + 1) * n + i + 1,
                (j + 1) * n + i,
            ];
            let definidas = esquinas.iter().filter(|k| q[**k].is_some()).count();
            if definidas == 0 {
                continue;
            }
            // Saltos: con las cuatro esquinas, si el centro cae fuera de lo que dan (y hay
            // mucha diferencia), o si una arista es enorme.
            if definidas == 4 {
                let ps: Vec<[f64; 3]> = esquinas.iter().map(|k| q[*k].expect("4")).collect();
                let largo_max = (0..4)
                    .map(|k| largo(resta(ps[k], ps[(k + 1) % 4])))
                    .fold(0.0, f64::max);
                let mut salto = largo_max > SALTO * 4.0;
                if altura {
                    let lo = ps.iter().map(|p| nz(*p)).fold(f64::INFINITY, f64::min);
                    let hi = ps.iter().map(|p| nz(*p)).fold(f64::NEG_INFINITY, f64::max);
                    if hi - lo > 0.3 * zspan {
                        let c = centros[j * (n - 1) + i]
                            .map(|c| caja.normalizar(c)[2])
                            .unwrap_or(f64::NAN);
                        let m = 0.05 * (hi - lo);
                        if !(lo - m..=hi + m).contains(&c) {
                            salto = true;
                        }
                    }
                } else if largo_max > SALTO {
                    salto = true;
                }
                if salto {
                    continue;
                }
            }
            // El polígono de lo definido: esquinas definidas y, entre una definida y otra que
            // no, el borde.
            poligono.clear();
            for k in 0..4 {
                let (a, b) = (esquinas[k], esquinas[(k + 1) % 4]);
                if let Some(pa) = q[a] {
                    poligono.push(V {
                        p: pa,
                        n: normales[a],
                    });
                }
                if q[a].is_some() != q[b].is_some()
                    && let Some(pb) = borde(a, b)
                {
                    let cerca = if q[a].is_some() { a } else { b };
                    poligono.push(V {
                        p: pb,
                        n: normales[cerca],
                    });
                }
            }
            if poligono.len() < 3 {
                continue;
            }
            // En abanico, recortado contra la caja.
            for k in 1..poligono.len() - 1 {
                recortar_triangulo([poligono[0], poligono[k], poligono[k + 1]], &mut tri);
            }
        }
    }
    Pieza {
        triangulos: tri,
        ..Pieza::default()
    }
}

/// Recorta un triángulo contra la caja (−1..1 en cada eje) y lo agrega en abanico.
fn recortar_triangulo(t: [V; 3], out: &mut Vec<[Vertice; 3]>) {
    const L: f64 = 1.0 + 1e-6;
    let fuera = |v: &V| v.p.iter().any(|c| c.abs() > L);
    if !t.iter().any(fuera) {
        out.push(t.map(vertice));
        return;
    }
    let mut poli: Vec<V> = t.to_vec();
    let mut sig: Vec<V> = Vec::with_capacity(8);
    for eje in 0..3 {
        for lado in [-1.0f64, 1.0] {
            sig.clear();
            let d = |v: &V| L - lado * v.p[eje];
            for k in 0..poli.len() {
                let (a, b) = (poli[k], poli[(k + 1) % poli.len()]);
                let (da, db) = (d(&a), d(&b));
                if da >= 0.0 {
                    sig.push(a);
                }
                if (da >= 0.0) != (db >= 0.0) {
                    let s = da / (da - db);
                    let mezcla = |x: [f64; 3], y: [f64; 3]| {
                        [
                            x[0] + (y[0] - x[0]) * s,
                            x[1] + (y[1] - x[1]) * s,
                            x[2] + (y[2] - x[2]) * s,
                        ]
                    };
                    sig.push(V {
                        p: mezcla(a.p, b.p),
                        n: mezcla(a.n, b.n),
                    });
                }
            }
            std::mem::swap(&mut poli, &mut sig);
            if poli.len() < 3 {
                return;
            }
        }
    }
    for k in 1..poli.len() - 1 {
        out.push([poli[0], poli[k], poli[k + 1]].map(vertice));
    }
}

fn vertice(v: V) -> Vertice {
    Vertice {
        p: f32x3(v.p),
        n: unitario(v.n),
        h: ((v.p[2] + 1.0) / 2.0).clamp(0.0, 1.0) as f32,
    }
}

/// Las líneas recortadas contra la caja (Liang–Barsky), en la caja normalizada; un tramo enorme
/// es un salto y corta.
fn recortar_lineas(ls: &[Vec<[f64; 3]>], caja: &Caja) -> Vec<Vec<[f32; 3]>> {
    let mut out = Vec::new();
    for l in ls {
        let mut actual: Vec<[f32; 3]> = Vec::new();
        for w in l.windows(2) {
            let (a, b) = (caja.normalizar(w[0]), caja.normalizar(w[1]));
            if largo(resta(b, a)) > SALTO * 2.0 && largo(resta(acotar(b), acotar(a))) > SALTO {
                if actual.len() >= 2 {
                    out.push(std::mem::take(&mut actual));
                }
                actual.clear();
                continue;
            }
            match recortar_tramo(a, b) {
                Some((p, q, entero_a, entero_b)) => {
                    if !entero_a || actual.is_empty() {
                        if actual.len() >= 2 {
                            out.push(std::mem::take(&mut actual));
                        }
                        actual.clear();
                        actual.push(f32x3(p));
                    }
                    actual.push(f32x3(q));
                    if !entero_b {
                        if actual.len() >= 2 {
                            out.push(std::mem::take(&mut actual));
                        }
                        actual.clear();
                    }
                }
                None => {
                    if actual.len() >= 2 {
                        out.push(std::mem::take(&mut actual));
                    }
                    actual.clear();
                }
            }
        }
        if actual.len() >= 2 {
            out.push(actual);
        }
    }
    out
}

/// El tramo a–b dentro de la caja: (desde, hasta, ¿empieza en a?, ¿termina en b?).
fn recortar_tramo(a: [f64; 3], b: [f64; 3]) -> Option<([f64; 3], [f64; 3], bool, bool)> {
    const L: f64 = 1.0 + 1e-9;
    let d = resta(b, a);
    let (mut t0, mut t1) = (0.0f64, 1.0f64);
    for k in 0..3 {
        for (p, q) in [(-d[k], a[k] + L), (d[k], L - a[k])] {
            if p == 0.0 {
                if q < 0.0 {
                    return None;
                }
            } else {
                let r = q / p;
                if p < 0.0 {
                    t0 = t0.max(r);
                } else {
                    t1 = t1.min(r);
                }
            }
        }
    }
    if t0 > t1 {
        return None;
    }
    let en = |t: f64| [a[0] + d[0] * t, a[1] + d[1] * t, a[2] + d[2] * t];
    Some((en(t0), en(t1), t0 == 0.0, t1 == 1.0))
}

// --- Implícitas: surface nets ---------------------------------------------------------------

fn implicita(f: &Expr, params: &[f64], caja: &Caja, m: usize) -> Pieza {
    let m = m.max(2);
    let lado = m + 1;
    let coord = |k: usize, i: usize| {
        let (a, b) = caja.eje(k);
        a + (b - a) * i as f64 / m as f64
    };
    let ev = |x: f64, y: f64, z: f64| f.eval(&vars(x, y, z, 0.0, 0.0, 0.0), params);
    // Los valores en los nodos de la grilla.
    let mut val = vec![0.0f64; lado * lado * lado];
    let xs: Vec<f64> = (0..lado).map(|i| coord(0, i)).collect();
    let ys: Vec<f64> = (0..lado).map(|i| coord(1, i)).collect();
    let zs: Vec<f64> = (0..lado).map(|i| coord(2, i)).collect();
    for k in 0..lado {
        for j in 0..lado {
            for i in 0..lado {
                val[(k * lado + j) * lado + i] = ev(xs[i], ys[j], zs[k]);
            }
        }
    }
    let nodo = |i: usize, j: usize, k: usize| (k * lado + j) * lado + i;
    // Un cambio de signo entre dos nodos definidos (no los dos infinitos).
    let cruza = |a: f64, b: f64| {
        !a.is_nan()
            && !b.is_nan()
            && (a < 0.0) != (b < 0.0)
            && !(a.is_infinite() && b.is_infinite())
    };
    // Un vértice por celda que cruza: el promedio de los cruces de sus aristas.
    const ARISTAS: [([usize; 3], [usize; 3]); 12] = [
        ([0, 0, 0], [1, 0, 0]),
        ([0, 1, 0], [1, 1, 0]),
        ([0, 0, 1], [1, 0, 1]),
        ([0, 1, 1], [1, 1, 1]),
        ([0, 0, 0], [0, 1, 0]),
        ([1, 0, 0], [1, 1, 0]),
        ([0, 0, 1], [0, 1, 1]),
        ([1, 0, 1], [1, 1, 1]),
        ([0, 0, 0], [0, 0, 1]),
        ([1, 0, 0], [1, 0, 1]),
        ([0, 1, 0], [0, 1, 1]),
        ([1, 1, 0], [1, 1, 1]),
    ];
    let polo = |a: f64, b: f64, pa: [f64; 3], pb: [f64; 3]| -> bool {
        // Con una punta infinita (el polo justo en un nodo), se mira el medio.
        let finitas = a.is_finite() && b.is_finite();
        let s = if finitas { a / (a - b) } else { 0.5 };
        let p = [
            pa[0] + (pb[0] - pa[0]) * s,
            pa[1] + (pb[1] - pa[1]) * s,
            pa[2] + (pb[2] - pa[2]) * s,
        ];
        let fp = ev(p[0], p[1], p[2]);
        let tope = if finitas {
            a.abs().max(b.abs())
        } else if a.is_finite() {
            a.abs()
        } else {
            b.abs()
        };
        !fp.is_finite() || fp.abs() > tope
    };
    let paso = 2.0 / m as f64;
    let mut indice = vec![u32::MAX; m * m * m];
    let mut vertices: Vec<V> = Vec::new();
    for k in 0..m {
        for j in 0..m {
            for i in 0..m {
                let mut suma = [0.0f64; 3];
                let mut cuantos = 0;
                for (a, b) in ARISTAS {
                    let va = val[nodo(i + a[0], j + a[1], k + a[2])];
                    let vb = val[nodo(i + b[0], j + b[1], k + b[2])];
                    let punto = |c: [usize; 3]| [xs[i + c[0]], ys[j + c[1]], zs[k + c[2]]];
                    if !cruza(va, vb) || polo(va, vb, punto(a), punto(b)) {
                        continue;
                    }
                    let s = if va.is_infinite() {
                        1.0
                    } else if vb.is_infinite() {
                        0.0
                    } else {
                        (va / (va - vb)).clamp(0.0, 1.0)
                    };
                    for e in 0..3 {
                        suma[e] += a[e] as f64 + (b[e] as f64 - a[e] as f64) * s;
                    }
                    cuantos += 1;
                }
                if cuantos == 0 {
                    continue;
                }
                let c = cuantos as f64;
                let p = [
                    -1.0 + (i as f64 + suma[0] / c) * paso,
                    -1.0 + (j as f64 + suma[1] / c) * paso,
                    -1.0 + (k as f64 + suma[2] / c) * paso,
                ];
                // La normal: el gradiente de F en la celda (en la grilla, que es pareja en la
                // caja normalizada).
                let v = |a: usize, b: usize, cc: usize| val[nodo(i + a, j + b, k + cc)];
                let mut n = [
                    v(1, 0, 0) - v(0, 0, 0) + v(1, 1, 0) - v(0, 1, 0) + v(1, 0, 1) - v(0, 0, 1)
                        + v(1, 1, 1)
                        - v(0, 1, 1),
                    v(0, 1, 0) - v(0, 0, 0) + v(1, 1, 0) - v(1, 0, 0) + v(0, 1, 1) - v(0, 0, 1)
                        + v(1, 1, 1)
                        - v(1, 0, 1),
                    v(0, 0, 1) - v(0, 0, 0) + v(1, 0, 1) - v(1, 0, 0) + v(0, 1, 1) - v(0, 1, 0)
                        + v(1, 1, 1)
                        - v(1, 1, 0),
                ];
                if !n.iter().all(|c| c.is_finite()) {
                    n = [0.0, 0.0, 1.0];
                }
                indice[(k * m + j) * m + i] = vertices.len() as u32;
                vertices.push(V { p, n });
            }
        }
    }
    // Un cuadrilátero por arista de la grilla con cambio de signo, entre las cuatro celdas que
    // la comparten. Un polo (|F| en el cruce mayor que en las puntas) no es superficie.
    let celda = |i: usize, j: usize, k: usize| indice[(k * m + j) * m + i];
    let mut tri = Vec::new();
    let quad = |c: [u32; 4], tri: &mut Vec<[Vertice; 3]>| {
        if c.contains(&u32::MAX) {
            return;
        }
        let v = c.map(|k| vertices[k as usize]);
        // Por la diagonal más corta.
        let d02 = largo(resta(v[0].p, v[2].p));
        let d13 = largo(resta(v[1].p, v[3].p));
        if d02 <= d13 {
            tri.push([v[0], v[1], v[2]].map(vertice));
            tri.push([v[0], v[2], v[3]].map(vertice));
        } else {
            tri.push([v[0], v[1], v[3]].map(vertice));
            tri.push([v[1], v[2], v[3]].map(vertice));
        }
    };
    for k in 0..lado {
        for j in 0..lado {
            for i in 0..lado {
                let a = val[nodo(i, j, k)];
                let pa = [xs[i], ys[j], zs[k]];
                // Arista en x.
                if i < m && j > 0 && k > 0 && j < m && k < m {
                    let b = val[nodo(i + 1, j, k)];
                    if cruza(a, b) && !polo(a, b, pa, [xs[i + 1], ys[j], zs[k]]) {
                        quad(
                            [
                                celda(i, j - 1, k - 1),
                                celda(i, j, k - 1),
                                celda(i, j, k),
                                celda(i, j - 1, k),
                            ],
                            &mut tri,
                        );
                    }
                }
                // Arista en y.
                if j < m && i > 0 && k > 0 && i < m && k < m {
                    let b = val[nodo(i, j + 1, k)];
                    if cruza(a, b) && !polo(a, b, pa, [xs[i], ys[j + 1], zs[k]]) {
                        quad(
                            [
                                celda(i - 1, j, k - 1),
                                celda(i, j, k - 1),
                                celda(i, j, k),
                                celda(i - 1, j, k),
                            ],
                            &mut tri,
                        );
                    }
                }
                // Arista en z.
                if k < m && i > 0 && j > 0 && i < m && j < m {
                    let b = val[nodo(i, j, k + 1)];
                    if cruza(a, b) && !polo(a, b, pa, [xs[i], ys[j], zs[k + 1]]) {
                        quad(
                            [
                                celda(i - 1, j - 1, k),
                                celda(i, j - 1, k),
                                celda(i, j, k),
                                celda(i - 1, j, k),
                            ],
                            &mut tri,
                        );
                    }
                }
            }
        }
    }
    Pieza {
        triangulos: tri,
        ..Pieza::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bloque::leer;

    fn opciones(m: u32) -> Opciones {
        Opciones {
            superficies: None,
            implicitas: m,
            caja: None,
        }
    }

    #[test]
    fn superficie_dentro_de_la_caja_y_con_altura() {
        let g = leer("z = x^2 - y^2").unwrap();
        let e = escena(&g, &[], &opciones(16));
        assert_eq!(e.caja.x, (-3.0, 3.0));
        assert!(e.caja.z.0 < -8.0 && e.caja.z.1 > 8.0, "{:?}", e.caja);
        let t = &e.piezas[0].triangulos;
        assert_eq!(t.len(), 59 * 59 * 2);
        for v in t.iter().flatten() {
            assert!(v.p.iter().all(|c| c.abs() <= 1.0001));
            assert!((0.0..=1.0).contains(&v.h));
            let l = (v.n[0] * v.n[0] + v.n[1] * v.n[1] + v.n[2] * v.n[2]).sqrt();
            assert!((l - 1.0).abs() < 1e-3);
        }
    }

    #[test]
    fn superficie_con_borde_de_dominio_sin_dientes() {
        // Media esfera: el borde se busca por bisección, sin cuadros enteros de más ni de menos.
        let g = leer("x: -1..1\ny: -1..1\nz = \\sqrt{1 - x^2 - y^2}").unwrap();
        let e = escena(&g, &[], &opciones(16));
        let t = &e.piezas[0].triangulos;
        assert!(!t.is_empty());
        // Los vértices del borde están casi sobre el círculo x² + y² = 1.
        let mut en_borde = 0;
        for v in t.iter().flatten() {
            let w = e.caja.mundo(v.p);
            let r = (w[0] * w[0] + w[1] * w[1]).sqrt();
            assert!(r <= 1.0 + 1e-3, "{r}");
            if r > 0.995 {
                en_borde += 1;
            }
        }
        assert!(en_borde > 50, "{en_borde}");
    }

    #[test]
    fn polos_y_saltos_no_hacen_paredes() {
        let g = leer("x: -2..2\ny: -2..2\nz: -3..3\nz = \\tan(x + y)").unwrap();
        let e = escena(&g, &[], &opciones(16));
        // Ningún triángulo cruza la caja de abajo a arriba (las paredes de una asíntota).
        for t in &e.piezas[0].triangulos {
            let lo = t.iter().map(|v| v.p[2]).fold(f32::INFINITY, f32::min);
            let hi = t.iter().map(|v| v.p[2]).fold(f32::NEG_INFINITY, f32::max);
            assert!(hi - lo < 1.0, "pared de {lo} a {hi}");
        }
        // Un polo en una implícita: z = 1/x no tiene pared en x = 0.
        let g = leer("x: -2..2\ny: -2..2\nz: -2..2\nz - 1/x = 0").unwrap();
        let e = escena(&g, &[], &opciones(24));
        assert!(!e.piezas[0].triangulos.is_empty());
        for v in e.piezas[0].triangulos.iter().flatten() {
            let w = e.caja.mundo(v.p);
            assert!(w[0].abs() > 0.2, "vértice en la pared x = 0: {w:?}");
        }
    }

    #[test]
    fn esfera_implicita_cerrada() {
        let g = leer("x: -1.5..1.5\ny: -1.5..1.5\nz: -1.5..1.5\nx^2 + y^2 + z^2 = 1").unwrap();
        for m in [16, 32, 48] {
            let e = escena(&g, &[], &opciones(m));
            assert_eq!(e.resolucion_implicitas, m);
            let t = &e.piezas[0].triangulos;
            // Todos los vértices sobre la esfera (con el error de la grilla).
            for v in t.iter().flatten() {
                let w = e.caja.mundo(v.p);
                let r = (w[0] * w[0] + w[1] * w[1] + w[2] * w[2]).sqrt();
                assert!((r - 1.0).abs() < 3.0 / m as f64, "{r} con {m}");
            }
            // Cerrada: cada arista está en dos triángulos.
            let mut aristas = std::collections::HashMap::new();
            let clave = |v: &Vertice| v.p.map(|c| (c * 1e5).round() as i64);
            for t in t {
                for k in 0..3 {
                    let (a, b) = (clave(&t[k]), clave(&t[(k + 1) % 3]));
                    *aristas
                        .entry(if a < b { (a, b) } else { (b, a) })
                        .or_insert(0) += 1;
                }
            }
            assert!(aristas.values().all(|c| *c == 2), "abierta con {m}");
        }
    }

    #[test]
    fn parametricas_curvas_y_puntos() {
        let g = leer(
            "(\\cos u \\sin v, \\sin u \\sin v, \\cos v) ; v: 0..π\n(cos(t), sin(t), t/5) ; t: 0..4π\n(0, 0, 0), (1, 1, 1)\n(2, 2, 9)",
        )
        .unwrap();
        let e = escena(&g, &[], &opciones(16));
        // Caja ajustada a lo que se dibuja (no se pidió x): la esfera, la hélice y los puntos.
        assert!(e.caja.z.1 > 9.0 && e.caja.x.0 < -1.0);
        assert!(e.piezas[0].triangulos.len() > 3000);
        assert_eq!(e.piezas[1].lineas.len(), 1);
        assert_eq!(e.piezas[2].puntos.len(), 2);
        assert_eq!(e.piezas[3].puntos.len(), 1);
        // Con la caja pedida, el punto de afuera no está y la hélice se recorta.
        let g = leer("x: -1..1\ny: -1..1\nz: 0..1\n(cos(t), sin(t), t/5) ; t: 0..4π\n(2, 2, 9)")
            .unwrap();
        let e = escena(&g, &[], &opciones(16));
        assert!(e.piezas[1].puntos.is_empty());
        let l = &e.piezas[0].lineas;
        assert_eq!(l.len(), 1);
        assert!(l[0].iter().all(|p| p[2] <= 1.0001));
        let ultimo = l[0].last().unwrap();
        assert!(
            (ultimo[2] - 1.0).abs() < 1e-3,
            "termina en el techo: {ultimo:?}"
        );
    }

    #[test]
    fn la_caja_no_salta_con_los_deslizadores() {
        let g = leer("a = 1 (0..3)\nz = a(x^2 + y^2)").unwrap();
        let e = escena(&g, &[1.0], &opciones(16));
        let o = Opciones {
            caja: Some(e.caja),
            ..opciones(16)
        };
        let e2 = escena(&g, &[3.0], &o);
        assert_eq!(e.caja, e2.caja);
        assert!(e2.piezas[0].triangulos.len() < e.piezas[0].triangulos.len() * 2);
    }

    #[test]
    fn niveles_de_refinamiento() {
        let g = leer("x^2 + y^2 + z^2 = 1").unwrap();
        assert_eq!(niveles_implicitas(&g), vec![16, 32, 48]);
        let g = leer("resolución: 20\nx^2 + y^2 + z^2 = 1").unwrap();
        assert_eq!(niveles_implicitas(&g), vec![16, 20]);
        assert!(tiene_implicitas(&g));
    }
}
