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
