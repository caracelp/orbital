//! Motor de fórmulas LaTeX de Orbital (050), sin GTK.
//!
//! Recibe el LaTeX de una fórmula (sin delimitadores) y devuelve una imagen RGBA lista para una
//! textura, o un SVG autónomo para HTML (notas `.html`, exportar). Lo usan la Lectura, la vista
//! previa de Código, la Visual de las notas `.html`, exportar y OnStage; las specs de Visual Math
//! (editor de ecuaciones, gráficos, química) se apoyan en el mismo módulo.
//!
//! Todo es puro y se puede llamar desde cualquier hilo. Las fuentes de KaTeX van embebidas y se
//! leen recién con la primera fórmula (carga perezosa: sin fórmulas no cuesta nada).

pub mod delimitadores;
pub mod html;

use ratex_layout::{LayoutOptions, layout, to_display_list, validate_layout};
use ratex_types::color::Color;
use ratex_types::display_item::DisplayList;
use ratex_types::math_style::MathStyle;

/// Cómo se dibuja la fórmula: en el renglón (`$…$`) o en bloque (`$$…$$`, ` ```math `).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Modo {
    EnLinea,
    Bloque,
}

/// Tamaño y color de una imagen.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Estilo {
    /// Tamaño del em en píxeles lógicos (el de la letra del texto que la rodea).
    pub tamano: f32,
    /// Píxeles físicos por píxel lógico (factor de escala del monitor).
    pub escala: f32,
    /// Color del trazo, RGBA sin premultiplicar.
    pub color: [u8; 4],
}

/// Una fórmula dibujada: RGBA sin premultiplicar, en píxeles físicos.
#[derive(Clone, Debug)]
pub struct Imagen {
    pub ancho: u32,
    pub alto: u32,
    /// Distancia en píxeles físicos desde el borde de arriba hasta la línea base.
    pub base: f32,
    /// La `escala` con que se dibujó: el tamaño lógico es `ancho / escala`.
    pub escala: f32,
    pub rgba: Vec<u8>,
}

impl Imagen {
    /// Lo que la fórmula baja de la línea base, en píxeles lógicos.
    pub fn profundidad(&self) -> f32 {
        (self.alto as f32 - self.base) / self.escala
    }
}

/// Una fórmula como SVG autónomo (glifos como trazos, sin fuentes externas). Mide en `em` del
/// texto que la rodea y pinta con `currentColor`, salvo los colores que pida el propio LaTeX.
#[derive(Clone, Debug)]
pub struct Svg {
    pub marcado: String,
    pub ancho_em: f64,
    pub alto_em: f64,
    /// Lo que baja de la línea base, en `em`.
    pub profundidad_em: f64,
}

/// Una fórmula que no se puede dibujar: el mensaje dice por qué.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Error {
    pub mensaje: String,
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.mensaje)
    }
}

/// Margen alrededor de la imagen, en píxeles lógicos: el suavizado no se corta en los bordes.
const MARGEN: f32 = 1.0;
/// Fórmulas más largas que esto no se dibujan (una nota pegada por error dentro de `$$`).
const MAX_BYTES: usize = 16 * 1024;

/// Analiza y dispone la fórmula.
fn disponer(tex: &str, modo: Modo, color: Color) -> Result<DisplayList, Error> {
    if tex.len() > MAX_BYTES {
        return Err(Error {
            mensaje: format!("La fórmula es demasiado larga (más de {MAX_BYTES} bytes)"),
        });
    }
    let ast = ratex_parser::parser::parse(tex).map_err(|e| Error {
        mensaje: traducir(&e.to_string()),
    })?;
    let estilo = match modo {
        Modo::EnLinea => MathStyle::Text,
        Modo::Bloque => MathStyle::Display,
    };
    let opts = LayoutOptions::default()
        .with_style(estilo)
        .with_color(color);
    let caja = layout(&ast, &opts);
    validate_layout(&caja).map_err(|mensaje| Error {
        mensaje: traducir(&mensaje),
    })?;
    Ok(to_display_list(&caja))
}

/// Solo comprueba que la fórmula se pueda dibujar (para el aviso de error sin dibujar nada).
pub fn validar(tex: &str) -> Result<(), Error> {
    disponer(tex, Modo::EnLinea, Color::BLACK).map(|_| ())
}

/// El tamaño de una imagen, sin dibujarla.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Medida {
    pub ancho: u32,
    pub alto: u32,
    /// Distancia en píxeles físicos desde el borde de arriba hasta la línea base.
    pub base: f32,
    pub escala: f32,
}

impl Medida {
    /// Lo que la fórmula baja de la línea base, en píxeles lógicos.
    pub fn profundidad(&self) -> f32 {
        (self.alto as f32 - self.base) / self.escala
    }
}

fn color_de(estilo: &Estilo) -> Color {
    let [r, g, b, a] = estilo.color;
    Color {
        r: f32::from(r) / 255.0,
        g: f32::from(g) / 255.0,
        b: f32::from(b) / 255.0,
        a: f32::from(a) / 255.0,
    }
}

/// Tamaño y escala dentro de lo razonable.
fn acotar(estilo: &Estilo) -> (f32, f32) {
    (
        estilo.tamano.clamp(4.0, 400.0),
        estilo.escala.clamp(0.5, 4.0),
    )
}

/// Las mismas cuentas que hace el dibujo para el tamaño de la imagen.
fn medida_de(lista: &DisplayList, tamano: f32, escala: f32) -> Medida {
    let em = tamano * escala;
    let margen = MARGEN * escala;
    let ancho = (lista.width as f32 * em + 2.0 * margen).ceil().max(1.0) as u32;
    let alto = ((lista.height + lista.depth) as f32 * em + 2.0 * margen)
        .ceil()
        .max(1.0) as u32;
    Medida {
        ancho,
        alto,
        base: (MARGEN + lista.height as f32 * tamano) * escala,
        escala,
    }
}

/// El tamaño que tendrá la imagen de [`dibujar`], sin dibujarla (análisis y disposición: unas
/// decenas de microsegundos).
pub fn medir(tex: &str, modo: Modo, estilo: &Estilo) -> Result<Medida, Error> {
    let lista = disponer(tex, modo, color_de(estilo))?;
    let (tamano, escala) = acotar(estilo);
    Ok(medida_de(&lista, tamano, escala))
}

/// Dibuja la fórmula en una imagen RGBA.
pub fn dibujar(tex: &str, modo: Modo, estilo: &Estilo) -> Result<Imagen, Error> {
    let lista = disponer(tex, modo, color_de(estilo))?;
    let (tamano, escala) = acotar(estilo);
    let medida = medida_de(&lista, tamano, escala);
    let opts = ratex_render::RenderOptions {
        font_size: tamano,
        padding: MARGEN,
        background_color: Color {
            r: 0.0,
            g: 0.0,
            b: 0.0,
            a: 0.0,
        },
        font_dir: String::new(),
        device_pixel_ratio: escala,
    };
    let png = ratex_render::render_to_png(&lista, &opts).map_err(|mensaje| Error {
        mensaje: traducir(&mensaje),
    })?;
    let (ancho, alto, rgba) = decodificar(&png).ok_or_else(|| Error {
        mensaje: "No se pudo dibujar la fórmula".into(),
    })?;
    debug_assert_eq!((ancho, alto), (medida.ancho, medida.alto));
    Ok(Imagen {
        ancho,
        alto,
        base: medida.base,
        escala,
        rgba,
    })
}

/// PNG RGBA de 8 bits → píxeles.
fn decodificar(png: &[u8]) -> Option<(u32, u32, Vec<u8>)> {
    let mut dec = png::Decoder::new(png);
    dec.set_transformations(png::Transformations::EXPAND);
    let mut lector = dec.read_info().ok()?;
    let mut buf = vec![0; lector.output_buffer_size()];
    let info = lector.next_frame(&mut buf).ok()?;
    buf.truncate(info.buffer_size());
    let rgba = match info.color_type {
        png::ColorType::Rgba => buf,
        png::ColorType::Rgb => buf
            .as_chunks::<3>()
            .0
            .iter()
            .flat_map(|&[r, g, b]| [r, g, b, 255])
            .collect(),
        _ => return None,
    };
    Some((info.width, info.height, rgba))
}

/// Color centinela que se cambia por `currentColor` en el SVG.
const CENTINELA: Color = Color {
    r: 1.0 / 255.0,
    g: 2.0 / 255.0,
    b: 3.0 / 255.0,
    a: 1.0,
};

/// La fórmula como SVG para meter en HTML (sin `xmlns`, que en HTML sobra), con un nombre
/// accesible (`role="img"`, `aria-label` y `<title>` con la lectura). Sin márgenes: mide lo que
/// mide la fórmula.
pub fn svg(tex: &str, modo: Modo) -> Result<Svg, Error> {
    let lista = disponer(tex, modo, CENTINELA)?;
    // 100 unidades por em: coordenadas enteras casi siempre y SVG corto.
    let opts = ratex_svg::SvgOptions {
        font_size: 100.0,
        padding: 0.0,
        stroke_width: 4.0,
        embed_glyphs: true,
        font_dir: String::new(),
    };
    let crudo =
        ratex_svg::render_to_svg_with_color_syntax(&lista, &opts, ratex_svg::SvgColorSyntax::Rgb);
    let cuerpo = crudo
        .find('>')
        .and_then(|i| crudo[i + 1..].strip_suffix("</svg>"))
        .unwrap_or("")
        .replace("rgb(1,2,3)", "currentColor");
    let ancho = lista.width.max(0.0);
    let alto = (lista.height + lista.depth).max(0.0);
    let prof = lista.depth.max(0.0);
    let etiqueta = html::escapar_atributo(&lectura(tex));
    let marcado = format!(
        "<svg viewBox=\"0 0 {:.0} {:.0}\" width=\"{:.3}em\" \
height=\"{:.3}em\" style=\"vertical-align:-{:.3}em;overflow:visible\" fill=\"currentColor\" \
role=\"img\" aria-label=\"{etiqueta}\"><title>{etiqueta}</title>{cuerpo}</svg>",
        (ancho * 100.0).ceil(),
        (alto * 100.0).ceil(),
        ancho,
        alto,
        prof,
    );
    Ok(Svg {
        marcado,
        ancho_em: ancho,
        alto_em: alto,
        profundidad_em: prof,
    })
}

/// Lo que lee el lector de pantalla: el LaTeX, con los espacios juntados.
pub fn lectura(tex: &str) -> String {
    let mut out = String::with_capacity(tex.len() + 9);
    out.push_str("Fórmula: ");
    let mut espacio = false;
    for c in tex.trim().chars() {
        if c.is_whitespace() {
            espacio = true;
        } else {
            if espacio {
                out.push(' ');
                espacio = false;
            }
            out.push(c);
        }
    }
    out
}

/// Los mensajes del motor vienen en inglés: los frecuentes se dicen en español; el resto queda
/// como está, detrás de una explicación.
fn traducir(mensaje: &str) -> String {
    let m = mensaje
        .strip_prefix("ParseError: ")
        .unwrap_or(mensaje)
        .trim();
    let (posicion, m) = match m.strip_prefix("ParseError at position ") {
        Some(resto) => match resto.split_once(": ") {
            Some((n, m)) => (n.parse::<usize>().ok(), m),
            None => (None, resto),
        },
        None => (None, m),
    };
    let base = if let Some(cmd) = m
        .strip_prefix("Undefined control sequence: ")
        .or_else(|| m.strip_prefix("Unknown function "))
    {
        format!("comando desconocido {}", cmd.trim_matches(['\'', '"']))
    } else if m.starts_with("Unexpected end of input") || m.starts_with("Expected '}'") {
        "falta cerrar una llave o un argumento".into()
    } else if m.contains("Missing") || m.contains("Expected") {
        format!("falta algo: {m}")
    } else {
        m.to_owned()
    };
    match posicion {
        Some(n) => format!("Error en la fórmula (carácter {}): {base}", n + 1),
        None => format!("Error en la fórmula: {base}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ESTILO: Estilo = Estilo {
        tamano: 16.0,
        escala: 1.0,
        color: [20, 20, 20, 255],
    };

    #[test]
    fn dibuja_entornos() {
        for tex in [
            r"x^2 + y^2 = z^2",
            r"\frac{a}{b} + \sqrt[3]{x}",
            r"\begin{aligned} a &= b \\ c &= d \end{aligned}",
            r"\begin{align} a &= b \\ c &= d \end{align}",
            r"f(x) = \begin{cases} 1 & x > 0 \\ 0 & x \le 0 \end{cases}",
            r"\begin{pmatrix} a & b \\ c & d \end{pmatrix}",
            r"\begin{bmatrix} 1 \\ 2 \end{bmatrix}",
            r"\begin{vmatrix} a & b \\ c & d \end{vmatrix}",
            r"\begin{matrix} a & b \end{matrix}",
            r"\begin{array}{c|c} 1 & 2 \\ \hline 3 & 4 \end{array}",
            r"\begin{gather} a \\ b \end{gather}",
            r"\sum_{n=1}^\infty \frac{1}{n^2}",
            r"\begin{Vmatrix} a \\ b \end{Vmatrix}",
            r"\begin{split} a &= b \\ &= c \end{split}",
            r"\begin{align*} a &= b \end{align*}",
        ] {
            let img = dibujar(tex, Modo::Bloque, &ESTILO).unwrap_or_else(|e| panic!("{tex}: {e}"));
            assert!(img.ancho > 4 && img.alto > 4, "{tex}");
            assert_eq!(img.rgba.len(), (img.ancho * img.alto * 4) as usize);
            assert!(
                img.rgba.iter().skip(3).step_by(4).any(|a| *a > 0),
                "{tex} vacía"
            );
        }
    }

    #[test]
    fn linea_base_y_profundidad() {
        let alta = dibujar(r"\frac{a}{b}", Modo::EnLinea, &ESTILO).unwrap();
        let plana = dibujar("x", Modo::EnLinea, &ESTILO).unwrap();
        assert!(alta.profundidad() > plana.profundidad());
        assert!(alta.base > 0.0 && alta.base < alta.alto as f32);
        let doble = dibujar(
            "x",
            Modo::EnLinea,
            &Estilo {
                escala: 2.0,
                ..ESTILO
            },
        )
        .unwrap();
        assert!(doble.ancho >= plana.ancho * 2 - 2);
    }

    #[test]
    fn errores_con_mensaje_y_sin_panico() {
        for tex in [
            r"\frac{a",
            r"\noexiste{x}",
            "}",
            r"\begin{pmatrix} a",
            r"\left(",
            "^^^",
            r"\sqrt[",
            "\\",
            "",
        ] {
            let r = dibujar(tex, Modo::EnLinea, &ESTILO);
            if let Err(e) = r {
                assert!(e.mensaje.starts_with("Error en la fórmula"), "{tex}: {e}");
            }
        }
        let e = validar(r"\frac{a").unwrap_err();
        assert!(e.mensaje.contains("llave"), "{e}");
        let e = validar(r"\noexiste").unwrap_err();
        assert!(e.mensaje.contains("noexiste"), "{e}");
        assert!(validar(&"x".repeat(MAX_BYTES + 1)).is_err());
    }

    #[test]
    fn svg_autonomo_y_accesible() {
        let s = svg(r"\frac{1}{2} < x", Modo::EnLinea).unwrap();
        assert!(s.marcado.starts_with("<svg"));
        assert!(s.marcado.contains("role=\"img\""));
        assert!(
            s.marcado
                .contains("aria-label=\"Fórmula: \\frac{1}{2} &lt; x\"")
        );
        assert!(s.marcado.contains("currentColor"));
        assert!(!s.marcado.contains("rgb(1,2,3)"));
        assert!(!s.marcado.contains("<text"), "los glifos van como trazos");
        assert!(s.profundidad_em > 0.0 && s.ancho_em > 0.0);
        let rojo = svg(r"\color{red}{x}", Modo::EnLinea).unwrap();
        assert!(rojo.marcado.contains("rgb(255,0,0)"));
    }

    #[test]
    fn lectura_junta_espacios() {
        assert_eq!(lectura("  a  +\n b "), "Fórmula: a + b");
    }
}
