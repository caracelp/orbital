//! Identificadores de nota (`id: AT-001`, specs 040 y 041).
//!
//! Una nota declara su identificador en la propiedad `id` y lo repite en los delimitadores de sus
//! partes (`<!-- block 1.2 AT-001 -->`, `<!-- summary AT-001 -->`, `<!-- sources AT-001 -->`,
//! `<!-- connections AT-001 -->`). El índice de la bóveda junta los dos orígenes para que un
//! identificador nuevo no repita ninguno y para avisar de los repetidos.

/// Forma canónica de un identificador para compararlo (sin espacios, en mayúsculas).
pub fn normalize(id: &str) -> String {
    id.trim().trim_matches(['"', '\'']).to_ascii_uppercase()
}

/// Identificador que nombra un delimitador `<!-- block … ID -->` o de sección
/// (`summary`/`síntesis`, `sources`/`fuentes`, `connections`/`conexiones`), si la línea lo es.
pub fn delimiter_id(line: &str) -> Option<&str> {
    let inner = line
        .trim()
        .strip_prefix("<!--")?
        .strip_suffix("-->")?
        .trim();
    let (word, rest) = inner.split_once(char::is_whitespace)?;
    let rest = match word.to_lowercase().as_str() {
        // Se tolera la falta de espacio entre la jerarquía y el id (`1.1.2AP-001`).
        "block" => rest
            .trim_start()
            .trim_start_matches(|c: char| c.is_ascii_digit() || c == '.'),
        "summary" | "sintesis" | "síntesis" | "sources" | "fuentes" | "bibliography"
        | "connections" | "conexiones" => rest,
        _ => return None,
    };
    let id = rest.trim();
    (!id.is_empty()).then_some(id)
}

/// Identificadores de una nota: el de su propiedad `id` y los de sus delimitadores, normalizados
/// y sin repetir. Recorre solo las líneas que empiezan con `<!--`.
pub fn ids_of(prop_id: Option<&str>, source: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut push = |id: &str| {
        let n = normalize(id);
        if !n.is_empty() && !out.contains(&n) {
            out.push(n);
        }
    };
    if let Some(id) = prop_id {
        push(id);
    }
    if source.contains("<!--") {
        for line in source.lines() {
            if line.trim_start().starts_with("<!--")
                && let Some(id) = delimiter_id(line)
            {
                push(id);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn junta_propiedad_y_delimitadores() {
        let src = "---\nid: AT-001\n---\n<!-- block 1 AT-001 -->\n# A\n<!---->\n<!-- block 1.1.2AP-004 -->\nx\n<!---->\n<!-- connections co-002 -->\n<!---->\n<!-- otra cosa -->\n";
        assert_eq!(
            ids_of(Some(" at-001 "), src),
            vec!["AT-001", "AP-004", "CO-002"]
        );
        assert_eq!(delimiter_id("<!---->"), None);
        assert_eq!(delimiter_id("<!-- block 1 -->"), None);
        assert_eq!(delimiter_id("<!-- Síntesis SI-003 -->"), Some("SI-003"));
    }
}
