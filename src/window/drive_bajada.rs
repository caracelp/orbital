//! Bajar lo «solo en Drive» para abrirlo o para «Abrir con otra aplicación» (061b): el progreso
//! real (porcentaje, bytes y el tiempo que falta si se puede estimar), «Cancelar» y los anuncios
//! al lector de pantalla cada 25 %. Para abrir se muestra en el centro; para «Abrir con», en un
//! diálogo (la nota abierta sigue a la vista). La red y el disco están en el hilo del motor: acá
//! solo se reciben sus eventos.
//!
//! Lo que se abre con otra aplicación se vigila: si esa aplicación guarda la copia de la caché,
//! se le avisa al motor, que la sube (el vigilante de la bóveda no mira la caché).

use super::*;

use crate::drive_progreso::Medidor;
use orbital_drive::Orden;

#[cfg(test)]
thread_local! {
    /// Solo pruebas: lo que se habría abierto con otra aplicación.
    pub(super) static LANZADAS: std::cell::RefCell<Vec<PathBuf>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

/// Para qué se baja.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ParaQue {
    Abrir,
    AbrirCon,
}

/// La bajada que la usuaria espera (una a la vez).
pub(super) struct Bajada {
    pub(super) ruta: String,
    pub(super) pedido: u64,
    pub(super) para: ParaQue,
    medidor: Medidor,
    barra: gtk::ProgressBar,
    detalle: gtk::Label,
    #[cfg_attr(not(test), allow(dead_code))]
    pub(super) cancelar: gtk::Button,
    /// El diálogo de «Abrir con» (para abrir, la página del centro).
    dialogo: Option<adw::Dialog>,
}

#[cfg(test)]
impl Bajada {
    pub(super) fn barra_fraccion(&self) -> f64 {
        self.barra.fraction()
    }

    pub(super) fn barra_texto(&self) -> String {
        self.barra.text().map(|t| t.to_string()).unwrap_or_default()
    }
}

/// La barra, el detalle y «Cancelar».
fn vista(nombre: &str) -> (gtk::Box, gtk::ProgressBar, gtk::Label, gtk::Button) {
    let caja = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(12)
        .halign(gtk::Align::Center)
        .width_request(320)
        .build();
    let barra = gtk::ProgressBar::builder()
        .show_text(true)
        .text("0 %")
        .build();
    barra.update_property(&[gtk::accessible::Property::Label(&format!(
        "Bajada de «{nombre}»"
    ))]);
    let detalle = gtk::Label::builder()
        .label("Conectando con Google Drive…")
        .css_classes(["dim-label", "numeric"])
        .wrap(true)
        .build();
    let cancelar = gtk::Button::builder()
        .label("Cancelar")
        .css_classes(["pill"])
        .halign(gtk::Align::Center)
        .build();
    cancelar.update_property(&[gtk::accessible::Property::Description(&format!(
        "Deja de bajar «{nombre}»; no queda nada a medias en este equipo"
    ))]);
    caja.append(&barra);
    caja.append(&detalle);
    caja.append(&cancelar);
    (caja, barra, detalle, cancelar)
}

impl Window {
    /// La página del centro para bajar y abrir (se crea la primera vez).
    fn drive_pagina_bajada(&self) -> adw::StatusPage {
        let stack = &self.0.content_stack;
        if let Some(p) = stack
            .child_by_name("drive-bajada")
            .and_downcast::<adw::StatusPage>()
        {
            return p;
        }
        let p = adw::StatusPage::builder()
            .icon_name("folder-remote-symbolic")
            .title("Bajando de Drive…")
            .build();
        stack.add_named(&p, Some("drive-bajada"));
        p
    }

    /// Empieza a bajar `rel` («solo en Drive») para abrirlo acá o con otra aplicación.
    pub(super) fn drive_bajar(&self, rel: &str, para: ParaQue) {
        let nombre = rel.rsplit('/').next().unwrap_or(rel).to_owned();
        // Una a la vez: si ya se bajaba otra cosa, se cancela (la usuaria eligió otra).
        let vieja = {
            let mut d = self.0.drive.borrow_mut();
            match d.bajada.as_mut() {
                Some(b) if b.ruta == rel => {
                    // La misma: sigue la bajada en curso, ahora para lo último que se pidió.
                    if b.para == para {
                        return;
                    }
                    if para == ParaQue::Abrir {
                        // Pasa del diálogo a la página: el diálogo se cierra sin cancelar.
                        b.para = para;
                        let dl = b.dialogo.take();
                        let caja = b.barra.parent();
                        drop(d);
                        if let Some(dl) = dl {
                            dl.force_close();
                        }
                        if let Some(c) = caja {
                            c.unparent();
                            self.drive_pagina_bajada().set_child(Some(&c));
                        }
                        self.drive_mostrar_pagina_bajada(&nombre);
                    }
                    return;
                }
                _ => d.bajada.take(),
            }
        };
        if let Some(v) = vieja {
            self.drive_soltar_bajada(v);
        }
        let (caja, barra, detalle, cancelar) = vista(&nombre);
        let voz = barra.clone();
        let pedido = {
            let mut d = self.0.drive.borrow_mut();
            d.siguiente_pedido += 1;
            d.siguiente_pedido
        };
        let this = self.clone();
        cancelar.connect_clicked(move |_| this.drive_cancelar_bajada(pedido));
        let dialogo = (para == ParaQue::AbrirCon).then(|| {
            let cuerpo = gtk::Box::builder()
                .orientation(gtk::Orientation::Vertical)
                .spacing(18)
                .margin_top(12)
                .margin_bottom(24)
                .margin_start(24)
                .margin_end(24)
                .build();
            let texto = gtk::Label::builder()
                .label(format!(
                    "«{nombre}» está solo en Drive: se abre con otra aplicación apenas termine de bajar."
                ))
                .wrap(true)
                .max_width_chars(40)
                .justify(gtk::Justification::Center)
                .build();
            cuerpo.append(&texto);
            cuerpo.append(&caja);
            let tv = adw::ToolbarView::new();
            tv.add_top_bar(&adw::HeaderBar::new());
            tv.set_content(Some(&cuerpo));
            let dl = adw::Dialog::builder()
                .title("Bajando de Drive")
                .content_width(420)
                .child(&tv)
                .build();
            // Cerrar el diálogo (Esc, la cruz) es cancelar (si sigue siendo para «Abrir con»).
            let this = self.clone();
            dl.connect_closed(move |_| {
                let sigue = this
                    .0
                    .drive
                    .borrow()
                    .bajada
                    .as_ref()
                    .is_some_and(|b| b.pedido == pedido && b.para == ParaQue::AbrirCon);
                if sigue {
                    this.drive_cancelar_bajada(pedido);
                }
            });
            dl
        });
        self.0.drive.borrow_mut().bajada = Some(Bajada {
            ruta: rel.to_owned(),
            pedido,
            para,
            medidor: Medidor::new(&nombre),
            barra,
            detalle,
            cancelar: cancelar.clone(),
            dialogo: dialogo.clone(),
        });
        match dialogo {
            Some(dl) => {
                dl.present(Some(&self.0.window));
                voz.announce(
                    &format!("Bajando «{nombre}» de Drive para abrirlo con otra aplicación"),
                    gtk::AccessibleAnnouncementPriority::Medium,
                );
            }
            None => {
                self.drive_pagina_bajada().set_child(Some(&caja));
                self.drive_mostrar_pagina_bajada(&nombre);
            }
        }
        // Si el motor todavía no arrancó (recién abierta la bóveda), lo pide al arrancar.
        self.drive_orden(Orden::Traer(rel.to_owned(), pedido));
    }

    fn drive_mostrar_pagina_bajada(&self, nombre: &str) {
        let p = self.drive_pagina_bajada();
        p.set_title("Bajando de Drive…");
        let texto = format!(
            "«{nombre}» está solo en Drive: se abre apenas termine de bajar. La copia queda en la caché de este equipo, fuera de la bóveda."
        );
        p.set_description(Some(&texto));
        self.0.content_stack.set_visible_child_name("drive-bajada");
        self.0.list_view.announce(
            &format!("Bajando de Drive. {texto}"),
            gtk::AccessibleAnnouncementPriority::Medium,
        );
    }

    /// El pedido que hay que repetir cuando arranca el motor (si se pidió antes).
    pub(super) fn drive_bajada_pendiente(&self) -> Option<(String, u64)> {
        let d = self.0.drive.borrow();
        d.bajada.as_ref().map(|b| (b.ruta.clone(), b.pedido))
    }

    /// Suelta una bajada sin abrir nada (otra la reemplaza, o cambió la bóveda).
    pub(super) fn drive_soltar_bajada(&self, b: Bajada) {
        if let Some(m) = &self.0.drive.borrow().motor {
            m.cancelar(b.pedido);
        }
        if let Some(dl) = b.dialogo {
            dl.force_close();
        }
    }

    /// «Cancelar»: el motor corta la bajada entre tramo y tramo y borra lo bajado.
    fn drive_cancelar_bajada(&self, pedido: u64) {
        let b = {
            let mut d = self.0.drive.borrow_mut();
            if d.bajada.as_ref().is_none_or(|b| b.pedido != pedido) {
                return;
            }
            d.bajada.take().expect("bajada")
        };
        let nombre = b.ruta.rsplit('/').next().unwrap_or(&b.ruta).to_owned();
        let para = b.para;
        self.drive_soltar_bajada(b);
        let texto = format!("«{nombre}» sigue solo en Drive. Elegilo otra vez para bajarlo.");
        match para {
            ParaQue::Abrir => {
                self.drive_pagina("folder-remote-symbolic", "Bajada cancelada", &texto)
            }
            ParaQue::AbrirCon => {
                self.toast(&format!("Se canceló la bajada de «{nombre}»"));
            }
        }
    }

    /// Avance de una bajada (de la que se espera y de los embeds).
    pub(super) fn drive_progreso(&self, ruta: &str, pedido: u64, bajado: u64, total: u64) {
        crate::drive_remoto::progreso(ruta, bajado, total);
        let mut d = self.0.drive.borrow_mut();
        let Some(b) = d.bajada.as_mut().filter(|b| b.pedido == pedido) else {
            return;
        };
        let l = b.medidor.avanzar(std::time::Instant::now(), bajado, total);
        b.barra.set_fraction(l.fraccion);
        b.barra.set_text(Some(&l.porcentaje));
        b.detalle.set_label(&l.detalle);
        if let Some(a) = l.anuncio {
            b.barra
                .announce(&a, gtk::AccessibleAnnouncementPriority::Low);
        }
    }

    /// Llegó la respuesta de una bajada que se esperaba (`true`) o no era esta (`false`).
    pub(super) fn drive_bajada_terminada(
        &self,
        rel: &str,
        pedido: u64,
        res: &Result<PathBuf, String>,
    ) -> bool {
        let b = {
            let mut d = self.0.drive.borrow_mut();
            if d.bajada.as_ref().is_none_or(|b| b.pedido != pedido) {
                return false;
            }
            d.bajada.take().expect("bajada")
        };
        let para = b.para;
        if let Some(dl) = b.dialogo {
            dl.force_close();
        }
        match (para, res) {
            (ParaQue::Abrir, Ok(_)) => {
                if let Some(e) = self.entry(rel) {
                    self.show_entry(e);
                }
            }
            (ParaQue::Abrir, Err(m)) => self.drive_pagina(
                "dialog-warning-symbolic",
                "No se pudo bajar de Drive",
                &format!("«{rel}»: {m}"),
            ),
            (ParaQue::AbrirCon, Ok(copia)) => self.drive_lanzar_copia(copia),
            (ParaQue::AbrirCon, Err(m)) => {
                self.toast(&format!("No se pudo bajar «{rel}» de Drive: {m}"))
            }
        }
        true
    }

    /// «Abrir con otra aplicación» de cualquier archivo de la bóveda: lo «solo en Drive» se baja
    /// antes (con el mismo progreso y «Cancelar»); sin conexión se explica por qué no.
    pub(super) fn abrir_con_otra(&self, rel: &str) {
        let Some(root) = self
            .0
            .vault
            .borrow()
            .as_ref()
            .map(|v| v.root().to_path_buf())
        else {
            return;
        };
        if !self.drive_es_marcador(rel) {
            let p = RelPath::new(rel).map(|r| r.to_path(&root));
            if let Ok(p) = p {
                self.launch_file(&p);
            }
            return;
        }
        let fresca = self.0.drive.borrow().frescos.contains(rel);
        let copia = orbital_vault::ubicacion::resolver(&root, rel);
        if fresca && let Some(c) = copia.filter(|c| c.is_file()) {
            return self.drive_lanzar_copia(&c);
        }
        let arrancando = {
            let d = self.0.drive.borrow();
            d.motor.is_none() && d.detenido.is_none()
        };
        if !arrancando && !self.drive_en_linea() {
            let nombre = rel.rsplit('/').next().unwrap_or(rel);
            return self.toast(&format!(
                "«{nombre}» está solo en Drive y no hay conexión: se va a poder abrir cuando vuelva la red"
            ));
        }
        self.drive_bajar(rel, ParaQue::AbrirCon);
    }

    /// Abre la copia de la caché con la aplicación del sistema y la vigila: si esa aplicación la
    /// guarda, el motor sube el cambio.
    fn drive_lanzar_copia(&self, copia: &Path) {
        // En las pruebas no se abre otra aplicación de verdad: se anota.
        #[cfg(test)]
        LANZADAS.with(|l| l.borrow_mut().push(copia.to_path_buf()));
        #[cfg(not(test))]
        self.launch_file(copia);
        let Some(dir) = copia.parent() else {
            return;
        };
        if self.0.drive.borrow().vigiladas.contains_key(dir) {
            return;
        }
        // La carpeta de la copia, no el archivo: muchas aplicaciones guardan escribiendo otro
        // archivo y renombrándolo encima.
        let Ok(monitor) = gio::File::for_path(dir)
            .monitor_directory(gio::FileMonitorFlags::WATCH_MOVES, gio::Cancellable::NONE)
        else {
            return;
        };
        let weak = Rc::downgrade(&self.0);
        let copia = copia.to_path_buf();
        monitor.connect_changed(move |_, f, otro, ev| {
            use gio::FileMonitorEvent as E;
            let tocado = match ev {
                E::ChangesDoneHint | E::Created | E::MovedIn => f.path(),
                E::Renamed => otro.and_then(|o| o.path()),
                _ => None,
            };
            if tocado.as_deref() != Some(copia.as_path()) {
                return;
            }
            let (Some(i), Some((_, rel))) =
                (weak.upgrade(), orbital_vault::ubicacion::origen(&copia))
            else {
                return;
            };
            Window(i).drive_orden(Orden::Cambios(vec![rel]));
        });
        self.0
            .drive
            .borrow_mut()
            .vigiladas
            .insert(dir.to_path_buf(), monitor);
    }
}
