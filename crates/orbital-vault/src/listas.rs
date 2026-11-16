//! Favoritos y Guardados (027): `.orbital/listas.yaml`, con el mismo trato que `busquedas.yaml`
//! (snapshot con hash, escritura atómica, respaldo) y funciones puras para cada cambio.
//!
//! A diferencia de las búsquedas guardadas, un archivo dañado queda escribible: el primer cambio
//! de la usuaria respalda el contenido dañado y recién después escribe (FR-011, research R3).
//! El respaldo se hace solo cuando el contenido en disco no lo escribió esta misma sesión
//! (research R4): reordenar no llena `.orbital/respaldo/` de copias.

use crate::entry::{Entry, RelPath};
use crate::props::{PropValue, emit, yaml};
use crate::write::{atomic_write, backup, hash};
use crate::{Vault, VaultError, WriteCtx};
use saphyr_parser::{Event, Parser, ScalarStyle};
use std::{
    collections::HashSet,
    fs, io,
    path::{Path, PathBuf},
};

/// Ruta del archivo dentro de la bóveda.
pub const LISTAS_PATH: &str = ".orbital/listas.yaml";

/// Versión del formato que esta versión de Orbital entiende.
const FORMAT_VERSION: u64 = 1;

/// Favoritos y colecciones de la bóveda.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Listas {
    /// En el orden elegido, sin repetidos.
    pub favoritos: Vec<RelPath>,
    /// En orden de creación, con nombres únicos sin distinguir mayúsculas.
    pub colecciones: Vec<Coleccion>,
}

/// Una colección de Guardados (un solo nivel).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Coleccion {
    /// Sin espacios en los bordes y no vacío.
    pub nombre: String,
    /// En el orden elegido, sin repetidos; puede estar vacía.
    pub notas: Vec<RelPath>,
}

/// Cuál de las listas.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Lista {
    Favoritos,
    /// Índice en `Listas::colecciones`.
    Coleccion(usize),
}

/// Por qué no se acepta un nombre de colección.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ErrorNombre {
    Vacio,
    Repetido,
}

impl std::fmt::Display for ErrorNombre {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Vacio => "El nombre de la colección no puede estar vacío",
            Self::Repetido => "Ya existe una colección con ese nombre",
        })
    }
}

/// Entradas quitadas por [`Listas::quitar_ruta`], con su lista y su posición, para reponerlas.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Quitadas(pub Vec<(Lista, usize, RelPath)>);

impl Quitadas {
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

/// Lo leído del disco: el hash pertenece al archivo que se mostró, no a una relectura al guardar.
#[derive(Clone, Debug)]
pub struct ListasSnapshot {
    /// Vacío si el archivo falta o está dañado.
    pub listas: Listas,
    /// XXH3 de los bytes leídos; `None` si el archivo no existía.
    pub hash: Option<u64>,
    /// Aviso si el archivo estaba dañado o no se pudo leer.
    pub warning: Option<String>,
    root: Option<PathBuf>,
    writable: bool,
    /// El contenido en disco lo escribió esta misma sesión (este snapshot salió de
    /// [`Vault::guardar_listas`]): la próxima escritura no lo respalda.
    propio: bool,
}

impl ListasSnapshot {
    /// Se puede guardar encima (también con el archivo dañado; no con uno ilegible o de una
    /// versión más nueva del formato).
    pub fn writable(&self) -> bool {
        self.writable
    }

    /// El contenido en disco lo escribió esta sesión de Orbital.
    pub fn propio(&self) -> bool {
        self.propio
    }
}

fn invalid(message: impl Into<String>) -> VaultError {
    VaultError::InvalidProps {
        path: RelPath::from_static(LISTAS_PATH),
        line: 1,
        message: message.into(),
    }
}

/// Clave para comparar nombres de colección: sin espacios en los bordes y en minúsculas.
fn clave(nombre: &str) -> String {
    nombre.trim().to_lowercase()
}

/// Ruta válida para las listas: `RelPath` sin componentes ocultos.
pub(crate) fn ruta_valida(s: &str) -> Option<RelPath> {
    let ruta = RelPath::new(s).ok()?;
    (!ruta.as_str().split('/').any(|c| c.starts_with('.'))).then_some(ruta)
}

/// Agrega `ruta` si no estaba (una ruta repetida se toma una sola vez, la primera).
fn push_unica(lista: &mut Vec<RelPath>, ruta: RelPath) {
    if !lista.contains(&ruta) {
        lista.push(ruta);
    }
}

/// Árbol mínimo del YAML: solo escalares, listas y mapas (sin anclas, alias ni etiquetas).
pub(crate) enum Nodo {
    Escalar(String, ScalarStyle, usize),
    Lista(Vec<Nodo>, usize),
    Mapa(Vec<(Nodo, Nodo)>, usize),
}

impl Nodo {
    pub(crate) fn linea(&self) -> usize {
        match self {
            Self::Escalar(_, _, l) | Self::Lista(_, l) | Self::Mapa(_, l) => *l,
        }
    }

    /// Escalar vacío, `~` o `null` sin comillas.
    pub(crate) fn es_nulo(&self) -> bool {
        matches!(self, Self::Escalar(v, ScalarStyle::Plain, _)
            if yaml::scalar_value(v, ScalarStyle::Plain) == PropValue::Null)
    }

    /// Texto de un escalar que se lee como texto (no número, fecha ni booleano sin comillas).
    pub(crate) fn texto(&self) -> Option<&str> {
        match self {
            Self::Escalar(v, ScalarStyle::Plain, _) => matches!(
                yaml::scalar_value(v, ScalarStyle::Plain),
                PropValue::Text(_) | PropValue::Link(_)
            )
            .then_some(v.as_str()),
            Self::Escalar(v, _, _) => Some(v),
            _ => None,
        }
    }
}

pub(crate) fn arbol(text: &str) -> Result<Option<Nodo>, String> {
    let mut parser = Parser::new_from_str(text);
    // Pila de contenedores abiertos; cada mapa guarda la clave pendiente.
    enum Abierto {
        Lista(Vec<Nodo>, usize),
        Mapa(Vec<(Nodo, Nodo)>, Option<Nodo>, usize),
    }
    let mut pila: Vec<Abierto> = Vec::new();
    let mut raiz: Option<Nodo> = None;
    let mut documentos = 0;
    while let Some(evento) = parser.next_event() {
        let (evento, span) = evento
            .map_err(|e| format!("Línea {}: {}", e.marker().line(), yaml::translate(e.info())))?;
        let linea = span.start.line();
        let otro = || format!("Línea {linea}: el archivo no sigue el formato");
        let nodo = match evento {
            Event::StreamStart | Event::StreamEnd | Event::Nothing | Event::DocumentEnd => {
                continue;
            }
            Event::DocumentStart(_) => {
                documentos += 1;
                if documentos > 1 {
                    return Err(otro());
                }
                continue;
            }
            Event::Scalar(v, style, 0, None) => Nodo::Escalar(v.into_owned(), style, linea),
            Event::SequenceStart(0, None) => {
                pila.push(Abierto::Lista(Vec::new(), linea));
                continue;
            }
            Event::MappingStart(0, None) => {
                pila.push(Abierto::Mapa(Vec::new(), None, linea));
                continue;
            }
            Event::SequenceEnd => match pila.pop() {
                Some(Abierto::Lista(items, l)) => Nodo::Lista(items, l),
                _ => return Err(otro()),
            },
            Event::MappingEnd => match pila.pop() {
                Some(Abierto::Mapa(pares, None, l)) => Nodo::Mapa(pares, l),
                _ => return Err(otro()),
            },
            // Anclas, alias y etiquetas no tienen lugar en este archivo.
            _ => return Err(otro()),
        };
        match pila.last_mut() {
            None => {
                if raiz.is_some() {
                    return Err(otro());
                }
                raiz = Some(nodo);
            }
            Some(Abierto::Lista(items, _)) => items.push(nodo),
            Some(Abierto::Mapa(pares, pendiente, _)) => match pendiente.take() {
                None => *pendiente = Some(nodo),
                Some(k) => pares.push((k, nodo)),
            },
        }
    }
    Ok(raiz)
}

/// Lista de rutas: `[]`, una lista YAML o nada (`favoritos:` sin valor).
fn rutas(nodo: &Nodo, que: &str) -> Result<Vec<RelPath>, String> {
    if nodo.es_nulo() {
        return Ok(Vec::new());
    }
    let Nodo::Lista(items, _) = nodo else {
        return Err(format!(
            "Línea {}: «{que}» tiene que ser una lista de rutas",
            nodo.linea()
        ));
    };
    let mut out = Vec::with_capacity(items.len());
    for item in items {
        let ruta = item.texto().and_then(ruta_valida).ok_or_else(|| {
            format!(
                "Línea {}: «{}» no es una ruta de la bóveda",
                item.linea(),
                item.texto().unwrap_or("?")
            )
        })?;
        push_unica(&mut out, ruta);
    }
    Ok(out)
}

/// Lee el archivo, estricto: cualquier cosa fuera del contrato es un archivo dañado (research R2).
pub fn parse(text: &str) -> Result<Listas, String> {
    let Some(raiz) = arbol(text)? else {
        return Ok(Listas::default());
    };
    if raiz.es_nulo() {
        return Ok(Listas::default());
    }
    let Nodo::Mapa(pares, _) = raiz else {
        return Err("El archivo de listas tiene que ser un mapa YAML".into());
    };
    let mut listas = Listas::default();
    let mut vistas = HashSet::new();
    for (k, v) in &pares {
        let Some(nombre) = k.texto() else {
            return Err(format!("Línea {}: clave inválida", k.linea()));
        };
        if !vistas.insert(nombre) {
            return Err(format!("Línea {}: «{nombre}» está repetida", k.linea()));
        }
        match nombre {
            "format_version" => {
                let version = match v {
                    Nodo::Escalar(s, ScalarStyle::Plain, _) => s.parse::<u64>().ok(),
                    _ => None,
                }
                .ok_or_else(|| {
                    format!(
                        "Línea {}: «format_version» tiene que ser un entero",
                        v.linea()
                    )
                })?;
                if version > FORMAT_VERSION {
                    return Err(format!(
                        "el archivo es de una versión más nueva del formato ({version})"
                    ));
                }
            }
            "favoritos" => listas.favoritos = rutas(v, "favoritos")?,
            "colecciones" => listas.colecciones = colecciones(v)?,
            otra => return Err(format!("Línea {}: clave desconocida «{otra}»", k.linea())),
        }
    }
    Ok(listas)
}

fn colecciones(nodo: &Nodo) -> Result<Vec<Coleccion>, String> {
    if nodo.es_nulo() {
        return Ok(Vec::new());
    }
    let Nodo::Lista(items, _) = nodo else {
        return Err(format!(
            "Línea {}: «colecciones» tiene que ser una lista",
            nodo.linea()
        ));
    };
    let mut out: Vec<Coleccion> = Vec::with_capacity(items.len());
    let mut claves = HashSet::new();
    for item in items {
        let Nodo::Mapa(pares, linea) = item else {
            return Err(format!(
                "Línea {}: cada colección es un mapa con «nombre» y «notas»",
                item.linea()
            ));
        };
        let (mut nombre, mut notas) = (None, None);
        for (k, v) in pares {
            match k.texto() {
                Some("nombre") if nombre.is_none() => {
                    nombre = Some(v.texto().map(str::trim).ok_or_else(|| {
                        format!("Línea {}: el nombre tiene que ser texto", v.linea())
                    })?)
                }
                Some("notas") if notas.is_none() => notas = Some(rutas(v, "notas")?),
                _ => {
                    return Err(format!(
                        "Línea {}: clave inválida en una colección",
                        k.linea()
                    ));
                }
            }
        }
        let nombre = nombre
            .filter(|n| !n.is_empty())
            .ok_or_else(|| format!("Línea {linea}: una colección no tiene nombre"))?;
        if !claves.insert(clave(nombre)) {
            return Err(format!(
                "Línea {linea}: hay dos colecciones llamadas «{nombre}»"
            ));
        }
        out.push(Coleccion {
            nombre: nombre.to_owned(),
            notas: notas.unwrap_or_default(),
        });
    }
    Ok(out)
}

/// Escribe el archivo como en el contrato (`docs/formato-boveda.md`).
pub fn serialize(listas: &Listas) -> String {
    let mut out = String::from("format_version: 1\n");
    let lista = |out: &mut String, rutas: &[RelPath], sangria: &str| {
        for r in rutas {
            out.push_str(sangria);
            out.push_str("- ");
            out.push_str(&emit::scalar(r.as_str()));
            out.push('\n');
        }
    };
    if listas.favoritos.is_empty() {
        out.push_str("favoritos: []\n");
    } else {
        out.push_str("favoritos:\n");
        lista(&mut out, &listas.favoritos, "  ");
    }
    if listas.colecciones.is_empty() {
        out.push_str("colecciones: []\n");
    } else {
        out.push_str("colecciones:\n");
        for c in &listas.colecciones {
            out.push_str("  - nombre: ");
            out.push_str(&emit::scalar(&c.nombre));
            out.push('\n');
            if c.notas.is_empty() {
                out.push_str("    notas: []\n");
            } else {
                out.push_str("    notas:\n");
                lista(&mut out, &c.notas, "      ");
            }
        }
    }
    out
}

/// Lo que se escribe tiene que poder leerse igual.
fn validate(listas: &Listas) -> Result<(), VaultError> {
    let ruta_ok = |r: &RelPath| ruta_valida(r.as_str()).is_some();
    let sin_repetidos = |v: &[RelPath]| {
        let mut vistas = HashSet::with_capacity(v.len());
        v.iter().all(|r| vistas.insert(r))
    };
    if !listas.favoritos.iter().all(ruta_ok) || !sin_repetidos(&listas.favoritos) {
        return Err(invalid("Favoritos tiene una ruta inválida o repetida"));
    }
    let mut claves = HashSet::new();
    for c in &listas.colecciones {
        if c.nombre.trim().is_empty() || c.nombre.trim() != c.nombre {
            return Err(invalid(ErrorNombre::Vacio.to_string()));
        }
        if !claves.insert(clave(&c.nombre)) {
            return Err(invalid(ErrorNombre::Repetido.to_string()));
        }
        if !c.notas.iter().all(ruta_ok) || !sin_repetidos(&c.notas) {
            return Err(invalid(format!(
                "La colección «{}» tiene una ruta inválida o repetida",
                c.nombre
            )));
        }
    }
    Ok(())
}

/// Solo para esta ruta de configuración. No relaja la prohibición genérica de escribir ocultos.
pub(crate) fn check_directories(root: &Path, parts: &[&str]) -> Result<(), VaultError> {
    let mut path = root.to_path_buf();
    for part in parts {
        path.push(part);
        match fs::symlink_metadata(&path) {
            Ok(meta) if !meta.file_type().is_symlink() && meta.is_dir() => {}
            Ok(_) => return Err(VaultError::InvalidPath(path.display().to_string())),
            Err(error) if error.kind() == io::ErrorKind::NotFound => break,
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}

fn read_config(root: &Path) -> Result<Option<Vec<u8>>, VaultError> {
    leer_config(root, LISTAS_PATH)
}

/// Lee un archivo de `.orbital/` sin seguir enlaces simbólicos; `None` si no existe.
pub(crate) fn leer_config(root: &Path, ruta: &'static str) -> Result<Option<Vec<u8>>, VaultError> {
    check_directories(root, &[".orbital"])?;
    let path = root.join(ruta);
    match fs::symlink_metadata(&path) {
        Ok(meta) if meta.file_type().is_symlink() || !meta.is_file() => {
            return Err(VaultError::InvalidPath(ruta.into()));
        }
        Ok(meta) if meta.len() > crate::entry::MAX_EDITABLE_BYTES => {
            return Err(VaultError::TooLarge(RelPath::from_static(ruta)));
        }
        Ok(_) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    }
    fs::read(path).map(Some).map_err(Into::into)
}

impl Vault {
    /// Lee `.orbital/listas.yaml`. Ausente = vacío sin aviso; dañado = vacío con aviso y
    /// escribible (el primer cambio respalda el dañado); ilegible = vacío, con aviso y sin escribir.
    /// Nunca crea nada.
    pub fn listas_snapshot(&self) -> ListasSnapshot {
        let root = self.root().canonicalize().ok();
        let mut snapshot = ListasSnapshot {
            listas: Listas::default(),
            hash: None,
            warning: None,
            writable: false,
            propio: false,
            root,
        };
        let bytes = match read_config(self.root()) {
            Ok(bytes) => bytes,
            Err(error) => {
                snapshot.warning = Some(format!(
                    "No se cargaron los favoritos ni los guardados: {error}. El archivo no se sobreescribirá."
                ));
                return snapshot;
            }
        };
        snapshot.writable = snapshot.root.is_some();
        let Some(bytes) = bytes else {
            return snapshot;
        };
        snapshot.hash = Some(hash(&bytes));
        let leido = std::str::from_utf8(&bytes)
            .map_err(|_| "el archivo no está en UTF-8".to_owned())
            .and_then(parse);
        match leido {
            Ok(listas) => snapshot.listas = listas,
            Err(error) => {
                // Un formato más nuevo no se pisa: esta versión no sabe qué perdería.
                let nuevo = error.contains("versión más nueva");
                snapshot.writable &= !nuevo;
                snapshot.warning = Some(if nuevo {
                    format!(
                        "No se cargaron los favoritos ni los guardados: {error}. El archivo no se sobreescribirá."
                    )
                } else {
                    format!(
                        "El archivo de favoritos y guardados tiene errores ({error}). Se muestran vacíos; \
                         al primer cambio se guarda una copia del archivo antes de reemplazarlo."
                    )
                });
            }
        }
        snapshot
    }

    /// Escribe las listas si cambiaron. `Conflict` si el archivo en disco ya no es el del
    /// snapshot. Respalda el contenido anterior solo si no lo escribió esta sesión
    /// (`baseline.propio()`). Crea `.orbital/` recién acá.
    pub fn guardar_listas(
        &self,
        listas: &Listas,
        baseline: &ListasSnapshot,
        ctx: &WriteCtx,
    ) -> Result<ListasSnapshot, VaultError> {
        let root = self.root().canonicalize()?;
        if !baseline.writable || baseline.root.as_deref() != Some(&root) {
            return Err(invalid(
                "Las listas no tienen un estado de lectura válido para esta bóveda",
            ));
        }
        validate(listas)?;
        let before = read_config(&root)?;
        if before.as_deref().map(hash) != baseline.hash {
            return Err(VaultError::Conflict {
                path: RelPath::from_static(LISTAS_PATH),
            });
        }
        let output = serialize(listas);
        if output.len() as u64 > crate::entry::MAX_EDITABLE_BYTES {
            return Err(VaultError::TooLarge(RelPath::from_static(LISTAS_PATH)));
        }
        let mut propio = baseline.propio;
        if before.as_deref() != Some(output.as_bytes()) {
            check_directories(&root, &[".orbital"])?;
            fs::create_dir_all(root.join(".orbital"))?;
            if let Some(bytes) = before.filter(|_| !baseline.propio) {
                let date = ctx.date.to_string();
                check_directories(&root, &[".orbital", "respaldo", &date, ".orbital"])?;
                backup(
                    &root,
                    &RelPath::from_static(LISTAS_PATH),
                    &bytes,
                    hash(output.as_bytes()),
                    ctx,
                )?;
            }
            atomic_write(&root.join(LISTAS_PATH), output.as_bytes())?;
            propio = true;
        }
        Ok(ListasSnapshot {
            listas: listas.clone(),
            hash: Some(hash(output.as_bytes())),
            warning: None,
            root: Some(root),
            writable: true,
            propio,
        })
    }
}

/// `ruta` es `carpeta` o está dentro de ella.
pub(crate) fn dentro_de(ruta: &str, carpeta: &str) -> bool {
    ruta == carpeta
        || ruta
            .strip_prefix(carpeta)
            .is_some_and(|resto| resto.starts_with('/'))
}

impl Listas {
    pub fn es_favorito(&self, ruta: &RelPath) -> bool {
        self.favoritos.contains(ruta)
    }

    /// En cuántas colecciones está `ruta`.
    pub fn colecciones_de(&self, ruta: &RelPath) -> usize {
        self.colecciones
            .iter()
            .filter(|c| c.notas.contains(ruta))
            .count()
    }

    /// Índice de la colección llamada `nombre` (sin distinguir mayúsculas ni espacios en los bordes).
    pub fn buscar_coleccion(&self, nombre: &str) -> Option<usize> {
        let k = clave(nombre);
        self.colecciones.iter().position(|c| clave(&c.nombre) == k)
    }

    fn lista_mut(&mut self, lista: Lista) -> Option<&mut Vec<RelPath>> {
        match lista {
            Lista::Favoritos => Some(&mut self.favoritos),
            Lista::Coleccion(i) => self.colecciones.get_mut(i).map(|c| &mut c.notas),
        }
    }

    /// Las rutas de una lista (vacío si la colección no existe).
    pub fn lista(&self, lista: Lista) -> &[RelPath] {
        match lista {
            Lista::Favoritos => &self.favoritos,
            Lista::Coleccion(i) => self.colecciones.get(i).map_or(&[], |c| &c.notas),
        }
    }

    /// Agrega al final de Favoritos o la quita. Siempre cambia algo.
    pub fn alternar_favorito(&mut self, ruta: &RelPath) -> bool {
        if let Some(n) = self.favoritos.iter().position(|r| r == ruta) {
            self.favoritos.remove(n);
        } else {
            self.favoritos.push(ruta.clone());
        }
        true
    }

    fn nombre_libre(&self, nombre: &str, salvo: Option<usize>) -> Result<String, ErrorNombre> {
        let nombre = nombre.trim();
        if nombre.is_empty() {
            return Err(ErrorNombre::Vacio);
        }
        match self.buscar_coleccion(nombre) {
            Some(i) if Some(i) != salvo => Err(ErrorNombre::Repetido),
            _ => Ok(nombre.to_owned()),
        }
    }

    /// Crea una colección al final, opcionalmente con su primera nota.
    pub fn crear_coleccion(
        &mut self,
        nombre: &str,
        primera: Option<RelPath>,
    ) -> Result<bool, ErrorNombre> {
        let nombre = self.nombre_libre(nombre, None)?;
        self.colecciones.push(Coleccion {
            nombre,
            notas: primera.into_iter().collect(),
        });
        Ok(true)
    }

    /// Cambia el nombre y conserva las notas.
    pub fn renombrar_coleccion(&mut self, i: usize, nombre: &str) -> Result<bool, ErrorNombre> {
        let nombre = self.nombre_libre(nombre, Some(i))?;
        let Some(c) = self.colecciones.get_mut(i) else {
            return Ok(false);
        };
        if c.nombre == nombre {
            return Ok(false);
        }
        c.nombre = nombre;
        Ok(true)
    }

    /// Borra la colección (las notas no se tocan).
    pub fn borrar_coleccion(&mut self, i: usize) -> bool {
        if i < self.colecciones.len() {
            self.colecciones.remove(i);
            true
        } else {
            false
        }
    }

    /// La casilla de «Guardar en…»: pone (al final) o saca `ruta` de la colección `i`.
    pub fn poner_en(&mut self, i: usize, ruta: &RelPath, dentro: bool) -> bool {
        let Some(c) = self.colecciones.get_mut(i) else {
            return false;
        };
        match (c.notas.iter().position(|r| r == ruta), dentro) {
            (None, true) => {
                c.notas.push(ruta.clone());
                true
            }
            (Some(n), false) => {
                c.notas.remove(n);
                true
            }
            _ => false,
        }
    }

    /// Mueve la entrada `desde` a la posición `hasta` (arrastrar, «Subir», «Bajar»).
    pub fn mover(&mut self, lista: Lista, desde: usize, hasta: usize) -> bool {
        let Some(v) = self.lista_mut(lista) else {
            return false;
        };
        if desde >= v.len() || desde == hasta {
            return false;
        }
        let hasta = hasta.min(v.len() - 1);
        if desde == hasta {
            return false;
        }
        let r = v.remove(desde);
        v.insert(hasta, r);
        true
    }

    /// Quita la entrada `indice` de una lista.
    pub fn quitar(&mut self, lista: Lista, indice: usize) -> bool {
        match self.lista_mut(lista) {
            Some(v) if indice < v.len() => {
                v.remove(indice);
                true
            }
            _ => false,
        }
    }

    fn todas_mut(&mut self) -> impl Iterator<Item = (Lista, &mut Vec<RelPath>)> {
        std::iter::once((Lista::Favoritos, &mut self.favoritos)).chain(
            self.colecciones
                .iter_mut()
                .enumerate()
                .map(|(i, c)| (Lista::Coleccion(i), &mut c.notas)),
        )
    }

    /// Sigue un renombre: el archivo `desde` exacto o, si es una carpeta, todo lo que está dentro.
    /// Si la ruta nueva ya estaba en una lista, queda una sola, en la posición de la primera.
    pub fn renombrar_ruta(&mut self, desde: &RelPath, hasta: &RelPath) -> bool {
        let (desde, hasta) = (desde.as_str(), hasta.as_str());
        if desde == hasta {
            return false;
        }
        let mut cambio = false;
        for (_, v) in self.todas_mut() {
            if !v.iter().any(|r| dentro_de(r.as_str(), desde)) {
                continue;
            }
            let viejas = std::mem::take(v);
            for r in viejas {
                let nueva = match r.as_str().strip_prefix(desde) {
                    Some(resto) if dentro_de(r.as_str(), desde) => {
                        match RelPath::new(&format!("{hasta}{resto}")) {
                            Ok(n) => n,
                            Err(_) => r,
                        }
                    }
                    _ => r,
                };
                push_unica(v, nueva);
            }
            cambio = true;
        }
        cambio
    }

    /// Quita `ruta` (exacta o, si es carpeta, todo lo de adentro) de todas las listas y devuelve
    /// lo quitado con sus posiciones.
    pub fn quitar_ruta(&mut self, ruta: &RelPath) -> Quitadas {
        let ruta = ruta.as_str();
        let mut quitadas = Vec::new();
        for (lista, v) in self.todas_mut() {
            let mut n = 0;
            let mut original = 0;
            while n < v.len() {
                if dentro_de(v[n].as_str(), ruta) {
                    quitadas.push((lista, original, v.remove(n)));
                } else {
                    n += 1;
                }
                original += 1;
            }
        }
        Quitadas(quitadas)
    }

    /// Deshace [`Listas::quitar_ruta`]: cada entrada vuelve a su posición (o al final si ya no
    /// cabe). Lo que ya está en la lista, o cuya colección ya no existe, se saltea.
    pub fn reponer(&mut self, quitadas: &Quitadas) -> bool {
        let mut orden: Vec<&(Lista, usize, RelPath)> = quitadas.0.iter().collect();
        orden.sort_by_key(|(_, n, _)| *n);
        let mut cambio = false;
        for (lista, n, ruta) in orden {
            let Some(v) = self.lista_mut(*lista) else {
                continue;
            };
            if v.contains(ruta) {
                continue;
            }
            v.insert((*n).min(v.len()), ruta.clone());
            cambio = true;
        }
        cambio
    }
}

/// Las rutas de las listas que no están en `entries` (que viene ordenado por ruta, como lo deja
/// el recorrido). Derivado: nunca se guarda.
pub fn faltantes(listas: &Listas, entries: &[Entry]) -> HashSet<RelPath> {
    let existe = |r: &RelPath| entries.binary_search_by(|e| e.path.cmp(r)).is_ok();
    listas
        .favoritos
        .iter()
        .chain(listas.colecciones.iter().flat_map(|c| c.notas.iter()))
        .filter(|r| !existe(r))
        .cloned()
        .collect()
}
