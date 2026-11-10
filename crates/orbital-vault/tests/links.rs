//! Tests de extracción, parsing y reescritura de enlaces (especificación 003).

use orbital_vault::links::{LinkKind, LinkTarget, extract_links, parse_link_target};

#[test]
fn parse_target_forms() {
    // Wikilink simple
    let t = parse_link_target("[[Entropía]]");
    assert_eq!(
        t,
        LinkTarget {
            path: "Entropía".into(),
            anchor: None,
            alias: None,
        }
    );

    // Wikilink con alias
    let t = parse_link_target("[[Entropía|Medida de desorden]]");
    assert_eq!(
        t,
        LinkTarget {
            path: "Entropía".into(),
            anchor: None,
            alias: Some("Medida de desorden".into()),
        }
    );

    // Wikilink con ancla de encabezado
    let t = parse_link_target("[[Entropía#Segunda ley]]");
    assert_eq!(
        t,
        LinkTarget {
            path: "Entropía".into(),
            anchor: Some("Segunda ley".into()),
            alias: None,
        }
    );

    // Wikilink con ancla y alias
    let t = parse_link_target("[[Entropía#Segunda ley|Segunda ley termodinámica]]");
    assert_eq!(
        t,
        LinkTarget {
            path: "Entropía".into(),
            anchor: Some("Segunda ley".into()),
            alias: Some("Segunda ley termodinámica".into()),
        }
    );

    // Wikilink con subcarpeta
    let t = parse_link_target("[[fisica/termodinamica#Calor]]");
    assert_eq!(
        t,
        LinkTarget {
            path: "fisica/termodinamica".into(),
            anchor: Some("Calor".into()),
            alias: None,
        }
    );

    // Markdown link
    let t = parse_link_target("/wiki/termodinamica.md#seccion");
    assert_eq!(
        t,
        LinkTarget {
            path: "/wiki/termodinamica.md".into(),
            anchor: Some("seccion".into()),
            alias: None,
        }
    );
}

#[test]
fn extract_markdown_links_basic() {
    let md = r#"---
type: atomo
title: Prueba
---
# Título

Enlace a [[Entropía]] y también a [[Física|La física]] en el párrafo.
Transclusión aquí:
![[Constante de Boltzmann]]

Y un enlace Markdown: [Sitio GNOME](https://gnome.org) o nota interna [Termo](/wiki/termo.md).
"#;

    let links = extract_links(md, false);
    assert_eq!(links.len(), 5);

    // [[Entropía]]
    assert_eq!(links[0].kind, LinkKind::Wikilink);
    assert_eq!(links[0].target.path, "Entropía");
    assert_eq!(links[0].line, 7);
    assert!(links[0].snippet.contains("[[Entropía]]"));

    // [[Física|La física]]
    assert_eq!(links[1].kind, LinkKind::Wikilink);
    assert_eq!(links[1].target.path, "Física");
    assert_eq!(links[1].target.alias.as_deref(), Some("La física"));
    assert_eq!(links[1].line, 7);

    // ![[Constante de Boltzmann]]
    assert_eq!(links[2].kind, LinkKind::Transclusion);
    assert_eq!(links[2].target.path, "Constante de Boltzmann");
    assert_eq!(links[2].line, 9);

    // [Sitio GNOME](https://gnome.org)
    assert_eq!(links[3].kind, LinkKind::Markdown);
    assert_eq!(links[3].target.path, "https://gnome.org");
    assert_eq!(links[3].target.alias.as_deref(), Some("Sitio GNOME"));
    assert_eq!(links[3].line, 11);

    // [Termo](/wiki/termo.md)
    assert_eq!(links[4].kind, LinkKind::Markdown);
    assert_eq!(links[4].target.path, "/wiki/termo.md");
    assert_eq!(links[4].target.alias.as_deref(), Some("Termo"));
    assert_eq!(links[4].line, 11);
}

#[test]
fn extract_markdown_skips_code_blocks() {
    let md = r#"# Nota con código

Texto con [[NotaReal]].

```rust
// Esto es código: [[NoEsEnlace]]
let x = "![[Tampoco]]";
```

En línea: `[[TampocoEsEnlace]]` pero acá sí [[NotaReal2]].
"#;

    let links = extract_links(md, false);
    assert_eq!(links.len(), 2);
    assert_eq!(links[0].target.path, "NotaReal");
    assert_eq!(links[1].target.path, "NotaReal2");
}

#[test]
fn extract_html_links_basic() {
    let html = r#"<!doctype html>
<html>
<head><title>Test</title></head>
<body>
  <p>Párrafo con <a href="/wiki/calor.md">Calor</a> y enlace a <a href="https://example.com">Web</a>.</p>
  <p>Texto con wikilink embebido: [[Termodinámica|Termo]] y transclusión ![[Resumen]].</p>
</body>
</html>
"#;

    let links = extract_links(html, true);
    assert_eq!(links.len(), 4);

    assert_eq!(links[0].kind, LinkKind::Markdown);
    assert_eq!(links[0].target.path, "/wiki/calor.md");
    assert_eq!(links[0].target.alias.as_deref(), Some("Calor"));

    assert_eq!(links[1].kind, LinkKind::Markdown);
    assert_eq!(links[1].target.path, "https://example.com");

    assert_eq!(links[2].kind, LinkKind::Wikilink);
    assert_eq!(links[2].target.path, "Termodinámica");
    assert_eq!(links[2].target.alias.as_deref(), Some("Termo"));

    assert_eq!(links[3].kind, LinkKind::Transclusion);
    assert_eq!(links[3].target.path, "Resumen");
}
