//! Medios incrustados (057): resolver `![[archivo]]` a adjuntos de la bóveda y la tarjeta de las
//! notas `.html`.
mod common;
use common::TempDir;
use orbital_vault::{
    RelPath,
    links::Resolution,
    medios::Tipo,
    render::{BlockKind, Embed, render_markdown},
    transclusion::card_html,
};
use std::fs;

fn rel(s: &str) -> RelPath {
    RelPath::new(s).unwrap()
}

fn fixture() -> (TempDir, orbital_vault::Vault) {
    let t = TempDir::new("medios");
    let r = t.path();
    for d in ["wiki/clase", "wiki/medios", "raw", "wiki/.oculta"] {
        fs::create_dir_all(r.join(d)).unwrap();
    }
    fs::write(r.join("wiki/clase/nota.md"), "# Nota\n").unwrap();
    fs::write(r.join("wiki/clase/local.mp3"), b"ID3").unwrap();
    fs::write(r.join("wiki/medios/clip.mp4"), b"mp4").unwrap();
    fs::write(r.join("wiki/medios/Mi Canción.ogg"), b"ogg").unwrap();
    fs::write(r.join("wiki/medios/repetido.wav"), b"wav").unwrap();
    fs::write(r.join("wiki/clase/repetido.wav"), b"wav").unwrap();
    fs::write(r.join("raw/fuente.flac"), b"flac").unwrap();
    fs::write(r.join("wiki/.oculta/secreto.mp3"), b"x").unwrap();
    let vault = orbital_vault::open(r).unwrap();
    (t, vault)
}

fn found(r: Resolution) -> String {
    match r {
        Resolution::Found(p) => p.as_str().to_owned(),
        otro => panic!("{otro:?}"),
    }
}

#[test]
fn resuelve_relativo_por_raiz_y_por_nombre() {
    let (_t, v) = fixture();
    let from = rel("wiki/clase/nota.md");
    // Relativo a la carpeta de la nota.
    assert_eq!(
        found(v.resolve_attachment(&from, "local.mp3")),
        "wiki/clase/local.mp3"
    );
    assert_eq!(
        found(v.resolve_attachment(&from, "../medios/clip.mp4")),
        "wiki/medios/clip.mp4"
    );
    // Desde la raíz de la bóveda y de wiki/.
    assert_eq!(
        found(v.resolve_attachment(&from, "raw/fuente.flac")),
        "raw/fuente.flac"
    );
    assert_eq!(
        found(v.resolve_attachment(&from, "/medios/clip.mp4")),
        "wiki/medios/clip.mp4"
    );
    // Por nombre, sin distinguir mayúsculas, con espacios escritos como %20.
    assert_eq!(
        found(v.resolve_attachment(&from, "CLIP.mp4")),
        "wiki/medios/clip.mp4"
    );
    assert_eq!(
        found(v.resolve_attachment(&from, "Mi%20Canción.ogg")),
        "wiki/medios/Mi Canción.ogg"
    );
    assert_eq!(
        found(v.resolve_attachment(&from, "fuente.flac")),
        "raw/fuente.flac"
    );
    // Con la carpeta como final de la ruta.
    assert_eq!(
        found(v.resolve_attachment(&rel("raw/x.md"), "medios/repetido.wav")),
        "wiki/medios/repetido.wav"
    );
    // Con varios del mismo nombre gana el de la carpeta de la nota.
    assert_eq!(
        found(v.resolve_attachment(&rel("wiki/medios/otra.md"), "repetido.wav")),
        "wiki/medios/repetido.wav"
    );
    assert_eq!(
        found(v.resolve_attachment(&from, "repetido.wav")),
        "wiki/clase/repetido.wav"
    );
    // Con fragmento o alias.
    assert_eq!(
        found(v.resolve_attachment(&from, "clip.mp4#t=30")),
        "wiki/medios/clip.mp4"
    );
    // Inexistentes, ocultos, remotos y fuera de la bóveda.
    for malo in [
        "no.mp3",
        "secreto.mp3",
        ".oculta/secreto.mp3",
        "https://x.org/a.mp3",
        "../../../../etc/a.mp3",
    ] {
        assert_eq!(
            v.resolve_attachment(&from, malo),
            Resolution::Missing,
            "{malo}"
        );
    }
}

#[test]
fn transclusion_de_medio_es_un_archivo() {
    let (t, v) = fixture();
    let mut doc =
        render_markdown("![[clip.mp4#t=1:30]]\n\n![[local.mp3]]\n\n![[nada.mp3]]\n\n![[nota]]\n");
    v.expand_transclusions(&rel("wiki/clase/nota.md"), &mut doc);
    let embeds: Vec<&Embed> = doc
        .blocks
        .iter()
        .map(|b| match &b.kind {
            BlockKind::Transclusion(t) => &t.content,
            otro => panic!("{otro:?}"),
        })
        .collect();
    match embeds[0] {
        Embed::Archivo { path, abs, tipo } => {
            assert_eq!(path.as_str(), "wiki/medios/clip.mp4");
            assert_eq!(abs, &t.path().join("wiki/medios/clip.mp4"));
            assert_eq!(*tipo, Tipo::Video);
        }
        otro => panic!("{otro:?}"),
    }
    assert!(matches!(
        embeds[1],
        Embed::Archivo {
            tipo: Tipo::Audio,
            ..
        }
    ));
    assert_eq!(embeds[2], &Embed::Missing);
    // Una nota que se incrusta a sí misma sigue siendo un ciclo, no un medio.
    assert!(matches!(embeds[3], Embed::Cycle { .. }));
}

#[test]
fn tarjeta_html_de_medios() {
    let (_t, v) = fixture();
    let from = rel("wiki/clase/nota.md");
    let href = |p: &RelPath| format!("../{}", p.as_str().strip_prefix("wiki/").unwrap());
    let (e, _) = v.embed_one(&from, "clip.mp4", Some("t=30"));
    let html = card_html("clip.mp4", Some("t=30"), &e, &href);
    assert!(html.starts_with("<figure "), "{html}");
    assert!(html.contains("<video "), "{html}");
    assert!(html.contains("<figcaption"), "{html}");
    assert!(html.contains("controls"), "{html}");
    assert!(html.contains("preload=\"none\""), "{html}");
    assert!(html.contains("src=\"../medios/clip.mp4#t=30\""), "{html}");
    assert!(html.contains("aria-label=\"Video: clip.mp4\""), "{html}");
    assert!(html.contains("data-orbital-embed"), "{html}");
    let (e, _) = v.embed_one(&from, "local.mp3", None);
    let html = card_html("local.mp3", None, &e, &href);
    assert!(html.contains("<audio "), "{html}");
    assert!(html.contains("preload=\"none\""), "{html}");
    let (e, _) = v.embed_one(&from, "falta.mp3", None);
    let html = card_html("falta.mp3", None, &e, &href);
    assert!(html.contains("Archivo inexistente: «falta.mp3»"), "{html}");
}
