//! Progreso de una bajada de lo «solo en Drive» (061b): el porcentaje, los bytes, el tiempo que
//! falta cuando se puede estimar con confianza y qué anunciarle al lector de pantalla (cada 25 %,
//! no en cada tramo). Sin GTK: lo usan la página de la nota y el diálogo de «Abrir con».

use std::time::{Duration, Instant};

/// Lo que se muestra en un momento de la bajada.
#[derive(Clone, Debug, PartialEq)]
pub struct Lectura {
    /// De 0 a 1.
    pub fraccion: f64,
    /// `45 %`.
    pub porcentaje: String,
    /// `12,3 MB de 27,1 MB · faltan unos 20 s` (el tiempo, solo si es fiable).
    pub detalle: String,
    /// Lo que se le anuncia al lector de pantalla, si se cruzó otro cuarto.
    pub anuncio: Option<String>,
}

/// Sigue una bajada: estima la velocidad con un promedio que olvida de a poco (los primeros
/// tramos, con la conexión arrancando, no cuentan) y recuerda el último cuarto anunciado.
#[derive(Debug)]
pub struct Medidor {
    nombre: String,
    inicio: Option<(Instant, u64)>,
    ultimo: Option<(Instant, u64)>,
    velocidad: Option<f64>,
    cuarto: u8,
}

/// Antes de esto no se muestra el tiempo que falta (la velocidad todavía no es estable).
const ESTABLE: Duration = Duration::from_millis(1500);

impl Medidor {
    pub fn new(nombre: &str) -> Self {
        Self {
            nombre: nombre.to_owned(),
            inicio: None,
            ultimo: None,
            velocidad: None,
            cuarto: 0,
        }
    }

    pub fn avanzar(&mut self, ahora: Instant, bajado: u64, total: u64) -> Lectura {
        let bajado = bajado.min(total);
        let inicio = *self.inicio.get_or_insert((ahora, bajado));
        if let Some((t, b)) = self.ultimo {
            let dt = ahora.saturating_duration_since(t).as_secs_f64();
            if dt >= 0.2 && bajado >= b {
                let v = (bajado - b) as f64 / dt;
                self.velocidad = Some(match self.velocidad {
                    Some(x) => x * 0.7 + v * 0.3,
                    None => v,
                });
                self.ultimo = Some((ahora, bajado));
            }
        } else {
            self.ultimo = Some((ahora, bajado));
        }
        let fraccion = if total == 0 {
            1.0
        } else {
            bajado as f64 / total as f64
        };
        let pct = (fraccion * 100.0).floor() as u64;
        let mut detalle = format!("{} de {}", tamano(bajado), tamano(total));
        let estable = ahora.saturating_duration_since(inicio.0) >= ESTABLE;
        if let Some(v) = self.velocidad.filter(|v| *v > 1.0 && estable)
            && bajado < total
        {
            let falta = (total - bajado) as f64 / v;
            detalle.push_str(" · ");
            detalle.push_str(&falta_texto(falta));
        }
        let cuarto = (fraccion * 4.0).floor().min(4.0) as u8;
        let anuncio = (cuarto > self.cuarto && cuarto < 4).then(|| {
            self.cuarto = cuarto;
            format!("Bajando «{}»: {} %", self.nombre, cuarto as u32 * 25)
        });
        Lectura {
            fraccion,
            porcentaje: format!("{pct} %"),
            detalle,
            anuncio,
        }
    }
}

/// `faltan unos 20 s`, `falta menos de 10 s`, `faltan unos 3 min`.
fn falta_texto(s: f64) -> String {
    if s < 10.0 {
        "falta menos de 10 s".into()
    } else if s < 60.0 {
        format!("faltan unos {} s", ((s / 5.0).round() as u64 * 5).max(10))
    } else if s < 3600.0 {
        format!("faltan unos {} min", (s / 60.0).round() as u64)
    } else {
        format!("faltan unas {} h", (s / 3600.0).round() as u64)
    }
}

/// `850 KB`, `12,3 MB`, `1,2 GB` (con coma decimal).
pub fn tamano(b: u64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = KB * 1024.0;
    const GB: f64 = MB * 1024.0;
    let x = b as f64;
    let t = if x >= GB {
        format!("{:.1} GB", x / GB)
    } else if x >= MB {
        format!("{:.1} MB", x / MB)
    } else {
        format!("{:.0} KB", (x / KB).ceil())
    };
    t.replace('.', ",")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tamanos_y_tiempos() {
        assert_eq!(tamano(0), "0 KB");
        assert_eq!(tamano(850 * 1024), "850 KB");
        assert_eq!(tamano(12_900_000), "12,3 MB");
        assert_eq!(tamano(3 << 30), "3,0 GB");
        assert_eq!(falta_texto(4.0), "falta menos de 10 s");
        assert_eq!(falta_texto(22.0), "faltan unos 20 s");
        assert_eq!(falta_texto(200.0), "faltan unos 3 min");
    }

    #[test]
    fn el_tiempo_solo_cuando_es_estable_y_anuncia_cada_cuarto() {
        let t0 = Instant::now();
        let mut m = Medidor::new("video.mp4");
        let total = 100 << 20;
        let l = m.avanzar(t0, 0, total);
        assert_eq!(l.porcentaje, "0 %");
        assert!(!l.detalle.contains("falta"), "{}", l.detalle);
        assert_eq!(l.anuncio, None);
        // 1 MB por segundo, durante un segundo: todavía no es fiable.
        let l = m.avanzar(t0 + Duration::from_secs(1), 1 << 20, total);
        assert!(!l.detalle.contains("falta"));
        // Más de 1,5 s a ritmo parejo: ya se muestra (≈ 97 s → «unos 2 min»).
        let l = m.avanzar(t0 + Duration::from_secs(3), 3 << 20, total);
        assert!(l.detalle.contains("faltan unos 2 min"), "{}", l.detalle);
        // Cruza el 25 %: un anuncio, una sola vez.
        let l = m.avanzar(t0 + Duration::from_secs(26), 26 << 20, total);
        assert_eq!(l.anuncio.as_deref(), Some("Bajando «video.mp4»: 25 %"));
        let l = m.avanzar(t0 + Duration::from_secs(27), 27 << 20, total);
        assert_eq!(l.anuncio, None);
        // Un salto grande: se anuncia el cuarto al que llegó, no los intermedios.
        let l = m.avanzar(t0 + Duration::from_secs(30), 80 << 20, total);
        assert_eq!(l.anuncio.as_deref(), Some("Bajando «video.mp4»: 75 %"));
        // Al terminar no se anuncia el 100 % (se anuncia lo que se abre).
        let l = m.avanzar(t0 + Duration::from_secs(40), total, total);
        assert_eq!((l.porcentaje.as_str(), l.anuncio), ("100 %", None));
        assert!(!l.detalle.contains("falta"));
    }
}
