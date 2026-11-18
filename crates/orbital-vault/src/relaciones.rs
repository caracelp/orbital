//! Conexiones con tipo y motivo (049).
//!
//! Dentro de la parte Conexiones de una nota `.md` (`<!-- connections ID --> … <!---->`, 041),
//! cada renglón puede declarar el tipo de la relación con la forma de los campos `clave:: valor`
//! y, opcionalmente, un motivo después de un guion:
//!
//! ```markdown
//! <!-- connections AP-001 -->
//! - amplía:: [[Derivadas]] — agrega la regla de la cadena
//! - contradice a:: [[Límites intuitivos]] - no vale para x→0
//! - [[Cálculo]]
//! <!---->
//! ```
//!
//! El tipo puede tener espacios y tildes; se compara sin mayúsculas ni tildes. Cualquier tipo es
//! válido: los de [`TIPOS_BASE`] tienen además un nombre inverso con el que se muestran en la
//! nota destino («ampliada por»). Orbital nunca reescribe estos renglones.

use std::ops::Range;
use std::sync::Arc;

use crate::fold::fold;
use crate::links::{LinkKind, LinkTarget, extract_links};

/// Un tipo de la lista base: su nombre y el de la relación vista desde la otra nota.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TipoBase {
    pub nombre: &'static str,
    pub inversa: &'static str,
}

impl TipoBase {
    /// Simétrica: es su propia inversa («se opone a»).
    pub fn simetrica(&self) -> bool {
        self.nombre == self.inversa
    }
}

/// Los tipos que sugiere el autocompletado, en este orden.
pub const TIPOS_BASE: &[TipoBase] = &[
    TipoBase {
        nombre: "amplía",
        inversa: "ampliada por",
    },
    TipoBase {
        nombre: "contradice a",
        inversa: "contradicha por",
    },
    TipoBase {
        nombre: "es fuente de",
        inversa: "tiene como fuente",
    },
    TipoBase {
        nombre: "ejemplifica",
        inversa: "ejemplificada por",
    },
    TipoBase {
        nombre: "responde a",
        inversa: "respondida por",
    },
    TipoBase {
        nombre: "depende de",
        inversa: "es requisito de",
    },
    TipoBase {
        nombre: "es parte de",
        inversa: "contiene",
    },
    TipoBase {
        nombre: "precede a",
        inversa: "sigue a",
    },
    TipoBase {
        nombre: "se opone a",
        inversa: "se opone a",
    },
    TipoBase {
        nombre: "complementa",
        inversa: "complementa",
    },
];

/// Largo máximo de un tipo, en caracteres: más que eso no es un tipo sino una oración.
const MAX_TIPO: usize = 60;

/// Clave para comparar tipos: sin mayúsculas ni tildes y con los espacios colapsados.
pub fn clave(tipo: &str) -> String {
    let mut out = String::with_capacity(tipo.len());
    for palabra in tipo.split_whitespace() {
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(&fold(palabra));
    }
    out
}

/// El tipo base al que corresponde `tipo` y si se escribió con su nombre inverso.
pub fn tipo_base(tipo: &str) -> Option<(&'static TipoBase, bool)> {
    let k = clave(tipo);
    TIPOS_BASE.iter().find_map(|t| {
        if clave(t.nombre) == k {
            Some((t, false))
        } else if clave(t.inversa) == k {
            Some((t, true))
        } else {
            None
        }
    })
}

/// Nombre con que se muestra el tipo en la nota que lo declara: el de la lista base (con sus
/// tildes) o, para un tipo libre, tal como se escribió con los espacios colapsados.
pub fn nombre_canonico(tipo: &str) -> String {
    match tipo_base(tipo) {
        Some((t, false)) => t.nombre.to_owned(),
        Some((t, true)) => t.inversa.to_owned(),
        None => tipo.split_whitespace().collect::<Vec<_>>().join(" "),
    }
}

/// Nombre de la relación vista desde la nota destino: si A «amplía» B, en B es «ampliada por».
/// Las simétricas son su propia inversa; un tipo escrito con su nombre inverso vuelve al directo;
/// un tipo libre se muestra como «← tipo».
pub fn nombre_inverso(tipo: &str) -> String {
    match tipo_base(tipo) {
        Some((t, false)) => t.inversa.to_owned(),
        Some((t, true)) => t.nombre.to_owned(),
        None => format!("← {}", nombre_canonico(tipo)),
    }
}

/// Lo que una relación con tipo agrega a un enlace: el tipo como se escribió y el motivo.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct RelacionInfo {
    pub tipo: String,
    pub motivo: Option<String>,
}

impl RelacionInfo {
    pub fn clave(&self) -> String {
        clave(&self.tipo)
    }
}

/// Una relación declarada en la parte Conexiones: un enlace con su tipo.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Relacion {
    pub info: Arc<RelacionInfo>,
    pub kind: LinkKind,
    pub destino: LinkTarget,
    /// Línea en el archivo (base 1).
    pub linea: usize,
}

/// Lleva la cuenta de si un renglón está dentro de una parte Conexiones. Se le pasan los
/// renglones en orden; devuelve `true` para los del contenido (no para los delimitadores).
#[derive(Clone, Copy, Debug, Default)]
pub struct Rastreador {
    dentro: bool,
    en_cerca: bool,
}

impl Rastreador {
    pub fn paso(&mut self, linea: &str) -> bool {
        let t = linea.trim();
        if self.dentro {
            if t.starts_with("```") || t.starts_with("~~~") {
                self.en_cerca = !self.en_cerca;
                return true;
            }
            if self.en_cerca {
                return true;
            }
        }
        if let Some(kind) = apertura(t) {
            self.dentro = kind == Delimitador::Conexiones;
            self.en_cerca = false;
            return false;
        }
        self.dentro
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Delimitador {
    Conexiones,
    Otro,
}

/// Si el renglón (ya recortado) abre o cierra una parte: `<!-- connections ID -->` (o
/// `conexiones`), otra parte o bloque (`summary`, `sources`, `block`…) o el cierre `<!---->`.
fn apertura(t: &str) -> Option<Delimitador> {
    if !t.starts_with("<!--") {
        return None;
    }
    if t == "<!---->" {
        return Some(Delimitador::Otro);
    }
    let interior = t.strip_prefix("<!--")?.strip_suffix("-->")?;
    let palabra = interior.split_whitespace().next()?.to_lowercase();
    match palabra.as_str() {
        "connections" | "conexiones" => Some(Delimitador::Conexiones),
        "summary" | "sintesis" | "síntesis" | "sources" | "fuentes" | "bibliography" | "block" => {
            Some(Delimitador::Otro)
        }
        _ => None,
    }
}

/// Atajo barato: el texto puede tener una parte Conexiones.
pub fn tiene_conexiones(texto: &str) -> bool {
    let b = texto.as_bytes();
    memchr::memmem::find(b, b"<!-- connections").is_some()
        || memchr::memmem::find(b, b"<!-- conexiones").is_some()
        || memchr::memmem::find(b, b"<!--connections").is_some()
}

/// Quita el marcador de lista (`- `, `* `, `+ `, `1. `, `1) `) del principio del renglón.
pub fn sin_marcador(linea: &str) -> &str {
    let rest = linea.trim_start();
    if let Some(after) = rest
        .strip_prefix("- ")
        .or_else(|| rest.strip_prefix("* "))
        .or_else(|| rest.strip_prefix("+ "))
    {
        return after.trim_start();
    }
    let n = rest.bytes().take_while(u8::is_ascii_digit).count();
    if n > 0 {
        let after = &rest[n..];
        if let Some(a) = after
            .strip_prefix(". ")
            .or_else(|| after.strip_prefix(") "))
        {
            return a.trim_start();
        }
    }
    rest
}

/// El tipo de un texto que termina en `::` (lo que va antes del primer enlace), si es válido:
/// no vacío, sin corchetes, barras verticales ni comillas invertidas y de hasta 60 caracteres.
pub fn tipo_de_prefijo(antes: &str) -> Option<&str> {
    let tipo = sin_marcador(antes).trim_end().strip_suffix("::")?.trim();
    let valido = !tipo.is_empty()
        && !tipo.contains(['[', ']', '|', '`', '<', '>'])
        && !tipo.contains("::")
        && tipo.chars().count() <= MAX_TIPO;
    valido.then_some(tipo)
}

/// El motivo que sigue al último enlace: lo que queda después de un separador `—`, `–`, `--`,
/// `-` o `:` (el separador es opcional). Vacío: sin motivo.
pub fn motivo_de_sufijo(despues: &str) -> Option<&str> {
    let t = despues.trim_start();
    let t = ["—", "–", "--", "-", ":"]
        .iter()
        .find_map(|s| t.strip_prefix(s))
        .unwrap_or(t)
        .trim();
    (!t.is_empty()).then_some(t)
}

/// Un renglón con tipo: `tipo:: [[A]], [[B]] — motivo`. Los rangos son de los enlaces dentro
/// del renglón.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RenglonTipado<'a> {
    pub tipo: &'a str,
    pub motivo: Option<&'a str>,
    pub enlaces: Vec<(LinkKind, LinkTarget, Range<usize>)>,
}

/// Analiza un renglón de la parte Conexiones. `None` si no declara un tipo o no tiene enlaces
/// (un `[[enlace]]` solo, sin tipo, sigue valiendo como enlace común).
pub fn analizar_renglon(linea: &str) -> Option<RenglonTipado<'_>> {
    // Una relación es un ítem de lista.
    if sin_marcador(linea).len() == linea.trim_start().len() {
        return None;
    }
    let sep = linea.find("::")?;
    // El `::` tiene que estar antes del primer enlace.
    if linea[..sep].contains("[[") || linea[..sep].contains("](") {
        return None;
    }
    let tipo = tipo_de_prefijo(&linea[..sep + 2])?;
    let enlaces: Vec<_> = extract_links(linea, false)
        .into_iter()
        .filter(|o| o.kind != LinkKind::Transclusion && o.byte_range.start > sep)
        .map(|o| (o.kind, o.target, o.byte_range))
        .collect();
    let fin = enlaces.last()?.2.end;
    Some(RenglonTipado {
        tipo,
        motivo: motivo_de_sufijo(&linea[fin..]),
        enlaces,
    })
}

/// Las relaciones con tipo de una nota `.md`, en orden. Un recorrido de renglones, que corta
/// enseguida si la nota no tiene parte Conexiones.
pub fn relaciones(texto: &str) -> Vec<Relacion> {
    let mut out = Vec::new();
    if !tiene_conexiones(texto) {
        return out;
    }
    let mut r = Rastreador::default();
    for (n, linea) in texto.split('\n').enumerate() {
        if !r.paso(linea) || r.en_cerca {
            continue;
        }
        let Some(rt) = analizar_renglon(linea) else {
            continue;
        };
        let info = Arc::new(RelacionInfo {
            tipo: rt.tipo.to_owned(),
            motivo: rt.motivo.map(str::to_owned),
        });
        for (kind, destino, _) in rt.enlaces {
            out.push(Relacion {
                info: info.clone(),
                kind,
                destino,
                linea: n + 1,
            });
        }
    }
    out
}

/// El tipo que se está escribiendo al principio de un renglón de lista (para el
/// autocompletado): lo que va del marcador hasta el cursor, si todavía no hay `::` ni enlaces
/// y tiene al menos un carácter. Devuelve el texto y su posición en bytes dentro de `hasta_cursor`.
pub fn tipo_en_escritura(hasta_cursor: &str) -> Option<(&str, usize)> {
    let resto = hasta_cursor.trim_start();
    let marcado =
        resto.starts_with("- ") || resto.starts_with("* ") || resto.starts_with("+ ") || {
            let n = resto.bytes().take_while(u8::is_ascii_digit).count();
            n > 0 && (resto[n..].starts_with(". ") || resto[n..].starts_with(") "))
        };
    if !marcado {
        return None;
    }
    let tipo = sin_marcador(hasta_cursor);
    let inicio = hasta_cursor.len() - tipo.len();
    let valido = !tipo.trim().is_empty()
        && !tipo.contains(['[', ']', '|', '`', '<', '>', ':', '#'])
        && tipo.chars().count() <= MAX_TIPO;
    valido.then_some((tipo, inicio))
}

/// Filtra tipos para el autocompletado: primero los que empiezan con la consulta (sin tildes ni
/// mayúsculas), después los que la contienen; sin repetir claves.
pub fn filtrar_tipos<'a>(candidatos: &'a [String], consulta: &str, limite: usize) -> Vec<&'a str> {
    let q = clave(consulta);
    let mut empiezan = Vec::new();
    let mut contienen = Vec::new();
    let mut vistos: Vec<String> = Vec::new();
    for c in candidatos {
        let k = clave(c);
        if vistos.contains(&k) {
            continue;
        }
        if k.starts_with(&q) {
            empiezan.push(c.as_str());
        } else if k.contains(&q) {
            contienen.push(c.as_str());
        } else {
            continue;
        }
        vistos.push(k);
    }
    empiezan.into_iter().chain(contienen).take(limite).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOTA: &str = "---\nid: AP-001\n---\n<!-- block 1 AP-001 -->\n- amplía:: [[No cuenta]]\n<!---->\n\n<!-- connections AP-001 -->\n## Conexiones\n\n- amplía:: [[Derivadas]] — agrega la regla de la cadena\n- Contradice A:: [[Límites intuitivos]] - no vale para x→0\n- es fuente de:: [[Integrales]], [[Series|series]]\n- [[Cálculo]]\n- Prerrequisito: [[Base]]\n* refuta:: [Otra](/otra.md) -- por un contraejemplo\n- ![[embebida]]\n- sin enlace:: nada\n<!---->\n- amplía:: [[Afuera]]\n";

    #[test]
    fn extrae_solo_dentro_de_conexiones() {
        let r = relaciones(NOTA);
        let resumen: Vec<(&str, &str, Option<&str>, usize)> = r
            .iter()
            .map(|x| {
                (
                    x.info.tipo.as_str(),
                    x.destino.path.as_str(),
                    x.info.motivo.as_deref(),
                    x.linea,
                )
            })
            .collect();
        assert_eq!(
            resumen,
            [
                (
                    "amplía",
                    "Derivadas",
                    Some("agrega la regla de la cadena"),
                    11
                ),
                (
                    "Contradice A",
                    "Límites intuitivos",
                    Some("no vale para x→0"),
                    12
                ),
                ("es fuente de", "Integrales", None, 13),
                ("es fuente de", "Series", None, 13),
                ("refuta", "/otra.md", Some("por un contraejemplo"), 16),
            ]
        );
        assert_eq!(r[2].info, r[3].info);
    }

    #[test]
    fn tipos_e_inversas() {
        assert_eq!(clave("  Contradice   A "), "contradice a");
        assert_eq!(clave("AMPLÍA"), "amplia");
        assert_eq!(nombre_inverso("amplia"), "ampliada por");
        assert_eq!(nombre_inverso("Contradice a"), "contradicha por");
        assert_eq!(nombre_inverso("se opone a"), "se opone a");
        assert_eq!(nombre_inverso("ampliada por"), "amplía");
        assert_eq!(nombre_inverso("refuta"), "← refuta");
        assert_eq!(nombre_canonico("AMPLIA"), "amplía");
        assert_eq!(nombre_canonico("  mi  tipo "), "mi tipo");
        assert!(TIPOS_BASE.iter().filter(|t| t.simetrica()).count() >= 1);
        // Ninguna clave se repite entre nombres e inversas de tipos distintos.
        let mut claves: Vec<String> = TIPOS_BASE
            .iter()
            .flat_map(|t| {
                let mut v = vec![clave(t.nombre)];
                if !t.simetrica() {
                    v.push(clave(t.inversa));
                }
                v
            })
            .collect();
        let n = claves.len();
        claves.sort();
        claves.dedup();
        assert_eq!(claves.len(), n);
    }

    #[test]
    fn renglones_que_no_son_relaciones() {
        assert!(analizar_renglon("- [[Cálculo]]").is_none());
        assert!(analizar_renglon("- Prerrequisito: [[Base]]").is_none());
        assert!(analizar_renglon("- [[A::b]]").is_none());
        assert!(analizar_renglon("- tipo:: sin enlace").is_none());
        assert!(analizar_renglon("- ver [[A]] y algo:: [[B]]").is_none());
        assert!(
            analizar_renglon("amplía:: [[A]]").is_none(),
            "sin marcador de lista"
        );
        // Un nombre de nota con guiones no se confunde con el motivo.
        let r = analizar_renglon("- amplía:: [[Uno - dos]] - motivo").unwrap();
        assert_eq!(r.enlaces[0].1.path, "Uno - dos");
        assert_eq!(r.motivo, Some("motivo"));
        // Sin separador, el texto que sigue al enlace también es el motivo.
        let r = analizar_renglon("- amplía:: [[A]] porque sí").unwrap();
        assert_eq!(r.motivo, Some("porque sí"));
    }

    #[test]
    fn sin_parte_conexiones_no_hay_nada() {
        assert!(relaciones("- amplía:: [[A]]\n").is_empty());
        assert!(relaciones("<!-- connections X -->\n```\n- amplía:: [[A]]\n```\n").is_empty());
    }

    #[test]
    fn escritura_y_filtro_de_tipos() {
        assert_eq!(tipo_en_escritura("- amp"), Some(("amp", 2)));
        assert_eq!(tipo_en_escritura("  1. contra"), Some(("contra", 5)));
        assert_eq!(tipo_en_escritura("- "), None);
        assert_eq!(tipo_en_escritura("amp"), None);
        assert_eq!(tipo_en_escritura("- [[A"), None);
        assert_eq!(tipo_en_escritura("- amplía::"), None);
        let tipos: Vec<String> = TIPOS_BASE
            .iter()
            .map(|t| t.nombre.to_owned())
            .chain(["refuta".to_owned(), "AMPLIA".to_owned()])
            .collect();
        assert_eq!(filtrar_tipos(&tipos, "AMP", 8), ["amplía"]);
        // Primero los que empiezan con la consulta, después los que la contienen.
        assert_eq!(
            filtrar_tipos(&tipos, "es ", 8),
            ["es fuente de", "es parte de", "responde a"]
        );
        assert_eq!(filtrar_tipos(&tipos, "fuente", 8), ["es fuente de"]);
        assert_eq!(filtrar_tipos(&tipos, "ref", 8), ["refuta"]);
    }
}
