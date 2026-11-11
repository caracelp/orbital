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

/// Grafo de enlaces de 5000 notas (SC-003 de la 003: < 500 ms en segundo plano).
#[divan::bench(sample_count = 5)]
fn link_graph_5000(bencher: divan::Bencher) {
    let vault = orbital_vault::open(vault_5000()).unwrap();
    bencher.bench(|| {
        orbital_vault::LinkGraph::build(divan::black_box(&vault))
            .unwrap()
            .unresolved_count()
    });
}

/// Extraer los enlaces de una nota de 100 KB (objetivo de la 003: < 2 ms).
#[divan::bench]
fn extract_links_100k(bencher: divan::Bencher) {
    let (dir, path) = big_note_vault();
    let text = std::fs::read_to_string(path.to_path(&dir)).unwrap();
    bencher.bench(|| orbital_vault::extract_links(divan::black_box(&text), false).len());
}

/// Poner al día el grafo de 5000 notas cuando se guarda una (objetivo de la 003: < 1 ms).
#[divan::bench(sample_count = 50)]
fn link_graph_update_one(bencher: divan::Bencher) {
    let vault = orbital_vault::open(vault_5000()).unwrap();
    let mut graph = orbital_vault::LinkGraph::build(&vault).unwrap();
    let path = note_paths()[2500].clone();
    let text = std::fs::read_to_string(path.to_path(vault.root())).unwrap();
    bencher.bench_local(|| graph.update_file(&vault, &path, divan::black_box(&text), false));
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

/// Actualizar una entrada de un `index.md` de 5000 entradas (objetivo: < 20 ms).
#[divan::bench]
fn index_upsert_5000(bencher: divan::Bencher) {
    let text = std::fs::read_to_string(vault_5000().join("wiki").join("index.md")).unwrap();
    let entry = orbital_vault::okf_files::IndexEntry {
        title: "Nota 2500 cambiada".into(),
        link: "/nota-02500.md".into(),
        description: "Otra descripción.".into(),
        section: "atomo".into(),
    };
    bencher.bench(|| orbital_vault::okf_files::index_upsert(divan::black_box(&text), &entry).len());
}

/// Agregar una entrada al registro.
#[divan::bench]
fn log_add(bencher: divan::Bencher) {
    let text = std::fs::read_to_string(vault_5000().join("wiki").join("log.md")).unwrap();
    bencher.bench(|| {
        orbital_vault::okf_files::log_add(
            divan::black_box(&text),
            "2026-11-10",
            orbital_vault::okf_files::Verb::Creation,
            "[X](/x.md)",
        )
        .len()
    });
}

/// Vista formateada de una nota de ~100 KB (objetivo: < 5 ms).
#[divan::bench]
fn render_100k(bencher: divan::Bencher) {
    let mut text = String::from("---\ntype: atomo\ntitle: Grande\n---\n");
    let mut i = 0;
    while text.len() < 100 * 1024 {
        text.push_str(&format!(
            "## Sección {i}\n\nPárrafo con **negrita**, *cursiva*, `código` y [enlace](/otra.md).\n\n- uno\n- [ ] tarea\n\n> [!nota] Título\n> cuerpo\n\n```rust\nfn main() {{}}\n```\n\n| a | b |\n|---|---|\n| 1 | 2 |\n\n"
        ));
        i += 1;
    }
    bencher.bench(|| {
        orbital_vault::render::render_markdown(divan::black_box(&text))
            .blocks
            .len()
    });
}

/// Solo pulldown-cmark sobre el mismo texto (referencia para render_100k).
#[divan::bench]
fn cmark_only_100k(bencher: divan::Bencher) {
    let mut text = String::new();
    let mut i = 0;
    while text.len() < 100 * 1024 {
        text.push_str(&format!(
            "## Sección {i}\n\nPárrafo con **negrita**, *cursiva*, `código` y [enlace](/otra.md).\n\n- uno\n- [ ] tarea\n\n> [!nota] Título\n> cuerpo\n\n```rust\nfn main() {{}}\n```\n\n| a | b |\n|---|---|\n| 1 | 2 |\n\n"
        ));
        i += 1;
    }
    let mut opts = pulldown_cmark::Options::empty();
    opts.insert(pulldown_cmark::Options::ENABLE_TABLES);
    opts.insert(pulldown_cmark::Options::ENABLE_STRIKETHROUGH);
    opts.insert(pulldown_cmark::Options::ENABLE_TASKLISTS);
    bencher.bench(|| {
        pulldown_cmark::Parser::new_ext(divan::black_box(&text), opts)
            .into_offset_iter()
            .count()
    });
}

/// Construcción del índice de búsqueda de 5000 notas (SC-002: < 1 s en segundo plano).
#[divan::bench(sample_count = 5)]
fn search_index_5000(bencher: divan::Bencher) {
    let vault = orbital_vault::open(vault_5000()).unwrap();
    bencher.bench(|| orbital_vault::SearchIndex::build(divan::black_box(&vault)).len());
}

/// Búsqueda de una palabra en 5000 notas (SC-001: < 100 ms).
#[divan::bench(sample_count = 20)]
fn search_word_5000(bencher: divan::Bencher) {
    let vault = orbital_vault::open(vault_5000()).unwrap();
    let index = orbital_vault::SearchIndex::build(&vault);
    let query = orbital_vault::parse_query("entropia").unwrap();
    let opts = orbital_vault::SearchOptions::default();
    bencher.bench(|| {
        index
            .search(&vault, divan::black_box(&query), divan::black_box(&opts))
            .total_notes
    });
}

/// Búsqueda con tres términos y un filtro en 5000 notas (SC-001: < 100 ms).
#[divan::bench(sample_count = 20)]
fn search_three_terms_filter_5000(bencher: divan::Bencher) {
    let vault = orbital_vault::open(vault_5000()).unwrap();
    let index = orbital_vault::SearchIndex::build(&vault);
    let query = orbital_vault::parse_query("entropia fisica sistema tag:#ciencia").unwrap();
    let opts = orbital_vault::SearchOptions::default();
    bencher.bench(|| {
        index
            .search(&vault, divan::black_box(&query), divan::black_box(&opts))
            .total_notes
    });
}

/// Actualizar el índice de 5000 notas cuando se guarda una (SC-003: < 200 ms).
#[divan::bench(sample_count = 50)]
fn search_update_one(bencher: divan::Bencher) {
    let vault = orbital_vault::open(vault_5000()).unwrap();
    let mut index = orbital_vault::SearchIndex::build(&vault);
    let path = note_paths()[2500].clone();
    let text = std::fs::read_to_string(path.to_path(vault.root())).unwrap();
    bencher.bench_local(|| {
        index.update_file(&vault, &path, divan::black_box(&text));
    });
}

/// Memoria del índice con 5000 notas (SC-006: < 100 MB).
#[divan::bench(sample_count = 1)]
fn search_memory_5000(bencher: divan::Bencher) {
    let vault = orbital_vault::open(vault_5000()).unwrap();
    let index = orbital_vault::SearchIndex::build(&vault);
    let bytes = index.memory_bytes();
    let mb = bytes as f64 / 1_048_576.0;
    eprintln!(
        "\n=== MEMORIA INDICE 5000 NOTAS: {:.2} MB ({bytes} bytes) ===\n",
        mb
    );
    bencher.bench(|| index.memory_bytes());
}
