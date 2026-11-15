//! Pruebas GTK en proceso de los ids de bloque al guardar (026, T012).

use std::fs;
use std::sync::Arc;

use crate::editor::Editor;
use gtk::glib;
use gtk::prelude::*;
use orbital_vault::{RelPath, Vault};

fn boveda(nombre: &str, texto: &str) -> (std::path::PathBuf, Arc<Vault>, RelPath) {
    let root = std::env::temp_dir().join(format!("orbital-026-{nombre}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join("wiki")).unwrap();
    fs::create_dir_all(root.join("raw")).unwrap();
    let rel = RelPath::new("wiki/nota.md").unwrap();
    fs::write(rel.to_path(&root), texto).unwrap();
    (
        root.clone(),
        Arc::new(orbital_vault::open(&root).unwrap()),
        rel,
    )
}

/// Cuántos ` ^xxxxxx` hay en un texto.
fn ids(t: &str) -> usize {
    t.lines()
        .filter(|l| {
            l.rsplit_once(" ^").is_some_and(|(_, id)| {
                id.len() == 6 && id.bytes().all(|b| b.is_ascii_alphanumeric())
            })
        })
        .count()
}

#[test]
#[ignore = "requiere una sesión gráfica; tools/pruebas-gtk.sh"]
fn ids_al_guardar_salvo_el_bloque_del_cursor() {
    adw::init().expect("Se necesita una sesión gráfica GTK");
    glib::MainContext::default().block_on(async {
        let (root, vault, rel) = boveda("cursor", "Uno.\n\nDos.\n");
        let editor = Editor::new();
        editor.load(vault.clone(), &vault.read_note(&rel).unwrap());

        // Escribir al final de «Dos.», con el cursor ahí.
        let b = editor.buffer().clone();
        let mut fin = b.iter_at_line_index(2, 4).unwrap();
        b.place_cursor(&fin);
        b.insert(&mut fin, " más");
        editor.save_now(true).await;

        let disco = fs::read_to_string(rel.to_path(&root)).unwrap();
        assert_eq!(disco, editor.text(), "el disco y el buffer quedan iguales");
        assert_eq!(ids(&disco), 1, "solo «Uno.» recibe id: {disco:?}");
        assert!(disco.contains("\nDos. más\n"), "{disco:?}");

        // Al dejar la nota, también el bloque del cursor.
        b.insert_at_cursor("!");
        editor.flush().await;
        let disco = fs::read_to_string(rel.to_path(&root)).unwrap();
        assert_eq!(disco, editor.text());
        assert_eq!(ids(&disco), 2, "{disco:?}");

        // Deshacer después del guardado nunca pierde lo escrito.
        while b.can_undo() {
            b.undo();
            assert!(editor.text().starts_with("Uno."), "{:?}", editor.text());
        }
        let _ = fs::remove_dir_all(&root);
    });
}

#[test]
#[ignore = "requiere una sesión gráfica; tools/pruebas-gtk.sh"]
fn abrir_y_dejar_sin_tocar_no_modifica() {
    adw::init().expect("Se necesita una sesión gráfica GTK");
    glib::MainContext::default().block_on(async {
        let original = "Uno.\n\n- ítem\n";
        let (root, vault, rel) = boveda("intacta", original);
        let editor = Editor::new();
        editor.load(vault.clone(), &vault.read_note(&rel).unwrap());
        editor.flush().await;
        assert_eq!(fs::read_to_string(rel.to_path(&root)).unwrap(), original);
        let _ = fs::remove_dir_all(&root);
    });
}
