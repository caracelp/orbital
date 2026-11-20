//! El flujo de un medio (057): un `playbin` de GStreamer que lee el archivo por su `file://`,
//! envuelto como `GtkMediaStream` para usar los controles de GTK (`GtkMediaControls`).
//!
//! No se usa `GtkMediaFile`: GTK le pasa a GStreamer el archivo como flujo de bytes
//! (`gtk-media-stream://`) y con GStreamer 1.28 eso aborta el proceso entero con cualquier MP3
//! (`decodebin3: assertion failed: (collection)`; ver `quickstart.md`). Con la `uri` del archivo
//! el mismo GStreamer lo reproduce bien.
//!
//! Iniciar GStreamer y crear el `playbin` (cargan el registro y bibliotecas) van en un hilo
//! aparte; en el hilo de GTK solo se arma la salida de video y se escucha el bus. Apagar el tubo
//! (`NULL` espera a sus hilos) también va aparte.

use std::cell::{Cell, RefCell};
use std::path::Path;
use std::time::Duration;

use gstreamer as gst;
use gstreamer::prelude::*;
use gtk::subclass::prelude::*;
use gtk::{gdk, gio, glib, prelude::*};

/// Cada cuánto se avisa la posición a los controles mientras suena.
const PASO: Duration = Duration::from_millis(100);

mod imp {
    use super::*;

    #[derive(Default)]
    pub struct Flujo {
        pub(super) tubo: RefCell<Option<gst::Element>>,
        pub(super) bus: RefCell<Option<gst::bus::BusWatchGuard>>,
        pub(super) reloj: RefCell<Option<glib::SourceId>>,
        /// Segundos de inicio (`#t=`), pendientes hasta que el medio está listo.
        pub(super) inicio: Cell<Option<f64>>,
        /// Se pidió reproducir antes de que estuviera listo.
        pub(super) reproducir: Cell<bool>,
        pub(super) listo: Cell<bool>,
        pub(super) volumen: Cell<(bool, f64)>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for Flujo {
        const NAME: &'static str = "OrbitalFlujoMedio";
        type Type = super::Flujo;
        type ParentType = gtk::MediaStream;
    }

    impl ObjectImpl for Flujo {
        fn dispose(&self) {
            self.obj().apagar();
        }
    }

    impl MediaStreamImpl for Flujo {
        fn play(&self) -> bool {
            if !self.listo.get() {
                self.reproducir.set(true);
                return false;
            }
            let Some(t) = self.tubo.borrow().clone() else {
                return false;
            };
            if t.set_state(gst::State::Playing).is_err() {
                return false;
            }
            self.obj().seguir_posicion(true);
            true
        }

        fn pause(&self) {
            self.reproducir.set(false);
            if let Some(t) = self.tubo.borrow().as_ref() {
                let _ = t.set_state(gst::State::Paused);
            }
            self.obj().seguir_posicion(false);
        }

        fn seek(&self, timestamp: i64) {
            let obj = self.obj();
            let ok = self.tubo.borrow().as_ref().is_some_and(|t| {
                t.seek_simple(
                    gst::SeekFlags::FLUSH | gst::SeekFlags::ACCURATE,
                    gst::ClockTime::from_useconds(timestamp.max(0) as u64),
                )
                .is_ok()
            });
            if ok {
                obj.seek_success();
                obj.update(timestamp);
            } else {
                obj.seek_failed();
            }
        }

        fn update_audio(&self, muted: bool, volume: f64) {
            self.volumen.set((muted, volume));
            if let Some(t) = self.tubo.borrow().as_ref() {
                t.set_property("mute", muted);
                t.set_property("volume", volume.clamp(0.0, 1.0));
            }
        }
    }
}

glib::wrapper! {
    pub struct Flujo(ObjectSubclass<imp::Flujo>)
        @extends gtk::MediaStream,
        @implements gdk::Paintable;
}

impl Flujo {
    /// Empieza a abrir `path` y devuelve el flujo enseguida (los controles quedan inactivos
    /// hasta que el medio está listo). Con `pantalla`, el video se dibuja ahí. Al quedar listo
    /// salta a `inicio` y, si se pidió `reproducir`, suena.
    pub fn abrir(
        path: &Path,
        pantalla: Option<&gtk::Picture>,
        inicio: Option<f64>,
        reproducir: bool,
    ) -> Self {
        let f: Self = glib::Object::new();
        f.imp().inicio.set(inicio.filter(|t| *t > 0.0));
        f.imp().reproducir.set(reproducir);
        f.imp().volumen.set((false, 1.0));
        let uri = gio::File::for_path(path).uri().to_string();
        let video = pantalla.is_some();
        let pantalla = pantalla.map(|p| p.downgrade());
        let debil = f.downgrade();
        glib::spawn_future_local(async move {
            let armado = gio::spawn_blocking(move || -> Result<gst::Element, String> {
                gst::init().map_err(|e| e.to_string())?;
                let tubo = gst::ElementFactory::make("playbin")
                    .property("uri", &uri)
                    .build()
                    .map_err(|_| "falta el componente «playbin» de GStreamer".to_owned())?;
                // Solo lo que se muestra: sin subtítulos ni visualizaciones, y en un audio
                // tampoco la imagen de tapa (se abriría en una ventana suelta).
                let flags = if video {
                    "audio+video+soft-volume"
                } else {
                    "audio+soft-volume"
                };
                tubo.set_property_from_str("flags", flags);
                // La biblioteca de la salida de video se carga acá: en el hilo de GTK solo se crea.
                if video {
                    let _ = gst::ElementFactory::find("gtk4paintablesink").map(|f| f.load());
                }
                Ok(tubo)
            })
            .await;
            let Some(f) = debil.upgrade() else { return };
            match armado {
                Ok(Ok(tubo)) => f.conectar(tubo, pantalla.and_then(|p| p.upgrade())),
                Ok(Err(e)) => f.fallar(&e),
                Err(_) => f.fallar("no se pudo iniciar GStreamer"),
            }
        });
        f
    }

    /// En el hilo de GTK: la salida de video, el bus y a preparar (`PAUSED`, asíncrono).
    fn conectar(&self, tubo: gst::Element, pantalla: Option<gtk::Picture>) {
        if let Some(p) = &pantalla {
            match gst::ElementFactory::make("gtk4paintablesink").build() {
                // La primera vez en el proceso arma el contexto GL de GDK (≈ 110 ms, una sola
                // vez); las siguientes, ≈ 1 ms.
                Ok(sink) => {
                    let imagen = sink.property::<gdk::Paintable>("paintable");
                    p.set_paintable(Some(&imagen));
                    tubo.set_property("video-sink", &sink);
                }
                Err(_) => {
                    self.fallar("falta el componente «gtk4paintablesink» de GStreamer");
                    return;
                }
            }
        }
        let (muted, volume) = self.imp().volumen.get();
        tubo.set_property("mute", muted);
        tubo.set_property("volume", volume.clamp(0.0, 1.0));
        let Some(bus) = tubo.bus() else {
            self.fallar("GStreamer sin bus");
            return;
        };
        let debil = self.downgrade();
        let guarda = bus.add_watch_local(move |_, msg| {
            if let Some(f) = debil.upgrade() {
                f.mensaje(msg);
            }
            glib::ControlFlow::Continue
        });
        match guarda {
            Ok(g) => *self.imp().bus.borrow_mut() = Some(g),
            Err(_) => {
                self.fallar("GStreamer sin bus");
                return;
            }
        }
        if tubo.set_state(gst::State::Paused).is_err() {
            *self.imp().tubo.borrow_mut() = Some(tubo);
            self.fallar("no se pudo abrir el archivo");
            return;
        }
        *self.imp().tubo.borrow_mut() = Some(tubo);
    }

    fn mensaje(&self, msg: &gst::Message) {
        use gst::MessageView;
        let imp = self.imp();
        match msg.view() {
            MessageView::AsyncDone(_) if !imp.listo.get() => {
                let Some(tubo) = imp.tubo.borrow().clone() else {
                    return;
                };
                imp.listo.set(true);
                let duracion = tubo
                    .query_duration::<gst::ClockTime>()
                    .map_or(0, |d| d.useconds() as i64);
                let mut q = gst::query::Seeking::new(gst::Format::Time);
                let buscable = tubo.query(&mut q) && q.result().0;
                let audio = tubo.property::<i32>("n-audio") > 0;
                let video = tubo.property::<i32>("n-video") > 0;
                self.stream_prepared(audio, video, buscable, duracion);
                if let Some(t) = imp.inicio.take()
                    && buscable
                {
                    self.seek((t * 1_000_000.0) as i64);
                }
                if imp.reproducir.take() {
                    self.play();
                }
            }
            MessageView::Eos(_) => {
                self.seguir_posicion(false);
                if let Some(t) = imp.tubo.borrow().as_ref() {
                    let _ = t.set_state(gst::State::Paused);
                }
                self.update(self.duration());
                self.stream_ended();
            }
            MessageView::Error(e) => {
                let err = e.error();
                glib::g_debug!(
                    "orbital",
                    "medio: {err} ({:?})",
                    e.debug().map(|d| d.to_string())
                );
                self.fallar(err.message());
            }
            _ => {}
        }
    }

    /// Avisa la posición a los controles cada [`PASO`] mientras suena.
    fn seguir_posicion(&self, si: bool) {
        let imp = self.imp();
        if let Some(id) = imp.reloj.take() {
            id.remove();
        }
        self.avisar_posicion();
        if !si {
            return;
        }
        let debil = self.downgrade();
        let id = glib::timeout_add_local(PASO, move || match debil.upgrade() {
            Some(f) => {
                f.avisar_posicion();
                glib::ControlFlow::Continue
            }
            None => glib::ControlFlow::Break,
        });
        *imp.reloj.borrow_mut() = Some(id);
    }

    fn avisar_posicion(&self) {
        let pos = self
            .imp()
            .tubo
            .borrow()
            .as_ref()
            .and_then(|t| t.query_position::<gst::ClockTime>());
        if let Some(p) = pos {
            self.update(p.useconds() as i64);
        }
    }

    fn fallar(&self, motivo: &str) {
        self.apagar();
        self.set_error(glib::Error::new(gio::IOErrorEnum::Failed, motivo));
    }

    /// Suelta el archivo: el tubo pasa a `NULL` en otro hilo (espera a los hilos de GStreamer).
    pub fn apagar(&self) {
        let imp = self.imp();
        if let Some(id) = imp.reloj.take() {
            id.remove();
        }
        imp.bus.take();
        if let Some(t) = imp.tubo.take() {
            gio::spawn_blocking(move || {
                let _ = t.set_state(gst::State::Null);
            });
        }
    }
}
