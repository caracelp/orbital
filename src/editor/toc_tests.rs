//! Pruebas GTK en proceso de lo que completó la 035: «Insertar índice…» (el bloque en el cursor,
//! en su párrafo y con un solo paso de deshacer) y los valores por defecto de Preferencias, que
//! se aplican en vivo a la Lectura y que el bloque puede pisar. Con `ORBITAL_CAPTURA_DIR`,
//! capturas.

use std::fs;
use std::sync::Arc;

use super::carrusel_tests::{esperar, todos, ventana};
use crate::editor::graficos_tests::capturar;
use crate::editor::{Editor, View};
use adw::prelude::*;
use gtk::glib;
use orbital_vault::RelPath;

const NOTA: &str = "# Alfa\n\nTexto.\n\n## Beta\n\n### Gamma\n\n## Delta\n";

/// Una bóveda temporal con un nombre no ASCII y la nota `wiki/índice ñ.md`.
fn boveda(nombre: &str, texto: &str) -> (std::path::PathBuf, Arc<orbital_vault::Vault>, RelPath) {
    let root = std::env::temp_dir().join(format!(
        "orbital-035-{nombre}-Bóveda ñandú-{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join("wiki")).unwrap();
    let rel = RelPath::new("wiki/índice ñ.md").unwrap();
    fs::write(rel.to_path(&root), texto).unwrap();
    (
        root.clone(),
        Arc::new(orbital_vault::open(&root).unwrap()),
        rel,
    )
}

fn texto_lectura(editor: &Editor) -> String {
    let b = editor.0.reader.text_view().buffer();
    b.text(&b.start_iter(), &b.end_iter(), true).to_string()
}

fn veces(texto: &str, que: &str) -> usize {
    texto.matches(que).count()
}

/// «Insertar índice…»: el diálogo arma el bloque en el cursor con lo que difiere de
/// Preferencias, un solo paso de deshacer, y la Lectura lo muestra.
#[test]
#[ignore = "requiere una sesión gráfica; tools/pruebas-gtk.sh"]
fn insertar_indice_con_el_dialogo() {
    adw::init().expect("Se necesita una sesión gráfica GTK");
    crate::style::apply();
    glib::MainContext::default().block_on(async {
        let (root, vault, rel) = boveda("insertar", NOTA);
        let editor = Editor::new();
        let window = ventana(&editor, 900, 800);
        editor.load(vault.clone(), &vault.read_note(&rel).unwrap());
        editor.set_view(View::Codigo);
        let b = &editor.0.buffer;
        let original = b.text(&b.start_iter(), &b.end_iter(), true).to_string();
        let en = original.find("## Beta").unwrap();
        b.place_cursor(&b.iter_at_offset(original[..en].chars().count() as i32));
        let d = editor.insertar_toc().expect("el diálogo");
        glib::timeout_future(std::time::Duration::from_millis(300)).await;
        capturar(&d, "035-insertar-indice");
        let combo = todos(&d)
            .into_iter()
            .find_map(|w| w.downcast::<adw::ComboRow>().ok())
            .expect("el estilo");
        combo.set_selected(1);
        let spins: Vec<adw::SpinRow> = todos(&d)
            .into_iter()
            .filter_map(|w| w.downcast::<adw::SpinRow>().ok())
            .collect();
        assert_eq!(spins.len(), 2);
        spins[1].set_value(2.0);
        todos(&d)
            .into_iter()
            .find_map(|w| w.downcast::<adw::EntryRow>().ok())
            .expect("el título")
            .set_text("En esta nota");
        todos(&d)
            .into_iter()
            .filter_map(|w| w.downcast::<gtk::Button>().ok())
            .find(|b| b.label().as_deref() == Some("Insertar"))
            .unwrap()
            .emit_clicked();
        let nuevo = b.text(&b.start_iter(), &b.end_iter(), true).to_string();
        assert_eq!(
            nuevo,
            "# Alfa\n\nTexto.\n\n```toc\ntitle: En esta nota\nstyle: lista-ordenada\nmaxLevel: 2\n```\n\n## Beta\n\n### Gamma\n\n## Delta\n"
        );
        // Un solo paso de deshacer.
        b.undo();
        assert_eq!(b.text(&b.start_iter(), &b.end_iter(), true), original);
        b.redo();
        editor.set_view(View::Lectura);
        editor.render_now();
        assert!(esperar(|| texto_lectura(&editor).contains("En esta nota")).await);
        let t = texto_lectura(&editor);
        // `Gamma` (nivel 3) queda fuera de la tabla: solo el encabezado.
        assert_eq!(veces(&t, "Gamma"), 1, "{t}");
        assert_eq!(veces(&t, "Delta"), 2, "{t}");
        capturar(&window, "035-indice-insertado");
        // El archivo no cambia hasta que se guarda lo que la usuaria escribió (virtual).
        assert_eq!(fs::read_to_string(rel.to_path(&root)).unwrap(), NOTA);
        window.destroy();
        let _ = fs::remove_dir_all(&root);
    });
}

/// Los valores por defecto de Preferencias se aplican en vivo a la Lectura; lo que escribe el
/// bloque manda. Con `ORBITAL_CAPTURA_DIR`, `035-preferencias.png`.
#[test]
#[ignore = "requiere una sesión gráfica; tools/pruebas-gtk.sh"]
fn preferencias_de_la_tabla_en_vivo() {
    adw::init().expect("Se necesita una sesión gráfica GTK");
    crate::style::apply();
    glib::MainContext::default().block_on(async {
        crate::editor::reader::set_toc_defaults(Default::default());
        let texto = format!("```toc\n```\n\n```toc\nmaxLevel: 0\ntitle: Completa\n```\n\n{NOTA}");
        let (root, vault, rel) = boveda("prefs", &texto);
        let editor = Editor::new();
        let window = ventana(&editor, 900, 900);
        editor.load(vault.clone(), &vault.read_note(&rel).unwrap());
        editor.set_view(View::Lectura);
        editor.render_now();
        assert!(esperar(|| texto_lectura(&editor).contains("Completa")).await);
        // Dos tablas y los encabezados: cada uno tres veces.
        assert!(esperar(|| veces(&texto_lectura(&editor), "Gamma") == 3).await);
        let e = editor.clone();
        let dialog = crate::preferences::show(
            &window,
            root.clone(),
            vault.config().clone(),
            false,
            move || e.render_now(),
            |_| {},
        );
        glib::timeout_future(std::time::Duration::from_millis(400)).await;
        let filas = todos(&dialog);
        let maximo = filas
            .iter()
            .find_map(|w| {
                w.clone()
                    .downcast::<adw::SpinRow>()
                    .ok()
                    .filter(|r| r.title() == "Nivel máximo")
            })
            .expect("la fila «Nivel máximo»");
        maximo.set_value(2.0);
        assert_eq!(crate::editor::reader::toc_defaults().max_level, 2);
        // La primera tabla (sin opciones) pierde `Gamma`; la que escribe `maxLevel: 0`, no.
        assert!(
            esperar(|| veces(&texto_lectura(&editor), "Gamma") == 2).await,
            "{}",
            texto_lectura(&editor)
        );
        let titulo = filas
            .iter()
            .find_map(|w| {
                w.clone()
                    .downcast::<adw::EntryRow>()
                    .ok()
                    .filter(|r| r.title().starts_with("Título"))
            })
            .expect("la fila del título");
        titulo.set_text("Contenido");
        assert!(esperar(|| texto_lectura(&editor).contains("Contenido")).await);
        assert_eq!(crate::editor::reader::toc_defaults().title, "Contenido");
        capturar(&dialog, "035-preferencias");
        dialog.force_close();
        glib::timeout_future(std::time::Duration::from_millis(600)).await;
        capturar(&window, "035-lectura-con-preferencias");
        crate::editor::reader::set_toc_defaults(Default::default());
        assert_eq!(fs::read_to_string(rel.to_path(&root)).unwrap(), texto);
        window.destroy();
        let _ = fs::remove_dir_all(&root);
    });
}
