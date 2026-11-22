//! Sincronización de la bóveda con Google Drive (spec 061a: espejo con Drive).
//!
//! - [`motor`]: el motor de sincronización, que corre en su propio hilo y es el único que usa la red.
//! - [`cliente`]: la API REST de Drive v3 con libsoup 3 (síncrona, en el hilo del motor).
//! - [`oauth`]: inicio de sesión para aplicaciones de escritorio con PKCE y redirección a 127.0.0.1.
//! - [`llavero`]: credenciales en el llavero del sistema (Secret Service, por libsecret).
//! - [`reglas`]: `.orbital/drive.yaml` (conexión y reglas «solo en este equipo»).
//! - [`estado`]: el estado de cada equipo, fuera de la bóveda.
//! - [`fusion`]: fusión de tres vías de texto cuando los cambios no se pisan.
//! - `falso` (función `servidor-falso`): servidor HTTP local que imita la API, para las pruebas.
//!
//! Nada de esto se carga si la bóveda no está conectada.

pub mod api;
pub mod cliente;
pub mod estado;
#[cfg(feature = "servidor-falso")]
pub mod falso;
pub mod fusion;
pub mod llavero;
pub mod local;
pub mod motor;
pub mod oauth;
pub mod reglas;

pub use api::{Api, Archivo, ErrorApi};
pub use estado::Estado;
pub use motor::{EstadoArchivo, Evento, InfoConflicto, Motor, Orden, Papelera, Resumen};
pub use reglas::Conexion;

/// Ruta relativa a la bóveda de `.orbital/drive.yaml`.
pub const ARCHIVO_CONEXION: &str = ".orbital/drive.yaml";

/// Si la bóveda en `root` está conectada a Drive (solo mira si existe el archivo de conexión).
pub fn conectada(root: &std::path::Path) -> bool {
    root.join(ARCHIVO_CONEXION).is_file()
}

/// La API real para el motor de este equipo: credenciales del llavero y cliente de libsoup.
/// Se llama **dentro** del hilo del motor.
pub fn api_del_equipo(estado: &Estado) -> Result<Box<dyn Api>, String> {
    let l = llavero::del_entorno();
    let c = llavero::leer_cliente(&*l)?
        .ok_or("faltan el id y el secreto del cliente de Google: conectá la bóveda otra vez")?;
    let r = llavero::leer_cuenta(&*l, &estado.cuenta)?.ok_or_else(|| {
        format!(
            "falta la sesión de Google de {}: iniciá sesión otra vez",
            estado.cuenta
        )
    })?;
    Ok(Box::new(cliente::Cliente::new(
        cliente::Direcciones::del_entorno(),
        cliente::Credenciales {
            client_id: c.id,
            client_secret: c.secreto,
            refresh_token: r,
        },
    )))
}
