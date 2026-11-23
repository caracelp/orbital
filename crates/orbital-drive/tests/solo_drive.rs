//! Escenarios de la 061b («solo en Drive» y el selector de ubicación) contra el servidor falso.

mod comun;

use comun::Equipo;
use orbital_drive::falso::{Falla, ServidorFalso};
use orbital_drive::motor::Destino;
use orbital_drive::reglas::Ubicacion;

/// A conecta una bóveda con contenido. B se une con «lo nuevo en Drive: solo en Drive».
fn a_y_b_solo_drive(srv: &ServidorFalso) -> (Equipo, Equipo, String) {
    let mut a = Equipo::new("a", srv);
    a.escribir("wiki/nota.md", b"# Nota\n\n[[otra]] #etiqueta\n");
    a.escribir("wiki/otra.md", b"# Otra\n");
    a.escribir("raw/foto.png", &[0x89, b'P', b'N', b'G', 9, 9, 9]);
    a.escribir(".orbital/listas.yaml", b"favoritos: []\n");
    a.conectar(srv, Destino::Nueva("Bóveda".into()));
    a.sincronizar();
    let id = a.s().estado.raiz.clone();
    let mut b = Equipo::new("b", srv);
    b.conectar(srv, Destino::Existente(id.clone()));
    b.s()
        .predeterminados(Ubicacion::Ambos, Ubicacion::SoloDrive);
    b.sincronizar();
    (a, b, id)
}

#[test]
fn lo_nuevo_en_drive_queda_solo_en_drive_con_extracto() {
    let srv = ServidorFalso::iniciar();
    let (a, mut b, _) = a_y_b_solo_drive(&srv);
    // En la bóveda de B no hay notas ni la imagen: solo carpetas y la configuración compartida.
    assert!(b.leer("wiki/nota.md").is_none());
    assert!(b.leer("raw/foto.png").is_none());
    assert!(b.root.join("wiki").is_dir(), "las carpetas sí");
    assert_eq!(b.leer(".orbital/listas.yaml").unwrap(), b"favoritos: []\n");
    assert_eq!(
        b.marcadores(),
        vec!["raw/foto.png", "wiki/nota.md", "wiki/otra.md"]
    );
    // Lo que se ve es lo mismo que en A.
    let va = a.vista(&srv);
    assert_eq!(b.vista(&srv), va);
    // Las notas tienen su extracto en la caché (para el índice); la imagen, no.
    assert!(
        b.copias()
            .iter()
            .any(|c| c == b"# Nota\n\n[[otra]] #etiqueta\n")
    );
    assert!(!b.copias().iter().any(|c| c[0] == 0x89));
    // Los marcadores quedaron escritos para el escaneo de la app, sin red.
    let m = orbital_drive::estado::leer_marcadores(&b.dir_estado);
    assert_eq!(m.len(), 3);
    assert_eq!(m[0].ruta, "raw/foto.png");
    assert_eq!(m[0].tam, 7);
}

#[test]
fn editar_la_copia_sube_y_sigue_solo_en_drive() {
    let srv = ServidorFalso::iniciar();
    let (mut a, mut b, id) = a_y_b_solo_drive(&srv);
    let copia = b.s().traer("wiki/nota.md").unwrap();
    assert!(
        !copia.starts_with(&b.root),
        "la copia está fuera de la bóveda"
    );
    std::thread::sleep(std::time::Duration::from_millis(5));
    std::fs::write(&copia, b"# Nota\n\neditada en B\n").unwrap();
    b.sincronizar();
    assert!(
        b.leer("wiki/nota.md").is_none(),
        "sigue sin estar en la bóveda"
    );
    assert_eq!(b.marcadores().len(), 3);
    let nid = srv.datos().buscar("wiki/nota.md", &id).unwrap();
    assert_eq!(
        srv.datos().archivos[&nid].contenido,
        b"# Nota\n\neditada en B\n"
    );
    a.sincronizar();
    assert_eq!(a.leer("wiki/nota.md").unwrap(), b"# Nota\n\neditada en B\n");
    // La web la cambia: el extracto de B se renueva solo.
    srv.datos().editar(&nid, b"# Nota\n\ndesde la web\n", "Web");
    b.sincronizar();
    assert_eq!(std::fs::read(&copia).unwrap(), b"# Nota\n\ndesde la web\n");
}

#[test]
fn traer_una_imagen_para_mostrarla() {
    let srv = ServidorFalso::iniciar();
    let (_a, mut b, _) = a_y_b_solo_drive(&srv);
    let copia = b.s().traer("raw/foto.png").unwrap();
    assert_eq!(
        std::fs::read(&copia).unwrap()[..4],
        [0x89, b'P', b'N', b'G']
    );
    assert!(b.leer("raw/foto.png").is_none());
    // Sin red no se trae (la app lo muestra bloqueado).
    std::fs::remove_file(&copia).unwrap();
    srv.datos().caido = true;
    assert!(b.s().traer("raw/foto.png").is_err());
}

#[test]
fn liberar_espacio_confirma_con_drive_antes_de_sacar() {
    let srv = ServidorFalso::iniciar();
    let (mut a, _b, id) = a_y_b_solo_drive(&srv);
    // Un cambio sin subir: no se libera hasta que Drive lo tiene.
    a.escribir("raw/foto.png", b"PNG nueva");
    a.s().ubicar("raw/", Some(Ubicacion::SoloDrive));
    // Un error al confirmar con Drive: no se saca nada.
    a.ciclo().unwrap();
    srv.datos().fallar(Falla::Error(500, "backendError"));
    let _ = a.ciclo();
    a.sincronizar();
    let fid = srv.datos().buscar("raw/foto.png", &id).unwrap();
    assert_eq!(srv.datos().archivos[&fid].contenido, b"PNG nueva");
    assert!(
        a.leer("raw/foto.png").is_none(),
        "liberada después de subir"
    );
    assert_eq!(a.marcadores(), vec!["raw/foto.png"]);
    assert_eq!(a.vista(&srv)["raw/foto.png"], b"PNG nueva");
    // «Mantener en este equipo»: vuelve a la bóveda.
    a.s().ubicar("raw/foto.png", Some(Ubicacion::Ambos));
    a.sincronizar();
    assert_eq!(a.leer("raw/foto.png").unwrap(), b"PNG nueva");
    assert!(a.marcadores().is_empty());
    // Y la regla de la carpeta sigue para lo demás.
    a.escribir("raw/otra.bin", b"otra");
    a.sincronizar();
    assert!(a.leer("raw/otra.bin").is_none());
    assert!(srv.datos().buscar("raw/otra.bin", &id).is_some());
}

#[test]
fn corte_a_mitad_de_liberar_no_pierde_nada() {
    let srv = ServidorFalso::iniciar();
    let (mut a, _b, id) = a_y_b_solo_drive(&srv);
    a.s().ubicar("wiki/nota.md", Some(Ubicacion::SoloDrive));
    a.s().cortar_liberar = true;
    assert!(a.ciclo().is_err(), "el corte provocado");
    // Se guardó el estado («solo en Drive») pero la copia sigue en la bóveda: Orbital se cierra.
    assert!(a.leer("wiki/nota.md").is_some());
    a.arrancar(&srv);
    // Al volver, no se borra nada en Drive y se termina de liberar.
    a.sincronizar();
    let nid = srv.datos().buscar("wiki/nota.md", &id).unwrap();
    assert!(!srv.datos().archivos[&nid].papelera);
    assert!(a.leer("wiki/nota.md").is_none());
    assert_eq!(a.marcadores(), vec!["wiki/nota.md"]);
    assert_eq!(
        a.vista(&srv)["wiki/nota.md"],
        b"# Nota\n\n[[otra]] #etiqueta\n"
    );
}

#[test]
fn lo_nuevo_de_este_equipo_con_predeterminado_solo_en_drive() {
    let srv = ServidorFalso::iniciar();
    let (mut a, _b, id) = a_y_b_solo_drive(&srv);
    a.s()
        .predeterminados(Ubicacion::SoloDrive, Ubicacion::Ambos);
    a.escribir("raw/video.mp4", b"un video largo");
    a.sincronizar();
    assert!(a.leer("raw/video.mp4").is_none());
    assert!(srv.datos().buscar("raw/video.mp4", &id).is_some());
    // «Solo en este equipo» como predeterminado: no se sube.
    a.s().predeterminados(Ubicacion::Local, Ubicacion::Ambos);
    a.escribir("raw/privado.txt", b"no subir");
    a.sincronizar();
    assert!(srv.datos().buscar("raw/privado.txt", &id).is_none());
    assert!(a.s().mapa().get("raw/privado.txt") == Some(&orbital_drive::EstadoArchivo::Local));
}

#[test]
fn la_web_renombra_y_borra_lo_solo_en_drive() {
    let srv = ServidorFalso::iniciar();
    let (_a, mut b, id) = a_y_b_solo_drive(&srv);
    let nid = srv.datos().buscar("wiki/nota.md", &id).unwrap();
    srv.datos().renombrar(&nid, "renombrada.md");
    b.sincronizar();
    assert_eq!(
        b.marcadores(),
        vec!["raw/foto.png", "wiki/otra.md", "wiki/renombrada.md"]
    );
    let copia = b.s().traer("wiki/renombrada.md").unwrap();
    assert!(copia.ends_with("renombrada.md"), "la copia sigue al nombre");
    // Cambios sin subir y la web la manda a la papelera: vuelve a la bóveda y se sube de nuevo.
    std::thread::sleep(std::time::Duration::from_millis(5));
    std::fs::write(&copia, b"cambios de B").unwrap();
    srv.datos().a_papelera(&nid);
    b.sincronizar();
    assert_eq!(b.leer("wiki/renombrada.md").unwrap(), b"cambios de B");
    let nuevo = srv.datos().buscar("wiki/renombrada.md", &id).unwrap();
    assert_eq!(srv.datos().archivos[&nuevo].contenido, b"cambios de B");
    // Sin cambios: la papelera de Drive basta, la copia se suelta.
    let oid = srv.datos().buscar("wiki/otra.md", &id).unwrap();
    srv.datos().a_papelera(&oid);
    b.sincronizar();
    assert!(!b.marcadores().contains(&"wiki/otra.md".to_owned()));
    assert!(!b.copias().iter().any(|c| c == b"# Otra\n"));
}

#[test]
fn cambiado_en_los_dos_lados_abre_el_conflicto_de_siempre() {
    let srv = ServidorFalso::iniciar();
    let (_a, mut b, id) = a_y_b_solo_drive(&srv);
    let copia = b.s().traer("raw/foto.png").unwrap();
    std::thread::sleep(std::time::Duration::from_millis(5));
    std::fs::write(&copia, b"foto de B").unwrap();
    let fid = srv.datos().buscar("raw/foto.png", &id).unwrap();
    srv.datos().editar(&fid, b"foto de la web", "Web");
    b.sincronizar();
    assert_eq!(b.conflictos(), vec!["raw/foto.png"]);
    assert_eq!(b.leer("raw/foto.png").unwrap(), b"foto de B");
    assert!(b.todo().iter().any(|v| v == b"foto de la web"));
    // Se resuelve y vuelve a ser «solo en Drive» (el predeterminado con que llegó).
    b.s().resolver("raw/foto.png", true).unwrap();
    b.sincronizar();
    assert_eq!(srv.datos().archivos[&fid].contenido, b"foto de B");
    assert!(b.leer("raw/foto.png").is_none());
    assert!(b.marcadores().contains(&"raw/foto.png".to_owned()));
}

#[test]
fn borrar_y_mover_lo_solo_en_drive() {
    let srv = ServidorFalso::iniciar();
    let (_a, mut b, id) = a_y_b_solo_drive(&srv);
    let fid = srv.datos().buscar("raw/foto.png", &id).unwrap();
    b.s()
        .mover_marcador("raw/foto.png", "wiki/foto.png")
        .unwrap();
    assert_eq!(srv.datos().buscar("wiki/foto.png", &id), Some(fid.clone()));
    b.s().borrar_marcador("wiki/foto.png").unwrap();
    assert!(
        srv.datos().archivos[&fid].papelera,
        "a la papelera de Drive"
    );
    assert!(!b.marcadores().contains(&"wiki/foto.png".to_owned()));
}

#[test]
fn renombrar_una_carpeta_conserva_su_id() {
    let srv = ServidorFalso::iniciar();
    let mut a = Equipo::new("a", &srv);
    a.escribir("wiki/proyecto/a.md", b"a");
    a.escribir("wiki/proyecto/sub/b.md", b"b");
    a.conectar(&srv, Destino::Nueva("Bóveda".into()));
    a.sincronizar();
    let raiz = a.s().estado.raiz.clone();
    let cid = srv.datos().buscar("wiki/proyecto", &raiz).unwrap();
    let pedidos = srv.datos().pedidos;
    std::fs::rename(a.root.join("wiki/proyecto"), a.root.join("wiki/Proyecto X")).unwrap();
    a.s()
        .marcar(["wiki/proyecto".to_owned(), "wiki/Proyecto X".to_owned()]);
    a.sincronizar();
    assert_eq!(srv.datos().buscar("wiki/Proyecto X", &raiz), Some(cid));
    assert!(
        srv.datos()
            .buscar("wiki/Proyecto X/sub/b.md", &raiz)
            .is_some()
    );
    assert!(srv.datos().buscar("wiki/proyecto", &raiz).is_none());
    assert!(
        srv.datos().pedidos - pedidos < 20,
        "un pedido por la carpeta, no uno por archivo"
    );
}

#[test]
fn cuota_y_tope_de_la_cache() {
    let srv = ServidorFalso::iniciar();
    let (_a, mut b, _) = a_y_b_solo_drive(&srv);
    b.ciclo().unwrap();
    let c = b.s().cuota.expect("cuota");
    assert!(c.usado > 0 && c.limite.is_some());
    // Con un tope chico, la copia abierta de la imagen se suelta; los extractos de texto, no.
    let copia = b.s().traer("raw/foto.png").unwrap();
    assert!(copia.is_file());
    let mut cfg = b.config();
    cfg.tope_cache = 1;
    let estado = b.s().estado.clone();
    let conexion = b.s().conexion.clone();
    b.sinc = Some(orbital_drive::motor::Sincronizador::new(
        cfg,
        comun::cliente(&srv),
        estado,
        conexion,
        Box::new(|_| {}),
    ));
    b.escribir("wiki/cambio.md", b"algo para que haya cambios");
    b.sincronizar();
    assert!(!copia.is_file(), "la copia abierta se recortó");
    assert!(
        b.copias().iter().any(|c| c == b"# Otra\n"),
        "el extracto queda"
    );
}

#[test]
fn nombre_repetido_en_drive_con_marcadores() {
    let srv = ServidorFalso::iniciar();
    let (_a, mut b, id) = a_y_b_solo_drive(&srv);
    // La web renombra «otra.md» (id más chico) a «nota.md»: dos «nota.md» en la carpeta.
    let otra = srv.datos().buscar("wiki/otra.md", &id).unwrap();
    let nota = srv.datos().buscar("wiki/nota.md", &id).unwrap();
    assert_ne!(otra, nota);
    srv.datos().renombrar(&otra, "nota.md");
    b.sincronizar();
    let mut m = b.marcadores();
    m.sort();
    assert_eq!(
        m,
        vec!["raw/foto.png", "wiki/nota (2).md", "wiki/nota.md"],
        "{m:?}"
    );
}
