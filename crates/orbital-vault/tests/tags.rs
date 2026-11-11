use orbital_vault::tags::{extract_tags, inline_fields};

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
