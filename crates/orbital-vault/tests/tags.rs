mod common;

use common::TempDir;
use orbital_vault::search::SearchIndex;
use orbital_vault::tags::{extract_tags, inline_fields};

#[test]
fn completion_respects_markdown_context_and_unicode_offsets() {
    use orbital_vault::tags::completion_query;
    assert_eq!(
        completion_query("#", 1, false),
        None,
        "un # al comienzo es encabezado"
    );
    assert_eq!(completion_query("Texto #123", 10, false), Some(("123", 6)));
    assert_eq!(
        completion_query("Texto #ff00ff", 13, false),
        Some(("ff00ff", 6))
    );
    let yaml = "---\ntags: [#fis]\n---\nTexto";
    assert_eq!(
        completion_query(yaml, "---\ntags: [#fis".len(), false),
        None
    );
    for prefix in ["Texto #", "Texto #Fís", "😀 #ciencia/fís", "**#fís", "#fís"] {
        let (query, start) = completion_query(prefix, prefix.len(), false).unwrap();
        assert_eq!(&prefix[start + 1..], query);
        assert_eq!(&prefix[start..start + 1], "#");
    }
    for prefix in [
        "# Título #fís",
        "`#fís",
        "`` #fís ``",
        "```rust\n#fís",
        "~~~\n#fís",
        "    #fís",
        "[[nota#fís",
        "[texto #fís",
        "[texto](url#fís",
        "https://web/#fís",
        "palabra#fís",
        "\\#fís",
    ] {
        assert_eq!(
            completion_query(prefix, prefix.len(), false),
            None,
            "{prefix}"
        );
    }
    let text = "Texto #física y más";
    assert_eq!(
        completion_query(text, "Texto #fís".len(), false),
        Some(("fís", 6))
    );
    let html = "<html><body><p>#fís";
    assert!(completion_query(html, html.len(), true).is_some());
    for html in [
        "<body><code>#fís",
        "<body><pre>#fís",
        "<body><p title=\"#fís",
        "<body><script>#fís",
    ] {
        assert_eq!(completion_query(html, html.len(), true), None, "{html}");
    }
}

#[test]
fn html_tag_offsets_survive_unicode_case_expansion() {
    for text in [
        "<html><body>İ #fi</body></html>",
        "<html><body>İ<p>#fi</p></body></html>",
        "<html><body>ẞ<code>#ignorado</code>#fi</body></html>",
        "<html><body>ẞ <p>#fi</p></body></html>",
    ] {
        let tags = extract_tags(text, true);
        assert_eq!(tags.len(), 1);
        assert_eq!(&text[tags[0].byte_range.clone()], "#fi");
    }
}

#[test]
fn markdown_code_mask_preserves_utf8_source_ranges() {
    let text = "😀 `código` y #energía";
    let tags = extract_tags(text, false);
    assert_eq!(tags.len(), 1);
    assert_eq!(tags[0].tag, "energía");
    assert_eq!(&text[tags[0].byte_range.clone()], "#energía");
}

#[test]
fn extract_tags_markdown_and_frontmatter() {
    let md = "\
---
title: Nota con etiquetas
tags:
  - fisica/mecanica
  - astronomia
---
# Introducción

Este texto habla de #fisica/termo y también de #quimica.
Un encabezado # No es etiqueta.
Un color #ff00ff o #fff no debe tomarse como etiqueta.
Un número solo #123 tampoco.
Código en línea `#no_etiqueta` y bloque:
```rust
// #tampoco_aqui
```
Enlace wikilink [[nota#seccion]] y enlace markdown [web](https://ejemplo.com#ancla).
Etiqueta con puntuación: #biologia, #geologia.
";

    let tags = extract_tags(md, false);
    let names: Vec<&str> = tags.iter().map(|t| t.tag.as_str()).collect();

    // Debe incluir las del frontmatter
    assert!(
        names.contains(&"fisica/mecanica"),
        "Falta fisica/mecanica de frontmatter"
    );
    assert!(
        names.contains(&"astronomia"),
        "Falta astronomia de frontmatter"
    );

    // Debe incluir las del texto
    assert!(
        names.contains(&"fisica/termo"),
        "Falta fisica/termo del texto"
    );
    assert!(names.contains(&"quimica"), "Falta quimica del texto");
    assert!(names.contains(&"biologia"), "Falta biologia del texto");
    assert!(names.contains(&"geologia"), "Falta geologia del texto");

    // NO debe incluir falsos positivos
    assert!(
        !names.iter().any(|t| t.starts_with("ff00ff")),
        "Tomó color hex"
    );
    assert!(
        !names.iter().any(|t| t.starts_with("fff")),
        "Tomó color hex corto"
    );
    assert!(
        !names.iter().any(|t| t.starts_with("123")),
        "Tomó número puro"
    );
    assert!(!names.contains(&"no_etiqueta"), "Tomó etiqueta en código");
    assert!(
        !names.contains(&"tampoco_aqui"),
        "Tomó etiqueta en bloque de código"
    );
    assert!(!names.contains(&"seccion"), "Tomó ancla de wikilink");
    assert!(!names.contains(&"ancla"), "Tomó ancla de URL");

    // Verificar que los rangos en bytes correspondan en el texto para las etiquetas del cuerpo
    for occ in tags.iter().filter(|t| !t.from_prop) {
        let slice = &md[occ.byte_range.clone()];
        assert!(slice.starts_with('#'), "Rango no empieza con #: {slice}");
        assert_eq!(&slice[1..], occ.tag.as_str(), "Slice no coincide con tag");
    }
}

#[test]
fn extract_tags_html() {
    let html = "\
<!doctype html>
<html>
<head>
  <script type=\"application/yaml\">
tags: [literatura, poesia/siglo-xx]
  </script>
</head>
<body>
  <h1>Título</h1>
  <p>Texto con #filosofia y <a href=\"#local\">enlace</a>.</p>
  <code>#en_codigo</code>
</body>
</html>";

    let tags = extract_tags(html, true);
    let names: Vec<&str> = tags.iter().map(|t| t.tag.as_str()).collect();

    assert!(names.contains(&"literatura"));
    assert!(names.contains(&"poesia/siglo-xx"));
    assert!(names.contains(&"filosofia"));
    assert!(!names.contains(&"local"));
    assert!(!names.contains(&"en_codigo"));
}

#[test]
fn inline_fields_extraction() {
    let md = "\
# Ficha

estado:: pendiente
prioridad:: alta
  autor:: Alan Turing
- proyecto:: Orbital
* categoría:: Software

Texto con URL https://example.com::algo no debe ser campo.
Texto normal con campo:: en el medio no es campo si no empieza línea.
`codigo:: no_campo`
```
en_bloque:: tampoco
```
";

    let fields = inline_fields(md);
    assert!(fields.contains(&("estado".to_string(), "pendiente".to_string())));
    assert!(fields.contains(&("prioridad".to_string(), "alta".to_string())));
    assert!(fields.contains(&("autor".to_string(), "Alan Turing".to_string())));
    assert!(fields.contains(&("proyecto".to_string(), "Orbital".to_string())));
    assert!(fields.contains(&("categoría".to_string(), "Software".to_string())));

    let keys: Vec<&str> = fields.iter().map(|(k, _)| k.as_str()).collect();
    assert!(!keys.contains(&"codigo"));
    assert!(!keys.contains(&"en_bloque"));
    assert!(!keys.contains(&"https"));
    assert!(!keys.contains(&"campo"));
}

#[test]
fn tag_edge_cases() {
    let md = "\
---
tags: sola/escalar
---
# Encabezado 1
## Encabezado 2
#12345 no es tag
#abcdef es color hex
#12345678 es color hex de 8
#cafe es tag (len 4 hex)
#etiqueta/con/muchos/niveles
#termina_en_barra/
";
    let tags = extract_tags(md, false);
    let names: Vec<&str> = tags.iter().map(|t| t.tag.as_str()).collect();

    assert!(names.contains(&"sola/escalar"));
    assert!(names.contains(&"cafe"));
    assert!(names.contains(&"etiqueta/con/muchos/niveles"));
    assert!(names.contains(&"termina_en_barra"));

    assert!(!names.contains(&"Encabezado"));
    assert!(!names.iter().any(|t| t.starts_with("12345")));
    assert!(!names.iter().any(|t| t.starts_with("abcdef")));
    assert!(!names.iter().any(|t| t.starts_with("12345678")));
}

#[test]
fn tag_tree_and_tag_names_most_used_form() {
    let tmp = TempDir::new("tags-tree-vault");
    let root = tmp.path();
    std::fs::create_dir_all(root.join("wiki")).unwrap();

    // Nota 1: '#Física' y '#Física/Termodinámica' (forma con mayúscula y tilde)
    std::fs::write(
        root.join("wiki/n1.md"),
        "\
---
tags: [\"Física/Termodinámica\"]
---
Texto con #Física
",
    )
    .unwrap();

    // Nota 2: '#Física' (forma con mayúscula y tilde)
    std::fs::write(
        root.join("wiki/n2.md"),
        "\
# Nota 2
Texto con #Física
",
    )
    .unwrap();

    // Nota 3: '#fisica' (forma en minúscula sin tilde)
    std::fs::write(
        root.join("wiki/n3.md"),
        "\
# Nota 3
Texto con #fisica
",
    )
    .unwrap();

    // Nota 4: '#química/orgánica'
    std::fs::write(
        root.join("wiki/n4.md"),
        "\
# Nota 4
Texto con #química/orgánica
",
    )
    .unwrap();

    let vault = orbital_vault::open(root).unwrap();
    let index = SearchIndex::build(&vault);

    let tree = index.tags();
    // 2 raíces: 'Física' (normalizada 'fisica') y 'química' (normalizada 'quimica')
    assert_eq!(tree.roots.len(), 2);

    let fisica_node = tree.roots.iter().find(|n| n.path == "fisica").unwrap();
    // La forma más usada es 'Física' (2 veces vs 1 'fisica')
    assert_eq!(fisica_node.name, "Física");
    // Cantidad total de notas contando las hijas: n1, n2, n3 = 3
    assert_eq!(fisica_node.notes, 3);
    assert_eq!(fisica_node.children.len(), 1);

    let termo_node = &fisica_node.children[0];
    assert_eq!(termo_node.path, "fisica/termodinamica");
    assert_eq!(termo_node.name, "Termodinámica");
    assert_eq!(termo_node.notes, 1);

    let quimica_node = tree.roots.iter().find(|n| n.path == "quimica").unwrap();
    assert_eq!(quimica_node.name, "química");
    assert_eq!(quimica_node.notes, 1); // cuenta a n4 que tiene la hija
    assert_eq!(quimica_node.children.len(), 1);

    // tag_names: incluye las formas más usadas
    let names = index.tag_names();
    assert!(names.contains(&"Física".to_string()));
    assert!(names.contains(&"Física/Termodinámica".to_string()));
    assert!(names.contains(&"química".to_string()));
    assert!(names.contains(&"química/orgánica".to_string()));
}

#[test]
fn tag_tree_keeps_hierarchy_with_similar_siblings_and_updates() {
    use orbital_vault::entry::RelPath;
    let tmp = TempDir::new("tags-tree-siblings");
    std::fs::create_dir_all(tmp.path().join("wiki")).unwrap();
    std::fs::write(
        tmp.path().join("wiki/nota.md"),
        "#tema #tema/hija/nieta #tema-otra #tema_otra #temas #tema/hija",
    )
    .unwrap();
    std::fs::write(
        tmp.path().join("wiki/otra.html"),
        "<html><body>#Tema/hija #tema/hija/segunda</body></html>",
    )
    .unwrap();
    let vault = orbital_vault::open(tmp.path()).unwrap();
    let mut index = SearchIndex::build(&vault);
    let tree = index.tags();
    assert_eq!(tree.roots.len(), 4);
    let parent = tree.roots.iter().find(|n| n.path == "tema").unwrap();
    assert_eq!(parent.notes, 2);
    assert_eq!(parent.children.len(), 1);
    assert_eq!(parent.children[0].notes, 2);
    assert_eq!(parent.children[0].children.len(), 2);
    assert_eq!(index.tag_names().len(), 7);

    let md = RelPath::new("wiki/nota.md").unwrap();
    let html = RelPath::new("wiki/otra.html").unwrap();
    index.update_file(&vault, &md, "#nueva");
    index.remove_file(&html);
    assert_eq!(index.tag_names(), vec!["nueva"]);
    index.remove_file(&md);
    assert!(index.tags().roots.is_empty());
    assert!(index.tag_names().is_empty());
}
