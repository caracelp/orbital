//! T040 / SC-005: leer y guardar sin cambios deja los archivos idénticos; editar una propiedad
//! cambia solo sus líneas.

mod common;

use std::fs;
use std::path::Path;

use common::{TempDir, copy_tree, ctx, fixture};
use orbital_vault::props::PropValue;
use orbital_vault::{RelPath, Vault, VaultError};

fn setup() -> (TempDir, Vault) {
    let t = TempDir::new("roundtrip");
    copy_tree(&fixture("notas"), &t.path().join("wiki"));
    fs::create_dir(t.path().join("raw")).unwrap();
    let v = orbital_vault::open(t.path()).unwrap();
    (t, v)
}

fn rel(s: &str) -> RelPath {
    RelPath::new(s).unwrap()
}

fn read(root: &Path, p: &str) -> String {
    fs::read_to_string(root.join(p)).unwrap()
}

#[test]
fn save_without_changes_is_byte_identical() {
    let (t, v) = setup();
    for name in fs::read_dir(t.path().join("wiki")).unwrap() {
        let name = name.unwrap().file_name().into_string().unwrap();
        let p = rel(&format!("wiki/{name}"));
        let before = fs::read(p.to_path(t.path())).unwrap();
        let note = v.read_note(&p).unwrap();
        let receipt = v.save_note(&note, &note.source, &ctx()).unwrap();
        assert!(receipt.written.is_empty(), "{name}: no debería escribir");
        assert_eq!(
            fs::read(p.to_path(t.path())).unwrap(),
            before,
            "{name} cambió"
        );
    }
    assert!(!t.path().join(".orbital").exists());
}

#[test]
fn set_prop_changes_only_its_lines() {
    let (t, v) = setup();
    let p = rel("wiki/lf.md");
    let note = v.read_note(&p).unwrap();
    v.set_prop(
        &note,
        "title",
        &PropValue::Text("Entropía (S)".into()),
        &ctx(),
    )
    .unwrap();
    assert_eq!(
        read(t.path(), "wiki/lf.md"),
        "---\ntype: atomo\ntitle: Entropía (S)\n# comentario sobre tags\ntags: [fisica, termo]\nsources:\n  - id: callen\n    resource: /raw/callen.pdf\n---\n# Entropía\n\nCuerpo con --- en el medio.\n"
    );

    // Una clave con valor en bloque se reemplaza entera, sin tocar el comentario anterior.
    let note = v.read_note(&p).unwrap();
    v.set_prop(&note, "sources", &PropValue::List(vec![]), &ctx())
        .unwrap();
    assert!(
        read(t.path(), "wiki/lf.md")
            .contains("# comentario sobre tags\ntags: [fisica, termo]\nsources: []\n---\n")
    );

    // Se guardó un respaldo de cada versión anterior.
    assert!(t.path().join(".orbital/respaldo/2026-11-09/wiki").is_dir());
}

#[test]
fn new_key_goes_at_end_with_file_line_ending() {
    let (t, v) = setup();
    let p = rel("wiki/crlf.md");
    let note = v.read_note(&p).unwrap();
    v.set_prop(&note, "status", &PropValue::Text("draft".into()), &ctx())
        .unwrap();
    assert_eq!(
        read(t.path(), "wiki/crlf.md"),
        "---\r\ntype: atomo\r\ntitle: Ventanas\r\nstatus: draft\r\n---\r\n# Ventanas\r\n"
    );
}

#[test]
fn remove_prop_removes_only_its_lines() {
    let (t, v) = setup();
    let p = rel("wiki/lf.md");
    let note = v.read_note(&p).unwrap();
    v.remove_prop(&note, "sources", &ctx()).unwrap();
    assert_eq!(
        read(t.path(), "wiki/lf.md"),
        "---\ntype: atomo\ntitle: Entropía\n# comentario sobre tags\ntags: [fisica, termo]\n---\n# Entropía\n\nCuerpo con --- en el medio.\n"
    );
}

#[test]
fn bom_and_missing_frontmatter() {
    let (t, v) = setup();
    let note = v.read_note(&rel("wiki/bom.md")).unwrap();
    let title = note.prop("title").unwrap();
    assert_eq!(title, &PropValue::Text("Con BOM".into()));

    let p = rel("wiki/sin-frontmatter.md");
    let note = v.read_note(&p).unwrap();
    assert!(note.props.is_none());
    v.set_prop(&note, "type", &PropValue::Text("atomo".into()), &ctx())
        .unwrap();
    assert_eq!(
        read(t.path(), "wiki/sin-frontmatter.md"),
        "---\ntype: atomo\n---\n# Solo cuerpo\n\nSin propiedades.\n"
    );

    // Un `---` sin cierre no es frontmatter.
    assert!(
        v.read_note(&rel("wiki/sin-cierre.md"))
            .unwrap()
            .props
            .is_none()
    );
}

#[test]
fn invalid_props_block_edits_and_never_rewrites() {
    let (t, v) = setup();
    let p = rel("wiki/invalido.md");
    let before = fs::read(p.to_path(t.path())).unwrap();
    let note = v.read_note(&p).unwrap();
    let err = note.props.as_ref().unwrap().error.as_ref().unwrap();
    assert!(err.line >= 2);
    assert!(matches!(
        v.set_prop(&note, "title", &PropValue::Text("x".into()), &ctx()),
        Err(VaultError::InvalidProps { .. })
    ));
    assert_eq!(fs::read(p.to_path(t.path())).unwrap(), before);
}

#[test]
fn external_change_is_a_conflict() {
    let (t, v) = setup();
    let p = rel("wiki/lf.md");
    let note = v.read_note(&p).unwrap();
    fs::write(p.to_path(t.path()), "cambiado por otro programa\n").unwrap();
    assert!(matches!(
        v.set_prop(&note, "title", &PropValue::Text("x".into()), &ctx()),
        Err(VaultError::Conflict { .. })
    ));
    assert_eq!(read(t.path(), "wiki/lf.md"), "cambiado por otro programa\n");
}

#[test]
fn not_utf8_and_raw_are_protected() {
    let (t, v) = setup();
    fs::write(t.path().join("wiki/latin1.md"), b"caf\xe9\n").unwrap();
    assert!(matches!(
        v.read_note(&rel("wiki/latin1.md")),
        Err(VaultError::NotUtf8(_))
    ));
    fs::write(t.path().join("raw/fuente.md"), "---\ntitle: fuente\n---\n").unwrap();
    let note = v.read_note(&rel("raw/fuente.md")).unwrap();
    assert!(note.read_only);
    assert!(matches!(
        v.set_prop(&note, "title", &PropValue::Text("x".into()), &ctx()),
        Err(VaultError::ReadOnly(_))
    ));
}
