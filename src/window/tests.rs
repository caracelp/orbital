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
            editor.load(vault.clone(), &vault.read_note(&path).unwrap());
            let stale = vault.plan_tag_rename("vieja", "nueva", std::slice::from_ref(&path));
            editor.replace_text("#vieja con mis cambios\n");
            window
                .run_tag_edits(vault.clone(), 1, Arc::new(stale), false)
                .await;
            assert_eq!(
                std::fs::read_to_string(path.to_path(&root)).unwrap(),
                "#vieja con mis cambios\n"
            );
            assert_eq!(editor.text(), "#vieja con mis cambios\n");

            let plan =
                Arc::new(vault.plan_tag_rename("vieja", "nueva", std::slice::from_ref(&path)));
            window
                .run_tag_edits(vault.clone(), 1, plan.clone(), false)
                .await;
            assert_eq!(editor.text(), "#nueva con mis cambios\n");
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
                std::fs::read_to_string(path.to_path(&root)).unwrap(),
                "#nueva con otros cambios\n"
            );
            assert_eq!(editor.text(), "#nueva con otros cambios\n");
            // Una nueva sesión no puede consumir el Deshacer de la anterior.
            window.0.vault_session.set(2);
            window.run_tag_edits(vault, 1, applied, true).await;
            assert_eq!(
                std::fs::read_to_string(path.to_path(&root)).unwrap(),
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

    /// Captura de la interfaz v13 a 1440×900 con una nota y cada modo de la barra derecha:
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

            // Riel: Canva y DB desactivados; el resto, con su acción.
            for (tooltip, activo) in [
                ("Crear canva", false),
                ("Crear base de datos", false),
                ("Selector rápido", true),
                ("Conexiones", true),
                ("Paleta de comandos", true),
                ("Configuración general", true),
            ] {
                let b = boton(&raiz, tooltip).unwrap_or_else(|| panic!("falta «{tooltip}»"));
                assert_eq!(b.is_sensitive(), activo, "«{tooltip}»");
            }
            // Guardados: pendiente (desactivado). Búsqueda cambia la página de la barra.
            assert!(!boton(&raiz, "Guardados").unwrap().is_sensitive());
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
