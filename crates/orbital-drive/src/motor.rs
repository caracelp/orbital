//! Motor de sincronización (061a): espejo de la bóveda con una carpeta de Drive.
//!
//! [`Sincronizador`] tiene la lógica (un ciclo: traer los cambios remotos, conciliar las rutas que
//! cambiaron, guardar el estado) y se prueba solo contra el servidor falso. [`Motor`] lo corre en
//! su propio hilo, agrupa los cambios locales (5 s sin cambios antes de subir), consulta los
//! cambios remotos cada 30 s con la ventana activa y cada 3 min sin foco, y espera más tras un
//! error de red.
//!
//! Reglas que nunca se rompen (FR-006, FR-008, SC-004):
//! - nada se borra: lo que sale de la bóveda va a una papelera, y lo que se reemplaza (si es una
//!   nota de texto) al historial; en Drive, solo papelera;
//! - si una ruta cambió en los dos lados, se fusiona (texto, sin pisarse) o se guarda la versión de
//!   Drive en `.orbital/conflictos/` y no se sube nada encima hasta que la usuaria elija;
//! - las decisiones se toman por id y suma, nunca por fecha.

use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use crate::api::{Api, Archivo, ErrorApi, Resultado};
use crate::estado::{self, Candado, Conflicto, Estado, Nodo, Registro};
pub use crate::local::Papelera;
use crate::local::{self, EnDisco, Firma};
use crate::reglas::{self, Conexion};

/// Tope para guardar una versión de texto en el historial o usarla como base de una fusión.
pub const TOPE_TEXTO: u64 = 1024 * 1024;

/// Estado de una ruta para el explorador (FR-011; «solo en Drive» y «bloqueado» son de la 061b).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum EstadoArchivo {
    /// En este equipo y en Drive, al día.
    Ambos,
    /// Por subir o subiendo.
    Subiendo,
    /// Bajando.
    Bajando,
    /// En conflicto: hay dos versiones.
    Conflicto,
}

impl EstadoArchivo {
    /// Prioridad para resumir una carpeta (el peor estado de su contenido).
    fn peso(self) -> u8 {
        match self {
            Self::Ambos => 0,
            Self::Bajando => 1,
            Self::Subiendo => 2,
            Self::Conflicto => 3,
        }
    }
}

/// Resumen para la barra de estado.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Resumen {
    pub cuenta: String,
    pub en_linea: bool,
    pub sincronizando: bool,
    pub pendientes: usize,
    pub conflictos: usize,
    /// Pausa por algo que la usuaria tiene que resolver (sesión vencida, cuota llena).
    pub pausa: Option<String>,
    /// Último error pasajero (red, límites).
    pub error: Option<String>,
    /// Hora de la última sincronización completa (`HH:MM`).
    pub ultima: Option<String>,
}

/// Lo que el motor le avisa a la interfaz.
#[derive(Clone, Debug)]
pub enum Evento {
    /// Mapa completo ruta → estado (carpetas incluidas, con el peor estado de su contenido) y las
    /// reglas «solo en este equipo» vigentes.
    Estados(Arc<HashMap<String, EstadoArchivo>>, Arc<Vec<String>>),
    /// Una ruta cambió de estado (subidas y bajadas largas).
    Uno(String, EstadoArchivo),
    Resumen(Resumen),
    /// Se abrió un conflicto en esta ruta.
    Conflicto(String),
    /// Se unieron cambios de los dos lados sin preguntar.
    Fusionada(String),
    /// Aviso para la usuaria (una vez).
    Aviso(String),
    /// Enlace de Drive pedido con [`Orden::Enlace`].
    Enlace(String, Option<String>),
    /// Los conflictos abiertos (cuando cambian).
    Conflictos(Arc<Vec<InfoConflicto>>),
    /// El motor terminó (con el motivo si fue un error).
    Detenido(Option<String>),
}

/// Un conflicto abierto, para la interfaz.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InfoConflicto {
    pub ruta: String,
    /// Ruta relativa de la versión de Drive (`.orbital/conflictos/...`).
    pub copia: String,
    pub autor: Option<String>,
    pub fecha: String,
}

/// Órdenes para el motor.
#[derive(Clone, Debug)]
pub enum Orden {
    /// Rutas relativas que cambiaron en el disco.
    Cambios(Vec<String>),
    Foco(bool),
    Red(bool),
    /// Sincronizar ya.
    Ahora,
    /// Resolver un conflicto: `mia` conserva la de este equipo; si no, la de Drive.
    Resolver {
        ruta: String,
        mia: bool,
    },
    /// Pedir el enlace de Drive de una ruta.
    Enlace(String),
    /// Mandar a la papelera de Drive la copia de una ruta que pasó a «solo en este equipo».
    QuitarDeDrive(String),
    Detener,
}

/// Configuración del motor.
#[derive(Clone, Debug)]
pub struct Config {
    pub root: PathBuf,
    /// Carpeta del estado de este equipo ([`estado::carpeta_estado`]).
    pub dir_estado: PathBuf,
    pub papelera: Papelera,
    pub espera_subida: Duration,
    pub intervalo_activo: Duration,
    pub intervalo_fondo: Duration,
    /// Cada cuánto se recorre la bóveda entera por si algo se escapó al vigilante.
    pub intervalo_completo: Duration,
}

impl Config {
    pub fn new(root: &Path) -> Self {
        Self {
            root: root.to_path_buf(),
            dir_estado: estado::carpeta_estado(root),
            papelera: Papelera::Sistema,
            espera_subida: Duration::from_secs(5),
            intervalo_activo: Duration::from_secs(30),
            intervalo_fondo: Duration::from_secs(180),
            intervalo_completo: Duration::from_secs(600),
        }
    }
}

/// La lógica de sincronización (un hilo, sin estado global).
pub struct Sincronizador<A: Api> {
    cfg: Config,
    api: A,
    pub estado: Estado,
    pub conexion: Conexion,
    id_ruta: HashMap<String, String>,
    ruta_id: HashMap<String, String>,
    sucio: BTreeSet<String>,
    completa: bool,
    emitir: Box<dyn FnMut(Evento) + Send>,
    /// Rutas por subir que el motor todavía espera (para mostrarlas «subiendo»).
    pub pendientes: BTreeSet<String>,
    cuota_llena: bool,
    avisos: BTreeSet<String>,
    ultima_completa: Instant,
    ultimo_mapa: Option<Arc<HashMap<String, EstadoArchivo>>>,
    ultimos_conflictos: Option<Arc<Vec<InfoConflicto>>>,
    /// Firma de los archivos compartidos de `.orbital/` (el vigilante no mira `.orbital/`).
    firmas_orbital: BTreeMap<String, EnDisco>,
    pub ultima: Option<String>,
}

fn padre_de(rel: &str) -> &str {
    rel.rsplit_once('/').map_or("", |(p, _)| p)
}

fn nombre_de(rel: &str) -> &str {
    rel.rsplit_once('/').map_or(rel, |(_, n)| n)
}

fn unir(padre: &str, nombre: &str) -> String {
    if padre.is_empty() {
        nombre.to_owned()
    } else {
        format!("{padre}/{nombre}")
    }
}

impl<A: Api> Sincronizador<A> {
    pub fn new(
        cfg: Config,
        api: A,
        estado: Estado,
        conexion: Conexion,
        emitir: Box<dyn FnMut(Evento) + Send>,
    ) -> Self {
        let mut s = Self {
            cfg,
            api,
            estado,
            conexion,
            id_ruta: HashMap::new(),
            ruta_id: HashMap::new(),
            sucio: BTreeSet::new(),
            completa: true,
            emitir,
            pendientes: BTreeSet::new(),
            cuota_llena: false,
            avisos: BTreeSet::new(),
            ultima_completa: Instant::now(),
            ultimo_mapa: None,
            ultimos_conflictos: None,
            firmas_orbital: BTreeMap::new(),
            ultima: None,
        };
        s.indexar();
        s
    }

    pub fn api(&mut self) -> &mut A {
        &mut self.api
    }

    /// Rearma los índices id ↔ ruta desde los nodos.
    fn indexar(&mut self) {
        self.id_ruta = self.estado.rutas();
        self.ruta_id = self
            .id_ruta
            .iter()
            .map(|(id, r)| (r.clone(), id.clone()))
            .collect();
    }

    fn poner_nodo(&mut self, a: &Archivo, padre: &str) {
        let n = Nodo::de(a, padre);
        if let Some(vieja) = self.id_ruta.remove(&a.id) {
            self.ruta_id.remove(&vieja);
        }
        let ruta = if padre == self.estado.raiz {
            Some(local::nombre_local(&n.nombre))
        } else {
            self.id_ruta
                .get(padre)
                .map(|p| unir(p, &local::nombre_local(&n.nombre)))
        };
        self.estado.nodos.insert(a.id.clone(), n);
        if let Some(r) = ruta {
            self.ruta_id.insert(r.clone(), a.id.clone());
            self.id_ruta.insert(a.id.clone(), r);
        }
    }

    fn quitar_nodo(&mut self, id: &str) {
        self.estado.nodos.remove(id);
        if let Some(r) = self.id_ruta.remove(id) {
            self.ruta_id.remove(&r);
        }
    }

    /// Marca rutas relativas que cambiaron en el disco.
    pub fn marcar<I: IntoIterator<Item = String>>(&mut self, rutas: I) {
        for r in rutas {
            if r.is_empty() {
                self.completa = true;
                continue;
            }
            if matches!(local::en_disco(&self.cfg.root.join(&r)), EnDisco::Carpeta)
                || self.estado.registros.get(&r).is_some_and(|b| b.carpeta)
            {
                self.completa = true;
            }
            self.sucio.insert(r);
        }
    }

    pub fn pedir_completa(&mut self) {
        self.completa = true;
    }

    fn emitir(&mut self, e: Evento) {
        (self.emitir)(e);
    }

    fn avisar(&mut self, clave: &str, texto: String) {
        if self.avisos.insert(clave.to_owned()) {
            self.emitir(Evento::Aviso(texto));
        }
    }

    // --- ciclo ----------------------------------------------------------------------------------

    /// Un ciclo completo. Un error pasajero corta el ciclo: lo que quedó sin hacer sigue marcado.
    pub fn ciclo(&mut self) -> Resultado<()> {
        if let Ok(Some(c)) = Conexion::leer(&self.cfg.root)
            && c != self.conexion
        {
            self.conexion = c;
            self.completa = true;
        }
        self.cuota_llena = false;
        if self.ultima_completa.elapsed() >= self.cfg.intervalo_completo {
            self.completa = true;
        }
        self.mirar_orbital();
        let r = self.ciclo_interno();
        let _ = self.estado.guardar(&self.cfg.dir_estado);
        self.emitir_estados();
        if r.is_ok() {
            self.ultima = Some(
                gio::glib::DateTime::now_local()
                    .and_then(|d| d.format("%H:%M"))
                    .map(|s| s.to_string())
                    .unwrap_or_default(),
            );
        }
        r
    }

    fn mirar_orbital(&mut self) {
        for n in reglas::ORBITAL_COMPARTIDOS {
            let r = format!(".orbital/{n}");
            let ahora = local::en_disco(&self.cfg.root.join(&r));
            if self.firmas_orbital.get(&r) != Some(&ahora) {
                self.firmas_orbital.insert(r.clone(), ahora);
                self.sucio.insert(r);
            }
        }
    }

    fn ciclo_interno(&mut self) -> Resultado<()> {
        if self.estado.token.is_none() {
            self.listado_inicial()?;
        } else {
            self.traer_cambios()?;
        }
        let completa = std::mem::take(&mut self.completa);
        let rutas: BTreeSet<String>;
        let mut disco: HashMap<String, EnDisco> = HashMap::new();
        if completa {
            let recorrido = local::recorrer(&self.cfg.root, &self.conexion);
            let mut todas: BTreeSet<String> = recorrido.keys().cloned().collect();
            todas.extend(self.estado.registros.keys().cloned());
            todas.extend(self.ruta_id.keys().cloned());
            todas.extend(std::mem::take(&mut self.sucio));
            disco.extend(recorrido);
            rutas = todas;
        } else {
            rutas = std::mem::take(&mut self.sucio);
        }
        let res = self.conciliar(rutas.clone(), &mut disco);
        if res.is_err() {
            // Lo que no se hizo vuelve a quedar marcado para el próximo ciclo.
            self.sucio.extend(rutas);
            self.completa |= completa;
        } else if completa {
            self.ultima_completa = Instant::now();
            self.estado.primera_union = false;
        }
        res
    }

    /// Primer listado de la carpeta (y de cualquier carpeta que entra a la bóveda).
    fn listado_inicial(&mut self) -> Resultado<()> {
        let token = self.api.token_inicio()?;
        let raiz = self.estado.raiz.clone();
        self.listar_desde(&raiz)?;
        self.estado.token = Some(token);
        self.indexar();
        self.completa = true;
        Ok(())
    }

    fn listar_desde(&mut self, carpeta: &str) -> Resultado<()> {
        let mut cola = VecDeque::from([carpeta.to_owned()]);
        while let Some(c) = cola.pop_front() {
            let mut pagina: Option<String> = None;
            loop {
                let (archivos, sig) = self.api.hijos(&c, pagina.as_deref())?;
                for a in archivos {
                    if a.carpeta() {
                        cola.push_back(a.id.clone());
                    }
                    self.estado.nodos.insert(a.id.clone(), Nodo::de(&a, &c));
                }
                match sig {
                    Some(s) => pagina = Some(s),
                    None => break,
                }
            }
        }
        Ok(())
    }

    /// Trae los cambios remotos desde el último `pageToken` y marca las rutas que tocan.
    fn traer_cambios(&mut self) -> Resultado<()> {
        let mut token = self.estado.token.clone().unwrap_or_default();
        let antes = self.id_ruta.clone();
        let mut tocados = BTreeSet::new();
        loop {
            let pag = self.api.cambios(&token)?;
            for c in pag.cambios {
                if let Some(r) = antes.get(&c.id) {
                    self.sucio.insert(r.clone());
                    if self.estado.nodos.get(&c.id).is_some_and(|n| n.carpeta) {
                        self.completa = true;
                    }
                }
                match c.archivo {
                    Some(a) if !c.quitado && !a.papelera && !a.padres.is_empty() => {
                        if a.carpeta() {
                            self.completa = true;
                        }
                        let padre = a.padres[0].clone();
                        self.estado.nodos.insert(a.id.clone(), Nodo::de(&a, &padre));
                    }
                    _ => {
                        self.estado.nodos.remove(&c.id);
                    }
                }
                tocados.insert(c.id);
            }
            match (pag.siguiente, pag.nuevo_inicio) {
                (Some(s), _) => token = s,
                (None, Some(n)) => {
                    token = n;
                    break;
                }
                (None, None) => break,
            }
        }
        self.estado.podar();
        self.indexar();
        // Carpetas que entraron a la bóveda (movidas desde afuera): hay que listar su contenido.
        let nuevas: Vec<String> = tocados
            .iter()
            .filter(|id| {
                !antes.contains_key(*id)
                    && self.estado.nodos.get(*id).is_some_and(|n| n.carpeta)
                    && self.id_ruta.contains_key(*id)
            })
            .cloned()
            .collect();
        for id in nuevas {
            self.listar_desde(&id)?;
            self.completa = true;
        }
        if self.completa {
            self.indexar();
        }
        for id in &tocados {
            if let Some(r) = self.id_ruta.get(id) {
                self.sucio.insert(r.clone());
            }
        }
        self.estado.token = Some(token);
        Ok(())
    }

    fn excluida(&self, rel: &str) -> bool {
        !reglas::sincronizable(rel) || self.conexion.es_local(rel)
    }

    fn disco(&self, disco: &mut HashMap<String, EnDisco>, rel: &str) -> EnDisco {
        *disco
            .entry(rel.to_owned())
            .or_insert_with(|| local::en_disco(&self.cfg.root.join(rel)))
    }

    fn md5_local(&self, rel: &str, f: Firma) -> Option<String> {
        if let Some(b) = self.estado.registros.get(rel)
            && !b.carpeta
            && b.tam == f.tam
            && b.mtime == f.mtime
        {
            return b.md5.clone();
        }
        local::md5_archivo(&self.cfg.root.join(rel)).ok()
    }

    /// Concilia un conjunto de rutas (archivos primero, carpetas al final y de adentro hacia afuera).
    fn conciliar(
        &mut self,
        rutas: BTreeSet<String>,
        disco: &mut HashMap<String, EnDisco>,
    ) -> Resultado<()> {
        let mut rutas: BTreeSet<String> = rutas.into_iter().filter(|r| !r.is_empty()).collect();
        self.movimientos_locales(&mut rutas, disco)?;
        self.movimientos_remotos(&mut rutas, disco);
        let mut carpetas = Vec::new();
        for r in &rutas {
            if self.excluida(r) {
                continue;
            }
            let es_carpeta = matches!(self.disco(disco, r), EnDisco::Carpeta)
                || self
                    .ruta_id
                    .get(r)
                    .and_then(|id| self.estado.nodos.get(id))
                    .is_some_and(|n| n.carpeta)
                || self.estado.registros.get(r).is_some_and(|b| b.carpeta);
            if es_carpeta {
                carpetas.push(r.clone());
                continue;
            }
            match self.conciliar_archivo(r, disco) {
                Ok(()) => {}
                Err(e) if e.transitorio() => return Err(e),
                Err(ErrorApi::Cuota) => {
                    self.cuota_llena = true;
                    self.avisar(
                        "cuota",
                        "El espacio de Google Drive está lleno: se pausaron las subidas.".into(),
                    );
                }
                Err(e) => self.avisar(
                    &format!("error:{r}"),
                    format!("No se pudo sincronizar «{r}»: {e}"),
                ),
            }
        }
        self.conciliar_conflictos(disco)?;
        // Carpetas: las más profundas primero.
        carpetas.sort_by_key(|c| std::cmp::Reverse(c.matches('/').count()));
        for c in carpetas {
            match self.conciliar_carpeta(&c, disco) {
                Ok(()) => {}
                Err(e) if e.transitorio() => return Err(e),
                Err(e) => self.avisar(
                    &format!("error:{c}"),
                    format!("No se pudo sincronizar «{c}»: {e}"),
                ),
            }
        }
        Ok(())
    }

    /// Un archivo movido o renombrado en el disco: mismo contenido que una ruta que desapareció.
    fn movimientos_locales(
        &mut self,
        rutas: &mut BTreeSet<String>,
        disco: &mut HashMap<String, EnDisco>,
    ) -> Resultado<()> {
        let mut faltan: Vec<(String, Registro)> = Vec::new();
        let mut nuevos: Vec<(String, Firma)> = Vec::new();
        for r in rutas.iter() {
            if self.excluida(r) {
                continue;
            }
            let en = self.disco(disco, r);
            match (self.estado.registros.get(r), en) {
                (Some(b), EnDisco::Nada) if !b.carpeta => {
                    let sigue = self.estado.nodos.get(&b.id).is_some_and(|n| n.md5 == b.md5)
                        && self.id_ruta.get(&b.id) == Some(r);
                    if sigue {
                        faltan.push((r.clone(), b.clone()));
                    }
                }
                (None, EnDisco::Archivo(f)) if !self.ruta_id.contains_key(r) => {
                    nuevos.push((r.clone(), f));
                }
                _ => {}
            }
        }
        if faltan.is_empty() || nuevos.is_empty() {
            return Ok(());
        }
        for (nueva, f) in nuevos {
            let Some(pos) = faltan.iter().position(|(_, b)| b.tam == f.tam) else {
                continue;
            };
            let Some(md5) = local::md5_archivo(&self.cfg.root.join(&nueva)).ok() else {
                continue;
            };
            let Some(pos) = faltan[pos..]
                .iter()
                .position(|(_, b)| b.tam == f.tam && b.md5.as_deref() == Some(md5.as_str()))
                .map(|p| p + pos)
            else {
                continue;
            };
            let (vieja, b) = faltan.remove(pos);
            let padre_nuevo = self.carpeta_remota(padre_de(&nueva))?;
            let padre_viejo = self.estado.nodos.get(&b.id).map(|n| n.padre.clone());
            let nombre = nombre_de(&nueva);
            let cambia_nombre = nombre_de(&vieja) != nombre;
            let cambia_padre = padre_viejo.as_deref() != Some(padre_nuevo.as_str());
            match self.api.actualizar(
                &b.id,
                cambia_nombre.then_some(nombre),
                cambia_padre.then_some(padre_nuevo.as_str()),
                if cambia_padre {
                    padre_viejo.as_deref()
                } else {
                    None
                },
            ) {
                Ok(a) => {
                    self.poner_nodo(&a, &padre_nuevo);
                    self.estado.registros.remove(&vieja);
                    self.estado.registros.insert(
                        nueva.clone(),
                        Registro {
                            id: b.id.clone(),
                            carpeta: false,
                            md5: a.md5.clone().or(b.md5.clone()),
                            tam: f.tam,
                            mtime: f.mtime,
                        },
                    );
                    if let Some(s) = self.estado.subidas.remove(&vieja) {
                        self.estado.subidas.insert(nueva.clone(), s);
                    }
                    rutas.remove(&vieja);
                    rutas.remove(&nueva);
                }
                Err(e) if e.transitorio() => return Err(e),
                Err(_) => {} // se resuelve como borrado + nuevo
            }
        }
        Ok(())
    }

    /// Un archivo que se movió o renombró en Drive: se mueve en el disco si no hay nada en el destino.
    fn movimientos_remotos(
        &mut self,
        rutas: &mut BTreeSet<String>,
        disco: &mut HashMap<String, EnDisco>,
    ) {
        let mut mover: Vec<(String, String)> = Vec::new();
        for r in rutas.iter() {
            let Some(b) = self.estado.registros.get(r) else {
                continue;
            };
            if b.carpeta {
                continue;
            }
            let Some(q) = self.id_ruta.get(&b.id) else {
                continue;
            };
            if q == r || self.excluida(q) || self.estado.conflictos.contains_key(r) {
                continue;
            }
            if self.estado.registros.contains_key(q) {
                continue;
            }
            mover.push((r.clone(), q.clone()));
        }
        for (r, q) in mover {
            if !matches!(self.disco(disco, &r), EnDisco::Archivo(_))
                || !matches!(self.disco(disco, &q), EnDisco::Nada)
            {
                continue;
            }
            if local::mover(&self.cfg.root.join(&r), &self.cfg.root.join(&q)).is_ok() {
                // Renombrar no cambia la fecha: el registro sigue valiendo en la ruta nueva.
                if let Some(b) = self.estado.registros.remove(&r) {
                    self.estado.registros.insert(q.clone(), b);
                }
                disco.insert(r.clone(), EnDisco::Nada);
                disco.remove(&q);
                rutas.insert(q);
                self.estado.subidas.remove(&r);
            }
        }
    }

    /// La decisión de tres vías para un archivo.
    fn conciliar_archivo(
        &mut self,
        r: &str,
        disco: &mut HashMap<String, EnDisco>,
    ) -> Resultado<()> {
        if self.estado.conflictos.contains_key(r) {
            return Ok(());
        }
        let en = self.disco(disco, r);
        let remoto = self
            .ruta_id
            .get(r)
            .and_then(|id| self.estado.nodos.get(id).map(|n| (id.clone(), n.clone())));
        if remoto.as_ref().is_some_and(|(_, n)| n.nativo || n.carpeta) {
            return Ok(());
        }
        if matches!(en, EnDisco::Carpeta) {
            return Ok(());
        }
        let base = self.estado.registros.get(r).cloned();
        let local = match en {
            EnDisco::Archivo(f) => Some((f, self.md5_local(r, f))),
            _ => None,
        };
        match (&base, &local, &remoto) {
            (None, None, None) => Ok(()),
            (None, Some((f, _)), None) => self.subir(r, None, *f),
            (None, None, Some((id, n))) => self.bajar(r, id, n, EnDisco::Nada),
            (None, Some((f, md5)), Some((id, n))) => {
                if md5.is_some() && *md5 == n.md5 {
                    self.acordar(r, id, n.md5.clone(), *f);
                    Ok(())
                } else {
                    self.divergencia(r, None, *f, id, n, en)
                }
            }
            (Some(b), _, _) => {
                let cambio_local = match &local {
                    None => true,
                    Some((_, md5)) => *md5 != b.md5,
                };
                let cambio_remoto = match &remoto {
                    None => true,
                    Some((id, n)) => *id != b.id || n.md5 != b.md5,
                };
                match (cambio_local, cambio_remoto, &local, &remoto) {
                    (false, false, Some((f, _)), _) => {
                        // Igual en los dos lados: solo se actualiza la firma (p. ej., `touch`).
                        if f.mtime != b.mtime {
                            let mut b = b.clone();
                            b.mtime = f.mtime;
                            self.estado.registros.insert(r.to_owned(), b);
                        }
                        Ok(())
                    }
                    (true, false, None, Some((id, _))) => self.papelera_remota(r, &id.clone()),
                    (true, false, Some((f, _)), Some(_)) => self.subir(r, Some(b.clone()), *f),
                    (false, true, Some(_), None) => self.papelera_local(r),
                    (false, true, Some(_), Some((id, n))) => self.bajar(r, id, n, en),
                    (true, true, None, None) => {
                        self.olvidar(r);
                        Ok(())
                    }
                    (true, true, None, Some((id, n))) => self.bajar(r, id, n, EnDisco::Nada),
                    (true, true, Some((f, _)), None) => {
                        // Borrado en Drive y cambiado acá: se vuelve a subir (no se pierde).
                        self.olvidar(r);
                        self.avisar(
                            &format!("resubida:{r}"),
                            format!("«{r}» se había borrado en Drive y tenía cambios en este equipo: se volvió a subir."),
                        );
                        self.subir(r, None, *f)
                    }
                    (true, true, Some((f, md5)), Some((id, n))) => {
                        if md5.is_some() && *md5 == n.md5 {
                            self.acordar(r, id, n.md5.clone(), *f);
                            Ok(())
                        } else {
                            self.divergencia(r, Some(b.clone()), *f, id, n, en)
                        }
                    }
                    _ => Ok(()),
                }
            }
        }
    }

    fn acordar(&mut self, r: &str, id: &str, md5: Option<String>, f: Firma) {
        self.estado.registros.insert(
            r.to_owned(),
            Registro {
                id: id.to_owned(),
                carpeta: false,
                md5,
                tam: f.tam,
                mtime: f.mtime,
            },
        );
        self.guardar_base(r, id);
    }

    fn olvidar(&mut self, r: &str) {
        if let Some(b) = self.estado.registros.remove(r) {
            let _ = fs::remove_file(estado::ruta_base(&self.cfg.dir_estado, &b.id));
        }
        self.estado.subidas.remove(r);
    }

    /// Guarda la versión acordada de una nota de texto como base de futuras fusiones.
    fn guardar_base(&self, r: &str, id: &str) {
        if !local::es_texto(r) {
            return;
        }
        let ruta = self.cfg.root.join(r);
        if fs::metadata(&ruta).is_ok_and(|m| m.len() <= TOPE_TEXTO)
            && let Ok(b) = fs::read(&ruta)
        {
            let destino = estado::ruta_base(&self.cfg.dir_estado, id);
            let _ = fs::create_dir_all(destino.parent().expect("carpeta de bases"));
            let _ = orbital_vault::write::atomic_write(&destino, &b);
        }
    }

    /// Guarda en el historial el texto que se va a reemplazar (nunca se pierde una versión).
    fn al_historial(&self, r: &str, contenido: &[u8]) {
        if local::es_texto(r)
            && contenido.len() as u64 <= TOPE_TEXTO
            && let (Ok(t), Ok(ruta)) = (
                std::str::from_utf8(contenido),
                orbital_vault::RelPath::new(r),
            )
            && orbital_vault::historial::guardar_version_en(&self.cfg.root, &ruta, t, "drive")
                .is_ok()
        {
            return;
        }
        // Lo que el historial no puede guardar (binario, grande, no UTF-8) va al respaldo: nunca
        // se descarta (la prueba de caos encontró un binario con nombre `.md`).
        let destino = local::libre(
            &self
                .cfg
                .root
                .join(".orbital/respaldo")
                .join(local::marca_de_tiempo())
                .join(r),
        );
        if let Some(p) = destino.parent() {
            let _ = fs::create_dir_all(p);
        }
        let _ = orbital_vault::write::atomic_write(&destino, contenido);
    }

    /// Lo que no es texto y se reemplaza va al respaldo de la bóveda.
    fn al_respaldo(&self, r: &str, origen: &Path) -> std::io::Result<()> {
        let destino = local::libre(
            &self
                .cfg
                .root
                .join(".orbital/respaldo")
                .join(local::marca_de_tiempo())
                .join(r),
        );
        local::mover(origen, &destino)
    }

    fn carpeta_temporal(&self) -> PathBuf {
        self.cfg.root.join(".orbital/sincronizacion")
    }

    /// Id de la carpeta de Drive para una carpeta de la bóveda (la crea si falta).
    fn carpeta_remota(&mut self, rel: &str) -> Resultado<String> {
        if rel.is_empty() {
            return Ok(self.estado.raiz.clone());
        }
        if let Some(id) = self.ruta_id.get(rel)
            && self.estado.nodos.get(id).is_some_and(|n| n.carpeta)
        {
            return Ok(id.clone());
        }
        let padre = self.carpeta_remota(padre_de(rel))?;
        let a = self.api.crear_carpeta(&padre, nombre_de(rel))?;
        self.poner_nodo(&a, &padre);
        if reglas::sincronizable(rel) {
            self.estado.registros.insert(
                rel.to_owned(),
                Registro {
                    id: a.id.clone(),
                    carpeta: true,
                    ..Default::default()
                },
            );
        }
        Ok(a.id)
    }

    fn subir(&mut self, r: &str, base: Option<Registro>, f: Firma) -> Resultado<()> {
        if self.cuota_llena {
            return Ok(());
        }
        let ruta = self.cfg.root.join(r);
        self.emitir(Evento::Uno(r.to_owned(), EstadoArchivo::Subiendo));
        let mut sesion = self.estado.subidas.get(r).cloned();
        let resultado = match &base {
            Some(b) => {
                // Justo antes de subir encima: ¿Drive sigue teniendo lo acordado?
                match self.api.archivo(&b.id) {
                    Ok(a) if !a.papelera && a.md5 == b.md5 => {
                        let mut s = sesion.take();
                        let (estado, dir) = (&mut self.estado, &self.cfg.dir_estado);
                        let r2 = self.api.subir_cambio(&b.id, &ruta, &mut s, &mut |n| {
                            estado.subidas.insert(r.to_owned(), n.clone());
                            let _ = estado.guardar(dir);
                        });
                        sesion = s;
                        r2.map(|a| (a, None))
                    }
                    Ok(a) if !a.papelera => {
                        // Cambió en Drive después de traer los cambios: se resuelve en el próximo ciclo.
                        let padre = a.padres.first().cloned().unwrap_or_default();
                        self.poner_nodo(&a, &padre);
                        self.sucio.insert(r.to_owned());
                        return Ok(());
                    }
                    Ok(_) | Err(ErrorApi::NoEncontrado) => {
                        self.olvidar(r);
                        let padre = self.carpeta_remota(padre_de(r))?;
                        let mut s = sesion.take();
                        let (estado, dir) = (&mut self.estado, &self.cfg.dir_estado);
                        let r2 =
                            self.api
                                .subir_nuevo(&padre, nombre_de(r), &ruta, &mut s, &mut |n| {
                                    estado.subidas.insert(r.to_owned(), n.clone());
                                    let _ = estado.guardar(dir);
                                });
                        sesion = s;
                        r2.map(|a| (a, Some(padre)))
                    }
                    Err(e) => Err(e),
                }
            }
            None => {
                let padre = self.carpeta_remota(padre_de(r))?;
                let mut s = sesion.take();
                let (estado, dir) = (&mut self.estado, &self.cfg.dir_estado);
                let r2 = self
                    .api
                    .subir_nuevo(&padre, nombre_de(r), &ruta, &mut s, &mut |n| {
                        estado.subidas.insert(r.to_owned(), n.clone());
                        let _ = estado.guardar(dir);
                    });
                sesion = s;
                r2.map(|a| (a, Some(padre)))
            }
        };
        match resultado {
            Ok((a, padre)) => {
                self.estado.subidas.remove(r);
                let padre = padre
                    .or_else(|| a.padres.first().cloned())
                    .or_else(|| self.estado.nodos.get(&a.id).map(|n| n.padre.clone()))
                    .unwrap_or_else(|| self.estado.raiz.clone());
                self.poner_nodo(&a, &padre);
                self.estado.registros.insert(
                    r.to_owned(),
                    Registro {
                        id: a.id.clone(),
                        carpeta: false,
                        md5: a.md5.clone(),
                        tam: f.tam,
                        mtime: f.mtime,
                    },
                );
                // Base: solo si el disco tiene justo lo que se subió.
                if local::md5_archivo(&ruta).ok() == a.md5 {
                    self.guardar_base(r, &a.id);
                }
                Ok(())
            }
            Err(e) => {
                match sesion {
                    Some(s) => {
                        self.estado.subidas.insert(r.to_owned(), s);
                    }
                    None => {
                        self.estado.subidas.remove(r);
                    }
                }
                if matches!(e, ErrorApi::Local(_))
                    && !matches!(local::en_disco(&ruta), EnDisco::Archivo(_))
                {
                    // Se borró mientras subía: el próximo ciclo lo trata como borrado.
                    self.sucio.insert(r.to_owned());
                    return Ok(());
                }
                Err(e)
            }
        }
    }

    /// Baja la versión de Drive a un temporal y la pone en su lugar si el disco no cambió mientras.
    fn bajar(&mut self, r: &str, id: &str, n: &Nodo, esperado: EnDisco) -> Resultado<()> {
        self.emitir(Evento::Uno(r.to_owned(), EstadoArchivo::Bajando));
        let tmp = self.bajar_a_temporal(id, n)?;
        let Some(tmp) = tmp else { return Ok(()) };
        let destino = self.cfg.root.join(r);
        if local::en_disco(&destino) != esperado {
            let _ = fs::remove_file(&tmp);
            self.sucio.insert(r.to_owned());
            return Ok(());
        }
        // Lo que se reemplaza va al historial solo si Drive no lo tiene: el historial guarda 20
        // versiones por nota y llenarlo con copias de lo que ya está en Drive desplazaría las
        // únicas (la prueba de caos lo encontró).
        if let EnDisco::Archivo(_) = esperado
            && let Ok(viejo) = fs::read(&destino)
            && self.estado.registros.get(r).and_then(|b| b.md5.as_deref())
                != Some(local::md5_bytes(&viejo).as_str())
        {
            self.al_historial(r, &viejo);
        }
        local::mover(&tmp, &destino).map_err(|e| ErrorApi::Local(e.to_string()))?;
        if let EnDisco::Archivo(f) = local::en_disco(&destino) {
            self.acordar(r, id, n.md5.clone(), f);
        }
        Ok(())
    }

    /// Baja a `.orbital/sincronizacion/<id>.parcial` y verifica la suma. `None` si la suma no
    /// coincide (cambió otra vez en Drive: lo trae el próximo ciclo).
    fn bajar_a_temporal(&mut self, id: &str, n: &Nodo) -> Resultado<Option<PathBuf>> {
        let dir = self.carpeta_temporal();
        fs::create_dir_all(&dir).map_err(|e| ErrorApi::Local(e.to_string()))?;
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
        // El temporal lleva la suma de la versión: un tramo de otra versión nunca se retoma (la
        // prueba de caos encontró una bajada corrupta así), y los de versiones viejas se borran.
        let version = n.md5.as_deref().unwrap_or("sin-suma");
        let tmp = dir.join(format!("{limpio}-{version}.parcial"));
        if let Ok(rd) = fs::read_dir(&dir) {
            let prefijo = format!("{limpio}-");
            for e in rd.flatten() {
                let nombre = e.file_name();
                let nombre = nombre.to_string_lossy();
                if nombre.starts_with(&prefijo) && e.path() != tmp {
                    let _ = fs::remove_file(e.path());
                }
            }
        }
        if fs::metadata(&tmp).is_ok_and(|m| m.len() > n.tam) {
            let _ = fs::remove_file(&tmp);
        }
        match self.api.bajar(id, &tmp) {
            Ok(()) => {}
            Err(ErrorApi::NoEncontrado) => {
                let _ = fs::remove_file(&tmp);
                self.quitar_nodo(id);
                return Ok(None);
            }
            Err(e) => return Err(e),
        }
        // Una bajada cortada que el servidor dio por terminada: es un corte de red, y el tramo se
        // conserva para seguir (la prueba de caos lo encontró).
        let largo = fs::metadata(&tmp).map(|m| m.len()).unwrap_or(0);
        if largo < n.tam {
            return Err(ErrorApi::SinRed(format!(
                "bajada incompleta ({largo} de {} bytes)",
                n.tam
            )));
        }
        let md5 = local::md5_archivo(&tmp).map_err(|e| ErrorApi::Local(e.to_string()))?;
        if n.md5.as_deref().is_some_and(|m| m != md5) {
            let _ = fs::remove_file(&tmp);
            // Se vuelve a pedir el archivo en el próximo ciclo.
            if let Ok(a) = self.api.archivo(id) {
                let padre = a.padres.first().cloned().unwrap_or_default();
                self.estado.nodos.insert(a.id.clone(), Nodo::de(&a, &padre));
            }
            self.completa = true;
            return Ok(None);
        }
        Ok(Some(tmp))
    }

    fn papelera_remota(&mut self, r: &str, id: &str) -> Resultado<()> {
        self.api.a_papelera(id)?;
        self.quitar_nodo(id);
        self.olvidar(r);
        Ok(())
    }

    fn papelera_local(&mut self, r: &str) -> Resultado<()> {
        local::a_papelera(&self.cfg.root, r, &self.cfg.papelera)
            .map_err(|e| ErrorApi::Local(e.to_string()))?;
        self.olvidar(r);
        Ok(())
    }

    /// Los dos lados cambiaron: se fusiona (texto, sin pisarse) o se abre un conflicto.
    fn divergencia(
        &mut self,
        r: &str,
        base: Option<Registro>,
        f: Firma,
        id: &str,
        n: &Nodo,
        en: EnDisco,
    ) -> Resultado<()> {
        self.emitir(Evento::Uno(r.to_owned(), EstadoArchivo::Bajando));
        let Some(tmp) = self.bajar_a_temporal(id, n)? else {
            return Ok(());
        };
        let destino = self.cfg.root.join(r);
        // Fusión de tres vías (decisión 4).
        if let Some(b) = &base
            && local::es_texto(r)
            && f.tam <= TOPE_TEXTO
            && n.tam <= TOPE_TEXTO
            && let Ok(texto_base) =
                fs::read_to_string(estado::ruta_base(&self.cfg.dir_estado, &b.id))
            && let Ok(texto_local) = fs::read_to_string(&destino)
            && let Ok(texto_remoto) = fs::read_to_string(&tmp)
            && let Some(unido) = crate::fusion::fusionar(&texto_base, &texto_local, &texto_remoto)
        {
            if local::en_disco(&destino) != en {
                let _ = fs::remove_file(&tmp);
                self.sucio.insert(r.to_owned());
                return Ok(());
            }
            self.al_historial(r, texto_local.as_bytes());
            self.al_historial(r, texto_remoto.as_bytes());
            let _ = fs::remove_file(&tmp);
            orbital_vault::write::atomic_write(&destino, unido.as_bytes())
                .map_err(|e| ErrorApi::Local(e.to_string()))?;
            // Lo acordado pasa a ser la versión de Drive; la fusión sube en este mismo paso.
            let ff = match local::en_disco(&destino) {
                EnDisco::Archivo(ff) => ff,
                _ => f,
            };
            self.estado.registros.insert(
                r.to_owned(),
                Registro {
                    id: id.to_owned(),
                    carpeta: false,
                    md5: n.md5.clone(),
                    tam: n.tam,
                    mtime: 0,
                },
            );
            self.emitir(Evento::Fusionada(r.to_owned()));
            let b2 = self.estado.registros.get(r).cloned();
            return self.subir(r, b2, ff);
        }
        // Primera unión con una carpeta de Drive que ya tenía contenido: en las notas de texto y
        // la configuración compartida gana la de Drive; la de este equipo va al historial (o al
        // respaldo, la configuración). Nunca se pierde.
        let config = r.starts_with(".orbital/");
        if base.is_none()
            && self.estado.primera_union
            && ((local::es_texto(r) && f.tam <= TOPE_TEXTO) || config)
        {
            if config {
                self.al_respaldo(r, &destino)
                    .map_err(|e| ErrorApi::Local(e.to_string()))?;
            } else if let Ok(viejo) = fs::read(&destino) {
                self.al_historial(r, &viejo);
            }
            local::mover(&tmp, &destino).map_err(|e| ErrorApi::Local(e.to_string()))?;
            if let EnDisco::Archivo(ff) = local::en_disco(&destino) {
                self.acordar(r, id, n.md5.clone(), ff);
            }
            return Ok(());
        }
        // Conflicto: la de Drive se guarda aparte y no se sube nada encima.
        let ext = Path::new(r)
            .extension()
            .map(|e| format!(".{}", e.to_string_lossy()))
            .unwrap_or_default();
        let copia_rel = format!(".orbital/conflictos/{r}/{}{ext}", local::marca_de_tiempo());
        let copia = local::libre(&self.cfg.root.join(&copia_rel));
        local::mover(&tmp, &copia).map_err(|e| ErrorApi::Local(e.to_string()))?;
        let copia_rel = copia
            .strip_prefix(&self.cfg.root)
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or(copia_rel);
        self.estado.conflictos.insert(
            r.to_owned(),
            Conflicto {
                id: id.to_owned(),
                md5: n.md5.clone(),
                copia: copia_rel,
                autor: n.autor.clone(),
                fecha: n.modificado.clone().unwrap_or_default(),
            },
        );
        self.emitir(Evento::Conflicto(r.to_owned()));
        Ok(())
    }

    /// Conflictos abiertos: si Drive cambió otra vez, se guarda también la versión nueva.
    fn conciliar_conflictos(&mut self, disco: &mut HashMap<String, EnDisco>) -> Resultado<()> {
        let abiertos: Vec<(String, Conflicto)> = self
            .estado
            .conflictos
            .iter()
            .map(|(r, c)| (r.clone(), c.clone()))
            .collect();
        for (r, c) in abiertos {
            let nodo = self.estado.nodos.get(&c.id).cloned();
            if let Some(n) = &nodo
                && n.md5 != c.md5
                && let Some(tmp) = self.bajar_a_temporal(&c.id, n)?
            {
                let ext = Path::new(&r)
                    .extension()
                    .map(|e| format!(".{}", e.to_string_lossy()))
                    .unwrap_or_default();
                let copia = local::libre(&self.cfg.root.join(format!(
                    ".orbital/conflictos/{r}/{}{ext}",
                    local::marca_de_tiempo()
                )));
                local::mover(&tmp, &copia).map_err(|e| ErrorApi::Local(e.to_string()))?;
                if let Some(cc) = self.estado.conflictos.get_mut(&r) {
                    cc.md5 = n.md5.clone();
                    cc.copia = copia
                        .strip_prefix(&self.cfg.root)
                        .map(|p| p.to_string_lossy().into_owned())
                        .unwrap_or_default();
                    cc.autor = n.autor.clone();
                    cc.fecha = n.modificado.clone().unwrap_or_default();
                }
            }
            // Si la de este equipo ya no está, gana la de Drive.
            if matches!(self.disco(disco, &r), EnDisco::Nada) {
                self.resolver(&r, false)?;
            }
        }
        Ok(())
    }

    /// Resuelve un conflicto. La versión descartada va al historial (texto) o al respaldo.
    pub fn resolver(&mut self, r: &str, mia: bool) -> Resultado<()> {
        let Some(c) = self.estado.conflictos.get(r).cloned() else {
            return Ok(());
        };
        let destino = self.cfg.root.join(r);
        let copia = self.cfg.root.join(&c.copia);
        if mia && let EnDisco::Archivo(f) = local::en_disco(&destino) {
            // Si Drive cambió otra vez desde que se abrió el conflicto, se guarda también esa
            // versión y el conflicto sigue: no se sube nada encima de algo que la usuaria no vio.
            match self.api.archivo(&c.id) {
                Ok(a) if !a.papelera && a.md5 != c.md5 => {
                    let padre = a.padres.first().cloned().unwrap_or_default();
                    self.poner_nodo(&a, &padre);
                    let mut disco = HashMap::new();
                    self.conciliar_conflictos(&mut disco)?;
                    self.emitir(Evento::Conflicto(r.to_owned()));
                    return Ok(());
                }
                Ok(_) | Err(ErrorApi::NoEncontrado) => {}
                Err(e) => return Err(e),
            }
            // Se sube la de este equipo encima de la de Drive (que ya está guardada aparte).
            let b = Registro {
                id: c.id.clone(),
                carpeta: false,
                md5: c.md5.clone(),
                tam: 0,
                mtime: 0,
            };
            self.estado.registros.insert(r.to_owned(), b.clone());
            self.estado.conflictos.remove(r);
            if let Err(e) = self.subir(r, Some(b), f) {
                self.estado.conflictos.insert(r.to_owned(), c);
                return Err(e);
            }
            self.descartar_copia(r, &copia);
        } else {
            // Gana la de Drive: la de este equipo se guarda antes de reemplazarla.
            if let Ok(viejo) = fs::read(&destino) {
                if local::es_texto(r) && viejo.len() as u64 <= TOPE_TEXTO {
                    self.al_historial(r, &viejo);
                } else {
                    self.al_respaldo(r, &destino)
                        .map_err(|e| ErrorApi::Local(e.to_string()))?;
                }
            }
            if copia.exists() {
                local::mover(&copia, &destino).map_err(|e| ErrorApi::Local(e.to_string()))?;
            }
            self.estado.conflictos.remove(r);
            if let EnDisco::Archivo(f) = local::en_disco(&destino) {
                self.acordar(r, &c.id, c.md5.clone(), f);
            }
            self.sucio.insert(r.to_owned());
        }
        let _ = self.estado.guardar(&self.cfg.dir_estado);
        self.emitir_estados();
        Ok(())
    }

    /// La copia de Drive descartada: al historial si es texto; si no, al respaldo.
    fn descartar_copia(&self, r: &str, copia: &Path) {
        if let Ok(b) = fs::read(copia)
            && local::es_texto(r)
            && b.len() as u64 <= TOPE_TEXTO
        {
            self.al_historial(r, &b);
            let _ = fs::remove_file(copia);
        } else {
            let _ = self.al_respaldo(r, copia);
        }
    }

    fn conciliar_carpeta(
        &mut self,
        c: &str,
        disco: &mut HashMap<String, EnDisco>,
    ) -> Resultado<()> {
        if self.excluida(c) {
            return Ok(());
        }
        let en = self.disco(disco, c);
        let remoto = self
            .ruta_id
            .get(c)
            .filter(|id| self.estado.nodos.get(*id).is_some_and(|n| n.carpeta))
            .cloned();
        let base = self.estado.registros.get(c).cloned();
        match (base, en, remoto) {
            (_, EnDisco::Carpeta, Some(id)) => {
                self.estado.registros.insert(
                    c.to_owned(),
                    Registro {
                        id,
                        carpeta: true,
                        ..Default::default()
                    },
                );
            }
            (None, EnDisco::Carpeta, None) => {
                self.carpeta_remota(c)?;
            }
            (None, EnDisco::Nada, Some(id)) => {
                fs::create_dir_all(self.cfg.root.join(c))
                    .map_err(|e| ErrorApi::Local(e.to_string()))?;
                disco.insert(c.to_owned(), EnDisco::Carpeta);
                self.estado.registros.insert(
                    c.to_owned(),
                    Registro {
                        id,
                        carpeta: true,
                        ..Default::default()
                    },
                );
            }
            (Some(_), EnDisco::Nada, Some(id)) => {
                // Borrada en el disco: si en Drive quedó vacía, a la papelera de Drive. Si tiene
                // algo nuevo, se vuelve a crear acá (lo bajan los archivos).
                let tiene_hijos = self.estado.nodos.values().any(|n| n.padre == id);
                if tiene_hijos {
                    fs::create_dir_all(self.cfg.root.join(c))
                        .map_err(|e| ErrorApi::Local(e.to_string()))?;
                    self.completa = true;
                } else {
                    self.api.a_papelera(&id)?;
                    self.quitar_nodo(&id);
                    self.estado.registros.remove(c);
                }
            }
            (Some(_), EnDisco::Carpeta, None) => {
                // Borrada en Drive: si acá quedó vacía, se saca; si no, se vuelve a crear allá.
                let dir = self.cfg.root.join(c);
                let vacia = fs::read_dir(&dir)
                    .map(|mut d| d.next().is_none())
                    .unwrap_or(false);
                self.estado.registros.remove(c);
                if vacia {
                    let _ = fs::remove_dir(&dir);
                } else {
                    self.carpeta_remota(c)?;
                    self.completa = true;
                }
            }
            (Some(_), EnDisco::Nada, None) => {
                self.estado.registros.remove(c);
            }
            _ => {}
        }
        Ok(())
    }

    // --- estados para la interfaz ----------------------------------------------------------------

    /// Mapa ruta → estado, con las carpetas resumidas.
    pub fn mapa(&self) -> HashMap<String, EstadoArchivo> {
        let mut m: HashMap<String, EstadoArchivo> =
            HashMap::with_capacity(self.estado.registros.len());
        for (r, b) in &self.estado.registros {
            if !b.carpeta {
                m.insert(r.clone(), EstadoArchivo::Ambos);
            } else {
                m.entry(r.clone()).or_insert(EstadoArchivo::Ambos);
            }
        }
        for r in self.pendientes.iter().chain(self.estado.subidas.keys()) {
            if !self.excluida(r) && self.cfg.root.join(r).is_file() {
                m.insert(r.clone(), EstadoArchivo::Subiendo);
            }
        }
        for r in self.estado.conflictos.keys() {
            m.insert(r.clone(), EstadoArchivo::Conflicto);
        }
        // Carpetas: el peor estado de su contenido.
        let hojas: Vec<(String, EstadoArchivo)> = m
            .iter()
            .filter(|(_, e)| **e != EstadoArchivo::Ambos)
            .map(|(r, e)| (r.clone(), *e))
            .collect();
        for (r, e) in hojas {
            let mut p = padre_de(&r);
            while !p.is_empty() {
                let v = m.entry(p.to_owned()).or_insert(EstadoArchivo::Ambos);
                if e.peso() > v.peso() {
                    *v = e;
                }
                p = padre_de(p);
            }
        }
        m
    }

    pub fn emitir_estados(&mut self) {
        let m = Arc::new(self.mapa());
        if self.ultimo_mapa.as_deref() != Some(&*m) {
            self.ultimo_mapa = Some(m.clone());
            let reglas = Arc::new(self.conexion.solo_local.clone());
            self.emitir(Evento::Estados(m, reglas));
        }
        let c: Vec<InfoConflicto> = self
            .estado
            .conflictos
            .iter()
            .map(|(r, c)| InfoConflicto {
                ruta: r.clone(),
                copia: c.copia.clone(),
                autor: c.autor.clone(),
                fecha: c.fecha.clone(),
            })
            .collect();
        if self.ultimos_conflictos.as_deref() != Some(&c) {
            let c = Arc::new(c);
            self.ultimos_conflictos = Some(c.clone());
            self.emitir(Evento::Conflictos(c));
        }
    }

    /// Manda a la papelera de Drive lo que hay en una ruta (y debajo, si es carpeta) que pasó a
    /// «solo en este equipo». El disco no se toca.
    pub fn quitar_de_drive(&mut self, r: &str) -> Resultado<()> {
        let ids: Vec<(String, String)> = self
            .id_ruta
            .iter()
            .filter(|(_, ruta)| {
                ruta.as_str() == r || ruta.strip_prefix(r).is_some_and(|x| x.starts_with('/'))
            })
            .map(|(id, ruta)| (id.clone(), ruta.clone()))
            .collect();
        // Primero lo de más arriba: si la carpeta va a la papelera, su contenido va con ella.
        let mut ids = ids;
        ids.sort_by_key(|(_, ruta)| ruta.matches('/').count());
        for (id, ruta) in ids {
            if !self.estado.nodos.contains_key(&id) {
                continue;
            }
            self.api.a_papelera(&id)?;
            let hijos: Vec<String> = self
                .id_ruta
                .iter()
                .filter(|(_, x)| {
                    x.strip_prefix(ruta.as_str())
                        .is_some_and(|y| y.starts_with('/'))
                })
                .map(|(i, _)| i.clone())
                .collect();
            for h in hijos {
                self.quitar_nodo(&h);
            }
            self.quitar_nodo(&id);
        }
        let quitar: Vec<String> = self
            .estado
            .registros
            .keys()
            .filter(|x| x.as_str() == r || x.strip_prefix(r).is_some_and(|y| y.starts_with('/')))
            .cloned()
            .collect();
        for x in quitar {
            self.olvidar(&x);
        }
        let _ = self.estado.guardar(&self.cfg.dir_estado);
        self.emitir_estados();
        Ok(())
    }

    pub fn conflictos(&self) -> usize {
        self.estado.conflictos.len()
    }

    pub fn por_subir(&self) -> usize {
        self.pendientes.len() + self.estado.subidas.len() + self.sucio.len()
    }

    /// El enlace de Drive de una ruta.
    pub fn enlace(&self, r: &str) -> Option<String> {
        let id = self.ruta_id.get(r)?;
        self.estado.nodos.get(id).and_then(|n| n.enlace.clone())
    }
}

/// Conecta una bóveda: crea la carpeta en Drive (o usa una existente), escribe `drive.yaml` y el
/// estado del equipo. No sube nada: eso lo hace el motor.
pub fn conectar(
    api: &mut dyn Api,
    root: &Path,
    dir_estado: &Path,
    cuenta: &str,
    carpeta: Destino,
) -> Resultado<Conexion> {
    let (id, existente) = match carpeta {
        Destino::Nueva(nombre) => (api.crear_carpeta("root", &nombre)?.id, false),
        Destino::Existente(texto) => {
            let id = id_de_enlace(&texto).ok_or_else(|| {
                ErrorApi::Servidor(0, "no es un enlace de carpeta de Drive".into())
            })?;
            let a = api.archivo(&id)?;
            if !a.carpeta() {
                return Err(ErrorApi::Servidor(
                    0,
                    "el enlace no es de una carpeta".into(),
                ));
            }
            (a.id, true)
        }
    };
    // Si la carpeta ya trae una conexión (otro equipo), se conservan sus reglas: se baja su
    // `drive.yaml` antes de nada.
    if existente {
        traer_conexion(api, root, &id)?;
    }
    let mut conexion = Conexion::leer(root).ok().flatten().unwrap_or_default();
    conexion.cuenta = cuenta.to_owned();
    conexion.carpeta = id.clone();
    conexion
        .guardar(root)
        .map_err(|e| ErrorApi::Local(e.to_string()))?;
    let mut e = Estado::nuevo(cuenta, &id);
    e.primera_union = existente;
    e.guardar(dir_estado)
        .map_err(|e| ErrorApi::Local(e.to_string()))?;
    Ok(conexion)
}

/// Baja `.orbital/drive.yaml` de la carpeta de Drive, si tiene.
fn traer_conexion(api: &mut dyn Api, root: &Path, carpeta: &str) -> Resultado<()> {
    let buscar = |api: &mut dyn Api, padre: &str, nombre: &str| -> Resultado<Option<Archivo>> {
        let mut pagina: Option<String> = None;
        loop {
            let (lista, sig) = api.hijos(padre, pagina.as_deref())?;
            if let Some(a) = lista.into_iter().find(|a| a.nombre == nombre) {
                return Ok(Some(a));
            }
            match sig {
                Some(s) => pagina = Some(s),
                None => return Ok(None),
            }
        }
    };
    let Some(orbital) = buscar(api, carpeta, ".orbital")? else {
        return Ok(());
    };
    let Some(yaml) = buscar(api, &orbital.id, "drive.yaml")? else {
        return Ok(());
    };
    let destino = root.join(crate::ARCHIVO_CONEXION);
    fs::create_dir_all(destino.parent().expect("carpeta .orbital"))
        .map_err(|e| ErrorApi::Local(e.to_string()))?;
    let tmp = destino.with_extension("yaml.parcial");
    let _ = fs::remove_file(&tmp);
    api.bajar(&yaml.id, &tmp)?;
    fs::rename(&tmp, &destino).map_err(|e| ErrorApi::Local(e.to_string()))
}

/// Adónde conectar la bóveda.
#[derive(Clone, Debug)]
pub enum Destino {
    /// Una carpeta nueva en «Mi unidad» con este nombre.
    Nueva(String),
    /// Una carpeta existente (enlace o id).
    Existente(String),
}

/// Saca el id de un enlace de carpeta de Drive (`…/folders/<id>?…`, `?id=<id>`) o lo toma tal cual.
pub fn id_de_enlace(texto: &str) -> Option<String> {
    let t = texto.trim();
    let id = if let Some(p) = t.find("/folders/") {
        &t[p + "/folders/".len()..]
    } else if let Some(p) = t.find("id=") {
        &t[p + 3..]
    } else {
        t
    };
    let id: String = id
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .collect();
    (id.len() >= 10 || (!id.is_empty() && !t.contains('/'))).then_some(id)
}

// --- el hilo -----------------------------------------------------------------------------------

/// El motor en su hilo.
pub struct Motor {
    tx: mpsc::Sender<Orden>,
    hilo: Option<JoinHandle<()>>,
}

impl Motor {
    /// Arranca el motor. `fabrica` crea la API **dentro** del hilo (libsoup no se pasa entre
    /// hilos). `emitir` recibe los eventos en el hilo del motor (la app los pasa a GTK).
    pub fn iniciar<F, E>(cfg: Config, fabrica: F, emitir: E) -> std::io::Result<Self>
    where
        F: FnOnce(&Estado) -> Result<Box<dyn Api>, String> + Send + 'static,
        E: Fn(Evento) + Send + Sync + 'static,
    {
        let (tx, rx) = mpsc::channel();
        let hilo = std::thread::Builder::new()
            .name("orbital-drive".into())
            .spawn(move || bucle(cfg, fabrica, Arc::new(emitir), rx))?;
        Ok(Self {
            tx,
            hilo: Some(hilo),
        })
    }

    pub fn enviar(&self, o: Orden) {
        let _ = self.tx.send(o);
    }

    /// Pide terminar y espera a que el hilo guarde el estado.
    pub fn detener(mut self) {
        let _ = self.tx.send(Orden::Detener);
        if let Some(h) = self.hilo.take() {
            let _ = h.join();
        }
    }
}

impl Drop for Motor {
    fn drop(&mut self) {
        let _ = self.tx.send(Orden::Detener);
    }
}

fn bucle<F>(
    cfg: Config,
    fabrica: F,
    emitir: Arc<dyn Fn(Evento) + Send + Sync>,
    rx: mpsc::Receiver<Orden>,
) where
    F: FnOnce(&Estado) -> Result<Box<dyn Api>, String>,
{
    let _candado = match Candado::tomar(&cfg.dir_estado) {
        Ok(c) => c,
        Err(e) => return emitir(Evento::Detenido(Some(e.to_string()))),
    };
    let estado = match Estado::leer(&cfg.dir_estado) {
        Ok(Some(e)) => e,
        Ok(None) => {
            return emitir(Evento::Detenido(Some(
                "este equipo no está conectado".into(),
            )));
        }
        Err(e) => return emitir(Evento::Detenido(Some(e.to_string()))),
    };
    let conexion = Conexion::leer(&cfg.root).ok().flatten().unwrap_or_default();
    let api = match fabrica(&estado) {
        Ok(a) => a,
        Err(e) => return emitir(Evento::Detenido(Some(e))),
    };
    let em = emitir.clone();
    let mut s = Sincronizador::new(cfg.clone(), api, estado, conexion, Box::new(move |e| em(e)));
    let mut activo = true;
    let mut en_linea = true;
    let mut pausa: Option<String> = None;
    let mut error: Option<String> = None;
    let mut espera_error = Duration::ZERO;
    let mut proximo = Instant::now();
    let mut pendientes: HashMap<String, Instant> = HashMap::new();
    let resumen = |s: &Sincronizador<Box<dyn Api>>,
                   en_linea: bool,
                   sincronizando: bool,
                   pausa: &Option<String>,
                   error: &Option<String>| {
        Evento::Resumen(Resumen {
            cuenta: s.estado.cuenta.clone(),
            en_linea,
            sincronizando,
            pendientes: s.por_subir(),
            conflictos: s.conflictos(),
            pausa: pausa.clone(),
            error: error.clone(),
            ultima: s.ultima.clone(),
        })
    };
    s.emitir_estados();
    loop {
        let ahora = Instant::now();
        let hasta_pendiente = pendientes.values().min().map(|t| *t + cfg.espera_subida);
        let siguiente = hasta_pendiente.map_or(proximo, |p| p.min(proximo));
        let espera = siguiente.saturating_duration_since(ahora);
        match rx.recv_timeout(espera) {
            Ok(Orden::Cambios(v)) => {
                let t = Instant::now();
                for r in v {
                    pendientes.insert(r, t);
                }
                s.pendientes = pendientes.keys().cloned().collect();
                s.emitir_estados();
                continue;
            }
            Ok(Orden::Foco(f)) => {
                activo = f;
                if f {
                    proximo = proximo.min(Instant::now() + Duration::from_secs(2));
                }
                continue;
            }
            Ok(Orden::Red(r)) => {
                en_linea = r;
                if r {
                    espera_error = Duration::ZERO;
                    proximo = Instant::now();
                }
                emitir(resumen(&s, en_linea, false, &pausa, &error));
                continue;
            }
            Ok(Orden::Ahora) => {
                pausa = None;
                proximo = Instant::now();
            }
            Ok(Orden::Resolver { ruta, mia }) => {
                if let Err(e) = s.resolver(&ruta, mia) {
                    emitir(Evento::Aviso(format!("No se pudo resolver «{ruta}»: {e}")));
                }
                emitir(resumen(&s, en_linea, false, &pausa, &error));
                continue;
            }
            Ok(Orden::Enlace(r)) => {
                emitir(Evento::Enlace(r.clone(), s.enlace(&r)));
                continue;
            }
            Ok(Orden::QuitarDeDrive(r)) => {
                if let Err(e) = s.quitar_de_drive(&r) {
                    emitir(Evento::Aviso(format!(
                        "No se pudo sacar «{r}» de Drive: {e}"
                    )));
                }
                continue;
            }
            Ok(Orden::Detener) | Err(mpsc::RecvTimeoutError::Disconnected) => {
                let _ = s.estado.guardar(&cfg.dir_estado);
                emitir(Evento::Detenido(None));
                return;
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
        }
        let ahora = Instant::now();
        let listos: Vec<String> = pendientes
            .iter()
            .filter(|(_, t)| **t + cfg.espera_subida <= ahora)
            .map(|(r, _)| r.clone())
            .collect();
        for r in &listos {
            pendientes.remove(r);
        }
        s.marcar(listos.iter().cloned());
        s.pendientes = pendientes.keys().cloned().collect();
        let toca = ahora >= proximo || !listos.is_empty();
        if !toca || !en_linea || pausa.is_some() {
            if ahora >= proximo {
                proximo = ahora + cfg.intervalo_fondo;
            }
            continue;
        }
        emitir(resumen(&s, en_linea, true, &pausa, &error));
        match s.ciclo() {
            Ok(()) => {
                espera_error = Duration::ZERO;
                error = None;
                if ahora >= proximo {
                    proximo = ahora
                        + if activo {
                            cfg.intervalo_activo
                        } else {
                            cfg.intervalo_fondo
                        };
                }
            }
            Err(ErrorApi::Autorizacion(m)) => {
                pausa = Some(format!(
                    "La sesión de Google ya no es válida ({m}). Volvé a iniciar sesión."
                ));
            }
            Err(e) => {
                espera_error =
                    (espera_error * 2).clamp(Duration::from_secs(5), Duration::from_secs(300));
                proximo = Instant::now() + espera_error;
                error = Some(e.to_string());
            }
        }
        emitir(resumen(&s, en_linea, false, &pausa, &error));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_de_enlaces() {
        assert_eq!(
            id_de_enlace("https://drive.google.com/drive/folders/1AbCdEfGhIjKlMnOp?usp=sharing")
                .as_deref(),
            Some("1AbCdEfGhIjKlMnOp")
        );
        assert_eq!(
            id_de_enlace("https://drive.google.com/drive/u/0/folders/1AbCdEfGhIjKlMnOp").as_deref(),
            Some("1AbCdEfGhIjKlMnOp")
        );
        assert_eq!(
            id_de_enlace("1AbCdEfGhIjKlMnOp").as_deref(),
            Some("1AbCdEfGhIjKlMnOp")
        );
        assert_eq!(id_de_enlace("https://example.com/x"), None);
    }
}
