//! Outliner plegable (028): el estado de `.orbital/plegado.yaml` (lectura diferida, cola de
//! escritura, conflicto), las acciones `win.alternar-plegado`, `win.plegar-todo` y
//! `win.desplegar-todo`, y los enganches de renombrar y borrar notas.
//!
//! El archivo se lee en otro hilo al abrir la primera nota `.md` de la bóveda (no en el arranque,
//! FR-019). Cada cambio se aplica en memoria y se escribe en segundo plano, de a una escritura por
//! vez y con el último estado (como las listas de la 027). No hay vigilante: ante un `Conflict`
//! se relee, se repiten los cambios pendientes y se reintenta una vez (research R10).

use super::*;
use orbital_vault::{Clave, Plegado, PlegadoSnapshot};

/// Un cambio del estado de plegado: devuelve si cambió algo. Se puede repetir sobre otro estado.
pub(super) type AccionPlegado = Rc<dyn Fn(&mut Plegado) -> bool>;

/// Aviso para las notas `.html` (FR-017).
const SOLO_MARKDOWN: &str = "El plegado es para notas Markdown";

impl Window {
    /// Conecta el editor y las acciones (una vez, desde `conexiones`).
    pub(super) fn setup_plegado(&self) {
        let weak = Rc::downgrade(&self.0);
        self.0.code_editor.set_on_plegado(move |ruta, claves| {
            if let Some(i) = weak.upgrade() {
                let ruta = ruta.clone();
                Window(i).aplicar_plegado(move |p| p.poner(&ruta, claves.clone()));
            }
        });
        let weak = Rc::downgrade(&self.0);
        self.0.code_editor.set_on_enfoque(move || {
            if let Some(i) = weak.upgrade() {
                Window(i).habilitar_acciones_plegado();
            }
        });
        let accion = |nombre: &str, f: fn(&Editor)| {
            let this = self.clone();
            gio::ActionEntry::builder(nombre)
                .activate(move |_: &adw::ApplicationWindow, _, _| {
                    let editor = &this.0.code_editor;
                    if editor.format() == Format::Html {
                        this.toast(SOLO_MARKDOWN);
                    } else if editor.plegado_disponible() {
                        f(editor);
                    }
                })
                .build()
        };
        self.0.window.add_action_entries([
            accion("alternar-plegado", |e| {
                if e.current_view() == crate::editor::View::Lectura {
                    e.alternar_plegado_en_lectura();
                } else {
                    e.alternar_plegado_en_cursor();
                }
            }),
            accion("plegar-todo", Editor::plegar_todo),
            accion("desplegar-todo", Editor::desplegar_todo),
            accion("enfocar", |e| {
                if e.current_view() == crate::editor::View::Lectura {
                    e.enfocar_en_lectura();
                } else {
                    e.enfocar_en_cursor();
                }
            }),
            accion("salir-enfoque", |e| {
                e.salir_enfoque();
            }),
        ]);
        self.habilitar_acciones_plegado();
    }

    /// Las acciones valen con una nota abierta en el editor (con una `.html`, avisan).
    pub(super) fn habilitar_acciones_plegado(&self) {
        let i = &self.0;
        let abierta = i.code_editor.current_path().is_some()
            && i.code_editor.current_path().as_ref()
                == i.current_entry.borrow().as_ref().map(|e| &e.path)
            && !i.gone.get();
        let enfocado = abierta && i.code_editor.esta_enfocado();
        for (name, enabled) in [
            ("alternar-plegado", abierta),
            ("plegar-todo", abierta),
            ("desplegar-todo", abierta),
            ("enfocar", abierta),
            ("salir-enfoque", enfocado),
        ] {
            if let Some(a) = i
                .window
                .lookup_action(name)
                .and_downcast::<gio::SimpleAction>()
            {
                a.set_enabled(enabled);
            }
        }
    }

    /// Opciones del menú «Más opciones de la nota» para plegado y enfoque (T036).
    pub(super) fn items_menu_plegado(&self) -> Vec<MenuItem> {
        let mut items = vec![
            MenuItem::new(
                "Plegar o desplegar el elemento",
                "win.alternar-plegado",
                "Ctrl+Arriba",
            ),
            MenuItem::new("Plegar todo", "win.plegar-todo", ""),
            MenuItem::new("Desplegar todo", "win.desplegar-todo", ""),
            MenuItem::new("Enfocar", "win.enfocar", "Alt+Derecha"),
        ];
        if self.0.code_editor.esta_enfocado() {
            items.push(MenuItem::new("Salir del enfoque", "win.salir-enfoque", ""));
        }
        items
    }

    /// Desde `show_vault`: una bóveda nueva olvida el estado (se lee al abrir la primera nota).
    pub(super) fn plegado_en_show_vault(&self) {
        let i = &self.0;
        let sesion = i.vault_session.get();
        if i.plegado_sesion.get() == sesion {
            return;
        }
        i.plegado_sesion.set(sesion);
        *i.plegado.borrow_mut() = None;
        i.plegado_pedido.set(false);
        i.plegado_acciones.borrow_mut().clear();
        i.plegado_escribiendo.set(false);
        i.plegado_pendiente.set(false);
    }

    /// Al abrir una nota `.md` en el editor: sus pliegues guardados, o pedir la lectura del
    /// archivo si todavía no se hizo (llegan después y se aplican si no se plegó nada).
    pub(super) fn plegado_al_abrir(&self, ruta: &RelPath) {
        if !self.0.code_editor.plegado_disponible() {
            return;
        }
        let claves = self
            .0
            .plegado
            .borrow()
            .as_ref()
            .map(|s| s.plegado.de(ruta).to_vec());
        match claves {
            Some(c) if !c.is_empty() => self.0.code_editor.aplicar_claves(&c),
            Some(_) => {}
            None => self.pedir_plegado(),
        }
    }

    /// Lee `.orbital/plegado.yaml` en otro hilo, una vez por bóveda.
    fn pedir_plegado(&self) {
        let i = &self.0;
        if i.plegado_pedido.replace(true) {
            return;
        }
        let Some(vault) = i.vault.borrow().clone() else {
            return;
        };
        let sesion = i.vault_session.get();
        let this = self.clone();
        glib::spawn_future_local(async move {
            let Ok(snap) = gio::spawn_blocking(move || vault.plegado_snapshot()).await else {
                return;
            };
            if this.0.vault_session.get() != sesion {
                return;
            }
            if let Some(aviso) = &snap.warning {
                this.toast(aviso);
            }
            this.plegado_cargado(snap);
        });
    }

    /// Pone lo leído, repite encima los cambios que llegaron antes y lo aplica a la nota abierta
    /// si la usuaria no plegó nada mientras tanto.
    fn plegado_cargado(&self, mut snap: PlegadoSnapshot) {
        let pendientes = !self.0.plegado_acciones.borrow().is_empty();
        for a in self.0.plegado_acciones.borrow().iter() {
            a(&mut snap.plegado);
        }
        *self.0.plegado.borrow_mut() = Some(snap);
        self.aplicar_plegado_a_la_nota_abierta();
        if pendientes {
            self.escribir_plegado();
        }
    }

    /// Los pliegues guardados de la nota abierta, si el editor no tiene ninguno.
    pub(super) fn aplicar_plegado_a_la_nota_abierta(&self) {
        let editor = &self.0.code_editor;
        let Some(ruta) = editor.current_path() else {
            return;
        };
        if editor.tiene_pliegues() || !editor.plegado_disponible() {
            return;
        }
        let claves: Vec<Clave> = match self.0.plegado.borrow().as_ref() {
            Some(s) => s.plegado.de(&ruta).to_vec(),
            None => return,
        };
        if !claves.is_empty() {
            editor.aplicar_claves(&claves);
        }
    }

    /// El estado ya se leyó (para las pruebas).
    #[cfg(test)]
    pub(super) fn plegado_cargado_ya(&self) -> bool {
        self.0.plegado.borrow().is_some()
    }

    /// Hay una escritura en curso o pendiente (para las pruebas).
    #[cfg(test)]
    pub(super) fn plegado_escribiendo(&self) -> bool {
        self.0.plegado_escribiendo.get() || !self.0.plegado_acciones.borrow().is_empty()
    }

    /// Para las pruebas: las claves guardadas en memoria de una nota.
    #[cfg(test)]
    pub(super) fn plegado_de(&self, ruta: &RelPath) -> Vec<Clave> {
        self.0
            .plegado
            .borrow()
            .as_ref()
            .map(|s| s.plegado.de(ruta).to_vec())
            .unwrap_or_default()
    }

    /// Aplica un cambio en memoria y encola la escritura. Sin el estado leído, el cambio espera la
    /// lectura (que se pide).
    pub(super) fn aplicar_plegado(&self, accion: impl Fn(&mut Plegado) -> bool + 'static) {
        let accion: AccionPlegado = Rc::new(accion);
        let cambio = match self.0.plegado.borrow_mut().as_mut() {
            Some(s) => accion(&mut s.plegado),
            None => true,
        };
        if !cambio {
            return;
        }
        self.0.plegado_acciones.borrow_mut().push(accion);
        if self.0.plegado.borrow().is_some() {
            self.escribir_plegado();
        } else {
            self.pedir_plegado();
        }
    }

    /// Escribe el estado en otro hilo, una escritura por vez. Antes descarta las notas que ya no
    /// existen en la bóveda.
    fn escribir_plegado(&self) {
        let i = &self.0;
        if i.plegado_escribiendo.get() {
            i.plegado_pendiente.set(true);
            return;
        }
        let (Some(vault), Some(snap)) = (i.vault.borrow().clone(), i.plegado.borrow().clone())
        else {
            return;
        };
        if i.gone.get() || !snap.writable() {
            return;
        }
        i.plegado_escribiendo.set(true);
        i.plegado_pendiente.set(false);
        let enviadas = i.plegado_acciones.borrow().len();
        let sesion = i.vault_session.get();
        let this = self.clone();
        glib::spawn_future_local(async move {
            let ctx = write_ctx::now(Some(vault.config()));
            let v = vault.clone();
            let resultado = gio::spawn_blocking(move || {
                let mut p = snap.plegado.clone();
                p.podar(v.entries());
                match v.guardar_plegado(&p, &snap, &ctx) {
                    Err(orbital_vault::VaultError::Conflict { .. }) => {
                        Err(Some(v.plegado_snapshot()))
                    }
                    Err(e) => {
                        eprintln!("No se guardó el plegado: {e}");
                        Err(None)
                    }
                    Ok(nuevo) => Ok(nuevo),
                }
            })
            .await;
            if this.0.vault_session.get() != sesion {
                return;
            }
            this.0.plegado_escribiendo.set(false);
            match resultado {
                Ok(Ok(nuevo)) => {
                    this.0.plegado_acciones.borrow_mut().drain(..enviadas);
                    if let Some(s) = this.0.plegado.borrow_mut().as_mut() {
                        // Lo de memoria puede ser más nuevo que lo escrito: se conserva.
                        let actual = std::mem::take(&mut s.plegado);
                        *s = nuevo;
                        s.plegado = actual;
                    }
                    if this.0.plegado_pendiente.get() {
                        this.escribir_plegado();
                    }
                }
                Ok(Err(Some(releido))) => this.reintentar_plegado(&vault, releido, sesion).await,
                _ => this.toast("No se pudo guardar lo plegado de las notas"),
            }
        });
    }

    /// El archivo cambió afuera: repite sobre lo releído los cambios que no llegaron al disco y
    /// reintenta una vez; si falla otra vez, queda lo que hay en disco.
    async fn reintentar_plegado(
        &self,
        vault: &Arc<Vault>,
        mut releido: PlegadoSnapshot,
        sesion: u64,
    ) {
        let disco = releido.clone();
        for a in self.0.plegado_acciones.borrow().iter() {
            a(&mut releido.plegado);
        }
        let i = &self.0;
        i.plegado_escribiendo.set(true);
        i.plegado_pendiente.set(false);
        let enviadas = i.plegado_acciones.borrow().len();
        let ctx = write_ctx::now(Some(vault.config()));
        let v = vault.clone();
        let base = releido.clone();
        let resultado = gio::spawn_blocking(move || {
            let mut p = base.plegado.clone();
            p.podar(v.entries());
            v.guardar_plegado(&p, &base, &ctx)
        })
        .await;
        if i.vault_session.get() != sesion {
            return;
        }
        i.plegado_escribiendo.set(false);
        match resultado {
            Ok(Ok(mut nuevo)) => {
                i.plegado_acciones.borrow_mut().drain(..enviadas);
                nuevo.plegado = releido.plegado;
                *i.plegado.borrow_mut() = Some(nuevo);
                if i.plegado_pendiente.get() {
                    self.escribir_plegado();
                }
            }
            _ => {
                i.plegado_acciones.borrow_mut().clear();
                *i.plegado.borrow_mut() = Some(disco);
                self.toast("Lo plegado cambió en el disco: se usa lo que hay ahí");
            }
        }
    }

    /// Una nota o una carpeta se renombró o se movió desde Orbital (FR-010); también en el
    /// deshacer, que llama a `do_rename` al revés. Si la nota renombrada está abierta, se pliega.
    pub(super) fn seguir_renombre_en_plegado(&self, desde: &RelPath, hasta: &RelPath) {
        let (desde, hasta) = (desde.clone(), hasta.clone());
        self.aplicar_plegado(move |p| p.renombrar_ruta(&desde, &hasta));
        self.aplicar_plegado_a_la_nota_abierta();
    }

    /// Una nota o una carpeta se borró desde Orbital: su estado se descarta. Devuelve lo quitado
    /// para «Deshacer» (lo de la última vez si el cambio se repite tras un conflicto).
    pub(super) fn quitar_de_plegado(
        &self,
        ruta: &RelPath,
    ) -> Rc<RefCell<orbital_vault::plegado::Quitadas>> {
        let quitadas: Rc<RefCell<orbital_vault::plegado::Quitadas>> = Rc::default();
        let (ruta, q) = (ruta.clone(), quitadas.clone());
        self.aplicar_plegado(move |p| {
            let quitado = p.quitar_ruta(&ruta);
            let cambio = !quitado.is_empty();
            *q.borrow_mut() = quitado;
            cambio
        });
        quitadas
    }

    /// Deshacer el borrado: el estado vuelve (y se pliega si la nota quedó abierta).
    pub(super) fn reponer_en_plegado(
        &self,
        quitadas: &Rc<RefCell<orbital_vault::plegado::Quitadas>>,
    ) {
        if quitadas.borrow().is_empty() {
            return;
        }
        let quitadas = quitadas.borrow().clone();
        self.aplicar_plegado(move |p| p.reponer(&quitadas));
        self.aplicar_plegado_a_la_nota_abierta();
    }
}
