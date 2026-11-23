//! Pendientes de la 061b contra el servidor falso: bajar con progreso y cancelar, y los cambios
//! de lo «solo en Drive» hechos sin conexión (en cola hasta que vuelve la red).

mod comun;

use comun::Equipo;
use orbital_drive::falso::{Falla, ServidorFalso};
use orbital_drive::motor::{CANCELADA, Destino};
use orbital_drive::reglas::Ubicacion;
use orbital_drive::{EstadoArchivo, Evento};

/// A conecta una bóveda con un video grande y unas notas; B se une con «lo nuevo en Drive: solo
/// en Drive» (en B todo es marcador).
fn a_y_b(srv: &ServidorFalso) -> (Equipo, Equipo, String) {
    let mut a = Equipo::new("a", srv);
    a.escribir("wiki/nota.md", b"# Nota\n");
    a.escribir("wiki/otra.md", b"# Otra\n");
    let video: Vec<u8> = (0..600_000u32).map(|i| (i % 253) as u8).collect();
    a.escribir("raw/video.mp4", &video);
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

/// Lo que hay en las carpetas de bajadas a medias de la caché.
fn a_medias(b: &Equipo) -> Vec<String> {
    let mut v = Vec::new();
    for d in ["cache/parcial"] {
        let dir = b.root.parent().unwrap().join(d);
        if let Ok(rd) = std::fs::read_dir(dir) {
            v.extend(
                rd.flatten()
                    .map(|e| e.file_name().to_string_lossy().into_owned()),
            );
        }
    }
    v
}

fn progresos(b: &Equipo) -> Vec<(u64, u64)> {
    b.eventos
        .lock()
        .unwrap()
        .iter()
        .filter_map(|e| match e {
            Evento::Progreso { bajado, total, .. } => Some((*bajado, *total)),
            _ => None,
        })
        .collect()
}

#[test]
fn bajar_avisa_el_progreso() {
    let srv = ServidorFalso::iniciar();
    let (_a, mut b, _) = a_y_b(&srv);
    srv.datos().ritmo_bajada = Some((64 * 1024, std::time::Duration::from_millis(30)));
    b.eventos.lock().unwrap().clear();
    let copia = b.s().traer_pedido("raw/video.mp4", 1).unwrap();
    assert_eq!(std::fs::metadata(&copia).unwrap().len(), 600_000);
    let p = progresos(&b);
    assert!(p.len() >= 3, "varios avisos: {p:?}");
    assert!(p.iter().all(|(_, t)| *t == 600_000));
    assert!(p.windows(2).all(|w| w[0].0 <= w[1].0), "crece: {p:?}");
    assert_eq!(p.last().unwrap().0, 600_000, "el último es el total");
}

#[test]
fn cancelar_la_bajada_no_deja_nada_a_medias() {
    let srv = ServidorFalso::iniciar();
    let (_a, mut b, _) = a_y_b(&srv);
    srv.datos().ritmo_bajada = Some((32 * 1024, std::time::Duration::from_millis(5)));
    let estado_antes = b.s().estado.registros.clone();
    // Se corta cuando lleva 100 KB (como la usuaria que aprieta «Cancelar» a mitad).
    b.s().cancelador.cancelar_en(7, 100_000);
    let r = b.s().traer_pedido("raw/video.mp4", 7);
    assert_eq!(r, Err(CANCELADA.to_owned()));
    assert!(a_medias(&b).is_empty(), "nada a medias: {:?}", a_medias(&b));
    assert!(
        !b.copias().iter().any(|c| c.len() > 1000),
        "ninguna copia del video"
    );
    assert_eq!(
        b.s().estado.registros,
        estado_antes,
        "el estado sigue igual"
    );
    // Cancelado antes de empezar: ni siquiera se pide.
    let pedidos = srv.datos().pedidos;
    b.s().cancelador.cancelar(8);
    assert_eq!(
        b.s().traer_pedido("raw/video.mp4", 8),
        Err(CANCELADA.to_owned())
    );
    assert_eq!(srv.datos().pedidos, pedidos);
    // El motor sigue sano: un ciclo y otra bajada (el mismo pedido no queda cancelado).
    srv.datos().ritmo_bajada = None;
    b.sincronizar();
    let copia = b.s().traer_pedido("raw/video.mp4", 7).unwrap();
    assert_eq!(std::fs::metadata(&copia).unwrap().len(), 600_000);
}

#[test]
fn cancelar_despues_de_un_corte_borra_el_tramo() {
    let srv = ServidorFalso::iniciar();
    let (_a, mut b, _) = a_y_b(&srv);
    // Un corte de red deja un tramo para seguir…
    srv.datos().fallar(Falla::CortarBajada(200_000));
    assert!(b.s().traer_pedido("raw/video.mp4", 1).is_err());
    assert_eq!(a_medias(&b).len(), 1, "el tramo queda para retomar");
    // …y si al reintentar la usuaria cancela, no queda nada.
    srv.datos().ritmo_bajada = Some((32 * 1024, std::time::Duration::from_millis(2)));
    b.s().cancelador.cancelar_en(2, 250_000);
    assert_eq!(
        b.s().traer_pedido("raw/video.mp4", 2),
        Err(CANCELADA.to_owned())
    );
    assert!(a_medias(&b).is_empty());
}

/// Ciclo sin red: corta y no aplica nada.
fn sin_red(srv: &ServidorFalso) {
    srv.datos().caido = true;
}

fn con_red(srv: &ServidorFalso) {
    srv.datos().caido = false;
}

#[test]
fn renombrar_sin_conexion_queda_en_cola_y_sobrevive_al_reinicio() {
    let srv = ServidorFalso::iniciar();
    let (_a, mut b, id) = a_y_b(&srv);
    let fid = srv.datos().buscar("raw/video.mp4", &id).unwrap();
    sin_red(&srv);
    assert!(
        b.s()
            .pedir_mover("raw/video.mp4", "raw/viaje.mp4", false)
            .unwrap()
    );
    // La bóveda lo muestra hecho y «pendiente».
    assert!(b.marcadores().contains(&"raw/viaje.mp4".to_owned()));
    assert!(!b.marcadores().contains(&"raw/video.mp4".to_owned()));
    assert_eq!(
        b.s().mapa().get("raw/viaje.mp4"),
        Some(&EstadoArchivo::Pendiente)
    );
    let m = orbital_drive::estado::leer_marcadores(&b.dir_estado);
    assert!(m.iter().any(|x| x.ruta == "raw/viaje.mp4"));
    // Otro cambio sin conexión sobre lo mismo: se juntan en uno.
    assert!(
        b.s()
            .pedir_mover("raw/viaje.mp4", "wiki/viaje.mp4", false)
            .unwrap()
    );
    assert_eq!(b.s().estado.cola.len(), 1);
    assert!(b.ciclo().is_err());
    // Orbital se cierra sin red y vuelve a abrir: la cola sigue.
    b.arrancar(&srv);
    assert_eq!(b.s().estado.cola.len(), 1);
    con_red(&srv);
    b.sincronizar();
    assert!(b.s().estado.cola.is_empty());
    assert_eq!(srv.datos().buscar("wiki/viaje.mp4", &id), Some(fid));
    assert!(srv.datos().buscar("raw/video.mp4", &id).is_none());
    assert_eq!(
        b.s().mapa().get("wiki/viaje.mp4"),
        Some(&EstadoArchivo::SoloDrive)
    );
}

#[test]
fn renombrar_en_cola_y_la_web_lo_cambia() {
    let srv = ServidorFalso::iniciar();
    let (mut a, mut b, id) = a_y_b(&srv);
    let nid = srv.datos().buscar("wiki/nota.md", &id).unwrap();
    sin_red(&srv);
    b.s()
        .pedir_mover("wiki/nota.md", "wiki/Nota final.md", false)
        .unwrap();
    con_red(&srv);
    // Mientras tanto, la web edita el contenido y le pone otro nombre.
    srv.datos().editar(&nid, b"# Nota\n\ndesde la web\n", "Web");
    srv.datos().renombrar(&nid, "nota web.md");
    b.sincronizar();
    // Gana el renombre de la usuaria (es lo último que pidió); el contenido nuevo no se pierde.
    assert_eq!(
        srv.datos().buscar("wiki/Nota final.md", &id),
        Some(nid.clone())
    );
    assert_eq!(
        srv.datos().archivos[&nid].contenido,
        b"# Nota\n\ndesde la web\n"
    );
    assert!(
        b.copias().iter().any(|c| c == b"# Nota\n\ndesde la web\n"),
        "el extracto se renovó"
    );
    a.sincronizar();
    assert_eq!(
        a.leer("wiki/Nota final.md").unwrap(),
        b"# Nota\n\ndesde la web\n"
    );
}

#[test]
fn renombrar_en_cola_a_un_nombre_que_tomo_otro() {
    let srv = ServidorFalso::iniciar();
    let (_a, mut b, id) = a_y_b(&srv);
    let nid = srv.datos().buscar("wiki/nota.md", &id).unwrap();
    sin_red(&srv);
    b.s()
        .pedir_mover("wiki/nota.md", "wiki/nueva.md", false)
        .unwrap();
    con_red(&srv);
    // La web crea otro «nueva.md» antes de que vuelva la red de B.
    let wiki = srv.datos().buscar("wiki", &id).unwrap();
    let otro = srv
        .datos()
        .crear(&wiki, "nueva.md", b"# De la web\n", "Web");
    b.sincronizar();
    let mut m = b.marcadores();
    m.sort();
    assert!(m.contains(&"wiki/nueva.md".to_owned()), "{m:?}");
    assert!(m.contains(&"wiki/nueva (2).md".to_owned()), "{m:?}");
    assert_eq!(srv.datos().archivos[&nid].nombre, "nueva (2).md");
    assert_eq!(srv.datos().archivos[&otro].nombre, "nueva.md");
    assert_eq!(srv.datos().archivos[&nid].contenido, b"# Nota\n");
}

#[test]
fn renombrar_en_cola_y_la_web_lo_borra_con_cambios_en_la_copia() {
    let srv = ServidorFalso::iniciar();
    let (_a, mut b, id) = a_y_b(&srv);
    let nid = srv.datos().buscar("wiki/nota.md", &id).unwrap();
    // El extracto de la nota está en la caché; se edita (como el editor) antes de cortarse la red.
    let copia = b.s().traer("wiki/nota.md").unwrap();
    std::thread::sleep(std::time::Duration::from_millis(5));
    std::fs::write(&copia, b"# Nota\n\ncambio de B sin subir\n").unwrap();
    sin_red(&srv);
    b.s()
        .pedir_mover("wiki/nota.md", "wiki/movida.md", false)
        .unwrap();
    con_red(&srv);
    srv.datos().a_papelera(&nid);
    b.sincronizar();
    // El cambio de B no se pierde: vuelve como archivo nuevo en la ruta que pidió.
    let nueva = srv
        .datos()
        .buscar("wiki/movida.md", &id)
        .expect("se volvió a subir");
    assert_eq!(
        srv.datos().archivos[&nueva].contenido,
        b"# Nota\n\ncambio de B sin subir\n"
    );
}

#[test]
fn borrar_sin_conexion_y_al_volver_la_red() {
    let srv = ServidorFalso::iniciar();
    let (_a, mut b, id) = a_y_b(&srv);
    let vid = srv.datos().buscar("raw/video.mp4", &id).unwrap();
    sin_red(&srv);
    assert!(b.s().pedir_borrar("raw/video.mp4", false).unwrap());
    assert!(!b.marcadores().contains(&"raw/video.mp4".to_owned()));
    b.arrancar(&srv);
    con_red(&srv);
    b.sincronizar();
    assert!(
        srv.datos().archivos[&vid].papelera,
        "a la papelera de Drive"
    );
    assert!(!b.marcadores().contains(&"raw/video.mp4".to_owned()));
    assert!(b.s().estado.cola.is_empty());
}

#[test]
fn borrar_en_cola_y_la_web_lo_cambia_se_conserva() {
    let srv = ServidorFalso::iniciar();
    let (mut a, mut b, id) = a_y_b(&srv);
    let nid = srv.datos().buscar("wiki/otra.md", &id).unwrap();
    sin_red(&srv);
    b.s().pedir_borrar("wiki/otra.md", false).unwrap();
    con_red(&srv);
    srv.datos()
        .editar(&nid, b"# Otra\n\nversion nueva de la web\n", "Web");
    b.sincronizar();
    assert!(!srv.datos().archivos[&nid].papelera, "no fue a la papelera");
    assert!(
        b.marcadores().contains(&"wiki/otra.md".to_owned()),
        "vuelve a aparecer"
    );
    a.sincronizar();
    assert_eq!(
        a.leer("wiki/otra.md").unwrap(),
        b"# Otra\n\nversion nueva de la web\n"
    );
    assert!(
        b.eventos
            .lock()
            .unwrap()
            .iter()
            .any(|e| matches!(e, Evento::Aviso(t) if t.contains("se conservó"))),
        "avisa"
    );
}

#[test]
fn con_conexion_va_directo_y_si_se_corta_queda_en_cola() {
    let srv = ServidorFalso::iniciar();
    let (_a, mut b, id) = a_y_b(&srv);
    let nid = srv.datos().buscar("wiki/nota.md", &id).unwrap();
    assert!(
        !b.s()
            .pedir_mover("wiki/nota.md", "wiki/n2.md", true)
            .unwrap()
    );
    assert_eq!(srv.datos().archivos[&nid].nombre, "n2.md");
    // La red se corta justo en el intento: queda en cola en vez de perderse.
    sin_red(&srv);
    assert!(b.s().pedir_mover("wiki/n2.md", "wiki/n3.md", true).unwrap());
    con_red(&srv);
    b.sincronizar();
    assert_eq!(srv.datos().archivos[&nid].nombre, "n3.md");
}
