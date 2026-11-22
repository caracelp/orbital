mod bases;
mod carrusel;
mod diagrama_ampliado;
mod diagramas;
mod ecuaciones;
mod editor;
mod export;
mod formulas;
mod galeria;
mod galeria_lateral;
mod graficos;
mod grafo;
mod guardar_en;
mod html_view;
mod iconos;
mod imagenes;
mod listas_panel;
mod medios;
mod medir_arranque;
mod onstage;
mod open_dialog;
mod panel_derecho;
mod pestanas;
mod pie_boveda;
mod preferences;
mod reparto;
mod resaltado;
mod search_panel;
mod state;
mod status_bar;
mod style;
mod switcher;
mod tag_panel;
mod visor_pdf;
mod watcher;
mod webkit_perezoso;
mod window;
mod write_ctx;

use std::path::PathBuf;
use std::time::Duration;

use adw::prelude::*;
use gtk::{gio, glib};

use state::AppState;
use window::{LecturaAnticipada, Window};

const APP_ID: &str = "io.github.caracelp.Orbital";

/// Modo de desarrollo: `orbital --captura <salida.png> [carpeta] [nota relativa]`.
/// Abre la carpeta con «Abrir», dibuja la ventana en un PNG y sale, sin guardar estado.
struct Capture {
    out: PathBuf,
    vault: Option<PathBuf>,
    note: Option<String>,
    /// `--medir-escritura`: en vez de capturar, mide la latencia al escribir.
    measure: bool,
}

fn parse_capture() -> Option<Capture> {
    let mut args = std::env::args().skip(1);
    let measure = match args.next().as_deref() {
        Some("--captura") => false,
        Some("--medir-escritura") => true,
        _ => return None,
    };
    if measure {
        // `--medir-escritura <carpeta> <nota>`: mide la latencia por tecla y sale.
        return Some(Capture {
            out: PathBuf::new(),
            vault: args.next().map(PathBuf::from),
            note: args.next(),
            measure: true,
        });
    }
    Some(Capture {
        out: PathBuf::from(args.next()?),
        vault: args.next().map(PathBuf::from),
        note: args.next(),
        measure: false,
    })
}

/// `orbital <carpeta>`: abre esa carpeta con «Abrir» en vez de la última bóveda.
/// `orbital --salir-al-mostrar <carpeta>`: lo mismo, y sale apenas la lista se dibuja (para medir
/// el arranque con hyperfine).
fn parse_folder() -> Option<(PathBuf, bool)> {
    let mut args = std::env::args().skip(1);
    let first = args.next()?;
    if first == "--salir-al-mostrar" {
        return args.next().map(|p| (PathBuf::from(p), true));
    }
    (!first.starts_with('-')).then(|| (PathBuf::from(first), false))
}

fn main() -> glib::ExitCode {
    medir_arranque::empezar();
    let capture = parse_capture();
    let folder = if capture.is_none() {
        parse_folder()
    } else {
        None
    };
    let argv0 = std::env::args().next().unwrap_or_default();
    // La bóveda se empieza a leer ya, en otro hilo, mientras GTK se inicia y la ventana se arma
    // (060): la carpeta pedida o, en el uso normal, la última abierta.
    let lectura = if capture.is_some() {
        None
    } else {
        let ruta = match &folder {
            Some((p, _)) => Some(std::path::absolute(p).unwrap_or_else(|_| p.clone())),
            None => AppState::load().last_vault.filter(|p| p.is_dir()),
        };
        ruta.map(LecturaAnticipada::empezar)
    };
    let lectura = std::cell::RefCell::new(lectura);

    // Modos de desarrollo (captura y medición): instancia propia que sale sola.
    if capture.is_some() || folder.as_ref().is_some_and(|f| f.1) {
        let app = adw::Application::builder()
            .application_id(APP_ID)
            .flags(gio::ApplicationFlags::NON_UNIQUE)
            .build();
        app.connect_startup(|_| {
            medir_arranque::marca("startup (GTK y libadwaita iniciados)");
            style::apply();
            medir_arranque::marca("startup: CSS");
        });
        let capture = std::cell::RefCell::new(capture);
        let folder = std::cell::RefCell::new(folder);
        app.connect_activate(move |app| {
            activate(
                app,
                capture.borrow_mut().take(),
                folder.borrow_mut().take().map(|f| f.1),
                lectura.borrow_mut().take(),
            )
        });
        medir_arranque::marca("main: aplicación armada");
        let salida = app.run_with_args(&[argv0]);
        medir_arranque::marca("fin de run");
        return salida;
    }

    // Uso normal: una sola instancia, como las apps de GNOME. Lanzar Orbital otra vez trae al
    // frente la ventana existente; `orbital <carpeta>` la abre ahí. Para pruebas,
    // ORBITAL_INSTANCIA_PROPIA=1 corre una instancia separada que no toca la del usuario.
    let mut flags = gio::ApplicationFlags::HANDLES_OPEN;
    if std::env::var_os("ORBITAL_INSTANCIA_PROPIA").is_some() {
        flags |= gio::ApplicationFlags::NON_UNIQUE;
    }
    let app = adw::Application::builder()
        .application_id(APP_ID)
        .flags(flags)
        .build();
    app.connect_startup(|_| style::apply());
    let main_window: std::rc::Rc<std::cell::RefCell<Option<Window>>> = std::rc::Rc::default();
    let lectura = std::rc::Rc::new(lectura);
    {
        let main_window = main_window.clone();
        let lectura = lectura.clone();
        app.connect_activate(move |app| {
            if let Some(w) = main_window.borrow().as_ref() {
                w.present();
                return;
            }
            let w = Window::new(app);
            w.maximize();
            // Sin carpeta pedida, la lectura anticipada es la de la última bóveda.
            let espera = lectura
                .borrow_mut()
                .take()
                .and_then(|l| w.abrir_anticipada(l));
            w.present();
            *main_window.borrow_mut() = Some(w.clone());
            if let Some(espera) = espera {
                glib::spawn_future_local(espera);
            }
        });
    }
    app.connect_open(move |app, files, _| {
        let Some(path) = files.first().and_then(|f| f.path()) else {
            return;
        };
        let existing = main_window.borrow().clone();
        let w = existing.unwrap_or_else(|| {
            let w = Window::new(app);
            w.maximize();
            *main_window.borrow_mut() = Some(w.clone());
            w
        });
        // La primera vez, la carpeta pedida ya se está leyendo (060).
        let anticipada = lectura.borrow_mut().take();
        match anticipada {
            Some(l) if l.ruta() == path => {
                let espera = w.abrir_anticipada(l);
                w.present();
                if let Some(espera) = espera {
                    glib::spawn_future_local(espera);
                }
            }
            _ => {
                w.present();
                glib::spawn_future_local(async move { w.open_path(path).await });
            }
        }
    });
    let mut args = vec![argv0];
    if let Some((path, _)) = folder {
        args.push(path.to_string_lossy().into_owned());
    }
    app.run_with_args(&args)
}

/// Reabre la última bóveda, si todavía existe; si no, queda la pantalla inicial.
fn reopen_last(window: Window) {
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

/// Modos de desarrollo: `exit_when_shown` es `Some` con `orbital [--salir-al-mostrar] <carpeta>`
/// (la carpeta llega como `lectura`).
fn activate(
    app: &adw::Application,
    capture: Option<Capture>,
    exit_when_shown: Option<bool>,
    lectura: Option<LecturaAnticipada>,
) {
    medir_arranque::marca("activate");
    let window = Window::new(app);
    medir_arranque::marca("Window::new");
    // Si la bóveda ya se leyó, el primer cuadro sale con ella (060).
    let espera = lectura.and_then(|l| window.abrir_anticipada(l));
    let ya_abierta = exit_when_shown.is_some() && espera.is_none();
    // Maximizada como en el uso normal (056), salvo que la captura pida un tamaño fijo.
    if capture.is_none() || std::env::var_os("ORBITAL_CAPTURA_TAMANO").is_none() {
        window.maximize();
    }
    window.present();
    medir_arranque::marca("present");
    window.fases_del_proximo_cuadro("primer cuadro");
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
            if cap.measure {
                glib::timeout_future(Duration::from_millis(1500)).await;
                let mut medidas = window.measure_typing(301).await;
                // La primera tecla lleva el cursor al final y calcula la disposición de todo el
                // texto; se informa aparte.
                let first = if medidas.is_empty() {
                    0.0
                } else {
                    let (i, c) = medidas.remove(0);
                    i + c
                };
                println!("Primera tecla (disposición de todo el texto): {first:.2} ms");
                let mediana = |mut v: Vec<f64>| -> (f64, f64, f64) {
                    if v.is_empty() {
                        return (0.0, 0.0, 0.0);
                    }
                    v.sort_by(f64::total_cmp);
                    let at = |q: f64| v[((v.len() as f64 - 1.0) * q).round() as usize];
                    (at(0.5), at(0.99), v[v.len() - 1])
                };
                let (m, p99, max) = mediana(medidas.iter().map(|(i, c)| i + c).collect());
                let (mi, _, _) = mediana(medidas.iter().map(|(i, _)| *i).collect());
                let (mc, _, _) = mediana(medidas.iter().map(|(_, c)| *c).collect());
                println!(
                    "Latencia por tecla ({} de 300 medidas): mediana {m:.2} ms, p99 {p99:.2} ms, máx {max:.2} ms \
                     (mediana de la inserción {mi:.2} ms, del cuadro {mc:.2} ms)",
                    medidas.len()
                );
                app.quit();
                return;
            }
            // WebKit dibuja en otro proceso: se le da tiempo para que la página aparezca.
            glib::timeout_future(Duration::from_millis(1500)).await;
            // Dos veces: el primer desplazamiento usa alturas de renglón todavía estimadas.
            for _ in 0..2 {
                window.preparar_captura();
                glib::timeout_future(Duration::from_millis(700)).await;
            }
            match window.capture(&cap.out) {
                Ok(()) => println!("Captura guardada en {}", cap.out.display()),
                Err(e) => eprintln!("No se pudo capturar la ventana: {e}"),
            }
            app.quit();
        });
        return;
    }
    if let Some(exit_when_shown) = exit_when_shown {
        if exit_when_shown {
            window.set_persist(false);
        }
        let app = app.clone();
        let mostrada = move |window: Window| {
            medir_arranque::marca("bóveda abierta");
            if exit_when_shown {
                if ya_abierta {
                    // La bóveda salió en el primer cuadro: sus fases son las de «primer cuadro».
                    medir_arranque::marca("cuadro: es el primero");
                } else {
                    window.fases_del_proximo_cuadro("cuadro");
                }
                // Sale después del próximo cuadro: la lista ya está dibujada.
                window.after_next_frame(move || {
                    medir_arranque::marca("cuadro dibujado");
                    if webkit_perezoso::cargado() {
                        medir_arranque::marca("aviso: WebKit se cargó antes del primer cuadro");
                    }
                    app.quit()
                });
            }
        };
        match espera {
            None => mostrada(window),
            Some(espera) => {
                glib::spawn_future_local(async move {
                    espera.await;
                    mostrada(window);
                });
            }
        }
        return;
    }
    reopen_last(window);
}
