//! T011: vista formateada (render_markdown y toggle_task).

use orbital_vault::render::{
    Align, BlockKind, CalloutKind, Inline, plain_text, render_markdown, toggle_task,
};

#[test]
fn tags_in_reading_are_links_with_source_spelling_and_safe_contexts() {
    fn links(inlines: &[Inline], out: &mut Vec<String>) {
        for inline in inlines {
            match inline {
                Inline::Link { url, children } => {
                    out.push(url.clone());
                    links(children, out);
                }
                Inline::Strong(children) | Inline::Emph(children) | Inline::Strike(children) => {
                    links(children, out)
                }
                _ => {}
            }
        }
    }
    let source = "---\ntags: [propiedad]\n---\n\n😀 **#Física** y #ciencia/física, #123 #ff00ff `#código` [#enlace](destino) [[nota#sección]] &amp; #energía\n\n```\n#bloque\n```\n\n    #indentado\n\n[#referencia][ref]\n\n[ref]: destino\n";
    let document = render_markdown(source);
    let mut found = Vec::new();
    for block in &document.blocks {
        if let BlockKind::Paragraph(inlines) = &block.kind {
            links(inlines, &mut found);
        }
    }
    assert_eq!(
        found,
        [
            "orbital-tag:Física",
            "orbital-tag:ciencia/física",
            "destino",
            "[[nota#sección]]",
            "orbital-tag:energía",
            "destino"
        ]
    );
}

const NOTA: &str = "\u{feff}---
type: atomo
title: Prueba
---
# Título

Párrafo con **negrita**, *cursiva*, ~~tachado~~, `código` y [enlace](/otra.md).

- uno
- [ ] tarea pendiente
- [x] tarea hecha

1. primero
2. segundo

> Cita común.

> [!advertencia]- Cuidado con esto
> Cuerpo del callout.

> [!tip]
> Consejo sin título.

> [!raro] Algo
> x

| a | b |
|:--|--:|
| 1 | 2 |

---

![diagrama](img/d.png)

```rust
fn main() {}
```
";

fn doc() -> Vec<orbital_vault::render::Block> {
    render_markdown(NOTA).blocks
}

#[test]
fn wikilinks_in_render() {
    let md = "Texto con [[Entropía]] y con [[Física|La física]] y [[Nota#Encabezado]].";
    let blocks = render_markdown(md).blocks;
    let BlockKind::Paragraph(p) = &blocks[0].kind else {
        panic!()
    };
    assert_eq!(p.len(), 7);
    assert_eq!(p[0], Inline::Text("Texto con ".into()));
    assert_eq!(
        p[1],
        Inline::Link {
            url: "[[Entropía]]".into(),
            children: vec![Inline::Text("Entropía".into())],
        }
    );
    assert_eq!(p[2], Inline::Text(" y con ".into()));
    assert_eq!(
        p[3],
        Inline::Link {
            url: "[[Física|La física]]".into(),
            children: vec![Inline::Text("La física".into())],
        }
    );
    assert_eq!(p[4], Inline::Text(" y ".into()));
    assert_eq!(
        p[5],
        Inline::Link {
            url: "[[Nota#Encabezado]]".into(),
            children: vec![Inline::Text("Nota#Encabezado".into())],
        }
    );
    assert_eq!(p[6], Inline::Text(".".into()));
}

#[test]
fn headings_and_inlines() {
    let b = doc();
    match &b[0].kind {
        BlockKind::Heading { level: 1, inlines } => assert_eq!(plain_text(inlines), "Título"),
        other => panic!("{other:?}"),
    }
    let BlockKind::Paragraph(p) = &b[1].kind else {
        panic!()
    };
    assert!(p.iter().any(|i| matches!(i, Inline::Strong(_))));
    assert!(p.iter().any(|i| matches!(i, Inline::Emph(_))));
    assert!(p.iter().any(|i| matches!(i, Inline::Strike(_))));
    assert!(
        p.iter()
            .any(|i| matches!(i, Inline::Code(c) if c == "código"))
    );
    assert!(
        p.iter()
            .any(|i| matches!(i, Inline::Link { url, .. } if url == "/otra.md"))
    );
}

#[test]
fn ranges_and_lines_point_to_the_full_file() {
    let b = doc();
    // El frontmatter no se muestra, pero los rangos cuentan desde el inicio del archivo (con BOM).
    assert_eq!(&NOTA[b[0].source.clone()].trim_end(), &"# Título");
    assert_eq!(b[0].line, 4);
    assert!(NOTA[b[1].source.clone()].starts_with("Párrafo con"));
}

#[test]
fn lists_and_tasks() {
    let b = doc();
    let BlockKind::List { start: None, items } = &b[2].kind else {
        panic!("{:?}", b[2].kind)
    };
    assert_eq!(items.len(), 3);
    assert!(items[0].task.is_none());
    let pending = items[1].task.unwrap();
    let done = items[2].task.unwrap();
    assert!(!pending.checked && done.checked);
    assert_eq!(&NOTA[pending.offset..pending.offset + 1], " ");
    assert_eq!(&NOTA[done.offset..done.offset + 1], "x");
    assert_eq!(
        plain_text(match &items[1].blocks[0].kind {
            BlockKind::Paragraph(p) => p,
            _ => panic!(),
        }),
        "tarea pendiente"
    );
    // Marcar cambia un solo byte.
    let toggled = toggle_task(NOTA, pending.offset).unwrap();
    assert_eq!(toggled.len(), NOTA.len());
    let diff: Vec<_> = NOTA
        .bytes()
        .zip(toggled.bytes())
        .enumerate()
        .filter(|(_, (a, b))| a != b)
        .collect();
    assert_eq!(diff.len(), 1);
    assert_eq!(diff[0].0, pending.offset);
    assert_eq!(toggle_task(&toggled, pending.offset).unwrap(), NOTA);
    assert!(toggle_task(NOTA, 3).is_none(), "no es una casilla");

    let BlockKind::List {
        start: Some(1),
        items,
    } = &b[3].kind
    else {
        panic!()
    };
    assert_eq!(items.len(), 2);
}

#[test]
fn quotes_and_callouts() {
    let b = doc();
    assert!(matches!(&b[4].kind, BlockKind::Quote(_)));
    match &b[5].kind {
        BlockKind::Callout {
            kind: CalloutKind::Advertencia,
            title,
            folded: Some(true),
            body,
        } => {
            assert_eq!(title, "Cuidado con esto");
            let BlockKind::Paragraph(p) = &body[0].kind else {
                panic!()
            };
            assert_eq!(plain_text(p), "Cuerpo del callout.");
        }
        other => panic!("{other:?}"),
    }
    match &b[6].kind {
        BlockKind::Callout {
            kind: CalloutKind::Consejo,
            title,
            folded: None,
            ..
        } => assert_eq!(title, "Consejo"),
        other => panic!("{other:?}"),
    }
    match &b[7].kind {
        BlockKind::Callout {
            kind: CalloutKind::Otro(k),
            title,
            ..
        } => {
            assert_eq!(k, "raro");
            assert_eq!(title, "Algo");
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn tables_rules_images_code() {
    let b = doc();
    match &b[8].kind {
        BlockKind::Table {
            align,
            header,
            rows,
        } => {
            assert_eq!(align, &vec![Align::Left, Align::Right]);
            assert_eq!(header.len(), 2);
            assert_eq!(plain_text(&rows[0][1]), "2");
        }
        other => panic!("{other:?}"),
    }
    assert!(matches!(b[9].kind, BlockKind::Rule));
    let BlockKind::Paragraph(p) = &b[10].kind else {
        panic!()
    };
    assert!(matches!(&p[0], Inline::Image { url, alt } if url == "img/d.png" && alt == "diagrama"));
    match &b[11].kind {
        BlockKind::Code { lang, text } => {
            assert_eq!(lang, "rust");
            assert_eq!(text, "fn main() {}\n");
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn no_frontmatter_and_empty() {
    assert!(render_markdown("").blocks.is_empty());
    let d = render_markdown("Solo texto.\n");
    assert_eq!(d.blocks.len(), 1);
    assert_eq!(d.blocks[0].line, 0);
}

// --- 005 T006: codeblocks generados `indice` y `toc` ---

#[test]
fn indice_codeblock_becomes_generated_with_tag() {
    let text = "Antes.\n\n```indice\ntag: #fisica\n```\n\nDespués.\n";
    let doc = render_markdown(text);
    assert_eq!(doc.blocks.len(), 3);
    match &doc.blocks[1].kind {
        BlockKind::Generated(g) => {
            assert_eq!(g.kind, orbital_vault::render::GeneratedKind::Indice);
            let tag = g
                .params
                .iter()
                .find(|e| e.key == "tag")
                .and_then(|e| e.value.as_text());
            assert_eq!(tag, Some("#fisica"));
            assert_eq!(&text[g.source.clone()], "```indice\ntag: #fisica\n```");
        }
        other => panic!("esperaba Generated, era {other:?}"),
    }
}

#[test]
fn toc_codeblock_becomes_generated_and_accepts_params() {
    let text = "```toc\nminLevel: 2\nmaxLevel: 3\n```\n";
    let doc = render_markdown(text);
    match &doc.blocks[0].kind {
        BlockKind::Generated(g) => {
            assert_eq!(g.kind, orbital_vault::render::GeneratedKind::Toc);
            assert!(g.params.iter().any(|e| e.key == "minLevel"));
        }
        other => panic!("esperaba Generated, era {other:?}"),
    }
}

#[test]
fn indice_sin_tag_y_toc_vacio_son_comunes_o_validos() {
    // indice sin tag: codeblock común.
    let doc = render_markdown("```indice\nsin tag\n```\n");
    assert!(matches!(&doc.blocks[0].kind, BlockKind::Code { .. }));
    // indice con yaml inválido: codeblock común.
    let doc = render_markdown("```indice\ntag: [\n```\n");
    assert!(matches!(&doc.blocks[0].kind, BlockKind::Code { .. }));
    // toc sin parámetros: bloque generado válido (todo por defecto).
    let doc = render_markdown("```toc\n```\n");
    assert!(matches!(&doc.blocks[0].kind, BlockKind::Generated(_)));
    // otro lenguaje: nunca es generado.
    let doc = render_markdown("```rust\ntag: #x\n```\n");
    assert!(matches!(&doc.blocks[0].kind, BlockKind::Code { .. }));
}
