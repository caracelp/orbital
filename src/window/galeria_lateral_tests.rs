//! Pruebas GTK en proceso de la galería lateral (034): con la barra cerrada no se calcula nada;
//! los grupos por encabezado, plegar, buscar, filtrar y el estado por nota; el visor y el foco
//! al cerrarlo; «Ir al embed»; la edición en vivo; el estado vacío. Con `ORBITAL_CAPTURA_DIR`,
//! capturas. `medir_galeria_lateral` mide (no corre en `tools/pruebas-gtk.sh`).

use super::*;
use crate::galeria_lateral::disposicion::Pieza;
use gtk::gdk;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

fn png(path: &Path, ancho: i32, alto: i32, color: [u8; 3]) {
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

const COLORES: [[u8; 3]; 5] = [
    [255, 105, 180],
    [90, 160, 255],
    [255, 170, 60],
    [80, 210, 170],
    [190, 120, 255],
];

async fn esperar(mut f: impl FnMut() -> bool) -> bool {
    for _ in 0..300 {
        if f() {
            return true;
        }
        glib::timeout_future(Duration::from_millis(20)).await;
    }
    false
}

fn app(nombre: &str) -> adw::Application {
    adw::init().expect("se necesita acceso a GTK");
    crate::style::apply();
    let app = adw::Application::builder()
        .application_id(format!("io.github.caracelp.Orbital.Galeria034.{nombre}"))
        .flags(gio::ApplicationFlags::NON_UNIQUE)
        .build();
    app.register(None::<&gio::Cancellable>).unwrap();
    app
}

/// Una bóveda con un nombre no ASCII bajo `base` (o el temporal), con las notas dadas y
/// `fotos` imágenes en `wiki/fotos ñ/`.
fn boveda(nombre: &str, notas: &[(&str, &str)], fotos: usize, base: Option<&Path>) -> PathBuf {
    boveda_con(nombre, notas, fotos, base, 1)
}

/// Como [`boveda`], con las fotos `escala` veces más grandes.
fn boveda_con(
    nombre: &str,
    notas: &[(&str, &str)],
    fotos: usize,
    base: Option<&Path>,
    escala: i32,
) -> PathBuf {
    let raiz = base.map_or_else(std::env::temp_dir, Path::to_path_buf);
    let root = raiz.join(format!(
        "orbital-034-{nombre}-Bóveda ñandú-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("wiki/fotos ñ")).unwrap();
    for k in 0..fotos {
        png(
            &root.join(format!("wiki/fotos ñ/foto-{k}.png")),
            (320 + (k as i32 % 3) * 80) * escala,
            240 * escala,
            COLORES[k % COLORES.len()],
        );
    }
    png(&root.join("wiki/esquema-uno.png"), 1200, 900, COLORES[0]);
    png(&root.join("wiki/esquema-dos.png"), 900, 1200, COLORES[1]);
    std::fs::write(root.join("wiki/clip.mp4"), b"no es un video").unwrap();
    for (ruta, texto) in notas {
        std::fs::write(root.join(ruta), texto).unwrap();
    }
    root
}

async fn ventana(app: &adw::Application, root: &Path) -> Window {
    let window = Window::new(app);
    window.set_persist(false);
    window.0.window.set_default_size(1440, 900);
    let v = orbital_vault::open(root).unwrap();
    let tree = build_tree(v.entries());
    window.show_vault(v, tree, orbital_vault::NoteMeta::default());
    window.0.window.present();
    glib::timeout_future(Duration::from_millis(200)).await;
    window
}

async fn abrir(window: &Window, ruta: &str) {
    window.show_path(ruta);
    let w = window.clone();
    assert!(
        esperar(|| w
            .0
            .code_editor
            .current_path()
            .map(|p| p.to_string())
            .as_deref()
            == Some(ruta))
        .await,
        "se abre {ruta}"
    );
}

const NOTA_A: &str = "# Esquemas\n![[esquema-uno.png]] ![[esquema-dos.png]]\n\nTexto.\n\n# Fotos\n![](fotos%20ñ/foto-0.png) ![[clip.mp4]]\n![[borrada.png]]\n\n## Viaje\n```carrusel\n![[fotos ñ/foto-1.png|Playa]]\n```\n";

async fn guardar(window: &Window, nombre: &str) {
    guardar_ventana(&window.0.window.clone().upcast(), nombre).await;
}

async fn guardar_ventana(ventana: &gtk::Window, nombre: &str) {
    let Some(dir) = std::env::var_os("ORBITAL_CAPTURA_DIR") else {
        return;
    };
    // Que se dibuje lo último que cambió (y lleguen las miniaturas).
    glib::timeout_future(Duration::from_millis(400)).await;
    let (ancho, alto) = (ventana.width(), ventana.height());
    let pintable = gtk::WidgetPaintable::new(Some(ventana));
    let snap = gtk::Snapshot::new();
    pintable.snapshot(&snap, f64::from(ancho), f64::from(alto));
    let node = snap.to_node().expect("cuadro");
    let png = Path::new(&dir).join(format!("{nombre}.png"));
    ventana
        .renderer()
        .unwrap()
        .render_texture(&node, None)
        .save_to_png(&png)
        .unwrap();
    println!("Captura: {}", png.display());
}

fn visor() -> Option<gtk::Window> {
    gtk::Window::list_toplevels()
        .into_iter()
        .filter_map(|w| w.downcast::<gtk::Window>().ok())
        .find(|w| w.has_css_class("orbital-carrusel-ventana") && w.is_visible())
}

/// US1–US3: barra cerrada sin costo, grupos, plegar, buscar, filtrar, estado por nota, visor,
/// «Ir al embed», edición en vivo y estado vacío.
#[test]
#[ignore = "requiere GTK"]
fn galeria_lateral_grupos_busqueda_visor_y_embed() {
    let app = app("Grupos");
    glib::MainContext::default().block_on(async {
        let root = boveda(
            "grupos",
            &[
                ("wiki/a.md", NOTA_A),
                ("wiki/b.md", "Intro ![[fotos ñ/foto-2.png]]\n"),
                ("wiki/c.md", "# Solo texto\n\nNada que ver.\n"),
            ],
            3,
            None,
        );
        let window = ventana(&app, &root).await;
        // Barra cerrada: cambiar de nota no arma ninguna lista ni crea el panel.
        window.0.right_split.set_show_sidebar(false);
        let antes = galeria_lateral::PEDIDOS.load(Ordering::Relaxed);
        abrir(&window, "wiki/a.md").await;
        abrir(&window, "wiki/b.md").await;
        window.0.right_split.set_show_sidebar(true);
        abrir(&window, "wiki/a.md").await;
        glib::timeout_future(Duration::from_millis(300)).await;
        assert_eq!(galeria_lateral::PEDIDOS.load(Ordering::Relaxed), antes);
        assert!(
            window.0.panel.galeria().is_none(),
            "el panel no existe hasta elegirlo"
        );

        // Elegir el modo Galería (con el atajo).
        gio::prelude::ActionGroupExt::activate_action(&window.0.window, "galeria-lateral", None);
        let g = window.0.panel.galeria().expect("panel creado");
        assert!(esperar(|| g.pagina() == "grilla" && !g.vivas().is_empty()).await);
        assert_eq!(
            g.piezas(),
            [
                Pieza::Titulo(0),
                Pieza::Mosaico(0),
                Pieza::Mosaico(1),
                Pieza::Titulo(1),
                Pieza::Mosaico(2),
                Pieza::Mosaico(3),
                Pieza::Mosaico(4),
                Pieza::Titulo(2),
                Pieza::Mosaico(5),
            ],
            "Esquemas, Fotos y el carrusel de Viaje"
        );
        assert_eq!(g.resumen(), "6 medios en 3 grupos");
        assert!(
            esperar(|| g.con_textura() >= 4).await,
            "miniaturas de las imágenes"
        );
        guardar(&window, "034-panel").await;

        // Plegar un grupo (clic o Enter en su título) y todos con el menú.
        g.activar(Pieza::Titulo(0));
        assert!(!g.piezas().contains(&Pieza::Mosaico(0)), "Esquemas plegado");
        g.activar(Pieza::Titulo(0));
        g.accion("plegar-todo", None, None);
        assert_eq!(
            g.piezas(),
            [Pieza::Titulo(0), Pieza::Titulo(1), Pieza::Titulo(2)]
        );
        guardar(&window, "034-plegado").await;
        g.accion("desplegar-todo", None, None);
        assert_eq!(g.piezas().len(), 9);

        // Buscar: por encabezado (con tildes o sin ellas) y por nombre.
        g.buscar("esquemas");
        assert_eq!(g.resumen(), "2 de 6 medios en 1 grupo");
        g.buscar("playa");
        assert_eq!(
            g.resumen(),
            "Ninguno de los 6 medios coincide",
            "la leyenda no cuenta"
        );
        g.buscar("viaje");
        assert_eq!(
            g.piezas(),
            [Pieza::Titulo(2), Pieza::Mosaico(5)],
            "el carrusel de Viaje"
        );
        g.buscar("foto");
        assert_eq!(g.resumen(), "4 de 6 medios en 2 grupos");
        guardar(&window, "034-busqueda").await;
        g.filtrar("videos");
        assert!(esperar(|| g.piezas() == [Pieza::Titulo(1), Pieza::Mosaico(3)]).await);
        g.filtrar("todo");
        g.buscar("");

        // Otra nota y de vuelta: la búsqueda es por nota.
        g.buscar("uno");
        abrir(&window, "wiki/b.md").await;
        assert!(esperar(|| g.ruta().as_deref() == Some("wiki/b.md")).await);
        assert_eq!(g.piezas(), [Pieza::Titulo(0), Pieza::Mosaico(0)]);
        abrir(&window, "wiki/a.md").await;
        assert!(esperar(|| g.ruta().as_deref() == Some("wiki/a.md")).await);
        assert_eq!(g.piezas(), [Pieza::Titulo(0), Pieza::Mosaico(0)]);
        g.buscar("");

        // Visor: clic en un mosaico; al cerrar, el foco vuelve a la grilla.
        g.activar(Pieza::Mosaico(1));
        assert!(esperar(|| visor().is_some()).await, "se abre el visor");
        guardar_ventana(&visor().unwrap(), "034-visor").await;
        visor().unwrap().close();
        assert!(esperar(|| visor().is_none()).await);
        assert_eq!(g.activo(), Some(Pieza::Mosaico(1)));

        // «Ir al embed» con Ctrl+Enter: el cursor del editor va al renglón del embed.
        g.activar(Pieza::Mosaico(4));
        visor().unwrap().close();
        assert!(esperar(|| visor().is_none()).await);
        g.pulsar(gdk::Key::Return, gdk::ModifierType::CONTROL_MASK);
        assert_eq!(window.0.code_editor.cursor_line(), 8, "![[borrada.png]]");
        assert!(
            window
                .0
                .code_editor
                .buffer()
                .tag_table()
                .lookup("bloque-resaltado")
                .is_some()
        );
        // Un medio de bloque lleva al bloque.
        g.activar(Pieza::Mosaico(5));
        visor().unwrap().close();
        assert!(esperar(|| visor().is_none()).await);
        g.pulsar(gdk::Key::Return, gdk::ModifierType::CONTROL_MASK);
        assert_eq!(window.0.code_editor.cursor_line(), 11, "```carrusel");

        // Editar: agregar un embed se ve después de la pausa, sin cambiar de nota.
        let b = window.0.code_editor.buffer();
        b.insert(&mut b.end_iter(), "\n# Nuevo\n![[fotos ñ/foto-2.png]]\n");
        assert!(esperar(|| g.resumen() == "7 medios en 4 grupos").await);

        // Sin medios: el estado vacío.
        abrir(&window, "wiki/c.md").await;
        assert!(esperar(|| g.pagina() == "vacio").await);
        guardar(&window, "034-vacia").await;
        window.0.window.destroy();
        let _ = std::fs::remove_dir_all(root);
    });
}

/// Teclado: Tab entra a un solo elemento, las flechas recorren títulos y mosaicos, el título
/// fijo aparece al desplazar.
#[test]
#[ignore = "requiere GTK"]
fn galeria_lateral_teclado_y_titulo_fijo() {
    let app = app("Teclado");
    glib::MainContext::default().block_on(async {
        let mut texto = String::new();
        for s in 0..8 {
            texto.push_str(&format!("# Sección {s}\n"));
            for k in 0..6 {
                texto.push_str(&format!("![[fotos ñ/foto-{}.png]]\n", (s * 6 + k) % 4));
            }
        }
        let root = boveda("teclado", &[("wiki/larga.md", &texto)], 4, None);
        let window = ventana(&app, &root).await;
        abrir(&window, "wiki/larga.md").await;
        window.0.panel.set_modo("galeria");
        let g = window.0.panel.galeria().unwrap();
        assert!(esperar(|| !g.vivas().is_empty()).await);
        assert_eq!(
            g.activo(),
            Some(Pieza::Titulo(0)),
            "Tab entra al primer título"
        );
        g.pulsar(gdk::Key::Down, gdk::ModifierType::empty());
        assert_eq!(g.activo(), Some(Pieza::Mosaico(0)));
        g.pulsar(gdk::Key::Right, gdk::ModifierType::empty());
        assert_eq!(g.activo(), Some(Pieza::Mosaico(1)));
        g.pulsar(gdk::Key::Down, gdk::ModifierType::empty());
        assert_eq!(g.activo(), Some(Pieza::Mosaico(4)), "tres columnas");
        g.pulsar(gdk::Key::Down, gdk::ModifierType::empty());
        assert_eq!(g.activo(), Some(Pieza::Titulo(1)));
        g.pulsar(gdk::Key::End, gdk::ModifierType::empty());
        assert_eq!(g.activo(), Some(Pieza::Mosaico(47)));
        // Al llegar al final, el título del grupo que queda arriba de la vista se ve fijo
        // (con una pantalla de 577 px, el de la sección 5).
        assert!(
            esperar(|| g
                .titulo_fijo()
                .is_some_and(|t| t.starts_with("Sección ") && !t.starts_with("Sección 0")))
            .await,
            "título fijo: {:?}, desplazamiento {} de {} (página {})",
            g.titulo_fijo(),
            g.scroller().vadjustment().value(),
            g.scroller().vadjustment().upper(),
            g.scroller().vadjustment().page_size()
        );
        assert!(
            g.vivas().len() < 48,
            "solo lo visible y un margen: {}",
            g.vivas().len()
        );
        guardar(&window, "034-titulo-fijo").await;
        g.pulsar(gdk::Key::Home, gdk::ModifierType::empty());
        assert_eq!(g.activo(), Some(Pieza::Titulo(0)));
        // Tamaños fijos.
        g.accion("tamano", None, Some(&"grande".to_variant()));
        assert!(esperar(|| !g.vivas().is_empty()).await);
        guardar(&window, "034-grandes").await;
        window.0.window.destroy();
        let _ = std::fs::remove_dir_all(root);
    });
}

/// SC-003: «Ir al embed» en una nota de 200 KiB con embeds al principio, en el medio y al final,
/// en Código y en Lectura, también después de agregar texto arriba.
#[test]
#[ignore = "requiere GTK"]
fn galeria_lateral_ir_al_embed_en_nota_grande() {
    let app = app("Embed");
    glib::MainContext::default().block_on(async {
        let relleno = "Un párrafo de relleno con algo de texto para que la nota pese. ".repeat(4);
        let mut texto = String::from("![[esquema-uno.png]]\n");
        let mut lineas = vec![1];
        while texto.len() < 100 * 1024 {
            texto.push_str(&relleno);
            texto.push_str("\n\n");
        }
        texto.push_str("# Medio\n![[esquema-dos.png]]\n");
        lineas.push(texto.matches('\n').count());
        while texto.len() < 200 * 1024 {
            texto.push_str(&relleno);
            texto.push_str("\n\n");
        }
        texto.push_str("# Final\n![[fotos ñ/foto-0.png]]\n");
        lineas.push(texto.matches('\n').count());
        let root = boveda("embed", &[("wiki/grande.md", &texto)], 1, None);
        let window = ventana(&app, &root).await;
        abrir(&window, "wiki/grande.md").await;
        window.0.panel.set_modo("galeria");
        let g = window.0.panel.galeria().unwrap();
        assert!(
            esperar(|| g.resumen() == "3 medios en 3 grupos").await,
            "{}",
            g.resumen()
        );
        for vista in [crate::editor::View::Codigo, crate::editor::View::Lectura] {
            window.0.code_editor.set_view(vista);
            glib::timeout_future(Duration::from_millis(300)).await;
            for (k, linea) in lineas.iter().enumerate() {
                g.activar(Pieza::Mosaico(k));
                visor().unwrap().close();
                assert!(esperar(|| visor().is_none()).await);
                g.pulsar(gdk::Key::Return, gdk::ModifierType::CONTROL_MASK);
                assert_eq!(
                    window.0.code_editor.cursor_line() as usize,
                    *linea,
                    "{vista:?} {k}"
                );
            }
        }
        // Texto nuevo arriba, sin esperar a que la lista se rearme: el embed se busca cerca.
        window.0.code_editor.set_view(crate::editor::View::Codigo);
        let b = window.0.code_editor.buffer();
        b.insert(&mut b.start_iter(), "Nuevo\n\n");
        g.pulsar(gdk::Key::Return, gdk::ModifierType::CONTROL_MASK);
        assert_eq!(window.0.code_editor.cursor_line() as usize, lineas[2] + 2);
        window.0.window.destroy();
        let _ = std::fs::remove_dir_all(root);
    });
}

/// Mide (release): cambiar de nota con la barra cerrada, con otro modo y con la galería; abrir
/// la galería en una nota de 200 medios (primer lote visible y con miniaturas) y el mayor hueco
/// del hilo de GTK al abrir y al desplazar. `ORBITAL_MEDIR_DIR=<carpeta>` para la bóveda (si no,
/// el temporal).
#[test]
#[ignore = "medición; requiere GTK"]
fn medir_galeria_lateral() {
    let app = app("Medir");
    glib::MainContext::default().block_on(async {
        let base = std::env::var_os("ORBITAL_MEDIR_DIR").map(PathBuf::from);
        let mut grande = String::new();
        for s in 0..20 {
            grande.push_str(&format!("# Capítulo {s}\n\nTexto del capítulo.\n\n"));
            for k in 0..10 {
                grande.push_str(&format!("![[fotos ñ/foto-{}.png]] ", s * 10 + k));
            }
            grande.push('\n');
        }
        let mut notas: Vec<(String, String)> = vec![("wiki/grande.md".into(), grande)];
        for n in 0..10 {
            notas.push((
                format!("wiki/nota-{n}.md"),
                format!("# Nota {n}\n\n![[fotos ñ/foto-{n}.png]]\n\n{}", "Texto. ".repeat(400)),
            ));
        }
        let refs: Vec<(&str, &str)> = notas.iter().map(|(a, b)| (a.as_str(), b.as_str())).collect();
        let root = boveda_con("medir", &refs, 200, base.as_deref(), 3);
        let window = ventana(&app, &root).await;
        // El mayor hueco sin volver al bucle de GTK (reloj de 1 ms).
        let hueco = Rc::new(Cell::new(0f64));
        let ultimo = Rc::new(Cell::new(Instant::now()));
        {
            let (h, u) = (hueco.clone(), ultimo.clone());
            glib::timeout_add_local(Duration::from_millis(1), move || {
                let ahora = Instant::now();
                h.set(h.get().max((ahora - u.get()).as_secs_f64() * 1000.0));
                u.set(ahora);
                glib::ControlFlow::Continue
            });
        }
        // Abrir la galería con la nota de 200 medios.
        // Primero, con la caché de miniaturas vacía (fotos de 1200×800).
        window.0.panel.set_modo("propiedades");
        abrir(&window, "wiki/grande.md").await;
        glib::timeout_future(Duration::from_millis(500)).await;
        let texto = window.0.code_editor.text();
        let vault = window.0.vault.borrow().clone().unwrap();
        let rel = RelPath::new("wiki/grande.md").unwrap();
        let t = Instant::now();
        let mut m = orbital_vault::medios_nota::extraer(&texto);
        let t_extraer = t.elapsed().as_secs_f64() * 1000.0;
        vault.resolver_medios_nota(&rel, &mut m);
        println!(
            "Lista de 200 medios (fuera del hilo de GTK): extraer {t_extraer:.2} ms, extraer y resolver {:.2} ms",
            t.elapsed().as_secs_f64() * 1000.0
        );
        hueco.set(0.0);
        ultimo.set(Instant::now());
        let t = Instant::now();
        window.0.panel.set_modo("galeria");
        let g = window.0.panel.galeria().unwrap();
        assert!(esperar(|| g.ruta().as_deref() == Some("wiki/grande.md") && !g.vivas().is_empty()).await);
        let visible = t.elapsed().as_secs_f64() * 1000.0;
        // Las miniaturas que se ven en la pantalla.
        let sw = g.scroller();
        let en_pantalla = || {
            let alto = sw.vadjustment().page_size();
            g.piezas().len().min(((alto / 98.0).ceil() as usize + 1) * 3)
        };
        let objetivo = en_pantalla().min(15);
        assert!(esperar(|| g.con_textura() >= objetivo).await);
        let con_miniaturas = t.elapsed().as_secs_f64() * 1000.0;
        println!(
            "ABRIR LA GALERÍA (200 medios): visible {visible:.0} ms, {objetivo} miniaturas {con_miniaturas:.0} ms, mayor hueco {:.1} ms, mosaicos vivos {}",
            hueco.get(),
            g.vivas().len()
        );
        guardar(&window, "034-grande").await;
        // Desplazar hasta el final, una pantalla por cuadro.
        glib::timeout_future(Duration::from_millis(800)).await;
        hueco.set(0.0);
        ultimo.set(Instant::now());
        let adj = sw.vadjustment();
        while adj.value() + adj.page_size() < adj.upper() - 1.0 {
            adj.set_value(adj.value() + adj.page_size());
            glib::timeout_future(Duration::from_millis(16)).await;
        }
        glib::timeout_future(Duration::from_millis(200)).await;
        println!(
            "DESPLAZAR hasta el final: mayor hueco {:.1} ms, mosaicos vivos {}",
            hueco.get(),
            g.vivas().len()
        );
        guardar(&window, "034-grande").await;
        // Desplazar hasta el final, una pantalla por cuadro.
        glib::timeout_future(Duration::from_millis(800)).await;
        hueco.set(0.0);
        ultimo.set(Instant::now());
        let adj = sw.vadjustment();
        while adj.value() + adj.page_size() < adj.upper() - 1.0 {
            adj.set_value(adj.value() + adj.page_size());
            glib::timeout_future(Duration::from_millis(16)).await;
        }
        glib::timeout_future(Duration::from_millis(200)).await;
        println!(
            "DESPLAZAR hasta el final: mayor hueco {:.1} ms, mosaicos vivos {}",
            hueco.get(),
            g.vivas().len()
        );
        // Cambiar de nota: desde `show_path` hasta que el editor tiene la nota y, con la
        // galería a la vista, hasta que la galería la muestra (reloj de 1 ms).
        let medir_cambios = async |nombre: &str, w: &Window, galeria: bool| {
            let mut editor = Vec::new();
            let mut panel = Vec::new();
            for vuelta in 0..3 {
                for n in 0..10 {
                    let ruta = format!("wiki/nota-{n}.md");
                    let t = Instant::now();
                    w.show_path(&ruta);
                    while w.0.code_editor.current_path().map(|p| p.to_string()).as_deref()
                        != Some(ruta.as_str())
                    {
                        glib::timeout_future(Duration::from_millis(1)).await;
                    }
                    let te = t.elapsed().as_secs_f64() * 1000.0;
                    if galeria {
                        let g = w.0.panel.galeria().unwrap();
                        while g.ruta().as_deref() != Some(ruta.as_str()) {
                            glib::timeout_future(Duration::from_millis(1)).await;
                        }
                    }
                    let tp = t.elapsed().as_secs_f64() * 1000.0;
                    glib::timeout_future(Duration::from_millis(150)).await;
                    if vuelta > 0 {
                        editor.push(te);
                        panel.push(tp);
                    }
                }
            }
            editor.sort_by(f64::total_cmp);
            panel.sort_by(f64::total_cmp);
            let m = |v: &Vec<f64>| (v[v.len() / 2], v[v.len() - 1]);
            let (em, ex) = m(&editor);
            print!("CAMBIAR DE NOTA ({nombre}): editor con la nota mediana {em:.1} ms, máx {ex:.1} ms");
            if galeria {
                let (pm, px) = m(&panel);
                print!("; galería con la nota mediana {pm:.1} ms, máx {px:.1} ms");
            }
            println!();
        };
        window.0.right_split.set_show_sidebar(false);
        let antes = galeria_lateral::PEDIDOS.load(Ordering::Relaxed);
        medir_cambios("barra cerrada", &window, false).await;
        println!(
            "Listas armadas con la barra cerrada: {}",
            galeria_lateral::PEDIDOS.load(Ordering::Relaxed) - antes
        );
        window.0.right_split.set_show_sidebar(true);
        window.0.panel.set_modo("propiedades");
        medir_cambios("barra abierta, Propiedades", &window, false).await;
        window.0.panel.set_modo("galeria");
        medir_cambios("barra abierta, Galería", &window, true).await;
        // El costo en el hilo de GTK de pedir la lista (copiar el texto y lanzar el hilo).
        let mut ms = Vec::new();
        for _ in 0..20 {
            let t = Instant::now();
            window.refresh_galeria_lateral();
            ms.push(t.elapsed().as_secs_f64() * 1000.0);
            glib::timeout_future(Duration::from_millis(30)).await;
        }
        ms.sort_by(f64::total_cmp);
        println!(
            "Pedir la lista (hilo de GTK, nota de 3 KB): mediana {:.3} ms, máx {:.3} ms",
            ms[ms.len() / 2],
            ms[ms.len() - 1]
        );
        window.0.window.destroy();
        let _ = std::fs::remove_dir_all(root);
    });
}
