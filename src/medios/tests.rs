use super::*;
use std::rc::Rc;
use std::time::{Duration, Instant};
use webkit6::prelude::WebViewExt;

#[test]
fn tiempo_legible() {
    assert_eq!(tiempo(0.0), "0:00");
    assert_eq!(tiempo(65.4), "1:05");
    assert_eq!(tiempo(3723.0), "1:02:03");
    assert_eq!(tiempo(-3.0), "0:00");
}

/// RSS en KiB de un proceso (`self` o un pid).
fn rss(pid: &str) -> u64 {
    std::fs::read_to_string(format!("/proc/{pid}/status"))
        .ok()
        .and_then(|s| {
            s.lines()
                .find_map(|l| l.strip_prefix("VmRSS:"))
                .and_then(|v| v.split_whitespace().next()?.parse().ok())
        })
        .unwrap_or(0)
}

/// RSS de los procesos hijos (los de WebKit).
fn rss_hijos() -> (u64, usize) {
    let yo = std::process::id().to_string();
    let mut total = 0;
    let mut n = 0;
    for e in std::fs::read_dir("/proc").into_iter().flatten().flatten() {
        let pid = e.file_name().to_string_lossy().into_owned();
        if !pid.bytes().all(|b| b.is_ascii_digit()) {
            continue;
        }
        let Ok(st) = std::fs::read_to_string(format!("/proc/{pid}/status")) else {
            continue;
        };
        if st
            .lines()
            .any(|l| l.strip_prefix("PPid:").map(str::trim) == Some(yo.as_str()))
        {
            total += rss(&pid);
            n += 1;
        }
    }
    (total, n)
}

/// Compara los dos reproductores posibles para la Lectura (057), cada uno en frío en su propio
/// proceso: `ORBITAL_MEDIR_MEDIOS=gtk|webkit`, `ORBITAL_MEDIR_ARCHIVO=<audio o video>`. Mide
/// desde que se pide hasta que el medio está listo para reproducir (metadatos leídos) y la
/// memoria que suma.
#[test]
#[ignore = "medición de los reproductores (057)"]
fn medir_medios() {
    let modo = std::env::var("ORBITAL_MEDIR_MEDIOS").unwrap_or_else(|_| "gtk".into());
    let archivo =
        PathBuf::from(std::env::var("ORBITAL_MEDIR_ARCHIVO").expect("ORBITAL_MEDIR_ARCHIVO"));
    gtk::init().expect("se necesita una sesión GTK");
    let ctx = glib::MainContext::default();
    // Una ventana ya dibujada antes de medir: su primer cuadro no cuenta.
    let p = gtk::Picture::new();
    let ventana = gtk::Window::builder()
        .child(&p)
        .default_width(700)
        .default_height(450)
        .build();
    ventana.present();
    ctx.block_on(glib::timeout_future(Duration::from_millis(800)));
    // El hilo de GTK: el mayor hueco entre dos vueltas de un reloj de 1 ms.
    let hueco = Rc::new(std::cell::Cell::new(Duration::ZERO));
    let ultimo = Rc::new(std::cell::Cell::new(Instant::now()));
    {
        let (hueco, ultimo) = (hueco.clone(), ultimo.clone());
        glib::timeout_add_local(Duration::from_millis(1), move || {
            let d = ultimo.get().elapsed();
            hueco.set(hueco.get().max(d));
            ultimo.set(Instant::now());
            glib::ControlFlow::Continue
        });
    }
    ultimo.set(Instant::now());
    let antes = rss("self");
    let t0 = Instant::now();
    let listo = ctx.block_on(async {
        match modo.as_str() {
            "flujo" => {
                let video = archivo.extension().is_some_and(|e| e == "webm" || e == "mp4");
                let t0 = Instant::now();
                let m = Flujo::abrir(&archivo, video.then_some(&p), None, false);
                let creado = t0.elapsed();
                while !m.is_prepared() && m.error().is_none() {
                    glib::timeout_future(Duration::from_millis(1)).await;
                }
                assert!(m.error().is_none(), "{:?}", m.error());
                let listo = t0.elapsed();
                m.play();
                let t1 = Instant::now();
                while m.timestamp() < 300_000 && t1.elapsed() < Duration::from_secs(5) {
                    glib::timeout_future(Duration::from_millis(5)).await;
                }
                println!(
                    "flujo: crear {:.1} ms, listo {:.1} ms, primer 0,3 s reproducido {:.1} ms después",
                    creado.as_secs_f64() * 1e3,
                    listo.as_secs_f64() * 1e3,
                    t1.elapsed().as_secs_f64() * 1e3 - 300.0
                );
                if std::env::var_os("ORBITAL_MEDIR_DOS").is_some() {
                    let p2 = gtk::Picture::new();
                    let t2 = Instant::now();
                    let m2 = Flujo::abrir(&archivo, video.then_some(&p2), None, false);
                    while !m2.is_prepared() && m2.error().is_none() {
                        glib::timeout_future(Duration::from_millis(1)).await;
                    }
                    println!("flujo: segundo listo {:.1} ms", t2.elapsed().as_secs_f64() * 1e3);
                    std::mem::forget(m2);
                }
                std::mem::forget(m);
                listo
            }
            "gtk" => {
                let m = gtk::MediaFile::for_filename(&archivo);
                let creado = t0.elapsed();
                while !m.is_prepared() && m.error().is_none() {
                    glib::timeout_future(Duration::from_millis(1)).await;
                }
                assert!(m.error().is_none(), "{:?}", m.error());
                let listo = t0.elapsed();
                // Que reproduzca de verdad: 300 ms de avance.
                m.play();
                let t1 = Instant::now();
                while m.timestamp() < 300_000 && t1.elapsed() < Duration::from_secs(5) {
                    glib::timeout_future(Duration::from_millis(5)).await;
                }
                println!(
                    "gtk: crear {:.1} ms, listo {:.1} ms, primer 0,3 s reproducido {:.1} ms después",
                    creado.as_secs_f64() * 1e3,
                    listo.as_secs_f64() * 1e3,
                    t1.elapsed().as_secs_f64() * 1e3 - 300.0
                );
                std::mem::forget(m);
                listo
            }
            _ => {
                let settings = webkit6::Settings::new();
                settings.set_enable_javascript(true);
                let session = webkit6::NetworkSession::new_ephemeral();
                let web = webkit6::WebView::builder()
                    .settings(&settings)
                    .network_session(&session)
                    .build();
                ventana.set_child(Some(&web));
                let creado = t0.elapsed();
                let uri = gio::File::for_path(&archivo).uri();
                let etiqueta = if archivo.extension().is_some_and(|e| e == "mp3" || e == "ogg") {
                    "audio"
                } else {
                    "video"
                };
                web.load_html(
                    &format!("<{etiqueta} controls preload=\"metadata\" src=\"{uri}\"></{etiqueta}>"),
                    Some(&(gio::File::for_path(archivo.parent().unwrap()).uri().to_string() + "/")),
                );
                loop {
                    let r = web
                        .evaluate_javascript_future(
                            &format!("document.querySelector('{etiqueta}')?.readyState ?? -1"),
                            None,
                            None,
                        )
                        .await
                        .map(|v| v.to_int32())
                        .unwrap_or(-1);
                    if r >= 1 || t0.elapsed() > Duration::from_secs(20) {
                        let estado = web
                            .evaluate_javascript_future(
                                &format!("(e => e ? e.readyState + ' ' + e.networkState + ' ' + (e.error && e.error.message) + ' mp3:' + e.canPlayType('audio/mpeg') + ' webm:' + e.canPlayType('video/webm') + ' src:' + e.currentSrc + ' loc:' + location.href : 'sin elemento')(document.querySelector('{etiqueta}'))"),
                                None,
                                None,
                            )
                            .await
                            .map(|v| v.to_str().to_string());
                        println!("webkit: readyState {r}, estado {estado:?}");
                        break;
                    }
                    glib::timeout_future(Duration::from_millis(2)).await;
                }
                let listo = t0.elapsed();
                println!(
                    "webkit: crear {:.1} ms, listo {:.1} ms",
                    creado.as_secs_f64() * 1e3,
                    listo.as_secs_f64() * 1e3
                );
                std::mem::forget(web);
                listo
            }
        }
    });
    ctx.block_on(glib::timeout_future(Duration::from_millis(300)));
    let (hijos, n) = rss_hijos();
    println!(
        "MEDIDA {modo} listo_ms={:.1} bloqueo_max_ms={:.1} rss_propio_kib={} rss_hijos_kib={hijos} procesos_hijos={n}",
        listo.as_secs_f64() * 1e3,
        hueco.get().as_secs_f64() * 1e3,
        rss("self").saturating_sub(antes)
    );
}

/// El reproductor no abre nada hasta que se pide: recién al tocar «Reproducir» aparecen los
/// controles y el medio queda listo; con `#t=` arranca desde ahí. Al sacarlo de la ventana se
/// pausa.
#[test]
#[ignore = "requiere una sesión GTK (tools/pruebas-gtk.sh)"]
fn reproductor_carga_al_pedirlo() {
    gtk::init().expect("se necesita una sesión GTK");
    let dir = std::env::temp_dir().join(format!("orbital-057-ñandú-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let wav = dir.join("tono ñ.wav");
    std::fs::write(&wav, wav_de_prueba(3.0)).unwrap();
    let ctx = glib::MainContext::default();
    ctx.block_on(async {
        let w = reproductor(&wav, Tipo::Audio, Some(1.5), false);
        let ventana = gtk::Window::builder().child(&w).build();
        ventana.present();
        let botones = botones(&w);
        // Antes: solo «Reproducir» y «Abrir con otra aplicación»; ningún control de medio.
        assert!(controles(&w).is_none());
        let play = botones
            .iter()
            .find(|b| b.tooltip_text().as_deref() == Some("Reproducir"))
            .expect("botón Reproducir")
            .clone();
        assert!(
            botones
                .iter()
                .any(|b| b.tooltip_text().as_deref() == Some("Abrir con otra aplicación"))
        );
        play.emit_clicked();
        let c = controles(&w).expect("controles después de reproducir");
        let m = c
            .media_stream()
            .expect("controles con su medio")
            .downcast::<Flujo>()
            .unwrap();
        for _ in 0..400 {
            if m.is_prepared() && m.timestamp() >= 1_500_000 {
                break;
            }
            assert!(m.error().is_none(), "{:?}", m.error());
            glib::timeout_future(Duration::from_millis(10)).await;
        }
        assert!(m.is_prepared(), "el medio no quedó listo");
        assert!(
            m.timestamp() >= 1_500_000,
            "no saltó al inicio #t= (va en {} µs)",
            m.timestamp()
        );
        assert!(m.is_playing());
        ventana.set_child(None::<&gtk::Widget>);
        assert!(!m.is_playing(), "sigue sonando fuera de la vista");
        ventana.destroy();
    });
    std::fs::remove_dir_all(&dir).ok();
}

/// Un archivo que GStreamer no entiende: aviso en el lugar de los controles, y queda el botón para
/// abrirlo afuera.
#[test]
#[ignore = "requiere una sesión GTK (tools/pruebas-gtk.sh)"]
fn reproductor_con_archivo_ilegible_avisa() {
    gtk::init().expect("se necesita una sesión GTK");
    let dir = std::env::temp_dir().join(format!("orbital-057-roto-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let roto = dir.join("roto.mp4");
    std::fs::write(&roto, b"esto no es un video").unwrap();
    let ctx = glib::MainContext::default();
    ctx.block_on(async {
        let w = reproductor(&roto, Tipo::Video, None, false);
        let ventana = gtk::Window::builder().child(&w).build();
        ventana.present();
        let play = botones(&w)
            .into_iter()
            .find(|b| b.tooltip_text().as_deref() == Some("Reproducir"))
            .unwrap();
        play.emit_clicked();
        let mut aviso = None;
        for _ in 0..300 {
            aviso = etiquetas(&w)
                .into_iter()
                .find(|l| l.label().starts_with("No se puede reproducir"));
            if aviso.is_some() {
                break;
            }
            glib::timeout_future(Duration::from_millis(10)).await;
        }
        assert!(aviso.is_some(), "sin aviso de error");
        assert!(
            botones(&w)
                .iter()
                .any(|b| b.tooltip_text().as_deref() == Some("Abrir con otra aplicación"))
        );
        ventana.destroy();
    });
    std::fs::remove_dir_all(&dir).ok();
}

fn descendientes(w: &gtk::Widget, out: &mut Vec<gtk::Widget>) {
    let mut c = w.first_child();
    while let Some(h) = c {
        out.push(h.clone());
        descendientes(&h, out);
        c = h.next_sibling();
    }
}

fn botones(w: &gtk::Widget) -> Vec<gtk::Button> {
    let mut v = Vec::new();
    descendientes(w, &mut v);
    v.into_iter().filter_map(|w| w.downcast().ok()).collect()
}

fn etiquetas(w: &gtk::Widget) -> Vec<gtk::Label> {
    let mut v = Vec::new();
    descendientes(w, &mut v);
    v.into_iter().filter_map(|w| w.downcast().ok()).collect()
}

fn controles(w: &gtk::Widget) -> Option<gtk::MediaControls> {
    let mut v = Vec::new();
    descendientes(w, &mut v);
    v.into_iter().find_map(|w| w.downcast().ok())
}

/// Un WAV PCM mono de 8 kHz con un tono, para no depender de archivos de prueba.
pub(crate) fn wav_de_prueba(segundos: f64) -> Vec<u8> {
    let tasa = 8000u32;
    let n = (f64::from(tasa) * segundos) as u32;
    let mut out = Vec::with_capacity(44 + n as usize * 2);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + n * 2).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&tasa.to_le_bytes());
    out.extend_from_slice(&(tasa * 2).to_le_bytes());
    out.extend_from_slice(&2u16.to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&(n * 2).to_le_bytes());
    for i in 0..n {
        let v = (f64::from(i) * 440.0 * std::f64::consts::TAU / f64::from(tasa)).sin() * 8000.0;
        out.extend_from_slice(&(v as i16).to_le_bytes());
    }
    out
}
