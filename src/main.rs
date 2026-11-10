mod state;
mod style;
mod window;

use adw::prelude::*;
use gtk::{gio, glib};

use state::AppState;
use window::Window;

const APP_ID: &str = "io.github.caracelp.Orbital";

fn main() -> glib::ExitCode {
    let app = adw::Application::builder().application_id(APP_ID).build();
    app.connect_startup(|_| style::apply());
    app.connect_activate(activate);
    app.run()
}

fn activate(app: &adw::Application) {
    let window = Window::new(app);
    window.present();
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
