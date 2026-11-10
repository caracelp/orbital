//! T058: plantillas (`type: plantilla`) con variables.

mod common;

use std::fs;

use common::{TempDir, copy_tree, ctx, fixture};
use orbital_vault::props::PropValue;
use orbital_vault::template::{Vars, render};
use orbital_vault::{RelPath, Vault};

fn rel(s: &str) -> RelPath {
    RelPath::new(s).unwrap()
}

#[test]
fn render_replaces_known_and_keeps_unknown() {
    let vars = Vars {
        fecha: "2026-11-09".into(),
        hora: "18:30".into(),
        titulo: "Calor".into(),
        carpeta: "fisica".into(),
    };
    let (out, unknown) = render(
        "# {{titulo}}\nCreada el {{ fecha }} a las {{hora}} en {{carpeta}}. {{autor}} {{titulo}}",
        &vars,
    );
    assert_eq!(
        out,
        "# Calor\nCreada el 2026-11-09 a las 18:30 en fisica. {{autor}} Calor"
    );
    assert_eq!(unknown, vec!["autor".to_owned()]);
    assert_eq!(render("sin variables {{", &vars).0, "sin variables {{");
}

fn vault_with_templates() -> (TempDir, Vault) {
    let t = TempDir::new("plantillas");
    copy_tree(&fixture("completa"), t.path());
    fs::create_dir_all(t.path().join("wiki/plantillas")).unwrap();
    fs::write(
        t.path().join("wiki/plantillas/concepto.md"),
        "---\ntype: plantilla\ntitle: \"{{titulo}}\"\ntags: [concepto]\ncreated: \"{{fecha}}\"\n---\n# {{titulo}}\n\nCreado en {{carpeta}} a las {{hora}}. {{desconocida}}\n",
    )
    .unwrap();
    fs::write(
        t.path().join("wiki/plantillas/sintesis.html"),
        "<!doctype html>\n<html><head><title>{{titulo}}</title>\n<script type=\"application/yaml\">\ntype: plantilla\ntitle: \"{{titulo}}\"\n</script>\n</head><body><h1>{{titulo}}</h1></body></html>\n",
    )
    .unwrap();
    let v = orbital_vault::open(t.path()).unwrap();
    (t, v)
}

#[test]
fn note_from_md_template() {
    let (t, v) = vault_with_templates();
    let (path, warnings) = v
        .create_from_template(
            &rel("wiki/fisica"),
            "Calor",
            &rel("wiki/plantillas/concepto.md"),
            &ctx(),
        )
        .unwrap();
    assert_eq!(path.as_str(), "wiki/fisica/Calor.md");
    let note = v.read_note(&path).unwrap();
    assert_eq!(note.prop("type"), Some(&PropValue::Text("atomo".into())));
    assert_eq!(note.prop("title"), Some(&PropValue::Text("Calor".into())));
    assert_eq!(
        note.prop("tags"),
        Some(&PropValue::List(vec![PropValue::Text("concepto".into())]))
    );
    assert!(
        note.body()
            .contains("# Calor\n\nCreado en wiki/fisica a las 18:30.")
    );
    assert!(note.body().contains("{{desconocida}}"));
    assert_eq!(warnings, vec!["desconocida".to_owned()]);
    // Entra en el índice y en el registro como cualquier nota nueva.
    assert!(
        fs::read_to_string(t.path().join("wiki/index.md"))
            .unwrap()
            .contains("(/fisica/Calor.md)")
    );
}

#[test]
fn note_from_html_template() {
    let (_t, v) = vault_with_templates();
    let (path, _) = v
        .create_from_template(
            &rel("wiki"),
            "Panorama",
            &rel("wiki/plantillas/sintesis.html"),
            &ctx(),
        )
        .unwrap();
    assert_eq!(path.as_str(), "wiki/Panorama.html");
    let note = v.read_note(&path).unwrap();
    assert_eq!(note.prop("type"), Some(&PropValue::Text("sintesis".into())));
    assert!(note.source.contains("<h1>Panorama</h1>"));
}

#[test]
fn templates_are_listed_and_kept_out_of_new_notes() {
    let (_t, v) = vault_with_templates();
    let found = v.templates();
    assert_eq!(
        found.iter().map(|p| p.as_str()).collect::<Vec<_>>(),
        vec![
            "wiki/plantillas/concepto.md",
            "wiki/plantillas/sintesis.html"
        ]
    );
}
