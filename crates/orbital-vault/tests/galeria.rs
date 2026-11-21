//! Galería en la nota (033): la carpeta de la galería con el índice de la bóveda.

mod common;

use std::fs;

use common::TempDir;
use orbital_vault::carrusel::Clase;
use orbital_vault::galeria::Galeria;
use orbital_vault::render::{BlockKind, render_markdown};
use orbital_vault::transclusion::fingerprint;
use orbital_vault::{RelPath, Vault};

fn vault(files: &[&str]) -> (TempDir, Vault) {
    let t = TempDir::new("galería ñandú");
    for path in files {
        let p = t.path().join(path);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, b"x").unwrap();
    }
    let v = orbital_vault::open(t.path()).unwrap();
    (t, v)
}

fn galeria(text: &str, v: &Vault, from: &str) -> (Galeria, u64) {
    let mut doc = render_markdown(text);
    v.expand_transclusions(&RelPath::new(from).unwrap(), &mut doc);
    let b = doc
        .blocks
        .iter()
        .find(|b| matches!(b.kind, BlockKind::Galeria(_)))
        .expect("una galería");
    let BlockKind::Galeria(g) = &b.kind else {
        unreachable!()
    };
    ((**g).clone(), fingerprint(b))
}

fn nombres(g: &Galeria) -> Vec<&str> {
    g.elementos.iter().map(|e| e.path.as_str()).collect()
}

#[test]
fn imagenes_y_videos_en_orden_alfabetico_estable() {
    let (_t, v) = vault(&[
        "wiki/n.md",
        "imagenes/tesis/b.png",
        "imagenes/tesis/A ñ.JPG",
        "imagenes/tesis/clip.mp4",
        "imagenes/tesis/notas.txt",
        "imagenes/tesis/informe.pdf",
        "imagenes/tesis/sub/c.png",
        "imagenes/tesis/.oculta/d.png",
    ]);
    let nota = "```galeria\ncarpeta: imagenes/tesis\n```\n";
    let (g, h1) = galeria(nota, &v, "wiki/n.md");
    assert_eq!(
        nombres(&g),
        [
            "imagenes/tesis/A ñ.JPG",
            "imagenes/tesis/b.png",
            "imagenes/tesis/clip.mp4"
        ]
    );
    assert_eq!(g.elementos[2].clase, Clase::Video);
    assert_eq!(g.elementos[0].size, 1);
    assert!(g.elementos[0].mtime.is_some());
    assert!(g.vacia.is_none() && g.avisos.is_empty(), "{:?}", g.avisos);
    assert_eq!(
        g.descripcion(),
        "Galería: imagenes/tesis, 2 imágenes y 1 video"
    );
    // Determinista: la misma carpeta, el mismo orden y la misma huella.
    let (g2, h2) = galeria(nota, &v, "wiki/n.md");
    assert_eq!(nombres(&g2), nombres(&g));
    assert_eq!(h1, h2);
    // Un archivo nuevo cambia la huella (rehace solo este bloque).
    let (_t3, v3) = vault(&[
        "wiki/n.md",
        "imagenes/tesis/b.png",
        "imagenes/tesis/A ñ.JPG",
        "imagenes/tesis/clip.mp4",
        "imagenes/tesis/z.webp",
    ]);
    let (g3, h3) = galeria(nota, &v3, "wiki/n.md");
    assert_eq!(g3.elementos.len(), 4);
    assert_ne!(h1, h3);
}

#[test]
fn recursiva_agrupada_o_continua_y_tipos() {
    let (_t, v) = vault(&[
        "wiki/n.md",
        "fotos/z.png",
        "fotos/viaje/b.png",
        "fotos/Alpes/a.png",
        "fotos/Alpes/v.webm",
        "fotos/a.gif",
    ]);
    let (g, _) = galeria(
        "```galeria\ncarpeta: fotos\nrecursivo: true\nagrupar: true\n```\n",
        &v,
        "wiki/n.md",
    );
    assert_eq!(
        nombres(&g),
        [
            "fotos/a.gif",
            "fotos/z.png",
            "fotos/Alpes/a.png",
            "fotos/Alpes/v.webm",
            "fotos/viaje/b.png"
        ]
    );
    let grupos: Vec<(&str, usize)> = g
        .grupos
        .iter()
        .map(|x| (x.nombre.as_str(), x.desde))
        .collect();
    assert_eq!(grupos, [("", 0), ("Alpes", 2), ("viaje", 4)]);
    assert!(g.con_titulos());
    assert_eq!(g.elementos[3].grupo, 1);
    // Continua: el orden de las rutas, sin títulos.
    let (c, _) = galeria(
        "```galeria\ncarpeta: fotos\nrecursive: true\nfileTypes: [png]\n```\n",
        &v,
        "wiki/n.md",
    );
    assert_eq!(
        nombres(&c),
        ["fotos/Alpes/a.png", "fotos/viaje/b.png", "fotos/z.png"]
    );
    assert!(!c.con_titulos());
    // Sin recursivo: solo la carpeta exacta.
    let (s, _) = galeria(
        "```galeria\ncarpeta: fotos\nagrupar: true\n```\n",
        &v,
        "wiki/n.md",
    );
    assert_eq!(nombres(&s), ["fotos/a.gif", "fotos/z.png"]);
}

#[test]
fn relativa_a_la_nota_inexistente_y_vacia() {
    let (_t, v) = vault(&[
        "wiki/proyecto/n.md",
        "wiki/proyecto/fotos/a.png",
        "fotos/raiz.png",
        "vacia/texto.md",
    ]);
    // `./fotos` es la de la nota, aunque exista `fotos` en la raíz.
    let (g, _) = galeria(
        "```galeria\ncarpeta: ./fotos\n```\n",
        &v,
        "wiki/proyecto/n.md",
    );
    assert_eq!(nombres(&g), ["wiki/proyecto/fotos/a.png"]);
    let (r, _) = galeria(
        "```galeria\ncarpeta: fotos\n```\n",
        &v,
        "wiki/proyecto/n.md",
    );
    assert_eq!(nombres(&r), ["fotos/raiz.png"], "sin ./: primero la raíz");
    let (u, _) = galeria(
        "```galeria\ncarpeta: ../../fotos\n```\n",
        &v,
        "wiki/proyecto/n.md",
    );
    assert_eq!(nombres(&u), ["fotos/raiz.png"]);
    let (afuera, _) = galeria(
        "```galeria\ncarpeta: ../../../x\n```\n",
        &v,
        "wiki/proyecto/n.md",
    );
    assert!(afuera.vacia.unwrap().contains("no existe"));
    let (no, _) = galeria("```galeria\ncarpeta: nada\n```\n", &v, "wiki/proyecto/n.md");
    assert_eq!(no.vacia.as_deref(), Some("La carpeta «nada» no existe"));
    let (sin, _) = galeria(
        "```galeria\ncarpeta: vacia\n```\n",
        &v,
        "wiki/proyecto/n.md",
    );
    assert_eq!(
        sin.vacia.as_deref(),
        Some("La carpeta «vacia» no tiene imágenes ni videos")
    );
}

#[cfg(unix)]
#[test]
fn los_enlaces_simbolicos_quedan_afuera() {
    let afuera = TempDir::new("afuera");
    fs::write(afuera.path().join("secreta.png"), b"x").unwrap();
    let (t, _) = vault(&["wiki/n.md", "fotos/a.png"]);
    std::os::unix::fs::symlink(
        afuera.path().join("secreta.png"),
        t.path().join("fotos/b.png"),
    )
    .unwrap();
    let v = orbital_vault::open(t.path()).unwrap();
    let (g, _) = galeria("```galeria\ncarpeta: fotos\n```\n", &v, "wiki/n.md");
    assert_eq!(nombres(&g), ["fotos/a.png"]);
}

#[test]
fn en_una_transclusion_resuelve_desde_su_nota() {
    let (_t, v) = vault(&["wiki/a/b.md", "wiki/a/fotos/x.png", "wiki/c.md"]);
    fs::write(
        v.root().join("wiki/a/b.md"),
        "# B\n\n```galeria\ncarpeta: ./fotos\n```\n",
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
    let g = blocks
        .iter()
        .find_map(|b| match &b.kind {
            BlockKind::Galeria(g) => Some(g),
            _ => None,
        })
        .unwrap();
    assert_eq!(nombres(g), ["wiki/a/fotos/x.png"]);
}
