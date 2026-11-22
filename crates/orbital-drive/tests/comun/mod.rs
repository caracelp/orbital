//! Utilidades de las pruebas: equipos (bóvedas temporales con su estado) contra el servidor falso.
#![allow(dead_code)]

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use orbital_drive::cliente::{Cliente, Credenciales, Direcciones};
use orbital_drive::falso::{CUENTA, ServidorFalso};
use orbital_drive::motor::{Config, Destino, Sincronizador, conectar};
use orbital_drive::{Estado, Evento, Papelera, reglas::Conexion};

pub fn carpeta_temporal(nombre: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!(
        "orbital-061-{}-{}-{nombre}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&d).unwrap();
    d
}

pub fn cliente(srv: &ServidorFalso) -> Cliente {
    cliente_de(&srv.url)
}

pub fn cliente_de(url: &str) -> Cliente {
    Cliente::new(
        Direcciones::locales(url),
        Credenciales {
            client_id: "id-falso".into(),
            client_secret: "secreto-falso".into(),
            refresh_token: "falso-renovar".into(),
        },
    )
}

/// Tamaños chicos para que la prueba de caos ejercite las subidas reanudables.
pub static SUBIDA_CHICA: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub struct Equipo {
    pub root: PathBuf,
    pub dir_estado: PathBuf,
    pub sinc: Option<Sincronizador<Cliente>>,
    pub eventos: Arc<Mutex<Vec<Evento>>>,
    url: String,
}

impl Equipo {
    pub fn new(nombre: &str, srv: &ServidorFalso) -> Self {
        let base = carpeta_temporal(nombre);
        let root = base.join("boveda");
        fs::create_dir_all(&root).unwrap();
        Self {
            dir_estado: base.join("estado"),
            root,
            sinc: None,
            eventos: Arc::default(),
            url: srv.url.clone(),
        }
    }

    pub fn config(&self) -> Config {
        Config {
            papelera: Papelera::Boveda,
            dir_estado: self.dir_estado.clone(),
            ..Config::new(&self.root)
        }
    }

    pub fn conectar(&mut self, srv: &ServidorFalso, destino: Destino) {
        let mut c = cliente(srv);
        conectar(&mut c, &self.root, &self.dir_estado, CUENTA, destino).unwrap();
        self.arrancar(srv);
    }

    /// (Re)crea el sincronizador desde el estado guardado (como un reinicio de Orbital).
    pub fn arrancar(&mut self, srv: &ServidorFalso) {
        let estado = Estado::leer(&self.dir_estado).unwrap().unwrap();
        let conexion = Conexion::leer(&self.root).unwrap().unwrap();
        let ev = self.eventos.clone();
        let mut c = cliente(srv);
        if SUBIDA_CHICA.load(std::sync::atomic::Ordering::Relaxed) {
            c.limite_simple = 1024;
            c.tramo = 2048;
        }
        self.sinc = Some(Sincronizador::new(
            self.config(),
            c,
            estado,
            conexion,
            Box::new(move |e| ev.lock().unwrap().push(e)),
        ));
    }

    pub fn s(&mut self) -> &mut Sincronizador<Cliente> {
        self.sinc.as_mut().unwrap()
    }

    pub fn ciclo(&mut self) -> Result<(), orbital_drive::ErrorApi> {
        self.s().pedir_completa();
        self.s().ciclo()
    }

    /// Ciclos hasta que no quede nada por hacer (como mucho `n`).
    pub fn sincronizar(&mut self) {
        for _ in 0..4 {
            self.ciclo().unwrap();
        }
    }

    pub fn escribir(&self, rel: &str, contenido: &[u8]) {
        let p = self.root.join(rel);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        // Garantiza otra fecha de modificación aunque el sistema tenga poca resolución.
        std::thread::sleep(Duration::from_millis(2));
        fs::write(p, contenido).unwrap();
    }

    pub fn leer(&self, rel: &str) -> Option<Vec<u8>> {
        fs::read(self.root.join(rel)).ok()
    }

    /// Archivos que se sincronizan (sin `.orbital/` salvo la configuración compartida).
    pub fn arbol(&self) -> BTreeMap<String, Vec<u8>> {
        let mut out = BTreeMap::new();
        recorrer(&self.root, &self.root, &mut out, false);
        out
    }

    /// Todo lo que hay en la bóveda, historial, conflictos y papelera incluidos.
    pub fn todo(&self) -> Vec<Vec<u8>> {
        let mut out = BTreeMap::new();
        recorrer(&self.root, &self.root, &mut out, true);
        out.into_values().collect()
    }

    pub fn conflictos(&mut self) -> Vec<String> {
        self.s().estado.conflictos.keys().cloned().collect()
    }
}

fn recorrer(root: &Path, dir: &Path, out: &mut BTreeMap<String, Vec<u8>>, todo: bool) {
    let Ok(rd) = fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        let rel = p.strip_prefix(root).unwrap().to_string_lossy().into_owned();
        if !todo && !orbital_drive::reglas::sincronizable(&rel) && rel != ".orbital" {
            continue;
        }
        if p.is_dir() {
            if !todo && rel == ".orbital" {
                for n in orbital_drive::reglas::ORBITAL_COMPARTIDOS {
                    if let Ok(b) = fs::read(p.join(n)) {
                        out.insert(format!(".orbital/{n}"), b);
                    }
                }
                continue;
            }
            if todo && rel == ".orbital/sincronizacion" {
                continue;
            }
            recorrer(root, &p, out, todo);
        } else if let Ok(b) = fs::read(&p) {
            out.insert(rel, b);
        }
    }
}

/// Generador de azar fijo (xorshift) para la prueba de caos.
pub struct Azar(pub u64);

impl Azar {
    pub fn sig(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }
    pub fn hasta(&mut self, n: usize) -> usize {
        (self.sig() % n as u64) as usize
    }
}
