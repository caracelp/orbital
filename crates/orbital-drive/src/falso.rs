//! Servidor falso de la API de Drive v3, en 127.0.0.1, para las pruebas (nunca una cuenta real).
//!
//! Imita lo que usa el motor: `about`, `changes` (con `pageToken`), `files` (listar, metadatos,
//! `alt=media` con `Range`, crear carpeta, `PATCH` con `addParents`/`removeParents`/`trashed`),
//! subidas multiparte y reanudables (sesiones con `308` y `Range`), `/token` y `/auth`.
//! Guarda **todas** las versiones de cada archivo (como las revisiones de Drive) para que las
//! pruebas verifiquen que nada se perdió. Permite simular la web de Drive (editar, renombrar,
//! mover, papelera), cortes de red y cortes a mitad de una subida.

use std::collections::{BTreeMap, HashMap, VecDeque};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::api::MIME_CARPETA;
use crate::local::md5_bytes;

pub const RAIZ: &str = "mi-unidad";
pub const CUENTA: &str = "prueba@ejemplo.org";

#[derive(Clone, Debug)]
pub struct ArchivoFalso {
    pub id: String,
    pub nombre: String,
    pub padres: Vec<String>,
    pub mime: String,
    pub contenido: Vec<u8>,
    /// Todas las versiones que tuvo (la actual incluida).
    pub versiones: Vec<Vec<u8>>,
    pub papelera: bool,
    pub rev: u64,
    pub autor: String,
}

/// Fallas que se pueden provocar.
#[derive(Clone, Debug)]
pub enum Falla {
    /// El próximo pedido responde este código y motivo.
    Error(u16, &'static str),
    /// El próximo tramo de subida reanudable se corta después de estos bytes (sin respuesta).
    CortarSubida(usize),
    /// El próximo pedido de bajada se corta después de estos bytes.
    CortarBajada(usize),
}

struct Sesion {
    destino: Option<String>,
    meta: serde_json::Value,
    tam: u64,
    datos: Vec<u8>,
}

#[derive(Default)]
pub struct Datos {
    pub archivos: BTreeMap<String, ArchivoFalso>,
    cambios: Vec<String>,
    sesiones: HashMap<String, Sesion>,
    fallas: VecDeque<Falla>,
    contador: u64,
    /// Sin red: corta cada conexión sin responder.
    pub caido: bool,
    /// Bytes de contenido recibidos en subidas (para comprobar que una subida se reanudó).
    pub bytes_subidos: u64,
    pub pedidos: u64,
    /// Cuota: si `Some`, tope de bytes totales.
    pub cuota: Option<u64>,
    /// Consultas «¿hasta dónde llegó?» de subidas reanudables (cada una es una subida retomada).
    pub reanudadas: u64,
    /// Tramos cortados a mitad.
    pub cortadas: u64,
    /// De lo que llega en un tramo cortado se conserva un múltiplo de esto (Drive: 256 KiB).
    pub granulo: usize,
    /// Bajadas lentas (para ver el progreso y cancelar): tramos de este tamaño con esta pausa.
    pub ritmo_bajada: Option<(usize, Duration)>,
}

impl Datos {
    fn nuevo_id(&mut self) -> String {
        self.contador += 1;
        format!("f{:06}x{}", self.contador, self.contador * 7919 % 1000)
    }

    fn cambio(&mut self, id: &str) {
        self.cambios.push(id.to_owned());
    }

    fn json(&self, a: &ArchivoFalso, base: &str) -> serde_json::Value {
        let mut v = serde_json::json!({
            "id": a.id,
            "name": a.nombre,
            "parents": a.padres,
            "mimeType": a.mime,
            "trashed": a.papelera,
            "capabilities": {"canEdit": true},
            "webViewLink": format!("{base}/ver/{}", a.id),
            "lastModifyingUser": {"displayName": a.autor},
            "modifiedTime": format!("2026-11-22T10:{:02}:00.000Z", a.rev % 60),
        });
        if a.mime != MIME_CARPETA {
            v["size"] = a.contenido.len().to_string().into();
            v["md5Checksum"] = md5_bytes(&a.contenido).into();
            v["headRevisionId"] = format!("r{}", a.rev).into();
        }
        v
    }

    fn usado(&self) -> u64 {
        self.archivos
            .values()
            .map(|a| a.contenido.len() as u64)
            .sum()
    }

    fn resolver(&self, id: &str) -> String {
        if id == "root" {
            RAIZ.to_owned()
        } else {
            id.to_owned()
        }
    }

    // --- lo que haría la web de Drive o el celular -------------------------------------------

    pub fn crear(&mut self, padre: &str, nombre: &str, contenido: &[u8], autor: &str) -> String {
        let id = self.nuevo_id();
        let padre = self.resolver(padre);
        self.archivos.insert(
            id.clone(),
            ArchivoFalso {
                id: id.clone(),
                nombre: nombre.into(),
                padres: vec![padre],
                mime: "application/octet-stream".into(),
                contenido: contenido.to_vec(),
                versiones: vec![contenido.to_vec()],
                papelera: false,
                rev: 1,
                autor: autor.into(),
            },
        );
        self.cambio(&id);
        id
    }

    pub fn crear_carpeta(&mut self, padre: &str, nombre: &str) -> String {
        let id = self.nuevo_id();
        let padre = self.resolver(padre);
        self.archivos.insert(
            id.clone(),
            ArchivoFalso {
                id: id.clone(),
                nombre: nombre.into(),
                padres: vec![padre],
                mime: MIME_CARPETA.into(),
                contenido: vec![],
                versiones: vec![],
                papelera: false,
                rev: 1,
                autor: "Prueba".into(),
            },
        );
        self.cambio(&id);
        id
    }

    pub fn editar(&mut self, id: &str, contenido: &[u8], autor: &str) {
        if let Some(a) = self.archivos.get_mut(id) {
            a.contenido = contenido.to_vec();
            a.versiones.push(contenido.to_vec());
            a.rev += 1;
            a.autor = autor.into();
        }
        self.cambio(id);
    }

    pub fn renombrar(&mut self, id: &str, nombre: &str) {
        if let Some(a) = self.archivos.get_mut(id) {
            a.nombre = nombre.into();
        }
        self.cambio(id);
    }

    pub fn mover(&mut self, id: &str, padre: &str) {
        let padre = self.resolver(padre);
        if let Some(a) = self.archivos.get_mut(id) {
            a.padres = vec![padre];
        }
        self.cambio(id);
    }

    pub fn a_papelera(&mut self, id: &str) {
        if let Some(a) = self.archivos.get_mut(id) {
            a.papelera = true;
        }
        self.cambio(id);
    }

    pub fn fallar(&mut self, f: Falla) {
        self.fallas.push_back(f);
    }

    pub fn fallas_pendientes(&self) -> usize {
        self.fallas.len()
    }

    pub fn sin_fallas(&mut self) {
        self.fallas.clear();
    }

    /// Ruta de un archivo debajo de `carpeta` (o `None` si no cuelga de ella).
    pub fn ruta(&self, id: &str, carpeta: &str) -> Option<String> {
        let mut partes = Vec::new();
        let mut actual = self.archivos.get(id)?;
        loop {
            partes.push(crate::local::nombre_local(&actual.nombre));
            let p = actual.padres.first()?;
            if p == carpeta {
                break;
            }
            actual = self.archivos.get(p)?;
            if actual.papelera {
                return None;
            }
        }
        partes.reverse();
        Some(partes.join("/"))
    }

    /// Árbol de archivos (no carpetas) vivos debajo de `carpeta`: ruta → contenido. Los nombres
    /// repetidos se distinguen como en el disco (` (2)`…), con la misma regla que el motor.
    pub fn arbol(&self, carpeta: &str) -> BTreeMap<String, Vec<u8>> {
        let mut e = crate::Estado::nuevo("", carpeta);
        for a in self
            .archivos
            .values()
            .filter(|a| !a.papelera && a.id != RAIZ)
        {
            if let Some(p) = a.padres.first() {
                e.nodos.insert(
                    a.id.clone(),
                    crate::estado::Nodo {
                        nombre: a.nombre.clone(),
                        padre: p.clone(),
                        carpeta: a.mime == MIME_CARPETA,
                        ..Default::default()
                    },
                );
            }
        }
        e.rutas()
            .into_iter()
            .filter(|(id, _)| self.archivos[id].mime != MIME_CARPETA)
            .map(|(id, r)| (r, self.archivos[&id].contenido.clone()))
            .collect()
    }

    pub fn buscar(&self, ruta: &str, carpeta: &str) -> Option<String> {
        self.archivos
            .values()
            .filter(|a| !a.papelera)
            .find(|a| self.ruta(&a.id, carpeta).as_deref() == Some(ruta))
            .map(|a| a.id.clone())
    }

    /// Todas las versiones de todos los archivos, en la papelera incluidos.
    pub fn todas_las_versiones(&self) -> Vec<Vec<u8>> {
        self.archivos
            .values()
            .flat_map(|a| a.versiones.iter().cloned())
            .collect()
    }
}

/// El servidor. Se detiene al soltarlo (deja de aceptar conexiones).
pub struct ServidorFalso {
    pub url: String,
    pub datos: Arc<Mutex<Datos>>,
}

impl ServidorFalso {
    pub fn iniciar() -> Self {
        let oyente = TcpListener::bind("127.0.0.1:0").expect("puerto local");
        let url = format!("http://{}", oyente.local_addr().unwrap());
        let datos = Arc::new(Mutex::new(Datos::default()));
        {
            let mut d = datos.lock().unwrap();
            d.archivos.insert(
                RAIZ.into(),
                ArchivoFalso {
                    id: RAIZ.into(),
                    nombre: "Mi unidad".into(),
                    padres: vec![],
                    mime: MIME_CARPETA.into(),
                    contenido: vec![],
                    versiones: vec![],
                    papelera: false,
                    rev: 1,
                    autor: "Prueba".into(),
                },
            );
        }
        let (d, u) = (datos.clone(), url.clone());
        std::thread::Builder::new()
            .name("drive-falso".into())
            .spawn(move || {
                for s in oyente.incoming().flatten() {
                    let (d, u) = (d.clone(), u.clone());
                    std::thread::spawn(move || {
                        let _ = atender(s, &d, &u);
                    });
                }
            })
            .expect("hilo del servidor falso");
        Self { url, datos }
    }

    pub fn datos(&self) -> std::sync::MutexGuard<'_, Datos> {
        self.datos.lock().unwrap()
    }
}

struct Pedido {
    metodo: String,
    ruta: String,
    consulta: HashMap<String, String>,
    cabeceras: HashMap<String, String>,
    cuerpo: Vec<u8>,
}

fn desescapar(s: &str) -> String {
    gio::glib::Uri::unescape_string(&s.replace('+', " "), None::<&str>)
        .map(|g| g.to_string())
        .unwrap_or_else(|| s.to_owned())
}

/// Lee la cabecera del pedido; el cuerpo lo lee quien atiende (para poder cortarlo).
fn leer_cabecera(r: &mut BufReader<TcpStream>) -> std::io::Result<Option<Pedido>> {
    let mut linea = String::new();
    if r.read_line(&mut linea)? == 0 {
        return Ok(None);
    }
    let mut partes = linea.split_whitespace();
    let metodo = partes.next().unwrap_or("").to_owned();
    let objetivo = partes.next().unwrap_or("").to_owned();
    let (ruta, q) = objetivo.split_once('?').unwrap_or((&objetivo, ""));
    let consulta = q
        .split('&')
        .filter(|p| !p.is_empty())
        .map(|p| {
            let (k, v) = p.split_once('=').unwrap_or((p, ""));
            (desescapar(k), desescapar(v))
        })
        .collect();
    let mut cabeceras = HashMap::new();
    loop {
        let mut l = String::new();
        r.read_line(&mut l)?;
        let l = l.trim_end();
        if l.is_empty() {
            break;
        }
        if let Some((k, v)) = l.split_once(':') {
            cabeceras.insert(k.trim().to_ascii_lowercase(), v.trim().to_owned());
        }
    }
    Ok(Some(Pedido {
        metodo,
        ruta: desescapar(ruta),
        consulta,
        cabeceras,
        cuerpo: Vec::new(),
    }))
}

fn responder(s: &mut TcpStream, codigo: u16, cabeceras: &[(&str, String)], cuerpo: &[u8]) {
    let texto = match codigo {
        200 => "OK",
        206 => "Partial Content",
        302 => "Found",
        308 => "Resume Incomplete",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        416 => "Range Not Satisfiable",
        429 => "Too Many Requests",
        _ => "Error",
    };
    let mut h = format!(
        "HTTP/1.1 {codigo} {texto}\r\nContent-Length: {}\r\nConnection: close\r\n",
        cuerpo.len()
    );
    for (k, v) in cabeceras {
        h.push_str(&format!("{k}: {v}\r\n"));
    }
    h.push_str("\r\n");
    let _ = s.write_all(h.as_bytes());
    let _ = s.write_all(cuerpo);
    let _ = s.flush();
}

fn json(s: &mut TcpStream, codigo: u16, v: &serde_json::Value) {
    responder(
        s,
        codigo,
        &[("Content-Type", "application/json".into())],
        v.to_string().as_bytes(),
    );
}

fn error(s: &mut TcpStream, codigo: u16, motivo: &str) {
    json(
        s,
        codigo,
        &serde_json::json!({"error": {"code": codigo, "message": motivo, "errors": [{"reason": motivo}]}}),
    );
}

/// Separa un cuerpo `multipart/related` en (metadatos, contenido).
fn multiparte(tipo: &str, cuerpo: &[u8]) -> Option<(serde_json::Value, Vec<u8>)> {
    let limite = tipo.split("boundary=").nth(1)?.trim_matches('"');
    let sep = format!("--{limite}");
    let mut partes = Vec::new();
    let mut resto = cuerpo;
    while let Some(p) = buscar(resto, sep.as_bytes()) {
        partes.push(&resto[..p]);
        resto = &resto[p + sep.len()..];
    }
    // partes[0] es lo que va antes del primer separador (vacío).
    let parte = |b: &[u8]| -> Option<Vec<u8>> {
        let ini = buscar(b, b"\r\n\r\n")? + 4;
        let mut fin = b.len();
        if b.ends_with(b"\r\n") {
            fin -= 2;
        }
        Some(b[ini..fin].to_vec())
    };
    let meta = serde_json::from_slice(&parte(partes.get(1)?)?).ok()?;
    let contenido = parte(partes.get(2)?)?;
    Some((meta, contenido))
}

fn buscar(h: &[u8], aguja: &[u8]) -> Option<usize> {
    h.windows(aguja.len()).position(|w| w == aguja)
}

fn atender(s: TcpStream, datos: &Arc<Mutex<Datos>>, base: &str) -> std::io::Result<()> {
    s.set_read_timeout(Some(Duration::from_secs(10)))?;
    let mut lector = BufReader::new(s.try_clone()?);
    let mut s = s;
    let Some(mut p) = leer_cabecera(&mut lector)? else {
        return Ok(());
    };
    let largo: usize = p
        .cabeceras
        .get("content-length")
        .and_then(|l| l.parse().ok())
        .unwrap_or(0);
    if std::env::var_os("ORBITAL_FALSO_VIVO").is_some() {
        eprintln!(
            "FALSO {} {} {:?} {:?}",
            p.metodo,
            p.ruta,
            p.cabeceras.get("content-range"),
            p.cabeceras.get("content-length")
        );
    }
    let es_subida = p.metodo == "PUT" && p.ruta.starts_with("/upload/sesion/");
    let es_bajada = p.consulta.get("alt").map(String::as_str) == Some("media");
    let (caido, falla) = {
        let mut d = datos.lock().unwrap();
        d.pedidos += 1;
        let caido = d.caido;
        // Cada falla espera al pedido que le corresponde (los cortes, a una subida o una bajada).
        let toca = match d.fallas.front() {
            Some(Falla::CortarSubida(_)) => es_subida,
            Some(Falla::CortarBajada(_)) => es_bajada,
            Some(Falla::Error(..)) => p.ruta != "/token",
            None => false,
        };
        let falla = if caido || !toca {
            None
        } else {
            d.fallas.pop_front()
        };
        (caido, falla)
    };
    if caido {
        // Sin red: se corta sin responder.
        let _ = s.shutdown(std::net::Shutdown::Both);
        return Ok(());
    }
    // Corte a mitad de un tramo de subida: se guardan los bytes que llegaron (en múltiplos de
    // 256 KiB, como Drive) y se corta la conexión.
    let d0_granulo = datos.lock().unwrap().granulo;
    if let Some(Falla::CortarSubida(n)) = &falla {
        let n = (*n).min(largo);
        let mut buf = vec![0u8; n];
        lector.read_exact(&mut buf)?;
        let g = if d0_granulo == 0 {
            256 * 1024
        } else {
            d0_granulo
        };
        let guardar = n / g * g;
        let sid = p.ruta.trim_start_matches("/upload/sesion/").to_owned();
        let mut d = datos.lock().unwrap();
        d.bytes_subidos += n as u64;
        d.cortadas += 1;
        if let Some(se) = d.sesiones.get_mut(&sid) {
            se.datos.extend_from_slice(&buf[..guardar]);
        }
        drop(d);
        let _ = s.shutdown(std::net::Shutdown::Both);
        return Ok(());
    }
    if let Some(Falla::Error(c, m)) = &falla {
        let mut buf = vec![0u8; largo];
        let _ = lector.read_exact(&mut buf);
        error(&mut s, *c, m);
        return Ok(());
    }
    let mut cuerpo = vec![0u8; largo];
    lector.read_exact(&mut cuerpo)?;
    p.cuerpo = cuerpo;
    let corte_bajada = match falla {
        Some(Falla::CortarBajada(n)) => Some(n),
        _ => None,
    };
    manejar(&mut s, p, datos, base, corte_bajada);
    Ok(())
}

fn manejar(
    s: &mut TcpStream,
    p: Pedido,
    datos: &Arc<Mutex<Datos>>,
    base: &str,
    corte: Option<usize>,
) {
    if p.ruta == "/token" {
        let mut d = datos.lock().unwrap();
        d.contador += 1;
        let n = d.contador;
        return json(
            s,
            200,
            &serde_json::json!({"access_token": format!("falso-{n}"), "expires_in": 3600, "refresh_token": "falso-renovar", "token_type": "Bearer"}),
        );
    }
    if p.ruta == "/auth" {
        let redir = p.consulta.get("redirect_uri").cloned().unwrap_or_default();
        let estado = p.consulta.get("state").cloned().unwrap_or_default();
        return responder(
            s,
            302,
            &[(
                "Location",
                format!("{redir}/?code=codigo-falso&state={estado}"),
            )],
            b"",
        );
    }
    let autorizado = p
        .cabeceras
        .get("authorization")
        .is_some_and(|a| a.starts_with("Bearer falso-"));
    if !autorizado {
        return error(s, 401, "authError");
    }
    let mut d = datos.lock().unwrap();
    let campo = |k: &str| p.consulta.get(k).cloned();
    match (p.metodo.as_str(), p.ruta.as_str()) {
        ("GET", "/drive/v3/about") => {
            // Sin cuota: 15 GiB, como una cuenta gratuita.
            let limite = d.cuota.unwrap_or(15 << 30);
            json(
                s,
                200,
                &serde_json::json!({
                    "user": {"emailAddress": CUENTA, "displayName": "Prueba"},
                    "storageQuota": {"limit": limite.to_string(), "usage": d.usado().to_string()},
                }),
            )
        }
        ("GET", "/drive/v3/changes/startPageToken") => {
            let t = d.cambios.len().to_string();
            json(s, 200, &serde_json::json!({ "startPageToken": t }))
        }
        ("GET", "/drive/v3/changes") => {
            let desde: usize = campo("pageToken").and_then(|t| t.parse().ok()).unwrap_or(0);
            let tam: usize = campo("pageSize")
                .and_then(|t| t.parse().ok())
                .unwrap_or(100)
                .min(1000);
            let total = d.cambios.len();
            let hasta = (desde + tam).min(total);
            let mut vistos = std::collections::BTreeSet::new();
            let mut lista = Vec::new();
            for id in &d.cambios[desde.min(total)..hasta] {
                if !vistos.insert(id.clone()) {
                    continue;
                }
                match d.archivos.get(id) {
                    Some(a) => lista.push(serde_json::json!({"fileId": id, "removed": false, "file": d.json(a, base)})),
                    None => lista.push(serde_json::json!({"fileId": id, "removed": true})),
                }
            }
            let mut v = serde_json::json!({ "changes": lista });
            if hasta < total {
                v["nextPageToken"] = hasta.to_string().into();
            } else {
                v["newStartPageToken"] = total.to_string().into();
            }
            json(s, 200, &v)
        }
        ("GET", "/drive/v3/files") => {
            let q = campo("q").unwrap_or_default();
            let padre = q.split('\'').nth(1).unwrap_or("").to_owned();
            let padre = d.resolver(&padre);
            let desde: usize = campo("pageToken").and_then(|t| t.parse().ok()).unwrap_or(0);
            let tam: usize = campo("pageSize")
                .and_then(|t| t.parse().ok())
                .unwrap_or(100)
                .min(1000);
            let hijos: Vec<&ArchivoFalso> = d
                .archivos
                .values()
                .filter(|a| !a.papelera && a.padres.contains(&padre))
                .collect();
            let hasta = (desde + tam).min(hijos.len());
            let lista: Vec<serde_json::Value> = hijos[desde.min(hijos.len())..hasta]
                .iter()
                .map(|a| d.json(a, base))
                .collect();
            let mut v = serde_json::json!({ "files": lista });
            if hasta < hijos.len() {
                v["nextPageToken"] = hasta.to_string().into();
            }
            json(s, 200, &v)
        }
        ("POST", "/drive/v3/files") => {
            let meta: serde_json::Value = serde_json::from_slice(&p.cuerpo).unwrap_or_default();
            let nombre = meta["name"].as_str().unwrap_or("sin nombre").to_owned();
            let padre = meta["parents"][0].as_str().unwrap_or("root").to_owned();
            let id = d.crear_carpeta(&padre, &nombre);
            let v = d.json(&d.archivos[&id], base);
            json(s, 200, &v)
        }
        ("POST", "/upload/drive/v3/files") | ("PATCH", _)
            if p.ruta.starts_with("/upload/drive/v3/files") =>
        {
            let destino = p
                .ruta
                .strip_prefix("/upload/drive/v3/files/")
                .map(str::to_owned);
            if let Some(id) = &destino
                && !d.archivos.get(id).is_some_and(|a| !a.papelera)
            {
                return error(s, 404, "notFound");
            }
            match campo("uploadType").as_deref() {
                Some("multipart") => {
                    let tipo = p.cabeceras.get("content-type").cloned().unwrap_or_default();
                    let Some((meta, contenido)) = multiparte(&tipo, &p.cuerpo) else {
                        return error(s, 400, "badRequest");
                    };
                    d.bytes_subidos += contenido.len() as u64;
                    if d.cuota
                        .is_some_and(|c| d.usado() + contenido.len() as u64 > c)
                    {
                        return error(s, 403, "storageQuotaExceeded");
                    }
                    let id = terminar(&mut d, destino, &meta, contenido);
                    let v = d.json(&d.archivos[&id], base);
                    json(s, 200, &v)
                }
                Some("resumable") => {
                    let meta: serde_json::Value =
                        serde_json::from_slice(&p.cuerpo).unwrap_or_default();
                    let tam = p
                        .cabeceras
                        .get("x-upload-content-length")
                        .and_then(|l| l.parse().ok())
                        .unwrap_or(0);
                    d.contador += 1;
                    let sid = format!("s{}", d.contador);
                    d.sesiones.insert(
                        sid.clone(),
                        Sesion {
                            destino,
                            meta,
                            tam,
                            datos: Vec::new(),
                        },
                    );
                    responder(
                        s,
                        200,
                        &[("Location", format!("{base}/upload/sesion/{sid}"))],
                        b"",
                    )
                }
                _ => error(s, 400, "badRequest"),
            }
        }
        ("PUT", r) if r.starts_with("/upload/sesion/") => {
            let sid = r.trim_start_matches("/upload/sesion/").to_owned();
            let rango = p
                .cabeceras
                .get("content-range")
                .cloned()
                .unwrap_or_default();
            if rango.starts_with("bytes */") && d.sesiones.contains_key(&sid) {
                d.reanudadas += 1;
            }
            let Some(se) = d.sesiones.get_mut(&sid) else {
                return error(s, 404, "notFound");
            };
            if !rango.starts_with("bytes */") {
                let desde: usize = rango
                    .trim_start_matches("bytes ")
                    .split('-')
                    .next()
                    .and_then(|x| x.parse().ok())
                    .unwrap_or(0);
                if desde != se.datos.len() {
                    // Drive no acepta huecos: responde dónde está.
                    let n = se.datos.len();
                    let h = if n > 0 {
                        vec![("Range", format!("bytes=0-{}", n - 1))]
                    } else {
                        vec![]
                    };
                    return responder(s, 308, &h, b"");
                }
                se.datos.extend_from_slice(&p.cuerpo);
                let n = p.cuerpo.len() as u64;
                d.bytes_subidos += n;
            }
            let se = d.sesiones.get(&sid).expect("sesión");
            if se.datos.len() as u64 >= se.tam {
                let se = d.sesiones.remove(&sid).expect("sesión");
                if d.cuota
                    .is_some_and(|c| d.usado() + se.datos.len() as u64 > c)
                {
                    return error(s, 403, "storageQuotaExceeded");
                }
                let id = terminar(&mut d, se.destino, &se.meta, se.datos);
                let v = d.json(&d.archivos[&id], base);
                json(s, 200, &v)
            } else {
                let n = se.datos.len();
                let h = if n > 0 {
                    vec![("Range", format!("bytes=0-{}", n - 1))]
                } else {
                    vec![]
                };
                responder(s, 308, &h, b"")
            }
        }
        ("GET", r) if r.starts_with("/drive/v3/files/") => {
            let id = r.trim_start_matches("/drive/v3/files/").to_owned();
            let id = d.resolver(&id);
            let Some(a) = d.archivos.get(&id).cloned() else {
                return error(s, 404, "notFound");
            };
            if campo("alt").as_deref() == Some("media") {
                let desde: usize = p
                    .cabeceras
                    .get("range")
                    .and_then(|r| {
                        r.trim_start_matches("bytes=")
                            .split('-')
                            .next()?
                            .parse()
                            .ok()
                    })
                    .unwrap_or(0);
                let ritmo = d.ritmo_bajada;
                drop(d);
                if desde > 0 && desde >= a.contenido.len() {
                    return responder(s, 416, &[], b"");
                }
                let parte = &a.contenido[desde..];
                let codigo = if desde > 0 { 206 } else { 200 };
                if let Some(n) = corte {
                    // Promete todo y manda solo `n` bytes.
                    let h = format!(
                        "HTTP/1.1 {codigo} OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        parte.len()
                    );
                    let _ = s.write_all(h.as_bytes());
                    let _ = s.write_all(&parte[..n.min(parte.len())]);
                    let _ = s.shutdown(std::net::Shutdown::Both);
                    return;
                }
                if let Some((tramo, pausa)) = ritmo {
                    let h = format!(
                        "HTTP/1.1 {codigo} OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        parte.len()
                    );
                    let _ = s.write_all(h.as_bytes());
                    for t in parte.chunks(tramo.max(1)) {
                        if s.write_all(t).and_then(|()| s.flush()).is_err() {
                            return; // el cliente cortó (canceló)
                        }
                        std::thread::sleep(pausa);
                    }
                    return;
                }
                return responder(s, codigo, &[], parte);
            }
            let v = d.json(&a, base);
            json(s, 200, &v)
        }
        ("PATCH", r) if r.starts_with("/drive/v3/files/") => {
            let id = r.trim_start_matches("/drive/v3/files/").to_owned();
            if !d.archivos.contains_key(&id) {
                return error(s, 404, "notFound");
            }
            let meta: serde_json::Value = serde_json::from_slice(&p.cuerpo).unwrap_or_default();
            let agregar = campo("addParents").map(|x| d.resolver(&x));
            let quitar = campo("removeParents").map(|x| d.resolver(&x));
            let a = d.archivos.get_mut(&id).expect("archivo");
            if let Some(n) = meta["name"].as_str() {
                a.nombre = n.to_owned();
            }
            if meta["trashed"].as_bool() == Some(true) {
                a.papelera = true;
            }
            if let Some(q) = quitar {
                a.padres.retain(|x| *x != q);
            }
            if let Some(ag) = agregar
                && !a.padres.contains(&ag)
            {
                a.padres.push(ag);
            }
            d.cambio(&id);
            let v = d.json(&d.archivos[&id], base);
            json(s, 200, &v)
        }
        _ => error(s, 404, "notFound"),
    }
}

/// Termina una subida: archivo nuevo o versión nueva del existente.
fn terminar(
    d: &mut Datos,
    destino: Option<String>,
    meta: &serde_json::Value,
    contenido: Vec<u8>,
) -> String {
    match destino {
        Some(id) => {
            d.editar(&id, &contenido, "Orbital");
            if let Some(n) = meta["name"].as_str()
                && let Some(a) = d.archivos.get_mut(&id)
            {
                a.nombre = n.to_owned();
            }
            id
        }
        None => {
            let nombre = meta["name"].as_str().unwrap_or("sin nombre").to_owned();
            let padre = meta["parents"][0].as_str().unwrap_or("root").to_owned();
            d.crear(&padre, &nombre, &contenido, "Orbital")
        }
    }
}
