//! T021 / SC-002: «Abrir» no crea, mueve ni modifica nada.

mod common;

use common::{TempDir, copy_tree, fixture, snapshot};

#[test]
fn open_leaves_every_fixture_untouched() {
    for name in [
        "completa",
        "sin-estructura",
        "ocultos",
        "no-ascii",
        "conflicto",
    ] {
        // Se trabaja sobre una copia para que la comparación no dependa del checkout de git.
        let t = TempDir::new("open");
        copy_tree(&fixture(name), t.path());
        let had_orbital = t.path().join(".orbital").exists();
        let before = snapshot(t.path());

        let vault = orbital_vault::open(t.path()).unwrap();
        assert!(!vault.entries().is_empty(), "{name}: sin entradas");

        let after = snapshot(t.path());
        assert_eq!(before, after, "{name}: la carpeta cambió al abrirla");
        assert_eq!(
            t.path().join(".orbital").exists(),
            had_orbital,
            "{name}: apareció .orbital/"
        );
    }
}
