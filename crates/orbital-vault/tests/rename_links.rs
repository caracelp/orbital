//! T030: actualización de enlaces al renombrar una nota (US6 de la 003).

mod common;

use std::fs;

use common::{TempDir, ctx};
use orbital_vault::{RelPath, Vault, VaultError};

fn rel(s: &str) -> RelPath {
    RelPath::new(s).unwrap()
}

fn vault(files: &[(&str, &str)]) -> (TempDir, Vault) {
    let t = TempDir::new("renombrar");
    for (path, text) in files {
        let p = t.path().join(path);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, text).unwrap();
    }
    let v = orbital_vault::open(t.path()).unwrap();
    (t, v)
}

const DESTINO: &str = "---\ntype: atomo\n---\n# Viejo\n\nVer [[viejo]] (a sí misma).\n";

#[test]
fn wikilinks_keep_alias_anchor_and_style() {
    let (_t, v) = vault(&[("wiki/fisica/viejo.md", DESTINO), ("wiki/a.md", "")]);
    let text = "\
[[viejo]] [[Viejo]] [[ viejo ]] [[viejo|Mi alias]] [[viejo#Introducción]] [[viejo#Sec|A]]
![[viejo]] ![[viejo#Sec]] [[fisica/viejo]] [[viejo.md]] [[viejo^bloque]]
[[otro]] [[viejos]]
```
[[viejo]] en código
```
`[[viejo]]` en línea
";
    let (out, n) = v.rewrite_links(
        &rel("wiki/a.md"),
        text,
        false,
        &rel("wiki/fisica/viejo.md"),
        &rel("wiki/fisica/nuevo.md"),
    );
    assert_eq!(n, 11, "{out}");
    assert_eq!(
        out,
        "\
[[nuevo]] [[nuevo]] [[ nuevo ]] [[nuevo|Mi alias]] [[nuevo#Introducción]] [[nuevo#Sec|A]]
![[nuevo]] ![[nuevo#Sec]] [[fisica/nuevo]] [[nuevo.md]] [[nuevo^bloque]]
[[otro]] [[viejos]]
```
[[viejo]] en código
```
`[[viejo]]` en línea
"
    );
}

#[test]
fn markdown_and_html_links_keep_their_form() {
    let (_t, v) = vault(&[
        ("wiki/fisica/viejo.md", DESTINO),
        ("wiki/quimica/q.md", ""),
        ("wiki/quimica/q.html", ""),
    ]);
    let (old, new) = (rel("wiki/fisica/viejo.md"), rel("wiki/fisica/nuevo.md"));
    let md = "\
[A](/fisica/viejo.md) [B](/fisica/viejo.md#sec \"título\") [C](../fisica/viejo.md)
[D](<../fisica/viejo.md>) [E](https://ejemplo.com/fisica/viejo.md) [F](/raw/viejo.md)
";
    let (out, n) = v.rewrite_links(&rel("wiki/quimica/q.md"), md, false, &old, &new);
    assert_eq!(n, 4, "{out}");
    assert_eq!(
        out,
        "\
[A](/fisica/nuevo.md) [B](/fisica/nuevo.md#sec \"título\") [C](../fisica/nuevo.md)
[D](<../fisica/nuevo.md>) [E](https://ejemplo.com/fisica/viejo.md) [F](/raw/viejo.md)
"
    );

    // HTML: solo cambia el valor de href; la etiqueta y el texto quedan iguales.
    let html =
        "<p><a class=\"x\" HREF='../fisica/viejo.md#s'>la <b>nota</b></a> y [[viejo|v]]</p>\n";
    let (out, n) = v.rewrite_links(&rel("wiki/quimica/q.html"), html, true, &old, &new);
    assert_eq!(n, 2, "{out}");
    assert_eq!(
        out,
        "<p><a class=\"x\" HREF='../fisica/nuevo.md#s'>la <b>nota</b></a> y [[nuevo|v]]</p>\n"
    );
}

#[test]
fn only_links_that_resolve_to_the_note() {
    // Dos `viejo.md`: `[[viejo]]` es ambiguo y no apunta a ninguna de las dos.
    let (_t, v) = vault(&[
        ("wiki/fisica/viejo.md", DESTINO),
        ("wiki/quimica/viejo.md", DESTINO),
        ("wiki/a.md", ""),
    ]);
    let text = "[[viejo]] [[fisica/viejo]] [[quimica/viejo]] [x](/quimica/viejo.md)\n";
    let (out, n) = v.rewrite_links(
        &rel("wiki/a.md"),
        text,
        false,
        &rel("wiki/fisica/viejo.md"),
        &rel("wiki/fisica/nuevo.md"),
    );
    assert_eq!(n, 1);
    assert_eq!(
        out,
        "[[viejo]] [[fisica/nuevo]] [[quimica/viejo]] [x](/quimica/viejo.md)\n"
    );
}

#[test]
fn new_name_with_spaces_folder_or_taken() {
    let (_t, v) = vault(&[
        ("wiki/fisica/viejo.md", DESTINO),
        ("wiki/otra/nuevo.md", ""),
        ("wiki/a.md", ""),
    ]);
    let old = rel("wiki/fisica/viejo.md");
    // Mover a otra carpeta con espacios: las URL se codifican salvo entre `<…>`.
    let moved = rel("wiki/mi carpeta/nombre nuevo.md");
    let text = "[[viejo]] [[fisica/viejo]] [a](/fisica/viejo.md) [b](<fisica/viejo.md>)\n";
    let (out, _) = v.rewrite_links(&rel("wiki/a.md"), text, false, &old, &moved);
    assert_eq!(
        out,
        "[[nombre nuevo]] [[mi carpeta/nombre nuevo]] [a](/mi%20carpeta/nombre%20nuevo.md) \
         [b](<mi carpeta/nombre nuevo.md>)\n"
    );
    // Si el nombre nuevo ya lo tiene otra nota, el wikilink lleva la carpeta para no quedar ambiguo.
    let (out, _) = v.rewrite_links(
        &rel("wiki/a.md"),
        "[[viejo|x]]\n",
        false,
        &old,
        &rel("wiki/fisica/nuevo.md"),
    );
    assert_eq!(out, "[[fisica/nuevo|x]]\n");
}

#[test]
fn plan_apply_and_revert() {
    let a = "Uno: [[viejo]].\n";
    let b = "<p><a href=\"fisica/viejo.md\">dos</a></p>\n";
    let c = "Sin enlaces.\n";
    let (t, v) = vault(&[
        ("wiki/fisica/viejo.md", DESTINO),
        ("wiki/a.md", a),
        ("wiki/b.html", b),
        ("wiki/c.md", c),
    ]);
    let (old, new) = (rel("wiki/fisica/viejo.md"), rel("wiki/fisica/nuevo.md"));
    let plan = v.plan_link_updates(&old, &new, None);
    assert_eq!((plan.links(), plan.files()), (3, 3));
    let changed: Vec<&str> = plan.changes.iter().map(|c| c.path.as_str()).collect();
    assert_eq!(
        changed,
        ["wiki/a.md", "wiki/b.html", "wiki/fisica/viejo.md"]
    );
    // Nada se escribió todavía.
    assert_eq!(fs::read_to_string(t.path().join("wiki/a.md")).unwrap(), a);

    v.rename(&old, &new, &ctx()).unwrap();
    let (receipt, failed) = v.apply_link_updates(&plan, &ctx());
    assert!(failed.is_empty(), "{failed:?}");
    assert!(receipt.written.len() >= 3);
    let read = |p: &str| fs::read_to_string(t.path().join(p)).unwrap();
    assert_eq!(read("wiki/a.md"), "Uno: [[nuevo]].\n");
    assert_eq!(
        read("wiki/b.html"),
        "<p><a href=\"fisica/nuevo.md\">dos</a></p>\n"
    );
    // La nota renombrada se enlazaba a sí misma: se escribe en su ruta nueva.
    assert!(read("wiki/fisica/nuevo.md").contains("Ver [[nuevo]] (a sí misma)."));
    assert_eq!(read("wiki/c.md"), c);

    // Deshacer: primero los enlaces, después el nombre. Todo vuelve a ser igual byte a byte.
    let (_, failed) = v.revert_link_updates(&plan, &ctx());
    assert!(failed.is_empty(), "{failed:?}");
    v.rename(&new, &old, &ctx()).unwrap();
    assert_eq!(read("wiki/a.md"), a);
    assert_eq!(read("wiki/b.html"), b);
    assert_eq!(read("wiki/fisica/viejo.md"), DESTINO);
}

#[test]
fn a_file_changed_meanwhile_is_not_overwritten() {
    let (t, v) = vault(&[
        ("wiki/fisica/viejo.md", "# Viejo\n"),
        ("wiki/a.md", "[[viejo]]\n"),
        ("wiki/b.md", "[[viejo]]\n"),
    ]);
    let (old, new) = (rel("wiki/fisica/viejo.md"), rel("wiki/fisica/nuevo.md"));
    let plan = v.plan_link_updates(&old, &new, Some(&[rel("wiki/a.md"), rel("wiki/b.md")]));
    assert_eq!(plan.files(), 2);
    fs::write(
        t.path().join("wiki/b.md"),
        "[[viejo]] y algo escrito afuera\n",
    )
    .unwrap();
    v.rename(&old, &new, &ctx()).unwrap();
    let (_, failed) = v.apply_link_updates(&plan, &ctx());
    assert_eq!(failed.len(), 1);
    assert_eq!(failed[0].0, rel("wiki/b.md"));
    assert!(matches!(failed[0].1, VaultError::Conflict { .. }));
    assert_eq!(
        fs::read_to_string(t.path().join("wiki/b.md")).unwrap(),
        "[[viejo]] y algo escrito afuera\n"
    );
    assert_eq!(
        fs::read_to_string(t.path().join("wiki/a.md")).unwrap(),
        "[[nuevo]]\n"
    );
}
