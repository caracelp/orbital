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

/// Valores por defecto de las opciones que se eligen en Preferencias (035): las que no son
/// expresiones. Un bloque que no escribe una opción toma la de acá.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TocDefaults {
    pub title: String,
    pub style: TocStyle,
    pub min_level: u8,
    pub max_level: u8,
    pub include_links: bool,
    pub hide_when_empty: bool,
}

impl Default for TocDefaults {
    fn default() -> Self {
        let o = TocOptions::default();
        Self {
            title: o.title,
            style: o.style,
            min_level: o.min_level,
            max_level: o.max_level,
            include_links: o.include_links,
            hide_when_empty: o.hide_when_empty,
        }
    }
}

impl TocDefaults {
    /// Las opciones de un bloque vacío con estos valores.
    pub fn options(&self) -> TocOptions {
        TocOptions {
            title: self.title.clone(),
            style: self.style,
            min_level: self.min_level,
            max_level: self.max_level,
            include: None,
            exclude: None,
            include_links: self.include_links,
            hide_when_empty: self.hide_when_empty,
        }
    }
}

impl TocStyle {
    /// Los tres estilos, en el orden en que se ofrecen.
    pub const TODOS: [TocStyle; 3] = [
        TocStyle::NestedList,
        TocStyle::NestedOrderedList,
        TocStyle::InlineFirstLevel,
    ];

    /// El nombre en español que se escribe en el bloque (`lista`, `lista-ordenada`,
    /// `linea-primer-nivel`).
    pub fn alias(self) -> &'static str {
        match self {
            TocStyle::NestedList => "lista",
            TocStyle::NestedOrderedList => "lista-ordenada",
            TocStyle::InlineFirstLevel => "linea-primer-nivel",
        }
    }

    /// Cualquiera de los dos nombres de un estilo.
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "nestedList" | "lista" => Some(TocStyle::NestedList),
            "nestedOrderedList" | "lista-ordenada" => Some(TocStyle::NestedOrderedList),
            "inlineFirstLevel" | "linea-primer-nivel" => Some(TocStyle::InlineFirstLevel),
            _ => None,
        }
    }
}

/// El texto de un bloque ` ```toc ` con las opciones de `elegidas` que difieren de `base` (los
/// valores por defecto vigentes): lo que coincide no se escribe, así el bloque sigue a
/// Preferencias. Sin salto de línea al final (lo pone quien lo inserta).
pub fn bloque(elegidas: &TocDefaults, base: &TocDefaults) -> String {
    let mut s = String::from("```toc\n");
    if elegidas.title != base.title {
        // Un título vacío frente a uno por defecto: `''` lo borra.
        let t = elegidas.title.trim();
        if t.is_empty() {
            s.push_str("title: ''\n");
        } else {
            s.push_str(&format!("title: {t}\n"));
        }
    }
    if elegidas.style != base.style {
        s.push_str(&format!("style: {}\n", elegidas.style.alias()));
    }
    if elegidas.min_level != base.min_level {
        s.push_str(&format!("minLevel: {}\n", elegidas.min_level));
    }
    if elegidas.max_level != base.max_level {
        s.push_str(&format!("maxLevel: {}\n", elegidas.max_level));
    }
    if elegidas.include_links != base.include_links {
        s.push_str(&format!("includeLinks: {}\n", elegidas.include_links));
    }
    if elegidas.hide_when_empty != base.hide_when_empty {
        s.push_str(&format!("hideWhenEmpty: {}\n", elegidas.hide_when_empty));
    }
    s.push_str("```");
    s
}

/// Lee las opciones del cuerpo del codeblock con los valores por defecto de fábrica. Las claves
/// desconocidas se ignoran.
pub fn parse_options(body: &str) -> Result<TocOptions, InvalidOption> {
    parse_options_with(body, TocOptions::default())
}

/// Como [`parse_options`], partiendo de `base` (los valores por defecto de Preferencias): lo que
/// el bloque escribe manda.
pub fn parse_options_with(body: &str, base: TocOptions) -> Result<TocOptions, InvalidOption> {
    let mut o = base;
    for line in body.lines() {
        if line.starts_with('#') {
            continue;
        }
        // La clave es el tramo de caracteres válidos que termina justo antes de un `:`.
        let Some((key, value)) = split_key(line) else {
            continue;
        };
        let raw = value.trim();
        // Un valor vacío (o solo un comentario, salvo en el título y las expresiones, donde `#`
        // es parte del valor) deja el valor por defecto.
        if raw.is_empty()
            || (!matches!(key, "title" | "include" | "exclude") && strip_comment(raw).is_empty())
        {
            continue;
        }
        let invalid = || InvalidOption(key.to_owned());
        let plain = || strip_comment(raw);
        match key {
            "title" => {
                o.title = match raw {
                    "null" | "\"\"" | "''" => String::new(),
                    t => t.to_owned(),
                }
            }
            "style" => o.style = TocStyle::from_name(plain()).ok_or_else(invalid)?,
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
            // Depuración: se acepta con cualquier valor y no hace nada.
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

/// HTML estático de la tabla (exportar, 036): el título como texto y la lista anidada (o el
/// primer nivel en una línea), sin enlaces (las páginas exportadas no tienen anclas). Sin
/// encabezados, el aviso o nada.
pub fn html(o: &TocOptions, headings: &[(u8, &str)]) -> String {
    let esc = crate::bloque_medios::escapar;
    let elegidos: Vec<(u8, &str)> = o
        .select(headings)
        .into_iter()
        .map(|i| headings[i])
        .collect();
    let mut s = String::from("<nav class=\"orbital-toc\">");
    if !o.title.is_empty() {
        s.push_str(&format!("<p>{}</p>", esc(&o.title)));
    }
    if elegidos.is_empty() {
        if o.hide_when_empty {
            return String::new();
        }
        s.push_str("<p><em>Tabla de contenidos: no hay encabezados</em></p></nav>");
        return s;
    }
    if o.style == TocStyle::InlineFirstLevel {
        let partes: Vec<String> = elegidos.iter().map(|(_, t)| esc(t)).collect();
        s.push_str(&format!("<p>{}</p></nav>", partes.join(" | ")));
        return s;
    }
    let (abre, cierra) = if o.style == TocStyle::NestedOrderedList {
        ("<ol>", "</ol>")
    } else {
        ("<ul>", "</ul>")
    };
    // Una pila de niveles abiertos: más profundo abre una sublista dentro del último ítem.
    let mut pila: Vec<u8> = Vec::new();
    for (nivel, t) in &elegidos {
        while pila.len() > 1
            && pila.last().is_some_and(|l| l > nivel)
            && pila[pila.len() - 2] >= *nivel
        {
            pila.pop();
            s.push_str("</li>");
            s.push_str(cierra);
        }
        match pila.last() {
            None => {
                s.push_str(abre);
                pila.push(*nivel);
            }
            Some(l) if l < nivel => {
                s.push_str(abre);
                pila.push(*nivel);
            }
            _ => s.push_str("</li>"),
        }
        s.push_str(&format!("<li>{}", esc(t)));
    }
    while pila.pop().is_some() {
        s.push_str("</li>");
        s.push_str(cierra);
    }
    s.push_str("</nav>");
    s
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

    #[test]
    fn el_bloque_manda_sobre_los_valores_por_defecto() {
        let base = TocDefaults {
            title: "En esta nota".into(),
            style: TocStyle::NestedOrderedList,
            max_level: 2,
            hide_when_empty: true,
            ..TocDefaults::default()
        };
        let o = parse_options_with("", base.options()).unwrap();
        assert_eq!(o.title, "En esta nota");
        assert_eq!(o.style, TocStyle::NestedOrderedList);
        assert_eq!(o.max_level, 2);
        assert!(o.hide_when_empty);
        let o = parse_options_with(
            "style: lista\nmaxLevel: 0\ntitle: ''\nhideWhenEmpty: false",
            base.options(),
        )
        .unwrap();
        assert_eq!(o.title, "");
        assert_eq!(o.style, TocStyle::NestedList);
        assert_eq!(o.max_level, 0);
        assert!(!o.hide_when_empty);
        // Un valor vacío deja el de Preferencias.
        let o = parse_options_with("maxLevel: \nstyle: # nada", base.options()).unwrap();
        assert_eq!((o.max_level, o.style), (2, TocStyle::NestedOrderedList));
    }

    #[test]
    fn bloque_escribe_solo_lo_distinto() {
        let base = TocDefaults::default();
        assert_eq!(bloque(&base, &base), "```toc\n```");
        let elegidas = TocDefaults {
            title: "Índice ñ".into(),
            style: TocStyle::InlineFirstLevel,
            min_level: 2,
            max_level: 4,
            include_links: false,
            hide_when_empty: true,
        };
        let b = bloque(&elegidas, &base);
        assert_eq!(
            b,
            "```toc\ntitle: Índice ñ\nstyle: linea-primer-nivel\nminLevel: 2\nmaxLevel: 4\nincludeLinks: false\nhideWhenEmpty: true\n```"
        );
        // Y se lee igual.
        let cuerpo = b.trim_start_matches("```toc\n").trim_end_matches("```");
        let o = parse_options(cuerpo).unwrap();
        assert_eq!(o.title, "Índice ñ");
        assert_eq!(o.style, TocStyle::InlineFirstLevel);
        assert_eq!((o.min_level, o.max_level), (2, 4));
        assert!(!o.include_links && o.hide_when_empty);
        // Sin título frente a uno por defecto.
        let con_titulo = TocDefaults {
            title: "X".into(),
            ..TocDefaults::default()
        };
        assert_eq!(bloque(&base, &con_titulo), "```toc\ntitle: ''\n```");
    }

    #[test]
    fn html_anidado_y_en_linea() {
        let h = [(1, "A"), (2, "B <x>"), (3, "C"), (2, "D"), (1, "E")];
        assert_eq!(
            html(&parse_options("").unwrap(), &h),
            "<nav class=\"orbital-toc\"><ul><li>A<ul><li>B &lt;x&gt;<ul><li>C</li></ul></li><li>D</li></ul></li><li>E</li></ul></nav>"
        );
        assert_eq!(
            html(&parse_options("style: linea-primer-nivel").unwrap(), &h),
            "<nav class=\"orbital-toc\"><p>A | E</p></nav>"
        );
        assert!(html(&parse_options("style: lista-ordenada").unwrap(), &h).contains("<ol>"));
        assert_eq!(
            html(&parse_options("hideWhenEmpty: true").unwrap(), &[]),
            ""
        );
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
