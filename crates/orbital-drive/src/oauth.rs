//! Inicio de sesión con Google para aplicaciones de escritorio: OAuth 2.0 con PKCE (S256) y
//! redirección a `http://127.0.0.1:<puerto>` (decisión 1). Corre fuera del hilo de GTK.
//!
//! 1. Se abre un puerto al azar en 127.0.0.1 y se arma la URL de autorización.
//! 2. La interfaz abre esa URL en el navegador del sistema.
//! 3. Google redirige al puerto con el código; se valida `state` y se responde una página corta.
//! 4. El código se cambia por los tokens; el de renovación va al llavero.

use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::time::{Duration, Instant};

use base64::Engine;
use sha2::Digest;

use crate::api::Api;
use crate::cliente::{Cliente, Credenciales, Direcciones, escapar};
use crate::llavero;

pub const PERMISO: &str = "https://www.googleapis.com/auth/drive";
const AUTORIZAR_GOOGLE: &str = "https://accounts.google.com/o/oauth2/v2/auth";

fn azar(n: usize) -> Vec<u8> {
    let mut b = vec![0u8; n];
    getrandom::fill(&mut b).expect("fuente de azar del sistema");
    b
}

fn b64(b: &[u8]) -> String {
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(b)
}

/// Verificador y desafío de PKCE (RFC 7636, S256).
pub fn pkce() -> (String, String) {
    let verificador = b64(&azar(48)); // 64 caracteres
    let desafio = b64(&sha2::Sha256::digest(verificador.as_bytes()));
    (verificador, desafio)
}

/// URL de autorización (la de Google o la del servidor falso).
pub fn url_autorizacion(
    cliente_id: &str,
    redireccion: &str,
    desafio: &str,
    estado: &str,
) -> String {
    let base = match std::env::var("ORBITAL_DRIVE_API") {
        Ok(b) if !b.is_empty() => format!("{}/auth", b.trim_end_matches('/')),
        _ => AUTORIZAR_GOOGLE.to_owned(),
    };
    format!(
        "{base}?client_id={}&redirect_uri={}&response_type=code&scope={}&code_challenge={desafio}&code_challenge_method=S256&state={estado}&access_type=offline&prompt=consent",
        escapar(cliente_id),
        escapar(redireccion),
        escapar(PERMISO)
    )
}

/// Resultado de iniciar sesión.
#[derive(Clone, Debug)]
pub struct Sesion {
    pub cuenta: String,
    pub refresh_token: String,
}

/// Inicia sesión: `abrir` recibe la URL para el navegador. Espera hasta `espera` la vuelta.
/// Guarda el cliente y el token de renovación en `llavero`.
pub fn iniciar_sesion(
    cliente: &llavero::Cliente,
    llavero: &dyn llavero::Llavero,
    abrir: impl FnOnce(String),
    espera: Duration,
) -> Result<Sesion, String> {
    let oyente = TcpListener::bind("127.0.0.1:0").map_err(|e| e.to_string())?;
    let puerto = oyente.local_addr().map_err(|e| e.to_string())?.port();
    let redireccion = format!("http://127.0.0.1:{puerto}");
    let (verificador, desafio) = pkce();
    let estado = b64(&azar(16));
    abrir(url_autorizacion(
        &cliente.id,
        &redireccion,
        &desafio,
        &estado,
    ));
    let codigo = esperar_codigo(&oyente, &estado, espera)?;
    let dir = Direcciones::del_entorno();
    let refresh = canjear(&dir, cliente, &codigo, &verificador, &redireccion)?;
    let mut c = Cliente::new(
        dir,
        Credenciales {
            client_id: cliente.id.clone(),
            client_secret: cliente.secreto.clone(),
            refresh_token: refresh.clone(),
        },
    );
    let cuenta = c.cuenta().map_err(|e| e.to_string())?;
    llavero::guardar_cliente(llavero, cliente)?;
    llavero::guardar_cuenta(llavero, &cuenta, &refresh)?;
    Ok(Sesion {
        cuenta,
        refresh_token: refresh,
    })
}

/// Espera la redirección con el código (ignora otros pedidos, como el ícono del navegador).
fn esperar_codigo(oyente: &TcpListener, estado: &str, espera: Duration) -> Result<String, String> {
    oyente.set_nonblocking(true).map_err(|e| e.to_string())?;
    let limite = Instant::now() + espera;
    loop {
        if Instant::now() > limite {
            return Err("se acabó el tiempo para iniciar sesión".into());
        }
        let (mut s, _) = match oyente.accept() {
            Ok(c) => c,
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(100));
                continue;
            }
            Err(e) => return Err(e.to_string()),
        };
        let _ = s.set_nonblocking(false);
        let _ = s.set_read_timeout(Some(Duration::from_secs(5)));
        let mut linea = String::new();
        if BufReader::new(&s).read_line(&mut linea).is_err() {
            continue;
        }
        let ruta = linea.split_whitespace().nth(1).unwrap_or("");
        let consulta = ruta.split_once('?').map_or("", |(_, q)| q);
        let param = |k: &str| {
            consulta.split('&').find_map(|p| {
                let (a, b) = p.split_once('=')?;
                (a == k).then(|| {
                    gio::glib::Uri::unescape_string(b, None::<&str>)
                        .map(|s| s.to_string())
                        .unwrap_or_default()
                })
            })
        };
        if param("state").is_none() && param("error").is_none() {
            let _ = s.write_all(
                b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
            );
            continue;
        }
        let (titulo, resultado) = if param("state").as_deref() != Some(estado) {
            (
                "No se pudo iniciar sesión",
                Err("la respuesta no coincide con el pedido".to_owned()),
            )
        } else if let Some(e) = param("error") {
            (
                "No se pudo iniciar sesión",
                Err(format!("Google respondió: {e}")),
            )
        } else if let Some(c) = param("code") {
            ("Listo: ya podés volver a Orbital", Ok(c))
        } else {
            (
                "No se pudo iniciar sesión",
                Err("la respuesta no trae el código".to_owned()),
            )
        };
        let html = format!(
            "<!doctype html><html lang=\"es\"><meta charset=\"utf-8\"><title>Orbital</title><body style=\"font-family:sans-serif;background:#111;color:#eee;display:grid;place-items:center;height:90vh\"><h1 style=\"color:#f5a3c7\">{titulo}</h1></body></html>"
        );
        let _ = write!(
            s,
            "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{html}",
            html.len()
        );
        return resultado;
    }
}

/// Cambia el código por el token de renovación.
fn canjear(
    dir: &Direcciones,
    cliente: &llavero::Cliente,
    codigo: &str,
    verificador: &str,
    redireccion: &str,
) -> Result<String, String> {
    use soup::prelude::*;
    let sesion = soup::Session::new();
    sesion.set_timeout(60);
    let msg = soup::Message::new("POST", &dir.token).map_err(|e| e.to_string())?;
    let cuerpo = format!(
        "grant_type=authorization_code&code={}&client_id={}&client_secret={}&code_verifier={}&redirect_uri={}",
        escapar(codigo),
        escapar(&cliente.id),
        escapar(&cliente.secreto),
        escapar(verificador),
        escapar(redireccion)
    );
    msg.set_request_body_from_bytes(
        Some("application/x-www-form-urlencoded"),
        Some(&gio::glib::Bytes::from_owned(cuerpo.into_bytes())),
    );
    let b = sesion
        .send_and_read(&msg, gio::Cancellable::NONE)
        .map_err(|e| format!("sin conexión: {}", e.message()))?;
    let v: serde_json::Value = serde_json::from_slice(&b).unwrap_or_default();
    if msg.status_code() != 200 {
        let detalle = v
            .get("error_description")
            .or_else(|| v.get("error"))
            .and_then(|e| e.as_str())
            .unwrap_or("sin detalle");
        return Err(format!("Google rechazó el inicio de sesión: {detalle}"));
    }
    v.get("refresh_token")
        .and_then(|t| t.as_str())
        .map(str::to_owned)
        .ok_or_else(|| "Google no devolvió un token de renovación".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pkce_s256() {
        let (v, d) = pkce();
        assert_eq!(v.len(), 64);
        assert_eq!(d, b64(&sha2::Sha256::digest(v.as_bytes())));
        // Ejemplo del RFC 7636, apéndice B.
        let v = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";
        assert_eq!(
            b64(&sha2::Sha256::digest(v.as_bytes())),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
    }
}
