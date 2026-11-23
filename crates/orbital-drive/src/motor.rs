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

use crate::api::{Api, Archivo, Cuota, ErrorApi, Resultado};
use crate::estado::{self, Candado, Conflicto, Estado, Marcador, Nodo, Pendiente, Registro};
pub use crate::local::Papelera;
use crate::local::{self, EnDisco, Firma};
use crate::reglas::{self, Conexion, Ubicacion};

/// Tope para guardar una versión de texto en el historial o usarla como base de una fusión, y
/// para el extracto de lo «solo en Drive» (decisión 3).
pub const TOPE_TEXTO: u64 = 1024 * 1024;

/// Tope de la caché de lo «solo en Drive» abierto (los extractos de texto no cuentan).
pub const TOPE_CACHE: u64 = 1 << 30;

/// Reglas vigentes para la interfaz: las compartidas «solo en este equipo» y las de este equipo.
pub type Reglas = Vec<(String, Ubicacion)>;

/// Estado de una ruta para el explorador (FR-011; «bloqueado sin conexión» lo deduce la app de
/// «solo en Drive» y la red).
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
    /// Solo en Drive (061b): un marcador, sin copia en la bóveda.
    SoloDrive,
    /// Nuevo en este equipo y sin subir por el predeterminado «Solo en este equipo».
    Local,
    /// «Solo en Drive» con un cambio en cola (renombrado o movido sin conexión): se aplica en
    /// Drive cuando vuelva la red.
    Pendiente,
}

impl EstadoArchivo {
    /// Prioridad para resumir una carpeta (el peor estado de su contenido).
    fn peso(self) -> u8 {
        match self {
            Self::Ambos | Self::SoloDrive | Self::Local => 0,
            Self::Bajando => 1,
            Self::Subiendo | Self::Pendiente => 2,
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
    /// Espacio de la cuenta (se consulta en cada recorrido completo).
    pub cuota: Option<Cuota>,
    /// Predeterminados de este equipo para lo nuevo (061b).
    pub nuevo_local: Ubicacion,
    pub nuevo_remoto: Ubicacion,
    /// Cuántos archivos están «solo en Drive».
    pub solo_drive: usize,
}

/// Lo que el motor le avisa a la interfaz.
#[derive(Clone, Debug)]
pub enum Evento {
    /// Mapa completo ruta → estado (carpetas incluidas, con el peor estado de su contenido) y las
    /// reglas vigentes (las compartidas «solo en este equipo» y las de este equipo).
    Estados(Arc<HashMap<String, EstadoArchivo>>, Arc<Reglas>),
    /// Lo «solo en Drive» cambió (la app lo suma al árbol; también quedó en `marcadores`).
    Marcadores(Arc<Vec<Marcador>>),
    /// Respuesta a [`Orden::Traer`]: dónde quedó la copia para abrirla, o por qué no se pudo
    /// (también si se canceló: [`CANCELADA`]), con el número de pedido.
    Traido(String, u64, Result<PathBuf, String>),
    /// Cuánto lleva bajado lo que se pidió con [`Orden::Traer`] (bytes; como mucho unas 8 veces
    /// por segundo).
    Progreso {
        ruta: String,
        pedido: u64,
        bajado: u64,
        total: u64,
    },
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
    /// Regla de este equipo para una ruta (carpetas con `/` final): «en ambos» o «solo en Drive».
    /// `None` la quita. «Solo en este equipo» es compartida y va en `drive.yaml`.
    Ubicar(String, Option<Ubicacion>),
    /// Predeterminados para lo nuevo de este equipo y lo nuevo en Drive.
    Predeterminados {
        local: Ubicacion,
        remoto: Ubicacion,
    },
    /// Traer a la caché un archivo «solo en Drive» para abrirlo o mostrarlo incrustado. El número
    /// es el pedido, para cancelarlo con [`Motor::cancelar`] (0: no se cancela).
    Traer(String, u64),
    /// La nota abierta en el editor: no se le libera espacio mientras se edita.
    Abierta(Option<String>),
    /// Mandar a la papelera de Drive un archivo «solo en Drive» (borrarlo desde el explorador).
    Borrar(String),
    /// Renombrar o mover un archivo «solo en Drive» (mismo id).
    Mover {
        de: String,
        a: String,
    },
    Detener,
}

/// Lo que dice [`Evento::Traido`] cuando la usuaria canceló la bajada.
pub const CANCELADA: &str = "se canceló";

/// Pedidos de [`Orden::Traer`] cancelados. Se comparte entre la app y el hilo del motor: el
/// motor está ocupado bajando y no lee órdenes, así que mira esto entre tramo y tramo.
/// Cada pedido cancelado lleva desde cuántos bytes bajados se corta (0: enseguida).
#[derive(Clone, Debug, Default)]
pub struct Cancelador(Arc<std::sync::Mutex<BTreeMap<u64, u64>>>);

impl Cancelador {
    fn mapa(&self) -> std::sync::MutexGuard<'_, BTreeMap<u64, u64>> {
        self.0.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn cancelar(&self, pedido: u64) {
        self.cancelar_en(pedido, 0);
    }

    /// Cancela el pedido cuando lleve `bytes` bajados (la prueba de caos corta así a mitad).
    pub fn cancelar_en(&self, pedido: u64, bytes: u64) {
        if pedido != 0 {
            self.mapa().insert(pedido, bytes);
        }
    }

    /// Si el pedido está cancelado para lo que lleva bajado.
    fn cortar(&self, pedido: u64, hay: u64) -> bool {
        pedido != 0 && self.mapa().get(&pedido).is_some_and(|t| hay >= *t)
    }

    fn cancelado(&self, pedido: u64) -> bool {
        pedido != 0 && self.mapa().contains_key(&pedido)
    }

    fn olvidar(&self, pedido: u64) {
        self.mapa().remove(&pedido);
    }
}

/// La bajada que se está mostrando (la de un [`Orden::Traer`]).
struct EnCurso {
    ruta: String,
    pedido: u64,
    ultimo: Option<Instant>,
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
    /// Caché de lo «solo en Drive» de este equipo ([`estado::carpeta_cache`]).
    pub dir_cache: PathBuf,
    /// Tope de la caché (sin contar los extractos de texto).
    pub tope_cache: u64,
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
            dir_cache: estado::carpeta_cache(root),
            tope_cache: TOPE_CACHE,
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
    /// El estado cambió desde la última vez que se guardó (y el mapa, desde que se emitió).
    mutado: bool,
    pendientes_emitidas: BTreeSet<String>,
    ultimos_marcadores: Option<Arc<Vec<Marcador>>>,
    /// Lo nuevo de este equipo que no se sube por el predeterminado «Solo en este equipo».
    locales_defecto: BTreeSet<String>,
    pub cuota: Option<Cuota>,
    /// La nota abierta en el editor (no se libera mientras está abierta).
    pub abierta: Option<String>,
    /// Solo pruebas: corta «Liberar espacio» justo después de guardar el estado y antes de sacar
    /// la copia de la bóveda (como un corte de luz).
    pub cortar_liberar: bool,
    /// Pedidos cancelados (compartido con la app).
    pub cancelador: Cancelador,
    en_curso: Option<EnCurso>,
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
            mutado: true,
            pendientes_emitidas: BTreeSet::new(),
            ultimos_marcadores: None,
            locales_defecto: BTreeSet::new(),
            cuota: None,
            abierta: None,
            cortar_liberar: false,
            cancelador: Cancelador::default(),
            en_curso: None,
        };
        s.indexar();
        s
    }

    /// El estado para cambiarlo (lo marca para guardar).
    fn e(&mut self) -> &mut Estado {
        self.mutado = true;
        &mut self.estado
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
        self.e().nodos.insert(a.id.clone(), n);
        if let Some(r) = ruta {
            // Otro archivo con el mismo nombre en la carpeta (Drive lo permite): los dos se
            // numeran (` (2)`) en orden de id, como en `Estado::rutas`; se rearma el índice. La
            // prueba de caos encontró un marcador que quedaba con la ruta del otro.
            if self.ruta_id.get(&r).is_some_and(|otro| *otro != a.id) {
                self.indexar();
                return;
            }
            self.ruta_id.insert(r.clone(), a.id.clone());
            self.id_ruta.insert(a.id.clone(), r);
        }
    }

    fn quitar_nodo(&mut self, id: &str) {
        self.e().nodos.remove(id);
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
        if self.mutado {
            self.recortar_cache(None);
        }
        // Solo si algo cambió: el estado de 5000 notas son ≈ 2 MB (medido en el quickstart).
        if self.mutado {
            let _ = self.estado.guardar(&self.cfg.dir_estado);
        }
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
        if !self.estado.cola.is_empty() {
            self.aplicar_cola()?;
        }
        let completa = std::mem::take(&mut self.completa);
        let mut disco: HashMap<String, EnDisco> = HashMap::new();
        let rutas: BTreeSet<String> = if completa {
            let recorrido = local::recorrer(&self.cfg.root, &|r| self.excluida(r));
            self.locales_defecto.clear();
            // La cuota, una vez por recorrido completo (cada 10 min): para el diálogo.
            if let Ok(c) = self.api.cuota()
                && self.cuota != Some(c)
            {
                self.cuota = Some(c);
            }
            let mut todas: BTreeSet<String> = recorrido.keys().cloned().collect();
            todas.extend(self.estado.registros.keys().cloned());
            todas.extend(self.ruta_id.keys().cloned());
            todas.extend(std::mem::take(&mut self.sucio));
            disco.extend(recorrido);
            todas
        } else {
            std::mem::take(&mut self.sucio)
        };
        let res = self.conciliar(rutas.clone(), &mut disco);
        if res.is_err() {
            // Lo que no se hizo vuelve a quedar marcado para el próximo ciclo.
            self.sucio.extend(rutas);
            self.completa |= completa;
        } else if completa {
            self.ultima_completa = Instant::now();
            self.e().primera_union = false;
        }
        res
    }

    /// Primer listado de la carpeta (y de cualquier carpeta que entra a la bóveda).
    fn listado_inicial(&mut self) -> Resultado<()> {
        let token = self.api.token_inicio()?;
        let raiz = self.estado.raiz.clone();
        self.listar_desde(&raiz)?;
        self.e().token = Some(token);
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
                    self.e().nodos.insert(a.id.clone(), Nodo::de(&a, &c));
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
        // Lo común: ninguna novedad. Se responde sin copiar índices ni recalcular rutas.
        let primera = self.api.cambios(&token)?;
        if primera.cambios.is_empty() && primera.siguiente.is_none() {
            if let Some(n) = primera.nuevo_inicio
                && n != token
            {
                self.e().token = Some(n);
            }
            return Ok(());
        }
        let mut primera = Some(primera);
        let antes = self.id_ruta.clone();
        let mut tocados = BTreeSet::new();
        loop {
            let pag = match primera.take() {
                Some(p) => p,
                None => self.api.cambios(&token)?,
            };
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
                        self.e().nodos.insert(a.id.clone(), Nodo::de(&a, &padre));
                    }
                    _ => {
                        self.e().nodos.remove(&c.id);
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
        self.e().podar();
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
        self.e().token = Some(token);
        Ok(())
    }

    /// Las reglas vigentes: las compartidas «solo en este equipo» y las de este equipo.
    fn reglas_vigentes(&self) -> impl Iterator<Item = (&str, Ubicacion)> {
        self.conexion
            .solo_local
            .iter()
            .map(|r| (r.as_str(), Ubicacion::Local))
            .chain(self.estado.reglas.iter().map(|(r, u)| (r.as_str(), *u)))
    }

    /// La regla más específica que cubre `rel` (sin regla: `None`).
    pub fn regla(&self, rel: &str) -> Option<Ubicacion> {
        if self.conexion.solo_local.is_empty() && self.estado.reglas.is_empty() {
            return None;
        }
        reglas::ubicacion(self.reglas_vigentes(), rel)
    }

    fn excluida(&self, rel: &str) -> bool {
        !reglas::sincronizable(rel) || self.regla(rel) == Some(Ubicacion::Local)
    }

    /// Si un archivo acordado «en ambos» tiene que pasar a «solo en Drive».
    fn toca_liberar(&self, r: &str, b: &Registro) -> bool {
        !b.carpeta
            && !b.solo_drive
            && !r.starts_with(".orbital/")
            && self.abierta.as_deref() != Some(r)
            && match self.regla(r) {
                Some(u) => u == Ubicacion::SoloDrive,
                None => b.liberar,
            }
    }

    /// Si un marcador tiene que volver a la bóveda (regla «en ambos» o «solo en este equipo»).
    fn toca_mantener(&self, r: &str) -> bool {
        matches!(
            self.regla(r),
            Some(Ubicacion::Ambos) | Some(Ubicacion::Local)
        )
    }

    fn copia(&self, b: &Registro) -> PathBuf {
        let nombre = self
            .estado
            .nodos
            .get(&b.id)
            .map(|n| n.nombre.as_str())
            .unwrap_or("archivo");
        estado::ruta_copia(&self.cfg.dir_cache, &b.id, nombre)
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
        self.movimientos_de_carpetas(&mut rutas, disco)?;
        self.movimientos_locales(&mut rutas, disco)?;
        self.movimientos_remotos(&mut rutas, disco);
        let mut carpetas = Vec::new();
        for r in &rutas {
            if self.excluida(r) {
                // Un marcador que pasó a «solo en este equipo»: primero vuelve a la bóveda.
                if reglas::sincronizable(r)
                    && self
                        .estado
                        .registros
                        .get(r)
                        .is_some_and(|b| b.solo_drive && !b.carpeta)
                {
                    match self.mantener(r) {
                        Ok(()) => self.olvidar(r),
                        Err(e) if e.transitorio() => return Err(e),
                        Err(e) => self.avisar(
                            &format!("error:{r}"),
                            format!("No se pudo traer «{r}» a este equipo: {e}"),
                        ),
                    }
                }
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
                (Some(b), EnDisco::Nada) if !b.carpeta && !b.solo_drive => {
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
                    self.e().registros.remove(&vieja);
                    self.e().registros.insert(
                        nueva.clone(),
                        Registro {
                            id: b.id.clone(),
                            carpeta: false,
                            md5: a.md5.clone().or(b.md5.clone()),
                            tam: f.tam,
                            mtime: f.mtime,
                            ..Default::default()
                        },
                    );
                    if let Some(s) = self.e().subidas.remove(&vieja) {
                        self.e().subidas.insert(nueva.clone(), s);
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
        let mut marcas: Vec<(String, String)> = Vec::new();
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
            if b.solo_drive {
                marcas.push((r.clone(), q.clone()));
            } else {
                mover.push((r.clone(), q.clone()));
            }
        }
        // Un marcador movido o renombrado en Drive: no hay nada en el disco, solo se cambia la
        // ruta (y el nombre de su copia en la caché).
        for (r, q) in marcas {
            if let Some(b) = self.e().registros.remove(&r) {
                self.e().registros.insert(q.clone(), b);
            }
            if let Some(u) = self.e().reglas.remove(&r) {
                self.e().reglas.insert(q.clone(), u);
            }
            rutas.insert(q);
        }
        for (r, q) in mover {
            if !matches!(self.disco(disco, &r), EnDisco::Archivo(_))
                || !matches!(self.disco(disco, &q), EnDisco::Nada)
            {
                continue;
            }
            if local::mover(&self.cfg.root.join(&r), &self.cfg.root.join(&q)).is_ok() {
                // Renombrar no cambia la fecha: el registro sigue valiendo en la ruta nueva.
                if let Some(b) = self.e().registros.remove(&r) {
                    self.e().registros.insert(q.clone(), b);
                }
                disco.insert(r.clone(), EnDisco::Nada);
                disco.remove(&q);
                rutas.insert(q);
                self.e().subidas.remove(&r);
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
        if let Some(b) = self
            .estado
            .registros
            .get(r)
            .filter(|b| b.solo_drive && !b.carpeta)
            .cloned()
        {
            return self.conciliar_marcador(r, b, disco);
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
            (None, Some((f, _)), None) => {
                // Lo nuevo de este equipo: la regla, o el predeterminado.
                let quiere = self.regla(r);
                let defecto = quiere.is_none().then_some(self.estado.nuevo_local);
                if defecto == Some(Ubicacion::Local) {
                    self.locales_defecto.insert(r.to_owned());
                    return Ok(());
                }
                self.subir(r, None, *f)?;
                if defecto == Some(Ubicacion::SoloDrive)
                    && let Some(b) = self.estado.registros.get_mut(r)
                {
                    b.liberar = true;
                    self.mutado = true;
                }
                if quiere == Some(Ubicacion::SoloDrive) || defecto == Some(Ubicacion::SoloDrive) {
                    self.sucio.insert(r.to_owned());
                }
                Ok(())
            }
            (None, None, Some((id, n))) => {
                // Lo nuevo en Drive: la regla, o el predeterminado.
                let quiere = self.regla(r).unwrap_or(self.estado.nuevo_remoto);
                if quiere == Ubicacion::SoloDrive && !r.starts_with(".orbital/") {
                    self.crear_marcador(r, id, n)
                } else {
                    self.bajar(r, id, n, EnDisco::Nada)
                }
            }
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
                            self.e().registros.insert(r.to_owned(), b);
                        }
                        if self.toca_liberar(r, b) && !self.estado.subidas.contains_key(r) {
                            return self.liberar(r);
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
        // «Volver a solo en Drive» sobrevive a un conflicto y a una subida.
        let liberar = self.estado.registros.get(r).is_some_and(|b| b.liberar);
        self.e().registros.insert(
            r.to_owned(),
            Registro {
                id: id.to_owned(),
                carpeta: false,
                md5,
                tam: f.tam,
                mtime: f.mtime,
                liberar,
                ..Default::default()
            },
        );
        self.guardar_base(r, id);
    }

    fn olvidar(&mut self, r: &str) {
        if let Some(b) = self.e().registros.remove(r) {
            let _ = fs::remove_file(estado::ruta_base(&self.cfg.dir_estado, &b.id));
        }
        self.e().subidas.remove(r);
    }

    /// Guarda la versión acordada de una nota de texto como base de futuras fusiones.
    fn guardar_base(&self, r: &str, id: &str) {
        self.guardar_base_de(r, id, &self.cfg.root.join(r));
    }

    fn guardar_base_de(&self, r: &str, id: &str, ruta: &Path) {
        if !local::es_texto(r) {
            return;
        }
        if fs::metadata(ruta).is_ok_and(|m| m.len() <= TOPE_TEXTO)
            && let Ok(b) = fs::read(ruta)
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
            self.e().registros.insert(
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
                        self.mutado = true;
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
                        self.mutado = true;
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
                self.mutado = true;
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
                self.e().subidas.remove(r);
                let liberar = self.estado.registros.get(r).is_some_and(|b| b.liberar);
                let padre = padre
                    .or_else(|| a.padres.first().cloned())
                    .or_else(|| self.estado.nodos.get(&a.id).map(|n| n.padre.clone()))
                    .unwrap_or_else(|| self.estado.raiz.clone());
                self.poner_nodo(&a, &padre);
                self.e().registros.insert(
                    r.to_owned(),
                    Registro {
                        id: a.id.clone(),
                        carpeta: false,
                        md5: a.md5.clone(),
                        tam: f.tam,
                        mtime: f.mtime,
                        liberar,
                        ..Default::default()
                    },
                );
                if liberar {
                    self.sucio.insert(r.to_owned());
                }
                // Base: solo si el disco tiene justo lo que se subió.
                if local::md5_archivo(&ruta).ok() == a.md5 {
                    self.guardar_base(r, &a.id);
                }
                Ok(())
            }
            Err(e) => {
                match sesion {
                    Some(s) => {
                        self.e().subidas.insert(r.to_owned(), s);
                    }
                    None => {
                        self.e().subidas.remove(r);
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
        self.bajar_a_temporal_en(&dir, id, n)
    }

    /// Como [`Self::bajar_a_temporal`], en otra carpeta (las copias de la caché bajan en la caché:
    /// un video «solo en Drive» nunca pasa por la bóveda).
    fn bajar_a_temporal_en(
        &mut self,
        dir: &Path,
        id: &str,
        n: &Nodo,
    ) -> Resultado<Option<PathBuf>> {
        fs::create_dir_all(dir).map_err(|e| ErrorApi::Local(e.to_string()))?;
        let limpio = local::id_limpio(id);
        // El temporal lleva la suma de la versión: un tramo de otra versión nunca se retoma (la
        // prueba de caos encontró una bajada corrupta así), y los de versiones viejas se borran.
        let version = n.md5.as_deref().unwrap_or("sin-suma");
        let tmp = dir.join(format!("{limpio}-{version}.parcial"));
        if let Ok(rd) = fs::read_dir(dir) {
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
        // El progreso, solo de lo que se pidió abrir; entre tramo y tramo se mira si se canceló.
        let (en_curso, cancelador, emitir) =
            (&mut self.en_curso, &self.cancelador, &mut self.emitir);
        let total = n.tam;
        let res = self.api.bajar(id, &tmp, &mut |hay| {
            let Some(c) = en_curso.as_mut() else {
                return true;
            };
            if cancelador.cortar(c.pedido, hay) {
                return false;
            }
            let ahora = Instant::now();
            if c.ultimo
                .is_none_or(|t| ahora - t >= Duration::from_millis(120))
                || hay >= total
            {
                c.ultimo = Some(ahora);
                emitir(Evento::Progreso {
                    ruta: c.ruta.clone(),
                    pedido: c.pedido,
                    bajado: hay.min(total),
                    total,
                });
            }
            true
        });
        match res {
            Ok(()) => {}
            Err(ErrorApi::Cancelada) => {
                // Nada a medias en la caché: el tramo se borra (no se retoma).
                let _ = fs::remove_file(&tmp);
                return Err(ErrorApi::Cancelada);
            }
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
                self.e().nodos.insert(a.id.clone(), Nodo::de(&a, &padre));
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
            self.e().registros.insert(
                r.to_owned(),
                Registro {
                    id: id.to_owned(),
                    carpeta: false,
                    md5: n.md5.clone(),
                    tam: n.tam,
                    mtime: 0,
                    ..Default::default()
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
        self.e().conflictos.insert(
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
                self.mutado = true;
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
                liberar: self.estado.registros.get(r).is_some_and(|b| b.liberar),
                ..Default::default()
            };
            self.e().registros.insert(r.to_owned(), b.clone());
            self.e().conflictos.remove(r);
            if let Err(e) = self.subir(r, Some(b), f) {
                self.e().conflictos.insert(r.to_owned(), c);
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
            self.e().conflictos.remove(r);
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
                self.e().registros.insert(
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
                self.e().registros.insert(
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
                    self.e().registros.remove(c);
                }
            }
            (Some(_), EnDisco::Carpeta, None) => {
                // Borrada en Drive: si acá quedó vacía, se saca; si no, se vuelve a crear allá.
                let dir = self.cfg.root.join(c);
                let vacia = fs::read_dir(&dir)
                    .map(|mut d| d.next().is_none())
                    .unwrap_or(false);
                self.e().registros.remove(c);
                if vacia {
                    let _ = fs::remove_dir(&dir);
                } else {
                    self.carpeta_remota(c)?;
                    self.completa = true;
                }
            }
            (Some(_), EnDisco::Nada, None) => {
                self.e().registros.remove(c);
            }
            _ => {}
        }
        Ok(())
    }

    // --- «solo en Drive» (061b) ------------------------------------------------------------------

    /// MD5 de la copia de la caché (con la firma acordada se evita leerla).
    fn md5_copia(&self, b: &Registro, copia: &Path) -> Option<(Firma, Option<String>)> {
        let EnDisco::Archivo(f) = local::en_disco(copia) else {
            return None;
        };
        if f.tam == b.tam && f.mtime == b.mtime {
            return Some((f, b.md5.clone()));
        }
        Some((f, local::md5_archivo(copia).ok()))
    }

    /// Si el nombre en Drive cambió, la copia de la caché sigue al nombre nuevo.
    fn ubicar_copia(&self, b: &Registro) -> PathBuf {
        let esperada = self.copia(b);
        if esperada.exists() {
            return esperada;
        }
        if let Some(dir) = esperada.parent()
            && let Ok(rd) = fs::read_dir(dir)
        {
            for e in rd.flatten() {
                let p = e.path();
                if p.is_file() && p.extension().is_none_or(|x| x != "orbital-copiando") {
                    let _ = fs::rename(&p, &esperada);
                    break;
                }
            }
        }
        esperada
    }

    /// Si el extracto de una nota «solo en Drive» se guarda (decisión 3: `.md` y `.html` hasta 1 MB).
    fn lleva_extracto(r: &str, tam: u64) -> bool {
        local::es_texto(r) && tam <= TOPE_TEXTO
    }

    /// Baja la versión de Drive a la copia de la caché. `false`: la suma no coincidió (el próximo
    /// ciclo vuelve a probar).
    fn bajar_copia(&mut self, r: &str, id: &str, n: &Nodo) -> Resultado<bool> {
        self.emitir(Evento::Uno(r.to_owned(), EstadoArchivo::Bajando));
        let dir = self.cfg.dir_cache.join("parcial");
        let Some(tmp) = self.bajar_a_temporal_en(&dir, id, n)? else {
            return Ok(false);
        };
        let copia = estado::ruta_copia(&self.cfg.dir_cache, id, &n.nombre);
        local::mover(&tmp, &copia).map_err(|e| ErrorApi::Local(e.to_string()))?;
        let f = match local::en_disco(&copia) {
            EnDisco::Archivo(f) => f,
            _ => return Ok(false),
        };
        self.e().registros.insert(
            r.to_owned(),
            Registro {
                id: id.to_owned(),
                md5: n.md5.clone(),
                tam: f.tam,
                mtime: f.mtime,
                solo_drive: true,
                ..Default::default()
            },
        );
        self.guardar_base_de(r, id, &copia);
        Ok(true)
    }

    /// Un archivo nuevo en Drive que queda «solo en Drive»: un marcador, con el extracto si es una
    /// nota de texto (para la búsqueda, el grafo y las etiquetas).
    fn crear_marcador(&mut self, r: &str, id: &str, n: &Nodo) -> Resultado<()> {
        if Self::lleva_extracto(r, n.tam) {
            self.bajar_copia(r, id, n)?;
            return Ok(());
        }
        self.e().registros.insert(
            r.to_owned(),
            Registro {
                id: id.to_owned(),
                md5: n.md5.clone(),
                solo_drive: true,
                ..Default::default()
            },
        );
        Ok(())
    }

    /// Un archivo «solo en Drive»: lo que cambió en Drive, en su copia de la caché o en la regla.
    fn conciliar_marcador(
        &mut self,
        r: &str,
        b: Registro,
        disco: &mut HashMap<String, EnDisco>,
    ) -> Resultado<()> {
        let copia = self.ubicar_copia(&b);
        let local = self.md5_copia(&b, &copia);
        let editada = local.as_ref().is_some_and(|(_, m)| *m != b.md5);
        // Apareció un archivo en la bóveda en esa ruta (un corte a mitad de «Liberar espacio», o
        // alguien lo puso): deja de ser un marcador y sigue el camino de siempre con lo acordado
        // como base. Si además la copia de la caché tenía cambios, se guarda antes (nunca se
        // pierde).
        if let EnDisco::Archivo(_) = self.disco(disco, r) {
            if editada
                && let Ok(c) = fs::read(&copia)
                && local::md5_archivo(&self.cfg.root.join(r)).ok().as_deref()
                    != Some(local::md5_bytes(&c).as_str())
            {
                self.al_historial(r, &c);
            }
            let _ = fs::remove_file(&copia);
            let mut b = b;
            b.solo_drive = false;
            b.liberar = true; // era «solo en Drive»: vuelve a serlo cuando quede acordado
            b.tam = 0;
            b.mtime = 0;
            self.e().registros.insert(r.to_owned(), b);
            // En el mismo paso (la prueba de caos lo encontró: si se dejaba para el próximo
            // ciclo, un cambio que la usuaria hacía antes pisaba esta versión sin subirla).
            disco.remove(r);
            return self.conciliar_archivo(r, disco);
        }
        let remoto = self
            .ruta_id
            .get(r)
            .filter(|id| **id == b.id)
            .and_then(|id| self.estado.nodos.get(id).cloned());
        let Some(n) = remoto else {
            // Ya no está en Drive (papelera, o fuera de la carpeta). Si la copia tenía cambios
            // sin subir, vuelve a la bóveda como un archivo nuevo y se sube (no se pierde); si no,
            // la copia era igual a una versión que Drive guarda en su papelera y se suelta.
            if editada && matches!(self.disco(disco, r), EnDisco::Nada) {
                local::mover(&copia, &self.cfg.root.join(r))
                    .map_err(|e| ErrorApi::Local(e.to_string()))?;
                disco.remove(r);
                self.avisar(
                    &format!("resubida:{r}"),
                    format!("«{r}» se había borrado en Drive y tenía cambios en este equipo: se volvió a subir."),
                );
                self.e().registros.remove(r);
                return self.conciliar_archivo(r, disco);
            }
            let _ = fs::remove_file(&copia);
            let _ = fs::remove_dir(copia.parent().unwrap_or(Path::new("")));
            self.olvidar(r);
            return Ok(());
        };
        let cambio_remoto = n.md5 != b.md5;
        if self.toca_mantener(r) {
            self.mantener(r)?;
            disco.remove(r);
            if self.estado.registros.get(r).is_some_and(|b| !b.solo_drive) {
                // Si volvió con cambios sin subir, se suben en este mismo paso.
                return self.conciliar_archivo(r, disco);
            }
            return Ok(());
        }
        match (cambio_remoto, editada) {
            (false, false) => {
                // Al día. Si es una nota de texto sin extracto (la marcó otro equipo, o se
                // recortó la caché), se baja el extracto.
                if local.is_none() && Self::lleva_extracto(r, n.tam) {
                    self.bajar_copia(r, &b.id, &n)?;
                }
                Ok(())
            }
            (true, false) => {
                // Cambió en Drive: se renueva la copia si había una o si lleva extracto; si no,
                // solo cambia lo acordado.
                if local.is_some() || Self::lleva_extracto(r, n.tam) {
                    self.bajar_copia(r, &b.id, &n)?;
                } else {
                    let mut b = b;
                    b.md5 = n.md5.clone();
                    self.e().registros.insert(r.to_owned(), b);
                }
                Ok(())
            }
            (false, true) => {
                let (f, _) = local.expect("copia editada");
                self.subir_copia(r, b, &copia, f)
            }
            (true, true) => {
                // Cambió en los dos lados: la copia de la caché vuelve a la bóveda y sigue el
                // camino de los conflictos (fusión o las dos versiones). Si la regla es «solo en
                // Drive», se libera otra vez cuando se resuelva.
                local::mover(&copia, &self.cfg.root.join(r))
                    .map_err(|e| ErrorApi::Local(e.to_string()))?;
                disco.remove(r);
                let mut b = b;
                b.solo_drive = false;
                b.liberar = true;
                b.tam = 0;
                b.mtime = 0;
                self.e().registros.insert(r.to_owned(), b);
                self.conciliar_archivo(r, disco)
            }
        }
    }

    /// Sube la copia editada de un archivo «solo en Drive» (sigue siendo «solo en Drive»).
    fn subir_copia(&mut self, r: &str, b: Registro, copia: &Path, f: Firma) -> Resultado<()> {
        if self.cuota_llena {
            return Ok(());
        }
        self.emitir(Evento::Uno(r.to_owned(), EstadoArchivo::Subiendo));
        // Justo antes de subir encima: ¿Drive sigue teniendo lo acordado?
        match self.api.archivo(&b.id) {
            Ok(a) if !a.papelera && a.md5 == b.md5 => {}
            Ok(a) if !a.papelera => {
                let padre = a.padres.first().cloned().unwrap_or_default();
                self.poner_nodo(&a, &padre);
                self.sucio.insert(r.to_owned());
                return Ok(());
            }
            Ok(_) | Err(ErrorApi::NoEncontrado) => {
                self.quitar_nodo(&b.id);
                self.sucio.insert(r.to_owned());
                return Ok(());
            }
            Err(e) => return Err(e),
        }
        let mut sesion = self.estado.subidas.get(r).cloned();
        self.mutado = true;
        let (estado, dir) = (&mut self.estado, &self.cfg.dir_estado);
        let res = self.api.subir_cambio(&b.id, copia, &mut sesion, &mut |s| {
            estado.subidas.insert(r.to_owned(), s.clone());
            let _ = estado.guardar(dir);
        });
        match res {
            Ok(a) => {
                self.e().subidas.remove(r);
                let padre = a
                    .padres
                    .first()
                    .cloned()
                    .or_else(|| self.estado.nodos.get(&a.id).map(|n| n.padre.clone()))
                    .unwrap_or_else(|| self.estado.raiz.clone());
                self.poner_nodo(&a, &padre);
                let mismo = local::md5_archivo(copia).ok() == a.md5;
                self.e().registros.insert(
                    r.to_owned(),
                    Registro {
                        id: a.id.clone(),
                        md5: a.md5.clone(),
                        // Si la copia cambió mientras subía, la firma vieja hace que se vuelva a
                        // mirar en el próximo ciclo.
                        tam: if mismo { f.tam } else { 0 },
                        mtime: if mismo { f.mtime } else { 0 },
                        solo_drive: true,
                        ..Default::default()
                    },
                );
                if mismo {
                    self.guardar_base_de(r, &a.id, copia);
                }
                Ok(())
            }
            Err(e) => {
                match sesion {
                    Some(x) => self.e().subidas.insert(r.to_owned(), x),
                    None => self.e().subidas.remove(r),
                };
                Err(e)
            }
        }
    }

    /// «Liberar espacio»: la copia de la bóveda se saca **solo** después de confirmar que Drive
    /// tiene esa misma versión. Orden (un corte en cualquier punto no pierde nada):
    /// 1. la suma del disco, leída de nuevo, es la acordada (si no, hay cambios sin subir);
    /// 2. Drive tiene esa suma (`files.get`);
    /// 3. si es una nota de texto, se copia a la caché como extracto;
    /// 4. el estado dice «solo en Drive» y se guarda;
    /// 5. recién ahí se saca el archivo de la bóveda. Si se corta entre 4 y 5, el próximo ciclo
    ///    ve el archivo en la bóveda con la misma suma y vuelve a empezar.
    fn liberar(&mut self, r: &str) -> Resultado<()> {
        let Some(b) = self.estado.registros.get(r).cloned() else {
            return Ok(());
        };
        let ruta = self.cfg.root.join(r);
        let Ok(md5) = local::md5_archivo(&ruta) else {
            return Ok(());
        };
        if Some(md5.as_str()) != b.md5.as_deref() {
            return Ok(()); // cambios sin subir: primero se suben
        }
        let a = match self.api.archivo(&b.id) {
            Ok(a) => a,
            Err(ErrorApi::NoEncontrado) => {
                self.sucio.insert(r.to_owned());
                return Ok(());
            }
            Err(e) => return Err(e),
        };
        if a.papelera || a.md5.as_deref() != Some(md5.as_str()) {
            // Drive cambió después de traer los cambios: se resuelve en el próximo ciclo.
            let padre = a.padres.first().cloned().unwrap_or_default();
            self.poner_nodo(&a, &padre);
            self.sucio.insert(r.to_owned());
            return Ok(());
        }
        let padre = a.padres.first().cloned().unwrap_or_default();
        self.poner_nodo(&a, &padre);
        let tam = fs::metadata(&ruta).map(|m| m.len()).unwrap_or(u64::MAX);
        let mut nuevo = Registro {
            id: b.id.clone(),
            md5: b.md5.clone(),
            solo_drive: true,
            ..Default::default()
        };
        if Self::lleva_extracto(r, tam) {
            let copia = estado::ruta_copia(&self.cfg.dir_cache, &b.id, &a.nombre);
            local::copiar(&ruta, &copia).map_err(|e| ErrorApi::Local(e.to_string()))?;
            if let EnDisco::Archivo(f) = local::en_disco(&copia) {
                nuevo.tam = f.tam;
                nuevo.mtime = f.mtime;
            }
        }
        self.e().registros.insert(r.to_owned(), nuevo);
        self.estado
            .guardar(&self.cfg.dir_estado)
            .map_err(|e| ErrorApi::Local(e.to_string()))?;
        if self.cortar_liberar {
            return Err(ErrorApi::SinRed("corte provocado al liberar".into()));
        }
        // La bóveda, sin la copia: Drive tiene exactamente esta versión (paso 2).
        if let Err(e) = fs::remove_file(&ruta) {
            // No se pudo sacar: se deshace (vuelve a «en ambos» y se reintenta).
            if let Some(x) = self.e().registros.get_mut(r) {
                x.solo_drive = false;
                x.tam = 0;
                x.mtime = 0;
            }
            return Err(ErrorApi::Local(e.to_string()));
        }
        Ok(())
    }

    /// «Mantener en este equipo»: el marcador vuelve a la bóveda. Si la copia de la caché tiene
    /// cambios sin subir, es esa la que vuelve (y se sube con lo acordado como base).
    fn mantener(&mut self, r: &str) -> Resultado<()> {
        let Some(b) = self.estado.registros.get(r).cloned() else {
            return Ok(());
        };
        let destino = self.cfg.root.join(r);
        if !matches!(local::en_disco(&destino), EnDisco::Nada) {
            return Ok(());
        }
        let copia = self.ubicar_copia(&b);
        let n = self
            .ruta_id
            .get(r)
            .and_then(|id| self.estado.nodos.get(id).cloned());
        if let Some((_, md5)) = self.md5_copia(&b, &copia) {
            let al_dia = n.as_ref().is_some_and(|n| n.md5 == md5);
            if al_dia || md5 != b.md5 {
                local::mover(&copia, &destino).map_err(|e| ErrorApi::Local(e.to_string()))?;
                let _ = fs::remove_dir(copia.parent().unwrap_or(Path::new("")));
                let f = match local::en_disco(&destino) {
                    EnDisco::Archivo(f) => f,
                    _ => Firma { tam: 0, mtime: 0 },
                };
                let mut b = b;
                b.solo_drive = false;
                b.liberar = false;
                if al_dia {
                    b.md5 = md5;
                    b.tam = f.tam;
                    b.mtime = f.mtime;
                } else {
                    // Cambios sin subir: lo acordado sigue siendo lo de antes; se sube enseguida.
                    b.tam = 0;
                    b.mtime = 0;
                    self.sucio.insert(r.to_owned());
                }
                self.e().registros.insert(r.to_owned(), b);
                return Ok(());
            }
        }
        let Some(n) = n else {
            self.olvidar(r);
            return Ok(());
        };
        self.bajar(r, &b.id.clone(), &n, EnDisco::Nada)?;
        let _ = fs::remove_file(&copia);
        let _ = fs::remove_dir(copia.parent().unwrap_or(Path::new("")));
        Ok(())
    }

    /// Como [`Self::traer`], con el progreso ([`Evento::Progreso`]) y cancelable con el número de
    /// pedido. Cancelada, no queda nada a medias en la caché y el estado no cambia.
    pub fn traer_pedido(&mut self, r: &str, pedido: u64) -> Result<PathBuf, String> {
        if self.cancelador.cortar(pedido, 0) {
            self.cancelador.olvidar(pedido);
            self.soltar_parcial(r);
            return Err(CANCELADA.into());
        }
        self.en_curso = Some(EnCurso {
            ruta: r.to_owned(),
            pedido,
            ultimo: None,
        });
        let res = self.traer(r);
        self.en_curso = None;
        let cancelado = res.is_err() && self.cancelador.cancelado(pedido);
        self.cancelador.olvidar(pedido);
        match res {
            Err(_) if cancelado => {
                self.soltar_parcial(r);
                Err(CANCELADA.into())
            }
            x => x,
        }
    }

    /// Borra lo bajado a medias de lo «solo en Drive» en `r` (también un tramo de un corte de
    /// red anterior, que se habría retomado): cancelar no deja nada a medias en la caché. La
    /// prueba de caos encontró un tramo que quedaba al cancelar antes de empezar.
    fn soltar_parcial(&self, r: &str) {
        let Some(b) = self.estado.registros.get(r) else {
            return;
        };
        let prefijo = format!("{}-", local::id_limpio(&b.id));
        if let Ok(rd) = fs::read_dir(self.cfg.dir_cache.join("parcial")) {
            for e in rd.flatten() {
                if e.file_name().to_string_lossy().starts_with(&prefijo) {
                    let _ = fs::remove_file(e.path());
                }
            }
        }
    }

    /// Trae a la caché un archivo «solo en Drive» para abrirlo (con conexión). Devuelve la ruta de
    /// la copia. Si la copia tiene cambios sin subir, se devuelve esa.
    pub fn traer(&mut self, r: &str) -> Result<PathBuf, String> {
        let Some(b) = self
            .estado
            .registros
            .get(r)
            .filter(|b| b.solo_drive)
            .cloned()
        else {
            let p = self.cfg.root.join(r);
            return if p.is_file() {
                Ok(p)
            } else {
                Err("no está en Drive".into())
            };
        };
        let copia = self.ubicar_copia(&b);
        let local = self.md5_copia(&b, &copia);
        if local.as_ref().is_some_and(|(_, m)| *m != b.md5) {
            tocar(&copia);
            return Ok(copia);
        }
        let a = self.api.archivo(&b.id).map_err(|e| e.to_string())?;
        if a.papelera {
            return Err("se mandó a la papelera de Drive".into());
        }
        let padre = a.padres.first().cloned().unwrap_or_default();
        self.poner_nodo(&a, &padre);
        if local.as_ref().is_some_and(|(_, m)| *m == a.md5) {
            tocar(&copia);
            return Ok(copia);
        }
        let n = Nodo::de(&a, &padre);
        match self.bajar_copia(r, &b.id, &n) {
            Ok(true) => {}
            Ok(false) => return Err("cambió en Drive mientras bajaba; probá otra vez".into()),
            Err(e) => {
                // `bajar_copia` avisó «bajando»: se vuelve a mandar el mapa (sigue como estaba).
                self.ultimo_mapa = None;
                self.emitir_estados();
                return Err(e.to_string());
            }
        }
        let copia = self.ubicar_copia(&b);
        tocar(&copia);
        self.recortar_cache(Some(r));
        let _ = self.estado.guardar(&self.cfg.dir_estado);
        self.emitir_estados();
        Ok(copia)
    }

    /// Manda a la papelera de Drive un archivo «solo en Drive». Si la copia de la caché tenía
    /// cambios sin subir, van antes al historial (o al respaldo).
    pub fn borrar_marcador(&mut self, r: &str) -> Resultado<()> {
        let Some(b) = self
            .estado
            .registros
            .get(r)
            .filter(|b| b.solo_drive)
            .cloned()
        else {
            return Ok(());
        };
        let copia = self.ubicar_copia(&b);
        if self.md5_copia(&b, &copia).is_some_and(|(_, m)| m != b.md5)
            && let Ok(c) = fs::read(&copia)
        {
            self.al_historial(r, &c);
        }
        self.api.a_papelera(&b.id)?;
        let _ = fs::remove_file(&copia);
        let _ = fs::remove_dir(copia.parent().unwrap_or(Path::new("")));
        self.quitar_nodo(&b.id);
        self.olvidar(r);
        self.e().reglas.remove(r);
        let _ = self.estado.guardar(&self.cfg.dir_estado);
        self.emitir_estados();
        Ok(())
    }

    /// Renombra o mueve un archivo «solo en Drive» (en Drive, mismo id).
    pub fn mover_marcador(&mut self, de: &str, a: &str) -> Resultado<()> {
        let Some(b) = self
            .estado
            .registros
            .get(de)
            .filter(|b| b.solo_drive)
            .cloned()
        else {
            return Ok(());
        };
        if self.estado.registros.contains_key(a) || self.cfg.root.join(a).exists() {
            return Err(ErrorApi::Local(format!("ya hay algo en «{a}»")));
        }
        let padre_nuevo = self.carpeta_remota(padre_de(a))?;
        let padre_viejo = self.estado.nodos.get(&b.id).map(|n| n.padre.clone());
        let cambia_nombre = nombre_de(de) != nombre_de(a);
        let cambia_padre = padre_viejo.as_deref() != Some(padre_nuevo.as_str());
        let x = self.api.actualizar(
            &b.id,
            cambia_nombre.then_some(nombre_de(a)),
            cambia_padre.then_some(padre_nuevo.as_str()),
            if cambia_padre {
                padre_viejo.as_deref()
            } else {
                None
            },
        )?;
        self.poner_nodo(&x, &padre_nuevo);
        self.e().registros.remove(de);
        self.e().registros.insert(a.to_owned(), b.clone());
        if let Some(u) = self.e().reglas.remove(de) {
            self.e().reglas.insert(a.to_owned(), u);
        }
        self.ubicar_copia(&b);
        let _ = self.estado.guardar(&self.cfg.dir_estado);
        self.emitir_estados();
        Ok(())
    }

    // --- cambios en cola (sin conexión) --------------------------------------------------------

    /// Renombra o mueve lo «solo en Drive»: con conexión, en Drive enseguida; sin conexión (o si
    /// la red se corta en el intento), queda en cola y la bóveda lo muestra hecho. `true`: quedó
    /// en cola.
    pub fn pedir_mover(&mut self, de: &str, a: &str, en_linea: bool) -> Resultado<bool> {
        if en_linea && self.estado.cola.is_empty() {
            match self.mover_marcador(de, a) {
                Ok(()) => return Ok(false),
                Err(e) if e.transitorio() => {}
                Err(e) => return Err(e),
            }
        }
        self.encolar_mover(de, a)?;
        Ok(true)
    }

    /// Como [`Self::pedir_mover`], para mandar a la papelera de Drive.
    pub fn pedir_borrar(&mut self, r: &str, en_linea: bool) -> Resultado<bool> {
        if en_linea && self.estado.cola.is_empty() {
            match self.borrar_marcador(r) {
                Ok(()) => return Ok(false),
                Err(e) if e.transitorio() => {}
                Err(e) => return Err(e),
            }
        }
        self.encolar_borrar(r)?;
        Ok(true)
    }

    /// Anota un renombre de lo «solo en Drive» para cuando vuelva la red. El registro pasa ya a
    /// la ruta nueva (el explorador lo muestra ahí, «pendiente»); la copia de la caché no se toca.
    pub fn encolar_mover(&mut self, de: &str, a: &str) -> Resultado<()> {
        let Some(b) = self
            .estado
            .registros
            .get(de)
            .filter(|b| b.solo_drive && !b.carpeta)
            .cloned()
        else {
            return Err(ErrorApi::Local(format!("«{de}» no está solo en Drive")));
        };
        if de == a {
            return Ok(());
        }
        if self.estado.registros.contains_key(a) || self.cfg.root.join(a).exists() {
            return Err(ErrorApi::Local(format!("ya hay algo en «{a}»")));
        }
        let origen = match self.estado.cola.get(&b.id) {
            Some(Pendiente::Mover { de: primero, .. }) => primero.clone(),
            _ => de.to_owned(),
        };
        let e = self.e();
        e.registros.remove(de);
        e.registros.insert(a.to_owned(), b.clone());
        if let Some(u) = e.reglas.remove(de) {
            e.reglas.insert(a.to_owned(), u);
        }
        e.cola.insert(
            b.id.clone(),
            Pendiente::Mover {
                de: origen,
                a: a.to_owned(),
            },
        );
        let _ = self.estado.guardar(&self.cfg.dir_estado);
        self.emitir_estados();
        Ok(())
    }

    /// Anota mandar a la papelera de Drive lo «solo en Drive» para cuando vuelva la red. Sale ya
    /// del explorador. Si la copia de la caché tenía cambios sin subir, van antes al historial.
    pub fn encolar_borrar(&mut self, r: &str) -> Resultado<()> {
        let Some(b) = self
            .estado
            .registros
            .get(r)
            .filter(|b| b.solo_drive && !b.carpeta)
            .cloned()
        else {
            return Err(ErrorApi::Local(format!("«{r}» no está solo en Drive")));
        };
        let copia = self.ubicar_copia(&b);
        if self.md5_copia(&b, &copia).is_some_and(|(_, m)| m != b.md5)
            && let Ok(c) = fs::read(&copia)
        {
            self.al_historial(r, &c);
        }
        let e = self.e();
        e.registros.remove(r);
        e.reglas.remove(r);
        e.cola.insert(
            b.id.clone(),
            Pendiente::Borrar {
                ruta: r.to_owned(),
                md5: b.md5.clone(),
            },
        );
        let _ = self.estado.guardar(&self.cfg.dir_estado);
        self.emitir_estados();
        Ok(())
    }

    /// Una ruta libre en Drive y en la bóveda para `a` (con ` (2)`, ` (3)`… si otro archivo de
    /// Drive ya tiene ese nombre; `id` es el que se mueve).
    fn ruta_libre(&self, a: &str, id: &str) -> String {
        let ocupada = |r: &str| {
            self.ruta_id.get(r).is_some_and(|x| x != id)
                || self.estado.registros.get(r).is_some_and(|b| b.id != id)
                || self.cfg.root.join(r).exists()
        };
        if !ocupada(a) {
            return a.to_owned();
        }
        let (padre, nombre) = (padre_de(a), nombre_de(a));
        let (base, ext) = match nombre.rsplit_once('.') {
            Some((b, e)) if !b.is_empty() => (b, format!(".{e}")),
            _ => (nombre, String::new()),
        };
        (2..)
            .map(|k| unir(padre, &format!("{base} ({k}){ext}")))
            .find(|r| !ocupada(r))
            .expect("hay un nombre libre")
    }

    /// Aplica en Drive los cambios en cola (al principio de cada ciclo, después de traer los
    /// cambios remotos y antes de conciliar: así lo que cambió en Drive mientras tanto ya se
    /// conoce). Nunca se pierde una versión:
    /// - renombrado en cola y en Drive cambió el contenido: se renombra igual (mismo id; la copia
    ///   y el extracto se renuevan después);
    /// - renombrado en cola y otro archivo tomó ese nombre en Drive: queda con ` (2)`;
    /// - renombrado en cola y se borró en Drive: se suelta la cola y sigue el camino de siempre
    ///   (si la copia tenía cambios, vuelve a la bóveda y se sube);
    /// - a la papelera en cola y cambió en Drive: no se manda a la papelera, vuelve a aparecer.
    fn aplicar_cola(&mut self) -> Resultado<()> {
        let cola: Vec<(String, Pendiente)> = self
            .estado
            .cola
            .iter()
            .map(|(id, p)| (id.clone(), p.clone()))
            .collect();
        for (id, p) in cola {
            match p {
                Pendiente::Mover { de, a } => self.aplicar_mover(&id, &de, &a)?,
                Pendiente::Borrar { ruta, md5 } => self.aplicar_borrar(&id, &ruta, md5)?,
            }
            self.e().cola.remove(&id);
        }
        Ok(())
    }

    fn aplicar_mover(&mut self, id: &str, de: &str, a: &str) -> Resultado<()> {
        let Some(b) = self
            .estado
            .registros
            .get(a)
            .filter(|b| b.id == id && b.solo_drive)
            .cloned()
        else {
            return Ok(()); // ya no está ahí (se trajo a la bóveda, se volvió a mover…)
        };
        let Some(n) = self.estado.nodos.get(id).cloned() else {
            // Se borró en Drive mientras tanto: lo resuelve `conciliar_marcador` (la copia
            // editada vuelve a la bóveda y se sube; si no, Drive la tiene en su papelera).
            self.sucio.insert(a.to_owned());
            return Ok(());
        };
        let destino = self.ruta_libre(a, id);
        if destino != a {
            let e = self.e();
            e.registros.remove(a);
            e.registros.insert(destino.clone(), b.clone());
            if let Some(u) = e.reglas.remove(a) {
                e.reglas.insert(destino.clone(), u);
            }
            // Si el ciclo se corta antes de llegar a Drive, la cola ya apunta a la ruta nueva.
            e.cola.insert(
                id.to_owned(),
                Pendiente::Mover {
                    de: de.to_owned(),
                    a: destino.clone(),
                },
            );
            self.avisar(
                &format!("renombre:{destino}"),
                format!(
                    "En Drive ya había otro «{}»: «{de}» quedó como «{}».",
                    nombre_de(a),
                    nombre_de(&destino)
                ),
            );
        }
        let padre_nuevo = self.carpeta_remota(padre_de(&destino))?;
        let cambia_nombre = n.nombre != nombre_de(&destino);
        let cambia_padre = n.padre != padre_nuevo;
        if cambia_nombre || cambia_padre {
            match self.api.actualizar(
                id,
                cambia_nombre.then_some(nombre_de(&destino)),
                cambia_padre.then_some(padre_nuevo.as_str()),
                cambia_padre.then_some(n.padre.as_str()),
            ) {
                Ok(x) => self.poner_nodo(&x, &padre_nuevo),
                Err(ErrorApi::NoEncontrado) => self.quitar_nodo(id),
                Err(e) => return Err(e),
            }
            self.ubicar_copia(&b);
        }
        self.sucio.insert(destino);
        Ok(())
    }

    fn aplicar_borrar(&mut self, id: &str, ruta: &str, md5: Option<String>) -> Resultado<()> {
        let nombre = self.estado.nodos.get(id).map(|n| n.nombre.clone());
        let copia = estado::ruta_copia(
            &self.cfg.dir_cache,
            id,
            nombre.as_deref().unwrap_or("archivo"),
        );
        match self.estado.nodos.get(id) {
            None => {} // ya no está en Drive
            Some(n) if n.md5 != md5 => {
                // Cambió en Drive después de mandarlo a la papelera sin conexión: esa versión
                // no se manda a la papelera. Vuelve como algo nuevo de Drive.
                if let Some(r) = self.id_ruta.get(id).cloned() {
                    self.sucio.insert(r);
                }
                self.avisar(
                    &format!("conservado:{ruta}"),
                    format!("«{ruta}» cambió en Drive mientras esperaba para ir a la papelera: se conservó la versión nueva."),
                );
            }
            Some(_) => {
                self.api.a_papelera(id)?;
                self.quitar_nodo(id);
            }
        }
        let _ = fs::remove_file(&copia);
        let _ = fs::remove_dir(copia.parent().unwrap_or(Path::new("")));
        Ok(())
    }

    /// Pone o quita la regla de este equipo de una ruta y marca lo que cubre para el próximo ciclo.
    pub fn ubicar(&mut self, ruta: &str, u: Option<Ubicacion>) {
        let ruta = ruta.to_owned();
        match u {
            Some(Ubicacion::Local) => {} // compartida: la escribe la app en `drive.yaml`
            Some(u) => {
                self.e().reglas.insert(ruta.clone(), u);
            }
            None => {
                self.e().reglas.remove(&ruta);
            }
        }
        let base = ruta.trim_end_matches('/');
        let cubiertas: Vec<String> = self
            .estado
            .registros
            .keys()
            .filter(|r| reglas::cubre(base, r))
            .cloned()
            .collect();
        self.sucio.extend(cubiertas);
        self.sucio.insert(base.to_owned());
        let _ = self.estado.guardar(&self.cfg.dir_estado);
        self.emitir_estados();
    }

    pub fn predeterminados(&mut self, local: Ubicacion, remoto: Ubicacion) {
        if self.estado.nuevo_local != local || self.estado.nuevo_remoto != remoto {
            self.e().nuevo_local = local;
            self.e().nuevo_remoto = remoto;
            let _ = self.estado.guardar(&self.cfg.dir_estado);
        }
    }

    /// Recorta la caché de lo abierto (no los extractos de texto ni las copias con cambios sin
    /// subir) hasta el tope, sacando primero lo que hace más que no se usa. `menos` no se saca
    /// (lo que se acaba de traer).
    fn recortar_cache(&mut self, menos: Option<&str>) {
        let mut copias: Vec<(std::time::SystemTime, u64, String, PathBuf)> = Vec::new();
        let mut total = 0u64;
        for (r, b) in &self.estado.registros {
            if !b.solo_drive || b.tam == 0 || Some(r.as_str()) == menos {
                continue;
            }
            if Self::lleva_extracto(r, b.tam) {
                continue;
            }
            let copia = self.copia(b);
            let Ok(m) = fs::metadata(&copia) else {
                continue;
            };
            let f = local::firma(&m);
            if f.tam != b.tam || f.mtime != b.mtime {
                continue; // cambios sin subir
            }
            total += f.tam;
            let uso = copia
                .parent()
                .and_then(|d| fs::metadata(d).ok())
                .and_then(|m| m.modified().ok())
                .unwrap_or(std::time::UNIX_EPOCH);
            copias.push((uso, f.tam, r.clone(), copia));
        }
        if total <= self.cfg.tope_cache {
            return;
        }
        copias.sort();
        for (_, tam, r, copia) in copias {
            if total <= self.cfg.tope_cache {
                break;
            }
            if fs::remove_file(&copia).is_ok() {
                total -= tam;
                if let Some(b) = self.e().registros.get_mut(&r) {
                    b.tam = 0;
                    b.mtime = 0;
                }
            }
        }
    }

    /// Una carpeta renombrada o movida en el disco: si en la ruta nueva está todo lo acordado de
    /// la vieja (mismo tamaño y fecha: renombrar no cambia la fecha), se renombra en Drive la
    /// misma carpeta (conserva su id) en vez de mover archivo por archivo.
    fn movimientos_de_carpetas(
        &mut self,
        rutas: &mut BTreeSet<String>,
        disco: &mut HashMap<String, EnDisco>,
    ) -> Resultado<()> {
        let mut faltan: Vec<(String, String)> = Vec::new();
        let mut nuevas: Vec<String> = Vec::new();
        for r in rutas.iter() {
            if self.excluida(r) {
                continue;
            }
            match (self.estado.registros.get(r), self.disco(disco, r)) {
                (Some(b), EnDisco::Nada)
                    if b.carpeta && self.id_ruta.get(&b.id).map(String::as_str) == Some(r) =>
                {
                    faltan.push((r.clone(), b.id.clone()));
                }
                (None, EnDisco::Carpeta) if !self.ruta_id.contains_key(r) => nuevas.push(r.clone()),
                _ => {}
            }
        }
        if faltan.is_empty() || nuevas.is_empty() {
            return Ok(());
        }
        // Las de más arriba primero: al renombrar una carpeta, sus subcarpetas van con ella.
        faltan.sort_by_key(|(r, _)| r.matches('/').count());
        for (vieja, id) in faltan {
            if !self.estado.registros.contains_key(&vieja) {
                continue; // ya se movió con una carpeta de más arriba
            }
            let prefijo = format!("{vieja}/");
            let hijos: Vec<(String, Registro)> = self
                .estado
                .registros
                .range(prefijo.clone()..)
                .take_while(|(r, _)| r.starts_with(&prefijo))
                .map(|(r, b)| (r.clone(), b.clone()))
                .collect();
            let archivos: Vec<&(String, Registro)> = hijos
                .iter()
                .filter(|(_, b)| !b.carpeta && !b.solo_drive)
                .collect();
            if archivos.is_empty() {
                continue;
            }
            let Some(pos) = nuevas.iter().position(|nueva| {
                archivos.iter().all(|(r, b)| {
                    let resto = &r[prefijo.len()..];
                    matches!(
                        local::en_disco(&self.cfg.root.join(format!("{nueva}/{resto}"))),
                        EnDisco::Archivo(f) if f.tam == b.tam && f.mtime == b.mtime
                    )
                })
            }) else {
                continue;
            };
            let nueva = nuevas.remove(pos);
            let padre_nuevo = self.carpeta_remota(padre_de(&nueva))?;
            let padre_viejo = self.estado.nodos.get(&id).map(|n| n.padre.clone());
            let cambia_nombre = nombre_de(&vieja) != nombre_de(&nueva);
            let cambia_padre = padre_viejo.as_deref() != Some(padre_nuevo.as_str());
            match self.api.actualizar(
                &id,
                cambia_nombre.then_some(nombre_de(&nueva)),
                cambia_padre.then_some(padre_nuevo.as_str()),
                if cambia_padre {
                    padre_viejo.as_deref()
                } else {
                    None
                },
            ) {
                Ok(a) => {
                    self.e()
                        .nodos
                        .insert(a.id.clone(), Nodo::de(&a, &padre_nuevo));
                    let cambiar = |r: &str| format!("{nueva}{}", &r[vieja.len()..]);
                    let mover: Vec<String> = std::iter::once(vieja.clone())
                        .chain(hijos.iter().map(|(r, _)| r.clone()))
                        .collect();
                    for r in mover {
                        let q = cambiar(&r);
                        if let Some(b) = self.e().registros.remove(&r) {
                            self.e().registros.insert(q.clone(), b);
                        }
                        if let Some(x) = self.e().subidas.remove(&r) {
                            self.e().subidas.insert(q.clone(), x);
                        }
                        if let Some(x) = self.e().conflictos.remove(&r) {
                            self.e().conflictos.insert(q.clone(), x);
                        }
                        rutas.remove(&r);
                        rutas.insert(q);
                    }
                    let reglas: Vec<String> = self
                        .estado
                        .reglas
                        .keys()
                        .filter(|r| reglas::cubre(&vieja, r))
                        .cloned()
                        .collect();
                    for r in reglas {
                        if let Some(u) = self.e().reglas.remove(&r) {
                            self.e().reglas.insert(cambiar(&r), u);
                        }
                    }
                    self.indexar();
                    disco.clear();
                }
                Err(e) if e.transitorio() => return Err(e),
                Err(_) => {} // se resuelve archivo por archivo
            }
        }
        Ok(())
    }

    // --- estados para la interfaz ----------------------------------------------------------------

    /// Mapa ruta → estado, con las carpetas resumidas.
    pub fn mapa(&self) -> HashMap<String, EstadoArchivo> {
        let mut m: HashMap<String, EstadoArchivo> =
            HashMap::with_capacity(self.estado.registros.len());
        for (r, b) in &self.estado.registros {
            if b.solo_drive && !b.carpeta {
                m.insert(r.clone(), EstadoArchivo::SoloDrive);
            } else if !b.carpeta {
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
        for r in &self.locales_defecto {
            m.insert(r.clone(), EstadoArchivo::Local);
        }
        for p in self.estado.cola.values() {
            if let Pendiente::Mover { a, .. } = p
                && self.estado.registros.contains_key(a)
            {
                m.insert(a.clone(), EstadoArchivo::Pendiente);
            }
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
        if !self.mutado && self.pendientes == self.pendientes_emitidas && self.ultimo_mapa.is_some()
        {
            return;
        }
        self.mutado = false;
        self.pendientes_emitidas = self.pendientes.clone();
        let m = Arc::new(self.mapa());
        if self.ultimo_mapa.as_deref() != Some(&*m) {
            self.ultimo_mapa = Some(m.clone());
            let reglas: Reglas = self
                .reglas_vigentes()
                .map(|(r, u)| (r.to_owned(), u))
                .collect();
            self.emitir(Evento::Estados(m, Arc::new(reglas)));
        }
        let marcas = self.estado.marcadores();
        if self.ultimos_marcadores.as_deref() != Some(&marcas) {
            let marcas = Arc::new(marcas);
            let _ = estado::guardar_marcadores(&self.cfg.dir_estado, &marcas);
            self.ultimos_marcadores = Some(marcas.clone());
            self.emitir(Evento::Marcadores(marcas));
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
        self.pendientes.len()
            + self.estado.subidas.len()
            + self.sucio.len()
            + self.estado.cola.len()
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
    api.bajar(&yaml.id, &tmp, &mut |_| true)?;
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
    cancelador: Cancelador,
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
        let cancelador = Cancelador::default();
        let c = cancelador.clone();
        let hilo = std::thread::Builder::new()
            .name("orbital-drive".into())
            .spawn(move || bucle(cfg, fabrica, Arc::new(emitir), rx, c))?;
        Ok(Self {
            tx,
            hilo: Some(hilo),
            cancelador,
        })
    }

    pub fn enviar(&self, o: Orden) {
        let _ = self.tx.send(o);
    }

    /// Cancela un [`Orden::Traer`] (aunque el motor esté bajando: no espera a que lea la orden).
    pub fn cancelar(&self, pedido: u64) {
        self.cancelador.cancelar(pedido);
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
    cancelador: Cancelador,
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
    s.cancelador = cancelador;
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
            cuota: s.cuota,
            nuevo_local: s.estado.nuevo_local,
            nuevo_remoto: s.estado.nuevo_remoto,
            solo_drive: s.estado.registros.values().filter(|b| b.solo_drive).count(),
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
            Ok(Orden::Abierta(r)) => {
                // La que se cerró puede liberarse ahora.
                if let Some(vieja) = std::mem::replace(&mut s.abierta, r) {
                    s.marcar([vieja]);
                }
                continue;
            }
            Ok(Orden::Ubicar(r, u)) => {
                s.ubicar(&r, u);
                proximo = Instant::now();
                continue;
            }
            Ok(Orden::Predeterminados { local, remoto }) => {
                s.predeterminados(local, remoto);
                emitir(resumen(&s, en_linea, false, &pausa, &error));
                continue;
            }
            Ok(Orden::Traer(r, pedido)) => {
                let res = if en_linea {
                    s.traer_pedido(&r, pedido)
                } else {
                    s.cancelador.olvidar(pedido);
                    Err("no hay conexión".into())
                };
                emitir(Evento::Traido(r, pedido, res));
                continue;
            }
            Ok(Orden::Borrar(r)) => {
                match s.pedir_borrar(&r, en_linea && pausa.is_none()) {
                    Ok(true) if en_linea => proximo = Instant::now(),
                    Ok(_) => {}
                    Err(e) => emitir(Evento::Aviso(format!(
                        "No se pudo mandar «{r}» a la papelera de Drive: {e}"
                    ))),
                }
                emitir(resumen(&s, en_linea, false, &pausa, &error));
                continue;
            }
            Ok(Orden::Mover { de, a }) => {
                match s.pedir_mover(&de, &a, en_linea && pausa.is_none()) {
                    Ok(true) if en_linea => proximo = Instant::now(),
                    Ok(_) => {}
                    Err(e) => emitir(Evento::Aviso(format!(
                        "No se pudo mover «{de}» en Drive: {e}"
                    ))),
                }
                emitir(resumen(&s, en_linea, false, &pausa, &error));
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

/// Marca una copia de la caché como recién usada (la carpeta de la copia lleva la fecha de uso:
/// la del archivo es parte de su firma y no se toca).
fn tocar(copia: &Path) {
    if let Some(d) = copia.parent()
        && let Ok(f) = fs::File::open(d)
    {
        let _ = f.set_modified(std::time::SystemTime::now());
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
