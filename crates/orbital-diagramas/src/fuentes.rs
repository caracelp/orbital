//! Las fuentes con que `resvg` escribe los textos de los diagramas.
//!
//! Cargar todas las fuentes del sistema (`fontdb::Database::load_system_fonts`) lee la cabecera
//! de cada archivo: con unas 1000 fuentes instaladas, segundos la primera vez en el N5100. Acá se
//! cargan solo las caras que hacen falta (la letra de la interfaz en normal, negrita, cursiva y
//! negrita cursiva, la `sans-serif` del sistema como respaldo y la monoespaciada), que se buscan
//! con fontconfig: GTK ya lo tiene cargado, así que no suma bibliotecas.
//!
//! Respaldo (059): si el texto de un diagrama trae letras que ninguna de esas caras tiene (emoji,
//! CJK, cirílico si la letra no lo trae…), se le pregunta a fontconfig por una cara que las tenga
//! y se suma, una sola vez por archivo. Un diagrama con texto ASCII no mira nada; uno con letras
//! ya vistas, solo una tabla.

use std::collections::HashMap;
use std::ffi::{CStr, CString, c_char, c_int, c_uint, c_void};
use std::path::PathBuf;
use std::sync::{Arc, LazyLock, Mutex};

use resvg::usvg;

/// Las fuentes (y las opciones de `usvg`) para una familia de letra.
pub struct Fuentes {
    familia: String,
    /// La familia que se pidió (puede no estar instalada y caer en otra).
    pedida: String,
    estado: Mutex<Estado>,
}

/// Lo que crece con los respaldos.
struct Estado {
    db: Arc<usvg::fontdb::Database>,
    /// Los archivos ya cargados: los `base` primeros son los de la interfaz.
    archivos: Vec<PathBuf>,
    base: usize,
    /// Si alguna cara cargada tiene cada letra no ASCII vista (después de buscar respaldo).
    cubiertas: HashMap<char, bool>,
    /// Las letras de cada archivo cargado (las de fontconfig: no hace falta leer la cara).
    letras: Vec<Letras>,
}

/// El conjunto de letras de una cara, de fontconfig (una referencia propia, que vive lo que el
/// proceso: las caras no se descargan).
struct Letras(*mut FcCharSet);

// SAFETY: un `FcCharSet` no cambia después de copiado y fontconfig lo consulta sin estado
// compartido; solo se lee con el `Mutex` de `Estado` tomado.
unsafe impl Send for Letras {}

impl Letras {
    fn tiene(&self, c: char) -> bool {
        // SAFETY: el puntero es una copia viva (nunca se destruye).
        unsafe { FcCharSetHasChar(self.0, c as c_uint) != 0 }
    }
}

impl Fuentes {
    /// Las opciones de `usvg` para dibujar `marcado`: antes, si su texto trae letras que las
    /// caras cargadas no tienen, se suman las de respaldo que fontconfig encuentre.
    pub(crate) fn opciones_para(&self, marcado: &str) -> usvg::Options<'static> {
        let db = {
            let mut e = self.estado.lock().unwrap_or_else(|e| e.into_inner());
            if !marcado.is_ascii() {
                e.cubrir(marcado, &self.familia);
            }
            e.db.clone()
        };
        usvg::Options {
            font_family: self.familia.clone(),
            fontdb: db,
            ..usvg::Options::default()
        }
    }

    /// La familia con que se escriben los textos.
    pub fn familia(&self) -> &str {
        &self.familia
    }

    /// Cuántas caras hay cargadas (con las de respaldo).
    pub fn caras(&self) -> usize {
        self.estado
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .db
            .len()
    }

    /// Los archivos de respaldo que se sumaron a las caras de la interfaz.
    pub fn respaldos(&self) -> usize {
        let e = self.estado.lock().unwrap_or_else(|e| e.into_inner());
        e.archivos.len().saturating_sub(e.base)
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
        let mut letras = Vec::new();
        let mut nombre = None;
        for p in &pedidas {
            let Some((ruta, fam, l)) = buscar(p) else {
                continue;
            };
            if nombre.is_none() {
                nombre = Some(fam);
            }
            if vistos.contains(&ruta) {
                continue;
            }
            cargar_archivo(&mut db, &ruta);
            vistos.push(ruta);
            letras.extend(l);
        }
        let familia_real = nombre.unwrap_or_else(|| familia.to_owned());
        // Lo que pida el SVG (`sans-serif`, `Inter`, `Arial`…) cae en la letra de la interfaz.
        db.set_sans_serif_family(familia_real.clone());
        db.set_serif_family(familia_real.clone());
        Self {
            familia: familia_real,
            pedida: familia.to_owned(),
            estado: Mutex::new(Estado {
                db: Arc::new(db),
                base: vistos.len(),
                archivos: vistos,
                cubiertas: HashMap::new(),
                letras,
            }),
        }
    }
}
impl Estado {
    /// Si algún archivo cargado tiene la letra `c`.
    fn tiene(&self, c: char) -> bool {
        self.letras.iter().any(|l| l.tiene(c))
    }

    /// Se asegura de que cada letra del texto de `marcado` tenga una cara: las que falten se
    /// buscan con fontconfig (una vez por letra; lo encontrado se carga una vez por archivo).
    fn cubrir(&mut self, marcado: &str, familia: &str) {
        let mut faltan: Vec<char> = Vec::new();
        for c in texto_no_ascii(marcado) {
            if self.cubiertas.contains_key(&c) || faltan.contains(&c) {
                continue;
            }
            if self.tiene(c) {
                self.cubiertas.insert(c, true);
            } else {
                faltan.push(c);
            }
        }
        for c in faltan {
            // Una cara cargada recién (por otra letra de este diagrama) puede tenerla ya.
            let mut hay = self.tiene(c);
            if !hay && let Some((ruta, l)) = respaldo(familia, c) {
                if !self.archivos.contains(&ruta) {
                    cargar_archivo(Arc::make_mut(&mut self.db), &ruta);
                    self.archivos.push(ruta);
                    self.letras.push(l);
                }
                hay = self.tiene(c);
            }
            self.cubiertas.insert(c, hay);
        }
    }
}

/// Suma las caras de un archivo, proyectado en memoria una sola vez: solo se leen las páginas
/// que se usan (una letra de emoji o CJK pesa 10–20 MB). Sin `make_shared_face_data`, `fontdb`
/// volvería a abrir y proyectar el archivo en cada consulta (4 veces más lenta la imagen).
fn cargar_archivo(db: &mut usvg::fontdb::Database, ruta: &std::path::Path) {
    if db.load_font_file(ruta).is_err() {
        return;
    }
    let nuevas: Vec<_> = db
        .faces()
        .filter(|f| matches!(&f.source, usvg::fontdb::Source::File(p) if p == ruta))
        .map(|f| f.id)
        .collect();
    if let Some(id) = nuevas.first() {
        // SAFETY: el archivo es una fuente del sistema que fontconfig acaba de dar; si otro
        // proceso lo cambiara mientras está proyectado, se leería lo nuevo (el mismo riesgo que
        // corren GTK y Pango, que también proyectan las fuentes). Una llamada comparte la
        // proyección con todas las caras del archivo.
        unsafe {
            db.make_shared_face_data(*id);
        }
    }
}

/// Las letras no ASCII del texto de un SVG (fuera de las etiquetas), sin las que no se dibujan
/// solas (espacios, controles, selectores de variante, unión de emoji).
fn texto_no_ascii(marcado: &str) -> impl Iterator<Item = char> + '_ {
    let mut en_etiqueta = false;
    marcado.chars().filter(move |&c| {
        match c {
            '<' => en_etiqueta = true,
            '>' => en_etiqueta = false,
            _ => {}
        }
        !en_etiqueta
            && !c.is_ascii()
            && !c.is_whitespace()
            && !c.is_control()
            && !matches!(c, '\u{200B}'..='\u{200F}' | '\u{FE00}'..='\u{FE0F}' | '\u{E0020}'..='\u{E007F}')
    })
}

/// Una letra que se ve mejor con la cara de emoji del sistema que con una de texto.
fn es_emoji(c: char) -> bool {
    matches!(c as u32, 0x1F000..=0x1FAFF | 0x2600..=0x27BF | 0x2B00..=0x2BFF | 0x2300..=0x23FF)
}

/// El archivo de una cara que tenga `c`, parecida a la letra de la interfaz (los emoji, de la
/// familia `emoji` del sistema).
fn respaldo(familia: &str, c: char) -> Option<(PathBuf, Letras)> {
    if es_emoji(c)
        && let Some(r) = buscar_con_letra("emoji", c)
    {
        return Some(r);
    }
    buscar_con_letra(familia, c).or_else(|| buscar_con_letra("sans-serif", c))
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
    fn FcCharSetCreate() -> *mut FcCharSet;
    fn FcCharSetAddChar(fcs: *mut FcCharSet, ucs4: c_uint) -> c_int;
    fn FcCharSetHasChar(fcs: *const FcCharSet, ucs4: c_uint) -> c_int;
    fn FcCharSetDestroy(fcs: *mut FcCharSet);
    fn FcCharSetCopy(src: *mut FcCharSet) -> *mut FcCharSet;
    fn FcPatternAddCharSet(p: *mut FcPattern, object: *const c_char, c: *const FcCharSet) -> c_int;
    fn FcPatternGetCharSet(
        p: *const FcPattern,
        object: *const c_char,
        n: c_int,
        c: *mut *mut FcCharSet,
    ) -> c_int;
    fn FcFontSort(
        config: *mut c_void,
        p: *mut FcPattern,
        trim: c_int,
        csp: *mut *mut FcCharSet,
        result: *mut c_int,
    ) -> *mut FcFontSet;
    fn FcFontSetDestroy(s: *mut FcFontSet);
}

#[allow(non_camel_case_types)]
type FcCharSet = c_void;

#[repr(C)]
struct FcFontSet {
    nfont: c_int,
    sfont: c_int,
    fonts: *mut *mut FcPattern,
}

/// El archivo de la primera cara que fontconfig ordena para `familia` y que de verdad tiene la
/// letra `c` (`FcFontMatch` solo la prefiere: puede elegir una que no la tenga).
fn buscar_con_letra(familia: &str, c: char) -> Option<(PathBuf, Letras)> {
    let nombre = CString::new(familia).ok()?;
    // SAFETY: como en `buscar`. El patrón, el conjunto de letras y la lista ordenada se crean y
    // se destruyen acá; las cadenas y conjuntos que se leen de la lista viven mientras ella vive
    // y la ruta se copia antes de destruirla.
    unsafe {
        let pat = FcNameParse(nombre.as_ptr().cast());
        if pat.is_null() {
            return None;
        }
        let letras = FcCharSetCreate();
        if !letras.is_null() {
            FcCharSetAddChar(letras, c as c_uint);
            FcPatternAddCharSet(pat, c"charset".as_ptr(), letras);
            FcCharSetDestroy(letras);
        }
        FcConfigSubstitute(std::ptr::null_mut(), pat, FC_MATCH_PATTERN);
        FcDefaultSubstitute(pat);
        let mut res = 0;
        let lista = FcFontSort(std::ptr::null_mut(), pat, 1, std::ptr::null_mut(), &mut res);
        FcPatternDestroy(pat);
        if lista.is_null() {
            return None;
        }
        let mut encontrada = None;
        let n = (*lista).nfont.max(0) as usize;
        for i in 0..n {
            let f = *(*lista).fonts.add(i);
            let mut cs: *mut FcCharSet = std::ptr::null_mut();
            if FcPatternGetCharSet(f, c"charset".as_ptr(), 0, &mut cs) != FC_RESULT_MATCH
                || cs.is_null()
                || FcCharSetHasChar(cs, c as c_uint) == 0
            {
                continue;
            }
            let mut s: *mut u8 = std::ptr::null_mut();
            if FcPatternGetString(f, c"file".as_ptr(), 0, &mut s) == FC_RESULT_MATCH && !s.is_null()
            {
                encontrada = Some((
                    PathBuf::from(CStr::from_ptr(s.cast()).to_string_lossy().into_owned()),
                    Letras(FcCharSetCopy(cs)),
                ));
                break;
            }
        }
        FcFontSetDestroy(lista);
        encontrada
    }
}

/// El archivo, la familia y las letras de la cara que fontconfig elige para `patron`
/// (`Adwaita Sans:bold`).
fn buscar(patron: &str) -> Option<(PathBuf, String, Option<Letras>)> {
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
        let mut cs: *mut FcCharSet = std::ptr::null_mut();
        let letras = (FcPatternGetCharSet(m, c"charset".as_ptr(), 0, &mut cs) == FC_RESULT_MATCH
            && !cs.is_null())
        .then(|| Letras(FcCharSetCopy(cs)));
        FcPatternDestroy(m);
        Some((PathBuf::from(archivo?), familia.unwrap_or_default(), letras))
    }
}
