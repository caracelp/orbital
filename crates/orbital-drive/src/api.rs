//! Lo que el motor necesita de Drive, sin atarse al transporte (el cliente real es
//! [`crate::cliente::Cliente`]; en las pruebas habla con el servidor falso por HTTP).

use std::fmt;
use std::path::Path;

/// Tipo MIME de una carpeta de Drive.
pub const MIME_CARPETA: &str = "application/vnd.google-apps.folder";

/// Campos que se piden de cada archivo (lo justo para sincronizar).
pub const CAMPOS_ARCHIVO: &str = "id,name,parents,mimeType,size,md5Checksum,headRevisionId,trashed,capabilities/canEdit,webViewLink,lastModifyingUser/displayName,modifiedTime";

/// Un archivo o carpeta de Drive.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Archivo {
    pub id: String,
    pub nombre: String,
    pub padres: Vec<String>,
    pub mime: String,
    pub tam: u64,
    pub md5: Option<String>,
    pub rev: Option<String>,
    pub papelera: bool,
    pub editable: bool,
    pub enlace: Option<String>,
    pub autor: Option<String>,
    pub modificado: Option<String>,
}

impl Archivo {
    pub fn carpeta(&self) -> bool {
        self.mime == MIME_CARPETA
    }

    /// Documento nativo de Google (Docs, Sheets…): sin contenido que bajar (061c).
    pub fn nativo(&self) -> bool {
        self.mime.starts_with("application/vnd.google-apps.") && !self.carpeta()
    }

    /// Lee un archivo del JSON de la API.
    pub fn desde_json(v: &serde_json::Value) -> Option<Self> {
        let texto = |k: &str| v.get(k).and_then(|x| x.as_str()).map(str::to_owned);
        Some(Self {
            id: texto("id")?,
            nombre: texto("name").unwrap_or_default(),
            padres: v
                .get("parents")
                .and_then(|p| p.as_array())
                .map(|a| {
                    a.iter()
                        .filter_map(|x| x.as_str().map(str::to_owned))
                        .collect()
                })
                .unwrap_or_default(),
            mime: texto("mimeType").unwrap_or_default(),
            tam: v
                .get("size")
                .and_then(|s| s.as_str())
                .and_then(|s| s.parse().ok())
                .unwrap_or(0),
            md5: texto("md5Checksum"),
            rev: texto("headRevisionId"),
            papelera: v.get("trashed").and_then(|t| t.as_bool()).unwrap_or(false),
            editable: v
                .pointer("/capabilities/canEdit")
                .and_then(|t| t.as_bool())
                .unwrap_or(true),
            enlace: texto("webViewLink"),
            autor: v
                .pointer("/lastModifyingUser/displayName")
                .and_then(|t| t.as_str())
                .map(str::to_owned),
            modificado: texto("modifiedTime"),
        })
    }
}

/// Un cambio de la lista `changes`.
#[derive(Clone, Debug)]
pub struct Cambio {
    pub id: String,
    /// Borrado definitivo o ya sin acceso.
    pub quitado: bool,
    pub archivo: Option<Archivo>,
}

/// Una página de cambios.
#[derive(Clone, Debug, Default)]
pub struct PaginaCambios {
    pub cambios: Vec<Cambio>,
    pub siguiente: Option<String>,
    pub nuevo_inicio: Option<String>,
}

/// Errores de la API, agrupados por lo que el motor hace con ellos.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ErrorApi {
    /// Sin conexión, tiempo agotado o conexión cortada: se reintenta.
    SinRed(String),
    /// La sesión ya no vale (revocada o vencida): se pausa hasta volver a iniciar sesión.
    Autorizacion(String),
    /// Cuota de Drive llena: se pausan las subidas.
    Cuota,
    /// Límite de pedidos (429, 403 `rateLimitExceeded`): espera creciente.
    Limite,
    /// El archivo ya no existe en Drive.
    NoEncontrado,
    /// Error del servidor (5xx) u otro código: espera creciente.
    Servidor(u16, String),
    /// Error local (disco) al subir o bajar.
    Local(String),
    /// La usuaria canceló la bajada (061b): no es un error del motor ni de la red.
    Cancelada,
}

impl ErrorApi {
    /// Si conviene cortar el ciclo y esperar (red, límites, servidor).
    pub fn transitorio(&self) -> bool {
        matches!(
            self,
            Self::SinRed(_) | Self::Limite | Self::Servidor(..) | Self::Autorizacion(_)
        )
    }
}

impl fmt::Display for ErrorApi {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SinRed(m) => write!(f, "sin conexión ({m})"),
            Self::Autorizacion(m) => write!(f, "la sesión de Google ya no es válida ({m})"),
            Self::Cuota => write!(f, "el espacio de Google Drive está lleno"),
            Self::Limite => write!(f, "Google Drive pidió esperar (demasiados pedidos)"),
            Self::NoEncontrado => write!(f, "el archivo ya no está en Drive"),
            Self::Servidor(c, m) => write!(f, "error de Google Drive ({c}: {m})"),
            Self::Local(m) => write!(f, "error en el disco ({m})"),
            Self::Cancelada => write!(f, "se canceló"),
        }
    }
}

impl std::error::Error for ErrorApi {}

pub type Resultado<T> = Result<T, ErrorApi>;

/// Sesión de subida reanudable en curso (sobrevive a un reinicio: va al estado del equipo).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SesionSubida {
    pub url: String,
    pub tam: u64,
    /// MD5 del archivo cuando empezó la subida: si cambió, se empieza de nuevo.
    pub md5: String,
}

/// Espacio de la cuenta de Drive (`about.storageQuota`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Cuota {
    pub usado: u64,
    /// `None`: sin límite (algunas cuentas de Workspace).
    pub limite: Option<u64>,
}

/// La API de Drive tal como la usa el motor.
pub trait Api {
    /// Correo de la cuenta.
    fn cuenta(&mut self) -> Resultado<String>;
    fn token_inicio(&mut self) -> Resultado<String>;
    /// Espacio usado y disponible de la cuenta.
    fn cuota(&mut self) -> Resultado<Cuota>;
    fn cambios(&mut self, token: &str) -> Resultado<PaginaCambios>;
    /// Hijos (no en la papelera) de una carpeta; devuelve la página siguiente si hay.
    fn hijos(
        &mut self,
        carpeta: &str,
        pagina: Option<&str>,
    ) -> Resultado<(Vec<Archivo>, Option<String>)>;
    fn archivo(&mut self, id: &str) -> Resultado<Archivo>;
    fn crear_carpeta(&mut self, padre: &str, nombre: &str) -> Resultado<Archivo>;
    /// Sube un archivo nuevo. `sesion` es la subida reanudable en curso (archivos grandes);
    /// `al_abrir` se llama apenas se abre una sesión nueva, para guardarla en el estado.
    fn subir_nuevo(
        &mut self,
        padre: &str,
        nombre: &str,
        ruta: &Path,
        sesion: &mut Option<SesionSubida>,
        al_abrir: &mut dyn FnMut(&SesionSubida),
    ) -> Resultado<Archivo>;
    /// Reemplaza el contenido de un archivo existente (mismo id).
    fn subir_cambio(
        &mut self,
        id: &str,
        ruta: &Path,
        sesion: &mut Option<SesionSubida>,
        al_abrir: &mut dyn FnMut(&SesionSubida),
    ) -> Resultado<Archivo>;
    /// Baja el contenido a `destino`; si `destino` ya tiene un tramo, sigue desde ahí.
    /// `progreso` recibe los bytes que ya tiene `destino` después de cada tramo leído; si
    /// devuelve `false`, la bajada se corta con [`ErrorApi::Cancelada`] (lo bajado queda en
    /// `destino`: lo borra quien llama).
    fn bajar(
        &mut self,
        id: &str,
        destino: &Path,
        progreso: &mut dyn FnMut(u64) -> bool,
    ) -> Resultado<()>;
    /// Renombra o mueve (el id no cambia).
    fn actualizar(
        &mut self,
        id: &str,
        nombre: Option<&str>,
        agregar_padre: Option<&str>,
        quitar_padre: Option<&str>,
    ) -> Resultado<Archivo>;
    /// Manda a la papelera de Drive (nunca borra).
    fn a_papelera(&mut self, id: &str) -> Resultado<()>;
}

impl<T: Api + ?Sized> Api for Box<T> {
    fn cuenta(&mut self) -> Resultado<String> {
        (**self).cuenta()
    }
    fn token_inicio(&mut self) -> Resultado<String> {
        (**self).token_inicio()
    }
    fn cuota(&mut self) -> Resultado<Cuota> {
        (**self).cuota()
    }
    fn cambios(&mut self, token: &str) -> Resultado<PaginaCambios> {
        (**self).cambios(token)
    }
    fn hijos(
        &mut self,
        carpeta: &str,
        pagina: Option<&str>,
    ) -> Resultado<(Vec<Archivo>, Option<String>)> {
        (**self).hijos(carpeta, pagina)
    }
    fn archivo(&mut self, id: &str) -> Resultado<Archivo> {
        (**self).archivo(id)
    }
    fn crear_carpeta(&mut self, padre: &str, nombre: &str) -> Resultado<Archivo> {
        (**self).crear_carpeta(padre, nombre)
    }
    fn subir_nuevo(
        &mut self,
        padre: &str,
        nombre: &str,
        ruta: &Path,
        sesion: &mut Option<SesionSubida>,
        al_abrir: &mut dyn FnMut(&SesionSubida),
    ) -> Resultado<Archivo> {
        (**self).subir_nuevo(padre, nombre, ruta, sesion, al_abrir)
    }
    fn subir_cambio(
        &mut self,
        id: &str,
        ruta: &Path,
        sesion: &mut Option<SesionSubida>,
        al_abrir: &mut dyn FnMut(&SesionSubida),
    ) -> Resultado<Archivo> {
        (**self).subir_cambio(id, ruta, sesion, al_abrir)
    }
    fn bajar(
        &mut self,
        id: &str,
        destino: &Path,
        progreso: &mut dyn FnMut(u64) -> bool,
    ) -> Resultado<()> {
        (**self).bajar(id, destino, progreso)
    }
    fn actualizar(
        &mut self,
        id: &str,
        nombre: Option<&str>,
        agregar_padre: Option<&str>,
        quitar_padre: Option<&str>,
    ) -> Resultado<Archivo> {
        (**self).actualizar(id, nombre, agregar_padre, quitar_padre)
    }
    fn a_papelera(&mut self, id: &str) -> Resultado<()> {
        (**self).a_papelera(id)
    }
}
