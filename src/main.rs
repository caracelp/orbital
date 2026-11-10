mod open_dialog;
mod state;
mod style;
mod window;
mod write_ctx;

use std::path::PathBuf;
use std::time::Duration;

use adw::prelude::*;
use gtk::{gio, glib};

use state::AppState;
use window::Window;

const APP_ID: &str = "io.github.caracelp.Orbital";

/// Modo de desarrollo: `orbital --captura <salida.png> [carpeta] [nota relativa]`.
/// Abre la carpeta con «Abrir», dibuja la ventana en un PNG y sale, sin guardar estado.
struct Capture {
    out: PathBuf,
    vault: Option<PathBuf>,
    note: Option<String>,
}

fn parse_capture() -> Option<Capture> {
    let mut args = std::env::args().skip(1);
    if args.next().as_deref() != Some("--captura") {
        return None;
    }
    Some(Capture {
        out: PathBuf::from(args.next()?),
        vault: args.next().map(PathBuf::from),
        note: args.next(),
    })
}

fn main() -> glib::ExitCode {
    let capture = parse_capture();
    let mut builder = adw::Application::builder().application_id(APP_ID);
    if capture.is_some() {
        // Instancia propia, para no reusar una ventana de Orbital ya abierta.
        builder = builder.flags(gio::ApplicationFlags::NON_UNIQUE);
    }
    let app = builder.build();
    app.connect_startup(|_| style::apply());
    let capture = std::rc::Rc::new(std::cell::RefCell::new(capture));
    app.connect_activate(move |app| activate(app, capture.borrow_mut().take()));
    // Los argumentos propios no se pasan a GApplication.
    app.run_with_args(&[std::env::args().next().unwrap_or_default()])
}

fn activate(app: &adw::Application, capture: Option<Capture>) {
    let window = Window::new(app);
    window.present();
    if let Some(cap) = capture {
        window.set_persist(false);
        let app = app.clone();
        glib::spawn_future_local(async move {
            if let Some(vault) = cap.vault {
                window.open_path(vault).await;
            }
            if let Some(note) = &cap.note {
                window.show_path(note);
            }
            glib::timeout_future(Duration::from_millis(800)).await;
            match window.capture(&cap.out) {
                Ok(()) => println!("Captura guardada en {}", cap.out.display()),
                Err(e) => eprintln!("No se pudo capturar la ventana: {e}"),
            }
            app.quit();
        });
        return;
    }
    // Reabre la última bóveda, si todavía existe; si no, queda la pantalla inicial.
    glib::spawn_future_local(async move {
        let last = gio::spawn_blocking(|| AppState::load().last_vault.filter(|p| p.is_dir()))
            .await
            .ok()
            .flatten();
        if let Some(path) = last {
            window.open_path(path).await;
        }
    });
}
