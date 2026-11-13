//! Notas al pie (012): renumeración y análisis de referencias/definiciones.
//!
//! Sintaxis Markdown: referencia `[^nombre]` en el cuerpo y definición `[^nombre]: contenido` al
//! pie (pulldown las emite con ENABLE_FOOTNOTES; acá se analizan del texto para la renumeración).

/// Una referencia o definición de nota al pie encontrada en el texto.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ocurrencia {
    pub nombre: String,
    /// Offset del `[^` inicial en el texto fuente.
    pub inicio: usize,
    /// Offset después del `]` (referencia) o del `:` si es definición.
    pub fin: usize,
    /// Es definición (`[^nombre]:`).
    pub es_definicion: bool,
}

/// Encuentra referencias y definiciones `[^nombre]` en el texto, fuera de código en línea y de
/// cercas (misma regla que las etiquetas de la 004).
pub fn ocurrencias(texto: &str) -> Vec<Ocurrencia> {
    let mut out = Vec::new();
    let mut in_fence = false;
    let mut at = 0usize;
    for (numero, linea) in texto.lines().enumerate() {
        let inicio_linea = at;
        at += linea.len() + 1;
        let trimmed = linea.trim_start();
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            in_fence = !in_fence;
            continue;
        }
        if in_fence {
            continue;
        }
        // Buscar [^nombre] fuera de código en línea (máscara por backticks).
        let mut bytes = linea.as_bytes().to_vec();
        let mut in_code = false;
        for byte in &mut bytes {
            if *byte == b'`' {
                in_code = !in_code;
                *byte = b' ';
            } else if in_code {
                *byte = b' ';
            }
        }
        let masked = String::from_utf8(bytes).expect("la máscara conserva UTF-8");
        let base = inicio_linea + (linea.len() - trimmed.len());
        let mut desde = 0usize;
        while let Some(pos) = masked[desde..].find("[^") {
            let abs = desde + pos;
            let cierre = masked[abs + 2..].find(']');
            let Some(bracket) = cierre else { break };
            let nombre = &masked[abs + 2..abs + 2 + bracket];
            if nombre.is_empty() || nombre.contains('[') || nombre.contains(']') {
                desde = abs + 2;
                continue;
            }
            let inicio_abs = base + abs;
            let fin_ref = inicio_abs + 2 + bracket + 1;
            // ¿Definición? después del `]` viene `:` (para definiciones, la línea empieza con el nombre).
            let al_inicio = abs == linea.len() - linea.trim_start().len();
            let es_def = al_inicio && texto[fin_ref..].starts_with(':') && !nombre.is_empty();
            out.push(Ocurrencia {
                nombre: nombre.to_owned(),
                inicio: inicio_abs,
                fin: if es_def { fin_ref + 1 } else { fin_ref },
                es_definicion: es_def,
            });
            desde = fin_ref - inicio_linea;
            if desde >= masked.len() {
                break;
            }
        }
        let _ = numero;
    }
    out
}

/// Número de línea (0-indexado) de un offset en el texto.
fn linea_de_offset(texto: &str, offset: usize) -> usize {
    texto[..offset].bytes().filter(|&b| b == b'\n').count()
}

/// Renumeración: devuelve el texto nuevo con referencias y definiciones renumeradas en orden de
/// aparición y las definiciones ordenadas por número al final, o None si ya está en orden.
pub fn renumerar(texto: &str) -> Option<String> {
    let occ = ocurrencias(texto);
    let definiciones: Vec<&Ocurrencia> = occ.iter().filter(|o| o.es_definicion).collect();
    if definiciones.is_empty() {
        return None;
    }
    // Orden de aparición de nombres en el texto completo: la primera aparición es [^1].
    let mut orden: Vec<String> = Vec::new();
    for o in &occ {
        if !orden.contains(&o.nombre) {
            orden.push(o.nombre.clone());
        }
    }
    // ¿Ya está todo en orden? (nombres [^1..n] por primera aparición)
    let ya = (1..=orden.len()).all(|n| orden.get(n - 1) == Some(&n.to_string()));
    if ya {
        return None;
    }
    let numero_de = |nombre: &str| -> String {
        orden
            .iter()
            .position(|n| n == nombre)
            .map(|p| (p + 1).to_string())
            .unwrap_or_else(|| nombre.to_owned())
    };
    // Reconstruir línea por línea: referencias renumeradas in situ; líneas de definición
    // renumeradas y ordenadas por número al final.
    let lineas_def: Vec<usize> = (0..texto.lines().count())
        .filter(|&n| {
            occ.iter()
                .any(|o| o.es_definicion && linea_de_offset(texto, o.inicio) == n)
        })
        .collect();
    let mut definiciones_nuevas: Vec<String> = Vec::new();
    let mut salida: Vec<String> = Vec::new();
    for (n, linea) in texto.lines().enumerate() {
        if lineas_def.contains(&n) {
            continue; // se reconstruyen al final
        }
        let mut nueva = String::new();
        let mut at = 0usize;
        while let Some(pos) = linea[at..].find("[^") {
            let abs = at + pos;
            let Some(bracket) = linea[abs + 2..].find(']') else {
                nueva.push_str(&linea[at..]);
                at = linea.len();
                break;
            };
            let nombre = &linea[abs + 2..abs + 2 + bracket];
            if nombre.is_empty() || nombre.contains('[') || nombre.contains(']') {
                let hasta = abs + 2 + bracket + 1;
                nueva.push_str(&linea[at..hasta]);
                at = hasta;
                continue;
            }
            nueva.push_str(&linea[at..abs]);
            nueva.push_str(&format!("[^{}]", numero_de(nombre)));
            at = abs + 2 + bracket + 1;
        }
        nueva.push_str(&linea[at.min(linea.len())..]);
        salida.push(nueva);
    }
    for (n, linea) in texto.lines().enumerate() {
        if !lineas_def.contains(&n) {
            continue;
        }
        // Reconstruir la definición con el número nuevo, preservando el contenido.
        let mut con = String::new();
        for o in occ.iter().filter(|o| o.es_definicion) {
            if linea_de_offset(texto, o.inicio) == n {
                con = texto[o.fin..].to_string();
            }
        }
        let nombre_de_esta = occ
            .iter()
            .find(|o| o.es_definicion && linea_de_offset(texto, o.inicio) == n)
            .map(|o| o.nombre.clone())
            .unwrap_or_default();
        let contenido = if con.is_empty() { String::new() } else { con };
        let _ = contenido;
        // El contenido original de la línea (sin el [^nombre]: inicial).
        if let Some(resto) = linea
            .trim_start()
            .strip_prefix(&format!("[^{}]:", nombre_de_esta))
            .or_else(|| {
                linea
                    .trim_start()
                    .strip_prefix(&format!("[^{}]:", nombre_de_esta))
            })
        {
            let numero = numero_de(&nombre_de_esta);
            definiciones_nuevas.push(format!("[^{}]:{}", numero, resto));
        } else {
            // Fallback: reemplazar el nombre dentro de la línea.
            definiciones_nuevas.push(linea.replace(
                &format!("[^{}]", nombre_de_esta),
                &format!("[^{}]", numero_de(&nombre_de_esta)),
            ));
        }
    }
    // Agregar una línea vacía antes de las definiciones si la última línea de salida no lo está.
    if let Some(ultima) = salida.last()
        && !ultima.trim().is_empty()
    {
        salida.push(String::new());
    }
    // Definiciones ordenadas por número (como tidy-footnotes).
    definiciones_nuevas.sort_by_key(|l| {
        l.trim_start()
            .trim_start_matches("[^")
            .split(']')
            .next()
            .and_then(|n| n.parse::<usize>().ok())
            .unwrap_or(usize::MAX)
    });
    salida.extend(definiciones_nuevas);
    let nuevo = salida.join("\n") + "\n";
    (nuevo != texto).then_some(nuevo)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encuentra_referencias_y_definiciones() {
        let texto = "Texto con [^1] y [^nota].\n\n[^1]: La primera.\n[^nota]: La otra.\n";
        let occ = ocurrencias(texto);
        assert_eq!(occ.len(), 4);
        assert_eq!(occ.iter().filter(|o| o.es_definicion).count(), 2);
        // Fuera de código en línea y cercas.
        let excluida = "```\n[^no]\n```\n`[^tampoco]`\n[^si]\n";
        let occ2 = ocurrencias(excluida);
        assert_eq!(occ2.len(), 1);
        assert_eq!(occ2[0].nombre, "si");
    }

    #[test]
    fn renumera_en_orden_de_aparicion() {
        let texto = "B [^3] y [^2].\n\n[^2]: la dos\n[^3]: la tres\n";
        let nuevo = renumerar(texto).unwrap();
        // La referencia [^3] aparece primero → [^1]; su definición («la tres») la acompaña.
        assert!(nuevo.contains("[^1] y [^2]"), "{nuevo}");
        assert!(nuevo.contains("[^1]: la tres"), "{nuevo}");
        assert!(nuevo.contains("[^2]: la dos"), "{nuevo}");
        // Ordenadas por número al final.
        let i_uno = nuevo.find("[^1]:").unwrap();
        let i_dos = nuevo.find("[^2]:").unwrap();
        assert!(i_uno < i_dos, "{nuevo}");
    }

    #[test]
    fn nombres_no_numericos_se_numeran() {
        let texto = "X [^mi-nota].\n\n[^mi-nota]: contenido\n";
        let nuevo = renumerar(texto).unwrap();
        assert!(nuevo.contains("[^1]."), "{nuevo}");
        assert!(nuevo.contains("[^1]: contenido"), "{nuevo}");
    }

    #[test]
    fn ya_ordenadas_devuelve_none() {
        let texto = "A [^1] y [^2].\n\n[^1]: la uno\n[^2]: la dos\n";
        assert!(renumerar(texto).is_none());
    }

    #[test]
    fn sin_definiciones_no_hace_nada() {
        let texto = "Solo [^1] sin definición.\n";
        assert!(renumerar(texto).is_none());
    }
}
