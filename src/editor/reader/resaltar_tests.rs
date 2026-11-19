//! Pruebas GTK en proceso del resaltado de los bloques de código en la vista formateada (055):
//! Lectura con varios lenguajes, sin lenguaje y desconocido, bloques especiales intactos, reabrir
//! con la caché, Dividida al escribir y lo exportado. Con `ORBITAL_CAPTURA_DIR`, guardan
//! `055-lectura.png`, `055-dividida.png` y `055-exportado.png`.

use super::*;
use std::time::Duration;

/// Nota con bloques de varios lenguajes, sin lenguaje, desconocido y especiales.
pub(crate) const NOTA: &str = r#"# Código en Lectura

Un bloque en Rust:

```rust
fn principal() {
    let saludo = "hola"; // comentario rust
    return 42;
}
```

En Python, con alias:

```py
def doble(x):
    # comentario python
    return "doble" * x
```

JavaScript y shell:

```js
const total = cuenta + 1; // comentario js
```

```sh
echo "hola $USER" # comentario sh
```

Sin lenguaje:

```
fn sin_lenguaje() { "texto plano" }
```

Lenguaje desconocido:

```xyzlang
fn desconocido() { "texto plano" }
```

Una fórmula y un gráfico:

```math
\frac{a}{b}
```

```grafico
y = x^2
```

> Una cita con código:
>
> ```yml
> clave: "valor" # comentario yaml
> ```
"#;

async fn pausa(ms: u64) {
    glib::timeout_future(Duration::from_millis(ms)).await;
}

/// Espera a que la vista termine de volcar y de resaltar.
async fn esperar_resaltado(reader: &Reader) {
    for _ in 0..500 {
        let i = &reader.0;
        if !i.blocks.borrow().is_empty()
            && i.resaltar.borrow().is_empty()
            && !i.resaltar_programado.get()
        {
            return;
        }
        pausa(5).await;
    }
    panic!("el resaltado no terminó");
}

/// Los nombres de las etiquetas de resaltado en el comienzo de `aguja` (en el buffer).
fn sintaxis_en(buffer: &gtk::TextBuffer, aguja: &str) -> Vec<String> {
    let (a, _) = buffer
        .start_iter()
        .forward_search(aguja, gtk::TextSearchFlags::empty(), None)
        .unwrap_or_else(|| panic!("«{aguja}» en la vista"));
    a.tags()
        .iter()
        .filter_map(|t| t.name())
        .map(|n| n.to_string())
        .filter(|n| n.starts_with("sintaxis-"))
        .collect()
}

/// Ninguna etiqueta de resaltado en todo `aguja`.
fn sin_sintaxis(buffer: &gtk::TextBuffer, aguja: &str) -> bool {
    let (mut a, b) = buffer
        .start_iter()
        .forward_search(aguja, gtk::TextSearchFlags::empty(), None)
        .unwrap_or_else(|| panic!("«{aguja}» en la vista"));
    while a < b {
        if a.tags()
            .iter()
            .any(|t| t.name().is_some_and(|n| n.starts_with("sintaxis-")))
        {
            return false;
        }
        a.forward_char();
    }
    true
}

fn comprobar_colores(reader: &Reader) {
    let buffer = &reader.buffer();
    let tiene = |aguja: &str, clase: &str| {
        let t = sintaxis_en(buffer, aguja);
        assert!(
            t.iter()
                .any(|n| n.starts_with(&format!("sintaxis-{clase}"))),
            "«{aguja}» con {clase}: {t:?}"
        );
    };
    tiene("fn principal", "sx-c");
    tiene("\"hola\"", "sx-s");
    tiene("// comentario rust", "sx-m");
    tiene("def doble", "sx-c");
    tiene("# comentario python", "sx-m");
    tiene("const total", "sx-c");
    tiene("// comentario js", "sx-m");
    tiene("# comentario sh", "sx-m");
    tiene("# comentario yaml", "sx-m");
    // Sin lenguaje y desconocido: como antes, sin color.
    assert!(sin_sintaxis(
        buffer,
        "fn sin_lenguaje() { \"texto plano\" }"
    ));
    assert!(sin_sintaxis(buffer, "fn desconocido() { \"texto plano\" }"));
    // Los especiales se dibujan como antes: ni la fórmula ni el gráfico son un bloque de código.
    let texto = buffer.text(&buffer.start_iter(), &buffer.end_iter(), true);
    let codeblock = buffer.tag_table().lookup("codeblock").unwrap();
    for fuente in ["\\frac{a}{b}", "y = x^2"] {
        let mut it = buffer.start_iter();
        while let Some((a, b)) = it.forward_search(fuente, gtk::TextSearchFlags::empty(), None) {
            assert!(
                !a.has_tag(&codeblock),
                "«{fuente}» no es un bloque de código"
            );
            assert!(sin_sintaxis(buffer, fuente));
            it = b;
        }
    }
    assert!(reader.0.hay_formulas.get(), "la fórmula se dibuja");
    // Copiar y buscar ven el texto plano del bloque, igual que antes.
    assert!(texto.contains(
        "fn principal() {\n    let saludo = \"hola\"; // comentario rust\n    return 42;"
    ));
}

#[test]
#[ignore = "requiere una sesión gráfica; tools/pruebas-gtk.sh"]
fn codigo_resaltado_en_lectura() {
    adw::init().expect("Se necesita una sesión gráfica GTK");
    crate::style::apply();
    let window = gtk::Window::builder()
        .default_width(900)
        .default_height(1300)
        .build();
    glib::MainContext::default().block_on(async {
        let reader = Reader::new();
        window.set_child(Some(reader.widget()));
        window.present();
        reader.render(Prepared::new(NOTA, &[]), || None, None);
        // El volcado no espera al resaltado: hay pendientes al terminar de volcar.
        for _ in 0..500 {
            if !reader.0.blocks.borrow().is_empty() {
                break;
            }
            pausa(1).await;
        }
        assert!(
            !reader.0.resaltar.borrow().is_empty() || reader.0.resaltar_programado.get(),
            "la primera vez se resalta en reposo"
        );
        esperar_resaltado(&reader).await;
        comprobar_colores(&reader);
        pausa(300).await;
        super::reader_tests::capturar_lector(&reader, "055-lectura");

        // Reabrir: todo sale de la caché en el mismo volcado, sin pendientes.
        let otro = Reader::new();
        window.set_child(Some(otro.widget()));
        otro.render(Prepared::new(NOTA, &[]), || None, None);
        for _ in 0..500 {
            if !otro.0.blocks.borrow().is_empty() {
                break;
            }
            pausa(1).await;
        }
        assert!(otro.0.resaltar.borrow().is_empty(), "todo en la caché");
        comprobar_colores(&otro);
    });
    window.destroy();
}

#[test]
#[ignore = "requiere una sesión gráfica; tools/pruebas-gtk.sh"]
fn codigo_resaltado_en_dividida_al_escribir() {
    use crate::editor::{Editor, View};
    adw::init().expect("Se necesita una sesión gráfica GTK");
    crate::style::apply();
    let root = std::env::temp_dir().join(format!("orbital-055-dividida-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("wiki")).unwrap();
    let rel = orbital_vault::RelPath::new("wiki/codigo.md").unwrap();
    std::fs::write(rel.to_path(&root), NOTA).unwrap();
    let vault = std::sync::Arc::new(orbital_vault::open(&root).unwrap());
    let e = Editor::new();
    e.load(vault.clone(), &vault.read_note(&rel).unwrap());
    e.set_view(View::Dividida);
    let win = gtk::Window::builder()
        .default_width(1400)
        .default_height(1100)
        .child(e.widget())
        .build();
    win.present();
    glib::MainContext::default().block_on(async {
        pausa(300).await;
        let reader = e.0.reader.clone();
        esperar_resaltado(&reader).await;
        comprobar_colores(&reader);
        // Escribir dentro del bloque de Rust: se vuelve a volcar en el sitio y sale con color
        // en el mismo volcado (lenguaje ya cargado, bloque corto).
        let b = e.buffer();
        let (_, fin) = b
            .start_iter()
            .forward_search("return 42;", gtk::TextSearchFlags::empty(), None)
            .unwrap();
        let mut at = fin;
        b.insert(&mut at, "\n    let nuevo = \"recién\"; // escrito");
        let mut visto = false;
        for _ in 0..400 {
            let t = reader.buffer();
            let txt = t.text(&t.start_iter(), &t.end_iter(), true);
            if txt.contains("// escrito") {
                visto = true;
                assert!(
                    !sintaxis_en(&t, "\"recién\"").is_empty(),
                    "el bloque editado sale con color en el mismo volcado"
                );
                break;
            }
            pausa(5).await;
        }
        assert!(visto, "Dividida se actualizó");
        esperar_resaltado(&reader).await;
        comprobar_colores(&reader);
        pausa(300).await;
        crate::editor::graficos_tests::capturar(e.widget(), "055-dividida");
    });
    win.destroy();
    let _ = std::fs::remove_dir_all(root);
}

#[test]
#[ignore = "requiere una sesión gráfica; tools/pruebas-gtk.sh"]
fn codigo_resaltado_al_exportar() {
    glib::set_prgname(Some("orbital"));
    adw::init().expect("Se necesita una sesión gráfica GTK");
    let html = crate::export::html_md(NOTA, "codigo.md");
    assert!(
        html.contains("<code class=\"language-rust\"><span class=\"sx-c"),
        "{html}"
    );
    assert!(
        html.contains("<span class=\"sx-m\">// comentario rust</span>"),
        "{html}"
    );
    assert!(html.contains("<code class=\"language-py\">"), "{html}");
    assert!(html.contains(crate::resaltado::CSS_PAPEL), "{html}");
    // Sin lenguaje y desconocido, como antes (texto escapado sin colores).
    assert!(
        html.contains("<pre><code>fn sin_lenguaje() { \"texto plano\" }\n</code></pre>"),
        "{html}"
    );
    assert!(
        html.contains(
            "<code class=\"language-xyzlang\">fn desconocido() { \"texto plano\" }\n</code>"
        ),
        "{html}"
    );
    // Los especiales siguen como SVG.
    assert_eq!(html.matches("<svg").count(), 2, "{html}");
    assert!(!html.contains("http"), "{html}");
    glib::MainContext::default().block_on(async {
        use webkit6::prelude::*;
        let web = crate::html_view::new_view(None);
        let window = gtk::Window::builder()
            .default_width(820)
            .default_height(1300)
            .child(&web)
            .build();
        window.present();
        web.load_html(&crate::html_view::with_csp(&html), None);
        for _ in 0..100 {
            if !web.is_loading() {
                break;
            }
            pausa(50).await;
        }
        pausa(300).await;
        if let Some(dir) = std::env::var_os("ORBITAL_CAPTURA_DIR") {
            let t = web
                .snapshot_future(
                    webkit6::SnapshotRegion::FullDocument,
                    webkit6::SnapshotOptions::NONE,
                )
                .await
                .unwrap();
            let png = std::path::Path::new(&dir).join("055-exportado.png");
            t.save_to_png(&png).unwrap();
            println!("Captura: {}", png.display());
        }
        window.close();
    });
}
