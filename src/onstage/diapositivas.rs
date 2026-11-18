use super::*;

/// Una diapositiva: el encabezado que la abre (nivel 1 o 2) y su texto fuente, con el encabezado
/// incluido (la diapositiva muestra su título).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Diapositiva {
    /// Texto del encabezado de corte, sin `#`.
    pub titulo: String,
    /// Nivel del encabezado de corte (1 o 2); 0 si la diapositiva abre sin encabezado.
    pub nivel: u8,
    /// Texto fuente completo de la diapositiva.
    pub texto: String,
}

/// Corta el texto en diapositivas por encabezados ATX de nivel 1 o 2. El texto previo al primer
/// encabezado (sin contar el frontmatter) va como primera diapositiva sin título si tiene
/// contenido. Un `#` dentro de un codeblock cerrado no corta; los encabezados con sangría de 4 o
/// más espacios son código, no encabezados.
pub fn dividir_diapositivas(texto: &str) -> Vec<Diapositiva> {
    let mut salidas: Vec<Diapositiva> = Vec::new();
    let mut desde = fin_frontmatter(texto);
    let mut titulo_actual: Option<(String, u8)> = None;
    let mut en_codeblock: Option<char> = None;
    let mut offset = desde;
    for linea in texto[desde..].lines() {
        let largo_linea = linea.len() + 1; // + '\n' (la última línea puede no llevarlo)
        let recortada = linea.trim_start();
        match en_codeblock {
            Some(marca) => {
                if cierra_codeblock(recortada, marca) {
                    en_codeblock = None;
                }
            }
            None => {
                if let Some(nivel) = nivel_encabezado(linea) {
                    if nivel <= 2 {
                        let slice = &texto[desde..offset.min(texto.len())];
                        if !slice.trim().is_empty() || titulo_actual.is_some() {
                            let (titulo, niv) =
                                titulo_actual.take().unwrap_or_else(|| (String::new(), 0));
                            salidas.push(Diapositiva {
                                titulo,
                                nivel: niv,
                                texto: slice.to_owned(),
                            });
                        }
                        desde = offset;
                        titulo_actual = Some((titulo_atx(recortada), nivel));
                    }
                } else {
                    en_codeblock = abre_codeblock(recortada);
                }
            }
        }
        offset = (offset + largo_linea).min(texto.len());
    }
    let slice = &texto[desde..];
    if titulo_actual.is_some() || !slice.trim().is_empty() {
        let (titulo, niv) = titulo_actual.take().unwrap_or_else(|| (String::new(), 0));
        salidas.push(Diapositiva {
            titulo,
            nivel: niv,
            texto: slice.to_owned(),
        });
    }
    salidas
}

/// Byte donde termina el frontmatter (con su salto de línea), si el texto abre con `---`.
fn fin_frontmatter(texto: &str) -> usize {
    let mut resto = texto;
    if let Some(sin_bom) = resto.strip_prefix('\u{feff}') {
        resto = sin_bom;
    }
    if !resto.starts_with("---\n") {
        return 0;
    }
    let base = texto.len() - resto.len();
    let mut offset = base + 4;
    for linea in texto[offset..].lines() {
        let largo_linea = linea.len() + 1;
        if linea.trim() == "---" {
            return (offset + largo_linea).min(texto.len());
        }
        offset = (offset + largo_linea).min(texto.len());
    }
    0
}

/// Sangría en espacios, con cada tabulador contando como 4 (regla de Markdown).
fn sangria(linea: &str) -> usize {
    linea
        .chars()
        .take_while(|c| c.is_whitespace())
        .map(|c| if c == '\t' { 4 } else { 1 })
        .sum()
}

/// Nivel de un encabezado ATX (`#` a `######` seguido de espacio o fin de línea), si la sangría
/// es menor a 4 espacios. `#etiqueta` no es encabezado.
fn nivel_encabezado(linea: &str) -> Option<u8> {
    if sangria(linea) >= 4 {
        return None;
    }
    let recortada = linea.trim_start();
    let nivel = recortada.chars().take_while(|c| *c == '#').count();
    if nivel == 0 || nivel > 6 {
        return None;
    }
    let resto = &recortada[nivel..];
    (resto.is_empty() || resto.starts_with(' ') || resto.starts_with('\t')).then_some(nivel as u8)
}

/// Texto del encabezado ATX sin `#` inicial ni cierre `###` final.
fn titulo_atx(linea: &str) -> String {
    let sin_hashes = linea.trim_start_matches('#').trim_start();
    let sin_cierre = sin_hashes.trim_end_matches('#').trim_end();
    sin_cierre.to_owned()
}

/// Marcador de codeblock si la línea abre uno (``` o ~~~).
fn abre_codeblock(linea: &str) -> Option<char> {
    let marca = linea.chars().next()?;
    (marca == '`' || marca == '~')
        .then(|| linea.chars().take_while(|c| *c == marca).count())
        .filter(|n| *n >= 3)
        .map(|_| marca)
}

/// Si la línea cierra un codeblock abierto con la marca dada (mismo carácter, corrida de 3+).
fn cierra_codeblock(linea: &str, marca: char) -> bool {
    let corrida = linea.chars().take_while(|c| *c == marca).count();
    corrida >= 3 && linea[corrida..].trim().is_empty()
}

/// Propiedades del frontmatter (solo `title` y `description` escalares para la portada), fuera
/// del hilo principal. Sin título no hay portada.
pub(super) fn portada_de(yaml: &[PropEntry]) -> Option<(String, Option<String>)> {
    let titulo = yaml
        .iter()
        .find(|e| e.key == "title")
        .and_then(|e| e.value.as_text().map(str::to_owned))?;
    let descripcion = yaml
        .iter()
        .find(|e| e.key == "description")
        .and_then(|e| e.value.as_text().map(str::to_owned));
    Some((titulo, descripcion))
}
