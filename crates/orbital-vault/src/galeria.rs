//! Galería incrustada en la nota (033): un codeblock ` ```galeria ` con una carpeta de la bóveda
//! que la Lectura muestra como una grilla de sus imágenes y videos.
//!
//! El cuerpo son opciones `clave: valor` (opcionalmente entre dos renglones `---`), con el nombre
//! en español o el heredado (`carpeta` o `folder`, `recursivo` o `recursive`); ver
//! `docs/formato-boveda.md`. Sin GTK y sin leer los archivos: [`parse`] analiza el texto y
//! [`Vault::resolver_galeria`] arma la lista con el índice de la bóveda, en el hilo de fondo que
//! prepara la nota.

use std::path::PathBuf;
use std::time::SystemTime;

use crate::RelPath;
use crate::bloque_medios::{
    booleano, carpeta_de, escapar, ext_limpia, extension, lista_de_extensiones, opcion, recortar,
    sin_comillas,
};
use crate::carrusel::{Carrusel, Clase, Diapositiva, Estado};
use crate::vault::Vault;

/// Elementos por galería como máximo (la lista es barata; las miniaturas se preparan solo cerca
/// de lo visible).
pub const MAXIMO: usize = 10_000;
/// Alto de las filas de `justificadas` si no se dice otra cosa (píxeles lógicos).
pub const ALTO: u32 = 180;
/// Segundos por foto del pase automático del visor si no se dice otra cosa.
pub const VELOCIDAD: f32 = 4.0;

/// Si un lenguaje de codeblock es una galería.
pub fn es_lenguaje(lang: &str) -> bool {
    matches!(lang, "galeria" | "galería" | "gallery")
}

/// La forma de la grilla.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Layout {
    /// Celdas cuadradas, con la foto recortada.
    #[default]
    Miniaturas,
    /// Columnas con el alto natural de cada foto.
    Masonry,
    /// Filas de alto parejo que llenan el ancho.
    Justificadas,
    /// Una foto grande a la vez (el carrusel de la 032).
    Carrusel,
}

impl Layout {
    /// El nombre como se escribe en el bloque.
    pub fn nombre(self) -> &'static str {
        match self {
            Self::Miniaturas => "miniaturas",
            Self::Masonry => "masonry",
            Self::Justificadas => "justificadas",
            Self::Carrusel => "carrusel",
        }
    }

    pub const TODOS: [Layout; 4] = [
        Self::Miniaturas,
        Self::Masonry,
        Self::Justificadas,
        Self::Carrusel,
    ];

    fn de(v: &str) -> Result<Self, bool> {
        Ok(match v {
            "miniaturas" | "grilla" | "cuadricula" | "cuadrícula" | "grid" | "thumbnails" => {
                Self::Miniaturas
            }
            "masonry" | "columnas" => Self::Masonry,
            "justificadas" | "justificada" | "filas" | "justified" => Self::Justificadas,
            "carrusel" | "carousel" | "slider" => Self::Carrusel,
            // Disposiciones de una fase posterior: `Err(true)` (se avisa que todavía no están).
            "mosaico" | "mosaic" | "embeds" | "sinmarco" | "sin-marco" | "frameless" => {
                return Err(true);
            }
            _ => return Err(false),
        })
    }
}

/// Las opciones de la galería.
#[derive(Clone, Debug, PartialEq)]
pub struct Opciones {
    /// La carpeta como está escrita.
    pub carpeta: Option<String>,
    pub layout: Layout,
    pub recursivo: bool,
    /// Con `recursivo`, un título por subcarpeta.
    pub agrupar: bool,
    /// Extensiones en minúscula y sin punto; vacío: todas las imágenes y videos.
    pub tipos: Vec<String>,
    /// Elementos por página; `None`: la grilla entera.
    pub paginar: Option<usize>,
    /// Columnas fijas de `miniaturas` y `masonry`; `None`: según el ancho.
    pub columnas: Option<u32>,
    /// Alto de las filas de `justificadas`.
    pub alto: u32,
    /// Segundos por foto del pase automático del visor.
    pub velocidad: f32,
}

impl Default for Opciones {
    fn default() -> Self {
        Self {
            carpeta: None,
            layout: Layout::default(),
            recursivo: false,
            agrupar: false,
            tipos: Vec::new(),
            paginar: None,
            columnas: None,
            alto: ALTO,
            velocidad: VELOCIDAD,
        }
    }
}

/// Un archivo de la galería.
#[derive(Clone, Debug, PartialEq)]
pub struct Elemento {
    pub path: RelPath,
    pub abs: PathBuf,
    /// [`Clase::Imagen`] o [`Clase::Video`].
    pub clase: Clase,
    /// Tamaño y fecha del índice de la bóveda (para el panel de información).
    pub size: u64,
    pub mtime: Option<SystemTime>,
    /// Índice en [`Galeria::grupos`].
    pub grupo: usize,
}

impl Elemento {
    /// El nombre del archivo, sin carpetas.
    pub fn nombre(&self) -> &str {
        self.path.file_name()
    }
}

/// Una subcarpeta con su título (con `agrupar`), o la carpeta entera.
#[derive(Clone, Debug, PartialEq)]
pub struct Grupo {
    /// La subcarpeta relativa a la carpeta de la galería (`""`: la carpeta misma).
    pub nombre: String,
    /// El primer elemento del grupo.
    pub desde: usize,
}

/// Una galería analizada (y, después de [`Vault::resolver_galeria`], resuelta).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Galeria {
    pub opciones: Opciones,
    pub elementos: Vec<Elemento>,
    /// Vacío o un solo grupo: sin títulos.
    pub grupos: Vec<Grupo>,
    /// Lo que se ignoró, para un aviso discreto debajo de la galería.
    pub avisos: Vec<String>,
    /// Por qué no hay nada que mostrar («la carpeta no existe», «sin imágenes»).
    pub vacia: Option<String>,
    /// La carpeta en la bóveda, ya resuelta (`""`: la raíz).
    pub carpeta: Option<String>,
    /// Ya pasó por [`Vault::resolver_galeria`].
    pub resuelto: bool,
}

impl Galeria {
    /// Si hay títulos de grupo.
    pub fn con_titulos(&self) -> bool {
        self.grupos.len() > 1
    }

    /// Las páginas de la grilla (1 sin `paginar`).
    pub fn paginas(&self) -> usize {
        match self.opciones.paginar {
            Some(n) if n > 0 => self.elementos.len().div_ceil(n).max(1),
            _ => 1,
        }
    }

    /// Los elementos de la página `p` (desde 0).
    pub fn rango_de_pagina(&self, p: usize) -> std::ops::Range<usize> {
        let total = self.elementos.len();
        match self.opciones.paginar {
            Some(n) if n > 0 => {
                let a = (p * n).min(total);
                a..(a + n).min(total)
            }
            _ => 0..total,
        }
    }

    /// El nombre para el lector de pantalla: «Galería: carpeta, 12 imágenes y 2 videos».
    pub fn descripcion(&self) -> String {
        let imagenes = self
            .elementos
            .iter()
            .filter(|e| e.clase == Clase::Imagen)
            .count();
        let videos = self.elementos.len() - imagenes;
        let carpeta = self
            .opciones
            .carpeta
            .as_deref()
            .unwrap_or("sin carpeta")
            .trim_start_matches("./");
        let mut s = format!("Galería: {carpeta}, ");
        let plural = |n: usize, uno: &str, varios: &str| {
            format!("{n} {}", if n == 1 { uno } else { varios })
        };
        match (imagenes, videos) {
            (_, 0) => s.push_str(&plural(imagenes, "imagen", "imágenes")),
            (0, _) => s.push_str(&plural(videos, "video", "videos")),
            _ => {
                s.push_str(&plural(imagenes, "imagen", "imágenes"));
                s.push_str(" y ");
                s.push_str(&plural(videos, "video", "videos"));
            }
        }
        s
    }

    /// La lista de la galería como un carrusel (032): el visor a pantalla completa y la
    /// disposición `carrusel` son el mismo componente. `miniaturas`: con su tira.
    pub fn a_carrusel(&self, miniaturas: bool) -> Carrusel {
        let mut c = Carrusel {
            diapositivas: self
                .elementos
                .iter()
                .map(|e| Diapositiva {
                    escrito: e.path.as_str().to_owned(),
                    fragmento: None,
                    leyenda: None,
                    clase: e.clase,
                    estado: Estado::Encontrado {
                        path: e.path.clone(),
                        abs: e.abs.clone(),
                    },
                    de_carpeta: true,
                    enlace: None,
                })
                .collect(),
            resuelto: true,
            ..Carrusel::default()
        };
        let o = &mut c.opciones;
        o.miniaturas = miniaturas;
        o.ampliada = true;
        o.autoplay = true;
        o.velocidad = self.opciones.velocidad;
        o.maximo = MAXIMO;
        c
    }
}

/// Analiza el cuerpo de un codeblock de galería. Nunca falla: lo que no entiende va a
/// [`Galeria::avisos`].
pub fn parse(cuerpo: &str) -> Galeria {
    let mut g = Galeria::default();
    // Clave con valor en los renglones siguientes (`fileTypes:` y debajo `- png`).
    let mut lista_abierta = false;
    let mut tipos: Option<Vec<String>> = None;
    for r in cuerpo.lines() {
        let t = r.trim();
        if t.is_empty() || t == "---" || t.starts_with('#') {
            lista_abierta &= t.is_empty();
            continue;
        }
        if lista_abierta && let Some(item) = t.strip_prefix('-') {
            let e = ext_limpia(item.trim().trim_matches(['"', '\'']));
            if !e.is_empty() {
                tipos.get_or_insert_with(Vec::new).push(e);
            }
            continue;
        }
        lista_abierta = false;
        if let Some((clave, valor)) = opcion(t) {
            let clave_min = clave.to_lowercase();
            if valor.is_empty() && matches!(clave_min.as_str(), "filetypes" | "tipos" | "types") {
                lista_abierta = true;
                continue;
            }
            aplicar(&mut g, &clave_min, clave, valor, &mut tipos);
            continue;
        }
        // `[[carpeta/]]` o una ruta suelta terminada en `/`: la carpeta.
        let carpeta = t
            .strip_prefix("[[")
            .and_then(|x| x.strip_suffix("]]"))
            .unwrap_or(t);
        if carpeta.ends_with('/') && g.opciones.carpeta.is_none() && !carpeta.contains("://") {
            g.opciones.carpeta = Some(carpeta.trim_end_matches('/').to_owned());
            continue;
        }
        let aviso = if t.contains("://") {
            format!("Las direcciones externas no se cargan: «{}»", recortar(t))
        } else {
            format!("Renglón ignorado: «{}»", recortar(t))
        };
        g.avisos.push(aviso);
    }
    if let Some(t) = tipos {
        let (medios, otros): (Vec<String>, Vec<String>) =
            t.into_iter().partition(|e| clase_de_ext(e).is_some());
        if !otros.is_empty() {
            g.avisos.push(format!(
                "La galería muestra imágenes y videos: se ignoran los tipos {}",
                otros.join(", ")
            ));
        }
        g.opciones.tipos = medios;
    }
    if g.opciones.carpeta.is_none() {
        g.vacia = Some("La galería no dice qué carpeta mostrar («carpeta: …»)".into());
    }
    g
}

fn aplicar(
    g: &mut Galeria,
    clave: &str,
    original: &str,
    valor: &str,
    tipos: &mut Option<Vec<String>>,
) {
    let o = &mut g.opciones;
    let mal = |avisos: &mut Vec<String>| {
        avisos.push(format!(
            "Valor no válido para «{original}»: «{}»",
            recortar(valor)
        ))
    };
    match clave {
        "carpeta" | "folder" | "path" | "ruta" => {
            let v = sin_comillas(valor)
                .trim_start_matches("[[")
                .trim_end_matches("]]")
                .trim()
                .trim_end_matches('/');
            if v.is_empty() && sin_comillas(valor).is_empty() {
                mal(&mut g.avisos);
            } else {
                o.carpeta = Some(if v.is_empty() {
                    "/".into()
                } else {
                    v.to_owned()
                });
            }
        }
        "layout" | "disposicion" | "disposición" => {
            match Layout::de(&sin_comillas(valor).to_lowercase()) {
                Ok(l) => o.layout = l,
                Err(true) => g.avisos.push(format!(
                    "La disposición «{}» todavía no está en Orbital: se usa «miniaturas»",
                    sin_comillas(valor)
                )),
                Err(false) => mal(&mut g.avisos),
            }
        }
        "recursive" | "recursivo" | "subcarpetas" => match booleano(valor) {
            Some(b) => o.recursivo = b,
            None => mal(&mut g.avisos),
        },
        "agrupar" | "group" | "groupbyfolder" => match booleano(valor) {
            Some(b) => o.agrupar = b,
            None => mal(&mut g.avisos),
        },
        "filetypes" | "tipos" | "types" => {
            let lista = lista_de_extensiones(valor);
            if lista.is_empty() {
                mal(&mut g.avisos);
            } else {
                *tipos = Some(lista);
            }
        }
        "paginar" | "pagination" | "paginate" | "perpage" | "porpagina" | "porpágina" => {
            if booleano(valor) == Some(false) {
                o.paginar = None;
            } else {
                match sin_comillas(valor).parse::<usize>() {
                    Ok(n) if n > 0 => o.paginar = Some(n.min(MAXIMO)),
                    _ => mal(&mut g.avisos),
                }
            }
        }
        "columnas" | "columns" => match sin_comillas(valor).parse::<u32>() {
            Ok(n) if (1..=12).contains(&n) => o.columnas = Some(n),
            _ => mal(&mut g.avisos),
        },
        "alto" | "altura" | "rowheight" | "height" => {
            match sin_comillas(valor).trim_end_matches("px").parse::<u32>() {
                Ok(n) if n > 0 => o.alto = n.clamp(60, 480),
                _ => mal(&mut g.avisos),
            }
        }
        "velocidad" | "slideshowspeed" | "speed" => {
            match sin_comillas(valor)
                .trim_end_matches('s')
                .trim()
                .replace(',', ".")
                .parse::<f32>()
            {
                Ok(v) if v.is_finite() && v > 0.0 => o.velocidad = v.clamp(1.0, 3600.0),
                _ => mal(&mut g.avisos),
            }
        }
        _ => g
            .avisos
            .push(format!("Opción desconocida ignorada: «{original}»")),
    }
}

/// La clase de una extensión que la galería muestra: imagen o video.
fn clase_de_ext(ext: &str) -> Option<Clase> {
    if crate::medios::IMAGEN.contains(&ext) {
        Some(Clase::Imagen)
    } else if crate::medios::VIDEO.contains(&ext) {
        Some(Clase::Video)
    } else {
        None
    }
}

impl Vault {
    /// Arma la lista de una galería de la nota `from` con el índice de la bóveda (sin leer el
    /// disco salvo para dejar afuera los enlaces simbólicos): la carpeta relativa a la nota
    /// (`./fotos`), a la raíz o a `wiki/`; sus imágenes y videos en orden alfabético estable; con
    /// `recursivo`, también las subcarpetas (con `agrupar`, la carpeta primero y después cada
    /// subcarpeta). Idempotente.
    pub fn resolver_galeria(&self, from: &RelPath, g: &mut Galeria) {
        if g.resuelto {
            return;
        }
        g.resuelto = true;
        let Some(escrita) = g.opciones.carpeta.clone() else {
            return;
        };
        let escrita = if escrita == "/" { "" } else { escrita.as_str() };
        let Some(base) = self.carpeta_de_bloque(carpeta_de(from), escrita) else {
            g.vacia = Some(format!("La carpeta «{escrita}» no existe"));
            return;
        };
        let o = &g.opciones;
        let hallados = self.archivos_de_carpeta(&base, o.recursivo, |e| {
            clase_de_ext(e).is_some() && (o.tipos.is_empty() || o.tipos.iter().any(|t| t == e))
        });
        let mut elementos: Vec<(String, Elemento)> = Vec::with_capacity(hallados.len());
        let largo_base = if base.is_empty() { 0 } else { base.len() + 1 };
        for e in hallados {
            let abs = e.path.to_path(self.root());
            // Solo archivos de verdad de la bóveda: un enlace simbólico puede apuntar afuera.
            if std::fs::symlink_metadata(&abs).is_ok_and(|m| m.file_type().is_symlink()) {
                continue;
            }
            let Some(clase) = extension(e.path.as_str()).and_then(|x| clase_de_ext(&x)) else {
                continue;
            };
            let sub = carpeta_de(&e.path)
                .get(largo_base..)
                .unwrap_or("")
                .to_owned();
            elementos.push((
                sub,
                Elemento {
                    path: e.path.clone(),
                    abs,
                    clase,
                    size: e.size,
                    mtime: e.mtime,
                    grupo: 0,
                },
            ));
        }
        if o.recursivo && o.agrupar {
            // La carpeta misma primero; después cada subcarpeta (el orden de adentro se mantiene).
            elementos.sort_by_cached_key(|(sub, _)| (!sub.is_empty(), sub.to_lowercase()));
        }
        if elementos.len() > MAXIMO {
            g.avisos.push(format!(
                "La carpeta tiene {} imágenes y videos: se muestran los primeros {MAXIMO}",
                elementos.len()
            ));
            elementos.truncate(MAXIMO);
        }
        let agrupar = o.recursivo && o.agrupar;
        let mut grupos: Vec<Grupo> = Vec::new();
        for (k, (sub, e)) in elementos.iter_mut().enumerate() {
            if grupos.last().is_none_or(|gr| agrupar && gr.nombre != *sub) {
                grupos.push(Grupo {
                    nombre: if agrupar {
                        std::mem::take(sub)
                    } else {
                        String::new()
                    },
                    desde: k,
                });
            }
            e.grupo = grupos.len() - 1;
        }
        g.elementos = elementos.into_iter().map(|(_, e)| e).collect();
        g.grupos = grupos;
        if g.elementos.is_empty() {
            g.vacia = Some(format!(
                "La carpeta «{}» no tiene imágenes ni videos",
                if escrita.is_empty() { "/" } else { escrita }
            ));
        }
        g.carpeta = Some(base);
    }
}

/// La galería en HTML estático (exportar, la Visual de las notas `.html` y las notas
/// incrustadas en ellas): la misma disposición con CSS, sin JavaScript; las imágenes con carga
/// diferida. `src` da la dirección de cada archivo; `reproducibles`: los videos con controles
/// (si no, el nombre y un enlace, para papel). Sin paginar: todas las fotos.
pub fn html(g: &Galeria, src: &dyn Fn(&Elemento) -> Option<String>, reproducibles: bool) -> String {
    let o = &g.opciones;
    let mut out = format!(
        "<div class=\"orbital-galeria orbital-galeria-{}\" role=\"group\" aria-label=\"{}\" style=\"margin:1em 0\">",
        o.layout.nombre(),
        escapar(&g.descripcion())
    );
    if let Some(v) = &g.vacia {
        out.push_str(&format!(
            "<p style=\"opacity:0.7;margin:0\">{}</p>",
            escapar(v)
        ));
    }
    let contenedor = match o.layout {
        Layout::Miniaturas | Layout::Carrusel => {
            let col = o
                .columnas
                .map_or("repeat(auto-fill,minmax(140px,1fr))".to_owned(), |n| {
                    format!("repeat({n},1fr)")
                });
            format!("display:grid;grid-template-columns:{col};gap:6px")
        }
        Layout::Masonry => {
            let col = o.columnas.map_or("column-width:200px".to_owned(), |n| {
                format!("column-count:{n}")
            });
            format!("{col};column-gap:6px")
        }
        Layout::Justificadas => "display:flex;flex-wrap:wrap;gap:6px".to_owned(),
    };
    let estilo_img = match o.layout {
        Layout::Miniaturas | Layout::Carrusel => {
            "width:100%;aspect-ratio:1;object-fit:cover;display:block;border-radius:4px".to_owned()
        }
        Layout::Masonry => {
            "width:100%;height:auto;display:block;margin:0 0 6px;break-inside:avoid;border-radius:4px"
                .to_owned()
        }
        Layout::Justificadas => format!(
            "height:{}px;flex:1 1 auto;max-width:100%;object-fit:cover;display:block;border-radius:4px",
            o.alto
        ),
    };
    let mut abierto = false;
    for (k, e) in g.elementos.iter().enumerate() {
        if g.con_titulos() && g.grupos.get(e.grupo).is_some_and(|gr| gr.desde == k) {
            if abierto {
                out.push_str("</div>");
            }
            let nombre = &g.grupos[e.grupo].nombre;
            let titulo = if nombre.is_empty() {
                o.carpeta.as_deref().unwrap_or("").trim_start_matches("./")
            } else {
                nombre
            };
            out.push_str(&format!(
                "<h4 style=\"margin:0.8em 0 0.4em\">{}</h4>",
                escapar(titulo)
            ));
            abierto = false;
        }
        if !abierto {
            out.push_str(&format!("<div style=\"{contenedor}\">"));
            abierto = true;
        }
        let nombre = escapar(e.nombre());
        match (src(e), e.clase) {
            (None, _) => out.push_str(&format!(
                "<p style=\"opacity:0.7;margin:0\">Archivo inexistente: «{nombre}»</p>"
            )),
            (Some(s), Clase::Video) if reproducibles => out.push_str(&format!(
                "<video controls preload=\"none\" src=\"{}\" aria-label=\"Video: {nombre}\" style=\"{estilo_img};background:#000\"></video>",
                escapar(&s)
            )),
            (Some(s), Clase::Video) => out.push_str(&format!(
                "<p style=\"margin:0\">Video: <a href=\"{}\">{nombre}</a></p>",
                escapar(&s)
            )),
            (Some(s), _) => out.push_str(&format!(
                "<img src=\"{}\" alt=\"{nombre}\" loading=\"lazy\" style=\"{estilo_img}\">",
                escapar(&s)
            )),
        }
    }
    if abierto {
        out.push_str("</div>");
    }
    out.push_str("</div>");
    for a in &g.avisos {
        out.push_str(&format!(
            "<p class=\"orbital-galeria-aviso\" style=\"font-size:0.85em;opacity:0.7\">{}</p>",
            escapar(a)
        ));
    }
    out
}

/// El bloque que arma «Insertar galería…»: la carpeta (tal cual se eligió) y la disposición.
pub fn bloque(carpeta: &str, layout: Layout, recursivo: bool) -> String {
    let mut s = String::from("```galeria\n");
    let carpeta = if carpeta.is_empty() { "/" } else { carpeta };
    // Comillas si el nombre tiene algo que se confunda con YAML.
    if carpeta.contains([':', '#', '"']) || carpeta.starts_with(['\'', '[', '-']) {
        s.push_str(&format!("carpeta: \"{}\"\n", carpeta.replace('"', "")));
    } else {
        s.push_str(&format!("carpeta: {carpeta}\n"));
    }
    s.push_str(&format!("layout: {}\n", layout.nombre()));
    if recursivo {
        s.push_str("recursivo: true\n");
    }
    s.push_str("```");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opciones_en_espanol_y_heredadas() {
        let g = parse(
            "---\ncarpeta: imagenes/tesis\nlayout: masonry\nrecursivo: sí\nagrupar: true\ntipos: [png, .JPG]\npaginar: 100\ncolumnas: 3\nalto: 200px\nvelocidad: 2,5\n---\n",
        );
        assert!(g.avisos.is_empty(), "{:?}", g.avisos);
        let o = &g.opciones;
        assert_eq!(o.carpeta.as_deref(), Some("imagenes/tesis"));
        assert_eq!(o.layout, Layout::Masonry);
        assert!(o.recursivo && o.agrupar);
        assert_eq!(o.tipos, ["png", "jpg"]);
        assert_eq!(o.paginar, Some(100));
        assert_eq!(o.columnas, Some(3));
        assert_eq!(o.alto, 200);
        assert_eq!(o.velocidad, 2.5);
        let h = parse(
            "folder: \"fotos\"\nrecursive: true\nfileTypes:\n  - webp\n  - mp4\nlayout: justified\n",
        );
        assert!(h.avisos.is_empty(), "{:?}", h.avisos);
        assert_eq!(h.opciones.carpeta.as_deref(), Some("fotos"));
        assert_eq!(h.opciones.tipos, ["webp", "mp4"]);
        assert_eq!(h.opciones.layout, Layout::Justificadas);
        assert!(h.vacia.is_none());
    }

    #[test]
    fn cada_layout_por_su_nombre() {
        for l in Layout::TODOS {
            let g = parse(&format!("carpeta: x\nlayout: {}\n", l.nombre()));
            assert_eq!(g.opciones.layout, l);
        }
        assert_eq!(
            parse("carpeta: x\nlayout: Carousel").opciones.layout,
            Layout::Carrusel
        );
        assert_eq!(
            parse("[[fotos/]]\n").opciones.carpeta.as_deref(),
            Some("fotos")
        );
        assert_eq!(
            parse("carpeta: ./fotos/\n").opciones.carpeta.as_deref(),
            Some("./fotos")
        );
    }

    #[test]
    fn lo_invalido_y_lo_postergado_avisan() {
        let g = parse(
            "carpeta: x\nlayout: mosaico\ncolumnas: 40\npaginar: muchas\ncolor: rojo\ntipos: [png, pdf]\nhttps://ejemplo.com/a.png\n",
        );
        assert_eq!(g.opciones.layout, Layout::Miniaturas);
        assert_eq!(g.avisos.len(), 6, "{:?}", g.avisos);
        assert!(
            g.avisos[0].contains("todavía no está en Orbital"),
            "{:?}",
            g.avisos
        );
        assert!(g.avisos.iter().any(|a| a.contains("«color»")));
        assert!(g.avisos.iter().any(|a| a.contains("externas")));
        assert!(
            g.avisos
                .iter()
                .any(|a| a.contains("se ignoran los tipos pdf"))
        );
        assert_eq!(g.opciones.tipos, ["png"]);
        assert!(
            parse("layout: masonry")
                .vacia
                .unwrap()
                .contains("qué carpeta")
        );
    }

    #[test]
    fn paginas_y_rangos() {
        let mut g = parse("carpeta: x\npaginar: 3\n");
        g.elementos = (0..7)
            .map(|k| Elemento {
                path: RelPath::new(&format!("x/{k}.png")).unwrap(),
                abs: PathBuf::from(format!("/v/x/{k}.png")),
                clase: Clase::Imagen,
                size: 0,
                mtime: None,
                grupo: 0,
            })
            .collect();
        assert_eq!(g.paginas(), 3);
        assert_eq!(g.rango_de_pagina(0), 0..3);
        assert_eq!(g.rango_de_pagina(2), 6..7);
        g.opciones.paginar = None;
        assert_eq!(g.paginas(), 1);
        assert_eq!(g.rango_de_pagina(0), 0..7);
        assert_eq!(g.descripcion(), "Galería: x, 7 imágenes");
        let c = g.a_carrusel(false);
        assert_eq!(c.diapositivas.len(), 7);
        assert_eq!(c.diapositivas[3].embed(), "![[x/3.png]]");
        assert!(c.opciones.ampliada && c.opciones.autoplay && !c.opciones.miniaturas);
        let h = html(
            &g,
            &|e| Some(format!("file:///v/{}", e.path.as_str())),
            true,
        );
        assert_eq!(h.matches("<img ").count(), 7, "sin paginar en lo estático");
        assert!(h.contains("aspect-ratio:1"), "{h}");
    }

    #[test]
    fn el_bloque_insertado_se_vuelve_a_leer_igual() {
        let b = bloque("fotos ñ/viaje: 2026", Layout::Justificadas, true);
        assert_eq!(
            b,
            "```galeria\ncarpeta: \"fotos ñ/viaje: 2026\"\nlayout: justificadas\nrecursivo: true\n```"
        );
        let cuerpo = b.lines().skip(1).take(3).collect::<Vec<_>>().join("\n");
        let g = parse(&cuerpo);
        assert!(g.avisos.is_empty(), "{:?}", g.avisos);
        assert_eq!(g.opciones.carpeta.as_deref(), Some("fotos ñ/viaje: 2026"));
        assert_eq!(g.opciones.layout, Layout::Justificadas);
        assert!(g.opciones.recursivo);
        assert_eq!(parse("carpeta: /\n").opciones.carpeta.as_deref(), Some("/"));
    }
}
