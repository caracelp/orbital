//! T059 y T062: diario, archivado y adjuntos.

mod common;

use std::fs;

use common::{TempDir, copy_tree, ctx, fixture};
use orbital_vault::props::PropValue;
use orbital_vault::{RelPath, Vault};

fn rel(s: &str) -> RelPath {
    RelPath::new(s).unwrap()
}

fn vault() -> (TempDir, Vault) {
    let t = TempDir::new("diario");
    copy_tree(&fixture("completa"), t.path());
    let v = orbital_vault::open(t.path()).unwrap();
    (t, v)
}

#[test]
fn journal_is_created_once() {
    let (t, v) = vault();
    let p = v.open_journal(&ctx()).unwrap();
    assert_eq!(p.as_str(), "wiki/2026-11-09.md");
    let note = v.read_note(&p).unwrap();
    assert_eq!(note.prop("type"), Some(&PropValue::Text("diario".into())));
    let first = fs::read(p.to_path(t.path())).unwrap();
    // La segunda vez no la toca.
    assert_eq!(v.open_journal(&ctx()).unwrap(), p);
    assert_eq!(fs::read(p.to_path(t.path())).unwrap(), first);
    assert!(
        fs::read_to_string(t.path().join("wiki/index.md"))
            .unwrap()
            .contains("# diario\n\n* [2026-11-09](/2026-11-09.md)")
    );
}

#[test]
fn journal_name_follows_config() {
    let (t, _) = vault();
    fs::create_dir_all(t.path().join(".orbital")).unwrap();
    fs::write(
        t.path().join(".orbital/config.yaml"),
        "journal_name: diario/AAAA/MM-DD\n",
    )
    .unwrap();
    let v = orbital_vault::open(t.path()).unwrap();
    let p = v.open_journal(&ctx()).unwrap();
    assert_eq!(p.as_str(), "wiki/diario/2026/11-09.md");
}

#[test]
fn archive_marks_without_moving_and_is_logged() {
    let (t, v) = vault();
    let p = rel("wiki/fisica/entropia.md");
    v.archive(&p, true, &ctx()).unwrap();
    assert!(p.to_path(t.path()).exists(), "no se mueve");
    assert_eq!(
        v.read_note(&p).unwrap().prop("archived"),
        Some(&PropValue::Bool(true))
    );
    assert!(v.is_archived(&p).unwrap());
    v.archive(&p, false, &ctx()).unwrap();
    assert_eq!(v.read_note(&p).unwrap().prop("archived"), None);
    let log = fs::read_to_string(t.path().join("wiki/log.md")).unwrap();
    assert!(log.contains("* **Archivado**: [Entropía](/fisica/entropia.md)"));
    assert!(
        log.contains("* **Actualización**: [Entropía](/fisica/entropia.md) vuelve del archivo")
    );
}

#[test]
fn attachments_go_next_to_the_note_and_are_linked() {
    let (t, v) = vault();
    let note = rel("wiki/fisica/entropia.md");
    let a = v.attach(&note, "diagrama.png", b"png1", &ctx()).unwrap();
    assert_eq!(a.as_str(), "wiki/fisica/diagrama.png");
    let b = v.attach(&note, "diagrama.png", b"png2", &ctx()).unwrap();
    assert_eq!(b.as_str(), "wiki/fisica/diagrama-1.png");
    assert_eq!(fs::read(b.to_path(t.path())).unwrap(), b"png2");
    let text = fs::read_to_string(note.to_path(t.path())).unwrap();
    assert!(text.ends_with("![diagrama.png](diagrama.png)\n\n![diagrama-1.png](diagrama-1.png)\n"));

    // En una nota HTML se enlaza con <img> antes de </body>.
    let html = rel("wiki/termodinamica.html");
    v.attach(&html, "foto de campo.jpg", b"jpg", &ctx())
        .unwrap();
    let text = fs::read_to_string(html.to_path(t.path())).unwrap();
    assert!(
        text.contains("<img src=\"foto%20de%20campo.jpg\" alt=\"foto de campo.jpg\">\n</body>")
    );

    // Nombres peligrosos se limpian.
    let c = v
        .attach(&note, "../../.oculto/x.png", b"x", &ctx())
        .unwrap();
    assert_eq!(c.as_str(), "wiki/fisica/x.png");
}
