//! Dónde hay fórmulas en un texto plano (el de una nota `.html`, ya sin etiquetas), con las mismas
//! reglas que la Lectura de las notas `.md` (050):
//!
//! - `$$…$$`: fórmula en bloque; puede ocupar varios renglones, pero no un renglón en blanco.
//! - `$…$`: fórmula en línea. El `$` de apertura no va seguido de un espacio; el de cierre no va
//!   precedido de un espacio ni seguido de un dígito. Así `$5 y $10` sigue siendo dinero.
//! - `\$` es un signo pesos literal.
//! - Vacía (`$$` o `$ $`) no es fórmula.

use std::ops::Range;

/// Una fórmula encontrada: el rango incluye los delimitadores.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hallada {
    pub rango: Range<usize>,
    /// El rango del LaTeX, sin los delimitadores.
    pub contenido: Range<usize>,
    pub modo: crate::Modo,
}

/// Las fórmulas de `texto`, en orden.
pub fn buscar(texto: &str) -> Vec<Hallada> {
    let b = texto.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'\\' => i += 2,
            b'$' if b.get(i + 1) == Some(&b'$') => match cierre_bloque(b, i + 2) {
                Some(fin) if !texto[i + 2..fin].trim().is_empty() => {
                    out.push(Hallada {
                        rango: i..fin + 2,
                        contenido: i + 2..fin,
                        modo: crate::Modo::Bloque,
                    });
                    i = fin + 2;
                }
                _ => i += 2,
            },
            b'$' => {
                let abre = b.get(i + 1).is_some_and(|c| !c.is_ascii_whitespace());
                match abre.then(|| cierre_linea(b, i + 1)).flatten() {
                    Some(fin) => {
                        out.push(Hallada {
                            rango: i..fin + 1,
                            contenido: i + 1..fin,
                            modo: crate::Modo::EnLinea,
                        });
                        i = fin + 1;
                    }
                    None => i += 1,
                }
            }
            _ => i += 1,
        }
    }
    out
}

/// Un renglón en blanco (`\n`, espacios, `\n`) entre `desde` y `i`.
fn blanco_antes(b: &[u8], i: usize, ultimo_salto: &mut Option<usize>) -> bool {
    if b[i] != b'\n' {
        return false;
    }
    let blanco = ultimo_salto.is_some_and(|s| b[s + 1..i].iter().all(u8::is_ascii_whitespace));
    *ultimo_salto = Some(i);
    blanco
}

/// El `$$` que cierra un bloque abierto en `desde`.
fn cierre_bloque(b: &[u8], desde: usize) -> Option<usize> {
    let mut i = desde;
    let mut salto = None;
    while i < b.len() {
        match b[i] {
            b'\\' => i += 2,
            b'$' if b.get(i + 1) == Some(&b'$') => return Some(i),
            _ => {
                if blanco_antes(b, i, &mut salto) {
                    return None;
                }
                i += 1;
            }
        }
    }
    None
}

/// El `$` que cierra una fórmula en línea abierta antes de `desde`.
fn cierre_linea(b: &[u8], desde: usize) -> Option<usize> {
    let mut i = desde;
    let mut salto = None;
    while i < b.len() {
        match b[i] {
            b'\\' => i += 2,
            b'$' => {
                let antes = b[i - 1];
                let despues = b.get(i + 1).copied();
                if i > desde
                    && !antes.is_ascii_whitespace()
                    && !despues.is_some_and(|c| c.is_ascii_digit())
                {
                    return Some(i);
                }
                if despues == Some(b'$') {
                    return None;
                }
                i += 1;
            }
            _ => {
                if blanco_antes(b, i, &mut salto) {
                    return None;
                }
                i += 1;
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Modo;

    fn formulas(t: &str) -> Vec<(&str, Modo)> {
        buscar(t)
            .into_iter()
            .map(|h| (&t[h.contenido], h.modo))
            .collect()
    }

    #[test]
    fn en_linea_y_bloque() {
        assert_eq!(
            formulas(r"Sea $x^2$ y $$\int f$$."),
            vec![("x^2", Modo::EnLinea), (r"\int f", Modo::Bloque)]
        );
        assert_eq!(
            formulas("$$\n\\begin{aligned} a &= b \\\\ c &= d \\end{aligned}\n$$"),
            vec![(
                "\n\\begin{aligned} a &= b \\\\ c &= d \\end{aligned}\n",
                Modo::Bloque
            )]
        );
    }

    #[test]
    fn dinero_no_es_formula() {
        assert!(formulas("Cuesta $5 y $10.").is_empty());
        assert!(formulas("De $5 a $ 10").is_empty());
        assert!(formulas("Pagué $5,$10 y listo").is_empty());
        assert!(formulas(r"Un \$ suelto y otro \$").is_empty());
        assert!(formulas("$ x $").is_empty());
        assert_eq!(formulas("$5$ dólares"), vec![("5", Modo::EnLinea)]);
    }

    #[test]
    fn sin_renglon_en_blanco() {
        assert!(formulas("$a\n\nb$").is_empty());
        assert!(formulas("$$a\n  \nb$$").is_empty());
        assert_eq!(formulas("$a\nb$"), vec![("a\nb", Modo::EnLinea)]);
    }

    #[test]
    fn escapes_adentro() {
        assert_eq!(formulas(r"$a \$ b$"), vec![(r"a \$ b", Modo::EnLinea)]);
        assert!(formulas("$$").is_empty());
        assert!(formulas("$$ $$").is_empty());
        assert!(formulas("$").is_empty());
        assert!(formulas("\\").is_empty());
    }
}
