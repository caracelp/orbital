//! Íconos personalizados en la ventana (062): carga diferida de `.orbital/iconos.yaml` y
//! `.orbital/iconos/*.svg`, guardado atómico, resolución con metadatos del índice en memoria,
//! actualización al renombrar/borrar y diálogos de selector y reglas.

use std::path::PathBuf;

use orbital_vault::{
    AsignacionIcono, IconoResuelto, IconosConfig, IconosSnapshot, MetaNotaIcono, ReglaIcono,
    RelPath, SvgLocal,
};

use super::*;

impl Window {
    /// Programa la lectura diferida de `.orbital/iconos.yaml` después del primer cuadro (no frena
    /// el arranque).
    pub(super) fn iconos_en_show_vault(&self) {
        let sesion = self.0.vault_session.get();
        self.0.iconos_sesion.set(sesion);
        *self.0.iconos.borrow_mut() = None;
        crate::iconos_personalizados::limpiar_cache();
        let this = self.clone();
        self.after_next_frame(move || {
            if this.0.vault_session.get() == sesion {
                this.cargar_iconos();
            }
        });
    }

    /// Inicia la lectura en segundo plano de `.orbital/iconos.yaml` y `.orbital/iconos/*.svg`.
    pub(super) fn cargar_iconos(&self) {
        let i = &self.0;
        let sesion = i.vault_session.get();
        i.iconos_sesion.set(sesion);
        let Some(vault) = i.vault.borrow().clone() else {
            return;
        };
        let this = self.clone();
        glib::spawn_future_local(async move {
            let snap = gio::spawn_blocking(move || vault.load_iconos()).await.ok();
            let Some(snap) = snap else { return };
            if this.0.vault_session.get() != sesion {
                return;
            }
            if let Some(w) = &snap.warning {
                this.toast(w);
            }
            let tiene_algo = !snap.config.asignaciones.is_empty() || !snap.config.reglas.is_empty();
            *this.0.iconos.borrow_mut() = Some(snap);
            this.sincronizar_iconos_editor();
            if tiene_algo {
                this.refrescar_iconos_ui();
            }
        });
    }

    /// Metadatos livianos de una nota sacados del índice en memoria (cero E/S, no bloquea GTK).
    fn meta_icono_de(&self, ruta: &str) -> Option<MetaNotaIcono> {
        let arc = self.0.search_index.borrow().clone()?;
        let guard = arc.try_read().ok()?;
        let rel = RelPath::new(ruta).ok()?;
        guard.meta_icono(&rel)
    }

    /// Resuelve el ícono personalizado de `ruta` (frontmatter → explícito → herencia → reglas).
    pub(super) fn resolver_icono(&self, ruta: &str) -> Option<(IconoResuelto, Option<PathBuf>)> {
        let snap_ref = self.0.iconos.borrow();
        let snap = snap_ref.as_ref();
        let meta = self.meta_icono_de(ruta);
        let existe = |n: &str| {
            snap.is_some_and(|s| s.ruta_svg_local(n).is_some())
                || orbital_vault::iconos_catalogo::buscar(n).is_some()
        };
        let resuelto = match snap {
            Some(s) => s.config.resolver(ruta, meta.as_ref(), existe)?,
            None => IconosConfig::default().resolver(ruta, meta.as_ref(), existe)?,
        };
        let svg = snap.and_then(|s| s.ruta_svg_local(&resuelto.icono).map(PathBuf::from));
        Some((resuelto, svg))
    }

    /// Pintable del ícono personalizado de `ruta` (para el explorador, Favoritos y selector rápido).
    pub(super) fn paintable_icono_de(&self, ruta: &str, lado: i32) -> Option<gtk::gdk::Paintable> {
        let (res, svg) = self.resolver_icono(ruta)?;
        crate::iconos_personalizados::paintable(
            &res.icono,
            res.color.as_deref(),
            lado,
            svg.as_deref(),
        )
    }

    /// `gio::Icon` del ícono personalizado de `ruta` (para `adw::TabPage`).
    pub(super) fn gicon_icono_de(&self, ruta: &str) -> Option<gio::Icon> {
        let (res, svg) = self.resolver_icono(ruta)?;
        crate::iconos_personalizados::gicon(&res.icono, res.color.as_deref(), 16, svg.as_deref())
    }

    /// Pinta el ícono de la pestaña correspondiente a `ruta`, si está abierta.
    pub(super) fn aplicar_icono_pestana(&self, ruta: &str) {
        let icon = self.gicon_icono_de(ruta);
        self.0.pestanas.set_icono(ruta, icon.as_ref());
    }

    /// Actualiza los SVGs locales conocidos por el editor (autocompletado `:...` y Lectura).
    pub(super) fn sincronizar_iconos_editor(&self) {
        let svgs = self
            .0
            .iconos
            .borrow()
            .as_ref()
            .map(|s| s.svgs_locales.clone())
            .unwrap_or_default();
        self.0.code_editor.set_iconos_locales(svgs);
    }

    /// Refresca todos los lugares de la interfaz que muestran íconos personalizados.
    pub(super) fn refrescar_iconos_ui(&self) {
        // 1. Explorador: re-enlaza el árbol preservando carpetas expandidas y selección.
        let sel_actual = self.selected_explorer_path();
        let tree = self.0.tree.borrow().clone();
        if !tree.is_empty() {
            self.set_list_model(tree);
            if let Some(rel) = sel_actual
                && let Some(sel) = self
                    .0
                    .list_view
                    .model()
                    .and_downcast::<gtk::SingleSelection>()
                && let Some(pos) = (0..sel.n_items()).find(|&n| {
                    sel.item(n)
                        .and_downcast::<gtk::TreeListRow>()
                        .and_then(|r| r.item().and_downcast::<gtk::StringObject>())
                        .is_some_and(|o| o.string() == rel)
                })
            {
                sel.set_selected(pos);
            }
        }
        // 2. Pestañas abiertas.
        for clave in self.0.pestanas.claves_abiertas() {
            let icon = self.gicon_icono_de(&clave);
            self.0.pestanas.set_icono(&clave, icon.as_ref());
        }
        // 3. Favoritos y Guardados.
        self.0.listas_panel.forzar_refresco_iconos();
        self.redibujar_listas();
    }

    /// Aplica una mutación a `IconosConfig`, actualiza la interfaz y guarda en disco.
    fn mutar_iconos(&self, f: impl FnOnce(&mut IconosConfig)) {
        let Some(vault) = self.0.vault.borrow().clone() else {
            return;
        };
        {
            let mut slot = self.0.iconos.borrow_mut();
            if slot.is_none() {
                *slot = Some(vault.load_iconos());
            }
            if let Some(snap) = slot.as_mut() {
                f(&mut snap.config);
            }
        }
        self.refrescar_iconos_ui();
        self.guardar_iconos();
    }

    fn guardar_iconos(&self) {
        let i = &self.0;
        if i.iconos_escribiendo.get() {
            i.iconos_pendiente.set(true);
            return;
        }
        let (Some(vault), Some(snap)) = (i.vault.borrow().clone(), i.iconos.borrow().clone())
        else {
            return;
        };
        if !snap.writable() {
            return;
        }
        let sesion = i.vault_session.get();
        let cfg = snap.config.clone();
        i.iconos_escribiendo.set(true);
        let this = self.clone();
        glib::spawn_future_local(async move {
            let res = gio::spawn_blocking(move || {
                let ctx = write_ctx::now(Some(vault.config()));
                vault.save_iconos(&cfg, &snap, &ctx)
            })
            .await;
            this.0.iconos_escribiendo.set(false);
            if this.0.vault_session.get() != sesion {
                return;
            }
            match res {
                Ok(Ok(nuevo)) => {
                    // Preserva mutaciones que entraron en memoria mientras se guardaba.
                    let cfg_actual = this.0.iconos.borrow().as_ref().map(|s| s.config.clone());
                    let mut final_snap = nuevo;
                    if let Some(c) = cfg_actual
                        && this.0.iconos_pendiente.get()
                    {
                        final_snap.config = c;
                    }
                    *this.0.iconos.borrow_mut() = Some(final_snap);
                }
                Ok(Err(orbital_vault::VaultError::Conflict { .. })) => {
                    // Si cambió en disco afuera, recargamos y reintentamos si hay pendiente.
                    let vault_opt = this.0.vault.borrow().clone();
                    if let Some(v) = vault_opt {
                        let disco: Option<IconosSnapshot> =
                            gio::spawn_blocking(move || v.load_iconos()).await.ok();
                        if let Some(d) = disco {
                            *this.0.iconos.borrow_mut() = Some(d);
                        }
                    }
                }
                Ok(Err(e)) => this.toast(&format!("No se pudo guardar los íconos: {e}")),
                Err(_) => this.toast("No se pudo guardar los íconos"),
            }
            if this.0.iconos_pendiente.replace(false) {
                this.guardar_iconos();
            }
        });
    }

    fn registrar_svg_importado(&self, svg: SvgLocal) {
        if let Some(snap) = self.0.iconos.borrow_mut().as_mut()
            && !snap.svgs_locales.iter().any(|s| s.nombre == svg.nombre)
        {
            snap.svgs_locales.push(svg);
            snap.svgs_locales.sort_by(|a, b| a.nombre.cmp(&b.nombre));
        }
        self.sincronizar_iconos_editor();
    }

    /// Abre el diálogo selector de ícono para una ruta del explorador o la nota abierta.
    pub(super) fn abrir_selector_icono(&self, rel: &str) {
        let Some(vault) = self.0.vault.borrow().clone() else {
            return;
        };
        let Ok(ruta) = RelPath::new(rel) else {
            return;
        };
        let es_carpeta = self.entry(rel).is_some_and(|e| e.kind == EntryKind::Folder);
        let (actual, svgs_locales) = {
            let slot = self.0.iconos.borrow();
            match slot.as_ref() {
                Some(s) => (s.config.asignacion_de(rel).cloned(), s.svgs_locales.clone()),
                None => (None, Vec::new()),
            }
        };
        let nombre = ruta.file_name().to_owned();
        let opts = crate::iconos_personalizados::OpcionesSelector {
            titulo: format!("Ícono para «{nombre}»"),
            icono_actual: actual.as_ref().map(|a| a.icono.clone()),
            color_actual: actual.as_ref().and_then(|a| a.color.clone()),
            heredar_actual: actual.as_ref().is_some_and(|a| a.heredar),
            es_carpeta,
            mostrar_quitar: actual.is_some(),
            svgs_locales,
            raiz_boveda: vault.root().to_path_buf(),
        };
        let this = self.clone();
        let this_imp = self.clone();
        crate::iconos_personalizados::mostrar_selector(
            &self.0.window,
            opts,
            move |acc| match acc {
                crate::iconos_personalizados::AccionSelector::Aplicar {
                    icono,
                    color,
                    heredar,
                } => {
                    let r = ruta.clone();
                    this.mutar_iconos(move |cfg| {
                        cfg.asignar(r, icono, color, es_carpeta && heredar);
                    });
                }
                crate::iconos_personalizados::AccionSelector::Quitar => {
                    let s = ruta.as_str().to_owned();
                    this.mutar_iconos(move |cfg| {
                        cfg.quitar(&s);
                    });
                }
            },
            move |svg| {
                this_imp.registrar_svg_importado(svg);
            },
        );
    }

    /// Quita la asignación explícita de ícono de `rel`.
    pub(super) fn quitar_icono_de(&self, rel: &str) {
        let s = rel.to_owned();
        self.mutar_iconos(move |cfg| {
            cfg.quitar(&s);
        });
    }

    /// Alterna «Heredar a hijos» en una carpeta ya asignada.
    pub(super) fn alternar_heredar_icono_de(&self, rel: &str) {
        let s = rel.to_owned();
        self.mutar_iconos(move |cfg| {
            cfg.alternar_heredar(&s);
        });
    }

    /// Devuelve la asignación explícita actual de `rel`, si existe.
    pub(super) fn asignacion_icono_de(&self, rel: &str) -> Option<AsignacionIcono> {
        self.0
            .iconos
            .borrow()
            .as_ref()
            .and_then(|s| s.config.asignacion_de(rel).cloned())
    }

    /// Abre el gestor de reglas automáticas de íconos.
    pub(super) fn abrir_reglas_iconos(&self) {
        let Some(vault) = self.0.vault.borrow().clone() else {
            return;
        };
        let (reglas, svgs_locales) = {
            let slot = self.0.iconos.borrow();
            match slot.as_ref() {
                Some(s) => (s.config.reglas.clone(), s.svgs_locales.clone()),
                None => (Vec::new(), Vec::new()),
            }
        };
        let this = self.clone();
        let this_imp = self.clone();
        crate::iconos_personalizados::mostrar_reglas(
            &self.0.window,
            reglas,
            svgs_locales,
            vault.root().to_path_buf(),
            move |nuevas: Vec<ReglaIcono>| {
                this.mutar_iconos(move |cfg| {
                    cfg.reglas = nuevas;
                });
            },
            move |svg| {
                this_imp.registrar_svg_importado(svg);
            },
        );
    }

    /// Sigue el renombrado o movimiento de un archivo o carpeta en `.orbital/iconos.yaml`.
    pub(super) fn seguir_renombre_en_iconos(&self, from: &RelPath, to: &RelPath) {
        let cambio = self
            .0
            .iconos
            .borrow_mut()
            .as_mut()
            .is_some_and(|s| s.config.renombrar_ruta(from, to));
        if cambio {
            self.refrescar_iconos_ui();
            self.guardar_iconos();
        }
    }

    /// Quita las asignaciones de una ruta borrada y las devuelve para «Deshacer».
    pub(super) fn quitar_de_iconos(&self, ruta: &RelPath) -> Vec<AsignacionIcono> {
        let quitadas = self
            .0
            .iconos
            .borrow_mut()
            .as_mut()
            .map(|s| s.config.borrar_ruta(ruta))
            .unwrap_or_default();
        if !quitadas.is_empty() {
            self.refrescar_iconos_ui();
            self.guardar_iconos();
        }
        quitadas
    }

    /// Repone asignaciones quitadas al deshacer un borrado.
    pub(super) fn reponer_iconos(&self, quitadas: Vec<AsignacionIcono>) {
        if quitadas.is_empty() {
            return;
        }
        self.mutar_iconos(move |cfg| {
            for a in quitadas {
                cfg.asignar(a.ruta, a.icono, a.color, a.heredar);
            }
        });
    }
}
