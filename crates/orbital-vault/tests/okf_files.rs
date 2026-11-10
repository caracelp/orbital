//! T051 y T056: `index.md` y `log.md` al día (FR-020 a FR-023, SC-006) y renombre inverso.

mod common;

use std::fs;

use common::{TempDir, copy_tree, ctx, fixture};
use orbital_vault::okf_files::{IndexEntry, Verb, index_remove, index_upsert, log_add};
use orbital_vault::{Format, RelPath, Vault, VaultError};

fn rel(s: &str) -> RelPath {
    RelPath::new(s).unwrap()
}

fn entry(title: &str, link: &str, desc: &str, section: &str) -> IndexEntry {
    IndexEntry {
        title: title.into(),
        link: link.into(),
        description: desc.into(),
        section: section.into(),
    }
}

#[test]
fn index_upsert_keeps_handwritten_text() {
    let text = "Texto a mano arriba.\n\n# atomo\n\n* [A](/a.md) - Primera.\nNota suelta escrita a mano.\n\n# sintesis\n\n* [S](/s.html) - Síntesis.\n";
    // Alta en una sección existente: al final de la sección, antes de la siguiente.
    let out = index_upsert(text, &entry("B", "/b.md", "Segunda.", "atomo"));
    assert_eq!(
        out,
        "Texto a mano arriba.\n\n# atomo\n\n* [A](/a.md) - Primera.\nNota suelta escrita a mano.\n* [B](/b.md) - Segunda.\n\n# sintesis\n\n* [S](/s.html) - Síntesis.\n"
    );
    // Actualizar en el lugar.
    let out2 = index_upsert(&out, &entry("A2", "/a.md", "Cambiada.", "atomo"));
    assert!(out2.contains("* [A2](/a.md) - Cambiada.\nNota suelta"));
    // Sección nueva: al final.
    let out3 = index_upsert(text, &entry("D", "/d.md", "", "diario"));
    assert!(out3.ends_with("* [S](/s.html) - Síntesis.\n\n# diario\n\n* [D](/d.md)\n"));
    // Cambio de sección: se mueve.
    let out4 = index_upsert(text, &entry("A", "/a.md", "Primera.", "sintesis"));
    assert!(!out4.contains("# atomo\n\n* [A]"));
    assert!(out4.ends_with("* [S](/s.html) - Síntesis.\n* [A](/a.md) - Primera.\n"));
    // Vaciar una sección y volver a llenarla no acumula líneas en blanco.
    let emptied = index_remove("# atomo\n\n* [A](/a.md)\n\n# sintesis\n", "/a.md");
    assert_eq!(
        index_upsert(&emptied, &entry("A", "/a.md", "", "atomo")),
        "# atomo\n\n* [A](/a.md)\n\n# sintesis\n"
    );
    // Baja.
    let out5 = index_remove(text, "/a.md");
    assert_eq!(
        out5,
        "Texto a mano arriba.\n\n# atomo\n\nNota suelta escrita a mano.\n\n# sintesis\n\n* [S](/s.html) - Síntesis.\n"
    );
    // Índice vacío.
    assert_eq!(
        index_upsert("", &entry("A", "/a b.md", "x", "atomo")),
        "# atomo\n\n* [A](</a b.md>) - x\n"
    );
}

#[test]
fn log_groups_by_day_newest_first() {
    let text = "# Registro de la wiki\n\n## 2026-11-09\n* **Creación**: Estructura de la bóveda\n";
    let a = log_add(text, "2026-11-10", Verb::Creation, "[A](/a.md)");
    let b = log_add(&a, "2026-11-10", Verb::Rename, "[B](/b.md) (antes `a.md`)");
    assert_eq!(
        b,
        "# Registro de la wiki\n\n## 2026-11-10\n* **Creación**: [A](/a.md)\n* **Renombre**: [B](/b.md) (antes `a.md`)\n\n## 2026-11-09\n* **Creación**: Estructura de la bóveda\n"
    );
    assert_eq!(
        log_add("# Registro de la wiki\n", "2026-11-10", Verb::Deletion, "x"),
        "# Registro de la wiki\n\n## 2026-11-10\n* **Borrado**: x\n"
    );
    // CRLF se respeta.
    assert_eq!(
        log_add("# R\r\n", "2026-11-10", Verb::Archive, "x"),
        "# R\r\n\r\n## 2026-11-10\r\n* **Archivado**: x\r\n"
    );
}

fn vault_with_okf() -> (TempDir, Vault) {
    let t = TempDir::new("okf");
    copy_tree(&fixture("completa"), t.path());
    let v = orbital_vault::open(t.path()).unwrap();
    (t, v)
}

fn read(t: &TempDir, p: &str) -> String {
    fs::read_to_string(t.path().join(p)).unwrap()
}

#[test]
fn operations_keep_index_and_log_in_sync() {
    let (t, v) = vault_with_okf();
    let c = ctx();

    let n = v
        .create_note(&rel("wiki"), "Calor", Format::Md, &c)
        .unwrap();
    assert!(read(&t, "wiki/index.md").contains("* [Calor](/Calor.md)\n"));
    assert!(read(&t, "wiki/log.md").contains("## 2026-11-09\n* **Creación**: Estructura de la bóveda\n* **Creación**: [Calor](/Calor.md)\n"));

    // set_prop de description actualiza el índice, sin agregar al registro.
    let log_before = read(&t, "wiki/log.md");
    let note = v.read_note(&n).unwrap();
    v.set_prop(
        &note,
        "description",
        &orbital_vault::props::PropValue::Text("Energía en tránsito.".into()),
        &c,
    )
    .unwrap();
    assert!(read(&t, "wiki/index.md").contains("* [Calor](/Calor.md) - Energía en tránsito.\n"));
    assert_eq!(read(&t, "wiki/log.md"), log_before);

    // Mover a una subcarpeta.
    let moved = rel("wiki/fisica/calor.md");
    v.rename(&n, &moved, &c).unwrap();
    let idx = read(&t, "wiki/index.md");
    assert!(idx.contains("* [Calor](/fisica/calor.md) - Energía en tránsito.\n"));
    assert!(!idx.contains("(/Calor.md)"));
    assert!(
        read(&t, "wiki/log.md")
            .contains("* **Movimiento**: [Calor](/fisica/calor.md) (antes `Calor.md`)\n")
    );

    // Archivar quita del índice; desarchivar lo repone.
    v.archive(&moved, true, &c).unwrap();
    assert!(!read(&t, "wiki/index.md").contains("calor.md"));
    assert!(read(&t, "wiki/fisica/calor.md").contains("archived: true"));
    assert!(read(&t, "wiki/log.md").contains("* **Archivado**: [Calor](/fisica/calor.md)\n"));
    v.archive(&moved, false, &c).unwrap();
    assert!(read(&t, "wiki/index.md").contains("(/fisica/calor.md)"));
    assert!(!read(&t, "wiki/fisica/calor.md").contains("archived"));

    // Borrar: el núcleo no borra; la app manda a la papelera y después avisa.
    let plan = v.delete(&moved).unwrap();
    assert!(t.path().join("wiki/fisica/calor.md").exists());
    fs::remove_file(t.path().join("wiki/fisica/calor.md")).unwrap();
    v.after_delete(&plan, &c).unwrap();
    assert!(!read(&t, "wiki/index.md").contains("calor.md"));
    assert!(read(&t, "wiki/log.md").contains("* **Borrado**: [Calor](/fisica/calor.md)\n"));

    // Lo escrito a mano en el índice sigue igual.
    assert!(
        read(&t, "wiki/index.md")
            .contains("* [Entropía](/fisica/entropia.md) - Medida del desorden.\n")
    );
}

#[test]
fn rename_undo_restores_everything() {
    let (t, v) = vault_with_okf();
    let c = ctx();
    let from = rel("wiki/fisica/entropia.md");
    let to = rel("wiki/entropia-s.md");
    v.rename(&from, &to, &c).unwrap();
    assert!(read(&t, "wiki/index.md").contains("(/entropia-s.md)"));
    let after_first = read(&t, "wiki/index.md");
    // Deshacer = renombre inverso.
    v.rename(&to, &from, &c).unwrap();
    // El índice vuelve a ser idéntico al original: misma posición, sin líneas en blanco de más.
    assert_eq!(
        read(&t, "wiki/index.md"),
        fs::read_to_string(fixture("completa").join("wiki/index.md")).unwrap()
    );
    assert_eq!(after_first.matches("\n\n\n").count(), 0);
    let idx = read(&t, "wiki/index.md");
    assert!(idx.contains("* [Entropía](/fisica/entropia.md) - Medida del desorden.\n"));
    assert!(!idx.contains("entropia-s"));
    let log = read(&t, "wiki/log.md");
    assert!(log.contains("**Movimiento**: [Entropía](/entropia-s.md)"));
    assert!(
        log.contains("**Movimiento**: [Entropía](/fisica/entropia.md) (antes `entropia-s.md`)")
    );
}

#[test]
fn rename_rules() {
    let (t, v) = vault_with_okf();
    let c = ctx();
    let from = rel("wiki/fisica/entropia.md");
    assert!(matches!(
        v.rename(&from, &rel("wiki/index.md"), &c),
        Err(VaultError::ReservedName(_))
    ));
    assert!(matches!(
        v.rename(&from, &rel("raw/x.md"), &c),
        Err(VaultError::ReadOnly(_))
    ));
    assert!(matches!(
        v.rename(&rel("raw/fuente.txt"), &rel("wiki/f.txt"), &c),
        Err(VaultError::ReadOnly(_))
    ));
    assert!(matches!(
        v.rename(&from, &rel("wiki/termodinamica.html"), &c),
        Err(VaultError::Conflict { .. })
    ));
    assert!(t.path().join("wiki/fisica/entropia.md").exists());
}

#[test]
fn no_okf_files_means_none_are_created() {
    let t = TempDir::new("okf-none");
    copy_tree(&fixture("completa"), t.path());
    fs::remove_file(t.path().join("wiki/index.md")).unwrap();
    fs::remove_file(t.path().join("wiki/log.md")).unwrap();
    let v = orbital_vault::open(t.path()).unwrap();
    let n = v
        .create_note(&rel("wiki"), "Suelta", Format::Md, &ctx())
        .unwrap();
    v.rename(&n, &rel("wiki/otra.md"), &ctx()).unwrap();
    assert!(!t.path().join("wiki/index.md").exists());
    assert!(!t.path().join("wiki/log.md").exists());
}

#[test]
fn external_edit_of_index_is_preserved() {
    let (t, v) = vault_with_okf();
    // Un agente agrega algo al índice después de abrir la bóveda.
    let mut idx = read(&t, "wiki/index.md");
    idx.push_str("\nAgregado por un agente.\n");
    fs::write(t.path().join("wiki/index.md"), &idx).unwrap();
    v.create_note(&rel("wiki"), "Nueva", Format::Md, &ctx())
        .unwrap();
    let after = read(&t, "wiki/index.md");
    assert!(after.contains("Agregado por un agente."));
    assert!(after.contains("(/Nueva.md)"));
}

#[test]
fn vault_trash_fallback_is_undoable() {
    let (t, v) = vault_with_okf();
    let c = ctx();
    let p = rel("wiki/termodinamica.html");
    let before = fs::read(p.to_path(t.path())).unwrap();
    let plan = v.delete(&p).unwrap();
    let trashed = v.move_to_vault_trash(&p, &c).unwrap();
    v.after_delete(&plan, &c).unwrap();
    assert!(!p.to_path(t.path()).exists());
    assert!(trashed.starts_with(t.path().join(".orbital/papelera")));
    assert!(!read(&t, "wiki/index.md").contains("termodinamica"));
    // La papelera de la bóveda está oculta: no aparece al abrirla.
    let reopened = orbital_vault::open(t.path()).unwrap();
    assert!(
        reopened
            .entries()
            .iter()
            .all(|e| !e.path.as_str().contains("termodinamica"))
    );
    // Deshacer.
    v.restore_from(&trashed, &p).unwrap();
    v.after_restore(&plan, &c).unwrap();
    assert_eq!(fs::read(p.to_path(t.path())).unwrap(), before);
    // La línea vuelve tal cual estaba, con la descripción escrita a mano.
    assert_eq!(
        read(&t, "wiki/index.md"),
        fs::read_to_string(fixture("completa").join("wiki/index.md")).unwrap()
    );
    assert!(read(&t, "wiki/log.md").contains("vuelve de la papelera"));
}
