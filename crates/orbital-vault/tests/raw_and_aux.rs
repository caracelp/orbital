//! T064: `raw/` de solo lectura, copiar fuentes, enlaces y archivos auxiliares (SC-007, SC-008).

mod common;

use std::fs;

use common::{TempDir, copy_tree, ctx, fixture, snapshot};
use orbital_vault::links::Resolution;
use orbital_vault::props::PropValue;
use orbital_vault::{RelPath, Vault, VaultError};

fn rel(s: &str) -> RelPath {
    RelPath::new(s).unwrap()
}

fn vault() -> (TempDir, Vault) {
    let t = TempDir::new("raw-aux");
    copy_tree(&fixture("completa"), t.path());
    fs::write(
        t.path().join("wiki/fisica/calor.md"),
        "---\ntype: atomo\ntitle: Calor\nsources:\n  - id: f\n    resource: /raw/fuente.txt\n---\nVer [entropía](/fisica/entropia.md) y [[termodinamica]].\n",
    )
    .unwrap();
    let v = orbital_vault::open(t.path()).unwrap();
    (t, v)
}

#[test]
fn raw_is_read_only_except_copy_into_raw() {
    let (t, v) = vault();
    let c = ctx();
    let raw_before = snapshot(&t.path().join("raw"));
    let src = rel("raw/fuente.txt");
    let note = v.read_note(&rel("wiki/fisica/entropia.md")).unwrap();

    assert!(matches!(
        v.rename(&src, &rel("raw/otra.txt"), &c),
        Err(VaultError::ReadOnly(_))
    ));
    assert!(matches!(v.delete(&src), Err(VaultError::ReadOnly(_))));
    assert!(matches!(
        v.create_note(&rel("raw"), "x", orbital_vault::Format::Md, &c),
        Err(VaultError::ReadOnly(_))
    ));
    assert!(matches!(
        v.move_to_vault_trash(&src, &c),
        Err(VaultError::ReadOnly(_))
    ));
    let _ = note;
    assert_eq!(snapshot(&t.path().join("raw")), raw_before);

    // copy_into_raw: copia idéntica y sin pisar.
    let outside = TempDir::new("afuera");
    let pdf = outside.path().join("artículo.pdf");
    fs::write(&pdf, b"%PDF-1.7 \x00\xff binario").unwrap();
    let a = v.copy_into_raw(&pdf, &c).unwrap();
    assert_eq!(a.as_str(), "raw/artículo.pdf");
    assert_eq!(
        fs::read(a.to_path(t.path())).unwrap(),
        fs::read(&pdf).unwrap()
    );
    let b = v.copy_into_raw(&pdf, &c).unwrap();
    assert_eq!(b.as_str(), "raw/artículo-1.pdf");
    assert!(fs::read(&pdf).is_ok(), "el original no se toca");
}

#[test]
fn links_resolve() {
    let (t, v) = vault();
    let from = rel("wiki/fisica/calor.md");
    let found = |s: &str| match v.resolve_link(&from, s) {
        Resolution::Found(p) => p.to_string(),
        other => format!("{other:?}"),
    };
    assert_eq!(found("/raw/fuente.txt"), "raw/fuente.txt");
    assert_eq!(found("/fisica/entropia.md"), "wiki/fisica/entropia.md");
    assert_eq!(found("./entropia.md"), "wiki/fisica/entropia.md");
    assert_eq!(
        found("../termodinamica.html#seccion"),
        "wiki/termodinamica.html"
    );
    assert_eq!(found("[[termodinamica]]"), "wiki/termodinamica.html");
    assert_eq!(found("[[Entropia|la entropía]]"), "wiki/fisica/entropia.md");
    assert_eq!(
        found("[[fisica/entropia#Definición]]"),
        "wiki/fisica/entropia.md"
    );
    assert!(matches!(
        v.resolve_link(&from, "[[no existe]]"),
        Resolution::Missing
    ));
    assert!(matches!(
        v.resolve_link(&from, "../../../etc/passwd"),
        Resolution::Missing
    ));

    // Ambiguo: mismo nombre en .md y .html; con extensión se resuelve.
    fs::write(
        t.path().join("wiki/termodinamica.md"),
        "---\ntype: atomo\n---\n",
    )
    .unwrap();
    let v = orbital_vault::open(t.path()).unwrap();
    assert!(matches!(
        v.resolve_link(&from, "[[termodinamica]]"),
        Resolution::Ambiguous(ref c) if c.len() == 2
    ));
    assert!(matches!(
        v.resolve_link(&from, "[[termodinamica.html]]"),
        Resolution::Found(ref p) if p.as_str() == "wiki/termodinamica.html"
    ));

    // La cita de `sources` apunta a raw/.
    let note = v.read_note(&from).unwrap();
    let Some(PropValue::List(sources)) = note.prop("sources") else {
        panic!("sin sources")
    };
    let PropValue::Map(entry) = &sources[0] else {
        panic!()
    };
    let resource = entry
        .iter()
        .find(|(k, _)| k == "resource")
        .and_then(|(_, v)| v.as_text())
        .unwrap();
    assert!(matches!(
        v.resolve_link(&from, resource),
        Resolution::Found(ref p) if p.as_str() == "raw/fuente.txt"
    ));
}

#[test]
fn aux_files_are_validated_and_protected() {
    let (t, v) = vault();
    let c = ctx();
    fs::write(
        t.path().join("wiki/proyectos.base"),
        "format_version: 1\nfilters: [type == atomo]\n",
    )
    .unwrap();
    fs::write(
        t.path().join("wiki/mapa.canvas"),
        "{\"format_version\": 1, \"nodes\": [], \"edges\": []}\n",
    )
    .unwrap();
    fs::write(t.path().join("wiki/sin-version.base"), "filters: []\n").unwrap();
    fs::write(t.path().join("wiki/roto.canvas"), "{\"nodes\": [\n").unwrap();

    assert!(
        v.read_aux(&rel("wiki/proyectos.base"))
            .unwrap()
            .error
            .is_none()
    );
    let canvas = v.read_aux(&rel("wiki/mapa.canvas")).unwrap();
    assert_eq!(canvas.format_version, Some(1));
    let missing = v.read_aux(&rel("wiki/sin-version.base")).unwrap();
    assert!(
        missing
            .error
            .as_ref()
            .unwrap()
            .message
            .contains("format_version")
    );
    let broken = v.read_aux(&rel("wiki/roto.canvas")).unwrap();
    let err = broken.error.clone().unwrap();
    assert!(err.line >= 1);

    // Con error, no se sobrescribe salvo confirmación explícita.
    let before = fs::read(t.path().join("wiki/roto.canvas")).unwrap();
    assert!(matches!(
        v.save_aux(&broken, "{\"format_version\": 1}\n", false, &c),
        Err(VaultError::InvalidProps { .. })
    ));
    assert_eq!(fs::read(t.path().join("wiki/roto.canvas")).unwrap(), before);
    v.save_aux(&broken, "{\"format_version\": 1}\n", true, &c)
        .unwrap();
    assert!(
        v.read_aux(&rel("wiki/roto.canvas"))
            .unwrap()
            .error
            .is_none()
    );
}

#[test]
fn deleting_dot_orbital_loses_nothing() {
    let (t, v) = vault();
    let c = ctx();
    let note = v.read_note(&rel("wiki/fisica/entropia.md")).unwrap();
    v.set_prop(&note, "status", &PropValue::Text("draft".into()), &c)
        .unwrap();
    assert!(t.path().join(".orbital").exists());
    let before: Vec<_> = snapshot(t.path())
        .into_iter()
        .filter(|(p, _)| !p.starts_with(".orbital"))
        .collect();
    fs::remove_dir_all(t.path().join(".orbital")).unwrap();
    let reopened = orbital_vault::open(t.path()).unwrap();
    let after: Vec<_> = snapshot(t.path()).into_iter().collect();
    assert_eq!(before, after);
    assert_eq!(reopened.config(), &orbital_vault::VaultConfig::default());
}
