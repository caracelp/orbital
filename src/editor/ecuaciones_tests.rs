//! Pruebas GTK en proceso del editor de ecuaciones (051): editar e insertar desde Código, la
//! paleta, la conversión de unidades, y la química y las unidades dibujadas en Lectura.

use std::fs;
use std::sync::Arc;
use std::time::Duration;

use crate::ecuaciones::EditorEcuaciones;
use crate::editor::{Editor, View};
use crate::formulas::Modo;
use adw::prelude::*;
use gtk::glib;
use orbital_vault::RelPath;

const NOTA: &str = "Sea $x^2$ en el renglón.\n\n$$\n\\frac{a}{b}\n$$\n\n```math\n\\sqrt{2}\n```\n\nTexto sin fórmula.\n";

const QUIMICA: &str = "# Química y unidades\n\nAgua: $\\ce{2H2 + O2 -> 2H2O}$ y el sulfato $\\ce{SO4^2-}$.\n\n$$\n\\ce{CO2 + C <=> 2CO}\n$$\n\nVelocidad: $v = \\qty{90}{km/h} = \\SI{25}{\\metre\\per\\second}$, a $\\qty{25}{\\degreeCelsius}$ y $\\qty{4.7}{\\kilo\\ohm}$.\n\n$$\n\\Delta H = \\pu{-285.8 kJ/mol}\n$$\n";

fn boveda(nombre: &str, notas: &[(&str, &str)]) -> (std::path::PathBuf, Arc<orbital_vault::Vault>) {
    let root = std::env::temp_dir().join(format!("orbital-051-{nombre}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join("wiki")).unwrap();
    for (ruta, texto) in notas {
        fs::write(root.join(ruta), texto).unwrap();
    }
    (root.clone(), Arc::new(orbital_vault::open(&root).unwrap()))
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

/// Con `ORBITAL_CAPTURA_DIR`, guarda `widget` como PNG.
fn capturar(widget: &impl IsA<gtk::Widget>, nombre: &str) {
    let Some(dir) = std::env::var_os("ORBITAL_CAPTURA_DIR") else {
        return;
    };
    let w = widget.as_ref();
    let (ancho, alto) = (w.width() as f32, w.height() as f32);
    let snap = gtk::Snapshot::new();
    snap.append_color(
        &crate::style::WINDOW_BG.rgba(1.0),
        &gtk::graphene::Rect::new(0.0, 0.0, ancho, alto),
    );
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

fn texto(e: &Editor) -> String {
    e.text()
}

fn poner(e: &Editor, aguja: &str) {
    let t = texto(e);
    let off = t.find(aguja).unwrap_or_else(|| panic!("{aguja} en {t}"));
    let b = e.buffer();
    b.place_cursor(&b.iter_at_offset(t[..off].chars().count() as i32 + 1));
}

fn cambiar(ed: &EditorEcuaciones, tex: &str) {
    ed.texto().buffer().set_text(tex);
}

#[test]
#[ignore = "requiere una sesión gráfica; tools/pruebas-gtk.sh"]
fn editor_de_ecuaciones_en_codigo() {
    adw::init().expect("Se necesita una sesión gráfica GTK");
    crate::style::apply();
    glib::MainContext::default().block_on(async {
        let (root, vault) = boveda(
            "codigo",
            &[("wiki/nota.md", NOTA), ("wiki/pagina.html", "<p>$x$</p>")],
        );
        let rel = RelPath::new("wiki/nota.md").unwrap();
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

        // 1. Editar una fórmula en línea: abre con su LaTeX y reescribe solo esa.
        poner(&editor, "x^2$");
        let ed = editor.abrir_ecuacion(false).expect("editor abierto");
        assert_eq!(ed.tex(), "x^2");
        assert!(
            ed.texto().has_focus() || ed.texto().is_focus(),
            "foco en el LaTeX"
        );
        assert!(
            esperar(|| ed.estado().0).await,
            "vista previa dibujada al abrir"
        );
        cambiar(&ed, r"\frac{1}{2} + y");
        assert!(esperar(|| ed.estado().0 && ed.estado().1.is_none()).await);
        // Con error: el motivo, y no se puede aceptar vacío.
        cambiar(&ed, r"\frac{1");
        assert!(
            esperar(|| ed.estado().1.is_some()).await,
            "error a la vista"
        );
        cambiar(&ed, "");
        assert!(!ed.boton_aceptar().is_sensitive());
        cambiar(&ed, r"\frac{1}{2} + y");
        // La paleta: la plantilla envuelve lo seleccionado.
        let b = ed.texto().buffer();
        b.select_range(&b.iter_at_offset(14), &b.end_iter());
        ed.insertar_plantilla(r"\sqrt{◌}");
        assert_eq!(ed.tex(), r"\frac{1}{2} + \sqrt{y}");
        ed.insertar_plantilla(r"\alpha");
        assert_eq!(ed.tex(), r"\frac{1}{2} + \sqrt{y}\alpha");
        glib::timeout_future(Duration::from_millis(300)).await;
        capturar(ed.dialogo(), "051-editor-ecuaciones");
        ed.boton_aceptar().emit_clicked();
        assert!(
            esperar(|| editor.0.view.has_focus()).await,
            "el foco vuelve a la nota"
        );
        assert_eq!(
            texto(&editor),
            NOTA.replace("$x^2$", r"$\frac{1}{2} + \sqrt{y}\alpha$")
        );
        // Una sola acción de deshacer.
        editor.undo();
        assert_eq!(texto(&editor), NOTA);

        // 2. `$$` en varios renglones: conserva los saltos.
        poner(&editor, "frac{a}");
        let ed = editor.abrir_ecuacion(false).unwrap();
        assert_eq!(ed.tex(), r"\frac{a}{b}");
        cambiar(&ed, r"\frac{a}{c}");
        ed.boton_aceptar().emit_clicked();
        assert_eq!(texto(&editor), NOTA.replace(r"\frac{a}{b}", r"\frac{a}{c}"));
        editor.undo();

        // 3. ```math.
        poner(&editor, "sqrt{2}");
        let ed = editor.abrir_ecuacion(false).unwrap();
        assert_eq!(ed.tex(), r"\sqrt{2}");
        cambiar(&ed, r"\sqrt{3}");
        ed.boton_aceptar().emit_clicked();
        assert_eq!(texto(&editor), NOTA.replace(r"\sqrt{2}", r"\sqrt{3}"));
        editor.undo();

        // 4. Sin fórmula bajo el cursor: inserta una nueva, en línea o en bloque.
        poner(&editor, "sin fórmula");
        let ed = editor.abrir_ecuacion(false).unwrap();
        assert_eq!(ed.tex(), "");
        cambiar(&ed, "a + b");
        ed.boton_aceptar().emit_clicked();
        assert_eq!(
            texto(&editor),
            NOTA.replace("Texto sin fórmula", "Texto s$a + b$in fórmula")
        );
        editor.undo();
        assert_eq!(texto(&editor), NOTA);
        let b = editor.buffer();
        b.place_cursor(&b.end_iter());
        let ed = editor.abrir_ecuacion(true).unwrap();
        ed.elegir_modo(Modo::Bloque);
        cambiar(&ed, r"\int_0^1 x\,dx");
        ed.boton_aceptar().emit_clicked();
        assert_eq!(texto(&editor), format!("{NOTA}\n$$\n\\int_0^1 x\\,dx\n$$"));
        editor.undo();
        // Cancelar no cambia nada.
        poner(&editor, "x^2$");
        let ed = editor.abrir_ecuacion(false).unwrap();
        cambiar(&ed, "zzz");
        ed.dialogo().close();
        assert_eq!(texto(&editor), NOTA);

        // 5. El menú contextual dice qué va a hacer.
        let rotulo = |e: &Editor| {
            e.0.view
                .extra_menu()
                .and_then(|m| m.item_attribute_value(0, "label", None))
                .and_then(|v| v.get::<String>())
                .unwrap_or_default()
        };
        editor.0.view.grab_focus();
        poner(&editor, "x^2$");
        editor.actualizar_vista_previa();
        assert_eq!(rotulo(&editor), "Editar ecuación…");
        poner(&editor, "sin fórmula");
        editor.actualizar_vista_previa();
        assert_eq!(rotulo(&editor), "Insertar ecuación…");

        // 6. Ni en Lectura ni en las notas .html.
        editor.set_view(View::Lectura);
        assert!(editor.abrir_ecuacion(false).is_none());
        let html = RelPath::new("wiki/pagina.html").unwrap();
        editor.load(vault.clone(), &vault.read_note(&html).unwrap());
        editor.set_view(View::Codigo);
        assert!(editor.abrir_ecuacion(false).is_none());

        // El disco: lo editado se guarda solo con el autoguardado; acá no se tocó.
        assert_eq!(fs::read_to_string(rel.to_path(&root)).unwrap(), NOTA);
        window.close();
        let _ = fs::remove_dir_all(&root);
    });
}

#[test]
#[ignore = "requiere una sesión gráfica; tools/pruebas-gtk.sh"]
fn conversion_de_unidades_en_el_editor() {
    adw::init().expect("Se necesita una sesión gráfica GTK");
    crate::style::apply();
    glib::MainContext::default().block_on(async {
        let window = gtk::Window::builder()
            .default_width(1000)
            .default_height(800)
            .build();
        window.present();
        let aceptado = std::rc::Rc::new(std::cell::RefCell::new(None));
        let a = aceptado.clone();
        let ed = EditorEcuaciones::presentar(
            &window,
            r"v = \qty{90}{km/h}",
            Modo::EnLinea,
            false,
            move |tex, modo| {
                a.replace(Some((tex, modo)));
            },
            || {},
        );
        // El cursor dentro de la cantidad.
        let b = ed.texto().buffer();
        b.place_cursor(&b.iter_at_offset(10));
        ed.ir_a("convertir");
        let (valor, desde, hacia, resultado, insertar) = ed.conversor();
        assert_eq!(valor.text(), "90");
        assert_eq!(desde.text(), "km/h");
        assert_eq!(insertar.label().as_deref(), Some("Reemplazar"));
        hacia.set_text("m/s");
        assert_eq!(resultado.label(), "= 25 m/s");
        assert!(insertar.is_sensitive());
        glib::timeout_future(Duration::from_millis(200)).await;
        capturar(ed.dialogo(), "051-convertir-unidades");
        insertar.emit_clicked();
        assert_eq!(ed.tex(), r"v = \qty{25}{m/s}");
        // Otra vez, desde lo insertado, a una unidad compuesta con comandos.
        hacia.set_text(r"\kilo\metre\per\hour");
        insertar.emit_clicked();
        assert_eq!(ed.tex(), r"v = \qty{90}{\kilo\metre\per\hour}");
        // Unidades que no miden lo mismo: el motivo, sin insertar.
        hacia.set_text("kg");
        assert!(
            resultado.label().contains("no miden lo mismo"),
            "{}",
            resultado.label()
        );
        assert!(!insertar.is_sensitive());
        // Temperaturas.
        valor.set_text("100");
        desde.set_text("°C");
        hacia.set_text("°F");
        assert_eq!(resultado.label(), "= 212 °F");
        ed.boton_aceptar().emit_clicked();
        assert_eq!(
            aceptado.borrow().clone(),
            Some((
                r"v = \qty{90}{\kilo\metre\per\hour}".to_owned(),
                Modo::EnLinea
            ))
        );
        window.close();
    });
}

#[test]
#[ignore = "requiere una sesión gráfica; tools/pruebas-gtk.sh"]
fn quimica_y_unidades_en_lectura() {
    adw::init().expect("Se necesita una sesión gráfica GTK");
    crate::style::apply();
    glib::MainContext::default().block_on(async {
        let (root, vault) = boveda("lectura", &[("wiki/quimica.md", QUIMICA)]);
        let rel = RelPath::new("wiki/quimica.md").unwrap();
        let editor = Editor::new();
        let window = gtk::Window::builder()
            .default_width(900)
            .default_height(560)
            .child(editor.widget())
            .build();
        window.present();
        editor.load(vault.clone(), &vault.read_note(&rel).unwrap());
        editor.set_view(View::Lectura);
        editor.render_now();
        let cuantas = |e: &Editor| {
            let b = e.0.reader.buffer();
            let mut n = 0;
            let mut it = b.start_iter();
            loop {
                if it.paintable().is_some() {
                    n += 1;
                }
                if !it.forward_char() {
                    break;
                }
            }
            n
        };
        assert!(
            esperar(|| cuantas(&editor) == 7).await,
            "siete fórmulas dibujadas: {}",
            cuantas(&editor)
        );
        let b = editor.0.reader.buffer();
        let error = b.tag_table().lookup("formula-error");
        let mut it = b.start_iter();
        loop {
            if let Some(t) = &error {
                assert!(!it.has_tag(t), "ninguna con error");
            }
            if !it.forward_char() {
                break;
            }
        }
        glib::timeout_future(Duration::from_millis(400)).await;
        capturar(&window, "051-quimica-unidades-lectura");
        window.close();
        let _ = fs::remove_dir_all(&root);
    });
}
