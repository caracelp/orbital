//! Pruebas GTK en proceso de los diagramas Mermaid (058): Lectura con los nueve tipos y un error,
//! en oscuro y con una vista clara; una nota sin diagramas no carga el motor; el globito de
//! Código. Con `ORBITAL_CAPTURA_DIR`, guardan capturas.

use std::fs;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::editor::{Editor, View};
use adw::prelude::*;
use gtk::glib;
use orbital_vault::RelPath;

pub(crate) const NOTA: &str = r#"# Diagramas

## Flujo

```mermaid
flowchart TD
    A[Idea en una nota] --> B{¿Tiene enlaces?}
    B -->|Sí| C[Conectar con el grafo]
    B -->|No| D[Etiquetar]
    C --> E((Listo))
    D --> E
    E -.-> F[(Bóveda)]
```

## Secuencia

```mermaid
sequenceDiagram
    participant U as Usuaria
    participant O as Orbital
    participant D as Disco
    U->>O: Escribe la nota
    O->>D: Guarda (autoguardado)
    D-->>O: Listo
    Note over O,D: Fuera del hilo de GTK
    O-->>U: Muestra la Lectura
```

## Clases

```mermaid
classDiagram
    class Nota {
        +String titulo
        +Vec~Etiqueta~ etiquetas
        +guardar() bool
    }
    class Boveda {
        +PathBuf raiz
        +abrir(ruta) Nota
    }
    Boveda "1" --> "*" Nota : contiene
    Nota <|-- NotaHtml
```

## Estados

```mermaid
stateDiagram-v2
    [*] --> Cerrada
    Cerrada --> Abierta : abrir
    Abierta --> Editando : escribir
    Editando --> Abierta : guardar
    Abierta --> [*] : cerrar
```

## Entidades

```mermaid
erDiagram
    BOVEDA ||--o{ NOTA : contiene
    NOTA }o--o{ ETIQUETA : tiene
    NOTA {
        string titulo
        date creada
    }
```

## Gantt

```mermaid
gantt
    title Plan del bloque D
    dateFormat YYYY-MM-DD
    section Fórmulas
    LaTeX           :a1, 2026-11-01, 5d
    Editor          :after a1, 4d
    section Gráficos
    2D y polares    :g1, 2026-11-10, 4d
    3D              :after g1, 3d
```

## Torta

```mermaid
pie title Notas por tipo
    "Markdown" : 68
    "HTML" : 22
    "Bases" : 10
```

## Mapa mental

```mermaid
mindmap
  root((Orbital))
    Notas
      Markdown
      HTML
    Vistas
      Lectura
      Código
    Visual Math
```

## Línea de tiempo

```mermaid
timeline
    title Historia de Orbital
    2026-10 : Bóveda : Editor
    2026-11 : Fórmulas LaTeX : Gráficos 2D y 3D
            : Audio y video
```

## Con un error

```mermaid
flowchart TD
    A --> B
    B -->> C[
```
"#;

/// Una bóveda temporal con un nombre no ASCII en la ruta.
fn boveda(nombre: &str, texto: &str) -> (std::path::PathBuf, Arc<orbital_vault::Vault>, RelPath) {
    let root = std::env::temp_dir().join(format!(
        "orbital-058-{nombre}-Bóveda ñandú-{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join("wiki")).unwrap();
    let rel = RelPath::new("wiki/diagramas.md").unwrap();
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

/// Las imágenes de diagramas de un widget, en orden.
fn diagramas(w: &gtk::Widget) -> Vec<gtk::Picture> {
    let mut out = Vec::new();
    let mut hijo = w.first_child();
    while let Some(h) = hijo {
        match h.downcast_ref::<gtk::Picture>() {
            Some(p) if p.has_css_class("orbital-diagrama") => out.push(p.clone()),
            _ => out.extend(diagramas(&h)),
        }
        hijo = h.next_sibling();
    }
    out
}

/// Guarda `widget` sobre un fondo `fondo` (con `ORBITAL_CAPTURA_DIR`).
fn capturar(widget: &impl IsA<gtk::Widget>, nombre: &str, fondo: gtk::gdk::RGBA) {
    let Some(dir) = std::env::var_os("ORBITAL_CAPTURA_DIR") else {
        return;
    };
    let w = widget.as_ref();
    let (ancho, alto) = (w.width() as f32, w.height() as f32);
    let snap = gtk::Snapshot::new();
    snap.append_color(&fondo, &gtk::graphene::Rect::new(0.0, 0.0, ancho, alto));
    gtk::WidgetPaintable::new(Some(w)).snapshot(&snap, f64::from(ancho), f64::from(alto));
    let node = snap.to_node().expect("cuadro");
    let png = std::path::Path::new(&dir).join(format!("{nombre}.png"));
    w.root()
        .and_then(|r| r.renderer())
        .expect("renderizador")
        .render_texture(&node, None)
        .save_to_png(&png)
        .unwrap();
    println!("Captura: {}", png.display());
}

const NOMBRES: [&str; 9] = [
    "Diagrama Mermaid: de flujo",
    "Diagrama Mermaid: de secuencia",
    "Diagrama Mermaid: de clases",
    "Diagrama Mermaid: de estados",
    "Diagrama Mermaid: entidad-relación",
    "Diagrama Mermaid: de Gantt",
    "Diagrama Mermaid: de torta",
    "Diagrama Mermaid: mapa mental",
    "Diagrama Mermaid: línea de tiempo",
];

#[test]
#[ignore = "requiere una sesión gráfica; tools/pruebas-gtk.sh"]
fn diagramas_en_lectura() {
    adw::init().expect("Se necesita una sesión gráfica GTK");
    crate::style::apply();
    glib::MainContext::default().block_on(async {
        let (root, vault, rel) = boveda("lectura", NOTA);
        let editor = Editor::new();
        let window = gtk::Window::builder()
            .default_width(1000)
            .default_height(1100)
            .child(editor.widget())
            .build();
        window.present();
        editor.load(vault.clone(), &vault.read_note(&rel).unwrap());
        editor.set_view(View::Lectura);
        editor.render_now();
        let vista = editor.0.reader.text_view().clone();
        // Los widgets se anclan por tandas cerca de lo visible: se recorre la nota.
        assert!(esperar(|| !diagramas(vista.upcast_ref()).is_empty()).await);
        for h in ["Entidades", "Torta", "Con un error"] {
            editor.0.reader.scroll_to_heading(h);
            glib::timeout_future(Duration::from_millis(300)).await;
        }
        assert!(
            esperar(|| diagramas(vista.upcast_ref()).len() == 9).await,
            "nueve diagramas: {}",
            diagramas(vista.upcast_ref()).len()
        );
        editor.0.reader.scroll_to_heading("Diagramas");
        let ds = diagramas(vista.upcast_ref());
        assert!(
            esperar(|| ds.iter().all(|p| p.paintable().is_some())).await,
            "todos dibujados"
        );
        for (p, n) in ds.iter().zip(NOMBRES) {
            assert_eq!(p.alternative_text().as_deref(), Some(n));
            assert_eq!(p.accessible_role(), gtk::AccessibleRole::Img);
            // Ninguno más ancho que la columna.
            assert!(p.width_request() <= editor.0.reader.text_view().width());
        }
        // El de error: el fuente como código y el motivo con el renglón, como texto.
        let buf = editor.0.reader.buffer();
        let texto = buf.text(&buf.start_iter(), &buf.end_iter(), true);
        assert!(
            texto.contains("⚠ Error en el diagrama (renglón 3): "),
            "{texto}"
        );
        assert!(texto.contains("B -->> C["), "{texto}");
        let oscuro = crate::style::VIEW_BG.rgba(1.0);
        glib::timeout_future(Duration::from_millis(300)).await;
        let lector = editor.0.reader.widget().clone();
        capturar(&lector, "058-lectura-oscuro-1", oscuro);
        editor.0.reader.scroll_to_heading("Entidades");
        glib::timeout_future(Duration::from_millis(400)).await;
        capturar(&lector, "058-lectura-oscuro-2", oscuro);
        editor.0.reader.scroll_to_heading("Torta");
        glib::timeout_future(Duration::from_millis(400)).await;
        capturar(&lector, "058-lectura-oscuro-3", oscuro);
        editor.0.reader.scroll_to_heading("Con un error");
        glib::timeout_future(Duration::from_millis(400)).await;
        capturar(&lector, "058-error", oscuro);

        // Una vista clara (Orbital es oscuro; el CSS es de la prueba): los diagramas pasan a la
        // paleta clara al volver a volcar.
        let css = gtk::CssProvider::new();
        css.load_from_string(
            ".orbital-reader, .orbital-reader text { background-color: #ffffff; color: #1a1a1c; }",
        );
        gtk::style_context_add_provider_for_display(
            &gtk::gdk::Display::default().unwrap(),
            &css,
            gtk::STYLE_PROVIDER_PRIORITY_USER + 10,
        );
        assert_eq!(
            editor.0.reader.estilo_diagramas().tema,
            crate::diagramas::Tema::Claro
        );
        // El tema solo cambia en una vista nueva (la Lectura no vuelve a volcar los bloques que
        // no cambiaron; Orbital no cambia de tema en marcha).
        window.close();
        let editor = Editor::new();
        let window = gtk::Window::builder()
            .default_width(1000)
            .default_height(1100)
            .child(editor.widget())
            .build();
        window.present();
        editor.load(vault.clone(), &vault.read_note(&rel).unwrap());
        editor.set_view(View::Lectura);
        editor.render_now();
        let lector = editor.0.reader.widget().clone();
        let vista = editor.0.reader.text_view().clone();
        assert!(esperar(|| !diagramas(vista.upcast_ref()).is_empty()).await);
        for h in ["Entidades", "Torta", "Con un error"] {
            editor.0.reader.scroll_to_heading(h);
            glib::timeout_future(Duration::from_millis(300)).await;
        }
        assert!(
            esperar(|| {
                let ds = diagramas(vista.upcast_ref());
                ds.len() == 9 && ds.iter().all(|p| p.paintable().is_some())
            })
            .await,
            "dibujados en claro"
        );
        let blanco = gtk::gdk::RGBA::new(1.0, 1.0, 1.0, 1.0);
        editor.0.reader.scroll_to_heading("Diagramas");
        glib::timeout_future(Duration::from_millis(400)).await;
        capturar(&lector, "058-lectura-claro-1", blanco);
        editor.0.reader.scroll_to_heading("Entidades");
        glib::timeout_future(Duration::from_millis(400)).await;
        capturar(&lector, "058-lectura-claro-2", blanco);
        editor.0.reader.scroll_to_heading("Torta");
        glib::timeout_future(Duration::from_millis(400)).await;
        capturar(&lector, "058-lectura-claro-3", blanco);
        gtk::style_context_remove_provider_for_display(
            &gtk::gdk::Display::default().unwrap(),
            &css,
        );
        // El disco no cambia.
        assert_eq!(fs::read_to_string(rel.to_path(&root)).unwrap(), NOTA);
        window.close();
        let _ = fs::remove_dir_all(&root);
    });
}

/// Una nota sin diagramas no toca el motor (ni sus fuentes).
#[test]
#[ignore = "requiere una sesión gráfica; tools/pruebas-gtk.sh"]
fn sin_diagramas_no_carga_el_motor() {
    adw::init().expect("Se necesita una sesión gráfica GTK");
    crate::style::apply();
    glib::MainContext::default().block_on(async {
        let nota =
            "# Sin diagramas\n\n```rust\nlet mermaid = 1;\n```\n\nTexto con la palabra mermaid.\n";
        let (root, vault, rel) = boveda("sin", nota);
        let editor = Editor::new();
        let window = gtk::Window::builder()
            .default_width(800)
            .default_height(600)
            .child(editor.widget())
            .build();
        window.present();
        editor.load(vault.clone(), &vault.read_note(&rel).unwrap());
        editor.set_view(View::Lectura);
        editor.render_now();
        glib::timeout_future(Duration::from_millis(500)).await;
        let buf = editor.0.reader.buffer();
        let texto = buf.text(&buf.start_iter(), &buf.end_iter(), true);
        assert!(texto.contains("Texto con la palabra"), "{texto}");
        assert!(!crate::diagramas::usado(), "el motor no se cargó");
        window.close();
        let _ = fs::remove_dir_all(&root);
    });
}

#[test]
#[ignore = "requiere una sesión gráfica; tools/pruebas-gtk.sh"]
fn globito_de_diagrama_en_codigo() {
    adw::init().expect("Se necesita una sesión gráfica GTK");
    crate::style::apply();
    glib::MainContext::default().block_on(async {
        let nota = "Antes.\n\n```mermaid\nsequenceDiagram\n    Usuaria->>Orbital: Escribe\n    Orbital-->>Usuaria: Muestra\n```\n\n```mermaid\ngraph TD\n    A -->> B[\n```\n\nDespués.\n";
        let (root, vault, rel) = boveda("globito", nota);
        let editor = Editor::new();
        let window = gtk::Window::builder()
            .default_width(1000)
            .default_height(800)
            .child(editor.widget())
            .build();
        window.present();
        editor.load(vault.clone(), &vault.read_note(&rel).unwrap());
        editor.set_view(View::Codigo);
        editor.0.view.grab_focus();
        let b = editor.buffer().clone();
        let poner = |aguja: &str| {
            let t = editor.text();
            let off = t.find(aguja).unwrap();
            b.place_cursor(&b.iter_at_offset(t[..off].chars().count() as i32 + 1));
        };
        poner("Escribe");
        editor.actualizar_vista_previa();
        let con_imagen = |e: &Editor| {
            e.0.vista_previa.borrow().as_ref().is_some_and(|v| {
                v.popover.is_visible()
                    && diagramas(v.grafico.upcast_ref())
                        .first()
                        .is_some_and(|p| p.paintable().is_some())
            })
        };
        assert!(esperar(|| con_imagen(&editor)).await, "globito con el diagrama");
        glib::timeout_future(Duration::from_millis(200)).await;
        crate::editor::graficos_tests::capturar_con_globito(&window, &editor, "058-globito");
        // En el que tiene un error, el motivo.
        poner("A -->>");
        editor.actualizar_vista_previa();
        let error = |e: &Editor| {
            e.0.vista_previa
                .borrow()
                .as_ref()
                .is_some_and(|v| v.error.is_visible() && v.error.label().contains("renglón 2"))
        };
        assert!(esperar(|| error(&editor)).await, "globito con el error");
        window.close();
        let _ = fs::remove_dir_all(&root);
    });
}

/// Mide el volcado de la nota con los nueve tipos: tiempo hasta tener todas las imágenes y el
/// mayor hueco del hilo de GTK (reloj de 1 ms), en frío (primera vez del proceso: fuentes y
/// motor) y en caliente (caché). `cargo test --release --bin orbital medir_lectura_diagramas --
/// --ignored --nocapture`.
#[test]
#[ignore = "medición; requiere una sesión gráfica"]
fn medir_lectura_diagramas() {
    adw::init().expect("Se necesita una sesión gráfica GTK");
    crate::style::apply();
    glib::MainContext::default().block_on(async {
        // Con `ORBITAL_MEDIR_SIN_DIAGRAMAS=1`, la misma nota con los bloques como código común:
        // lo que cuesta la nota sin los diagramas.
        let sin = std::env::var_os("ORBITAL_MEDIR_SIN_DIAGRAMAS").is_some();
        let nota = if sin {
            NOTA.replace("```mermaid", "```texto")
        } else {
            NOTA.to_owned()
        };
        let (root, vault, rel) = boveda("medir", &nota);
        let editor = Editor::new();
        let window = gtk::Window::builder()
            .default_width(1000)
            .default_height(1100)
            .child(editor.widget())
            .build();
        window.present();
        glib::timeout_future(Duration::from_millis(500)).await;
        let hueco = std::rc::Rc::new(std::cell::Cell::new(Duration::ZERO));
        let ultimo = std::rc::Rc::new(std::cell::Cell::new(Instant::now()));
        let (h, u) = (hueco.clone(), ultimo.clone());
        let reloj = glib::timeout_add_local(Duration::from_millis(1), move || {
            let ahora = Instant::now();
            h.set(h.get().max(ahora - u.get()));
            u.set(ahora);
            glib::ControlFlow::Continue
        });
        let estilo = editor.0.reader.estilo_diagramas();
        let claves: Vec<crate::diagramas::Clave> = NOTA
            .split("```mermaid\n")
            .skip(1)
            .map(|t| crate::diagramas::Clave::new(&t[..t.find("```").unwrap()], estilo))
            .collect();
        for vuelta in ["frío", "caliente"] {
            hueco.set(Duration::ZERO);
            ultimo.set(Instant::now());
            let t = Instant::now();
            editor.load(vault.clone(), &vault.read_note(&rel).unwrap());
            editor.set_view(View::Lectura);
            editor.render_now();
            let vista = editor.0.reader.text_view().clone();
            if sin {
                glib::timeout_future(Duration::from_millis(500)).await;
                for h in ["Entidades", "Torta", "Con un error"] {
                    editor.0.reader.scroll_to_heading(h);
                    glib::timeout_future(Duration::from_millis(200)).await;
                }
                println!(
                    "{vuelta}, sin diagramas: mayor hueco del hilo de GTK {:.1} ms",
                    hueco.get().as_secs_f64() * 1e3
                );
                editor.set_view(View::Codigo);
                glib::timeout_future(Duration::from_millis(300)).await;
                continue;
            }
            assert!(
                esperar(|| diagramas(vista.upcast_ref())
                    .first()
                    .is_some_and(|p| p.paintable().is_some()))
                .await
            );
            let primero = t.elapsed();
            assert!(
                esperar(|| claves
                    .iter()
                    .filter(|c| crate::diagramas::en_cache(c).is_some())
                    .count()
                    >= 9)
                .await
            );
            let todos = t.elapsed();
            // Recorrer la nota: se anclan los demás.
            for h in ["Entidades", "Torta", "Con un error"] {
                editor.0.reader.scroll_to_heading(h);
                glib::timeout_future(Duration::from_millis(200)).await;
            }
            assert!(esperar(|| diagramas(vista.upcast_ref()).len() == 9).await);
            println!(
                "{vuelta}: primer diagrama a la vista en {:.0} ms; las nueve imágenes en {:.0} ms; mayor hueco del hilo de GTK (al volcar y al recorrer) {:.1} ms",
                primero.as_secs_f64() * 1e3,
                todos.as_secs_f64() * 1e3,
                hueco.get().as_secs_f64() * 1e3
            );
            editor.set_view(View::Codigo);
            glib::timeout_future(Duration::from_millis(300)).await;
        }
        reloj.remove();
        window.close();
        let _ = fs::remove_dir_all(&root);
    });
}
