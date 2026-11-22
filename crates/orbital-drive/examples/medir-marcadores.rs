//! Costo de escanear lo «solo en Drive» (quickstart de la 061b):
//! `cargo run --release -p orbital-drive --example medir-marcadores -- [N] [notas]`.
//!
//! Arma una bóveda de `notas` notas en disco (5000 por omisión) y un archivo de marcadores con N
//! archivos «solo en Drive» (5000 por omisión; la mitad notas con extracto). Mide lo que la app
//! hace en el hilo del escaneo: leer los marcadores, sumarlos a la bóveda y registrar sus copias;
//! y aparte, el costo de `RelPath::to_path` con y sin nada registrado. Mediana de 15 vueltas.

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::time::Instant;

use orbital_drive::estado::{self, Marcador};
use orbital_vault::{RelPath, ubicacion};

fn mediana(mut v: Vec<f64>) -> f64 {
    v.sort_by(|a, b| a.total_cmp(b));
    v[v.len() / 2]
}

fn main() {
    let mut args = std::env::args().skip(1);
    let n: usize = args.next().and_then(|a| a.parse().ok()).unwrap_or(5000);
    let notas: usize = args.next().and_then(|a| a.parse().ok()).unwrap_or(5000);
    let base = std::env::temp_dir().join(format!("orbital-061b-marcadores-{}", std::process::id()));
    let root = base.join("boveda");
    let dir = base.join("estado");
    let cache = base.join("cache");
    for i in 0..notas {
        let p = root.join(format!("wiki/carpeta-{}/nota-{i}.md", i % 50));
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, format!("# Nota {i}\n\n[[nota-{}]]\n", (i + 1) % notas)).unwrap();
    }
    fs::create_dir_all(root.join("raw")).unwrap();
    let marcas: Vec<Marcador> = (0..n)
        .map(|i| {
            let (ruta, nombre) = if i % 2 == 0 {
                (
                    format!("raw/videos-{}/clip-{i}.mp4", i % 40),
                    format!("clip-{i}.mp4"),
                )
            } else {
                (
                    format!("wiki/drive-{}/remota-{i}.md", i % 40),
                    format!("remota-{i}.md"),
                )
            };
            Marcador {
                ruta,
                id: format!("1AbCdEfGhIjKlMnOpQrStUvWx{i:06}"),
                tam: 1000 + i as u64,
                mtime: 1_790_000_000 + i as i64,
                nombre,
            }
        })
        .collect();
    estado::guardar_marcadores(&dir, &marcas).unwrap();
    let tam_archivo = fs::metadata(dir.join("marcadores")).unwrap().len();

    let (mut t_leer, mut t_sumar, mut t_registrar, mut t_abrir) = (vec![], vec![], vec![], vec![]);
    for _ in 0..15 {
        let t = Instant::now();
        let v0 = orbital_vault::open(&root).unwrap();
        t_abrir.push(t.elapsed().as_secs_f64() * 1e3);
        let mut v = v0;
        let t = Instant::now();
        let m = estado::leer_marcadores(&dir);
        t_leer.push(t.elapsed().as_secs_f64() * 1e3);
        let t = Instant::now();
        let remotos: Vec<(RelPath, u64, Option<std::time::SystemTime>)> = m
            .iter()
            .filter_map(|x| {
                let p = RelPath::new(&x.ruta).ok()?;
                let t = std::time::UNIX_EPOCH + std::time::Duration::from_secs(x.mtime as u64);
                Some((p, x.tam, Some(t)))
            })
            .collect();
        v.agregar_remotos(&remotos);
        t_sumar.push(t.elapsed().as_secs_f64() * 1e3);
        let t = Instant::now();
        let mapa: HashMap<String, PathBuf> = m
            .iter()
            .map(|x| (x.ruta.clone(), estado::ruta_copia(&cache, &x.id, &x.nombre)))
            .collect();
        if !ubicacion::igual(&root, &mapa) {
            ubicacion::registrar(&root, mapa);
        }
        t_registrar.push(t.elapsed().as_secs_f64() * 1e3);
        assert_eq!(
            v.entries()
                .iter()
                .filter(|e| e.kind != orbital_vault::EntryKind::Folder)
                .count(),
            notas + n
        );
        ubicacion::registrar(&root, HashMap::new());
    }
    // `to_path` de todas las entradas: sin nada registrado y con los N registrados.
    let v = orbital_vault::open(&root).unwrap();
    let rutas: Vec<RelPath> = v.entries().iter().map(|e| e.path.clone()).collect();
    let vuelta = |rutas: &[RelPath]| {
        let t = Instant::now();
        let mut x = 0usize;
        for r in rutas {
            x += r.to_path(&root).as_os_str().len();
        }
        std::hint::black_box(x);
        t.elapsed().as_secs_f64() * 1e3
    };
    let sin = mediana((0..15).map(|_| vuelta(&rutas)).collect());
    let mapa: HashMap<String, PathBuf> = marcas
        .iter()
        .map(|x| (x.ruta.clone(), estado::ruta_copia(&cache, &x.id, &x.nombre)))
        .collect();
    ubicacion::registrar(&root, mapa);
    let con = mediana((0..15).map(|_| vuelta(&rutas)).collect());
    ubicacion::registrar(&root, HashMap::new());

    println!(
        "marcadores: {n} (archivo de {} KB), bóveda: {notas} notas",
        tam_archivo / 1024
    );
    println!("abrir la bóveda (referencia): {:.2} ms", mediana(t_abrir));
    println!("leer los marcadores:          {:.2} ms", mediana(t_leer));
    println!("sumarlos a la bóveda:         {:.2} ms", mediana(t_sumar));
    println!(
        "registrar sus copias:         {:.2} ms",
        mediana(t_registrar)
    );
    println!(
        "to_path de {} entradas: {:.3} ms sin nada registrado, {:.3} ms con {n} registrados",
        rutas.len(),
        sin,
        con
    );
    fs::remove_dir_all(&base).unwrap();
}
