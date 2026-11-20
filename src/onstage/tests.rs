use super::*;

/// T002: corte por nivel 1 y 2; niveles 3+ quedan dentro.
#[test]
fn t002_corte_por_nivel_1_y_2() {
    let texto = "# A\n\nuno\n\n## B\n\ndos\n\n### C\n\nsigue en B\n\n## D\n\nfin\n";
    let d = dividir_diapositivas(texto);
    assert_eq!(d.len(), 3, "{d:?}");
    assert_eq!(d[0].titulo, "A");
    assert_eq!(d[0].nivel, 1);
    assert!(d[0].texto.starts_with("# A"));
    assert_eq!(d[1].titulo, "B");
    assert!(d[1].texto.contains("### C"), "{:?}", d[1].texto);
    assert!(d[1].texto.contains("sigue en B"));
    assert_eq!(d[2].titulo, "D");
}

/// T002: el frontmatter no genera diapositiva ni corta.
#[test]
fn t002_frontmatter_se_ignora() {
    let texto = "---\ntitle: X\n---\n\n# T\n\ncuerpo\n";
    let d = dividir_diapositivas(texto);
    assert_eq!(d.len(), 1, "{d:?}");
    assert_eq!(d[0].titulo, "T");
}

/// T002: texto previo al primer encabezado va como diapositiva sin título.
#[test]
fn t002_previo_al_primer_encabezado() {
    let texto = "intro\n\n# T\n\ncuerpo\n";
    let d = dividir_diapositivas(texto);
    assert_eq!(d.len(), 2, "{d:?}");
    assert_eq!(d[0].titulo, "");
    assert_eq!(d[0].nivel, 0);
    assert_eq!(d[1].titulo, "T");
}

/// T002: nota sin encabezados es una sola diapositiva; nota vacía no tiene ninguna.
#[test]
fn t002_sin_encabezados_y_vacia() {
    let d = dividir_diapositivas("solo texto\n");
    assert_eq!(d.len(), 1);
    assert_eq!(d[0].titulo, "");
    assert!(dividir_diapositivas("").is_empty());
    assert!(dividir_diapositivas("\n\n").is_empty());
}

/// T002: un `#` dentro de un codeblock cerrado no corta; el que abre el codeblock sí.
#[test]
fn t002_codeblock_no_corta() {
    let texto = "# A\n\n```rust\n# no es encabezado\n```\n\n# B\n\n~~~\n# tampoco\n~~~\n\nfin\n";
    let d = dividir_diapositivas(texto);
    assert_eq!(d.len(), 2, "{d:?}");
    assert!(d[0].texto.contains("# no es encabezado"));
    assert!(d[1].texto.contains("# tampoco"), "{:?}", d[1].texto);
    assert!(d[1].texto.contains("~~~"));
    assert_eq!(d[1].titulo, "B");
}

/// T002: `#etiqueta` y encabezados con sangría de 4+ no cortan; `##` con cierre `##` sí.
#[test]
fn t002_atx_estricto() {
    let texto = "# A\n\n#etiqueta no corta\n\n    ## con sangría de 4 no corta\n\n## B ##\n\nfin\n";
    let d = dividir_diapositivas(texto);
    assert_eq!(d.len(), 2, "{d:?}");
    assert!(d[0].texto.contains("#etiqueta"));
    assert!(d[0].texto.contains("    ## con sangría"));
    assert_eq!(d[1].titulo, "B");
}

/// T002: el título se recorta sin `#` inicial ni cierre final.
#[test]
fn t002_titulo_limpio() {
    let texto = "# El título ##\n\nx\n";
    let d = dividir_diapositivas(texto);
    assert_eq!(d[0].titulo, "El título");
}

/// T002: frontmatter con BOM se ignora igual.
#[test]
fn t002_frontmatter_con_bom() {
    let texto = "\u{feff}---\ntitle: X\n---\n\n# T\n\ncuerpo\n";
    let d = dividir_diapositivas(texto);
    assert_eq!(d.len(), 1, "{d:?}");
    assert_eq!(d[0].titulo, "T");
}

/// FR-3 de la portada: solo `title` y `description` escalares del frontmatter.
#[test]
fn t003_portada_de_yaml() {
    let texto = "---\ntitle: Entropía\ndescription: Medida del desorden.\ntags: [fisica]\n---\n";
    let parsed = orbital_vault::props::markdown::locate(texto)
        .map(|r| orbital_vault::props::yaml::parse_block(&texto[r]))
        .unwrap();
    let yaml = parsed.unwrap();
    let portada = portada_de(&yaml).unwrap();
    assert_eq!(portada.0, "Entropía");
    assert_eq!(portada.1.as_deref(), Some("Medida del desorden."));
    // Sin título: sin portada.
    assert!(portada_de(&[]).is_none());
}

/// US1/US3 en proceso: ventana, portada, navegación por botones, índice y recorte al
/// alternar los toggles. Requiere sesión gráfica; lo corre el verificador con
/// `--include-ignored`. Las flechas y Esc van por revisión manual (sin teclas sintéticas).
#[test]
#[ignore = "requiere una sesión gráfica"]
fn us1_ventana_portada_navegacion_e_indice() {
    gtk::init().expect("se necesita acceso GTK");
    let raiz = std::env::temp_dir().join(format!("orbital-onstage-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&raiz);
    std::fs::create_dir_all(raiz.join("wiki")).unwrap();
    let nota = "---\ntype: atomo\ntitle: Órbitas\ndescription: Demo.\n---\n\n# Uno\n\nTexto uno.\n\n## Dos\n\nTexto dos.\n";
    std::fs::write(raiz.join("wiki/a.md"), nota).unwrap();
    let vault = Arc::new(orbital_vault::open(&raiz).unwrap());
    let ruta = orbital_vault::RelPath::new("wiki/a.md").unwrap();

    let principal = gtk::Window::new();
    principal.present();
    let onstage = OnStage::new(&principal);
    onstage.set_proveedor(move || {
        Some(Fuente {
            texto: nota.to_owned(),
            ruta: ruta.clone(),
            vault: Some(vault.clone()),
        })
    });
    onstage.presentar();
    let context = glib::MainContext::default();
    context.block_on(async {
        // El refresco corre en dos saltos async (trabajo de fondo + volcado).
        glib::timeout_future(Duration::from_millis(400)).await;
        glib::timeout_future(Duration::from_millis(400)).await;
        // Portada activada por defecto y con título: 1 de 3 (portada + dos diapositivas).
        assert_eq!(onstage.0.contador_barra.label().as_str(), "1 de 3");
        assert_eq!(
            onstage.0.stack.visible_child_name().as_deref(),
            Some("portada")
        );
        // Navegación por botones: el contenido de cada diapositiva queda volcado.
        onstage.0.adelante.emit_clicked();
        assert_eq!(onstage.0.contador_barra.label().as_str(), "2 de 3");
        // El volcado es asíncrono: se espera a que llegue en vez de un plazo fijo (en el
        // CI, 300 ms no siempre alcanzaban).
        // (El Reader estrena buffer en cada volcado: se lo pide de nuevo en cada lectura.)
        let leer = || {
            let buffer = onstage.0.reader.buffer();
            buffer
                .text(&buffer.start_iter(), &buffer.end_iter(), false)
                .to_string()
        };
        for _ in 0..250 {
            if leer().contains("Texto uno.") {
                break;
            }
            glib::timeout_future(Duration::from_millis(20)).await;
        }
        let texto = leer();
        assert!(texto.contains("Texto uno."), "{texto:?}");
        onstage.0.adelante.emit_clicked();
        assert_eq!(onstage.0.contador_barra.label().as_str(), "3 de 3");
        assert!(!onstage.0.adelante.is_sensitive());
        // El índice se inserta en la lista sin perder la posición (3 de 4): llega
        // navegando hacia atrás.
        onstage.0.toggle_toc.set_active(true);
        assert_eq!(onstage.0.contador_barra.label().as_str(), "3 de 4");
        assert_eq!(
            onstage.0.stack.visible_child_name().as_deref(),
            Some("diapositiva")
        );
        onstage.0.atras.emit_clicked();
        assert_eq!(
            onstage.0.stack.visible_child_name().as_deref(),
            Some("indice")
        );
        assert_eq!(onstage.0.contador_barra.label().as_str(), "2 de 4");
        let botones = {
            let mut out = Vec::new();
            let mut hijo = onstage.0.toc_lista.first_child();
            while let Some(w) = hijo {
                if let Ok(boton) = w.clone().downcast::<gtk::Button>() {
                    out.push(boton);
                }
                hijo = w.next_sibling();
            }
            out
        };
        assert_eq!(botones.len(), 2, "el índice lista las dos diapositivas");
        botones[1].emit_clicked();
        glib::timeout_future(Duration::from_millis(300)).await;
        assert_eq!(onstage.0.contador_barra.label().as_str(), "4 de 4");
        assert_eq!(
            onstage.0.stack.visible_child_name().as_deref(),
            Some("diapositiva")
        );
        // Sin índice, la posición se recorta a «Dos» (3 de 3).
        onstage.0.toggle_toc.set_active(false);
        assert_eq!(onstage.0.contador_barra.label().as_str(), "3 de 3");
        // Sin portada quedan las diapositivas; la posición sigue en «Dos» (2 de 2).
        onstage.0.toggle_portada.set_active(false);
        assert_eq!(onstage.0.contador_barra.label().as_str(), "2 de 2");
        // Reabrir vuelve a la primera diapositiva con los valores por defecto (FR-6).
        onstage.presentar();
        glib::timeout_future(Duration::from_millis(400)).await;
        glib::timeout_future(Duration::from_millis(400)).await;
        assert_eq!(onstage.0.contador_barra.label().as_str(), "1 de 3");
        assert_eq!(
            onstage.0.stack.visible_child_name().as_deref(),
            Some("portada")
        );
        assert!(!onstage.0.toggle_toc.is_active());
    });
    let _ = std::fs::remove_dir_all(&raiz);
}

/// 057: con el foco en un reproductor, Espacio, Enter y las flechas son del reproductor (no pasan
/// de diapositiva); con el foco en otro lado, Espacio pasa de diapositiva como siempre.
#[test]
#[ignore = "requiere una sesión gráfica"]
fn espacio_en_el_reproductor_no_pasa_de_diapositiva() {
    use gtk::gdk::Key;
    gtk::init().expect("se necesita acceso GTK");
    let raiz = std::env::temp_dir().join(format!("orbital-onstage-057-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&raiz);
    std::fs::create_dir_all(raiz.join("wiki")).unwrap();
    std::fs::write(
        raiz.join("wiki/tono.wav"),
        crate::medios::tests::wav_de_prueba(2.0),
    )
    .unwrap();
    let nota = "# Uno\n\n![[tono.wav]]\n\n# Dos\n\nTexto dos.\n";
    std::fs::write(raiz.join("wiki/a.md"), nota).unwrap();
    let vault = Arc::new(orbital_vault::open(&raiz).unwrap());
    let ruta = orbital_vault::RelPath::new("wiki/a.md").unwrap();
    let principal = gtk::Window::new();
    principal.present();
    let onstage = OnStage::new(&principal);
    onstage.set_proveedor(move || {
        Some(Fuente {
            texto: nota.to_owned(),
            ruta: ruta.clone(),
            vault: Some(vault.clone()),
        })
    });
    onstage.presentar();
    glib::MainContext::default().block_on(async {
        fn boton(w: &gtk::Widget) -> Option<gtk::Button> {
            if let Some(b) = w.downcast_ref::<gtk::Button>()
                && b.tooltip_text().as_deref() == Some("Reproducir")
            {
                return Some(b.clone());
            }
            let mut c = w.first_child();
            while let Some(h) = c {
                if let Some(b) = boton(&h) {
                    return Some(b);
                }
                c = h.next_sibling();
            }
            None
        }
        let mut play = None;
        for _ in 0..200 {
            play = boton(onstage.0.reader.widget().upcast_ref());
            if play.is_some() {
                break;
            }
            glib::timeout_future(Duration::from_millis(20)).await;
        }
        let play = play.expect("el reproductor en la diapositiva");
        let contador = onstage.0.contador_barra.label().to_string();
        assert!(play.grab_focus(), "el botón no tomó el foco");
        assert!(crate::medios::tiene_foco(&onstage.0.window));
        for k in [Key::space, Key::Return, Key::Right, Key::Left] {
            assert_eq!(onstage.tecla(k), glib::Propagation::Proceed, "{k:?}");
        }
        assert_eq!(onstage.0.contador_barra.label().as_str(), contador);
        // Re Pág sigue navegando aunque el foco esté en el reproductor.
        assert_eq!(onstage.tecla(Key::Page_Down), glib::Propagation::Stop);
        assert_ne!(onstage.0.contador_barra.label().as_str(), contador);
        onstage.0.atras.emit_clicked();
        // Con el foco fuera del reproductor, Espacio pasa de diapositiva.
        onstage.0.adelante.grab_focus();
        assert!(!crate::medios::tiene_foco(&onstage.0.window));
        let antes = onstage.0.contador_barra.label().to_string();
        assert_eq!(onstage.tecla(Key::space), glib::Propagation::Stop);
        assert_ne!(onstage.0.contador_barra.label().as_str(), antes);
    });
    onstage.0.window.close();
    let _ = std::fs::remove_dir_all(&raiz);
}
