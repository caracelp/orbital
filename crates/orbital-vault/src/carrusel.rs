//! Carrusel de medios (032): un codeblock ` ```carrusel ` (también ` ```media-slider `, el nombre
//! heredado) con una lista de medios de la bóveda que la Lectura muestra de a uno.
//!
//! El cuerpo tiene opciones `clave: valor` (opcionalmente entre dos renglones `---`) y un medio
//! por renglón: `![[figura.png|Leyenda]]`, `![Leyenda](figura.png)`, `figura.png` o `[[carpeta/]]`
//! (todos los medios de esa carpeta). Las opciones aceptan el nombre en español y el heredado
//! (`miniaturas` o `carouselShowThumbnails`); ver `docs/formato-boveda.md`.
//!
//! Sin GTK y sin disco: [`parse`] analiza el texto y [`Vault::resolver_carrusel`] resuelve cada
//! medio y las carpetas contra el índice de la bóveda (en el hilo de fondo que prepara la nota).

use crate::RelPath;
use crate::links::Resolution;
use crate::vault::Vault;

/// Medios por carpeta como máximo si no se dice otra cosa.
pub const MAXIMO: usize = 100;
/// Tope del máximo configurable.
pub const MAXIMO_TOPE: usize = 1000;
/// Segundos por diapositiva del pase automático si no se dice otra cosa.
pub const VELOCIDAD: f32 = 5.0;

/// Si un lenguaje de codeblock es un carrusel.
pub fn es_lenguaje(lang: &str) -> bool {
    matches!(lang, "carrusel" | "media-slider")
}

/// Dónde va la tira de miniaturas.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Posicion {
    Arriba,
    #[default]
    Abajo,
    Izquierda,
    Derecha,
}

/// Dónde va la leyenda de cada diapositiva.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Leyenda {
    /// Sobre el medio, abajo (`overlay`).
    #[default]
    Encima,
    /// Debajo del medio (`below`).
    Debajo,
}

/// Las opciones del carrusel.
#[derive(Clone, Debug, PartialEq)]
pub struct Opciones {
    pub id: Option<String>,
    pub miniaturas: bool,
    pub posicion: Posicion,
    pub leyenda: Leyenda,
    pub autoplay: bool,
    /// Segundos por diapositiva del pase automático.
    pub velocidad: f32,
    /// Botones de pantalla completa y de copiar el embed.
    pub ampliada: bool,
    /// Medios por carpeta como máximo.
    pub maximo: usize,
}

impl Default for Opciones {
    fn default() -> Self {
        Self {
            id: None,
            miniaturas: false,
            posicion: Posicion::default(),
            leyenda: Leyenda::default(),
            autoplay: false,
            velocidad: VELOCIDAD,
            ampliada: false,
            maximo: MAXIMO,
        }
    }
}

/// Qué es cada diapositiva (decide el visor).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Clase {
    Imagen,
    Audio,
    Video,
    Pdf,
    /// Otro archivo: se muestra como enlace.
    Otro,
}

impl Clase {
    /// La clase por la extensión del nombre.
    pub fn de(nombre: &str) -> Self {
        match crate::medios::tipo_de(nombre) {
            Some(crate::medios::Tipo::Imagen) => Self::Imagen,
            Some(crate::medios::Tipo::Audio) => Self::Audio,
            Some(crate::medios::Tipo::Video) => Self::Video,
            None if extension(nombre).as_deref() == Some("pdf") => Self::Pdf,
            None => Self::Otro,
        }
    }

    /// Nombre de la clase para la interfaz y el lector de pantalla.
    pub fn nombre(self) -> &'static str {
        match self {
            Self::Imagen => "Imagen",
            Self::Audio => "Audio",
            Self::Video => "Video",
            Self::Pdf => "PDF",
            Self::Otro => "Archivo",
        }
    }
}

/// Dónde quedó el archivo de una diapositiva.
#[derive(Clone, Debug, PartialEq)]
pub enum Estado {
    /// Todavía no se resolvió contra la bóveda.
    Pendiente,
    Encontrado {
        path: RelPath,
        abs: std::path::PathBuf,
    },
    /// No existe en la bóveda.
    Faltante,
}

/// Un medio del carrusel.
#[derive(Clone, Debug, PartialEq)]
pub struct Diapositiva {
    /// La ruta como está escrita (o la ruta en la bóveda si viene de una carpeta).
    pub escrito: String,
    /// Lo que sigue a `#` (`t=30` para audio y video, `page=3` para PDF).
    pub fragmento: Option<String>,
    pub leyenda: Option<String>,
    pub clase: Clase,
    pub estado: Estado,
    /// Viene de una fuente por carpeta.
    pub de_carpeta: bool,
    /// Enlace al archivo relativo a la carpeta de la nota (para abrir un PDF en su visor).
    pub enlace: Option<String>,
}

impl Diapositiva {
    fn nueva(escrito: &str, fragmento: Option<String>, leyenda: Option<String>) -> Self {
        Self {
            clase: Clase::de(escrito),
            escrito: escrito.to_owned(),
            fragmento,
            leyenda,
            estado: Estado::Pendiente,
            de_carpeta: false,
            enlace: None,
        }
    }

    /// El nombre del archivo, sin carpetas.
    pub fn nombre(&self) -> &str {
        self.escrito.rsplit('/').next().unwrap_or(&self.escrito)
    }

    /// El embed exacto para pegar en otra nota: `![[ruta#fragmento|leyenda]]`.
    pub fn embed(&self) -> String {
        let mut s = format!("![[{}", self.escrito);
        if let Some(f) = &self.fragmento {
            s.push('#');
            s.push_str(f);
        }
        if let Some(l) = &self.leyenda {
            s.push('|');
            s.push_str(l);
        }
        s.push_str("]]");
        s
    }

    /// El texto que lee el lector de pantalla: la clase, el nombre y la leyenda.
    pub fn descripcion(&self) -> String {
        match &self.leyenda {
            Some(l) => format!("{}: {} — {l}", self.clase.nombre(), self.nombre()),
            None => format!("{}: {}", self.clase.nombre(), self.nombre()),
        }
    }

    /// La ruta absoluta si el archivo existe.
    pub fn abs(&self) -> Option<&std::path::Path> {
        match &self.estado {
            Estado::Encontrado { abs, .. } => Some(abs),
            _ => None,
        }
    }
}

/// Una fuente por carpeta.
#[derive(Clone, Debug, PartialEq)]
pub struct FuenteCarpeta {
    /// La carpeta como está escrita.
    pub carpeta: String,
    /// Extensiones en minúscula y sin punto; vacío: todos los medios.
    pub tipos: Vec<String>,
    pub recursivo: bool,
    /// Lugar de la lista en que se insertan sus medios.
    pub lugar: usize,
}

/// Un carrusel analizado (y, después de [`Vault::resolver_carrusel`], resuelto).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Carrusel {
    pub opciones: Opciones,
    pub diapositivas: Vec<Diapositiva>,
    pub carpetas: Vec<FuenteCarpeta>,
    /// Lo que se ignoró o se cortó, para un aviso discreto debajo del carrusel.
    pub avisos: Vec<String>,
    /// Ya pasó por [`Vault::resolver_carrusel`].
    pub resuelto: bool,
}

/// Opciones que pertenecen a funciones posteriores del formato heredado (comparación, zoom,
/// transiciones, notas, dibujo, visualizador): se aceptan sin aviso y no hacen nada.
const POSTERGADAS: &[&str] = &[
    "transitioneffect",
    "transitionduration",
    "comparemode",
    "compareorientation",
    "compareinitialposition",
    "interactivenotes",
    "enablezoom",
    "zoom",
    "drawing",
    "enabledrawing",
    "audiovisualizer",
    "visualizer",
    "lazyload",
    "width",
    "height",
];

/// Analiza el cuerpo de un codeblock de carrusel. Nunca falla: lo que no entiende va a
/// [`Carrusel::avisos`].
pub fn parse(cuerpo: &str) -> Carrusel {
    let mut c = Carrusel::default();
    let mut renglones = cuerpo.lines().peekable();
    // Las opciones entre `---` (la forma heredada) o sueltas antes y entre los medios.
    let mut tipos: Option<Vec<String>> = None;
    let mut recursivo = false;
    let mut carpeta_opcion: Option<String> = None;
    let mut en_cabecera = false;
    let mut primera = true;
    // Clave con valor en los renglones siguientes (`fileTypes:` y debajo `- png`).
    let mut lista_abierta: Option<String> = None;
    while let Some(r) = renglones.next() {
        let t = r.trim();
        if t.is_empty() {
            continue;
        }
        if t == "---" {
            // Abre la cabecera si es lo primero; la cierra si estaba abierta.
            if primera || en_cabecera {
                en_cabecera = primera;
            }
            primera = false;
            lista_abierta = None;
            continue;
        }
        primera = false;
        if let Some(clave) = &lista_abierta
            && let Some(item) = t.strip_prefix("- ").or_else(|| t.strip_prefix('-'))
            && !item.trim_start().starts_with('!')
            && !item.trim_start().starts_with("[[")
        {
            let item = item.trim().trim_matches(['"', '\'']);
            match clave.as_str() {
                "filetypes" | "tipos" => tipos.get_or_insert_with(Vec::new).push(ext_limpia(item)),
                _ => {}
            }
            continue;
        }
        lista_abierta = None;
        if let Some((clave, valor)) = opcion(t) {
            let clave_min = clave.to_ascii_lowercase();
            if valor.is_empty() && matches!(clave_min.as_str(), "filetypes" | "tipos") {
                lista_abierta = Some(clave_min);
                continue;
            }
            aplicar(
                &mut c,
                &clave_min,
                clave,
                valor,
                &mut tipos,
                &mut recursivo,
                &mut carpeta_opcion,
            );
            continue;
        }
        if en_cabecera {
            c.avisos
                .push(format!("Renglón de opciones ignorado: «{}»", recortar(t)));
            continue;
        }
        medio(&mut c, t);
    }
    if let Some(carpeta) = carpeta_opcion {
        let lugar = c.diapositivas.len();
        c.carpetas.push(FuenteCarpeta {
            carpeta,
            tipos: Vec::new(),
            recursivo: false,
            lugar,
        });
    }
    // `fileTypes` y `recursive` valen para todas las carpetas.
    for f in &mut c.carpetas {
        f.recursivo |= recursivo;
        if let Some(t) = &tipos {
            f.tipos.clone_from(t);
        }
    }
    if c.diapositivas.is_empty() && c.carpetas.is_empty() {
        c.avisos.push("El carrusel no tiene medios".into());
    }
    c
}

/// `clave: valor` si el renglón es una opción (no un medio).
fn opcion(t: &str) -> Option<(&str, &str)> {
    if t.starts_with('!') || t.starts_with("[[") || t.starts_with('-') {
        return None;
    }
    let (clave, valor) = t.split_once(':')?;
    let clave = clave.trim();
    // Una ruta con dos puntos (`C:`, `http:`) no es una opción.
    if clave.is_empty()
        || !clave
            .chars()
            .all(|ch| ch.is_alphanumeric() || ch == '_' || ch == '-')
        || valor.starts_with("//")
    {
        return None;
    }
    Some((clave, valor.trim()))
}

#[allow(clippy::too_many_arguments)]
fn aplicar(
    c: &mut Carrusel,
    clave: &str,
    original: &str,
    valor: &str,
    tipos: &mut Option<Vec<String>>,
    recursivo: &mut bool,
    carpeta: &mut Option<String>,
) {
    let o = &mut c.opciones;
    let mal = |c: &mut Vec<String>| {
        c.push(format!(
            "Valor no válido para «{original}»: «{}»",
            recortar(valor)
        ))
    };
    match clave {
        "sliderid" | "id" => o.id = Some(sin_comillas(valor).to_owned()),
        "carouselshowthumbnails" | "miniaturas" => match booleano(valor) {
            Some(b) => o.miniaturas = b,
            None => mal(&mut c.avisos),
        },
        "thumbnailposition" | "posicionminiaturas" => {
            o.posicion = match sin_comillas(valor).to_ascii_lowercase().as_str() {
                "top" | "arriba" => Posicion::Arriba,
                "bottom" | "abajo" => Posicion::Abajo,
                "left" | "izquierda" => Posicion::Izquierda,
                "right" | "derecha" => Posicion::Derecha,
                _ => return mal(&mut c.avisos),
            }
        }
        "captionmode" | "leyenda" | "leyendas" => {
            o.leyenda = match sin_comillas(valor).to_ascii_lowercase().as_str() {
                "overlay" | "encima" | "sobre" => Leyenda::Encima,
                "below" | "debajo" => Leyenda::Debajo,
                _ => return mal(&mut c.avisos),
            }
        }
        "autoplay" | "paseautomatico" | "paseautomático" => match booleano(valor) {
            Some(b) => o.autoplay = b,
            None => mal(&mut c.avisos),
        },
        "slideshowspeed" | "velocidad" => {
            match sin_comillas(valor)
                .trim_end_matches('s')
                .trim()
                .replace(',', ".")
                .parse::<f32>()
            {
                Ok(v) if v.is_finite() && v > 0.0 => o.velocidad = v.clamp(1.0, 3600.0),
                _ => mal(&mut c.avisos),
            }
        }
        "enhancedview" | "vistaampliada" | "ampliada" => match booleano(valor) {
            Some(b) => o.ampliada = b,
            None => mal(&mut c.avisos),
        },
        "carpeta" | "folder" => {
            let v = sin_comillas(valor)
                .trim_start_matches("[[")
                .trim_end_matches("]]");
            let v = v.trim().trim_end_matches('/').trim_start_matches("./");
            if v.is_empty() {
                mal(&mut c.avisos);
            } else {
                *carpeta = Some(v.to_owned());
            }
        }
        "filetypes" | "tipos" => {
            let v = valor.trim().trim_start_matches('[').trim_end_matches(']');
            let lista: Vec<String> = v
                .split(',')
                .map(|e| ext_limpia(e.trim().trim_matches(['"', '\''])))
                .filter(|e| !e.is_empty())
                .collect();
            if lista.is_empty() {
                mal(&mut c.avisos);
            } else {
                *tipos = Some(lista);
            }
        }
        "recursive" | "recursivo" => match booleano(valor) {
            Some(b) => *recursivo = b,
            None => mal(&mut c.avisos),
        },
        "maximo" | "máximo" | "maxslides" | "max" => match sin_comillas(valor).parse::<usize>() {
            Ok(n) if n > 0 => o.maximo = n.min(MAXIMO_TOPE),
            _ => mal(&mut c.avisos),
        },
        otra if POSTERGADAS.contains(&otra) => {}
        _ => c
            .avisos
            .push(format!("Opción desconocida ignorada: «{original}»")),
    }
}

/// Un renglón de medio.
fn medio(c: &mut Carrusel, t: &str) {
    // Viñetas de lista: `- ![[a.png]]`.
    let t = t
        .strip_prefix("- ")
        .or_else(|| t.strip_prefix("* "))
        .unwrap_or(t)
        .trim();
    if t.contains("://") {
        c.avisos.push(format!(
            "Las direcciones externas no se cargan: «{}»",
            recortar(t)
        ));
        return;
    }
    // `[[carpeta/]]`: la fuente por carpeta heredada.
    if let Some(inner) = t.strip_prefix("[[").and_then(|x| x.strip_suffix("]]"))
        && inner.trim_end().ends_with('/')
    {
        let carpeta = inner.trim().trim_end_matches('/').trim_start_matches("./");
        if !carpeta.is_empty() {
            c.carpetas.push(FuenteCarpeta {
                carpeta: carpeta.to_owned(),
                tipos: Vec::new(),
                recursivo: false,
                lugar: c.diapositivas.len(),
            });
            return;
        }
    }
    // `![[ruta#frag|leyenda]]` (o sin `!`).
    if let Some(inner) = t
        .strip_prefix("![[")
        .or_else(|| t.strip_prefix("[["))
        .and_then(|x| x.strip_suffix("]]"))
        && !inner.contains("]]")
    {
        let lt = crate::links::parse_link_target(inner);
        let leyenda = lt.alias.filter(|a| !a.is_empty() && !es_medida(a));
        if lt.path.is_empty() {
            c.avisos
                .push(format!("Renglón ignorado: «{}»", recortar(t)));
            return;
        }
        let fragmento = lt.anchor.filter(|a| !a.is_empty());
        c.diapositivas
            .push(Diapositiva::nueva(&lt.path, fragmento, leyenda));
        return;
    }
    // `![leyenda](ruta)`.
    if let Some(resto) = t.strip_prefix("![")
        && let Some((alt, resto)) = resto.split_once("](")
        && let Some(url) = resto.strip_suffix(')')
    {
        let url = url.trim().trim_start_matches('<').trim_end_matches('>');
        let (ruta, fragmento) = url
            .split_once('#')
            .map_or((url, None), |(r, f)| (r, Some(f.to_owned())));
        let ruta = crate::links::percent_decode(ruta);
        if ruta.is_empty() {
            c.avisos
                .push(format!("Renglón ignorado: «{}»", recortar(t)));
            return;
        }
        let alt = alt.trim();
        let leyenda = (!alt.is_empty() && !es_medida(alt)).then(|| alt.to_owned());
        c.diapositivas
            .push(Diapositiva::nueva(&ruta, fragmento, leyenda));
        return;
    }
    // Una ruta suelta con extensión conocida.
    if !t.contains(' ') || Clase::de(t) != Clase::Otro {
        let (ruta, leyenda) = t
            .split_once('|')
            .map_or((t, None), |(r, l)| (r.trim(), Some(l.trim().to_owned())));
        if Clase::de(ruta) != Clase::Otro {
            let (ruta, fragmento) = ruta
                .split_once('#')
                .map_or((ruta, None), |(r, f)| (r, Some(f.to_owned())));
            c.diapositivas.push(Diapositiva::nueva(
                ruta,
                fragmento,
                leyenda.filter(|l| !l.is_empty()),
            ));
            return;
        }
    }
    c.avisos
        .push(format!("Renglón ignorado: «{}»", recortar(t)));
}

/// `300`, `300x200`: el ancho heredado de `![[a.png|300]]`, no una leyenda.
fn es_medida(s: &str) -> bool {
    let s = s.trim();
    !s.is_empty()
        && s.split('x')
            .all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()))
}

fn booleano(v: &str) -> Option<bool> {
    match sin_comillas(v).to_lowercase().as_str() {
        "true" | "sí" | "si" | "yes" | "1" | "on" => Some(true),
        "false" | "no" | "0" | "off" => Some(false),
        _ => None,
    }
}

fn sin_comillas(v: &str) -> &str {
    v.trim().trim_matches(['"', '\'']).trim()
}

fn ext_limpia(e: &str) -> String {
    e.trim()
        .trim_start_matches("*.")
        .trim_start_matches('.')
        .to_lowercase()
}

/// La extensión en minúscula.
fn extension(nombre: &str) -> Option<String> {
    let archivo = nombre.rsplit('/').next().unwrap_or(nombre);
    let (_, ext) = archivo.rsplit_once('.')?;
    (!ext.is_empty()).then(|| ext.to_lowercase())
}

/// Un renglón largo, cortado para un aviso.
fn recortar(t: &str) -> String {
    const N: usize = 60;
    if t.chars().count() <= N {
        t.to_owned()
    } else {
        let corte: String = t.chars().take(N).collect();
        format!("{corte}…")
    }
}

/// Si una extensión es de un medio que el carrusel muestra (imagen, audio, video o PDF).
fn es_medio_de_carpeta(ext: &str) -> bool {
    ext == "pdf"
        || crate::medios::IMAGEN.contains(&ext)
        || crate::medios::AUDIO.contains(&ext)
        || crate::medios::VIDEO.contains(&ext)
}

/// Enlace de `destino` visto desde la carpeta `dir` (con `..` si hace falta), con los espacios y
/// `#` escapados.
pub fn enlace_relativo(dir: &str, destino: &str) -> String {
    let a: Vec<&str> = dir.split('/').filter(|p| !p.is_empty()).collect();
    let b: Vec<&str> = destino.split('/').filter(|p| !p.is_empty()).collect();
    let comunes = a.iter().zip(&b).take_while(|(x, y)| x == y).count();
    let mut partes: Vec<&str> = std::iter::repeat_n("..", a.len() - comunes).collect();
    partes.extend(&b[comunes..]);
    partes
        .join("/")
        .replace('%', "%25")
        .replace(' ', "%20")
        .replace('#', "%23")
}

impl Vault {
    /// Resuelve los medios de un carrusel de la nota `from`: cada archivo escrito (con la misma
    /// regla que `![[…]]`, ver [`Vault::resolve_attachment`]) y las carpetas, contra el índice
    /// de la bóveda (sin leer el disco), en orden alfabético estable y con el máximo de
    /// [`Opciones::maximo`]. Idempotente.
    pub fn resolver_carrusel(&self, from: &RelPath, c: &mut Carrusel) {
        if c.resuelto {
            return;
        }
        c.resuelto = true;
        let dir = from.as_str().rsplit_once('/').map_or("", |(d, _)| d);
        for d in &mut c.diapositivas {
            d.estado = match self.resolve_attachment(from, &d.escrito) {
                Resolution::Found(path) => Estado::Encontrado {
                    abs: path.to_path(self.root()),
                    path,
                },
                _ => Estado::Faltante,
            };
        }
        // Las carpetas, de la última a la primera para que los lugares sigan valiendo.
        let carpetas = std::mem::take(&mut c.carpetas);
        let mut insertar: Vec<(usize, Vec<Diapositiva>)> = Vec::new();
        for f in &carpetas {
            let Some(base) = self.carpeta_de(dir, &f.carpeta) else {
                c.avisos
                    .push(format!("No existe la carpeta «{}»", f.carpeta));
                continue;
            };
            let prefijo = if base.is_empty() {
                String::new()
            } else {
                format!("{base}/")
            };
            let mut hallados: Vec<&RelPath> = self
                .entries()
                .iter()
                .filter(|e| e.kind != crate::EntryKind::Folder)
                .map(|e| &e.path)
                .filter(|p| {
                    let Some(resto) = p.as_str().strip_prefix(&prefijo) else {
                        return false;
                    };
                    (f.recursivo || !resto.contains('/'))
                        && !resto.split('/').any(|c| c.starts_with('.'))
                })
                .filter(|p| {
                    extension(p.as_str()).is_some_and(|e| {
                        if f.tipos.is_empty() {
                            es_medio_de_carpeta(&e)
                        } else {
                            f.tipos.contains(&e)
                        }
                    })
                })
                .collect();
            hallados.sort_by(|a, b| {
                let (a, b) = (a.as_str(), b.as_str());
                a.to_lowercase().cmp(&b.to_lowercase()).then(a.cmp(b))
            });
            // Lo escrito a mano (con su leyenda) no se repite.
            hallados.retain(|p| {
                !c.diapositivas
                    .iter()
                    .any(|d| d.abs() == Some(&*p.to_path(self.root())))
            });
            let total = hallados.len();
            if total > c.opciones.maximo {
                let fuera: Vec<&str> = hallados[c.opciones.maximo..]
                    .iter()
                    .take(3)
                    .map(|p| p.file_name())
                    .collect();
                let resto = total - c.opciones.maximo - fuera.len();
                let mas = if resto > 0 {
                    format!(" y {resto} más")
                } else {
                    String::new()
                };
                c.avisos.push(format!(
                    "La carpeta «{}» tiene {total} medios: entraron {} (máximo). Quedaron fuera {}{mas}",
                    f.carpeta,
                    c.opciones.maximo,
                    fuera.join(", ")
                ));
                hallados.truncate(c.opciones.maximo);
            }
            if total == 0 {
                c.avisos
                    .push(format!("La carpeta «{}» no tiene medios", f.carpeta));
            }
            let nuevas = hallados
                .into_iter()
                .map(|p| Diapositiva {
                    escrito: p.as_str().to_owned(),
                    fragmento: None,
                    leyenda: None,
                    clase: Clase::de(p.as_str()),
                    estado: Estado::Encontrado {
                        abs: p.to_path(self.root()),
                        path: p.clone(),
                    },
                    de_carpeta: true,
                    enlace: None,
                })
                .collect();
            insertar.push((f.lugar.min(c.diapositivas.len()), nuevas));
        }
        for (lugar, nuevas) in insertar.into_iter().rev() {
            c.diapositivas.splice(lugar..lugar, nuevas);
        }
        c.carpetas = carpetas;
        for d in &mut c.diapositivas {
            if let Estado::Encontrado { path, .. } = &d.estado {
                let mut e = enlace_relativo(dir, path.as_str());
                if let Some(f) = &d.fragmento {
                    e.push('#');
                    e.push_str(f);
                }
                d.enlace = Some(e);
            }
        }
        if c.diapositivas.is_empty() && !c.avisos.iter().any(|a| a.contains("no tiene medios")) {
            c.avisos.push("El carrusel no tiene medios".into());
        }
    }

    /// La carpeta de una fuente por carpeta: relativa a la raíz de la bóveda, a la carpeta de la
    /// nota o a `wiki/`. `Some("")`: la raíz.
    fn carpeta_de(&self, dir: &str, escrita: &str) -> Option<String> {
        let escrita = escrita.trim().trim_matches('/');
        if escrita.is_empty() || escrita == "." {
            return Some(String::new());
        }
        let es_carpeta = |rel: &str| {
            self.entries()
                .iter()
                .any(|e| e.kind == crate::EntryKind::Folder && e.path.as_str() == rel)
        };
        let candidatos = [
            escrita.to_owned(),
            if dir.is_empty() {
                escrita.to_owned()
            } else {
                format!("{dir}/{escrita}")
            },
            format!("wiki/{escrita}"),
        ];
        candidatos
            .into_iter()
            .filter(|c| !c.split('/').any(|p| p == ".." || p.starts_with('.')))
            .find(|c| es_carpeta(c))
    }
}

/// Escapa texto para HTML (contenido y atributos).
fn escapar(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            c => out.push(c),
        }
    }
    out
}

/// El carrusel en HTML estático (exportar y la Visual de las notas `.html`): una grilla con
/// cada medio y su leyenda, sin JavaScript. `src` da la dirección de cada archivo;
/// `reproducibles`: audio y video con controles (si no, el nombre y un enlace, para papel).
pub fn html(
    c: &Carrusel,
    src: &dyn Fn(&Diapositiva) -> Option<String>,
    reproducibles: bool,
) -> String {
    let n = c.diapositivas.len();
    let mut out = format!(
        "<div class=\"orbital-carrusel\" role=\"group\" aria-label=\"Carrusel: {n} {}\" \
style=\"display:grid;grid-template-columns:repeat(auto-fill,minmax(180px,1fr));gap:0.75em;margin:1em 0\">",
        if n == 1 { "medio" } else { "medios" }
    );
    for (i, d) in c.diapositivas.iter().enumerate() {
        let leyenda = d.leyenda.as_deref().unwrap_or_else(|| d.nombre());
        let nombre = escapar(d.nombre());
        out.push_str(&format!(
            "<figure style=\"margin:0;display:flex;flex-direction:column;gap:0.3em\" aria-label=\"{} de {n}: {}\">",
            i + 1,
            escapar(&d.descripcion())
        ));
        match (src(d), d.clase) {
            (None, _) => out.push_str(&format!(
                "<p style=\"opacity:0.7;margin:0\">Archivo inexistente: «{nombre}»</p>"
            )),
            (Some(s), Clase::Imagen) => out.push_str(&format!(
                "<img src=\"{}\" alt=\"{}\" loading=\"lazy\" style=\"width:100%;height:auto;max-height:240px;object-fit:contain\">",
                escapar(&s),
                escapar(leyenda)
            )),
            (Some(s), clase @ (Clase::Audio | Clase::Video)) if reproducibles => {
                let etiqueta = if clase == Clase::Video { "video" } else { "audio" };
                out.push_str(&format!(
                    "<{etiqueta} controls preload=\"none\" src=\"{}\" aria-label=\"{}: {nombre}\" style=\"width:100%\"></{etiqueta}>",
                    escapar(&s),
                    clase.nombre()
                ));
            }
            (Some(s), clase) => out.push_str(&format!(
                "<p style=\"margin:0\">{}: <a href=\"{}\">{nombre}</a></p>",
                clase.nombre(),
                escapar(&s)
            )),
        }
        if let Some(l) = &d.leyenda {
            out.push_str(&format!(
                "<figcaption style=\"font-size:0.9em;opacity:0.8\">{}</figcaption>",
                escapar(l)
            ));
        }
        out.push_str("</figure>");
    }
    out.push_str("</div>");
    for a in &c.avisos {
        out.push_str(&format!(
            "<p class=\"orbital-carrusel-aviso\" style=\"font-size:0.85em;opacity:0.7\">{}</p>",
            escapar(a)
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lista_con_leyendas_y_opciones_heredadas() {
        let c = parse(
            "---\nsliderId: tesis\ncarouselShowThumbnails: true\nthumbnailPosition: left\n\
captionMode: below\nautoplay: true\nslideshowSpeed: 3\nenhancedView: true\n---\n\
![[figura-1.png|Corte sagital]]\n![[clip.mp4#t=30]]\n![[a.png|300]]\n![Plano](fotos/plano%20b.jpg)\n- ![[doc.pdf#page=2|Informe]]\n",
        );
        assert!(c.avisos.is_empty(), "{:?}", c.avisos);
        let o = &c.opciones;
        assert_eq!(o.id.as_deref(), Some("tesis"));
        assert!(o.miniaturas && o.autoplay && o.ampliada);
        assert_eq!(o.posicion, Posicion::Izquierda);
        assert_eq!(o.leyenda, Leyenda::Debajo);
        assert_eq!(o.velocidad, 3.0);
        let d = &c.diapositivas;
        assert_eq!(d.len(), 5);
        assert_eq!(d[0].leyenda.as_deref(), Some("Corte sagital"));
        assert_eq!(d[0].embed(), "![[figura-1.png|Corte sagital]]");
        assert_eq!(d[1].clase, Clase::Video);
        assert_eq!(d[1].fragmento.as_deref(), Some("t=30"));
        assert_eq!(d[1].embed(), "![[clip.mp4#t=30]]");
        assert_eq!(d[2].leyenda, None, "|300 es un ancho");
        assert_eq!(d[3].escrito, "fotos/plano b.jpg");
        assert_eq!(d[3].leyenda.as_deref(), Some("Plano"));
        assert_eq!(d[4].clase, Clase::Pdf);
        assert_eq!(d[4].embed(), "![[doc.pdf#page=2|Informe]]");
    }

    #[test]
    fn opciones_en_espanol_sin_cabecera_y_carpeta() {
        let c = parse(
            "miniaturas: sí\nposicionMiniaturas: arriba\ncarpeta: imágenes/tesis\ntipos: [png, .JPG]\nrecursivo: true\nmaximo: 7\n![[extra.png]]\n",
        );
        assert!(c.avisos.is_empty(), "{:?}", c.avisos);
        assert!(c.opciones.miniaturas);
        assert_eq!(c.opciones.posicion, Posicion::Arriba);
        assert_eq!(c.opciones.maximo, 7);
        assert_eq!(c.carpetas.len(), 1);
        let f = &c.carpetas[0];
        assert_eq!(f.carpeta, "imágenes/tesis");
        assert_eq!(f.tipos, ["png", "jpg"]);
        assert!(f.recursivo);
        assert_eq!(f.lugar, 1, "después de lo escrito");
    }

    #[test]
    fn tipos_en_lista_de_renglones_y_carpeta_heredada() {
        let c = parse("fileTypes:\n  - png\n  - webp\n[[fotos/]]\n![[a.png]]\n");
        assert!(c.avisos.is_empty(), "{:?}", c.avisos);
        assert_eq!(c.carpetas[0].carpeta, "fotos");
        assert_eq!(c.carpetas[0].tipos, ["png", "webp"]);
        assert_eq!(c.carpetas[0].lugar, 0);
        assert_eq!(c.diapositivas.len(), 1);
    }

    #[test]
    fn lo_invalido_se_ignora_con_aviso() {
        let c = parse(
            "autoplay: quizás\nslideshowSpeed: -2\nthumbnailPosition: centro\ncolor: rojo\ntransitionEffect: flip\n![[a.png]]\nhttps://ejemplo.com/x.png\nesto no es nada\n",
        );
        assert_eq!(c.diapositivas.len(), 1);
        assert_eq!(c.avisos.len(), 6, "{:?}", c.avisos);
        assert!(!c.opciones.autoplay);
        assert_eq!(c.opciones.velocidad, VELOCIDAD);
        assert!(c.avisos.iter().any(|a| a.contains("«color»")));
        assert!(c.avisos.iter().any(|a| a.contains("externas")));
        // La transición es de una fase posterior: sin aviso.
        assert!(!c.avisos.iter().any(|a| a.contains("transitionEffect")));
        assert!(
            parse("")
                .avisos
                .iter()
                .any(|a| a.contains("no tiene medios"))
        );
    }

    #[test]
    fn enlaces_relativos() {
        assert_eq!(
            enlace_relativo("wiki/notas", "raw/a b.pdf"),
            "../../raw/a%20b.pdf"
        );
        assert_eq!(enlace_relativo("wiki", "wiki/x/#1.pdf"), "x/%231.pdf");
        assert_eq!(enlace_relativo("", "raw/a.pdf"), "raw/a.pdf");
    }

    #[test]
    fn html_estatico() {
        let mut c = parse("![[a.png|Uno <b>]]\n![[b.mp3]]\n![[falta.png]]\n");
        c.diapositivas[2].estado = Estado::Faltante;
        let src = |d: &Diapositiva| {
            (d.estado != Estado::Faltante).then(|| format!("file:///v/{}", d.escrito))
        };
        let h = html(&c, &src, true);
        assert!(h.contains("aria-label=\"Carrusel: 3 medios\""), "{h}");
        assert!(h.contains("alt=\"Uno &lt;b&gt;\""), "{h}");
        assert!(h.contains("<audio controls preload=\"none\""), "{h}");
        assert!(h.contains("Archivo inexistente: «falta.png»"), "{h}");
        let papel = html(&c, &src, false);
        assert!(
            papel.contains("Audio: <a href=\"file:///v/b.mp3\">b.mp3</a>"),
            "{papel}"
        );
    }
}
