//! Escenarios de la 061a contra el servidor falso (nunca una cuenta real).

mod comun;

use comun::Equipo;
use orbital_drive::falso::{Falla, RAIZ, ServidorFalso};
use orbital_drive::motor::Destino;
use orbital_drive::reglas::Conexion;

fn carpeta(a: &mut Equipo) -> String {
    a.s().estado.raiz.clone()
}

/// A conecta una bóveda con contenido; B se une a la misma carpeta.
fn dos_equipos(srv: &ServidorFalso) -> (Equipo, Equipo, String) {
    let mut a = Equipo::new("a", srv);
    a.escribir("wiki/nota.md", "# Nota\n\nhola\n".as_bytes());
    a.escribir("wiki/sub/ñandú.md", "# Ñandú\n".as_bytes());
    a.escribir("raw/imagen.png", &[0x89, b'P', b'N', b'G', 0, 1, 2, 3]);
    a.escribir(".orbital/listas.yaml", b"favoritos: []\n");
    a.escribir(".orbital/historial/wiki/nota.md/x.txt", b"no se sube");
    a.escribir(".git/config", b"no se sube");
    a.escribir("wiki/privado/secreto.md", b"solo en este equipo");
    a.conectar(srv, Destino::Nueva("Bóveda".into()));
    let mut c = Conexion::leer(&a.root).unwrap().unwrap();
    c.poner_local("wiki/privado/", true);
    c.guardar(&a.root).unwrap();
    a.sincronizar();
    let id = carpeta(&mut a);
    let mut b = Equipo::new("b", srv);
    b.conectar(
        srv,
        Destino::Existente(format!(
            "https://drive.google.com/drive/folders/{id}?usp=sharing"
        )),
    );
    b.sincronizar();
    (a, b, id)
}

#[test]
fn sube_todo_y_otro_equipo_lo_baja() {
    let srv = ServidorFalso::iniciar();
    let (a, b, id) = dos_equipos(&srv);
    let remoto = srv.datos().arbol(&id);
    assert!(remoto.contains_key("wiki/nota.md"));
    assert!(remoto.contains_key("wiki/sub/ñandú.md"));
    assert!(remoto.contains_key("raw/imagen.png"));
    assert!(remoto.contains_key(".orbital/listas.yaml"));
    assert!(remoto.contains_key(".orbital/drive.yaml"));
    assert!(
        !remoto
            .keys()
            .any(|k| k.contains("historial") || k.starts_with(".git"))
    );
    assert!(
        !remoto.contains_key("wiki/privado/secreto.md"),
        "lo «solo en este equipo» no se sube"
    );
    // B tiene lo mismo que Drive, y la regla compartida (no subiría su propia carpeta privada).
    assert_eq!(b.arbol(), remoto);
    assert!(
        Conexion::leer(&b.root)
            .unwrap()
            .unwrap()
            .es_local("wiki/privado/x.md")
    );
    // La carpeta de Drive quedó dentro de «Mi unidad».
    assert_eq!(srv.datos().archivos[&id].padres, vec![RAIZ.to_owned()]);
    drop(a);
}

#[test]
fn cambios_cruzados_entre_dos_equipos() {
    let srv = ServidorFalso::iniciar();
    let (mut a, mut b, _) = dos_equipos(&srv);
    a.escribir("wiki/nota.md", b"# Nota\n\ncambiada en A\n");
    a.escribir("wiki/nueva.md", b"nueva en A\n");
    a.sincronizar();
    b.sincronizar();
    assert_eq!(
        b.leer("wiki/nota.md").unwrap(),
        b"# Nota\n\ncambiada en A\n"
    );
    assert_eq!(b.leer("wiki/nueva.md").unwrap(), b"nueva en A\n");
    // La versión reemplazada en B quedó en su historial.
    assert!(b.todo().iter().any(|v| v == b"# Nota\n\nhola\n"));
    std::fs::remove_file(b.root.join("wiki/nueva.md")).unwrap();
    b.sincronizar();
    a.sincronizar();
    assert!(a.leer("wiki/nueva.md").is_none(), "el borrado llega a A");
    assert!(
        a.todo().iter().any(|v| v == b"nueva en A\n"),
        "y lo borrado quedó en la papelera"
    );
}

#[test]
fn la_web_edita_renombra_mueve_y_borra() {
    let srv = ServidorFalso::iniciar();
    let (mut a, _b, id) = dos_equipos(&srv);
    let (nota, img, sub) = {
        let d = srv.datos();
        (
            d.buscar("wiki/nota.md", &id).unwrap(),
            d.buscar("raw/imagen.png", &id).unwrap(),
            d.buscar("wiki/sub", &id).unwrap(),
        )
    };
    {
        let mut d = srv.datos();
        d.editar(&nota, b"# Nota\n\ndesde el celular\n", "Celular");
        d.renombrar(&nota, "nota renombrada.md");
        d.mover(&img, &sub);
        let otra = d.buscar("wiki/sub/ñandú.md", &id).unwrap();
        d.a_papelera(&otra);
        d.crear(&sub, "de la web.md", b"creada en la web\n", "Web");
    }
    a.sincronizar();
    assert!(a.leer("wiki/nota.md").is_none());
    assert_eq!(
        a.leer("wiki/nota renombrada.md").unwrap(),
        b"# Nota\n\ndesde el celular\n"
    );
    assert!(a.leer("raw/imagen.png").is_none());
    assert_eq!(a.leer("wiki/sub/imagen.png").unwrap()[0], 0x89);
    assert!(a.leer("wiki/sub/ñandú.md").is_none());
    assert!(
        a.todo().iter().any(|v| v == "# Ñandú\n".as_bytes()),
        "a la papelera, no borrado"
    );
    assert_eq!(
        a.leer("wiki/sub/de la web.md").unwrap(),
        b"creada en la web\n"
    );
    // El renombre no subió nada nuevo: el id sigue siendo el mismo.
    assert_eq!(
        srv.datos()
            .buscar("wiki/nota renombrada.md", &id)
            .as_deref(),
        Some(nota.as_str())
    );
}

#[test]
fn renombrar_y_mover_en_el_disco_conserva_el_id() {
    let srv = ServidorFalso::iniciar();
    let (mut a, mut b, id) = dos_equipos(&srv);
    let nota = srv.datos().buscar("wiki/nota.md", &id).unwrap();
    let antes = srv.datos().archivos.len();
    std::fs::create_dir_all(a.root.join("wiki/otra")).unwrap();
    std::fs::rename(
        a.root.join("wiki/nota.md"),
        a.root.join("wiki/otra/nota movida.md"),
    )
    .unwrap();
    a.sincronizar();
    let d = srv.datos();
    assert_eq!(
        d.buscar("wiki/otra/nota movida.md", &id).as_deref(),
        Some(nota.as_str())
    );
    // Solo se creó la carpeta `otra`.
    assert_eq!(d.archivos.len(), antes + 1);
    drop(d);
    b.sincronizar();
    assert!(b.leer("wiki/nota.md").is_none());
    assert_eq!(
        b.leer("wiki/otra/nota movida.md").unwrap(),
        b"# Nota\n\nhola\n"
    );
}

#[test]
fn conflicto_conserva_las_dos_y_se_resuelve() {
    let srv = ServidorFalso::iniciar();
    let (mut a, mut b, id) = dos_equipos(&srv);
    a.escribir("wiki/nota.md", b"# Nota\n\nhola desde A\n");
    b.escribir("wiki/nota.md", b"# Nota\n\nhola desde B\n");
    a.sincronizar();
    b.sincronizar();
    // B no pisó a A: Drive tiene la de A, B conserva la suya y guardó la de Drive aparte.
    let nota = srv.datos().buscar("wiki/nota.md", &id).unwrap();
    assert_eq!(
        srv.datos().archivos[&nota].contenido,
        b"# Nota\n\nhola desde A\n"
    );
    assert_eq!(b.leer("wiki/nota.md").unwrap(), b"# Nota\n\nhola desde B\n");
    assert_eq!(b.conflictos(), vec!["wiki/nota.md".to_owned()]);
    let copia = b.s().estado.conflictos["wiki/nota.md"].copia.clone();
    assert!(copia.starts_with(".orbital/conflictos/wiki/nota.md/"));
    assert_eq!(b.leer(&copia).unwrap(), b"# Nota\n\nhola desde A\n");
    assert!(
        b.eventos
            .lock()
            .unwrap()
            .iter()
            .any(|e| matches!(e, orbital_drive::Evento::Conflicto(r) if r == "wiki/nota.md"))
    );
    // Mientras dura el conflicto no se sube nada encima.
    b.escribir("wiki/nota.md", b"# Nota\n\nhola desde B, otra vez\n");
    b.sincronizar();
    assert_eq!(
        srv.datos().archivos[&nota].contenido,
        b"# Nota\n\nhola desde A\n"
    );
    // «Conservar la mía»: sube la de B; la de A queda en el historial de B.
    b.s().resolver("wiki/nota.md", true).unwrap();
    b.sincronizar();
    assert_eq!(
        srv.datos().archivos[&nota].contenido,
        b"# Nota\n\nhola desde B, otra vez\n"
    );
    assert!(b.todo().iter().any(|v| v == b"# Nota\n\nhola desde A\n"));
    assert!(b.conflictos().is_empty());
    a.sincronizar();
    assert_eq!(
        a.leer("wiki/nota.md").unwrap(),
        b"# Nota\n\nhola desde B, otra vez\n"
    );

    // Otro conflicto, binario, resuelto con «Usar la de Drive».
    a.escribir("raw/imagen.png", b"\x89PNG A");
    b.escribir("raw/imagen.png", b"\x89PNG B");
    a.sincronizar();
    b.sincronizar();
    assert_eq!(b.conflictos(), vec!["raw/imagen.png".to_owned()]);
    b.s().resolver("raw/imagen.png", false).unwrap();
    b.sincronizar();
    assert_eq!(b.leer("raw/imagen.png").unwrap(), b"\x89PNG A");
    assert!(
        b.todo().iter().any(|v| v == b"\x89PNG B"),
        "la de B quedó en el respaldo"
    );
}

#[test]
fn fusiona_cuando_los_cambios_no_se_pisan() {
    let srv = ServidorFalso::iniciar();
    let (mut a, mut b, id) = dos_equipos(&srv);
    let base = "# Nota\n\nprincipio\n\nmedio\n\nfinal\n";
    a.escribir("wiki/nota.md", base.as_bytes());
    a.sincronizar();
    b.sincronizar();
    a.escribir(
        "wiki/nota.md",
        b"# Nota\n\nPRINCIPIO (A)\n\nmedio\n\nfinal\n",
    );
    b.escribir(
        "wiki/nota.md",
        b"# Nota\n\nprincipio\n\nmedio\n\nFINAL (B)\n",
    );
    a.sincronizar();
    b.sincronizar();
    let unida = b"# Nota\n\nPRINCIPIO (A)\n\nmedio\n\nFINAL (B)\n";
    assert_eq!(b.leer("wiki/nota.md").unwrap(), unida);
    assert!(b.conflictos().is_empty());
    let nota = srv.datos().buscar("wiki/nota.md", &id).unwrap();
    assert_eq!(srv.datos().archivos[&nota].contenido, unida);
    assert!(
        b.eventos
            .lock()
            .unwrap()
            .iter()
            .any(|e| matches!(e, orbital_drive::Evento::Fusionada(_)))
    );
    // Las dos originales en el historial de B.
    let todo = b.todo();
    assert!(
        todo.iter()
            .any(|v| v == b"# Nota\n\nprincipio\n\nmedio\n\nFINAL (B)\n")
    );
    assert!(
        todo.iter()
            .any(|v| v == b"# Nota\n\nPRINCIPIO (A)\n\nmedio\n\nfinal\n")
    );
    a.sincronizar();
    assert_eq!(a.leer("wiki/nota.md").unwrap(), unida);
}

#[test]
fn borrado_en_drive_con_cambios_aca_se_vuelve_a_subir() {
    let srv = ServidorFalso::iniciar();
    let (mut a, _b, id) = dos_equipos(&srv);
    let nota = srv.datos().buscar("wiki/nota.md", &id).unwrap();
    srv.datos().a_papelera(&nota);
    a.escribir("wiki/nota.md", b"# Nota\n\ncambio que no se pierde\n");
    a.sincronizar();
    let nueva = srv.datos().buscar("wiki/nota.md", &id).unwrap();
    assert_ne!(nueva, nota);
    assert_eq!(
        srv.datos().archivos[&nueva].contenido,
        b"# Nota\n\ncambio que no se pierde\n"
    );
}

#[test]
fn subida_grande_se_reanuda_tras_un_corte_y_un_reinicio() {
    let srv = ServidorFalso::iniciar();
    let (mut a, mut b, id) = dos_equipos(&srv);
    let tam = 6 * 1024 * 1024 + 1234;
    let grande: Vec<u8> = (0..tam).map(|i| (i * 31 % 251) as u8).collect();
    a.escribir("raw/video.bin", &grande);
    a.s().api().tramo = 1024 * 1024;
    let antes = srv.datos().bytes_subidos;
    // El primer tramo se corta a los 700 KiB (Drive conserva 512 KiB: múltiplo de 256 KiB).
    srv.datos().fallar(Falla::CortarSubida(700 * 1024));
    assert!(matches!(a.ciclo(), Err(orbital_drive::ErrorApi::SinRed(_))));
    // Reinicio: el estado guardado tiene la sesión de subida; la nueva instancia sigue desde ahí.
    let guardado = orbital_drive::Estado::leer(&a.dir_estado).unwrap().unwrap();
    assert!(
        guardado.subidas.contains_key("raw/video.bin"),
        "la sesión quedó en el estado"
    );
    a.arrancar(&srv);
    a.s().api().tramo = 1024 * 1024;
    a.sincronizar();
    let v = srv.datos().buscar("raw/video.bin", &id).unwrap();
    assert_eq!(srv.datos().archivos[&v].contenido, grande);
    let subidos = srv.datos().bytes_subidos - antes;
    assert!(
        subidos < (tam + 300 * 1024) as u64,
        "se reanudó desde el último tramo confirmado ({subidos} bytes para {tam})"
    );
    b.sincronizar();
    assert_eq!(b.leer("raw/video.bin").unwrap(), grande);
}

#[test]
fn bajada_cortada_no_deja_archivos_a_medias() {
    let srv = ServidorFalso::iniciar();
    let (_a, mut b, id) = dos_equipos(&srv);
    let sub = srv.datos().buscar("wiki/sub", &id).unwrap();
    let contenido: Vec<u8> = (0..300_000u32).map(|i| (i % 253) as u8).collect();
    srv.datos().crear(&sub, "pesado.bin", &contenido, "Web");
    srv.datos().fallar(Falla::Error(500, "backendError"));
    assert!(b.ciclo().is_err());
    srv.datos().fallar(Falla::CortarBajada(100_000));
    let _ = b.ciclo();
    assert!(
        b.leer("wiki/sub/pesado.bin").is_none(),
        "nada a medias en la bóveda"
    );
    b.sincronizar();
    assert_eq!(b.leer("wiki/sub/pesado.bin").unwrap(), contenido);
}

#[test]
fn sin_red_no_se_pierde_la_cola() {
    let srv = ServidorFalso::iniciar();
    let (mut a, mut b, _) = dos_equipos(&srv);
    srv.datos().caido = true;
    a.escribir("wiki/sin red.md", b"escrita sin red\n");
    assert!(matches!(a.ciclo(), Err(orbital_drive::ErrorApi::SinRed(_))));
    assert!(a.ciclo().is_err());
    srv.datos().caido = false;
    a.sincronizar();
    b.sincronizar();
    assert_eq!(b.leer("wiki/sin red.md").unwrap(), b"escrita sin red\n");
}

#[test]
fn cuota_llena_pausa_las_subidas_y_avisa() {
    let srv = ServidorFalso::iniciar();
    let (mut a, _b, id) = dos_equipos(&srv);
    let usado: u64 = srv
        .datos()
        .archivos
        .values()
        .map(|x| x.contenido.len() as u64)
        .sum();
    srv.datos().cuota = Some(usado + 10);
    a.escribir("wiki/no entra.md", &[b'x'; 100]);
    a.sincronizar();
    assert!(srv.datos().buscar("wiki/no entra.md", &id).is_none());
    assert!(
        a.eventos
            .lock()
            .unwrap()
            .iter()
            .any(|e| matches!(e, orbital_drive::Evento::Aviso(t) if t.contains("lleno")))
    );
    srv.datos().cuota = None;
    a.sincronizar();
    assert!(srv.datos().buscar("wiki/no entra.md", &id).is_some());
}

#[test]
fn nombres_repetidos_en_drive_no_chocan_en_el_disco() {
    let srv = ServidorFalso::iniciar();
    let (_a, mut b, id) = dos_equipos(&srv);
    let wiki = srv.datos().buscar("wiki", &id).unwrap();
    srv.datos()
        .crear(&wiki, "nota.md", b"otra con el mismo nombre\n", "Web");
    srv.datos().crear(&wiki, "a/b.md", b"con barra\n", "Web");
    b.sincronizar();
    let mut contenidos: Vec<Vec<u8>> = ["wiki/nota.md", "wiki/nota (2).md"]
        .iter()
        .map(|r| b.leer(r).unwrap())
        .collect();
    contenidos.sort();
    assert_eq!(
        contenidos,
        vec![
            b"# Nota\n\nhola\n".to_vec(),
            b"otra con el mismo nombre\n".to_vec()
        ]
    );
    assert_eq!(b.leer("wiki/a∕b.md").unwrap(), b"con barra\n");
}

#[test]
fn primera_union_con_contenido_en_los_dos_lados() {
    let srv = ServidorFalso::iniciar();
    let (_a, _b, id) = dos_equipos(&srv);
    let mut c = Equipo::new("c", &srv);
    c.escribir("wiki/nota.md", b"plantilla de este equipo\n");
    c.escribir("wiki/solo en C.md", b"de C\n");
    c.conectar(&srv, Destino::Existente(id.clone()));
    c.sincronizar();
    // En las notas de texto gana Drive; la de C, al historial.
    assert_eq!(c.leer("wiki/nota.md").unwrap(), b"# Nota\n\nhola\n");
    assert!(c.todo().iter().any(|v| v == b"plantilla de este equipo\n"));
    assert!(srv.datos().buscar("wiki/solo en C.md", &id).is_some());
}
