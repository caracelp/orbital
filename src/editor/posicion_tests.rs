//! Pruebas GTK del cálculo de posición bajo el mouse (aborto de GTK 4.24 con texto invisible).

use std::time::Duration;

use gtk::glib;

use super::*;

/// Una vista con espacio debajo de cada renglón y un tramo invisible en el primero.
fn vista_con_invisible() -> (gtk::Window, gtk::TextView) {
    let view = gtk::TextView::builder()
        .pixels_below_lines(12)
        .wrap_mode(gtk::WrapMode::WordChar)
        .build();
    let b = view.buffer();
    // El aborto depende del renglón siguiente: GTK sigue contando en él y falla si es más corto.
    b.set_text("uno dos tres cuatro\ncorto\nsegundo renglón visible\nx\n");
    let oculto = b
        .create_tag(Some("oculto"), &[("invisible", &true)])
        .unwrap();
    let mut a = b.iter_at_offset(4);
    let mut z = b.iter_at_offset(8);
    b.apply_tag(&oculto, &a, &z);
    // También un renglón que termina en algo oculto (como el título de un callout plegado).
    a = b.iter_at_line_offset(2, 8).unwrap();
    z = b.iter_at_line(3).unwrap();
    b.apply_tag(&oculto, &a, &z);
    let win = gtk::Window::builder()
        .default_width(400)
        .default_height(300)
        .child(&view)
        .build();
    win.present();
    (win, view)
}

async fn esperar_disposicion(view: &gtk::TextView) {
    for _ in 0..100 {
        glib::timeout_future(Duration::from_millis(10)).await;
        if view.width() > 0 && view.line_yrange(&view.buffer().start_iter()).1 > 0 {
            break;
        }
    }
    glib::timeout_future(Duration::from_millis(50)).await;
}

/// Con `ORBITAL_REPRO_ABORTO=1` hace la llamada cruda de GTK en la franja peligrosa: el proceso
/// aborta (`Byte index 24 is off the end of the line`, como el cierre de la 045). Con
/// `ORBITAL_VER_ABORTO=1`, la prueba lo corre además en un proceso aparte y muestra cómo terminó
/// (no siempre: cada aborto deja un volcado de memoria).
#[test]
#[ignore = "requiere una sesión gráfica; tools/pruebas-gtk.sh"]
fn iter_seguro_debajo_de_renglon_con_invisible() {
    gtk::init().expect("el proceso necesita conectarse a una sesión GTK");
    let crudo = std::env::var_os("ORBITAL_REPRO_ABORTO").is_some();
    glib::MainContext::default().block_on(async {
        let (win, view) = vista_con_invisible();
        esperar_disposicion(&view).await;
        let b = view.buffer();
        for linea in [0, 2] {
            let it = b.iter_at_line(linea).unwrap();
            let (y, h) = view.line_yrange(&it);
            // Dentro de los 12 px de abajo del renglón.
            let by = y + h - 3;
            if crudo {
                let _ = view.iter_at_location(10, by);
                continue;
            }
            match punto(&view, 10, by) {
                Punto::Debajo { peligroso } => {
                    assert!(peligroso, "renglón {linea}: tiene algo invisible");
                }
                _ => panic!("renglón {linea}: el punto está debajo del texto"),
            }
            assert!(iter_en(&view, 10, by).is_none());
            // Sobre el texto, la posición es la de GTK.
            let arriba = iter_en(&view, 10, y + 2).expect("sobre el texto");
            assert_eq!(arriba.line(), linea);
        }
        // Un renglón sin nada oculto: el margen no es peligroso y GTK no aborta.
        let it = b.iter_at_line(1).unwrap();
        let (y, h) = view.line_yrange(&it);
        assert!(matches!(
            punto(&view, 10, y + h - 3),
            Punto::Debajo { peligroso: false }
        ));
        let _ = view.iter_at_location(10, y + h - 3);
        win.destroy();
    });
    if crudo || std::env::var_os("ORBITAL_VER_ABORTO").is_none() {
        return;
    }
    // La misma llamada sin protección, en otro proceso: GTK 4.24 aborta.
    let salida = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--ignored",
            "--exact",
            "editor::posicion::posicion_tests::iter_seguro_debajo_de_renglon_con_invisible",
        ])
        .env("ORBITAL_REPRO_ABORTO", "1")
        .output()
        .unwrap();
    let err = String::from_utf8_lossy(&salida.stderr);
    eprintln!(
        "llamada cruda de GTK: {} {}",
        salida.status,
        err.lines()
            .find(|l| l.contains("off the end"))
            .unwrap_or("(sin aborto: ¿GTK lo arregló?)")
    );
}
