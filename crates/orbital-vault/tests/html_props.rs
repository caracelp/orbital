//! T041 / SC-004: propiedades en notas HTML.

mod common;

use std::fs;

use common::{TempDir, copy_tree, ctx, fixture};
use orbital_vault::props::{PropValue, Warning};
use orbital_vault::{Format, RelPath, Vault, VaultError};

fn setup() -> (TempDir, Vault) {
    let t = TempDir::new("html");
    copy_tree(&fixture("notas"), &t.path().join("wiki"));
    fs::create_dir(t.path().join("raw")).unwrap();
    let v = orbital_vault::open(t.path()).unwrap();
    (t, v)
}

fn rel(s: &str) -> RelPath {
    RelPath::new(s).unwrap()
}

/// Verifica que el documento conserva su esqueleto y que el bloque está dentro de <head>.
fn assert_skeleton(html: &str) {
    let lower = html.to_ascii_lowercase();
    let pos = |s: &str| lower.find(s).unwrap_or_else(|| panic!("falta {s}: {html}"));
    let block = pos("<script type=\"application/yaml\">");
    assert!(pos("<html") < pos("<head") && pos("<head") < block);
    assert!(block < pos("</head>") && pos("</head>") < pos("<body"));
    assert_eq!(
        lower.matches("</script>").count(),
        lower.matches("<script").count()
    );
}

#[test]
fn md_and_html_give_same_props() {
    let (t, v) = setup();
    fs::write(
        t.path().join("wiki/termo.md"),
        "---\ntype: sintesis\ntitle: Termo\ntags: [fisica, termo]\n---\n",
    )
    .unwrap();
    let md = v.read_note(&rel("wiki/termo.md")).unwrap();
    let html = v.read_note(&rel("wiki/bloque.html")).unwrap();
    assert_eq!(html.format, Format::Html);
    let strip = |n: &orbital_vault::Note| {
        n.props
            .as_ref()
            .unwrap()
            .entries
            .iter()
            .map(|e| (e.key.clone(), e.value.clone()))
            .collect::<Vec<_>>()
    };
    assert_eq!(strip(&md), strip(&html));
}

#[test]
fn script_close_is_escaped() {
    let (t, v) = setup();
    let p = rel("wiki/bloque.html");
    let note = v.read_note(&p).unwrap();
    v.set_prop(
        &note,
        "description",
        &PropValue::Text("usa </script> en el texto".into()),
        &ctx(),
    )
    .unwrap();
    let html = fs::read_to_string(p.to_path(t.path())).unwrap();
    assert!(html.contains("description: \"usa <\\/script> en el texto\""));
    assert_skeleton(&html);
    let back = v.read_note(&p).unwrap();
    assert_eq!(
        back.prop("description"),
        Some(&PropValue::Text("usa </script> en el texto".into()))
    );
}

#[test]
fn html_without_block_gets_one_inside_head() {
    let (t, v) = setup();
    let p = rel("wiki/sin-bloque.html");
    let note = v.read_note(&p).unwrap();
    assert!(note.props.is_none());
    v.set_prop(&note, "type", &PropValue::Text("sintesis".into()), &ctx())
        .unwrap();
    let html = fs::read_to_string(p.to_path(t.path())).unwrap();
    assert_skeleton(&html);
    assert!(html.starts_with("<!DOCTYPE html>\n<html>\n<head>\n  <title>Sin bloque</title>\n"));
    assert_eq!(
        v.read_note(&p).unwrap().prop("type"),
        Some(&PropValue::Text("sintesis".into()))
    );
}

#[test]
fn html_without_head_is_refused() {
    let (t, v) = setup();
    let p = rel("wiki/sin-head.html");
    let before = fs::read(p.to_path(t.path())).unwrap();
    let note = v.read_note(&p).unwrap();
    assert!(matches!(
        v.set_prop(&note, "type", &PropValue::Text("sintesis".into()), &ctx()),
        Err(VaultError::NoHead(_))
    ));
    assert_eq!(fs::read(p.to_path(t.path())).unwrap(), before);
}

#[test]
fn first_block_wins_with_warning() {
    let (_t, v) = setup();
    let note = v.read_note(&rel("wiki/dos-bloques.html")).unwrap();
    assert_eq!(note.prop("title"), Some(&PropValue::Text("primero".into())));
    assert!(note.warnings().contains(&Warning::ExtraBlocks(1)));
}

#[test]
fn comments_and_other_scripts_are_skipped() {
    let (_t, v) = setup();
    let note = v.read_note(&rel("wiki/comentado.html")).unwrap();
    assert_eq!(note.prop("title"), Some(&PropValue::Text("real".into())));
}

#[test]
fn create_note_html_and_md() {
    let (t, v) = setup();
    let html = v
        .create_note(&rel("wiki"), "Síntesis nueva", Format::Html, &ctx())
        .unwrap();
    let text = fs::read_to_string(html.to_path(t.path())).unwrap();
    assert_skeleton(&text);
    let note = v.read_note(&html).unwrap();
    assert_eq!(note.prop("type"), Some(&PropValue::Text("sintesis".into())));
    assert_eq!(
        note.prop("title"),
        Some(&PropValue::Text("Síntesis nueva".into()))
    );
    assert_eq!(
        note.prop("created"),
        Some(&PropValue::DateTime("2026-11-09T18:30:05-03:00".into()))
    );

    let md = v
        .create_note(&rel("wiki"), "Átomo", Format::Md, &ctx())
        .unwrap();
    assert_eq!(md.as_str(), "wiki/Átomo.md");
    let note = v.read_note(&md).unwrap();
    assert_eq!(note.prop("type"), Some(&PropValue::Text("atomo".into())));
    assert_eq!(
        note.prop("generated"),
        Some(&PropValue::Map(vec![
            ("by".into(), PropValue::Text("human:prueba".into())),
            (
                "at".into(),
                PropValue::DateTime("2026-11-09T18:30:05-03:00".into())
            ),
        ]))
    );

    assert!(matches!(
        v.create_note(&rel("wiki"), "index", Format::Md, &ctx()),
        Err(VaultError::ReservedName(_))
    ));
    assert!(matches!(
        v.create_note(&rel("wiki"), "Átomo", Format::Md, &ctx()),
        Err(VaultError::Conflict { .. })
    ));
}

#[test]
fn warnings_for_okf_and_known_keys() {
    let (t, v) = setup();
    fs::write(
        t.path().join("wiki/avisos.md"),
        "---\ntitle: 3\nstatus: listo\nverified: { by: human:ejemplo, at: 2026-11-09T10:00:00Z }\ntags: suelto\nextra: conservada\n---\n",
    )
    .unwrap();
    let note = v.read_note(&rel("wiki/avisos.md")).unwrap();
    let w = note.warnings();
    assert!(w.contains(&Warning::MissingType));
    assert!(w.contains(&Warning::InvalidStatus("listo".into())));
    assert!(w.contains(&Warning::UnexpectedType {
        key: "title".into()
    }));
    assert!(w.contains(&Warning::UnexpectedType { key: "tags".into() }));
    assert!(
        !w.iter()
            .any(|x| matches!(x, Warning::UnexpectedType { key } if key == "verified"))
    );
    assert_eq!(
        note.prop("extra"),
        Some(&PropValue::Text("conservada".into()))
    );

    // Fuera de wiki/ no se exige `type`.
    fs::write(t.path().join("suelta.md"), "---\ntitle: x\n---\n").unwrap();
    let loose = v.read_note(&rel("suelta.md")).unwrap();
    assert!(!loose.warnings().contains(&Warning::MissingType));
}

#[test]
fn reserved_files_never_ask_for_type() {
    let t = TempDir::new("reservados");
    copy_tree(&fixture("completa"), t.path());
    let v = orbital_vault::open(t.path()).unwrap();
    for p in ["wiki/index.md", "wiki/log.md"] {
        let note = v.read_note(&rel(p)).unwrap();
        assert!(
            !note.warnings().contains(&Warning::MissingType),
            "{p} no debe pedir type: OKF §8/§9 no le permite propiedades"
        );
    }
}
