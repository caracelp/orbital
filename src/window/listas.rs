//! Favoritos y Guardados (027): estado de las listas, carga diferida, cola de escritura, acciones
//! `win.alternar-favorito` y `win.guardar-en`, y el menú de la nota con su estado.
//!
//! Cada cambio se aplica en memoria y se redibuja enseguida (SC-001); la escritura va a otro hilo,
//! de a una por vez, con el último estado. Las acciones que todavía no llegaron al disco se
//! guardan para repetirlas sobre lo releído si el archivo cambió afuera (research R4).

use super::*;
use crate::listas_panel::Orden;
use orbital_vault::{Lista, Listas, ListasSnapshot};

/// Un cambio de las listas: devuelve si cambió algo. Se puede repetir sobre otro estado.
pub(super) type AccionListas = Rc<dyn Fn(&mut Listas) -> bool>;

impl Window {
    /// Conecta el panel, el menú de la nota y las acciones (una vez, desde `conexiones`).
    pub(super) fn setup_listas(&self) {
        let weak = Rc::downgrade(&self.0);
        self.0
            .listas_panel
            .connect_abrir(move |ruta, _lista, falta| {
                let Some(i) = weak.upgrade() else { return };
                let w = Window(i);
                if falta {
                    // Decisión 8: no se busca el archivo por su nombre.
                    w.toast("El archivo ya no existe");
                } else {
                    w.show_path(ruta.as_str());
                }
            });
        let weak = Rc::downgrade(&self.0);
        self.0.listas_panel.connect_orden(move |orden| {
            if let Some(i) = weak.upgrade() {
                Window(i).orden_de_listas(orden);
            }
        });
        // La página Guardados se arma la primera vez que se muestra.
        let weak = Rc::downgrade(&self.0);
        self.0
            .sidebar_stack
            .connect_visible_child_name_notify(move |st| {
                if st.visible_child_name().as_deref() == Some("guardados")
                    && let Some(i) = weak.upgrade()
                    && i.listas_panel.construir_guardados()
                {
                    Window(i).redibujar_listas();
                }
            });
        // El menú de la nota dice «Quitar de favoritos» o «Guardada en N colecciones…» según el
        // modelo en memoria: se rearma cada vez que se abre (research R5).
        let weak = Rc::downgrade(&self.0);
        self.0.note_menu.connect_show(move |p| {
            if let Some(i) = weak.upgrade() {
                fill_menu(p, &Window(i).items_menu_nota());
            }
        });
        let alternar = gio::ActionEntry::builder("alternar-favorito")
            .activate({
                let this = self.clone();
                move |_: &adw::ApplicationWindow, _, _| this.alternar_favorito()
            })
            .build();
        let guardar_en = gio::ActionEntry::builder("guardar-en")
            .activate({
                let this = self.clone();
                move |_: &adw::ApplicationWindow, _, _| {
                    this.abrir_guardar_en();
                }
            })
            .build();
        self.0.window.add_action_entries([alternar, guardar_en]);
        self.habilitar_acciones_listas(false);
    }

    /// Las acciones de las listas solo valen con un archivo abierto.
    pub(super) fn habilitar_acciones_listas(&self, habilitadas: bool) {
        let habilitadas = habilitadas && !self.0.gone.get();
        for name in ["alternar-favorito", "guardar-en"] {
            if let Some(a) = self
                .0
                .window
                .lookup_action(name)
                .and_downcast::<gio::SimpleAction>()
            {
                a.set_enabled(habilitadas);
            }
        }
    }

    /// Opciones del menú «Más opciones de la nota».
    pub(super) fn items_menu_nota(&self) -> Vec<MenuItem> {
        let ruta = self
            .0
            .current_entry
            .borrow()
            .as_ref()
            .map(|e| e.path.clone());
        let (favorito, colecciones) = match (&ruta, self.0.listas.borrow().as_ref()) {
            (Some(r), Some(s)) => (s.listas.es_favorito(r), s.listas.colecciones_de(r)),
            _ => (false, 0),
        };
        let guardar = match colecciones {
            0 => "Guardar en…".to_owned(),
            1 => "Guardada en 1 colección…".to_owned(),
            n => format!("Guardada en {n} colecciones…"),
        };
        vec![
            MenuItem::new(
                if favorito {
                    "Quitar de favoritos"
                } else {
                    "Agregar a favoritos"
                },
                "win.alternar-favorito",
                "",
            ),
            MenuItem::new(&guardar, "win.guardar-en", ""),
            MenuItem::new("Renombrar o mover…", "win.rename", "F2"),
            MenuItem::new("Archivar", "win.archive", "Ctrl+Mayús+A"),
            MenuItem::new(
                "Pegar imagen como adjunto",
                "win.paste-image",
                "Ctrl+Mayús+V",
            ),
            MenuItem::new("Mover a la papelera", "win.trash", "Supr"),
        ]
    }

    /// Desde `show_vault`: una bóveda nueva carga sus listas después del primer cuadro; una
    /// relectura de la misma bóveda solo recalcula las faltantes (T012).
    pub(super) fn listas_en_show_vault(&self) {
        let i = &self.0;
        let sesion = i.vault_session.get();
        if i.listas_sesion.get() == sesion {
            self.redibujar_listas();
            return;
        }
        i.listas_sesion.set(sesion);
        *i.listas.borrow_mut() = None;
        i.listas_acciones.borrow_mut().clear();
        i.listas_escribiendo.set(false);
        i.listas_pendiente.set(false);
        i.listas_eco.set(None);
        *i.listas_watcher.borrow_mut() = None;
        let this = self.clone();
        self.after_next_frame(move || {
            crate::medir_arranque::marca("listas: lectura pedida (después del primer cuadro)");
            this.cargar_listas(sesion)
        });
    }

    /// Vigila el archivo de listas por cambios hechos afuera (T024); se crea al terminar la
    /// carga para no sumar nada antes del primer cuadro.
    fn vigilar_listas(&self, sesion: u64) {
        let Some(vault) = self.0.vault.borrow().clone() else {
            return;
        };
        let ruta = vault.root().join(orbital_vault::listas::LISTAS_PATH);
        let weak = Rc::downgrade(&self.0);
        *self.0.listas_watcher.borrow_mut() =
            Some(crate::watcher::ListasWatcher::new(&ruta, move || {
                if let Some(i) = weak.upgrade() {
                    Window(i).releer_listas(sesion);
                }
            }));
    }

    /// El archivo de listas cambió en disco: si no es el eco de una escritura propia, se relee y
    /// se muestra. Con cambios propios todavía sin escribir no se toca nada: su escritura va a
    /// encontrar el conflicto y repetirlos sobre lo nuevo (research R4).
    fn releer_listas(&self, sesion: u64) {
        let i = &self.0;
        if i.vault_session.get() != sesion || !self.listas_cargadas_ya() {
            return;
        }
        let Some(vault) = i.vault.borrow().clone() else {
            return;
        };
        let this = self.clone();
        glib::spawn_future_local(async move {
            let Ok(snap) = gio::spawn_blocking(move || vault.listas_snapshot()).await else {
                return;
            };
            let i = &this.0;
            if i.vault_session.get() != sesion
                || i.listas_escribiendo.get()
                || !i.listas_acciones.borrow().is_empty()
            {
                return;
            }
            // El eco de la última escritura propia, o lo mismo que ya se muestra.
            let mostrado = i.listas.borrow().as_ref().and_then(|s| s.hash);
            let eco = i.listas_eco.get();
            if snap.hash == mostrado || (eco.is_some() && snap.hash == eco) {
                return;
            }
            if let Some(aviso) = &snap.warning {
                this.toast(aviso);
            }
            *i.listas.borrow_mut() = Some(snap);
            this.redibujar_listas();
        });
    }

    /// Lee `.orbital/listas.yaml` en otro hilo (FR-013).
    fn cargar_listas(&self, sesion: u64) {
        let Some(vault) = self.0.vault.borrow().clone() else {
            return;
        };
        let this = self.clone();
        glib::spawn_future_local(async move {
            let Ok(snap) = gio::spawn_blocking(move || vault.listas_snapshot()).await else {
                return;
            };
            if this.0.vault_session.get() != sesion {
                return;
            }
            if let Some(aviso) = &snap.warning {
                this.toast(aviso);
            }
            this.listas_cargadas(snap);
            crate::medir_arranque::marca("listas: cargadas y dibujadas");
            this.vigilar_listas(sesion);
        });
    }

    /// Pone lo leído y repite encima las acciones que llegaron antes de la carga.
    fn listas_cargadas(&self, mut snap: ListasSnapshot) {
        let pendientes = !self.0.listas_acciones.borrow().is_empty();
        for a in self.0.listas_acciones.borrow().iter() {
            a(&mut snap.listas);
        }
        *self.0.listas.borrow_mut() = Some(snap);
        self.redibujar_listas();
        if pendientes {
            self.escribir_listas();
        }
    }

    /// Las listas ya se cargaron (para las pruebas y para quien necesite esperar la carga).
    pub(super) fn listas_cargadas_ya(&self) -> bool {
        self.0.listas.borrow().is_some()
    }

    /// Aplica un cambio en memoria, redibuja y encola la escritura. Si las listas todavía no se
    /// cargaron, el cambio espera la carga.
    pub(super) fn aplicar_listas(&self, accion: impl Fn(&mut Listas) -> bool + 'static) {
        let accion: AccionListas = Rc::new(accion);
        let cambio = {
            let mut l = self.0.listas.borrow_mut();
            match l.as_mut() {
                Some(s) => accion(&mut s.listas),
                // Sin cargar: se guarda para aplicarla sobre lo leído.
                None => true,
            }
        };
        if !cambio {
            return;
        }
        self.0.listas_acciones.borrow_mut().push(accion);
        if self.listas_cargadas_ya() {
            self.redibujar_listas();
            self.escribir_listas();
        }
    }

    /// Escribe el estado en memoria en otro hilo, una escritura por vez.
    fn escribir_listas(&self) {
        let i = &self.0;
        if i.listas_escribiendo.get() {
            i.listas_pendiente.set(true);
            return;
        }
        let (Some(vault), Some(snap)) = (i.vault.borrow().clone(), i.listas.borrow().clone())
        else {
            return;
        };
        if i.gone.get() {
            return self.toast("La bóveda no está disponible: los favoritos no se guardaron");
        }
        i.listas_escribiendo.set(true);
        i.listas_pendiente.set(false);
        let enviadas = i.listas_acciones.borrow().len();
        let sesion = i.vault_session.get();
        let this = self.clone();
        glib::spawn_future_local(async move {
            let ctx = write_ctx::now(Some(vault.config()));
            let listas = snap.listas.clone();
            let v = vault.clone();
            let resultado =
                gio::spawn_blocking(move || match v.guardar_listas(&listas, &snap, &ctx) {
                    Err(orbital_vault::VaultError::Conflict { .. }) => {
                        Err(Some(v.listas_snapshot()))
                    }
                    Err(e) => {
                        eprintln!("No se guardaron las listas: {e}");
                        Err(None)
                    }
                    Ok(nuevo) => Ok(nuevo),
                })
                .await;
            if this.0.vault_session.get() != sesion {
                return;
            }
            this.0.listas_escribiendo.set(false);
            match resultado {
                Ok(Ok(nuevo)) => {
                    this.0.listas_eco.set(nuevo.hash);
                    this.0.listas_acciones.borrow_mut().drain(..enviadas);
                    if let Some(s) = this.0.listas.borrow_mut().as_mut() {
                        // Lo de memoria puede ser más nuevo que lo escrito: se conserva, con el
                        // hash y el origen de lo que quedó en disco.
                        let actual = std::mem::take(&mut s.listas);
                        *s = nuevo;
                        s.listas = actual;
                    }
                    if this.0.listas_pendiente.get() {
                        this.escribir_listas();
                    }
                }
                Ok(Err(Some(releido))) => this.reintentar_listas(&vault, releido, sesion).await,
                _ => this.toast("No se pudieron guardar los favoritos ni los guardados"),
            }
        });
    }

    /// El archivo cambió afuera: repite sobre lo releído las acciones que no llegaron al disco y
    /// reintenta una vez; si falla otra vez, muestra lo que hay en disco.
    async fn reintentar_listas(
        &self,
        vault: &Arc<Vault>,
        mut releido: ListasSnapshot,
        sesion: u64,
    ) {
        let disco = releido.clone();
        for a in self.0.listas_acciones.borrow().iter() {
            a(&mut releido.listas);
        }
        let i = &self.0;
        i.listas_escribiendo.set(true);
        i.listas_pendiente.set(false);
        let enviadas = i.listas_acciones.borrow().len();
        let ctx = write_ctx::now(Some(vault.config()));
        let listas = releido.listas.clone();
        let v = vault.clone();
        let base = releido.clone();
        let resultado = gio::spawn_blocking(move || v.guardar_listas(&listas, &base, &ctx)).await;
        if i.vault_session.get() != sesion {
            return;
        }
        i.listas_escribiendo.set(false);
        match resultado {
            Ok(Ok(nuevo)) => {
                i.listas_eco.set(nuevo.hash);
                i.listas_acciones.borrow_mut().drain(..enviadas);
                *i.listas.borrow_mut() = Some(nuevo);
                self.redibujar_listas();
                if i.listas_pendiente.get() {
                    self.escribir_listas();
                }
            }
            _ => {
                i.listas_acciones.borrow_mut().clear();
                *i.listas.borrow_mut() = Some(disco);
                self.redibujar_listas();
                self.toast("Los favoritos cambiaron en el disco: se muestra lo que hay ahí");
            }
        }
    }

    /// Redibuja Favoritos (y Guardados si ya se armó) con las faltantes de la bóveda abierta.
    pub(super) fn redibujar_listas(&self) {
        let i = &self.0;
        let listas = i.listas.borrow();
        let Some(snap) = listas.as_ref() else {
            return;
        };
        let faltantes = match i.vault.borrow().as_ref() {
            Some(v) => orbital_vault::listas::faltantes(&snap.listas, v.entries()),
            None => Default::default(),
        };
        i.listas_panel
            .mostrar_favoritos(&snap.listas.favoritos, &faltantes);
        i.listas_panel
            .mostrar_guardados(&snap.listas.colecciones, &faltantes);
    }

    /// `win.alternar-favorito`: el archivo abierto entra o sale de Favoritos.
    pub(super) fn alternar_favorito(&self) {
        let Some(ruta) = self
            .0
            .current_entry
            .borrow()
            .as_ref()
            .map(|e| e.path.clone())
        else {
            return;
        };
        let antes = self
            .0
            .listas
            .borrow()
            .as_ref()
            .map(|s| s.listas.es_favorito(&ruta));
        self.aplicar_listas(move |l| l.alternar_favorito(&ruta));
        let mensaje = match antes {
            Some(true) => "Se quitó de favoritos",
            _ => "Se agregó a favoritos",
        };
        self.0
            .list_view
            .announce(mensaje, gtk::AccessibleAnnouncementPriority::Medium);
    }

    /// `win.guardar-en`: abre el diálogo y aplica lo elegido en un solo cambio. Devuelve el
    /// diálogo (las pruebas lo manejan desde adentro).
    pub(super) fn abrir_guardar_en(&self) -> Option<crate::guardar_en::GuardarEn> {
        let ruta = self
            .0
            .current_entry
            .borrow()
            .as_ref()
            .map(|e| e.path.clone())?;
        let colecciones: Vec<(String, bool)> = match self.0.listas.borrow().as_ref() {
            Some(s) => s
                .listas
                .colecciones
                .iter()
                .map(|c| (c.nombre.clone(), c.notas.contains(&ruta)))
                .collect(),
            None => {
                self.toast("Los guardados todavía se están cargando");
                return None;
            }
        };
        let dialogo = crate::guardar_en::GuardarEn::new(ruta.file_name(), &colecciones);
        let this = self.clone();
        let leer = dialogo.lector();
        dialogo.dialogo.connect_response(None, move |_, respuesta| {
            if respuesta == "guardar" {
                this.aplicar_guardar_en(&ruta, leer());
            }
        });
        dialogo.dialogo.present(Some(&self.0.window));
        Some(dialogo)
    }

    fn aplicar_guardar_en(&self, ruta: &RelPath, eleccion: crate::guardar_en::Eleccion) {
        let ruta = ruta.clone();
        let nueva = eleccion.nueva.clone();
        self.aplicar_listas(move |l| {
            let mut cambio = false;
            // Por nombre y no por índice: si el archivo cambió afuera, los índices pueden ser otros.
            for (nombre, dentro) in &eleccion.casillas {
                if let Some(n) = l.buscar_coleccion(nombre) {
                    cambio |= l.poner_en(n, &ruta, *dentro);
                }
            }
            if let Some(nombre) = &eleccion.nueva {
                match l.buscar_coleccion(nombre) {
                    Some(n) => cambio |= l.poner_en(n, &ruta, true),
                    None => {
                        cambio |= l
                            .crear_coleccion(nombre, Some(ruta.clone()))
                            .unwrap_or(false)
                    }
                }
            }
            cambio
        });
        if let Some(nombre) = nueva {
            self.toast(&format!("Guardada en «{nombre}»"));
        }
    }

    /// Lo que piden los menús de las filas y de las colecciones, y el arrastre (T022). Los
    /// índices son los de lo que se ve: se traducen a la ruta y al nombre de la colección para
    /// que la acción se pueda repetir sobre lo releído si el archivo cambió afuera (research R4).
    pub(super) fn orden_de_listas(&self, orden: Orden) {
        let (lista, indice) = match orden {
            Orden::Mover { lista, desde, .. } => (lista, desde),
            Orden::Quitar { lista, indice } => (lista, indice),
            Orden::RenombrarColeccion(n) => {
                self.pedir_renombrar_coleccion(n);
                return;
            }
            Orden::BorrarColeccion(n) => {
                self.pedir_borrar_coleccion(n);
                return;
            }
        };
        let (ruta, coleccion) = {
            let listas = self.0.listas.borrow();
            let Some(s) = listas.as_ref() else { return };
            let Some(ruta) = s.listas.lista(lista).get(indice).cloned() else {
                return;
            };
            let coleccion = match lista {
                Lista::Coleccion(n) => s.listas.colecciones.get(n).map(|c| c.nombre.clone()),
                Lista::Favoritos => None,
            };
            (ruta, coleccion)
        };
        // La lista como está ahora (por nombre si es una colección) y la posición de la ruta.
        let ubicar = move |l: &Listas| -> Option<(Lista, usize)> {
            let lista = match &coleccion {
                Some(nombre) => Lista::Coleccion(l.buscar_coleccion(nombre)?),
                None => Lista::Favoritos,
            };
            let n = l.lista(lista).iter().position(|r| *r == ruta)?;
            Some((lista, n))
        };
        let nombre = self
            .0
            .listas
            .borrow()
            .as_ref()
            .and_then(|s| {
                s.listas
                    .lista(lista)
                    .get(indice)
                    .map(|r| r.file_name().to_owned())
            })
            .unwrap_or_default();
        let mensaje = match orden {
            Orden::Mover { hasta, .. } => {
                self.aplicar_listas(move |l| match ubicar(l) {
                    Some((lista, n)) => l.mover(lista, n, hasta),
                    None => false,
                });
                format!("{nombre} ahora es la {}.ª", hasta + 1)
            }
            _ => {
                self.aplicar_listas(move |l| match ubicar(l) {
                    Some((lista, n)) => l.quitar(lista, n),
                    None => false,
                });
                match lista {
                    Lista::Favoritos => format!("Se quitó {nombre} de favoritos"),
                    Lista::Coleccion(_) => format!("Se quitó {nombre} de la colección"),
                }
            }
        };
        self.0
            .list_view
            .announce(&mensaje, gtk::AccessibleAnnouncementPriority::Medium);
    }

    /// «Renombrar…» de una colección: diálogo con el nombre; vacío o repetido no se acepta.
    /// Devuelve el diálogo y su campo (las pruebas lo manejan desde adentro).
    pub(super) fn pedir_renombrar_coleccion(
        &self,
        n: usize,
    ) -> Option<(adw::AlertDialog, gtk::Entry)> {
        let (viejo, otros): (String, Vec<String>) = {
            let listas = self.0.listas.borrow();
            let l = &listas.as_ref()?.listas;
            let viejo = l.colecciones.get(n)?.nombre.clone();
            let otros = l
                .colecciones
                .iter()
                .enumerate()
                .filter(|(k, _)| *k != n)
                .map(|(_, c)| c.nombre.trim().to_lowercase())
                .collect();
            (viejo, otros)
        };
        let campo = gtk::Entry::builder()
            .text(&viejo)
            .activates_default(true)
            .build();
        campo.update_property(&[gtk::accessible::Property::Label("Nombre de la colección")]);
        let aviso = gtk::Label::builder()
            .xalign(0.0)
            .wrap(true)
            .visible(false)
            .css_classes(["error"])
            .build();
        let caja = gtk::Box::new(gtk::Orientation::Vertical, 12);
        caja.append(&campo);
        caja.append(&aviso);
        let dialogo = adw::AlertDialog::new(
            Some("Renombrar la colección"),
            Some(&format!(
                "Nombre nuevo para «{viejo}». Las notas se conservan."
            )),
        );
        dialogo.set_extra_child(Some(&caja));
        dialogo.add_response("cancelar", "Cancelar");
        dialogo.add_response("renombrar", "Renombrar");
        dialogo.set_response_appearance("renombrar", adw::ResponseAppearance::Suggested);
        dialogo.set_default_response(Some("renombrar"));
        dialogo.set_close_response("cancelar");
        let d = dialogo.downgrade();
        campo.connect_changed(move |e| {
            let texto = e.text();
            let problema = if texto.trim().is_empty() {
                Some("El nombre no puede estar vacío")
            } else if otros.contains(&texto.trim().to_lowercase()) {
                Some("Ya existe una colección con ese nombre")
            } else {
                None
            };
            aviso.set_label(problema.unwrap_or(""));
            aviso.set_visible(problema.is_some());
            if let Some(d) = d.upgrade() {
                d.set_response_enabled("renombrar", problema.is_none());
            }
            match problema {
                Some(p) => e.update_property(&[gtk::accessible::Property::Description(p)]),
                None => e.reset_property(gtk::AccessibleProperty::Description),
            }
        });
        let this = self.clone();
        let c = campo.downgrade();
        dialogo.connect_response(None, move |_, respuesta| {
            let Some(nuevo) = c.upgrade().map(|c| c.text().trim().to_owned()) else {
                return;
            };
            if respuesta != "renombrar" || nuevo == viejo {
                return;
            }
            let viejo = viejo.clone();
            let mut error = None;
            if let Some(s) = this.0.listas.borrow().as_ref() {
                let mut prueba = s.listas.clone();
                if let Some(k) = prueba.buscar_coleccion(&viejo) {
                    error = prueba.renombrar_coleccion(k, &nuevo).err();
                }
            }
            if let Some(e) = error {
                return this.toast(&e.to_string());
            }
            let n2 = nuevo.clone();
            this.aplicar_listas(move |l| match l.buscar_coleccion(&viejo) {
                Some(k) => l.renombrar_coleccion(k, &n2).unwrap_or(false),
                None => false,
            });
            this.toast(&format!("La colección ahora se llama «{nuevo}»"));
        });
        let f = campo.clone();
        glib::idle_add_local_once(move || {
            f.grab_focus();
            f.select_region(0, -1);
        });
        dialogo.present(Some(&self.0.window));
        Some((dialogo, campo))
    }

    /// «Borrar colección…»: confirmación con respuesta destructiva; las notas no se tocan.
    pub(super) fn pedir_borrar_coleccion(&self, n: usize) -> Option<adw::AlertDialog> {
        let (nombre, notas) = {
            let listas = self.0.listas.borrow();
            let c = listas.as_ref()?.listas.colecciones.get(n)?;
            (c.nombre.clone(), c.notas.len())
        };
        let dialogo = adw::AlertDialog::new(
            Some(&format!("¿Borrar la colección «{nombre}»?")),
            Some(&match notas {
                0 => "La colección está vacía. Las notas no se tocan.".to_owned(),
                1 => "Tiene 1 nota. Las notas no se tocan.".to_owned(),
                k => format!("Tiene {k} notas. Las notas no se tocan."),
            }),
        );
        dialogo.add_response("cancelar", "Cancelar");
        dialogo.add_response("borrar", "Borrar");
        dialogo.set_response_appearance("borrar", adw::ResponseAppearance::Destructive);
        dialogo.set_close_response("cancelar");
        let this = self.clone();
        dialogo.connect_response(None, move |_, respuesta| {
            if respuesta != "borrar" {
                return;
            }
            let nombre = nombre.clone();
            this.toast(&format!("Se borró la colección «{nombre}»"));
            this.aplicar_listas(move |l| match l.buscar_coleccion(&nombre) {
                Some(k) => l.borrar_coleccion(k),
                None => false,
            });
        });
        dialogo.present(Some(&self.0.window));
        Some(dialogo)
    }

    /// Un archivo o una carpeta se renombró desde Orbital: las entradas lo siguen (T023).
    pub(super) fn seguir_renombre_en_listas(&self, desde: &RelPath, hasta: &RelPath) {
        let (desde, hasta) = (desde.clone(), hasta.clone());
        self.aplicar_listas(move |l| l.renombrar_ruta(&desde, &hasta));
    }

    /// Un archivo o una carpeta se borró desde Orbital: sale de todas las listas. Devuelve lo
    /// quitado (con sus posiciones) para «Deshacer»; si la acción se repite sobre lo releído
    /// tras un cambio externo, guarda lo de la última vez.
    pub(super) fn quitar_de_listas(
        &self,
        ruta: &RelPath,
    ) -> Rc<RefCell<orbital_vault::listas::Quitadas>> {
        let quitadas: Rc<RefCell<orbital_vault::listas::Quitadas>> = Rc::default();
        let (ruta, q) = (ruta.clone(), quitadas.clone());
        self.aplicar_listas(move |l| {
            let quitado = l.quitar_ruta(&ruta);
            let cambio = !quitado.is_empty();
            *q.borrow_mut() = quitado;
            cambio
        });
        quitadas
    }

    /// «Deshacer» un borrado: las entradas vuelven a su lugar (decisión 2 del plan).
    pub(super) fn reponer_en_listas(
        &self,
        quitadas: &Rc<RefCell<orbital_vault::listas::Quitadas>>,
    ) {
        if quitadas.borrow().is_empty() {
            return;
        }
        let quitadas = quitadas.borrow().clone();
        self.aplicar_listas(move |l| l.reponer(&quitadas));
    }

    /// Para las pruebas: la lista pedida tal como está en memoria.
    #[cfg(test)]
    pub(super) fn listas_en_memoria(&self, lista: orbital_vault::Lista) -> Vec<RelPath> {
        self.0
            .listas
            .borrow()
            .as_ref()
            .map(|s| s.listas.lista(lista).to_vec())
            .unwrap_or_default()
    }
}
