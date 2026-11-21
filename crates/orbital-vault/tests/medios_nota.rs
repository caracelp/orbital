//! Medios de la nota abierta (034): los embeds y los bloques de una nota resueltos contra el
//! índice de la bóveda.

mod common;

use std::fs;

use common::TempDir;
use orbital_vault::RelPath;
use orbital_vault::carrusel::Clase;
use orbital_vault::medios_nota::{Origen, extraer};

#[test]
fn resuelve_embeds_carruseles_y_galerias() {
    let t = TempDir::new("medios ñandú");
    for path in [
        "wiki/viaje/nota.md",
        "wiki/viaje/portada.png",
        "adjuntos/mapa ñ.jpg",
        "fotos/a.png",
        "fotos/b.webp",
        "fotos/clip.mp4",
        "fotos/leeme.txt",
        "carrusel/x.png",
        "carrusel/canción.mp3",
    ] {
        let p = t.path().join(path);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, b"x").unwrap();
    }
    let v = orbital_vault::open(t.path()).unwrap();
    let texto = "# Viaje\n![[portada.png]] ![](../../adjuntos/mapa%20ñ.jpg)\n![[borrada.png]]\n\n```carrusel\n![[carrusel/x.png|Leyenda]]\n![[carrusel/canción.mp3]]\n```\n\n## Fotos\n```galeria\ncarpeta: /fotos\n```\n```galeria\ncarpeta: no-existe\n```\n";
    let mut m = extraer(texto);
    v.resolver_medios_nota(&RelPath::new("wiki/viaje/nota.md").unwrap(), &mut m);
    let titulos: Vec<_> = m.secciones.iter().map(|s| s.titulo.as_str()).collect();
    assert_eq!(
        titulos,
        ["Viaje", "Carrusel", "Galería: /fotos"],
        "la galería vacía no deja grupo"
    );
    let medios: Vec<_> = m
        .medios
        .iter()
        .map(|x| (x.path.as_ref().map(|p| p.as_str()), x.seccion))
        .collect();
    assert_eq!(
        medios,
        [
            (Some("wiki/viaje/portada.png"), 0),
            (Some("adjuntos/mapa ñ.jpg"), 0),
            (None, 0),
            (Some("carrusel/x.png"), 1),
            (Some("fotos/a.png"), 2),
            (Some("fotos/b.webp"), 2),
            (Some("fotos/clip.mp4"), 2),
        ]
    );
    assert_eq!(m.medios[2].nombre(), "borrada.png");
    assert_eq!(m.medios[3].leyenda.as_deref(), Some("Leyenda"));
    assert_eq!(m.medios[6].clase, Clase::Video);
    assert_eq!(m.secciones[2].origen, Origen::Galeria);
    // «Ir al embed» de un medio de bloque lleva al bloque entero.
    assert_eq!(m.medios[4].ubicacion.linea, 10);
    assert_eq!(m.medios[4].ubicacion.lineas, 3);
    assert!(m.medios[4].abs.as_ref().unwrap().is_file());
}
