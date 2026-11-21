//! Mide el motor de diagramas (058): carga de fuentes, SVG e imagen de cada archivo `.mmd` de una
//! carpeta, en frío (la primera vez del proceso) y en caliente (mediana de 20).
//!
//! `cargo run --release -p orbital-diagramas --example medir -- <carpeta con .mmd> [salida]`
//! Con `salida`, guarda cada imagen como PNG (oscuro y claro) para mirarla.

use std::time::Instant;

use orbital_diagramas::{Fuentes, Tema, imagen, svg};

fn mediana(mut v: Vec<f64>) -> f64 {
    v.sort_by(f64::total_cmp);
    v[v.len() / 2]
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let dir = args.get(1).expect("carpeta con .mmd");
    let salida = args.get(2);
    let familia = std::env::var("ORBITAL_FAMILIA").unwrap_or_else(|_| "Adwaita Sans".into());
    let t = Instant::now();
    let f = Fuentes::para(&familia);
    println!(
        "fuentes: {:.1} ms ({} caras, familia «{}»)",
        t.elapsed().as_secs_f64() * 1e3,
        f.caras(),
        f.familia()
    );
    let mut archivos: Vec<_> = std::fs::read_dir(dir)
        .unwrap()
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|e| e == "mmd"))
        .collect();
    archivos.sort();
    println!("archivo\tSVG frío\tSVG\timagen fría\timagen\tpx\tKB SVG");
    for p in archivos {
        let fuente = std::fs::read_to_string(&p).unwrap();
        let nombre = p.file_stem().unwrap().to_string_lossy();
        let t = Instant::now();
        let r = svg(&fuente, Tema::Oscuro);
        let svg_frio = t.elapsed().as_secs_f64() * 1e3;
        let d = match r {
            Ok(d) => d,
            Err(e) => {
                println!("{nombre}\t{svg_frio:.2} ms\t{e}");
                continue;
            }
        };
        let svg_cal = mediana(
            (0..20)
                .map(|_| {
                    let t = Instant::now();
                    let _ = svg(&fuente, Tema::Oscuro);
                    t.elapsed().as_secs_f64() * 1e3
                })
                .collect(),
        );
        let t = Instant::now();
        let img = imagen(&d, 1.0, &f).unwrap();
        let img_frio = t.elapsed().as_secs_f64() * 1e3;
        let img_cal = mediana(
            (0..20)
                .map(|_| {
                    let t = Instant::now();
                    let _ = imagen(&d, 1.0, &f);
                    t.elapsed().as_secs_f64() * 1e3
                })
                .collect(),
        );
        println!(
            "{nombre}\t{svg_frio:.2} ms\t{svg_cal:.2} ms\t{img_frio:.2} ms\t{img_cal:.2} ms\t{}x{}\t{:.1}",
            img.ancho,
            img.alto,
            d.marcado.len() as f64 / 1024.0
        );
        if let Some(s) = salida {
            for (tema, sufijo, fondo) in [
                (Tema::Oscuro, "oscuro", [0x16, 0x16, 0x18]),
                (Tema::Claro, "claro", [0xff, 0xff, 0xff]),
            ] {
                let d = svg(&fuente, tema).unwrap();
                let i = imagen(&d, 1.5, &f).unwrap();
                guardar(&format!("{s}/{nombre}-{sufijo}.ppm"), &i, fondo);
            }
        }
    }
}

/// PPM sobre un fondo (sin dependencias para escribir PNG).
fn guardar(ruta: &str, i: &orbital_diagramas::Imagen, fondo: [u8; 3]) {
    let mut out = format!("P6\n{} {}\n255\n", i.ancho, i.alto).into_bytes();
    for p in i.rgba.chunks(4) {
        let a = u32::from(p[3]);
        for k in 0..3 {
            out.push((u32::from(p[k]) + u32::from(fondo[k]) * (255 - a) / 255) as u8);
        }
    }
    std::fs::write(ruta, out).unwrap();
}
