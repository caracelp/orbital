//! Bloques enlazables e identificadores (026).

use orbital_vault::bloques::{self, ClaseBloque};

const NOTA: &str = "---
type: atomo
tags: [fisica]
---
# Entropía

La entropía mide el desorden
de un sistema aislado.

- Primer principio
  - Un subítem
- Segundo principio

> Una cita de varias líneas
> que termina acá.

> [!nota] Un callout
> con contenido.

| a | b |
|---|---|
| 1 | 2 |

```rust
fn main() {}
```

$$
E = mc^2
$$

---

[^1]: Una nota al pie.
";

fn clases(texto: &str) -> Vec<ClaseBloque> {
    bloques::escanear(texto).iter().map(|b| b.clase).collect()
}

#[test]
fn escanea_los_siete_tipos_y_nada_mas() {
    use ClaseBloque::*;
    assert_eq!(
        clases(NOTA),
        [
            Parrafo, Item, Item, Item, Cita, Callout, Tabla, Codigo, Matematica
        ]
    );
}

/// SC-003: el texto fuera de los ids queda idéntico byte a byte; todos distintos.
#[test]
fn inserciones_solo_agregan_ids() {
    let ed = bloques::inserciones(NOTA, &[]);
    assert_eq!(ed.len(), 9, "un id por bloque");
    let nuevo = bloques::aplicar(NOTA, &ed);
    // Sacando los ids se vuelve al original.
    let mut sin_ids = String::new();
    for linea in nuevo.split_inclusive('\n') {
        let t = linea.trim_end_matches('\n');
        if let Some(i) = t.rfind(" ^") {
            sin_ids.push_str(&t[..i]);
            sin_ids.push_str(&linea[t.len()..]);
        } else if t.starts_with('^') && t.len() == 7 {
            continue;
        } else {
            sin_ids.push_str(linea);
        }
    }
    assert_eq!(sin_ids, NOTA);
    // Todos tienen id y son distintos.
    let despues = bloques::escanear(&nuevo);
    let ids: Vec<&str> = despues
        .iter()
        .map(|b| &nuevo[b.id.clone().expect("con id")])
        .collect();
    let mut unicos = ids.clone();
    unicos.sort();
    unicos.dedup();
    assert_eq!(unicos.len(), 9);
    for id in ids {
        assert!(
            id.len() == 6
                && id
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
        );
    }
    // Volver a asignar no cambia nada.
    assert!(bloques::inserciones(&nuevo, &[]).is_empty());
}

#[test]
fn lugares_del_id() {
    let nuevo = bloques::aplicar(NOTA, &bloques::inserciones(NOTA, &[]));
    assert!(nuevo.contains("de un sistema aislado. ^"), "{nuevo}");
    assert!(nuevo.contains("- Primer principio ^"));
    assert!(nuevo.contains("  - Un subítem ^"));
    assert!(nuevo.contains("> que termina acá. ^"));
    assert!(nuevo.contains("> con contenido. ^"));
    assert!(nuevo.contains("| 1 | 2 |\n^"));
    assert!(nuevo.contains("```\n^"));
    assert!(nuevo.contains("$$\n^"));
    assert!(!nuevo.contains("# Entropía ^"));
    assert!(!nuevo.contains("tags: [fisica] ^"));
    assert!(!nuevo.contains("Una nota al pie. ^"));
}

#[test]
fn ids_existentes_y_falsos() {
    let texto = "Con id a mano ^mi-id\n\nLa fórmula x^n y el cuadrado ^2no\n\nE = x^2\n";
    let bs = bloques::escanear(texto);
    assert_eq!(&texto[bs[0].id.clone().unwrap()], "mi-id");
    // `^2no` está separado por un espacio: cuenta como id; `x^2` no.
    assert_eq!(&texto[bs[1].id.clone().unwrap()], "2no");
    assert!(bs[2].id.is_none());
    let ed = bloques::inserciones(texto, &[]);
    assert_eq!(ed.len(), 1, "solo el último bloque recibe uno");
}

#[test]
fn repetidos_se_reemplazan_desde_el_segundo() {
    let texto = "Uno ^abc123\n\nDos ^abc123\n";
    let ed = bloques::inserciones(texto, &[]);
    assert_eq!(ed.len(), 1);
    let nuevo = bloques::aplicar(texto, &ed);
    assert!(nuevo.starts_with("Uno ^abc123\n"));
    assert!(!nuevo.ends_with("Dos ^abc123\n"));
}

/// FR-004: el bloque con un cursor no recibe id.
#[test]
fn excluir_el_bloque_del_cursor() {
    let texto = "Primero\n\nSegundo en curso\n\nTercero\n";
    let cursor = texto.find("en curso").unwrap();
    let ed = bloques::inserciones(texto, std::slice::from_ref(&(cursor..cursor)));
    let nuevo = bloques::aplicar(texto, &ed);
    assert!(nuevo.contains("Segundo en curso\n"));
    assert_eq!(ed.len(), 2);
    // Cursor al final del bloque (justo donde iría el id): tampoco.
    let fin = texto.find("Segundo en curso").unwrap() + "Segundo en curso".len();
    assert_eq!(
        bloques::inserciones(texto, std::slice::from_ref(&(fin..fin))).len(),
        2
    );
}

#[test]
fn ultima_linea_sin_salto() {
    let texto = "Párrafo final";
    assert_eq!(
        bloques::aplicar(texto, &bloques::inserciones(texto, &[])).len(),
        texto.len() + 8
    );
    let codigo = "```\nx\n```";
    let nuevo = bloques::aplicar(codigo, &bloques::inserciones(codigo, &[]));
    assert!(nuevo.starts_with("```\nx\n```\n^"), "{nuevo}");
}

#[test]
fn html_atributo_id() {
    let html = "<!doctype html><html><head><style>p{}</style></head><body>\
<p class=\"a\">Uno</p><p id=\"ya\">Dos</p><ul><li>Ítem</li></ul>\
<!-- <p>comentario</p> --><script>let p = '<p>';</script>\
<blockquote>Cita</blockquote><table><tr><td>1</td></tr></table><pre>x</pre>\
<figure><img src=\"a.png\"></figure><div>No</div></body></html>";
    let ed = bloques::inserciones_html(html, &[]);
    assert_eq!(ed.len(), 6, "p, li, blockquote, table, pre y figure sin id");
    let nuevo = bloques::aplicar(html, &ed);
    assert!(nuevo.contains("<p id=\"ya\">Dos</p>"));
    assert!(nuevo.contains("<p class=\"a\" id=\""));
    assert!(nuevo.contains("<!-- <p>comentario</p> -->"));
    assert!(nuevo.contains("let p = '<p>';"));
    assert!(nuevo.contains("<div>No</div>"));
    assert!(bloques::inserciones_html(&nuevo, &[]).is_empty());
}

/// FR-014: el texto para la búsqueda no tiene ids y conserva las líneas.
#[test]
fn sin_ids_conserva_lineas() {
    let con = bloques::aplicar(NOTA, &bloques::inserciones(NOTA, &[]));
    let sin = bloques::sin_ids(&con);
    assert_eq!(sin.lines().count(), con.lines().count());
    assert!(!sin.contains('^') || !sin.lines().any(|l| l.trim_start().starts_with('^')));
    assert!(sin.contains("de un sistema aislado.\n"));
    assert!(sin.contains("- Primer principio\n"));
}

/// El render guarda el id en el bloque y no lo muestra.
#[test]
fn render_sin_ids() {
    use orbital_vault::render::{BlockKind, Inline, render_markdown};
    let texto = "Un párrafo ^abc123\n\n- Ítem ^def456\n\n| a |\n|---|\n| 1 |\n^tab001\n\n> Cita ^cit001\n";
    let doc = render_markdown(texto);
    assert_eq!(doc.blocks.len(), 4, "el párrafo `^tab001` se va: {:?}", doc.blocks);
    assert_eq!(doc.blocks[0].id.as_deref(), Some("abc123"));
    match &doc.blocks[0].kind {
        BlockKind::Paragraph(inl) => assert_eq!(inl, &vec![Inline::Text("Un párrafo".into())]),
        k => panic!("{k:?}"),
    }
    match &doc.blocks[1].kind {
        BlockKind::List { items, .. } => assert_eq!(items[0].blocks[0].id.as_deref(), Some("def456")),
        k => panic!("{k:?}"),
    }
    assert_eq!(doc.blocks[2].id.as_deref(), Some("tab001"));
    assert_eq!(doc.blocks[3].id.as_deref(), Some("cit001"));
}
