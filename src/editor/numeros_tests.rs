//! Pruebas GTK en proceso de los números de línea en Código (054): la preferencia se aplica en
//! vivo a todas las vistas de nota, convive con el canal de las flechitas del plegado (028) y no
//! corre la columna centrada (`code::center_column`). Con `ORBITAL_CAPTURA_DIR`, guarda capturas.

use std::fs;
use std::sync::Arc;

use super::code;
use super::plegado::plegado_tests::OUTLINE;
use crate::editor::{Editor, View};
use adw::prelude::*;
use gtk::glib;
use orbital_vault::{RelPath, Vault};

/// Línea de «## Historia» en `OUTLINE` (plegable).
const HISTORIA: u32 = 6;

fn boveda(nombre: &str, rel: &str, texto: &str) -> (std::path::PathBuf, Arc<Vault>, RelPath) {
    let root = std::env::temp_dir().join(format!("orbital-054-{nombre}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join("wiki")).unwrap();
    fs::create_dir_all(root.join("raw")).unwrap();
    let rel = RelPath::new(rel).unwrap();
    fs::write(rel.to_path(&root), texto).unwrap();
    (
        root.clone(),
        Arc::new(orbital_vault::open(&root).unwrap()),
        rel,
    )
}

async fn pausa(ms: u64) {
    glib::timeout_future(std::time::Duration::from_millis(ms)).await;
}

/// Lo que importa de la disposición del margen izquierdo, en coordenadas de la vista.
#[derive(Debug)]
struct Margen {
    /// Dónde empieza el texto.
    texto: i32,
    /// Borde derecho de los números (si se ven).
    numeros_fin: Option<f32>,
    /// Borde izquierdo y derecho del canal de las flechitas (si se ve).
    canal: Option<(f32, f32)>,
}

fn margen(e: &Editor) -> Margen {
    let view = &e.0.view;
    let pagina = e.0.code_scroller.hadjustment().page_size() as i32;
    let texto = view.width() - pagina + view.left_margin();
    let numeros_fin = Some(&e.0.numeros)
        .filter(|r| r.is_visible())
        .and_then(|r| r.compute_bounds(view))
        .map(|b| b.x() + b.width());
    let canal = Some(&e.0.flechas)
        .filter(|c| c.is_visible())
        .and_then(|c| c.compute_bounds(view))
        .map(|b| (b.x(), b.x() + b.width()));
    Margen {
        texto,
        numeros_fin,
        canal,
    }
}

/// Con `ORBITAL_CAPTURA_DIR`, guarda la vista de la nota como PNG.
fn capturar(widget: &impl IsA<gtk::Widget>, nombre: &str) {
    super::graficos_tests::capturar(widget, nombre);
}

/// Los números se prenden y apagan en vivo en todas las vistas, quedan a la izquierda de las
/// flechitas sin pisarlas, el texto no se corre de la columna centrada, y lo plegado no rompe nada.
#[test]
#[ignore = "requiere una sesión gráfica; tools/pruebas-gtk.sh"]
fn numeros_en_vivo_con_plegado_y_columna() {
    adw::init().expect("Se necesita una sesión gráfica GTK");
    crate::style::apply();
    code::set_line_numbers(true);
    let (root, vault, rel) = boveda("md", "wiki/outline.md", OUTLINE);
    let (root2, vault2, rel2) = boveda(
        "html",
        "wiki/pagina.html",
        "<!doctype html>\n<html>\n<body>\n<h1>Título</h1>\n<p>Un párrafo.</p>\n</body>\n</html>\n",
    );
    let e = Editor::new();
    let otra = Editor::new();
    e.load(vault.clone(), &vault.read_note(&rel).unwrap());
    e.set_view(View::Codigo);
    let _ = e.arbol();
    otra.load(vault2.clone(), &vault2.read_note(&rel2).unwrap());
    otra.set_view(View::Codigo);
    let win = gtk::Window::builder()
        .default_width(1200)
        .default_height(800)
        .child(e.widget())
        .build();
    // La otra vista, en otra ventana del mismo ancho. Se la pone delante para medirla: GNOME no
    // le da cuadros (ni disposición) a una ventana tapada.
    let win2 = gtk::Window::builder()
        .default_width(1200)
        .default_height(800)
        .child(otra.widget())
        .build();
    win2.present();
    win.present();
    glib::MainContext::default().block_on(async {
        pausa(600).await;
        let view = e.0.view.clone();
        assert!(e.0.numeros.is_visible(), "prendidos por defecto");
        let con = margen(&e);
        let (canal_ini, canal_fin) = con.canal.expect("las flechitas se ven en .md");
        let numeros_fin = con.numeros_fin.expect("los números se ven");
        assert!(
            numeros_fin <= canal_ini + 0.5,
            "los números a la izquierda de las flechitas: {con:?}"
        );
        assert!(
            canal_fin <= con.texto as f32 + 0.5,
            "las flechitas antes del texto: {con:?}"
        );
        // La columna de 860 px centrada en la vista.
        let esperado = (view.width() - 860) / 2;
        assert!(
            (con.texto - esperado).abs() <= 1,
            "el texto empieza en la columna centrada: {con:?}, esperado {esperado}"
        );
        capturar(&e.0.code_scroller, "054-codigo");

        // Con una sección plegada.
        assert!(e.plegar(HISTORIA));
        pausa(300).await;
        let plegado = margen(&e);
        assert_eq!(plegado.texto, con.texto, "plegar no corre el texto");
        assert_eq!(plegado.numeros_fin, con.numeros_fin);
        capturar(&e.0.code_scroller, "054-plegado");

        // La nota .html, en otra vista: sin flechitas, los números junto al texto.
        win2.present();
        pausa(300).await;
        let h = margen(&otra);
        assert!(otra.0.numeros.is_visible());
        assert!(h.canal.is_none(), "sin flechitas en .html");
        let hf = h.numeros_fin.expect("números en .html");
        assert!(hf <= h.texto as f32 + 0.5, "{h:?}");
        let esperado_html = (otra.0.view.width() - 860) / 2;
        assert_eq!(h.texto, con.texto, "la misma columna con y sin flechitas");
        assert!((h.texto - esperado_html).abs() <= 1, "{h:?}");
        capturar(&otra.0.code_scroller, "054-html");

        // Apagar: en vivo, en las dos vistas, y el texto queda donde estaba.
        win.present();
        pausa(300).await;
        code::set_line_numbers(false);
        pausa(300).await;
        assert!(!e.0.numeros.is_visible() && !otra.0.numeros.is_visible());
        let sin = margen(&e);
        assert!(sin.numeros_fin.is_none());
        assert_eq!(sin.texto, con.texto, "apagar no corre el texto: {sin:?}");
        let (_, sin_canal_fin) = sin.canal.expect("las flechitas siguen");
        assert!(sin_canal_fin <= sin.texto as f32 + 0.5);
        assert!(e.esta_plegado(HISTORIA), "lo plegado sigue plegado");
        win2.present();
        pausa(300).await;
        assert_eq!(margen(&otra).texto, h.texto, "en la otra ventana tampoco");
        win.present();
        pausa(300).await;
        capturar(&e.0.code_scroller, "054-sin-numeros");

        // Prender de nuevo: igual que al principio.
        code::set_line_numbers(true);
        pausa(300).await;
        let otra_vez = margen(&e);
        assert_eq!(otra_vez.texto, con.texto);
        assert_eq!(otra_vez.numeros_fin, con.numeros_fin);
        assert_eq!(otra_vez.canal, plegado.canal);

        // Dividida: la mitad de Código es angosta; los números toman su ancho natural y nada
        // se pisa.
        e.desplegar_todo();
        e.set_view(View::Dividida);
        pausa(600).await;
        let d = margen(&e);
        let (di, df) = d.canal.expect("flechitas en Dividida");
        let dn = d.numeros_fin.expect("números en Dividida");
        assert!(dn <= di + 0.5 && df <= d.texto as f32 + 0.5, "{d:?}");
        capturar(&e.0.code_scroller, "054-dividida");
    });
    win.destroy();
    win2.destroy();
    let _ = fs::remove_dir_all(root);
    let _ = fs::remove_dir_all(root2);
}

/// El interruptor «Números de línea» de Preferencias aplica en el acto. Con
/// `ORBITAL_CAPTURA_DIR`, guarda `054-preferencias.png`.
#[test]
#[ignore = "requiere una sesión gráfica; tools/pruebas-gtk.sh"]
fn preferencia_en_el_dialogo() {
    adw::init().expect("Se necesita una sesión gráfica GTK");
    crate::style::apply();
    code::set_line_numbers(true);
    let (root, vault, rel) = boveda("prefs", "wiki/outline.md", OUTLINE);
    let e = Editor::new();
    e.load(vault.clone(), &vault.read_note(&rel).unwrap());
    e.set_view(View::Codigo);
    let win = adw::ApplicationWindow::builder()
        .default_width(1000)
        .default_height(800)
        .build();
    win.set_content(Some(e.widget()));
    win.present();
    glib::MainContext::default().block_on(async {
        pausa(300).await;
        let dialog =
            crate::preferences::show(&win, root.clone(), vault.config().clone(), false, |_| {});
        pausa(500).await;
        let fila = descendientes(dialog.upcast_ref())
            .into_iter()
            .find_map(|w| {
                w.downcast::<adw::SwitchRow>()
                    .ok()
                    .filter(|r| r.title() == "Números de línea")
            })
            .expect("la fila «Números de línea»");
        assert!(fila.is_active());
        capturar(&dialog, "054-preferencias");
        fila.set_active(false);
        assert!(!code::line_numbers());
        assert!(!e.0.numeros.is_visible(), "se apaga en vivo");
        fila.set_active(true);
        assert!(e.0.numeros.is_visible(), "se prende en vivo");
        dialog.force_close();
    });
    win.destroy();
    let _ = fs::remove_dir_all(root);
}

fn descendientes(w: &gtk::Widget) -> Vec<gtk::Widget> {
    let mut v = Vec::new();
    let mut hijo = w.first_child();
    while let Some(c) = hijo {
        v.extend(descendientes(&c));
        hijo = c.next_sibling();
        v.push(c);
    }
    v
}
