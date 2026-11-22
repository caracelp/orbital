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
        // La ventana puede no tener un cuadro listo todavía (captura vacía): reintentar.
        let paintable = gtk::WidgetPaintable::new(Some(&window));
        let mut node = None;
        for _ in 0..40 {
            let snapshot = gtk::Snapshot::new();
            paintable.snapshot(&snapshot, f64::from(window.width()), f64::from(window.height()));
            node = snapshot.to_node();
            if node.is_some() {
                break;
            }
            glib::timeout_future(std::time::Duration::from_millis(50)).await;
        }
        let node = node.expect("captura de la ventana de prueba");
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

/// 026 T009: en Visual, al guardar cada `p`/`li`/… tiene un `id` único, también el que crea Enter
/// (que copia el atributo del bloque partido).
#[test]
#[ignore = "requiere una sesión gráfica"]
fn webkit_ids_de_bloque() {
    glib::set_prgname(Some("orbital"));
    adw::init().expect("Se necesita acceso a una sesión gráfica GTK");
    let context = glib::MainContext::default();
    context.block_on(async {
        let root = std::env::temp_dir().join(format!("orbital-026-visual-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("wiki")).unwrap();
        let source = "<!doctype html><html><head><title>s</title></head><body><p id=\"fijo01\">Uno.</p><ul><li>ítem</li></ul><p>Dos.</p></body></html>";
        fs::write(root.join("wiki/s.html"), source).unwrap();
        let path = orbital_vault::RelPath::new("wiki/s.html").unwrap();
        let vault = Arc::new(orbital_vault::open(&root).unwrap());
        let editor = Editor::new();
        let window = gtk::Window::builder()
            .default_width(1000)
            .default_height(700)
            .child(editor.widget())
            .build();
        window.present();
        editor.load(vault.clone(), &vault.read_note(&path).unwrap());
        assert_eq!(editor.0.mode.get(), View::Visual);
        let web = editor.0.visual.web();
        ready(&web).await;
        // Partir «Uno.» con Enter: el párrafo nuevo hereda `id="fijo01"`.
        eval(&web, "const p = document.querySelector('p'); const r = document.createRange(); r.setStart(p.firstChild, 2); r.collapse(true); getSelection().removeAllRanges(); getSelection().addRange(r); document.execCommand('insertParagraph')").await;
        glib::timeout_future(Duration::from_millis(500)).await;
        editor.flush().await;
        let saved = fs::read_to_string(root.join("wiki/s.html")).unwrap();
        let ids: Vec<&str> = saved
            .split(" id=\"")
            .skip(1)
            .map(|r| &r[..r.find('"').unwrap()])
            .collect();
        assert_eq!(ids.len(), 4, "tres p y un li con id: {saved}");
        assert_eq!(ids[0], "fijo01", "el id existente se conserva: {saved}");
        let unicos: std::collections::HashSet<_> = ids.iter().collect();
        assert_eq!(unicos.len(), ids.len(), "ids repetidos: {saved}");
        assert_eq!(editor.text(), saved);
        println!("OK: Visual guarda ids únicos en cada bloque");
        window.close();
        let _ = fs::remove_dir_all(&root);
    });
}

/// 050: en Visual, las fórmulas se ven dibujadas (SVG con nombre accesible), las con error con su
/// fuente y el motivo, y al guardar el disco conserva el LaTeX tal cual.
/// `cargo test --bin orbital webkit_formulas -- --ignored --nocapture`.
#[test]
#[ignore = "requiere una sesión gráfica"]
fn webkit_formulas() {
    glib::set_prgname(Some("orbital"));
    adw::init().expect("Se necesita acceso a una sesión gráfica GTK");
    let context = glib::MainContext::default();
    context.block_on(async {
        let root = std::env::temp_dir().join(format!("orbital-050-{}", std::process::id()));
        fs::create_dir_all(root.join("wiki")).unwrap();
        let source = "<!doctype html><html><head>\n<script type=\"application/yaml\">\ntype: sintesis\n</script>\n</head><body><p>Sea $a &lt; b^2$ y cuesta $5 o $10.</p>\n<p>$$\\frac{1}{2}$$</p>\n<p>Mal: $x^^2$ y <code>$c$</code>.</p></body></html>";
        fs::write(root.join("wiki/s.html"), source).unwrap();
        let path = orbital_vault::RelPath::new("wiki/s.html").unwrap();
        let vault = Arc::new(orbital_vault::open(&root).unwrap());
        let editor = Editor::new();
        let window = gtk::Window::builder()
            .title("Orbital — prueba aislada 050 fórmulas")
            .default_width(1000)
            .default_height(700)
            .child(editor.widget())
            .build();
        window.present();
        editor.load(vault.clone(), &vault.read_note(&path).unwrap());
        let web = editor.0.visual.web();
        ready(&web).await;
        let todas = "document.querySelectorAll('[data-orbital-formula]')";
        for _ in 0..50 {
            if eval(&web, &format!("{todas}.length")).await == "3" {
                break;
            }
            glib::timeout_future(Duration::from_millis(100)).await;
        }
        assert_eq!(eval(&web, &format!("{todas}.length")).await, "3");
        assert_eq!(
            eval(&web, &format!("{todas}[0].dataset.orbitalFormula")).await,
            "$a < b^2$"
        );
        assert_eq!(
            eval(
                &web,
                &format!("{todas}[0].querySelector('svg').getAttribute('aria-label')")
            )
            .await,
            "Fórmula: a < b^2"
        );
        assert_eq!(
            eval(&web, &format!("{todas}[1].className")).await,
            "orbital-formula orbital-formula-bloque"
        );
        let error = eval(&web, &format!("{todas}[2].title")).await;
        assert!(error.starts_with("Error en la fórmula"), "{error}");
        assert_eq!(
            eval(&web, &format!("{todas}[2].textContent")).await,
            "$x^^2$"
        );
        assert_eq!(
            eval(&web, "document.querySelector('code').textContent").await,
            "$c$"
        );
        assert!(
            eval(&web, "document.body.textContent")
                .await
                .contains("cuesta $5 o $10.")
        );
        println!("OK: fórmulas dibujadas, la con error con su fuente, dinero y código intactos");

        // Editar alrededor y guardar: el disco conserva el LaTeX, nunca el SVG.
        eval(&web,
            "const p = document.querySelectorAll('body > p')[2]; getSelection().selectAllChildren(p); document.execCommand('insertText', false, 'Editado $y^2$.')").await;
        glib::timeout_future(Duration::from_millis(600)).await;
        editor.flush().await;
        let saved = fs::read_to_string(root.join("wiki/s.html")).unwrap();
        assert!(
            saved.contains("Sea $a &lt; b^2$ y cuesta $5 o $10."),
            "{saved}"
        );
        assert!(saved.contains(">$$\\frac{1}{2}$$</p>"), "{saved}");
        assert!(saved.contains("Editado $y^2$."), "{saved}");
        assert!(!saved.contains("<svg"), "{saved}");
        assert!(!saved.contains("data-orbital-formula"), "{saved}");
        assert_eq!(saved.split("<body>").next(), source.split("<body>").next());
        println!("OK: el disco conserva las fórmulas como LaTeX");
        window.close();
        let _ = fs::remove_dir_all(&root);
    });
}

/// 052: en Visual, cada `pre > code.language-grafico` se ve como su SVG (estático, con nombre
/// accesible) en una figura no editable; uno con error, con su motivo. Al guardar, el disco
/// conserva el fuente y nada de las figuras.
/// `cargo test --bin orbital webkit_graficos -- --ignored --nocapture`.
#[test]
#[ignore = "requiere una sesión gráfica"]
fn webkit_graficos() {
    glib::set_prgname(Some("orbital"));
    adw::init().expect("Se necesita acceso a una sesión gráfica GTK");
    glib::MainContext::default().block_on(async {
        let root = std::env::temp_dir().join(format!("orbital-052-html-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("wiki")).unwrap();
        let source = "<!doctype html><html><head>\n<script type=\"application/yaml\">\ntype: sintesis\n</script>\n</head><body><p>Antes.</p>\n<pre><code class=\"language-grafico\">título: Ondas\ny = \\sin x\nx^2 + y^2 &lt; 1</code></pre>\n<pre><code class=\"language-grafico\">título: Ondas\ny = \\sin x\nr = 2</code></pre>\n<p>Después.</p></body></html>";
        fs::write(root.join("wiki/g.html"), source).unwrap();
        let path = orbital_vault::RelPath::new("wiki/g.html").unwrap();
        let vault = Arc::new(orbital_vault::open(&root).unwrap());
        let editor = Editor::new();
        let window = gtk::Window::builder()
            .title("Orbital — prueba aislada 052 gráficos")
            .default_width(1000)
            .default_height(800)
            .child(editor.widget())
            .build();
        window.present();
        editor.load(vault.clone(), &vault.read_note(&path).unwrap());
        let web = editor.0.visual.web();
        ready(&web).await;
        let figuras = "document.querySelectorAll('[data-orbital-grafico]')";
        for _ in 0..60 {
            if eval(&web, &format!("{figuras}.length")).await == "2" {
                break;
            }
            glib::timeout_future(Duration::from_millis(100)).await;
        }
        assert_eq!(eval(&web, &format!("{figuras}.length")).await, "2");
        // El primero tiene una desigualdad: el motivo, y el fuente queda a la vista.
        let error = eval(&web, &format!("{figuras}[0].textContent")).await;
        assert!(error.contains("Error en el gráfico (renglón 3)"), "{error}");
        assert_eq!(
            eval(
                &web,
                "document.querySelectorAll('pre')[0].hasAttribute('data-orbital-grafico-oculto')"
            )
            .await,
            "false"
        );
        // El segundo, dibujado; su `pre`, oculto.
        assert_eq!(
            eval(
                &web,
                &format!("{figuras}[1].querySelector('svg').getAttribute('aria-label')")
            )
            .await,
            "Gráfico: Ondas. y = \\sin x; r = 2. x de −10 a 10."
        );
        assert_eq!(
            eval(
                &web,
                "getComputedStyle(document.querySelectorAll('pre')[1]).display"
            )
            .await,
            "none"
        );
        if let Some(dir) = std::env::var_os("ORBITAL_CAPTURA_DIR") {
            glib::timeout_future(Duration::from_millis(300)).await;
            let t = web
                .snapshot_future(
                    webkit6::SnapshotRegion::Visible,
                    webkit6::SnapshotOptions::NONE,
                )
                .await
                .unwrap();
            let png = std::path::Path::new(&dir).join("052-visual-html.png");
            t.save_to_png(&png).unwrap();
            println!("Captura: {}", png.display());
        }
        // Editar alrededor y guardar: el disco conserva el fuente de los gráficos.
        eval(&web,
            "const p = document.querySelectorAll('body > p')[1]; getSelection().selectAllChildren(p); document.execCommand('insertText', false, 'Editado.')").await;
        glib::timeout_future(Duration::from_millis(600)).await;
        editor.flush().await;
        let saved = fs::read_to_string(root.join("wiki/g.html")).unwrap();
        assert!(saved.contains("Editado."), "{saved}");
        assert!(
            saved.contains("<code class=\"language-grafico\">título: Ondas\ny = \\sin x\nr = 2</code>"),
            "{saved}"
        );
        assert!(!saved.contains("<svg"), "{saved}");
        assert!(!saved.contains("data-orbital-grafico"), "{saved}");
        assert!(!saved.contains("<figure"), "{saved}");
        println!("OK: gráficos en Visual y el disco con su fuente");
        window.close();
        let _ = fs::remove_dir_all(&root);
    });
}

/// 052: lo exportado muestra los gráficos como SVG para papel en WebKit (lo mismo que se
/// imprime a PDF). Con `ORBITAL_CAPTURA_DIR`, guarda `052-exportado.png`.
#[test]
#[ignore = "requiere una sesión gráfica"]
fn exportar_graficos_en_webkit() {
    glib::set_prgname(Some("orbital"));
    adw::init().expect("Se necesita acceso a una sesión gráfica GTK");
    glib::MainContext::default().block_on(async {
        let nota = crate::editor::graficos_tests::NOTA;
        let html = crate::export::html_md(nota, "graficos.md");
        assert_eq!(
            html.matches("<figure class=\"grafico\"><svg").count(),
            4,
            "{html}"
        );
        assert!(html.contains("class=\"grafico-error\""), "{html}");
        let web = crate::html_view::new_view(None);
        let window = gtk::Window::builder()
            .default_width(860)
            .default_height(900)
            .child(&web)
            .build();
        window.present();
        web.load_html(&crate::html_view::with_csp(&html), None);
        for _ in 0..100 {
            if !web.is_loading() {
                break;
            }
            glib::timeout_future(Duration::from_millis(50)).await;
        }
        glib::timeout_future(Duration::from_millis(400)).await;
        if let Some(dir) = std::env::var_os("ORBITAL_CAPTURA_DIR") {
            // El documento entero (la página exportada no corre JavaScript).
            let t = web
                .snapshot_future(
                    webkit6::SnapshotRegion::FullDocument,
                    webkit6::SnapshotOptions::NONE,
                )
                .await
                .unwrap();
            let png = std::path::Path::new(&dir).join("052-exportado.png");
            t.save_to_png(&png).unwrap();
            println!("Captura: {}", png.display());
        }
        window.close();
    });
}

/// 058: en Visual, cada `pre > code.language-mermaid` se ve como su SVG (texto en
/// `currentColor`) en una figura no editable; uno con error, con su motivo. Los gráficos de la
/// misma página no se pisan con los diagramas. Al guardar, el disco conserva el fuente.
/// `cargo test --bin orbital webkit_diagramas -- --ignored --nocapture`.
#[test]
#[ignore = "requiere una sesión gráfica"]
fn webkit_diagramas() {
    glib::set_prgname(Some("orbital"));
    adw::init().expect("Se necesita acceso a una sesión gráfica GTK");
    glib::MainContext::default().block_on(async {
        let root = std::env::temp_dir().join(format!("orbital-058-html-ñ-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("wiki")).unwrap();
        let source = "<!doctype html><html><head>\n<script type=\"application/yaml\">\ntype: sintesis\n</script>\n</head><body><p>Antes.</p>\n<pre><code class=\"language-mermaid\">graph TD\n    A[Año] --&gt; B{¿Sí?}\n    B --&gt;|No| C[Fin]</code></pre>\n<pre><code class=\"language-mermaid\">graph TD\n    A --&gt;&gt; B[</code></pre>\n<pre><code class=\"language-grafico\">y = x^2</code></pre>\n<p>Después.</p></body></html>";
        fs::write(root.join("wiki/d.html"), source).unwrap();
        let path = orbital_vault::RelPath::new("wiki/d.html").unwrap();
        let vault = Arc::new(orbital_vault::open(&root).unwrap());
        let editor = Editor::new();
        let window = gtk::Window::builder()
            .title("Orbital — prueba aislada 058 diagramas")
            .default_width(1000)
            .default_height(800)
            .child(editor.widget())
            .build();
        window.present();
        editor.load(vault.clone(), &vault.read_note(&path).unwrap());
        let web = editor.0.visual.web();
        ready(&web).await;
        let figuras = "document.querySelectorAll('[data-orbital-tipo=\"mermaid\"]')";
        let graficos = "document.querySelectorAll('[data-orbital-tipo=\"grafico\"]')";
        for _ in 0..60 {
            if eval(&web, &format!("{figuras}.length + {graficos}.length")).await == "3" {
                break;
            }
            glib::timeout_future(Duration::from_millis(100)).await;
        }
        assert_eq!(eval(&web, &format!("{figuras}.length")).await, "2");
        assert_eq!(eval(&web, &format!("{graficos}.length")).await, "1");
        assert_eq!(
            eval(
                &web,
                &format!("{figuras}[0].querySelector('svg').getAttribute('aria-label')")
            )
            .await,
            "Diagrama Mermaid: de flujo"
        );
        let error = eval(&web, &format!("{figuras}[1].textContent")).await;
        assert!(error.contains("Error en el diagrama (renglón 2)"), "{error}");
        assert_eq!(
            eval(&web, "getComputedStyle(document.querySelectorAll('pre')[0]).display").await,
            "none"
        );
        if let Some(dir) = std::env::var_os("ORBITAL_CAPTURA_DIR") {
            glib::timeout_future(Duration::from_millis(300)).await;
            let t = web
                .snapshot_future(
                    webkit6::SnapshotRegion::Visible,
                    webkit6::SnapshotOptions::NONE,
                )
                .await
                .unwrap();
            let png = std::path::Path::new(&dir).join("058-visual-html.png");
            t.save_to_png(&png).unwrap();
            println!("Captura: {}", png.display());
        }
        eval(&web,
            "const p = document.querySelectorAll('body > p')[1]; getSelection().selectAllChildren(p); document.execCommand('insertText', false, 'Editado.')").await;
        glib::timeout_future(Duration::from_millis(600)).await;
        editor.flush().await;
        let saved = fs::read_to_string(root.join("wiki/d.html")).unwrap();
        assert!(saved.contains("Editado."), "{saved}");
        assert!(saved.contains("<code class=\"language-mermaid\">graph TD\n    A[Año] --&gt; B{¿Sí?}"), "{saved}");
        assert!(!saved.contains("<svg"), "{saved}");
        assert!(!saved.contains("data-orbital-tipo"), "{saved}");
        assert!(!saved.contains("<figure"), "{saved}");
        window.close();
        let _ = fs::remove_dir_all(&root);
    });
}

/// 058: lo exportado muestra los diagramas como SVG de papel en WebKit (lo mismo que se imprime
/// a PDF). Con `ORBITAL_CAPTURA_DIR`, guarda `058-exportado.png`.
#[test]
#[ignore = "requiere una sesión gráfica"]
fn exportar_diagramas_en_webkit() {
    glib::set_prgname(Some("orbital"));
    adw::init().expect("Se necesita acceso a una sesión gráfica GTK");
    glib::MainContext::default().block_on(async {
        let nota = crate::editor::diagramas_tests::NOTA;
        let html = crate::export::html_md(nota, "diagramas.md");
        assert_eq!(
            html.matches("<figure class=\"diagrama\"><svg").count(),
            9,
            "{html}"
        );
        assert!(html.contains("class=\"grafico-error\""), "{html}");
        let web = crate::html_view::new_view(None);
        let window = gtk::Window::builder()
            .default_width(860)
            .default_height(900)
            .child(&web)
            .build();
        window.present();
        web.load_html(&crate::html_view::with_csp(&html), None);
        for _ in 0..100 {
            if !web.is_loading() {
                break;
            }
            glib::timeout_future(Duration::from_millis(50)).await;
        }
        glib::timeout_future(Duration::from_millis(400)).await;
        // Cada SVG tiene tamaño en la página (no quedó un hueco).
        let anchos = eval(
            &web,
            "Array.from(document.querySelectorAll('figure.diagrama svg')).map(s => s.getBoundingClientRect().width > 50 && s.getBoundingClientRect().height > 50).every(x => x)",
        )
        .await;
        assert_eq!(anchos, "true");
        if let Some(dir) = std::env::var_os("ORBITAL_CAPTURA_DIR") {
            let t = web
                .snapshot_future(
                    webkit6::SnapshotRegion::FullDocument,
                    webkit6::SnapshotOptions::NONE,
                )
                .await
                .unwrap();
            let png = std::path::Path::new(&dir).join("058-exportado.png");
            t.save_to_png(&png).unwrap();
            println!("Captura: {}", png.display());
        }
        window.close();
    });
}

/// 050: lo exportado (007) muestra las fórmulas como SVG en WebKit, lo mismo que se imprime a
/// PDF. Con `ORBITAL_CAPTURA_DIR`, guarda `050-exportado.png`.
#[test]
#[ignore = "requiere una sesión gráfica"]
fn exportar_formulas_en_webkit() {
    glib::set_prgname(Some("orbital"));
    adw::init().expect("Se necesita acceso a una sesión gráfica GTK");
    glib::MainContext::default().block_on(async {
        let nota = r"# Exportado

La identidad $e^{i\pi} + 1 = 0$ y una fracción $\frac{a}{b}$; cuesta $5 o $10.

$$
\begin{aligned} (a+b)^2 &= a^2 + 2ab + b^2 \\ (a-b)^2 &= a^2 - 2ab + b^2 \end{aligned}
$$

```math
\begin{pmatrix} a & b \\ c & d \end{pmatrix}
```

Con error: $x^^2$.
";
        let html = crate::export::html_md(nota, "exportado.md");
        let web = crate::html_view::new_view(None);
        let window = gtk::Window::builder()
            .default_width(820)
            .default_height(620)
            .child(&web)
            .build();
        window.present();
        web.load_html(&crate::html_view::with_csp(&html), None);
        for _ in 0..100 {
            if !web.is_loading() {
                break;
            }
            glib::timeout_future(Duration::from_millis(50)).await;
        }
        glib::timeout_future(Duration::from_millis(300)).await;
        assert_eq!(html.matches("<svg").count(), 4, "{html}");
        if let Some(dir) = std::env::var_os("ORBITAL_CAPTURA_DIR") {
            let t = web
                .snapshot_future(
                    webkit6::SnapshotRegion::Visible,
                    webkit6::SnapshotOptions::NONE,
                )
                .await
                .unwrap();
            let png = std::path::Path::new(&dir).join("050-exportado.png");
            t.save_to_png(&png).unwrap();
            println!("Captura: {}", png.display());
        }
        window.close();
    });
}

/// 057: en la Visual de una nota `.html`, `![[tono.wav]]` es un `<audio>` con controles, sin
/// precargar, con el nombre debajo; suena (desde `#t=`) y no se guarda en el archivo. Lo que
/// falta dice «Archivo inexistente». La bóveda va en `/tmp`, que el proceso web de WebKit no ve
/// por su cuenta: la vista la monta en su sandbox (`html_view::preparar_sandbox`).
#[test]
#[ignore = "requiere una sesión gráfica"]
fn medios_en_visual_html() {
    glib::set_prgname(Some("orbital"));
    adw::init().expect("Se necesita acceso a una sesión gráfica GTK");
    let context = glib::MainContext::default();
    context.block_on(async {
        let root = std::env::temp_dir().join(format!(
            "orbital-057-visual-ñ-{}",
            std::process::id()
        ));
        fs::create_dir_all(root.join("wiki/medios")).unwrap();
        fs::write(
            root.join("wiki/medios/tono ñ.wav"),
            crate::medios::tests::wav_de_prueba(3.0),
        )
        .unwrap();
        let source = "<!doctype html><html><head></head><body><p>Oíd:</p><p>![[tono ñ.wav#t=1]]</p><p>![[falta.mp3]]</p></body></html>";
        fs::write(root.join("wiki/s.html"), source).unwrap();
        let path = orbital_vault::RelPath::new("wiki/s.html").unwrap();
        let vault = Arc::new(orbital_vault::open(&root).unwrap());
        let editor = Editor::new();
        let window = gtk::Window::builder()
            .default_width(1000)
            .default_height(700)
            .child(editor.widget())
            .build();
        window.present();
        editor.load(vault.clone(), &vault.read_note(&path).unwrap());
        let web = editor.0.visual.web();
        ready(&web).await;
        let audio = "document.querySelector('figure[data-orbital-embed] audio')";
        for _ in 0..50 {
            if eval(&web, &format!("!!{audio}")).await == "true" {
                break;
            }
            glib::timeout_future(Duration::from_millis(100)).await;
        }
        assert_eq!(eval(&web, &format!("{audio}.preload")).await, "none");
        assert_eq!(eval(&web, &format!("String({audio}.controls)")).await, "true");
        assert!(
            eval(&web, "document.querySelector('figure[data-orbital-embed] figcaption').textContent")
                .await
                .contains("tono ñ.wav")
        );
        assert!(
            eval(&web, "document.body.textContent").await.contains("Archivo inexistente: «falta.mp3»")
        );
        eval(&web, &format!("{audio}.play(); 'ok'")).await;
        let mut t = String::new();
        for _ in 0..60 {
            t = eval(&web, &format!("String({audio}.currentTime)")).await;
            if t.parse::<f64>().unwrap_or(0.0) > 1.2 {
                break;
            }
            glib::timeout_future(Duration::from_millis(100)).await;
        }
        assert!(t.parse::<f64>().unwrap_or(0.0) > 1.2, "no sonó desde #t=1: {t}");
        println!("OK: el audio suena en la Visual (va en {t} s)");
        eval(&web, &format!("{audio}.pause(); 'ok'")).await;
        eval(&web, "const p = document.querySelectorAll('body > p')[0]; getSelection().selectAllChildren(p); document.execCommand('insertText', false, 'Oíd, editado:')").await;
        glib::timeout_future(Duration::from_millis(500)).await;
        editor.flush().await;
        let saved = fs::read_to_string(root.join("wiki/s.html")).unwrap();
        assert!(saved.contains("Oíd, editado:"), "{saved}");
        assert!(saved.contains("![[tono ñ.wav#t=1]]"), "{saved}");
        assert!(!saved.contains("<audio"), "{saved}");
        window.close();
        let _ = fs::remove_dir_all(&root);
    });
}

/// Varios `![[…]]` en un mismo párrafo de una nota `.html`: cada uno sale en el orden del texto,
/// también los repetidos (antes, invertidos y uno solo por destino).
#[test]
#[ignore = "requiere una sesión gráfica"]
fn varios_medios_en_un_renglon_visual_html() {
    glib::set_prgname(Some("orbital"));
    adw::init().expect("Se necesita acceso a una sesión gráfica GTK");
    glib::MainContext::default().block_on(async {
        let root = std::env::temp_dir().join(format!(
            "orbital-embeds-renglon-ñ-{}",
            std::process::id()
        ));
        fs::create_dir_all(root.join("wiki/medios")).unwrap();
        fs::write(
            root.join("wiki/medios/tono ñ.wav"),
            crate::medios::tests::wav_de_prueba(1.0),
        )
        .unwrap();
        let source = "<!doctype html><html><head></head><body><p>![[falta.mp3]] ![[tono ñ.wav]] ![[falta.mp3]]</p><p>Fin.</p></body></html>";
        fs::write(root.join("wiki/r.html"), source).unwrap();
        let path = orbital_vault::RelPath::new("wiki/r.html").unwrap();
        let vault = Arc::new(orbital_vault::open(&root).unwrap());
        let editor = Editor::new();
        let window = gtk::Window::builder()
            .default_width(1000)
            .default_height(700)
            .child(editor.widget())
            .build();
        window.present();
        editor.load(vault.clone(), &vault.read_note(&path).unwrap());
        let web = editor.0.visual.web();
        ready(&web).await;
        let orden = "[...document.querySelectorAll('[data-orbital-embed]')].map(e => e.querySelector('audio') ? 'A' : (e.textContent.includes('Archivo inexistente') ? 'F' : '?')).join('')";
        let mut visto = String::new();
        for _ in 0..50 {
            visto = eval(&web, orden).await;
            if visto.len() == 3 {
                break;
            }
            glib::timeout_future(Duration::from_millis(100)).await;
        }
        assert_eq!(visto, "FAF");
        window.close();
        let _ = fs::remove_dir_all(&root);
    });
}

/// 032: un carrusel en una nota `.html` se ve en la Visual como la grilla estática de sus medios
/// (resueltos contra la bóveda, con la imagen cargada) y el archivo conserva el bloque. Con
/// `ORBITAL_CAPTURA_DIR`, guarda `032-visual-html.png`.
#[test]
#[ignore = "requiere una sesión gráfica"]
fn webkit_carruseles() {
    glib::set_prgname(Some("orbital"));
    adw::init().expect("Se necesita acceso a una sesión gráfica GTK");
    glib::MainContext::default().block_on(async {
        let root = std::env::temp_dir().join(format!("orbital-032-html-ñ-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("wiki/fotos ñ")).unwrap();
        for (n, color) in [("a", 0xff69b4ffu32), ("b", 0x5aa0ffff)] {
            let pb = gtk::gdk_pixbuf::Pixbuf::new(
                gtk::gdk_pixbuf::Colorspace::Rgb,
                false,
                8,
                320,
                200,
            )
            .unwrap();
            pb.fill(color);
            pb.savev(root.join(format!("wiki/fotos ñ/{n}.png")), "png", &[])
                .unwrap();
        }
        let source = "<!doctype html><html><head></head><body><p>Antes.</p>\n<pre><code class=\"language-carrusel\">carpeta: fotos ñ\n![[falta.png|Falta]]</code></pre>\n<p>Después.</p></body></html>";
        fs::write(root.join("wiki/c.html"), source).unwrap();
        let path = orbital_vault::RelPath::new("wiki/c.html").unwrap();
        let vault = Arc::new(orbital_vault::open(&root).unwrap());
        let editor = Editor::new();
        let window = gtk::Window::builder()
            .title("Orbital — prueba aislada 032 carrusel")
            .default_width(1000)
            .default_height(700)
            .child(editor.widget())
            .build();
        window.present();
        editor.load(vault.clone(), &vault.read_note(&path).unwrap());
        let web = editor.0.visual.web();
        ready(&web).await;
        let figuras = "document.querySelectorAll('[data-orbital-tipo=\"carrusel\"]')";
        for _ in 0..60 {
            if eval(&web, &format!("{figuras}.length")).await == "1" {
                break;
            }
            glib::timeout_future(Duration::from_millis(100)).await;
        }
        assert_eq!(eval(&web, &format!("{figuras}.length")).await, "1");
        assert_eq!(
            eval(&web, &format!("{figuras}[0].querySelectorAll('img').length")).await,
            "2"
        );
        let texto = eval(&web, &format!("{figuras}[0].textContent")).await;
        assert!(texto.contains("Archivo inexistente: «falta.png»"), "{texto}");
        // Los medios del carrusel no se muestran además como tarjetas sueltas.
        glib::timeout_future(Duration::from_millis(400)).await;
        assert_eq!(eval(&web, "document.querySelectorAll('.orbital-embed').length").await, "0");
        // La imagen se cargó (la página está en la carpeta de la nota).
        let mut ancho = String::new();
        for _ in 0..40 {
            ancho = eval(&web, &format!("{figuras}[0].querySelector('img').naturalWidth")).await;
            if ancho == "320" {
                break;
            }
            glib::timeout_future(Duration::from_millis(100)).await;
        }
        assert_eq!(ancho, "320");
        if let Some(dir) = std::env::var_os("ORBITAL_CAPTURA_DIR") {
            glib::timeout_future(Duration::from_millis(300)).await;
            let t = web
                .snapshot_future(
                    webkit6::SnapshotRegion::Visible,
                    webkit6::SnapshotOptions::NONE,
                )
                .await
                .unwrap();
            let png = std::path::Path::new(&dir).join("032-visual-html.png");
            t.save_to_png(&png).unwrap();
            println!("Captura: {}", png.display());
        }
        eval(&web,
            "const p = document.querySelectorAll('body > p')[1]; getSelection().selectAllChildren(p); document.execCommand('insertText', false, 'Editado.')").await;
        glib::timeout_future(Duration::from_millis(600)).await;
        editor.flush().await;
        let saved = fs::read_to_string(root.join("wiki/c.html")).unwrap();
        assert!(saved.contains("Editado."), "{saved}");
        assert!(saved.contains("<code class=\"language-carrusel\">carpeta: fotos ñ"), "{saved}");
        assert!(!saved.contains("orbital-carrusel"), "{saved}");
        assert!(!saved.contains("<img"), "{saved}");
        window.close();
        let _ = fs::remove_dir_all(&root);
    });
}

/// 033: una galería en una nota `.html` se ve en la Visual como la grilla estática de su carpeta
/// (con la disposición y las imágenes cargadas) y el archivo conserva el bloque. Con
/// `ORBITAL_CAPTURA_DIR`, guarda `033-visual-html.png`.
#[test]
#[ignore = "requiere una sesión gráfica"]
fn webkit_galerias() {
    glib::set_prgname(Some("orbital"));
    adw::init().expect("Se necesita acceso a una sesión gráfica GTK");
    glib::MainContext::default().block_on(async {
        let root = std::env::temp_dir().join(format!("orbital-033-html-ñ-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("wiki/fotos ñ")).unwrap();
        for (n, color) in [("a", 0xff69b4ffu32), ("b", 0x5aa0ffff)] {
            let pb = gtk::gdk_pixbuf::Pixbuf::new(
                gtk::gdk_pixbuf::Colorspace::Rgb,
                false,
                8,
                320,
                200,
            )
            .unwrap();
            pb.fill(color);
            pb.savev(root.join(format!("wiki/fotos ñ/{n}.png")), "png", &[])
                .unwrap();
        }
        let source = "<!doctype html><html><head></head><body><p>Antes.</p>\n<pre><code class=\"language-galeria\">carpeta: ./fotos ñ\nlayout: justificadas</code></pre>\n<p>Después.</p></body></html>";
        fs::write(root.join("wiki/c.html"), source).unwrap();
        let path = orbital_vault::RelPath::new("wiki/c.html").unwrap();
        let vault = Arc::new(orbital_vault::open(&root).unwrap());
        let editor = Editor::new();
        let window = gtk::Window::builder()
            .title("Orbital — prueba aislada 033 galería")
            .default_width(1000)
            .default_height(700)
            .child(editor.widget())
            .build();
        window.present();
        editor.load(vault.clone(), &vault.read_note(&path).unwrap());
        let web = editor.0.visual.web();
        ready(&web).await;
        let figuras = "document.querySelectorAll('[data-orbital-tipo=\"galeria\"]')";
        for _ in 0..60 {
            if eval(&web, &format!("{figuras}.length")).await == "1" {
                break;
            }
            glib::timeout_future(Duration::from_millis(100)).await;
        }
        assert_eq!(eval(&web, &format!("{figuras}.length")).await, "1");
        assert_eq!(
            eval(&web, &format!("{figuras}[0].querySelectorAll('img').length")).await,
            "2"
        );
        assert_eq!(
            eval(&web, &format!("{figuras}[0].querySelectorAll('.orbital-galeria-justificadas').length")).await,
            "1"
        );
        // La imagen se cargó (la página está en la carpeta de la nota).
        let mut ancho = String::new();
        for _ in 0..40 {
            ancho = eval(&web, &format!("{figuras}[0].querySelector('img').naturalWidth")).await;
            if ancho == "320" {
                break;
            }
            glib::timeout_future(Duration::from_millis(100)).await;
        }
        assert_eq!(ancho, "320");
        if let Some(dir) = std::env::var_os("ORBITAL_CAPTURA_DIR") {
            glib::timeout_future(Duration::from_millis(300)).await;
            let t = web
                .snapshot_future(
                    webkit6::SnapshotRegion::Visible,
                    webkit6::SnapshotOptions::NONE,
                )
                .await
                .unwrap();
            let png = std::path::Path::new(&dir).join("033-visual-html.png");
            t.save_to_png(&png).unwrap();
            println!("Captura: {}", png.display());
        }
        eval(&web,
            "const p = document.querySelectorAll('body > p')[1]; getSelection().selectAllChildren(p); document.execCommand('insertText', false, 'Editado.')").await;
        glib::timeout_future(Duration::from_millis(600)).await;
        editor.flush().await;
        let saved = fs::read_to_string(root.join("wiki/c.html")).unwrap();
        assert!(saved.contains("Editado."), "{saved}");
        assert!(saved.contains("<code class=\"language-galeria\">carpeta: ./fotos ñ"), "{saved}");
        assert!(!saved.contains("orbital-galeria"), "{saved}");
        assert!(!saved.contains("<img"), "{saved}");
        window.close();
        let _ = fs::remove_dir_all(&root);
    });
}

/// 032: lo exportado muestra el carrusel como una grilla estática con las imágenes cargadas
/// (lo mismo que se imprime a PDF). Con `ORBITAL_CAPTURA_DIR`, guarda `032-exportado.png`.
#[test]
#[ignore = "requiere una sesión gráfica"]
fn exportar_carrusel_en_webkit() {
    glib::set_prgname(Some("orbital"));
    adw::init().expect("Se necesita acceso a una sesión gráfica GTK");
    glib::MainContext::default().block_on(async {
        let root = std::env::temp_dir().join(format!("orbital-032-exp-ñ-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("wiki/fotos")).unwrap();
        for (n, color) in [("a", 0xff69b4ffu32), ("b", 0x5aa0ffff), ("c", 0xffaa3cff)] {
            let pb = gtk::gdk_pixbuf::Pixbuf::new(
                gtk::gdk_pixbuf::Colorspace::Rgb,
                false,
                8,
                400,
                250,
            )
            .unwrap();
            pb.fill(color);
            pb.savev(root.join(format!("wiki/fotos/{n}.png")), "png", &[])
                .unwrap();
        }
        fs::write(root.join("wiki/sonido.mp3"), b"x").unwrap();
        let nota = "# Exportado\n\n```carrusel\ncarpeta: fotos\n![[sonido.mp3|Entrevista]]\n![[falta.png]]\n```\n";
        fs::write(root.join("wiki/n.md"), nota).unwrap();
        let vault = orbital_vault::open(&root).unwrap();
        let rel = orbital_vault::RelPath::new("wiki/n.md").unwrap();
        let resolver = |c: &mut orbital_vault::carrusel::Carrusel| vault.resolver_carrusel(&rel, c);
        let m = crate::export::Medios {
            carpeta: Some(root.join("wiki")),
            adjunto: None,
            reproducibles: false,
            carrusel: Some(&resolver),
            galeria: None,
        };
        let html = crate::export::html_md_con(nota, "n.md", &m);
        assert_eq!(html.matches("<img ").count(), 3, "{html}");
        let archivo = root.join("exportado.html");
        fs::write(&archivo, &html).unwrap();
        crate::html_view::preparar_sandbox(Some(&root));
        let web = crate::html_view::new_view(None);
        let window = gtk::Window::builder()
            .default_width(860)
            .default_height(700)
            .child(&web)
            .build();
        window.present();
        web.load_uri(&gtk::gio::File::for_path(&archivo).uri());
        let mut cargadas = String::new();
        for _ in 0..60 {
            cargadas = eval(
                &web,
                "Array.from(document.images).filter(i => i.complete && i.naturalWidth > 0).length",
            )
            .await;
            if cargadas == "3" {
                break;
            }
            glib::timeout_future(Duration::from_millis(100)).await;
        }
        assert_eq!(cargadas, "3");
        if let Some(dir) = std::env::var_os("ORBITAL_CAPTURA_DIR") {
            glib::timeout_future(Duration::from_millis(300)).await;
            let t = web
                .snapshot_future(
                    webkit6::SnapshotRegion::FullDocument,
                    webkit6::SnapshotOptions::NONE,
                )
                .await
                .unwrap();
            let png = std::path::Path::new(&dir).join("032-exportado.png");
            t.save_to_png(&png).unwrap();
            println!("Captura: {}", png.display());
        }
        window.close();
        let _ = fs::remove_dir_all(&root);
    });
}
