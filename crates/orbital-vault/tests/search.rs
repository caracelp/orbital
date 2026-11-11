mod common;

use common::TempDir;
use orbital_vault::entry::{RelPath, Zone};
use orbital_vault::query::parse_query;
use orbital_vault::search::{SearchIndex, SearchOptions};

fn create_test_vault() -> (TempDir, orbital_vault::Vault) {
    let tmp = TempDir::new("search-vault");
    let root = tmp.path();

    std::fs::create_dir_all(root.join("wiki/sub")).unwrap();
    std::fs::create_dir_all(root.join("raw")).unwrap();

    // 1. Nota md con título y etiquetas
    std::fs::write(
        root.join("wiki/entropia.md"),
        "\
---
title: Ley de la Entropía
tags: [fisica/termodinamica, ciencia]
---
# Segunda Ley

La entropía siempre aumenta en un sistema aislado.
Este es un concepto fundamental de la física.
",
    )
    .unwrap();

    // 2. Nota md con texto común
    std::fs::write(
        root.join("wiki/sub/mecanica.md"),
        "\
# Mecánica Clásica

Leyes de Newton y movimiento.
Habla de física general y energía.
",
    )
    .unwrap();

    // 3. Nota html
    std::fs::write(
        root.join("wiki/quimica.html"),
        "\
<!doctype html>
<html>
<head><title>Química Orgánica</title></head>
<body>
  <h1>Química y Reacciones</h1>
  <p>La química estudia la materia y la energía.</p>
  <p>Etiqueta: #quimica/organica</p>
</body>
</html>
",
    )
    .unwrap();

    // 4. Archivo de texto en raw/
    std::fs::write(
        root.join("raw/datos.txt"),
        "Datos de laboratorio sobre energía y física aplicada.\nSegunda línea.\n",
    )
    .unwrap();

    // 5. Archivo binario en raw/ (debe ser ignorado)
    let binary_bytes: Vec<u8> = vec![0x00, 0xFF, 0xFE, 0x00, 0x12, 0x34];
    std::fs::write(root.join("raw/imagen.png"), &binary_bytes).unwrap();

    let vault = orbital_vault::open(root).unwrap();
    (tmp, vault)
}

#[test]
fn search_index_build_and_file_types() {
    let (_tmp, vault) = create_test_vault();
    let index = SearchIndex::build(&vault);

    // Debe contener las notas wiki y el txt de raw, pero NO la imagen png
    let rel_entropia = RelPath::new("wiki/entropia.md").unwrap();
    let rel_mecanica = RelPath::new("wiki/sub/mecanica.md").unwrap();
    let rel_quimica = RelPath::new("wiki/quimica.html").unwrap();
    let rel_datos = RelPath::new("raw/datos.txt").unwrap();
    let rel_png = RelPath::new("raw/imagen.png").unwrap();

    assert!(index.contains(&rel_entropia), "Debe indexar entropia.md");
    assert!(index.contains(&rel_mecanica), "Debe indexar mecanica.md");
    assert!(index.contains(&rel_quimica), "Debe indexar quimica.html");
    assert!(index.contains(&rel_datos), "Debe indexar datos.txt en raw");
    assert!(!index.contains(&rel_png), "Debe ignorar binarios en raw");
}

#[test]
fn search_index_large_file_indexed_up_to_limit() {
    let tmp = TempDir::new("search-large");
    let root = tmp.path();
    std::fs::create_dir_all(root.join("wiki")).unwrap();

    // Archivo de más de 5 MB: 5.5 MB
    let size = 5 * 1024 * 1024 + 500 * 1024;
    let mut large_content = String::with_capacity(size);
    while large_content.len() < size {
        large_content.push_str("Línea repetida para generar contenido extenso de prueba.\n");
    }
    std::fs::write(root.join("wiki/grande.md"), &large_content).unwrap();

    let vault = orbital_vault::open(root).unwrap();
    let index = SearchIndex::build(&vault);

    let rel = RelPath::new("wiki/grande.md").unwrap();
    assert!(
        index.contains(&rel),
        "Debe indexar archivos grandes hasta el límite"
    );
}

#[test]
fn search_update_and_remove_file() {
    let (_tmp, vault) = create_test_vault();
    let mut index = SearchIndex::build(&vault);

    let rel = RelPath::new("wiki/nueva.md").unwrap();
    assert!(!index.contains(&rel));

    // update_file agrega la nota si no existía
    index.update_file(&vault, &rel, "# Nota Nueva\nContenido de prueba.\n");
    assert!(index.contains(&rel));

    let q = parse_query("nueva").unwrap();
    let res = index.search(&vault, &q, &SearchOptions::default());
    assert_eq!(res.notes.len(), 1);
    assert_eq!(res.notes[0].path, rel);

    // remove_file elimina del índice
    index.remove_file(&rel);
    assert!(!index.contains(&rel));

    let res = index.search(&vault, &q, &SearchOptions::default());
    assert_eq!(res.notes.len(), 0);
}

#[test]
fn search_words_and_accents() {
    let (_tmp, vault) = create_test_vault();
    let index = SearchIndex::build(&vault);

    // Buscar "fisica" sin tilde debe encontrar "física" y "Física"
    let q = parse_query("fisica").unwrap();
    let res = index.search(
        &vault,
        &q,
        &SearchOptions {
            include_raw: true,
            ..Default::default()
        },
    );

    // Debe coincidir en entropia.md, mecanica.md y datos.txt
    assert_eq!(res.total_notes, 3);
    let paths: Vec<&str> = res.notes.iter().map(|n| n.path.as_str()).collect();
    assert!(paths.contains(&"wiki/entropia.md"));
    assert!(paths.contains(&"wiki/sub/mecanica.md"));
    assert!(paths.contains(&"raw/datos.txt"));
}

#[test]
fn search_phrases_and_negation() {
    let (_tmp, vault) = create_test_vault();
    let index = SearchIndex::build(&vault);

    // Frase exacta
    let q = parse_query("\"concepto fundamental\"").unwrap();
    let res = index.search(&vault, &q, &SearchOptions::default());
    assert_eq!(res.notes.len(), 1);
    assert_eq!(res.notes[0].path.as_str(), "wiki/entropia.md");

    // Negación: física pero no mecánica
    let q = parse_query("fisica -mecanica").unwrap();
    let res = index.search(&vault, &q, &SearchOptions::default());
    assert_eq!(res.notes.len(), 1);
    assert_eq!(res.notes[0].path.as_str(), "wiki/entropia.md");
}

#[test]
fn search_or_alternatives() {
    let (_tmp, vault) = create_test_vault();
    let index = SearchIndex::build(&vault);

    let q = parse_query("entropia OR newton").unwrap();
    let res = index.search(&vault, &q, &SearchOptions::default());
    assert_eq!(res.total_notes, 2);
    let paths: Vec<&str> = res.notes.iter().map(|n| n.path.as_str()).collect();
    assert!(paths.contains(&"wiki/entropia.md"));
    assert!(paths.contains(&"wiki/sub/mecanica.md"));
}

#[test]
fn search_tag_filter_hierarchical() {
    let (_tmp, vault) = create_test_vault();
    let index = SearchIndex::build(&vault);

    // tag:#fisica debe incluir fisica/termodinamica (entropia.md)
    let q = parse_query("tag:#fisica").unwrap();
    let res = index.search(&vault, &q, &SearchOptions::default());
    assert_eq!(res.total_notes, 1);
    assert_eq!(res.notes[0].path.as_str(), "wiki/entropia.md");

    // tag:#quimica debe incluir quimica/organica (quimica.html)
    let q = parse_query("tag:#quimica").unwrap();
    let res = index.search(&vault, &q, &SearchOptions::default());
    assert_eq!(res.total_notes, 1);
    assert_eq!(res.notes[0].path.as_str(), "wiki/quimica.html");
}

#[test]
fn search_path_and_file_filters() {
    let (_tmp, vault) = create_test_vault();
    let index = SearchIndex::build(&vault);

    let q = parse_query("path:sub").unwrap();
    let res = index.search(&vault, &q, &SearchOptions::default());
    assert_eq!(res.total_notes, 1);
    assert_eq!(res.notes[0].path.as_str(), "wiki/sub/mecanica.md");

    let q = parse_query("file:quimica").unwrap();
    let res = index.search(&vault, &q, &SearchOptions::default());
    assert_eq!(res.total_notes, 1);
    assert_eq!(res.notes[0].path.as_str(), "wiki/quimica.html");
}

#[test]
fn search_matches_snippets_and_highlights() {
    let (_tmp, vault) = create_test_vault();
    let index = SearchIndex::build(&vault);

    let q = parse_query("fundamental").unwrap();
    let res = index.search(&vault, &q, &SearchOptions::default());

    assert_eq!(res.notes.len(), 1);
    let hit = &res.notes[0];
    assert_eq!(hit.matches.len(), 1);
    let m = &hit.matches[0];
    assert_eq!(m.line, 8); // En entropia.md, la línea con "fundamental"
    assert!(m.snippet.contains("concepto fundamental"));

    // El highlight debe marcar la palabra "fundamental" en el snippet (en caracteres)
    assert_eq!(m.highlights.len(), 1);
    let range = &m.highlights[0];
    let matched: String = m
        .snippet
        .chars()
        .skip(range.start)
        .take(range.end - range.start)
        .collect();
    assert_eq!(matched, "fundamental");
    assert_eq!(&m.snippet[range.clone()], "fundamental");
}

#[test]
fn search_ranking_and_relevance() {
    let (_tmp, vault) = create_test_vault();
    let index = SearchIndex::build(&vault);

    // "quimica":
    // quimica.html tiene "quimica" en el nombre (x10), en h1 (x3), y en el texto.
    // Debe tener un score significativamente más alto que una mención casual en texto.
    let q = parse_query("quimica").unwrap();
    let res = index.search(&vault, &q, &SearchOptions::default());
    assert_eq!(res.notes.len(), 1);
    assert_eq!(res.notes[0].path.as_str(), "wiki/quimica.html");
    assert!(res.notes[0].score >= 10);
}

#[test]
fn search_limits_and_raw_inclusion() {
    let (_tmp, vault) = create_test_vault();
    let index = SearchIndex::build(&vault);

    // Sin raw (default)
    let q = parse_query("energia").unwrap();
    let res_no_raw = index.search(
        &vault,
        &q,
        &SearchOptions {
            include_raw: false,
            ..Default::default()
        },
    );
    let paths_no_raw: Vec<&str> = res_no_raw.notes.iter().map(|n| n.path.as_str()).collect();
    assert!(!paths_no_raw.contains(&"raw/datos.txt"));

    // Con raw
    let res_raw = index.search(
        &vault,
        &q,
        &SearchOptions {
            include_raw: true,
            ..Default::default()
        },
    );
    let paths_raw: Vec<&str> = res_raw.notes.iter().map(|n| n.path.as_str()).collect();
    assert!(paths_raw.contains(&"raw/datos.txt"));
    let datos_hit = res_raw
        .notes
        .iter()
        .find(|n| n.path.as_str() == "raw/datos.txt")
        .unwrap();
    assert_eq!(datos_hit.zone, Zone::Raw);

    // Límite de notas: max_notes = 1
    let res_limit = index.search(
        &vault,
        &q,
        &SearchOptions {
            include_raw: true,
            max_notes: 1,
            ..Default::default()
        },
    );
    assert_eq!(res_limit.notes.len(), 1);
    assert!(res_limit.total_notes > 1);
}
