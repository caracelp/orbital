//! Reproductor de audio y video de la Lectura (057), también en Dividida y en OnStage (usan el
//! mismo `Reader`).
//!
//! Es nativo: GStreamer (`playbin`, ver [`Flujo`]) con `GtkMediaControls` (reproducir y
//! pausar, barra de tiempo, volumen), sin `WebView` (ver la medición en
//! `specs/057-audio-y-video/quickstart.md`). Hasta que se pide reproducir no se abre el archivo
//! ni se inicia GStreamer: el widget anclado es una fila con el nombre y un botón. Así una nota
//! con medios cuesta lo mismo que una con imágenes, y el arranque no cambia.
//!
//! Al sacar el widget de la vista (cambiar de nota, rearmar el bloque, pasar de diapositiva) se
//! pausa y se suelta el archivo.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use gtk::prelude::*;
use gtk::{gio, glib};
use orbital_vault::medios::Tipo;

/// Alto del video antes de conocer sus medidas: 16:9 del ancho.
fn alto_169(ancho: i32) -> i32 {
    ancho * 9 / 16
}

/// El reproductor de un archivo de audio o video que existe. `inicio`: segundos desde donde
/// empieza (`#t=`). `ancho`: el de la columna de texto (la vista de texto da a los anclados su
/// tamaño mínimo, así que se pide explícito).
pub fn reproductor(path: &Path, tipo: Tipo, inicio: Option<f64>, ancho: i32) -> gtk::Widget {
    let ancho = ancho.clamp(240, 760);
    let nombre = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let es_video = tipo == Tipo::Video;
    let clase = if es_video { "Video" } else { "Audio" };
    let raiz = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(6)
        .css_classes(["orbital-medio"])
        .build();
    raiz.set_size_request(ancho, -1);
    raiz.update_property(&[gtk::accessible::Property::Label(&format!(
        "{clase}: {nombre}"
    ))]);

    // Encabezado: ícono, nombre (con el inicio si lo hay) y abrir con otra aplicación.
    let cabecera = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(8)
        .build();
    let icono = gtk::Image::from_icon_name(if es_video {
        "video-x-generic-symbolic"
    } else {
        "audio-x-generic-symbolic"
    });
    icono.set_accessible_role(gtk::AccessibleRole::Presentation);
    let rotulo = match inicio {
        Some(t) if t > 0.0 => format!("{nombre}  ·  desde {}", tiempo(t)),
        _ => nombre.clone(),
    };
    let etiqueta = gtk::Label::builder()
        .label(&rotulo)
        .xalign(0.0)
        .hexpand(true)
        .ellipsize(gtk::pango::EllipsizeMode::Middle)
        .css_classes(["orbital-medio-nombre"])
        .build();
    let afuera = boton_abrir_afuera(path, &nombre);
    cabecera.append(&icono);
    cabecera.append(&etiqueta);
    cabecera.append(&afuera);
    raiz.append(&cabecera);

    // La pantalla del video (vacía hasta reproducir) y la zona de los controles.
    let pantalla = es_video.then(|| {
        let p = gtk::Picture::builder()
            .can_shrink(true)
            .content_fit(gtk::ContentFit::Contain)
            .css_classes(["orbital-medio-pantalla"])
            .build();
        p.set_size_request(ancho, alto_169(ancho));
        p.set_alternative_text(Some(&format!("Video: {nombre}")));
        p
    });
    if let Some(p) = &pantalla {
        raiz.append(p);
    }
    let zona = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    let reproducir = gtk::Button::builder()
        .icon_name("media-playback-start-symbolic")
        .tooltip_text("Reproducir")
        .halign(gtk::Align::Start)
        .css_classes(["circular", "suggested-action"])
        .build();
    reproducir.update_property(&[gtk::accessible::Property::Label(&format!(
        "Reproducir {}: {nombre}",
        clase.to_lowercase()
    ))]);
    zona.append(&reproducir);
    raiz.append(&zona);

    let flujo: Rc<RefCell<Option<Flujo>>> = Rc::default();
    let ruta = path.to_path_buf();
    {
        let flujo = flujo.clone();
        let zona = zona.clone();
        let pantalla = pantalla.clone();
        reproducir.connect_clicked(move |b| {
            let media = cargar(&ruta, inicio, &zona, pantalla.as_ref(), ancho);
            zona.remove(b);
            *flujo.borrow_mut() = Some(media);
        });
    }
    // Un clic en la pantalla también reproduce o pausa (como cualquier reproductor).
    if let Some(p) = &pantalla {
        let clic = gtk::GestureClick::new();
        let flujo = flujo.clone();
        let reproducir = reproducir.downgrade();
        clic.connect_released(move |_, _, _, _| {
            if let Some(m) = flujo.borrow().as_ref() {
                m.set_playing(!m.is_playing());
            } else if let Some(b) = reproducir.upgrade() {
                b.emit_clicked();
            }
        });
        p.add_controller(clic);
    }
    // Fuera de la vista: se pausa y se suelta (GStreamer cierra el archivo).
    raiz.connect_unrealize(move |_| {
        if let Some(m) = flujo.borrow_mut().take() {
            m.pause();
            m.apagar();
        }
    });
    raiz.upcast()
}

/// Abre el archivo y pone los controles en `zona`; con `inicio`, salta ahí cuando el medio está
/// listo. Empieza a reproducir enseguida (se pidió con el botón).
fn cargar(
    ruta: &Path,
    inicio: Option<f64>,
    zona: &gtk::Box,
    pantalla: Option<&gtk::Picture>,
    ancho: i32,
) -> Flujo {
    let media = Flujo::abrir(ruta, pantalla, inicio, true);
    let controles = gtk::MediaControls::new(Some(&media));
    controles.set_hexpand(true);
    zona.append(&controles);
    let pantalla = pantalla.map(|p| p.downgrade());
    media.connect_prepared_notify(move |m| {
        if !m.is_prepared() {
            return;
        }
        // El alto del video según sus medidas (sin pasar de un cuadrado).
        if let Some(p) = pantalla.as_ref().and_then(|p| p.upgrade()) {
            if !m.has_video() {
                // Un «video» sin imagen (solo sonido): sin pantalla negra.
                p.set_visible(false);
                return;
            }
            let medidas = p
                .paintable()
                .map(|i| (i.intrinsic_width(), i.intrinsic_height()));
            if let Some((w, h)) = medidas.filter(|(w, h)| *w > 0 && *h > 0) {
                let ancho = w.min(ancho);
                p.set_size_request(ancho, (h * ancho / w).min(ancho));
            }
        }
    });
    let zona_d = zona.downgrade();
    let ruta_error = ruta.to_path_buf();
    media.connect_error_notify(move |m| {
        let Some(e) = m.error() else { return };
        let Some(zona) = zona_d.upgrade() else { return };
        while let Some(h) = zona.first_child() {
            zona.remove(&h);
        }
        let aviso = gtk::Label::builder()
            .label(format!(
                "No se puede reproducir acá ({}). Probá «Abrir con otra aplicación».",
                e.message()
            ))
            .wrap(true)
            .xalign(0.0)
            .css_classes(["dim-label"])
            .build();
        zona.append(&aviso);
        glib::g_debug!("orbital", "medio {}: {e}", ruta_error.display());
    });
    // El foco pasa a los controles: quien usa el teclado sigue en el reproductor.
    controles.child_focus(gtk::DirectionType::TabForward);
    media
}

/// Botón para abrir el archivo con la aplicación del sistema (la salida si GStreamer no
/// reproduce el formato).
fn boton_abrir_afuera(path: &Path, nombre: &str) -> gtk::Button {
    let b = gtk::Button::builder()
        .icon_name("external-link-symbolic")
        .tooltip_text("Abrir con otra aplicación")
        .css_classes(["flat"])
        .valign(gtk::Align::Center)
        .build();
    b.update_property(&[gtk::accessible::Property::Label(&format!(
        "Abrir {nombre} con otra aplicación"
    ))]);
    let path: PathBuf = path.to_path_buf();
    b.connect_clicked(move |b| abrir_afuera(b, &path));
    b
}

/// Abre un archivo con la aplicación predeterminada del sistema.
pub fn abrir_afuera(desde: &impl IsA<gtk::Widget>, path: &Path) {
    let ventana = desde.root().and_downcast::<gtk::Window>();
    gtk::FileLauncher::new(Some(&gio::File::for_path(path))).launch(
        ventana.as_ref(),
        None::<&gio::Cancellable>,
        |_| {},
    );
}

/// `1:05`, `1:02:03`.
pub fn tiempo(segundos: f64) -> String {
    let s = segundos.max(0.0) as u64;
    let (h, m, s) = (s / 3600, (s / 60) % 60, s % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}

mod flujo;
pub use flujo::Flujo;

#[cfg(test)]
pub(crate) mod tests;
