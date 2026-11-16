//! Outliner plegable (028): qué se puede plegar en una nota `.md`, cómo se reconoce un pliegue y
//! el estado guardado en `.orbital/plegado.yaml`.
//!
//! [`plegables`] hace un solo recorrido de líneas (con los ayudantes del escáner de bloques de la
//! 026) y devuelve los encabezados con contenido y los ítems con subítems, con su anidamiento. El
//! resto son funciones puras sobre ese árbol (claves, rangos de líneas ocultas, la diferencia entre
//! lo oculto y lo que debería estarlo) y el archivo, con el mismo trato que `listas.yaml` (027).
//! Las notas nunca se escriben desde acá.

use crate::bloques::{
    cerca, es_encabezado, es_item, es_regla, fin_frontmatter, id_al_final, lineas,
};
use crate::entry::{Entry, RelPath};
use crate::listas::{Nodo, arbol, check_directories, dentro_de, leer_config, ruta_valida};
use crate::props::emit;
use crate::write::{atomic_write, backup, hash};
use crate::{Vault, VaultError, WriteCtx};
use saphyr_parser::ScalarStyle;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::ops::Range;
use std::path::PathBuf;

/// Ruta del archivo dentro de la bóveda.
pub const PLEGADO_PATH: &str = ".orbital/plegado.yaml";

/// Versión del formato que esta versión de Orbital entiende.
const FORMAT_VERSION: u64 = 1;

/// Clase de elemento plegable.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Clase {
    /// Encabezado ATX de nivel 1 a 6.
    Encabezado { nivel: u8 },
    /// Ítem de lista; la sangría es la de su marcador (un tab cuenta 4).
    Item { sangria: u32 },
}

/// Un elemento plegable de una nota `.md`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Plegable {
    pub clase: Clase,
    /// Línea (desde 0) del elemento: la que queda visible.
    pub linea: u32,
    /// Última línea del contenido que se oculta (≥ `linea + 1`).
    pub ultima: u32,
    /// Índice del plegable que lo contiene.
    pub padre: Option<u32>,
    /// Bytes del texto de la clave: la primera línea sin `#`, marcador, casilla ni ` ^id`.
    pub texto: Range<usize>,
    /// Bytes del `^id` de la primera línea, sin el `^` (solo ítems).
    pub id: Option<Range<usize>>,
}

impl Plegable {
    pub fn es_item(&self) -> bool {
        matches!(self.clase, Clase::Item { .. })
    }

    /// Las líneas que oculta: `linea + 1 ..= ultima`.
    pub fn oculta(&self) -> Range<u32> {
        self.linea + 1..self.ultima + 1
    }
}

/// Sangría de una línea en columnas (un tab cuenta 4).
fn sangria(linea: &str) -> u32 {
    let mut n = 0;
    for b in linea.bytes() {
        match b {
            b' ' => n += 1,
            b'\t' => n += 4,
            _ => break,
        }
    }
    n
}

/// `a..b` sin los espacios de los bordes.
fn recortar(texto: &str, a: usize, b: usize) -> Range<usize> {
    let s = &texto[a..b];
    let inicio = a + (s.len() - s.trim_start().len());
    let fin = inicio + s.trim().len();
    inicio..fin
}

/// Texto y id de la primera línea de un ítem (`linea` empieza en el byte `base`).
fn texto_de_item(texto: &str, linea: &str, base: usize) -> (Range<usize>, Option<Range<usize>>) {
    let ini = linea.len() - linea.trim_start().len();
    let s = &linea[ini..];
    let marcador = if s.starts_with(['-', '*', '+']) {
        2
    } else {
        s.bytes().take_while(u8::is_ascii_digit).count() + 2
    };
    let mut desde = (ini + marcador).min(linea.len());
    let resto = &linea[desde..];
    if resto.starts_with("[ ] ") || resto.starts_with("[x] ") || resto.starts_with("[X] ") {
        desde += 4;
    }
    let id = id_al_final(linea, base);
    let hasta = match &id {
        // Antes del `^`.
        Some(r) => r.start - 1 - base,
        None => linea.len(),
    };
    let desde = desde.min(hasta);
    (recortar(texto, base + desde, base + hasta), id)
}

/// Los elementos plegables de una nota `.md`, en orden de línea, en un recorrido.
///
/// Se saltan el frontmatter, los bloques de código y `$$`, y las líneas de citas y callouts. Un
/// encabezado es plegable si hay al menos una línea no vacía antes del próximo encabezado de nivel
/// igual o menor; un ítem, si tiene al menos un subítem. El bloque de un ítem son las líneas
/// siguientes con más sangría (también párrafos), con líneas vacías intermedias si la próxima no
/// vacía tiene más sangría. Ningún elemento cuenta las líneas vacías del final.
pub fn plegables(texto: &str) -> Vec<Plegable> {
    let desde = fin_frontmatter(texto);
    // Una estimación holgada evita realojar en notas con muchos ítems.
    let mut out: Vec<Plegable> = Vec::with_capacity(texto.len() / 40);
    // Ítem con subítem (por índice de `out`).
    let mut con_sub: Vec<bool> = Vec::with_capacity(texto.len() / 40);
    let mut pila: Vec<u32> = Vec::new();
    let mut ultima_no_vacia: u32 = 0;
    let mut en_cerca: Option<(u8, usize)> = None;
    let mut en_math = false;

    fn cerrar_hasta(
        out: &mut [Plegable],
        pila: &mut Vec<u32>,
        ultima: u32,
        sigue: impl Fn(&Plegable) -> bool,
    ) {
        while let Some(&tope) = pila.last() {
            let p = &mut out[tope as usize];
            if !sigue(p) {
                break;
            }
            p.ultima = ultima;
            pila.pop();
        }
    }

    for l in lineas(texto) {
        if l.inicio < desde {
            continue;
        }
        let n = l.numero as u32;
        let t = l.texto.trim_start();
        let vacia = t.trim_end().is_empty();
        if let Some((c, k)) = en_cerca {
            if cerca(t).is_some_and(|(c2, k2)| c2 == c && k2 >= k)
                && t.trim().bytes().all(|b| b == c)
            {
                en_cerca = None;
            }
            if !vacia {
                ultima_no_vacia = n;
            }
            continue;
        }
        if en_math {
            if t.trim_end() == "$$" {
                en_math = false;
            }
            if !vacia {
                ultima_no_vacia = n;
            }
            continue;
        }
        if vacia {
            continue;
        }
        let ind = sangria(l.texto);
        let anterior = ultima_no_vacia;
        if ind <= 3 && es_encabezado(t) {
            let nivel = t.bytes().take_while(|b| *b == b'#').count() as u8;
            // Un ítem no contiene encabezados; una sección termina en otra de nivel ≤.
            cerrar_hasta(&mut out, &mut pila, anterior, |p| match p.clase {
                Clase::Item { .. } => true,
                Clase::Encabezado { nivel: m } => m >= nivel,
            });
            let base = l.inicio + (l.texto.len() - t.len());
            out.push(Plegable {
                clase: Clase::Encabezado { nivel },
                linea: n,
                ultima: n,
                padre: pila.last().copied(),
                texto: recortar(texto, base + nivel as usize, l.fin),
                id: None,
            });
            con_sub.push(false);
            pila.push(out.len() as u32 - 1);
            ultima_no_vacia = n;
            continue;
        }
        cerrar_hasta(
            &mut out,
            &mut pila,
            anterior,
            |p| matches!(p.clase, Clase::Item { sangria } if sangria >= ind),
        );
        if !t.starts_with('>') && !es_regla(t) && es_item(l.texto) {
            let padre = pila.last().copied();
            if let Some(p) = padre
                && out[p as usize].es_item()
            {
                con_sub[p as usize] = true;
            }
            let (rango, id) = texto_de_item(texto, l.texto, l.inicio);
            out.push(Plegable {
                clase: Clase::Item { sangria: ind },
                linea: n,
                ultima: n,
                padre,
                texto: rango,
                id,
            });
            con_sub.push(false);
            pila.push(out.len() as u32 - 1);
        } else if let Some(c) = cerca(t) {
            en_cerca = Some(c);
        } else if t.trim_end() == "$$" {
            en_math = true;
        }
        ultima_no_vacia = n;
    }
    cerrar_hasta(&mut out, &mut pila, ultima_no_vacia, |_| true);

    // Solo los plegables; los padres se renumeran (un padre siempre es plegable).
    let mut nuevo = vec![u32::MAX; out.len()];
    let mut k = 0u32;
    for (n, p) in out.iter().enumerate() {
        let ok = match p.clase {
            Clase::Encabezado { .. } => p.ultima > p.linea,
            Clase::Item { .. } => con_sub[n],
        };
        if ok {
            nuevo[n] = k;
            k += 1;
        }
    }
    let mut n = 0;
    out.retain_mut(|p| {
        let ok = nuevo[n] != u32::MAX;
        n += 1;
        if ok {
            p.padre = p
                .padre
                .map(|q| nuevo[q as usize])
                .filter(|q| *q != u32::MAX);
        }
        ok
    });
    out
}

/// Cómo se reconoce un pliegue en el archivo.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Clave {
    /// Ítem con `^id` (026) en su primera línea.
    Bloque { id: String },
    /// Ítem sin id, por su texto y su orden entre los ítems plegables con el mismo texto.
    Item { texto: String, aparicion: u32 },
    /// Encabezado, por su texto y su orden entre los encabezados plegables con el mismo texto.
    Encabezado { texto: String, aparicion: u32 },
}

/// Orden (desde 1) del plegable `i` entre los de su misma clase con el mismo texto.
fn aparicion(texto: &str, ps: &[Plegable], i: usize) -> u32 {
    let p = &ps[i];
    let t = &texto[p.texto.clone()];
    1 + ps[..i]
        .iter()
        .filter(|q| q.es_item() == p.es_item() && &texto[q.texto.clone()] == t)
        .count() as u32
}

/// La clave del plegable `i`.
pub fn clave(texto: &str, ps: &[Plegable], i: usize) -> Clave {
    let p = &ps[i];
    if let Some(id) = &p.id {
        return Clave::Bloque {
            id: texto[id.clone()].to_owned(),
        };
    }
    let t = texto[p.texto.clone()].to_owned();
    let aparicion = aparicion(texto, ps, i);
    if p.es_item() {
        Clave::Item {
            texto: t,
            aparicion,
        }
    } else {
        Clave::Encabezado {
            texto: t,
            aparicion,
        }
    }
}

/// Las claves de varios plegables a la vez (una pasada para las apariciones).
pub fn claves(texto: &str, ps: &[Plegable], indices: &[u32]) -> Vec<Clave> {
    let mut vistos: HashMap<(bool, &str), u32> = HashMap::new();
    let mut ap = vec![0u32; ps.len()];
    for (n, p) in ps.iter().enumerate() {
        let c = vistos
            .entry((p.es_item(), &texto[p.texto.clone()]))
            .or_insert(0);
        *c += 1;
        ap[n] = *c;
    }
    indices
        .iter()
        .filter_map(|&i| {
            let p = ps.get(i as usize)?;
            Some(match &p.id {
                Some(id) => Clave::Bloque {
                    id: texto[id.clone()].to_owned(),
                },
                None if p.es_item() => Clave::Item {
                    texto: texto[p.texto.clone()].to_owned(),
                    aparicion: ap[i as usize],
                },
                None => Clave::Encabezado {
                    texto: texto[p.texto.clone()].to_owned(),
                    aparicion: ap[i as usize],
                },
            })
        })
        .collect()
}

/// Los plegables que corresponden a cada clave, en orden de línea. Las que no se encuentran se
/// omiten (nunca se pliega «el más parecido»); dos claves nunca dan el mismo plegable.
pub fn resolver(texto: &str, ps: &[Plegable], claves: &[Clave]) -> Vec<u32> {
    if claves.is_empty() {
        return Vec::new();
    }
    let mut por_id: HashMap<&str, u32> = HashMap::new();
    let mut por_texto: HashMap<(bool, &str, u32), u32> = HashMap::new();
    let mut vistos: HashMap<(bool, &str), u32> = HashMap::new();
    for (n, p) in ps.iter().enumerate() {
        if let Some(id) = &p.id {
            por_id.entry(&texto[id.clone()]).or_insert(n as u32);
        }
        let t = &texto[p.texto.clone()];
        let c = vistos.entry((p.es_item(), t)).or_insert(0);
        *c += 1;
        por_texto.insert((p.es_item(), t, *c), n as u32);
    }
    let mut usados = HashSet::new();
    let mut out: Vec<u32> = claves
        .iter()
        .filter_map(|c| match c {
            Clave::Bloque { id } => por_id.get(id.as_str()).copied(),
            Clave::Item { texto, aparicion } => {
                por_texto.get(&(true, texto.as_str(), *aparicion)).copied()
            }
            Clave::Encabezado { texto, aparicion } => {
                por_texto.get(&(false, texto.as_str(), *aparicion)).copied()
            }
        })
        .filter(|n| usados.insert(*n))
        .collect();
    out.sort_unstable();
    out
}

/// El plegable cuya línea visible es `linea`.
pub fn en_linea(ps: &[Plegable], linea: u32) -> Option<u32> {
    ps.binary_search_by_key(&linea, |p| p.linea)
        .ok()
        .map(|n| n as u32)
}

/// Los plegables cuyo contenido oculto incluye `linea`, de afuera hacia adentro.
pub fn contienen(ps: &[Plegable], linea: u32) -> Vec<u32> {
    // Los rangos están anidados o separados: los que contienen la línea son el último plegable
    // que empieza antes de ella y sus ancestros, desde el primero que la contiene.
    let n = ps.partition_point(|p| p.linea < linea);
    let mut out = Vec::new();
    let mut actual = n.checked_sub(1).map(|n| n as u32);
    while let Some(i) = actual {
        let p = &ps[i as usize];
        if p.ultima >= linea {
            out.push(i);
        }
        actual = p.padre;
    }
    out.reverse();
    out
}

/// Ancestros de `i`, del primer nivel hacia adentro (la miga de pan).
pub fn camino(ps: &[Plegable], i: u32) -> Vec<u32> {
    let mut out = Vec::new();
    let mut actual = ps.get(i as usize).and_then(|p| p.padre);
    while let Some(n) = actual {
        out.push(n);
        actual = ps[n as usize].padre;
    }
    out.reverse();
    out
}

/// Unión ordenada y disjunta de las líneas que ocultan los plegables `plegados`.
pub fn ocultas(ps: &[Plegable], plegados: &[u32]) -> Vec<Range<u32>> {
    let mut rangos: Vec<Range<u32>> = plegados
        .iter()
        .filter_map(|&i| ps.get(i as usize).map(Plegable::oculta))
        .collect();
    unir(&mut rangos);
    rangos
}

/// Ordena y funde rangos que se tocan o se solapan.
pub fn unir(rangos: &mut Vec<Range<u32>>) {
    rangos.retain(|r| !r.is_empty());
    rangos.sort_unstable_by_key(|r| r.start);
    let mut out: Vec<Range<u32>> = Vec::with_capacity(rangos.len());
    for r in rangos.drain(..) {
        match out.last_mut() {
            Some(u) if r.start <= u.end => u.end = u.end.max(r.end),
            _ => out.push(r),
        }
    }
    *rangos = out;
}

/// Lo que oculta el enfoque en `i`: lo de antes de su línea y lo de después de su última.
pub fn fuera_de(ps: &[Plegable], i: u32, total: u32) -> Vec<Range<u32>> {
    let Some(p) = ps.get(i as usize) else {
        return Vec::new();
    };
    [0..p.linea, p.ultima + 1..total]
        .into_iter()
        .filter(|r| !r.is_empty())
        .collect()
}

/// `a − b` para listas ordenadas y disjuntas.
fn restar(a: &[Range<u32>], b: &[Range<u32>]) -> Vec<Range<u32>> {
    let mut out = Vec::new();
    let mut j = 0;
    for r in a {
        let mut inicio = r.start;
        while j < b.len() && b[j].end <= inicio {
            j += 1;
        }
        let mut k = j;
        while k < b.len() && b[k].start < r.end {
            if b[k].start > inicio {
                out.push(inicio..b[k].start);
            }
            inicio = inicio.max(b[k].end);
            k += 1;
        }
        if inicio < r.end {
            out.push(inicio..r.end);
        }
    }
    out
}

/// Tramos a agregar y a quitar para pasar de `actual` a `deseado` (ordenados y disjuntos).
/// Vacíos si son iguales.
pub fn diferencia(
    actual: &[Range<u32>],
    deseado: &[Range<u32>],
) -> (Vec<Range<u32>>, Vec<Range<u32>>) {
    if actual == deseado {
        return (Vec::new(), Vec::new());
    }
    (restar(deseado, actual), restar(actual, deseado))
}

/// Pliegues de cada nota de la bóveda.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Plegado {
    /// Solo notas con algo plegado; claves en orden de línea.
    pub notas: BTreeMap<RelPath, Vec<Clave>>,
}

/// Entradas quitadas por [`Plegado::quitar_ruta`], para reponerlas.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Quitadas(pub Vec<(RelPath, Vec<Clave>)>);

impl Quitadas {
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl Plegado {
    /// Reemplaza las claves de una nota (vacío la quita). Devuelve si cambió algo.
    pub fn poner(&mut self, ruta: &RelPath, claves: Vec<Clave>) -> bool {
        if claves.is_empty() {
            return self.notas.remove(ruta).is_some();
        }
        if self.notas.get(ruta) == Some(&claves) {
            return false;
        }
        self.notas.insert(ruta.clone(), claves);
        true
    }

    pub fn de(&self, ruta: &RelPath) -> &[Clave] {
        self.notas.get(ruta).map_or(&[], Vec::as_slice)
    }

    /// Sigue un renombre: el archivo `desde` exacto o, si es carpeta, todo lo de adentro. Si
    /// `hasta` ya tenía estado, gana el de `desde`.
    pub fn renombrar_ruta(&mut self, desde: &RelPath, hasta: &RelPath) -> bool {
        let (d, h) = (desde.as_str(), hasta.as_str());
        if d == h {
            return false;
        }
        let mover: Vec<RelPath> = self
            .notas
            .keys()
            .filter(|r| dentro_de(r.as_str(), d))
            .cloned()
            .collect();
        let mut cambio = false;
        for r in mover {
            let resto = &r.as_str()[d.len()..];
            let Ok(nueva) = RelPath::new(&format!("{h}{resto}")) else {
                continue;
            };
            if let Some(c) = self.notas.remove(&r) {
                self.notas.insert(nueva, c);
                cambio = true;
            }
        }
        cambio
    }

    /// Quita `ruta` (exacta o, si es carpeta, todo lo de adentro) y devuelve lo quitado.
    pub fn quitar_ruta(&mut self, ruta: &RelPath) -> Quitadas {
        let quitar: Vec<RelPath> = self
            .notas
            .keys()
            .filter(|r| dentro_de(r.as_str(), ruta.as_str()))
            .cloned()
            .collect();
        Quitadas(
            quitar
                .into_iter()
                .filter_map(|r| self.notas.remove(&r).map(|c| (r, c)))
                .collect(),
        )
    }

    /// Deshace [`Plegado::quitar_ruta`]. Lo que ya tiene estado no se pisa.
    pub fn reponer(&mut self, quitadas: &Quitadas) -> bool {
        let mut cambio = false;
        for (r, c) in &quitadas.0 {
            if !self.notas.contains_key(r) {
                self.notas.insert(r.clone(), c.clone());
                cambio = true;
            }
        }
        cambio
    }

    /// Quita las notas que no están en `entries` (ordenadas por ruta, como las deja el recorrido).
    pub fn podar(&mut self, entries: &[Entry]) -> bool {
        let antes = self.notas.len();
        self.notas
            .retain(|r, _| entries.binary_search_by(|e| e.path.cmp(r)).is_ok());
        self.notas.len() != antes
    }
}

/// Texto de un escalar no nulo, tal cual (un encabezado `2024` es texto).
fn escalar(nodo: &Nodo) -> Option<&str> {
    match nodo {
        Nodo::Escalar(v, _, _) if !nodo.es_nulo() => Some(v),
        _ => None,
    }
}

/// Lee el archivo, estricto: cualquier cosa fuera del contrato es un archivo dañado.
///
/// El archivo que escribe Orbital ([`serialize`]) se lee con un recorrido de líneas; cualquier
/// otra forma (editado a mano, comentarios, otro estilo de YAML) pasa por el analizador estricto
/// de eventos. Los dos dan lo mismo para el formato canónico (el de eventos cuesta ≈ 50 µs por KB).
pub fn parse(text: &str) -> Result<Plegado, String> {
    match canonico(text) {
        Some(p) => Ok(p),
        None => parse_estricto(text),
    }
}

/// Un escalar tal como lo escribe [`emit::scalar`]: sin comillas si es seguro, o entre comillas
/// dobles sin escapes. `None` si no es la forma canónica.
fn escalar_canonico(v: &str) -> Option<String> {
    let t = match v.strip_prefix('"') {
        Some(resto) => {
            let t = resto.strip_suffix('"')?;
            if t.contains(['\\', '"']) {
                return None;
            }
            t
        }
        None => v,
    };
    (emit::scalar(t) == v).then(|| t.to_owned())
}

/// El formato exacto de [`serialize`], o `None`.
fn canonico(text: &str) -> Option<Plegado> {
    let mut ls = text.split_inclusive('\n');
    if ls.next()? != "format_version: 1\n" {
        return None;
    }
    let mut p = Plegado::default();
    match ls.next()? {
        "notas: []\n" => return ls.next().is_none().then_some(p),
        "notas:\n" => {}
        _ => return None,
    }
    let mut actual: Option<(RelPath, Vec<Clave>)> = None;
    let cerrar = |p: &mut Plegado, a: Option<(RelPath, Vec<Clave>)>| -> Option<()> {
        if let Some((r, c)) = a
            && (c.is_empty() || p.notas.insert(r, c).is_some())
        {
            return None;
        }
        Some(())
    };
    let mut esperando_plegados = false;
    for l in ls {
        let l = l.strip_suffix('\n')?;
        if let Some(r) = l.strip_prefix("  - ruta: ") {
            if esperando_plegados {
                return None;
            }
            cerrar(&mut p, actual.take())?;
            let r = escalar_canonico(r)?;
            let r = ruta_valida(&r).filter(|r| r.as_str().ends_with(".md"))?;
            actual = Some((r, Vec::new()));
            esperando_plegados = true;
        } else if l == "    plegados:" {
            if !esperando_plegados {
                return None;
            }
            esperando_plegados = false;
        } else if let Some(resto) = l.strip_prefix("      - ") {
            let (_, claves) = actual.as_mut().filter(|_| !esperando_plegados)?;
            let (k, v) = resto.split_once(": ")?;
            let v = escalar_canonico(v)?;
            claves.push(match k {
                "bloque" => {
                    if v.is_empty() || !v.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-') {
                        return None;
                    }
                    Clave::Bloque { id: v }
                }
                "item" => Clave::Item {
                    texto: v,
                    aparicion: 1,
                },
                "encabezado" => Clave::Encabezado {
                    texto: v,
                    aparicion: 1,
                },
                _ => return None,
            });
        } else {
            let n = l.strip_prefix("        aparicion: ")?;
            let (_, claves) = actual.as_mut()?;
            let n: u32 = n.parse().ok().filter(|n| *n > 1)?;
            if n.to_string() != l["        aparicion: ".len()..] {
                return None;
            }
            let (Clave::Item { aparicion, .. } | Clave::Encabezado { aparicion, .. }) =
                claves.last_mut()?
            else {
                return None;
            };
            (*aparicion == 1).then_some(())?;
            *aparicion = n;
        }
    }
    if esperando_plegados {
        return None;
    }
    cerrar(&mut p, actual)?;
    (!p.notas.is_empty()).then_some(p)
}

/// El analizador de eventos (para cualquier YAML).
pub fn parse_estricto(text: &str) -> Result<Plegado, String> {
    let Some(raiz) = arbol(text)? else {
        return Ok(Plegado::default());
    };
    if raiz.es_nulo() {
        return Ok(Plegado::default());
    }
    let Nodo::Mapa(pares, _) = raiz else {
        return Err("El archivo de plegado tiene que ser un mapa YAML".into());
    };
    let mut plegado = Plegado::default();
    let mut vistas = HashSet::new();
    for (k, v) in &pares {
        let Some(nombre) = k.texto() else {
            return Err(format!("Línea {}: clave inválida", k.linea()));
        };
        if !vistas.insert(nombre) {
            return Err(format!("Línea {}: «{nombre}» está repetida", k.linea()));
        }
        match nombre {
            "format_version" => {
                let version = match v {
                    Nodo::Escalar(s, ScalarStyle::Plain, _) => s.parse::<u64>().ok(),
                    _ => None,
                }
                .ok_or_else(|| {
                    format!(
                        "Línea {}: «format_version» tiene que ser un entero",
                        v.linea()
                    )
                })?;
                if version > FORMAT_VERSION {
                    return Err(format!(
                        "el archivo es de una versión más nueva del formato ({version})"
                    ));
                }
            }
            "notas" => plegado.notas = notas(v)?,
            otra => return Err(format!("Línea {}: clave desconocida «{otra}»", k.linea())),
        }
    }
    Ok(plegado)
}

fn notas(nodo: &Nodo) -> Result<BTreeMap<RelPath, Vec<Clave>>, String> {
    let mut out = BTreeMap::new();
    if nodo.es_nulo() {
        return Ok(out);
    }
    let Nodo::Lista(items, _) = nodo else {
        return Err(format!(
            "Línea {}: «notas» tiene que ser una lista",
            nodo.linea()
        ));
    };
    for item in items {
        let Nodo::Mapa(pares, linea) = item else {
            return Err(format!(
                "Línea {}: cada nota es un mapa con «ruta» y «plegados»",
                item.linea()
            ));
        };
        let (mut ruta, mut plegados) = (None, None);
        for (k, v) in pares {
            match k.texto() {
                Some("ruta") if ruta.is_none() => {
                    let r = v
                        .texto()
                        .and_then(ruta_valida)
                        .filter(|r| r.as_str().ends_with(".md"))
                        .ok_or_else(|| {
                            format!("Línea {}: la ruta no es una nota de la bóveda", v.linea())
                        })?;
                    ruta = Some(r);
                }
                Some("plegados") if plegados.is_none() => plegados = Some(pliegues(v)?),
                _ => {
                    return Err(format!("Línea {}: clave inválida en una nota", k.linea()));
                }
            }
        }
        let ruta = ruta.ok_or_else(|| format!("Línea {linea}: una nota no tiene «ruta»"))?;
        let plegados = plegados
            .filter(|p| !p.is_empty())
            .ok_or_else(|| format!("Línea {linea}: «plegados» no puede estar vacío"))?;
        if out.insert(ruta, plegados).is_some() {
            return Err(format!("Línea {linea}: la nota está repetida"));
        }
    }
    Ok(out)
}

fn pliegues(nodo: &Nodo) -> Result<Vec<Clave>, String> {
    let Nodo::Lista(items, _) = nodo else {
        return Err(format!(
            "Línea {}: «plegados» tiene que ser una lista",
            nodo.linea()
        ));
    };
    let mut out = Vec::with_capacity(items.len());
    for item in items {
        let Nodo::Mapa(pares, linea) = item else {
            return Err(format!(
                "Línea {}: cada pliegue es un mapa con una clave",
                item.linea()
            ));
        };
        let mut clase: Option<(&str, &str)> = None;
        let mut aparicion: Option<u32> = None;
        for (k, v) in pares {
            match k.texto() {
                Some(c @ ("bloque" | "item" | "encabezado")) if clase.is_none() => {
                    let t = escalar(v)
                        .ok_or_else(|| format!("Línea {}: «{c}» tiene que ser texto", v.linea()))?;
                    clase = Some((c, t));
                }
                Some("aparicion") if aparicion.is_none() => {
                    let n = match v {
                        Nodo::Escalar(s, ScalarStyle::Plain, _) => s.parse::<u32>().ok(),
                        _ => None,
                    }
                    .filter(|n| *n >= 1)
                    .ok_or_else(|| {
                        format!(
                            "Línea {}: «aparicion» tiene que ser un entero desde 1",
                            v.linea()
                        )
                    })?;
                    aparicion = Some(n);
                }
                _ => {
                    return Err(format!("Línea {}: clave inválida en un pliegue", k.linea()));
                }
            }
        }
        let clave = match clase {
            Some(("bloque", id)) if aparicion.is_none() => {
                if id.is_empty() || !id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-') {
                    return Err(format!("Línea {linea}: «{id}» no es un id de bloque"));
                }
                Clave::Bloque { id: id.to_owned() }
            }
            Some(("item", t)) => Clave::Item {
                texto: t.to_owned(),
                aparicion: aparicion.unwrap_or(1),
            },
            Some(("encabezado", t)) => Clave::Encabezado {
                texto: t.to_owned(),
                aparicion: aparicion.unwrap_or(1),
            },
            _ => {
                return Err(format!(
                    "Línea {linea}: un pliegue no tiene una clave válida"
                ));
            }
        };
        out.push(clave);
    }
    Ok(out)
}

/// Escribe el archivo como en el contrato (`docs/formato-boveda.md`).
pub fn serialize(plegado: &Plegado) -> String {
    let mut out = String::from("format_version: 1\n");
    if plegado.notas.is_empty() {
        out.push_str("notas: []\n");
        return out;
    }
    out.push_str("notas:\n");
    for (ruta, claves) in &plegado.notas {
        out.push_str("  - ruta: ");
        out.push_str(&emit::scalar(ruta.as_str()));
        out.push_str("\n    plegados:\n");
        for c in claves {
            let (k, t, ap) = match c {
                Clave::Bloque { id } => ("bloque", id.as_str(), 1),
                Clave::Item { texto, aparicion } => ("item", texto.as_str(), *aparicion),
                Clave::Encabezado { texto, aparicion } => {
                    ("encabezado", texto.as_str(), *aparicion)
                }
            };
            out.push_str("      - ");
            out.push_str(k);
            out.push_str(": ");
            out.push_str(&emit::scalar(t));
            out.push('\n');
            if ap > 1 {
                out.push_str("        aparicion: ");
                out.push_str(&ap.to_string());
                out.push('\n');
            }
        }
    }
    out
}

/// Lo que se escribe tiene que poder leerse igual.
fn validate(plegado: &Plegado) -> Result<(), VaultError> {
    let ok = plegado.notas.iter().all(|(r, c)| {
        !c.is_empty()
            && ruta_valida(r.as_str()).is_some()
            && r.as_str().ends_with(".md")
            && c.iter().all(|c| match c {
                Clave::Bloque { id } => {
                    !id.is_empty() && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
                }
                Clave::Item { aparicion, .. } | Clave::Encabezado { aparicion, .. } => {
                    *aparicion >= 1
                }
            })
    });
    if ok {
        Ok(())
    } else {
        Err(invalid(
            "El estado de plegado tiene una ruta o una clave inválida",
        ))
    }
}

fn invalid(message: impl Into<String>) -> VaultError {
    VaultError::InvalidProps {
        path: RelPath::from_static(PLEGADO_PATH),
        line: 1,
        message: message.into(),
    }
}

/// Lo leído del disco: el hash pertenece al archivo que se leyó, no a una relectura al guardar.
#[derive(Clone, Debug)]
pub struct PlegadoSnapshot {
    /// Vacío si el archivo falta o está dañado.
    pub plegado: Plegado,
    /// XXH3 de los bytes leídos; `None` si el archivo no existía.
    pub hash: Option<u64>,
    /// Aviso si el archivo estaba dañado o no se pudo leer.
    pub warning: Option<String>,
    root: Option<PathBuf>,
    writable: bool,
    /// El contenido en disco lo escribió esta misma sesión: la próxima escritura no lo respalda.
    propio: bool,
}

impl PlegadoSnapshot {
    pub fn writable(&self) -> bool {
        self.writable
    }

    pub fn propio(&self) -> bool {
        self.propio
    }
}

impl Vault {
    /// Lee `.orbital/plegado.yaml`. Ausente = vacío sin aviso; dañado = vacío con aviso y
    /// escribible (el primer cambio respalda el dañado); ilegible o de una versión más nueva =
    /// vacío, con aviso y sin escribir. Nunca crea nada.
    pub fn plegado_snapshot(&self) -> PlegadoSnapshot {
        let root = self.root().canonicalize().ok();
        let mut snapshot = PlegadoSnapshot {
            plegado: Plegado::default(),
            hash: None,
            warning: None,
            writable: false,
            propio: false,
            root,
        };
        let bytes = match leer_config(self.root(), PLEGADO_PATH) {
            Ok(bytes) => bytes,
            Err(error) => {
                snapshot.warning = Some(format!(
                    "No se cargó lo plegado de las notas: {error}. El archivo no se sobreescribirá."
                ));
                return snapshot;
            }
        };
        snapshot.writable = snapshot.root.is_some();
        let Some(bytes) = bytes else {
            return snapshot;
        };
        snapshot.hash = Some(hash(&bytes));
        let leido = std::str::from_utf8(&bytes)
            .map_err(|_| "el archivo no está en UTF-8".to_owned())
            .and_then(parse);
        match leido {
            Ok(p) => snapshot.plegado = p,
            Err(error) => {
                let nuevo = error.contains("versión más nueva");
                snapshot.writable &= !nuevo;
                snapshot.warning = Some(if nuevo {
                    format!(
                        "No se cargó lo plegado de las notas: {error}. El archivo no se sobreescribirá."
                    )
                } else {
                    format!(
                        "El archivo de plegado tiene errores ({error}). Las notas se abren \
                         desplegadas; al primer cambio se guarda una copia antes de reemplazarlo."
                    )
                });
            }
        }
        snapshot
    }

    /// Escribe el estado si cambió. `Conflict` si el archivo en disco ya no es el del snapshot.
    /// Respalda el contenido anterior solo si no lo escribió esta sesión. Crea `.orbital/`
    /// recién acá.
    pub fn guardar_plegado(
        &self,
        plegado: &Plegado,
        baseline: &PlegadoSnapshot,
        ctx: &WriteCtx,
    ) -> Result<PlegadoSnapshot, VaultError> {
        let root = self.root().canonicalize()?;
        if !baseline.writable || baseline.root.as_deref() != Some(&root) {
            return Err(invalid(
                "El plegado no tiene un estado de lectura válido para esta bóveda",
            ));
        }
        validate(plegado)?;
        let before = leer_config(&root, PLEGADO_PATH)?;
        if before.as_deref().map(hash) != baseline.hash {
            return Err(VaultError::Conflict {
                path: RelPath::from_static(PLEGADO_PATH),
            });
        }
        let output = serialize(plegado);
        if output.len() as u64 > crate::entry::MAX_EDITABLE_BYTES {
            return Err(VaultError::TooLarge(RelPath::from_static(PLEGADO_PATH)));
        }
        let mut propio = baseline.propio;
        // Sin archivo y sin nada plegado no hace falta crear nada.
        let nada = before.is_none() && plegado.notas.is_empty();
        if !nada && before.as_deref() != Some(output.as_bytes()) {
            check_directories(&root, &[".orbital"])?;
            fs::create_dir_all(root.join(".orbital"))?;
            if let Some(bytes) = before.filter(|_| !baseline.propio) {
                let date = ctx.date.to_string();
                check_directories(&root, &[".orbital", "respaldo", &date, ".orbital"])?;
                backup(
                    &root,
                    &RelPath::from_static(PLEGADO_PATH),
                    &bytes,
                    hash(output.as_bytes()),
                    ctx,
                )?;
            }
            atomic_write(&root.join(PLEGADO_PATH), output.as_bytes())?;
            propio = true;
        }
        Ok(PlegadoSnapshot {
            plegado: plegado.clone(),
            hash: if nada {
                None
            } else {
                Some(hash(output.as_bytes()))
            },
            warning: None,
            root: Some(root),
            writable: true,
            propio,
        })
    }
}
