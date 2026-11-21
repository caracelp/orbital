//! Las fuentes con que `resvg` escribe los textos de los diagramas.
//!
//! Cargar todas las fuentes del sistema (`fontdb::Database::load_system_fonts`) lee la cabecera
//! de cada archivo: con unas 1000 fuentes instaladas, segundos la primera vez en el N5100. Acá se
//! cargan solo las caras que hacen falta (la letra de la interfaz en normal, negrita, cursiva y
//! negrita cursiva, la `sans-serif` del sistema como respaldo y la monoespaciada), que se buscan
//! con fontconfig: GTK ya lo tiene cargado, así que no suma bibliotecas.

use std::ffi::{CStr, CString, c_char, c_int, c_void};
use std::path::PathBuf;
use std::sync::{Arc, LazyLock, Mutex};

use resvg::usvg;

/// Las fuentes (y las opciones de `usvg`) para una familia de letra.
pub struct Fuentes {
    opciones: usvg::Options<'static>,
    familia: String,
    /// La familia que se pidió (puede no estar instalada y caer en otra).
    pedida: String,
    caras: usize,
}

impl Fuentes {
    pub(crate) fn opciones(&self) -> &usvg::Options<'static> {
        &self.opciones
    }

    /// La familia con que se escriben los textos.
    pub fn familia(&self) -> &str {
        &self.familia
    }

    /// Cuántas caras se cargaron.
    pub fn caras(&self) -> usize {
        self.caras
    }

    /// Las fuentes para la familia `familia` (la de la interfaz), cargadas una sola vez por
    /// familia. Se puede llamar desde cualquier hilo; la primera vez lee los archivos.
    pub fn para(familia: &str) -> Arc<Fuentes> {
        static CARGADAS: LazyLock<Mutex<Vec<Arc<Fuentes>>>> = LazyLock::new(Mutex::default);
        let mut cargadas = CARGADAS.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(f) = cargadas.iter().find(|f| f.pedida(familia)) {
            return f.clone();
        }
        let f = Arc::new(Self::cargar(familia));
        cargadas.push(f.clone());
        f
    }

    fn pedida(&self, familia: &str) -> bool {
        self.pedida == familia
    }

    fn cargar(familia: &str) -> Self {
        let mut db = usvg::fontdb::Database::new();
        let pedidas = [
            familia.to_owned(),
            format!("{familia}:bold"),
            format!("{familia}:italic"),
            format!("{familia}:bold:italic"),
            "sans-serif".to_owned(),
            "sans-serif:bold".to_owned(),
            "monospace".to_owned(),
        ];
        let mut vistos: Vec<PathBuf> = Vec::new();
        let mut nombre = None;
        for p in &pedidas {
            let Some((ruta, fam)) = buscar(p) else {
                continue;
            };
            if nombre.is_none() {
                nombre = Some(fam);
            }
            if vistos.contains(&ruta) {
                continue;
            }
            if let Ok(datos) = std::fs::read(&ruta) {
                db.load_font_data(datos);
            }
            vistos.push(ruta);
        }
        let familia_real = nombre.unwrap_or_else(|| familia.to_owned());
        // Lo que pida el SVG (`sans-serif`, `Inter`, `Arial`…) cae en la letra de la interfaz.
        db.set_sans_serif_family(familia_real.clone());
        db.set_serif_family(familia_real.clone());
        let caras = db.len();
        let mut opciones = usvg::Options {
            font_family: familia_real.clone(),
            ..usvg::Options::default()
        };
        *opciones.fontdb_mut() = db;
        Self {
            opciones,
            familia: familia_real,
            pedida: familia.to_owned(),
            caras,
        }
    }
}

// fontconfig: lo justo para «qué archivo tiene esta letra».
#[allow(non_camel_case_types)]
type FcPattern = c_void;
const FC_MATCH_PATTERN: c_int = 0;
const FC_RESULT_MATCH: c_int = 0;

#[link(name = "fontconfig")]
unsafe extern "C" {
    fn FcNameParse(name: *const u8) -> *mut FcPattern;
    fn FcConfigSubstitute(config: *mut c_void, p: *mut FcPattern, kind: c_int) -> c_int;
    fn FcDefaultSubstitute(p: *mut FcPattern);
    fn FcFontMatch(config: *mut c_void, p: *mut FcPattern, result: *mut c_int) -> *mut FcPattern;
    fn FcPatternGetString(
        p: *const FcPattern,
        object: *const c_char,
        n: c_int,
        s: *mut *mut u8,
    ) -> c_int;
    fn FcPatternDestroy(p: *mut FcPattern);
}

/// El archivo y la familia que fontconfig elige para `patron` (`Adwaita Sans:bold`).
fn buscar(patron: &str) -> Option<(PathBuf, String)> {
    let c = CString::new(patron).ok()?;
    // SAFETY: fontconfig es seguro entre hilos desde 2.10 para estas llamadas (configuración por
    // defecto, patrones propios); cada patrón creado se destruye acá, y las cadenas que devuelve
    // `FcPatternGetString` viven mientras vive el patrón, por eso se copian antes de destruirlo.
    unsafe {
        let pat = FcNameParse(c.as_ptr().cast());
        if pat.is_null() {
            return None;
        }
        FcConfigSubstitute(std::ptr::null_mut(), pat, FC_MATCH_PATTERN);
        FcDefaultSubstitute(pat);
        let mut res = 0;
        let m = FcFontMatch(std::ptr::null_mut(), pat, &mut res);
        FcPatternDestroy(pat);
        if m.is_null() {
            return None;
        }
        let leer = |obj: &CStr| {
            let mut s: *mut u8 = std::ptr::null_mut();
            (FcPatternGetString(m, obj.as_ptr(), 0, &mut s) == FC_RESULT_MATCH && !s.is_null())
                .then(|| CStr::from_ptr(s.cast()).to_string_lossy().into_owned())
        };
        let archivo = leer(c"file");
        let familia = leer(c"family");
        FcPatternDestroy(m);
        Some((PathBuf::from(archivo?), familia.unwrap_or_default()))
    }
}
