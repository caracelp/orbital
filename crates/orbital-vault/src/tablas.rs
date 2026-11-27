//! Tablas avanzadas en Markdown GFM (064): detección, formateo alineado, navegación por celdas,
//! manipulación de filas y columnas, orden, CSV y fórmulas `<!-- TBLFM: ... -->`.
//!
//! Todo es puro (sin GTK ni E/S), pensado para operar en microsegundos sobre tablas en memoria.

use std::cmp::Ordering;

use crate::fold::fold;

/// Alineación de una columna Markdown.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Alineacion {
    #[default]
    Ninguna,
    Izquierda,
    Centro,
    Derecha,
}

impl Alineacion {
    fn desde_celda(s: &str) -> Option<Self> {
        let t = s.trim();
        let izq = t.starts_with(':');
        let der = t.ends_with(':');
        let centro = t.trim_start_matches(':').trim_end_matches(':');
        if centro.is_empty() || !centro.bytes().all(|b| b == b'-') {
            return None;
        }
        Some(match (izq, der) {
            (true, true) => Self::Centro,
            (true, false) => Self::Izquierda,
            (false, true) => Self::Derecha,
            (false, false) => Self::Ninguna,
        })
    }

    fn separador(self, ancho: usize) -> String {
        let ancho = ancho.max(3);
        match self {
            Self::Ninguna => "-".repeat(ancho),
            Self::Izquierda => format!(":{}", "-".repeat(ancho - 1)),
            Self::Derecha => format!("{}:", "-".repeat(ancho - 1)),
            Self::Centro => format!(":{}:", "-".repeat(ancho.saturating_sub(2).max(1))),
        }
    }
}

/// Tabla Markdown GFM parseada.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Tabla {
    /// Fila 0 = cabecera; filas 1.. = datos. Todas normalizadas a `columnas()` celdas.
    pub filas: Vec<Vec<String>>,
    pub alineaciones: Vec<Alineacion>,
    /// Sangría inicial (espacios antes del primer `|`), preservada al reformatear.
    pub sangria: String,
    /// Líneas `<!-- TBLFM: ... -->` que seguían inmediatamente a la tabla, si las hay.
    pub formulas: Vec<String>,
}

/// Tabla ubicada dentro de un texto (rangos de líneas 0-indexados, inclusivo).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TablaEnTexto {
    pub linea_inicio: usize,
    /// Última línea de la tabla (sin incluir líneas `<!-- TBLFM: ... -->`).
    pub linea_fin_tabla: usize,
    /// Última línea del bloque (incluyendo líneas `<!-- TBLFM: ... -->` si existen).
    pub linea_fin_total: usize,
    pub tabla: Tabla,
    /// Fila lógica del cursor (0 = cabecera, 1.. = datos; si estaba en el separador, 0).
    pub fila_cursor: usize,
    /// Si el cursor estaba en la línea separadora (línea `linea_inicio + 1`).
    pub en_separador: bool,
    /// Columna 0-indexada bajo el cursor.
    pub col_cursor: usize,
}

/// Resultado de una edición de tabla: texto nuevo del bloque (sin salto final) y nueva posición
/// del cursor relativa al inicio del bloque `(linea_relativa, col_en_caracteres)`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EdicionTabla {
    pub texto: String,
    pub linea_rel: usize,
    pub col_char: usize,
}

/// Parte una línea Markdown en celdas respetando `\|` escapado y `` `|` `` dentro de código en línea.
pub fn dividir_celdas(linea: &str) -> Vec<String> {
    let trimmed = linea.trim();
    let inner = trimmed.strip_prefix('|').unwrap_or(trimmed);
    // Quitar `|` final solo si no está escapado.
    let inner = if let Some(sin_fin) = inner.strip_suffix('|') {
        let barras = sin_fin.bytes().rev().take_while(|&b| b == b'\\').count();
        if barras % 2 == 0 { sin_fin } else { inner }
    } else {
        inner
    };

    let mut celdas = Vec::new();
    let mut actual = String::new();
    let mut en_codigo = false;
    let mut chars = inner.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\\' => {
                actual.push(c);
                if let Some(sig) = chars.next() {
                    actual.push(sig);
                }
            }
            '`' => {
                en_codigo = !en_codigo;
                actual.push(c);
            }
            '|' if !en_codigo => {
                celdas.push(actual.trim().to_owned());
                actual.clear();
            }
            _ => actual.push(c),
        }
    }
    celdas.push(actual.trim().to_owned());
    celdas
}

fn parece_fila_tabla(linea: &str) -> bool {
    let t = linea.trim();
    !t.is_empty() && t.contains('|') && !es_linea_formula(t)
}

fn es_linea_separadora(linea: &str) -> Option<Vec<Alineacion>> {
    let t = linea.trim();
    if !t.contains('|') && !t.contains('-') {
        return None;
    }
    let celdas = dividir_celdas(t);
    if celdas.is_empty() {
        return None;
    }
    celdas
        .iter()
        .map(|c| Alineacion::desde_celda(c))
        .collect::<Option<Vec<_>>>()
}

/// Verdadero si la línea es un comentario `<!-- TBLFM: ... -->`.
pub fn es_linea_formula(linea: &str) -> bool {
    let t = linea.trim();
    t.starts_with("<!--")
        && t.ends_with("-->")
        && t[4..t.len() - 3]
            .trim_start()
            .to_ascii_uppercase()
            .starts_with("TBLFM:")
}

/// Calcula en qué columna (0-indexada) cae `col_char` dentro de una línea de tabla.
fn columna_en_linea(linea: &str, col_char: usize) -> usize {
    let mut col = 0usize;
    let mut vio_primer_pipe = false;
    let mut en_codigo = false;
    let mut escape = false;
    for (idx, c) in linea.chars().enumerate() {
        if idx >= col_char {
            break;
        }
        if escape {
            escape = false;
            continue;
        }
        match c {
            '\\' => escape = true,
            '`' => en_codigo = !en_codigo,
            '|' if !en_codigo => {
                if !vio_primer_pipe
                    && linea[..linea.char_indices().nth(idx).map_or(0, |(b, _)| b)]
                        .trim()
                        .is_empty()
                {
                    vio_primer_pipe = true;
                } else {
                    col += 1;
                }
            }
            _ => {}
        }
    }
    col
}

/// Detecta si `linea_cursor` (0-indexada) cae dentro de una tabla Markdown GFM (con al menos
/// cabecera + línea separadora). Devuelve la tabla parseada y su ubicación exacta.
pub fn detectar_tabla(
    lineas: &[&str],
    linea_cursor: usize,
    col_char: usize,
) -> Option<TablaEnTexto> {
    if linea_cursor >= lineas.len() {
        return None;
    }
    // No detectar dentro de bloques de código cercados.
    let mut en_cerca = false;
    for l in &lineas[..=linea_cursor] {
        let t = l.trim();
        if t.starts_with("```") || t.starts_with("~~~") {
            en_cerca = !en_cerca;
        }
    }
    if en_cerca {
        return None;
    }
    // Si el cursor está en una línea TBLFM, subimos a la fila de tabla inmediatamente arriba.
    let mut ancla = linea_cursor;
    while ancla > 0 && es_linea_formula(lineas[ancla]) {
        ancla -= 1;
    }
    if !parece_fila_tabla(lineas[ancla]) {
        return None;
    }

    let mut inicio = ancla;
    while inicio > 0 && parece_fila_tabla(lineas[inicio - 1]) {
        inicio -= 1;
    }
    let mut fin_tabla = ancla;
    while fin_tabla + 1 < lineas.len() && parece_fila_tabla(lineas[fin_tabla + 1]) {
        fin_tabla += 1;
    }
    // Requiere al menos 2 líneas y que la segunda sea separadora válida.
    if fin_tabla < inicio + 1 {
        return None;
    }
    let alineaciones_raw = es_linea_separadora(lineas[inicio + 1])?;

    let sangria_len = lineas[inicio].len() - lineas[inicio].trim_start().len();
    let sangria = lineas[inicio][..sangria_len].to_owned();

    let mut filas = Vec::new();
    let mut num_cols = alineaciones_raw.len();
    for (offset, &l) in lineas[inicio..=fin_tabla].iter().enumerate() {
        if offset == 1 {
            continue;
        }
        let celdas = dividir_celdas(l);
        num_cols = num_cols.max(celdas.len());
        filas.push(celdas);
    }
    if num_cols == 0 {
        return None;
    }
    for f in &mut filas {
        f.resize(num_cols, String::new());
    }
    let mut alineaciones = alineaciones_raw;
    alineaciones.resize(num_cols, Alineacion::Ninguna);

    let mut fin_total = fin_tabla;
    let mut formulas = Vec::new();
    while fin_total + 1 < lineas.len() && es_linea_formula(lineas[fin_total + 1]) {
        fin_total += 1;
        formulas.push(lineas[fin_total].trim().to_owned());
    }

    let en_separador = linea_cursor == inicio + 1;
    let fila_cursor = if linea_cursor <= inicio + 1 {
        0
    } else if linea_cursor > fin_tabla {
        filas.len().saturating_sub(1)
    } else {
        (linea_cursor - inicio - 1).min(filas.len().saturating_sub(1))
    };
    let col_cursor = columna_en_linea(lineas[ancla], col_char).min(num_cols - 1);

    Some(TablaEnTexto {
        linea_inicio: inicio,
        linea_fin_tabla: fin_tabla,
        linea_fin_total: fin_total,
        tabla: Tabla {
            filas,
            alineaciones,
            sangria,
            formulas,
        },
        fila_cursor,
        en_separador,
        col_cursor,
    })
}

impl Tabla {
    pub fn columnas(&self) -> usize {
        self.alineaciones.len()
    }

    /// Calcula el ancho en caracteres de cada columna (mínimo 3).
    fn anchos(&self) -> Vec<usize> {
        let mut anchos = vec![3usize; self.columnas()];
        for fila in &self.filas {
            for (c, celda) in fila.iter().enumerate() {
                if c < anchos.len() {
                    anchos[c] = anchos[c].max(celda.chars().count());
                }
            }
        }
        anchos
    }

    /// Formatea la tabla con columnas alineadas y devuelve el texto completo (incluidas fórmulas).
    pub fn formatear(&self) -> String {
        let anchos = self.anchos();
        let mut lineas = Vec::with_capacity(self.filas.len() + 1 + self.formulas.len());
        let emitir_fila = |celdas: &[String]| -> String {
            let mut s = String::new();
            s.push_str(&self.sangria);
            s.push('|');
            for (c, ancho) in anchos.iter().enumerate() {
                let txt = celdas.get(c).map(String::as_str).unwrap_or("");
                let len = txt.chars().count();
                let pad = ancho.saturating_sub(len);
                let align = self.alineaciones.get(c).copied().unwrap_or_default();
                s.push(' ');
                match align {
                    Alineacion::Derecha => {
                        for _ in 0..pad {
                            s.push(' ');
                        }
                        s.push_str(txt);
                    }
                    Alineacion::Centro => {
                        let izq = pad / 2;
                        let der = pad - izq;
                        for _ in 0..izq {
                            s.push(' ');
                        }
                        s.push_str(txt);
                        for _ in 0..der {
                            s.push(' ');
                        }
                    }
                    Alineacion::Ninguna | Alineacion::Izquierda => {
                        s.push_str(txt);
                        for _ in 0..pad {
                            s.push(' ');
                        }
                    }
                }
                s.push_str(" |");
            }
            s
        };
        if let Some(cab) = self.filas.first() {
            lineas.push(emitir_fila(cab));
        }
        let mut sep = String::new();
        sep.push_str(&self.sangria);
        sep.push('|');
        for (c, &ancho) in anchos.iter().enumerate() {
            let align = self.alineaciones.get(c).copied().unwrap_or_default();
            sep.push(' ');
            sep.push_str(&align.separador(ancho));
            sep.push_str(" |");
        }
        lineas.push(sep);
        for fila in self.filas.iter().skip(1) {
            lineas.push(emitir_fila(fila));
        }
        for fm in &self.formulas {
            lineas.push(format!("{}{}", self.sangria, fm));
        }
        lineas.join("\n")
    }

    /// Calcula `(linea_rel, col_char)` para ubicar el cursor al inicio del contenido de `(fila, col)`
    /// en el texto devuelto por [`Self::formatear`].
    pub fn posicion_celda(&self, fila: usize, col: usize) -> (usize, usize) {
        let anchos = self.anchos();
        let linea_rel = if fila == 0 { 0 } else { fila + 1 };
        let col = col.min(anchos.len().saturating_sub(1));
        let mut x = self.sangria.chars().count() + 2; // "| "
        for w in anchos.iter().take(col) {
            x += w + 3; // " | "
        }
        (linea_rel, x)
    }

    /// Genera una edición formateada posicionando el cursor en `(fila, col)`.
    pub fn edicion(&self, fila: usize, col: usize) -> EdicionTabla {
        let fila = fila.min(self.filas.len().saturating_sub(1));
        let col = col.min(self.columnas().saturating_sub(1));
        let (linea_rel, col_char) = self.posicion_celda(fila, col);
        EdicionTabla {
            texto: self.formatear(),
            linea_rel,
            col_char,
        }
    }

    /// Navega con `Tab` (`adelante = true`) o `Shift+Tab` (`adelante = false`), creando una fila
    /// nueva al avanzar desde la última celda.
    pub fn navegar_tab(&mut self, fila: usize, col: usize, adelante: bool) -> EdicionTabla {
        let cols = self.columnas();
        if adelante {
            if col + 1 < cols {
                self.edicion(fila, col + 1)
            } else if fila + 1 < self.filas.len() {
                self.edicion(fila + 1, 0)
            } else {
                self.filas.push(vec![String::new(); cols]);
                self.edicion(self.filas.len() - 1, 0)
            }
        } else if col > 0 {
            self.edicion(fila, col - 1)
        } else if fila > 0 {
            self.edicion(fila - 1, cols.saturating_sub(1))
        } else {
            self.edicion(0, 0)
        }
    }

    /// Navega con `Enter`: baja a la fila siguiente en la misma columna (creando una fila nueva al
    /// final).
    pub fn navegar_enter(&mut self, fila: usize, col: usize) -> EdicionTabla {
        let cols = self.columnas();
        if fila + 1 < self.filas.len() {
            self.edicion(fila + 1, col)
        } else {
            self.filas.push(vec![String::new(); cols]);
            self.edicion(self.filas.len() - 1, col)
        }
    }

    /// Inserta una fila vacía arriba (`abajo = false`) o abajo (`abajo = true`) de `fila`.
    /// Nunca inserta encima de la cabecera (fila 0).
    pub fn insertar_fila(&mut self, fila: usize, abajo: bool) -> EdicionTabla {
        let cols = self.columnas();
        let destino = if abajo {
            (fila + 1).min(self.filas.len())
        } else {
            fila.max(1).min(self.filas.len())
        };
        self.filas.insert(destino, vec![String::new(); cols]);
        self.edicion(destino, 0)
    }

    /// Borra la fila `fila` (no borra la cabecera ni deja la tabla sin filas de datos si solo hay cabecera).
    pub fn borrar_fila(&mut self, fila: usize) -> EdicionTabla {
        if fila >= 1 && fila < self.filas.len() {
            self.filas.remove(fila);
        }
        let nueva = fila.min(self.filas.len().saturating_sub(1));
        self.edicion(nueva, 0)
    }

    /// Mueve la fila de datos `fila` hacia arriba o abajo.
    pub fn mover_fila(&mut self, fila: usize, arriba: bool, col: usize) -> EdicionTabla {
        let mut nueva = fila;
        if arriba && fila >= 2 && fila < self.filas.len() {
            self.filas.swap(fila, fila - 1);
            nueva = fila - 1;
        } else if !arriba && fila >= 1 && fila + 1 < self.filas.len() {
            self.filas.swap(fila, fila + 1);
            nueva = fila + 1;
        }
        self.edicion(nueva, col)
    }

    /// Inserta una columna a izquierda o derecha de `col`.
    pub fn insertar_columna(&mut self, col: usize, derecha: bool, fila: usize) -> EdicionTabla {
        let destino = if derecha {
            (col + 1).min(self.columnas())
        } else {
            col.min(self.columnas())
        };
        for f in &mut self.filas {
            f.insert(destino, String::new());
        }
        self.alineaciones.insert(destino, Alineacion::Ninguna);
        self.edicion(fila, destino)
    }

    /// Borra la columna `col` (siempre queda al menos 1 columna).
    pub fn borrar_columna(&mut self, col: usize, fila: usize) -> EdicionTabla {
        if self.columnas() > 1 && col < self.columnas() {
            for f in &mut self.filas {
                if col < f.len() {
                    f.remove(col);
                }
            }
            self.alineaciones.remove(col);
        }
        self.edicion(fila, col.min(self.columnas().saturating_sub(1)))
    }

    /// Mueve la columna `col` a izquierda o derecha.
    pub fn mover_columna(&mut self, col: usize, izquierda: bool, fila: usize) -> EdicionTabla {
        let mut nueva = col;
        if izquierda && col > 0 && col < self.columnas() {
            for f in &mut self.filas {
                f.swap(col, col - 1);
            }
            self.alineaciones.swap(col, col - 1);
            nueva = col - 1;
        } else if !izquierda && col + 1 < self.columnas() {
            for f in &mut self.filas {
                f.swap(col, col + 1);
            }
            self.alineaciones.swap(col, col + 1);
            nueva = col + 1;
        }
        self.edicion(fila, nueva)
    }

    /// Cambia la alineación de la columna `col`.
    pub fn alinear_columna(
        &mut self,
        col: usize,
        alineacion: Alineacion,
        fila: usize,
    ) -> EdicionTabla {
        if let Some(slot) = self.alineaciones.get_mut(col) {
            *slot = alineacion;
        }
        self.edicion(fila, col)
    }

    /// Ordena las filas de datos (1..) por la columna `col`. Si todas las celdas no vacías son
    /// numéricas, ordena por número; si no, alfabéticamente sin distinguir mayúsculas ni tildes.
    pub fn ordenar_por_columna(&mut self, col: usize, descendente: bool) -> EdicionTabla {
        if self.filas.len() <= 2 || col >= self.columnas() {
            return self.edicion(0, col);
        }
        let datos = &mut self.filas[1..];
        let numerico = datos
            .iter()
            .filter_map(|r| r.get(col).map(String::as_str))
            .filter(|s| !s.trim().is_empty())
            .all(|s| parse_numero(s).is_some());

        datos.sort_by(|a, b| {
            let va = a.get(col).map(String::as_str).unwrap_or("").trim();
            let vb = b.get(col).map(String::as_str).unwrap_or("").trim();
            let ord = if numerico {
                match (parse_numero(va), parse_numero(vb)) {
                    (Some(na), Some(nb)) => na.partial_cmp(&nb).unwrap_or(Ordering::Equal),
                    (Some(_), None) => Ordering::Less,
                    (None, Some(_)) => Ordering::Greater,
                    (None, None) => Ordering::Equal,
                }
            } else {
                fold(va).cmp(&fold(vb))
            };
            if descendente { ord.reverse() } else { ord }
        });
        self.edicion(1, col)
    }

    /// Exporta la tabla como CSV estándar (RFC 4180).
    pub fn a_csv(&self) -> String {
        let mut out = String::new();
        for (i, fila) in self.filas.iter().enumerate() {
            if i > 0 {
                out.push('\n');
            }
            for (c, celda) in fila.iter().enumerate() {
                if c > 0 {
                    out.push(',');
                }
                if celda.contains([',', '"', '\n', '\r']) {
                    out.push('"');
                    out.push_str(&celda.replace('"', "\"\""));
                    out.push('"');
                } else {
                    out.push_str(celda);
                }
            }
        }
        out
    }

    /// Evalúa todas las líneas `<!-- TBLFM: ... -->` asociadas a la tabla. Devuelve cuántas celdas
    /// se calcularon (o error legible en español).
    pub fn evaluar_formulas(&mut self) -> Result<usize, String> {
        if self.formulas.is_empty() {
            return Ok(0);
        }
        let formulas = self.formulas.clone();
        let mut aplicadas = 0usize;
        for linea in &formulas {
            let t = linea.trim();
            let cuerpo = t
                .strip_prefix("<!--")
                .and_then(|s| s.strip_suffix("-->"))
                .map(str::trim)
                .unwrap_or("");
            let Some(rest) = cuerpo
                .get(6..)
                .filter(|_| cuerpo.len() >= 6 && cuerpo[..6].eq_ignore_ascii_case("TBLFM:"))
            else {
                continue;
            };
            for asig in rest.split("::") {
                let asig = asig.trim();
                if asig.is_empty() {
                    continue;
                }
                let Some((lhs, rhs)) = asig.split_once('=') else {
                    return Err(format!("Fórmula sin «=»: {asig}"));
                };
                aplicadas += self.aplicar_asignacion_formula(lhs.trim(), rhs.trim())?;
            }
        }
        Ok(aplicadas)
    }

    fn aplicar_asignacion_formula(&mut self, lhs: &str, rhs: &str) -> Result<usize, String> {
        let (target_row, target_col) = parse_objetivo(lhs, self.filas.len(), self.columnas())
            .ok_or_else(|| format!("Destino de fórmula inválido: «{lhs}»"))?;

        let mut filas_objetivo = Vec::new();
        match target_row {
            Some(r) => filas_objetivo.push(r),
            None => {
                // Fórmula de columna (`$4=$2*$3`): aplica a todas las filas de datos (1..).
                for r in 1..self.filas.len() {
                    filas_objetivo.push(r);
                }
            }
        }
        let mut cont = 0;
        for r in filas_objetivo {
            let val = evaluar_expr(rhs, self, r, target_col)?;
            if r < self.filas.len() && target_col < self.columnas() {
                self.filas[r][target_col] = formatear_numero(val);
                cont += 1;
            }
        }
        Ok(cont)
    }
}

/// Crea una tabla nueva vacía de `filas_datos` filas de datos y `cols` columnas.
pub fn nueva_tabla(cols: usize, filas_datos: usize) -> Tabla {
    let cols = cols.max(1);
    let mut filas = Vec::with_capacity(filas_datos + 1);
    filas.push((1..=cols).map(|n| format!("Columna {n}")).collect());
    for _ in 0..filas_datos {
        filas.push(vec![String::new(); cols]);
    }
    Tabla {
        filas,
        alineaciones: vec![Alineacion::Ninguna; cols],
        sangria: String::new(),
        formulas: Vec::new(),
    }
}

/// Convierte texto CSV (comas, punto y coma o tabulaciones) en una tabla Markdown formateada.
pub fn csv_a_tabla(csv: &str) -> Option<Tabla> {
    let lineas_raw: Vec<&str> = csv.lines().filter(|l| !l.trim().is_empty()).collect();
    if lineas_raw.is_empty() {
        return None;
    }
    let primera = lineas_raw[0];
    let sep = if primera.contains('\t') {
        '\t'
    } else if primera.contains(';') && !primera.contains(',') {
        ';'
    } else {
        ','
    };
    let mut filas = Vec::with_capacity(lineas_raw.len());
    let mut cols = 0usize;
    for l in lineas_raw {
        let row = parse_csv_linea(l, sep);
        cols = cols.max(row.len());
        filas.push(row);
    }
    if cols == 0 {
        return None;
    }
    for f in &mut filas {
        for c in f.iter_mut() {
            *c = c.replace('|', "\\|");
        }
        f.resize(cols, String::new());
    }
    Some(Tabla {
        filas,
        alineaciones: vec![Alineacion::Ninguna; cols],
        sangria: String::new(),
        formulas: Vec::new(),
    })
}

fn parse_csv_linea(linea: &str, sep: char) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut en_comillas = false;
    let mut chars = linea.chars().peekable();
    while let Some(c) = chars.next() {
        if en_comillas {
            if c == '"' {
                if chars.peek() == Some(&'"') {
                    chars.next();
                    cur.push('"');
                } else {
                    en_comillas = false;
                }
            } else {
                cur.push(c);
            }
        } else if c == '"' && cur.trim().is_empty() {
            cur.clear();
            en_comillas = true;
        } else if c == sep {
            out.push(cur.trim().to_owned());
            cur.clear();
        } else {
            cur.push(c);
        }
    }
    out.push(cur.trim().to_owned());
    out
}

// ---------------------------------------------------------------------------
// Evaluador de fórmulas `<!-- TBLFM: ... -->`
// ---------------------------------------------------------------------------

fn parse_numero(s: &str) -> Option<f64> {
    let t = s.trim().replace(',', ".");
    let limpio = t
        .trim_start_matches(['$', '€', '£'])
        .trim_end_matches('%')
        .trim();
    if limpio.is_empty() {
        return None;
    }
    limpio.parse::<f64>().ok().filter(|v| v.is_finite())
}

fn formatear_numero(v: f64) -> String {
    if !v.is_finite() {
        return "0".to_owned();
    }
    if (v - v.round()).abs() < 1e-9 {
        format!("{}", v.round() as i64)
    } else {
        let s = format!("{v:.4}");
        s.trim_end_matches('0').trim_end_matches('.').to_owned()
    }
}

/// Parsea el lado izquierdo de una fórmula: `@>$2`, `@3$4` o `$4` (toda la columna de datos).
/// Devuelve `(fila_0_based_opcional, col_0_based)`.
fn parse_objetivo(
    lhs: &str,
    total_filas: usize,
    total_cols: usize,
) -> Option<(Option<usize>, usize)> {
    if let Some(rest) = lhs.strip_prefix('@') {
        let (r_part, c_part) = rest.split_once('$')?;
        let r = resolver_fila(r_part, total_filas, total_filas.saturating_sub(1))?;
        let c = resolver_col(c_part, total_cols, 0)?;
        Some((Some(r), c))
    } else if let Some(c_part) = lhs.strip_prefix('$') {
        let c = resolver_col(c_part, total_cols, 0)?;
        Some((None, c))
    } else {
        None
    }
}

/// Resuelve un índice de fila Org/TBLFM (`1` = cabecera, `2` o `I` = primera fila de datos,
/// `>` = última fila, `-1` = fila anterior a `cur_row`, `+1` = siguiente).
fn resolver_fila(tok: &str, total_filas: usize, cur_row: usize) -> Option<usize> {
    if total_filas == 0 {
        return None;
    }
    match tok {
        ">" => Some(total_filas - 1),
        "<" => Some(0),
        "I" | "i" => Some(1.min(total_filas - 1)),
        s if s.starts_with('-') => {
            let d: usize = s[1..].parse().ok()?;
            cur_row.checked_sub(d)
        }
        s if s.starts_with('+') => {
            let d: usize = s[1..].parse().ok()?;
            let r = cur_row + d;
            (r < total_filas).then_some(r)
        }
        s => {
            let n: usize = s.parse().ok()?;
            if n >= 1 && n <= total_filas {
                Some(n - 1)
            } else {
                None
            }
        }
    }
}

fn resolver_col(tok: &str, total_cols: usize, cur_col: usize) -> Option<usize> {
    if total_cols == 0 {
        return None;
    }
    match tok {
        ">" => Some(total_cols - 1),
        "<" => Some(0),
        s if s.starts_with('-') => {
            let d: usize = s[1..].parse().ok()?;
            cur_col.checked_sub(d)
        }
        s if s.starts_with('+') => {
            let d: usize = s[1..].parse().ok()?;
            let c = cur_col + d;
            (c < total_cols).then_some(c)
        }
        s => {
            let n: usize = s.parse().ok()?;
            if n >= 1 && n <= total_cols {
                Some(n - 1)
            } else {
                None
            }
        }
    }
}

fn resolver_ref(
    tok: &str,
    tabla: &Tabla,
    cur_row: usize,
    cur_col: usize,
) -> Option<(usize, usize)> {
    if let Some(rest) = tok.strip_prefix('@') {
        if let Some((r_part, c_part)) = rest.split_once('$') {
            let r = resolver_fila(r_part, tabla.filas.len(), cur_row)?;
            let c = resolver_col(c_part, tabla.columnas(), cur_col)?;
            Some((r, c))
        } else {
            let r = resolver_fila(rest, tabla.filas.len(), cur_row)?;
            Some((r, cur_col))
        }
    } else if let Some(c_part) = tok.strip_prefix('$') {
        let c = resolver_col(c_part, tabla.columnas(), cur_col)?;
        Some((cur_row, c))
    } else {
        None
    }
}

fn valores_rango(
    expr: &str,
    tabla: &Tabla,
    cur_row: usize,
    cur_col: usize,
) -> Result<Vec<f64>, String> {
    let mut vals = Vec::new();
    for parte in expr.split(',') {
        let p = parte.trim();
        if p.is_empty() {
            continue;
        }
        if let Some((a, b)) = p.split_once("..") {
            let (r0, c0) = resolver_ref(a.trim(), tabla, cur_row, cur_col)
                .ok_or_else(|| format!("Referencia inválida: «{}»", a.trim()))?;
            let (r1, c1) = resolver_ref(b.trim(), tabla, cur_row, cur_col)
                .ok_or_else(|| format!("Referencia inválida: «{}»", b.trim()))?;
            let (r_min, r_max) = (r0.min(r1), r0.max(r1));
            let (c_min, c_max) = (c0.min(c1), c0.max(c1));
            for r in r_min..=r_max {
                for c in c_min..=c_max {
                    if let Some(txt) = tabla.filas.get(r).and_then(|row| row.get(c))
                        && let Some(n) = parse_numero(txt)
                    {
                        vals.push(n);
                    }
                }
            }
        } else {
            vals.push(evaluar_expr(p, tabla, cur_row, cur_col)?);
        }
    }
    Ok(vals)
}

struct ExprParser<'a> {
    src: &'a str,
    pos: usize,
    tabla: &'a Tabla,
    cur_row: usize,
    cur_col: usize,
}

fn evaluar_expr(src: &str, tabla: &Tabla, cur_row: usize, cur_col: usize) -> Result<f64, String> {
    let mut p = ExprParser {
        src,
        pos: 0,
        tabla,
        cur_row,
        cur_col,
    };
    let v = p.parse_add()?;
    p.skip_ws();
    if p.pos < p.src.len() {
        return Err(format!(
            "Texto inesperado en fórmula: «{}»",
            &p.src[p.pos..]
        ));
    }
    Ok(v)
}

impl<'a> ExprParser<'a> {
    fn skip_ws(&mut self) {
        while let Some(c) = self.peek() {
            if c.is_whitespace() {
                self.pos += c.len_utf8();
            } else {
                break;
            }
        }
    }

    fn peek(&self) -> Option<char> {
        self.src[self.pos..].chars().next()
    }

    fn parse_add(&mut self) -> Result<f64, String> {
        let mut lhs = self.parse_mul()?;
        loop {
            self.skip_ws();
            match self.peek() {
                Some('+') => {
                    self.pos += 1;
                    lhs += self.parse_mul()?;
                }
                Some('-') => {
                    self.pos += 1;
                    lhs -= self.parse_mul()?;
                }
                _ => break,
            }
        }
        Ok(lhs)
    }

    fn parse_mul(&mut self) -> Result<f64, String> {
        let mut lhs = self.parse_unary()?;
        loop {
            self.skip_ws();
            match self.peek() {
                Some('*') => {
                    self.pos += 1;
                    lhs *= self.parse_unary()?;
                }
                Some('/') => {
                    self.pos += 1;
                    let rhs = self.parse_unary()?;
                    lhs = if rhs.abs() < 1e-12 { 0.0 } else { lhs / rhs };
                }
                _ => break,
            }
        }
        Ok(lhs)
    }

    fn parse_unary(&mut self) -> Result<f64, String> {
        self.skip_ws();
        match self.peek() {
            Some('+') => {
                self.pos += 1;
                self.parse_unary()
            }
            Some('-') => {
                self.pos += 1;
                Ok(-self.parse_unary()?)
            }
            _ => self.parse_primary(),
        }
    }

    fn parse_primary(&mut self) -> Result<f64, String> {
        self.skip_ws();
        let rest = &self.src[self.pos..];
        if rest.starts_with('(') {
            self.pos += 1;
            let v = self.parse_add()?;
            self.skip_ws();
            if self.peek() != Some(')') {
                return Err("Falta paréntesis de cierre en la fórmula".into());
            }
            self.pos += 1;
            return Ok(v);
        }
        // Referencia de celda `@...` o `$...`
        if rest.starts_with('@') || rest.starts_with('$') {
            let start = self.pos;
            self.pos += 1;
            while let Some(c) = self.peek() {
                if c.is_ascii_alphanumeric() || matches!(c, '$' | '>' | '<' | '-' | '+') {
                    // Detener si el `-`/`+` no sigue a `@` o `$` (es operador binario).
                    if matches!(c, '-' | '+')
                        && !matches!(self.src.as_bytes()[self.pos - 1], b'@' | b'$')
                    {
                        break;
                    }
                    self.pos += c.len_utf8();
                } else {
                    break;
                }
            }
            let tok = &self.src[start..self.pos];
            let (r, c) = resolver_ref(tok, self.tabla, self.cur_row, self.cur_col)
                .ok_or_else(|| format!("Referencia inválida: «{tok}»"))?;
            let cell = self
                .tabla
                .filas
                .get(r)
                .and_then(|row| row.get(c))
                .map(String::as_str)
                .unwrap_or("");
            return Ok(parse_numero(cell).unwrap_or(0.0));
        }
        // Función: nombre(...)
        if self.peek().is_some_and(|c| c.is_ascii_alphabetic()) {
            let start = self.pos;
            while self
                .peek()
                .is_some_and(|c| c.is_ascii_alphanumeric() || c == '_')
            {
                self.pos += 1;
            }
            let nombre = self.src[start..self.pos].to_ascii_lowercase();
            self.skip_ws();
            if self.peek() != Some('(') {
                return Err(format!("Función desconocida o mal escrita: «{nombre}»"));
            }
            self.pos += 1;
            let arg_start = self.pos;
            let mut depth = 1usize;
            while self.pos < self.src.len() && depth > 0 {
                match self.src.as_bytes()[self.pos] {
                    b'(' => depth += 1,
                    b')' => depth -= 1,
                    _ => {}
                }
                if depth > 0 {
                    self.pos += 1;
                }
            }
            if depth != 0 {
                return Err(format!("Falta «)» en {nombre}(…)"));
            }
            let arg = &self.src[arg_start..self.pos];
            self.pos += 1; // ')'
            let vals = valores_rango(arg, self.tabla, self.cur_row, self.cur_col)?;
            return match nombre.as_str() {
                "sum" | "vsum" | "suma" => Ok(vals.iter().sum()),
                "avg" | "mean" | "vmean" | "promedio" => {
                    if vals.is_empty() {
                        Ok(0.0)
                    } else {
                        Ok(vals.iter().sum::<f64>() / vals.len() as f64)
                    }
                }
                "min" | "vmin" => Ok(vals.into_iter().reduce(f64::min).unwrap_or(0.0)),
                "max" | "vmax" => Ok(vals.into_iter().reduce(f64::max).unwrap_or(0.0)),
                "count" | "contar" => Ok(vals.len() as f64),
                "round" | "redondear" => Ok(vals.first().copied().unwrap_or(0.0).round()),
                _ => Err(format!("Función no soportada: «{nombre}»")),
            };
        }
        // Número literal
        let start = self.pos;
        while self.peek().is_some_and(|c| c.is_ascii_digit() || c == '.') {
            self.pos += 1;
        }
        if self.pos > start {
            return self.src[start..self.pos]
                .parse::<f64>()
                .map_err(|_| format!("Número inválido: «{}»", &self.src[start..self.pos]));
        }
        Err(format!(
            "Expresión inválida cerca de «{}»",
            &self.src[self.pos..]
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detectar_formatear_y_navegar() {
        let txt = "| Producto | Precio |\n| --- | ---: |\n| Pan | 250 |\n";
        let lineas: Vec<&str> = txt.lines().collect();
        let mut t = detectar_tabla(&lineas, 2, 4).expect("detecta tabla");
        assert_eq!(t.fila_cursor, 1);
        assert_eq!(t.col_cursor, 0);

        let ed = t.tabla.navegar_tab(1, 1, true);
        assert_eq!(t.tabla.filas.len(), 3, "agrega fila al tabular al final");
        assert!(ed.texto.contains("| Producto | Precio |"));
        assert!(ed.texto.contains("| -------- | -----: |"));
        assert_eq!(ed.linea_rel, 3);
    }

    #[test]
    fn ordenar_numerico_y_alfabetico() {
        let txt = "| Item | Puntaje |\n| --- | --- |\n| B | 10 |\n| A | 2 |\n";
        let lineas: Vec<&str> = txt.lines().collect();
        let mut t = detectar_tabla(&lineas, 2, 2).unwrap();
        t.tabla.ordenar_por_columna(1, false);
        assert_eq!(t.tabla.filas[1][0], "A");
        assert_eq!(t.tabla.filas[2][0], "B");
        t.tabla.ordenar_por_columna(0, true);
        assert_eq!(t.tabla.filas[1][0], "B");
    }

    #[test]
    fn evaluar_formulas_tblfm() {
        let txt = "\
| Producto | Cant | Precio | Total |
| --- | ---: | ---: | ---: |
| Manzana | 3 | 120 |  |
| Pan | 2 | 250 |  |
| Total |  |  |  |
<!-- TBLFM: $4=$2*$3 :: @>$2=sum(@I..@-1) :: @>$4=sum(@I..@-1) -->";
        let lineas: Vec<&str> = txt.lines().collect();
        let mut t = detectar_tabla(&lineas, 2, 2).expect("detecta con tblfm");
        let n = t.tabla.evaluar_formulas().expect("evalúa tblfm");
        assert!(n >= 4);
        assert_eq!(t.tabla.filas[1][3], "360");
        assert_eq!(t.tabla.filas[2][3], "500");
        assert_eq!(t.tabla.filas[3][1], "5");
        assert_eq!(t.tabla.filas[3][3], "860");
    }

    #[test]
    fn csv_ida_y_vuelta() {
        let csv = "Nombre,Nota\n\"Ana, M.\",10\nLuis,8";
        let tabla = csv_a_tabla(csv).expect("csv válido");
        assert_eq!(tabla.filas[1][0], "Ana, M.");
        let out = tabla.a_csv();
        assert!(out.contains("\"Ana, M.\",10"));
    }
}
