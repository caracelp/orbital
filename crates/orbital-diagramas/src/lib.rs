//! Diagramas Mermaid de Orbital (058), sin GTK.
//!
//! Un bloque ` ```mermaid ` se convierte en SVG con [`svg`] (el motor es `mermaid-svg`, Rust puro)
//! con los colores del tema de Orbital ([`Tema`]), y en una imagen RGBA con [`imagen`] (`resvg`)
//! para la app. Lo usan la Lectura, el globito de Código, OnStage, la Visual de las notas `.html`
//! y exportar.
//!
//! Todo es puro y se puede llamar desde cualquier hilo. Las fuentes ([`Fuentes`]) se cargan con
//! el primer diagrama: solo las caras de la letra de la interfaz, buscadas con fontconfig.

mod fuentes;
pub mod html;
mod tema;

use std::fmt;

pub use fuentes::Fuentes;
pub use tema::Tema;

/// Lado más largo de una imagen, en píxeles físicos: un diagrama enorme no ocupa cientos de MB.
pub const MAX_LADO: u32 = 4096;
/// Píxeles de una imagen, como mucho (16 millones: 64 MB en RGBA).
pub const MAX_PIXELES: u64 = 16 * 1024 * 1024;

/// Un diagrama como SVG.
#[derive(Clone, Debug)]
pub struct Diagrama {
    /// El SVG entero (`<svg …>…</svg>`), con `role="img"` y `aria-label`.
    pub marcado: String,
    /// Tamaño natural, en píxeles lógicos.
    pub ancho: f32,
    pub alto: f32,
    /// La palabra del encabezado (`flowchart`, `sequenceDiagram`…).
    pub tipo: String,
}

impl Diagrama {
    /// El nombre accesible: «Diagrama Mermaid: de flujo».
    pub fn nombre(&self) -> String {
        format!("Diagrama Mermaid: {}", nombre_tipo(&self.tipo))
    }
}

/// Por qué no se pudo dibujar un diagrama.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Error {
    /// El motivo, en español.
    pub mensaje: String,
    /// Renglón del bloque (desde 1), si se sabe.
    pub renglon: Option<usize>,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.renglon {
            Some(r) => write!(f, "Error en el diagrama (renglón {r}): {}", self.mensaje),
            None => write!(f, "Error en el diagrama: {}", self.mensaje),
        }
    }
}

impl std::error::Error for Error {}

/// La palabra del encabezado: el primer renglón con algo que no sea un comentario (`%%`), una
/// directiva (`%%{…}%%`) ni el frontmatter (`---`).
pub fn tipo(fuente: &str) -> Option<&str> {
    let mut lineas = fuente.lines().map(str::trim).peekable();
    // Frontmatter.
    while lineas.peek().is_some_and(|l| l.is_empty()) {
        lineas.next();
    }
    if lineas.peek() == Some(&"---") {
        lineas.next();
        for l in lineas.by_ref() {
            if l == "---" {
                break;
            }
        }
    }
    lineas
        .find(|l| !l.is_empty() && !l.starts_with("%%"))
        .and_then(|l| l.split(|c: char| c.is_whitespace() || c == ';').next())
}

/// El tipo en palabras: «de flujo», «de secuencia»…
pub fn nombre_tipo(tipo: &str) -> &str {
    match tipo {
        "flowchart" | "graph" | "flowchart-elk" => "de flujo",
        "sequenceDiagram" => "de secuencia",
        "classDiagram" | "classDiagram-v2" => "de clases",
        "stateDiagram" | "stateDiagram-v2" => "de estados",
        "erDiagram" => "entidad-relación",
        "gantt" => "de Gantt",
        "pie" => "de torta",
        "mindmap" => "mapa mental",
        "timeline" => "línea de tiempo",
        "journey" => "recorrido",
        "gitGraph" => "de ramas",
        "quadrantChart" => "de cuadrantes",
        "xychart" | "xychart-beta" => "de ejes",
        "sankey" | "sankey-beta" => "de flujos (Sankey)",
        "requirementDiagram" => "de requisitos",
        "kanban" => "kanban",
        "block" | "block-beta" => "de bloques",
        "packet" | "packet-beta" => "de paquete",
        "architecture" | "architecture-beta" => "de arquitectura",
        "radar" | "radar-beta" => "radar",
        "treemap" | "treemap-beta" => "de rectángulos",
        t if t.starts_with("C4") => "C4",
        _ => "diagrama",
    }
}

/// El diagrama de `fuente` como SVG con los colores de `tema`.
pub fn svg(fuente: &str, tema: Tema) -> Result<Diagrama, Error> {
    let tipo = tipo(fuente).ok_or_else(|| Error {
        mensaje: "el diagrama está vacío".into(),
        renglon: None,
    })?;
    let theme = tema.theme();
    let crudo = mermaid_svg::render_with(fuente, &theme).map_err(traducir)?;
    let marcado = tema::ajustar(&crudo, tema);
    let (ancho, alto) = tamano(&marcado).ok_or_else(|| Error {
        mensaje: "el motor no dio el tamaño del diagrama".into(),
        renglon: None,
    })?;
    let tipo = tipo.to_owned();
    let nombre = format!("Diagrama Mermaid: {}", nombre_tipo(&tipo));
    let marcado = accesible(marcado, &nombre);
    Ok(Diagrama {
        marcado,
        ancho,
        alto,
        tipo,
    })
}

/// El error del motor, en español.
fn traducir(e: mermaid_svg::RenderError) -> Error {
    use mermaid_svg::{ParseError, RenderError, SyntaxKind};
    let RenderError::Parse(p) = e else {
        return Error {
            mensaje: e.to_string(),
            renglon: None,
        };
    };
    match p {
        ParseError::Syntax {
            kind,
            message,
            line,
        } => {
            let que = match kind {
                SyntaxKind::MissingHeader => "falta el tipo de diagrama en el primer renglón",
                SyntaxKind::UnknownStatement => "no se entiende el renglón",
                SyntaxKind::InvalidNumber => "un número no es válido",
                SyntaxKind::Unclosed => "falta cerrar un corchete, una llave o unas comillas",
                SyntaxKind::Malformed => "el renglón está incompleto o mal formado",
                _ => "no se entiende el renglón",
            };
            Error {
                mensaje: format!("{que} ({message})"),
                renglon: (line > 0).then_some(line),
            }
        }
        ParseError::UnknownDiagramType(t) => Error {
            mensaje: format!("tipo de diagrama desconocido: «{}»", t.trim()),
            renglon: None,
        },
        ParseError::Empty => Error {
            mensaje: "el diagrama está vacío".into(),
            renglon: None,
        },
        otro => Error {
            mensaje: otro.to_string(),
            renglon: None,
        },
    }
}

/// Ancho y alto del `viewBox` del `<svg>` raíz.
fn tamano(svg: &str) -> Option<(f32, f32)> {
    let raiz = &svg[..svg.find('>')?];
    let vb = raiz.split("viewBox=\"").nth(1)?.split('"').next()?;
    let mut n = vb.split([' ', ',']).filter(|s| !s.is_empty());
    let (_, _) = (n.next()?, n.next()?);
    let w: f32 = n.next()?.parse().ok()?;
    let h: f32 = n.next()?.parse().ok()?;
    (w > 0.0 && h > 0.0 && w.is_finite() && h.is_finite()).then_some((w, h))
}

/// Reemplaza el rol del `<svg>` raíz por una imagen con nombre (lo lee el lector de pantalla en
/// las notas `.html` y en lo exportado).
fn accesible(svg: String, nombre: &str) -> String {
    let Some(fin) = svg.find('>') else {
        return svg;
    };
    let raiz = &svg[..fin];
    let mut nueva = String::with_capacity(svg.len() + nombre.len() + 32);
    let sin_rol = quitar_atributo(&quitar_atributo(raiz, "role"), "aria-roledescription");
    nueva.push_str(&sin_rol);
    nueva.push_str(" role=\"img\" aria-label=\"");
    html::escapar_en(&mut nueva, nombre);
    nueva.push('"');
    nueva.push_str(&svg[fin..]);
    nueva
}

fn quitar_atributo(tag: &str, nombre: &str) -> String {
    let patron = format!(" {nombre}=\"");
    match tag.find(&patron) {
        Some(i) => {
            let resto = &tag[i + patron.len()..];
            match resto.find('"') {
                Some(f) => format!("{}{}", &tag[..i], &resto[f + 1..]),
                None => tag.to_owned(),
            }
        }
        None => tag.to_owned(),
    }
}

/// Un diagrama como imagen: RGBA premultiplicado, en píxeles físicos.
#[derive(Clone, Debug)]
pub struct Imagen {
    pub ancho: u32,
    pub alto: u32,
    /// Píxeles físicos por píxel lógico con que se dibujó (puede ser menor que la escala pedida
    /// si el diagrama pasaba de [`MAX_LADO`] o [`MAX_PIXELES`]).
    pub escala: f32,
    pub rgba: Vec<u8>,
}

impl Imagen {
    pub fn ancho_logico(&self) -> f32 {
        self.ancho as f32 / self.escala
    }
    pub fn alto_logico(&self) -> f32 {
        self.alto as f32 / self.escala
    }
}

/// Dibuja el SVG de un diagrama a `escala` píxeles físicos por lógico (con un tope de tamaño).
pub fn imagen(d: &Diagrama, escala: f32, fuentes: &Fuentes) -> Result<Imagen, Error> {
    use resvg::{tiny_skia, usvg};
    let tree =
        usvg::Tree::from_str(&d.marcado, &fuentes.opciones_para(&d.marcado)).map_err(|e| {
            Error {
                mensaje: format!("no se pudo dibujar el diagrama ({e})"),
                renglon: None,
            }
        })?;
    let size = tree.size();
    let (w, h) = (size.width().max(1.0), size.height().max(1.0));
    let mut k = escala.max(0.25);
    k = k.min(MAX_LADO as f32 / w).min(MAX_LADO as f32 / h);
    let pixeles = f64::from(w * k) * f64::from(h * k);
    if pixeles > MAX_PIXELES as f64 {
        k *= (MAX_PIXELES as f64 / pixeles).sqrt() as f32;
    }
    let (pw, ph) = ((w * k).ceil() as u32, (h * k).ceil() as u32);
    let mut pm = tiny_skia::Pixmap::new(pw.max(1), ph.max(1)).ok_or_else(|| Error {
        mensaje: "el diagrama es demasiado grande".into(),
        renglon: None,
    })?;
    let escala = tiny_skia::Transform::from_scale(k, k);
    resvg::render(&tree, escala, &mut pm.as_mut());
    // Los emoji de color (059) son imágenes PNG dentro de la letra, y `resvg` sin sus
    // decodificadores de imágenes (GIF, WebP y JPEG: ≈ 440 KB) no las dibuja. Acá se dibujan
    // solo esas, con el decodificador PNG que `tiny-skia` ya trae. Un texto ASCII no tiene.
    if !d.marcado.is_ascii() {
        imagenes_png(tree.root(), escala, &mut pm.as_mut());
    }
    Ok(Imagen {
        ancho: pm.width(),
        alto: pm.height(),
        escala: k,
        rgba: pm.take(),
    })
}

/// Dibuja las imágenes PNG del árbol (las de los emoji de color, que `resvg` deja sin dibujar).
fn imagenes_png(
    grupo: &resvg::usvg::Group,
    ts: resvg::tiny_skia::Transform,
    pm: &mut resvg::tiny_skia::PixmapMut,
) {
    use resvg::{tiny_skia, usvg};
    let ts = ts.pre_concat(grupo.transform());
    for nodo in grupo.children() {
        match nodo {
            usvg::Node::Group(g) => imagenes_png(g, ts, pm),
            usvg::Node::Text(t) => imagenes_png(t.flattened(), ts, pm),
            usvg::Node::Image(img) if img.is_visible() => {
                let usvg::ImageKind::PNG(datos) = img.kind() else {
                    continue;
                };
                let Ok(png) = tiny_skia::Pixmap::decode_png(datos) else {
                    continue;
                };
                let Some(rect) = tiny_skia::Size::from_wh(png.width() as f32, png.height() as f32)
                    .and_then(|s| s.to_rect(0.0, 0.0))
                else {
                    continue;
                };
                let paint = tiny_skia::Paint {
                    shader: tiny_skia::Pattern::new(
                        png.as_ref(),
                        tiny_skia::SpreadMode::Pad,
                        tiny_skia::FilterQuality::Bicubic,
                        1.0,
                        tiny_skia::Transform::default(),
                    ),
                    ..tiny_skia::Paint::default()
                };
                pm.fill_rect(rect, &paint, ts, None);
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests;
