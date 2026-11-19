//! Pruebas GTK en proceso de los gráficos 3D (053): Lectura con cada tipo, la interacción
//! (girar, acercar, teclado, deslizador, el punto bajo el puntero), las implícitas que se
//! refinan, el editor (página «Gráfico»), OnStage y exportar.

use std::fs;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::editor::graficos_tests::{capturar, lienzos};
use crate::editor::{Editor, View};
use crate::graficos::{GraficoVista, Lienzo};
use adw::prelude::*;
use gtk::glib;
use orbital_vault::RelPath;
use webkit6::prelude::*;

pub(crate) const NOTA: &str = r"# Gráficos 3D

## Superficie

```grafico
título: Silla de montar
a = 1 (0..2)
z = a(x^2 - y^2)
```

## Curva en el espacio

```grafico
título: Hélice
(\cos t, \sin t, t/6) ; t: 0..6π ; nombre: hélice
(0, 0, 0), (1, 0, 0), (0, 1, 0) ; nombre: puntos
```

## Superficies paramétricas

```grafico
título: Esfera
(\cos u \sin v, \sin u \sin v, \cos v) ; v: 0..π ; color: celeste
```

```grafico
título: Toro
((2 + \cos v)\cos u, (2 + \cos v)\sin u, \sin v) ; color: violeta
```

```grafico
título: Cinta de Möbius
u: 0..2π
v: -0.4..0.4
((1 + v\cos(u/2))\cos u, (1 + v\cos(u/2))\sin u, v\sin(u/2)) ; color: naranja
```

## Implícita

```grafico
título: Superficie implícita
x: -1.6..1.6
y: -1.6..1.6
z: -1.6..1.6
x^4 + y^4 + z^4 - (x^2 + y^2 + z^2) = -0.3 ; color: rosa
```

## Nube de puntos

```grafico
título: Mediciones
datos:
| x | y | z |
|---|---|---|
| 0 | 0 | 1 |
| 1 | 2 | 3 |
| 2 | 1 | 2.5 |
| 3 | 3 | 4 |
| 1 | 3 | 2 |
| 2 | 0 | 1.5 |
| 3 | 1 | 3.5 |
```

## Con un error

```grafico
z = k(x^2 + y^2)
```
";

fn boveda(nombre: &str, texto: &str) -> (std::path::PathBuf, Arc<orbital_vault::Vault>, RelPath) {
    let root = std::env::temp_dir().join(format!("orbital-053-{nombre}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join("wiki")).unwrap();
    let rel = RelPath::new("wiki/graficos3d.md").unwrap();
    fs::write(rel.to_path(&root), texto).unwrap();
    (
        root.clone(),
        Arc::new(orbital_vault::open(&root).unwrap()),
        rel,
    )
}

async fn esperar(mut f: impl FnMut() -> bool) -> bool {
    for _ in 0..400 {
        if f() {
            return true;
        }
        glib::timeout_future(Duration::from_millis(25)).await;
    }
    false
}

#[test]
#[ignore = "requiere una sesión gráfica; tools/pruebas-gtk.sh"]
fn graficos_3d_en_lectura() {
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
        // Los gráficos se anclan cerca de lo visible: se baja por la nota para verlos todos.
        let mut vistos: Vec<Lienzo> = Vec::new();
        for (titulo, captura) in [
            ("Superficie", "053-lectura-superficie"),
            ("Curva en el espacio", "053-lectura-curva"),
            ("Superficies paramétricas", "053-lectura-parametricas"),
            ("Implícita", "053-lectura-implicita"),
            ("Nube de puntos", "053-lectura-nube"),
        ] {
            editor.0.reader.scroll_to_heading(titulo);
            assert!(
                esperar(|| {
                    let ls = lienzos(vista.upcast_ref());
                    !ls.is_empty() && ls.iter().all(|l| l.pintados() > 0 || !l.is_mapped())
                })
                .await,
                "pintados en {titulo}: {:?}",
                lienzos(vista.upcast_ref())
                    .iter()
                    .map(|l| (
                        l.muestreos(),
                        l.pintados(),
                        l.is_mapped(),
                        l.width(),
                        l.height(),
                        l.ocupado()
                    ))
                    .collect::<Vec<_>>()
            );
            glib::timeout_future(Duration::from_millis(250)).await;
            for l in lienzos(vista.upcast_ref()) {
                if !vistos.iter().any(|v| v == &l) {
                    vistos.push(l);
                }
            }
            capturar(&editor.0.reader.widget().clone(), captura);
        }
        assert_eq!(vistos.len(), 7, "siete gráficos 3D");
        let nombres = [
            "Gráfico 3D: Silla de montar. z = a(x^2 - y^2).",
            "Gráfico 3D: Hélice. (\\cos t, \\sin t, t/6); (0, 0, 0), (1, 0, 0), (0, 1, 0).",
            "Gráfico 3D: Esfera.",
            "Gráfico 3D: Toro.",
            "Gráfico 3D: Cinta de Möbius.",
            "Gráfico 3D: Superficie implícita.",
            "Gráfico 3D: Mediciones. datos: z (7 puntos).",
        ];
        for (l, n) in vistos.iter().zip(nombres) {
            assert!(l.es_3d());
            let d = l.grafico().unwrap().descripcion();
            assert!(d.starts_with(n), "{d}");
            assert!(l.is_focusable());
            assert_eq!(l.accessible_role(), gtk::AccessibleRole::Img);
        }
        // La implícita se refinó hasta la grilla fina.
        let implicita = &vistos[5];
        assert!(esperar(|| implicita.resolucion_implicitas() == 48).await);
        // El error: el fuente como código y el motivo como texto.
        let buf = editor.0.reader.buffer();
        let texto = buf.text(&buf.start_iter(), &buf.end_iter(), true);
        assert!(
            texto.contains("⚠ Error en el gráfico (renglón 1): «k» no está definido"),
            "{texto}"
        );
        assert_eq!(fs::read_to_string(rel.to_path(&root)).unwrap(), NOTA);
        window.close();
        let _ = fs::remove_dir_all(&root);
    });
}

#[test]
#[ignore = "requiere una sesión gráfica; tools/pruebas-gtk.sh"]
fn grafico_3d_interactivo() {
    adw::init().expect("Se necesita una sesión gráfica GTK");
    crate::style::apply();
    glib::MainContext::default().block_on(async {
        let fuente = "título: Ondas\na = 1 (0..3)\nz = a\\sin(x)\\cos(y)\n(\\cos t, \\sin t, 1.5) ; t: 0..2π ; nombre: anillo";
        let g = crate::graficos::leer(fuente).unwrap();
        let gv = GraficoVista::new(g, fuente, true);
        let window = gtk::Window::builder()
            .default_width(760)
            .default_height(560)
            .child(gv.widget())
            .build();
        window.present();
        let l = gv.lienzo().clone();
        assert!(esperar(|| l.pintados() > 0 && !l.ocupado()).await);
        let c0 = l.camara().unwrap();
        assert_eq!((c0.giro, c0.elevacion), (-60.0, 28.0));
        // El punto bajo el centro está sobre la superficie: z = sin(x) cos(y).
        let (cx, cy) = (f64::from(l.width()) / 2.0, f64::from(l.height()) / 2.0);
        let p = l.punto_bajo(cx, cy).expect("la superficie bajo el centro");
        assert!(
            (p[2] - p[0].sin() * p[1].cos()).abs() < 0.15,
            "{p:?} sobre z = sin x cos y"
        );
        // Teclado: flechas giran, + y − acercan, 0 vuelve.
        assert!(l.tecla(gtk::gdk::Key::Right));
        assert_eq!(l.camara().unwrap().giro, -70.0);
        assert!(l.tecla(gtk::gdk::Key::Down));
        assert_eq!(l.camara().unwrap().elevacion, 38.0);
        assert!(l.tecla(gtk::gdk::Key::plus));
        assert!(l.camara().unwrap().zoom > 1.2);
        assert!(l.tecla(gtk::gdk::Key::_0));
        assert_eq!(l.camara().unwrap(), c0, "vuelve a la vista original");
        // Girar como un arrastre, 60 cuadros: cada cambio deja el hilo de GTK libre enseguida
        // (el pintado va al pool de gio) y se pinta lo último.
        let n = l.pintados();
        let mut peor = Duration::ZERO;
        for k in 0..60 {
            let t = Instant::now();
            l.fijar_camara(c0.girar(f64::from(k) * 3.0, f64::from(k % 10)));
            peor = peor.max(t.elapsed());
            glib::timeout_future(Duration::from_millis(16)).await;
        }
        assert!(esperar(|| !l.ocupado()).await);
        let pintados = l.pintados() - n;
        println!("Girar: {pintados} cuadros pintados de 60 pedidos; peor bloqueo {peor:?}");
        assert!(pintados >= 10, "{pintados}");
        assert!(peor < Duration::from_millis(8), "{peor:?}");
        capturar(gv.widget(), "053-rotado");
        // El deslizador: vuelve a muestrear fuera del hilo de GTK.
        let s = gv.deslizadores()[0].clone();
        let m = l.muestreos();
        let mut peor = Duration::ZERO;
        for k in 1..=20 {
            let t = Instant::now();
            s.set_value(f64::from(k) * 0.15);
            peor = peor.max(t.elapsed());
            glib::timeout_future(Duration::from_millis(16)).await;
        }
        assert!(esperar(|| !l.ocupado() && l.muestreos() > m).await);
        println!("Deslizador 3D: peor bloqueo del hilo de GTK por cambio {peor:?}");
        assert!(peor < Duration::from_millis(16), "{peor:?}");
        // Con a = 3, la superficie sube hasta 3: el punto bajo el centro sigue siendo z = 3 sin x cos y.
        let p = l.punto_bajo(cx, cy).expect("superficie");
        assert!((p[2] - 3.0 * p[0].sin() * p[1].cos()).abs() < 0.3, "{p:?}");
        window.close();
    });
}

#[test]
#[ignore = "requiere una sesión gráfica; tools/pruebas-gtk.sh"]
fn implicita_3d_progresiva() {
    adw::init().expect("Se necesita una sesión gráfica GTK");
    crate::style::apply();
    glib::MainContext::default().block_on(async {
        let fuente = "x: -4..4\ny: -4..4\nz: -4..4\nresolución: 64\n\\sin x \\cos y + \\sin y \\cos z + \\sin z \\cos x = 0";
        let g = crate::graficos::leer(fuente).unwrap();
        let gv = GraficoVista::new(g, fuente, true);
        let window = gtk::Window::builder()
            .default_width(760)
            .default_height(520)
            .child(gv.widget())
            .build();
        let max = std::rc::Rc::new(std::cell::Cell::new(0.0f64));
        let (m, ultimo) = (max.clone(), std::rc::Rc::new(std::cell::Cell::new(Instant::now())));
        let tic = glib::timeout_add_local(Duration::from_millis(1), move || {
            let ahora = Instant::now();
            m.set(m.get().max((ahora - ultimo.get()).as_secs_f64() * 1000.0));
            ultimo.set(ahora);
            glib::ControlFlow::Continue
        });
        let t = Instant::now();
        window.present();
        let l = gv.lienzo().clone();
        assert!(esperar(|| l.pintados() > 0).await);
        let primero = t.elapsed();
        let nivel = l.resolucion_implicitas();
        assert!(esperar(|| l.resolucion_implicitas() == 64 && !l.ocupado()).await);
        let fino = t.elapsed();
        tic.remove();
        println!(
            "Implícita: primer cuadro {primero:?} (grilla de {nivel}); refinada (64) {fino:?}; tramo más largo del hilo de GTK {:.1} ms",
            max.get()
        );
        assert!(nivel < 64, "primero la gruesa");
        window.close();
    });
}

#[test]
#[ignore = "requiere una sesión gráfica; tools/pruebas-gtk.sh"]
fn editor_de_graficos_3d() {
    adw::init().expect("Se necesita una sesión gráfica GTK");
    crate::style::apply();
    glib::MainContext::default().block_on(async {
        let nota = "Antes.\n\n```grafico\nz = x^2 + y^2\n```\n\nDespués.\n";
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
        let t = editor.text();
        let off = t.find("x^2").unwrap();
        b.place_cursor(&b.iter_at_offset(t[..off].chars().count() as i32 + 1));
        // El globito, estático.
        editor.actualizar_vista_previa();
        let globito = |e: &Editor| {
            e.0.vista_previa
                .borrow()
                .as_ref()
                .is_some_and(|v| v.popover.is_visible() && v.grafico.is_visible())
        };
        assert!(esperar(|| globito(&editor)).await, "globito con el gráfico");
        let ls = {
            let v = editor.0.vista_previa.borrow();
            lienzos(v.as_ref().unwrap().grafico.upcast_ref())
        };
        assert!(ls[0].es_3d() && !ls[0].is_focusable());
        assert!(esperar(|| ls[0].pintados() > 0).await);
        // Ctrl+M: la página «Gráfico» con un gráfico 3D interactivo; las plantillas 3D.
        let ed = editor.abrir_ecuacion(false).expect("editor abierto");
        assert_eq!(ed.fuente_grafico(), "z = x^2 + y^2");
        let campo = ed.texto_grafico().unwrap();
        campo
            .buffer()
            .set_text("título: Editado\na = 1 (0..2)\nz = a(x^2 - y^2)");
        campo.buffer().place_cursor(&campo.buffer().end_iter());
        ed.insertar_plantilla_grafico("(\\cos t, \\sin t, t/6) ; t: 0..6π");
        ed.insertar_plantilla_grafico("cámara: -60, 28");
        ed.dibujar_grafico();
        let (g, error) = ed.estado_grafico();
        assert!(error.is_none(), "{error:?}");
        let g = g.unwrap();
        assert!(g.lienzo().es_3d() && g.lienzo().is_focusable());
        assert_eq!(g.deslizadores().len(), 1);
        assert!(esperar(|| g.lienzo().pintados() > 0).await);
        glib::timeout_future(Duration::from_millis(300)).await;
        capturar(ed.dialogo(), "053-editor");
        ed.boton_aceptar().emit_clicked();
        assert_eq!(
            editor.text(),
            "Antes.\n\n```grafico\ntítulo: Editado\na = 1 (0..2)\nz = a(x^2 - y^2)\n(\\cos t, \\sin t, t/6) ; t: 0..6π\ncámara: -60, 28\n```\n\nDespués.\n"
        );
        b.undo();
        assert_eq!(editor.text(), nota);
        window.close();
        let _ = fs::remove_dir_all(&root);
    });
}

#[test]
#[ignore = "requiere una sesión gráfica"]
fn exportar_graficos_3d_en_webkit() {
    glib::set_prgname(Some("orbital"));
    adw::init().expect("Se necesita acceso a una sesión gráfica GTK");
    glib::MainContext::default().block_on(async {
        let html = crate::export::html_md(NOTA, "graficos3d.md");
        assert_eq!(
            html.matches("<figure class=\"grafico\"><svg").count(),
            7,
            "siete gráficos"
        );
        assert!(html.contains("aria-label=\"Gráfico 3D: Toro."));
        assert!(html.contains("class=\"grafico-error\""));
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
        glib::timeout_future(Duration::from_millis(500)).await;
        if let Some(dir) = std::env::var_os("ORBITAL_CAPTURA_DIR") {
            let t = web
                .snapshot_future(
                    webkit6::SnapshotRegion::FullDocument,
                    webkit6::SnapshotOptions::NONE,
                )
                .await
                .unwrap();
            let png = std::path::Path::new(&dir).join("053-exportado.png");
            t.save_to_png(&png).unwrap();
            println!("Captura: {}", png.display());
        }
        window.close();
    });
}

/// Medición (053): abrir en Lectura una nota con muchos gráficos 3D (`ORBITAL_MEDIR_GRAFICOS=N`,
/// 12 si no), seis veces en el mismo proceso; después, el cuadro al girar, acercar y mover un
/// deslizador, y una implícita del primer cuadro a la refinada.
/// `cargo test --release --bin orbital medir_graficos_3d -- --ignored --nocapture`
#[test]
#[ignore = "medición; requiere una sesión gráfica"]
fn medir_graficos_3d() {
    use crate::editor::reader::{Prepared, Reader};
    use std::cell::Cell;
    use std::rc::Rc;
    adw::init().expect("Se necesita una sesión gráfica GTK");
    crate::style::apply();
    let n: usize = std::env::var("ORBITAL_MEDIR_GRAFICOS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(12);
    let corpus = [
        "z = \\sin(Kx/3) \\cos(y)",
        "a = 1 (0..3)\nz = a(x^2 - y^2)/K",
        "(\\cos t, \\sin t, t/K) ; t: 0..6π",
        "((2 + \\cos v)\\cos u, (2 + \\cos v)\\sin u, \\sin(K v)/2)",
        "x: -2..2\ny: -2..2\nz: -2..2\nx^2 + y^2 + z^2 = K/4",
        "datos: nube\n0 0 0\n1 K 2\n2 1 K\n3 2 1",
    ];
    let mut text = String::from("# Muchos gráficos 3D\n\n");
    for k in 0..n {
        let g = corpus[k % corpus.len()].replace('K', &format!("{}", k % 5 + 2));
        text.push_str(&format!("Gráfico {k}:\n\n```grafico\n{g}\n```\n\n"));
    }
    let window = gtk::Window::builder()
        .default_width(900)
        .default_height(800)
        .build();
    let mediana = |mut v: Vec<f64>| {
        v.sort_by(f64::total_cmp);
        (v[v.len() / 2], v[v.len() - 1])
    };
    let reloj = || {
        let max = Rc::new(Cell::new(0.0f64));
        let (m, ultimo) = (max.clone(), Rc::new(Cell::new(Instant::now())));
        let tic = glib::timeout_add_local(Duration::from_millis(1), move || {
            let ahora = Instant::now();
            m.set(m.get().max((ahora - ultimo.get()).as_secs_f64() * 1000.0));
            ultimo.set(ahora);
            glib::ControlFlow::Continue
        });
        (max, tic)
    };
    glib::MainContext::default().block_on(async {
        for vuelta in 0..6 {
            let reader = Reader::new();
            window.set_child(Some(reader.widget()));
            window.present();
            let texto = text.clone();
            let prepared = gtk::gio::spawn_blocking(move || Prepared::new(&texto, &[]))
                .await
                .unwrap();
            let (max, tic) = reloj();
            let t1 = Instant::now();
            reader.render(prepared, || None, None);
            let vista = reader.text_view().clone();
            let mut visibles = Vec::new();
            for _ in 0..2000 {
                visibles = lienzos(vista.upcast_ref());
                if !visibles.is_empty()
                    && visibles
                        .iter()
                        .all(|l| (l.pintados() > 0 && !l.ocupado()) || !l.is_mapped())
                {
                    break;
                }
                glib::timeout_future(Duration::from_millis(2)).await;
            }
            let listo = t1.elapsed().as_secs_f64() * 1000.0;
            glib::timeout_future(Duration::from_millis(100)).await;
            tic.remove();
            println!(
                "Apertura {}: {} gráficos anclados, todo pintado (suavizado) en {listo:.1} ms, tramo más largo del hilo de GTK {:.1} ms",
                vuelta + 1,
                visibles.len(),
                max.get()
            );
        }
        // El cuadro de una superficie de 60 × 60 con una curva al girar, acercar y mover el
        // deslizador.
        let fuente = "a = 1 (0..3)\nz = a\\sin(x)\\cos(y)\n(\\cos t, \\sin t, 1.5) ; t: 0..2π";
        let gv = GraficoVista::new(crate::graficos::leer(fuente).unwrap(), fuente, true);
        window.set_child(Some(gv.widget()));
        window.set_default_size(900, 600);
        let l = gv.lienzo().clone();
        assert!(esperar(|| l.pintados() > 0 && !l.ocupado()).await);
        let c0 = l.camara().unwrap();
        for (nombre, accion) in [("girar", 0), ("acercar", 1), ("deslizador", 2)] {
            let mut cuadro = Vec::new();
            let mut hilo = Vec::new();
            let mut pool = Vec::new();
            let (p0, m0) = (l.pintados(), l.muestreos());
            let (max, tic) = reloj();
            for k in 0..60 {
                let t = Instant::now();
                match accion {
                    0 => l.fijar_camara(c0.girar(f64::from(k) * 3.0, f64::from(k % 10))),
                    1 => l.fijar_camara(c0.acercar(1.0 + f64::from(k % 20) * 0.05)),
                    _ => gv.deslizadores()[0].set_value(f64::from(k % 20) * 0.15),
                }
                hilo.push(t.elapsed().as_secs_f64() * 1000.0);
                glib::timeout_future(Duration::from_millis(16)).await;
                cuadro.push(l.medir_cuadro());
                pool.push(l.ms_pintado());
            }
            assert!(esperar(|| !l.ocupado()).await);
            tic.remove();
            let (mc, xc) = mediana(cuadro);
            let (mh, xh) = mediana(hilo);
            let (mp, xp) = mediana(pool);
            println!(
                "{nombre} ({}×{} px): el cambio en el hilo de GTK mediana {mh:.3} ms (máx {xh:.3}); armar el cuadro mediana {mc:.2} ms (máx {xc:.2}); pintar en el pool mediana {mp:.2} ms (máx {xp:.2}); {} cuadros pintados y {} muestreos para 60 cambios; tramo más largo del hilo de GTK {:.1} ms",
                l.width(),
                l.height(),
                l.pintados() - p0,
                l.muestreos() - m0,
                max.get()
            );
        }
        // Una implícita difícil: el primer cuadro (grilla gruesa) y la refinada.
        let fuente = "x: -4..4\ny: -4..4\nz: -4..4\n\\sin x \\cos y + \\sin y \\cos z + \\sin z \\cos x = 0";
        let (max, tic) = reloj();
        let t = Instant::now();
        let gv = GraficoVista::new(crate::graficos::leer(fuente).unwrap(), fuente, true);
        window.set_child(Some(gv.widget()));
        let l = gv.lienzo().clone();
        assert!(esperar(|| l.pintados() > 0).await);
        let primero = t.elapsed().as_secs_f64() * 1000.0;
        let nivel = l.resolucion_implicitas();
        assert!(esperar(|| l.resolucion_implicitas() == 48 && !l.ocupado()).await);
        tic.remove();
        println!(
            "Implícita (giroide): primer cuadro en {primero:.0} ms (grilla de {nivel}); refinada (48) y suavizada en {:.0} ms; tramo más largo del hilo de GTK {:.1} ms",
            t.elapsed().as_secs_f64() * 1000.0,
            max.get()
        );
    });
}
