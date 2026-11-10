//! T033: «Abrir como bóveda de Orbital» crea solo lo que falta (FR-003, FR-007).

mod common;

use std::fs;

use common::{TempDir, copy_tree, ctx, fixture, snapshot};
use orbital_vault::vault::{CreationKind, apply_open_as, plan_open_as};
use orbital_vault::{EntryKind, Presence};

fn planned(root: &std::path::Path) -> Vec<String> {
    plan_open_as(root)
        .unwrap()
        .creations
        .iter()
        .map(|c| c.path.to_string())
        .collect()
}

#[test]
fn empty_folder_gets_full_structure() {
    let t = TempDir::new("openas-empty");
    assert_eq!(
        planned(t.path()),
        vec!["raw", "wiki", "wiki/index.md", "wiki/log.md"]
    );
    // Planificar no escribe nada.
    assert!(fs::read_dir(t.path()).unwrap().next().is_none());

    let plan = plan_open_as(t.path()).unwrap();
    assert_eq!(plan.creations[0].kind, CreationKind::Folder);
    assert_eq!(plan.creations[2].kind, CreationKind::File);
    let vault = apply_open_as(t.path(), &plan, &ctx()).unwrap();
    assert!(vault.layout().is_complete());
    assert_eq!(
        fs::read_to_string(t.path().join("wiki/index.md")).unwrap(),
        ""
    );
    assert_eq!(
        fs::read_to_string(t.path().join("wiki/log.md")).unwrap(),
        "# Registro de la wiki\n\n## 2026-11-09\n* **Creación**: Estructura de la bóveda\n"
    );
    assert!(!t.path().join("AGENTS.md").exists(), "nunca crea AGENTS.md");
    assert!(
        !t.path().join(".orbital").exists(),
        "no hace falta .orbital/"
    );
}

#[test]
fn only_missing_parts_are_created() {
    let t = TempDir::new("openas-partial");
    copy_tree(&fixture("completa"), t.path());
    fs::remove_dir_all(t.path().join("raw")).unwrap();
    let before = snapshot(t.path());

    assert_eq!(planned(t.path()), vec!["raw"]);
    let plan = plan_open_as(t.path()).unwrap();
    let vault = apply_open_as(t.path(), &plan, &ctx()).unwrap();
    assert!(vault.layout().is_complete());

    let after = snapshot(t.path());
    for (path, state) in &before {
        assert_eq!(after.get(path), Some(state), "{path:?} cambió");
    }
    assert_eq!(after.len(), before.len() + 1, "solo se agregó raw/");

    // AGENTS.md existente se lista como nota editable.
    let agents = vault
        .entries()
        .iter()
        .find(|e| e.path.as_str() == "AGENTS.md")
        .unwrap();
    assert_eq!(agents.kind, EntryKind::Agents);
    assert!(!agents.read_only);
}

#[test]
fn conflicts_are_reported_and_never_touched() {
    let t = TempDir::new("openas-conflict");
    copy_tree(&fixture("conflicto"), t.path());
    let plan = plan_open_as(t.path()).unwrap();
    assert!(plan.creations.is_empty());
    assert_eq!(
        plan.conflicts
            .iter()
            .map(|p| p.to_string())
            .collect::<Vec<_>>(),
        vec!["wiki"]
    );
    let vault = apply_open_as(t.path(), &plan, &ctx()).unwrap();
    assert_eq!(vault.layout().wiki, Presence::Conflict);
    assert_eq!(
        fs::read_to_string(t.path().join("wiki")).unwrap(),
        "soy un archivo\n"
    );
}

#[test]
fn existing_log_is_not_modified() {
    let t = TempDir::new("openas-log");
    copy_tree(&fixture("completa"), t.path());
    fs::remove_file(t.path().join("wiki/index.md")).unwrap();
    let log_before = fs::read(t.path().join("wiki/log.md")).unwrap();
    let plan = plan_open_as(t.path()).unwrap();
    apply_open_as(t.path(), &plan, &ctx()).unwrap();
    assert_eq!(fs::read(t.path().join("wiki/log.md")).unwrap(), log_before);
    assert!(t.path().join("wiki/index.md").exists());
}

#[test]
fn apply_skips_what_appeared_after_planning() {
    let t = TempDir::new("openas-race");
    let plan = plan_open_as(t.path()).unwrap();
    fs::create_dir(t.path().join("wiki")).unwrap();
    fs::write(t.path().join("wiki/index.md"), "# atomo\n").unwrap();
    apply_open_as(t.path(), &plan, &ctx()).unwrap();
    assert_eq!(
        fs::read_to_string(t.path().join("wiki/index.md")).unwrap(),
        "# atomo\n"
    );
}
