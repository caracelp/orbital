//! Índices por etiqueta (036): la rama anidada por sub-etiqueta, prioridad, títulos, carpetas
//! excluidas, meta-índice, la pista de etiquetas parecidas y que nada escribe en el disco. Bóveda
//! temporal con un nombre no ASCII. Medición (con `--ignored --nocapture`, en release): el índice
//! de una etiqueta con 2000 notas en una bóveda de 5000.

use std::fs;
use std::path::PathBuf;

use orbital_vault::indice::{IndiceDefaults, parse};
use orbital_vault::{RelPath, SearchIndex};

fn boveda(nombre: &str, notas: &[(&str, &str)]) -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "orbital-036-{nombre}-Bóveda ñandú-{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&root);
    for (ruta, texto) in notas {
        let p = root.join(ruta);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, texto).unwrap();
    }
    root
}

fn nombres(n: &orbital_vault::indice::Nodo) -> Vec<String> {
    n.notas.iter().map(|e| e.nombre.clone()).collect()
}

#[test]
fn rama_anidada_prioridad_titulos_y_exclusiones() {
    let root = boveda(
        "rama",
        &[
            ("wiki/zeta.md", "---\ntags: [proyectos]\n---\nTexto.\n"),
            ("wiki/alfa.md", "Sin frontmatter #Proyectos/Tesis\n"),
            (
                "wiki/b12.md",
                "---\ntitle: Borrador de la charla\ntags: [proyectos/papers, top]\n---\n",
            ),
            ("wiki/c.md", "#proyectos/papers y #proyectos/tesis\n"),
            ("wiki/plantillas/molde.md", "#proyectos/tesis\n"),
            ("wiki/otra.md", "#proyectosviejos\n"),
            (
                "wiki/indice.md",
                "# Índice\n\n```indice\netiqueta: proyectos\n```\n",
            ),
        ],
    );
    let antes: Vec<(PathBuf, Vec<u8>)> = walk(&root);
    let vault = orbital_vault::open(&root).unwrap();
    let idx = SearchIndex::build(&vault);
    let base = IndiceDefaults::default();
    let o = parse("etiqueta: #proyectos\nexcluir: [plantillas]", &base).unwrap();
    let i = idx.indice_etiqueta(&o);
    // Notas propias de la raíz; después las sub-etiquetas en orden, con la forma escrita.
    assert_eq!(nombres(&i.raiz), vec!["zeta"]);
    let hijas: Vec<&str> = i.raiz.hijas.iter().map(|h| h.nombre.as_str()).collect();
    assert_eq!(hijas, vec!["papers", "Tesis"]);
    // La prioritaria (`#top`) va arriba, con su título; `c` está en dos ramas.
    let papers = &i.raiz.hijas[0];
    assert_eq!(nombres(papers), vec!["b12", "c"]);
    assert!(papers.notas[0].prioritaria);
    assert_eq!(
        papers.notas[0].titulo.as_deref(),
        Some("Borrador de la charla")
    );
    assert!(!papers.notas[1].prioritaria);
    // `plantillas` excluida; `#proyectosviejos` no es de la rama.
    assert_eq!(nombres(&i.raiz.hijas[1]), vec!["alfa", "c"]);
    assert_eq!(i.raiz.total(), 5);
    // Sin títulos ni prioridad.
    let o = parse("proyectos\ntitulo: false\nprioridad: no", &base).unwrap();
    let i = idx.indice_etiqueta(&o);
    let papers = &i.raiz.hijas[0];
    assert!(
        papers
            .notas
            .iter()
            .all(|e| e.titulo.is_none() && !e.prioritaria)
    );
    assert_eq!(nombres(&i.raiz.hijas[1]), vec!["alfa", "c", "molde"]);
    // Una sub-rama sola.
    let o = parse("#proyectos/tesis", &base).unwrap();
    assert_eq!(
        nombres(&idx.indice_etiqueta(&o).raiz),
        vec!["alfa", "c", "molde"]
    );
    // Meta-índice: las notas con un bloque `indice` y su etiqueta.
    let o = parse("meta: true", &base).unwrap();
    let i = idx.indice_etiqueta(&o);
    assert_eq!(nombres(&i.raiz), vec!["indice"]);
    assert_eq!(i.raiz.notas[0].etiqueta.as_deref(), Some("proyectos"));
    // Una etiqueta mal escrita: vacío, con la pista.
    let o = parse("proyecto", &base).unwrap();
    let i = idx.indice_etiqueta(&o);
    assert_eq!(i.raiz.total(), 0);
    assert!(
        i.parecidas.contains(&"proyectos".to_owned()),
        "{:?}",
        i.parecidas
    );
    let aviso = orbital_vault::indice::vacio(&i, &o);
    assert!(aviso.contains("#proyectos"), "{aviso}");
    // HTML estático con enlaces y escapado.
    let o = parse("proyectos", &base).unwrap();
    let html = orbital_vault::indice::html(&idx.indice_etiqueta(&o), &o, &|p: &RelPath| {
        p.as_str().to_owned()
    });
    assert!(
        html.contains("<strong><a href=\"wiki/b12.md\">b12</a></strong>"),
        "{html}"
    );
    // Sin excluir `plantillas`, la forma más usada es «tesis».
    assert!(html.contains("<li>#tesis<ul>"), "{html}");
    // Nada se escribió.
    assert_eq!(walk(&root), antes);
    let _ = fs::remove_dir_all(&root);
}

/// Etiquetar una nota (actualizar el índice en memoria) la hace entrar; quitarle la etiqueta, salir.
#[test]
fn sigue_al_indice_en_memoria() {
    let root = boveda(
        "vivo",
        &[("wiki/a.md", "#curso/fisica\n"), ("wiki/b.md", "Nada.\n")],
    );
    let vault = orbital_vault::open(&root).unwrap();
    let mut idx = SearchIndex::build(&vault);
    let o = parse("curso", &IndiceDefaults::default()).unwrap();
    assert_eq!(idx.indice_etiqueta(&o).raiz.total(), 1);
    let b = RelPath::new("wiki/b.md").unwrap();
    idx.update_file(&vault, &b, "Ahora #curso/fisica.\n");
    let i = idx.indice_etiqueta(&o);
    assert_eq!(nombres(&i.raiz.hijas[0]), vec!["a", "b"]);
    idx.update_file(&vault, &b, "Sin etiqueta.\n");
    assert_eq!(idx.indice_etiqueta(&o).raiz.total(), 1);
    idx.remove_file(&RelPath::new("wiki/a.md").unwrap());
    assert_eq!(idx.indice_etiqueta(&o).raiz.total(), 0);
    let _ = fs::remove_dir_all(&root);
}

/// Cuánto cuesta armar el índice de una etiqueta con 2000 notas (en 20 sub-etiquetas) en una
/// bóveda de 5000: `cargo test --release -p orbital-vault --test indice medir -- --ignored
/// --nocapture`.
#[test]
#[ignore = "medición"]
fn medir_indice_2000() {
    let mut notas: Vec<(String, String)> = Vec::new();
    for k in 0..5000 {
        let texto = if k < 2000 {
            let top = if k % 50 == 0 { ", top" } else { "" };
            format!(
                "---\ntitle: Nota número {k}\ntags: [tesis/capitulo-{}{top}]\n---\n# N {k}\n\nTexto #otra/{}.\n",
                k % 20,
                k % 7
            )
        } else {
            format!("# N {k}\n\nTexto con #otra/{} y algo más.\n", k % 7)
        };
        notas.push((format!("wiki/c{}/nota-{k}.md", k % 10), texto));
    }
    let refs: Vec<(&str, &str)> = notas
        .iter()
        .map(|(a, b)| (a.as_str(), b.as_str()))
        .collect();
    let root = boveda("medir", &refs);
    let vault = orbital_vault::open(&root).unwrap();
    let idx = SearchIndex::build(&vault);
    let o = parse("tesis", &IndiceDefaults::default()).unwrap();
    let mut tiempos = Vec::new();
    for _ in 0..30 {
        let t = std::time::Instant::now();
        let i = idx.indice_etiqueta(&o);
        tiempos.push(t.elapsed().as_secs_f64() * 1e3);
        assert_eq!(i.raiz.total(), 2000);
    }
    tiempos.sort_by(f64::total_cmp);
    let html_t = std::time::Instant::now();
    let i = idx.indice_etiqueta(&o);
    let html = orbital_vault::indice::html(&i, &o, &|p: &RelPath| p.as_str().to_owned());
    println!(
        "índice de #tesis (2000 notas, 20 sub-etiquetas, bóveda de 5000): mediana {:.2} ms, mín {:.2} ms; con el HTML {:.2} ms ({} KiB)",
        tiempos[tiempos.len() / 2],
        tiempos[0],
        html_t.elapsed().as_secs_f64() * 1e3,
        html.len() / 1024
    );
    let _ = fs::remove_dir_all(&root);
}

fn walk(root: &std::path::Path) -> Vec<(PathBuf, Vec<u8>)> {
    let mut out = Vec::new();
    let mut pila = vec![root.to_path_buf()];
    while let Some(d) = pila.pop() {
        for e in fs::read_dir(&d).unwrap() {
            let p = e.unwrap().path();
            if p.is_dir() {
                pila.push(p);
            } else {
                out.push((p.clone(), fs::read(&p).unwrap()));
            }
        }
    }
    out.sort();
    out
}
