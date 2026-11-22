//! Diálogo «Google Drive» (061a): conectar la bóveda (con el proyecto de Google Cloud de la
//! usuaria), ver el estado, los conflictos y las reglas «solo en este equipo».
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
use orbital_drive::{Conexion, InfoConflicto, Resumen};

/// Lo que el diálogo le pide a la ventana.
pub enum Pedido {
    Conectar { cuenta: String, destino: Destino },
    Ahora,
    Desconectar,
    PonerLocal(String, bool),
    Resolver(String),
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
fn reglas(page: &adw::PreferencesPage, datos: &Datos, pedir: &Rc<dyn Fn(Pedido)>) {
    let g = adw::PreferencesGroup::builder()
        .title("Solo en este equipo")
        .description("Lo que está acá nunca se sube a Drive (en ningún equipo). Una nota se puede marcar desde su menú.")
        .build();
    let c = datos.conexion.clone().unwrap_or_default();
    if c.solo_local.is_empty() {
        g.add(
            &adw::ActionRow::builder()
                .title("Nada: toda la bóveda se sincroniza")
                .build(),
        );
    }
    for r in &c.solo_local {
        let fila = adw::ActionRow::builder().title(r).build();
        fila.add_prefix(&gtk::Image::from_icon_name("computer-symbolic"));
        let quitar = gtk::Button::builder()
            .icon_name("list-remove-symbolic")
            .valign(gtk::Align::Center)
            .tooltip_text("Volver a sincronizar")
            .css_classes(["flat"])
            .build();
        quitar.update_property(&[gtk::accessible::Property::Label(&format!(
            "Volver a sincronizar {r}"
        ))]);
        let (p, r2, fila2, g2) = (pedir.clone(), r.clone(), fila.clone(), g.clone());
        quitar.connect_clicked(move |_| {
            p(Pedido::PonerLocal(r2.clone(), false));
            g2.remove(&fila2);
        });
        fila.add_suffix(&quitar);
        g.add(&fila);
    }
    let agregar = gtk::Button::builder()
        .label("Agregar una carpeta…")
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
