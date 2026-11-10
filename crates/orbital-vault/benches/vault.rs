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

/// Notas de la bóveda de 5000.
fn note_paths() -> &'static Vec<orbital_vault::RelPath> {
    static PATHS: OnceLock<Vec<orbital_vault::RelPath>> = OnceLock::new();
    PATHS.get_or_init(|| {
        let v = orbital_vault::open(vault_5000()).unwrap();
        v.entries()
            .iter()
            .filter(|e| {
                matches!(
                    e.kind,
                    orbital_vault::EntryKind::AtomNote | orbital_vault::EntryKind::SynthesisNote
                )
            })
            .map(|e| e.path.clone())
            .collect()
    })
}

/// Propiedades de las 5000 notas, en serie (objetivo: < 1 s).
#[divan::bench(sample_count = 5)]
fn read_props_5000(bencher: divan::Bencher) {
    let v = orbital_vault::open(vault_5000()).unwrap();
    let paths = note_paths();
    bencher.bench(|| {
        paths
            .iter()
            .filter_map(|p| v.read_props(p).ok().flatten())
            .count()
    });
}

/// Nota de ~100 KB con frontmatter, en una carpeta de disco real (no tmpfs).
fn big_note_vault() -> (PathBuf, orbital_vault::RelPath) {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("bench-nota-100k");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("wiki")).unwrap();
    std::fs::create_dir_all(dir.join("raw")).unwrap();
    let mut text = String::from(
        "---\ntype: atomo\ntitle: Grande\ntags: [a, b, c]\ndescription: Nota de 100 KB.\n---\n",
    );
    while text.len() < 100 * 1024 {
        text.push_str("Párrafo de relleno para medir la edición de propiedades en notas largas.\n");
    }
    std::fs::write(dir.join("wiki/grande.md"), text).unwrap();
    (dir, orbital_vault::RelPath::new("wiki/grande.md").unwrap())
}

fn bench_ctx() -> orbital_vault::WriteCtx {
    orbital_vault::WriteCtx {
        date: orbital_vault::Date {
            year: 2026,
            month: 11,
            day: 9,
        },
        time: orbital_vault::Time {
            hour: 12,
            minute: 0,
            second: 0,
        },
        utc_offset: orbital_vault::Offset { minutes: -180 },
        actor: orbital_vault::Actor::human("bench"),
    }
}

/// Solo el cálculo del contenido nuevo (sin disco).
#[divan::bench]
fn edit_prop_100k(bencher: divan::Bencher) {
    let (dir, path) = big_note_vault();
    let v = orbital_vault::open(&dir).unwrap();
    let note = v.read_note(&path).unwrap();
    let value = orbital_vault::props::PropValue::Text("Otro título".into());
    bencher.bench(|| note.with_prop("title", &value).unwrap().len());
}

/// `set_prop` completo: respaldo + escritura atómica con fsync (objetivo: < 5 ms).
#[divan::bench(sample_count = 30)]
fn set_prop_100k(bencher: divan::Bencher) {
    let (dir, path) = big_note_vault();
    let v = orbital_vault::open(&dir).unwrap();
    let ctx = bench_ctx();
    let mut n = 0u64;
    bencher
        .with_inputs(|| {
            n += 1;
            (
                v.read_note(&path).unwrap(),
                orbital_vault::props::PropValue::Text(format!("Título {n}")),
            )
        })
        .bench_local_values(|(note, value)| v.set_prop(&note, "title", &value, &ctx).unwrap());
}
