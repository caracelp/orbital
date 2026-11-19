//! Herramienta de desarrollo del motor de fórmulas.
//!
//! - `cargo run -p orbital-formulas --example probar -- <carpeta> <tex>…` dibuja cada fórmula a
//!   `<carpeta>/pN.png` para mirarla.
//! - `cargo run --release -p orbital-formulas --example probar -- --medir <tex>…` mide, para cada
//!   fórmula, la primera vez en el proceso y la media de 200 variantes distintas (` + k` al final).

use std::time::Instant;

fn main() {
    let mut args = std::env::args().skip(1);
    let primero = args.next().expect("carpeta o --medir");
    let e = orbital_formulas::Estilo {
        tamano: 40.0,
        escala: 1.0,
        color: [0, 0, 0, 255],
    };
    if primero == "--convertir" {
        // `--convertir <desde> <hacia>`: media de 10 000 conversiones (análisis de las dos
        // expresiones incluido) y la sugerencia de destinos.
        let desde = args.next().expect("desde");
        let hacia = args.next().expect("hacia");
        let t = Instant::now();
        let mut s = 0.0;
        for k in 0..10_000 {
            s += orbital_formulas::unidades::convertir(f64::from(k), &desde, &hacia)
                .expect("conversión");
        }
        let media = t.elapsed() / 10_000;
        let t = Instant::now();
        let n = orbital_formulas::unidades::sugerencias(&desde).len();
        println!(
            "{desde} → {hacia}: media {media:?} ({s:.0}); {n} sugerencias en {:?}",
            t.elapsed()
        );
        return;
    }
    if primero == "--medir" {
        for t in args {
            let t0 = Instant::now();
            let r = orbital_formulas::dibujar(&t, orbital_formulas::Modo::EnLinea, &e);
            let frio = t0.elapsed();
            if let Err(err) = r {
                println!("{t}: {err}");
                continue;
            }
            let t1 = Instant::now();
            for k in 0..200 {
                let _ = orbital_formulas::dibujar(
                    &format!("{t} + {k}"),
                    orbital_formulas::Modo::EnLinea,
                    &e,
                );
            }
            let media = t1.elapsed() / 200;
            println!("{t}\tprimera {frio:?}\tmedia {media:?}");
        }
        return;
    }
    for (i, t) in args.enumerate() {
        match orbital_formulas::dibujar(&t, orbital_formulas::Modo::EnLinea, &e) {
            Ok(img) => {
                let f = std::fs::File::create(format!("{primero}/p{i}.png")).expect("archivo");
                let mut enc = png::Encoder::new(f, img.ancho, img.alto);
                enc.set_color(png::ColorType::Rgba);
                enc.set_depth(png::BitDepth::Eight);
                enc.write_header()
                    .and_then(|mut w| w.write_image_data(&img.rgba))
                    .expect("png");
                println!("{i} ok {t}");
            }
            Err(err) => println!("{i} ERR {t}: {err}"),
        }
    }
}
