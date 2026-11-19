//! Mide los gráficos 3D (053, desarrollo):
//!
//! ```text
//! cargo run --release -p orbital-graficos --example medir3d             # tiempos por tipo
//! cargo run --release -p orbital-graficos --example medir3d -- --svg <carpeta>   # SVG y PAM de cada uno
//! ```
//!
//! También compara, para las implícitas, *surface nets* (lo que usa Orbital) con *marching
//! tetrahedra* (seis tetraedros por cubo, sin tablas que copiar), escrito acá solo para medir.

use std::time::Instant;

use orbital_graficos::camara::{Camara, Proyeccion};
use orbital_graficos::espacio::{Opciones, escena, niveles_implicitas};
use orbital_graficos::raster::{Imagen, rasterizar, rellenos};
use orbital_graficos::{Paleta, Tipo, leer, svg};

const CASOS: &[(&str, &str)] = &[
    ("superficie", "z = \\sin(x) \\cos(y)"),
    ("silla", "a = 1 (0..2)\nz = a(x^2 - y^2)"),
    ("polo", "z = \\frac{1}{x^2 + y^2}"),
    (
        "media-esfera",
        "x: -1..1\ny: -1..1\nz = \\sqrt{1 - x^2 - y^2}",
    ),
    ("curva", "(\\cos t, \\sin t, t/6) ; t: 0..6π"),
    (
        "esfera",
        "(\\cos u \\sin v, \\sin u \\sin v, \\cos v) ; v: 0..π",
    ),
    (
        "toro",
        "((2 + \\cos v)\\cos u, (2 + \\cos v)\\sin u, \\sin v)",
    ),
    (
        "mobius",
        "u: 0..2π\nv: -0.4..0.4\n((1 + v\\cos(u/2))\\cos u, (1 + v\\cos(u/2))\\sin u, v\\sin(u/2))",
    ),
    (
        "implicita-esfera",
        "x: -1.2..1.2\ny: -1.2..1.2\nz: -1.2..1.2\nx^2 + y^2 + z^2 = 1",
    ),
    (
        "implicita-gyroide",
        "x: -4..4\ny: -4..4\nz: -4..4\n\\sin x \\cos y + \\sin y \\cos z + \\sin z \\cos x = 0",
    ),
    (
        "nube",
        "datos:\n0 0 0\n1 2 3\n2 1 1\n3 3 2\n1 1 4\n2 3 0\n0 2 2",
    ),
];

fn mediana(mut v: Vec<f64>) -> f64 {
    v.sort_by(f64::total_cmp);
    v[v.len() / 2]
}

fn medir(n: usize, mut f: impl FnMut()) -> f64 {
    let mut t = Vec::with_capacity(n);
    for _ in 0..n {
        let i = Instant::now();
        f();
        t.push(i.elapsed().as_secs_f64() * 1e3);
    }
    mediana(t)
}

/// Marching tetrahedra (solo para comparar): devuelve la cantidad de triángulos.
fn tetraedros(f: &orbital_graficos::expr::Expr, m: usize, caja: [(f64, f64); 3]) -> usize {
    let lado = m + 1;
    let c = |k: usize, i: usize| caja[k].0 + (caja[k].1 - caja[k].0) * i as f64 / m as f64;
    let mut val = vec![0.0f64; lado * lado * lado];
    for k in 0..lado {
        for j in 0..lado {
            for i in 0..lado {
                val[(k * lado + j) * lado + i] =
                    f.eval(&[c(0, i), c(1, j), 0.0, 0.0, c(2, k), 0.0, 0.0], &[]);
            }
        }
    }
    const CUBO: [[usize; 3]; 8] = [
        [0, 0, 0],
        [1, 0, 0],
        [1, 1, 0],
        [0, 1, 0],
        [0, 0, 1],
        [1, 0, 1],
        [1, 1, 1],
        [0, 1, 1],
    ];
    const TETRA: [[usize; 4]; 6] = [
        [0, 5, 1, 6],
        [0, 1, 2, 6],
        [0, 2, 3, 6],
        [0, 3, 7, 6],
        [0, 7, 4, 6],
        [0, 4, 5, 6],
    ];
    let mut triangulos = 0usize;
    let mut puntos: Vec<[f64; 3]> = Vec::new();
    for k in 0..m {
        for j in 0..m {
            for i in 0..m {
                let v: Vec<f64> = CUBO
                    .iter()
                    .map(|d| val[((k + d[2]) * lado + j + d[1]) * lado + i + d[0]])
                    .collect();
                for t in TETRA {
                    let dentro: Vec<usize> = t.iter().copied().filter(|q| v[*q] < 0.0).collect();
                    if dentro.is_empty() || dentro.len() == 4 {
                        continue;
                    }
                    puntos.clear();
                    for &a in &t {
                        for &b in &t {
                            if a < b && (v[a] < 0.0) != (v[b] < 0.0) {
                                let s = v[a] / (v[a] - v[b]);
                                let pa = CUBO[a];
                                let pb = CUBO[b];
                                puntos.push(
                                    [0, 1, 2]
                                        .map(|e| pa[e] as f64 + (pb[e] as f64 - pa[e] as f64) * s),
                                );
                            }
                        }
                    }
                    triangulos += if puntos.len() == 4 { 2 } else { 1 };
                }
            }
        }
    }
    std::hint::black_box(&puntos);
    triangulos
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some("--svg") {
        let dir = std::path::PathBuf::from(args.get(1).expect("carpeta"));
        std::fs::create_dir_all(&dir).expect("carpeta");
        for (nombre, fuente) in CASOS {
            let g = leer(fuente).expect(nombre);
            let s = svg::svg(&g, &svg::Opciones::default());
            std::fs::write(dir.join(format!("{nombre}.svg")), &s).expect("escribir");
            // La imagen de la app (PAM, sin compresión), para mirarla.
            let e = escena(
                &g,
                &g.valores(),
                &Opciones {
                    superficies: None,
                    implicitas: *niveles_implicitas(&g).last().unwrap(),
                    caja: None,
                },
            );
            let p = Proyeccion::new(&Camara::inicial(&g), 640.0, 360.0);
            let mut img = Imagen::new();
            rasterizar(
                &e,
                &rellenos(&g, Paleta::Oscura),
                &p,
                Paleta::Oscura,
                2,
                &mut img,
            );
            let mut pam = format!(
                "P7\nWIDTH {}\nHEIGHT {}\nDEPTH 4\nMAXVAL 255\nTUPLTYPE RGB_ALPHA\nENDHDR\n",
                img.ancho, img.alto
            )
            .into_bytes();
            for px in img.bytes().as_chunks::<4>().0 {
                pam.extend_from_slice(&[px[2], px[1], px[0], px[3]]);
            }
            std::fs::write(dir.join(format!("{nombre}.pam")), pam).expect("escribir");
        }
        return;
    }
    let (w, h) = (640.0, 360.0);
    println!(
        "caso                 leer µs  escena ms  triángulos  pintar 1× ms  pintar 2× ms  svg ms  svg KB"
    );
    for (nombre, fuente) in CASOS {
        let t = Instant::now();
        let g = leer(fuente).expect(nombre);
        let t_leer = t.elapsed().as_secs_f64() * 1e6;
        let p = g.valores();
        let nivel = *niveles_implicitas(&g).last().unwrap();
        let o = Opciones {
            superficies: None,
            implicitas: nivel,
            caja: None,
        };
        let t_escena = medir(10, || {
            std::hint::black_box(escena(&g, &p, &o));
        });
        let e = escena(&g, &p, &o);
        let tri: usize = e.piezas.iter().map(|p| p.triangulos.len()).sum();
        let rell = rellenos(&g, Paleta::Oscura);
        let mut img = Imagen::new();
        let mut cam = Camara::inicial(&g);
        let t1 = medir(30, || {
            cam = cam.girar(3.0, 0.0);
            let pr = Proyeccion::new(&cam, w, h);
            rasterizar(&e, &rell, &pr, Paleta::Oscura, 1, &mut img);
            std::hint::black_box(img.bytes());
        });
        let t2 = medir(10, || {
            cam = cam.girar(3.0, 0.0);
            let pr = Proyeccion::new(&cam, w, h);
            rasterizar(&e, &rell, &pr, Paleta::Oscura, 2, &mut img);
            std::hint::black_box(img.bytes());
        });
        let mut largo = 0;
        let t_svg = medir(5, || {
            largo = svg::svg(&g, &svg::Opciones::default()).len();
        });
        println!(
            "{nombre:<20} {t_leer:>7.0}  {t_escena:>9.2}  {tri:>10}  {t1:>12.2}  {t2:>12.2}  {t_svg:>6.1}  {:>6.0}",
            largo as f64 / 1024.0
        );
    }
    // Implícitas: el primer cuadro (grilla gruesa) y la refinada, y la comparación con
    // marching tetrahedra.
    println!(
        "\nimplícita            nivel  escena ms  triángulos (surface nets)  tetraedros ms  triángulos (tetraedros)"
    );
    for (nombre, fuente) in CASOS {
        let g = leer(fuente).expect(nombre);
        let Some(f) = g.series.iter().find_map(|s| match &s.tipo {
            Tipo::Implicita3(f) => Some(f.clone()),
            _ => None,
        }) else {
            continue;
        };
        for m in [16u32, 32, 48, 64] {
            let o = Opciones {
                superficies: None,
                implicitas: m,
                caja: None,
            };
            let t = medir(5, || {
                std::hint::black_box(escena(&g, &[], &o));
            });
            let e = escena(&g, &[], &o);
            let tri: usize = e.piezas.iter().map(|p| p.triangulos.len()).sum();
            let caja = [e.caja.x, e.caja.y, e.caja.z];
            let mut tt = 0;
            let tm = medir(5, || {
                tt = tetraedros(&f, m as usize, caja);
            });
            println!("{nombre:<20} {m:>5}  {t:>9.2}  {tri:>25}  {tm:>13.2}  {tt:>23}");
        }
    }
}
