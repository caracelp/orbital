//! T022: exclusión de ocultos (FR-008), clasificación de entradas y detección de la estructura.

mod common;

use common::fixture;
use orbital_vault::{EntryKind, Presence, ReservedKind, Zone};

fn paths(name: &str) -> Vec<String> {
    orbital_vault::open(&fixture(name))
        .unwrap()
        .entries()
        .iter()
        .map(|e| e.path.to_string())
        .collect()
}

#[test]
fn hidden_entries_never_listed() {
    let p = paths("ocultos");
    assert_eq!(
        p,
        vec![
            "raw",
            "raw/f.txt",
            "wiki",
            "wiki/visible",
            "wiki/visible/nota.md"
        ]
    );
    assert!(p.iter().all(|x| !x.split('/').any(|c| c.starts_with('.'))));
}

#[test]
fn classifies_entries_and_zones() {
    let v = orbital_vault::open(&fixture("completa")).unwrap();
    let get = |p: &str| v.entries().iter().find(|e| e.path.as_str() == p).unwrap();
    assert_eq!(get("raw/fuente.txt").zone, Zone::Raw);
    assert!(get("raw/fuente.txt").read_only);
    assert!(get("raw").read_only);
    assert_eq!(get("wiki/fisica/entropia.md").kind, EntryKind::AtomNote);
    assert_eq!(get("wiki/fisica/entropia.md").zone, Zone::Wiki);
    assert!(!get("wiki/fisica/entropia.md").read_only);
    assert_eq!(
        get("wiki/termodinamica.html").kind,
        EntryKind::SynthesisNote
    );
    assert_eq!(
        get("wiki/index.md").kind,
        EntryKind::Reserved(ReservedKind::Index)
    );
    assert_eq!(
        get("wiki/log.md").kind,
        EntryKind::Reserved(ReservedKind::Log)
    );
    assert_eq!(get("AGENTS.md").kind, EntryKind::Agents);
    assert_eq!(get("AGENTS.md").zone, Zone::Root);

    let loose = orbital_vault::open(&fixture("sin-estructura")).unwrap();
    assert!(loose.entries().iter().all(|e| e.zone == Zone::Outside));
}

#[test]
fn non_ascii_paths() {
    let p = paths("no-ascii");
    assert!(p.contains(&"wiki/Bóveda/Música/canción.md".to_owned()));
    assert!(p.contains(&"wiki/ñandú.md".to_owned()));
}

#[test]
fn detects_layout() {
    let full = orbital_vault::open(&fixture("completa")).unwrap();
    assert!(full.layout().is_complete());
    assert_eq!(full.layout().agents, Presence::Present);

    let loose = orbital_vault::open(&fixture("sin-estructura")).unwrap();
    let l = loose.layout();
    assert!(!l.is_complete());
    assert_eq!(
        (l.raw, l.wiki, l.index, l.log),
        (
            Presence::Missing,
            Presence::Missing,
            Presence::Missing,
            Presence::Missing
        )
    );

    let conflict = orbital_vault::open(&fixture("conflicto")).unwrap();
    assert_eq!(conflict.layout().raw, Presence::Present);
    assert_eq!(conflict.layout().wiki, Presence::Conflict);
    assert!(!conflict.layout().is_complete());

    let hidden = orbital_vault::open(&fixture("ocultos")).unwrap();
    assert_eq!(hidden.layout().index, Presence::Missing);
}

#[test]
fn reads_config_without_creating_it() {
    let v = orbital_vault::open(&fixture("ocultos")).unwrap();
    assert_eq!(v.config().format_version, 1);
    assert!(v.warnings().is_empty());
}
