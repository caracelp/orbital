//! Arma una bóveda de prueba conectada al **servidor falso** de Drive y lo deja corriendo, para
//! probar Orbital a mano o sacar capturas sin ninguna cuenta real:
//!
//! ```sh
//! export XDG_STATE_HOME=$(mktemp -d) ORBITAL_DRIVE_LLAVERO=archivo:$(mktemp -u).json
//! cargo run -p orbital-drive --features servidor-falso --example preparar-prueba -- /tmp/boveda-drive &
//! # copiar la línea ORBITAL_DRIVE_API=… que imprime y abrir Orbital con esas variables:
//! XDG_CONFIG_HOME=$(mktemp -d) ORBITAL_INSTANCIA_PROPIA=1 ORBITAL_DRIVE_API=… orbital /tmp/boveda-drive
//! ```
//!
//! Deja: notas sincronizadas, una carpeta «solo en este equipo», un conflicto (`wiki/Proyecto.md`)
//! y una nota nueva sin subir (la sube Orbital al abrir).

use std::fs;
use std::path::PathBuf;

use orbital_drive::cliente::{Cliente, Credenciales, Direcciones};
use orbital_drive::estado::carpeta_estado;
use orbital_drive::falso::{CUENTA, ServidorFalso};
use orbital_drive::llavero::{self, Llavero};
use orbital_drive::motor::{Config, Destino, Sincronizador, conectar};
use orbital_drive::{Conexion, Estado, Papelera};

fn main() {
    let root = PathBuf::from(
        std::env::args()
            .nth(1)
            .expect("carpeta de la bóveda de prueba"),
    );
    // Nunca una bóveda real: ni `~/Documentos` ni las carpetas de `ORBITAL_BOVEDAS_PROTEGIDAS`.
    let mut protegidas: Vec<PathBuf> = std::env::var_os("HOME")
        .filter(|h| !h.is_empty())
        .map(|h| PathBuf::from(h).join("Documentos"))
        .into_iter()
        .collect();
    if let Some(extra) = std::env::var_os("ORBITAL_BOVEDAS_PROTEGIDAS") {
        protegidas.extend(std::env::split_paths(&extra).filter(|p| !p.as_os_str().is_empty()));
    }
    let absoluta = std::path::absolute(&root).unwrap_or_else(|_| root.clone());
    assert!(
        !protegidas.iter().any(|p| absoluta.starts_with(p)),
        "nunca una bóveda real (carpeta protegida)"
    );
    let _ = fs::remove_dir_all(&root);
    let escribir = |r: &str, t: &[u8]| {
        let p = root.join(r);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, t).unwrap();
    };
    escribir(
        "wiki/index.md",
        b"# \xc3\x8dndice\n\n- [[Proyecto]]\n- [[Lecturas/Libro]]\n",
    );
    escribir("wiki/log.md", b"# Registro\n");
    escribir(
        "wiki/Proyecto.md",
        "# Proyecto\n\nObjetivo del trimestre.\n\n## Tareas\n\n- Escribir el informe\n".as_bytes(),
    );
    escribir(
        "wiki/Lecturas/Libro.md",
        "# Libro\n\nNotas de lectura.\n".as_bytes(),
    );
    escribir("wiki/Lecturas/Artículo.md", "# Artículo\n".as_bytes());
    escribir(
        "wiki/privado/Diario íntimo.md",
        "# Solo en este equipo\n".as_bytes(),
    );
    escribir("raw/foto.png", &[0x89, b'P', b'N', b'G', 13, 10, 26, 10]);
    escribir(".orbital/config.yaml", b"format_version: 1\n");

    let srv = ServidorFalso::iniciar();
    let dir_estado = carpeta_estado(&root);
    let _ = fs::remove_dir_all(&dir_estado);
    let cliente = || {
        Cliente::new(
            Direcciones::locales(&srv.url),
            Credenciales {
                client_id: "id-falso".into(),
                client_secret: "secreto-falso".into(),
                refresh_token: "falso-renovar".into(),
            },
        )
    };
    let mut c = cliente();
    conectar(
        &mut c,
        &root,
        &dir_estado,
        CUENTA,
        Destino::Nueva("Bóveda de prueba".into()),
    )
    .unwrap();
    let mut con = Conexion::leer(&root).unwrap().unwrap();
    con.poner_local("wiki/privado/", true);
    con.guardar(&root).unwrap();
    let cfg = Config {
        papelera: Papelera::Boveda,
        dir_estado: dir_estado.clone(),
        ..Config::new(&root)
    };
    let mut s = Sincronizador::new(
        cfg,
        cliente(),
        Estado::leer(&dir_estado).unwrap().unwrap(),
        con,
        Box::new(|_| {}),
    );
    for _ in 0..3 {
        s.pedir_completa();
        s.ciclo().unwrap();
    }
    // Un conflicto: la misma nota cambiada acá y «desde el celular».
    let raiz = s.estado.raiz.clone();
    let id = srv.datos().buscar("wiki/Proyecto.md", &raiz).unwrap();
    srv.datos().editar(
        &id,
        "# Proyecto\n\nObjetivo del trimestre (cambiado desde el celular).\n\n## Tareas\n\n- Escribir el informe\n- Llamar a Ana\n".as_bytes(),
        "Ana (celular)",
    );
    std::thread::sleep(std::time::Duration::from_millis(5));
    escribir(
        "wiki/Proyecto.md",
        "# Proyecto\n\nObjetivo del trimestre (cambiado en la notebook).\n\n## Tareas\n\n- Escribir el informe\n".as_bytes(),
    );
    s.pedir_completa();
    s.ciclo().unwrap();
    assert_eq!(s.estado.conflictos.len(), 1, "quedó el conflicto");
    drop(s);
    // Una nota nueva sin subir: la sube Orbital al abrir.
    escribir("wiki/Lecturas/Nueva.md", "# Nueva\n".as_bytes());

    // Credenciales falsas en el llavero de archivo (nunca en el del sistema).
    match std::env::var("ORBITAL_DRIVE_LLAVERO") {
        Ok(v) if v.starts_with("archivo:") => {
            let l = llavero::Archivo(PathBuf::from(&v["archivo:".len()..]));
            llavero::guardar_cliente(
                &l,
                &llavero::Cliente {
                    id: "id-falso.apps.googleusercontent.com".into(),
                    secreto: "secreto-falso".into(),
                },
            )
            .unwrap();
            llavero::guardar_cuenta(&l, CUENTA, "falso-renovar").unwrap();
            let _ = l.leer("cliente");
        }
        _ => panic!("falta ORBITAL_DRIVE_LLAVERO=archivo:<ruta> (nunca el llavero del sistema)"),
    }
    println!("ORBITAL_DRIVE_API={}", srv.url);
    println!("Bóveda: {}", root.display());
    println!("Estado del equipo: {}", dir_estado.display());
    println!("Servidor falso corriendo (Ctrl+C para terminar).");
    loop {
        std::thread::park();
    }
}
