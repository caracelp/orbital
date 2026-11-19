//! Muestreo: de las series del bloque a líneas y puntos del plano, para una vista dada.
//!
//! - Funciones `y = f(x)`, polares y paramétricas: muestreo adaptativo en píxeles. Un tramo se
//!   parte mientras el punto del medio se aleje más de medio píxel de la cuerda; en el último
//!   nivel, un salto grande es una discontinuidad (`1/x`, `tan x`, `floor x`) y corta la línea en
//!   lugar de unir la asíntota. Donde la función no está definida (`sqrt x` con x < 0) también se
//!   corta, buscando el borde por bisección.
//! - Implícitas `F(x, y) = 0`: marching squares sobre una grilla gruesa (~8 px), refinando por
//!   cuadrantes las celdas que cruza la curva hasta ~1 px; un cambio de signo con |F| creciendo
//!   (un polo, como en `y = 1/x` escrita implícita) no es curva.
//! - Todo tiene tope de puntos: una expresión rebuscada se ve más gruesa, nunca traba.

use crate::bloque::{Grafico, Serie, Tipo};
use crate::ejes::Vista;
use crate::expr::Expr;

/// Lo que se dibuja de una serie, en coordenadas del plano.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Trazo {
    /// Líneas (cada una, una sucesión de puntos unidos).
    pub lineas: Vec<Vec<[f64; 2]>>,
    /// Puntos sueltos.
    pub puntos: Vec<[f64; 2]>,
}

impl Trazo {
    pub fn cantidad(&self) -> usize {
        self.lineas.iter().map(Vec::len).sum::<usize>() + self.puntos.len()
    }
}

/// Las líneas en píxeles de `vista`, listas para dibujar: sin los puntos de los tramos que corren
/// enteros fuera de la vista del mismo lado (lo que una parábola tiene por encima) ni las líneas
/// de menos de medio píxel.
pub fn a_pixeles(lineas: &[Vec<[f64; 2]>], vista: &Vista) -> Vec<Vec<[f32; 2]>> {
    const M: f64 = 4.0;
    let codigo = |x: f64, y: f64| -> u8 {
        let mut c = 0;
        if x < -M {
            c |= 1;
        }
        if x > vista.ancho + M {
            c |= 2;
        }
        if y < -M {
            c |= 4;
        }
        if y > vista.alto + M {
            c |= 8;
        }
        c
    };
    let mut out = Vec::with_capacity(lineas.len());
    let mut px: Vec<(f64, f64, u8)> = Vec::new();
    for l in lineas {
        px.clear();
        px.extend(l.iter().map(|p| {
            let (x, y) = vista.px(p[0], p[1]);
            (x, y, codigo(x, y))
        }));
        let mut linea: Vec<[f32; 2]> = Vec::with_capacity(px.len());
        let (mut xmin, mut xmax, mut ymin, mut ymax) = (f64::MAX, f64::MIN, f64::MAX, f64::MIN);
        for k in 0..px.len() {
            let c = px[k].2;
            let antes = if k > 0 { px[k - 1].2 } else { c };
            let despues = px.get(k + 1).map_or(c, |p| p.2);
            if c != 0 && (c & antes & despues) != 0 {
                continue;
            }
            let (x, y) = (px[k].0, px[k].1);
            xmin = xmin.min(x);
            xmax = xmax.max(x);
            ymin = ymin.min(y);
            ymax = ymax.max(y);
            linea.push([x as f32, y as f32]);
        }
        if linea.len() >= 2 && (xmax - xmin > 0.5 || ymax - ymin > 0.5) {
            out.push(linea);
        }
    }
    out
}

/// Tolerancia en píxeles para partir un tramo.
const TOLERANCIA: f64 = 0.5;
/// Niveles de partición.
const NIVELES: u32 = 10;
/// Tope de puntos por serie.
const TOPE: usize = 40_000;

/// Muestrea todas las series del gráfico para `vista` con los valores de los parámetros.
pub fn muestrear(g: &Grafico, vista: &Vista, params: &[f64]) -> Vec<Trazo> {
    g.series
        .iter()
        .map(|s| muestrear_serie(g, s, vista, params))
        .collect()
}

pub fn muestrear_serie(g: &Grafico, s: &Serie, vista: &Vista, params: &[f64]) -> Trazo {
    match &s.tipo {
        Tipo::Funcion { f, dominio } => funcion(f, *dominio, vista, params),
        Tipo::Polar(f) => curva(
            |th| {
                let r = f.eval(&[0.0, 0.0, 0.0, th], params);
                [r * th.cos(), r * th.sin()]
            },
            g.theta,
            vista,
        ),
        Tipo::Parametrica(fx, fy) => curva(
            |t| {
                let v = [0.0, 0.0, t, 0.0];
                [fx.eval(&v, params), fy.eval(&v, params)]
            },
            g.t,
            vista,
        ),
        Tipo::Implicita(f) => implicita(f, vista, params),
        Tipo::Puntos(ps) => {
            let puntos: Vec<[f64; 2]> = ps
                .iter()
                .map(|(a, b)| [a.eval(&[0.0; 4], params), b.eval(&[0.0; 4], params)])
                .filter(|p| p[0].is_finite() && p[1].is_finite())
                .collect();
            // Con `estilo: ambos` o `línea`, unidos en orden.
            Trazo {
                lineas: vec![puntos.clone()],
                puntos,
            }
        }
        Tipo::Datos(ps) => Trazo {
            lineas: vec![ps.iter().map(|(x, y)| [*x, *y]).collect()],
            puntos: ps.iter().map(|(x, y)| [*x, *y]).collect(),
        },
    }
}

/// Acota un valor muy fuera de la vista (para no mandar 1e300 a la tarjeta gráfica). El error de
/// pendiente que eso introduce cae fuera de la vista.
#[inline]
fn acotar_y(y: f64, vista: &Vista) -> f64 {
    let alto = vista.y1 - vista.y0;
    y.clamp(vista.y0 - 8.0 * alto, vista.y1 + 8.0 * alto)
}

struct Lineas {
    out: Vec<Vec<[f64; 2]>>,
    actual: Vec<[f64; 2]>,
    total: usize,
}

impl Lineas {
    fn new() -> Self {
        Self {
            out: Vec::new(),
            actual: Vec::new(),
            total: 0,
        }
    }
    fn punto(&mut self, p: [f64; 2]) {
        if self.actual.last() != Some(&p) {
            self.actual.push(p);
            self.total += 1;
        }
    }
    fn cortar(&mut self) {
        if self.actual.len() >= 2 {
            self.out.push(std::mem::take(&mut self.actual));
        } else if self.actual.len() == 1 {
            // Un punto aislado no se dibuja como línea; se descarta.
            self.actual.clear();
        }
    }
    fn fin(mut self) -> Vec<Vec<[f64; 2]>> {
        self.cortar();
        self.out
    }
}

/// `y = f(x)` adaptativo en x.
fn funcion(f: &Expr, dominio: Option<(f64, f64)>, vista: &Vista, params: &[f64]) -> Trazo {
    let (mut a, mut b) = (vista.x0, vista.x1);
    if let Some((d0, d1)) = dominio {
        a = a.max(d0);
        b = b.min(d1);
        if a >= b {
            return Trazo::default();
        }
    }
    let ev = |x: f64| f.eval(&[x, 0.0, 0.0, 0.0], params);
    let (ux, uy) = vista.por_pixel();
    // Un tramo inicial cada ~2 píxeles.
    let n = (((b - a) / ux) / 2.0).ceil().clamp(16.0, 2048.0) as usize;
    let paso = (b - a) / n as f64;
    let mut l = Lineas::new();
    let mut xa = a;
    let mut ya = ev(xa);
    if ya.is_finite() {
        l.punto([xa, acotar_y(ya, vista)]);
    }
    for k in 1..=n {
        let xb = if k == n { b } else { a + paso * k as f64 };
        let yb = ev(xb);
        tramo(&ev, xa, ya, xb, yb, 0, ux, uy, vista, &mut l);
        xa = xb;
        ya = yb;
        if l.total > TOPE {
            break;
        }
    }
    Trazo {
        lineas: l.fin(),
        puntos: Vec::new(),
    }
}

/// Un tramo de una función: agrega los puntos hasta `xb` (sin `xa`, que ya está).
#[allow(clippy::too_many_arguments)]
fn tramo(
    ev: &impl Fn(f64) -> f64,
    xa: f64,
    ya: f64,
    xb: f64,
    yb: f64,
    nivel: u32,
    ux: f64,
    uy: f64,
    vista: &Vista,
    l: &mut Lineas,
) {
    let fa = ya.is_finite();
    let fb = yb.is_finite();
    if !fa && !fb {
        l.cortar();
        return;
    }
    if fa != fb {
        // Borde del dominio: se busca por bisección el último punto definido.
        if nivel >= NIVELES {
            if fb {
                l.cortar();
                l.punto([xb, acotar_y(yb, vista)]);
            } else {
                l.cortar();
            }
            return;
        }
        let xm = (xa + xb) / 2.0;
        let ym = ev(xm);
        tramo(ev, xa, ya, xm, ym, nivel + 1, ux, uy, vista, l);
        tramo(ev, xm, ym, xb, yb, nivel + 1, ux, uy, vista, l);
        return;
    }
    // Los dos extremos muy por encima o muy por debajo de la vista: nada visible que afinar.
    let alto = vista.y1 - vista.y0;
    let fuera = |y: f64| {
        if y > vista.y1 + alto {
            1
        } else if y < vista.y0 - alto {
            -1
        } else {
            0
        }
    };
    let (oa, ob) = (fuera(ya), fuera(yb));
    if oa != 0 && oa == ob && nivel > 2 {
        // Un polo entre medio no se ve (los dos lados del mismo lado), pero un cruce rápido sí:
        // se corta si el medio vuelve a la vista.
        l.punto([xb, acotar_y(yb, vista)]);
        return;
    }
    let dy_px = ((yb - ya) / uy).abs();
    if nivel >= NIVELES {
        // Último nivel: un salto de más de unos píxeles en un tramo de una fracción de píxel es
        // una discontinuidad.
        if dy_px > 4.0 {
            l.cortar();
        }
        l.punto([xb, acotar_y(yb, vista)]);
        return;
    }
    let xm = (xa + xb) / 2.0;
    let ym = ev(xm);
    let necesita = if !ym.is_finite() {
        true
    } else {
        let lineal = (ya + yb) / 2.0;
        let desvio = ((ym - lineal) / uy).abs();
        desvio > TOLERANCIA || dy_px > 64.0 || ((xb - xa) / ux) > 4.0
    };
    if necesita && l.total < TOPE {
        tramo(ev, xa, ya, xm, ym, nivel + 1, ux, uy, vista, l);
        tramo(ev, xm, ym, xb, yb, nivel + 1, ux, uy, vista, l);
    } else {
        l.punto([xb, acotar_y(yb, vista)]);
    }
}

/// Una curva `p(t)` (polar o paramétrica) adaptativa en el largo en píxeles.
fn curva(p: impl Fn(f64) -> [f64; 2], (t0, t1): (f64, f64), vista: &Vista) -> Trazo {
    let n = 256usize;
    let paso = (t1 - t0) / n as f64;
    let mut l = Lineas::new();
    let acotar = |q: [f64; 2]| {
        let ancho = vista.x1 - vista.x0;
        [
            q[0].clamp(vista.x0 - 8.0 * ancho, vista.x1 + 8.0 * ancho),
            acotar_y(q[1], vista),
        ]
    };
    let mut ta = t0;
    let mut pa = p(ta);
    if pa[0].is_finite() && pa[1].is_finite() {
        l.punto(acotar(pa));
    }
    for k in 1..=n {
        let tb = if k == n { t1 } else { t0 + paso * k as f64 };
        let pb = p(tb);
        tramo_curva(&p, ta, pa, tb, pb, 0, vista, &mut l, &acotar);
        ta = tb;
        pa = pb;
        if l.total > TOPE {
            break;
        }
    }
    Trazo {
        lineas: l.fin(),
        puntos: Vec::new(),
    }
}

#[allow(clippy::too_many_arguments)]
fn tramo_curva(
    p: &impl Fn(f64) -> [f64; 2],
    ta: f64,
    pa: [f64; 2],
    tb: f64,
    pb: [f64; 2],
    nivel: u32,
    vista: &Vista,
    l: &mut Lineas,
    acotar: &impl Fn([f64; 2]) -> [f64; 2],
) {
    let ok = |q: [f64; 2]| q[0].is_finite() && q[1].is_finite();
    let (fa, fb) = (ok(pa), ok(pb));
    if !fa && !fb {
        l.cortar();
        return;
    }
    let (ux, uy) = vista.por_pixel();
    let largo = |a: [f64; 2], b: [f64; 2]| ((b[0] - a[0]) / ux).hypot((b[1] - a[1]) / uy);
    if nivel >= NIVELES {
        if !fa || !fb || largo(pa, pb) > 6.0 {
            l.cortar();
        }
        if fb {
            l.punto(acotar(pb));
        }
        return;
    }
    let tm = (ta + tb) / 2.0;
    let pm = p(tm);
    let necesita = if !fa || !fb || !ok(pm) {
        true
    } else {
        let medio = [(pa[0] + pb[0]) / 2.0, (pa[1] + pb[1]) / 2.0];
        let d = largo(pa, pb);
        // Fuera de la vista y lejos: no hace falta afinar tanto.
        let lejos = |q: [f64; 2]| {
            let ancho = vista.x1 - vista.x0;
            let alto = vista.y1 - vista.y0;
            q[0] < vista.x0 - ancho
                || q[0] > vista.x1 + ancho
                || q[1] < vista.y0 - alto
                || q[1] > vista.y1 + alto
        };
        if lejos(pa) && lejos(pb) && lejos(pm) && nivel > 2 {
            false
        } else {
            largo(medio, pm) > TOLERANCIA || d > 6.0
        }
    };
    if necesita && l.total < TOPE {
        tramo_curva(p, ta, pa, tm, pm, nivel + 1, vista, l, acotar);
        tramo_curva(p, tm, pm, tb, pb, nivel + 1, vista, l, acotar);
    } else if fb {
        l.punto(acotar(pb));
    }
}

/// Celda gruesa de la grilla de las implícitas, en píxeles.
const CELDA: f64 = 8.0;
/// Niveles de refinamiento de una celda gruesa (8 → 4 → 2 → 1 px).
const REFINAR: u32 = 3;

/// `F(x, y) = 0` por marching squares con refinamiento.
fn implicita(f: &Expr, vista: &Vista, params: &[f64]) -> Trazo {
    let ev = |x: f64, y: f64| f.eval(&[x, y, 0.0, 0.0], params);
    let cols = ((vista.ancho / CELDA).ceil() as usize).clamp(4, 400);
    let filas = ((vista.alto / CELDA).ceil() as usize).clamp(4, 400);
    let dx = (vista.x1 - vista.x0) / cols as f64;
    let dy = (vista.y1 - vista.y0) / filas as f64;
    // Valores en las esquinas de la grilla gruesa.
    let mut v = vec![0.0f64; (cols + 1) * (filas + 1)];
    for j in 0..=filas {
        let y = vista.y0 + dy * j as f64;
        for i in 0..=cols {
            v[j * (cols + 1) + i] = ev(vista.x0 + dx * i as f64, y);
        }
    }
    let mut segs: Vec<[f64; 2]> = Vec::new();
    for j in 0..filas {
        for i in 0..cols {
            let c = [
                v[j * (cols + 1) + i],
                v[j * (cols + 1) + i + 1],
                v[(j + 1) * (cols + 1) + i + 1],
                v[(j + 1) * (cols + 1) + i],
            ];
            let x = vista.x0 + dx * i as f64;
            let y = vista.y0 + dy * j as f64;
            celda(&ev, x, y, dx, dy, c, 0, &mut segs);
            if segs.len() > TOPE {
                break;
            }
        }
    }
    // Los segmentos, de a pares, se unen en líneas donde se tocan (menos cortes al dibujar).
    let cuanto = dx.abs().min(dy.abs()) / 4096.0;
    Trazo {
        lineas: unir(segs, cuanto),
        puntos: Vec::new(),
    }
}

fn signo(v: f64) -> Option<bool> {
    if v.is_nan() { None } else { Some(v > 0.0) }
}

/// Una celda con esquinas (abajo-izq, abajo-der, arriba-der, arriba-izq).
#[allow(clippy::too_many_arguments)]
fn celda(
    ev: &impl Fn(f64, f64) -> f64,
    x: f64,
    y: f64,
    dx: f64,
    dy: f64,
    c: [f64; 4],
    nivel: u32,
    segs: &mut Vec<[f64; 2]>,
) {
    let s: Vec<Option<bool>> = c.iter().map(|v| signo(*v)).collect();
    if s.iter().any(Option::is_none) {
        // Parte sin definir: se refina para encontrar lo que sí está.
        if nivel >= REFINAR || s.iter().all(Option::is_none) {
            return;
        }
    } else {
        let todos_iguales = s.iter().all(|v| *v == s[0]);
        if todos_iguales {
            // Sin cambio de signo en las esquinas; en la grilla gruesa se mira el centro
            // (curvas chicas o tangentes que entran y salen de la celda).
            if nivel > 0 {
                return;
            }
            let m = ev(x + dx / 2.0, y + dy / 2.0);
            if signo(m) == s[0] || m.is_nan() {
                return;
            }
        }
    }
    if nivel < REFINAR {
        let (hx, hy) = (dx / 2.0, dy / 2.0);
        let m = [
            ev(x + hx, y),      // abajo medio
            ev(x + dx, y + hy), // derecha medio
            ev(x + hx, y + dy), // arriba medio
            ev(x, y + hy),      // izquierda medio
            ev(x + hx, y + hy), // centro
        ];
        celda(ev, x, y, hx, hy, [c[0], m[0], m[4], m[3]], nivel + 1, segs);
        celda(
            ev,
            x + hx,
            y,
            hx,
            hy,
            [m[0], c[1], m[1], m[4]],
            nivel + 1,
            segs,
        );
        celda(
            ev,
            x + hx,
            y + hy,
            hx,
            hy,
            [m[4], m[1], c[2], m[2]],
            nivel + 1,
            segs,
        );
        celda(
            ev,
            x,
            y + hy,
            hx,
            hy,
            [m[3], m[4], m[2], c[3]],
            nivel + 1,
            segs,
        );
        return;
    }
    if s.iter().any(Option::is_none) {
        return;
    }
    // Marching squares en la celda fina.
    let p = [[x, y], [x + dx, y], [x + dx, y + dy], [x, y + dy]];
    let mut cruces: Vec<([f64; 2], usize)> = Vec::with_capacity(4);
    for k in 0..4 {
        let (a, b) = (k, (k + 1) % 4);
        if (c[a] > 0.0) != (c[b] > 0.0) {
            // Siempre desde la esquina de abajo o de la izquierda: la celda vecina calcula el
            // mismo punto con los mismos números (para unir los segmentos).
            let (a, b) = if (p[a][0], p[a][1]) <= (p[b][0], p[b][1]) {
                (a, b)
            } else {
                (b, a)
            };
            let t = c[a] / (c[a] - c[b]);
            let q = [
                p[a][0] + (p[b][0] - p[a][0]) * t,
                p[a][1] + (p[b][1] - p[a][1]) * t,
            ];
            // Un polo cambia de signo sin pasar por cero: |F| ahí es grande.
            let fq = ev(q[0], q[1]);
            if fq.is_finite() && fq.abs() <= c[a].abs().max(c[b].abs()) {
                cruces.push((q, k));
            }
        }
    }
    match cruces.len() {
        2 => {
            segs.push(cruces[0].0);
            segs.push(cruces[1].0);
        }
        4 => {
            // Silla: el centro decide cómo se emparejan.
            let centro = ev(x + dx / 2.0, y + dy / 2.0);
            let (u, w) = if (centro > 0.0) == (c[0] > 0.0) {
                ((0, 1), (2, 3))
            } else {
                ((0, 3), (1, 2))
            };
            for (a, b) in [u, w] {
                segs.push(cruces[a].0);
                segs.push(cruces[b].0);
            }
        }
        _ => {}
    }
}

/// Une segmentos sueltos en líneas por sus extremos coincidentes.
fn unir(segs: Vec<[f64; 2]>, cuanto: f64) -> Vec<Vec<[f64; 2]>> {
    use std::collections::HashMap;
    // Puntos a menos de `cuanto` son el mismo (las esquinas se calculan con sumas distintas).
    let clave = |p: [f64; 2]| {
        (
            (p[0] / cuanto).round() as i64,
            (p[1] / cuanto).round() as i64,
        )
    };
    let n = segs.len() / 2;
    let mut usado = vec![false; n];
    let mut por_punto: HashMap<(i64, i64), Vec<usize>> = HashMap::with_capacity(n * 2);
    for k in 0..n {
        por_punto.entry(clave(segs[2 * k])).or_default().push(k);
        por_punto.entry(clave(segs[2 * k + 1])).or_default().push(k);
    }
    let otro = |k: usize, p: [f64; 2]| {
        if clave(segs[2 * k]) == clave(p) {
            segs[2 * k + 1]
        } else {
            segs[2 * k]
        }
    };
    let mut out = Vec::new();
    for inicio in 0..n {
        if usado[inicio] {
            continue;
        }
        usado[inicio] = true;
        let mut linea = std::collections::VecDeque::from([segs[2 * inicio], segs[2 * inicio + 1]]);
        // Hacia adelante y hacia atrás.
        for adelante in [true, false] {
            loop {
                let extremo = if adelante {
                    *linea.back().expect("no vacía")
                } else {
                    *linea.front().expect("no vacía")
                };
                let siguiente = por_punto
                    .get(&clave(extremo))
                    .and_then(|v| v.iter().copied().find(|k| !usado[*k]));
                let Some(k) = siguiente else { break };
                usado[k] = true;
                let q = otro(k, extremo);
                if adelante {
                    linea.push_back(q);
                } else {
                    linea.push_front(q);
                }
            }
        }
        out.push(linea.into_iter().collect());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bloque::leer;

    fn vista() -> Vista {
        Vista::nueva((-10.0, 10.0), Some((-10.0, 10.0)), 600.0, 600.0)
    }

    fn trazos(fuente: &str) -> Vec<Trazo> {
        let g = leer(fuente).unwrap();
        muestrear(&g, &vista(), &g.valores())
    }

    /// Ningún segmento de una línea cruza de un lado al otro de `x0` con un salto enorme.
    fn sin_asintota(t: &Trazo, x0: f64) {
        for l in &t.lineas {
            for w in l.windows(2) {
                if (w[0][0] - x0) * (w[1][0] - x0) < 0.0 {
                    assert!(
                        (w[1][1] - w[0][1]).abs() < 5.0,
                        "une la asíntota en {x0}: {:?}",
                        w
                    );
                }
            }
        }
    }

    #[test]
    fn discontinuidades_no_se_unen() {
        let t = &trazos("y = 1/x")[0];
        assert!(t.lineas.len() >= 2, "1/x en dos ramas: {}", t.lineas.len());
        sin_asintota(t, 0.0);
        let t = &trazos("y = \\tan x")[0];
        assert!(t.lineas.len() >= 6, "tan x en ramas: {}", t.lineas.len());
        for k in -3..=3 {
            sin_asintota(t, (k as f64 + 0.5) * std::f64::consts::PI);
        }
        let t = &trazos("y = floor(x)")[0];
        assert!(t.lineas.len() >= 18, "escalones: {}", t.lineas.len());
    }

    #[test]
    fn borde_del_dominio() {
        let t = &trazos("y = sqrt(x)")[0];
        assert_eq!(t.lineas.len(), 1);
        let primero = t.lineas[0][0];
        assert!(
            primero[0] >= 0.0 && primero[0] < 0.01,
            "empieza en 0: {primero:?}"
        );
        assert!(t.lineas[0].iter().all(|p| p[0] >= 0.0));
    }

    #[test]
    fn muestreo_adaptativo_y_acotado() {
        let recta = trazos("y = 2x + 1")[0].cantidad();
        let curva = trazos("y = sin(20x)")[0].cantidad();
        assert!(
            recta < curva,
            "la recta lleva menos puntos ({recta} < {curva})"
        );
        assert!(curva < TOPE);
        // Muy oscilante: se acota.
        let loca = trazos("y = sin(1/x)")[0].cantidad();
        assert!(loca <= TOPE + 10, "{loca}");
        for p in &trazos("y = x^2")[0].lineas[0] {
            assert!((p[1] - p[0] * p[0]).abs() < 1e-9 || p[1] > 10.0, "{p:?}");
        }
    }

    #[test]
    fn polar_y_parametrica() {
        let t = &trazos("r = 2")[0];
        assert!(!t.lineas.is_empty());
        for l in &t.lineas {
            for p in l {
                assert!((p[0].hypot(p[1]) - 2.0).abs() < 1e-9);
            }
        }
        let t = &trazos("(\\cos t, \\sin t)")[0];
        let ultimo = *t.lineas.last().unwrap().last().unwrap();
        assert!(
            (ultimo[0] - 1.0).abs() < 1e-9 && ultimo[1].abs() < 1e-9,
            "cierra"
        );
        // r = 1/cos θ es la recta x = 1 con un polo en θ = π/2: sin unir el infinito.
        let t = &trazos("r = 1/\\cos\\theta")[0];
        for l in &t.lineas {
            for p in l {
                assert!((p[0] - 1.0).abs() < 1e-6, "{p:?}");
            }
        }
    }

    #[test]
    fn implicitas() {
        let t = &trazos("x^2 + y^2 = 16")[0];
        assert!(!t.lineas.is_empty());
        let mut n = 0;
        for l in &t.lineas {
            for p in l {
                assert!((p[0].hypot(p[1]) - 4.0).abs() < 0.05, "{p:?}");
                n += 1;
            }
        }
        assert!(n > 100, "{n}");
        assert!(
            t.lineas.len() <= 8,
            "se unen los segmentos: {}",
            t.lineas.len()
        );
        // Polo de una implícita: y − 1/x = 0 cambia de signo en x = 0 sin ser curva.
        let t = &trazos("y - 1/x = 0")[0];
        assert!(!t.lineas.is_empty());
        for l in &t.lineas {
            for w in l.windows(2) {
                assert!(w[0][0] * w[1][0] >= 0.0, "cruza el eje: {w:?}");
            }
            for p in l {
                assert!(
                    p[0].abs() > 0.02 || p[1].abs() > 5.0,
                    "dibuja la asíntota: {p:?}"
                );
            }
        }
        // Una curva chica, más chica que la celda gruesa.
        let t = &trazos("x^2 + y^2 = 0.04")[0];
        assert!(!t.lineas.is_empty(), "círculo de radio 0.2 (6 px)");
    }

    #[test]
    fn puntos_y_datos() {
        let g = leer("a = 2 (0..5)\n(a, 1), (3, 4)\ndatos:\n0 1\n2 3").unwrap();
        let t = muestrear(&g, &vista(), &[4.0]);
        assert_eq!(t[0].puntos, vec![[4.0, 1.0], [3.0, 4.0]]);
        assert_eq!(t[1].lineas, vec![vec![[0.0, 1.0], [2.0, 3.0]]]);
    }
}
