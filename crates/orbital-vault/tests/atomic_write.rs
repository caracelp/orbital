//! T011 y T018: escritura atómica, GuardadoSeguro y `safe_write`.

mod common;

use std::fs;
use std::os::unix::fs::PermissionsExt;

use common::{TempDir, ctx};
use orbital_vault::write::{atomic_write, hash, safe_write};
use orbital_vault::{RelPath, VaultError};

#[test]
fn writes_complete_file_without_leftovers() {
    let t = TempDir::new("atomic");
    let p = t.path().join("nota.md");
    atomic_write(&p, b"hola").unwrap();
    assert_eq!(fs::read(&p).unwrap(), b"hola");
    atomic_write(&p, b"chau").unwrap();
    assert_eq!(fs::read(&p).unwrap(), b"chau");
    let names: Vec<_> = fs::read_dir(t.path())
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    assert_eq!(names, vec![std::ffi::OsString::from("nota.md")]);
}

#[test]
fn keeps_permissions() {
    let t = TempDir::new("perm");
    let p = t.path().join("nota.md");
    fs::write(&p, "a").unwrap();
    fs::set_permissions(&p, fs::Permissions::from_mode(0o600)).unwrap();
    atomic_write(&p, b"b").unwrap();
    assert_eq!(
        fs::metadata(&p).unwrap().permissions().mode() & 0o777,
        0o600
    );
}

#[test]
fn reports_orphan_temp_without_deleting_it() {
    let t = TempDir::new("orphan");
    let p = t.path().join("nota.md");
    let orphan = t.path().join(".nota.md.orbital-tmp");
    fs::write(&orphan, "a medias").unwrap();
    let outcome = atomic_write(&p, b"completo").unwrap();
    assert_eq!(outcome.orphan.as_deref(), Some(orphan.as_path()));
    assert!(orphan.exists(), "el huérfano no se borra");
    assert_eq!(fs::read(&p).unwrap(), b"completo");
}

#[test]
fn safe_write_backs_up_and_detects_conflicts() {
    let t = TempDir::new("safe");
    let root = t.path();
    let rel = RelPath::new("wiki/nota.md").unwrap();
    let c = ctx();

    // Archivo nuevo: no crea .orbital/ porque no hay nada que respaldar.
    let r = safe_write(root, &rel, b"uno", None, &c).unwrap();
    assert_eq!(r.written, vec![(rel.clone(), hash(b"uno"))]);
    assert!(!root.join(".orbital").exists());

    // Crear de nuevo algo que ya existe es un conflicto.
    assert!(matches!(
        safe_write(root, &rel, b"x", None, &c),
        Err(VaultError::Conflict { .. })
    ));

    // Mismo contenido: no escribe nada.
    let r = safe_write(root, &rel, b"uno", Some(hash(b"uno")), &c).unwrap();
    assert!(r.written.is_empty());

    // Cambio: respalda el anterior en .orbital/respaldo/AAAA-MM-DD/<ruta>.<HHMMSS>.<ext>.
    safe_write(root, &rel, b"dos", Some(hash(b"uno")), &c).unwrap();
    assert_eq!(fs::read(rel.to_path(root)).unwrap(), b"dos");
    let copy = root.join(".orbital/respaldo/2026-11-09/wiki/nota.183005.md");
    assert_eq!(fs::read(&copy).unwrap(), b"uno");
    let meta =
        fs::read_to_string(root.join(".orbital/respaldo/2026-11-09/wiki/nota.183005.md.yaml"))
            .unwrap();
    assert!(meta.contains("original_path: wiki/nota.md"));
    assert!(meta.contains("saved_at: 2026-11-09T18:30:05-03:00"));

    // Otro programa lo cambió: conflicto y no se pisa.
    fs::write(rel.to_path(root), "externo").unwrap();
    assert!(matches!(
        safe_write(root, &rel, b"tres", Some(hash(b"dos")), &c),
        Err(VaultError::Conflict { .. })
    ));
    assert_eq!(fs::read(rel.to_path(root)).unwrap(), b"externo");

    // Segundo respaldo en el mismo segundo: no pisa el primero.
    safe_write(root, &rel, b"cuatro", Some(hash(b"externo")), &c).unwrap();
    assert!(
        root.join(".orbital/respaldo/2026-11-09/wiki/nota.183005-1.md")
            .exists()
    );
}

#[test]
fn safe_write_refuses_raw_and_hidden() {
    let t = TempDir::new("ro");
    let c = ctx();
    let raw = RelPath::new("raw/fuente.pdf").unwrap();
    assert!(matches!(
        safe_write(t.path(), &raw, b"x", None, &c),
        Err(VaultError::ReadOnly(_))
    ));
    let hidden = RelPath::new("wiki/.oculta/n.md").unwrap();
    assert!(matches!(
        safe_write(t.path(), &hidden, b"x", None, &c),
        Err(VaultError::Hidden(_))
    ));
}
