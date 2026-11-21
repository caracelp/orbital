//! Carrusel de medios (032): resolver los medios y las carpetas contra la bóveda.

mod common;

use std::fs;

use common::TempDir;
use orbital_vault::carrusel::{Carrusel, Clase, Estado};
use orbital_vault::render::{BlockKind, render_markdown};
use orbital_vault::transclusion::fingerprint;
use orbital_vault::{RelPath, Vault};

fn vault(files: &[&str]) -> (TempDir, Vault) {
    let t = TempDir::new("carrusel ñandú");
    for path in files {
        let p = t.path().join(path);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, b"x").unwrap();
    }
    let v = orbital_vault::open(t.path()).unwrap();
    (t, v)
}

fn carrusel(text: &str, v: &Vault, from: &str) -> (Carrusel, u64) {
    let mut doc = render_markdown(text);
    v.expand_transclusions(&RelPath::new(from).unwrap(), &mut doc);
    let b = doc
        .blocks
        .iter()
        .find(|b| matches!(b.kind, BlockKind::Carrusel(_)))
        .expect("un carrusel");
    let BlockKind::Carrusel(c) = &b.kind else {
        unreachable!()
    };
    ((**c).clone(), fingerprint(b))
}

#[test]
fn lista_y_carpeta_mezcladas_en_orden() {
    let (_t, v) = vault(&[
        "wiki/n.md",
        "wiki/fotos/b.png",
        "wiki/fotos/A.jpg",
        "wiki/fotos/c.txt",
        "wiki/fotos/sub/d.png",
        "wiki/fotos/.oculta/e.png",
        "raw/clip ñ.mp4",
        "raw/doc.pdf",
    ]);
    let nota = "```carrusel\ncarpeta: fotos\n![[clip ñ.mp4|Video]]\n![[fotos/b.png|Con leyenda]]\n![[doc.pdf#page=2]]\n![[no-está.png]]\n```\n";
    let (c, _) = carrusel(nota, &v, "wiki/n.md");
    let nombres: Vec<&str> = c.diapositivas.iter().map(|d| d.escrito.as_str()).collect();
    // Lo escrito primero; después la carpeta (sin repetir b.png, sin .txt ni subcarpetas).
    assert_eq!(
        nombres,
        [
            "clip ñ.mp4",
            "fotos/b.png",
            "doc.pdf",
            "no-está.png",
            "wiki/fotos/A.jpg"
        ]
    );
    assert!(c.diapositivas[0].abs().is_some());
    assert_eq!(c.diapositivas[2].clase, Clase::Pdf);
    assert_eq!(
        c.diapositivas[2].enlace.as_deref(),
        Some("../raw/doc.pdf#page=2")
    );
    assert_eq!(c.diapositivas[3].estado, Estado::Faltante);
    assert!(c.diapositivas[4].de_carpeta);
    assert_eq!(c.diapositivas[4].embed(), "![[wiki/fotos/A.jpg]]");
    assert!(c.avisos.is_empty(), "{:?}", c.avisos);
}

#[test]
fn carpeta_recursiva_con_tipos_y_maximo() {
    let mut files: Vec<String> = (0..12).map(|i| format!("raw/img/f{i:02}.png")).collect();
    files.push("raw/img/sub/z.webp".into());
    files.push("raw/img/sub/y.png".into());
    files.push("wiki/n.md".into());
    let refs: Vec<&str> = files.iter().map(String::as_str).collect();
    let (_t, v) = vault(&refs);
    let nota = "```media-slider\n---\nfileTypes: [png]\nrecursive: true\nmaximo: 10\n---\n[[raw/img/]]\n```\n";
    let (c, _) = carrusel(nota, &v, "wiki/n.md");
    assert_eq!(c.diapositivas.len(), 10);
    assert_eq!(c.diapositivas[0].escrito, "raw/img/f00.png");
    assert_eq!(c.avisos.len(), 1, "{:?}", c.avisos);
    let a = &c.avisos[0];
    assert!(a.contains("tiene 13 medios: entraron 10"), "{a}");
    assert!(a.contains("f10.png, f11.png, y.png"), "{a}");
    assert!(!a.contains("más"), "{a}");
}

#[test]
fn carpeta_inexistente_avisa_y_la_huella_sigue_a_la_carpeta() {
    let (_t, v) = vault(&["wiki/n.md", "wiki/g/a.png"]);
    let (c, _) = carrusel("```carrusel\ncarpeta: nada\n```\n", &v, "wiki/n.md");
    assert!(
        c.avisos
            .iter()
            .any(|a| a.contains("No existe la carpeta «nada»"))
    );
    let (_, h1) = carrusel("```carrusel\ncarpeta: g\n```\n", &v, "wiki/n.md");
    let (_t2, v2) = vault(&["wiki/n.md", "wiki/g/a.png", "wiki/g/b.png"]);
    let (c2, h2) = carrusel("```carrusel\ncarpeta: g\n```\n", &v2, "wiki/n.md");
    assert_eq!(c2.diapositivas.len(), 2);
    assert_ne!(h1, h2, "un archivo nuevo en la carpeta rehace el bloque");
}

#[test]
fn en_una_transclusion_resuelve_desde_su_nota() {
    let (_t, v) = vault(&["wiki/a/b.md", "wiki/a/foto.png", "wiki/c.md"]);
    fs::write(
        v.root().join("wiki/a/b.md"),
        "# B\n\n```carrusel\n![[foto.png]]\n```\n",
    )
    .unwrap();
    let mut doc = render_markdown("![[b]]\n");
    v.expand_transclusions(&RelPath::new("wiki/c.md").unwrap(), &mut doc);
    let BlockKind::Transclusion(t) = &doc.blocks[0].kind else {
        panic!("{:?}", doc.blocks[0].kind)
    };
    let orbital_vault::render::Embed::Note { blocks, .. } = &t.content else {
        panic!("{:?}", t.content)
    };
    let c = blocks
        .iter()
        .find_map(|b| match &b.kind {
            BlockKind::Carrusel(c) => Some(c),
            _ => None,
        })
        .unwrap();
    assert!(c.diapositivas[0].abs().is_some());
    assert_eq!(c.diapositivas[0].enlace.as_deref(), Some("foto.png"));
}
