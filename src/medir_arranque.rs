//! Medición del arranque por fases (desarrollo): con `ORBITAL_MEDIR_ARRANQUE=1` y
//! `orbital --salir-al-mostrar <carpeta>`, imprime en stderr cuándo termina cada etapa desde el
//! inicio del proceso y las fases del primer cuadro después de abrir la bóveda. Sin la variable no
//! hace nada (una consulta de entorno al arrancar).

use gtk::prelude::*;
use gtk::{gdk, glib};
use std::sync::OnceLock;
use std::time::Instant;

static INICIO: OnceLock<Instant> = OnceLock::new();
static ACTIVA: OnceLock<bool> = OnceLock::new();

fn activa() -> bool {
    *ACTIVA.get_or_init(|| std::env::var_os("ORBITAL_MEDIR_ARRANQUE").is_some())
}

/// Fija el instante cero (al entrar a `main`). Si quien lanza el proceso pasa
/// `ORBITAL_MEDIR_T0_NS` (su `CLOCK_MONOTONIC` en nanosegundos justo antes de lanzarlo), imprime
/// también lo que tardó en llegar a `main` (cargar el binario y enlazar las bibliotecas).
pub fn empezar() {
    if !activa() {
        return;
    }
    INICIO.get_or_init(Instant::now);
    if let Some(t0) = std::env::var("ORBITAL_MEDIR_T0_NS")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
    {
        let mut ts = std::mem::MaybeUninit::<Timespec>::uninit();
        // SAFETY: `clock_gettime` escribe un `timespec` completo en el puntero válido que recibe.
        let ahora = unsafe {
            (clock_gettime(1, ts.as_mut_ptr()) == 0).then(|| {
                let ts = ts.assume_init();
                ts.tv_sec as u64 * 1_000_000_000 + ts.tv_nsec as u64
            })
        };
        if let Some(ahora) = ahora {
            eprintln!(
                "ARRANQUE antes de main: {:.1} ms",
                ahora.saturating_sub(t0) as f64 / 1e6
            );
        }
    }
}

#[repr(C)]
struct Timespec {
    tv_sec: i64,
    tv_nsec: i64,
}

unsafe extern "C" {
    /// `CLOCK_MONOTONIC` es 1 en Linux; `Instant` usa el mismo reloj.
    fn clock_gettime(clock: i32, ts: *mut Timespec) -> i32;
}

fn ms() -> f64 {
    INICIO
        .get()
        .map_or(0.0, |t| t.elapsed().as_secs_f64() * 1000.0)
}

/// Marca el fin de una etapa.
pub fn marca(etapa: &str) {
    if activa() {
        eprintln!("ARRANQUE {etapa}: {:.1} ms", ms());
    }
}

/// Imprime las fases (update, layout, paint) del próximo cuadro de `ventana`, con `etiqueta`
/// delante (`cuadro`, `primer cuadro`…).
pub fn fases_del_proximo_cuadro(ventana: &impl IsA<gtk::Widget>, etiqueta: &'static str) {
    if !activa() {
        return;
    }
    let Some(reloj) = ventana.frame_clock() else {
        return;
    };
    // Solo el próximo cuadro: cada manejador se desconecta después de la primera vez.
    let fase = |nombre: &'static str| {
        let id = std::rc::Rc::new(std::cell::Cell::new(None::<glib::SignalHandlerId>));
        let id2 = id.clone();
        (id, move |r: &gdk::FrameClock| {
            eprintln!("ARRANQUE {etiqueta}/{nombre}: {:.1} ms", ms());
            if let Some(h) = id2.take() {
                r.disconnect(h);
            }
        })
    };
    let (id, f) = fase("update");
    id.set(Some(reloj.connect_update(f)));
    let (id, f) = fase("layout");
    id.set(Some(reloj.connect_layout(f)));
    let (id, f) = fase("paint");
    id.set(Some(reloj.connect_paint(f)));
}
