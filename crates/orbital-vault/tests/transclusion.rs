//! T025: transclusiones `![[nota]]` (US5 de la 003).

mod common;

use std::fs;

use common::TempDir;
use orbital_vault::render::{BlockKind, Embed, plain_text, render_markdown};
use orbital_vault::transclusion::fingerprint;
use orbital_vault::{RelPath, Vault};

fn rel(s: &str) -> RelPath {
    RelPath::new(s).unwrap()
}

fn vault(files: &[(&str, &str)]) -> (TempDir, Vault) {
    let t = TempDir::new("transclusion");
    for (path, text) in files {
        let p = t.path().join(path);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, text).unwrap();
    }
    let v = orbital_vault::open(t.path()).unwrap();
    (t, v)
}

fn embed(text: &str, v: &Vault, from: &str) -> (Vec<Embed>, Vec<RelPath>) {
    let mut doc = render_markdown(text);
    let deps = v.expand_transclusions(&rel(from), &mut doc);
    let embeds = doc
        .blocks
        .into_iter()
        .filter_map(|b| match b.kind {
            BlockKind::Transclusion(t) => Some(t.content),
            _ => None,
        })
        .collect();
    (embeds, deps)
}

fn texts(e: &Embed) -> Vec<String> {
    let Embed::Note { blocks, .. } = e else {
        panic!("{e:?}")
    };
    blocks
        .iter()
        .map(|b| match &b.kind {
            BlockKind::Heading { inlines, .. } | BlockKind::Paragraph(inlines) => {
                plain_text(inlines)
            }
            other => format!("{other:?}"),
        })
        .collect()
}

#[test]
fn only_a_paragraph_of_its_own_is_a_transclusion() {
    let doc =
        render_markdown("![[nota]]\n\n![[nota#Sección|alias]]\n\nTexto con ![[nota]] adentro.\n");
    let kinds: Vec<_> = doc.blocks.iter().map(|b| &b.kind).collect();
    let BlockKind::Transclusion(t) = kinds[0] else {
        panic!("{kinds:?}")
    };
    assert_eq!((t.target.as_str(), t.anchor.as_deref()), ("nota", None));
    assert_eq!(t.content, Embed::Pending);
    let BlockKind::Transclusion(t) = kinds[1] else {
        panic!()
    };
    assert_eq!(t.anchor.as_deref(), Some("Sección"));
    assert!(matches!(kinds[2], BlockKind::Paragraph(_)));
}

#[test]
fn note_and_section() {
    let atomo = "---\ntype: atomo\n---\n# Entropía\n\nIntro.\n\n## Segunda ley\n\nNunca baja.\n\n### Detalle\n\nMás.\n\n## Otra\n\nFuera.\n";
    let (_t, v) = vault(&[("wiki/fisica/entropia.md", atomo), ("wiki/a.md", "")]);
    let (e, deps) = embed(
        "![[entropia]]\n\n![[entropia#segunda  LEY]]\n",
        &v,
        "wiki/a.md",
    );
    assert_eq!(deps, vec![rel("wiki/fisica/entropia.md")]);
    // La nota entera, sin el frontmatter.
    assert_eq!(texts(&e[0])[..2], ["Entropía", "Intro."]);
    // La sección: su encabezado y lo de adentro, hasta el próximo del mismo nivel.
    assert_eq!(
        texts(&e[1]),
        ["Segunda ley", "Nunca baja.", "Detalle", "Más."]
    );
}

#[test]
fn missing_ambiguous_html_and_section_missing() {
    let (_t, v) = vault(&[
        ("wiki/a/x.md", "# X\n"),
        ("wiki/b/x.md", "# X\n"),
        ("wiki/s.html", "<p>hola</p>"),
        ("wiki/n.md", "# N\n"),
        ("wiki/c.md", ""),
    ]);
    let (e, deps) = embed(
        "![[nada]]\n\n![[x]]\n\n![[s]]\n\n![[n#no existe]]\n",
        &v,
        "wiki/c.md",
    );
    assert_eq!(e[0], Embed::Missing);
    assert!(matches!(&e[1], Embed::Ambiguous(all) if all.len() == 2));
    assert_eq!(
        e[2],
        Embed::Html {
            path: rel("wiki/s.html")
        }
    );
    assert_eq!(
        e[3],
        Embed::SectionMissing {
            path: rel("wiki/n.md")
        }
    );
    // Las notas que existen cuentan como dependencias (si cambian, se vuelve a dibujar).
    assert_eq!(deps, vec![rel("wiki/n.md"), rel("wiki/s.html")]);
}

#[test]
fn cycles_and_depth() {
    let (_t, v) = vault(&[
        ("wiki/a.md", "# A\n\n![[b]]\n"),
        ("wiki/b.md", "# B\n\n![[a]]\n"),
        ("wiki/n1.md", "# 1\n\n![[n2]]\n"),
        ("wiki/n2.md", "# 2\n\n![[n3]]\n"),
        ("wiki/n3.md", "# 3\n\n![[n4]]\n"),
        ("wiki/n4.md", "# 4\n"),
        ("wiki/c.md", ""),
    ]);
    // A incrusta a B, que incrusta a A: el segundo A es un ciclo, no un bucle infinito.
    let (e, _) = embed("# A\n\n![[b]]\n", &v, "wiki/a.md");
    let Embed::Note { blocks, .. } = &e[0] else {
        panic!("{e:?}")
    };
    let BlockKind::Transclusion(inner) = &blocks[1].kind else {
        panic!()
    };
    assert_eq!(
        inner.content,
        Embed::Cycle {
            path: rel("wiki/a.md")
        }
    );

    // n1 → n2 → n3 → n4: el cuarto nivel ya no se expande.
    let (e, deps) = embed("![[n1]]\n", &v, "wiki/c.md");
    let level = |e: &Embed| -> Embed {
        let Embed::Note { blocks, .. } = e else {
            panic!("{e:?}")
        };
        let BlockKind::Transclusion(t) = &blocks[1].kind else {
            panic!()
        };
        t.content.clone()
    };
    let l2 = level(&e[0]);
    let l3 = level(&l2);
    assert_eq!(level(&l3), Embed::TooDeep);
    assert_eq!(
        deps,
        vec![rel("wiki/n1.md"), rel("wiki/n2.md"), rel("wiki/n3.md")]
    );
}

#[test]
fn transclusions_inside_lists_and_fingerprint_changes() {
    let (t, v) = vault(&[("wiki/x.md", "Uno.\n"), ("wiki/c.md", "")]);
    let text = "- item\n\n  ![[x]]\n\nTexto.\n";
    let fp = |v: &Vault| {
        let mut doc = render_markdown(text);
        v.expand_transclusions(&rel("wiki/c.md"), &mut doc);
        doc.blocks.iter().map(fingerprint).collect::<Vec<_>>()
    };
    let before = fp(&v);
    assert_ne!(before[0], 0, "la lista incrusta una nota");
    assert_eq!(before[1], 0, "el párrafo no incrusta nada");
    fs::write(t.path().join("wiki/x.md"), "Dos.\n").unwrap();
    let after = fp(&v);
    assert_ne!(before[0], after[0]);
    assert_eq!(before[1], after[1]);
}

#[test]
fn card_html_for_html_notes() {
    let (_t, v) = vault(&[
        (
            "wiki/a.md",
            "# A <b>\n\nTexto [x](javascript:alert(1)) y [y](https://e.com).\n\n- [x] hecha\n\n![[b]]\n",
        ),
        ("wiki/b.md", "Dentro de B.\n"),
        ("wiki/s.html", ""),
    ]);
    let (embed, deps) = v.embed_one(&rel("wiki/s.html"), "a", None);
    assert_eq!(deps, vec![rel("wiki/a.md"), rel("wiki/b.md")]);
    let href = |p: &RelPath| p.as_str().strip_prefix("wiki/").unwrap().to_owned();
    let html = orbital_vault::transclusion::card_html("a", None, &embed, &href);
    assert!(html.starts_with(
        "<div class=\"orbital-embed\" data-orbital-embed=\"\" contenteditable=\"false\">"
    ));
    assert!(html.contains("<a href=\"a.md\">a</a>"));
    assert!(html.contains("<h1>A &lt;b&gt;</h1>"), "{html}");
    assert!(!html.contains("javascript:"), "{html}");
    assert!(html.contains("<a href=\"https://e.com\">y</a>"));
    assert!(html.contains("<input type=\"checkbox\" disabled checked>"));
    // La transclusión anidada es otra tarjeta adentro.
    assert_eq!(html.matches("data-orbital-embed").count(), 2);
    assert!(html.contains("<p>Dentro de B.</p>"));

    let (missing, _) = v.embed_one(&rel("wiki/s.html"), "nada", None);
    let html = orbital_vault::transclusion::card_html("nada", None, &missing, &href);
    assert!(html.contains("Nota inexistente: «nada»"));
}

/// 026 T022: `![[nota#^id]]` incrusta solo ese bloque (párrafo, ítem con subítems, tabla), avisa si
/// no existe, detecta ciclos y la huella cambia si cambia el bloque de origen.
#[test]
fn incrustar_bloque() {
    let origen = "# T\n\nOtro párrafo.\n\nEl párrafo buscado. ^par001\n\n- uno\n- dos ^ite001\n  - sub\n\n| a |\n|---|\n| 1 |\n^tab001\n\n> cita ^cit001\n";
    let (t, v) = vault(&[
        ("wiki/n.md", origen),
        ("wiki/ciclo.md", "![[ciclo#^c00001]]\n\n^c00001\n"),
    ]);
    let texto = "![[n#^par001]]\n\n![[n#^ite001]]\n\n![[n#^tab001]]\n\n![[n#^cit001]]\n\n![[n#^nohay1]]\n";
    let (e, deps) = embed(texto, &v, "wiki/c.md");
    let bloques = |x: &Embed| match x {
        Embed::Note { blocks, .. } => blocks.clone(),
        o => panic!("{o:?}"),
    };
    let p = bloques(&e[0]);
    assert_eq!(p.len(), 1);
    match &p[0].kind {
        BlockKind::Paragraph(i) => assert_eq!(plain_text(i), "El párrafo buscado."),
        k => panic!("{k:?}"),
    }
    match &bloques(&e[1])[0].kind {
        BlockKind::List { items, .. } => {
            assert_eq!(items.len(), 1, "solo el ítem buscado");
            assert_eq!(items[0].blocks.len(), 2, "con su subítem");
        }
        k => panic!("{k:?}"),
    }
    assert!(matches!(bloques(&e[2])[0].kind, BlockKind::Table { ref rows, .. } if rows.len() == 1));
    assert!(matches!(bloques(&e[3])[0].kind, BlockKind::Quote(_)));
    assert_eq!(e[4], Embed::BlockMissing { path: rel("wiki/n.md") });
    assert!(deps.contains(&rel("wiki/n.md")));

    // Ciclo: la nota se incrusta a sí misma por un bloque.
    let (e, _) = embed("![[ciclo#^c00001]]\n", &v, "wiki/c.md");
    let dentro = bloques(&e[0]);
    let hay_ciclo = matches!(
        &dentro[0].kind,
        BlockKind::Transclusion(t) if matches!(t.content, Embed::Cycle { .. })
    );
    assert!(hay_ciclo, "el bloque que se incrusta a sí mismo corta el ciclo: {dentro:?}");

    // La huella cambia si cambia el bloque de origen.
    let huella = |v: &Vault| {
        let mut doc = render_markdown("![[n#^par001]]\n");
        v.expand_transclusions(&rel("wiki/c.md"), &mut doc);
        fingerprint(&doc.blocks[0])
    };
    let antes = huella(&v);
    fs::write(t.path().join("wiki/n.md"), origen.replace("buscado.", "cambiado.")).unwrap();
    assert_ne!(antes, huella(&v));
}
