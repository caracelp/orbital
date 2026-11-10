//! Contexto de escritura de la app: fecha, hora y actor (FR-015, research R10).

use gtk::glib;
use orbital_vault::{Actor, Date, Offset, Time, VaultConfig, WriteCtx};

/// Arma el `WriteCtx` con la hora local actual. El actor sale de la configuración de la bóveda
/// o, si no está, del usuario del sistema (`human:<usuario>`).
pub fn now(config: Option<&VaultConfig>) -> WriteCtx {
    let actor = config
        .and_then(|c| c.actor.clone())
        .map(Actor)
        .unwrap_or_else(|| Actor::human(&glib::user_name().to_string_lossy()));
    match glib::DateTime::now_local() {
        Ok(dt) => WriteCtx {
            date: Date {
                year: dt.year(),
                month: dt.month() as u8,
                day: dt.day_of_month() as u8,
            },
            time: Time {
                hour: dt.hour() as u8,
                minute: dt.minute() as u8,
                second: dt.second() as u8,
            },
            utc_offset: Offset {
                minutes: (dt.utc_offset().as_minutes()) as i32,
            },
            actor,
        },
        // Sin zona horaria local (muy raro): se usa la fecha de época en UTC para no fallar.
        Err(_) => WriteCtx {
            date: Date {
                year: 1970,
                month: 1,
                day: 1,
            },
            time: Time {
                hour: 0,
                minute: 0,
                second: 0,
            },
            utc_offset: Offset { minutes: 0 },
            actor,
        },
    }
}
