//! Color rápido de texto y resaltado en Markdown sin HTML ni CSS (063).
//!
//! Sintaxis propia:
//! - `{rosa:texto}` o `{#ff8cc6:texto}`: color de texto.
//! - `{fondo-amarillo:texto}` (alias `{bg-amarillo:texto}`): resaltado/fondo.
//! - `{rosa/gris:texto}`: color de texto y fondo a la vez.
//! - `==texto==`: resaltado rápido (fondo rosa suave de Orbital).

use crate::iconos_catalogo::normalizar_color;

/// Un color base de la paleta de Orbital con variante de primer plano y de fondo (WCAG AA).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ColorBase {
    pub clave: &'static str,
    pub nombre: &'static str,
    /// Color de texto sobre fondo oscuro (`#1e1e24`).
    pub fg: &'static str,
    /// Color de fondo suave para resaltado (mantiene el texto claro legible).
    pub bg: &'static str,
}

/// Fondo del resaltado rápido `==texto==`.
pub const RESALTADO_RAPIDO_BG: &str = "#522540";

/// Paleta base de 10 colores con buen contraste sobre la base oscura de Orbital.
pub const PALETA_BASE: &[ColorBase] = &[
    ColorBase {
        clave: "rosa",
        nombre: "Rosa",
        fg: "#ff8cc6",
        bg: "#522540",
    },
    ColorBase {
        clave: "rojo",
        nombre: "Rojo",
        fg: "#ff7a93",
        bg: "#552430",
    },
    ColorBase {
        clave: "naranja",
        nombre: "Naranja",
        fg: "#ff9e64",
        bg: "#53341f",
    },
    ColorBase {
        clave: "amarillo",
        nombre: "Amarillo",
        fg: "#e8c468",
        bg: "#4d421e",
    },
    ColorBase {
        clave: "verde",
        nombre: "Verde",
        fg: "#9ece6a",
        bg: "#284426",
    },
    ColorBase {
        clave: "turquesa",
        nombre: "Turquesa",
        fg: "#73daca",
        bg: "#1e4440",
    },
    ColorBase {
        clave: "celeste",
        nombre: "Celeste",
        fg: "#7dcfff",
        bg: "#1f3e52",
    },
    ColorBase {
        clave: "azul",
        nombre: "Azul",
        fg: "#82aaff",
        bg: "#233459",
    },
    ColorBase {
        clave: "violeta",
        nombre: "Violeta",
        fg: "#c099ff",
        bg: "#3c2859",
    },
    ColorBase {
        clave: "gris",
        nombre: "Gris",
        fg: "#a9b1d6",
        bg: "#36394a",
    },
];

/// Especificación ya parseada de un tramo `{spec:…}` o `==…==`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EstiloColor {
    pub fg: Option<String>,
    pub bg: Option<String>,
}

/// Busca un color en la paleta base por su clave (insensible a mayúsculas y alias simples).
pub fn buscar_base(nombre: &str) -> Option<&'static ColorBase> {
    let n = nombre.trim().to_ascii_lowercase();
    let clave = match n.as_str() {
        "pink" => "rosa",
        "red" => "rojo",
        "orange" => "naranja",
        "yellow" => "amarillo",
        "green" => "verde",
        "teal" | "cyan" => "turquesa",
        "sky" => "celeste",
        "blue" => "azul",
        "purple" | "violet" => "violeta",
        "gray" | "grey" => "gris",
        other => other,
    };
    PALETA_BASE.iter().find(|c| c.clave == clave)
}

/// Valida un nombre corto de color personalizado (`[a-z][a-z0-9_-]{1,23}`).
pub fn nombre_color_valido(s: &str) -> bool {
    let s = s.trim();
    (2..=24).contains(&s.len())
        && s.as_bytes()[0].is_ascii_alphabetic()
        && s.bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_')
}

/// Normaliza un token de color (`#rgb`/`#rrggbb` o nombre válido).
pub fn normalizar_token(raw: &str) -> Option<String> {
    let s = raw.trim();
    if s.starts_with('#') {
        return normalizar_color(s);
    }
    let lower = s.to_ascii_lowercase();
    nombre_color_valido(&lower).then_some(lower)
}

/// Parsea el especificador `spec` de `{spec:texto}`:
/// - `fondo-<c>` o `bg-<c>` -> solo fondo
/// - `<fg>/<bg>` -> texto y fondo
/// - `<fg>` -> solo texto
pub fn parse_spec(spec: &str) -> Option<EstiloColor> {
    let s = spec.trim();
    if s.is_empty() || s.chars().any(char::is_whitespace) {
        return None;
    }
    if let Some(rest) = s
        .strip_prefix("fondo-")
        .or_else(|| s.strip_prefix("bg-"))
        .or_else(|| s.strip_prefix("FONDO-"))
        .or_else(|| s.strip_prefix("BG-"))
    {
        let bg = normalizar_token(rest)?;
        return Some(EstiloColor {
            fg: None,
            bg: Some(bg),
        });
    }
    if let Some((a, b)) = s.split_once('/') {
        if b.contains('/') {
            return None;
        }
        let fg = normalizar_token(a)?;
        let bg = normalizar_token(b)?;
        return Some(EstiloColor {
            fg: Some(fg),
            bg: Some(bg),
        });
    }
    let fg = normalizar_token(s)?;
    Some(EstiloColor {
        fg: Some(fg),
        bg: None,
    })
}

/// Resuelve un token de color de primer plano a `#rrggbb` usando paleta base + colores propios.
pub fn resolver_fg(token: &str, propios: &[(String, String)]) -> Option<String> {
    if token.starts_with('#') {
        return normalizar_color(token);
    }
    if let Some(c) = buscar_base(token) {
        return Some(c.fg.to_owned());
    }
    let k = token.trim().to_ascii_lowercase();
    propios
        .iter()
        .find(|(n, _)| n.eq_ignore_ascii_case(&k))
        .and_then(|(_, hex)| normalizar_color(hex))
}

/// Resuelve un token de color de fondo a `#rrggbb` usando paleta base + colores propios.
/// Para colores hex arbitrarios o propios, atenúa el color hacia la base oscura (`#1e1e24`)
/// para que el texto claro siga teniendo buen contraste.
pub fn resolver_bg(token: &str, propios: &[(String, String)]) -> Option<String> {
    if token == "==" {
        return Some(RESALTADO_RAPIDO_BG.to_owned());
    }
    if let Some(c) = buscar_base(token) {
        return Some(c.bg.to_owned());
    }
    let hex = if token.starts_with('#') {
        normalizar_color(token)?
    } else {
        let k = token.trim().to_ascii_lowercase();
        propios
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(&k))
            .and_then(|(_, h)| normalizar_color(h))?
    };
    Some(suavizar_para_fondo(&hex))
}

/// Mezcla un color `#rrggbb` al 32 % sobre la base oscura `#1e1e24` para fondo legible.
fn suavizar_para_fondo(hex: &str) -> String {
    let parse_byte = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).unwrap_or(0x80) as f32;
    let (r, g, b) = (parse_byte(1), parse_byte(3), parse_byte(5));
    let mix = |c: f32, base: f32| (c * 0.34 + base * 0.66).round().clamp(0.0, 255.0) as u8;
    format!(
        "#{:02x}{:02x}{:02x}",
        mix(r, 0x1e as f32),
        mix(g, 0x1e as f32),
        mix(b, 0x24 as f32)
    )
}

/// Desenvuelve un tramo `{spec:contenido}` o `==contenido==` si cubre todo `texto`.
pub fn desenvolver(texto: &str) -> Option<(EstiloColor, &str)> {
    let t = texto.trim();
    if let Some(inner) = t.strip_prefix("==").and_then(|s| s.strip_suffix("=="))
        && !inner.is_empty()
        && !inner.contains("==")
    {
        return Some((
            EstiloColor {
                fg: None,
                bg: Some("==".to_owned()),
            },
            inner,
        ));
    }
    if let Some(rest) = t.strip_prefix('{').and_then(|s| s.strip_suffix('}'))
        && let Some((spec, inner)) = rest.split_once(':')
        && let Some(estilo) = parse_spec(spec)
    {
        return Some((estilo, inner));
    }
    None
}

/// Quita el envoltorio de color o resaltado de `texto` si lo tiene.
pub fn quitar_color(texto: &str) -> String {
    match desenvolver(texto) {
        Some((_, inner)) => inner.to_owned(),
        None => texto.to_owned(),
    }
}

/// Aplica un color de texto (`fg`) y/o de fondo (`bg`) sobre `texto`, combinando o reemplazando
/// un envoltorio existente sin anidar llaves.
pub fn aplicar_estilo(texto: &str, nuevo_fg: Option<&str>, nuevo_bg: Option<&str>) -> String {
    let (prev, inner) = match desenvolver(texto) {
        Some((e, i)) => (Some(e), i),
        None => (None, texto),
    };
    let fg = nuevo_fg
        .map(str::to_owned)
        .or_else(|| prev.as_ref().and_then(|p| p.fg.clone()));
    let bg = nuevo_bg.map(str::to_owned).or_else(|| {
        prev.as_ref()
            .and_then(|p| p.bg.clone())
            .filter(|b| b != "==")
    });
    match (fg, bg) {
        (Some(f), Some(b)) => format!("{{{f}/{b}:{inner}}}"),
        (Some(f), None) => format!("{{{f}:{inner}}}"),
        (None, Some(b)) if b == "==" => format!("=={inner}=="),
        (None, Some(b)) => format!("{{fondo-{b}:{inner}}}"),
        (None, None) => inner.to_owned(),
    }
}

/// Alterna el resaltado rápido `==texto==`: si ya está envuelto en `==…==`, lo quita; si no, lo envuelve.
pub fn alternar_resaltado(texto: &str) -> String {
    if let Some((e, inner)) = desenvolver(texto)
        && e.fg.is_none()
        && e.bg.as_deref() == Some("==")
    {
        return inner.to_owned();
    }
    format!("=={texto}==")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parsea_especificadores_y_rechaza_ruido() {
        assert_eq!(
            parse_spec("rosa"),
            Some(EstiloColor {
                fg: Some("rosa".into()),
                bg: None
            })
        );
        assert_eq!(
            parse_spec("fondo-amarillo"),
            Some(EstiloColor {
                fg: None,
                bg: Some("amarillo".into())
            })
        );
        assert_eq!(
            parse_spec("bg-#ff0"),
            Some(EstiloColor {
                fg: None,
                bg: Some("#ffff00".into())
            })
        );
        assert_eq!(
            parse_spec("rosa/gris"),
            Some(EstiloColor {
                fg: Some("rosa".into()),
                bg: Some("gris".into())
            })
        );
        assert_eq!(parse_spec("con espacio"), None);
        assert_eq!(parse_spec(""), None);
        assert_eq!(parse_spec("a/b/c"), None);
    }

    #[test]
    fn aplicar_combinar_y_quitar_sin_anidar() {
        assert_eq!(aplicar_estilo("hola", Some("rosa"), None), "{rosa:hola}");
        assert_eq!(
            aplicar_estilo("{rosa:hola}", Some("azul"), None),
            "{azul:hola}"
        );
        assert_eq!(
            aplicar_estilo("{rosa:hola}", None, Some("amarillo")),
            "{rosa/amarillo:hola}"
        );
        assert_eq!(quitar_color("{rosa/amarillo:hola}"), "hola");
        assert_eq!(alternar_resaltado("clave"), "==clave==");
        assert_eq!(alternar_resaltado("==clave=="), "clave");
    }

    #[test]
    fn resuelve_colores_base_y_propios() {
        let propios = vec![("menta".to_owned(), "#88ffcc".to_owned())];
        assert_eq!(resolver_fg("rosa", &propios).as_deref(), Some("#ff8cc6"));
        assert_eq!(resolver_fg("menta", &propios).as_deref(), Some("#88ffcc"));
        assert!(resolver_bg("menta", &propios).is_some());
        assert_eq!(resolver_fg("inexistente", &propios), None);
    }
}
