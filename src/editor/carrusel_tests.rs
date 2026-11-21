//! Pruebas GTK en proceso del carrusel de medios (032): navegación con botones, teclado y
//! miniaturas; leyendas; cada tipo con su visor; copiar el embed; pantalla completa; pase
//! automático; una nota sin carrusel no prepara nada. Con `ORBITAL_CAPTURA_DIR`, capturas.

use std::fs;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::editor::graficos_tests::capturar;
use crate::editor::{Editor, View};
use adw::prelude::*;
use gtk::{gdk, glib};
use orbital_vault::RelPath;

/// Una imagen PNG de prueba con un degradé del color dado.
fn png(path: &std::path::Path, ancho: i32, alto: i32, color: [u8; 3]) {
    let mut px = Vec::with_capacity((ancho * alto * 3) as usize);
    for y in 0..alto {
        for x in 0..ancho {
            let k = (x + y) as f32 / (ancho + alto) as f32;
            for c in color {
                px.push((f32::from(c) * (0.35 + 0.65 * k)) as u8);
            }
        }
    }
    let pb = gtk::gdk_pixbuf::Pixbuf::from_bytes(
        &glib::Bytes::from_owned(px),
        gtk::gdk_pixbuf::Colorspace::Rgb,
        false,
        8,
        ancho,
        alto,
        ancho * 3,
    );
    pb.savev(path, "png", &[]).unwrap();
}

/// Un GIF de 1×1.
const GIF: &[u8] = &[
    0x47, 0x49, 0x46, 0x38, 0x39, 0x61, 0x01, 0x00, 0x01, 0x00, 0x80, 0x00, 0x00, 0xff, 0x00, 0xff,
    0x00, 0x00, 0x00, 0x21, 0xf9, 0x04, 0x00, 0x00, 0x00, 0x00, 0x00, 0x2c, 0x00, 0x00, 0x00, 0x00,
    0x01, 0x00, 0x01, 0x00, 0x00, 0x02, 0x02, 0x44, 0x01, 0x00, 0x3b,
];

const COLORES: [[u8; 3]; 5] = [
    [255, 105, 180],
    [90, 160, 255],
    [255, 170, 60],
    [80, 210, 170],
    [190, 120, 255],
];

/// Una bóveda temporal con un nombre no ASCII y `n` imágenes en `wiki/fotos ñ/`.
fn boveda(
    nombre: &str,
    texto: &str,
    n: usize,
) -> (std::path::PathBuf, Arc<orbital_vault::Vault>, RelPath) {
    let root = std::env::temp_dir().join(format!(
        "orbital-032-{nombre}-Bóveda ñandú-{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join("wiki/fotos ñ")).unwrap();
    for k in 0..n {
        png(
            &root.join(format!("wiki/fotos ñ/foto-{k:02}.png")),
            1600,
            1000,
            COLORES[k % COLORES.len()],
        );
    }
    fs::write(root.join("wiki/animada.gif"), GIF).unwrap();
    fs::write(
        root.join("wiki/sonido.wav"),
        crate::medios::tests::wav_de_prueba(0.5),
    )
    .unwrap();
    fs::write(root.join("wiki/clip.mp4"), b"no es un video").unwrap();
    fs::write(root.join("wiki/informe.pdf"), b"%PDF-1.4\n").unwrap();
    let rel = RelPath::new("wiki/carrusel.md").unwrap();
    fs::write(rel.to_path(&root), texto).unwrap();
    (
        root.clone(),
        Arc::new(orbital_vault::open(&root).unwrap()),
        rel,
    )
}

async fn esperar(mut f: impl FnMut() -> bool) -> bool {
    for _ in 0..240 {
        if f() {
            return true;
        }
        glib::timeout_future(Duration::from_millis(25)).await;
    }
    false
}

fn descendientes(w: &gtk::Widget, out: &mut Vec<gtk::Widget>) {
    let mut h = w.first_child();
    while let Some(x) = h {
        out.push(x.clone());
        descendientes(&x, out);
        h = x.next_sibling();
    }
}

fn todos(w: &impl IsA<gtk::Widget>) -> Vec<gtk::Widget> {
    let mut v = Vec::new();
    descendientes(w.as_ref(), &mut v);
    v
}

fn carruseles(w: &impl IsA<gtk::Widget>) -> Vec<gtk::Widget> {
    todos(w)
        .into_iter()
        .filter(|w| w.has_css_class("orbital-carrusel"))
        .collect()
}

fn boton(w: &gtk::Widget, tooltip: &str) -> gtk::Button {
    todos(w)
        .into_iter()
        .filter_map(|w| w.downcast::<gtk::Button>().ok())
        .find(|b| b.tooltip_text().as_deref() == Some(tooltip))
        .unwrap_or_else(|| panic!("botón «{tooltip}»"))
}

fn contador(w: &gtk::Widget) -> String {
    todos(w)
        .into_iter()
        .filter_map(|w| w.downcast::<gtk::Label>().ok())
        .find(|l| l.has_css_class("numeric"))
        .map(|l| l.label().to_string())
        .unwrap_or_default()
}

/// La página visible del escenario.
fn pagina(w: &gtk::Widget) -> gtk::Widget {
    todos(w)
        .into_iter()
        .find_map(|w| w.downcast::<gtk::Stack>().ok())
        .and_then(|s| s.visible_child())
        .expect("una página")
}

fn leyenda_visible(w: &gtk::Widget) -> Option<String> {
    todos(w)
        .into_iter()
        .filter_map(|w| w.downcast::<gtk::Label>().ok())
        .find(|l| {
            (l.has_css_class("orbital-carrusel-leyenda")
                || l.has_css_class("orbital-carrusel-leyenda-debajo"))
                && l.is_visible()
        })
        .map(|l| l.label().to_string())
}

/// Manda una tecla al controlador de teclado del carrusel.
fn tecla(w: &gtk::Widget, k: gdk::Key) -> bool {
    let ctrls = w.observe_controllers();
    for n in 0..ctrls.n_items() {
        if let Some(c) = ctrls.item(n).and_downcast::<gtk::EventControllerKey>() {
            return c.emit_by_name::<bool>(
                "key-pressed",
                &[&k.into_glib(), &0u32, &gdk::ModifierType::empty()],
            );
        }
    }
    panic!("sin controlador de teclado");
}

use gtk::glib::translate::IntoGlib;

const NOTA: &str = "# Carrusel\n\nAntes.\n\n```media-slider\n---\ncarouselShowThumbnails: true\nthumbnailPosition: bottom\nenhancedView: true\n---\n![[foto-00.png|Corte sagital]]\n![[foto-01.png]]\n![[fotos ñ/foto-02.png|Vista lateral]]\n```\n\nDespués.\n";

fn ventana(editor: &Editor, ancho: i32, alto: i32) -> gtk::Window {
    let tostadas = adw::ToastOverlay::new();
    tostadas.set_child(Some(editor.widget()));
    let w = gtk::Window::builder()
        .default_width(ancho)
        .default_height(alto)
        .child(&tostadas)
        .build();
    w.present();
    w
}

#[test]
#[ignore = "requiere una sesión gráfica; tools/pruebas-gtk.sh"]
fn carrusel_en_lectura() {
    adw::init().expect("Se necesita una sesión gráfica GTK");
    crate::style::apply();
    glib::MainContext::default().block_on(async {
        let (root, vault, rel) = boveda("lectura", NOTA, 3);
        let editor = Editor::new();
        let window = ventana(&editor, 1000, 900);
        editor.load(vault.clone(), &vault.read_note(&rel).unwrap());
        editor.set_view(View::Lectura);
        editor.render_now();
        let vista = editor.0.reader.text_view().clone();
        assert!(
            esperar(|| carruseles(&vista).len() == 1).await,
            "un carrusel"
        );
        let c = carruseles(&vista).remove(0);
        assert!(esperar(|| c.is_mapped()).await);
        assert_eq!(c.accessible_role(), gtk::AccessibleRole::Group);
        assert!(
            esperar(|| contador(&c) == "1 de 3").await,
            "{}",
            contador(&c)
        );
        let ant = boton(&c, "Diapositiva anterior");
        let sig = boton(&c, "Diapositiva siguiente");
        assert!(!ant.is_sensitive() && sig.is_sensitive());
        assert_eq!(leyenda_visible(&c).as_deref(), Some("Corte sagital"));
        // La imagen llega del hilo de fondo, achicada al escenario.
        let foto = |c: &gtk::Widget| {
            todos(&pagina(c))
                .into_iter()
                .find_map(|w| w.downcast::<gtk::Picture>().ok())
        };
        assert!(esperar(|| foto(&c).and_then(|p| p.paintable()).is_some()).await);
        let t = foto(&c).unwrap().paintable().unwrap();
        assert!(t.intrinsic_width() <= 1000 * c.scale_factor(), "achicada");
        assert_eq!(
            foto(&c).unwrap().alternative_text().as_deref(),
            Some("Corte sagital")
        );
        // Las miniaturas, con su imagen.
        let minis: Vec<gtk::ToggleButton> = todos(&c)
            .into_iter()
            .filter_map(|w| w.downcast::<gtk::ToggleButton>().ok())
            .collect();
        assert_eq!(minis.len(), 3);
        assert!(minis[0].is_active());
        assert!(
            esperar(|| todos(&minis[2])
                .into_iter()
                .filter_map(|w| w.downcast::<gtk::Picture>().ok())
                .all(|p| p.paintable().is_some()))
            .await
        );
        capturar(&window, "032-lectura");
        // Siguiente con el botón; con teclado; con una miniatura.
        sig.emit_clicked();
        assert_eq!(contador(&c), "2 de 3");
        assert_eq!(leyenda_visible(&c), None, "sin leyenda");
        assert!(minis[1].is_active() && !minis[0].is_active());
        assert!(tecla(&c, gdk::Key::End));
        assert_eq!(contador(&c), "3 de 3");
        assert!(!sig.is_sensitive() && ant.is_sensitive());
        assert!(
            tecla(&c, gdk::Key::Right),
            "en el extremo no se va más allá"
        );
        assert_eq!(contador(&c), "3 de 3");
        assert!(tecla(&c, gdk::Key::Home));
        assert_eq!(contador(&c), "1 de 3");
        minis[2].set_active(true);
        assert_eq!(contador(&c), "3 de 3");
        assert_eq!(leyenda_visible(&c).as_deref(), Some("Vista lateral"));
        // El foco no se pierde al llegar al final con el botón.
        tecla(&c, gdk::Key::Left);
        sig.grab_focus();
        sig.emit_clicked();
        assert!(!sig.is_sensitive());
        assert!(c.has_focus(), "el foco pasa al carrusel");
        // SC-001: pasar de diapositiva, por tecla.
        let mut peor = Duration::ZERO;
        for k in 0..20 {
            let t0 = Instant::now();
            tecla(
                &c,
                if k % 2 == 0 {
                    gdk::Key::Left
                } else {
                    gdk::Key::Right
                },
            );
            peor = peor.max(t0.elapsed());
        }
        println!("peor tecla: {:.2} ms", peor.as_secs_f64() * 1e3);
        // Solo la vigente tiene widgets (después del fundido).
        let escenario = todos(&c)
            .into_iter()
            .find_map(|w| w.downcast::<gtk::Stack>().ok())
            .unwrap();
        assert!(esperar(|| !escenario.is_transition_running()).await);
        glib::timeout_future(Duration::from_millis(50)).await;
        let mut paginas = 0;
        let mut h = escenario.first_child();
        while let Some(x) = h {
            paginas += 1;
            h = x.next_sibling();
        }
        assert_eq!(paginas, 1, "una sola página en el escenario");
        // Copiar el embed (US3).
        minis[2].set_active(true);
        boton(&c, "Copiar embed").emit_clicked();
        let texto = c.clipboard().read_text_future().await.unwrap();
        assert_eq!(
            texto.as_deref(),
            Some("![[fotos ñ/foto-02.png|Vista lateral]]")
        );
        capturar(&window, "032-copiado");
        // Pantalla completa: una ventana propia, y al salir queda la diapositiva.
        let antes = gtk::Window::list_toplevels().len();
        boton(&c, "Pantalla completa (F)").emit_clicked();
        assert!(esperar(|| gtk::Window::list_toplevels().len() > antes).await);
        let completa = gtk::Window::list_toplevels()
            .into_iter()
            .filter_map(|w| w.downcast::<gtk::Window>().ok())
            .find(|w| w.has_css_class("orbital-carrusel-ventana"))
            .expect("ventana a pantalla completa");
        assert!(esperar(|| completa.is_fullscreen()).await, "fullscreen");
        let cc = carruseles(&completa).remove(0);
        assert_eq!(contador(&cc), "3 de 3");
        assert!(
            esperar(|| todos(&pagina(&cc))
                .into_iter()
                .find_map(|w| w.downcast::<gtk::Picture>().ok())
                .and_then(|p| p.paintable())
                .is_some())
            .await
        );
        glib::timeout_future(Duration::from_millis(300)).await;
        capturar(&completa, "032-pantalla-completa");
        tecla(&cc, gdk::Key::Left);
        assert_eq!(contador(&cc), "2 de 3");
        boton(&cc, "Salir de pantalla completa (Esc)").emit_clicked();
        assert!(esperar(|| !completa.is_visible()).await);
        assert_eq!(contador(&c), "2 de 3", "vuelve a la que quedó");
        // En Código el bloque es código.
        editor.set_view(View::Codigo);
        let b = &editor.0.buffer;
        assert!(
            b.text(&b.start_iter(), &b.end_iter(), true)
                .contains("```media-slider")
        );
        // Nada se escribió en la bóveda.
        assert_eq!(fs::read_to_string(rel.to_path(&root)).unwrap(), NOTA);
        window.destroy();
        let _ = fs::remove_dir_all(&root);
    });
}

#[test]
#[ignore = "requiere una sesión gráfica; tools/pruebas-gtk.sh"]
fn carrusel_con_cada_tipo() {
    adw::init().expect("Se necesita una sesión gráfica GTK");
    crate::style::apply();
    glib::MainContext::default().block_on(async {
        let nota = "```carrusel\nleyenda: debajo\nminiaturas: true\nposicionMiniaturas: derecha\n![[foto-00.png|Una foto]]\n![[animada.gif]]\n![[clip.mp4#t=5]]\n![[sonido.wav|Sonido]]\n![[informe.pdf#page=2]]\n![[borrada.png]]\ncolor: rojo\n```\n";
        let (root, vault, rel) = boveda("tipos", nota, 1);
        let editor = Editor::new();
        let window = ventana(&editor, 1000, 800);
        let abiertos = std::rc::Rc::new(std::cell::RefCell::new(Vec::<String>::new()));
        {
            let a = abiertos.clone();
            editor.0.reader.set_on_link(move |u| a.borrow_mut().push(u.to_owned()));
        }
        editor.load(vault.clone(), &vault.read_note(&rel).unwrap());
        editor.set_view(View::Lectura);
        editor.render_now();
        let vista = editor.0.reader.text_view().clone();
        assert!(esperar(|| carruseles(&vista).len() == 1).await);
        let c = carruseles(&vista).remove(0);
        assert!(esperar(|| contador(&c) == "1 de 6").await);
        // La opción inválida, como aviso en el texto.
        let buf = editor.0.reader.buffer();
        let texto = buf.text(&buf.start_iter(), &buf.end_iter(), true);
        assert!(texto.contains("⚠ Opción desconocida ignorada: «color»"), "{texto}");
        assert_eq!(leyenda_visible(&c).as_deref(), Some("Una foto"));
        // Los no-imagen tienen un marcador con su tipo en la miniatura.
        let marcadores: Vec<String> = todos(&c)
            .into_iter()
            .filter(|w| w.has_css_class("orbital-carrusel-marcador"))
            .flat_map(|m| todos(&m))
            .filter_map(|w| w.downcast::<gtk::Label>().ok())
            .map(|l| l.label().to_string())
            .collect();
        assert_eq!(marcadores, ["Video", "Audio", "PDF", "Falta"]);
        tecla(&c, gdk::Key::Right);
        assert!(
            todos(&pagina(&c))
                .into_iter()
                .any(|w| w.is::<gtk::Picture>()),
            "el GIF es una imagen"
        );
        tecla(&c, gdk::Key::Right);
        let p = pagina(&c);
        assert!(
            todos(&p).into_iter().any(|w| w.has_css_class("orbital-medio")),
            "video con su reproductor"
        );
        glib::timeout_future(Duration::from_millis(400)).await;
        capturar(&window, "032-video");
        tecla(&c, gdk::Key::Right);
        assert!(todos(&pagina(&c)).into_iter().any(|w| w.has_css_class("orbital-medio")));
        assert_eq!(leyenda_visible(&c).as_deref(), Some("Sonido"));
        tecla(&c, gdk::Key::Right);
        let pdf = todos(&pagina(&c))
            .into_iter()
            .filter_map(|w| w.downcast::<gtk::Button>().ok())
            .find(|b| b.label().as_deref() == Some("Abrir en el visor PDF"))
            .expect("botón del visor PDF");
        pdf.emit_clicked();
        assert_eq!(*abiertos.borrow(), ["informe.pdf#page=2"]);
        tecla(&c, gdk::Key::Right);
        assert!(
            todos(&pagina(&c))
                .into_iter()
                .filter_map(|w| w.downcast::<gtk::Label>().ok())
                .any(|l| l.label() == "Archivo inexistente: «borrada.png»")
        );
        glib::timeout_future(Duration::from_millis(400)).await;
        capturar(&window, "032-faltante");
        window.destroy();
        let _ = fs::remove_dir_all(&root);
    });
}

#[test]
#[ignore = "requiere una sesión gráfica; tools/pruebas-gtk.sh"]
fn carrusel_pase_automatico_y_carpeta() {
    adw::init().expect("Se necesita una sesión gráfica GTK");
    crate::style::apply();
    glib::MainContext::default().block_on(async {
        let nota =
            "```carrusel\ncarpeta: fotos ñ\nautoplay: true\nslideshowSpeed: 1\nmaximo: 4\n```\n";
        let (root, vault, rel) = boveda("pase", nota, 6);
        let editor = Editor::new();
        let window = ventana(&editor, 900, 700);
        editor.load(vault.clone(), &vault.read_note(&rel).unwrap());
        editor.set_view(View::Lectura);
        editor.render_now();
        let vista = editor.0.reader.text_view().clone();
        assert!(esperar(|| carruseles(&vista).len() == 1).await);
        let c = carruseles(&vista).remove(0);
        assert!(
            esperar(|| contador(&c) == "1 de 4").await,
            "{}",
            contador(&c)
        );
        let buf = editor.0.reader.buffer();
        let texto = buf.text(&buf.start_iter(), &buf.end_iter(), true);
        assert!(texto.contains("tiene 6 medios: entraron 4"), "{texto}");
        // Avanza solo.
        assert!(esperar(|| contador(&c) == "2 de 4").await, "avanza solo");
        // Interactuar lo pausa: no avanza más.
        tecla(&c, gdk::Key::Left);
        assert_eq!(contador(&c), "1 de 4");
        let pausa = boton(&c, "Reanudar el pase automático");
        glib::timeout_future(Duration::from_millis(1600)).await;
        assert_eq!(contador(&c), "1 de 4", "pausado");
        // Reanudar.
        pausa.emit_clicked();
        assert_eq!(
            pausa.tooltip_text().as_deref(),
            Some("Pausar el pase automático")
        );
        assert!(esperar(|| contador(&c) == "2 de 4").await);
        // Al pasar a Código (la vista se oculta) se detiene.
        editor.set_view(View::Codigo);
        glib::timeout_future(Duration::from_millis(100)).await;
        assert!(!c.is_mapped());
        let quieto = contador(&c);
        glib::timeout_future(Duration::from_millis(1600)).await;
        assert_eq!(contador(&c), quieto, "sin pase con la vista oculta");
        window.destroy();
        let _ = fs::remove_dir_all(&root);
    });
}

/// Una nota sin carrusel no decodifica ni crea nada del carrusel.
#[test]
#[ignore = "requiere una sesión gráfica; tools/pruebas-gtk.sh"]
fn sin_carrusel_no_prepara_nada() {
    adw::init().expect("Se necesita una sesión gráfica GTK");
    glib::MainContext::default().block_on(async {
        crate::imagenes::vaciar();
        let (root, vault, rel) = boveda("sin", "# Nada\n\n```rust\nfn main() {}\n```\n", 2);
        let editor = Editor::new();
        let window = ventana(&editor, 800, 600);
        editor.load(vault.clone(), &vault.read_note(&rel).unwrap());
        editor.set_view(View::Lectura);
        editor.render_now();
        glib::timeout_future(Duration::from_millis(500)).await;
        assert!(carruseles(&editor.0.reader.text_view().clone()).is_empty());
        assert_eq!(crate::imagenes::en_cache_total(), 0);
        window.destroy();
        let _ = fs::remove_dir_all(&root);
    });
}

/// Mide abrir una nota con un carrusel de 20 imágenes de 1600×1000: hasta ver el carrusel,
/// hasta la primera imagen y hasta las 20 miniaturas, y el mayor hueco del hilo de GTK. Con
/// `ORBITAL_MEDIR_SIN_CARRUSEL=1`, la misma nota con el bloque como código común.
#[test]
#[ignore = "medición del carrusel (032)"]
fn medir_lectura_carrusel() {
    adw::init().expect("Se necesita una sesión gráfica GTK");
    crate::style::apply();
    glib::MainContext::default().block_on(async {
        let sin = std::env::var_os("ORBITAL_MEDIR_SIN_CARRUSEL").is_some();
        let mut nota = String::from("# Veinte\n\n```");
        nota.push_str(if sin { "texto" } else { "carrusel" });
        nota.push_str("\nminiaturas: true\nvistaAmpliada: true\n");
        for k in 0..20 {
            nota.push_str(&format!("![[foto-{k:02}.png|Foto {k}]]\n"));
        }
        nota.push_str("```\n\nFin.\n");
        let (root, vault, rel) = boveda("medir", &nota, 20);
        let editor = Editor::new();
        let window = ventana(&editor, 1000, 900);
        glib::timeout_future(Duration::from_millis(500)).await;
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
            // El buffer de la vista cambia al volcar: se lo pide cada vez.
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
                "sin carrusel: volcado {:.0} ms, hueco {:.1} ms",
                t0.elapsed().as_secs_f64() * 1e3,
                hueco.get().as_secs_f64() * 1e3
            );
        } else {
            assert!(esperar(|| carruseles(&vista).len() == 1).await);
            let c = carruseles(&vista).remove(0);
            let visto = t0.elapsed();
            let foto = |c: &gtk::Widget| {
                todos(&pagina(c))
                    .into_iter()
                    .find_map(|w| w.downcast::<gtk::Picture>().ok())
                    .and_then(|p| p.paintable())
                    .is_some()
            };
            assert!(esperar(|| c.is_mapped() && foto(&c)).await);
            let primera = t0.elapsed();
            let minis = |c: &gtk::Widget| {
                todos(c)
                    .into_iter()
                    .filter_map(|w| w.downcast::<gtk::ToggleButton>().ok())
                    .flat_map(|b| todos(&b))
                    .filter_map(|w| w.downcast::<gtk::Picture>().ok())
                    .filter(|p| p.paintable().is_some())
                    .count()
            };
            assert!(esperar(|| minis(&c) == 20).await);
            let todas = t0.elapsed();
            let hueco_abrir = hueco.get();
            // Pasar 19 diapositivas por tecla.
            let mut peor = Duration::ZERO;
            hueco.set(Duration::ZERO);
            for _ in 0..19 {
                let t = Instant::now();
                tecla(&c, gdk::Key::Right);
                peor = peor.max(t.elapsed());
                glib::timeout_future(Duration::from_millis(30)).await;
            }
            println!(
                "carrusel de 20: visible {:.0} ms, primera imagen {:.0} ms, 20 miniaturas {:.0} ms, hueco al abrir {:.1} ms; peor tecla {:.2} ms, hueco al navegar {:.1} ms",
                visto.as_secs_f64() * 1e3,
                primera.as_secs_f64() * 1e3,
                todas.as_secs_f64() * 1e3,
                hueco_abrir.as_secs_f64() * 1e3,
                peor.as_secs_f64() * 1e3,
                hueco.get().as_secs_f64() * 1e3
            );
        }
        window.destroy();
        let _ = fs::remove_dir_all(&root);
    });
}
