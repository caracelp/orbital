//! Cliente de la API REST de Drive v3 con libsoup 3, síncrono.
//!
//! Se crea y se usa **en el hilo del motor** (una `soup::Session` no se pasa entre hilos). TLS,
//! certificados y proxy son los del sistema (glib-networking). Medido en la investigación: +30 KB
//! de binario y ≈ 0 ms de CPU por pedido con la conexión abierta.
//!
//! `ORBITAL_DRIVE_API=http://127.0.0.1:<puerto>` apunta todo (API, subida y token) al servidor
//! falso de las pruebas.

use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;
use std::time::{Duration, Instant};

use gio::prelude::*;
use soup::prelude::*;

use crate::api::{
    Api, Archivo, CAMPOS_ARCHIVO, Cambio, ErrorApi, MIME_CARPETA, PaginaCambios, Resultado,
    SesionSubida,
};

/// Hasta este tamaño la subida es de un solo pedido (multiparte); más grande, reanudable.
pub const LIMITE_SIMPLE: u64 = 5 * 1024 * 1024;
/// Tramo de las subidas reanudables (múltiplo de 256 KiB, como pide Drive).
pub const TRAMO: usize = 8 * 1024 * 1024;

const BASE_GOOGLE: &str = "https://www.googleapis.com";
const TOKEN_GOOGLE: &str = "https://oauth2.googleapis.com/token";

/// Credenciales para pedir tokens de acceso.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Credenciales {
    pub client_id: String,
    pub client_secret: String,
    pub refresh_token: String,
}

/// Direcciones de la API (las de Google, o las del servidor falso).
#[derive(Clone, Debug)]
pub struct Direcciones {
    pub base: String,
    pub token: String,
}

impl Direcciones {
    /// Las de Google, salvo que `ORBITAL_DRIVE_API` diga otra cosa.
    pub fn del_entorno() -> Self {
        match std::env::var("ORBITAL_DRIVE_API") {
            Ok(base) if !base.is_empty() => Self::locales(&base),
            _ => Self {
                base: BASE_GOOGLE.to_owned(),
                token: TOKEN_GOOGLE.to_owned(),
            },
        }
    }

    /// Todo contra un servidor (el falso de las pruebas).
    pub fn locales(base: &str) -> Self {
        let base = base.trim_end_matches('/').to_owned();
        Self {
            token: format!("{base}/token"),
            base,
        }
    }
}

/// Escapa un valor para la URL.
pub fn escapar(s: &str) -> String {
    gio::glib::Uri::escape_string(s, None, false).to_string()
}

/// Respuesta de un pedido.
struct Respuesta {
    estado: u32,
    cuerpo: Vec<u8>,
    ubicacion: Option<String>,
    rango: Option<String>,
}

/// Cliente de Drive. Guarda el token de acceso en memoria (nunca en disco).
pub struct Cliente {
    sesion: soup::Session,
    dir: Direcciones,
    cred: Credenciales,
    acceso: Option<(String, Instant)>,
    /// Tramo de subida (8 MiB; las pruebas lo achican para cortar a mitad).
    pub tramo: usize,
}

impl Cliente {
    pub fn new(dir: Direcciones, cred: Credenciales) -> Self {
        let sesion = soup::Session::new();
        sesion.set_timeout(60);
        sesion.set_idle_timeout(60);
        sesion.set_user_agent(concat!("Orbital/", env!("CARGO_PKG_VERSION")));
        Self {
            sesion,
            dir,
            cred,
            acceso: None,
            tramo: TRAMO,
        }
    }

    /// Pide un token de acceso con el de renovación.
    fn renovar(&mut self) -> Resultado<String> {
        let cuerpo = format!(
            "grant_type=refresh_token&refresh_token={}&client_id={}&client_secret={}",
            escapar(&self.cred.refresh_token),
            escapar(&self.cred.client_id),
            escapar(&self.cred.client_secret)
        );
        let r = self.crudo(
            "POST",
            &self.dir.token.clone(),
            &[],
            Some(("application/x-www-form-urlencoded", cuerpo.into_bytes())),
            false,
        )?;
        let v: serde_json::Value = serde_json::from_slice(&r.cuerpo).unwrap_or_default();
        if r.estado != 200 {
            let motivo = v
                .get("error")
                .and_then(|e| e.as_str())
                .unwrap_or("sin detalle")
                .to_owned();
            return Err(if r.estado == 400 || r.estado == 401 {
                ErrorApi::Autorizacion(motivo)
            } else {
                ErrorApi::Servidor(r.estado as u16, motivo)
            });
        }
        let token = v
            .get("access_token")
            .and_then(|t| t.as_str())
            .ok_or_else(|| ErrorApi::Autorizacion("respuesta sin token".into()))?
            .to_owned();
        let dura = v.get("expires_in").and_then(|e| e.as_u64()).unwrap_or(3600);
        // Un minuto de margen.
        let vence = Instant::now() + Duration::from_secs(dura.saturating_sub(60));
        self.acceso = Some((token.clone(), vence));
        Ok(token)
    }

    fn token(&mut self) -> Resultado<String> {
        match &self.acceso {
            Some((t, vence)) if Instant::now() < *vence => Ok(t.clone()),
            _ => self.renovar(),
        }
    }

    /// Un pedido sin reintentos. `autorizado` agrega el token de acceso.
    fn crudo(
        &mut self,
        metodo: &str,
        url: &str,
        cabeceras: &[(&str, String)],
        cuerpo: Option<(&str, Vec<u8>)>,
        autorizado: bool,
    ) -> Resultado<Respuesta> {
        let msg = soup::Message::new(metodo, url)
            .map_err(|e| ErrorApi::Servidor(0, format!("dirección inválida: {e}")))?;
        let h = msg.request_headers().expect("cabeceras del pedido");
        if autorizado {
            let t = self.token()?;
            h.append("Authorization", &format!("Bearer {t}"));
        }
        for (k, v) in cabeceras {
            h.append(k, v);
        }
        match cuerpo {
            Some((tipo, bytes)) => msg.set_request_body_from_bytes(
                Some(tipo),
                Some(&gio::glib::Bytes::from_owned(bytes)),
            ),
            None if matches!(metodo, "POST" | "PUT" | "PATCH") => {
                msg.set_request_body_from_bytes(None, Some(&gio::glib::Bytes::from_static(b"")))
            }
            None => {}
        }
        let bytes = self
            .sesion
            .send_and_read(&msg, gio::Cancellable::NONE)
            .map_err(|e| ErrorApi::SinRed(e.message().to_owned()))?;
        let rh = msg.response_headers();
        Ok(Respuesta {
            estado: msg.status_code(),
            cuerpo: bytes.to_vec(),
            ubicacion: rh
                .as_ref()
                .and_then(|h| h.one("Location"))
                .map(|s| s.to_string()),
            rango: rh
                .as_ref()
                .and_then(|h| h.one("Range"))
                .map(|s| s.to_string()),
        })
    }

    /// Un pedido autorizado: con 401 renueva el token una vez y repite.
    fn pedir(
        &mut self,
        metodo: &str,
        url: &str,
        cabeceras: &[(&str, String)],
        cuerpo: Option<(&str, Vec<u8>)>,
    ) -> Resultado<Respuesta> {
        let r = self.crudo(metodo, url, cabeceras, cuerpo.clone(), true)?;
        if r.estado == 401 {
            self.acceso = None;
            let r2 = self.crudo(metodo, url, cabeceras, cuerpo, true)?;
            if r2.estado == 401 {
                return Err(ErrorApi::Autorizacion("401".into()));
            }
            return Ok(r2);
        }
        Ok(r)
    }

    /// Pedido que tiene que devolver JSON con 200.
    fn json(
        &mut self,
        metodo: &str,
        url: &str,
        cuerpo: Option<(&str, Vec<u8>)>,
    ) -> Resultado<serde_json::Value> {
        let r = self.pedir(metodo, url, &[], cuerpo)?;
        respuesta_json(&r)
    }

    fn url(&self, ruta: &str) -> String {
        format!("{}{}", self.dir.base, ruta)
    }

    /// Subida multiparte (metadatos y contenido en un pedido), para archivos chicos.
    fn multiparte(
        &mut self,
        metodo: &str,
        url: &str,
        metadatos: &serde_json::Value,
        ruta: &Path,
    ) -> Resultado<Archivo> {
        let contenido = std::fs::read(ruta).map_err(|e| ErrorApi::Local(e.to_string()))?;
        let limite = "orbital-limite-061";
        let mut cuerpo = Vec::with_capacity(contenido.len() + 512);
        cuerpo.extend_from_slice(
            format!(
                "--{limite}\r\nContent-Type: application/json; charset=UTF-8\r\n\r\n{metadatos}\r\n--{limite}\r\nContent-Type: application/octet-stream\r\n\r\n"
            )
            .as_bytes(),
        );
        cuerpo.extend_from_slice(&contenido);
        cuerpo.extend_from_slice(format!("\r\n--{limite}--\r\n").as_bytes());
        let tipo = format!("multipart/related; boundary={limite}");
        let r = self.pedir(metodo, url, &[], Some((&tipo, cuerpo)))?;
        archivo_de(&respuesta_json(&r)?)
    }

    /// Subida reanudable: abre la sesión si no hay una válida y manda los tramos que faltan.
    fn reanudable(
        &mut self,
        metodo: &str,
        url_inicio: &str,
        metadatos: &serde_json::Value,
        ruta: &Path,
        sesion: &mut Option<SesionSubida>,
        al_abrir: &mut dyn FnMut(&SesionSubida),
    ) -> Resultado<Archivo> {
        let mut f = File::open(ruta).map_err(|e| ErrorApi::Local(e.to_string()))?;
        let tam = f
            .metadata()
            .map_err(|e| ErrorApi::Local(e.to_string()))?
            .len();
        let md5 = crate::local::md5_archivo(ruta).map_err(|e| ErrorApi::Local(e.to_string()))?;
        // ¿Sigue valiendo la sesión guardada? (mismo contenido y la sesión responde).
        let mut desde = None;
        if let Some(s) = sesion.as_ref().filter(|s| s.md5 == md5 && s.tam == tam) {
            let url = s.url.clone();
            let r = self.pedir(
                "PUT",
                &url,
                &[("Content-Range", format!("bytes */{tam}"))],
                None,
            )?;
            match r.estado {
                200 | 201 => return archivo_de(&respuesta_json(&r)?),
                308 => desde = Some(siguiente_byte(r.rango.as_deref())),
                _ => {} // vencida (404/410): se empieza otra
            }
        }
        if desde.is_none() {
            let r = self.pedir(
                metodo,
                url_inicio,
                &[
                    ("X-Upload-Content-Type", "application/octet-stream".into()),
                    ("X-Upload-Content-Length", tam.to_string()),
                ],
                Some((
                    "application/json; charset=UTF-8",
                    metadatos.to_string().into_bytes(),
                )),
            )?;
            if r.estado != 200 {
                return Err(error_de(&r));
            }
            let url = r.ubicacion.ok_or_else(|| {
                ErrorApi::Servidor(r.estado as u16, "sin sesión de subida".into())
            })?;
            let nueva = SesionSubida {
                url,
                tam,
                md5: md5.clone(),
            };
            al_abrir(&nueva);
            *sesion = Some(nueva);
            desde = Some(0);
        }
        let url = sesion.as_ref().expect("sesión").url.clone();
        let mut pos = desde.unwrap_or(0);
        let mut buf = vec![0u8; self.tramo.min(tam.max(1) as usize)];
        loop {
            f.seek(SeekFrom::Start(pos))
                .map_err(|e| ErrorApi::Local(e.to_string()))?;
            let n = leer_lleno(&mut f, &mut buf).map_err(|e| ErrorApi::Local(e.to_string()))?;
            let rango = if tam == 0 {
                "bytes */0".to_owned()
            } else {
                format!("bytes {}-{}/{}", pos, pos + n as u64 - 1, tam)
            };
            let r = self.pedir(
                "PUT",
                &url,
                &[("Content-Range", rango)],
                Some(("application/octet-stream", buf[..n].to_vec())),
            )?;
            match r.estado {
                200 | 201 => {
                    *sesion = None;
                    return archivo_de(&respuesta_json(&r)?);
                }
                308 => {
                    pos = siguiente_byte(r.rango.as_deref());
                    if n == 0 {
                        return Err(ErrorApi::Servidor(308, "subida sin avance".into()));
                    }
                }
                404 | 410 => {
                    *sesion = None;
                    return Err(ErrorApi::SinRed("la sesión de subida venció".into()));
                }
                _ => return Err(error_de(&r)),
            }
        }
    }
}

/// Lee hasta llenar `buf` o llegar al final.
fn leer_lleno(f: &mut File, buf: &mut [u8]) -> std::io::Result<usize> {
    let mut n = 0;
    while n < buf.len() {
        let k = f.read(&mut buf[n..])?;
        if k == 0 {
            break;
        }
        n += k;
    }
    Ok(n)
}

/// Del `Range: bytes=0-N` de un 308: el próximo byte a mandar.
fn siguiente_byte(rango: Option<&str>) -> u64 {
    rango
        .and_then(|r| r.rsplit('-').next())
        .and_then(|n| n.trim().parse::<u64>().ok())
        .map_or(0, |n| n + 1)
}

fn error_de(r: &Respuesta) -> ErrorApi {
    let v: serde_json::Value = serde_json::from_slice(&r.cuerpo).unwrap_or_default();
    let motivo = v
        .pointer("/error/errors/0/reason")
        .and_then(|m| m.as_str())
        .unwrap_or("");
    let mensaje = v
        .pointer("/error/message")
        .and_then(|m| m.as_str())
        .unwrap_or("")
        .to_owned();
    match (r.estado, motivo) {
        (403, "storageQuotaExceeded" | "quotaExceeded") => ErrorApi::Cuota,
        (403, "userRateLimitExceeded" | "rateLimitExceeded") | (429, _) => ErrorApi::Limite,
        (401, _) => ErrorApi::Autorizacion(mensaje),
        (404, _) => ErrorApi::NoEncontrado,
        (c, _) => ErrorApi::Servidor(c as u16, mensaje),
    }
}

fn respuesta_json(r: &Respuesta) -> Resultado<serde_json::Value> {
    if !(200..300).contains(&r.estado) {
        return Err(error_de(r));
    }
    serde_json::from_slice(&r.cuerpo)
        .map_err(|e| ErrorApi::Servidor(r.estado as u16, format!("JSON inválido: {e}")))
}

fn archivo_de(v: &serde_json::Value) -> Resultado<Archivo> {
    Archivo::desde_json(v).ok_or_else(|| ErrorApi::Servidor(200, "archivo sin id".into()))
}

impl Api for Cliente {
    fn cuenta(&mut self) -> Resultado<String> {
        let url = self.url("/drive/v3/about?fields=user(emailAddress)");
        let v = self.json("GET", &url, None)?;
        Ok(v.pointer("/user/emailAddress")
            .and_then(|e| e.as_str())
            .unwrap_or_default()
            .to_owned())
    }

    fn token_inicio(&mut self) -> Resultado<String> {
        let url = self.url("/drive/v3/changes/startPageToken");
        let v = self.json("GET", &url, None)?;
        v.get("startPageToken")
            .and_then(|t| t.as_str())
            .map(str::to_owned)
            .ok_or_else(|| ErrorApi::Servidor(200, "sin startPageToken".into()))
    }

    fn cambios(&mut self, token: &str) -> Resultado<PaginaCambios> {
        let url = self.url(&format!(
            "/drive/v3/changes?pageToken={}&pageSize=1000&includeRemoved=true&spaces=drive&fields={}",
            escapar(token),
            escapar(&format!(
                "nextPageToken,newStartPageToken,changes(fileId,removed,file({CAMPOS_ARCHIVO}))"
            ))
        ));
        let v = self.json("GET", &url, None)?;
        let cambios = v
            .get("changes")
            .and_then(|c| c.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|c| {
                        Some(Cambio {
                            id: c.get("fileId")?.as_str()?.to_owned(),
                            quitado: c.get("removed").and_then(|r| r.as_bool()).unwrap_or(false),
                            archivo: c.get("file").and_then(Archivo::desde_json),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();
        let texto = |k: &str| v.get(k).and_then(|x| x.as_str()).map(str::to_owned);
        Ok(PaginaCambios {
            cambios,
            siguiente: texto("nextPageToken"),
            nuevo_inicio: texto("newStartPageToken"),
        })
    }

    fn hijos(
        &mut self,
        carpeta: &str,
        pagina: Option<&str>,
    ) -> Resultado<(Vec<Archivo>, Option<String>)> {
        let q = format!(
            "'{}' in parents and trashed = false",
            carpeta.replace('\'', "\\'")
        );
        let mut url = self.url(&format!(
            "/drive/v3/files?q={}&pageSize=1000&fields={}",
            escapar(&q),
            escapar(&format!("nextPageToken,files({CAMPOS_ARCHIVO})"))
        ));
        if let Some(p) = pagina {
            url.push_str(&format!("&pageToken={}", escapar(p)));
        }
        let v = self.json("GET", &url, None)?;
        let archivos = v
            .get("files")
            .and_then(|f| f.as_array())
            .map(|a| a.iter().filter_map(Archivo::desde_json).collect())
            .unwrap_or_default();
        Ok((
            archivos,
            v.get("nextPageToken")
                .and_then(|t| t.as_str())
                .map(str::to_owned),
        ))
    }

    fn archivo(&mut self, id: &str) -> Resultado<Archivo> {
        let url = self.url(&format!(
            "/drive/v3/files/{}?fields={}",
            escapar(id),
            escapar(CAMPOS_ARCHIVO)
        ));
        archivo_de(&self.json("GET", &url, None)?)
    }

    fn crear_carpeta(&mut self, padre: &str, nombre: &str) -> Resultado<Archivo> {
        let url = self.url(&format!(
            "/drive/v3/files?fields={}",
            escapar(CAMPOS_ARCHIVO)
        ));
        let cuerpo =
            serde_json::json!({"name": nombre, "mimeType": MIME_CARPETA, "parents": [padre]});
        archivo_de(&self.json(
            "POST",
            &url,
            Some((
                "application/json; charset=UTF-8",
                cuerpo.to_string().into_bytes(),
            )),
        )?)
    }

    fn subir_nuevo(
        &mut self,
        padre: &str,
        nombre: &str,
        ruta: &Path,
        sesion: &mut Option<SesionSubida>,
        al_abrir: &mut dyn FnMut(&SesionSubida),
    ) -> Resultado<Archivo> {
        let tam = std::fs::metadata(ruta)
            .map_err(|e| ErrorApi::Local(e.to_string()))?
            .len();
        let meta = serde_json::json!({"name": nombre, "parents": [padre]});
        let campos = escapar(CAMPOS_ARCHIVO);
        if tam <= LIMITE_SIMPLE {
            let url = self.url(&format!(
                "/upload/drive/v3/files?uploadType=multipart&fields={campos}"
            ));
            self.multiparte("POST", &url, &meta, ruta)
        } else {
            let url = self.url(&format!(
                "/upload/drive/v3/files?uploadType=resumable&fields={campos}"
            ));
            self.reanudable("POST", &url, &meta, ruta, sesion, al_abrir)
        }
    }

    fn subir_cambio(
        &mut self,
        id: &str,
        ruta: &Path,
        sesion: &mut Option<SesionSubida>,
        al_abrir: &mut dyn FnMut(&SesionSubida),
    ) -> Resultado<Archivo> {
        let tam = std::fs::metadata(ruta)
            .map_err(|e| ErrorApi::Local(e.to_string()))?
            .len();
        let meta = serde_json::json!({});
        let campos = escapar(CAMPOS_ARCHIVO);
        let id = escapar(id);
        if tam <= LIMITE_SIMPLE {
            let url = self.url(&format!(
                "/upload/drive/v3/files/{id}?uploadType=multipart&fields={campos}"
            ));
            self.multiparte("PATCH", &url, &meta, ruta)
        } else {
            let url = self.url(&format!(
                "/upload/drive/v3/files/{id}?uploadType=resumable&fields={campos}"
            ));
            self.reanudable("PATCH", &url, &meta, ruta, sesion, al_abrir)
        }
    }

    fn bajar(&mut self, id: &str, destino: &Path) -> Resultado<()> {
        let ya = std::fs::metadata(destino).map(|m| m.len()).unwrap_or(0);
        let url = self.url(&format!("/drive/v3/files/{}?alt=media", escapar(id)));
        let mut intento = 0;
        loop {
            let msg = soup::Message::new("GET", &url)
                .map_err(|e| ErrorApi::Servidor(0, e.to_string()))?;
            let h = msg.request_headers().expect("cabeceras");
            let t = self.token()?;
            h.append("Authorization", &format!("Bearer {t}"));
            if ya > 0 {
                h.append("Range", &format!("bytes={ya}-"));
            }
            let flujo = self
                .sesion
                .send(&msg, gio::Cancellable::NONE)
                .map_err(|e| ErrorApi::SinRed(e.message().to_owned()))?;
            let estado = msg.status_code();
            if estado == 401 && intento == 0 {
                intento += 1;
                self.acceso = None;
                let _ = flujo.close(gio::Cancellable::NONE);
                continue;
            }
            if estado == 416 {
                // El tramo ya estaba completo.
                return Ok(());
            }
            if estado != 200 && estado != 206 {
                let mut cuerpo = Vec::new();
                let mut buf = [0u8; 4096];
                while let Ok(n) = flujo.read(&mut buf, gio::Cancellable::NONE) {
                    if n == 0 {
                        break;
                    }
                    cuerpo.extend_from_slice(&buf[..n]);
                }
                return Err(error_de(&Respuesta {
                    estado,
                    cuerpo,
                    ubicacion: None,
                    rango: None,
                }));
            }
            let mut f = OpenOptions::new()
                .create(true)
                .write(true)
                .truncate(estado == 200)
                .append(estado == 206)
                .open(destino)
                .map_err(|e| ErrorApi::Local(e.to_string()))?;
            let mut buf = vec![0u8; 256 * 1024];
            loop {
                let n = flujo
                    .read(&mut buf, gio::Cancellable::NONE)
                    .map_err(|e| ErrorApi::SinRed(e.message().to_owned()))?;
                if n == 0 {
                    break;
                }
                f.write_all(&buf[..n])
                    .map_err(|e| ErrorApi::Local(e.to_string()))?;
            }
            f.sync_all().map_err(|e| ErrorApi::Local(e.to_string()))?;
            return Ok(());
        }
    }

    fn actualizar(
        &mut self,
        id: &str,
        nombre: Option<&str>,
        agregar_padre: Option<&str>,
        quitar_padre: Option<&str>,
    ) -> Resultado<Archivo> {
        let mut url = self.url(&format!(
            "/drive/v3/files/{}?fields={}",
            escapar(id),
            escapar(CAMPOS_ARCHIVO)
        ));
        if let Some(p) = agregar_padre {
            url.push_str(&format!("&addParents={}", escapar(p)));
        }
        if let Some(p) = quitar_padre {
            url.push_str(&format!("&removeParents={}", escapar(p)));
        }
        let cuerpo = match nombre {
            Some(n) => serde_json::json!({ "name": n }),
            None => serde_json::json!({}),
        };
        archivo_de(&self.json(
            "PATCH",
            &url,
            Some((
                "application/json; charset=UTF-8",
                cuerpo.to_string().into_bytes(),
            )),
        )?)
    }

    fn a_papelera(&mut self, id: &str) -> Resultado<()> {
        let url = self.url(&format!("/drive/v3/files/{}?fields=id", escapar(id)));
        let cuerpo = serde_json::json!({ "trashed": true });
        match self.json(
            "PATCH",
            &url,
            Some((
                "application/json; charset=UTF-8",
                cuerpo.to_string().into_bytes(),
            )),
        ) {
            Ok(_) | Err(ErrorApi::NoEncontrado) => Ok(()),
            Err(e) => Err(e),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rango_de_308() {
        assert_eq!(siguiente_byte(Some("bytes=0-262143")), 262_144);
        assert_eq!(siguiente_byte(None), 0);
    }

    #[test]
    fn escapa_consultas() {
        assert_eq!(escapar("a b'c/ñ"), "a%20b%27c%2F%C3%B1");
    }
}
