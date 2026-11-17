//! Bases producen contenido efímero idéntico en las notas MD y HTML.
mod common;
use common::TempDir;
use orbital_vault::{
    RelPath,
    render::{BlockKind, Embed, render_markdown},
    transclusion::card_html,
};
use std::fs;

fn rel(s: &str) -> RelPath {
    RelPath::new(s).unwrap()
}
fn fixture() -> (TempDir, orbital_vault::Vault) {
    let t = TempDir::new("base-embeds");
    fs::create_dir_all(t.path().join("wiki")).unwrap();
    fs::write(t.path().join("wiki/lista.base"), "format_version: 1\ncolumns: [{property: title, label: Título, type: auto}, {property: estado, label: Estado, type: text}]\nfilters: [{property: estado, value: activo}]\n").unwrap();
    fs::write(
        t.path().join("wiki/a.md"),
        "---\ntitle: '<A & B>'\nestado: activo\n---\nCuerpo intacto\n",
    )
    .unwrap();
    fs::write(t.path().join("wiki/b.html"),"<html><head><script type=\"application/yaml\">\ntitle: HTML\nestado: activo\n</script></head><body>Cuerpo HTML</body></html>").unwrap();
    let vault = orbital_vault::open(t.path()).unwrap();
    (t, vault)
}
fn rows(embed: &Embed) -> usize {
    let Embed::Note { blocks, .. } = embed else {
        panic!("{embed:?}")
    };
    let BlockKind::Table { rows, .. } = &blocks[0].kind else {
        panic!("{blocks:?}")
    };
    rows.len()
}
#[test]
fn markdown_and_html_share_snapshot_and_escape_ephemeral_content() {
    let (t, vault) = fixture();
    let source = "![[lista.base]]\n";
    let mut doc = render_markdown(source);
    let deps = vault.expand_transclusions(&rel("wiki/host.md"), &mut doc);
    assert_eq!(deps, vec![rel("wiki/lista.base")]);
    let BlockKind::Transclusion(transclusion) = &doc.blocks[0].kind else {
        panic!()
    };
    assert_eq!(rows(&transclusion.content), 2);
    let (html_embed, _) = vault.embed_one(&rel("wiki/host.html"), "lista.base", None);
    assert_eq!(html_embed, transclusion.content);
    let html = card_html("lista.base", None, &html_embed, &|p| p.to_string());
    assert!(html.contains("&lt;A &amp; B&gt;"));
    assert!(html.contains("data-orbital-embed"));
    assert!(html.contains("<table>"));
    assert_eq!(
        fs::read_to_string(t.path().join("wiki/a.md")).unwrap(),
        "---\ntitle: '<A & B>'\nestado: activo\n---\nCuerpo intacto\n"
    );
    assert_eq!(source, "![[lista.base]]\n");
}
#[test]
fn index_updates_new_members_removed_members_and_changed_values() {
    let (t, vault) = fixture();
    assert_eq!(
        rows(&vault.embed_one(&rel("wiki/host.md"), "lista.base", None).0),
        2
    );
    fs::write(
        t.path().join("wiki/nueva.md"),
        "---\nestado: activo\n---\nNueva",
    )
    .unwrap();
    vault.update_base_index(&rel("wiki/nueva.md"));
    assert_eq!(
        rows(&vault.embed_one(&rel("wiki/host.md"), "lista.base", None).0),
        3
    );
    fs::write(
        t.path().join("wiki/a.md"),
        "---\nestado: cerrado\n---\nCuerpo intacto",
    )
    .unwrap();
    vault.update_base_index(&rel("wiki/a.md"));
    assert_eq!(
        rows(&vault.embed_one(&rel("wiki/host.md"), "lista.base", None).0),
        2
    );
    fs::remove_file(t.path().join("wiki/b.html")).unwrap();
    vault.update_base_index(&rel("wiki/b.html"));
    assert_eq!(
        rows(&vault.embed_one(&rel("wiki/host.md"), "lista.base", None).0),
        1
    );
}
#[test]
fn missing_ambiguous_invalid_and_escaping_references_do_not_write() {
    let (t, _) = fixture();
    fs::create_dir_all(t.path().join("wiki/otra")).unwrap();
    fs::write(t.path().join("wiki/otra/lista.base"), "format_version: 1\n").unwrap();
    fs::write(t.path().join("wiki/invalida.base"), "format_version: 99\n").unwrap();
    let vault = orbital_vault::open(t.path()).unwrap();
    assert!(matches!(
        vault.embed_one(&rel("wiki/h.md"), "lista.base", None).0,
        Embed::Ambiguous(_)
    ));
    assert!(matches!(
        vault.embed_one(&rel("wiki/h.md"), "falta.base", None).0,
        Embed::Missing
    ));
    assert!(matches!(
        vault.embed_one(&rel("wiki/h.md"), "../escape.base", None).0,
        Embed::Missing
    ));
    let (invalid, _) = vault.embed_one(&rel("wiki/h.md"), "invalida.base", None);
    let html = card_html("invalida.base", None, &invalid, &|p| p.to_string());
    assert!(html.contains("No se pudo consultar la base"));
    assert_eq!(
        fs::read_to_string(t.path().join("wiki/invalida.base")).unwrap(),
        "format_version: 99\n"
    );
}

#[test]
fn cards_links_and_large_embeds_have_bounded_visible_content() {
    let (t, _) = fixture();
    for n in 0..105 {
        fs::write(
            t.path().join(format!("wiki/z{n}.md")),
            "---\nestado: activo\n---\n",
        )
        .unwrap();
    }
    let vault = orbital_vault::open(t.path()).unwrap();
    let (table, _) = vault.embed_one(&rel("wiki/host.html"), "lista.base", None);
    assert_eq!(rows(&table), 100);
    let html = card_html("lista.base", None, &table, &|p| p.to_string());
    assert!(html.contains("Mostrando 100 de 107"));
    assert!(html.contains("orbital-path:wiki/a.md"));
    assert!(html.contains("orbital-path:wiki/lista.base"));
    let source = fs::read_to_string(t.path().join("wiki/lista.base")).unwrap();
    fs::write(
        t.path().join("wiki/lista.base"),
        format!("{source}view: cards\n"),
    )
    .unwrap();
    let (cards, _) = vault.embed_one(&rel("wiki/host.md"), "lista.base", None);
    let Embed::Note { blocks, .. } = cards else {
        panic!()
    };
    assert!(matches!(blocks[0].kind, BlockKind::Quote(_)));
    assert_eq!(blocks.len(), 101);
}
