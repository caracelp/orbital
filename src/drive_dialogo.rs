//! Diálogo «Google Drive» (061a, 061b): conectar la bóveda (con el proyecto de Google Cloud de la
//! usuaria), ver el estado, la cuota, los conflictos, qué pasa con lo nuevo y las reglas de
//! ubicación (solo en este equipo, solo en Drive, en ambos).
//!
//! El id y el secreto del cliente los escribe la usuaria acá y van al llavero del sistema junto
//! con el token de renovación; nunca a la bóveda ni al repositorio.

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::Duration;

use adw::prelude::*;
use gtk::glib;
use orbital_drive::llavero::{self, Cliente};
use orbital_drive::motor::Destino;
use orbital_drive::reglas::Ubicacion;
use orbital_drive::{Conexion, InfoConflicto, Resumen};

/// Lo que el diálogo le pide a la ventana.
pub enum Pedido {
    Conectar {
        cuenta: String,
        destino: Destino,
    },
    Ahora,
    Desconectar,
    PonerLocal(String, bool),
    Resolver(String),
    /// Quitar una regla de este equipo («en ambos» / «solo en Drive»).
    QuitarRegla(String),
    /// Qué pasa con lo nuevo de este equipo y con lo nuevo en Drive.
    Predeterminados(Ubicacion, Ubicacion),
}

/// Lo que el diálogo muestra.
pub struct Datos {
    pub nombre_boveda: String,
    pub root: PathBuf,
    pub conexion: Option<Conexion>,
    /// El motor de este equipo está corriendo.
    pub activa: bool,
    pub resumen: Option<Resumen>,
    pub detenido: Option<String>,
    pub conflictos: Vec<InfoConflicto>,
    pub cliente: Option<Cliente>,
    /// Las reglas de ubicación vigentes (compartidas y de este equipo).
    pub reglas: Vec<(String, Ubicacion)>,
}

/// Bytes en GB o MB, para la cuota.
fn tamano(b: u64) -> String {
    const GB: f64 = (1u64 << 30) as f64;
    const MB: f64 = (1u64 << 20) as f64;
    let b = b as f64;
    if b >= GB {
        format!("{:.1} GB", b / GB).replace('.', ",")
    } else {
        format!("{:.0} MB", (b / MB).max(1.0))
    }
}

/// Texto corto del estado (barra de estado y diálogo).
pub fn texto_resumen(r: &Resumen) -> String {
    if let Some(p) = &r.pausa {
        return format!("En pausa: {p}");
    }
    if !r.en_linea {
        return "Sin conexión: los cambios se suben al volver la red".into();
    }
    let mut partes = Vec::new();
    if r.sincronizando {
        partes.push("Sincronizando…".to_owned());
    } else if r.pendientes > 0 {
        partes.push(format!("{} por subir", r.pendientes));
    } else {
        partes.push(match &r.ultima {
            Some(h) => format!("Al día (última sincronización: {h})"),
            None => "Al día".into(),
        });
    }
    if r.conflictos == 1 {
        partes.push("1 conflicto".into());
    } else if r.conflictos > 1 {
        partes.push(format!("{} conflictos", r.conflictos));
    }
    if let Some(e) = &r.error {
        partes.push(format!("reintentando: {e}"));
    }
    partes.join(" · ")
}

fn fila_texto(titulo: &str, sub: &str) -> adw::ActionRow {
    let r = adw::ActionRow::builder()
        .title(titulo)
        .subtitle(sub)
        .subtitle_selectable(true)
        .build();
    r.add_css_class("property");
    r
}

pub fn mostrar(
    parent: &impl IsA<gtk::Widget>,
    datos: Datos,
    pedir: Rc<dyn Fn(Pedido)>,
) -> adw::PreferencesDialog {
    let dialog = adw::PreferencesDialog::builder()
        .title("Google Drive")
        .content_width(620)
        .build();
    let page = adw::PreferencesPage::new();
    if datos.activa {
        estado(&page, &datos, &pedir, &dialog);
    } else {
        conectar(&page, &datos, &pedir, &dialog);
    }
    if datos.conexion.is_some() {
        reglas(&page, &datos, &pedir);
    }
    dialog.add(&page);
    dialog.present(Some(parent));
    dialog
}

/// Bóveda conectada en este equipo: estado, conflictos y acciones.
fn estado(
    page: &adw::PreferencesPage,
    datos: &Datos,
    pedir: &Rc<dyn Fn(Pedido)>,
    dialog: &adw::PreferencesDialog,
) {
    let c = datos.conexion.clone().unwrap_or_default();
    let g = adw::PreferencesGroup::builder()
        .title("Sincronización")
        .description("La bóveda está en este equipo y en Google Drive. Orbital sube y baja los cambios solo.")
        .build();
    g.add(&fila_texto("Cuenta", &c.cuenta));
    let carpeta = fila_texto("Carpeta en Drive", &c.carpeta);
    let abrir = gtk::Button::builder()
        .icon_name("adw-external-link-symbolic")
        .valign(gtk::Align::Center)
        .tooltip_text("Abrir la carpeta en Google Drive")
        .css_classes(["flat"])
        .build();
    abrir.update_property(&[gtk::accessible::Property::Label(
        "Abrir la carpeta en Google Drive",
    )]);
    let url = format!("https://drive.google.com/drive/folders/{}", c.carpeta);
    abrir.connect_clicked(move |b| {
        gtk::UriLauncher::new(&url).launch(
            b.root().and_downcast_ref::<gtk::Window>(),
            gtk::gio::Cancellable::NONE,
            |_| {},
        );
    });
    carpeta.add_suffix(&abrir);
    g.add(&carpeta);
    let texto = match (&datos.resumen, &datos.detenido) {
        (_, Some(d)) => format!("Detenida: {d}"),
        (Some(r), None) => texto_resumen(r),
        (None, None) => "Empezando…".into(),
    };
    g.add(&fila_texto("Estado", &texto));
    if let Some(c) = datos.resumen.as_ref().and_then(|r| r.cuota) {
        let (texto, fraccion) = match c.limite {
            Some(l) if l > 0 => (
                format!(
                    "{} de {} usados ({} libres)",
                    tamano(c.usado),
                    tamano(l),
                    tamano(l.saturating_sub(c.usado))
                ),
                Some(c.usado as f64 / l as f64),
            ),
            _ => (format!("{} usados, sin límite", tamano(c.usado)), None),
        };
        let fila = fila_texto("Espacio en Google Drive", &texto);
        if let Some(f) = fraccion {
            let barra = gtk::LevelBar::builder()
                .min_value(0.0)
                .max_value(1.0)
                .value(f.clamp(0.0, 1.0))
                .valign(gtk::Align::Center)
                .width_request(120)
                .build();
            barra.update_property(&[gtk::accessible::Property::Label(
                "Espacio usado en Google Drive",
            )]);
            fila.add_suffix(&barra);
        }
        g.add(&fila);
    }
    if let Some(r) = &datos.resumen
        && r.solo_drive > 0
    {
        g.add(&fila_texto(
            "Solo en Drive",
            &if r.solo_drive == 1 {
                "1 archivo no ocupa lugar en este equipo".to_owned()
            } else {
                format!("{} archivos no ocupan lugar en este equipo", r.solo_drive)
            },
        ));
    }
    let botones = gtk::Box::builder()
        .spacing(12)
        .halign(gtk::Align::End)
        .margin_top(12)
        .build();
    let ahora = gtk::Button::with_label("Sincronizar ahora");
    let desconectar = gtk::Button::builder()
        .label("Desconectar este equipo…")
        .css_classes(["destructive-action"])
        .build();
    botones.append(&desconectar);
    botones.append(&ahora);
    let p = pedir.clone();
    ahora.connect_clicked(move |_| p(Pedido::Ahora));
    let (p, d) = (pedir.clone(), dialog.clone());
    desconectar.connect_clicked(move |_| {
        let aviso = adw::AlertDialog::new(
            Some("¿Desconectar este equipo de Drive?"),
            Some("Orbital deja de sincronizar esta bóveda en este equipo. No se borra nada: ni los archivos de este equipo ni los de Drive. Los otros equipos siguen sincronizados."),
        );
        aviso.add_response("cancelar", "Cancelar");
        aviso.add_response("desconectar", "Desconectar");
        aviso.set_response_appearance("desconectar", adw::ResponseAppearance::Destructive);
        aviso.set_close_response("cancelar");
        let (p, d2) = (p.clone(), d.clone());
        aviso.choose(Some(&d), gtk::gio::Cancellable::NONE, move |r| {
            if r == "desconectar" {
                p(Pedido::Desconectar);
                d2.close();
            }
        });
    });
    g.add(&botones);
    page.add(&g);
    if let Some(r) = &datos.resumen {
        lo_nuevo(page, r, pedir);
    }

    if !datos.conflictos.is_empty() {
        let gc = adw::PreferencesGroup::builder()
            .title("Conflictos")
            .description("Estos archivos cambiaron en este equipo y en Drive. Las dos versiones están a salvo hasta que elijas.")
            .build();
        for c in &datos.conflictos {
            let sub = match &c.autor {
                Some(a) => format!("En Drive lo cambió {a}"),
                None => "Cambió en este equipo y en Drive".into(),
            };
            let fila = adw::ActionRow::builder()
                .title(&c.ruta)
                .subtitle(&sub)
                .build();
            let b = gtk::Button::builder()
                .label("Resolver…")
                .valign(gtk::Align::Center)
                .build();
            b.update_property(&[gtk::accessible::Property::Label(&format!(
                "Resolver el conflicto de {}",
                c.ruta
            ))]);
            let (p, r, d) = (pedir.clone(), c.ruta.clone(), dialog.clone());
            b.connect_clicked(move |_| {
                d.close();
                p(Pedido::Resolver(r.clone()));
            });
            fila.add_suffix(&b);
            gc.add(&fila);
        }
        page.add(&gc);
    }
}

/// Bóveda sin conectar en este equipo: los tres pasos.
fn conectar(
    page: &adw::PreferencesPage,
    datos: &Datos,
    pedir: &Rc<dyn Fn(Pedido)>,
    dialog: &adw::PreferencesDialog,
) {
    if let Some(d) = &datos.detenido {
        let g = adw::PreferencesGroup::new();
        let banner = adw::ActionRow::builder()
            .title("La sincronización está detenida")
            .subtitle(d)
            .build();
        banner.add_prefix(&gtk::Image::from_icon_name("dialog-warning-symbolic"));
        g.add(&banner);
        page.add(&g);
    }
    // 1. El proyecto de Google Cloud.
    let g1 = adw::PreferencesGroup::builder()
        .title("1. Tu proyecto de Google Cloud")
        .description("Orbital se conecta con un proyecto de Google Cloud tuyo (gratis, se crea una sola vez): en console.cloud.google.com creá un proyecto, habilitá la «Google Drive API», configurá la pantalla de consentimiento como «Externa» y publicala («En producción»), y creá un ID de cliente OAuth de tipo «App de escritorio». Copiá acá el ID y el secreto: quedan en el llavero de GNOME, no en la bóveda.")
        .build();
    let id = adw::EntryRow::builder().title("ID del cliente").build();
    let secreto = adw::PasswordEntryRow::builder()
        .title("Secreto del cliente")
        .build();
    if let Some(c) = &datos.cliente {
        id.set_text(&c.id);
        secreto.set_text(&c.secreto);
    }
    g1.add(&id);
    g1.add(&secreto);
    page.add(&g1);

    // 2. Iniciar sesión.
    let g2 = adw::PreferencesGroup::builder()
        .title("2. Tu cuenta de Google")
        .description("Se abre el navegador para que autorices a Orbital a usar tu Drive. Google puede avisar que la app no está verificada: es tu propio proyecto, elegí «Continuar».")
        .build();
    let sesion = adw::ActionRow::builder()
        .title("Sin iniciar sesión")
        .build();
    let iniciar = gtk::Button::builder()
        .label("Iniciar sesión con Google")
        .valign(gtk::Align::Center)
        .css_classes(["suggested-action"])
        .build();
    sesion.add_suffix(&iniciar);
    g2.add(&sesion);
    page.add(&g2);

    // 3. La carpeta de Drive.
    let g3 = adw::PreferencesGroup::builder()
        .title("3. La carpeta de Drive")
        .description("Para tener esta bóveda en otra computadora: abrí ahí una carpeta vacía con Orbital y conectala a la misma carpeta de Drive.")
        .build();
    let nueva = adw::ActionRow::builder()
        .title(format!(
            "Crear la carpeta «{}» en Mi unidad",
            datos.nombre_boveda
        ))
        .activatable(true)
        .build();
    let r_nueva = gtk::CheckButton::new();
    nueva.add_prefix(&r_nueva);
    nueva.set_activatable_widget(Some(&r_nueva));
    let existente = adw::ActionRow::builder()
        .title("Usar una carpeta que ya está en Drive")
        .activatable(true)
        .build();
    let r_existente = gtk::CheckButton::builder().group(&r_nueva).build();
    existente.add_prefix(&r_existente);
    existente.set_activatable_widget(Some(&r_existente));
    let enlace = adw::EntryRow::builder()
        .title("Enlace o id de la carpeta")
        .build();
    match &datos.conexion {
        Some(c) if !c.carpeta.is_empty() => {
            r_existente.set_active(true);
            enlace.set_text(&format!(
                "https://drive.google.com/drive/folders/{}",
                c.carpeta
            ));
        }
        _ => r_nueva.set_active(true),
    }
    g3.add(&nueva);
    g3.add(&existente);
    g3.add(&enlace);
    let conectar = gtk::Button::builder()
        .label("Conectar")
        .halign(gtk::Align::End)
        .margin_top(12)
        .sensitive(false)
        .css_classes(["suggested-action"])
        .build();
    g3.add(&conectar);
    page.add(&g3);

    let cuenta: Rc<RefCell<Option<String>>> = Rc::default();
    let (id_, sec_, ses_, con_, cuenta_) = (
        id.clone(),
        secreto.clone(),
        sesion.clone(),
        conectar.clone(),
        cuenta.clone(),
    );
    iniciar.connect_clicked(move |b| {
        let cliente = Cliente {
            id: id_.text().trim().to_owned(),
            secreto: sec_.text().trim().to_owned(),
        };
        if cliente.id.is_empty() || cliente.secreto.is_empty() {
            ses_.set_title("Falta el ID o el secreto del cliente (paso 1)");
            return;
        }
        b.set_sensitive(false);
        ses_.set_title("Esperando la autorización en el navegador…");
        let (tx_url, rx_url) = futures_channel::oneshot::channel::<String>();
        let (tx_fin, rx_fin) = futures_channel::oneshot::channel();
        std::thread::spawn(move || {
            let l = llavero::del_entorno();
            let r = orbital_drive::oauth::iniciar_sesion(
                &cliente,
                &*l,
                move |u| {
                    let _ = tx_url.send(u);
                },
                Duration::from_secs(300),
            );
            let _ = tx_fin.send(r);
        });
        let ventana = b.root().and_downcast::<gtk::Window>();
        glib::spawn_future_local(async move {
            if let Ok(u) = rx_url.await {
                gtk::UriLauncher::new(&u).launch(
                    ventana.as_ref(),
                    gtk::gio::Cancellable::NONE,
                    |_| {},
                );
            }
        });
        let (b, ses, con, cuenta) = (b.clone(), ses_.clone(), con_.clone(), cuenta_.clone());
        glib::spawn_future_local(async move {
            match rx_fin.await {
                Ok(Ok(s)) => {
                    ses.set_title(&format!("Sesión iniciada como {}", s.cuenta));
                    *cuenta.borrow_mut() = Some(s.cuenta);
                    con.set_sensitive(true);
                }
                Ok(Err(e)) => ses.set_title(&format!("No se pudo iniciar sesión: {e}")),
                Err(_) => ses.set_title("No se pudo iniciar sesión"),
            }
            b.set_sensitive(true);
        });
    });
    let (p, d, nombre) = (pedir.clone(), dialog.clone(), datos.nombre_boveda.clone());
    conectar.connect_clicked(move |_| {
        let Some(c) = cuenta.borrow().clone() else {
            return;
        };
        let destino = if r_existente.is_active() {
            Destino::Existente(enlace.text().to_string())
        } else {
            Destino::Nueva(nombre.clone())
        };
        p(Pedido::Conectar { cuenta: c, destino });
        d.close();
    });
}

/// Reglas «solo en este equipo» (compartidas con los otros equipos, decisión 2).
/// Qué pasa con lo nuevo (061b): lo que se crea en este equipo y lo que aparece en Drive.
fn lo_nuevo(page: &adw::PreferencesPage, r: &Resumen, pedir: &Rc<dyn Fn(Pedido)>) {
    let g = adw::PreferencesGroup::builder()
        .title("Lo nuevo")
        .description("Para lo que no tiene una ubicación elegida en el explorador. Vale solo para este equipo.")
        .build();
    let locales = [Ubicacion::Ambos, Ubicacion::SoloDrive, Ubicacion::Local];
    let remotos = [Ubicacion::Ambos, Ubicacion::SoloDrive];
    let modelo =
        |us: &[Ubicacion]| gtk::StringList::new(&us.iter().map(|u| u.texto()).collect::<Vec<_>>());
    let local = adw::ComboRow::builder()
        .title("Lo que se crea en este equipo")
        .model(&modelo(&locales))
        .selected(
            locales
                .iter()
                .position(|u| *u == r.nuevo_local)
                .unwrap_or(0) as u32,
        )
        .build();
    let remoto = adw::ComboRow::builder()
        .title("Lo que aparece nuevo en Drive")
        .subtitle("«Solo en Drive»: no se baja solo; se abre con conexión")
        .model(&modelo(&remotos))
        .selected(
            remotos
                .iter()
                .position(|u| *u == r.nuevo_remoto)
                .unwrap_or(0) as u32,
        )
        .build();
    let avisar = {
        let (p, l, rr) = (pedir.clone(), local.clone(), remoto.clone());
        move || {
            let a = locales[(l.selected() as usize).min(locales.len() - 1)];
            let b = remotos[(rr.selected() as usize).min(remotos.len() - 1)];
            p(Pedido::Predeterminados(a, b));
        }
    };
    let a2 = avisar.clone();
    local.connect_selected_notify(move |_| a2());
    remoto.connect_selected_notify(move |_| avisar());
    g.add(&local);
    g.add(&remoto);
    page.add(&g);
}

fn reglas(page: &adw::PreferencesPage, datos: &Datos, pedir: &Rc<dyn Fn(Pedido)>) {
    let g = adw::PreferencesGroup::builder()
        .title("Ubicación")
        .description("Lo elegido en el explorador o en el menú de la nota. «Solo en este equipo» vale para todos los equipos; «Solo en Drive» y «En ambos», solo para este. Una carpeta vale para lo que tiene adentro, salvo lo que tenga su propia ubicación.")
        .build();
    let mut reglas = datos.reglas.clone();
    if datos.reglas.is_empty()
        && let Some(c) = &datos.conexion
    {
        reglas = c
            .solo_local
            .iter()
            .map(|r| (r.clone(), Ubicacion::Local))
            .collect();
    }
    if reglas.is_empty() {
        g.add(
            &adw::ActionRow::builder()
                .title("Nada elegido: todo está en este equipo y en Drive")
                .build(),
        );
    }
    for (r, u) in &reglas {
        let fila = adw::ActionRow::builder()
            .title(r)
            .subtitle(u.texto())
            .build();
        let icono = match u {
            Ubicacion::Local => "computer-symbolic",
            Ubicacion::SoloDrive => "folder-remote-symbolic",
            Ubicacion::Ambos => "weather-overcast-symbolic",
        };
        fila.add_prefix(&gtk::Image::from_icon_name(icono));
        let quitar = gtk::Button::builder()
            .icon_name("list-remove-symbolic")
            .valign(gtk::Align::Center)
            .tooltip_text("Quitar esta ubicación")
            .css_classes(["flat"])
            .build();
        quitar.update_property(&[gtk::accessible::Property::Label(&format!(
            "Quitar la ubicación de {r}: {}",
            u.texto()
        ))]);
        let (p, r2, u2, fila2, g2) = (pedir.clone(), r.clone(), *u, fila.clone(), g.clone());
        quitar.connect_clicked(move |_| {
            if u2 == Ubicacion::Local {
                p(Pedido::PonerLocal(r2.clone(), false));
            } else {
                p(Pedido::QuitarRegla(r2.clone()));
            }
            g2.remove(&fila2);
        });
        fila.add_suffix(&quitar);
        g.add(&fila);
    }
    let agregar = gtk::Button::builder()
        .label("Solo en este equipo: agregar una carpeta…")
        .halign(gtk::Align::End)
        .margin_top(12)
        .build();
    let (p, root) = (pedir.clone(), datos.root.clone());
    agregar.connect_clicked(move |b| {
        let elegir = gtk::FileDialog::builder()
            .title("Carpeta que no se sube a Drive")
            .initial_folder(&gtk::gio::File::for_path(&root))
            .modal(true)
            .build();
        let (p, root) = (p.clone(), root.clone());
        let ventana = b.root().and_downcast::<gtk::Window>();
        elegir.select_folder(ventana.as_ref(), gtk::gio::Cancellable::NONE, move |r| {
            if let Ok(f) = r
                && let Some(path) = f.path()
                && let Ok(rel) = path.strip_prefix(&root)
                && !rel.as_os_str().is_empty()
            {
                p(Pedido::PonerLocal(
                    format!("{}/", rel.to_string_lossy()),
                    true,
                ));
            }
        });
    });
    g.add(&agregar);
    page.add(&g);
}

/// Elegir qué versión queda en un conflicto con Drive. `local` y `drive` son los textos (solo
/// para comparar las notas de texto).
pub fn resolver(
    parent: &impl IsA<gtk::Widget>,
    c: &InfoConflicto,
    textos: Option<(String, String)>,
    formato: orbital_vault::Format,
    elegir: impl Fn(bool) + 'static,
) {
    let quien = match &c.autor {
        Some(a) => format!(" (la cambió {a})"),
        None => String::new(),
    };
    let dialog = adw::AlertDialog::builder()
        .heading("Conflicto con Google Drive")
        .body(format!(
            "«{}» cambió en este equipo y en Drive{quien}. Las dos versiones están a salvo: elegí cuál queda. La otra va al historial de versiones (o al respaldo, si no es una nota).",
            c.ruta
        ))
        .close_response("cancelar")
        .build();
    dialog.add_response("mia", "Conservar la de este equipo");
    dialog.add_response("drive", "Usar la de Drive");
    if textos.is_some() {
        dialog.add_response("comparar", "Comparar");
    }
    dialog.add_response("cancelar", "Cancelar");
    dialog.set_response_appearance("mia", adw::ResponseAppearance::Suggested);
    dialog.set_default_response(Some("mia"));
    let elegir = Rc::new(elegir);
    let parent_w = parent.as_ref().clone();
    let ruta = c.ruta.clone();
    dialog.choose(
        Some(parent),
        gtk::gio::Cancellable::NONE,
        move |r| match r.as_str() {
            "mia" => elegir(true),
            "drive" => elegir(false),
            "comparar" => {
                if let Some((local, remoto)) = &textos {
                    let e = elegir.clone();
                    crate::editor::conflict::show_compare_window_con(
                        &parent_w,
                        &ruta,
                        local,
                        remoto,
                        formato,
                        crate::editor::conflict::Textos {
                            mia: "Versión de este equipo",
                            otra: "Versión de Drive",
                            usar_otra: "Usar la de Drive",
                            conservar: "Conservar la de este equipo",
                        },
                        move |c| e(c == crate::editor::conflict::ConflictChoice::KeepMine),
                    );
                }
            }
            _ => {}
        },
    );
}
