//! Mide el motor de gráficos (desarrollo):
//!
//! ```text
//! cargo run --release -p orbital-graficos --example medir            # tiempos por tipo
//! cargo run --release -p orbital-graficos --example medir -- --svg <carpeta>   # SVG de cada uno
//! ```

use std::time::Instant;

use orbital_graficos::{Vista, expr::Expr, leer, muestrear, svg};

const CASOS: &[(&str, &str)] = &[
    ("funcion", "y = x^2"),
    ("seno", "y = \\sin(3x) + \\frac{x}{4}"),
    ("asintotas", "y = \\tan x\ny = 1/x"),
    ("polar", "r = 1 + 2\\cos(3\\theta)"),
    ("parametrica", "(\\cos 3t, \\sin 2t)"),
    ("implicita", "x^2 + y^2 = 16"),
    ("implicita-dificil", "\\sin(x) + \\cos(y) = 0.5"),
    ("puntos", "(1, 2), (3, 4), (5, 1)"),
    ("datos", "datos: líneas\n0 0\n1 1\n2 4\n3 9\n4 16"),
    (
        "todo",
        "a = 1 (0..5)\ny = a\\sin x\nr = 2 + \\cos\\theta\n(\\cos t, \\sin t)\nx^2 - y^2 = 4\n(1, 1), (2, 3)",
    ),
];

fn mediana(mut v: Vec<f64>) -> f64 {
    v.sort_by(f64::total_cmp);
    v[v.len() / 2]
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some("--svg") {
        let dir = std::path::PathBuf::from(args.get(1).expect("carpeta"));
        std::fs::create_dir_all(&dir).expect("carpeta");
        for (nombre, fuente) in CASOS {
            let g = leer(fuente).expect(nombre);
            let s = svg::svg(&g, &svg::Opciones::default());
            let html = format!(
                "<!doctype html><meta charset=utf-8><body style=\"background:#fff;color:#1a1a1c\">{s}</body>"
            );
            std::fs::write(dir.join(format!("{nombre}.html")), html).expect("escribir");
        }
        return;
    }
    let v = Vista::nueva((-10.0, 10.0), None, 700.0, 300.0);
    println!("caso                 leer (µs)  muestrear (ms, mediana de 30)  puntos   svg (ms)");
    for (nombre, fuente) in CASOS {
        let t = Instant::now();
        let g = leer(fuente).expect(nombre);
        let t_leer = t.elapsed().as_secs_f64() * 1e6;
        let p = g.valores();
        let mut tiempos = Vec::new();
        let mut puntos = 0;
        for _ in 0..30 {
            let t = Instant::now();
            let tr = muestrear(&g, &v, &p);
            tiempos.push(t.elapsed().as_secs_f64() * 1e3);
            puntos = tr.iter().map(|t| t.cantidad()).sum();
        }
        let t = Instant::now();
        let s = svg::svg(&g, &svg::Opciones::default());
        let t_svg = t.elapsed().as_secs_f64() * 1e3;
        println!(
            "{nombre:<20} {t_leer:>9.1}  {:>12.3}                  {puntos:>7}  {t_svg:>8.3} ({} KB)",
            mediana(tiempos),
            s.len() / 1024
        );
    }
    // Evaluación sola: 10⁶ evaluaciones de cada expresión.
    for texto in [
        "sin(x)/x + x^2",
        "\\frac{\\sin x}{x} + x^{2}",
        "a*sin(b*x) + c*x^3 - sqrt(abs(x))",
    ] {
        let e = Expr::leer(texto, &["a", "b", "c"]).expect("expresión");
        let params = [1.5, 2.0, 0.1];
        let mut suma = 0.0;
        let t = Instant::now();
        for k in 0..1_000_000 {
            let x = -10.0 + k as f64 * 2e-5;
            suma += e.eval(&[x, 0.0, 0.0, 0.0], &params);
        }
        let ns = t.elapsed().as_secs_f64() * 1e9 / 1e6;
        println!("eval {texto:<40} {ns:>6.1} ns ({suma:.3})");
    }
}
