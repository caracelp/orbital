//! Fórmulas LaTeX en el análisis de la Lectura (050).

use orbital_vault::render::{BlockKind, Inline, render_markdown};

fn bloques(texto: &str) -> Vec<BlockKind> {
    render_markdown(texto)
        .blocks
        .into_iter()
        .map(|b| b.kind)
        .collect()
}

fn math(tex: &str, display: bool) -> Inline {
    Inline::Math {
        tex: tex.into(),
        display,
    }
}

#[test]
fn en_linea_y_dinero() {
    let b = bloques("Sea $x^2$ y cuesta $5 y $10, o $5,$10.\n");
    let BlockKind::Paragraph(p) = &b[0] else {
        panic!("{b:?}")
    };
    assert_eq!(p[0], Inline::Text("Sea ".into()));
    assert_eq!(p[1], math("x^2", false));
    let resto: String = p[2..]
        .iter()
        .map(|i| match i {
            Inline::Text(t) => t.clone(),
            otro => panic!("{otro:?}"),
        })
        .collect();
    assert_eq!(resto, " y cuesta $5 y $10, o $5,$10.");
}

#[test]
fn bloque_dollar_y_cerca_math_con_id() {
    let b = render_markdown(
        "$$\n\\begin{aligned} a &= b \\\\ c &= d \\end{aligned}\n$$\n^ecu-1\n\n```math\n\\sqrt{2}\n```\n",
    )
    .blocks;
    assert_eq!(
        b[0].kind,
        BlockKind::Math {
            tex: "\n\\begin{aligned} a &= b \\\\ c &= d \\end{aligned}\n".into()
        }
    );
    assert_eq!(b[0].id.as_deref(), Some("ecu-1"));
    assert_eq!(
        b[1].kind,
        BlockKind::Math {
            tex: "\\sqrt{2}\n".into()
        }
    );
}

#[test]
fn nunca_en_codigo_ni_escapado() {
    let b = bloques("Usá `$x$` y \\$5.\n\n```\n$y$\n```\n");
    let BlockKind::Paragraph(p) = &b[0] else {
        panic!()
    };
    assert!(!p.iter().any(|i| matches!(i, Inline::Math { .. })), "{p:?}");
    assert!(matches!(&b[1], BlockKind::Code { .. }));
}

#[test]
fn display_dentro_de_un_parrafo_y_en_listas() {
    let b = bloques("Antes $$a+b$$ después.\n\n- ítem\n\n  $$c$$\n\n> $$d$$\n");
    let BlockKind::Paragraph(p) = &b[0] else {
        panic!()
    };
    assert!(p.contains(&math("a+b", true)));
    let BlockKind::List { items, .. } = &b[1] else {
        panic!()
    };
    assert_eq!(items[0].blocks[1].kind, BlockKind::Math { tex: "c".into() });
    let BlockKind::Quote(q) = &b[2] else { panic!() };
    assert_eq!(q[0].kind, BlockKind::Math { tex: "d".into() });
}

#[test]
fn una_etiqueta_dentro_de_una_formula_no_es_etiqueta() {
    let b = bloques("La $a \\#b$ y #real\n");
    let BlockKind::Paragraph(p) = &b[0] else {
        panic!()
    };
    let enlaces: Vec<_> = p
        .iter()
        .filter_map(|i| match i {
            Inline::Link { url, .. } => Some(url.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(enlaces, vec!["orbital-tag:real"]);
}
