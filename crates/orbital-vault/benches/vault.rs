//! Benchmarks del núcleo. Objetivos en la máquina de referencia (Celeron N5100), ver quickstart.md.
//!
//! La bóveda de prueba se genera una sola vez con el ejemplo `generar-boveda`, en la carpeta
//! temporal del sistema.

use std::path::PathBuf;
use std::process::Command;
use std::sync::OnceLock;

fn main() {
    divan::main();
}

/// Bóveda de 5000 notas, creada la primera vez que se necesita.
fn vault_5000() -> &'static PathBuf {
    static DIR: OnceLock<PathBuf> = OnceLock::new();
    DIR.get_or_init(|| {
        let dir = std::env::temp_dir().join("orbital-bench-boveda-5000");
        if !dir.join("wiki").join("index.md").exists() {
            let _ = std::fs::remove_dir_all(&dir);
            let status = Command::new(env!("CARGO"))
                .args([
                    "run",
                    "--release",
                    "-q",
                    "-p",
                    "orbital-vault",
                    "--example",
                    "generar-boveda",
                    "--",
                ])
                .arg(&dir)
                .arg("5000")
                .status()
                .expect("no se pudo ejecutar el generador");
            assert!(status.success(), "el generador falló");
        }
        dir
    })
}

/// Recorrido de la bóveda (objetivo: < 100 ms).
#[divan::bench(sample_count = 20)]
fn scan_5000(bencher: divan::Bencher) {
    let dir = vault_5000();
    bencher.bench(|| {
        orbital_vault::scan::scan(divan::black_box(dir))
            .unwrap()
            .entries
            .len()
    });
}

/// Apertura completa: recorrido, estructura y configuración.
#[divan::bench(sample_count = 20)]
fn open_5000(bencher: divan::Bencher) {
    let dir = vault_5000();
    bencher.bench(|| {
        orbital_vault::open(divan::black_box(dir))
            .unwrap()
            .entries()
            .len()
    });
}
