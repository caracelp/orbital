//! Índices por etiqueta (036): el bloque ` ```indice ` lista las notas de una rama de etiquetas,
//! anidadas por sub-etiqueta. Es **virtual**: se arma en la vista con el índice de búsqueda en
//! memoria (las etiquetas de cada nota, 004) y nunca se escribe en la nota.
//!
//! El cuerpo son líneas `clave: valor` (no YAML estricto) o, en la forma corta, solo la etiqueta
//! en el primer renglón (`#proyectos` o `proyectos`). Claves (con su sinónimo):
//!
//! - `etiqueta` (`tag`, `indice`): la etiqueta o rama, con o sin `#`.
//! - `prioridad` (`priority`): la etiqueta que manda arriba y en negrita; vacío, `no` o `ninguna`:
//!   sin prioridad.
//! - `titulo` (`título`, `title`): mostrar la propiedad `title` de cada nota junto al enlace.
//! - `excluir` (`exclude`): carpetas fuera del índice (`[plantillas, borradores]` o separadas por
//!   comas).
//! - `meta`: listar las notas que tienen un bloque `indice` (un índice de índices).
//!
//! Lo que el bloque no escribe toma el valor por defecto de Preferencias ([`IndiceDefaults`]).

use std::collections::{BTreeMap, HashMap};

use crate::RelPath;
use crate::fold::fold;
use crate::search::SearchIndex;

/// Valores por defecto (Preferencias): los toma un bloque que no escribe esa opción.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IndiceDefaults {
    /// Etiqueta de prioridad, sin `#`; vacía: sin prioridad.
    pub prioridad: String,
    pub titulo: bool,
    /// Carpetas fuera del índice.
    pub excluir: Vec<String>,
}

impl Default for IndiceDefaults {
    fn default() -> Self {
        Self {
            prioridad: "top".into(),
            titulo: true,
            excluir: Vec::new(),
        }
    }
}

/// Las opciones de un bloque, ya con los valores por defecto aplicados.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IndiceOpciones {
    /// La etiqueta como se escribió, sin `#` (para mostrarla).
    pub etiqueta: String,
    /// La etiqueta plegada (sin mayúsculas ni tildes), sin `#` ni `/` al final.
    pub ruta: String,
    /// Etiqueta de prioridad plegada; vacía: sin prioridad.
    pub prioridad: String,
    pub titulo: bool,
    pub excluir: Vec<String>,
    pub meta: bool,
}

/// Un error del cuerpo del bloque (se muestra en lugar del índice).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ErrorIndice {
    /// Falta la etiqueta (y no es un meta-índice).
    SinEtiqueta,
    /// Un valor que no se entiende: el nombre de la opción.
    Valor(String),
}

impl std::fmt::Display for ErrorIndice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::SinEtiqueta => write!(
                f,
                "Falta la etiqueta del índice (por ejemplo «etiqueta: proyectos»)"
            ),
            Self::Valor(k) => write!(f, "Valor inválido en «{k}»"),
        }
    }
}

fn limpiar_etiqueta(t: &str) -> &str {
    t.trim()
        .trim_matches(['"', '\''])
        .trim()
        .trim_start_matches('#')
        .trim_end_matches('/')
}

fn booleano(v: &str) -> Option<bool> {
    match fold(v).as_str() {
        "true" | "si" | "yes" => Some(true),
        "false" | "no" => Some(false),
        _ => None,
    }
}

/// Lista `[a, b]` o `a, b`, sin comillas ni barras de los extremos.
fn lista(v: &str) -> Vec<String> {
    let v = v.trim();
    let v = v
        .strip_prefix('[')
        .and_then(|v| v.strip_suffix(']'))
        .unwrap_or(v);
    v.split(',')
        .map(|s| s.trim().trim_matches(['"', '\'']).trim_matches('/').trim())
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .collect()
}

/// Lee el cuerpo del bloque partiendo de `base`. Las claves desconocidas se ignoran.
pub fn parse(cuerpo: &str, base: &IndiceDefaults) -> Result<IndiceOpciones, ErrorIndice> {
    let mut etiqueta = String::new();
    let mut prioridad = base.prioridad.clone();
    let mut titulo = base.titulo;
    let mut excluir = base.excluir.clone();
    let mut meta = false;
    let mut primera = true;
    for linea in cuerpo.lines() {
        let l = linea.trim();
        if l.is_empty() {
            continue;
        }
        let es_primera = std::mem::replace(&mut primera, false);
        let par = l.split_once(':').filter(|(k, _)| {
            !k.is_empty()
                && k.chars()
                    .all(|c| c.is_alphanumeric() || c == '_' || c == ' ')
        });
        let Some((clave, valor)) = par else {
            // Forma corta: la etiqueta sola en el primer renglón. Otro renglón sin `:` (o un
            // comentario `# …` con espacio) se ignora.
            if es_primera && !l.starts_with("# ") && !l.contains(char::is_whitespace) {
                etiqueta = limpiar_etiqueta(l).to_owned();
            }
            continue;
        };
        let valor = valor.trim();
        match fold(clave.trim()).as_str() {
            "etiqueta" | "tag" | "indice" => etiqueta = limpiar_etiqueta(valor).to_owned(),
            "prioridad" | "priority" => {
                let p = limpiar_etiqueta(valor);
                prioridad = if matches!(fold(p).as_str(), "" | "no" | "ninguna" | "false") {
                    String::new()
                } else {
                    p.to_owned()
                };
            }
            "titulo" | "title" if !valor.is_empty() => {
                titulo = booleano(valor).ok_or_else(|| ErrorIndice::Valor("titulo".into()))?;
            }
            "excluir" | "exclude" => excluir = lista(valor),
            "meta" if !valor.is_empty() => {
                meta = booleano(valor).ok_or_else(|| ErrorIndice::Valor("meta".into()))?;
            }
            _ => {}
        }
    }
    if etiqueta.is_empty() && !meta {
        return Err(ErrorIndice::SinEtiqueta);
    }
    Ok(IndiceOpciones {
        ruta: fold(&etiqueta),
        etiqueta,
        prioridad: fold(&prioridad),
        titulo,
        excluir,
        meta,
    })
}

/// El texto de un bloque ` ```indice ` (lo que inserta el comando): la etiqueta y lo que difiere
/// de `base`. Sin salto de línea al final.
pub fn bloque(
    etiqueta: &str,
    prioridad: &str,
    titulo: bool,
    meta: bool,
    base: &IndiceDefaults,
) -> String {
    let mut s = String::from("```indice\n");
    let etiqueta = limpiar_etiqueta(etiqueta);
    if meta {
        s.push_str("meta: true\n");
    } else {
        s.push_str(&format!("etiqueta: {etiqueta}\n"));
    }
    let p = limpiar_etiqueta(prioridad);
    if fold(p) != fold(&base.prioridad) {
        s.push_str(&format!(
            "prioridad: {}\n",
            if p.is_empty() { "no" } else { p }
        ));
    }
    if titulo != base.titulo {
        s.push_str(&format!("titulo: {titulo}\n"));
    }
    s.push_str("```");
    s
}

/// Una nota del índice.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entrada {
    pub path: RelPath,
    /// El nombre del archivo sin extensión.
    pub nombre: String,
    /// La propiedad `title`, si se pidió y difiere del nombre.
    pub titulo: Option<String>,
    /// Tiene la etiqueta de prioridad: va arriba y en negrita.
    pub prioritaria: bool,
    /// En un meta-índice: la etiqueta de su (primer) bloque `indice`.
    pub etiqueta: Option<String>,
}

/// Una etiqueta de la rama: sus notas y sus sub-etiquetas, en orden.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Nodo {
    /// La ruta plegada completa (`proyectos/tesis`).
    pub ruta: String,
    /// El último tramo, con la forma escrita en las notas (`Tesis`).
    pub nombre: String,
    pub notas: Vec<Entrada>,
    pub hijas: Vec<Nodo>,
}

impl Nodo {
    /// Notas en el nodo y sus hijas (una nota con dos sub-etiquetas cuenta dos veces).
    pub fn total(&self) -> usize {
        self.notas.len() + self.hijas.iter().map(Nodo::total).sum::<usize>()
    }
}

/// El índice armado.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Indice {
    pub raiz: Nodo,
    /// Si está vacío: etiquetas existentes parecidas (hasta tres), como pista.
    pub parecidas: Vec<String>,
}

fn nombre_de(path: &RelPath) -> String {
    let n = path.file_name();
    n.rsplit_once('.').map_or(n, |(s, _)| s).to_owned()
}

/// Si la ruta cae dentro de alguna carpeta excluida (en cualquier nivel: `plantillas` saca
/// `wiki/plantillas/…`).
fn excluida(path: &str, excluir: &[String]) -> bool {
    excluir
        .iter()
        .any(|f| path.starts_with(&format!("{f}/")) || path.contains(&format!("/{f}/")))
}

/// La propiedad `title` como se escribió (los valores tipados del índice; `Doc::title` está
/// plegado), si no está vacía ni repite el nombre del archivo.
fn titulo_de(doc: &crate::search::Doc, nombre: &str) -> Option<String> {
    doc.values
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("title"))
        .and_then(|(_, v)| v.as_text())
        .map(str::trim)
        .filter(|t| !t.is_empty() && fold(t) != fold(nombre))
        .map(str::to_owned)
}

fn ordenar(notas: &mut [Entrada]) {
    notas.sort_by_cached_key(|e| (!e.prioritaria, fold(&e.nombre), e.path.as_str().to_owned()));
}

impl SearchIndex {
    /// El índice de una rama de etiquetas (o el meta-índice): una sola pasada por las notas en
    /// memoria, sin leer el disco.
    pub fn indice_etiqueta(&self, o: &IndiceOpciones) -> Indice {
        if o.meta {
            return self.meta_indice(o);
        }
        let raiz = o.ruta.as_str();
        let prefijo = format!("{raiz}/");
        // Notas por sub-ruta relativa ("" = la raíz) y la forma escrita de cada tramo.
        let mut por_ruta: BTreeMap<String, Vec<Entrada>> = BTreeMap::new();
        let mut formas: HashMap<String, HashMap<String, usize>> = HashMap::new();
        for (path, doc) in &self.docs {
            if excluida(path.as_str(), &o.excluir) {
                continue;
            }
            let mut rutas: Vec<&str> = doc
                .tags
                .iter()
                .filter_map(|t| {
                    if t == raiz {
                        Some("")
                    } else {
                        t.strip_prefix(&prefijo)
                    }
                })
                .collect();
            if rutas.is_empty() {
                continue;
            }
            rutas.sort_unstable();
            rutas.dedup();
            let prioritaria = !o.prioridad.is_empty()
                && doc.tags.iter().any(|t| {
                    t == &o.prioridad
                        || t.strip_prefix(&o.prioridad)
                            .is_some_and(|r| r.starts_with('/'))
                });
            let nombre = nombre_de(path);
            let titulo = if o.titulo {
                titulo_de(doc, &nombre)
            } else {
                None
            };
            for r in &rutas {
                por_ruta.entry((*r).to_owned()).or_default().push(Entrada {
                    path: path.clone(),
                    nombre: nombre.clone(),
                    titulo: titulo.clone(),
                    prioritaria,
                    etiqueta: None,
                });
            }
            // La forma escrita de cada tramo, de las etiquetas originales de la nota.
            for crudo in &doc.raw_tags {
                let f = fold(crudo);
                let rel = if f == raiz {
                    ""
                } else if let Some(r) = f.strip_prefix(&prefijo) {
                    r
                } else {
                    continue;
                };
                let tramos_crudos: Vec<&str> = crudo.split('/').collect();
                let salto = raiz.split('/').count();
                let mut acumulada = String::new();
                for (k, tramo) in rel.split('/').filter(|s| !s.is_empty()).enumerate() {
                    if !acumulada.is_empty() {
                        acumulada.push('/');
                    }
                    acumulada.push_str(tramo);
                    if let Some(c) = tramos_crudos.get(salto + k) {
                        *formas
                            .entry(acumulada.clone())
                            .or_default()
                            .entry((*c).to_owned())
                            .or_insert(0) += 1;
                    }
                }
            }
        }
        // El árbol: cada sub-ruta cuelga de su madre (las intermedias sin notas también).
        let mut raiz_nodo = Nodo {
            ruta: raiz.to_owned(),
            nombre: o
                .etiqueta
                .rsplit('/')
                .next()
                .unwrap_or(&o.etiqueta)
                .to_owned(),
            ..Nodo::default()
        };
        for (rel, mut notas) in por_ruta {
            ordenar(&mut notas);
            let mut nodo = &mut raiz_nodo;
            let mut acumulada = String::new();
            for tramo in rel.split('/').filter(|s| !s.is_empty()) {
                if !acumulada.is_empty() {
                    acumulada.push('/');
                }
                acumulada.push_str(tramo);
                let k = match nodo
                    .hijas
                    .iter()
                    .position(|h| h.ruta.ends_with(&format!("/{acumulada}")))
                {
                    Some(k) => k,
                    None => {
                        nodo.hijas.push(Nodo {
                            ruta: format!("{raiz}/{acumulada}"),
                            // La forma más usada (en empate, la primera en orden).
                            nombre: formas
                                .get(&acumulada)
                                .and_then(|f| {
                                    f.iter()
                                        .max_by(|(a, x), (b, y)| x.cmp(y).then_with(|| b.cmp(a)))
                                        .map(|(s, _)| s.clone())
                                })
                                .unwrap_or_else(|| tramo.to_owned()),
                            ..Nodo::default()
                        });
                        nodo.hijas.len() - 1
                    }
                };
                nodo = &mut nodo.hijas[k];
            }
            nodo.notas = notas;
        }
        let parecidas = if raiz_nodo.notas.is_empty() && raiz_nodo.hijas.is_empty() {
            self.parecidas(raiz)
        } else {
            Vec::new()
        };
        Indice {
            raiz: raiz_nodo,
            parecidas,
        }
    }

    /// Las notas con un bloque `indice`, del texto plegado en memoria.
    fn meta_indice(&self, o: &IndiceOpciones) -> Indice {
        let mut notas = Vec::new();
        let base = IndiceDefaults::default();
        for (path, doc) in &self.docs {
            if excluida(path.as_str(), &o.excluir) {
                continue;
            }
            let Some(k) = doc.folded.find("```indice") else {
                continue;
            };
            let resto = &doc.folded[k + "```indice".len()..];
            let resto = resto.split_once('\n').map_or("", |(_, r)| r);
            let cuerpo = resto.split("```").next().unwrap_or("");
            let etiqueta = parse(cuerpo, &base)
                .ok()
                .filter(|p| !p.meta)
                .map(|p| p.etiqueta);
            let nombre = nombre_de(path);
            notas.push(Entrada {
                path: path.clone(),
                titulo: if o.titulo {
                    titulo_de(doc, &nombre)
                } else {
                    None
                },
                nombre,
                prioritaria: false,
                etiqueta,
            });
        }
        ordenar(&mut notas);
        Indice {
            raiz: Nodo {
                notas,
                ..Nodo::default()
            },
            parecidas: Vec::new(),
        }
    }

    /// Hasta tres etiquetas existentes parecidas a `ruta`: la contienen, la ruta las contiene, o
    /// están a dos letras de distancia.
    fn parecidas(&self, ruta: &str) -> Vec<String> {
        if ruta.is_empty() {
            return Vec::new();
        }
        let mut todas: Vec<&str> = self
            .docs
            .values()
            .flat_map(|d| d.tags.iter().map(String::as_str))
            .collect();
        todas.sort_unstable();
        todas.dedup();
        let mut puntuadas: Vec<(usize, &str)> = todas
            .into_iter()
            .filter_map(|t| {
                let d = distancia(t, ruta);
                let ultimo = t.rsplit('/').next().unwrap_or(t);
                if d <= 2 || distancia(ultimo, ruta) <= 1 {
                    Some((d, t))
                } else if ruta.len() >= 3 && (t.contains(ruta) || ruta.contains(t)) {
                    Some((3, t))
                } else {
                    None
                }
            })
            .collect();
        puntuadas.sort();
        puntuadas
            .into_iter()
            .take(3)
            .map(|(_, t)| t.to_owned())
            .collect()
    }
}

/// Distancia de edición (Levenshtein) por caracteres; se corta pasado 3.
fn distancia(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    if a.len().abs_diff(b.len()) > 3 {
        return 4;
    }
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    let mut cur = vec![0; b.len() + 1];
    for (i, ca) in a.iter().enumerate() {
        cur[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            cur[j + 1] = (prev[j] + usize::from(ca != cb))
                .min(prev[j + 1] + 1)
                .min(cur[j] + 1);
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    prev[b.len()]
}

/// HTML estático del índice (exportar, la Visual de las notas `.html`, papel): listas anidadas
/// con enlaces relativos que arma `href` (la ruta de la nota → la dirección).
pub fn html(i: &Indice, o: &IndiceOpciones, href: &dyn Fn(&RelPath) -> String) -> String {
    let esc = crate::bloque_medios::escapar;
    let mut s = String::from("<nav class=\"orbital-indice\">");
    if i.raiz.notas.is_empty() && i.raiz.hijas.is_empty() {
        s.push_str(&format!("<p><em>{}</em></p>", esc(&vacio(i, o))));
        s.push_str("</nav>");
        return s;
    }
    fn nodo(s: &mut String, n: &Nodo, href: &dyn Fn(&RelPath) -> String) {
        let esc = crate::bloque_medios::escapar;
        s.push_str("<ul>");
        for e in &n.notas {
            s.push_str("<li>");
            let enlace = format!("<a href=\"{}\">{}</a>", esc(&href(&e.path)), esc(&e.nombre));
            if e.prioritaria {
                s.push_str(&format!("<strong>{enlace}</strong>"));
            } else {
                s.push_str(&enlace);
            }
            if let Some(t) = &e.titulo {
                s.push_str(&format!(" — <em>{}</em>", esc(t)));
            }
            if let Some(t) = &e.etiqueta {
                s.push_str(&format!(" <code>#{}</code>", esc(t)));
            }
            s.push_str("</li>");
        }
        for h in &n.hijas {
            s.push_str(&format!("<li>#{}", esc(&h.nombre)));
            nodo(s, h, href);
            s.push_str("</li>");
        }
        s.push_str("</ul>");
    }
    nodo(&mut s, &i.raiz, href);
    s.push_str("</nav>");
    s
}

/// El aviso de un índice vacío, con la pista de las parecidas.
pub fn vacio(i: &Indice, o: &IndiceOpciones) -> String {
    if o.meta {
        return "No hay notas con un bloque indice".into();
    }
    let mut s = format!("Sin notas con #{}", o.etiqueta);
    if !i.parecidas.is_empty() {
        let p: Vec<String> = i.parecidas.iter().map(|t| format!("#{t}")).collect();
        s.push_str(&format!(" (¿quisiste decir {}?)", p.join(", ")));
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    fn op(cuerpo: &str) -> IndiceOpciones {
        parse(cuerpo, &IndiceDefaults::default()).unwrap()
    }

    #[test]
    fn lee_la_forma_corta_y_las_claves() {
        assert_eq!(op("#Proyectos/Tesis").ruta, "proyectos/tesis");
        assert_eq!(op("proyectos\n").etiqueta, "proyectos");
        assert_eq!(op("tag: #fisica").ruta, "fisica");
        assert_eq!(op("indice: Física").ruta, "fisica");
        let o = op(
            "etiqueta: proyectos\nprioridad: urgente\ntitulo: false\nexcluir: [plantillas, borradores ñ]",
        );
        assert_eq!(o.prioridad, "urgente");
        assert!(!o.titulo);
        assert_eq!(o.excluir, vec!["plantillas", "borradores ñ"]);
        assert_eq!(op("etiqueta: x\nprioridad: no").prioridad, "");
        assert!(op("meta: true").meta);
        assert_eq!(
            parse("", &IndiceDefaults::default()),
            Err(ErrorIndice::SinEtiqueta)
        );
        assert_eq!(
            parse("etiqueta: x\ntitulo: quizás", &IndiceDefaults::default()),
            Err(ErrorIndice::Valor("titulo".into()))
        );
    }

    #[test]
    fn los_valores_por_defecto_y_el_bloque_mandan() {
        let base = IndiceDefaults {
            prioridad: "importante".into(),
            titulo: false,
            excluir: vec!["archivo".into()],
        };
        let o = parse("etiqueta: a", &base).unwrap();
        assert_eq!(o.prioridad, "importante");
        assert!(!o.titulo);
        assert_eq!(o.excluir, vec!["archivo"]);
        let o = parse("etiqueta: a\ntitulo: true\nexcluir: []", &base).unwrap();
        assert!(o.titulo);
        assert!(o.excluir.is_empty());
    }

    #[test]
    fn bloque_escribe_solo_lo_distinto() {
        let base = IndiceDefaults::default();
        assert_eq!(
            bloque("#proyectos", "top", true, false, &base),
            "```indice\netiqueta: proyectos\n```"
        );
        assert_eq!(
            bloque("proyectos/tesis", "", false, false, &base),
            "```indice\netiqueta: proyectos/tesis\nprioridad: no\ntitulo: false\n```"
        );
        assert_eq!(
            bloque("", "top", true, true, &base),
            "```indice\nmeta: true\n```"
        );
    }

    #[test]
    fn distancia_de_edicion() {
        assert_eq!(distancia("fisica", "fisica"), 0);
        assert_eq!(distancia("fisica", "fsica"), 1);
        assert_eq!(distancia("proyecto", "proyectos"), 1);
        assert!(distancia("a", "abcdefg") > 3);
    }
}
