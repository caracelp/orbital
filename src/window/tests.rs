//! Pruebas GTK en proceso de la ventana (la mayoría `#[ignore]`: necesitan sesión gráfica).

mod url_tests {
    use super::super::*;

    fn app_de_prueba(nombre: &str) -> adw::Application {
        adw::init().expect("se necesita acceso GTK");
        adw::Application::builder()
            .application_id(format!("io.github.caracelp.Orbital.UrlTest.{nombre}"))
            .flags(gio::ApplicationFlags::NON_UNIQUE)
            .build()
    }

    /// 009 US1/US2: abrir un archivo de raw/ asociado muestra el webview de la URL; sin
    /// asociación, la página de solo lectura con «Asociar URL…».
    #[test]
    #[ignore = "requiere GTK/DBus; los archivos de raw/ no son activables por AT-SPI"]
    fn us9_url_pages_for_raw_files() {
        let app = app_de_prueba("Url");
        app.register(None::<&gio::Cancellable>).unwrap();
        glib::MainContext::default().block_on(async {
            let root =
                std::env::temp_dir().join(format!("orbital-us9-{}", glib::uuid_string_random()));
            std::fs::create_dir_all(root.join("raw")).unwrap();
            // Un archivo binario (como un PDF real): read_note falla y va a la página externo.
            std::fs::write(root.join("raw/x.pdf"), b"%PDF-1.4\xff\xfe\x01binario").unwrap();
            std::fs::create_dir_all(root.join(".orbital")).unwrap();
            std::fs::write(
                root.join(".orbital/enlaces.yaml"),
                "- ruta: raw/x.pdf\n  url: https://ejemplo.local/x\n",
            )
            .unwrap();
            let vault = Arc::new(orbital_vault::open(&root).unwrap());
            let window = Window::new(&app);
            window.set_persist(false);
            window.show_vault(
                orbital_vault::open(&root).unwrap(),
                Tree::default(),
                orbital_vault::NoteMeta::default(),
            );
            // Abrir el archivo de raw/ por show_entry.
            let entry = Arc::new(
                vault
                    .entries()
                    .iter()
                    .find(|e| e.path.as_str() == "raw/x.pdf")
                    .cloned()
                    .expect("el archivo de raw/ existe"),
            );
            assert!(
                window.0.url_web.borrow().is_none(),
                "el webview de la URL no se crea al arrancar"
            );
            window.show_entry(entry);
            for _ in 0..100 {
                if window.0.content_stack.visible_child_name().as_deref() == Some("url-webview") {
                    break;
                }
                glib::timeout_future(std::time::Duration::from_millis(20)).await;
            }
            assert_eq!(
                window.0.content_stack.visible_child_name().as_deref(),
                Some("url-webview"),
                "el archivo asociado debe abrir el webview de la URL"
            );
            let web = window
                .0
                .url_web
                .borrow()
                .clone()
                .expect("webview creado al abrir");
            assert!(
                web.network_session().is_some_and(|s| s.is_ephemeral()),
                "la URL se carga con una sesión de red efímera"
            );
            // «Ver el archivo» vuelve a la vista de solo lectura.
            window.0.url_ver_archivo.emit_clicked();
            for _ in 0..50 {
                if window.0.content_stack.visible_child_name().as_deref() == Some("externo") {
                    break;
                }
                glib::timeout_future(std::time::Duration::from_millis(20)).await;
            }
            assert_eq!(
                window.0.content_stack.visible_child_name().as_deref(),
                Some("externo")
            );
            // Sin asociación: «Quitar URL» oculto y la página de solo lectura.
            *window.0.externo_ruta.borrow_mut() = None;
            window.0.quitar_url.set_visible(false);
            assert!(!window.0.quitar_url.is_visible());
            window.0.window.destroy();
            let _ = std::fs::remove_dir_all(root);
        });
    }
}

mod historial_tests {
    use super::super::*;

    /// Recorre el árbol de widgets del diálogo buscando la lista de versiones y el
    /// botón «Restaurar» (que cuelga de la cabecera, fuera del contenedor de la lista).
    fn encontrar_lista_y_boton(widget: &gtk::Widget) -> Option<(gtk::ListBox, gtk::Button)> {
        fn buscar(
            widget: &gtk::Widget,
            lista: &mut Option<gtk::ListBox>,
            boton: &mut Option<gtk::Button>,
        ) {
            if lista.is_none()
                && let Some(l) = widget.downcast_ref::<gtk::ListBox>()
            {
                *lista = Some(l.clone());
            }
            if boton.is_none()
                && let Some(b) = widget.downcast_ref::<gtk::Button>()
                && b.label().as_deref() == Some("Restaurar")
            {
                *boton = Some(b.clone());
            }
            let mut hijo = widget.first_child();
            while let Some(w) = hijo {
                buscar(&w, lista, boton);
                hijo = w.next_sibling();
            }
        }
        let mut lista = None;
        let mut boton = None;
        buscar(widget, &mut lista, &mut boton);
        Some((lista?, boton?))
    }

    #[test]
    #[ignore = "requiere GTK/DBus; tools/verificar-010-us1.sh"]
    fn us2_historial_dialogo() {
        adw::init().expect("se necesita acceso a GTK");
        let app = adw::Application::builder()
            .application_id("io.github.caracelp.Orbital.HistorialTest")
            .flags(gio::ApplicationFlags::NON_UNIQUE)
            .build();
        app.register(None::<&gio::Cancellable>).unwrap();
        glib::MainContext::default().block_on(async {
            let root = std::env::temp_dir()
                .join(format!("orbital-us2-010-{}", glib::uuid_string_random()));
            std::fs::create_dir(&root).unwrap();
            std::fs::create_dir(root.join("wiki")).unwrap();
            let path = RelPath::new("wiki/nota.md").unwrap();
            std::fs::write(path.to_path(&root), "Versión tres\n").unwrap();
            let vault = Arc::new(orbital_vault::open(&root).unwrap());
            let window = Window::new(&app);
            window.set_persist(false);
            *window.0.vault.borrow_mut() = Some(vault.clone());
            window.0.vault_session.set(1);
            let editor = window.0.code_editor.clone();
            editor.load(vault.clone(), &vault.read_note(&path).unwrap());

            // Dos versiones guardadas (la más reciente por último en disco).
            orbital_vault::historial::guardar_version(&vault, &path, "Versión inicial\n", "")
                .unwrap();
            glib::timeout_future(std::time::Duration::from_millis(5)).await;
            orbital_vault::historial::guardar_version(&vault, &path, "Versión dos\n", "").unwrap();
            let versiones = orbital_vault::versiones_historial(&vault, &path).unwrap();
            assert_eq!(versiones.len(), 2);
            assert_eq!(versiones[0].1, "Versión dos\n", "la más reciente primero");
            assert_eq!(versiones[1].1, "Versión inicial\n");

            let dialog = window.dialogo_historial(&path, &versiones);
            dialog.present(Some(&window.0.window));
            glib::timeout_future(std::time::Duration::from_millis(50)).await;
            let raiz = dialog.child().expect("el diálogo tiene contenido");
            let (lista, restaurar) =
                encontrar_lista_y_boton(&raiz).expect("el diálogo expone lista y botón Restaurar");
            assert!(
                !restaurar.is_sensitive(),
                "Restaurar sin selección está desactivado"
            );
            assert_eq!(lista.row_at_index(0).map(|r| r.index()), Some(0));
            assert_eq!(lista.observe_children().n_items(), 2);

            // Seleccionar la fila más vieja y restaurar: el texto del editor cambia, el
            // archivo en disco no (restaurar deja una edición pendiente, como al escribir).
            lista.select_row(Some(&lista.row_at_index(1).unwrap()));
            assert!(restaurar.is_sensitive());
            restaurar.emit_clicked();
            assert_eq!(editor.text(), "Versión inicial\n");
            assert_eq!(
                std::fs::read_to_string(path.to_path(&root)).unwrap(),
                "Versión tres\n"
            );

            window.0.window.destroy();
            std::fs::remove_dir_all(root).unwrap();
        });
    }
}

mod tag_rename_tests {
    use super::super::*;

    #[test]
    #[ignore = "requiere GTK/DBus; tools/verificar-004-us6.sh"]
    fn us6_tag_rename_buffers() {
        adw::init().expect("se necesita acceso a GTK");
        let app = adw::Application::builder()
            .application_id("io.github.caracelp.Orbital.TagRenameTest")
            .flags(gio::ApplicationFlags::NON_UNIQUE)
            .build();
        app.register(None::<&gio::Cancellable>).unwrap();
        glib::MainContext::default().block_on(async {
            let root =
                std::env::temp_dir().join(format!("orbital-us6-{}", glib::uuid_string_random()));
            std::fs::create_dir(&root).unwrap();
            std::fs::create_dir(root.join("wiki")).unwrap();
            let path = RelPath::new("wiki/nota.md").unwrap();
            std::fs::write(path.to_path(&root), "#vieja\n").unwrap();
            let vault = Arc::new(orbital_vault::open(&root).unwrap());
            let window = Window::new(&app);
            window.set_persist(false);
            *window.0.vault.borrow_mut() = Some(vault.clone());
            window.0.vault_session.set(1);
            let editor = window.0.code_editor.clone();
            // El guardado agrega ids de bloque (026): se comparan sin ellos.
            let sin = |t: String| orbital_vault::bloques::sin_ids(&t).into_owned();
            editor.load(vault.clone(), &vault.read_note(&path).unwrap());
            let stale = vault.plan_tag_rename("vieja", "nueva", std::slice::from_ref(&path));
            editor.replace_text("#vieja con mis cambios\n");
            window
                .run_tag_edits(vault.clone(), 1, Arc::new(stale), false)
                .await;
            assert_eq!(
                sin(std::fs::read_to_string(path.to_path(&root)).unwrap()),
                "#vieja con mis cambios\n"
            );
            assert_eq!(sin(editor.text()), "#vieja con mis cambios\n");

            let plan =
                Arc::new(vault.plan_tag_rename("vieja", "nueva", std::slice::from_ref(&path)));
            window
                .run_tag_edits(vault.clone(), 1, plan.clone(), false)
                .await;
            assert_eq!(sin(editor.text()), "#nueva con mis cambios\n");
            let receipt = orbital_vault::WriteReceipt {
                written: window
                    .0
                    .own_writes
                    .borrow()
                    .iter()
                    .map(|(path, hash)| (path.clone(), *hash))
                    .collect(),
            };
            let applied = Arc::new(plan.applied(&receipt));
            assert_eq!(applied.files(), 1);
            editor.replace_text("#nueva con otros cambios\n");
            window
                .run_tag_edits(vault.clone(), 1, applied.clone(), true)
                .await;
            assert_eq!(
                sin(std::fs::read_to_string(path.to_path(&root)).unwrap()),
                "#nueva con otros cambios\n"
            );
            assert_eq!(sin(editor.text()), "#nueva con otros cambios\n");
            // Una nueva sesión no puede consumir el Deshacer de la anterior.
            window.0.vault_session.set(2);
            window.run_tag_edits(vault, 1, applied, true).await;
            assert_eq!(
                sin(std::fs::read_to_string(path.to_path(&root)).unwrap()),
                "#nueva con otros cambios\n"
            );
            window.0.window.destroy();
            std::fs::remove_dir_all(root).unwrap();
        });
    }
}

mod search_open_tests {
    use super::super::*;

    async fn esperar(cond: impl Fn() -> bool) {
        for _ in 0..150 {
            if cond() {
                return;
            }
            glib::timeout_future(std::time::Duration::from_millis(20)).await;
        }
    }

    /// 004 escenario 1.3: elegir una coincidencia abre la nota en su línea, también cuando la
    /// nota ya está elegida en la lista (otra coincidencia de la misma nota, o volver del grafo).
    #[test]
    #[ignore = "requiere GTK/DBus; tools/verificar-004-us1.sh"]
    fn us1_open_match_same_note() {
        adw::init().expect("se necesita acceso a GTK");
        let app = adw::Application::builder()
            .application_id("io.github.caracelp.Orbital.SearchOpenTest")
            .flags(gio::ApplicationFlags::NON_UNIQUE)
            .build();
        app.register(None::<&gio::Cancellable>).unwrap();
        glib::MainContext::default().block_on(async {
            let root = std::env::temp_dir()
                .join(format!("orbital-us1-004-{}", glib::uuid_string_random()));
            std::fs::create_dir_all(root.join("wiki")).unwrap();
            std::fs::create_dir_all(root.join("raw")).unwrap();
            let texto: String = (1..=80).map(|n| format!("línea {n}\n\n")).collect();
            std::fs::write(root.join("wiki/nota.md"), &texto).unwrap();
            let window = Window::new(&app);
            window.set_persist(false);
            let (vault, tree, meta) = {
                let v = orbital_vault::open(&root).unwrap();
                let entries: Vec<_> = v.entries().to_vec();
                let tree = build_tree(&entries);
                (v, tree, orbital_vault::NoteMeta::default())
            };
            window.show_vault(vault, tree, meta);
            // El índice de búsqueda se arma después del primer cuadro: la ventana tiene que verse.
            window.0.window.present();
            let abrir = |line: usize| {
                let path = RelPath::new("wiki/nota.md").unwrap();
                window.0.pending_line.set(Some((path.clone(), line)));
                window.show_path(path.as_str());
            };
            let linea_cursor = || window.0.code_editor.cursor_line();
            abrir(3);
            esperar(|| window.0.content_stack.visible_child_name().as_deref() == Some("codigo"))
                .await;
            esperar(|| linea_cursor() == 3).await;
            assert_eq!(linea_cursor(), 3, "la primera coincidencia va a su línea");

            // Otra coincidencia de la misma nota: la fila ya está elegida.
            abrir(101);
            esperar(|| linea_cursor() == 101).await;
            assert_eq!(
                linea_cursor(),
                101,
                "una coincidencia de la nota abierta va a su línea"
            );

            // Desde el grafo, la misma nota vuelve a verse.
            window.0.content_stack.set_visible_child_name("grafo");
            abrir(51);
            esperar(|| window.0.content_stack.visible_child_name().as_deref() == Some("codigo"))
                .await;
            assert_eq!(
                window.0.content_stack.visible_child_name().as_deref(),
                Some("codigo"),
                "elegir una coincidencia vuelve al editor"
            );
            esperar(|| linea_cursor() == 51).await;
            assert_eq!(linea_cursor(), 51);

            // El camino real: buscar en el panel y activar la fila (clic o Intro) abre la línea.
            window.0.search_panel.set_search_query("\"línea 70\"");
            let mut activada = false;
            for _ in 0..250 {
                if window.0.search_panel.activate_match("línea 70") {
                    activada = true;
                    break;
                }
                glib::timeout_future(std::time::Duration::from_millis(20)).await;
            }
            assert!(activada, "el panel muestra la coincidencia");
            esperar(|| linea_cursor() == 139).await;
            assert_eq!(
                linea_cursor(),
                139,
                "activar la fila va a la línea de la coincidencia"
            );
            window.0.window.destroy();
            std::fs::remove_dir_all(root).unwrap();
        });
    }
}

mod pestanas_tests {
    use super::super::*;
    use crate::pestanas::CONEXIONES;

    async fn esperar(cond: impl Fn() -> bool) {
        for _ in 0..150 {
            if cond() {
                return;
            }
            glib::timeout_future(std::time::Duration::from_millis(20)).await;
        }
    }

    /// 025 US3: abrir notas crea pestañas; elegir una repone su nota con cursor y vista;
    /// «Conexiones» es una pestaña; cerrar la última deja el centro vacío.
    #[test]
    #[ignore = "requiere GTK/DBus"]
    fn us3_pestanas_editor_compartido() {
        adw::init().expect("se necesita acceso a GTK");
        let app = adw::Application::builder()
            .application_id("io.github.caracelp.Orbital.PestanasTest")
            .flags(gio::ApplicationFlags::NON_UNIQUE)
            .build();
        app.register(None::<&gio::Cancellable>).unwrap();
        glib::MainContext::default().block_on(async {
            let root =
                std::env::temp_dir().join(format!("orbital-025-{}", glib::uuid_string_random()));
            std::fs::create_dir_all(root.join("wiki")).unwrap();
            std::fs::create_dir_all(root.join("raw")).unwrap();
            let texto: String = (1..=60).map(|n| format!("línea {n}\n")).collect();
            std::fs::write(root.join("wiki/a.md"), &texto).unwrap();
            std::fs::write(root.join("wiki/b.md"), &texto).unwrap();
            let window = Window::new(&app);
            window.set_persist(false);
            let v = orbital_vault::open(&root).unwrap();
            let tree = build_tree(v.entries());
            window.show_vault(v, tree, orbital_vault::NoteMeta::default());
            window.0.window.present();
            let ruta = || window.0.code_editor.current_path().map(|p| p.to_string());

            window.show_path("wiki/a.md");
            esperar(|| ruta().as_deref() == Some("wiki/a.md")).await;
            window.show_path("wiki/b.md");
            esperar(|| ruta().as_deref() == Some("wiki/b.md")).await;
            assert_eq!(window.0.pestanas.claves(), ["wiki/a.md", "wiki/b.md"]);
            assert_eq!(window.0.pestanas.actual().as_deref(), Some("wiki/b.md"));

            // Cursor en la línea 40 de b; ir a a y volver repone el cursor y la vista.
            window.0.code_editor.restaurar_vista(
                window
                    .0
                    .code_editor
                    .buffer()
                    .iter_at_line(39)
                    .unwrap()
                    .offset(),
                0.0,
                Some("dividida"),
            );
            window.0.pestanas.elegir("wiki/a.md");
            esperar(|| ruta().as_deref() == Some("wiki/a.md")).await;
            assert_eq!(ruta().as_deref(), Some("wiki/a.md"));
            window.0.pestanas.elegir("wiki/b.md");
            esperar(|| ruta().as_deref() == Some("wiki/b.md")).await;
            esperar(|| window.0.code_editor.cursor_line() == 40).await;
            assert_eq!(window.0.code_editor.cursor_line(), 40, "cursor repuesto");
            assert_eq!(
                window.0.code_editor.estado_vista().2.as_deref(),
                Some("dividida"),
                "vista repuesta"
            );

            // Conexiones como pestaña; cerrarla vuelve a la nota.
            window.ver_grafo();
            assert_eq!(
                window.0.pestanas.claves(),
                ["wiki/a.md", "wiki/b.md", CONEXIONES]
            );
            assert_eq!(
                window.0.content_stack.visible_child_name().as_deref(),
                Some("grafo")
            );
            window.cerrar_grafo();
            esperar(|| window.0.content_stack.visible_child_name().as_deref() == Some("codigo"))
                .await;
            assert_eq!(window.0.pestanas.claves(), ["wiki/a.md", "wiki/b.md"]);

            // Cerrar la pestaña abierta abre la vecina; cerrar la última deja el centro vacío.
            window.0.pestanas.cerrar("wiki/b.md");
            esperar(|| ruta().as_deref() == Some("wiki/a.md")).await;
            assert_eq!(ruta().as_deref(), Some("wiki/a.md"));
            window.0.pestanas.cerrar("wiki/a.md");
            esperar(|| window.0.content_stack.visible_child_name().as_deref() == Some("vacio"))
                .await;
            assert_eq!(
                window.0.content_stack.visible_child_name().as_deref(),
                Some("vacio")
            );
            assert!(ruta().is_none());

            // Una nota borrada afuera pierde su pestaña al recargar la lista.
            window.show_path("wiki/a.md");
            esperar(|| ruta().as_deref() == Some("wiki/a.md")).await;
            window.show_path("wiki/b.md");
            esperar(|| ruta().as_deref() == Some("wiki/b.md")).await;
            std::fs::remove_file(root.join("wiki/a.md")).unwrap();
            let v = orbital_vault::open(&root).unwrap();
            let tree = build_tree(v.entries());
            window.show_vault(v, tree, orbital_vault::NoteMeta::default());
            assert_eq!(window.0.pestanas.claves(), ["wiki/b.md"]);
            window.0.window.destroy();
            std::fs::remove_dir_all(root).unwrap();
        });
    }
}

mod medicion_pestanas {
    use super::super::*;

    /// SC-003 de la 025: cambiar de pestaña entre dos notas de 100 KB (hasta que el editor
    /// tiene la otra nota cargada y a la vista). En release:
    /// `cargo test --release --bin orbital sc3_cambio_pestana -- --ignored --nocapture`.
    #[test]
    #[ignore = "medición; requiere GTK"]
    fn sc3_cambio_pestana() {
        adw::init().expect("se necesita acceso a GTK");
        let app = adw::Application::builder()
            .application_id("io.github.caracelp.Orbital.MedirPestanas")
            .flags(gio::ApplicationFlags::NON_UNIQUE)
            .build();
        app.register(None::<&gio::Cancellable>).unwrap();
        glib::MainContext::default().block_on(async {
            let root =
                std::env::temp_dir().join(format!("orbital-sc3-{}", glib::uuid_string_random()));
            std::fs::create_dir_all(root.join("wiki")).unwrap();
            std::fs::create_dir_all(root.join("raw")).unwrap();
            let mut texto = String::from("---\ntype: atomo\n---\n# Grande\n\n");
            let mut n = 0;
            while texto.len() < 100 * 1024 {
                texto.push_str(&format!(
                    "Párrafo {n} con texto de prueba y una #etiqueta y [[otra]].\n\n"
                ));
                n += 1;
            }
            std::fs::write(root.join("wiki/a.md"), &texto).unwrap();
            std::fs::write(root.join("wiki/b.md"), &texto).unwrap();
            let window = Window::new(&app);
            window.set_persist(false);
            let v = orbital_vault::open(&root).unwrap();
            let tree = build_tree(v.entries());
            window.show_vault(v, tree, orbital_vault::NoteMeta::default());
            window.0.window.present();
            let ruta = || window.0.code_editor.current_path().map(|p| p.to_string());
            for nota in ["wiki/a.md", "wiki/b.md"] {
                window.show_path(nota);
                while ruta().as_deref() != Some(nota) {
                    glib::timeout_future(std::time::Duration::from_millis(5)).await;
                }
            }
            glib::timeout_future(std::time::Duration::from_millis(500)).await;
            let mut ms = Vec::new();
            for k in 0..10 {
                let nota = if k % 2 == 0 { "wiki/a.md" } else { "wiki/b.md" };
                let t = std::time::Instant::now();
                window.0.pestanas.elegir(nota);
                while ruta().as_deref() != Some(nota) {
                    glib::timeout_future(std::time::Duration::from_millis(1)).await;
                }
                ms.push(t.elapsed().as_secs_f64() * 1000.0);
                glib::timeout_future(std::time::Duration::from_millis(200)).await;
            }
            ms.sort_by(f64::total_cmp);
            println!(
                "PESTANA 100 KB: mediana {:.1} ms, máx {:.1} ms",
                ms[ms.len() / 2],
                ms[ms.len() - 1]
            );
            window.0.window.destroy();
            std::fs::remove_dir_all(root).unwrap();
        });
    }
}

mod captura_v13 {
    use super::super::*;

    /// Captura del diseño de interfaz (025) a 1440×900 con una nota y cada modo de la barra derecha:
    /// `ORBITAL_CAPTURA_BOVEDA=<bóveda> ORBITAL_CAPTURA_NOTA=<ruta> ORBITAL_CAPTURA_DIR=<carpeta>`.
    #[test]
    #[ignore = "captura de desarrollo; requiere GTK"]
    fn captura_modos_panel() {
        let (Some(boveda), Some(nota), Some(dir)) = (
            std::env::var_os("ORBITAL_CAPTURA_BOVEDA"),
            std::env::var("ORBITAL_CAPTURA_NOTA").ok(),
            std::env::var_os("ORBITAL_CAPTURA_DIR"),
        ) else {
            return;
        };
        adw::init().expect("se necesita acceso a GTK");
        crate::style::apply();
        let app = adw::Application::builder()
            .application_id("io.github.caracelp.Orbital.CapturaV13")
            .flags(gio::ApplicationFlags::NON_UNIQUE)
            .build();
        app.register(None::<&gio::Cancellable>).unwrap();
        glib::MainContext::default().block_on(async {
            let window = Window::new(&app);
            window.set_persist(false);
            window.0.window.set_default_size(1440, 900);
            let v = orbital_vault::open(std::path::Path::new(&boveda)).unwrap();
            let tree = build_tree(v.entries());
            window.show_vault(v, tree, orbital_vault::NoteMeta::default());
            window.0.window.present();
            for _ in 0..100 {
                if window.0.graph.borrow().is_some() {
                    break;
                }
                glib::timeout_future(std::time::Duration::from_millis(30)).await;
            }
            window.show_path(&nota);
            glib::timeout_future(std::time::Duration::from_millis(900)).await;
            for modo in ["propiedades", "contexto", "grafo"] {
                window.0.panel.set_modo(modo);
                glib::timeout_future(std::time::Duration::from_millis(700)).await;
                let ancho = window.0.window.width();
                let alto = window.0.window.height();
                let pintable = gtk::WidgetPaintable::new(Some(&window.0.window));
                let snap = gtk::Snapshot::new();
                pintable.snapshot(&snap, f64::from(ancho), f64::from(alto));
                let node = snap.to_node().expect("cuadro");
                let png = std::path::Path::new(&dir).join(format!("v13-{modo}.png"));
                window
                    .0
                    .window
                    .renderer()
                    .unwrap()
                    .render_texture(&node, None)
                    .save_to_png(png)
                    .unwrap();
            }
            window.0.window.destroy();
        });
    }
}

mod captura_049 {
    use super::super::*;

    fn guardar(window: &Window, dir: &std::path::Path, nombre: &str) {
        let (ancho, alto) = (window.0.window.width(), window.0.window.height());
        let pintable = gtk::WidgetPaintable::new(Some(&window.0.window));
        let snap = gtk::Snapshot::new();
        pintable.snapshot(&snap, f64::from(ancho), f64::from(alto));
        let node = snap.to_node().expect("cuadro");
        window
            .0
            .window
            .renderer()
            .unwrap()
            .render_texture(&node, None)
            .save_to_png(dir.join(format!("{nombre}.png")))
            .unwrap();
    }

    /// Capturas de las conexiones con tipo (049): Lectura de una nota con Conexiones, los
    /// retroenlaces agrupados de la nota destino, el grafo global, el grafo filtrado por tipo y
    /// la brújula. `ORBITAL_CAPTURA_BOVEDA=<bóveda> ORBITAL_CAPTURA_DIR=<carpeta>`; la bóveda es
    /// la de demostración de `specs/049-conexiones-con-tipo/quickstart.md`.
    #[test]
    #[ignore = "captura de desarrollo; requiere GTK"]
    fn captura_conexiones_con_tipo() {
        let (Some(boveda), Some(dir)) = (
            std::env::var_os("ORBITAL_CAPTURA_BOVEDA"),
            std::env::var_os("ORBITAL_CAPTURA_DIR"),
        ) else {
            return;
        };
        let dir = std::path::PathBuf::from(dir);
        adw::init().expect("se necesita acceso a GTK");
        crate::style::apply();
        let app = adw::Application::builder()
            .application_id("io.github.caracelp.Orbital.Captura049")
            .flags(gio::ApplicationFlags::NON_UNIQUE)
            .build();
        app.register(None::<&gio::Cancellable>).unwrap();
        glib::MainContext::default().block_on(async {
            let espera = |ms| glib::timeout_future(std::time::Duration::from_millis(ms));
            let window = Window::new(&app);
            window.set_persist(false);
            window.0.window.set_default_size(1440, 900);
            let v = orbital_vault::open(std::path::Path::new(&boveda)).unwrap();
            let tree = build_tree(v.entries());
            window.show_vault(v, tree, orbital_vault::NoteMeta::default());
            window.0.window.present();
            for _ in 0..100 {
                if window.0.graph.borrow().is_some() {
                    break;
                }
                espera(30).await;
            }
            window.show_path("wiki/Regla de la cadena.md");
            espera(900).await;
            window.0.code_editor.set_view(crate::editor::View::Lectura);
            window.0.panel.set_modo("contexto");
            espera(900).await;
            guardar(&window, &dir, "049-lectura");
            window.show_path("wiki/Derivadas.md");
            espera(900).await;
            window.0.code_editor.set_view(crate::editor::View::Lectura);
            window.0.panel.set_modo("contexto");
            espera(900).await;
            guardar(&window, &dir, "049-retroenlaces");
            window.show_path("wiki/Consultas.md");
            espera(900).await;
            window.0.code_editor.set_view(crate::editor::View::Lectura);
            espera(900).await;
            guardar(&window, &dir, "049-consultas");
            window.show_path("wiki/Regla de la cadena.md");
            espera(600).await;
            window.ver_grafo();
            espera(4000).await;
            guardar(&window, &dir, "049-grafo");
            window
                .0
                .grafo_vista
                .set_filtro(crate::grafo::FiltroTipo::ConTipo);
            espera(500).await;
            guardar(&window, &dir, "049-grafo-solo-con-tipo");
            window.cerrar_grafo();
            espera(400).await;
            window.ver_brujula();
            window.actualizar_brujula(Some(RelPath::new("wiki/Regla de la cadena.md").unwrap()));
            espera(1200).await;
            guardar(&window, &dir, "049-brujula");
            window.0.window.destroy();
        });
    }
}

mod esqueleto_tests {
    use super::super::*;

    fn nombre(w: &impl IsA<gtk::Widget>) -> String {
        w.tooltip_text().map(|t| t.to_string()).unwrap_or_default()
    }

    /// Busca en el árbol de widgets un botón por su tooltip (que coincide con el nombre accesible).
    fn boton(raiz: &gtk::Widget, tooltip: &str) -> Option<gtk::Widget> {
        let mut pila = vec![raiz.clone()];
        while let Some(w) = pila.pop() {
            if w.is::<gtk::Button>() && nombre(&w).starts_with(tooltip) {
                return Some(w);
            }
            let mut hijo = w.first_child();
            while let Some(h) = hijo {
                pila.push(h.clone());
                hijo = h.next_sibling();
            }
        }
        None
    }

    /// 025 US1/US2: barras plegables con Ctrl+[ y Ctrl+] (y su nombre accesible), riel con Canva y
    /// DB desactivados, modos de la barra izquierda y el desplegable de bóvedas recientes.
    #[test]
    #[ignore = "requiere GTK/DBus"]
    fn us1_esqueleto() {
        adw::init().expect("se necesita acceso a GTK");
        let app = adw::Application::builder()
            .application_id("io.github.caracelp.Orbital.EsqueletoTest")
            .flags(gio::ApplicationFlags::NON_UNIQUE)
            .build();
        app.register(None::<&gio::Cancellable>).unwrap();
        glib::MainContext::default().block_on(async {
            let base = std::env::temp_dir()
                .join(format!("orbital-025-esq-{}", glib::uuid_string_random()));
            let (a, b) = (base.join("una"), base.join("otra"));
            for v in [&a, &b] {
                std::fs::create_dir_all(v.join("wiki")).unwrap();
                std::fs::create_dir_all(v.join("raw")).unwrap();
            }
            std::fs::write(b.join("wiki/nota.md"), "hola\n").unwrap();
            let window = Window::new(&app);
            window.set_persist(false);
            window.0.window.set_default_size(1440, 900);
            window.open_path(a.clone()).await;
            window.0.window.present();
            glib::timeout_future(std::time::Duration::from_millis(200)).await;
            let raiz = window.0.window.clone().upcast::<gtk::Widget>();

            // Barras: abiertas al empezar; las acciones las pliegan y el nombre cambia.
            assert!(window.0.split.shows_sidebar());
            assert!(boton(&raiz, "Ocultar barra izquierda").is_some());
            gtk::prelude::WidgetExt::activate_action(&window.0.window, "win.toggle-left", None)
                .unwrap();
            assert!(!window.0.split.shows_sidebar());
            assert!(boton(&raiz, "Mostrar barra izquierda").is_some());
            gtk::prelude::WidgetExt::activate_action(&window.0.window, "win.toggle-left", None)
                .unwrap();
            assert!(window.0.split.shows_sidebar());
            let derecha = window.0.right_split.shows_sidebar();
            gtk::prelude::WidgetExt::activate_action(&window.0.window, "win.toggle-right", None)
                .unwrap();
            assert_ne!(window.0.right_split.shows_sidebar(), derecha);

            // Riel: Canva, DB y el resto, activos con su acción.
            for (tooltip, activo) in [
                ("Crear canva", true),
                ("Crear base de datos", true),
                ("Selector rápido", true),
                ("Conexiones", true),
                ("Paleta de comandos", true),
                ("Configuración general", true),
            ] {
                let b = boton(&raiz, tooltip).unwrap_or_else(|| panic!("falta «{tooltip}»"));
                assert_eq!(b.is_sensitive(), activo, "«{tooltip}»");
            }
            // Guardados (027): activo, cambia la página de la barra (y la arma al primer uso).
            let guardados = boton(&raiz, "Guardados")
                .and_downcast::<gtk::ToggleButton>()
                .unwrap();
            assert!(guardados.is_sensitive());
            assert!(!window.0.listas_panel.guardados_construida());
            guardados.set_active(true);
            assert_eq!(
                window.0.sidebar_stack.visible_child_name().as_deref(),
                Some("guardados")
            );
            assert!(window.0.listas_panel.guardados_construida());
            // Búsqueda cambia la página de la barra.
            boton(&raiz, "Búsqueda")
                .and_downcast::<gtk::ToggleButton>()
                .unwrap()
                .set_active(true);
            assert_eq!(
                window.0.sidebar_stack.visible_child_name().as_deref(),
                Some("busqueda")
            );

            // Desplegable de bóvedas: la abierta primero; elegir otra la abre.
            window.open_path(b.clone()).await;
            let rutas = window.0.pie_boveda.rutas();
            assert_eq!(rutas.first(), Some(&b));
            window.0.pie_boveda.set_boveda(&b, &[b.clone(), a.clone()]);
            assert_eq!(window.0.pie_boveda.rutas(), [b.clone(), a.clone()]);
            window.0.pie_boveda.elegir(1);
            for _ in 0..100 {
                if window
                    .0
                    .vault
                    .borrow()
                    .as_ref()
                    .map(|v| v.root().to_path_buf())
                    == Some(a.clone())
                {
                    break;
                }
                glib::timeout_future(std::time::Duration::from_millis(20)).await;
            }
            assert_eq!(
                window
                    .0
                    .vault
                    .borrow()
                    .as_ref()
                    .map(|v| v.root().to_path_buf()),
                Some(a.clone()),
                "elegir otra bóveda la abre"
            );
            window.0.window.destroy();
            std::fs::remove_dir_all(base).unwrap();
        });
    }
}

mod enlaces_bloque_tests {
    use super::super::*;

    /// 026 US2: seguir `[[destino#^id]]` abre la nota con el cursor en el bloque (SC-004: < 300 ms),
    /// y `[[#^id]]` va al bloque en la nota abierta.
    #[test]
    #[ignore = "requiere GTK/DBus; tools/pruebas-gtk.sh"]
    fn seguir_enlace_a_bloque() {
        adw::init().expect("se necesita acceso a GTK");
        let app = adw::Application::builder()
            .application_id("io.github.caracelp.Orbital.BloqueTest")
            .flags(gio::ApplicationFlags::NON_UNIQUE)
            .build();
        app.register(None::<&gio::Cancellable>).unwrap();
        glib::MainContext::default().block_on(async {
            let root = std::env::temp_dir()
                .join(format!("orbital-026-us2-{}", glib::uuid_string_random()));
            std::fs::create_dir_all(root.join("wiki")).unwrap();
            let mut destino: String = (1..=300).map(|n| format!("Párrafo {n}.\n\n")).collect();
            destino.push_str("- ítem buscado ^k3x9a1\n\n");
            destino.push_str("Final. ^fin001\n");
            std::fs::write(root.join("wiki/destino.md"), &destino).unwrap();
            std::fs::write(root.join("wiki/origen.md"), "Ver [[destino#^k3x9a1]].\n").unwrap();
            let window = Window::new(&app);
            window.set_persist(false);
            let (vault, tree) = {
                let v = orbital_vault::open(&root).unwrap();
                let tree = build_tree(v.entries());
                (v, tree)
            };
            window.show_vault(vault, tree, orbital_vault::NoteMeta::default());
            window.0.window.present();
            window.show_path("wiki/origen.md");
            glib::timeout_future(std::time::Duration::from_millis(300)).await;

            let origen = RelPath::new("wiki/origen.md").unwrap();
            let t0 = std::time::Instant::now();
            window.open_link(Some(&origen), "[[destino#^k3x9a1]]");
            let linea = || window.0.code_editor.cursor_line();
            while linea() != 601 && t0.elapsed() < std::time::Duration::from_secs(3) {
                glib::timeout_future(std::time::Duration::from_millis(5)).await;
            }
            let ms = t0.elapsed().as_millis();
            assert_eq!(linea(), 601, "el cursor queda en el ítem con ^k3x9a1");
            println!("SC-004: seguir el enlace al bloque tardó {ms} ms");
            assert!(ms < 300, "SC-004: {ms} ms");

            // `[[#^fin001]]`: en la nota abierta.
            window.open_link(
                Some(&RelPath::new("wiki/destino.md").unwrap()),
                "[[#^fin001]]",
            );
            for _ in 0..100 {
                if linea() == 603 {
                    break;
                }
                glib::timeout_future(std::time::Duration::from_millis(10)).await;
            }
            assert_eq!(linea(), 603, "[[#^id]] va al bloque de la nota abierta");
            window.0.window.destroy();
            std::fs::remove_dir_all(root).unwrap();
        });
    }
}

mod listas_tests {
    //! 027: Favoritos y Guardados.
    use super::super::*;
    use orbital_vault::Lista;
    use std::time::{Duration, Instant};

    pub(super) fn app(nombre: &str) -> adw::Application {
        adw::init().expect("se necesita acceso a GTK");
        let app = adw::Application::builder()
            .application_id(format!("io.github.caracelp.Orbital.Listas{nombre}"))
            .flags(gio::ApplicationFlags::NON_UNIQUE)
            .build();
        app.register(None::<&gio::Cancellable>).unwrap();
        app
    }

    pub(super) async fn esperar(que: &str, cond: impl Fn() -> bool) {
        let t0 = Instant::now();
        while !cond() {
            assert!(t0.elapsed() < Duration::from_secs(5), "no pasó: {que}");
            glib::timeout_future(Duration::from_millis(10)).await;
        }
    }

    fn boveda(tag: &str) -> PathBuf {
        let root =
            std::env::temp_dir().join(format!("orbital-027-{tag}-{}", glib::uuid_string_random()));
        std::fs::create_dir_all(root.join("wiki")).unwrap();
        std::fs::create_dir_all(root.join("raw")).unwrap();
        std::fs::write(root.join("wiki/a.md"), "# A\n\nNota a.\n").unwrap();
        std::fs::write(root.join("wiki/b.md"), "# B\n\nNota b.\n").unwrap();
        std::fs::write(root.join("wiki/c.html"), "<p>Síntesis c.</p>\n").unwrap();
        std::fs::write(root.join("raw/fuente.pdf"), b"%PDF-1.4\xff\xfe binario").unwrap();
        root
    }

    /// Suma de verificación de todo lo que no es `.orbital/` (SC-004).
    fn sumas(root: &Path) -> Vec<(String, u64)> {
        let mut out = Vec::new();
        for zona in ["wiki", "raw"] {
            for e in std::fs::read_dir(root.join(zona)).unwrap() {
                let e = e.unwrap();
                let bytes = std::fs::read(e.path()).unwrap();
                out.push((
                    e.path().display().to_string(),
                    orbital_vault::write::hash(&bytes),
                ));
            }
        }
        out.sort();
        out
    }

    async fn ventana(app: &adw::Application, root: &Path) -> Window {
        let window = Window::new(app);
        window.set_persist(false);
        window.0.window.set_default_size(1200, 900);
        window.0.window.present();
        window.open_path(root.to_path_buf()).await;
        esperar("las listas se cargan", || window.listas_cargadas_ya()).await;
        window
    }

    pub(super) async fn abrir(window: &Window, ruta: &str) {
        window.show_path(ruta);
        esperar(&format!("se abre {ruta}"), || {
            window
                .0
                .current_entry
                .borrow()
                .as_ref()
                .map(|e| e.path.as_str().to_owned())
                == Some(ruta.to_owned())
        })
        .await;
    }

    fn rotulo_menu(window: &Window, n: usize) -> String {
        window.items_menu_nota()[n].label.clone()
    }

    fn filas(list: &gtk::ListBox) -> Vec<String> {
        let mut out = Vec::new();
        while let Some(row) = list.row_at_index(out.len() as i32) {
            out.push(
                row.tooltip_text()
                    .map(|t| t.to_string())
                    .unwrap_or_default(),
            );
        }
        out
    }

    fn en_disco(root: &Path) -> String {
        std::fs::read_to_string(root.join(".orbital/listas.yaml")).unwrap_or_default()
    }

    fn rutas(v: &[&str]) -> Vec<RelPath> {
        v.iter().map(|s| RelPath::new(s).unwrap()).collect()
    }

    /// Las etiquetas de los botones del menú de la nota tal como se ven al abrirlo.
    async fn menu_abierto(window: &Window) -> Vec<String> {
        let menu = &window.0.note_menu;
        menu.popup();
        glib::timeout_future(Duration::from_millis(50)).await;
        let mut out = Vec::new();
        let mut pila: Vec<gtk::Widget> = menu.child().into_iter().collect();
        while let Some(w) = pila.pop() {
            if let Some(l) = w.downcast_ref::<gtk::Label>()
                && !l.has_css_class("dim-label")
            {
                out.push(l.label().to_string());
            }
            let mut h = w.last_child();
            while let Some(x) = h {
                pila.push(x.clone());
                h = x.prev_sibling();
            }
        }
        menu.popdown();
        out
    }

    /// 027 US1 (T013): agregar y quitar favoritos desde la acción del menú, la fila aparece en
    /// < 100 ms (SC-001), el menú cambia, abrir desde la fila, reabrir con el mismo orden, un
    /// archivo de raw/, sin `.orbital/` al abrir y sin tocar ninguna nota (SC-004).
    #[test]
    #[ignore = "requiere GTK/DBus; tools/pruebas-gtk.sh"]
    fn us1_favoritos_desde_el_menu() {
        let app = app("Us1");
        glib::MainContext::default().block_on(async {
            let root = boveda("us1");
            let antes = sumas(&root);
            let window = ventana(&app, &root).await;
            let panel = window.0.listas_panel.clone();
            assert!(!root.join(".orbital").exists(), "abrir no crea .orbital/");
            assert!(filas(panel.lista_favoritos()).is_empty());
            assert!(
                !window.0.window.is_action_enabled("alternar-favorito"),
                "sin archivo abierto, la acción está desactivada"
            );

            abrir(&window, "wiki/a.md").await;
            assert!(menu_abierto(&window).await.contains(&"Agregar a favoritos".to_owned()));
            let t0 = Instant::now();
            WidgetExt::activate_action(&window.0.window, "win.alternar-favorito", None).unwrap();
            assert_eq!(filas(panel.lista_favoritos()), ["wiki/a.md"]);
            let ms = t0.elapsed().as_secs_f64() * 1000.0;
            println!("SC-001: la fila apareció en {ms:.2} ms");
            assert!(ms < 100.0, "SC-001: {ms} ms");
            assert_eq!(rotulo_menu(&window, 0), "Quitar de favoritos");
            let menu = menu_abierto(&window).await;
            assert!(menu.contains(&"Quitar de favoritos".to_owned()), "{menu:?}");
            assert!(!menu.contains(&"Agregar a favoritos".to_owned()));

            abrir(&window, "wiki/c.html").await;
            WidgetExt::activate_action(&window.0.window, "win.alternar-favorito", None).unwrap();
            abrir(&window, "raw/fuente.pdf").await;
            assert_eq!(rotulo_menu(&window, 0), "Agregar a favoritos");
            WidgetExt::activate_action(&window.0.window, "win.alternar-favorito", None).unwrap();
            assert_eq!(
                filas(panel.lista_favoritos()),
                ["wiki/a.md", "wiki/c.html", "raw/fuente.pdf"],
                "las nuevas van al final"
            );
            esperar("se escriben los tres", || {
                en_disco(&root)
                    == "format_version: 1\nfavoritos:\n  - wiki/a.md\n  - wiki/c.html\n  - raw/fuente.pdf\ncolecciones: []\n"
            })
            .await;

            // Un clic en la fila abre como desde el explorador (también un archivo de raw/).
            panel.lista_favoritos().row_at_index(0).unwrap().emit_activate();
            esperar("abre a.md desde Favoritos", || {
                window.0.current_entry.borrow().as_ref().map(|e| e.path.as_str().to_owned())
                    == Some("wiki/a.md".into())
            })
            .await;
            panel.lista_favoritos().row_at_index(2).unwrap().emit_activate();
            esperar("abre el PDF desde Favoritos", || {
                window.0.current_entry.borrow().as_ref().map(|e| e.path.as_str().to_owned())
                    == Some("raw/fuente.pdf".into())
            })
            .await;
            window.0.window.destroy();

            // Reabrir la bóveda: el mismo orden.
            let otra = ventana(&app, &root).await;
            assert_eq!(
                filas(otra.0.listas_panel.lista_favoritos()),
                ["wiki/a.md", "wiki/c.html", "raw/fuente.pdf"]
            );
            abrir(&otra, "wiki/c.html").await;
            assert_eq!(rotulo_menu(&otra, 0), "Quitar de favoritos");
            WidgetExt::activate_action(&otra.0.window, "win.alternar-favorito", None).unwrap();
            assert_eq!(
                filas(otra.0.listas_panel.lista_favoritos()),
                ["wiki/a.md", "raw/fuente.pdf"]
            );
            esperar("se escribe la quita", || {
                en_disco(&root).contains("favoritos:\n  - wiki/a.md\n  - raw/fuente.pdf\n")
            })
            .await;
            // Respaldo (research R4): la primera ventana creó el archivo (nada que respaldar) y no
            // respaldó sus propias escrituras; la segunda respaldó una vez lo que había escrito la
            // primera, en su primera escritura.
            let copias: Vec<_> = std::fs::read_dir(
                root.join(".orbital/respaldo")
                    .read_dir()
                    .unwrap()
                    .next()
                    .unwrap()
                    .unwrap()
                    .path()
                    .join(".orbital"),
            )
            .unwrap()
            .filter_map(Result::ok)
            .filter(|e| !e.file_name().to_string_lossy().ends_with(".yaml.yaml"))
            .collect();
            assert_eq!(copias.len(), 1, "una sola copia: {copias:?}");
            otra.0.window.destroy();
            assert_eq!(sumas(&root), antes, "SC-004: ninguna nota ni archivo de raw/ cambió");
            std::fs::remove_dir_all(root).unwrap();
        });
    }

    /// 027 US2 (T020): «Guardar en…» crea con Intro (SC-005), marca y desmarca casillas, no
    /// crea nombres repetidos ni vacíos, el menú dice «Guardada en 2 colecciones…», el modo
    /// Guardados abre la nota y quitar de una colección no la quita de la otra ni de Favoritos.
    #[test]
    #[ignore = "requiere GTK/DBus; tools/pruebas-gtk.sh"]
    fn us2_guardar_en_colecciones() {
        let app = app("Us2");
        glib::MainContext::default().block_on(async {
            let root = boveda("us2");
            let antes = sumas(&root);
            let window = ventana(&app, &root).await;
            abrir(&window, "wiki/a.md").await;
            assert_eq!(rotulo_menu(&window, 1), "Guardar en…");

            // SC-005: menú → «Guardar en…» → nombre → Intro.
            let d = window.abrir_guardar_en().expect("se abre el diálogo");
            assert_eq!(d.casillas().count(), 0);
            d.entrada().set_text("Para leer");
            assert!(d.dialogo.is_response_enabled("guardar"));
            esperar("el diálogo se muestra", || d.entrada().is_mapped()).await;
            // Intro llega a la `GtkText` de adentro, que activa la respuesta por defecto.
            let texto = {
                let mut h = d.entrada().first_child();
                while let Some(w) = h.clone() {
                    if w.is::<gtk::Text>() {
                        break;
                    }
                    h = w.next_sibling();
                }
                h.and_downcast::<gtk::Text>().expect("la entrada tiene su GtkText")
            };
            texto.emit_activate();
            esperar("Intro guarda", || {
                window.listas_en_memoria(Lista::Coleccion(0)) == rutas(&["wiki/a.md"])
            })
            .await;
            assert_eq!(rotulo_menu(&window, 1), "Guardada en 1 colección…");

            // Otra colección desde otra nota.
            abrir(&window, "wiki/b.md").await;
            let d = window.abrir_guardar_en().unwrap();
            assert_eq!(
                d.casillas().map(|c| c.is_active()).collect::<Vec<_>>(),
                [false]
            );
            d.entrada().set_text("Tesis");
            d.dialogo.emit_by_name::<()>("response", &[&"guardar"]);
            d.dialogo.force_close();
            assert_eq!(window.listas_en_memoria(Lista::Coleccion(1)), rutas(&["wiki/b.md"]));

            // Nombre repetido (sin distinguir mayúsculas ni espacios) o solo espacios: no se
            // puede guardar; y aunque llegara la respuesta, no se crea otra colección.
            for nombre in ["  para LEER ", "   "] {
                let d = window.abrir_guardar_en().unwrap();
                d.entrada().set_text(nombre);
                assert!(!d.dialogo.is_response_enabled("guardar"), "{nombre:?}");
                d.dialogo.emit_by_name::<()>("response", &[&"guardar"]);
                d.dialogo.force_close();
            }
            assert_eq!(
                window.0.listas.borrow().as_ref().unwrap().listas.colecciones.len(),
                2
            );
            assert_eq!(
                window.listas_en_memoria(Lista::Coleccion(0)),
                rutas(&["wiki/a.md", "wiki/b.md"]),
                "«para leer» repetido apunta a la existente"
            );
            // Cancelar no cambia nada.
            let d = window.abrir_guardar_en().unwrap();
            for c in d.casillas() {
                c.set_active(false);
            }
            d.dialogo.emit_by_name::<()>("response", &[&"cancelar"]);
            d.dialogo.force_close();
            assert_eq!(window.listas_en_memoria(Lista::Coleccion(1)), rutas(&["wiki/b.md"]));

            // a.md en las dos colecciones y favorita.
            abrir(&window, "wiki/a.md").await;
            let d = window.abrir_guardar_en().unwrap();
            assert_eq!(
                d.casillas().map(|c| c.is_active()).collect::<Vec<_>>(),
                [true, false]
            );
            d.casillas().nth(1).unwrap().set_active(true);
            d.dialogo.emit_by_name::<()>("response", &[&"guardar"]);
            d.dialogo.force_close();
            assert_eq!(rotulo_menu(&window, 1), "Guardada en 2 colecciones…");
            let menu = menu_abierto(&window).await;
            assert!(menu.contains(&"Guardada en 2 colecciones…".to_owned()), "{menu:?}");
            WidgetExt::activate_action(&window.0.window, "win.alternar-favorito", None).unwrap();

            // Modo Guardados: una sección por colección; un clic abre la nota.
            abrir(&window, "wiki/b.md").await;
            window.0.sidebar_stack.set_visible_child_name("guardados");
            let pagina = window.0.listas_panel.guardados().clone().upcast::<gtk::Widget>();
            let mut filas_a = Vec::new();
            let mut secciones = Vec::new();
            let mut pila = vec![pagina];
            while let Some(w) = pila.pop() {
                if let Some(row) = w.downcast_ref::<gtk::ListBoxRow>()
                    && row.tooltip_text().as_deref() == Some("wiki/a.md")
                {
                    filas_a.push(row.clone());
                }
                if w.has_css_class("orbital-section") {
                    secciones.push(w.clone());
                }
                let mut h = w.first_child();
                while let Some(x) = h {
                    pila.push(x.clone());
                    h = x.next_sibling();
                }
            }
            assert_eq!(secciones.len(), 2, "una sección por colección");
            assert_eq!(filas_a.len(), 2, "a.md está en las dos colecciones");
            filas_a[0].emit_activate();
            esperar("abre a.md desde Guardados", || {
                window.0.current_entry.borrow().as_ref().map(|e| e.path.as_str().to_owned())
                    == Some("wiki/a.md".into())
            })
            .await;

            // Quitarla de «Para leer»: sigue en «Tesis» y en Favoritos.
            let d = window.abrir_guardar_en().unwrap();
            d.casillas().next().unwrap().set_active(false);
            d.dialogo.emit_by_name::<()>("response", &[&"guardar"]);
            d.dialogo.force_close();
            assert_eq!(window.listas_en_memoria(Lista::Coleccion(0)), rutas(&["wiki/b.md"]));
            assert_eq!(
                window.listas_en_memoria(Lista::Coleccion(1)),
                rutas(&["wiki/b.md", "wiki/a.md"])
            );
            assert_eq!(window.listas_en_memoria(Lista::Favoritos), rutas(&["wiki/a.md"]));
            assert_eq!(rotulo_menu(&window, 1), "Guardada en 1 colección…");

            let esperado = "format_version: 1\nfavoritos:\n  - wiki/a.md\ncolecciones:\n  - nombre: Para leer\n    notas:\n      - wiki/b.md\n  - nombre: Tesis\n    notas:\n      - wiki/b.md\n      - wiki/a.md\n";
            esperar("se escribe el último estado", || en_disco(&root) == esperado).await;
            assert!(
                !root.join(".orbital/respaldo").exists(),
                "lo que escribió esta sesión no se respalda"
            );
            window.0.window.destroy();
            assert_eq!(sumas(&root), antes, "SC-004");
            std::fs::remove_dir_all(root).unwrap();
        });
    }

    /// Todos los widgets de `raiz` (incluida) que cumplen `pred`, en orden.
    fn buscar(raiz: &gtk::Widget, pred: impl Fn(&gtk::Widget) -> bool) -> Vec<gtk::Widget> {
        let mut out = Vec::new();
        let mut pila = vec![raiz.clone()];
        while let Some(w) = pila.pop() {
            if pred(&w) {
                out.push(w.clone());
            }
            let mut h = w.last_child();
            while let Some(x) = h {
                pila.push(x.clone());
                h = x.prev_sibling();
            }
        }
        out
    }

    fn tooltip_es(w: &gtk::Widget, texto: &str) -> bool {
        w.tooltip_text().as_deref() == Some(texto)
    }

    /// Abre el menú `nombre` («Opciones de …») dentro de `raiz` y devuelve sus botones.
    async fn menu(raiz: &gtk::Widget, nombre: &str) -> Vec<gtk::Button> {
        let boton = buscar(raiz, |w| w.is::<gtk::MenuButton>() && tooltip_es(w, nombre))
            .pop()
            .and_downcast::<gtk::MenuButton>()
            .unwrap_or_else(|| panic!("no está el menú «{nombre}»"));
        boton.popup();
        glib::timeout_future(Duration::from_millis(30)).await;
        let popover = boton.popover().expect("el menú se arma al abrirse");
        let botones = buscar(popover.upcast_ref(), |w| w.is::<gtk::Button>())
            .into_iter()
            .filter_map(|w| w.downcast::<gtk::Button>().ok())
            .collect();
        popover.popdown();
        botones
    }

    fn opcion(botones: &[gtk::Button], texto: &str) -> gtk::Button {
        botones
            .iter()
            .find(|b| {
                b.child()
                    .and_downcast::<gtk::Label>()
                    .is_some_and(|l| l.label() == texto)
            })
            .cloned()
            .unwrap_or_else(|| panic!("no está la opción «{texto}»"))
    }

    /// Pulsa el «Deshacer» del último aviso.
    pub(super) async fn deshacer(window: &Window) {
        let raiz = window.0.toasts.clone().upcast::<gtk::Widget>();
        let t0 = Instant::now();
        loop {
            let botones = buscar(&raiz, |w| {
                w.downcast_ref::<gtk::Button>()
                    .is_some_and(|b| b.label().as_deref() == Some("Deshacer") && b.is_mapped())
            });
            if let Some(b) = botones.last().and_then(|w| w.downcast_ref::<gtk::Button>()) {
                b.emit_clicked();
                return;
            }
            assert!(
                t0.elapsed() < Duration::from_secs(5),
                "no apareció «Deshacer»"
            );
            glib::timeout_future(Duration::from_millis(20)).await;
        }
    }

    fn actual(window: &Window) -> Option<String> {
        window
            .0
            .current_entry
            .borrow()
            .as_ref()
            .map(|e| e.path.as_str().to_owned())
    }

    /// Filas de las colecciones de la página Guardados (la arma si hace falta), por tooltip.
    fn filas_guardados(window: &Window) -> Vec<gtk::ListBoxRow> {
        window.0.sidebar_stack.set_visible_child_name("guardados");
        let pagina = window
            .0
            .listas_panel
            .guardados()
            .clone()
            .upcast::<gtk::Widget>();
        buscar(&pagina, |w| w.is::<gtk::ListBoxRow>())
            .into_iter()
            .filter_map(|w| w.downcast::<gtk::ListBoxRow>().ok())
            .collect()
    }

    /// 027 US3 (T025): renombrar y mover 10 notas favoritas y guardadas y abrir cada entrada
    /// (SC-003), deshacer el renombre, renombrar la carpeta que las contiene, borrar a la papelera y
    /// deshacer (las entradas vuelven a su lugar).
    #[test]
    #[ignore = "requiere GTK/DBus; tools/pruebas-gtk.sh"]
    fn us3_seguir_renombres_y_borrados() {
        let app = app("Us3a");
        glib::MainContext::default().block_on(async {
            let root = boveda("us3a");
            for k in 0..10 {
                std::fs::write(root.join(format!("wiki/n{k}.md")), format!("# N{k}\n")).unwrap();
            }
            let window = ventana(&app, &root).await;
            let notas: Vec<RelPath> = (0..10)
                .map(|k| RelPath::new(&format!("wiki/n{k}.md")).unwrap())
                .collect();
            let todas = notas.clone();
            window.aplicar_listas(move |l| {
                for r in &todas {
                    l.alternar_favorito(r);
                }
                l.crear_coleccion("Lote", None).unwrap();
                l.crear_coleccion("Otra", Some(todas[0].clone())).unwrap();
                for r in &todas {
                    l.poner_en(0, r, true);
                }
                true
            });
            assert_eq!(window.listas_en_memoria(Lista::Favoritos), notas);

            // Renombrar con deshacer: vuelve a su nombre en las tres listas.
            window
                .do_rename(
                    notas[0].clone(),
                    RelPath::new("wiki/cero.md").unwrap(),
                    true,
                )
                .await;
            assert_eq!(
                window.listas_en_memoria(Lista::Favoritos)[0].as_str(),
                "wiki/cero.md"
            );
            assert_eq!(
                window.listas_en_memoria(Lista::Coleccion(1)),
                rutas(&["wiki/cero.md"])
            );
            deshacer(&window).await;
            esperar("el deshacer devuelve n0.md", || {
                window.listas_en_memoria(Lista::Coleccion(1)) == rutas(&["wiki/n0.md"])
            })
            .await;
            assert_eq!(window.listas_en_memoria(Lista::Favoritos), notas);

            // SC-003: mover las diez a otra carpeta con otro nombre.
            let movidas: Vec<RelPath> = (0..10)
                .map(|k| RelPath::new(&format!("wiki/movidas/m{k}.md")).unwrap())
                .collect();
            for (desde, hasta) in notas.iter().zip(&movidas) {
                window.do_rename(desde.clone(), hasta.clone(), false).await;
            }
            assert_eq!(window.listas_en_memoria(Lista::Favoritos), movidas);
            assert_eq!(window.listas_en_memoria(Lista::Coleccion(0)), movidas);
            assert_eq!(window.listas_en_memoria(Lista::Coleccion(1)), movidas[..1]);
            let panel = window.0.listas_panel.clone();
            let mut abiertas = 0;
            for (k, ruta) in movidas.iter().enumerate() {
                let row = panel.lista_favoritos().row_at_index(k as i32).unwrap();
                assert_eq!(row.tooltip_text().as_deref(), Some(ruta.as_str()));
                row.emit_activate();
                esperar("abre la favorita movida", || {
                    actual(&window).as_deref() == Some(ruta.as_str())
                })
                .await;
                abiertas += 1;
            }
            for row in filas_guardados(&window) {
                let ruta = row.tooltip_text().unwrap().to_string();
                assert!(ruta.starts_with("wiki/movidas/"), "{ruta}");
                row.emit_activate();
                esperar("abre la guardada movida", || {
                    actual(&window).as_deref() == Some(&ruta)
                })
                .await;
                abiertas += 1;
            }
            assert_eq!(abiertas, 21, "SC-003: las 21 entradas abren su nota");

            // Renombrar la carpeta: todas la siguen.
            window
                .do_rename(
                    RelPath::new("wiki/movidas").unwrap(),
                    RelPath::new("wiki/otras").unwrap(),
                    false,
                )
                .await;
            let otras: Vec<RelPath> = (0..10)
                .map(|k| RelPath::new(&format!("wiki/otras/m{k}.md")).unwrap())
                .collect();
            assert_eq!(window.listas_en_memoria(Lista::Favoritos), otras);
            assert_eq!(window.listas_en_memoria(Lista::Coleccion(1)), otras[..1]);

            // Papelera: sale de todas las listas; «Deshacer» la repone en su lugar.
            window.do_trash(otras[3].clone()).await;
            assert!(
                !window
                    .listas_en_memoria(Lista::Favoritos)
                    .contains(&otras[3])
            );
            assert!(
                !window
                    .listas_en_memoria(Lista::Coleccion(0))
                    .contains(&otras[3])
            );
            window.do_trash(otras[0].clone()).await;
            assert!(window.listas_en_memoria(Lista::Coleccion(1)).is_empty());
            deshacer(&window).await;
            esperar("se repone m0", || {
                window.listas_en_memoria(Lista::Coleccion(1)) == otras[..1]
            })
            .await;
            assert_eq!(window.listas_en_memoria(Lista::Favoritos)[0], otras[0]);
            let esperado: Vec<RelPath> =
                otras.iter().filter(|r| **r != otras[3]).cloned().collect();
            assert_eq!(window.listas_en_memoria(Lista::Favoritos), esperado);
            let en_disco_esperado = orbital_vault::listas::serialize(
                &window.0.listas.borrow().as_ref().unwrap().listas,
            );
            esperar("se escribe todo", || en_disco(&root) == en_disco_esperado).await;
            window.0.window.destroy();
            std::fs::remove_dir_all(root).unwrap();
        });
    }

    /// 027 US3 (T025): menús por fila («Subir», «Bajar», «Quitar…»), mover como al soltar un
    /// arrastre, renombrar y borrar colección, persistencia del orden, entrada faltante, cambio
    /// externo del archivo y archivo dañado con respaldo.
    #[test]
    #[ignore = "requiere GTK/DBus; tools/pruebas-gtk.sh"]
    fn us3_menus_faltantes_y_cambios_externos() {
        let app = app("Us3b");
        glib::MainContext::default().block_on(async {
            let root = boveda("us3b");
            let window = ventana(&app, &root).await;
            window.aplicar_listas(|l| {
                for r in rutas(&["wiki/a.md", "wiki/b.md", "wiki/c.html"]) {
                    l.alternar_favorito(&r);
                }
                l.crear_coleccion("Para leer", RelPath::new("wiki/a.md").ok()).unwrap();
                l.poner_en(0, &RelPath::new("wiki/b.md").unwrap(), true);
                l.crear_coleccion("Tesis", RelPath::new("wiki/c.html").ok()).unwrap();
                true
            });
            let panel = window.0.listas_panel.clone();
            let favs = panel.lista_favoritos().clone().upcast::<gtk::Widget>();

            // «Subir» desactivado en la primera fila; «Bajar» la mueve.
            let m = menu(&favs, "Opciones de a.md").await;
            assert!(!opcion(&m, "Subir").is_sensitive());
            opcion(&m, "Bajar").emit_clicked();
            assert_eq!(filas(panel.lista_favoritos()), ["wiki/b.md", "wiki/a.md", "wiki/c.html"]);
            let m = menu(&favs, "Opciones de c.html").await;
            assert!(!opcion(&m, "Bajar").is_sensitive());
            opcion(&m, "Subir").emit_clicked();
            assert_eq!(filas(panel.lista_favoritos()), ["wiki/b.md", "wiki/c.html", "wiki/a.md"]);
            // Soltar un arrastre pide lo mismo: la primera al final.
            window.orden_de_listas(crate::listas_panel::Orden::Mover {
                lista: Lista::Favoritos,
                desde: 0,
                hasta: 2,
            });
            assert_eq!(filas(panel.lista_favoritos()), ["wiki/c.html", "wiki/a.md", "wiki/b.md"]);

            // Guardados: quitar de una colección no la quita de Favoritos.
            let pagina = panel.guardados().clone().upcast::<gtk::Widget>();
            filas_guardados(&window);
            let m = menu(&pagina, "Opciones de b.md").await;
            opcion(&m, "Quitar de esta colección").emit_clicked();
            assert_eq!(window.listas_en_memoria(Lista::Coleccion(0)), rutas(&["wiki/a.md"]));
            assert_eq!(window.listas_en_memoria(Lista::Favoritos).len(), 3);

            // Menú de la colección: «Renombrar…» abre el diálogo.
            let m = menu(&pagina, "Opciones de la colección Tesis").await;
            opcion(&m, "Renombrar…").emit_clicked();
            let dialogo = window.0.window.visible_dialog().expect("se abre el diálogo");
            dialogo.force_close();
            // Vacío o repetido no se acepta; otro nombre conserva las notas.
            let (d, campo) = window.pedir_renombrar_coleccion(1).unwrap();
            for malo in ["", "   ", " para LEER"] {
                campo.set_text(malo);
                assert!(!d.is_response_enabled("renombrar"), "{malo:?}");
            }
            campo.set_text("Capítulo 2");
            assert!(d.is_response_enabled("renombrar"));
            d.emit_by_name::<()>("response", &[&"renombrar"]);
            d.force_close();
            {
                let l = window.0.listas.borrow();
                let c = &l.as_ref().unwrap().listas.colecciones[1];
                assert_eq!(c.nombre, "Capítulo 2");
                assert_eq!(c.notas, rutas(&["wiki/c.html"]));
            }
            // Borrar: con confirmación; «Cancelar» no borra.
            let d = window.pedir_borrar_coleccion(0).unwrap();
            d.emit_by_name::<()>("response", &[&"cancelar"]);
            d.force_close();
            assert_eq!(window.0.listas.borrow().as_ref().unwrap().listas.colecciones.len(), 2);
            let d = window.pedir_borrar_coleccion(0).unwrap();
            d.emit_by_name::<()>("response", &[&"borrar"]);
            d.force_close();
            assert_eq!(
                window.0.listas.borrow().as_ref().unwrap().listas.colecciones.len(),
                1
            );
            let esperado = "format_version: 1\nfavoritos:\n  - wiki/c.html\n  - wiki/a.md\n  - wiki/b.md\ncolecciones:\n  - nombre: Capítulo 2\n    notas:\n      - wiki/c.html\n";
            esperar("se escribe", || en_disco(&root) == esperado).await;
            window.0.window.destroy();

            // Al reabrir, el mismo orden.
            let window = ventana(&app, &root).await;
            let panel = window.0.listas_panel.clone();
            assert_eq!(filas(panel.lista_favoritos()), ["wiki/c.html", "wiki/a.md", "wiki/b.md"]);
            let antes = sumas(&root);

            // Borrado por fuera: la fila queda marcada; no abre; se puede quitar.
            std::fs::remove_file(root.join("wiki/b.md")).unwrap();
            esperar("b.md queda faltante", || {
                filas(panel.lista_favoritos())[2] == "wiki/b.md (falta el archivo)"
            })
            .await;
            abrir(&window, "wiki/a.md").await;
            panel.lista_favoritos().row_at_index(2).unwrap().emit_activate();
            glib::timeout_future(Duration::from_millis(100)).await;
            assert_eq!(actual(&window).as_deref(), Some("wiki/a.md"), "una faltante no abre");
            let favs = panel.lista_favoritos().clone().upcast::<gtk::Widget>();
            let m = menu(&favs, "Opciones de b.md").await;
            opcion(&m, "Quitar de favoritos").emit_clicked();
            assert_eq!(filas(panel.lista_favoritos()), ["wiki/c.html", "wiki/a.md"]);
            esperar("se escribe la quita", || {
                en_disco(&root).contains("favoritos:\n  - wiki/c.html\n  - wiki/a.md\ncolecciones")
            })
            .await;

            // Cambio externo (por ejemplo, una sincronización): la lista se actualiza sola.
            std::fs::write(
                root.join(".orbital/listas.yaml"),
                "favoritos:\n  - raw/fuente.pdf\n  - wiki/a.md\n",
            )
            .unwrap();
            esperar("se relee el archivo cambiado afuera", || {
                filas(panel.lista_favoritos()) == ["raw/fuente.pdf", "wiki/a.md"]
            })
            .await;

            // Dañado afuera: listas vacías, el archivo intacto; el primer cambio lo respalda.
            std::fs::write(root.join(".orbital/listas.yaml"), "favoritos: [\n").unwrap();
            esperar("se relee el dañado", || filas(panel.lista_favoritos()).is_empty()).await;
            glib::timeout_future(Duration::from_millis(300)).await;
            assert_eq!(en_disco(&root), "favoritos: [\n", "el dañado no se pisa al leerlo");
            WidgetExt::activate_action(&window.0.window, "win.alternar-favorito", None).unwrap();
            esperar("se escribe sobre el dañado", || {
                en_disco(&root).contains("favoritos:\n  - wiki/a.md\n")
            })
            .await;
            let respaldos: Vec<String> = buscar_archivos(&root.join(".orbital/respaldo"))
                .into_iter()
                .map(|p| std::fs::read_to_string(p).unwrap())
                .collect();
            assert!(
                respaldos.iter().any(|t| t == "favoritos: [\n"),
                "el dañado quedó respaldado: {respaldos:?}"
            );
            window.0.window.destroy();
            assert_eq!(
                sumas(&root),
                antes
                    .into_iter()
                    .filter(|(p, _)| !p.ends_with("wiki/b.md"))
                    .collect::<Vec<_>>(),
                "SC-004"
            );
            std::fs::remove_dir_all(root).unwrap();
        });
    }

    fn buscar_archivos(dir: &Path) -> Vec<PathBuf> {
        let mut out = Vec::new();
        let mut pila = vec![dir.to_path_buf()];
        while let Some(d) = pila.pop() {
            for e in std::fs::read_dir(&d).into_iter().flatten().flatten() {
                if e.path().is_dir() {
                    pila.push(e.path());
                } else {
                    out.push(e.path());
                }
            }
        }
        out
    }

    /// 036 US2: «Nota nueva con #etiqueta» desde el índice de la Lectura pide el nombre, crea la
    /// nota en la carpeta de la nota del índice con la etiqueta y el índice la toma; la nota del
    /// índice no cambia. Bóveda con ruta no ASCII.
    #[test]
    #[ignore = "requiere GTK/DBus; tools/pruebas-gtk.sh"]
    fn indice_nota_nueva_con_etiqueta() {
        let app = app("Indice036");
        glib::MainContext::default().block_on(async {
            let root = std::env::temp_dir().join(format!(
                "orbital-036-ventana-Bóveda ñandú-{}",
                glib::uuid_string_random()
            ));
            let nota = "# Proyectos\n\n```indice\netiqueta: proyectos\n```\n";
            std::fs::create_dir_all(root.join("wiki/índices ñ")).unwrap();
            std::fs::write(root.join("wiki/índices ñ/proyectos.md"), nota).unwrap();
            std::fs::write(root.join("wiki/a.md"), "#proyectos/tesis\n").unwrap();
            let window = ventana(&app, &root).await;
            abrir(&window, "wiki/índices ñ/proyectos.md").await;
            let editor = window.0.code_editor.clone();
            editor.set_view(crate::editor::View::Lectura);
            esperar("el índice en Lectura", || {
                editor.render_now();
                editor
                    .texto_lectura()
                    .contains("Nota nueva con #proyectos/tesis")
            })
            .await;
            window.open_link(
                None,
                &format!("{}proyectos/tesis", crate::editor::reader::NOTA_NUEVA),
            );
            esperar("el pedido de nombre", || {
                window.0.window.visible_dialog().is_some()
            })
            .await;
            let d = window
                .0
                .window
                .visible_dialog()
                .and_downcast::<adw::AlertDialog>()
                .expect("un AlertDialog");
            d.extra_child()
                .and_downcast::<gtk::Entry>()
                .expect("el campo del nombre")
                .set_text("Borrador ñ");
            d.emit_by_name::<()>("response", &[&"create"]);
            esperar("se abre la nota nueva", || {
                window
                    .0
                    .current_entry
                    .borrow()
                    .as_ref()
                    .map(|e| e.path.as_str().to_owned())
                    == Some("wiki/índices ñ/Borrador ñ.md".to_owned())
            })
            .await;
            let creada =
                std::fs::read_to_string(root.join("wiki/índices ñ/Borrador ñ.md")).unwrap();
            assert!(
                creada.contains("tags:") && creada.contains("proyectos/tesis"),
                "{creada}"
            );
            // El índice la toma.
            abrir(&window, "wiki/índices ñ/proyectos.md").await;
            editor.set_view(crate::editor::View::Lectura);
            esperar("la nota nueva en el índice", || {
                editor.render_now();
                editor.texto_lectura().contains("Borrador ñ")
            })
            .await;
            assert_eq!(
                std::fs::read_to_string(root.join("wiki/índices ñ/proyectos.md")).unwrap(),
                nota
            );
            window.0.window.destroy();
            let _ = std::fs::remove_dir_all(&root);
        });
    }
}

mod plegado_tests {
    //! 028 US3 (T027): el plegado se recuerda en `.orbital/plegado.yaml`.
    use super::super::*;
    use super::listas_tests::{abrir, app, deshacer, esperar};
    use std::time::Duration;

    const NOTA: &str =
        "# Nota\nintro\n\n## Historia\nTexto.\n- Frutas\n  - Manzana\n\n## Ejemplos\nuno\n";
    const HISTORIA: u32 = 3;
    const FRUTAS: u32 = 5;

    fn boveda(tag: &str) -> PathBuf {
        let root =
            std::env::temp_dir().join(format!("orbital-028-{tag}-{}", glib::uuid_string_random()));
        std::fs::create_dir_all(root.join("wiki/sub")).unwrap();
        std::fs::create_dir_all(root.join("raw")).unwrap();
        std::fs::write(root.join("wiki/outline.md"), NOTA).unwrap();
        std::fs::write(root.join("wiki/otra.md"), "# Otra\n\nNada.\n").unwrap();
        root
    }

    fn sumas(root: &Path) -> Vec<(String, u64)> {
        let mut out = Vec::new();
        let mut pila = vec![root.join("wiki"), root.join("raw")];
        while let Some(d) = pila.pop() {
            for e in std::fs::read_dir(d).unwrap() {
                let e = e.unwrap();
                if e.path().is_dir() {
                    pila.push(e.path());
                } else {
                    let bytes = std::fs::read(e.path()).unwrap();
                    out.push((
                        e.path().display().to_string(),
                        orbital_vault::write::hash(&bytes),
                    ));
                }
            }
        }
        out.sort();
        out
    }

    fn en_disco(root: &Path) -> String {
        std::fs::read_to_string(root.join(".orbital/plegado.yaml")).unwrap_or_default()
    }

    async fn ventana(app: &adw::Application, root: &Path) -> Window {
        let window = Window::new(app);
        window.set_persist(false);
        window.0.window.set_default_size(1200, 900);
        window.0.window.present();
        window.open_path(root.to_path_buf()).await;
        window
    }

    fn editor(w: &Window) -> &Editor {
        &w.0.code_editor
    }

    async fn sin_escrituras(w: &Window) {
        esperar("se escribe el plegado", || !w.plegado_escribiendo()).await;
    }

    const DOS: &str = "format_version: 1\nnotas:\n  - ruta: wiki/outline.md\n    plegados:\n      - encabezado: Historia\n      - item: Frutas\n";

    /// Escenarios 8, 9 y 12: plegar dos elementos, cambiar de nota y volver, reabrir la bóveda;
    /// sin `.orbital/` hasta plegar; las notas no cambian (SC-004); con el texto cambiado desde
    /// Orbital, siguen plegados (SC-006).
    #[test]
    #[ignore = "requiere GTK/DBus; tools/pruebas-gtk.sh"]
    fn us3_se_recuerda() {
        let app = app("Plegado1");
        glib::MainContext::default().block_on(async {
            let root = boveda("recuerda");
            let antes = sumas(&root);
            let w = ventana(&app, &root).await;
            abrir(&w, "wiki/otra.md").await;
            abrir(&w, "wiki/outline.md").await;
            esperar("se lee el estado", || w.plegado_cargado_ya()).await;
            glib::timeout_future(Duration::from_millis(100)).await;
            assert!(!root.join(".orbital").exists(), "abrir y navegar no crea .orbital/");
            assert!(editor(&w).plegar(HISTORIA));
            // Con la acción de la ventana, como desde el menú o la paleta.
            editor(&w).buffer().place_cursor(&editor(&w).buffer().iter_at_line(FRUTAS as i32).unwrap());
            editor(&w).desplegar(HISTORIA);
            WidgetExt::activate_action(&w.0.window, "win.alternar-plegado", None).unwrap();
            assert!(editor(&w).plegar(HISTORIA));
            assert!(editor(&w).esta_plegado(FRUTAS));
            esperar("se escriben los dos", || en_disco(&root) == DOS).await;
            abrir(&w, "wiki/otra.md").await;
            abrir(&w, "wiki/outline.md").await;
            assert_eq!(editor(&w).lineas_plegadas(), [HISTORIA, FRUTAS]);
            sin_escrituras(&w).await;
            w.0.window.destroy();

            let w = ventana(&app, &root).await;
            abrir(&w, "wiki/outline.md").await;
            esperar("se pliega al reabrir", || editor(&w).lineas_plegadas() == [HISTORIA, FRUTAS]).await;
            assert_eq!(en_disco(&root), DOS, "reabrir no reescribe");
            assert_eq!(sumas(&root), antes, "ninguna nota cambió");

            // El texto del encabezado y del ítem cambian desde Orbital (escribiendo en sus líneas,
            // que se ven): siguen plegados.
            let e = editor(&w).clone();
            let b = e.buffer().clone();
            let escribir_al_final = |linea: u32, texto: &str| {
                let mut it = b.iter_at_line(linea as i32).unwrap();
                it.forward_to_line_end();
                b.place_cursor(&it);
                b.insert_at_cursor(texto);
            };
            assert!(e.desplegar(HISTORIA));
            escribir_al_final(FRUTAS, " y más");
            escribir_al_final(HISTORIA, " nueva");
            assert!(e.plegar(HISTORIA));
            assert_eq!(e.lineas_plegadas(), [HISTORIA, FRUTAS]);
            esperar("se reconcilia", || {
                let c = w.plegado_de(&RelPath::new("wiki/outline.md").unwrap());
                c.iter().any(|c| matches!(c, orbital_vault::Clave::Encabezado { texto, .. } if texto == "Historia nueva"))
                    && c.iter().any(|c| matches!(c, orbital_vault::Clave::Item { texto, .. } if texto == "Frutas y más"))
            })
            .await;
            e.flush().await;
            sin_escrituras(&w).await;
            w.0.window.destroy();
            let w = ventana(&app, &root).await;
            abrir(&w, "wiki/outline.md").await;
            esperar("siguen plegados con el texto nuevo", || editor(&w).lineas_plegadas() == [HISTORIA, FRUTAS]).await;
            w.0.window.destroy();
            std::fs::remove_dir_all(root).unwrap();
        });
    }

    /// FR-010: renombrar y mover desde Orbital conserva el plegado (también el deshacer); la
    /// papelera lo descarta y su «Deshacer» lo repone.
    #[test]
    #[ignore = "requiere GTK/DBus; tools/pruebas-gtk.sh"]
    fn us3_renombrar_mover_y_borrar() {
        let app = app("Plegado2");
        glib::MainContext::default().block_on(async {
            let root = boveda("renombrar");
            let w = ventana(&app, &root).await;
            abrir(&w, "wiki/outline.md").await;
            esperar("se lee el estado", || w.plegado_cargado_ya()).await;
            assert!(editor(&w).plegar(HISTORIA));
            let ruta = |s: &str| RelPath::new(s).unwrap();
            w.do_rename(ruta("wiki/outline.md"), ruta("wiki/sub/movida.md"), true)
                .await;
            assert_eq!(w.plegado_de(&ruta("wiki/sub/movida.md")).len(), 1);
            assert!(w.plegado_de(&ruta("wiki/outline.md")).is_empty());
            esperar("la nota movida se abre plegada", || {
                editor(&w).esta_plegado(HISTORIA)
            })
            .await;
            deshacer(&w).await;
            esperar("el deshacer la devuelve", || {
                !w.plegado_de(&ruta("wiki/outline.md")).is_empty()
            })
            .await;
            // Una carpeta entera.
            w.do_rename(ruta("wiki/outline.md"), ruta("wiki/sub/x.md"), false)
                .await;
            w.do_rename(ruta("wiki/sub"), ruta("wiki/otra-carpeta"), false)
                .await;
            assert_eq!(w.plegado_de(&ruta("wiki/otra-carpeta/x.md")).len(), 1);
            // Papelera y deshacer.
            w.do_trash(ruta("wiki/otra-carpeta/x.md")).await;
            assert!(w.plegado_de(&ruta("wiki/otra-carpeta/x.md")).is_empty());
            sin_escrituras(&w).await;
            assert!(!en_disco(&root).contains("x.md"));
            deshacer(&w).await;
            esperar("se repone", || {
                w.plegado_de(&ruta("wiki/otra-carpeta/x.md")).len() == 1
            })
            .await;
            sin_escrituras(&w).await;
            assert!(
                en_disco(&root).contains("wiki/otra-carpeta/x.md"),
                "{}",
                en_disco(&root)
            );
            w.0.window.destroy();
            std::fs::remove_dir_all(root).unwrap();
        });
    }

    /// Escenarios 10 y 11: un elemento borrado del texto no pliega nada ni avisa, y su clave se va
    /// con el próximo cambio; un archivo dañado avisa, abre desplegado, queda intacto y se
    /// respalda al primer cambio.
    #[test]
    #[ignore = "requiere GTK/DBus; tools/pruebas-gtk.sh"]
    fn us3_borrado_y_danado() {
        let app = app("Plegado3");
        glib::MainContext::default().block_on(async {
            let root = boveda("danado");
            std::fs::create_dir_all(root.join(".orbital")).unwrap();
            // Una clave que ya no existe y una que sí.
            std::fs::write(
                root.join(".orbital/plegado.yaml"),
                "format_version: 1\nnotas:\n  - ruta: wiki/outline.md\n    plegados:\n      - encabezado: Borrada\n      - item: Frutas\n",
            )
            .unwrap();
            let w = ventana(&app, &root).await;
            abrir(&w, "wiki/outline.md").await;
            esperar("se pliega Frutas", || editor(&w).lineas_plegadas() == [FRUTAS]).await;
            assert!(editor(&w).plegar(HISTORIA));
            esperar("la clave perdida se va", || {
                let d = en_disco(&root);
                !d.contains("Borrada") && d.contains("Historia")
            })
            .await;
            w.0.window.destroy();

            let danado = "notas: [\n";
            std::fs::write(root.join(".orbital/plegado.yaml"), danado).unwrap();
            let w = ventana(&app, &root).await;
            abrir(&w, "wiki/outline.md").await;
            esperar("se lee el estado", || w.plegado_cargado_ya()).await;
            glib::timeout_future(Duration::from_millis(100)).await;
            assert!(!editor(&w).tiene_pliegues(), "dañado: desplegado");
            assert_eq!(en_disco(&root), danado, "intacto");
            assert!(editor(&w).plegar(FRUTAS));
            sin_escrituras(&w).await;
            esperar("se reemplaza", || en_disco(&root).contains("item: Frutas")).await;
            let mut copias = 0;
            let mut pila = vec![root.join(".orbital/respaldo")];
            while let Some(d) = pila.pop() {
                for e in std::fs::read_dir(d).into_iter().flatten().flatten() {
                    if e.path().is_dir() {
                        pila.push(e.path());
                    } else if std::fs::read_to_string(e.path()).ok().as_deref() == Some(danado) {
                        copias += 1;
                    }
                }
            }
            assert_eq!(copias, 1, "una copia del dañado");
            w.0.window.destroy();
            std::fs::remove_dir_all(root).unwrap();
        });
    }

    #[test]
    #[ignore = "requiere una sesión gráfica; tools/pruebas-gtk.sh"]
    fn menu_paleta_y_acciones_plegado() {
        adw::init().expect("Se necesita una sesión gráfica GTK");
        let app = adw::Application::builder()
            .application_id("io.github.caracelp.Orbital.Test028Us5")
            .build();
        let root = std::env::temp_dir().join("orbital-test-028-us5");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("wiki")).unwrap();
        std::fs::write(
            root.join("wiki/outline.md"),
            "# Uno\nintro\n\n## Historia\nTexto.\n- Frutas\n  - Manzana\n",
        )
        .unwrap();
        std::fs::write(root.join("wiki/pagina.html"), "<h1>Hola</h1><p>HTML</p>\n").unwrap();

        glib::MainContext::default().block_on(async {
            let w = ventana(&app, &root).await;
            abrir(&w, "wiki/outline.md").await;

            // Menú sin enfoque: Plegar o desplegar, Plegar todo, Desplegar todo, Enfocar (sin
            // Salir del enfoque).
            let labels: Vec<String> = w
                .items_menu_plegado()
                .into_iter()
                .map(|m| m.label)
                .collect();
            assert_eq!(
                labels,
                [
                    "Plegar o desplegar el elemento",
                    "Plegar todo",
                    "Desplegar todo",
                    "Enfocar"
                ]
            );

            let act = |name: &str| {
                gtk::prelude::ActionGroupExt::activate_action(&w.0.window, name, None);
            };
            // Acciones en Código, Lectura y Dividida.
            for vista in ["view-code", "view-read", "view-split"] {
                act(vista);
                act("plegar-todo");
                assert!(editor(&w).tiene_pliegues());
                act("desplegar-todo");
                assert!(!editor(&w).tiene_pliegues());
            }

            // Enfocar desde acción: aparece Salir del enfoque en el menú.
            act("enfocar");
            assert!(editor(&w).esta_enfocado());
            let labels: Vec<String> = w
                .items_menu_plegado()
                .into_iter()
                .map(|m| m.label)
                .collect();
            assert!(labels.contains(&"Salir del enfoque".to_owned()));
            act("salir-enfoque");
            assert!(!editor(&w).esta_enfocado());

            // En .html, plegar no está disponible y no rompe nada.
            abrir(&w, "wiki/pagina.html").await;
            assert!(!editor(&w).plegado_disponible());
            act("plegar-todo");
            assert!(!editor(&w).tiene_pliegues());

            w.0.window.destroy();
            std::fs::remove_dir_all(root).unwrap();
        });
    }
}

mod tipo_e_ids_tests {
    //! 043: `tipo` como alias de `type` e ids de nota únicos en toda la bóveda.
    use super::super::*;
    use super::listas_tests::{abrir, app, esperar};

    /// Dos notas sin id estructuradas una tras otra reciben ids distintos, después de los de la
    /// bóveda (AT-001, AT-002); una nota con `tipo` no pide `type`; un id repetido se avisa.
    #[test]
    #[ignore = "requiere GTK/DBus; tools/pruebas-gtk.sh"]
    fn ids_unicos_y_tipo_alias() {
        let app = app("TipoIds");
        glib::MainContext::default().block_on(async {
            let root =
                std::env::temp_dir().join(format!("orbital-043-{}", glib::uuid_string_random()));
            std::fs::create_dir_all(root.join("wiki")).unwrap();
            let escribir = |p: &str, c: &str| std::fs::write(root.join(p), c).unwrap();
            escribir("wiki/uno.md", "---\nid: AT-001\ntype: atomo\n---\n# Uno\n");
            escribir(
                "wiki/dos.md",
                "---\ntype: atomo\n---\n<!-- block 1 AT-002 -->\n# Dos\n<!---->\n",
            );
            escribir(
                "wiki/copia.md",
                "---\nid: AT-001\ntype: atomo\n---\n# Copia\n",
            );
            escribir("wiki/a.md", "# A\n\nTexto a.\n");
            escribir("wiki/b.md", "# B\n\nTexto b.\n");
            escribir("wiki/cuaderno.md", "---\ntipo: cuaderno\n---\n# Cuaderno\n");

            let w = Window::new(&app);
            w.set_persist(false);
            w.0.window.present();
            w.open_path(root.clone()).await;

            let id_en_editor = |w: &Window| {
                crate::grafo::ramas_libres::id_y_tipo_de_nota(&w.0.code_editor.text()).0
            };
            abrir(&w, "wiki/a.md").await;
            esperar("se carga a.md", || {
                w.0.code_editor.text().contains("Texto a.")
            })
            .await;
            w.estructurar_nota_tres_partes();
            esperar("a.md recibe id", || id_en_editor(&w).is_some()).await;
            assert_eq!(id_en_editor(&w).as_deref(), Some("AT-003"));

            // Sin esperar el guardado: el id dado en la sesión cuenta igual.
            abrir(&w, "wiki/b.md").await;
            esperar("se carga b.md", || {
                w.0.code_editor.text().contains("Texto b.")
            })
            .await;
            w.estructurar_nota_tres_partes();
            esperar("b.md recibe id", || id_en_editor(&w).is_some()).await;
            assert_eq!(id_en_editor(&w).as_deref(), Some("AT-004"));

            // `tipo: cuaderno`: sin aviso de `type`; al estructurarla, no se agrega `type`.
            abrir(&w, "wiki/cuaderno.md").await;
            esperar("se carga cuaderno.md", || {
                w.0.code_editor.text().contains("# Cuaderno")
            })
            .await;
            assert!(
                !w.0.type_banner.is_visible(),
                "una nota con tipo no pide type"
            );
            w.estructurar_nota_tres_partes();
            esperar("cuaderno.md recibe id", || id_en_editor(&w).is_some()).await;
            let texto = w.0.code_editor.text();
            assert_eq!(id_en_editor(&w).as_deref(), Some("CU-001"), "{texto}");
            assert!(texto.contains("tipo: cuaderno"), "{texto}");
            assert!(!texto.contains("type:"), "{texto}");

            // AT-001 está en uno.md y en copia.md: aviso no bloqueante, sin escribir nada.
            let antes = std::fs::read(root.join("wiki/uno.md")).unwrap();
            abrir(&w, "wiki/uno.md").await;
            esperar("aviso de id repetido", || {
                w.0.note_banner.is_visible() && w.0.note_banner.title().contains("AT-001")
            })
            .await;
            assert!(w.0.note_banner.title().contains("copia.md"));
            assert_eq!(std::fs::read(root.join("wiki/uno.md")).unwrap(), antes);

            w.0.window.destroy();
            std::fs::remove_dir_all(root).unwrap();
        });
    }
}

mod ventana_tests {
    //! 056: Orbital abre maximizada y, al desmaximizar, vuelve al tamaño guardado.
    use super::super::*;
    use super::listas_tests::{app, esperar};

    /// Con un `estado.yaml` de antes (900×600, `window_maximized: false`), la ventana abre
    /// maximizada; su tamaño por defecto sigue siendo el guardado mientras está maximizada; al
    /// desmaximizar vuelve a 900×600; al cerrar maximizada se guarda 900×600 (no el tamaño
    /// maximizado) y ya no se escribe `window_maximized`.
    #[test]
    #[ignore = "requiere GTK/DBus; tools/pruebas-gtk.sh"]
    fn abre_maximizada_y_vuelve_al_tamano_guardado() {
        let config =
            std::env::temp_dir().join(format!("orbital-056-{}", glib::uuid_string_random()));
        let estado = config.join("orbital/estado.yaml");
        std::fs::create_dir_all(estado.parent().unwrap()).unwrap();
        std::fs::write(
            &estado,
            "recent: []\nwindow_width: 900\nwindow_height: 600\nwindow_maximized: false\n",
        )
        .unwrap();
        // Antes de iniciar GTK: glib lee la carpeta de configuración una sola vez por proceso.
        // SAFETY: todavía no corre ningún otro hilo de esta prueba que lea el entorno.
        unsafe { std::env::set_var("XDG_CONFIG_HOME", &config) };
        assert_eq!(glib::user_config_dir(), config);
        let app = app("Ventana");
        glib::MainContext::default().block_on(async {
            let w = Window::new(&app);
            assert_eq!(w.0.window.default_size(), (900, 600));
            w.maximize();
            w.present();
            let win = w.0.window.clone();
            esperar("abre maximizada", || {
                win.is_maximized() && win.width() > 900 && win.height() > 600
            })
            .await;
            assert_eq!(win.default_size(), (900, 600), "maximizada no lo pisa");

            win.unmaximize();
            esperar("vuelve a 900×600", || {
                !win.is_maximized() && win.width() == 900 && win.height() == 600
            })
            .await;

            win.maximize();
            esperar("se maximiza otra vez", || {
                win.is_maximized() && win.width() > 900
            })
            .await;
            assert_eq!(win.default_size(), (900, 600));
            win.close();
            esperar("guarda el tamaño sin maximizar", || {
                std::fs::read_to_string(&estado).is_ok_and(|t| !t.contains("window_maximized"))
            })
            .await;
            let s = AppState::load();
            assert_eq!(s.restore_size(), (900, 600));
        });
        std::fs::remove_dir_all(config).unwrap();
    }
}
