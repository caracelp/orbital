//! Pruebas de integración para US5 (conflictos y comparación).

use std::fs;
use std::sync::Arc;
use std::time::Duration;

use crate::editor::Editor;
use gtk::glib;
use orbital_vault::diff::{DiffKind, diff_lines};
use orbital_vault::write::hash;
use orbital_vault::{RelPath, Vault};

fn setup_temp_vault() -> (std::path::PathBuf, Arc<Vault>, RelPath) {
    let root = std::env::temp_dir().join(format!("orbital-us5-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join("wiki")).expect("crear directorio temporal");

    let rel = RelPath::new("wiki/nota.md").unwrap();
    let initial = "Línea 1\nLínea 2 original\nLínea 3\n";
    fs::write(rel.to_path(&root), initial).unwrap();

    let vault = Arc::new(orbital_vault::open(&root).unwrap());
    (root, vault, rel)
}

fn count_backups(root: &std::path::Path) -> usize {
    fn walk(p: &std::path::Path) -> usize {
        fs::read_dir(p).map_or(0, |d| {
            d.map(|e| {
                let p = e.unwrap().path();
                if p.is_dir() {
                    walk(&p)
                } else {
                    usize::from(!p.to_string_lossy().ends_with(".yaml"))
                }
            })
            .sum()
        })
    }
    walk(&root.join(".orbital/respaldo"))
}

#[test]
fn diff_tags_and_markers() {
    let left = "uno\ndos\ntres\n";
    let right = "uno\ndos modificado\ntres\ncuatro\n";
    let lines = diff_lines(left, right);

    let same_count = lines.iter().filter(|l| l.kind == DiffKind::Same).count();
    let removed_count = lines.iter().filter(|l| l.kind == DiffKind::Removed).count();
    let added_count = lines.iter().filter(|l| l.kind == DiffKind::Added).count();

    assert_eq!(same_count, 2); // "uno", "tres"
    assert_eq!(removed_count, 1); // "dos"
    assert_eq!(added_count, 2); // "dos modificado", "cuatro"
}

#[test]
#[ignore = "requiere una sesión gráfica; ejecutar tools/verificar-us5.sh"]
fn us5_conflict_scenarios() {
    glib::set_prgname(Some("orbital"));
    glib::set_application_name("Orbital");
    adw::init().expect("Se necesita acceso a una sesión gráfica GTK");
    let context = glib::MainContext::default();

    context.block_on(async {
        let (root, vault, rel) = setup_temp_vault();

        // Escenario 5.5: Cambio externo sin cambios locales -> recarga sola
        {
            let note = vault.read_note(&rel).unwrap();
            let editor = Editor::new();
            editor.load(vault.clone(), &note);
            assert_eq!(editor.text(), "Línea 1\nLínea 2 original\nLínea 3\n");

            // Cambiamos el archivo en disco
            let nuevo_disco = "Línea 1\nLínea 2 cambiada afuera\nLínea 3\n";
            fs::write(rel.to_path(&root), nuevo_disco).unwrap();
            let nuevo_hash = hash(nuevo_disco.as_bytes());

            // external_change devuelve Reload
            let action = editor.external_change(nuevo_hash);
            assert_eq!(action, orbital_vault::session::ExternalAction::Reload);

            // Recargamos la nota
            let note_updated = vault.read_note(&rel).unwrap();
            editor.load(vault.clone(), &note_updated);
            assert_eq!(editor.text(), nuevo_disco);
        }

        // Escenario 5.1 y 5.2: Cambios locales sin guardar + cambio externo -> Conflict -> Conservar la mía
        {
            let (root, vault, rel) = setup_temp_vault();
            let note = vault.read_note(&rel).unwrap();
            let editor = Editor::new();
            editor.load(vault.clone(), &note);

            // Escribimos en el editor (cambio local)
            let mi_version = "Línea 1\nLínea 2 con mi cambio local\nLínea 3\n";
            editor.replace_text(mi_version);
            assert!(editor.has_pending());

            // Cambiamos el archivo en disco afuera
            let disco_ajeno = "Línea 1\nLínea 2 desde otro editor\nLínea 3\n";
            fs::write(rel.to_path(&root), disco_ajeno).unwrap();
            let disco_hash = hash(disco_ajeno.as_bytes());

            // external_change detecta conflicto
            let action = editor.external_change(disco_hash);
            assert_eq!(action, orbital_vault::session::ExternalAction::AskUser);

            // Autoguardado está pausado; esperar no debe sobreescribir el disco
            glib::timeout_future(Duration::from_millis(500)).await;
            assert_eq!(fs::read_to_string(rel.to_path(&root)).unwrap(), disco_ajeno);

            // Elegimos «Conservar la mía»
            let respaldos_antes = count_backups(&root);
            editor.keep_mine(vault.clone()).await.unwrap();

            // El archivo en disco ahora tiene mi versión
            assert_eq!(fs::read_to_string(rel.to_path(&root)).unwrap(), mi_version);
            // SC-005: la versión del disco quedó respaldada
            assert_eq!(count_backups(&root), respaldos_antes + 1);
        }

        // Escenario 5.3: «Usar la del disco» -> Carga versión del disco y respalda la mía
        {
            let (root, vault, rel) = setup_temp_vault();
            let note = vault.read_note(&rel).unwrap();
            let editor = Editor::new();
            editor.load(vault.clone(), &note);

            let mi_version = "Línea 1\nMi versión que voy a descartar\nLínea 3\n";
            editor.replace_text(mi_version);

            let disco_nuevo = "Línea 1\nVersión del disco que quiero adoptar\nLínea 3\n";
            fs::write(rel.to_path(&root), disco_nuevo).unwrap();
            let disco_hash = hash(disco_nuevo.as_bytes());

            let action = editor.external_change(disco_hash);
            assert_eq!(action, orbital_vault::session::ExternalAction::AskUser);

            let respaldos_antes = count_backups(&root);
            editor.use_disk(vault.clone()).await.unwrap();

            // El editor ahora tiene la versión del disco
            assert_eq!(editor.text(), disco_nuevo);
            // SC-005: mi versión descartada quedó en respaldo
            assert_eq!(count_backups(&root), respaldos_antes + 1);
        }

        // Caso borde: Nota borrada afuera con cambios locales -> Descartar a respaldo
        {
            let (root, vault, rel) = setup_temp_vault();
            let note = vault.read_note(&rel).unwrap();
            let editor = Editor::new();
            editor.load(vault.clone(), &note);

            editor.replace_text("Cambios que no quiero perder aunque se borre el archivo\n");
            fs::remove_file(rel.to_path(&root)).unwrap();

            let respaldos_antes = count_backups(&root);
            editor.discard_to_backup(vault.clone()).await.unwrap();

            // SC-005: los cambios quedaron respaldados
            assert_eq!(count_backups(&root), respaldos_antes + 1);
        }

        let _ = fs::remove_dir_all(&root);
    });
}
