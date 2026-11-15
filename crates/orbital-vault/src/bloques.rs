//! Bloques enlazables de una nota y sus identificadores (026, `^block-id`).
//!
//! Un recorrido de líneas (sin pulldown-cmark: cuesta el doble del presupuesto de 1 ms con 100 KB)
//! encuentra párrafos, ítems de lista, citas, callouts, tablas, bloques de código y bloques `$$`, y
//! dónde va el identificador de cada uno. Las ediciones solo agregan identificadores (o reemplazan
//! los repetidos): el resto del texto no cambia nunca.

use std::collections::HashSet;
use std::hash::{BuildHasher, Hasher};
use std::ops::Range;

/// Clase de bloque enlazable.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClaseBloque {
    Parrafo,
    Item,
    Cita,
    Callout,
    Tabla,
    Codigo,
    Matematica,
}

impl ClaseBloque {
    /// El identificador va en una línea propia debajo del bloque (si no, al final de su última línea).
    pub fn linea_propia(self) -> bool {
        matches!(self, Self::Tabla | Self::Codigo | Self::Matematica)
    }
}

/// Un bloque de una nota `.md`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BloqueFuente {
    pub clase: ClaseBloque,
    /// Bytes del bloque, sin el identificador.
    pub rango: Range<usize>,
    /// Bytes del identificador existente, sin el `^`.
    pub id: Option<Range<usize>>,
    /// Dónde va ` ^id` (fin de la última línea) o, si `linea_propia`, el comienzo de la línea
    /// siguiente al bloque.
    pub insertar_en: usize,
    /// Línea (desde 0) donde empieza el bloque.
    pub linea: usize,
}

/// Un cambio en el texto: `rango` vacío es una inserción.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Edicion {
    pub rango: Range<usize>,
    pub texto: TextoCorto,
}

/// Texto de una edición: a lo sumo 12 bytes (` id="xxxxxx"`), sin memoria dinámica (una nota
/// de 100 KB puede pedir miles: SC-001).
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct TextoCorto {
    bytes: [u8; 12],
    largo: u8,
}

impl TextoCorto {
    fn new() -> Self {
        Self {
            bytes: [0; 12],
            largo: 0,
        }
    }

    fn push_str(&mut self, s: &str) {
        let n = self.largo as usize;
        self.bytes[n..n + s.len()].copy_from_slice(s.as_bytes());
        self.largo += s.len() as u8;
    }

    pub fn as_str(&self) -> &str {
        std::str::from_utf8(&self.bytes[..self.largo as usize]).expect("ascii")
    }
}

impl std::fmt::Debug for TextoCorto {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self.as_str())
    }
}

/// Una línea sin el salto: `inicio..fin` en bytes; `siguiente` es el comienzo de la próxima.
struct Linea<'a> {
    texto: &'a str,
    inicio: usize,
    fin: usize,
    siguiente: usize,
    numero: usize,
}

fn lineas(texto: &str) -> impl Iterator<Item = Linea<'_>> {
    let mut inicio = 0;
    let mut numero = 0;
    std::iter::from_fn(move || {
        if inicio >= texto.len() {
            return None;
        }
        let resto = &texto[inicio..];
        let (largo, salto) = match resto.find('\n') {
            Some(n) => (n, 1),
            None => (resto.len(), 0),
        };
        let mut fin = inicio + largo;
        if fin > inicio && texto.as_bytes()[fin - 1] == b'\r' {
            fin -= 1;
        }
        let l = Linea {
            texto: &texto[inicio..fin],
            inicio,
            fin,
            siguiente: inicio + largo + salto,
            numero,
        };
        inicio += largo + salto;
        numero += 1;
        Some(l)
    })
}

/// Fin del frontmatter (`---` … `---`) si la nota empieza con uno.
fn fin_frontmatter(texto: &str) -> usize {
    let cuerpo = texto.strip_prefix('\u{feff}').unwrap_or(texto);
    let bom = texto.len() - cuerpo.len();
    let mut ls = lineas(cuerpo);
    match ls.next() {
        Some(l) if l.texto.trim_end() == "---" => {}
        _ => return 0,
    }
    for l in ls {
        if l.texto.trim_end() == "---" || l.texto.trim_end() == "..." {
            return bom + l.siguiente;
        }
    }
    0
}

/// `^id` al final de la línea (separado por un espacio): bytes del id sin el `^`.
fn id_al_final(linea: &str, base: usize) -> Option<Range<usize>> {
    let t = linea.trim_end();
    let inicio = t.rfind('^')?;
    let id = &t[inicio + 1..];
    if id.is_empty() || !id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-') {
        return None;
    }
    // Separado por un espacio (o solo en la línea): `x^n` no es un id.
    if inicio > 0 && !t.as_bytes()[inicio - 1].is_ascii_whitespace() {
        return None;
    }
    Some(base + inicio + 1..base + t.len())
}

/// Una línea que es solo `^id` (debajo de una tabla o un bloque de código).
fn linea_de_id(l: &Linea) -> Option<Range<usize>> {
    let t = l.texto.trim();
    if !t.starts_with('^') {
        return None;
    }
    let inicio = l.texto.find('^')?;
    id_al_final(l.texto, l.inicio).filter(|r| r.start == l.inicio + inicio + 1)
}

/// Marcador de ítem de lista (`- `, `* `, `+ `, `1. `, `1) `, tareas): sangría si lo es.
fn es_item(linea: &str) -> bool {
    let t = linea.trim_start();
    if t.starts_with("- ") || t.starts_with("* ") || t.starts_with("+ ") {
        return true;
    }
    let digitos = t.bytes().take_while(u8::is_ascii_digit).count();
    digitos > 0
        && digitos < t.len()
        && matches!(t.as_bytes()[digitos], b'.' | b')')
        && t[digitos + 1..].starts_with(' ')
}

fn es_encabezado(t: &str) -> bool {
    let n = t.bytes().take_while(|b| *b == b'#').count();
    (1..=6).contains(&n) && (t.len() == n || t.as_bytes()[n] == b' ')
}

fn es_regla(t: &str) -> bool {
    let mut marca = 0u8;
    let mut n = 0;
    for b in t.bytes() {
        match b {
            b' ' | b'\t' => {}
            b'-' | b'*' | b'_' if marca == 0 || marca == b => {
                marca = b;
                n += 1;
            }
            _ => return false,
        }
    }
    n >= 3
}

fn cerca(t: &str) -> Option<(u8, usize)> {
    let c = *t.as_bytes().first()?;
    if c != b'`' && c != b'~' {
        return None;
    }
    let n = t.bytes().take_while(|b| *b == c).count();
    (n >= 3).then_some((c, n))
}

struct Abierto {
    clase: ClaseBloque,
    inicio: usize,
    linea: usize,
    /// Última línea del bloque: `inicio..fin` y comienzo de la siguiente.
    ultima: (usize, usize, usize),
}

/// Los bloques enlazables de una nota `.md`, en orden.
pub fn escanear(texto: &str) -> Vec<BloqueFuente> {
    let desde = fin_frontmatter(texto);
    let mut out: Vec<BloqueFuente> = Vec::new();
    let mut abierto: Option<Abierto> = None;
    // Cerca abierta: (carácter, largo, inicio del bloque, línea).
    let mut en_cerca: Option<(u8, usize, usize, usize)> = None;
    let mut en_math: Option<(usize, usize)> = None;
    // El último bloque cerrado acepta una línea `^id` justo debajo (tabla, código, `$$`).
    let mut acepta_id: Option<usize> = None;

    let cerrar = |abierto: &mut Option<Abierto>, out: &mut Vec<BloqueFuente>, texto: &str| {
        let Some(a) = abierto.take() else { return None };
        let (li, lf, sig) = a.ultima;
        let linea_propia = a.clase.linea_propia();
        let id = if linea_propia {
            None
        } else {
            id_al_final(&texto[li..lf], li)
        };
        let fin_texto = match &id {
            // El bloque termina antes de ` ^id`.
            Some(r) => texto[li..r.start - 1].trim_end().len() + li,
            None => lf,
        };
        out.push(BloqueFuente {
            clase: a.clase,
            rango: a.inicio..fin_texto,
            id,
            insertar_en: if linea_propia { sig } else { lf },
            linea: a.linea,
        });
        linea_propia.then(|| out.len() - 1)
    };

    for l in lineas(texto) {
        if l.inicio < desde {
            continue;
        }
        let t = l.texto.trim_start();

        // Dentro de un bloque de código: solo se busca la cerca de cierre.
        if let Some((c, n, inicio, linea)) = en_cerca {
            if cerca(t).is_some_and(|(c2, n2)| c2 == c && n2 >= n)
                && t.trim().bytes().all(|b| b == c)
            {
                en_cerca = None;
                out.push(BloqueFuente {
                    clase: ClaseBloque::Codigo,
                    rango: inicio..l.fin,
                    id: None,
                    insertar_en: l.siguiente,
                    linea,
                });
                acepta_id = Some(out.len() - 1);
            }
            continue;
        }
        if let Some((inicio, linea)) = en_math {
            if t.trim_end() == "$$" {
                en_math = None;
                out.push(BloqueFuente {
                    clase: ClaseBloque::Matematica,
                    rango: inicio..l.fin,
                    id: None,
                    insertar_en: l.siguiente,
                    linea,
                });
                acepta_id = Some(out.len() - 1);
            }
            continue;
        }

        // Una línea `^id` justo debajo de una tabla, un código o un `$$`.
        if let Some(r) = linea_de_id(&l) {
            if let Some(n) = acepta_id.take().or_else(|| {
                // Tabla todavía abierta.
                if abierto
                    .as_ref()
                    .is_some_and(|a| a.clase == ClaseBloque::Tabla)
                {
                    cerrar(&mut abierto, &mut out, texto)
                } else {
                    None
                }
            }) {
                out[n].id = Some(r);
                out[n].insertar_en = l.siguiente;
                continue;
            }
        }
        acepta_id = None;

        if t.trim().is_empty() {
            acepta_id = cerrar(&mut abierto, &mut out, texto);
            continue;
        }
        if let Some((c, n)) = cerca(t) {
            cerrar(&mut abierto, &mut out, texto);
            en_cerca = Some((c, n, l.inicio, l.numero));
            continue;
        }
        if t.trim_end() == "$$" {
            cerrar(&mut abierto, &mut out, texto);
            en_math = Some((l.inicio, l.numero));
            continue;
        }
        if es_encabezado(t) || es_regla(t) || t.starts_with("[^") || t.starts_with('<') {
            cerrar(&mut abierto, &mut out, texto);
            continue;
        }
        let clase = if t.starts_with('|') {
            ClaseBloque::Tabla
        } else if t.starts_with('>') {
            let mismo = abierto
                .as_ref()
                .is_some_and(|a| matches!(a.clase, ClaseBloque::Cita | ClaseBloque::Callout));
            if mismo {
                let a = abierto.as_mut().expect("abierto");
                a.ultima = (l.inicio, l.fin, l.siguiente);
                continue;
            }
            if t.trim_start_matches('>').trim_start().starts_with("[!") {
                ClaseBloque::Callout
            } else {
                ClaseBloque::Cita
            }
        } else if es_item(l.texto) {
            ClaseBloque::Item
        } else {
            ClaseBloque::Parrafo
        };
        let continua = match (&abierto, clase) {
            (Some(a), ClaseBloque::Tabla) => a.clase == ClaseBloque::Tabla,
            // Texto corrido sigue al párrafo o al ítem (continuación).
            (Some(a), ClaseBloque::Parrafo) => {
                matches!(a.clase, ClaseBloque::Parrafo | ClaseBloque::Item)
            }
            _ => false,
        };
        if continua {
            let a = abierto.as_mut().expect("abierto");
            a.ultima = (l.inicio, l.fin, l.siguiente);
            continue;
        }
        let cerrado = cerrar(&mut abierto, &mut out, texto);
        let _ = cerrado;
        abierto = Some(Abierto {
            clase,
            inicio: l.inicio + (l.texto.len() - t.len()),
            linea: l.numero,
            ultima: (l.inicio, l.fin, l.siguiente),
        });
    }
    cerrar(&mut abierto, &mut out, texto);
    out
}

/// Generador de ids: una semilla al azar por llamada (`RandomState` de la biblioteca estándar) y
/// splitmix64 para cada id. Los ids usados se guardan como número (base 36 de 6 cifras cabe en un
/// `u64`) para no asignar memoria por id.
struct Generador {
    estado: u64,
    usados: HashSet<u64, std::hash::BuildHasherDefault<Identidad>>,
}

/// Los números de id ya son aleatorios: no hace falta mezclarlos otra vez al guardarlos.
#[derive(Default)]
struct Identidad(u64);

impl Hasher for Identidad {
    fn finish(&self) -> u64 {
        self.0
    }
    fn write(&mut self, bytes: &[u8]) {
        for b in bytes {
            self.0 = self.0.rotate_left(8) ^ u64::from(*b);
        }
    }
    fn write_u64(&mut self, n: u64) {
        self.0 = n;
    }
}

const ALFABETO: &[u8] = b"abcdefghijklmnopqrstuvwxyz0123456789";

/// El número de un id de 6 caracteres `a-z0-9` (o `None` si no tiene esa forma).
fn numero_de(id: &str) -> Option<u64> {
    if id.len() != 6 {
        return None;
    }
    id.bytes().try_fold(0u64, |acc, b| {
        let d = ALFABETO.iter().position(|c| *c == b)? as u64;
        Some(acc * 36 + d)
    })
}

impl Generador {
    fn new<'a>(existentes: impl Iterator<Item = &'a str>) -> Self {
        let mut h = std::hash::RandomState::new().build_hasher();
        h.write_u64(0x6f72_6269_7461_6c00);
        Self {
            estado: h.finish(),
            usados: existentes.filter_map(numero_de).collect(),
        }
    }

    /// Un id nuevo, escrito al final de `destino`.
    fn escribir(&mut self, destino: &mut TextoCorto) {
        loop {
            self.estado = self.estado.wrapping_add(0x9e37_79b9_7f4a_7c15);
            let mut z = self.estado;
            z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
            z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
            z ^= z >> 31;
            let n = z % 2_176_782_336; // 36^6
            if self.usados.insert(n) {
                let mut v = n;
                let mut id = [0u8; 6];
                for c in id.iter_mut().rev() {
                    *c = ALFABETO[(v % 36) as usize];
                    v /= 36;
                }
                destino.push_str(std::str::from_utf8(&id).expect("ascii"));
                return;
            }
        }
    }

    fn nuevo(&mut self) -> TextoCorto {
        let mut s = TextoCorto::new();
        self.escribir(&mut s);
        s
    }
}

fn se_superpone(r: &Range<usize>, excluir: &[Range<usize>]) -> bool {
    excluir.iter().any(|e| e.start <= r.end && r.start <= e.end)
}

/// Las ediciones que dan un id a cada bloque de una nota `.md` que no tiene (y reemplazan los
/// repetidos), salvo los bloques que tocan `excluir` (donde están los cursores). Ordenadas de atrás
/// hacia adelante: aplicarlas en orden no corre las posiciones de las siguientes.
pub fn inserciones(texto: &str, excluir: &[Range<usize>]) -> Vec<Edicion> {
    let bloques = escanear(texto);
    let mut generador = Generador::new(
        bloques
            .iter()
            .filter_map(|b| b.id.as_ref().map(|r| &texto[r.clone()])),
    );
    let mut vistos: HashSet<&str> = HashSet::new();
    let mut out = Vec::with_capacity(bloques.len());
    generador.usados.reserve(bloques.len());
    for b in &bloques {
        let completo = b.rango.start..b.id.as_ref().map_or(b.insertar_en, |r| r.end);
        if se_superpone(&completo, excluir) {
            continue;
        }
        match &b.id {
            Some(r) => {
                if !vistos.insert(&texto[r.clone()]) {
                    out.push(Edicion {
                        rango: r.clone(),
                        texto: generador.nuevo(),
                    });
                }
            }
            None => {
                let mut texto_id = TextoCorto::new();
                if b.clase.linea_propia() {
                    let al_final_sin_salto = b.insertar_en == texto.len() && !texto.ends_with('\n');
                    texto_id.push_str(if al_final_sin_salto { "\n^" } else { "^" });
                    generador.escribir(&mut texto_id);
                    if !al_final_sin_salto {
                        texto_id.push_str("\n");
                    }
                } else {
                    texto_id.push_str(" ^");
                    generador.escribir(&mut texto_id);
                }
                out.push(Edicion {
                    rango: b.insertar_en..b.insertar_en,
                    texto: texto_id,
                });
            }
        }
    }
    // Los bloques salen en orden: de atrás hacia adelante es darlos vuelta.
    out.reverse();
    out
}

/// Aplica ediciones (ordenadas de atrás hacia adelante) a un texto.
pub fn aplicar(texto: &str, ediciones: &[Edicion]) -> String {
    let mut s = texto.to_owned();
    for e in ediciones {
        s.replace_range(e.rango.clone(), e.texto.as_str());
    }
    s
}

const ETIQUETAS_HTML: &[&str] = &["p", "li", "blockquote", "table", "pre", "figure"];

/// Ediciones que dan un atributo `id` a cada `p`, `li`, `blockquote`, `table`, `pre` y `figure` de
/// una nota `.html` que no tiene (y reemplazan los repetidos). Fuera de comentarios, `<script>` y
/// `<style>`. Las etiquetas que tocan `excluir` no se modifican.
pub fn inserciones_html(texto: &str, excluir: &[Range<usize>]) -> Vec<Edicion> {
    let bytes = texto.as_bytes();
    let minus = texto.to_ascii_lowercase();
    let mut etiquetas: Vec<(Range<usize>, Option<Range<usize>>, usize)> = Vec::new();
    let mut i = 0;
    while let Some(p) = minus[i..].find('<') {
        let a = i + p;
        let resto = &minus[a..];
        if resto.starts_with("<!--") {
            i = minus[a..].find("-->").map_or(texto.len(), |n| a + n + 3);
            continue;
        }
        let saltar_hasta = |cierre: &str| minus[a..].find(cierre).map_or(texto.len(), |n| a + n);
        if resto.starts_with("<script") {
            i = saltar_hasta("</script");
            continue;
        }
        if resto.starts_with("<style") {
            i = saltar_hasta("</style");
            continue;
        }
        // Fin de la etiqueta, respetando comillas.
        let mut j = a + 1;
        let mut comilla: Option<u8> = None;
        while j < bytes.len() {
            match (comilla, bytes[j]) {
                (None, b'"' | b'\'') => comilla = Some(bytes[j]),
                (Some(q), c) if c == q => comilla = None,
                (None, b'>') => break,
                _ => {}
            }
            j += 1;
        }
        if j >= bytes.len() {
            break;
        }
        let nombre: String = minus[a + 1..j]
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric())
            .collect();
        if ETIQUETAS_HTML.contains(&nombre.as_str()) {
            let id = buscar_atributo_id(&texto[a..j], a);
            let antes_de = if bytes[j - 1] == b'/' { j - 1 } else { j };
            etiquetas.push((a..j + 1, id, antes_de));
        }
        i = j + 1;
    }
    let mut generador = Generador::new(
        etiquetas
            .iter()
            .filter_map(|(_, id, _)| id.as_ref().map(|r| &texto[r.clone()])),
    );
    let mut vistos: HashSet<&str> = HashSet::new();
    let mut out = Vec::new();
    for (rango, id, antes_de) in &etiquetas {
        if se_superpone(rango, excluir) {
            continue;
        }
        match id {
            Some(r) => {
                if !vistos.insert(&texto[r.clone()]) {
                    out.push(Edicion {
                        rango: r.clone(),
                        texto: generador.nuevo(),
                    });
                }
            }
            None => {
                let mut t = TextoCorto::new();
                t.push_str(" id=\"");
                generador.escribir(&mut t);
                t.push_str("\"");
                out.push(Edicion {
                    rango: *antes_de..*antes_de,
                    texto: t,
                });
            }
        }
    }
    out.reverse();
    out
}

/// El valor del atributo `id` de una etiqueta de apertura (`<p class="x" id="abc">`).
fn buscar_atributo_id(etiqueta: &str, base: usize) -> Option<Range<usize>> {
    let b = etiqueta.as_bytes();
    let mut i = 1;
    // Saltar el nombre.
    while i < b.len() && b[i].is_ascii_alphanumeric() {
        i += 1;
    }
    while i < b.len() {
        while i < b.len() && (b[i].is_ascii_whitespace() || b[i] == b'/') {
            i += 1;
        }
        let ini = i;
        while i < b.len() && !b[i].is_ascii_whitespace() && b[i] != b'=' && b[i] != b'/' {
            i += 1;
        }
        let nombre = &etiqueta[ini..i];
        let mut valor: Option<Range<usize>> = None;
        if i < b.len() && b[i] == b'=' {
            i += 1;
            if i < b.len() && (b[i] == b'"' || b[i] == b'\'') {
                let q = b[i];
                let v0 = i + 1;
                i = v0;
                while i < b.len() && b[i] != q {
                    i += 1;
                }
                valor = Some(v0..i);
                i += 1;
            } else {
                let v0 = i;
                while i < b.len() && !b[i].is_ascii_whitespace() {
                    i += 1;
                }
                valor = Some(v0..i);
            }
        }
        if nombre.eq_ignore_ascii_case("id") {
            return valor.map(|r| base + r.start..base + r.end);
        }
        if ini == i {
            i += 1;
        }
    }
    None
}
