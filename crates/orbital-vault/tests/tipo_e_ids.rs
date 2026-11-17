//! 043: `tipo` como alias de `type` e identificadores de nota únicos en la bóveda.

mod common;

use std::fs;

use common::{TempDir, ctx};
use orbital_vault::bases::{BaseDefinition, BaseFilter};
use orbital_vault::props::{PropValue, Warning};
use orbital_vault::{RelPath, Vault};

fn rel(s: &str) -> RelPath {
    RelPath::new(s).unwrap()
}

fn vault_with(files: &[(&str, &str)]) -> (TempDir, Vault) {
    let t = TempDir::new("tipo-ids");
    fs::create_dir_all(t.path().join("wiki")).unwrap();
    fs::create_dir_all(t.path().join("raw")).unwrap();
    for (p, c) in files {
        fs::write(t.path().join(p), c).unwrap();
    }
    let v = orbital_vault::open(t.path()).unwrap();
    (t, v)
}

#[test]
fn tipo_es_alias_de_type() {
    let (_t, v) = vault_with(&[
        ("wiki/cuaderno.md", "---\ntipo: cuaderno\n---\n# C\n"),
        ("wiki/ambas.md", "---\ntipo: cuaderno\ntype: atomo\n---\n"),
        ("wiki/nada.md", "---\ntitle: x\n---\n"),
    ]);
    let c = v.read_note(&rel("wiki/cuaderno.md")).unwrap();
    assert!(!c.warnings().contains(&Warning::MissingType));
    assert!(c.warnings().is_empty(), "{:?}", c.warnings());
    assert_eq!(c.kind(), Some("cuaderno"));
    assert_eq!(c.type_key(), "tipo");

    // Con las dos, gana `type`.
    let a = v.read_note(&rel("wiki/ambas.md")).unwrap();
    assert_eq!(a.kind(), Some("atomo"));
    assert_eq!(a.type_key(), "type");

    let n = v.read_note(&rel("wiki/nada.md")).unwrap();
    assert!(n.warnings().contains(&Warning::MissingType));
    assert_eq!(n.type_key(), "type");
}

#[test]
fn bases_filtra_type_por_el_alias_y_edita_el_alias() {
    let (t, v) = vault_with(&[
        ("wiki/a.md", "---\ntipo: cuaderno\n---\n"),
        ("wiki/b.md", "---\ntype: atomo\n---\n"),
    ]);
    let def = BaseDefinition {
        filters: vec![BaseFilter::Property {
            key: "type".into(),
            value: Some(PropValue::Text("cuaderno".into())),
        }],
        ..Default::default()
    };
    let snap = v.base_snapshot(&def);
    assert_eq!(snap.rows.len(), 1);
    assert_eq!(snap.rows[0].path, rel("wiki/a.md"));

    let note = v.read_note(&rel("wiki/a.md")).unwrap();
    orbital_vault::bases::edit_cell(
        &v,
        &note.path,
        note.hash,
        "type",
        Some(&PropValue::Text("libreta".into())),
        &ctx(),
    )
    .unwrap();
    let texto = fs::read_to_string(t.path().join("wiki/a.md")).unwrap();
    assert!(texto.contains("tipo: libreta"), "{texto}");
    assert!(!texto.contains("type:"), "{texto}");
}

#[test]
fn ids_de_la_boveda_y_repetidos() {
    let (t, v) = vault_with(&[
        ("wiki/uno.md", "---\nid: AT-001\ntype: atomo\n---\n"),
        (
            "wiki/dos.md",
            "---\ntype: atomo\n---\n<!-- block 1 AT-002 -->\n# Dos\n<!---->\n",
        ),
        ("wiki/tres.md", "---\nid: at-001\ntipo: cuaderno\n---\n"),
    ]);
    let ids = v.used_note_ids();
    assert!(ids.contains("AT-001") && ids.contains("AT-002"), "{ids:?}");
    assert_eq!(
        v.duplicate_note_ids(),
        vec![(
            "AT-001".to_owned(),
            vec![rel("wiki/tres.md"), rel("wiki/uno.md")]
        )]
    );
    assert_eq!(v.notes_with_note_id("AT-002"), vec![rel("wiki/dos.md")]);

    // Al cambiar una nota, el índice se pone al día: deja de haber repetidos.
    fs::write(
        t.path().join("wiki/tres.md"),
        "---\nid: CU-001\ntipo: cuaderno\n---\n",
    )
    .unwrap();
    v.update_base_index(&rel("wiki/tres.md"));
    assert!(v.duplicate_note_ids().is_empty());
    assert!(v.used_note_ids().contains("CU-001"));
}
