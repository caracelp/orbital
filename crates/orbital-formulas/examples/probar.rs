//! Dibuja fórmulas a PNG para mirarlas: `cargo run -p orbital-formulas --example probar -- <carpeta> <tex>…`.

fn main() {
    let mut args = std::env::args().skip(1);
    let dir = args.next().expect("carpeta");
    let e = orbital_formulas::Estilo {
        tamano: 40.0,
        escala: 1.0,
        color: [0, 0, 0, 255],
    };
    for (i, t) in args.enumerate() {
        match orbital_formulas::dibujar(&t, orbital_formulas::Modo::EnLinea, &e) {
            Ok(img) => {
                let f = std::fs::File::create(format!("{dir}/p{i}.png")).expect("archivo");
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
