//! Google Drive (061a): arranca el motor de sincronización si la bóveda está conectada (después
//! del primer cuadro: el arranque no cambia), le pasa los cambios del disco, el foco y la red, y
//! muestra lo que avisa: un ícono por fila en el explorador, el resumen en la barra de estado, los
//! conflictos y los avisos. Una bóveda sin `.orbital/drive.yaml` no carga nada de esto.

use super::*;

use orbital_drive::{EstadoArchivo, Evento, InfoConflicto, Motor, Orden, Resumen};

/// Lo que crea la API dentro del hilo del motor.
type Fabrica =
    Box<dyn FnOnce(&orbital_drive::Estado) -> Result<Box<dyn orbital_drive::Api>, String> + Send>;

/// Lo que la ventana sabe de Drive.
#[derive(Default)]
pub(super) struct DriveUi {
    motor: Option<Motor>,
    root: Option<PathBuf>,
    /// Sube con cada arranque del motor: los eventos de un motor viejo se ignoran.
    generacion: u64,
    pub(super) estados: Arc<HashMap<String, EstadoArchivo>>,
    pub(super) reglas: Arc<Vec<String>>,
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
}

impl EstadoFila {
    pub(super) fn icono(self) -> &'static str {
        match self {
            Self::Drive(EstadoArchivo::Ambos) => "weather-overcast-symbolic",
            Self::Drive(EstadoArchivo::Subiendo) => "go-up-symbolic",
            Self::Drive(EstadoArchivo::Bajando) => "go-down-symbolic",
            Self::Drive(EstadoArchivo::Conflicto) => "dialog-warning-symbolic",
            Self::Local => "computer-symbolic",
        }
    }

    pub(super) fn texto(self) -> &'static str {
        match self {
            Self::Drive(EstadoArchivo::Ambos) => "en este equipo y en Drive",
            Self::Drive(EstadoArchivo::Subiendo) => "subiendo a Drive",
            Self::Drive(EstadoArchivo::Bajando) => "bajando de Drive",
            Self::Drive(EstadoArchivo::Conflicto) => "en conflicto con Drive",
            Self::Local => "solo en este equipo",
        }
    }
}

/// Solo pruebas: la API del servidor falso y una carpeta de estado temporal (nunca la cuenta ni
/// el estado de la usuaria).
#[cfg(test)]
pub(super) static PRUEBA: std::sync::Mutex<Option<(String, PathBuf)>> = std::sync::Mutex::new(None);

/// Si una regla «solo en este equipo» cubre `rel`.
fn cubre(reglas: &[String], rel: &str) -> bool {
    reglas.iter().any(|r| {
        let b = r.trim_end_matches('/');
        !b.is_empty() && (rel == b || rel.strip_prefix(b).is_some_and(|x| x.starts_with('/')))
    })
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
                self.0.drive.borrow_mut().resumen = Some(r);
                self.drive_barra();
            }
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
                self.drive_barra();
            }
        }
    }

    /// Estado de una ruta para el explorador (`None`: bóveda sin conectar o ruta sin datos).
    pub(super) fn drive_estado(&self, rel: &str) -> Option<EstadoFila> {
        let d = self.0.drive.borrow();
        if d.motor.is_none() && d.resumen.is_none() {
            return None;
        }
        if cubre(&d.reglas, rel) {
            return Some(EstadoFila::Local);
        }
        d.estados.get(rel).copied().map(EstadoFila::Drive)
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
        let local = self.drive_estado(&ruta) == Some(EstadoFila::Local);
        items.push(MenuItem::new(
            if local {
                "Sincronizar con Drive"
            } else {
                "Solo en este equipo (no subir a Drive)"
            },
            "win.drive-solo-local",
            "",
        ));
        if !local {
            items.push(MenuItem::new(
                "Copiar enlace de Drive",
                "win.drive-enlace",
                "",
            ));
        }
        items
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
        let solo_local = gio::ActionEntry::builder("drive-solo-local")
            .activate(move |_: &adw::ApplicationWindow, _, _| {
                if let Some(r) = ruta_actual(&this) {
                    let local = this.drive_estado(&r) == Some(EstadoFila::Local);
                    this.drive_poner_local(&r, !local);
                }
            })
            .build();
        self.0
            .window
            .add_action_entries([abrir, resolver, resolver_nota, enlace, solo_local]);
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
