//! Pruebas GTK del mouse en la vista formateada (045): sin abortos, plegado fácil y puntero.

use super::*;

/// Una nota como las de la usuaria (callouts plegables, tarjetas de estudio, secciones y listas
/// con subítems), en una ventana de verdad.
async fn lector_en_ventana(text: &str) -> (gtk::Window, Reader) {
    let reader = Reader::new();
    let mut prepared = Prepared::new(text, &[]);
    prepared.con_plegables(&orbital_vault::plegado::plegables(text));
    let window = gtk::Window::builder()
        .default_width(1100)
        .default_height(1600)
        .child(reader.widget())
        .build();
    window.present();
    reader.render(prepared, || None, None);
    for _ in 0..200 {
        if !reader.0.blocks.borrow().is_empty() {
            break;
        }
        glib::timeout_future(Duration::from_millis(10)).await;
    }
    for _ in 0..30 {
        glib::timeout_future(Duration::from_millis(10)).await;
    }
    (window, reader)
}

const NOTA: &str = "## Primera sección\n\nUn párrafo con un [[enlace]] y una #etiqueta.\n\n> [!note]- Callout plegado\n> Cuerpo oculto del callout.\n\n> [!tip]+ Callout abierto\n> Cuerpo visible.\n\n- uno\n  - dos\n- tres\n\n## Flashcards\n\n:::flashcard id=fc-001\n¿Qué dice el PBO?\n---\nTodo subconjunto no vacío de ℕ tiene mínimo.\n:::\n\n## Fin\n\nx\n\n:::flashcard id=fc-002\n¿Y en Z?\n---\nNo vale.\n:::\n\nUn renglón final bastante más largo que el anterior para que no quede corto.\n";

/// Pasar el mouse por toda la vista no aborta. GTK 4.24 abortaba (`Byte index … is off the end
/// of the line`) con el mouse debajo de un renglón con algo invisible, como el botón de una
/// tarjeta de estudio. Con `ORBITAL_REPRO_ABORTO=1` se barre con la llamada cruda que hacía el
/// manejador de antes, y el proceso aborta.
#[test]
#[ignore = "requiere acceso GTK; tools/pruebas-gtk.sh"]
fn mover_el_mouse_por_toda_la_lectura_no_aborta() {
    gtk::init().expect("el proceso necesita conectarse a una sesión GTK");
    let crudo = std::env::var_os("ORBITAL_REPRO_ABORTO").is_some();
    glib::MainContext::default().block_on(async {
        let (window, reader) = lector_en_ventana(NOTA).await;
        let view = reader.text_view().clone();
        let alto = view.height();
        assert!(alto > 0);
        let mut zonas = Vec::new();
        // Con las respuestas ocultas y después a la vista.
        for _ in 0..2 {
            for y in 0..alto {
                for x in [5, 30, 60, 200, 450] {
                    if crudo {
                        let (bx, by) =
                            view.window_to_buffer_coords(gtk::TextWindowType::Widget, x, y);
                        let _ = view.iter_at_location(bx, by);
                    } else {
                        reader.mover_mouse(f64::from(x), f64::from(y));
                        zonas.extend(reader.zona().filter(|z| !zonas.contains(z)));
                    }
                }
            }
            let urls = reader.urls_tab();
            for (n, u) in urls.iter().enumerate() {
                if u.starts_with(bloques::URL_CALLOUT) {
                    reader.activar_tab(n);
                }
            }
        }
        if !crudo {
            assert_eq!(zonas.len(), 3, "texto, mano y flecha: {zonas:?}");
        }
        window.destroy();
    });
}

/// Plegar no pide puntería: la franja de la flechita vale a la altura de todo el renglón, el
/// renglón entero del título de un callout plegable lo alterna, y el puntero es una mano sobre
/// lo que se acciona.
#[test]
#[ignore = "requiere acceso GTK; tools/pruebas-gtk.sh"]
fn plegar_con_el_mouse_es_facil() {
    gtk::init().expect("el proceso necesita conectarse a una sesión GTK");
    glib::MainContext::default().block_on(async {
        let (window, reader) = lector_en_ventana(NOTA).await;
        let view = reader.text_view().clone();
        let plegadas: Rc<RefCell<Vec<u32>>> = Rc::default();
        let p = plegadas.clone();
        reader.set_on_plegar(move |l| p.borrow_mut().push(l));
        let buf = reader.buffer();
        let a_vista = |it: &gtk::TextIter, dx: i32, dy: i32| {
            let r = view.iter_location(it);
            let (x, y) =
                view.buffer_to_window_coords(gtk::TextWindowType::Widget, r.x() + dx, r.y() + dy);
            (f64::from(x), f64::from(y))
        };
        let buscar = |t: &str| {
            buf.start_iter()
                .forward_search(t, gtk::TextSearchFlags::empty(), None)
                .unwrap_or_else(|| panic!("falta «{t}»"))
                .0
        };
        // La flechita de «Primera sección» (línea fuente 0): a su izquierda y más abajo que el
        // glifo (en el espacio de abajo del renglón), no sobre el texto del título.
        let titulo = buscar("Primera sección");
        let (ly, alto) = view.line_yrange(&titulo);
        let mut glifo = titulo;
        while glifo.char() != '▾' {
            assert!(glifo.backward_char(), "sin flechita antes del título");
        }
        let r = view.iter_location(&glifo);
        let (gx, _) = a_vista(&glifo, 0, 0);
        let (_, y_abajo) =
            view.buffer_to_window_coords(gtk::TextWindowType::Widget, 0, ly + alto - 2);
        let y_abajo = f64::from(y_abajo);
        assert!(gx > 20.0, "hay margen a la izquierda de la flechita");
        let x_izq = gx - 15.0;
        assert!(
            reader.clic_de_plegado(x_izq, y_abajo),
            "franja izquierda, abajo"
        );
        assert!(reader.clic_de_plegado(gx + f64::from(r.width()) / 2.0, y_abajo));
        assert_eq!(*plegadas.borrow(), vec![0, 0]);
        let (tx, ty) = a_vista(&titulo, 40, 4);
        assert!(
            !reader.clic_de_plegado(tx, ty),
            "el texto del título no pliega"
        );
        // El puntero: mano en la franja, barra sobre el texto, flecha a la derecha del texto.
        reader.mover_mouse(x_izq, y_abajo);
        assert_eq!(reader.zona(), Some(Zona::Mano));
        assert_eq!(reader.0.hover_plegable.get(), Some(0), "la flechita se ve");
        reader.mover_mouse(tx, ty);
        assert_eq!(reader.zona(), Some(Zona::Texto));
        reader.mover_mouse(f64::from(view.width() - 3), ty);
        assert_eq!(reader.zona(), Some(Zona::Nada));
        // Un enlace y una etiqueta: mano.
        for t in ["enlace", "etiqueta"] {
            let (x, y) = a_vista(&buscar(t), 3, 4);
            reader.mover_mouse(x, y);
            assert_eq!(reader.zona(), Some(Zona::Mano), "{t}");
        }
        // El título de un callout plegable: un clic en cualquier punto del renglón (incluso a la
        // derecha del texto) lo abre y otro lo cierra.
        let visible = || {
            buf.text(&buf.start_iter(), &buf.end_iter(), false)
                .to_string()
        };
        assert!(!visible().contains("Cuerpo oculto"));
        let (_, cy) = a_vista(&buscar("Callout plegado"), 0, 4);
        let derecha = f64::from(view.width() - 60);
        reader.mover_mouse(derecha, cy);
        assert_eq!(reader.zona(), Some(Zona::Mano));
        assert!(reader.clic_de_plegado(derecha, cy));
        assert!(visible().contains("Cuerpo oculto"));
        assert!(reader.clic_de_plegado(derecha, cy));
        assert!(!visible().contains("Cuerpo oculto"));
        // «Mostrar respuesta»: mano.
        let (x, y) = a_vista(&buscar("Mostrar respuesta"), 4, 4);
        reader.mover_mouse(x, y);
        assert_eq!(reader.zona(), Some(Zona::Mano));
        window.destroy();
    });
}
