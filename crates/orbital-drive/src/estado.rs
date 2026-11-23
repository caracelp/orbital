//! Estado de sincronización de **un equipo** (FR-014), fuera de la bóveda:
//! `$XDG_STATE_HOME/orbital/drive/<id>/estado.txt`, más `bases/` con la última versión acordada
//! de cada nota de texto (para la fusión de tres vías) y `marcadores` (lo «solo en Drive», que la
//! app lee al escanear la bóveda, sin red). Se escribe de forma atómica y con un candado de
//! archivo (dos Orbital con la misma bóveda no se pisan).
//!
//! El formato es de líneas con campos separados por tabuladores y escrito a mano (061b): el
//! `derive` de serde que usaba la 061a sumaba código al binario (ver el quickstart). El
//! `estado.json` de la 061a se lee una vez y se reemplaza.

use std::collections::{BTreeMap, HashMap};
use std::fs::{self, File};
use std::io;
use std::path::{Path, PathBuf};

use sha2::Digest;

use crate::api::{Archivo, SesionSubida};
use crate::reglas::Ubicacion;

pub const VERSION: u32 = 2;

/// Un archivo o carpeta de Drive que está dentro de la bóveda (sin la raíz).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Nodo {
    pub nombre: String,
    pub padre: String,
    pub carpeta: bool,
    pub md5: Option<String>,
    pub tam: u64,
    pub editable: bool,
    pub enlace: Option<String>,
    pub autor: Option<String>,
    pub modificado: Option<String>,
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
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Registro {
    pub id: String,
    pub carpeta: bool,
    pub md5: Option<String>,
    /// Tamaño y fecha de la copia local acordada: la de la bóveda o, si es «solo en Drive», la de
    /// la caché (0 si no hay copia).
    pub tam: u64,
    pub mtime: i128,
    /// «Solo en Drive» (061b): no hay copia en la bóveda; es un marcador.
    pub solo_drive: bool,
    /// Pasar a «solo en Drive» apenas Drive tenga esta versión (lo nuevo de este equipo, con el
    /// predeterminado «Solo en Drive»).
    pub liberar: bool,
}

/// Un conflicto abierto: la versión de Drive guardada aparte.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Conflicto {
    pub id: String,
    pub md5: Option<String>,
    /// Ruta relativa a la bóveda de la copia (`.orbital/conflictos/...`).
    pub copia: String,
    pub autor: Option<String>,
    pub fecha: String,
}

/// Un cambio de lo «solo en Drive» hecho sin conexión, que espera la red (061b). La bóveda y el
/// explorador ya lo muestran hecho; Drive lo recibe en el próximo ciclo.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Pendiente {
    /// Renombrar o mover: estaba en `de` y la usuaria lo puso en `a` (el registro ya está en `a`).
    Mover { de: String, a: String },
    /// Mandar a la papelera de Drive lo que estaba en `ruta`, si Drive sigue teniendo la versión
    /// `md5` (si cambió en Drive mientras tanto, se conserva).
    Borrar { ruta: String, md5: Option<String> },
}

/// Todo el estado de un equipo para una bóveda.
#[derive(Clone, Debug, Default)]
pub struct Estado {
    pub version: u32,
    pub cuenta: String,
    /// Id de la carpeta raíz en Drive.
    pub raiz: String,
    /// `pageToken` de la lista de cambios (vacío: falta el primer listado).
    pub token: Option<String>,
    pub nodos: HashMap<String, Nodo>,
    pub registros: BTreeMap<String, Registro>,
    pub conflictos: BTreeMap<String, Conflicto>,
    /// Subidas reanudables en curso, por ruta.
    pub subidas: BTreeMap<String, SesionSubida>,
    /// Conectada a una carpeta que ya tenía contenido y todavía sin la primera conciliación.
    pub primera_union: bool,
    /// Reglas de este equipo (decisión 2): «en ambos» o «solo en Drive», por ruta (carpetas con
    /// `/` final). Las «solo en este equipo» se comparten en `drive.yaml`.
    pub reglas: BTreeMap<String, Ubicacion>,
    /// Qué pasa con lo que se crea en este equipo y no tiene regla.
    pub nuevo_local: Ubicacion,
    /// Qué pasa con lo que aparece nuevo en Drive y no tiene regla.
    pub nuevo_remoto: Ubicacion,
    /// Cambios de lo «solo en Drive» hechos sin conexión, por id de Drive (uno por archivo).
    pub cola: BTreeMap<String, Pendiente>,
}

/// Un archivo «solo en Drive», tal como lo lee la app al escanear (sin red).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Marcador {
    pub ruta: String,
    pub id: String,
    pub tam: u64,
    /// Fecha de modificación en Drive (segundos desde 1970; 0 si no se sabe).
    pub mtime: i64,
    /// Nombre del archivo en la caché (`copias/<id>/<nombre>`).
    pub nombre: String,
}

fn id_boveda(root: &Path) -> String {
    let canon = fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
    let h = sha2::Sha256::digest(canon.as_os_str().as_encoded_bytes());
    h[..8].iter().map(|b| format!("{b:02x}")).collect()
}

fn base_xdg(var: &str, defecto: &str) -> PathBuf {
    std::env::var_os(var)
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(defecto)))
        .unwrap_or_else(std::env::temp_dir)
}

/// Caché de lo «solo en Drive» de una bóveda en este equipo (`$XDG_CACHE_HOME`): las copias
/// abiertas y los extractos para el índice.
pub fn carpeta_cache(root: &Path) -> PathBuf {
    base_xdg("XDG_CACHE_HOME", ".cache")
        .join("orbital/drive")
        .join(id_boveda(root))
}

/// La copia en la caché de un archivo «solo en Drive».
pub fn ruta_copia(dir_cache: &Path, id: &str, nombre: &str) -> PathBuf {
    dir_cache
        .join("copias")
        .join(limpio(id))
        .join(crate::local::nombre_local(nombre))
}

fn limpio(id: &str) -> String {
    id.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

/// Carpeta del estado de una bóveda en este equipo.
pub fn carpeta_estado(root: &Path) -> PathBuf {
    base_xdg("XDG_STATE_HOME", ".local/state")
        .join("orbital/drive")
        .join(id_boveda(root))
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
        match fs::read_to_string(dir.join("estado.txt")) {
            Ok(t) => return Self::desde_texto(&t).map(Some),
            Err(e) if e.kind() != io::ErrorKind::NotFound => return Err(e),
            Err(_) => {}
        }
        // El estado de la 061a (JSON): se lee una vez y se guarda en el formato nuevo.
        match fs::read(dir.join("estado.json")) {
            Ok(b) => {
                let e = Self::desde_json(&b)?;
                e.guardar(dir)?;
                let _ = fs::remove_file(dir.join("estado.json"));
                Ok(Some(e))
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e),
        }
    }

    pub fn guardar(&self, dir: &Path) -> io::Result<()> {
        fs::create_dir_all(dir)?;
        orbital_vault::write::atomic_write(&dir.join("estado.txt"), self.a_texto().as_bytes())
            .map(|_| ())
    }

    /// Lo «solo en Drive» (los archivos que son marcadores), ordenado por ruta.
    pub fn marcadores(&self) -> Vec<Marcador> {
        self.registros
            .iter()
            .filter(|(_, b)| b.solo_drive && !b.carpeta)
            .map(|(r, b)| {
                let n = self.nodos.get(&b.id);
                Marcador {
                    ruta: r.clone(),
                    id: b.id.clone(),
                    tam: n.map_or(0, |n| n.tam),
                    mtime: n
                        .and_then(|n| n.modificado.as_deref())
                        .map_or(0, segundos_rfc3339),
                    nombre: n.map_or_else(
                        || r.rsplit('/').next().unwrap_or(r).to_owned(),
                        |n| n.nombre.clone(),
                    ),
                }
            })
            .collect()
    }

    fn a_texto(&self) -> String {
        let mut s = String::with_capacity(
            128 + self.nodos.len() * 160 + self.registros.len() * 100 + self.reglas.len() * 40,
        );
        let o = |v: &Option<String>| v.as_deref().unwrap_or("").to_owned();
        linea(&mut s, &["orbital-drive-estado", &VERSION.to_string()]);
        linea(&mut s, &["cuenta", &self.cuenta]);
        linea(&mut s, &["raiz", &self.raiz]);
        linea(&mut s, &["token", &o(&self.token)]);
        linea(
            &mut s,
            &["union", if self.primera_union { "1" } else { "0" }],
        );
        linea(&mut s, &["nuevo_local", self.nuevo_local.clave()]);
        linea(&mut s, &["nuevo_remoto", self.nuevo_remoto.clave()]);
        for (id, n) in &self.nodos {
            let mut f = String::new();
            for (c, v) in [('c', n.carpeta), ('e', n.editable), ('n', n.nativo)] {
                if v {
                    f.push(c);
                }
            }
            linea(
                &mut s,
                &[
                    "N",
                    id,
                    &n.padre,
                    &f,
                    &o(&n.md5),
                    &n.tam.to_string(),
                    &n.nombre,
                    &o(&n.enlace),
                    &o(&n.autor),
                    &o(&n.modificado),
                ],
            );
        }
        for (r, b) in &self.registros {
            let mut f = String::new();
            for (c, v) in [('c', b.carpeta), ('d', b.solo_drive), ('l', b.liberar)] {
                if v {
                    f.push(c);
                }
            }
            linea(
                &mut s,
                &[
                    "R",
                    r,
                    &b.id,
                    &f,
                    &o(&b.md5),
                    &b.tam.to_string(),
                    &b.mtime.to_string(),
                ],
            );
        }
        for (r, c) in &self.conflictos {
            linea(
                &mut s,
                &["C", r, &c.id, &o(&c.md5), &c.copia, &o(&c.autor), &c.fecha],
            );
        }
        for (r, u) in &self.subidas {
            linea(&mut s, &["S", r, &u.url, &u.tam.to_string(), &u.md5]);
        }
        for (r, u) in &self.reglas {
            linea(&mut s, &["U", r, u.clave()]);
        }
        for (id, p) in &self.cola {
            match p {
                Pendiente::Mover { de, a } => linea(&mut s, &["Q", id, "m", de, a, ""]),
                Pendiente::Borrar { ruta, md5 } => {
                    linea(&mut s, &["Q", id, "b", ruta, "", &o(md5)])
                }
            }
        }
        s
    }

    fn desde_texto(t: &str) -> io::Result<Self> {
        let malo = |l: &str| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "estado de Drive dañado: «{}»",
                    l.chars().take(60).collect::<String>()
                ),
            )
        };
        let mut e = Self {
            version: VERSION,
            ..Default::default()
        };
        let mut lineas = t.lines();
        match lineas.next().map(campos) {
            Some(c) if c.first().map(String::as_str) == Some("orbital-drive-estado") => {}
            _ => return Err(malo(t.lines().next().unwrap_or(""))),
        }
        let num = |v: &str| v.parse::<u64>().unwrap_or(0);
        let op = |v: &String| (!v.is_empty()).then(|| v.clone());
        for l in lineas {
            if l.is_empty() {
                continue;
            }
            let c = campos(l);
            let k = |i: usize| c.get(i).cloned().unwrap_or_default();
            match c[0].as_str() {
                "cuenta" => e.cuenta = k(1),
                "raiz" => e.raiz = k(1),
                "token" => e.token = op(&k(1)),
                "union" => e.primera_union = k(1) == "1",
                "nuevo_local" => e.nuevo_local = Ubicacion::desde_clave(&k(1)).unwrap_or_default(),
                "nuevo_remoto" => {
                    e.nuevo_remoto = Ubicacion::desde_clave(&k(1)).unwrap_or_default()
                }
                "N" if c.len() >= 10 => {
                    let f = k(3);
                    e.nodos.insert(
                        k(1),
                        Nodo {
                            padre: k(2),
                            carpeta: f.contains('c'),
                            editable: f.contains('e'),
                            nativo: f.contains('n'),
                            md5: op(&k(4)),
                            tam: num(&k(5)),
                            nombre: k(6),
                            enlace: op(&k(7)),
                            autor: op(&k(8)),
                            modificado: op(&k(9)),
                        },
                    );
                }
                "R" if c.len() >= 7 => {
                    let f = k(3);
                    e.registros.insert(
                        k(1),
                        Registro {
                            id: k(2),
                            carpeta: f.contains('c'),
                            solo_drive: f.contains('d'),
                            liberar: f.contains('l'),
                            md5: op(&k(4)),
                            tam: num(&k(5)),
                            mtime: k(6).parse().unwrap_or(0),
                        },
                    );
                }
                "C" if c.len() >= 7 => {
                    e.conflictos.insert(
                        k(1),
                        Conflicto {
                            id: k(2),
                            md5: op(&k(3)),
                            copia: k(4),
                            autor: op(&k(5)),
                            fecha: k(6),
                        },
                    );
                }
                "S" if c.len() >= 5 => {
                    e.subidas.insert(
                        k(1),
                        SesionSubida {
                            url: k(2),
                            tam: num(&k(3)),
                            md5: k(4),
                        },
                    );
                }
                "U" if c.len() >= 3 => {
                    if let Some(u) = Ubicacion::desde_clave(&k(2)) {
                        e.reglas.insert(k(1), u);
                    }
                }
                "Q" if c.len() >= 6 => {
                    let p = match k(2).as_str() {
                        "m" => Pendiente::Mover { de: k(3), a: k(4) },
                        "b" => Pendiente::Borrar {
                            ruta: k(3),
                            md5: op(&k(5)),
                        },
                        _ => return Err(malo(l)),
                    };
                    e.cola.insert(k(1), p);
                }
                _ => return Err(malo(l)),
            }
        }
        Ok(e)
    }

    /// El `estado.json` de la 061a.
    fn desde_json(b: &[u8]) -> io::Result<Self> {
        let v: serde_json::Value =
            serde_json::from_slice(b).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        let t =
            |v: &serde_json::Value, k: &str| v.get(k).and_then(|x| x.as_str()).map(str::to_owned);
        let b_ =
            |v: &serde_json::Value, k: &str| v.get(k).and_then(|x| x.as_bool()).unwrap_or(false);
        let n = |v: &serde_json::Value, k: &str| v.get(k).and_then(|x| x.as_u64()).unwrap_or(0);
        let mapa = |k: &str| {
            v.get(k)
                .and_then(|x| x.as_object())
                .into_iter()
                .flatten()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect::<Vec<_>>()
        };
        let mut e = Self {
            version: VERSION,
            cuenta: t(&v, "cuenta").unwrap_or_default(),
            raiz: t(&v, "raiz").unwrap_or_default(),
            token: t(&v, "token"),
            primera_union: b_(&v, "primera_union"),
            ..Default::default()
        };
        for (id, x) in mapa("nodos") {
            e.nodos.insert(
                id,
                Nodo {
                    nombre: t(&x, "nombre").unwrap_or_default(),
                    padre: t(&x, "padre").unwrap_or_default(),
                    carpeta: b_(&x, "carpeta"),
                    md5: t(&x, "md5"),
                    tam: n(&x, "tam"),
                    editable: b_(&x, "editable"),
                    enlace: t(&x, "enlace"),
                    autor: t(&x, "autor"),
                    modificado: t(&x, "modificado"),
                    nativo: b_(&x, "nativo"),
                },
            );
        }
        for (r, x) in mapa("registros") {
            e.registros.insert(
                r,
                Registro {
                    id: t(&x, "id").unwrap_or_default(),
                    carpeta: b_(&x, "carpeta"),
                    md5: t(&x, "md5"),
                    tam: n(&x, "tam"),
                    mtime: x.get("mtime").and_then(|m| m.as_i64()).unwrap_or(0) as i128,
                    ..Default::default()
                },
            );
        }
        for (r, x) in mapa("conflictos") {
            e.conflictos.insert(
                r,
                Conflicto {
                    id: t(&x, "id").unwrap_or_default(),
                    md5: t(&x, "md5"),
                    copia: t(&x, "copia").unwrap_or_default(),
                    autor: t(&x, "autor"),
                    fecha: t(&x, "fecha").unwrap_or_default(),
                },
            );
        }
        for (r, x) in mapa("subidas") {
            e.subidas.insert(
                r,
                SesionSubida {
                    url: t(&x, "url").unwrap_or_default(),
                    tam: n(&x, "tam"),
                    md5: t(&x, "md5").unwrap_or_default(),
                },
            );
        }
        Ok(e)
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

/// Escribe los marcadores para el escaneo de la app (`marcadores` en la carpeta del estado).
/// Una línea por archivo: tamaño, fecha, id, nombre en Drive y ruta.
pub fn guardar_marcadores(dir: &Path, m: &[Marcador]) -> io::Result<()> {
    fs::create_dir_all(dir)?;
    let mut s = String::with_capacity(m.len() * 80);
    for x in m {
        linea(
            &mut s,
            &[
                &x.tam.to_string(),
                &x.mtime.to_string(),
                &x.id,
                &x.nombre,
                &x.ruta,
            ],
        );
    }
    orbital_vault::write::atomic_write(&dir.join("marcadores"), s.as_bytes()).map(|_| ())
}

/// Lee los marcadores que dejó el motor. Sin archivo, ninguno. Lo llama el escaneo de la app
/// (fuera del hilo de GTK): una lectura y un recorrido de líneas, sin red.
pub fn leer_marcadores(dir: &Path) -> Vec<Marcador> {
    let Ok(t) = fs::read_to_string(dir.join("marcadores")) else {
        return Vec::new();
    };
    t.lines()
        .filter_map(|l| {
            let c = campos(l);
            (c.len() >= 5).then(|| Marcador {
                tam: c[0].parse().unwrap_or(0),
                mtime: c[1].parse().unwrap_or(0),
                id: c[2].clone(),
                nombre: c[3].clone(),
                ruta: c[4].clone(),
            })
        })
        .collect()
}

/// Segundos desde 1970 de una fecha RFC 3339 de Drive (`2026-11-22T10:00:00.000Z`).
pub fn segundos_rfc3339(t: &str) -> i64 {
    gio::glib::DateTime::from_iso8601(t, None)
        .map(|d| d.to_unix())
        .unwrap_or(0)
}

/// Agrega una línea con los campos escapados (`\t`, `\n`, `\\`).
fn linea(s: &mut String, campos: &[&str]) {
    for (i, c) in campos.iter().enumerate() {
        if i > 0 {
            s.push('\t');
        }
        for ch in c.chars() {
            match ch {
                '\t' => s.push_str("\\t"),
                '\n' => s.push_str("\\n"),
                '\r' => s.push_str("\\r"),
                '\\' => s.push_str("\\\\"),
                _ => s.push(ch),
            }
        }
    }
    s.push('\n');
}

fn campos(l: &str) -> Vec<String> {
    l.split('\t')
        .map(|c| {
            if !c.contains('\\') {
                return c.to_owned();
            }
            let mut out = String::with_capacity(c.len());
            let mut it = c.chars();
            while let Some(ch) = it.next() {
                if ch == '\\' {
                    match it.next() {
                        Some('t') => out.push('\t'),
                        Some('n') => out.push('\n'),
                        Some('r') => out.push('\r'),
                        Some(o) => out.push(o),
                        None => {}
                    }
                } else {
                    out.push(ch);
                }
            }
            out
        })
        .collect()
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
    dir.join("bases").join(limpio(id))
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
        e.nodos.get_mut("n").unwrap().nombre = "con\ttab\\y\nsalto.md".into();
        e.registros.insert(
            "wiki/nota.md".into(),
            Registro {
                id: "m".into(),
                md5: Some("abc".into()),
                tam: 3,
                mtime: -5,
                solo_drive: true,
                ..Default::default()
            },
        );
        e.reglas.insert("raw/".into(), Ubicacion::SoloDrive);
        e.nuevo_remoto = Ubicacion::SoloDrive;
        e.cola.insert(
            "m".into(),
            Pendiente::Mover {
                de: "wiki/vie\tja.md".into(),
                a: "wiki/nota.md".into(),
            },
        );
        e.cola.insert(
            "q".into(),
            Pendiente::Borrar {
                ruta: "raw/v.mp4".into(),
                md5: Some("ff".into()),
            },
        );
        let dir = std::env::temp_dir().join(format!("orbital-drive-estado-{}", std::process::id()));
        e.guardar(&dir).unwrap();
        let l = Estado::leer(&dir).unwrap().unwrap();
        assert_eq!(l.nodos, e.nodos);
        assert_eq!(l.registros, e.registros);
        assert_eq!(l.reglas, e.reglas);
        assert_eq!(l.cola, e.cola);
        assert_eq!(l.nuevo_remoto, Ubicacion::SoloDrive);
        let _c = Candado::tomar(&dir).unwrap();
        assert!(Candado::tomar(&dir).is_err());
        let m = l.marcadores();
        assert_eq!(m.len(), 1);
        guardar_marcadores(&dir, &m).unwrap();
        assert_eq!(leer_marcadores(&dir), m);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn lee_el_estado_de_la_061a() {
        let dir = std::env::temp_dir().join(format!("orbital-drive-json-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("estado.json"),
            r#"{"version":1,"cuenta":"a@b","raiz":"R","token":"7","nodos":{"n":{"nombre":"x.md","padre":"R","md5":"m","tam":4,"editable":true}},"registros":{"x.md":{"id":"n","md5":"m","tam":4,"mtime":99}},"conflictos":{},"subidas":{"g.bin":{"url":"http://u","tam":9,"md5":"q"}},"primera_union":false}"#,
        )
        .unwrap();
        let e = Estado::leer(&dir).unwrap().unwrap();
        assert_eq!(e.token.as_deref(), Some("7"));
        assert_eq!(e.nodos["n"].nombre, "x.md");
        assert_eq!(e.registros["x.md"].mtime, 99);
        assert_eq!(e.subidas["g.bin"].tam, 9);
        assert!(dir.join("estado.txt").is_file() && !dir.join("estado.json").exists());
        fs::remove_dir_all(&dir).unwrap();
    }
}
