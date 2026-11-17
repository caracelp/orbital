//! Opciones y selección de encabezados de la tabla de contenidos automática de Orbital
//! (codeblocks `table-of-contents` o `toc`).
//!
//! El cuerpo del codeblock no es YAML: se lee línea por línea como `clave: valor`, con
//! comentarios `# …` al final (salvo en `title`, `include` y `exclude`, donde `#` es parte del
//! valor) y valores vacíos que dejan el valor por defecto.

use regex_lite::Regex;

/// Cómo se dibuja la tabla.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TocStyle {
    /// Lista con viñetas anidada por nivel (por defecto).
    NestedList,
    /// Lista numerada anidada por nivel.
    NestedOrderedList,
    /// Solo el primer nivel, en una línea separada por ` | `.
    InlineFirstLevel,
}

/// Opciones de un bloque de tabla de contenidos.
#[derive(Clone, Debug)]
pub struct TocOptions {
    /// Texto (Markdown) que va antes de la tabla; vacío: sin título.
    pub title: String,
    pub style: TocStyle,
    /// Nivel mínimo incluido; 0: el menor nivel presente en la nota.
    pub min_level: u8,
    /// Nivel máximo incluido; 0: sin límite.
    pub max_level: u8,
    /// Solo los encabezados que coinciden (tiene prioridad sobre `exclude`).
    pub include: Option<Expr>,
    /// Sin los encabezados que coinciden.
    pub exclude: Option<Expr>,
    /// Encabezados clicables (si no, texto plano).
    pub include_links: bool,
    /// Sin encabezados: no se muestra nada (si no, un aviso).
    pub hide_when_empty: bool,
}

impl Default for TocOptions {
    fn default() -> Self {
        Self {
            title: String::new(),
            style: TocStyle::NestedList,
            min_level: 0,
            max_level: 0,
            include: None,
            exclude: None,
            include_links: true,
            hide_when_empty: false,
        }
    }
}

/// Una expresión `/patrón/banderas` de `include` o `exclude`.
#[derive(Clone, Debug)]
pub struct Expr {
    re: Regex,
    /// Bandera `i`: `regex-lite` solo iguala mayúsculas en ASCII, así que además se compara con
    /// el encabezado en minúsculas (`/módulo/i` encuentra «MÓDULO»).
    fold: bool,
}

impl Expr {
    pub fn is_match(&self, text: &str) -> bool {
        self.re.is_match(text) || (self.fold && self.re.is_match(&text.to_lowercase()))
    }
}

/// Error de opciones: el nombre de la opción con un valor inválido (la tabla no se dibuja y se
/// avisa en su lugar).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InvalidOption(pub String);

/// Lee las opciones del cuerpo del codeblock. Las claves desconocidas se ignoran.
pub fn parse_options(body: &str) -> Result<TocOptions, InvalidOption> {
    let mut o = TocOptions::default();
    for line in body.lines() {
        if line.starts_with('#') {
            continue;
        }
        // La clave es el tramo de caracteres válidos que termina justo antes de un `:`.
        let Some((key, value)) = split_key(line) else {
            continue;
        };
        if value.is_empty() {
            continue;
        }
        let invalid = || InvalidOption(key.to_owned());
        let raw = value.trim();
        let plain = || strip_comment(raw);
        match key {
            "title" => {
                o.title = match raw {
                    "null" | "\"\"" | "''" => String::new(),
                    t => t.to_owned(),
                }
            }
            "style" => {
                o.style = match plain() {
                    "nestedList" | "lista" => TocStyle::NestedList,
                    "nestedOrderedList" | "lista-ordenada" => TocStyle::NestedOrderedList,
                    "inlineFirstLevel" | "linea-primer-nivel" => TocStyle::InlineFirstLevel,
                    _ => return Err(invalid()),
                }
            }
            "minLevel" | "maxLevel" => {
                let n = parse_int(plain()).ok_or_else(invalid)?;
                let n = n.min(u8::MAX as u64) as u8;
                if key == "minLevel" {
                    o.min_level = n;
                } else {
                    o.max_level = n;
                }
            }
            "include" | "exclude" => {
                if raw == "null" || raw.is_empty() {
                    continue;
                }
                let re = parse_regex(raw).ok_or_else(invalid)?;
                if key == "include" {
                    o.include = Some(re);
                } else {
                    o.exclude = Some(re);
                }
            }
            // Depuración de otros programas: se acepta con cualquier valor y no hace nada.
            "debugInConsole" => {}
            "includeLinks" | "hideWhenEmpty" => {
                let b = match plain() {
                    "true" => true,
                    "false" => false,
                    _ => return Err(invalid()),
                };
                if key == "includeLinks" {
                    o.include_links = b;
                } else {
                    o.hide_when_empty = b;
                }
            }
            _ => {}
        }
    }
    Ok(o)
}

/// `clave: valor` con la clave de letras, dígitos, `.`, `_` y espacios (recortada).
fn split_key(line: &str) -> Option<(&str, &str)> {
    let bytes = line.as_bytes();
    let valid = |b: u8| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b' ');
    let start = bytes.iter().position(|&b| valid(b))?;
    let mut end = start;
    while end < bytes.len() && valid(bytes[end]) {
        end += 1;
    }
    if bytes.get(end) != Some(&b':') {
        return None;
    }
    let key = line[start..end].trim();
    if key.is_empty() {
        return None;
    }
    Some((key, &line[end + 1..]))
}

/// Quita un comentario `# …` del final (desde el último `#`).
fn strip_comment(v: &str) -> &str {
    match v.rfind('#') {
        Some(i) => v[..i].trim(),
        None => v,
    }
}

/// Entero de los dígitos iniciales; negativo o sin dígitos es inválido.
fn parse_int(v: &str) -> Option<u64> {
    let digits: &str = &v[..v.bytes().take_while(u8::is_ascii_digit).count()];
    digits.parse().ok()
}

/// Expresión regular `/patrón/banderas`. Las banderas `i`, `m` y `s` pasan a la expresión; `g`, `u`,
/// `y` y `d` no cambian una prueba de coincidencia.
fn parse_regex(v: &str) -> Option<Expr> {
    let rest = v.strip_prefix('/')?;
    let close = rest.rfind('/')?;
    let (pattern, flags) = (&rest[..close], &rest[close + 1..]);
    if pattern.is_empty() || !flags.bytes().all(|b| b.is_ascii_lowercase()) {
        return None;
    }
    let inline: String = flags
        .chars()
        .filter(|c| matches!(c, 'i' | 'm' | 's'))
        .collect();
    let full = if inline.is_empty() {
        pattern.to_owned()
    } else {
        format!("(?{inline}){pattern}")
    };
    Some(Expr {
        re: Regex::new(&full).ok()?,
        fold: flags.contains('i'),
    })
}

impl TocOptions {
    fn matches(&self, heading: &str) -> bool {
        match (&self.include, &self.exclude) {
            (Some(re), _) => re.is_match(heading),
            (None, Some(re)) => !re.is_match(heading),
            (None, None) => true,
        }
    }

    fn first_level(&self, headings: &[(u8, &str)]) -> u8 {
        if self.min_level > 0 {
            self.min_level
        } else {
            headings.iter().map(|h| h.0).min().unwrap_or(0)
        }
    }

    /// Índices de los encabezados que entran en la tabla, en orden. Un
    /// encabezado que `include`/`exclude` deja afuera se lleva a sus subencabezados, y en
    /// `inlineFirstLevel` solo cuenta el primer nivel.
    pub fn select(&self, headings: &[(u8, &str)]) -> Vec<usize> {
        let first = self.first_level(headings);
        let mut out = Vec::new();
        if self.style == TocStyle::InlineFirstLevel {
            for (n, (level, text)) in headings.iter().enumerate() {
                if *level == first && !text.is_empty() && self.matches(text) {
                    out.push(n);
                }
            }
            return out;
        }
        let mut skip_below = 0u8;
        for (n, (level, text)) in headings.iter().enumerate() {
            if skip_below > 0 && *level > skip_below {
                continue;
            }
            if *level <= skip_below {
                skip_below = 0;
            }
            if !self.matches(text) {
                skip_below = *level;
                continue;
            }
            if *level < first || (self.max_level > 0 && *level > self.max_level) {
                continue;
            }
            if !text.is_empty() {
                out.push(n);
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Un bloque con todas las opciones, valores vacíos y comentarios.
    const COMPLETO: &str = "title: \nstyle: nestedList # TOC style (nestedList|nestedOrderedList|inlineFirstLevel)\nminLevel: 0 # Include headings from the specified level\nmaxLevel: 0 # Include headings up to the specified level\ninclude: \nexclude: \nincludeLinks: true # Make headings clickable\nhideWhenEmpty: false # Hide TOC if no headings are found\ndebugInConsole: false # Muestra información de depuración\n";

    #[test]
    fn bloque_con_comentarios_y_vacios_da_los_valores_por_defecto() {
        let o = parse_options(COMPLETO).unwrap();
        assert_eq!(o.title, "");
        assert_eq!(o.style, TocStyle::NestedList);
        assert_eq!((o.min_level, o.max_level), (0, 0));
        assert!(o.include.is_none() && o.exclude.is_none());
        assert!(o.include_links);
        assert!(!o.hide_when_empty);
        // Vacío también.
        let o = parse_options("").unwrap();
        assert_eq!(o.style, TocStyle::NestedList);
    }

    /// `debugInConsole` se acepta con cualquier valor y no cambia nada.
    #[test]
    fn debug_in_console_se_ignora() {
        for v in ["true", "false", "sí # comentario", "1", ""] {
            let o = parse_options(&format!("debugInConsole: {v}\nmaxLevel: 2")).unwrap();
            assert_eq!(o.max_level, 2, "{v}");
            assert!(o.include_links);
        }
    }

    #[test]
    fn lee_cada_opcion() {
        let o = parse_options(
            "title: **Índice** # con numeral\nstyle: nestedOrderedList\nminLevel: 2\nmaxLevel: 3 # hasta\ninclude: /Tema \\d+/i\nincludeLinks: false\nhideWhenEmpty: true",
        )
        .unwrap();
        // En el título el `#` es parte del valor.
        assert_eq!(o.title, "**Índice** # con numeral");
        assert_eq!(o.style, TocStyle::NestedOrderedList);
        assert_eq!((o.min_level, o.max_level), (2, 3));
        assert!(o.include.as_ref().unwrap().is_match("TEMA 4"));
        let e = parse_options("exclude: /módulo/i")
            .unwrap()
            .exclude
            .unwrap();
        assert!(e.is_match("MÓDULO 1"));
        assert!(!o.include_links);
        assert!(o.hide_when_empty);
        assert_eq!(
            parse_options("title: ''").unwrap().title,
            "",
            "comillas vacías: sin título"
        );
    }

    #[test]
    fn valores_invalidos_se_informan() {
        assert_eq!(
            parse_options("style: arbol").unwrap_err(),
            InvalidOption("style".into())
        );
        assert_eq!(
            parse_options("minLevel: -1").unwrap_err(),
            InvalidOption("minLevel".into())
        );
        assert_eq!(
            parse_options("includeLinks: si").unwrap_err(),
            InvalidOption("includeLinks".into())
        );
        assert_eq!(
            parse_options("exclude: sin barras").unwrap_err(),
            InvalidOption("exclude".into())
        );
        // Claves desconocidas y líneas sin `:` se ignoran.
        assert!(parse_options("otra: cosa\n# comentario\nsuelto").is_ok());
    }

    fn sel(o: &str, h: &[(u8, &str)]) -> Vec<usize> {
        parse_options(o).unwrap().select(h)
    }

    #[test]
    fn niveles_cero_sin_limite_y_minimo_desde_la_nota() {
        let h = [(2, "A"), (3, "B"), (6, "C"), (2, "")];
        assert_eq!(sel(COMPLETO, &h), vec![0, 1, 2]);
        assert_eq!(sel("minLevel: 3", &h), vec![1, 2]);
        assert_eq!(sel("maxLevel: 3", &h), vec![0, 1]);
    }

    #[test]
    fn excluir_un_encabezado_saca_sus_hijos() {
        let h = [(1, "A"), (2, "Flashcards"), (3, "x"), (2, "B"), (3, "y")];
        assert_eq!(sel("exclude: /flash/i", &h), vec![0, 3, 4]);
        // Con include, lo que no coincide también se lleva a sus hijos.
        assert_eq!(sel("include: /^[AB]$/", &h), vec![0, 3]);
    }

    #[test]
    fn primer_nivel_en_linea() {
        let h = [(2, "A"), (3, "B"), (2, "C")];
        assert_eq!(sel("style: inlineFirstLevel", &h), vec![0, 2]);
        assert_eq!(sel("style: linea-primer-nivel", &h), vec![0, 2]);
    }
}
