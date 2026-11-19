//! Pruebas GTK en proceso de las fórmulas (050): Lectura y vista previa de Código.

use std::fs;
use std::sync::Arc;
use std::time::Duration;

use crate::editor::{Editor, View};
use gtk::glib;
use gtk::prelude::*;
use orbital_vault::RelPath;

const NOTA: &str = "# Título $E=mc^2$\n\nSea $x^2$ y cuesta $5 o $10.\n\n$$\n\\begin{pmatrix} a & b \\\\ c & d \\end{pmatrix}\n$$\n\n```math\n\\sqrt{2}\n```\n\nMal: $x^^2$ y `$c$`.\n";

fn boveda(nombre: &str) -> (std::path::PathBuf, Arc<orbital_vault::Vault>, RelPath) {
    let root = std::env::temp_dir().join(format!("orbital-050-{nombre}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join("wiki")).unwrap();
    let rel = RelPath::new("wiki/nota.md").unwrap();
    fs::write(rel.to_path(&root), NOTA).unwrap();
    (
        root.clone(),
        Arc::new(orbital_vault::open(&root).unwrap()),
        rel,
    )
}

async fn esperar(mut f: impl FnMut() -> bool) -> bool {
    for _ in 0..100 {
        if f() {
            return true;
        }
        glib::timeout_future(Duration::from_millis(50)).await;
    }
    false
}

/// Los pintables de un buffer, en orden.
fn pintables(b: &gtk::TextBuffer) -> Vec<gtk::gdk::Paintable> {
    let mut out = Vec::new();
    let mut it = b.start_iter();
    loop {
        if let Some(p) = it.paintable() {
            out.push(p);
        }
        if !it.forward_char() {
            break;
        }
    }
    out
}

#[test]
#[ignore = "requiere una sesión gráfica; tools/pruebas-gtk.sh"]
fn formulas_en_lectura() {
    adw::init().expect("Se necesita una sesión gráfica GTK");
    glib::MainContext::default().block_on(async {
        let (root, vault, rel) = boveda("lectura");
        let editor = Editor::new();
        let window = gtk::Window::builder()
            .default_width(900)
            .default_height(700)
            .child(editor.widget())
            .build();
        window.present();
        editor.load(vault.clone(), &vault.read_note(&rel).unwrap());
        editor.set_view(View::Lectura);
        editor.render_now();
        let buf = editor.0.reader.buffer();
        assert!(
            esperar(|| pintables(&editor.0.reader.buffer()).len() == 4).await,
            "cuatro fórmulas dibujadas: {}",
            pintables(&editor.0.reader.buffer()).len()
        );
        let buf = if pintables(&buf).len() == 4 {
            buf
        } else {
            editor.0.reader.buffer()
        };
        let texto = buf.slice(&buf.start_iter(), &buf.end_iter(), true);
        // El LaTeX acompaña a cada fórmula (lector de pantalla, búsqueda, copiar).
        for fuente in ["$E=mc^2$", "$x^2$", "$\\sqrt{2}$"] {
            assert!(texto.contains(fuente), "{fuente} en {texto}");
        }
        assert!(texto.contains("cuesta $5 o $10."), "{texto}");
        assert!(texto.contains("$c$"), "{texto}");
        // La fórmula con error: su fuente con la etiqueta de error y el motivo.
        let tag = buf.tag_table().lookup("formula-error").unwrap();
        let at = texto.find("$x^^2$").unwrap();
        let it = buf.iter_at_offset(texto[..at].chars().count() as i32 + 2);
        assert!(it.has_tag(&tag));
        let motivo = editor.0.reader.error_de_formula_en(&it).unwrap();
        assert!(motivo.starts_with("Error en la fórmula"), "{motivo}");
        // Todas con imagen y tamaño.
        for p in pintables(&buf) {
            assert!(p.intrinsic_width() > 4 && p.intrinsic_height() > 4);
        }
        // El disco no cambia.
        assert_eq!(fs::read_to_string(rel.to_path(&root)).unwrap(), NOTA);
        window.close();
        let _ = fs::remove_dir_all(&root);
    });
}

#[test]
#[ignore = "requiere una sesión gráfica; tools/pruebas-gtk.sh"]
fn vista_previa_de_formula_en_codigo() {
    adw::init().expect("Se necesita una sesión gráfica GTK");
    glib::MainContext::default().block_on(async {
        let (root, vault, rel) = boveda("codigo");
        let editor = Editor::new();
        let window = gtk::Window::builder()
            .default_width(900)
            .default_height(700)
            .child(editor.widget())
            .build();
        window.present();
        editor.load(vault.clone(), &vault.read_note(&rel).unwrap());
        editor.set_view(View::Codigo);
        editor.0.view.grab_focus();
        let b = editor.buffer().clone();
        let poner = |aguja: &str| {
            let off = NOTA.find(aguja).unwrap();
            b.place_cursor(&b.iter_at_offset(NOTA[..off].chars().count() as i32 + 1));
        };
        let visible = |e: &Editor| {
            e.0.vista_previa
                .borrow()
                .as_ref()
                .is_some_and(|v| v.popover.is_visible())
        };
        poner("x^2$ y");
        editor.actualizar_vista_previa();
        assert!(esperar(|| visible(&editor)).await, "globito con $x^2$");
        {
            let v = editor.0.vista_previa.borrow();
            let v = v.as_ref().unwrap();
            assert!(v.imagen.paintable().is_some());
            assert!(!v.error.is_visible());
            assert!(!v.popover.can_focus() && !v.popover.is_autohide());
        }
        // En un ```math.
        poner("sqrt{2}");
        editor.actualizar_vista_previa();
        assert!(esperar(|| visible(&editor)).await);
        // Con error: el motivo.
        poner("x^^2");
        editor.actualizar_vista_previa();
        assert!(
            esperar(|| editor
                .0
                .vista_previa
                .borrow()
                .as_ref()
                .is_some_and(|v| v.error.is_visible()))
            .await
        );
        // Fuera de una fórmula (dinero, código): sin globito.
        for aguja in ["5 o $10", "c$`"] {
            poner(aguja);
            editor.actualizar_vista_previa();
            assert!(!visible(&editor), "sin globito en {aguja}");
        }
        window.close();
        let _ = fs::remove_dir_all(&root);
    });
}
