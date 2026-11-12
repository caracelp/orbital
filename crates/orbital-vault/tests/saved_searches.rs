mod common;
use common::{TempDir, ctx};
use orbital_vault::{SavedSearch, VaultError};
use std::fs;

fn search(name: &str, query: &str) -> SavedSearch {
    SavedSearch {
        name: name.into(),
        query: query.into(),
    }
}

#[test]
fn missing_is_empty_without_writing_then_roundtrips_and_backs_up_exact_bytes() {
    let tmp = TempDir::new("saved-searches");
    fs::create_dir(tmp.path().join("wiki")).unwrap();
    fs::write(tmp.path().join("wiki/nota.md"), "Intacta #física").unwrap();
    let vault = orbital_vault::open(tmp.path()).unwrap();
    let empty = vault.saved_searches_snapshot();
    assert!(empty.searches.is_empty());
    assert!(empty.warning.is_none());
    assert!(!tmp.path().join(".orbital").exists());
    let all = [
        search("Física: pendientes", "tag:#física [estado:pendiente]"),
        search("Comillas \" y \\ y\nsalto", "\"frase exacta\" OR -otra"),
    ];
    let saved = vault.save_searches_checked(&all, &empty, &ctx()).unwrap();
    assert_eq!(vault.saved_searches(), all);
    let before = fs::read(tmp.path().join(".orbital/busquedas.yaml")).unwrap();
    let no_op = vault.save_searches_checked(&all, &saved, &ctx()).unwrap();
    assert_eq!(no_op.hash, saved.hash);
    assert!(!tmp.path().join(".orbital/respaldo").exists());
    vault
        .save_searches_checked(&all[..1], &saved, &ctx())
        .unwrap();
    let backups = tmp
        .path()
        .join(".orbital/respaldo")
        .join(ctx().date.to_string())
        .join(".orbital");
    assert!(
        fs::read_dir(backups)
            .unwrap()
            .filter_map(Result::ok)
            .any(|entry| fs::read(entry.path()).is_ok_and(|bytes| bytes == before))
    );
    assert_eq!(
        fs::read_to_string(tmp.path().join("wiki/nota.md")).unwrap(),
        "Intacta #física"
    );
}

#[test]
fn invalid_file_is_empty_with_warning_and_is_not_silently_overwritten() {
    let tmp = TempDir::new("saved-invalid");
    fs::create_dir(tmp.path().join(".orbital")).unwrap();
    let vault = orbital_vault::open(tmp.path()).unwrap();
    for text in [
        "mal: mapa",
        "- name: Sin consulta\n",
        "- name: x\n  query: [lista]\n",
        "[]\n---\n[]\n",
        "- &a {name: x, query: y}\n- *a\n",
    ] {
        fs::write(tmp.path().join(".orbital/busquedas.yaml"), text).unwrap();
        let snapshot = vault.saved_searches_snapshot();
        assert!(snapshot.searches.is_empty(), "{text}");
        assert!(snapshot.warning.is_some(), "{text}");
        assert!(
            vault
                .save_searches_checked(&[search("Nueva", "texto")], &snapshot, &ctx())
                .is_err()
        );
        assert_eq!(
            fs::read_to_string(tmp.path().join(".orbital/busquedas.yaml")).unwrap(),
            text
        );
    }
}

#[test]
fn snapshot_detects_external_changes_and_cannot_cross_vaults() {
    let tmp = TempDir::new("saved-conflict");
    let vault = orbital_vault::open(tmp.path()).unwrap();
    let baseline = vault.saved_searches_snapshot();
    vault.save_searches(&[search("A", "a")], &ctx()).unwrap();
    assert!(matches!(
        vault.save_searches_checked(&[search("B", "b")], &baseline, &ctx()),
        Err(VaultError::Conflict { .. })
    ));
    let baseline = vault.saved_searches_snapshot();
    fs::write(
        tmp.path().join(".orbital/busquedas.yaml"),
        "- name: Afuera\n  query: otro\n",
    )
    .unwrap();
    assert!(matches!(
        vault.save_searches_checked(&[], &baseline, &ctx()),
        Err(VaultError::Conflict { .. })
    ));
    assert_eq!(vault.saved_searches(), [search("Afuera", "otro")]);
    let before_delete = vault.saved_searches_snapshot();
    fs::remove_file(tmp.path().join(".orbital/busquedas.yaml")).unwrap();
    assert!(matches!(
        vault.save_searches_checked(&[], &before_delete, &ctx()),
        Err(VaultError::Conflict { .. })
    ));
    let missing = vault.saved_searches_snapshot();
    vault.save_searches_checked(&[], &missing, &ctx()).unwrap();
    assert!(
        orbital_vault::open(tmp.path())
            .unwrap()
            .saved_searches()
            .is_empty()
    );
    let other = TempDir::new("saved-other");
    let other_vault = orbital_vault::open(other.path()).unwrap();
    assert!(
        other_vault
            .save_searches_checked(&[], &baseline, &ctx())
            .is_err()
    );
    assert!(!other.path().join(".orbital").exists());
}

#[test]
fn oversized_output_and_duplicate_names_do_not_create_or_change_files() {
    let tmp = TempDir::new("saved-invalid-write");
    let vault = orbital_vault::open(tmp.path()).unwrap();
    let baseline = vault.saved_searches_snapshot();
    let huge = search(
        "Grande",
        &"a".repeat(orbital_vault::entry::MAX_EDITABLE_BYTES as usize),
    );
    assert!(matches!(
        vault.save_searches_checked(&[huge], &baseline, &ctx()),
        Err(VaultError::TooLarge(_))
    ));
    assert!(!tmp.path().join(".orbital").exists());
    assert!(
        vault
            .save_searches_checked(&[search("A", "a"), search("A", "b")], &baseline, &ctx())
            .is_err()
    );
    assert!(!tmp.path().join(".orbital").exists());
}

#[test]
fn symlinked_config_file_directory_or_backup_are_refused() {
    use std::os::unix::fs::symlink;
    let tmp = TempDir::new("saved-symlinks");
    let outside = TempDir::new("saved-outside");
    let vault = orbital_vault::open(tmp.path()).unwrap();
    let baseline = vault.saved_searches_snapshot();
    symlink(outside.path(), tmp.path().join(".orbital")).unwrap();
    assert!(vault.saved_searches_snapshot().warning.is_some());
    assert!(
        vault
            .save_searches_checked(&[search("A", "a")], &baseline, &ctx())
            .is_err()
    );
    assert!(fs::read_dir(outside.path()).unwrap().next().is_none());
    fs::remove_file(tmp.path().join(".orbital")).unwrap();
    fs::create_dir(tmp.path().join(".orbital")).unwrap();
    fs::write(outside.path().join("busquedas.yaml"), "[]\n").unwrap();
    symlink(
        outside.path().join("busquedas.yaml"),
        tmp.path().join(".orbital/busquedas.yaml"),
    )
    .unwrap();
    assert!(vault.saved_searches_snapshot().warning.is_some());
    assert!(vault.save_searches_checked(&[], &baseline, &ctx()).is_err());
    fs::remove_file(tmp.path().join(".orbital/busquedas.yaml")).unwrap();
    let current = vault
        .save_searches_checked(&[search("A", "a")], &baseline, &ctx())
        .unwrap();
    symlink(outside.path(), tmp.path().join(".orbital/respaldo")).unwrap();
    assert!(
        vault
            .save_searches_checked(&[search("B", "b")], &current, &ctx())
            .is_err()
    );
    assert_eq!(vault.saved_searches(), [search("A", "a")]);
}
