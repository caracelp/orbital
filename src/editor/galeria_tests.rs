//! Pruebas GTK en proceso de la galería en la nota (033): los cuatro layouts, el visor a
//! pantalla completa, el teclado, la paginación, la carga perezosa de una carpeta grande, el
//! estado vacío y los videos; una nota sin galería no prepara nada. Con `ORBITAL_CAPTURA_DIR`,
//! capturas.

use std::fs;
use std::sync::Arc;
use std::time::{Duration, Instant};

use super::carrusel_tests::{contador, esperar, png, tecla, todos, ventana};
use crate::editor::graficos_tests::capturar;
use crate::editor::{Editor, View};
use adw::prelude::*;
use gtk::{gdk, glib};
use orbital_vault::RelPath;

const COLORES: [[u8; 3]; 6] = [
    [255, 105, 180],
    [90, 160, 255],
    [255, 170, 60],
    [80, 210, 170],
    [190, 120, 255],
    [240, 90, 90],
];

/// Proporciones variadas (apaisadas, verticales y cuadradas).
const MEDIDAS: [(i32, i32); 5] = [(640, 400), (400, 600), (500, 500), (800, 360), (360, 480)];

/// Una bóveda temporal con un nombre no ASCII, `n` imágenes en `imagenes/tesis ñ/` y la nota
/// `wiki/galeria.md`. `lado`: el ancho de las imágenes (escala de [`MEDIDAS`]).
fn boveda(
    nombre: &str,
    texto: &str,
    n: usize,
    escala: f32,
    base: Option<&std::path::Path>,
) -> (std::path::PathBuf, Arc<orbital_vault::Vault>, RelPath) {
    let raiz = base.map_or_else(std::env::temp_dir, std::path::Path::to_path_buf);
    let root = raiz.join(format!(
        "orbital-033-{nombre}-Bóveda ñandú-{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&root);
    let dir = root.join("imagenes/tesis ñ");
    fs::create_dir_all(&dir).unwrap();
    fs::create_dir_all(root.join("wiki")).unwrap();
    for k in 0..n {
        let (w, h) = MEDIDAS[k % MEDIDAS.len()];
        png(
            &dir.join(format!("foto-{k:04}.png")),
            (w as f32 * escala) as i32,
            (h as f32 * escala) as i32,
            COLORES[k % COLORES.len()],
        );
    }
    let rel = RelPath::new("wiki/galeria.md").unwrap();
    fs::write(rel.to_path(&root), texto).unwrap();
    (
        root.clone(),
        Arc::new(orbital_vault::open(&root).unwrap()),
        rel,
    )
}

fn galerias(w: &impl IsA<gtk::Widget>) -> Vec<gtk::Widget> {
    todos(w)
        .into_iter()
        .filter(|w| w.has_css_class("orbital-galeria"))
        .collect()
}

/// Las celdas vivas (visibles) de una galería.
fn celdas(g: &gtk::Widget) -> Vec<gtk::Button> {
    todos(g)
        .into_iter()
        .filter(|w| w.has_css_class("orbital-galeria-celda") && w.is_visible())
        .filter_map(|w| w.downcast::<gtk::Button>().ok())
        .collect()
}

fn con_imagen(c: &gtk::Button) -> bool {
    todos(c)
        .into_iter()
        .find_map(|w| w.downcast::<gtk::Picture>().ok())
        .and_then(|p| p.paintable())
        .is_some()
}

fn celda(g: &gtk::Widget, nombre: &str) -> gtk::Button {
    celdas(g)
        .into_iter()
        .find(|c| c.tooltip_text().as_deref() == Some(nombre))
        .unwrap_or_else(|| panic!("celda «{nombre}»"))
}

fn enfocada(g: &gtk::Widget) -> Option<String> {
    celdas(g)
        .into_iter()
        .find(|c| c.has_focus())
        .and_then(|c| c.tooltip_text())
        .map(|t| t.to_string())
}

fn visor() -> Option<gtk::Window> {
    gtk::Window::list_toplevels()
        .into_iter()
        .filter_map(|w| w.downcast::<gtk::Window>().ok())
        .find(|w| w.has_css_class("orbital-carrusel-ventana") && w.is_visible())
}

/// Abre la nota en Lectura y espera la galería con sus primeras miniaturas.
async fn abrir(editor: &Editor, vault: &Arc<orbital_vault::Vault>, rel: &RelPath) -> gtk::Widget {
    editor.load(vault.clone(), &vault.read_note(rel).unwrap());
    editor.set_view(View::Lectura);
    editor.render_now();
    let vista = editor.0.reader.text_view().clone();
    assert!(esperar(|| galerias(&vista).len() == 1).await, "una galería");
    galerias(&vista).remove(0)
}

fn nota(layout: &str, extra: &str) -> String {
    format!(
        "# Galería\n\nAntes.\n\n```galeria\ncarpeta: imagenes/tesis ñ\nlayout: {layout}\n{extra}```\n\nDespués.\n"
    )
}

#[test]
#[ignore = "requiere una sesión gráfica; tools/pruebas-gtk.sh"]
fn galeria_cuatro_layouts_y_visor() {
    adw::init().expect("Se necesita una sesión gráfica GTK");
    crate::style::apply();
    glib::MainContext::default().block_on(async {
        let texto = nota("miniaturas", "");
        let (root, _, rel) = boveda("layouts", &texto, 10, 1.0, None);
        let editor = Editor::new();
        let window = ventana(&editor, 1000, 900);
        for layout in ["miniaturas", "masonry", "justificadas", "carrusel"] {
            fs::write(rel.to_path(&root), nota(layout, "")).unwrap();
            let vault = Arc::new(orbital_vault::open(&root).unwrap());
            let g = abrir(&editor, &vault, &rel).await;
            if layout == "carrusel" {
                // El carrusel de la 032, con la carpeta entera.
                let c = todos(&g)
                    .into_iter()
                    .find(|w| w.has_css_class("orbital-carrusel"))
                    .expect("el carrusel");
                assert!(esperar(|| contador(&c) == "1 de 10").await);
                glib::timeout_future(Duration::from_millis(600)).await;
                capturar(&window, "033-carrusel");
                continue;
            }
            assert!(
                esperar(|| {
                    let c = celdas(&g);
                    c.len() == 10 && c.iter().all(con_imagen)
                })
                .await,
                "{layout}: diez celdas con miniatura"
            );
            glib::timeout_future(Duration::from_millis(100)).await;
            let medidas: Vec<(i32, i32)> = (0..10)
                .map(|k| {
                    let c = celda(&g, &format!("foto-{k:04}.png"));
                    (c.width(), c.height())
                })
                .collect();
            match layout {
                "miniaturas" => assert!(medidas.iter().all(|(w, h)| w == h), "{medidas:?}"),
                "masonry" => {
                    for (k, (w, h)) in medidas.iter().enumerate() {
                        let (iw, ih) = MEDIDAS[k % MEDIDAS.len()];
                        let r = *w as f32 / *h as f32;
                        let ri = iw as f32 / ih as f32;
                        assert!((r - ri).abs() < 0.05, "masonry sin recorte: {k} {r} {ri}");
                    }
                }
                _ => {
                    // Filas de alto parejo: las de una misma fila miden lo mismo.
                    let c0 = celda(&g, "foto-0000.png");
                    let c1 = celda(&g, "foto-0001.png");
                    assert_eq!(c0.height(), c1.height(), "{medidas:?}");
                }
            }
            capturar(&window, &format!("033-{layout}"));
        }
        // El visor: clic en la cuarta foto.
        fs::write(rel.to_path(&root), &texto).unwrap();
        let vault = Arc::new(orbital_vault::open(&root).unwrap());
        let g = abrir(&editor, &vault, &rel).await;
        assert!(esperar(|| celdas(&g).len() == 10).await);
        celda(&g, "foto-0003.png").emit_clicked();
        assert!(esperar(|| visor().is_some()).await, "abre el visor");
        let v = visor().unwrap();
        assert!(esperar(|| v.is_fullscreen()).await);
        let c = todos(&v)
            .into_iter()
            .find(|w| w.has_css_class("orbital-carrusel"))
            .unwrap();
        assert_eq!(contador(&c), "4 de 10");
        let t = Instant::now();
        tecla(&c, gdk::Key::Right);
        let peor = t.elapsed();
        assert_eq!(contador(&c), "5 de 10");
        assert!(peor < Duration::from_millis(50), "{peor:?}");
        glib::timeout_future(Duration::from_millis(500)).await;
        capturar(&v, "033-visor");
        // La información del archivo (I).
        tecla(&c, gdk::Key::i);
        let info = || {
            todos(&v)
                .into_iter()
                .filter(|w| w.has_css_class("orbital-visor-info"))
                .flat_map(|w| todos(&w))
                .filter_map(|w| w.downcast::<gtk::Label>().ok())
                .map(|l| l.label().to_string())
                .next()
                .unwrap_or_default()
        };
        assert!(
            esperar(|| info().contains("foto-0004.png\n360 × 480 px\n")).await,
            "{}",
            info()
        );
        // Zoom con el teclado: la foto crece más que la pantalla; 0 la ajusta.
        let foto = || {
            todos(&v)
                .into_iter()
                .filter(|w| w.has_css_class("orbital-visor-zoom") && w.is_mapped())
                .flat_map(|w| todos(&w))
                .find_map(|w| w.downcast::<gtk::Picture>().ok())
                .expect("la foto ampliable")
        };
        assert!(esperar(|| foto().paintable().is_some()).await);
        tecla(&c, gdk::Key::plus);
        tecla(&c, gdk::Key::plus);
        tecla(&c, gdk::Key::plus);
        assert!(
            foto().width_request() > v.width() / 2,
            "{}",
            foto().width_request()
        );
        assert!(
            foto().height_request() > v.height(),
            "{}",
            foto().height_request()
        );
        glib::timeout_future(Duration::from_millis(400)).await;
        capturar(&v, "033-visor-zoom");
        tecla(&c, gdk::Key::_0);
        assert_eq!(foto().width_request(), -1);
        tecla(&c, gdk::Key::i);
        tecla(&c, gdk::Key::Right);
        assert_eq!(contador(&c), "6 de 10");
        v.close();
        assert!(esperar(|| visor().is_none()).await);
        // El foco vuelve a la foto en que se salió.
        assert!(
            esperar(|| enfocada(&g).as_deref() == Some("foto-0005.png")).await,
            "{:?}",
            enfocada(&g)
        );
        // En Código, el bloque es código.
        editor.set_view(View::Codigo);
        let b = &editor.0.buffer;
        assert!(
            b.text(&b.start_iter(), &b.end_iter(), true)
                .contains("```galeria")
        );
        // Nada se escribió en la bóveda.
        assert_eq!(fs::read_to_string(rel.to_path(&root)).unwrap(), texto);
        window.destroy();
        let _ = fs::remove_dir_all(&root);
    });
}

#[test]
#[ignore = "requiere una sesión gráfica; tools/pruebas-gtk.sh"]
fn galeria_teclado_y_paginas() {
    adw::init().expect("Se necesita una sesión gráfica GTK");
    crate::style::apply();
    glib::MainContext::default().block_on(async {
        let texto = nota("miniaturas", "paginar: 4\ncolumnas: 2\n");
        let (root, vault, rel) = boveda("paginas", &texto, 10, 0.25, None);
        let editor = Editor::new();
        let window = ventana(&editor, 900, 900);
        let g = abrir(&editor, &vault, &rel).await;
        assert!(esperar(|| celdas(&g).len() == 4).await, "una página de 4");
        let pagina = || {
            todos(&g)
                .into_iter()
                .filter_map(|w| w.downcast::<gtk::Label>().ok())
                .find(|l| l.has_css_class("numeric"))
                .map(|l| l.label().to_string())
                .unwrap_or_default()
        };
        assert_eq!(pagina(), "Página 1 de 3");
        // Una sola celda entra con Tab: la primera.
        let foco: Vec<String> = celdas(&g)
            .iter()
            .filter(|c| c.is_focusable())
            .filter_map(|c| c.tooltip_text().map(|t| t.to_string()))
            .collect();
        assert_eq!(foco, ["foto-0000.png"]);
        assert!(celda(&g, "foto-0000.png").grab_focus());
        // Flechas: → baja de a una, ↓ salta la fila (2 columnas), Fin va a la última.
        tecla(&g, gdk::Key::Right);
        assert_eq!(enfocada(&g).as_deref(), Some("foto-0001.png"));
        tecla(&g, gdk::Key::Down);
        assert_eq!(enfocada(&g).as_deref(), Some("foto-0003.png"));
        tecla(&g, gdk::Key::Home);
        assert_eq!(enfocada(&g).as_deref(), Some("foto-0000.png"));
        tecla(&g, gdk::Key::End);
        assert_eq!(enfocada(&g).as_deref(), Some("foto-0003.png"));
        // Av Pág: la página siguiente, con el foco en su primera foto.
        tecla(&g, gdk::Key::Page_Down);
        assert_eq!(pagina(), "Página 2 de 3");
        assert!(esperar(|| enfocada(&g).as_deref() == Some("foto-0004.png")).await);
        // El botón de la página siguiente (la última tiene 2).
        let siguiente = todos(&g)
            .into_iter()
            .filter_map(|w| w.downcast::<gtk::Button>().ok())
            .find(|b| b.tooltip_text().as_deref() == Some("Página siguiente (Av Pág)"))
            .unwrap();
        siguiente.emit_clicked();
        assert_eq!(pagina(), "Página 3 de 3");
        assert!(esperar(|| celdas(&g).len() == 2).await);
        assert!(!siguiente.is_sensitive());
        // El visor recorre la carpeta entera desde ahí: «9 de 10» y vuelve a «10 de 10».
        assert!(esperar(|| enfocada(&g).is_some()).await);
        celda(&g, "foto-0008.png").activate();
        assert!(esperar(|| visor().is_some()).await);
        let v = visor().unwrap();
        let c = todos(&v)
            .into_iter()
            .find(|w| w.has_css_class("orbital-carrusel"))
            .unwrap();
        assert_eq!(contador(&c), "9 de 10");
        tecla(&c, gdk::Key::Home);
        assert_eq!(contador(&c), "1 de 10");
        v.close();
        assert!(esperar(|| visor().is_none()).await);
        // Salió en la primera: la galería vuelve a la página 1 con el foco en ella.
        assert!(esperar(|| pagina() == "Página 1 de 3").await);
        assert!(esperar(|| enfocada(&g).as_deref() == Some("foto-0000.png")).await);
        window.destroy();
        let _ = fs::remove_dir_all(&root);
    });
}

#[test]
#[ignore = "requiere una sesión gráfica; tools/pruebas-gtk.sh"]
fn galeria_vacia_videos_y_grupos() {
    adw::init().expect("Se necesita una sesión gráfica GTK");
    crate::style::apply();
    glib::MainContext::default().block_on(async {
        let texto = "```galeria\ncarpeta: no-existe\n```\n\n```galeria\ncarpeta: imagenes\nrecursivo: true\nagrupar: true\ncolor: rojo\n```\n";
        let (root, _, rel) = boveda("grupos", texto, 3, 0.25, None);
        fs::write(root.join("imagenes/clip.mp4"), b"no es un video").unwrap();
        fs::write(root.join("imagenes/notas.txt"), b"x").unwrap();
        let vault = Arc::new(orbital_vault::open(&root).unwrap());
        let editor = Editor::new();
        let window = ventana(&editor, 900, 800);
        editor.load(vault.clone(), &vault.read_note(&rel).unwrap());
        editor.set_view(View::Lectura);
        editor.render_now();
        let vista = editor.0.reader.text_view().clone();
        assert!(esperar(|| galerias(&vista).len() == 2).await);
        let gs = galerias(&vista);
        // La que no tiene carpeta muestra el motivo.
        assert!(
            todos(&gs[0])
                .into_iter()
                .filter_map(|w| w.downcast::<gtk::Label>().ok())
                .any(|l| l.label() == "La carpeta «no-existe» no existe")
        );
        // La otra: el video con su marcador, sin .txt; un título por carpeta.
        let g = &gs[1];
        assert!(esperar(|| celdas(g).len() == 4).await, "{}", celdas(g).len());
        let video = celda(g, "clip.mp4");
        assert!(video.has_css_class("orbital-galeria-video"));
        assert!(
            todos(&video)
                .into_iter()
                .any(|w| w.has_css_class("orbital-galeria-marca") && w.is_visible())
        );
        let titulos: Vec<String> = todos(g)
            .into_iter()
            .filter_map(|w| w.downcast::<gtk::Label>().ok())
            .filter(|l| l.has_css_class("orbital-galeria-titulo"))
            .map(|l| l.label().to_string())
            .collect();
        assert_eq!(titulos, ["imagenes", "tesis ñ"]);
        let buf = editor.0.reader.buffer();
        let t = buf.text(&buf.start_iter(), &buf.end_iter(), true);
        assert!(t.contains("⚠ Opción desconocida ignorada: «color»"), "{t}");
        glib::timeout_future(Duration::from_millis(300)).await;
        capturar(&window, "033-grupos-y-vacia");
        window.destroy();
        let _ = fs::remove_dir_all(&root);
    });
}

/// Una carpeta de 400 fotos sin paginar: solo hay celdas para lo visible y un margen, y al
/// desplazar hasta el final aparecen las de abajo y se sueltan las de arriba.
#[test]
#[ignore = "requiere una sesión gráfica; tools/pruebas-gtk.sh"]
fn galeria_grande_carga_perezosa() {
    adw::init().expect("Se necesita una sesión gráfica GTK");
    crate::style::apply();
    glib::MainContext::default().block_on(async {
        crate::imagenes::vaciar();
        let texto = nota("justificadas", "");
        let (root, vault, rel) = boveda("grande", &texto, 400, 0.1, None);
        let editor = Editor::new();
        let window = ventana(&editor, 900, 700);
        let g = abrir(&editor, &vault, &rel).await;
        assert!(esperar(|| !celdas(&g).is_empty()).await);
        // Esperar que se lean todas las proporciones (la grilla entera dispuesta).
        assert!(esperar(|| g.height() > 5000).await, "{}", g.height());
        glib::timeout_future(Duration::from_millis(300)).await;
        let vivas = celdas(&g).len();
        assert!(vivas < 150, "solo lo visible y un margen: {vivas}");
        assert!(
            celdas(&g)
                .iter()
                .any(|c| c.tooltip_text().as_deref() == Some("foto-0000.png"))
        );
        let sw = editor.0.reader.widget().clone();
        let adj = sw.vadjustment();
        adj.set_value(adj.upper());
        assert!(
            esperar(|| celdas(&g)
                .iter()
                .any(|c| c.tooltip_text().as_deref() == Some("foto-0399.png")))
            .await,
            "aparecen las del final"
        );
        assert!(
            esperar(|| !celdas(&g)
                .iter()
                .any(|c| c.tooltip_text().as_deref() == Some("foto-0001.png")))
            .await,
            "se suelta lo de arriba"
        );
        // La que recibe el foco con Tab (la primera) queda, para no perder el lugar.
        assert!(
            celdas(&g)
                .iter()
                .any(|c| c.tooltip_text().as_deref() == Some("foto-0000.png"))
        );
        assert!(esperar(|| celdas(&g).iter().filter(|c| con_imagen(c)).count() > 10).await);
        assert!(celdas(&g).len() < 150);
        assert!(crate::imagenes::en_cache_total() < 400);
        window.destroy();
        let _ = fs::remove_dir_all(&root);
    });
}

/// «Insertar galería…»: el diálogo arma el bloque en el cursor, sin escribir YAML a mano, y
/// la Lectura lo muestra.
#[test]
#[ignore = "requiere una sesión gráfica; tools/pruebas-gtk.sh"]
fn insertar_galeria_con_el_dialogo() {
    adw::init().expect("Se necesita una sesión gráfica GTK");
    crate::style::apply();
    glib::MainContext::default().block_on(async {
        let texto = "# Nota\n\nAntes.\nDespués.\n";
        let (root, vault, rel) = boveda("insertar", texto, 3, 0.25, None);
        let editor = Editor::new();
        let window = ventana(&editor, 900, 700);
        editor.load(vault.clone(), &vault.read_note(&rel).unwrap());
        editor.set_view(View::Codigo);
        let b = &editor.0.buffer;
        let fin_antes = b.text(&b.start_iter(), &b.end_iter(), true).find("Después").unwrap();
        b.place_cursor(&b.iter_at_offset(fin_antes as i32));
        let d = editor.insertar_galeria().expect("el diálogo");
        let filas: Vec<adw::ComboRow> = todos(&d)
            .into_iter()
            .filter_map(|w| w.downcast::<adw::ComboRow>().ok())
            .collect();
        assert_eq!(filas.len(), 2);
        let lista = filas[0].model().unwrap();
        let k = (0..lista.n_items())
            .find(|k| {
                lista.item(*k).and_downcast::<gtk::StringObject>().unwrap().string()
                    == "imagenes/tesis ñ"
            })
            .unwrap();
        filas[0].set_selected(k);
        filas[1].set_selected(2);
        todos(&d)
            .into_iter()
            .find_map(|w| w.downcast::<adw::SwitchRow>().ok())
            .unwrap()
            .set_active(true);
        todos(&d)
            .into_iter()
            .filter_map(|w| w.downcast::<gtk::Button>().ok())
            .find(|b| b.label().as_deref() == Some("Insertar"))
            .unwrap()
            .emit_clicked();
        let nuevo = b.text(&b.start_iter(), &b.end_iter(), true).to_string();
        assert_eq!(
            nuevo,
            "# Nota\n\nAntes.\n\n```galeria\ncarpeta: imagenes/tesis ñ\nlayout: justificadas\nrecursivo: true\n```\n\nDespués.\n"
        );
        // Una sola acción de deshacer.
        b.undo();
        assert_eq!(b.text(&b.start_iter(), &b.end_iter(), true), texto);
        b.redo();
        editor.set_view(View::Lectura);
        editor.render_now();
        let vista = editor.0.reader.text_view().clone();
        assert!(esperar(|| galerias(&vista).len() == 1).await);
        assert!(esperar(|| celdas(&galerias(&vista)[0]).len() == 3).await);
        window.destroy();
        let _ = fs::remove_dir_all(&root);
    });
}

/// Una nota sin galería no decodifica ni crea nada de la galería.
#[test]
#[ignore = "requiere una sesión gráfica; tools/pruebas-gtk.sh"]
fn sin_galeria_no_prepara_nada() {
    adw::init().expect("Se necesita una sesión gráfica GTK");
    glib::MainContext::default().block_on(async {
        crate::imagenes::vaciar();
        let (root, vault, rel) =
            boveda("sin", "# Nada\n\n```text\ncarpeta: x\n```\n", 2, 0.1, None);
        let editor = Editor::new();
        let window = ventana(&editor, 800, 600);
        editor.load(vault.clone(), &vault.read_note(&rel).unwrap());
        editor.set_view(View::Lectura);
        editor.render_now();
        glib::timeout_future(Duration::from_millis(500)).await;
        assert!(galerias(&editor.0.reader.text_view().clone()).is_empty());
        assert_eq!(crate::imagenes::en_cache_total(), 0);
        window.destroy();
        let _ = fs::remove_dir_all(&root);
    });
}

/// Memoria residente del proceso, en KiB.
fn rss_kib() -> u64 {
    fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|s| {
            s.lines()
                .find(|l| l.starts_with("VmRSS:"))
                .and_then(|l| l.split_whitespace().nth(1)?.parse().ok())
        })
        .unwrap_or(0)
}

/// Mide abrir una nota con una galería de 100 imágenes de 1600×1000 (`ORBITAL_MEDIR_FOTOS`,
/// `ORBITAL_MEDIR_LAYOUT`): hasta ver la galería, hasta las miniaturas visibles, el mayor hueco
/// del hilo de GTK al abrir y al desplazar hasta el final, y la memoria. Las imágenes van en
/// `~/.cache/orbital-033` (`/tmp` es chico). Con `ORBITAL_MEDIR_SIN_GALERIA=1`, la misma nota
/// con el bloque como código común.
#[test]
#[ignore = "medición de la galería (033)"]
fn medir_lectura_galeria() {
    adw::init().expect("Se necesita una sesión gráfica GTK");
    crate::style::apply();
    glib::MainContext::default().block_on(async {
        let sin = std::env::var_os("ORBITAL_MEDIR_SIN_GALERIA").is_some();
        let n: usize = std::env::var("ORBITAL_MEDIR_FOTOS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(100);
        let layout = std::env::var("ORBITAL_MEDIR_LAYOUT").unwrap_or_else(|_| "miniaturas".into());
        let cache = std::path::PathBuf::from(std::env::var_os("HOME").unwrap()).join(".cache/orbital-033");
        fs::create_dir_all(&cache).unwrap();
        let texto = format!(
            "# Cien\n\n```{}\ncarpeta: imagenes/tesis ñ\nlayout: {layout}\n```\n\nFin.\n",
            if sin { "text" } else { "galeria" }
        );
        // Reusar la bóveda de medición si ya tiene las fotos (generarlas tarda).
        let root = cache.join(format!("medir-{n}-Bóveda ñandú"));
        let dir = root.join("imagenes/tesis ñ");
        if fs::read_dir(&dir).map_or(0, |d| d.count()) != n {
            let _ = fs::remove_dir_all(&root);
            fs::create_dir_all(&dir).unwrap();
            for k in 0..n {
                let (w, h) = MEDIDAS[k % MEDIDAS.len()];
                // 1600 de ancho en las apaisadas, 1000-1600 en las demás.
                let f = 1600.0 / w.max(h) as f32;
                png(
                    &dir.join(format!("foto-{k:04}.png")),
                    (w as f32 * f) as i32,
                    (h as f32 * f) as i32,
                    COLORES[k % COLORES.len()],
                );
            }
        }
        fs::create_dir_all(root.join("wiki")).unwrap();
        let rel = RelPath::new("wiki/galeria.md").unwrap();
        fs::write(rel.to_path(&root), &texto).unwrap();
        let vault = Arc::new(orbital_vault::open(&root).unwrap());
        let editor = Editor::new();
        let window = ventana(&editor, 1000, 900);
        glib::timeout_future(Duration::from_millis(500)).await;
        let rss0 = rss_kib();
        let hueco = std::rc::Rc::new(std::cell::Cell::new(Duration::ZERO));
        let ultimo = std::rc::Rc::new(std::cell::Cell::new(Instant::now()));
        {
            let (hueco, ultimo) = (hueco.clone(), ultimo.clone());
            glib::timeout_add_local(Duration::from_millis(1), move || {
                let d = ultimo.get().elapsed();
                hueco.set(hueco.get().max(d));
                ultimo.set(Instant::now());
                glib::ControlFlow::Continue
            });
        }
        ultimo.set(Instant::now());
        let t0 = Instant::now();
        editor.load(vault.clone(), &vault.read_note(&rel).unwrap());
        editor.set_view(View::Lectura);
        editor.render_now();
        let vista = editor.0.reader.text_view().clone();
        if sin {
            let lector = editor.0.reader.clone();
            assert!(
                esperar(|| {
                    let buf = lector.buffer();
                    buf.text(&buf.start_iter(), &buf.end_iter(), true)
                        .contains("Fin.")
                })
                .await
            );
            glib::timeout_future(Duration::from_millis(300)).await;
            println!(
                "sin galería: volcado {:.0} ms, hueco {:.1} ms, memoria +{} KiB",
                t0.elapsed().as_secs_f64() * 1e3,
                hueco.get().as_secs_f64() * 1e3,
                rss_kib().saturating_sub(rss0)
            );
        } else {
            assert!(esperar(|| galerias(&vista).len() == 1).await);
            let g = galerias(&vista).remove(0);
            assert!(esperar(|| !celdas(&g).is_empty()).await);
            let visto = t0.elapsed();
            let sw = editor.0.reader.widget().clone();
            // Las celdas que se ven (dentro de la pantalla) con su miniatura.
            let visibles = |g: &gtk::Widget| {
                celdas(g)
                    .into_iter()
                    .filter(|c| {
                        c.compute_bounds(&sw)
                            .is_some_and(|b| b.y() + b.height() > 0.0 && b.y() < sw.height() as f32)
                    })
                    .collect::<Vec<_>>()
            };
            for _ in 0..2400 {
                let v = visibles(&g);
                if !v.is_empty() && v.iter().all(con_imagen) {
                    break;
                }
                glib::timeout_future(Duration::from_millis(5)).await;
            }
            let primeras = t0.elapsed();
            let cuantas = visibles(&g).len();
            let hueco_abrir = hueco.get();
            let rss_abrir = rss_kib().saturating_sub(rss0);
            // Desplazar hasta el final, de a una pantalla por cuadro.
            hueco.set(Duration::ZERO);
            let adj = sw.vadjustment();
            let t1 = Instant::now();
            let ultima = format!("foto-{:04}.png", n - 1);
            // La grilla puede seguir creciendo (proporciones que llegan): se baja hasta ver la última.
            for _ in 0..2000 {
                if celdas(&g)
                    .iter()
                    .any(|c| c.tooltip_text().as_deref() == Some(&ultima))
                {
                    break;
                }
                adj.set_value((adj.value() + adj.page_size()).min(adj.upper()));
                glib::timeout_future(Duration::from_millis(16)).await;
            }
            assert!(
                esperar(|| celdas(&g)
                    .iter()
                    .any(|c| c.tooltip_text().as_deref() == Some(&ultima) && con_imagen(c)))
                .await
            );
            let al_final = t1.elapsed();
            println!(
                "galería de {n} ({layout}): visible {:.0} ms, {cuantas} miniaturas visibles {:.0} ms, hueco al abrir {:.1} ms; \
                 desplazar al final {:.0} ms con hueco {:.1} ms; celdas vivas {}; memoria +{} KiB al abrir, +{} KiB al final; caché {} imágenes, {} KiB",
                visto.as_secs_f64() * 1e3,
                primeras.as_secs_f64() * 1e3,
                hueco_abrir.as_secs_f64() * 1e3,
                al_final.as_secs_f64() * 1e3,
                hueco.get().as_secs_f64() * 1e3,
                celdas(&g).len(),
                rss_abrir,
                rss_kib().saturating_sub(rss0),
                crate::imagenes::en_cache_total(),
                crate::imagenes::en_cache_bytes() / 1024
            );
        }
        window.destroy();
    });
}
