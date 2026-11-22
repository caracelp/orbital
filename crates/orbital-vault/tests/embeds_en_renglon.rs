//! Varias incrustaciones `![[…]]` en un mismo renglón o párrafo: cada una es su propia
//! transclusión, sin restos de texto («esquema-dos.png]]» o «clip.mp4]]» sueltos).
mod common;
use common::TempDir;
use orbital_vault::{
    RelPath,
    medios::Tipo,
    render::{Block, BlockKind, Embed, Inline, plain_text, render_markdown},
};
use std::fs;

/// Los bloques de un grupo, o el bloque solo.
fn partes(b: &Block) -> &[Block] {
    match &b.kind {
        BlockKind::Group(g) => g,
        _ => std::slice::from_ref(b),
    }
}

/// Lo que se vería de cada parte: `T:destino` para una transclusión, `P:texto` para un párrafo.
fn resumen(blocks: &[Block]) -> Vec<String> {
    blocks
        .iter()
        .flat_map(partes)
        .map(|b| match &b.kind {
            BlockKind::Transclusion(t) => format!("T:{}", t.target),
            BlockKind::Paragraph(i) => format!("P:{}", plain_text(i)),
            BlockKind::Heading { inlines, .. } => format!("H:{}", plain_text(inlines)),
            otro => format!("{otro:?}"),
        })
        .collect()
}

#[test]
fn dos_imagenes_en_un_renglon() {
    let doc = render_markdown("# Esquemas\n![[esquema-uno.png]] ![[esquema-dos.png]]\n");
    assert_eq!(
        resumen(&doc.blocks),
        ["H:Esquemas", "T:esquema-uno.png", "T:esquema-dos.png"]
    );
}

#[test]
fn imagen_markdown_video_y_faltante() {
    let fuente = "# Fotos\n![](fotos%20ñ/foto-0.png) ![[clip.mp4]]\n![[borrada.png]]\n";
    let doc = render_markdown(fuente);
    let r = resumen(&doc.blocks);
    assert_eq!(r[0], "H:Fotos");
    assert_eq!(r[2..], ["T:clip.mp4", "T:borrada.png"]);
    // La imagen Markdown queda como imagen, sola en su párrafo.
    let grupo = partes(&doc.blocks[1]);
    match &grupo[0].kind {
        BlockKind::Paragraph(i) => assert!(
            matches!(i.as_slice(), [Inline::Image { url, .. }] if url == "fotos%20ñ/foto-0.png"),
            "{i:?}"
        ),
        otro => panic!("{otro:?}"),
    }
    // Rangos y renglones de cada parte, sobre el fuente.
    assert_eq!(&fuente[grupo[1].source.clone()], "![[clip.mp4]]");
    assert_eq!(grupo[1].line, 1);
    assert_eq!(&fuente[grupo[2].source.clone()], "![[borrada.png]]");
    assert_eq!(grupo[2].line, 2);
}

#[test]
fn tres_imagenes_y_texto_alrededor() {
    let doc = render_markdown("Antes ![[a.png]] medio ![[b.png|200]]![[c.png]] después\n");
    assert_eq!(
        resumen(&doc.blocks),
        [
            "P:Antes",
            "T:a.png",
            "P:medio",
            "T:b.png",
            "T:c.png",
            "P:después"
        ]
    );
}

#[test]
fn incrustaciones_dentro_de_listas_citas_y_callouts() {
    let doc = render_markdown("- ![[a.png]] ![[b.png]]\n\n> ![[c.png]] ![[d.png]]\n");
    let BlockKind::List { items, .. } = &doc.blocks[0].kind else {
        panic!("{:?}", doc.blocks[0]);
    };
    assert_eq!(resumen(&items[0].blocks), ["T:a.png", "T:b.png"]);
    let BlockKind::Quote(body) = &doc.blocks[1].kind else {
        panic!("{:?}", doc.blocks[1]);
    };
    assert_eq!(resumen(body), ["T:c.png", "T:d.png"]);
}

#[test]
fn id_de_bloque_queda_en_el_grupo() {
    let doc = render_markdown("![[a.png]] ![[b.png]] ^fotos\n");
    assert_eq!(doc.blocks[0].id.as_deref(), Some("fotos"));
    assert_eq!(resumen(&doc.blocks), ["T:a.png", "T:b.png"]);
}

#[test]
fn lo_que_no_es_incrustacion_queda_igual() {
    // Escapada, en código o sin cerrar: texto, sin perder nada.
    for (fuente, visible) in [
        ("\\![[a.png]] y ![[b.png", "![[a.png]] y ![[b.png"),
        ("`![[a.png]]` y [[nota]]", "![[a.png]] y nota"),
        ("Texto ![[]] fin", "Texto ![[]] fin"),
    ] {
        let doc = render_markdown(fuente);
        assert_eq!(doc.blocks.len(), 1, "{fuente}: {:?}", doc.blocks);
        let BlockKind::Paragraph(i) = &doc.blocks[0].kind else {
            panic!("{fuente}: {:?}", doc.blocks[0]);
        };
        assert_eq!(plain_text(i), visible, "{fuente}");
    }
    // Un `[[enlace]]` después de un `![[…]]` sigue siendo enlace, y nada se pierde.
    let doc = render_markdown("Mirá ![[a.png]] y [[nota]]\n");
    assert_eq!(resumen(&doc.blocks), ["P:Mirá", "T:a.png", "P:y nota"]);
    let g = partes(&doc.blocks[0]);
    let BlockKind::Paragraph(i) = &g[2].kind else {
        panic!()
    };
    assert!(
        i.iter()
            .any(|x| matches!(x, Inline::Link { url, .. } if url == "[[nota]]"))
    );
}

#[test]
fn se_resuelven_contra_la_boveda() {
    let t = TempDir::new("embeds-renglon");
    let r = t.path();
    fs::create_dir_all(r.join("wiki/fotos ñ")).unwrap();
    fs::write(r.join("wiki/a.md"), "x").unwrap();
    fs::write(r.join("wiki/esquema-uno.png"), b"png").unwrap();
    fs::write(r.join("wiki/fotos ñ/clip.mp4"), b"mp4").unwrap();
    let v = orbital_vault::open(r).unwrap();
    let mut doc = render_markdown("![[esquema-uno.png]] ![[clip.mp4]]\n![[borrada.png]]\n");
    v.expand_transclusions(&RelPath::new("wiki/a.md").unwrap(), &mut doc);
    let g = partes(&doc.blocks[0]);
    let contenido = |n: usize| match &g[n].kind {
        BlockKind::Transclusion(t) => t.content.clone(),
        otro => panic!("{otro:?}"),
    };
    assert!(matches!(
        contenido(0),
        Embed::Archivo {
            tipo: Tipo::Imagen,
            ..
        }
    ));
    assert!(matches!(
        contenido(1),
        Embed::Archivo {
            tipo: Tipo::Video,
            ..
        }
    ));
    assert_eq!(contenido(2), Embed::Missing);
}
