//! Consultas vivas (048): el cuerpo de un codeblock `consulta` y su resultado sobre el índice de
//! búsqueda. La consulta usa el lenguaje de búsqueda de la 004 ([`crate::query`]); las opciones
//! van en líneas aparte (`vista:`, `columnas:`, `orden:`).

use std::cmp::Ordering;
use std::path::Path;
use std::time::SystemTime;

use crate::entry::{RelPath, Zone};
use crate::fold::fold;
use crate::props::PropValue;
use crate::query::{Query, parse_query};
use crate::search::{Doc, Match, SearchIndex};

/// Cuántas líneas por nota muestra la vista de coincidencias (las mismas que el panel de búsqueda).
pub const COINCIDENCIAS_POR_NOTA: usize = 20;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Vista {
    #[default]
    Lista,
    Tabla,
    Coincidencias,
    Conteo,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Criterio {
    #[default]
    Nombre,
    Modificado,
    Creado,
    Propiedad(String),
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Orden {
    pub criterio: Criterio,
    pub descendente: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Consulta {
    pub query: Query,
    pub vista: Vista,
    /// Propiedades de la vista de tabla, como se escribieron.
    pub columnas: Vec<String>,
    pub orden: Orden,
}

/// Un error del cuerpo del bloque, con su posición (línea y columna desde 1, en caracteres).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ErrorConsulta {
    pub linea: usize,
    pub columna: usize,
    pub mensaje: String,
}

impl std::fmt::Display for ErrorConsulta {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "línea {}, columna {}: {}",
            self.linea, self.columna, self.mensaje
        )
    }
}

impl std::error::Error for ErrorConsulta {}

const OPCIONES: [&str; 3] = ["vista", "columnas", "orden"];

/// Lee el cuerpo de un codeblock `consulta`. Las líneas que empiezan con `vista:`, `columnas:` u
/// `orden:` son opciones (el lenguaje de búsqueda no tiene esos operadores, así que no hay
/// ambigüedad); el resto, juntas, son la consulta.
pub fn parse(cuerpo: &str) -> Result<Consulta, ErrorConsulta> {
    // La consulta conserva el cuerpo con las opciones en blanco: las posiciones de `QueryError`
    // caen en el mismo lugar del bloque.
    let mut texto = String::with_capacity(cuerpo.len());
    let mut vista = None;
    let mut columnas = None;
    let mut orden = None;
    let mut inicio = 0;
    for linea in cuerpo.split_inclusive('\n') {
        let contenido = linea.trim_end_matches(['\n', '\r']);
        let sangria = contenido.len() - contenido.trim_start().len();
        let opcion = contenido.trim_start().split_once(':').and_then(|(k, v)| {
            let k = fold(k.trim_end());
            OPCIONES.contains(&k.as_str()).then_some((k, v))
        });
        match opcion {
            None => texto.push_str(linea),
            Some((clave, valor)) => {
                let col_valor = inicio + sangria + contenido.trim_start().len() - valor.len();
                let error = |mensaje: String| error_en(cuerpo, col_valor, mensaje);
                let repetida = match clave.as_str() {
                    "vista" => vista.replace(leer_vista(valor).map_err(error)?).is_some(),
                    "columnas" => columnas
                        .replace(leer_columnas(valor).map_err(error)?)
                        .is_some(),
                    _ => orden.replace(leer_orden(valor).map_err(error)?).is_some(),
                };
                if repetida {
                    return Err(error_en(
                        cuerpo,
                        inicio + sangria,
                        format!("La opción «{clave}» está repetida"),
                    ));
                }
                for c in linea.chars() {
                    match c {
                        '\n' | '\r' => texto.push(c),
                        _ => (0..c.len_utf8()).for_each(|_| texto.push(' ')),
                    }
                }
            }
        }
        inicio += linea.len();
    }
    let query = parse_query(&texto).map_err(|e| {
        let mensaje = if e.message == "Consulta vacía" {
            "Falta qué buscar (por ejemplo, tag:#proyecto)".to_owned()
        } else {
            e.message
        };
        error_en(cuerpo, e.at, mensaje)
    })?;
    Ok(Consulta {
        query,
        vista: vista.unwrap_or_default(),
        columnas: columnas.unwrap_or_default(),
        orden: orden.unwrap_or_default(),
    })
}

fn error_en(cuerpo: &str, at: usize, mensaje: String) -> ErrorConsulta {
    let mut at = at.min(cuerpo.len());
    while !cuerpo.is_char_boundary(at) {
        at -= 1;
    }
    let antes = &cuerpo[..at];
    let inicio_linea = antes.rfind('\n').map_or(0, |n| n + 1);
    ErrorConsulta {
        linea: antes.matches('\n').count() + 1,
        columna: antes[inicio_linea..].chars().count() + 1,
        mensaje,
    }
}

fn leer_vista(valor: &str) -> Result<Vista, String> {
    match fold(valor.trim()).as_str() {
        "lista" => Ok(Vista::Lista),
        "tabla" => Ok(Vista::Tabla),
        "coincidencias" => Ok(Vista::Coincidencias),
        "conteo" => Ok(Vista::Conteo),
        "" => Err("Falta la vista: lista, tabla, coincidencias o conteo".into()),
        otra => Err(format!(
            "Vista desconocida «{otra}»: usá lista, tabla, coincidencias o conteo"
        )),
    }
}

fn leer_columnas(valor: &str) -> Result<Vec<String>, String> {
    let columnas: Vec<String> = valor
        .split(',')
        .map(str::trim)
        .filter(|c| !c.is_empty())
        .map(str::to_owned)
        .collect();
    if columnas.is_empty() {
        return Err("Faltan las columnas (propiedades separadas por comas)".into());
    }
    Ok(columnas)
}

fn leer_orden(valor: &str) -> Result<Orden, String> {
    let valor = valor.trim();
    let (resto, descendente) = match valor.rsplit_once(char::is_whitespace) {
        Some((resto, dir)) => match fold(dir).as_str() {
            "asc" | "ascendente" => (resto.trim_end(), false),
            "desc" | "descendente" => (resto.trim_end(), true),
            _ => (valor, false),
        },
        None => (valor, false),
    };
    if resto.is_empty() || matches!(fold(resto).as_str(), "asc" | "desc") {
        return Err("Falta por qué ordenar: nombre, modificado, creado o una propiedad".into());
    }
    // `[clave]` fuerza una propiedad aunque se llame como un criterio (`[nombre]`).
    let criterio = match resto.strip_prefix('[').and_then(|r| r.strip_suffix(']')) {
        Some(clave) if !clave.trim().is_empty() => Criterio::Propiedad(clave.trim().to_owned()),
        Some(_) => return Err("Falta el nombre de la propiedad entre corchetes".into()),
        None => match fold(resto).as_str() {
            "nombre" => Criterio::Nombre,
            "modificado" | "modificacion" => Criterio::Modificado,
            "creado" | "creacion" => Criterio::Creado,
            _ => Criterio::Propiedad(resto.to_owned()),
        },
    };
    Ok(Orden {
        criterio,
        descendente,
    })
}

/// Una nota del resultado.
#[derive(Clone, Debug, PartialEq)]
pub struct Fila {
    pub path: RelPath,
    /// Un valor por columna (vista de tabla), `None` si la nota no tiene esa propiedad.
    pub valores: Vec<Option<PropValue>>,
    /// Líneas que coinciden (vista de coincidencias) y cuántas más quedaron sin mostrar.
    pub coincidencias: Vec<Match>,
    pub mas: usize,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Resultado {
    /// Vacío en la vista de conteo.
    pub filas: Vec<Fila>,
    pub total: usize,
}

/// El valor de una propiedad (clave sin distinguir mayúsculas ni tildes; `type` lee también
/// `tipo`, como en Bases).
fn valor<'d>(doc: &'d Doc, clave: &str) -> Option<&'d PropValue> {
    let buscar = |clave: &str| {
        doc.values
            .iter()
            .find(|(k, _)| fold(k) == clave)
            .map(|(_, v)| v)
    };
    let clave = fold(clave);
    match buscar(&clave) {
        None | Some(PropValue::Null) if clave == crate::props::TYPE_KEY => {
            buscar(crate::props::TYPE_ALIAS).or(buscar(&clave))
        }
        hallado => hallado,
    }
}

fn es_nota(path: &RelPath) -> bool {
    crate::Format::from_name(path.file_name()).is_some()
}

/// Corre la consulta sobre el índice. `raiz` (la carpeta de la bóveda) solo hace falta para la
/// vista de coincidencias, que lee las líneas de cada nota.
pub fn ejecutar(index: &SearchIndex, raiz: Option<&Path>, consulta: &Consulta) -> Resultado {
    let mut notas: Vec<(&RelPath, &Doc)> = index
        .docs
        .iter()
        .filter(|(path, doc)| {
            doc.zone != Zone::Raw
                && es_nota(path)
                && crate::search::matches_query(doc, path, &consulta.query)
        })
        .collect();
    let total = notas.len();
    if consulta.vista == Vista::Conteo {
        return Resultado {
            filas: Vec::new(),
            total,
        };
    }
    ordenar(&mut notas, &consulta.orden);
    let terminos = if consulta.vista == Vista::Coincidencias {
        crate::search::extract_positive_terms(&consulta.query)
    } else {
        Vec::new()
    };
    let filas = notas
        .into_iter()
        .map(|(path, doc)| {
            let valores = if consulta.vista == Vista::Tabla {
                consulta
                    .columnas
                    .iter()
                    .map(|c| valor(doc, c).cloned())
                    .collect()
            } else {
                Vec::new()
            };
            let (coincidencias, mas) = match raiz {
                Some(raiz) if !terminos.is_empty() => {
                    let texto = std::fs::read_to_string(path.to_path(raiz)).unwrap_or_default();
                    let mut todas = crate::search::find_matches_in_file(doc, &texto, &terminos);
                    let mas = todas.len().saturating_sub(COINCIDENCIAS_POR_NOTA);
                    todas.truncate(COINCIDENCIAS_POR_NOTA);
                    (todas, mas)
                }
                _ => (Vec::new(), 0),
            };
            Fila {
                path: path.clone(),
                valores,
                coincidencias,
                mas,
            }
        })
        .collect();
    Resultado { filas, total }
}

fn ordenar(notas: &mut Vec<(&RelPath, &Doc)>, orden: &Orden) {
    // La clave de cada nota se calcula una sola vez, no en cada comparación.
    enum Clave<'d> {
        Texto(String),
        Fecha(Option<SystemTime>),
        Valor(Option<&'d PropValue>),
    }
    let mut con_clave: Vec<(Clave, &RelPath, &Doc)> = notas
        .drain(..)
        .map(|(path, doc)| {
            let clave = match &orden.criterio {
                Criterio::Nombre => Clave::Texto(fold(path.file_name())),
                Criterio::Modificado => Clave::Fecha(doc.modified),
                Criterio::Creado => Clave::Fecha(doc.created),
                Criterio::Propiedad(k) => Clave::Valor(valor(doc, k)),
            };
            (clave, path, doc)
        })
        .collect();
    con_clave.sort_by(|(a, pa, _), (b, pb, _)| {
        let o = match (a, b) {
            (Clave::Texto(a), Clave::Texto(b)) => a.cmp(b),
            (Clave::Fecha(a), Clave::Fecha(b)) => a.cmp(b),
            (Clave::Valor(a), Clave::Valor(b)) => crate::bases::compare(*a, *b),
            _ => Ordering::Equal,
        };
        let o = if orden.descendente { o.reverse() } else { o };
        o.then_with(|| pa.cmp(pb))
    });
    notas.extend(con_clave.into_iter().map(|(_, p, d)| (p, d)));
}

/// El valor como se muestra en una celda.
pub fn mostrar(valor: &PropValue) -> String {
    match valor {
        PropValue::Null => String::new(),
        PropValue::Text(v)
        | PropValue::Number(v)
        | PropValue::Date(v)
        | PropValue::DateTime(v)
        | PropValue::Link(v) => v.clone(),
        PropValue::Bool(v) => if *v { "sí" } else { "no" }.into(),
        PropValue::List(v) => v.iter().map(mostrar).collect::<Vec<_>>().join(", "),
        PropValue::Map(v) => v
            .iter()
            .map(|(k, v)| format!("{k}: {}", mostrar(v)))
            .collect::<Vec<_>>()
            .join(", "),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opciones_y_consulta() {
        let c = parse("tag:#proyecto [estado:activo]\nvista: tabla\ncolumnas: estado, fecha\norden: fecha desc\n").unwrap();
        assert_eq!(c.vista, Vista::Tabla);
        assert_eq!(c.columnas, ["estado", "fecha"]);
        assert_eq!(
            c.orden,
            Orden {
                criterio: Criterio::Propiedad("fecha".into()),
                descendente: true
            }
        );
        assert_eq!(
            c.query,
            parse_query("tag:#proyecto [estado:activo]").unwrap()
        );
        // Por defecto: lista por nombre ascendente; la consulta puede ocupar varias líneas.
        let c = parse("tag:#a\nOR tag:#b").unwrap();
        assert_eq!(c.vista, Vista::Lista);
        assert_eq!(c.orden, Orden::default());
        assert_eq!(c.query.any_of.len(), 2);
    }

    #[test]
    fn orden_por_criterio() {
        let o = |s: &str| parse(&format!("x\norden: {s}")).unwrap().orden;
        assert_eq!(o("Modificación desc").criterio, Criterio::Modificado);
        assert!(o("modificado desc").descendente);
        assert_eq!(o("creado").criterio, Criterio::Creado);
        assert_eq!(o("nombre asc").criterio, Criterio::Nombre);
        assert_eq!(
            o("[nombre] desc").criterio,
            Criterio::Propiedad("nombre".into())
        );
        assert_eq!(
            o("fecha de inicio").criterio,
            Criterio::Propiedad("fecha de inicio".into())
        );
    }

    #[test]
    fn errores_con_posicion() {
        let e = parse("tag:#a \"sin cerrar").unwrap_err();
        assert_eq!((e.linea, e.columna), (1, 8));
        assert_eq!(e.mensaje, "Comillas sin cerrar");
        let e = parse("vista: tabla\ntag:#a (b").unwrap_err();
        assert_eq!((e.linea, e.columna), (2, 8));
        let e = parse("x\nvista: arbol").unwrap_err();
        assert_eq!((e.linea, e.columna), (2, 7));
        assert!(e.mensaje.contains("arbol"));
        let e = parse("x\nvista: tabla\nvista: lista").unwrap_err();
        assert_eq!(e.linea, 3);
        let e = parse("vista: conteo\n").unwrap_err();
        assert!(e.mensaje.starts_with("Falta qué buscar"));
        assert!(parse("x\norden: desc").is_err());
        assert!(parse("x\ncolumnas: ,").is_err());
        assert!(
            parse("áé\nvista: nada")
                .unwrap_err()
                .to_string()
                .starts_with("línea 2, columna 7")
        );
    }
}
