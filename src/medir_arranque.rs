//! Medición del arranque por fases (desarrollo): con `ORBITAL_MEDIR_ARRANQUE=1` y
//! `orbital --salir-al-mostrar <carpeta>`, imprime en stderr cuándo termina cada etapa desde el
//! inicio del proceso y las fases del primer cuadro después de abrir la bóveda. Sin la variable no
//! hace nada (una consulta de entorno al arrancar).

use gtk::gdk;
use gtk::prelude::*;
use std::sync::OnceLock;
use std::time::Instant;

static INICIO: OnceLock<Instant> = OnceLock::new();
static ACTIVA: OnceLock<bool> = OnceLock::new();

fn activa() -> bool {
    *ACTIVA.get_or_init(|| std::env::var_os("ORBITAL_MEDIR_ARRANQUE").is_some())
}

/// Fija el instante cero (al entrar a `main`).
pub fn empezar() {
    if activa() {
        INICIO.get_or_init(Instant::now);
    }
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

/// Imprime las fases (update, layout, paint) del próximo cuadro de `ventana`.
pub fn fases_del_proximo_cuadro(ventana: &impl IsA<gtk::Widget>) {
    if !activa() {
        return;
    }
    let Some(reloj) = ventana.frame_clock() else {
        return;
    };
    let fase = |nombre: &'static str| {
        move |_: &gdk::FrameClock| eprintln!("ARRANQUE cuadro/{nombre}: {:.1} ms", ms())
    };
    reloj.connect_update(fase("update"));
    reloj.connect_layout(fase("layout"));
    reloj.connect_paint(fase("paint"));
}
