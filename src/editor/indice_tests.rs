//! Pruebas GTK en proceso del índice por etiqueta (036): la Lectura lista la rama anidada (con
//! prioridad, títulos y «Nota nueva»), se actualiza cuando cambia el índice en memoria sin
//! escribir nada en el disco, «Insertar índice de etiqueta…» arma el bloque con un solo paso de
//! deshacer, «Nota nueva con esta etiqueta» encuentra la etiqueta del bloque y el globito de
//! Código resume el índice. Con `ORBITAL_CAPTURA_DIR`, capturas.

use std::fs;
use std::sync::{Arc, RwLock};
use std::time::Duration;

use super::carrusel_tests::{esperar, todos, ventana};
use crate::editor::graficos_tests::capturar;
use crate::editor::{Editor, View};
use adw::prelude::*;
use gtk::glib;
use orbital_vault::{RelPath, SearchIndex};

const NOTA: &str = "# Proyectos\n\n```indice\netiqueta: proyectos\n```\n\nFin.\n";

/// Una bóveda temporal con un nombre no ASCII, notas etiquetadas y la nota del índice
/// `wiki/índices ñ/proyectos.md`.
fn boveda(nombre: &str) -> (std::path::PathBuf, Arc<orbital_vault::Vault>, RelPath) {
    let root = std::env::temp_dir().join(format!(
        "orbital-036-{nombre}-Bóveda ñandú-{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&root);
    let notas = [
        ("wiki/zeta.md", "---\ntags: [proyectos]\n---\nTexto.\n"),
        ("wiki/alfa.md", "Sin frontmatter #proyectos/tesis\n"),
        (
            "wiki/b12.md",
            "---\ntitle: Borrador de la charla\ntags: [proyectos/papers, top]\n---\n",
        ),
        ("wiki/c.md", "#proyectos/papers y #proyectos/tesis\n"),
        ("wiki/índices ñ/proyectos.md", NOTA),
    ];
    for (ruta, texto) in notas {
        let p = root.join(ruta);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, texto).unwrap();
    }
    (
        root.clone(),
        Arc::new(orbital_vault::open(&root).unwrap()),
        RelPath::new("wiki/índices ñ/proyectos.md").unwrap(),
    )
}

fn texto_lectura(editor: &Editor) -> String {
    let b = editor.0.reader.text_view().buffer();
    b.text(&b.start_iter(), &b.end_iter(), true).to_string()
}

/// El índice en Lectura, en vivo: etiquetar una nota (actualizar el índice en memoria) la hace
/// entrar; el archivo no cambia.
#[test]
#[ignore = "requiere una sesión gráfica; tools/pruebas-gtk.sh"]
fn indice_en_lectura_y_en_vivo() {
    adw::init().expect("Se necesita una sesión gráfica GTK");
    crate::style::apply();
    glib::MainContext::default().block_on(async {
        let (root, vault, rel) = boveda("lectura");
        let indice = Arc::new(RwLock::new(SearchIndex::build(&vault)));
        let editor = Editor::new();
        let window = ventana(&editor, 900, 800);
        editor.set_search_index(Some(indice.clone()));
        editor.load(vault.clone(), &vault.read_note(&rel).unwrap());
        editor.set_view(View::Lectura);
        editor.render_now();
        assert!(esperar(|| texto_lectura(&editor).contains("Borrador de la charla")).await);
        let t = texto_lectura(&editor);
        for esperado in [
            "zeta",
            "#papers (2)",
            "b12",
            "#tesis (2)",
            "alfa",
            "Nota nueva con #proyectos/tesis",
        ] {
            assert!(t.contains(esperado), "falta «{esperado}» en {t}");
        }
        assert!(!t.contains("```"), "{t}");
        // La prioritaria va antes que `c` en papers.
        assert!(
            t.find("b12").unwrap() < t.find("\nc").unwrap_or(usize::MAX),
            "{t}"
        );
        assert!(editor.has_generated());
        glib::timeout_future(Duration::from_millis(400)).await;
        capturar(&window, "036-indice-lectura");
        // Etiquetar una nota nueva: entra tras actualizar el índice y volver a dibujar (como
        // `apply_tag_snapshot`).
        let nueva = RelPath::new("wiki/nueva ñ.md").unwrap();
        fs::write(nueva.to_path(&root), "#proyectos/tesis nueva\n").unwrap();
        indice
            .write()
            .unwrap()
            .update_file(&vault, &nueva, "#proyectos/tesis nueva\n");
        editor.render_now();
        assert!(esperar(|| texto_lectura(&editor).contains("nueva ñ")).await);
        assert!(texto_lectura(&editor).contains("#tesis (3)"));
        glib::timeout_future(Duration::from_millis(300)).await;
        capturar(&window, "036-indice-en-vivo");
        // El índice no se escribe en la nota.
        assert_eq!(fs::read_to_string(rel.to_path(&root)).unwrap(), NOTA);
        // Los enlaces: la nota, la etiqueta y «Nota nueva».
        let urls = editor.0.reader.link_urls();
        assert!(urls.contains(&"/alfa.md".to_owned()), "{urls:?}");
        assert!(
            urls.contains(&"orbital-tag:proyectos/tesis".to_owned()),
            "{urls:?}"
        );
        assert!(
            urls.contains(&format!(
                "{}proyectos/tesis",
                crate::editor::reader::NOTA_NUEVA
            )),
            "{urls:?}"
        );
        window.destroy();
        let _ = fs::remove_dir_all(&root);
    });
}

/// «Insertar índice de etiqueta…» y «Nota nueva con esta etiqueta» desde Código, y el globito.
#[test]
#[ignore = "requiere una sesión gráfica; tools/pruebas-gtk.sh"]
fn insertar_indice_de_etiqueta_y_globito() {
    adw::init().expect("Se necesita una sesión gráfica GTK");
    crate::style::apply();
    glib::MainContext::default().block_on(async {
        let (root, vault, rel) = boveda("insertar");
        let indice = Arc::new(RwLock::new(SearchIndex::build(&vault)));
        let editor = Editor::new();
        let window = ventana(&editor, 900, 700);
        editor.set_search_index(Some(indice.clone()));
        editor.load(vault.clone(), &vault.read_note(&rel).unwrap());
        editor.set_view(View::Codigo);
        let b = editor.0.buffer.clone();
        let original = editor.text();
        let en = original.find("Fin.").unwrap();
        b.place_cursor(&b.iter_at_offset(original[..en].chars().count() as i32));
        assert_eq!(editor.etiqueta_del_indice(), None, "fuera del bloque");
        let d = editor.insertar_indice().expect("el diálogo");
        glib::timeout_future(Duration::from_millis(300)).await;
        capturar(&d, "036-insertar-indice");
        let combo = todos(&d)
            .into_iter()
            .find_map(|w| w.downcast::<adw::ComboRow>().ok())
            .expect("la etiqueta");
        let lista = combo.model().unwrap();
        let k = (0..lista.n_items())
            .find(|k| {
                lista.item(*k).and_downcast::<gtk::StringObject>().unwrap().string()
                    == "proyectos/papers"
            })
            .expect("la etiqueta en la lista");
        combo.set_selected(k);
        todos(&d)
            .into_iter()
            .filter_map(|w| w.downcast::<adw::SwitchRow>().ok())
            .find(|r| r.title() == "Título de cada nota")
            .unwrap()
            .set_active(false);
        todos(&d)
            .into_iter()
            .filter_map(|w| w.downcast::<gtk::Button>().ok())
            .find(|b| b.label().as_deref() == Some("Insertar"))
            .unwrap()
            .emit_clicked();
        assert_eq!(
            editor.text(),
            "# Proyectos\n\n```indice\netiqueta: proyectos\n```\n\n```indice\netiqueta: proyectos/papers\ntitulo: false\n```\n\nFin.\n"
        );
        b.undo();
        assert_eq!(editor.text(), original);
        b.redo();
        // «Nota nueva con esta etiqueta»: la del bloque donde está el cursor.
        let t = editor.text();
        let pon = |aguja: &str| {
            let off = t.find(aguja).unwrap();
            b.place_cursor(&b.iter_at_offset(t[..off].chars().count() as i32 + 1));
        };
        pon("etiqueta: proyectos/papers");
        assert_eq!(editor.etiqueta_del_indice().as_deref(), Some("proyectos/papers"));
        pon("etiqueta: proyectos\n");
        assert_eq!(editor.etiqueta_del_indice().as_deref(), Some("proyectos"));
        // El globito de Código resume el índice.
        editor.0.view.grab_focus();
        editor.actualizar_vista_previa();
        let resumen = |e: &Editor| {
            e.0.vista_previa.borrow().as_ref().is_some_and(|v| {
                v.popover.is_visible() && v.error.label().starts_with("Índice de #proyectos: 5 notas")
            })
        };
        assert!(esperar(|| resumen(&editor)).await, "globito con el resumen");
        glib::timeout_future(Duration::from_millis(200)).await;
        crate::editor::graficos_tests::capturar_con_globito(&window, &editor, "036-globito");
        window.close();
        let _ = fs::remove_dir_all(&root);
    });
}
