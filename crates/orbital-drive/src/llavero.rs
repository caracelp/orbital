//! Credenciales en el llavero del sistema (FR-013): el id y el secreto del cliente que carga la
//! usuaria y el token de renovación de cada cuenta. Nunca en archivos de Orbital ni en la bóveda.
//!
//! El llavero del sistema se usa por libsecret (que WebKitGTK ya carga en Orbital: no suma
//! bibliotecas). Para las pruebas de interfaz, `ORBITAL_DRIVE_LLAVERO=archivo:<ruta>` usa un archivo
//! (solo desarrollo; nunca la configuración de la usuaria).

use std::collections::BTreeMap;
use std::ffi::{CStr, CString, c_char, c_int, c_void};
use std::path::PathBuf;
use std::sync::Mutex;

/// Un llavero.
pub trait Llavero: Send + Sync {
    fn leer(&self, clave: &str) -> Result<Option<String>, String>;
    fn guardar(&self, clave: &str, valor: &str) -> Result<(), String>;
    fn borrar(&self, clave: &str) -> Result<(), String>;
}

/// Id y secreto del cliente OAuth del proyecto de Google Cloud de la usuaria.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Cliente {
    pub id: String,
    pub secreto: String,
}

const CLAVE_CLIENTE: &str = "cliente";

pub fn leer_cliente(l: &dyn Llavero) -> Result<Option<Cliente>, String> {
    let Some(t) = l.leer(CLAVE_CLIENTE)? else {
        return Ok(None);
    };
    let v: serde_json::Value = serde_json::from_str(&t).map_err(|e| e.to_string())?;
    let campo = |k: &str| v.get(k).and_then(|x| x.as_str()).unwrap_or("").to_owned();
    Ok(Some(Cliente {
        id: campo("id"),
        secreto: campo("secreto"),
    }))
}

pub fn guardar_cliente(l: &dyn Llavero, c: &Cliente) -> Result<(), String> {
    l.guardar(
        CLAVE_CLIENTE,
        &serde_json::json!({"id": c.id, "secreto": c.secreto}).to_string(),
    )
}

pub fn leer_cuenta(l: &dyn Llavero, cuenta: &str) -> Result<Option<String>, String> {
    l.leer(&format!("cuenta:{cuenta}"))
}

pub fn guardar_cuenta(l: &dyn Llavero, cuenta: &str, refresh: &str) -> Result<(), String> {
    l.guardar(&format!("cuenta:{cuenta}"), refresh)
}

pub fn borrar_cuenta(l: &dyn Llavero, cuenta: &str) -> Result<(), String> {
    l.borrar(&format!("cuenta:{cuenta}"))
}

/// El llavero que corresponde: el del sistema, salvo `ORBITAL_DRIVE_LLAVERO=archivo:<ruta>`.
pub fn del_entorno() -> Box<dyn Llavero> {
    match std::env::var("ORBITAL_DRIVE_LLAVERO") {
        Ok(v) if v.starts_with("archivo:") => {
            Box::new(Archivo(PathBuf::from(&v["archivo:".len()..])))
        }
        _ => Box::new(Sistema),
    }
}

/// En memoria (pruebas).
#[derive(Default)]
pub struct Memoria(pub Mutex<BTreeMap<String, String>>);

impl Llavero for Memoria {
    fn leer(&self, clave: &str) -> Result<Option<String>, String> {
        Ok(self.0.lock().unwrap().get(clave).cloned())
    }
    fn guardar(&self, clave: &str, valor: &str) -> Result<(), String> {
        self.0.lock().unwrap().insert(clave.into(), valor.into());
        Ok(())
    }
    fn borrar(&self, clave: &str) -> Result<(), String> {
        self.0.lock().unwrap().remove(clave);
        Ok(())
    }
}

/// En un archivo JSON (solo pruebas de interfaz).
pub struct Archivo(pub PathBuf);

impl Archivo {
    fn todo(&self) -> BTreeMap<String, String> {
        std::fs::read(&self.0)
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default()
    }
}

impl Llavero for Archivo {
    fn leer(&self, clave: &str) -> Result<Option<String>, String> {
        Ok(self.todo().get(clave).cloned())
    }
    fn guardar(&self, clave: &str, valor: &str) -> Result<(), String> {
        let mut t = self.todo();
        t.insert(clave.into(), valor.into());
        std::fs::write(&self.0, serde_json::to_vec(&t).unwrap()).map_err(|e| e.to_string())
    }
    fn borrar(&self, clave: &str) -> Result<(), String> {
        let mut t = self.todo();
        t.remove(clave);
        std::fs::write(&self.0, serde_json::to_vec(&t).unwrap()).map_err(|e| e.to_string())
    }
}

// --- libsecret ---------------------------------------------------------------------------------

#[repr(C)]
struct AtributoEsquema {
    nombre: *const c_char,
    tipo: c_int,
}

#[repr(C)]
struct Esquema {
    nombre: *const c_char,
    banderas: c_int,
    atributos: [AtributoEsquema; 32],
    reservado: c_int,
    reservados: [*mut c_void; 7],
}

// SAFETY: el esquema es inmutable y sus punteros apuntan a cadenas estáticas.
unsafe impl Sync for Esquema {}

const NULO: AtributoEsquema = AtributoEsquema {
    nombre: std::ptr::null(),
    tipo: 0,
};

static ESQUEMA: Esquema = Esquema {
    nombre: c"io.github.caracelp.Orbital.Drive".as_ptr(),
    banderas: 0, // SECRET_SCHEMA_NONE
    atributos: {
        let mut a = [NULO; 32];
        a[0] = AtributoEsquema {
            nombre: c"clave".as_ptr(),
            tipo: 0, // SECRET_SCHEMA_ATTRIBUTE_STRING
        };
        a
    },
    reservado: 0,
    reservados: [std::ptr::null_mut(); 7],
};

#[link(name = "secret-1")]
unsafe extern "C" {
    fn secret_password_store_sync(
        schema: *const Esquema,
        collection: *const c_char,
        label: *const c_char,
        password: *const c_char,
        cancellable: *mut c_void,
        error: *mut *mut gio::glib::ffi::GError,
        ...
    ) -> c_int;
    fn secret_password_lookup_sync(
        schema: *const Esquema,
        cancellable: *mut c_void,
        error: *mut *mut gio::glib::ffi::GError,
        ...
    ) -> *mut c_char;
    fn secret_password_clear_sync(
        schema: *const Esquema,
        cancellable: *mut c_void,
        error: *mut *mut gio::glib::ffi::GError,
        ...
    ) -> c_int;
    fn secret_password_free(password: *mut c_char);
}

/// Toma el mensaje de un `GError` y lo libera.
unsafe fn mensaje(e: *mut gio::glib::ffi::GError) -> String {
    // SAFETY: `e` no es nulo y viene de libsecret.
    unsafe {
        let m = CStr::from_ptr((*e).message).to_string_lossy().into_owned();
        gio::glib::ffi::g_error_free(e);
        m
    }
}

/// El llavero del sistema (Secret Service: gnome-keyring, KWallet…).
pub struct Sistema;

impl Llavero for Sistema {
    fn leer(&self, clave: &str) -> Result<Option<String>, String> {
        let c = CString::new(clave).map_err(|e| e.to_string())?;
        let mut err = std::ptr::null_mut();
        // SAFETY: argumentos variádicos terminados en NULL, como pide libsecret.
        let p = unsafe {
            secret_password_lookup_sync(
                &ESQUEMA,
                std::ptr::null_mut(),
                &mut err,
                c"clave".as_ptr(),
                c.as_ptr(),
                std::ptr::null::<c_char>(),
            )
        };
        if !err.is_null() {
            // SAFETY: lo devolvió libsecret.
            return Err(unsafe { mensaje(err) });
        }
        if p.is_null() {
            return Ok(None);
        }
        // SAFETY: `p` es una cadena de libsecret, que se libera con su función.
        let v = unsafe { CStr::from_ptr(p).to_string_lossy().into_owned() };
        unsafe { secret_password_free(p) };
        Ok(Some(v))
    }

    fn guardar(&self, clave: &str, valor: &str) -> Result<(), String> {
        let c = CString::new(clave).map_err(|e| e.to_string())?;
        let v = CString::new(valor).map_err(|e| e.to_string())?;
        let etiqueta = CString::new(format!("Orbital: Google Drive ({clave})")).unwrap();
        let mut err = std::ptr::null_mut();
        // SAFETY: argumentos variádicos terminados en NULL; "default" es SECRET_COLLECTION_DEFAULT.
        let ok = unsafe {
            secret_password_store_sync(
                &ESQUEMA,
                c"default".as_ptr(),
                etiqueta.as_ptr(),
                v.as_ptr(),
                std::ptr::null_mut(),
                &mut err,
                c"clave".as_ptr(),
                c.as_ptr(),
                std::ptr::null::<c_char>(),
            )
        };
        if !err.is_null() {
            // SAFETY: lo devolvió libsecret.
            return Err(unsafe { mensaje(err) });
        }
        if ok == 0 {
            return Err("el llavero no guardó la credencial".into());
        }
        Ok(())
    }

    fn borrar(&self, clave: &str) -> Result<(), String> {
        let c = CString::new(clave).map_err(|e| e.to_string())?;
        let mut err = std::ptr::null_mut();
        // SAFETY: argumentos variádicos terminados en NULL.
        unsafe {
            secret_password_clear_sync(
                &ESQUEMA,
                std::ptr::null_mut(),
                &mut err,
                c"clave".as_ptr(),
                c.as_ptr(),
                std::ptr::null::<c_char>(),
            )
        };
        if !err.is_null() {
            // SAFETY: lo devolvió libsecret.
            return Err(unsafe { mensaje(err) });
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ida_y_vuelta(l: &dyn Llavero) {
        let c = Cliente {
            id: "123.apps.googleusercontent.com".into(),
            secreto: "GOCSPX-prueba".into(),
        };
        guardar_cliente(l, &c).unwrap();
        assert_eq!(leer_cliente(l).unwrap(), Some(c));
        guardar_cuenta(l, "a@b.c", "1//renovar").unwrap();
        assert_eq!(
            leer_cuenta(l, "a@b.c").unwrap().as_deref(),
            Some("1//renovar")
        );
        borrar_cuenta(l, "a@b.c").unwrap();
        assert_eq!(leer_cuenta(l, "a@b.c").unwrap(), None);
        l.borrar(CLAVE_CLIENTE).unwrap();
    }

    #[test]
    fn memoria_y_archivo() {
        ida_y_vuelta(&Memoria::default());
        let p = std::env::temp_dir().join(format!("orbital-llavero-{}.json", std::process::id()));
        ida_y_vuelta(&Archivo(p.clone()));
        let _ = std::fs::remove_file(p);
    }

    /// El llavero del sistema. **Nunca contra el de la usuaria**: correr en una sesión de D-Bus
    /// propia con un gnome-keyring de prueba (ver el quickstart de la 061):
    /// `tools/llavero-de-prueba.sh cargo test -p orbital-drive llavero_del_sistema -- --ignored`.
    #[test]
    #[ignore]
    fn llavero_del_sistema() {
        assert_eq!(
            std::env::var("ORBITAL_LLAVERO_DE_PRUEBA").as_deref(),
            Ok("1"),
            "solo dentro de tools/llavero-de-prueba.sh"
        );
        ida_y_vuelta(&Sistema);
    }
}
