//! Resaltado de sintaxis de los bloques de código cercados (055) fuera de la vista Código: en la
//! vista formateada y al exportar.
//!
//! No hay un segundo motor: el texto del bloque se pasa por un `sourceview5::Buffer` de trabajo
//! con el lenguaje de GtkSourceView y el esquema `orbital-dark` de Código, y se leen los tramos
//! que dejó coloreados. Cada color del esquema es un [`Rol`]; la Lectura lo pinta con el mismo
//! color que Código y la exportación con su par para papel blanco.
//!
//! GtkSourceView solo se usa desde el hilo de GTK. La vista formateada vuelca el bloque sin color
//! y lo resalta después, en reposo y por partes ([`avanzar`]); el resultado queda en caché por
//! (lenguaje, texto), así que al reabrir la nota o volver a volcar un bloque sale con color en el
//! mismo volcado. Un buffer de trabajo por lenguaje, para que GtkSourceView no vuelva a compilar
//! la definición del lenguaje en cada bloque.

use std::cell::RefCell;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::rc::Rc;

use gtk::prelude::*;
use sourceview5::prelude::*;

/// Con más texto que esto el bloque queda sin color (≈ 6000 renglones, unos 200 ms de reposo en
/// el N5100).
const MAX_TEXTO: usize = 256 * 1024;
/// Entradas de la caché antes de vaciarla.
const MAX_CACHE: usize = 1024;

/// Qué es un tramo coloreado, según el color que le dio el esquema `orbital-dark`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Rol {
    /// Palabras clave, sentencias, preprocesador, caracteres especiales (rosa de acento).
    Clave,
    Comentario,
    Cadena,
    /// Tipos y constantes.
    Tipo,
    /// Sin color propio (identificadores): solo negrita o cursiva.
    Texto,
}

/// Un tramo resaltado, en caracteres desde el comienzo del texto del bloque.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Tramo {
    pub inicio: u32,
    pub fin: u32,
    pub rol: Rol,
    pub negrita: bool,
    pub cursiva: bool,
}

/// Todos los roles (para crear las etiquetas de la vista una vez).
pub const ROLES: [Rol; 5] = [
    Rol::Clave,
    Rol::Comentario,
    Rol::Cadena,
    Rol::Tipo,
    Rol::Texto,
];

impl Rol {
    /// El color en la vista oscura: el mismo que en Código.
    pub fn color(self) -> crate::style::Rgb {
        use crate::style::*;
        match self {
            Rol::Clave => SYNTAX_KEYWORD,
            Rol::Comentario => SYNTAX_COMMENT,
            Rol::Cadena => SYNTAX_STRING,
            Rol::Tipo => SYNTAX_TYPE,
            Rol::Texto => SYNTAX_TEXT,
        }
    }

    /// El color sobre papel blanco (exportar): los mismos tonos, oscurecidos. Lo usa
    /// [`CSS_PAPEL`], escrito a mano; una prueba verifica que coincidan y su contraste.
    #[cfg(test)]
    pub fn color_papel(self) -> crate::style::Rgb {
        use crate::style::Rgb;
        match self {
            Rol::Clave => Rgb(0xb3, 0x13, 0x5f),
            Rol::Comentario => Rgb(0x5f, 0x5f, 0x66),
            Rol::Cadena => Rgb(0x8c, 0x2f, 0x6b),
            Rol::Tipo => Rgb(0x7b, 0x2a, 0x8c),
            Rol::Texto => Rgb(0x1a, 0x1a, 0x1c),
        }
    }

    /// Clase CSS en lo exportado.
    pub fn clase(self) -> &'static str {
        match self {
            Rol::Clave => "sx-c",
            Rol::Comentario => "sx-m",
            Rol::Cadena => "sx-s",
            Rol::Tipo => "sx-t",
            Rol::Texto => "sx-x",
        }
    }

    fn de_color(c: &gtk::gdk::RGBA) -> Rol {
        use crate::style::*;
        let rgb = crate::style::Rgb(
            (c.red() * 255.0).round() as u8,
            (c.green() * 255.0).round() as u8,
            (c.blue() * 255.0).round() as u8,
        );
        match rgb {
            x if x == SYNTAX_KEYWORD => Rol::Clave,
            x if x == SYNTAX_COMMENT => Rol::Comentario,
            x if x == SYNTAX_STRING => Rol::Cadena,
            x if x == SYNTAX_TYPE => Rol::Tipo,
            _ => Rol::Texto,
        }
    }
}

/// Lenguajes con otro nombre en GtkSourceView que la extensión no resuelve.
const ALIAS: &[(&str, &str)] = &[
    ("javascript", "js"),
    ("node", "js"),
    ("jsx", "js"),
    ("ts", "typescript"),
    ("tsx", "typescript-jsx"),
    ("shell", "sh"),
    ("bash", "sh"),
    ("zsh", "sh"),
    ("console", "sh"),
    ("py", "python3"),
    ("python", "python3"),
    ("py3", "python3"),
    ("rs", "rust"),
    ("yml", "yaml"),
    ("c++", "cpp"),
    ("cs", "c-sharp"),
    ("csharp", "c-sharp"),
    ("golang", "go"),
    ("htm", "html"),
    ("jsonc", "json"),
    ("tex", "latex"),
    ("make", "makefile"),
    ("patch", "diff"),
    ("ps1", "powershell"),
    ("kt", "kotlin"),
    ("rb", "ruby"),
    ("md", "markdown"),
];

/// Lenguajes que la Lectura dibuja de otro modo y no son código (fórmulas, gráficos, generados).
const ESPECIALES: &[&str] = &["math", "grafico", "indice", "toc", "consulta"];

/// El id de GtkSourceView para la etiqueta de un bloque cercado (` ```py ` → `python3`), o `None`.
fn id_lenguaje(lm: &sourceview5::LanguageManager, lang: &str) -> Option<sourceview5::Language> {
    let lang = lang.trim().to_ascii_lowercase();
    if lang.is_empty() || ESPECIALES.contains(&lang.as_str()) {
        return None;
    }
    let id = ALIAS
        .iter()
        .find(|(a, _)| *a == lang)
        .map_or(lang.as_str(), |(_, id)| id);
    lm.language(id).or_else(|| {
        // La extensión como nombre de archivo (`rs`, `toml`, `dockerfile`…): las mismas reglas con
        // que GtkSourceView elige el lenguaje de un archivo. Sin tipo de contenido (no consulta
        // la base MIME).
        if lang.contains(['/', '\\']) {
            return None;
        }
        lm.guess_language(Some(format!("x.{lang}").as_str()), None)
            .or_else(|| lm.guess_language(Some(lang.as_str()), None))
    })
}

#[derive(Clone, PartialEq, Eq, Hash)]
struct Clave {
    lang: String,
    hash: u64,
    largo: usize,
}

impl Clave {
    fn de(lang: &str, texto: &str) -> Self {
        let mut h = std::collections::hash_map::DefaultHasher::new();
        texto.hash(&mut h);
        Self {
            lang: lang.to_owned(),
            hash: h.finish(),
            largo: texto.len(),
        }
    }
}

/// El texto que se está resaltando por partes.
struct EnCurso {
    clave: Clave,
    buffer: sourceview5::Buffer,
    /// Bytes del texto ya pasados al buffer de trabajo.
    hecho: usize,
    tramos: Vec<Tramo>,
    estilos: HashMap<gtk::TextTag, (Option<Rol>, bool, bool)>,
}

#[derive(Default)]
struct Estado {
    /// `None`: GtkSourceView no conoce ese lenguaje.
    lenguajes: HashMap<String, Option<sourceview5::Buffer>>,
    cache: HashMap<Clave, Option<Rc<[Tramo]>>>,
    en_curso: Option<EnCurso>,
}

thread_local! {
    static ESTADO: RefCell<Estado> = RefCell::new(Estado::default());
}

/// Si un bloque con esta etiqueta podría tener resaltado, sin cargar nada: con lenguaje, no
/// especial, en el hilo de GTK y no conocido como desconocido.
pub fn puede(lang: &str) -> bool {
    en_cache(lang, "x") != Some(None)
}

/// Lo que ya se sabe de un bloque sin trabajar: `Some(None)` si queda sin color (sin lenguaje,
/// lenguaje desconocido, especial o demasiado largo), `Some(Some(_))` si ya está en la caché y
/// `None` si falta resaltarlo ([`avanzar`]).
pub fn en_cache(lang: &str, texto: &str) -> Option<Option<Rc<[Tramo]>>> {
    if lang.is_empty()
        || texto.is_empty()
        || texto.len() > MAX_TEXTO
        || ESPECIALES.contains(&lang)
        || !gtk::is_initialized_main_thread()
    {
        return Some(None);
    }
    ESTADO.with(|e| {
        let e = e.borrow();
        if let Some(None) = e.lenguajes.get(lang) {
            return Some(None);
        }
        e.cache.get(&Clave::de(lang, texto)).cloned()
    })
}

fn buffer_de(lang: &str) -> Option<sourceview5::Buffer> {
    ESTADO.with(|e| {
        if let Some(b) = e.borrow().lenguajes.get(lang) {
            return b.clone();
        }
        let lm = sourceview5::LanguageManager::default();
        let buf = id_lenguaje(&lm, lang).map(|l| {
            crate::editor::code::ensure_scheme();
            let b = sourceview5::Buffer::with_language(&l);
            b.set_highlight_syntax(true);
            b.set_highlight_matching_brackets(false);
            b.set_implicit_trailing_newline(false);
            if let Some(s) = sourceview5::StyleSchemeManager::default().scheme("orbital-dark") {
                b.set_style_scheme(Some(&s));
            }
            b
        });
        e.borrow_mut()
            .lenguajes
            .insert(lang.to_owned(), buf.clone());
        buf
    })
}

/// Renglones que se pasan al buffer de trabajo de una vez (≈ 1–2 ms en el N5100).
const RENGLONES_POR_PASO: usize = 48;

/// Resalta `texto` en `lang` hasta `limite`. Devuelve `true` cuando terminó (el resultado queda
/// en la caché: [`en_cache`]); `false` si falta, y se sigue en la próxima llamada con el mismo
/// texto. Cargar un lenguaje por primera vez (decenas de milisegundos) es un paso propio.
///
/// El texto se agrega al buffer de trabajo por tandas de renglones y se resalta cada tanda en el
/// acto: con el texto entero puesto, GtkSourceView lo analizaría solo en su propio reposo, en
/// tramos de hasta 30 ms.
pub fn avanzar(lang: &str, texto: &str, limite: std::time::Instant) -> bool {
    if en_cache(lang, texto).is_some() {
        return true;
    }
    let clave = Clave::de(lang, texto);
    let previo = ESTADO.with(|e| e.borrow_mut().en_curso.take());
    let mut curso = match previo {
        Some(c) if c.clave == clave => c,
        _ => {
            let nuevo = !ESTADO.with(|e| e.borrow().lenguajes.contains_key(lang));
            let Some(buffer) = buffer_de(lang) else {
                return true;
            };
            buffer.set_text("");
            let c = EnCurso {
                clave,
                buffer,
                hecho: 0,
                tramos: Vec::new(),
                estilos: HashMap::new(),
            };
            if nuevo {
                ESTADO.with(|e| e.borrow_mut().en_curso = Some(c));
                return false;
            }
            c
        }
    };
    loop {
        let resto = &texto[curso.hecho..];
        let corte = resto
            .match_indices('\n')
            .nth(RENGLONES_POR_PASO - 1)
            .map_or(resto.len(), |(n, _)| n + 1);
        let tanda = &resto[..corte];
        let buffer = &curso.buffer;
        let desde = buffer.end_iter().offset();
        buffer.insert(&mut buffer.end_iter(), tanda);
        curso.hecho += corte;
        let (a, b) = (buffer.iter_at_offset(desde), buffer.end_iter());
        // La primera tanda que nace con un lenguaje recién cargado compila sus expresiones.
        buffer.ensure_highlight(&a, &b);
        leer(&a, &b, &mut curso.tramos, &mut curso.estilos);
        if curso.hecho >= texto.len() {
            curso.buffer.set_text("");
            let t: Option<Rc<[Tramo]>> =
                (!curso.tramos.is_empty()).then(|| std::mem::take(&mut curso.tramos).into());
            ESTADO.with(|e| {
                let mut e = e.borrow_mut();
                if e.cache.len() >= MAX_CACHE {
                    e.cache.clear();
                }
                e.cache.insert(curso.clave, t);
            });
            return true;
        }
        if std::time::Instant::now() >= limite {
            ESTADO.with(|e| e.borrow_mut().en_curso = Some(curso));
            return false;
        }
    }
}

/// Resalta `texto` de una vez (exportar). `None` si queda sin color.
pub fn tramos(lang: &str, texto: &str) -> Option<Rc<[Tramo]>> {
    if let Some(t) = en_cache(lang, texto) {
        return t;
    }
    let lejos = std::time::Instant::now() + std::time::Duration::from_secs(3600);
    while !avanzar(lang, texto, lejos) {}
    en_cache(lang, texto).flatten()
}

/// Hasta este largo, un bloque de un lenguaje ya cargado se resalta en el acto al volver a
/// volcarlo en el sitio (≈ 100 renglones, 2–3 ms): al escribir en Dividida no parpadea sin color.
const MAX_EN_EL_ACTO: usize = 4096;

/// Como [`en_cache`], pero un bloque corto de un lenguaje ya cargado se resalta en el acto.
pub fn en_el_acto(lang: &str, texto: &str) -> Option<Option<Rc<[Tramo]>>> {
    let r = en_cache(lang, texto);
    if r.is_some()
        || texto.len() > MAX_EN_EL_ACTO
        || !ESTADO.with(|e| matches!(e.borrow().lenguajes.get(lang), Some(Some(_))))
    {
        return r;
    }
    Some(tramos(lang, texto))
}

/// Si el lenguaje tiene resaltado (carga el lenguaje si hace falta).
#[cfg(test)]
pub fn conocido(lang: &str) -> bool {
    en_cache(lang, "x") != Some(None) && buffer_de(lang).is_some()
}

/// Cómo pinta una etiqueta de GtkSourceView: (rol si tiene color, negrita, cursiva).
fn estilo(tag: &gtk::TextTag) -> (Option<Rol>, bool, bool) {
    let rol = tag
        .is_foreground_set()
        .then(|| tag.foreground_rgba())
        .flatten()
        .map(|c| Rol::de_color(&c));
    let negrita = tag.is_weight_set() && tag.weight() >= 600;
    let cursiva = tag.is_style_set() && tag.style() == gtk::pango::Style::Italic;
    (rol, negrita, cursiva)
}

/// Agrega a `out` los tramos con estilo entre `a` y `b`, en caracteres desde el comienzo del
/// buffer; une un tramo con el anterior si siguen con el mismo estilo.
fn leer(
    a: &gtk::TextIter,
    b: &gtk::TextIter,
    out: &mut Vec<Tramo>,
    estilos: &mut HashMap<gtk::TextTag, (Option<Rol>, bool, bool)>,
) {
    let mut it = *a;
    while it < *b {
        let desde = it.offset();
        let tags = it.tags();
        if !it.forward_to_tag_toggle(None::<&gtk::TextTag>) || it > *b {
            it = *b;
        }
        let hasta = it.offset();
        if tags.is_empty() || hasta <= desde {
            continue;
        }
        // La de más prioridad con color gana (como al dibujar); negrita o cursiva de cualquiera.
        let (mut rol, mut prio, mut negrita, mut cursiva) = (None, -1, false, false);
        for t in &tags {
            let (r, n, c) = *estilos.entry(t.clone()).or_insert_with(|| estilo(t));
            if r.is_some() && t.priority() > prio {
                rol = r;
                prio = t.priority();
            }
            negrita |= n;
            cursiva |= c;
        }
        let rol = rol.unwrap_or(Rol::Texto);
        if rol == Rol::Texto && !negrita && !cursiva {
            continue;
        }
        let (inicio, fin) = (desde as u32, hasta as u32);
        match out.last_mut() {
            Some(u)
                if u.fin == inicio
                    && u.rol == rol
                    && u.negrita == negrita
                    && u.cursiva == cursiva =>
            {
                u.fin = fin
            }
            _ => out.push(Tramo {
                inicio,
                fin,
                rol,
                negrita,
                cursiva,
            }),
        }
    }
}

/// El texto del bloque como HTML con un `<span class>` por tramo (exportar). `None` sin resaltado.
pub fn html(lang: &str, texto: &str) -> Option<String> {
    tramos(lang, texto).map(|t| html_de(texto, &t))
}

/// `texto` escapado para HTML con un `<span>` por tramo.
pub fn html_de(texto: &str, tramos: &[Tramo]) -> String {
    let mut out = String::with_capacity(texto.len() + tramos.len() * 24);
    let mut tramos = tramos.iter().peekable();
    let mut abierto: Option<&Tramo> = None;
    for (n, ch) in texto.chars().enumerate() {
        let n = n as u32;
        if abierto.is_some_and(|t| t.fin == n) {
            out.push_str("</span>");
            abierto = None;
        }
        if abierto.is_none()
            && let Some(t) = tramos.next_if(|t| t.inicio == n)
        {
            out.push_str("<span class=\"");
            out.push_str(t.rol.clase());
            if t.negrita {
                out.push_str(" sx-b");
            }
            if t.cursiva {
                out.push_str(" sx-i");
            }
            out.push_str("\">");
            abierto = Some(t);
        }
        match ch {
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '&' => out.push_str("&amp;"),
            c => out.push(c),
        }
    }
    if abierto.is_some() {
        out.push_str("</span>");
    }
    out
}

/// CSS de las clases de [`html`] sobre papel blanco (los colores de [`Rol::color_papel`]; una
/// prueba lo verifica).
pub const CSS_PAPEL: &str = ".sx-c{color:#b3135f}.sx-m{color:#5f5f66}.sx-s{color:#8c2f6b}\
.sx-t{color:#7b2a8c}.sx-b{font-weight:700}.sx-i{font-style:italic}";

#[cfg(test)]
#[path = "resaltado_tests.rs"]
mod resaltado_tests;
