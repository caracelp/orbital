//! Prueba con WebKit real: ejecutar explícitamente en una sesión gráfica.
use super::*;
use crate::editor::{Editor, View};
use std::{fs, sync::Arc, time::Duration};

async fn eval(web: &webkit6::WebView, script: &str) -> String {
    web.evaluate_javascript_future(script, Some(WORLD), None)
        .await
        .expect("JavaScript de prueba")
        .to_str()
        .to_string()
}

async fn ready(web: &webkit6::WebView) {
    for _ in 0..100 {
        if !web.is_loading()
            && web
                .evaluate_javascript_future("typeof window.orbital", Some(WORLD), None)
                .await
                .is_ok_and(|v| v.to_str() == "object")
        {
            return;
        }
        glib::timeout_future(Duration::from_millis(50)).await;
    }
    panic!("WebKit no cargó el script aislado en 5 segundos");
}

async fn wait_dom(
    web: &webkit6::WebView,
    expression: &str,
    expected: &str,
    diagnostics: &RefCell<Vec<String>>,
) {
    // AT-SPI acepta la activación antes de que GTK termine la animación y emita clicked.
    for _ in 0..60 {
        if eval(web, expression).await == expected {
            return;
        }
        glib::timeout_future(Duration::from_millis(50)).await;
    }
    panic!("No se cumplió {expression} == {expected}. Estado: {}. Señales/errores: {:?}",
        eval(web, "JSON.stringify({html:document.body.innerHTML,selection:String(getSelection()),focus:document.hasFocus(),active:document.activeElement?.tagName})").await,
        diagnostics.borrow());
}

fn observe_clicks(widget: &gtk::Widget, diagnostics: &Rc<RefCell<Vec<String>>>) {
    if let Some(button) = widget.downcast_ref::<gtk::Button>() {
        let name = button.tooltip_text().unwrap_or_default().to_string();
        let diagnostics = diagnostics.clone();
        button.connect_clicked(move |_| diagnostics.borrow_mut().push(format!("clicked: {name}")));
    }
    let mut child = widget.first_child();
    while let Some(widget) = child {
        observe_clicks(&widget, diagnostics);
        child = widget.next_sibling();
    }
}

async fn atspi(args: &[&str]) -> String {
    let args: Vec<String> = args.iter().map(|s| (*s).to_owned()).collect();
    let pid = std::process::id().to_string();
    let result = gio::spawn_blocking(move || {
        std::process::Command::new("python3")
            .args(["-I", "tools/atspi.py"])
            .args(args)
            .env("ORBITAL_PID", pid)
            .output()
            .expect("ejecutar AT-SPI")
    })
    .await
    .unwrap();
    assert!(
        result.status.success(),
        "AT-SPI: {} {}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    let output = String::from_utf8_lossy(&result.stdout).into_owned();
    if output.starts_with("Activé") {
        print!("{output}");
    }
    output
}

#[test]
#[ignore = "requiere una sesión gráfica; ejecutar tools/verificar-us4.sh"]
fn us4_webkit() {
    glib::set_prgname(Some("orbital"));
    glib::set_application_name("Orbital");
    adw::init().expect("Se necesita acceso a una sesión gráfica GTK");
    let context = glib::MainContext::default();
    context.block_on(async {
        let root = std::env::temp_dir().join(format!("orbital-us4-{}", std::process::id()));
        fs::create_dir(&root).expect("directorio temporal único");
        fs::create_dir(root.join("wiki")).unwrap();
        let path = orbital_vault::RelPath::new("wiki/prueba.html").unwrap();
        let source = "<!doctype html><html><head>\n<script type=\"application/yaml\">\ntitle: Prueba\n</script>\n<style>p { margin: 1em; }</style>\n</head><body><p>Original</p></body></html>";
        fs::write(root.join("wiki/prueba.html"), source).unwrap();
        let vault = Arc::new(orbital_vault::open(&root).unwrap());
        let editor = Editor::new();
        let window = gtk::Window::builder().title("Orbital — prueba aislada US4")
            .default_width(1000).default_height(700).child(editor.widget()).build();
        window.present();
        editor.load(vault.clone(), &vault.read_note(&path).unwrap());
        assert_eq!(editor.0.mode.get(), View::Visual);
        let web = editor.0.visual.web();
        ready(&web).await;
        // Escribir a través del motor de edición, sin teclas sintéticas ni otra ventana.
        eval(&web, "getSelection().selectAllChildren(document.querySelector('p')); document.execCommand('insertText', false, 'Editado visual')").await;
        glib::timeout_future(Duration::from_millis(100)).await;
        assert!(editor.has_pending(), "cuenta cambios antes del debounce de 300 ms");
        glib::timeout_future(Duration::from_millis(2500)).await;
        let saved = fs::read_to_string(root.join("wiki/prueba.html")).unwrap();
        assert!(saved.contains("Editado visual"));
        assert_eq!(saved.split("<body>").next(), source.split("<body>").next());
        assert!(root.join(".orbital/respaldo").exists());
        println!("OK: escritura visual, autoguardado, respaldo y head intacto");

        eval(&web, "getSelection().selectAllChildren(document.querySelector('p')); window.orbital.run('color','o-color-rosa')").await;
        assert_eq!(eval(&web, "!!document.querySelector('.o-color-rosa')").await, "true");
        eval(&web, "document.execCommand('undo')").await;
        assert_eq!(eval(&web, "!!document.querySelector('.o-color-rosa')").await, "false");
        eval(&web, "document.execCommand('redo')").await;
        assert_eq!(eval(&web, "!!document.querySelector('.o-color-rosa')").await, "true");
        eval(&web, "getSelection().selectAllChildren(document.querySelector('p')); window.orbital.run('columnas','o-columnas-2')").await;
        glib::timeout_future(Duration::from_millis(100)).await;
        editor.flush().await;
        assert!(!editor.has_pending());
        let saved = fs::read_to_string(root.join("wiki/prueba.html")).unwrap();
        assert!(saved.contains("o-color-rosa"));
        assert!(saved.contains("o-columnas-2"));
        assert!(!saved.contains("style="));
        assert_eq!(saved.matches("<style data-orbital>").count(), 1);
        let without_style = saved.replace(&format!("<style data-orbital>{}</style>\n", orbital_vault::html_edit::orbital_style(&saved).unwrap()), "");
        assert_eq!(without_style.split("<body>").next(), source.split("<body>").next());
        println!("OK: color, columnas, deshacer/rehacer, clases y estilos únicos");

        editor.set_view(View::Codigo);
        glib::timeout_future(Duration::from_millis(100)).await;
        assert_eq!(editor.0.mode.get(), View::Codigo);
        let code = saved.replace("Editado visual", "Editado código");
        editor.0.buffer.set_text(&code);
        editor.set_view(View::Visual);
        ready(&web).await;
        assert!(eval(&web, "document.body.textContent").await.contains("Editado código"));
        println!("OK: Visual → Código → Visual");

        editor.set_view(View::Codigo);
        glib::timeout_future(Duration::from_millis(100)).await;
        editor.0.buffer.set_text(&source.replace("<p>Original</p>", "<script>document.body.textContent='PELIGRO'</script><p>Seguro</p>"));
        editor.set_view(View::Visual);
        glib::timeout_future(Duration::from_millis(100)).await;
        assert_eq!(editor.0.mode.get(), View::Codigo);
        assert!(!editor.switcher().toggle_by_name("visual").unwrap().is_enabled());
        editor.0.buffer.set_text(source);
        editor.set_view(View::Visual);
        ready(&web).await;
        assert_eq!(editor.0.mode.get(), View::Visual);
        println!("OK: scripts desactivan Visual; quitarlos permite volver");
        // La barra real se acciona por accesibilidad, limitada al PID de esta prueba.
        let diagnostics = Rc::new(RefCell::new(Vec::new()));
        let errors = diagnostics.clone();
        editor.set_on_error(move |message| errors.borrow_mut().push(message.to_owned()));
        observe_clicks(editor.widget().upcast_ref(), &diagnostics);
        let tree = atspi(&["arbol"]).await;
        let artifacts = Path::new(env!("CARGO_MANIFEST_DIR")).join("target/us4-validacion");
        fs::create_dir_all(&artifacts).unwrap();
        fs::write(artifacts.join("accesibilidad.txt"), &tree).unwrap();
        let mut missing = Vec::new();
        for name in ["Negrita", "Cursiva", "Subrayado", "Tachado", "Tipo de bloque",
            "Lista con viñetas", "Lista numerada", "Enlace", "Tabla", "Imagen de la bóveda",
            "Separador", "Colores", "Alineación", "Columnas", "Estilo del bloque", "Deshacer", "Rehacer"] {
            // GTK puede tomar el nombre del texto visible del botón en lugar
            // de la propiedad Label. Ambos nombres identifican el mismo control.
            let visible_label = match name {
                "Tipo de bloque" => Some("[button] 'Párrafo'"),
                "Estilo del bloque" => Some("[button] 'Estilo'"),
                _ => None,
            };
            if !tree.contains(name) && !visible_label.is_some_and(|label| tree.contains(label)) {
                missing.push(name);
            }
        }
        assert!(missing.is_empty(), "Controles no encontrados: {missing:?}\nÁrbol AT-SPI completo:\n{tree}");
        eval(&web, "getSelection().selectAllChildren(document.querySelector('p'))").await;
        atspi(&["activar", "*", "=Negrita (Ctrl+B)"]).await;
        wait_dom(&web, "!!document.querySelector('b,strong')", "true", &diagnostics).await;
        atspi(&["activar", "*", "=Deshacer (Ctrl+Z)"]).await;
        wait_dom(&web, "!!document.querySelector('b,strong')", "false", &diagnostics).await;
        atspi(&["activar", "*", "=Tabla"]).await;
        atspi(&["activar", "*", "=Insertar tabla"]).await;
        wait_dom(&web, "document.querySelectorAll('table').length", "1", &diagnostics).await;
        println!("OK: nombres accesibles y acciones de negrita, deshacer y tabla por AT-SPI");
        editor.flush().await;
        let paintable = gtk::WidgetPaintable::new(Some(&window));
        let snapshot = gtk::Snapshot::new();
        paintable.snapshot(&snapshot, f64::from(window.width()), f64::from(window.height()));
        let node = snapshot.to_node().expect("captura de la ventana de prueba");
        let texture = window.renderer().unwrap().render_texture(&node, None);
        texture.save_to_png(artifacts.join("visual.png")).unwrap();
        println!("OK: captura y árbol accesible en {}", artifacts.display());
        window.close();
        fs::remove_dir_all(root).unwrap();
    });
}

/// US5 de la 003 (T027): transclusiones en una nota `.html` abierta en Visual. Ejecutar con
/// `cargo test --bin orbital us5_webkit_embeds -- --ignored --nocapture --test-threads=1`.
#[test]
#[ignore = "requiere una sesión gráfica"]
fn us5_webkit_embeds() {
    glib::set_prgname(Some("orbital"));
    adw::init().expect("Se necesita acceso a una sesión gráfica GTK");
    let context = glib::MainContext::default();
    context.block_on(async {
        let root = std::env::temp_dir().join(format!("orbital-us5-{}", std::process::id()));
        fs::create_dir_all(root.join("wiki")).unwrap();
        fs::write(root.join("wiki/a.md"), "# Átomo A\n\nTexto de A.\n").unwrap();
        let source = "<!doctype html><html><head>\n<script type=\"application/yaml\">\ntype: sintesis\n</script>\n</head><body><p>Antes.</p><p>![[a]]</p><p>Después.</p></body></html>";
        fs::write(root.join("wiki/s.html"), source).unwrap();
        let path = orbital_vault::RelPath::new("wiki/s.html").unwrap();
        let vault = Arc::new(orbital_vault::open(&root).unwrap());
        let editor = Editor::new();
        let window = gtk::Window::builder()
            .title("Orbital — prueba aislada US5")
            .default_width(1000)
            .default_height(700)
            .child(editor.widget())
            .build();
        window.present();
        editor.load(vault.clone(), &vault.read_note(&path).unwrap());
        assert_eq!(editor.0.mode.get(), View::Visual);
        let web = editor.0.visual.web();
        ready(&web).await;
        let card = "document.querySelector('[data-orbital-embed]')";
        for _ in 0..50 {
            if eval(&web, &format!("!!{card}")).await == "true" {
                break;
            }
            glib::timeout_future(Duration::from_millis(100)).await;
        }
        assert!(eval(&web, &format!("{card}.textContent")).await.contains("Texto de A."));
        println!("OK: la tarjeta muestra la nota incrustada");

        // Escribir en Visual y guardar: el archivo conserva `![[a]]` y no la tarjeta.
        eval(&web, "const p = document.querySelectorAll('body > p')[2]; getSelection().selectAllChildren(p); document.execCommand('insertText', false, 'Después, editado.')").await;
        glib::timeout_future(Duration::from_millis(500)).await;
        editor.flush().await;
        let saved = fs::read_to_string(root.join("wiki/s.html")).unwrap();
        assert!(saved.contains("Después, editado."), "{saved}");
        assert!(saved.contains("![[a]]"), "{saved}");
        assert!(!saved.contains("orbital-embed"), "{saved}");
        assert!(!saved.contains("Texto de A."), "{saved}");
        assert_eq!(saved.split("<body>").next(), source.split("<body>").next());
        println!("OK: la tarjeta no se guarda; el <head> queda igual");

        // En vivo: cambia la nota incrustada.
        fs::write(root.join("wiki/a.md"), "# Átomo A\n\nTexto nuevo de A.\n").unwrap();
        editor.dependency_changed(&orbital_vault::RelPath::new("wiki/a.md").unwrap());
        for _ in 0..50 {
            if eval(&web, &format!("{card}.textContent")).await.contains("Texto nuevo de A.") {
                break;
            }
            glib::timeout_future(Duration::from_millis(100)).await;
        }
        assert!(eval(&web, &format!("{card}.textContent")).await.contains("Texto nuevo de A."));
        assert_eq!(eval(&web, "document.querySelectorAll('[data-orbital-embed]').length").await, "1");
        println!("OK: la tarjeta se actualiza en vivo");
        window.close();
        let _ = fs::remove_dir_all(&root);
    });
}

/// 005 T010/T011: los marcadores `[%clave]` en Visual se ven con su valor pero no se editan,
/// y al guardar el disco conserva `[%clave]` byte a byte. Ejecutar con
/// `cargo test --bin orbital us5_webkit_markers -- --ignored --nocapture --test-threads=1`.
#[test]
#[ignore = "requiere una sesión gráfica"]
fn us5_webkit_markers() {
    glib::set_prgname(Some("orbital"));
    adw::init().expect("Se necesita acceso a una sesión gráfica GTK");
    let context = glib::MainContext::default();
    context.block_on(async {
        let root = std::env::temp_dir().join(format!("orbital-us5m-{}", std::process::id()));
        fs::create_dir_all(root.join("wiki")).unwrap();
        let source = "<!doctype html><html><head>\n<script type=\"application/yaml\">\ntype: sintesis\nestado: pausado\n</script>\n</head><body><p>El estado es [%estado] y el otro es [%ausente].</p>\n<p>estado:: local</p>\n<p>[%estado] repetido.</p></body></html>";
        fs::write(root.join("wiki/s.html"), source).unwrap();
        let path = orbital_vault::RelPath::new("wiki/s.html").unwrap();
        let vault = Arc::new(orbital_vault::open(&root).unwrap());
        let editor = Editor::new();
        let window = gtk::Window::builder()
            .title("Orbital — prueba aislada US5 marcadores")
            .default_width(1000)
            .default_height(700)
            .child(editor.widget())
            .build();
        window.present();
        editor.load(vault.clone(), &vault.read_note(&path).unwrap());
        let web = editor.0.visual.web();
        ready(&web).await;
        let span = "document.querySelector('[data-orbital-marker]')";
        for _ in 0..50 {
            if eval(&web, &format!("!!{span}")).await == "true" {
                break;
            }
            glib::timeout_future(Duration::from_millis(100)).await;
        }
        // El inline del cuerpo (local) gana al YAML (pausado); la clave sin valor es faltante.
        let first = eval(&web, &format!("{span}.textContent")).await;
        assert_eq!(first, "local", "{first}");
        assert_eq!(
            eval(&web, &format!("{span}.dataset.orbitalMarker")).await,
            "estado"
        );
        let spans = eval(&web, "document.querySelectorAll('[data-orbital-marker]').length").await;
        assert_eq!(spans, "3", "{spans}");
        let faltante = eval(&web,
            "document.querySelectorAll('[data-orbital-marker]')[1].textContent")
            .await;
        assert_eq!(faltante, "«faltante»");
        println!("OK: los marcadores se ven con su valor (inline gana; «faltante» si no hay)");

        // El span no es editable: el valor no se puede cambiar escribiendo dentro.
        assert_eq!(
            eval(&web, &format!("{span}.contentEditable")).await,
            "false"
        );

        // Editar alrededor: el texto nuevo entra, el marcador conserva su valor.
        eval(&web,
            "const p = document.querySelectorAll('body > p')[0]; getSelection().selectAllChildren(p); document.execCommand('insertText', false, 'El estado es [%estado] y el otro es [%ausente], editado.')").await;
        glib::timeout_future(Duration::from_millis(500)).await;
        let spans2 = eval(&web, "document.querySelectorAll('[data-orbital-marker]').length").await;
        assert_eq!(spans2, "3", "tras editar: {spans2}");
        println!("OK: editar alrededor conserva los marcadores");

        // Guardar: el disco conserva [%clave], nunca el valor ni los atributos del span.
        editor.flush().await;
        let saved = fs::read_to_string(root.join("wiki/s.html")).unwrap();
        assert!(saved.contains("[%estado]"), "{saved}");
        assert!(saved.contains("[%ausente]"), "{saved}");
        assert!(saved.contains("editado."), "{saved}");
        assert!(!saved.contains("«faltante»"), "{saved}");
        assert!(!saved.contains("data-orbital-marker"), "{saved}");
        assert_eq!(saved.split("<body>").next(), source.split("<body>").next());
        println!("OK: el disco conserva los marcadores byte a byte (el <head> no cambia)");
        window.close();
        let _ = fs::remove_dir_all(&root);
    });
}
