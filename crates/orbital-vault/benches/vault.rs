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

/// Barrido de exclusiones con miles de etiquetas/enlaces/código, sin patrón cuadrático.
#[divan::bench(args = [100, 200], sample_count = 20)]
fn render_tags_kib(bencher: divan::Bencher, kib: usize) {
    let row = "#física [enlace](nota) `#ignorada` y **#ciencia/energía**.\n\n";
    let text = row.repeat((kib * 1024).div_ceil(row.len()));
    bencher.bench(|| orbital_vault::render::render_markdown(divan::black_box(&text)));
}

/// Incluye copiar 100 KiB y convertir el cursor de caracteres a bytes, como hace el editor.
#[divan::bench(sample_count = 20)]
fn tag_completion_100k(bencher: divan::Bencher) {
    let row = "Párrafo con [enlace](nota), `código` y #física.\n\n";
    let text = format!("{}Texto #fís", row.repeat((100 * 1024) / row.len()));
    let cursor_chars = text.chars().count();
    bencher.bench(|| {
        let copy = divan::black_box(&text).clone();
        let cursor = copy
            .char_indices()
            .nth(cursor_chars)
            .map_or(copy.len(), |(at, _)| at);
        assert!(orbital_vault::tags::completion_query(&copy, cursor, false).is_some());
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

/// Nota densa en marcadores para medir el análisis de la 005 (SC-1: un marcador cada ~200 bytes).
fn texto_marcadores(kib: usize) -> String {
    let row = "Relleno del párrafo para separar los marcadores en la medición del análisis de \
               notas largas y densas.\n\nEl estado de esta entrada es [%estado] y su prioridad \
               [%prioridad].\n\n";
    row.repeat((kib * 1024).div_ceil(row.len()))
}

/// Escaneo de marcadores `[%clave]` sobre nota densa (005, T007).
#[divan::bench(args = [100, 200], sample_count = 20)]
fn markers_scan_kib(bencher: divan::Bencher, kib: usize) {
    let text = texto_marcadores(kib);
    bencher.bench(|| divan::black_box(orbital_vault::find_markers(divan::black_box(&text), false)));
}

/// Escaneo más resolución por sección, como los va a encadenar el render (005, T007).
#[divan::bench(args = [100, 200], sample_count = 20)]
fn render_markers_kib(bencher: divan::Bencher, kib: usize) {
    let text = texto_marcadores(kib);
    bencher
        .bench(|| divan::black_box(orbital_vault::resolve_markers(divan::black_box(&text), &[])));
}

/// Render puro de la nota con marcadores, para comparar contra el interpolado (SC-1 de la 005).
#[divan::bench(args = [100, 200], sample_count = 20)]
fn render_base_kib(bencher: divan::Bencher, kib: usize) {
    let text = texto_marcadores(kib);
    bencher.bench(|| {
        divan::black_box(orbital_vault::render::render_markdown(divan::black_box(
            &text,
        )))
    });
}

/// Render con los marcadores resueltos e interpolados (SC-1: regresión menor a 10 %).
#[divan::bench(args = [100, 200], sample_count = 20)]
fn render_interpolado_kib(bencher: divan::Bencher, kib: usize) {
    let text = texto_marcadores(kib);
    bencher.bench(|| {
        let repl = orbital_vault::resolve_markers(divan::black_box(&text), &[]);
        divan::black_box(orbital_vault::render::render_markdown_interpolated(
            divan::black_box(&text),
            &repl,
        ))
    });
}

/// Nota realista para SC-1: un marcador cada ~1 KiB (~100 marcadores en 100 KiB).
fn texto_marcadores_esparso(kib: usize) -> String {
    let parrafo = "Relleno del párrafo para acercarse a una nota real: listas, negritas y \
                   enlaces al texto, sin marcadores dentro.\n\n";
    let marcador = "El estado acá es [%estado] y sigue la nota.\n\n";
    let mut out = String::with_capacity(kib * 1024);
    while out.len() < kib * 1024 {
        for _ in 0..6 {
            out.push_str(parrafo);
        }
        out.push_str(marcador);
    }
    out
}

/// Render puro de la nota realista (SC-1 de la 005).
#[divan::bench(args = [100], sample_count = 20)]
fn render_base_realista(bencher: divan::Bencher, kib: usize) {
    let text = texto_marcadores_esparso(kib);
    bencher.bench(|| {
        divan::black_box(orbital_vault::render::render_markdown(divan::black_box(
            &text,
        )))
    });
}

/// Render interpolado de la nota realista (SC-1: regresión menor a 10 %).
#[divan::bench(args = [100], sample_count = 20)]
fn render_interpolado_realista(bencher: divan::Bencher, kib: usize) {
    let text = texto_marcadores_esparso(kib);
    bencher.bench(|| {
        let repl = orbital_vault::resolve_markers(divan::black_box(&text), &[]);
        divan::black_box(orbital_vault::render::render_markdown_interpolated(
            divan::black_box(&text),
            &repl,
        ))
    });
}

/// 026 SC-001: ids para todos los bloques de una nota de 100 KB sin ids (< 1 ms).
#[divan::bench(sample_count = 50)]
fn asignar_ids_100k(bencher: divan::Bencher) {
    let unidad = "Un párrafo con algo de texto para medir el escáner de bloques.\n\n\
- Un ítem de lista\n  - Un subítem\n- Otro ítem\n\n> Una cita\n> de dos líneas.\n\n\
| a | b |\n|---|---|\n| 1 | 2 |\n\n```\ncódigo\n```\n\n";
    let mut text = String::from("---\ntype: atomo\n---\n# Título\n\n");
    while text.len() < 100 * 1024 {
        text.push_str(unidad);
    }
    bencher.bench(|| orbital_vault::bloques::inserciones(divan::black_box(&text), &[]).len());
}

/// 026: solo el escáner de bloques sobre la misma nota (para separar su costo del de los ids).
#[divan::bench(sample_count = 50)]
fn escanear_bloques_100k(bencher: divan::Bencher) {
    let unidad = "Un párrafo con algo de texto para medir el escáner de bloques.\n\n\
- Un ítem de lista\n  - Un subítem\n- Otro ítem\n\n> Una cita\n> de dos líneas.\n\n\
| a | b |\n|---|---|\n| 1 | 2 |\n\n```\ncódigo\n```\n\n";
    let mut text = String::from("---\ntype: atomo\n---\n# Título\n\n");
    while text.len() < 100 * 1024 {
        text.push_str(unidad);
    }
    bencher.bench(|| orbital_vault::bloques::escanear(divan::black_box(&text)).len());
}

/// 026 SC-001 con una nota realista de 100 KB: párrafos de ~300 caracteres, alguna lista y alguna
/// cita (≈ 450 bloques, contra ≈ 4900 de `asignar_ids_100k`).
#[divan::bench(sample_count = 50)]
fn asignar_ids_100k_realista(bencher: divan::Bencher) {
    let parrafo = "La entropía de un sistema aislado nunca disminuye: es la forma más conocida del \
segundo principio de la termodinámica. En notas de estudio, un párrafo así suele ocupar tres o \
cuatro líneas, con algún enlace a [[otra nota]] y alguna #etiqueta para encontrarlo después.\n\n";
    let unidad = format!("{parrafo}{parrafo}- Un ítem\n- Otro ítem\n\n{parrafo}> Una cita.\n\n");
    let mut text = String::from("---\ntype: atomo\n---\n# Título\n\n");
    while text.len() < 100 * 1024 {
        text.push_str(&unidad);
    }
    bencher.bench(|| orbital_vault::bloques::inserciones(divan::black_box(&text), &[]).len());
}

/// 027: leer y analizar `listas.yaml` con 500 entradas (50 favoritos y 10 colecciones de 45),
/// escribirlo y calcular las faltantes contra la bóveda de 5000 (objetivo: < 1 ms).
#[divan::bench(sample_count = 100)]
fn listas_500(bencher: divan::Bencher) {
    use orbital_vault::listas;
    let vault = orbital_vault::open(vault_5000()).unwrap();
    let rutas = note_paths();
    let mut l = orbital_vault::Listas::default();
    // Algunas rutas que no existen, para que `faltantes` tenga trabajo de los dos lados.
    let ruta = |n: usize| {
        if n.is_multiple_of(25) {
            orbital_vault::RelPath::new(&format!("wiki/borrada-{n}.md")).unwrap()
        } else {
            rutas[(n * 7) % rutas.len()].clone()
        }
    };
    l.favoritos = (0..50).map(ruta).collect();
    for c in 0..10 {
        l.colecciones.push(orbital_vault::Coleccion {
            nombre: format!("Colección {c}"),
            notas: (0..45).map(|n| ruta(1000 + c * 45 + n)).collect(),
        });
    }
    let texto = listas::serialize(&l);
    assert_eq!(listas::parse(&texto).unwrap(), l);
    bencher.bench(|| {
        let leidas = listas::parse(divan::black_box(&texto)).unwrap();
        let escrito = listas::serialize(&leidas);
        listas::faltantes(&leidas, vault.entries()).len() + escrito.len()
    });
}

/// Nota de ≈ 100 KB con estructura (028): 60 encabezados de tres niveles, ≈ 3000 ítems de
/// cuatro niveles, código y citas.
fn nota_plegable_100k() -> String {
    let mut t = String::from("---\ntype: nota\n---\n");
    let mut item = 0;
    for s in 0..60 {
        let nivel = 1 + s % 3;
        t.push_str(&format!(
            "{} Sección {s}\n\nUn párrafo de la sección {s}.\n\n",
            "#".repeat(nivel)
        ));
        for k in 0..50 {
            let sangria = [0, 2, 4, 6, 4, 2, 0, 2, 4, 2][k % 10];
            t.push_str(&format!(
                "{}- Ítem {item} con algo de texto\n",
                " ".repeat(sangria)
            ));
            item += 1;
        }
        if s % 4 == 0 {
            t.push_str("\n```\n# no es encabezado\n- tampoco\n```\n\n> - una cita\n>   - sub\n\n");
        } else {
            t.push('\n');
        }
    }
    t
}

/// Árbol de plegables de una nota de 100 KB (objetivo: < 1 ms), realista y caso extremo (un
/// ítem con subítem cada ≈ 20 bytes).
#[divan::bench(args = ["realista", "extremo"], sample_count = 50)]
fn plegables_100k(bencher: divan::Bencher, caso: &str) {
    let texto = match caso {
        "realista" => nota_plegable_100k(),
        _ => {
            let mut t = String::new();
            while t.len() < 100 * 1024 {
                t.push_str("- padre x\n  - hijo y\n");
            }
            t
        }
    };
    assert!(texto.len() >= 90 * 1024);
    bencher.bench(|| orbital_vault::plegado::plegables(divan::black_box(&texto)).len());
}

/// Leer, analizar y escribir el estado de plegado de 500 notas con 3 claves y podarlo contra la
/// bóveda de 5000 (objetivo: < 2 ms).
#[divan::bench(sample_count = 100)]
fn plegado_500(bencher: divan::Bencher) {
    use orbital_vault::plegado::{self, Clave, Plegado};
    let vault = orbital_vault::open(vault_5000()).unwrap();
    let rutas: Vec<_> = note_paths()
        .iter()
        .filter(|r| r.as_str().ends_with(".md"))
        .cloned()
        .collect();
    let mut p = Plegado::default();
    for n in 0..500 {
        p.poner(
            &rutas[(n * 7) % rutas.len()],
            vec![
                Clave::Encabezado {
                    texto: format!("Sección {n}"),
                    aparicion: 1,
                },
                Clave::Bloque {
                    id: format!("b{n:05}"),
                },
                Clave::Item {
                    texto: "Comprar pan".into(),
                    aparicion: 2,
                },
            ],
        );
    }
    let texto = plegado::serialize(&p);
    assert_eq!(plegado::parse(&texto).unwrap(), p);
    bencher.bench(|| {
        let mut leido = plegado::parse(divan::black_box(&texto)).unwrap();
        leido.podar(vault.entries());
        plegado::serialize(&leido).len()
    });
}
