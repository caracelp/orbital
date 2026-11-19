//! Pruebas GTK en proceso de los gráficos (052): Lectura con cada tipo, la interacción (mover,
//! acercar, teclado, deslizador), el editor (página «Gráfico») y el globito de Código.

use std::fs;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::editor::{Editor, View};
use crate::graficos::{GraficoVista, Lienzo};
use adw::prelude::*;
use gtk::glib;
use orbital_vault::RelPath;

pub(crate) const NOTA: &str = r"# Gráficos

Funciones con asíntotas, en las dos notaciones:

```grafico
título: Funciones
x: -6..6
y: -4..4
y = \frac{1}{x}
y = tan(x) ; nombre: tan x
y = \sin(2x) + x/4
```

## Polar con deslizador

```grafico
título: Rosa polar
a = 3 (1..8, paso 1)
r = 2\cos(a\theta)
x: -3..3
y: -2.5..2.5
```

## Paramétrica e implícita

```grafico
título: Curvas
x: -4..4
y: -3..3
(2\cos t, \sin 3t) ; nombre: Lissajous
x^2/9 + y^2/4 = 1 ; nombre: elipse ; estilo: discontinua
x^2 - y^2 = 1 ; color: verde
```

## Datos

```grafico
título: Mediciones
x: -1..6
y: -1..30
datos: ambos
| t | v |
|---|---|
| 0 | 1 |
| 1 | 3 |
| 2 | 7 |
| 3 | 12 |
| 4 | 18 |
| 5 | 26 |
y = x^2 + 1 ; nombre: modelo ; color: celeste
(2.5, 20) ; nombre: atípico ; color: rojo
```

## Con un error

```grafico
y = k x
```
";

fn boveda(nombre: &str, texto: &str) -> (std::path::PathBuf, Arc<orbital_vault::Vault>, RelPath) {
    let root = std::env::temp_dir().join(format!("orbital-052-{nombre}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join("wiki")).unwrap();
    let rel = RelPath::new("wiki/graficos.md").unwrap();
    fs::write(rel.to_path(&root), texto).unwrap();
    (
        root.clone(),
        Arc::new(orbital_vault::open(&root).unwrap()),
        rel,
    )
}

async fn esperar(mut f: impl FnMut() -> bool) -> bool {
    for _ in 0..200 {
        if f() {
            return true;
        }
        glib::timeout_future(Duration::from_millis(25)).await;
    }
    false
}

/// Los lienzos de los gráficos de un widget, en orden.
pub(crate) fn lienzos(w: &gtk::Widget) -> Vec<Lienzo> {
    let mut out = Vec::new();
    let mut hijo = w.first_child();
    while let Some(h) = hijo {
        if let Some(l) = h.downcast_ref::<Lienzo>() {
            out.push(l.clone());
        } else {
            out.extend(lienzos(&h));
        }
        hijo = h.next_sibling();
    }
    out
}

/// Con `ORBITAL_CAPTURA_DIR`, guarda `widget` como PNG.
pub(crate) fn capturar(widget: &impl IsA<gtk::Widget>, nombre: &str) {
    let Some(dir) = std::env::var_os("ORBITAL_CAPTURA_DIR") else {
        return;
    };
    let w = widget.as_ref();
    let (ancho, alto) = (w.width() as f32, w.height() as f32);
    let snap = gtk::Snapshot::new();
    snap.append_color(
        &crate::style::VIEW_BG.rgba(1.0),
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

#[test]
#[ignore = "requiere una sesión gráfica; tools/pruebas-gtk.sh"]
fn graficos_en_lectura() {
    adw::init().expect("Se necesita una sesión gráfica GTK");
    crate::style::apply();
    glib::MainContext::default().block_on(async {
        let (root, vault, rel) = boveda("lectura", NOTA);
        let editor = Editor::new();
        let window = gtk::Window::builder()
            .default_width(900)
            .default_height(1000)
            .child(editor.widget())
            .build();
        window.present();
        editor.load(vault.clone(), &vault.read_note(&rel).unwrap());
        editor.set_view(View::Lectura);
        editor.render_now();
        let vista = editor.0.reader.text_view().clone();
        assert!(
            esperar(|| lienzos(vista.upcast_ref()).len() == 4).await,
            "cuatro gráficos: {}",
            lienzos(vista.upcast_ref()).len()
        );
        let ls = lienzos(vista.upcast_ref());
        assert!(
            esperar(|| ls.iter().all(|l| l.muestreos() > 0)).await,
            "todos muestreados"
        );
        // Cada uno con su nombre accesible (las expresiones) y enfocable.
        let nombres = [
            "Gráfico: Funciones. y = \\frac{1}{x}; y = tan(x); y = \\sin(2x) + x/4.",
            "Gráfico: Rosa polar. r = 2\\cos(a\\theta).",
            "Gráfico: Curvas. (2\\cos t, \\sin 3t); x^2/9 + y^2/4 = 1; x^2 - y^2 = 1.",
            "Gráfico: Mediciones. datos: v (6 puntos); y = x^2 + 1; (2.5, 20).",
        ];
        for (l, n) in ls.iter().zip(nombres) {
            let g = l.grafico().unwrap();
            assert!(g.descripcion().starts_with(n), "{}", g.descripcion());
            assert!(l.is_focusable());
            assert_eq!(l.accessible_role(), gtk::AccessibleRole::Img);
        }
        // El deslizador del parámetro de la rosa.
        let sliders = {
            let mut v = Vec::new();
            fn escalas(w: &gtk::Widget, v: &mut Vec<gtk::Scale>) {
                let mut h = w.first_child();
                while let Some(x) = h {
                    if let Some(s) = x.downcast_ref::<gtk::Scale>() {
                        v.push(s.clone());
                    } else {
                        escalas(&x, v);
                    }
                    h = x.next_sibling();
                }
            }
            escalas(vista.upcast_ref(), &mut v);
            v
        };
        assert_eq!(sliders.len(), 1);
        assert_eq!(sliders[0].value(), 3.0);
        // El gráfico con error: el fuente como código y el motivo como texto.
        let buf = editor.0.reader.buffer();
        let texto = buf.text(&buf.start_iter(), &buf.end_iter(), true);
        assert!(
            texto.contains("⚠ Error en el gráfico (renglón 1): «k» no está definido"),
            "{texto}"
        );
        assert!(texto.contains("y = k x"), "{texto}");
        glib::timeout_future(Duration::from_millis(300)).await;
        capturar(&editor.0.reader.widget().clone(), "052-lectura-1");
        editor.0.reader.scroll_to_heading("Datos");
        glib::timeout_future(Duration::from_millis(400)).await;
        capturar(&editor.0.reader.widget().clone(), "052-lectura-2");
        // El disco no cambia.
        assert_eq!(fs::read_to_string(rel.to_path(&root)).unwrap(), NOTA);
        window.close();
        let _ = fs::remove_dir_all(&root);
    });
}

#[test]
#[ignore = "requiere una sesión gráfica; tools/pruebas-gtk.sh"]
fn grafico_interactivo() {
    adw::init().expect("Se necesita una sesión gráfica GTK");
    crate::style::apply();
    glib::MainContext::default().block_on(async {
        let fuente = "a = 1 (0..5)\ny = a \\sin x\nx^2 + y^2 = 9";
        let g = crate::graficos::leer(fuente).unwrap();
        let gv = GraficoVista::new(g, fuente, true);
        let window = gtk::Window::builder()
            .default_width(700)
            .default_height(500)
            .child(gv.widget())
            .build();
        window.present();
        let l = gv.lienzo().clone();
        assert!(esperar(|| l.muestreos() > 0).await);
        let v0 = l.vista().unwrap();
        assert_eq!((v0.x0, v0.x1), (-10.0, 10.0));
        // Teclado: flechas mueven, + y − acercan y alejan, 0 vuelve.
        assert!(l.tecla(gtk::gdk::Key::Right));
        let v1 = l.vista().unwrap();
        assert!(v1.x0 > v0.x0, "a la derecha");
        assert!(l.tecla(gtk::gdk::Key::plus));
        let v2 = l.vista().unwrap();
        assert!((v2.x1 - v2.x0) < (v1.x1 - v1.x0) * 0.9, "acerca");
        assert!(l.tecla(gtk::gdk::Key::minus));
        // La rueda acerca alrededor del puntero (el punto bajo el puntero queda quieto).
        let antes = l.vista().unwrap();
        let (px, py) = (100.0, 80.0);
        let bajo = antes.mundo(px, py);
        l.acercar(0.5, px, py);
        let despues = l.vista().unwrap();
        let bajo2 = despues.mundo(px, py);
        assert!((bajo.0 - bajo2.0).abs() < 1e-9 && (bajo.1 - bajo2.1).abs() < 1e-9);
        // Arrastrar es mover la vista.
        l.fijar_vista(despues.mover(50.0, -20.0));
        let n = l.muestreos();
        assert!(esperar(|| !l.ocupado() && l.muestreos() > n).await);
        assert!(l.tecla(gtk::gdk::Key::_0));
        let v = l.vista().unwrap();
        assert_eq!((v.x0, v.x1), (-10.0, 10.0), "vuelve a la vista original");
        // El deslizador vuelve a muestrear con el valor nuevo, fuera del hilo de GTK: cada
        // cambio deja el hilo libre enseguida.
        let s = gv.deslizadores()[0].clone();
        let n = l.muestreos();
        let mut peor = Duration::ZERO;
        for k in 1..=20 {
            let t = Instant::now();
            s.set_value(f64::from(k) * 0.25);
            peor = peor.max(t.elapsed());
        }
        assert!(esperar(|| !l.ocupado() && l.muestreos() > n).await);
        println!("Deslizador: peor bloqueo del hilo de GTK por cambio {peor:?}");
        assert!(peor < Duration::from_millis(16), "{peor:?}");
        capturar(gv.widget(), "052-interactivo");
        window.close();
    });
}

#[test]
#[ignore = "requiere una sesión gráfica; tools/pruebas-gtk.sh"]
fn editor_de_graficos_en_codigo() {
    adw::init().expect("Se necesita una sesión gráfica GTK");
    crate::style::apply();
    glib::MainContext::default().block_on(async {
        let nota = "Antes.\n\n```grafico\ny = x^2\n```\n\nDespués.\n";
        let (root, vault, rel) = boveda("editor", nota);
        let editor = Editor::new();
        let window = gtk::Window::builder()
            .default_width(1000)
            .default_height(860)
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

        // 1. El globito de Código muestra el gráfico bajo el cursor.
        poner("x^2");
        editor.actualizar_vista_previa();
        let globito = |e: &Editor| {
            e.0.vista_previa
                .borrow()
                .as_ref()
                .is_some_and(|v| v.popover.is_visible() && v.grafico.is_visible())
        };
        assert!(esperar(|| globito(&editor)).await, "globito con el gráfico");
        {
            let ls = {
                let v = editor.0.vista_previa.borrow();
                lienzos(v.as_ref().unwrap().grafico.upcast_ref())
            };
            assert_eq!(ls.len(), 1);
            assert!(!ls[0].is_focusable(), "estático en el globito");
            assert!(esperar(|| ls[0].muestreos() > 0).await);
        }
        glib::timeout_future(Duration::from_millis(200)).await;
        capturar_con_globito(&window, &editor, "052-globito");

        // 2. Ctrl+M dentro del bloque: la página «Gráfico» con su fuente.
        let ed = editor.abrir_ecuacion(false).expect("editor abierto");
        assert_eq!(ed.fuente_grafico(), "y = x^2");
        assert!(esperar(|| ed.estado_grafico().0.is_some()).await);
        let campo = ed.texto_grafico().unwrap();
        campo.buffer().set_text("título: Editado\na = 2 (0..4)\ny = a\\sin x");
        ed.dibujar_grafico();
        let (g, error) = ed.estado_grafico();
        assert!(error.is_none(), "{error:?}");
        let g = g.unwrap();
        assert_eq!(g.deslizadores().len(), 1);
        assert!(esperar(|| g.lienzo().muestreos() > 0).await);
        // Una plantilla va en un renglón nuevo; un atributo, al final del renglón.
        campo.buffer().place_cursor(&campo.buffer().end_iter());
        ed.insertar_plantilla_grafico("r = 1 + \\cos\\theta");
        ed.insertar_plantilla_grafico(" ; color: celeste");
        assert_eq!(
            ed.fuente_grafico(),
            "título: Editado\na = 2 (0..4)\ny = a\\sin x\nr = 1 + \\cos\\theta ; color: celeste"
        );
        // Con un error: el motivo, y queda el último gráfico bueno.
        campo.buffer().set_text("y = (x");
        ed.dibujar_grafico();
        let (g2, error) = ed.estado_grafico();
        assert!(error.unwrap().contains("renglón 1"));
        assert!(g2.is_some());
        campo
            .buffer()
            .set_text("título: Editado\na = 2 (0..4)\ny = a\\sin x\nr = 1 + \\cos\\theta");
        ed.dibujar_grafico();
        glib::timeout_future(Duration::from_millis(300)).await;
        crate::editor::graficos_tests::capturar(ed.dialogo(), "052-editor-grafico");
        ed.boton_aceptar().emit_clicked();
        let esperado = "Antes.\n\n```grafico\ntítulo: Editado\na = 2 (0..4)\ny = a\\sin x\nr = 1 + \\cos\\theta\n```\n\nDespués.\n";
        assert_eq!(editor.text(), esperado);
        // Una sola acción de deshacer.
        b.undo();
        assert_eq!(editor.text(), nota);

        // 3. Insertar un gráfico nuevo en su propio párrafo (paleta de comandos).
        poner("Antes");
        let ed = editor.insertar_grafico().expect("editor abierto");
        assert_eq!(ed.fuente_grafico(), "y = x^2");
        ed.boton_aceptar().emit_clicked();
        assert!(
            editor
                .text()
                .starts_with("A\n\n```grafico\ny = x^2\n```\n\nntes."),
            "{}",
            editor.text()
        );
        b.undo();
        assert_eq!(editor.text(), nota);

        // 4. Ctrl+M fuera de todo: las dos páginas; desde «Gráfico» se inserta un gráfico.
        poner("Después");
        b.place_cursor(&b.end_iter());
        let ed = editor.abrir_ecuacion(false).unwrap();
        ed.ir_a_seccion("grafico");
        assert_eq!(ed.fuente_grafico(), "y = x^2");
        ed.boton_aceptar().emit_clicked();
        assert_eq!(
            editor.text(),
            format!("{nota}\n```grafico\ny = x^2\n```")
        );
        window.close();
        let _ = fs::remove_dir_all(&root);
    });
}

/// La ventana con el globito encima (el globito es otra superficie).
fn capturar_con_globito(window: &gtk::Window, editor: &Editor, nombre: &str) {
    let Some(dir) = std::env::var_os("ORBITAL_CAPTURA_DIR") else {
        return;
    };
    let (ancho, alto) = (window.width() as f32, window.height() as f32);
    let snap = gtk::Snapshot::new();
    snap.append_color(
        &crate::style::VIEW_BG.rgba(1.0),
        &gtk::graphene::Rect::new(0.0, 0.0, ancho, alto),
    );
    for w in [editor.0.code_scroller.clone().upcast::<gtk::Widget>()]
        .into_iter()
        .chain(
            editor
                .0
                .vista_previa
                .borrow()
                .as_ref()
                .map(|v| v.popover.clone().upcast()),
        )
    {
        if let Some(b) = w.compute_bounds(window) {
            snap.save();
            snap.translate(&gtk::graphene::Point::new(b.x(), b.y()));
            gtk::WidgetPaintable::new(Some(&w)).snapshot(
                &snap,
                f64::from(b.width()),
                f64::from(b.height()),
            );
            snap.restore();
        }
    }
    let node = snap.to_node().expect("cuadro");
    let png = std::path::Path::new(&dir).join(format!("{nombre}.png"));
    window
        .renderer()
        .expect("renderizador")
        .render_texture(&node, None)
        .save_to_png(&png)
        .unwrap();
    println!("Captura: {}", png.display());
}
