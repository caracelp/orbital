//! Pruebas GTK en proceso de la 061a: la ventana con una bóveda conectada al **servidor falso** de
//! Drive (nunca una cuenta real ni el estado de la usuaria).

use super::*;
use orbital_drive::cliente::{Cliente, Credenciales, Direcciones};
use orbital_drive::falso::{CUENTA, ServidorFalso};
use orbital_drive::motor::{Config, Destino, Sincronizador, conectar};

fn app_de_prueba() -> adw::Application {
    adw::init().expect("se necesita acceso GTK");
    adw::Application::builder()
        .application_id("io.github.caracelp.Orbital.DriveTest")
        .flags(gio::ApplicationFlags::NON_UNIQUE)
        .build()
}

fn cliente(url: &str) -> Cliente {
    Cliente::new(
        Direcciones::locales(url),
        Credenciales {
            client_id: "x".into(),
            client_secret: "x".into(),
            refresh_token: "x".into(),
        },
    )
}

async fn esperar(que: &str, mut f: impl FnMut() -> bool) {
    for _ in 0..300 {
        if f() {
            return;
        }
        glib::timeout_future(std::time::Duration::from_millis(50)).await;
    }
    panic!("no pasó: {que}");
}

/// Bóveda sincronizada con una nota en conflicto (como la deja `preparar-prueba`).
fn preparar(srv: &ServidorFalso) -> (PathBuf, PathBuf, String) {
    let base = std::env::temp_dir().join(format!("orbital-061-gtk-{}", glib::uuid_string_random()));
    let root = base.join("boveda");
    let dir = base.join("estado");
    let escribir = |r: &str, t: &str| {
        let p = root.join(r);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, t).unwrap();
    };
    escribir("wiki/Proyecto.md", "# Proyecto\n\nbase\n");
    escribir("wiki/Lecturas/Libro.md", "# Libro\n");
    escribir("wiki/privado/Diario.md", "# Diario\n");
    let mut c = cliente(&srv.url);
    conectar(&mut c, &root, &dir, CUENTA, Destino::Nueva("Prueba".into())).unwrap();
    let mut con = orbital_drive::Conexion::leer(&root).unwrap().unwrap();
    con.poner_local("wiki/privado/", true);
    con.guardar(&root).unwrap();
    let cfg = Config {
        papelera: orbital_drive::Papelera::Boveda,
        dir_estado: dir.clone(),
        ..Config::new(&root)
    };
    let mut s = Sincronizador::new(
        cfg,
        cliente(&srv.url),
        orbital_drive::Estado::leer(&dir).unwrap().unwrap(),
        con,
        Box::new(|_| {}),
    );
    s.ciclo().unwrap();
    let raiz = s.estado.raiz.clone();
    let id = srv.datos().buscar("wiki/Proyecto.md", &raiz).unwrap();
    srv.datos()
        .editar(&id, b"# Proyecto\n\nde Drive\n", "Celular");
    std::thread::sleep(std::time::Duration::from_millis(5));
    escribir("wiki/Proyecto.md", "# Proyecto\n\nde este equipo\n");
    s.pedir_completa();
    s.ciclo().unwrap();
    assert_eq!(s.estado.conflictos.len(), 1);
    (root, dir, raiz)
}

/// US1, US3, US5: el motor arranca solo con la bóveda conectada, el explorador muestra el
/// estado de cada fila (y lo dice en el nombre accesible), la barra de estado resume, el menú de
/// la nota ofrece resolver, y un cambio guardado llega a Drive.
#[test]
#[ignore = "requiere GTK"]
fn drive_explorador_barra_y_conflicto() {
    let app = app_de_prueba();
    app.register(None::<&gio::Cancellable>).unwrap();
    glib::MainContext::default().block_on(async {
        let srv = ServidorFalso::iniciar();
        let (root, dir, raiz) = preparar(&srv);
        *drive::PRUEBA.lock().unwrap() = Some((srv.url.clone(), dir));
        let window = Window::new(&app);
        window.set_persist(false);
        window.0.window.set_default_size(1100, 800);
        window.present();
        let (v, tree, meta) = prepare(orbital_vault::open(&root).unwrap());
        window.show_vault(v, tree, meta);
        esperar("el motor arranca", || window.drive_activo()).await;
        let w = window.clone();
        esperar("el estado de la nota llega", move || {
            w.drive_estado("wiki/Lecturas/Libro.md")
                == Some(drive::EstadoFila::Drive(
                    orbital_drive::EstadoArchivo::Ambos,
                ))
        })
        .await;
        assert_eq!(
            window.drive_estado("wiki/Proyecto.md"),
            Some(drive::EstadoFila::Drive(
                orbital_drive::EstadoArchivo::Conflicto
            ))
        );
        assert_eq!(
            window.drive_estado("wiki/privado"),
            Some(drive::EstadoFila::Local)
        );
        // La carpeta resume el peor estado de su contenido.
        assert_eq!(
            window.drive_estado("wiki"),
            Some(drive::EstadoFila::Drive(
                orbital_drive::EstadoArchivo::Conflicto
            ))
        );
        // Filas visibles: ícono y nombre accesible con el estado.
        let w = window.clone();
        esperar("las filas pintadas", move || {
            w.0.drive_filas.borrow().get("wiki").is_some_and(|f| {
                f.icono.upgrade().is_some_and(|i| {
                    i.is_visible() && i.icon_name().as_deref() == Some("dialog-warning-symbolic")
                })
            })
        })
        .await;
        let etiqueta = window.0.drive_filas.borrow()["wiki"]
            .item
            .upgrade()
            .unwrap()
            .accessible_label()
            .to_string();
        assert_eq!(etiqueta, "Carpeta wiki, en conflicto con Drive");
        // Barra de estado.
        let boton = window.0.status.drive_boton().clone();
        let b = boton.clone();
        esperar("la barra de estado", move || {
            b.is_visible() && b.tooltip_text().is_some_and(|t| t.contains("1 conflicto"))
        })
        .await;
        // Menú de la nota en conflicto.
        let entry = window.entry("wiki/Proyecto.md").unwrap();
        window.show_entry(entry);
        let w = window.clone();
        esperar("la nota abierta", move || {
            w.0.current_entry
                .borrow()
                .as_ref()
                .is_some_and(|e| e.path.as_str() == "wiki/Proyecto.md")
        })
        .await;
        let items: Vec<String> = window
            .items_menu_nota()
            .into_iter()
            .map(|i| i.label)
            .collect();
        assert!(
            items
                .iter()
                .any(|i| i == "Resolver el conflicto con Drive…"),
            "{items:?}"
        );
        assert!(items.iter().any(|i| i == "Copiar enlace de Drive"));
        // «Conservar la de este equipo»: Drive queda con la de este equipo.
        window.drive_orden(orbital_drive::Orden::Resolver {
            ruta: "wiki/Proyecto.md".into(),
            mia: true,
        });
        let (s2, r2) = (srv.datos.clone(), raiz.clone());
        esperar("la resolución llega a Drive", move || {
            let d = s2.lock().unwrap();
            d.buscar("wiki/Proyecto.md", &r2)
                .is_some_and(|id| d.archivos[&id].contenido == b"# Proyecto\n\nde este equipo\n")
        })
        .await;
        let w = window.clone();
        esperar("sin conflictos", move || {
            w.0.drive.borrow().conflictos.is_empty()
        })
        .await;
        // Un archivo nuevo en el disco llega a Drive (vigilante → motor).
        std::fs::write(root.join("wiki/Lecturas/Nueva.md"), "# Nueva\n").unwrap();
        let (s2, r2) = (srv.datos.clone(), raiz.clone());
        esperar("la nota nueva llega a Drive", move || {
            s2.lock()
                .unwrap()
                .buscar("wiki/Lecturas/Nueva.md", &r2)
                .is_some()
        })
        .await;
        // Captura para el quickstart.
        if let Ok(d) = std::env::var("ORBITAL_CAPTURA_DIR") {
            glib::timeout_future(std::time::Duration::from_millis(600)).await;
            window
                .capture(&Path::new(&d).join("061-explorador.png"))
                .unwrap();
        }
        window.0.window.destroy();
        *drive::PRUEBA.lock().unwrap() = None;
        let _ = std::fs::remove_dir_all(root.parent().unwrap());
    });
}

/// FR-015 y SC-001: una bóveda sin `drive.yaml` no arranca ningún motor ni muestra nada de Drive.
#[test]
#[ignore = "requiere GTK"]
fn drive_sin_conectar_no_carga_nada() {
    let app = app_de_prueba();
    app.register(None::<&gio::Cancellable>).unwrap();
    glib::MainContext::default().block_on(async {
        let root =
            std::env::temp_dir().join(format!("orbital-061-sin-{}", glib::uuid_string_random()));
        std::fs::create_dir_all(root.join("wiki")).unwrap();
        std::fs::write(root.join("wiki/a.md"), "# A\n").unwrap();
        let window = Window::new(&app);
        window.set_persist(false);
        window.present();
        let (v, tree, meta) = prepare(orbital_vault::open(&root).unwrap());
        window.show_vault(v, tree, meta);
        glib::timeout_future(std::time::Duration::from_millis(800)).await;
        assert!(!window.drive_activo());
        assert!(!window.0.status.drive_boton().is_visible());
        assert_eq!(window.drive_estado("wiki/a.md"), None);
        assert!(
            !root.join(".orbital").exists(),
            "no escribió nada en la bóveda"
        );
        assert!(window.items_menu_drive().is_empty());
        window.0.window.destroy();
        let _ = std::fs::remove_dir_all(root);
    });
}

/// Una bóveda conectada en la que «lo nuevo en Drive» queda «solo en Drive» (061b): una nota y
/// una imagen creadas en la web, más una nota de este equipo que incrusta la imagen.
fn preparar_solo_drive(srv: &ServidorFalso) -> (PathBuf, PathBuf, String) {
    let base =
        std::env::temp_dir().join(format!("orbital-061b-gtk-{}", glib::uuid_string_random()));
    let root = base.join("boveda");
    let dir = base.join("estado");
    std::fs::create_dir_all(root.join("wiki")).unwrap();
    std::fs::create_dir_all(root.join("raw")).unwrap();
    std::fs::write(root.join("wiki/Visible.md"), "# Visible\n\n![[foto.png]]\n").unwrap();
    let mut c = cliente(&srv.url);
    conectar(&mut c, &root, &dir, CUENTA, Destino::Nueva("Prueba".into())).unwrap();
    let cfg = Config {
        papelera: orbital_drive::Papelera::Boveda,
        dir_estado: dir.clone(),
        dir_cache: dir.join("cache"),
        ..Config::new(&root)
    };
    let mut s = Sincronizador::new(
        cfg,
        cliente(&srv.url),
        orbital_drive::Estado::leer(&dir).unwrap().unwrap(),
        orbital_drive::Conexion::leer(&root).unwrap().unwrap(),
        Box::new(|_| {}),
    );
    s.predeterminados(
        orbital_drive::reglas::Ubicacion::Ambos,
        orbital_drive::reglas::Ubicacion::SoloDrive,
    );
    s.ciclo().unwrap();
    let raiz = s.estado.raiz.clone();
    {
        let mut d = srv.datos();
        let wiki = d.buscar("wiki", &raiz).unwrap();
        let raw = d.buscar("raw", &raiz).unwrap();
        d.crear(
            &wiki,
            "Remota.md",
            "# Remota\n\npalabrarara y [[Visible]] #nube\n".as_bytes(),
            "Web",
        );
        let png =
            include_bytes!("../../data/icons/hicolor/scalable/apps/io.github.caracelp.Orbital.svg");
        d.crear(&raw, "foto.png", &png[..], "Web");
    }
    s.pedir_completa();
    s.ciclo().unwrap();
    s.ciclo().unwrap();
    assert_eq!(s.estado.marcadores().len(), 2);
    (root, dir, raiz)
}

/// US2 y US4: lo «solo en Drive» aparece en el explorador desde el escaneo (sin red), con su
/// ícono y su estado en el nombre accesible; sin conexión se ve bloqueado y no se abre; con
/// conexión se baja a la caché y se abre; lo editado se sube; el selector «Ubicación» marca la
/// actual y «En ambos» lo trae a la bóveda.
#[test]
#[ignore = "requiere GTK"]
fn drive_solo_en_drive_marcadores_y_apertura() {
    let app = app_de_prueba();
    app.register(None::<&gio::Cancellable>).unwrap();
    glib::MainContext::default().block_on(async {
        use orbital_drive::reglas::Ubicacion;
        let srv = ServidorFalso::iniciar();
        let (root, dir, raiz) = preparar_solo_drive(&srv);
        *drive::PRUEBA.lock().unwrap() = Some((srv.url.clone(), dir.clone()));
        let window = Window::new(&app);
        window.set_persist(false);
        window.0.window.set_default_size(1100, 800);
        window.present();
        let (v, tree, meta) = prepare(orbital_vault::open(&root).unwrap());
        assert!(
            !root.join("wiki/Remota.md").exists(),
            "no está en la bóveda"
        );
        window.show_vault(v, tree, meta);
        // Desde el escaneo, antes que el motor: la entrada y su marca.
        assert!(window.entry("wiki/Remota.md").is_some());
        assert!(window.drive_es_marcador("raw/foto.png"));
        esperar("el motor arranca", || window.drive_activo()).await;
        let w = window.clone();
        esperar("el resumen llega", move || {
            w.0.drive.borrow().resumen.is_some()
        })
        .await;
        assert_eq!(
            window.drive_estado("wiki/Remota.md"),
            Some(drive::EstadoFila::Drive(
                orbital_drive::EstadoArchivo::SoloDrive
            ))
        );
        // El selector marca la ubicación actual.
        let items = window.items_ubicacion("wiki/Remota.md", false);
        let marcada: Vec<&str> = items
            .iter()
            .filter(|i| i.marcado == Some(true))
            .map(|i| i.label.as_str())
            .collect();
        assert_eq!(marcada, ["Ubicación: Solo en Drive"]);
        assert_eq!(
            window.items_ubicacion("wiki/Visible.md", false)[2].marcado,
            Some(true),
            "«En ambos»"
        );
        // El menú contextual del explorador (captura del menú abierto).
        let lista = window.0.list_view.clone();
        let menu = window
            .menu_contextual("wiki/Remota.md", lista.upcast_ref(), None)
            .expect("con la bóveda conectada hay menú");
        if let Ok(d) = std::env::var("ORBITAL_CAPTURA_DIR") {
            let w = menu.clone();
            esperar("el menú dibujado", move || w.is_mapped() && w.width() > 1).await;
            glib::timeout_future(std::time::Duration::from_millis(600)).await;
            window
                .capture_widget(
                    menu.upcast_ref(),
                    &Path::new(&d).join("061b-menu-ubicacion.png"),
                )
                .unwrap();
        }
        menu.popdown();
        // La fila de la nota «solo en Drive», con su estado en el nombre accesible.
        window.mostrar_en_explorador("wiki/Remota.md");
        let w = window.clone();
        esperar("la fila pintada", move || {
            w.0.drive_filas
                .borrow()
                .get("wiki/Remota.md")
                .is_some_and(|f| {
                    f.icono.upgrade().is_some_and(|i| {
                        i.is_visible() && i.icon_name().as_deref() == Some("folder-remote-symbolic")
                    })
                })
        })
        .await;
        let etiqueta = |w: &Window| {
            w.0.drive_filas.borrow()["wiki/Remota.md"]
                .item
                .upgrade()
                .unwrap()
                .accessible_label()
                .to_string()
        };
        assert_eq!(etiqueta(&window), "Nota Remota.md, solo en Drive");
        if let Ok(d) = std::env::var("ORBITAL_CAPTURA_DIR") {
            glib::timeout_future(std::time::Duration::from_millis(400)).await;
            window
                .capture(&Path::new(&d).join("061b-explorador.png"))
                .unwrap();
        }
        // Sin conexión: bloqueado, y abrirla explica por qué no.
        srv.datos().caido = true;
        window.drive_orden(orbital_drive::Orden::Red(false));
        let w = window.clone();
        esperar("bloqueada", move || {
            w.drive_estado("wiki/Remota.md") == Some(drive::EstadoFila::Bloqueado)
        })
        .await;
        let w = window.clone();
        esperar("la fila dice bloqueada", move || {
            etiqueta(&w) == "Nota Remota.md, solo en Drive, bloqueado sin conexión"
        })
        .await;
        window.show_entry(window.entry("wiki/Remota.md").unwrap());
        let w = window.clone();
        esperar("la página que lo explica", move || {
            w.0.content_stack
                .child_by_name("vacio")
                .and_downcast::<adw::StatusPage>()
                .is_some_and(|p| p.title() == "Solo en Drive")
                && w.0.content_stack.visible_child_name().as_deref() == Some("vacio")
        })
        .await;
        assert!(window.0.current_entry.borrow().is_none());
        if let Ok(d) = std::env::var("ORBITAL_CAPTURA_DIR") {
            glib::timeout_future(std::time::Duration::from_millis(400)).await;
            window
                .capture(&Path::new(&d).join("061b-bloqueado.png"))
                .unwrap();
        }
        // Con conexión: se baja a la caché y se abre.
        srv.datos().caido = false;
        window.drive_orden(orbital_drive::Orden::Red(true));
        let w = window.clone();
        esperar("en línea", move || w.drive_en_linea()).await;
        window.show_entry(window.entry("wiki/Remota.md").unwrap());
        let w = window.clone();
        esperar("la nota abierta", move || {
            w.0.current_entry
                .borrow()
                .as_ref()
                .is_some_and(|e| e.path.as_str() == "wiki/Remota.md")
        })
        .await;
        let copia = RelPath::new("wiki/Remota.md").unwrap().to_path(&root);
        assert!(
            copia.starts_with(dir.join("cache")),
            "la copia está en la caché"
        );
        assert!(!root.join("wiki/Remota.md").exists(), "y no en la bóveda");
        // Lo editado (como lo guarda el editor: en la copia) se sube y sigue «solo en Drive».
        std::thread::sleep(std::time::Duration::from_millis(5));
        std::fs::write(&copia, "# Remota\n\neditada en este equipo\n").unwrap();
        window.drive_orden(orbital_drive::Orden::Cambios(vec!["wiki/Remota.md".into()]));
        let (s2, r2) = (srv.datos.clone(), raiz.clone());
        esperar("el cambio llega a Drive", move || {
            let d = s2.lock().unwrap();
            d.buscar("wiki/Remota.md", &r2).is_some_and(|id| {
                d.archivos[&id].contenido == b"# Remota\n\neditada en este equipo\n"
            })
        })
        .await;
        assert!(!root.join("wiki/Remota.md").exists());
        // «En ambos»: vuelve a la bóveda.
        window.drive_ubicar("wiki/Remota.md", false, Ubicacion::Ambos);
        let r = root.clone();
        esperar("en la bóveda", move || r.join("wiki/Remota.md").is_file()).await;
        let w = window.clone();
        esperar("ya no es un marcador", move || {
            !w.drive_es_marcador("wiki/Remota.md")
        })
        .await;
        window.0.window.destroy();
        *drive::PRUEBA.lock().unwrap() = None;
        let _ = std::fs::remove_dir_all(root.parent().unwrap());
    });
}
