//! Mediciones del motor contra el servidor falso (quickstart de la 061):
//! `cargo run --release -p orbital-drive --features servidor-falso --example medir -- [N]`.
//!
//! Arma una bóveda de N notas (5000 por omisión) en una carpeta temporal, la sube entera, y mide
//! el ciclo sin cambios (solo la lista de cambios), el ciclo con recorrido completo, el estado del
//! equipo (tamaño y lectura) y el mapa de estados para el explorador. La CPU es la del proceso
//! entero (el servidor falso corre en el mismo proceso: es una cota superior).

use std::fs;
use std::time::Instant;

use orbital_drive::cliente::{Cliente, Credenciales, Direcciones};
use orbital_drive::falso::{CUENTA, ServidorFalso};
use orbital_drive::motor::{Config, Destino, Sincronizador, conectar};
use orbital_drive::{Conexion, Estado, Papelera};

fn cpu_ms() -> f64 {
    let s = fs::read_to_string("/proc/self/stat").unwrap();
    let f: Vec<&str> = s.rsplit(')').next().unwrap().split_whitespace().collect();
    let tics = f[11].parse::<f64>().unwrap() + f[12].parse::<f64>().unwrap();
    tics * 10.0
}

fn rss_kb() -> u64 {
    fs::read_to_string("/proc/self/status")
        .unwrap()
        .lines()
        .find_map(|l| l.strip_prefix("VmRSS:"))
        .and_then(|v| v.split_whitespace().next()?.parse().ok())
        .unwrap_or(0)
}

fn main() {
    let n: usize = std::env::args()
        .nth(1)
        .and_then(|a| a.parse().ok())
        .unwrap_or(5000);
    let base = std::env::temp_dir().join(format!("orbital-061-medir-{}", std::process::id()));
    let root = base.join("boveda");
    let dir_estado = base.join("estado");
    for i in 0..n {
        let p = root.join(format!("wiki/carpeta-{:02}/nota-{i:05}.md", i % 50));
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        let cuerpo = "Texto de relleno para que la nota tenga un tamaño parecido al real. "
            .repeat(20 + i % 40);
        fs::write(
            p,
            format!("---\ntype: atomo\ntitle: Nota {i}\n---\n\n# Nota {i}\n\n{cuerpo}\n"),
        )
        .unwrap();
    }
    let srv = ServidorFalso::iniciar();
    let cliente = || {
        Cliente::new(
            Direcciones::locales(&srv.url),
            Credenciales {
                client_id: "x".into(),
                client_secret: "x".into(),
                refresh_token: "x".into(),
            },
        )
    };
    let mut c = cliente();
    conectar(
        &mut c,
        &root,
        &dir_estado,
        CUENTA,
        Destino::Nueva("Medir".into()),
    )
    .unwrap();
    let cfg = Config {
        papelera: Papelera::Boveda,
        dir_estado: dir_estado.clone(),
        ..Config::new(&root)
    };
    let rss0 = rss_kb();
    let mut s = Sincronizador::new(
        cfg.clone(),
        cliente(),
        Estado::leer(&dir_estado).unwrap().unwrap(),
        Conexion::leer(&root).unwrap().unwrap(),
        Box::new(|_| {}),
    );
    let (t, c0, p0) = (Instant::now(), cpu_ms(), srv.datos().pedidos);
    s.ciclo().unwrap();
    println!(
        "Subida inicial de {n} notas: {:.1} s, CPU {:.0} ms (con el servidor), {} pedidos",
        t.elapsed().as_secs_f64(),
        cpu_ms() - c0,
        srv.datos().pedidos - p0
    );
    let rss1 = rss_kb();
    // Ciclos sin cambios (el primero guarda el estado de la subida: se descarta).
    s.ciclo().unwrap();
    let mut tiempos = Vec::new();
    let c0 = cpu_ms();
    for _ in 0..100 {
        let t = Instant::now();
        s.ciclo().unwrap();
        tiempos.push(t.elapsed().as_secs_f64() * 1000.0);
    }
    let cpu = (cpu_ms() - c0) / 100.0;
    tiempos.sort_by(f64::total_cmp);
    println!(
        "Ciclo sin cambios (solo `changes`): mediana {:.2} ms, p99 {:.2} ms; CPU {:.2} ms por ciclo (100 ciclos, motor y servidor juntos)",
        tiempos[50], tiempos[98], cpu
    );
    let mut tiempos = Vec::new();
    for _ in 0..10 {
        let t = Instant::now();
        s.pedir_completa();
        s.ciclo().unwrap();
        tiempos.push(t.elapsed().as_secs_f64() * 1000.0);
    }
    tiempos.sort_by(f64::total_cmp);
    println!(
        "Ciclo con recorrido completo de la bóveda (sin cambios): mediana {:.1} ms, máx {:.1} ms",
        tiempos[5], tiempos[9]
    );
    let t = Instant::now();
    let m = s.mapa();
    println!(
        "Mapa de estados para el explorador ({} rutas): {:.2} ms",
        m.len(),
        t.elapsed().as_secs_f64() * 1000.0
    );
    drop(s);
    let tam = fs::metadata(dir_estado.join("estado.txt")).unwrap().len();
    let mut tiempos = Vec::new();
    for _ in 0..10 {
        let t = Instant::now();
        let e = Estado::leer(&dir_estado).unwrap().unwrap();
        tiempos.push(t.elapsed().as_secs_f64() * 1000.0);
        std::hint::black_box(e);
    }
    tiempos.sort_by(f64::total_cmp);
    println!(
        "Estado del equipo: {:.0} KB en disco, lectura {:.1} ms (mediana de 10); memoria del motor y su estado ≈ {} KB",
        tam as f64 / 1024.0,
        tiempos[5],
        rss1.saturating_sub(rss0)
    );
    let _ = fs::remove_dir_all(&base);
}
