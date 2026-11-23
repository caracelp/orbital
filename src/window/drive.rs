//! Google Drive (061a): arranca el motor de sincronización si la bóveda está conectada (después
//! del primer cuadro: el arranque no cambia), le pasa los cambios del disco, el foco y la red, y
//! muestra lo que avisa: un ícono por fila en el explorador, el resumen en la barra de estado, los
//! conflictos y los avisos. Una bóveda sin `.orbital/drive.yaml` no carga nada de esto.

use super::*;

use orbital_drive::motor::Reglas;
use orbital_drive::reglas::Ubicacion;
use orbital_drive::{EstadoArchivo, Evento, InfoConflicto, Motor, Orden, Resumen};

/// Lo que crea la API dentro del hilo del motor.
type Fabrica =
    Box<dyn FnOnce(&orbital_drive::Estado) -> Result<Box<dyn orbital_drive::Api>, String> + Send>;

/// Lo que la ventana sabe de Drive.
#[derive(Default)]
pub(super) struct DriveUi {
    pub(super) motor: Option<Motor>,
    root: Option<PathBuf>,
    /// Sube con cada arranque del motor: los eventos de un motor viejo se ignoran.
    generacion: u64,
    pub(super) estados: Arc<HashMap<String, EstadoArchivo>>,
    pub(super) reglas: Arc<Reglas>,
    /// Lo «solo en Drive» que ya se bajó en esta sesión (se abre sin volver a preguntar).
    pub(super) frescos: std::collections::HashSet<String>,
    /// Lo «solo en Drive» que se está bajando para abrirlo (acá o con otra aplicación).
    pub(super) bajada: Option<super::drive_bajada::Bajada>,
    pub(super) siguiente_pedido: u64,
    /// Carpetas de copias de la caché abiertas con otra aplicación (sus cambios se suben).
    pub(super) vigiladas: HashMap<PathBuf, gio::FileMonitor>,
    /// Embeds «solo en Drive» pedidos al motor (no se piden dos veces).
    pedidos: std::collections::HashSet<String>,
    /// La ruta abierta que se le avisó al motor (no se libera mientras está abierta).
    abierta: Option<String>,
    pub(super) conflictos: Arc<Vec<InfoConflicto>>,
    pub(super) resumen: Option<Resumen>,
    pub(super) detenido: Option<String>,
    señales: bool,
}

/// Una fila visible del explorador.
pub(super) struct FilaDrive {
    pub(super) icono: glib::WeakRef<gtk::Image>,
    pub(super) item: glib::WeakRef<gtk::ListItem>,
    /// Nombre accesible sin el estado de Drive.
    base: String,
}

/// Lo que se muestra de una ruta.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum EstadoFila {
    Drive(EstadoArchivo),
    Local,
    /// «Solo en Drive» y sin conexión: no se puede abrir.
    Bloqueado,
}

impl EstadoFila {
    pub(super) fn icono(self) -> &'static str {
        match self {
            Self::Drive(EstadoArchivo::Ambos) => "weather-overcast-symbolic",
            Self::Drive(EstadoArchivo::Subiendo) => "go-up-symbolic",
            Self::Drive(EstadoArchivo::Bajando) => "go-down-symbolic",
            Self::Drive(EstadoArchivo::Conflicto) => "dialog-warning-symbolic",
            Self::Drive(EstadoArchivo::SoloDrive) => "folder-remote-symbolic",
            Self::Drive(EstadoArchivo::Pendiente) => "document-open-recent-symbolic",
            Self::Drive(EstadoArchivo::Local) | Self::Local => "computer-symbolic",
            Self::Bloqueado => "system-lock-screen-symbolic",
        }
    }

    pub(super) fn texto(self) -> &'static str {
        match self {
            Self::Drive(EstadoArchivo::Ambos) => "en este equipo y en Drive",
            Self::Drive(EstadoArchivo::Subiendo) => "subiendo a Drive",
            Self::Drive(EstadoArchivo::Bajando) => "bajando de Drive",
            Self::Drive(EstadoArchivo::Conflicto) => "en conflicto con Drive",
            Self::Drive(EstadoArchivo::SoloDrive) => "solo en Drive",
            Self::Drive(EstadoArchivo::Pendiente) => {
                "solo en Drive, cambio pendiente: se aplica en Drive cuando vuelva la red"
            }
            Self::Drive(EstadoArchivo::Local) | Self::Local => "solo en este equipo",
            Self::Bloqueado => "solo en Drive, bloqueado sin conexión",
        }
    }
}

/// Solo pruebas: la API del servidor falso y una carpeta de estado temporal (nunca la cuenta ni
/// el estado de la usuaria).
#[cfg(test)]
pub(super) static PRUEBA: std::sync::Mutex<Option<(String, PathBuf)>> = std::sync::Mutex::new(None);

/// Carpetas del estado y de la caché de Drive de una bóveda en este equipo (en las pruebas, las
/// temporales de [`PRUEBA`]).
pub(super) fn carpetas(root: &Path) -> (PathBuf, PathBuf) {
    #[cfg(test)]
    if let Some((_, dir)) = PRUEBA.lock().unwrap().clone() {
        return (dir.clone(), dir.join("cache"));
    }
    (
        orbital_drive::estado::carpeta_estado(root),
        orbital_drive::estado::carpeta_cache(root),
    )
}

/// Registra dónde está la copia de cada archivo «solo en Drive» (para el índice, los embeds y el
/// guardado). `false` si ya estaba igual.
fn registrar_marcadores(root: &Path, marcas: &[orbital_drive::estado::Marcador]) -> bool {
    let (_, cache) = carpetas(root);
    let mapa: HashMap<String, PathBuf> = marcas
        .iter()
        .map(|m| {
            (
                m.ruta.clone(),
                orbital_drive::estado::ruta_copia(&cache, &m.id, &m.nombre),
            )
        })
        .collect();
    if orbital_vault::ubicacion::igual(root, &mapa) {
        return false;
    }
    orbital_vault::ubicacion::registrar(root, mapa);
    true
}

/// En el hilo del escaneo (`prepare`), sin red: suma a la bóveda lo «solo en Drive» que dejó
/// anotado el motor. Una bóveda sin Drive cuesta una mirada al disco.
pub(super) fn marcadores_al_escanear(v: &mut Vault) {
    let root = v.root().to_path_buf();
    if !orbital_drive::conectada(&root) {
        orbital_vault::ubicacion::registrar(&root, HashMap::new());
        return;
    }
    let (dir, _) = carpetas(&root);
    let marcas = orbital_drive::estado::leer_marcadores(&dir);
    let remotos: Vec<(RelPath, u64, Option<std::time::SystemTime>)> = marcas
        .iter()
        .filter_map(|m| {
            let p = RelPath::new(&m.ruta).ok()?;
            let t = (m.mtime > 0)
                .then(|| std::time::UNIX_EPOCH + std::time::Duration::from_secs(m.mtime as u64));
            Some((p, m.tam, t))
        })
        .collect();
    v.agregar_remotos(&remotos);
    registrar_marcadores(&root, &marcas);
}

/// Una regla para el selector: carpetas con `/` final.
fn ruta_regla(rel: &str, carpeta: bool) -> String {
    if carpeta {
        format!("{}/", rel.trim_end_matches('/'))
    } else {
        rel.to_owned()
    }
}

impl Window {
    /// Desde `show_vault`: una bóveda nueva detiene el motor de la anterior y, después del primer
    /// cuadro, arranca el suyo si está conectada.
    pub(super) fn drive_en_show_vault(&self, root: &Path) {
        if self.0.drive.borrow().root.as_deref() == Some(root) {
            return; // la misma bóveda releída: el motor sigue
        }
        {
            let mut d = self.0.drive.borrow_mut();
            // Soltar el motor le pide terminar; su hilo guarda el estado y sale solo.
            d.motor = None;
            d.root = Some(root.to_path_buf());
            d.generacion += 1;
            d.estados = Arc::default();
            d.reglas = Arc::default();
            d.conflictos = Arc::default();
            d.resumen = None;
            d.detenido = None;
            d.frescos.clear();
            d.pedidos.clear();
            d.abierta = None;
            d.vigiladas.clear();
        }
        let vieja = self.0.drive.borrow_mut().bajada.take();
        if let Some(b) = vieja {
            self.drive_soltar_bajada(b);
        }
        // Las filas anotadas son las de la lista nueva (las viejas se sueltan al desligarse).
        self.drive_repintar(None);
        self.0.status.set_drive(None);
        let (this, root) = (self.clone(), root.to_path_buf());
        self.after_next_frame(move || {
            // Una sola mirada al disco: sin `drive.yaml` no se carga nada más.
            if orbital_drive::conectada(&root) {
                this.drive_arrancar(&root);
            }
        });
    }

    /// Arranca el motor de la bóveda abierta.
    pub(super) fn drive_arrancar(&self, root: &Path) {
        let (tx, mut rx) = futures_channel::mpsc::unbounded::<Evento>();
        #[allow(unused_mut)]
        let mut cfg = orbital_drive::motor::Config::new(root);
        #[allow(unused_mut)]
        let mut fabrica: Fabrica = Box::new(orbital_drive::api_del_equipo);
        #[cfg(test)]
        if let Some((url, dir)) = PRUEBA.lock().unwrap().clone() {
            cfg.dir_cache = dir.join("cache");
            cfg.dir_estado = dir;
            cfg.papelera = orbital_drive::Papelera::Boveda;
            cfg.espera_subida = std::time::Duration::from_millis(300);
            cfg.intervalo_activo = std::time::Duration::from_millis(500);
            cfg.intervalo_fondo = std::time::Duration::from_millis(500);
            fabrica = Box::new(move |_| {
                Ok(Box::new(orbital_drive::cliente::Cliente::new(
                    orbital_drive::cliente::Direcciones::locales(&url),
                    orbital_drive::cliente::Credenciales {
                        client_id: "x".into(),
                        client_secret: "x".into(),
                        refresh_token: "x".into(),
                    },
                )) as Box<dyn orbital_drive::Api>)
            });
        }
        let motor = match Motor::iniciar(cfg, fabrica, move |e| {
            let _ = tx.unbounded_send(e);
        }) {
            Ok(m) => m,
            Err(e) => {
                self.toast(&format!("No se pudo empezar a sincronizar con Drive: {e}"));
                return;
            }
        };
        if !self.0.window.is_active() {
            motor.enviar(Orden::Foco(false));
        }
        let generacion = {
            let mut d = self.0.drive.borrow_mut();
            d.motor = Some(motor);
            d.root = Some(root.to_path_buf());
            d.generacion += 1;
            d.detenido = None;
            d.generacion
        };
        self.drive_barra();
        self.drive_senales();
        // Lo «solo en Drive» que se pidió abrir antes de que arrancara el motor.
        if let Some((r, pedido)) = self.drive_bajada_pendiente() {
            self.drive_orden(Orden::Traer(r, pedido));
        }
        let this = self.clone();
        glib::spawn_future_local(async move {
            use futures_core::Stream;
            while let Some(e) =
                std::future::poll_fn(|cx| std::pin::Pin::new(&mut rx).poll_next(cx)).await
            {
                if this.0.drive.borrow().generacion != generacion {
                    break;
                }
                this.drive_evento(e);
            }
        });
    }

    /// Foco de la ventana y estado de la red (una vez por ventana, al primer motor).
    fn drive_senales(&self) {
        if std::mem::replace(&mut self.0.drive.borrow_mut().señales, true) {
            return;
        }
        let weak = Rc::downgrade(&self.0);
        crate::drive_remoto::configurar(Rc::new(move |rel| {
            if let Some(i) = weak.upgrade() {
                Window(i).drive_pedir_embed(rel);
            }
        }));
        let this = self.clone();
        self.0.window.connect_is_active_notify(move |w| {
            this.drive_orden(Orden::Foco(w.is_active()));
        });
        let this = self.clone();
        let red = gio::NetworkMonitor::default();
        this.drive_orden(Orden::Red(red.is_network_available()));
        red.connect_network_changed(move |_, hay| this.drive_orden(Orden::Red(hay)));
    }

    pub(super) fn drive_orden(&self, o: Orden) {
        if let Some(m) = &self.0.drive.borrow().motor {
            m.enviar(o);
        }
    }

    /// Si hay conexión con Drive para bajar lo «solo en Drive».
    pub(super) fn drive_en_linea(&self) -> bool {
        let d = self.0.drive.borrow();
        d.motor.is_some()
            && d.resumen
                .as_ref()
                .is_none_or(|r| r.en_linea && r.pausa.is_none())
    }

    /// Si `rel` está «solo en Drive» (un marcador: no hay copia en la bóveda).
    pub(super) fn drive_es_marcador(&self, rel: &str) -> bool {
        self.0
            .drive
            .borrow()
            .root
            .as_deref()
            .is_some_and(|root| orbital_vault::ubicacion::resolver(root, rel).is_some())
    }

    /// Antes de abrir una entrada (desde `show_entry`): lo «solo en Drive» se baja primero (con
    /// conexión) o se explica por qué no se abre (sin conexión). `true`: no abrir todavía.
    pub(super) fn drive_antes_de_abrir(&self, entry: &Entry) -> bool {
        let rel = entry.path.as_str();
        if entry.kind == EntryKind::Folder || !self.drive_es_marcador(rel) {
            self.drive_avisar_abierta(Some(rel));
            return false;
        }
        let nombre = entry.path.file_name().to_owned();
        *self.0.current_entry.borrow_mut() = None;
        let arrancando = {
            let d = self.0.drive.borrow();
            d.motor.is_none() && d.detenido.is_none()
        };
        if !arrancando && !self.drive_en_linea() {
            self.drive_pagina(
                "system-lock-screen-symbolic",
                "Solo en Drive",
                &format!("«{nombre}» no está en este equipo y no hay conexión con Google Drive: se va a poder abrir cuando vuelva la red."),
            );
            return true;
        }
        if self.0.drive.borrow().frescos.contains(rel) {
            self.drive_avisar_abierta(Some(rel));
            return false;
        }
        self.drive_bajar(rel, super::drive_bajada::ParaQue::Abrir);
        true
    }

    /// Muestra la página vacía del centro con un aviso (lo «solo en Drive» que no se abre).
    pub(super) fn drive_pagina(&self, icono: &str, titulo: &str, texto: &str) {
        let i = &self.0;
        if let Some(p) = i
            .content_stack
            .child_by_name("vacio")
            .and_downcast::<adw::StatusPage>()
        {
            p.set_icon_name(Some(icono));
            p.set_title(titulo);
            p.set_description(Some(texto));
        }
        i.content_stack.set_visible_child_name("vacio");
        i.list_view.announce(
            &format!("{titulo}. {texto}"),
            gtk::AccessibleAnnouncementPriority::Medium,
        );
    }

    /// Respuesta del motor a un pedido de copia (abrir, «Abrir con» o embed).
    fn drive_traido(&self, rel: &str, pedido: u64, res: Result<PathBuf, String>) {
        {
            let mut d = self.0.drive.borrow_mut();
            d.pedidos.remove(rel);
            if res.is_ok() {
                d.frescos.insert(rel.to_owned());
            }
        }
        if self.drive_bajada_terminada(rel, pedido, &res) {
            return;
        }
        if res.is_ok() {
            // Un embed: se vuelve a dibujar la nota abierta.
            crate::drive_remoto::terminado(rel);
            if !self.0.code_editor.has_pending() {
                self.0.code_editor.render_now();
            }
        }
    }

    /// Un embed «solo en Drive» sin copia (desde la Lectura).
    fn drive_pedir_embed(&self, rel: String) {
        if !self.drive_en_linea() || !self.0.drive.borrow_mut().pedidos.insert(rel.clone()) {
            return;
        }
        self.drive_orden(Orden::Traer(rel, 0));
    }

    /// Le dice al motor qué nota está abierta (no se libera espacio de la nota que se edita).
    fn drive_avisar_abierta(&self, rel: Option<&str>) {
        let cambia = {
            let mut d = self.0.drive.borrow_mut();
            let nueva = rel.map(str::to_owned);
            if d.abierta == nueva {
                false
            } else {
                d.abierta = nueva;
                true
            }
        };
        if cambia {
            self.drive_orden(Orden::Abierta(rel.map(str::to_owned)));
        }
    }

    pub(super) fn drive_activo(&self) -> bool {
        self.0.drive.borrow().motor.is_some()
    }

    /// Cambios del disco (del vigilante de la bóveda) para el motor.
    pub(super) fn drive_cambios(&self, paths: &[PathBuf]) {
        let d = self.0.drive.borrow();
        let (Some(m), Some(root)) = (&d.motor, &d.root) else {
            return;
        };
        let rutas: Vec<String> = paths
            .iter()
            .filter_map(|p| p.strip_prefix(root).ok())
            .map(|p| p.to_string_lossy().into_owned())
            .collect();
        if !rutas.is_empty() {
            m.enviar(Orden::Cambios(rutas));
        }
    }

    fn drive_evento(&self, e: Evento) {
        match e {
            Evento::Estados(m, reglas) => {
                {
                    let mut d = self.0.drive.borrow_mut();
                    d.estados = m;
                    d.reglas = reglas;
                }
                self.drive_repintar(None);
            }
            Evento::Uno(r, e) => {
                {
                    let mut d = self.0.drive.borrow_mut();
                    Arc::make_mut(&mut d.estados).insert(r.clone(), e);
                }
                self.drive_repintar(Some(&r));
            }
            Evento::Resumen(r) => {
                let cambia_red = {
                    let mut d = self.0.drive.borrow_mut();
                    let antes = d.resumen.as_ref().map(|x| x.en_linea);
                    let ahora = r.en_linea;
                    d.resumen = Some(r);
                    antes != Some(ahora)
                };
                crate::drive_remoto::poner_en_linea(self.drive_en_linea());
                self.drive_barra();
                if cambia_red {
                    // «Solo en Drive» ↔ «bloqueado sin conexión».
                    self.drive_repintar(None);
                }
            }
            Evento::Marcadores(m) => {
                let Some(root) = self.0.drive.borrow().root.clone() else {
                    return;
                };
                if registrar_marcadores(&root, &m) {
                    // El árbol, el índice y el grafo con lo «solo en Drive» de ahora.
                    let this = self.clone();
                    glib::spawn_future_local(async move {
                        let actual = this
                            .0
                            .current_entry
                            .borrow()
                            .as_ref()
                            .map(|e| e.path.clone());
                        this.reload(actual).await;
                    });
                }
            }
            Evento::Traido(r, pedido, res) => self.drive_traido(&r, pedido, res),
            Evento::Progreso {
                ruta,
                pedido,
                bajado,
                total,
            } => self.drive_progreso(&ruta, pedido, bajado, total),
            Evento::Conflictos(c) => {
                self.0.drive.borrow_mut().conflictos = c;
            }
            Evento::Conflicto(r) => {
                let t = adw::Toast::builder()
                    .title(format!(
                        "«{r}» cambió acá y en Drive: se guardaron las dos versiones"
                    ))
                    .button_label("Resolver…")
                    .action_name("win.drive-resolver")
                    .action_target(&r.to_variant())
                    .timeout(10)
                    .build();
                self.0.toasts.add_toast(t);
            }
            Evento::Fusionada(r) => {
                self.toast(&format!(
                    "Se unieron los cambios de «{r}» hechos en otro equipo (las dos versiones quedaron en el historial)"
                ));
            }
            Evento::Aviso(t) => self.toast(&t),
            Evento::Enlace(r, enlace) => match enlace {
                Some(u) => {
                    self.0.window.clipboard().set_text(&u);
                    self.toast("Enlace de Drive copiado");
                }
                None => self.toast(&format!("«{r}» todavía no está en Drive")),
            },
            Evento::Detenido(motivo) => {
                let mut d = self.0.drive.borrow_mut();
                d.motor = None;
                d.detenido = motivo;
                drop(d);
                crate::drive_remoto::poner_en_linea(false);
                self.drive_barra();
                // Lo «solo en Drive» queda bloqueado: no hay motor que lo baje.
                self.drive_repintar(None);
            }
        }
    }

    /// Estado de una ruta para el explorador (`None`: bóveda sin conectar o ruta sin datos).
    pub(super) fn drive_estado(&self, rel: &str) -> Option<EstadoFila> {
        if self.drive_es_marcador(rel) {
            // Un renombre en cola (sin conexión): «pendiente» hasta que llegue a Drive.
            if self.0.drive.borrow().estados.get(rel) == Some(&EstadoArchivo::Pendiente) {
                return Some(EstadoFila::Drive(EstadoArchivo::Pendiente));
            }
            // Sin conexión, lo «solo en Drive» queda bloqueado (antes de saberlo, no).
            let conocido = {
                let d = self.0.drive.borrow();
                d.motor.is_some() || d.resumen.is_some() || d.detenido.is_some()
            };
            return Some(if conocido && !self.drive_en_linea() {
                EstadoFila::Bloqueado
            } else {
                EstadoFila::Drive(EstadoArchivo::SoloDrive)
            });
        }
        let d = self.0.drive.borrow();
        if d.motor.is_none() && d.resumen.is_none() {
            return None;
        }
        let regla =
            orbital_drive::reglas::ubicacion(d.reglas.iter().map(|(r, u)| (r.as_str(), *u)), rel);
        if regla == Some(Ubicacion::Local) {
            return Some(EstadoFila::Local);
        }
        match d.estados.get(rel).copied() {
            // Una carpeta «solo en Drive» con todo al día.
            Some(EstadoArchivo::Ambos) if regla == Some(Ubicacion::SoloDrive) => {
                Some(EstadoFila::Drive(EstadoArchivo::SoloDrive))
            }
            e => e.map(EstadoFila::Drive),
        }
    }

    /// La ubicación de una ruta para el selector: la regla que la cubre o, sin regla, lo que es.
    pub(super) fn drive_ubicacion(&self, rel: &str) -> Ubicacion {
        let regla = {
            let d = self.0.drive.borrow();
            orbital_drive::reglas::ubicacion(d.reglas.iter().map(|(r, u)| (r.as_str(), *u)), rel)
        };
        if let Some(u) = regla {
            return u;
        }
        match self.drive_estado(rel) {
            Some(EstadoFila::Drive(EstadoArchivo::SoloDrive)) | Some(EstadoFila::Bloqueado) => {
                Ubicacion::SoloDrive
            }
            Some(EstadoFila::Local) | Some(EstadoFila::Drive(EstadoArchivo::Local)) => {
                Ubicacion::Local
            }
            _ => Ubicacion::Ambos,
        }
    }

    /// Cambia la ubicación de una ruta (el selector del explorador y del menú de la nota).
    pub(super) fn drive_ubicar(&self, rel: &str, carpeta: bool, u: Ubicacion) {
        let regla = ruta_regla(rel, carpeta);
        let compartida = self
            .0
            .drive
            .borrow()
            .reglas
            .iter()
            .any(|(r, x)| *x == Ubicacion::Local && r.trim_end_matches('/') == rel);
        match u {
            Ubicacion::Local => {
                self.drive_orden(Orden::Ubicar(regla.clone(), None));
                self.drive_poner_local(&regla, true);
            }
            _ => {
                if compartida {
                    let actual: Vec<String> = self
                        .0
                        .drive
                        .borrow()
                        .reglas
                        .iter()
                        .filter(|(r, x)| *x == Ubicacion::Local && r.trim_end_matches('/') == rel)
                        .map(|(r, _)| r.clone())
                        .collect();
                    for r in actual {
                        self.drive_poner_local(&r, false);
                    }
                }
                self.drive_orden(Orden::Ubicar(regla, Some(u)));
            }
        }
        let nombre = rel.rsplit('/').next().unwrap_or(rel);
        self.toast(&match u {
            Ubicacion::SoloDrive => format!("«{nombre}» va a quedar solo en Drive: la copia de este equipo se saca cuando Drive confirme que tiene la misma versión"),
            Ubicacion::Ambos => format!("«{nombre}» va a estar en este equipo y en Drive"),
            Ubicacion::Local => format!("«{nombre}» queda solo en este equipo"),
        });
    }

    /// Pinta el estado de una fila del explorador y la anota para repintarla después.
    pub(super) fn drive_pintar_fila(
        &self,
        rel: &str,
        icono: &gtk::Image,
        item: &gtk::ListItem,
        base: String,
    ) {
        pintar(icono, item, &base, self.drive_estado(rel));
        self.0.drive_filas.borrow_mut().insert(
            rel.to_owned(),
            FilaDrive {
                icono: icono.downgrade(),
                item: item.downgrade(),
                base,
            },
        );
    }

    pub(super) fn drive_soltar_fila(&self, rel: &str) {
        self.0.drive_filas.borrow_mut().remove(rel);
    }

    /// Repinta las filas visibles (todas, o la de `solo` y sus carpetas).
    fn drive_repintar(&self, solo: Option<&str>) {
        let filas = self.0.drive_filas.borrow();
        let tocar = |rel: &str| {
            if let Some(f) = filas.get(rel)
                && let (Some(i), Some(it)) = (f.icono.upgrade(), f.item.upgrade())
            {
                pintar(&i, &it, &f.base, self.drive_estado(rel));
            }
        };
        match solo {
            Some(r) => {
                tocar(r);
                let mut p = r;
                while let Some((padre, _)) = p.rsplit_once('/') {
                    tocar(padre);
                    p = padre;
                }
            }
            None => {
                for rel in filas.keys() {
                    tocar(rel);
                }
            }
        }
    }

    /// La barra de estado.
    fn drive_barra(&self) {
        let d = self.0.drive.borrow();
        if d.root.is_none() || (d.motor.is_none() && d.detenido.is_none()) {
            drop(d);
            self.0.status.set_drive(None);
            return;
        }
        let (icono, texto, detalle) = match (&d.detenido, &d.resumen) {
            (Some(m), _) => (
                "dialog-warning-symbolic",
                "Drive: detenido".to_owned(),
                format!("La sincronización está detenida: {m}"),
            ),
            (None, None) => (
                "weather-overcast-symbolic",
                "Drive".to_owned(),
                "Empezando a sincronizar".to_owned(),
            ),
            (None, Some(r)) => {
                let detalle = crate::drive_dialogo::texto_resumen(r);
                let (icono, corto) = if r.pausa.is_some() {
                    ("dialog-warning-symbolic", "Drive: en pausa".to_owned())
                } else if r.conflictos > 0 {
                    (
                        "dialog-warning-symbolic",
                        format!("Drive: {} en conflicto", r.conflictos),
                    )
                } else if !r.en_linea {
                    ("network-offline-symbolic", "Drive: sin conexión".to_owned())
                } else if r.sincronizando {
                    ("view-refresh-symbolic", "Drive: sincronizando".to_owned())
                } else if r.pendientes > 0 {
                    (
                        "go-up-symbolic",
                        format!("Drive: {} por subir", r.pendientes),
                    )
                } else {
                    ("weather-overcast-symbolic", "Drive: al día".to_owned())
                };
                (icono, corto, detalle)
            }
        };
        drop(d);
        self.0.status.set_drive(Some((icono, &texto, &detalle)));
    }

    /// El diálogo «Google Drive».
    pub(super) fn drive_dialogo(&self) {
        let Some(root) = self
            .0
            .vault
            .borrow()
            .as_ref()
            .map(|v| v.root().to_path_buf())
        else {
            self.toast("Abrí una bóveda para conectarla con Google Drive");
            return;
        };
        let this = self.clone();
        glib::spawn_future_local(async move {
            let r = root.clone();
            // El llavero y el disco, fuera del hilo de GTK.
            let (conexion, cliente) = gio::spawn_blocking(move || {
                let c = orbital_drive::Conexion::leer(&r).ok().flatten();
                let l = orbital_drive::llavero::del_entorno();
                let cliente = orbital_drive::llavero::leer_cliente(&*l).ok().flatten();
                (c, cliente)
            })
            .await
            .unwrap_or((None, None));
            let d = this.0.drive.borrow();
            let datos = crate::drive_dialogo::Datos {
                nombre_boveda: root
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| "Bóveda".into()),
                root: root.clone(),
                conexion,
                activa: d.motor.is_some(),
                resumen: d.resumen.clone(),
                detenido: d.detenido.clone(),
                conflictos: d.conflictos.to_vec(),
                cliente,
                reglas: d.reglas.to_vec(),
            };
            drop(d);
            let t = this.clone();
            crate::drive_dialogo::mostrar(
                &this.0.window,
                datos,
                Rc::new(move |p| t.drive_pedido(p, &root)),
            );
        });
    }

    fn drive_pedido(&self, p: crate::drive_dialogo::Pedido, root: &Path) {
        use crate::drive_dialogo::Pedido;
        match p {
            Pedido::Ahora => self.drive_orden(Orden::Ahora),
            Pedido::Resolver(r) => self.drive_resolver(&r),
            Pedido::PonerLocal(r, local) => self.drive_poner_local(&r, local),
            Pedido::QuitarRegla(r) => self.drive_orden(Orden::Ubicar(r, None)),
            Pedido::Predeterminados(local, remoto) => {
                self.drive_orden(Orden::Predeterminados { local, remoto })
            }
            Pedido::Desconectar => {
                let dir = orbital_drive::estado::carpeta_estado(root);
                {
                    let mut d = self.0.drive.borrow_mut();
                    d.motor = None;
                    d.generacion += 1;
                    d.estados = Arc::default();
                    d.resumen = None;
                    d.detenido = None;
                }
                self.0.status.set_drive(None);
                self.drive_repintar(None);
                // El estado del equipo se borra cuando el hilo del motor soltó su candado.
                glib::spawn_future_local(async move {
                    let _ = gio::spawn_blocking(move || {
                        for _ in 0..50 {
                            if orbital_drive::estado::Candado::tomar(&dir).is_ok() {
                                break;
                            }
                            std::thread::sleep(std::time::Duration::from_millis(100));
                        }
                        let _ = std::fs::remove_dir_all(&dir);
                    })
                    .await;
                });
                self.toast("Este equipo dejó de sincronizar la bóveda con Drive");
            }
            Pedido::Conectar { cuenta, destino } => {
                let (this, root) = (self.clone(), root.to_path_buf());
                self.toast("Conectando con Google Drive…");
                glib::spawn_future_local(async move {
                    let r = root.clone();
                    let res = gio::spawn_blocking(move || -> Result<(), String> {
                        let estado = orbital_drive::Estado::nuevo(&cuenta, "");
                        let mut api = orbital_drive::api_del_equipo(&estado)?;
                        let dir = orbital_drive::estado::carpeta_estado(&r);
                        orbital_drive::motor::conectar(&mut *api, &r, &dir, &cuenta, destino)
                            .map(|_| ())
                            .map_err(|e| e.to_string())
                    })
                    .await
                    .unwrap_or_else(|_| Err("error inesperado".into()));
                    match res {
                        Ok(()) => {
                            this.drive_arrancar(&root);
                            this.toast("La bóveda quedó conectada con Google Drive");
                        }
                        Err(e) => this.toast(&format!("No se pudo conectar con Drive: {e}")),
                    }
                });
            }
        }
    }

    /// Agrega o quita una regla «solo en este equipo» (y pregunta si se saca la copia de Drive).
    pub(super) fn drive_poner_local(&self, rel: &str, local: bool) {
        let Some(root) = self.0.drive.borrow().root.clone() else {
            return;
        };
        let (this, rel) = (self.clone(), rel.to_owned());
        glib::spawn_future_local(async move {
            let (r, rr) = (root.clone(), rel.clone());
            let ok = gio::spawn_blocking(move || {
                let mut c = orbital_drive::Conexion::leer(&r).ok().flatten()?;
                c.poner_local(&rr, local);
                c.guardar(&r).ok()
            })
            .await
            .ok()
            .flatten()
            .is_some();
            if !ok {
                this.toast("No se pudo guardar la regla en .orbital/drive.yaml");
                return;
            }
            this.drive_orden(Orden::Ahora);
            let en_drive = this
                .0
                .drive
                .borrow()
                .estados
                .keys()
                .any(|k| k == rel.trim_end_matches('/') || k.starts_with(&rel));
            if local && en_drive {
                let aviso = adw::AlertDialog::new(
                    Some("¿Sacar también la copia de Drive?"),
                    Some(&format!(
                        "«{rel}» ya no se va a subir. La copia que ya está en Drive puede quedarse ahí o ir a la papelera de Drive (se puede recuperar desde la web). En este equipo no se toca nada."
                    )),
                );
                aviso.add_response("dejar", "Dejarla en Drive");
                aviso.add_response("papelera", "A la papelera de Drive");
                aviso.set_close_response("dejar");
                let t = this.clone();
                aviso.choose(Some(&this.0.window), gio::Cancellable::NONE, move |r| {
                    if r == "papelera" {
                        t.drive_orden(Orden::QuitarDeDrive(rel.trim_end_matches('/').to_owned()));
                    }
                });
            }
        });
    }

    /// Resolver el conflicto de una ruta.
    pub(super) fn drive_resolver(&self, rel: &str) {
        let (root, info) = {
            let d = self.0.drive.borrow();
            (
                d.root.clone(),
                d.conflictos.iter().find(|c| c.ruta == rel).cloned(),
            )
        };
        let (Some(root), Some(info)) = (root, info) else {
            self.toast("Ese conflicto ya está resuelto");
            return;
        };
        let this = self.clone();
        glib::spawn_future_local(async move {
            let (r, i) = (root.clone(), info.clone());
            let textos = gio::spawn_blocking(move || {
                let local = std::fs::read(r.join(&i.ruta)).ok()?;
                let remoto = std::fs::read(r.join(&i.copia)).ok()?;
                if !orbital_drive::local::es_texto(&i.ruta)
                    || local.len() > 1 << 20
                    || remoto.len() > 1 << 20
                {
                    return None;
                }
                Some((
                    String::from_utf8(local).ok()?,
                    String::from_utf8(remoto).ok()?,
                ))
            })
            .await
            .ok()
            .flatten();
            let formato = if info.ruta.ends_with(".html") || info.ruta.ends_with(".htm") {
                Format::Html
            } else {
                Format::Md
            };
            let t = this.clone();
            let ruta = info.ruta.clone();
            crate::drive_dialogo::resolver(&this.0.window, &info, textos, formato, move |mia| {
                t.drive_orden(Orden::Resolver {
                    ruta: ruta.clone(),
                    mia,
                });
            });
        });
    }

    /// Opciones de Drive del menú de la nota abierta (solo con la bóveda conectada).
    pub(super) fn items_menu_drive(&self) -> Vec<MenuItem> {
        if !self.drive_activo() {
            return Vec::new();
        }
        let Some(ruta) = self
            .0
            .current_entry
            .borrow()
            .as_ref()
            .map(|e| e.path.as_str().to_owned())
        else {
            return Vec::new();
        };
        let mut items = Vec::new();
        if self
            .0
            .drive
            .borrow()
            .conflictos
            .iter()
            .any(|c| c.ruta == ruta)
        {
            items.push(MenuItem::new(
                "Resolver el conflicto con Drive…",
                "win.drive-resolver-nota",
                "",
            ));
        }
        items.extend(self.items_ubicacion(&ruta, false));
        if self.drive_ubicacion(&ruta) != Ubicacion::Local {
            items.push(MenuItem::new(
                "Copiar enlace de Drive",
                "win.drive-enlace",
                "",
            ));
        }
        items
    }

    /// «Ubicación: Solo en este equipo · Solo en Drive · En ambos», con la actual marcada.
    pub(super) fn items_ubicacion(&self, rel: &str, carpeta: bool) -> Vec<MenuItem> {
        let actual = self.drive_ubicacion(rel);
        let tipo = if carpeta { "c" } else { "a" };
        [Ubicacion::Local, Ubicacion::SoloDrive, Ubicacion::Ambos]
            .into_iter()
            .map(|u| {
                let mut m =
                    MenuItem::new(&format!("Ubicación: {}", u.texto()), "win.drive-ubicar", "");
                m.target = Some(format!("{}\u{1f}{tipo}\u{1f}{rel}", u.clave()));
                m.marcado = Some(u == actual);
                m
            })
            .collect()
    }

    /// Acciones `win.drive*`.
    pub(super) fn setup_drive_actions(&self) {
        let ruta_actual = |w: &Window| {
            w.0.current_entry
                .borrow()
                .as_ref()
                .map(|e| e.path.as_str().to_owned())
        };
        let this = self.clone();
        let abrir = gio::ActionEntry::builder("drive")
            .activate(move |_: &adw::ApplicationWindow, _, _| this.drive_dialogo())
            .build();
        let this = self.clone();
        let resolver = gio::ActionEntry::builder("drive-resolver")
            .parameter_type(Some(glib::VariantTy::STRING))
            .activate(move |_: &adw::ApplicationWindow, _, p| {
                if let Some(r) = p.and_then(|v| v.get::<String>()) {
                    this.drive_resolver(&r);
                }
            })
            .build();
        let this = self.clone();
        let resolver_nota = gio::ActionEntry::builder("drive-resolver-nota")
            .activate(move |_: &adw::ApplicationWindow, _, _| {
                if let Some(r) = ruta_actual(&this) {
                    this.drive_resolver(&r);
                }
            })
            .build();
        let this = self.clone();
        let enlace = gio::ActionEntry::builder("drive-enlace")
            .activate(move |_: &adw::ApplicationWindow, _, _| {
                if let Some(r) = ruta_actual(&this) {
                    this.drive_orden(Orden::Enlace(r));
                }
            })
            .build();
        let this = self.clone();
        let ubicar = gio::ActionEntry::builder("drive-ubicar")
            .parameter_type(Some(glib::VariantTy::STRING))
            .activate(move |_: &adw::ApplicationWindow, _, p| {
                let Some(t) = p.and_then(|v| v.get::<String>()) else {
                    return;
                };
                let mut partes = t.splitn(3, '\u{1f}');
                if let (Some(u), Some(tipo), Some(rel)) =
                    (partes.next(), partes.next(), partes.next())
                    && let Some(u) = Ubicacion::desde_clave(u)
                {
                    this.drive_ubicar(rel, tipo == "c", u);
                }
            })
            .build();
        let this = self.clone();
        let abrir_con_de = gio::ActionEntry::builder("abrir-con-de")
            .parameter_type(Some(glib::VariantTy::STRING))
            .activate(move |_: &adw::ApplicationWindow, _, p| {
                if let Some(r) = p.and_then(|v| v.get::<String>()) {
                    this.abrir_con_otra(&r);
                }
            })
            .build();
        let this = self.clone();
        let enlace_de = gio::ActionEntry::builder("drive-enlace-de")
            .parameter_type(Some(glib::VariantTy::STRING))
            .activate(move |_: &adw::ApplicationWindow, _, p| {
                if let Some(r) = p.and_then(|v| v.get::<String>()) {
                    this.drive_orden(Orden::Enlace(r));
                }
            })
            .build();
        self.0.window.add_action_entries([
            abrir,
            resolver,
            resolver_nota,
            enlace,
            ubicar,
            enlace_de,
            abrir_con_de,
        ]);
        let this = self.clone();
        self.0.status.connect_drive(move || this.drive_dialogo());
    }
}

fn pintar(icono: &gtk::Image, item: &gtk::ListItem, base: &str, estado: Option<EstadoFila>) {
    match estado {
        Some(e) => {
            icono.set_icon_name(Some(e.icono()));
            icono.set_tooltip_text(Some(e.texto()));
            icono.set_visible(true);
            item.set_accessible_label(&format!("{base}, {}", e.texto()));
        }
        None => {
            icono.set_visible(false);
            item.set_accessible_label(base);
        }
    }
}
