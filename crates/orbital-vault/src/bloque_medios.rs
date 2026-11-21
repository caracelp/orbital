//! Lo común a los bloques de medios de una carpeta: el carrusel (032) y la galería (033).
//!
//! Las opciones `clave: valor` del cuerpo del codeblock, los valores (booleanos, extensiones) y,
//! sobre todo, cómo se encuentra una carpeta escrita en la nota y qué archivos tiene: siempre
//! con el índice de la bóveda, sin leer el disco.

use crate::RelPath;
use crate::vault::Vault;

/// `clave: valor` si el renglón es una opción (no un medio ni un elemento de lista).
pub(crate) fn opcion(t: &str) -> Option<(&str, &str)> {
    if t.starts_with('!') || t.starts_with("[[") || t.starts_with('-') {
        return None;
    }
    let (clave, valor) = t.split_once(':')?;
    let clave = clave.trim();
    // Una ruta con dos puntos (`C:`, `http:`) no es una opción.
    if clave.is_empty()
        || !clave
            .chars()
            .all(|ch| ch.is_alphanumeric() || ch == '_' || ch == '-')
        || valor.starts_with("//")
    {
        return None;
    }
    Some((clave, valor.trim()))
}

pub(crate) fn booleano(v: &str) -> Option<bool> {
    match sin_comillas(v).to_lowercase().as_str() {
        "true" | "sí" | "si" | "yes" | "1" | "on" => Some(true),
        "false" | "no" | "0" | "off" => Some(false),
        _ => None,
    }
}

pub(crate) fn sin_comillas(v: &str) -> &str {
    v.trim().trim_matches(['"', '\'']).trim()
}

/// Una extensión escrita como `png`, `.png` o `*.png`, en minúscula y sin punto.
pub(crate) fn ext_limpia(e: &str) -> String {
    e.trim()
        .trim_start_matches("*.")
        .trim_start_matches('.')
        .to_lowercase()
}

/// Una lista de extensiones en un renglón: `[png, .JPG]` o `png, jpg`.
pub(crate) fn lista_de_extensiones(valor: &str) -> Vec<String> {
    let v = valor.trim().trim_start_matches('[').trim_end_matches(']');
    v.split(',')
        .map(|e| ext_limpia(e.trim().trim_matches(['"', '\''])))
        .filter(|e| !e.is_empty())
        .collect()
}

/// La extensión en minúscula.
pub(crate) fn extension(nombre: &str) -> Option<String> {
    let archivo = nombre.rsplit('/').next().unwrap_or(nombre);
    let (_, ext) = archivo.rsplit_once('.')?;
    (!ext.is_empty()).then(|| ext.to_lowercase())
}

/// Un renglón largo, cortado para un aviso.
pub(crate) fn recortar(t: &str) -> String {
    const N: usize = 60;
    if t.chars().count() <= N {
        t.to_owned()
    } else {
        let corte: String = t.chars().take(N).collect();
        format!("{corte}…")
    }
}

/// Escapa texto para HTML (contenido y atributos).
pub(crate) fn escapar(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            c => out.push(c),
        }
    }
    out
}

/// Une `dir` y una ruta relativa con `.` y `..`; `None` si se sale de la bóveda.
fn normalizar(dir: &str, rel: &str) -> Option<String> {
    let mut partes: Vec<&str> = dir.split('/').filter(|p| !p.is_empty()).collect();
    for p in rel.split('/') {
        match p {
            "" | "." => {}
            ".." => {
                partes.pop()?;
            }
            p => partes.push(p),
        }
    }
    Some(partes.join("/"))
}

impl Vault {
    /// La carpeta de un bloque de medios, vista desde la carpeta `dir` de la nota. Escrita con
    /// `./` o `../`, es relativa a la nota y nada más; si no, se busca relativa a la raíz, a la
    /// carpeta de la nota y a `wiki/`, en ese orden. `Some("")`: la raíz. Nunca sale de la
    /// bóveda ni entra en carpetas ocultas.
    pub fn carpeta_de_bloque(&self, dir: &str, escrita: &str) -> Option<String> {
        let escrita = escrita.trim().trim_end_matches('/');
        let es_carpeta = |rel: &str| {
            rel.is_empty()
                || self
                    .entries()
                    .iter()
                    .any(|e| e.kind == crate::EntryKind::Folder && e.path.as_str() == rel)
        };
        let oculta = |c: &str| c.split('/').any(|p| p.starts_with('.') && !p.is_empty());
        if escrita == "." || escrita.starts_with("./") || escrita.starts_with("../") {
            let c = normalizar(dir, escrita)?;
            return (!oculta(&c) && es_carpeta(&c)).then_some(c);
        }
        let escrita = escrita.trim_start_matches('/');
        if escrita.is_empty() {
            return Some(String::new());
        }
        let candidatos = [
            escrita.to_owned(),
            if dir.is_empty() {
                escrita.to_owned()
            } else {
                format!("{dir}/{escrita}")
            },
            format!("wiki/{escrita}"),
        ];
        candidatos
            .into_iter()
            .filter(|c| !c.split('/').any(|p| p == ".." || p == "."))
            .filter(|c| !oculta(c))
            .find(|c| es_carpeta(c))
    }

    /// Los archivos de la carpeta `base` (de [`Vault::carpeta_de_bloque`]) que `acepta` (por su
    /// extensión en minúscula), del índice de la bóveda: sin subcarpetas salvo con `recursivo`,
    /// sin nada oculto, en orden alfabético estable sin distinguir mayúsculas.
    pub fn archivos_de_carpeta(
        &self,
        base: &str,
        recursivo: bool,
        acepta: impl Fn(&str) -> bool,
    ) -> Vec<&crate::Entry> {
        let prefijo = if base.is_empty() {
            String::new()
        } else {
            format!("{base}/")
        };
        let mut hallados: Vec<&crate::Entry> = self
            .entries()
            .iter()
            .filter(|e| e.kind != crate::EntryKind::Folder)
            .filter(|e| {
                let Some(resto) = e.path.as_str().strip_prefix(&prefijo) else {
                    return false;
                };
                (recursivo || !resto.contains('/')) && !resto.split('/').any(|c| c.starts_with('.'))
            })
            .filter(|e| extension(e.path.as_str()).is_some_and(|x| acepta(&x)))
            .collect();
        hallados.sort_by_cached_key(|e| (e.path.as_str().to_lowercase(), e.path.as_str()));
        hallados
    }
}

/// La carpeta de una ruta de la bóveda (`""` en la raíz).
pub(crate) fn carpeta_de(path: &RelPath) -> &str {
    path.as_str().rsplit_once('/').map_or("", |(d, _)| d)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normaliza_rutas_relativas() {
        assert_eq!(
            normalizar("wiki/notas", "./fotos").as_deref(),
            Some("wiki/notas/fotos")
        );
        assert_eq!(
            normalizar("wiki/notas", "../fotos").as_deref(),
            Some("wiki/fotos")
        );
        assert_eq!(normalizar("wiki", "../../x"), None);
        assert_eq!(normalizar("", ".").as_deref(), Some(""));
    }

    #[test]
    fn extensiones_en_un_renglon() {
        assert_eq!(
            lista_de_extensiones("[png, .JPG, '*.webp']"),
            ["png", "jpg", "webp"]
        );
        assert!(lista_de_extensiones("[]").is_empty());
    }
}
