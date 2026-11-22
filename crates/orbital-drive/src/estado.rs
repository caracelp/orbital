//! Estado de sincronización de **un equipo** (FR-014), fuera de la bóveda:
//! `$XDG_STATE_HOME/orbital/drive/<id>/estado.json`, más `bases/` con la última versión acordada
//! de cada nota de texto (para la fusión de tres vías). Se escribe de forma atómica y con un
//! candado de archivo (dos Orbital con la misma bóveda no se pisan).

use std::collections::{BTreeMap, HashMap};
use std::fs::{self, File};
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::Digest;

use crate::api::{Archivo, SesionSubida};

pub const VERSION: u32 = 1;

/// Un archivo o carpeta de Drive que está dentro de la bóveda (sin la raíz).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Nodo {
    pub nombre: String,
    pub padre: String,
    #[serde(default)]
    pub carpeta: bool,
    #[serde(default)]
    pub md5: Option<String>,
    #[serde(default)]
    pub tam: u64,
    #[serde(default)]
    pub editable: bool,
    #[serde(default)]
    pub enlace: Option<String>,
    #[serde(default)]
    pub autor: Option<String>,
    #[serde(default)]
    pub modificado: Option<String>,
    #[serde(default)]
    pub nativo: bool,
}

impl Nodo {
    pub fn de(a: &Archivo, padre: &str) -> Self {
        Self {
            nombre: a.nombre.clone(),
            padre: padre.to_owned(),
            carpeta: a.carpeta(),
            md5: a.md5.clone(),
            tam: a.tam,
            editable: a.editable,
            enlace: a.enlace.clone(),
            autor: a.autor.clone(),
            modificado: a.modificado.clone(),
            nativo: a.nativo(),
        }
    }
}

/// Lo último en que este equipo y Drive estuvieron de acuerdo para una ruta.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Registro {
    pub id: String,
    #[serde(default)]
    pub carpeta: bool,
    #[serde(default)]
    pub md5: Option<String>,
    #[serde(default)]
    pub tam: u64,
    #[serde(default)]
    pub mtime: i128,
}

/// Un conflicto abierto: la versión de Drive guardada aparte.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Conflicto {
    pub id: String,
    pub md5: Option<String>,
    /// Ruta relativa a la bóveda de la copia (`.orbital/conflictos/...`).
    pub copia: String,
    #[serde(default)]
    pub autor: Option<String>,
    #[serde(default)]
    pub fecha: String,
}

/// Todo el estado de un equipo para una bóveda.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Estado {
    pub version: u32,
    pub cuenta: String,
    /// Id de la carpeta raíz en Drive.
    pub raiz: String,
    /// `pageToken` de la lista de cambios (vacío: falta el primer listado).
    #[serde(default)]
    pub token: Option<String>,
    #[serde(default)]
    pub nodos: HashMap<String, Nodo>,
    #[serde(default)]
    pub registros: BTreeMap<String, Registro>,
    #[serde(default)]
    pub conflictos: BTreeMap<String, Conflicto>,
    /// Subidas reanudables en curso, por ruta.
    #[serde(default)]
    pub subidas: BTreeMap<String, SesionSubida>,
    /// Conectada a una carpeta que ya tenía contenido y todavía sin la primera conciliación.
    #[serde(default)]
    pub primera_union: bool,
}

/// Carpeta del estado de una bóveda en este equipo.
pub fn carpeta_estado(root: &Path) -> PathBuf {
    let base = std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/state")))
        .unwrap_or_else(std::env::temp_dir);
    let canon = fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
    let h = sha2::Sha256::digest(canon.as_os_str().as_encoded_bytes());
    let id: String = h[..8].iter().map(|b| format!("{b:02x}")).collect();
    base.join("orbital/drive").join(id)
}

impl Estado {
    pub fn nuevo(cuenta: &str, raiz: &str) -> Self {
        Self {
            version: VERSION,
            cuenta: cuenta.to_owned(),
            raiz: raiz.to_owned(),
            ..Default::default()
        }
    }

    pub fn leer(dir: &Path) -> io::Result<Option<Self>> {
        match fs::read(dir.join("estado.json")) {
            Ok(b) => serde_json::from_slice(&b)
                .map(Some)
                .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e)),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e),
        }
    }

    pub fn guardar(&self, dir: &Path) -> io::Result<()> {
        fs::create_dir_all(dir)?;
        let b = serde_json::to_vec(self).map_err(io::Error::other)?;
        orbital_vault::write::atomic_write(&dir.join("estado.json"), &b).map(|_| ())
    }

    /// Ruta relativa de cada nodo (las de las carpetas también), bajando desde la raíz. Los nodos
    /// que no llegan a la raíz no tienen ruta. Dos nombres iguales en una carpeta (Drive lo
    /// permite, el disco no) se distinguen con ` (2)`, ` (3)`… en orden de id.
    pub fn rutas(&self) -> HashMap<String, String> {
        let mut hijos: HashMap<&str, Vec<&str>> = HashMap::new();
        for (id, n) in &self.nodos {
            hijos.entry(n.padre.as_str()).or_default().push(id.as_str());
        }
        let mut rutas: HashMap<String, String> = HashMap::with_capacity(self.nodos.len());
        let mut cola: Vec<(&str, String)> = vec![(self.raiz.as_str(), String::new())];
        while let Some((padre, prefijo)) = cola.pop() {
            let Some(lista) = hijos.get_mut(padre) else {
                continue;
            };
            lista.sort_by(|a, b| {
                self.nodos[*a]
                    .nombre
                    .cmp(&self.nodos[*b].nombre)
                    .then(a.cmp(b))
            });
            let mut usados = std::collections::HashSet::new();
            for id in lista.iter() {
                let n = &self.nodos[*id];
                let base = crate::local::nombre_local(&n.nombre);
                let mut nombre = base.clone();
                let mut k = 2;
                while !usados.insert(nombre.clone()) {
                    nombre = con_numero(&base, k, n.carpeta);
                    k += 1;
                }
                let r = if prefijo.is_empty() {
                    nombre
                } else {
                    format!("{prefijo}/{nombre}")
                };
                if n.carpeta {
                    cola.push((id, r.clone()));
                }
                rutas.insert((*id).to_owned(), r);
            }
        }
        rutas
    }

    /// Saca los nodos que ya no cuelgan de la raíz (movidos fuera de la bóveda).
    pub fn podar(&mut self) {
        let rutas = self.rutas();
        self.nodos.retain(|id, _| rutas.contains_key(id));
    }
}

/// `nombre (k).ext` (en carpetas, `nombre (k)`).
fn con_numero(nombre: &str, k: usize, carpeta: bool) -> String {
    match nombre.rsplit_once('.') {
        Some((tallo, ext)) if !carpeta && !tallo.is_empty() => format!("{tallo} ({k}).{ext}"),
        _ => format!("{nombre} ({k})"),
    }
}

/// Candado del estado: mientras viva, ningún otro proceso usa este estado.
pub struct Candado(#[allow(dead_code)] File);

impl Candado {
    pub fn tomar(dir: &Path) -> io::Result<Self> {
        fs::create_dir_all(dir)?;
        let f = File::create(dir.join("candado"))?;
        f.try_lock().map_err(|_| {
            io::Error::new(
                io::ErrorKind::WouldBlock,
                "otro Orbital está sincronizando esta bóveda",
            )
        })?;
        Ok(Self(f))
    }
}

/// Bases de la fusión: la última versión acordada de cada nota de texto, por id de Drive.
pub fn ruta_base(dir: &Path, id: &str) -> PathBuf {
    let limpio: String = id
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    dir.join("bases").join(limpio)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn nodo(nombre: &str, padre: &str, carpeta: bool) -> Nodo {
        Nodo {
            nombre: nombre.into(),
            padre: padre.into(),
            carpeta,
            ..Default::default()
        }
    }

    #[test]
    fn rutas_y_poda() {
        let mut e = Estado::nuevo("a@b", "R");
        e.nodos.insert("w".into(), nodo("wiki", "R", true));
        e.nodos.insert("n".into(), nodo("nota.md", "w", false));
        e.nodos
            .insert("x".into(), nodo("huerfano.md", "fuera", false));
        e.nodos.insert("m".into(), nodo("nota.md", "w", false));
        let r = e.rutas();
        assert_eq!(r["m"], "wiki/nota.md");
        assert_eq!(r["n"], "wiki/nota (2).md");
        assert!(!r.contains_key("x"));
        e.podar();
        assert!(!e.nodos.contains_key("x"));
        let dir = std::env::temp_dir().join(format!("orbital-drive-estado-{}", std::process::id()));
        e.guardar(&dir).unwrap();
        let l = Estado::leer(&dir).unwrap().unwrap();
        assert_eq!(l.nodos, e.nodos);
        let _c = Candado::tomar(&dir).unwrap();
        assert!(Candado::tomar(&dir).is_err());
        fs::remove_dir_all(&dir).unwrap();
    }
}
