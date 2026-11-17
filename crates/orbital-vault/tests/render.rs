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
    // Las opciones se leen del cuerpo crudo (orbital_vault::toc), aunque no sea YAML válido.
    for lang in ["toc", "table-of-contents", "tabla-de-contenidos"] {
        let text = format!("```{lang}\ntitle: \nminLevel: 2 # desde\nmaxLevel: 3\n```\n");
        let doc = render_markdown(&text);
        match &doc.blocks[0].kind {
            BlockKind::Generated(g) => {
                assert_eq!(g.kind, orbital_vault::render::GeneratedKind::Toc);
                let o = orbital_vault::toc::parse_options(&g.raw).unwrap();
                assert_eq!((o.min_level, o.max_level), (2, 3));
            }
            other => panic!("esperaba Generated, era {other:?}"),
        }
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

// --- 005 T008: interpolación de marcadores en el render ---

use orbital_vault::render::render_markdown_interpolated;

#[test]
fn interpolacion_reemplaza_el_marcador_y_conserva_los_rangos() {
    let text = "estado:: activo\n\nEl estado es [%estado] y sigue la nota.\n\n# Sección\n\nOtro [%otro].\n";
    let mut repl = orbital_vault::resolve_markers(text, &[]);
    repl[1].1 = "valor yaml".to_owned();
    let doc = render_markdown_interpolated(text, &repl);
    // Los rangos de bloque siguen apuntando al fuente (con su salto de línea final).
    assert_eq!(
        &text[doc.blocks[1].source.clone()],
        "El estado es [%estado] y sigue la nota.\n"
    );
    assert_eq!(&text[doc.blocks[3].source.clone()], "Otro [%otro].\n");
    // El texto visible tiene el valor, no el marcador.
    let visible = plain_text(match &doc.blocks[1].kind {
        BlockKind::Paragraph(p) => p,
        _ => panic!(),
    });
    assert_eq!(visible, "El estado es activo y sigue la nota.");
    let visible2 = plain_text(match &doc.blocks[3].kind {
        BlockKind::Paragraph(p) => p,
        _ => panic!(),
    });
    assert_eq!(visible2, "Otro valor yaml.");
    // El render sin interpolación no cambia.
    let doc_plain = render_markdown(text);
    assert!(
        plain_text(match &doc_plain.blocks[1].kind {
            BlockKind::Paragraph(p) => p,
            _ => panic!(),
        })
        .contains("[%estado]")
    );
}

#[test]
fn interpolacion_dentro_de_negrita_y_encabezado() {
    let text =
        "# Título\nestado:: activo\n\n[%estado] en el encabezado\n\n**negrita [%estado] fina**\n";
    let repl = orbital_vault::resolve_markers(text, &[]);
    let doc = render_markdown_interpolated(text, &repl);
    let visible = plain_text(match &doc.blocks[0].kind {
        BlockKind::Heading { inlines, .. } => inlines,
        _ => panic!(),
    });
    assert_eq!(visible, "Título");
    let visible2 = plain_text(match &doc.blocks[2].kind {
        BlockKind::Paragraph(p) => p,
        _ => panic!(),
    });
    assert_eq!(visible2, "activo en el encabezado");
    let visible3 = plain_text(match &doc.blocks[3].kind {
        BlockKind::Paragraph(p) => p,
        _ => panic!(),
    });
    assert_eq!(visible3, "negrita activo fina");
}

#[test]
fn casilla_junto_a_marcador_sigue_mapeando_al_mismo_byte() {
    let text = "estado:: activo\n\n- [ ] Tarea con [%estado] dentro\n";
    let repl = orbital_vault::resolve_markers(text, &[]);
    let doc = render_markdown_interpolated(text, &repl);
    let BlockKind::List { items, .. } = &doc.blocks[1].kind else {
        panic!()
    };
    let task = items[0].task.unwrap();
    assert_eq!(&text[task.offset..task.offset + 1], " ");
    // Marcar cambia un solo byte del fuente, aunque el texto visible tenga el valor interpolado.
    let toggled = toggle_task(text, task.offset).unwrap();
    assert_eq!(toggled.len(), text.len());
    let visible = plain_text(match &items[0].blocks[0].kind {
        BlockKind::Paragraph(p) => p,
        _ => panic!(),
    });
    assert!(visible.contains("activo"));
    assert!(!visible.contains("[%estado]"));
}

/// 041: cada parte especial es un bloque `Seccion` con su contenido y el rango de la sección
/// completa (delimitadores incluidos); el Cuerpo y sus `<!-- block -->` quedan como estaban.
#[test]
fn partes_especiales_se_agrupan_en_secciones() {
    use orbital_vault::render::SeccionKind;
    let text = "---\nid: AT-001\ntype: atomo\n---\n\n<!-- summary AT-001 -->\n## Síntesis\n\nIdea central.\n<!---->\n\n<!-- block 1 AT-001 -->\n# Cuerpo\n<!---->\n\n<!-- sources AT-001 -->\n- Libro\n<!---->\n\n<!-- connections AT-001 -->\n- [[Otra]]\n- Prerrequisito: [[Base]]\n<!---->\n";
    let doc = render_markdown(text);
    let secciones: Vec<(SeccionKind, usize, &str)> = doc
        .blocks
        .iter()
        .filter_map(|b| match &b.kind {
            BlockKind::Seccion { kind, body } => Some((*kind, body.len(), &text[b.source.clone()])),
            _ => None,
        })
        .collect();
    assert_eq!(secciones.len(), 3, "{:#?}", doc.blocks);
    assert_eq!(secciones[0].0, SeccionKind::Sintesis);
    assert_eq!(secciones[0].1, 2, "encabezado y párrafo");
    assert!(secciones[0].2.starts_with("<!-- summary AT-001 -->"));
    assert!(secciones[0].2.trim_end().ends_with("<!---->"));
    assert_eq!(secciones[1].0, SeccionKind::Fuentes);
    assert_eq!(secciones[2].0, SeccionKind::Conexiones);
    assert!(secciones[2].2.contains("[[Base]]"));
    // El cuerpo sigue igual: su encabezado es de primer nivel.
    assert!(
        doc.blocks
            .iter()
            .any(|b| matches!(&b.kind, BlockKind::Heading { level: 1, .. }))
    );
    // Los wikilinks de los ítems compactos son enlaces.
    let conexiones = doc.blocks.iter().find_map(|b| match &b.kind {
        BlockKind::Seccion {
            kind: SeccionKind::Conexiones,
            body,
        } => Some(body),
        _ => None,
    });
    let Some(BlockKind::List { items, .. }) = conexiones.and_then(|b| b.first()).map(|b| &b.kind)
    else {
        panic!("la lista de conexiones");
    };
    let BlockKind::Paragraph(p) = &items[1].blocks[0].kind else {
        panic!()
    };
    assert!(
        p.iter()
            .any(|i| matches!(i, Inline::Link { url, .. } if url == "[[Base]]")),
        "{p:?}"
    );
    // Sin cierre: llega hasta el final.
    let doc = render_markdown("<!-- connections X -->\n- [[A]]\n");
    assert!(matches!(&doc.blocks[0].kind, BlockKind::Seccion { body, .. } if body.len() == 1));
}

/// 024: las tarjetas `:::flashcard` (caso real): un bloque por tarjeta con el rango completo, el
/// `id=` (también con espacios de más), la pregunta y la respuesta como Markdown; el `---` interno
/// no es una regla ni convierte la pregunta en encabezado.
#[test]
fn tarjetas_de_estudio() {
    use orbital_vault::render::Flashcard;
    let text = "---\ntags: [pbo]\n---\n## **5. Flashcards**\n\n:::flashcard id=fc-001\n¿Cuál es el enunciado formal del Principio del Buen Orden (PBO)?\n---\nTodo subconjunto no vacío de enteros no negativos tiene un elemento mínimo: $$\\forall C \\subseteq \\mathbb{N}, \\; (C \\neq \\emptyset) \\implies (\\exists c_0 \\in C : \\forall c \\in C, \\; c_0 \\le c)$$.\n:::\n\n:::flashcard id=fc-002 \n¿Por qué el PBO falla en el conjunto vacío ($\\emptyset$)?\n---\nPorque el conjunto vacío carece de elementos.\n\nSegundo párrafo con **negrita**.\n:::\n\nDespués.\n";
    let doc = render_markdown(text);
    let tarjetas: Vec<(&Flashcard, &str)> = doc
        .blocks
        .iter()
        .filter_map(|b| match &b.kind {
            BlockKind::Flashcard(f) => Some((f, &text[b.source.clone()])),
            _ => None,
        })
        .collect();
    assert_eq!(tarjetas.len(), 2, "{:#?}", doc.blocks);
    // Nada se convirtió en encabezado (salvo el de la nota) ni en regla.
    let encabezados = doc
        .blocks
        .iter()
        .filter(|b| matches!(b.kind, BlockKind::Heading { .. }))
        .count();
    assert_eq!(encabezados, 1);
    assert!(!doc.blocks.iter().any(|b| matches!(b.kind, BlockKind::Rule)));
    let (f1, src1) = tarjetas[0];
    assert!(src1.starts_with(":::flashcard id=fc-001") && src1.ends_with(":::"));
    assert_eq!(f1.id.as_deref(), Some("fc-001"));
    assert_eq!((f1.numero, f1.total), (1, 2));
    assert!(
        matches!(&f1.question[..], [q] if matches!(&q.kind, BlockKind::Paragraph(p) if plain_text(p).starts_with("¿Cuál es")))
    );
    assert!(
        matches!(&f1.answer[..], [a] if matches!(&a.kind, BlockKind::Paragraph(p) if plain_text(p).contains("$$\\forall C")))
    );
    let (f2, _) = tarjetas[1];
    assert_eq!(f2.id.as_deref(), Some("fc-002"));
    assert_eq!(f2.answer.len(), 2);
    // Los rangos y líneas de lo de adentro apuntan al texto completo.
    let q = &f2.question[0];
    assert!(text[q.source.clone()].starts_with("¿Por qué"));
    assert!(text.lines().nth(q.line).unwrap().starts_with("¿Por qué"));
    // El orden de los bloques sigue al del texto.
    let ultimo = doc.blocks.last().unwrap();
    assert!(matches!(&ultimo.kind, BlockKind::Paragraph(p) if plain_text(p) == "Después."));
    let starts: Vec<usize> = doc.blocks.iter().map(|b| b.source.start).collect();
    assert!(starts.windows(2).all(|w| w[0] < w[1]));
}
