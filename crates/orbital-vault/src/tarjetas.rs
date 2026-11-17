//! Tarjetas de estudio (024) escritas en la nota:
//!
//! ```text
//! :::flashcard id=fc-001
//! ¿Pregunta? (Markdown, fórmulas)
//! ---
//! Respuesta (varias líneas)
//! :::
//! ```
//!
//! Se reconocen por renglones, antes del análisis de Markdown: para pulldown-cmark, la pregunta
//! seguida de `---` sería un encabezado y `:::` un párrafo. Las tarjetas dentro de un bloque de
//! código no cuentan, ni un `---` o `:::` dentro de un bloque de código de la respuesta.

use std::ops::Range;

/// Apertura de una tarjeta.
const APERTURA: &str = ":::flashcard";

/// Una tarjeta encontrada en el texto. Los rangos son en bytes, relativos al texto analizado.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RegionTarjeta {
    /// Desde el comienzo de `:::flashcard` hasta el final del `:::` de cierre (sin su salto).
    pub completa: Range<usize>,
    /// Entre el renglón de apertura y el separador `---` (o el cierre, si no hay separador).
    pub pregunta: Range<usize>,
    /// Entre el separador y el cierre; vacío si no hay separador.
    pub respuesta: Range<usize>,
    /// El valor de `id=` en el renglón de apertura.
    pub id: Option<String>,
}

/// Renglones del texto con su comienzo y su final (sin el salto).
fn renglones(texto: &str) -> impl Iterator<Item = (usize, usize, &str)> + Clone {
    let mut desde = 0;
    std::iter::from_fn(move || {
        if desde >= texto.len() {
            return None;
        }
        let fin = texto[desde..].find('\n').map_or(texto.len(), |i| desde + i);
        let r = (desde, fin, texto[desde..fin].trim_end_matches('\r'));
        desde = fin + 1;
        Some(r)
    })
}

/// La cerca de un bloque de código (```` ``` ```` o `~~~`) con la que empieza el renglón.
fn cerca(renglon: &str) -> Option<(u8, usize)> {
    let t = renglon.trim_start_matches(' ');
    if renglon.len() - t.len() > 3 {
        return None;
    }
    let c = *t.as_bytes().first()?;
    if c != b'`' && c != b'~' {
        return None;
    }
    let n = t.bytes().take_while(|&b| b == c).count();
    (n >= 3).then_some((c, n))
}

/// Sigue el estado de los bloques de código: devuelve `true` si el renglón abre, cierra o está
/// dentro de uno.
fn en_codigo(abierta: &mut Option<(u8, usize)>, renglon: &str) -> bool {
    match (*abierta, cerca(renglon)) {
        (Some((c, n)), Some((c2, n2))) if c == c2 && n2 >= n && renglon.trim()[n2..].is_empty() => {
            *abierta = None;
            true
        }
        (Some(_), _) => true,
        (None, Some(f)) => {
            *abierta = Some(f);
            true
        }
        (None, None) => false,
    }
}

/// El `id=` del renglón de apertura (sin comillas ni espacios de más), si lo hay. `None` si el
/// renglón no abre una tarjeta.
fn apertura(renglon: &str) -> Option<Option<String>> {
    let resto = renglon.trim().strip_prefix(APERTURA)?;
    if !resto.is_empty() && !resto.starts_with(char::is_whitespace) {
        return None;
    }
    let id = resto
        .split_whitespace()
        .find_map(|t| t.strip_prefix("id="))
        .map(|v| v.trim_matches(['"', '\'']))
        .filter(|v| !v.is_empty())
        .map(str::to_owned);
    Some(id)
}

/// Las tarjetas del texto, en orden. Una apertura sin `:::` de cierre no es una tarjeta.
pub fn regiones(texto: &str) -> Vec<RegionTarjeta> {
    let mut out = Vec::new();
    if !texto.contains(APERTURA) {
        return out;
    }
    let mut codigo = None;
    let mut it = renglones(texto);
    while let Some((ini, fin, renglon)) = it.next() {
        if en_codigo(&mut codigo, renglon) {
            continue;
        }
        let Some(id) = apertura(renglon) else {
            continue;
        };
        // Se busca el cierre sobre una copia del iterador: sin cierre, se sigue desde la apertura.
        let mut busca = it.clone();
        let mut codigo_dentro = None;
        let mut separador: Option<(usize, usize)> = None;
        let cuerpo = (fin + 1).min(texto.len());
        let mut cierre = None;
        for (a, z, r) in busca.by_ref() {
            if en_codigo(&mut codigo_dentro, r) {
                continue;
            }
            let t = r.trim();
            if t == ":::" {
                cierre = Some((a, z));
                break;
            }
            if separador.is_none() && t == "---" {
                separador = Some((a, z));
            }
        }
        let Some((ca, cz)) = cierre else { continue };
        let (pregunta, respuesta) = match separador {
            Some((sa, sz)) => (cuerpo..sa, (sz + 1).min(ca)..ca),
            None => (cuerpo..ca, ca..ca),
        };
        out.push(RegionTarjeta {
            completa: ini..cz,
            pregunta,
            respuesta,
            id,
        });
        it = busca;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tarjeta_simple_con_id() {
        let t = "Intro\n\n:::flashcard id=fc-001\n¿Qué?\n---\nEsto.\n:::\n\nFin\n";
        let r = regiones(t);
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].id.as_deref(), Some("fc-001"));
        assert_eq!(&t[r[0].pregunta.clone()], "¿Qué?\n");
        assert_eq!(&t[r[0].respuesta.clone()], "Esto.\n");
        assert_eq!(
            &t[r[0].completa.clone()],
            ":::flashcard id=fc-001\n¿Qué?\n---\nEsto.\n:::"
        );
    }

    #[test]
    fn espacios_de_mas_y_sin_id() {
        let t = ":::flashcard id=fc-002 \nP\n---\nR\n:::\n:::flashcard\nP2\n---\nR2\n:::";
        let r = regiones(t);
        assert_eq!(r.len(), 2);
        assert_eq!(r[0].id.as_deref(), Some("fc-002"));
        assert_eq!(r[1].id, None);
        assert_eq!(&t[r[1].respuesta.clone()], "R2\n");
    }

    #[test]
    fn sin_cierre_o_en_codigo_no_es_tarjeta() {
        assert!(regiones(":::flashcard\nP\n---\nR\n").is_empty());
        assert!(regiones("```\n:::flashcard\nP\n---\nR\n:::\n```\n").is_empty());
        assert!(regiones(":::flashcards\nP\n:::\n").is_empty());
    }

    #[test]
    fn separador_y_cierre_dentro_de_codigo_no_cuentan() {
        let t = ":::flashcard\nP\n---\n```\n---\n:::\n```\nR\n:::\n";
        let r = regiones(t);
        assert_eq!(r.len(), 1);
        assert_eq!(&t[r[0].respuesta.clone()], "```\n---\n:::\n```\nR\n");
    }

    #[test]
    fn sin_separador_todo_es_pregunta() {
        let t = ":::flashcard\nSolo pregunta\n:::";
        let r = regiones(t);
        assert_eq!(&t[r[0].pregunta.clone()], "Solo pregunta\n");
        assert!(r[0].respuesta.is_empty());
    }
}
