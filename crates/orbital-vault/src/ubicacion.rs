//! Rutas de la bóveda que viven fuera de ella (lo «solo en Drive» de la 061b).
//!
//! Un archivo «solo en Drive» no está en la bóveda: está en Drive y, si se abrió o es una nota de
//! texto de hasta 1 MB (su extracto), en una caché del equipo. La app registra acá, por bóveda, la
//! ruta de esa copia; [`crate::RelPath::to_path`] la devuelve en lugar de la ruta en la bóveda.
//! Así la búsqueda, el grafo, las etiquetas, las propiedades, los embeds y el guardado leen y
//! escriben la copia sin saber nada de Drive.
//!
//! Sin nada registrado (una bóveda sin Drive, o sin nada «solo en Drive»), `to_path` solo mira un
//! `AtomicBool`.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};

static HAY: AtomicBool = AtomicBool::new(false);

type Mapa = Arc<HashMap<String, PathBuf>>;

static REGISTRO: RwLock<Vec<(PathBuf, Mapa)>> = RwLock::new(Vec::new());

/// Reemplaza las rutas registradas de la bóveda `root` (ruta relativa → copia fuera de la
/// bóveda). Un mapa vacío las quita.
pub fn registrar(root: &Path, mapa: HashMap<String, PathBuf>) {
    let mut r = REGISTRO.write().unwrap_or_else(|e| e.into_inner());
    r.retain(|(raiz, _)| raiz != root);
    if !mapa.is_empty() {
        r.push((root.to_path_buf(), Arc::new(mapa)));
    }
    HAY.store(!r.is_empty(), Ordering::Release);
}

/// Si lo registrado para `root` es exactamente `mapa` (vacío: nada registrado).
pub fn igual(root: &Path, mapa: &HashMap<String, PathBuf>) -> bool {
    let r = REGISTRO.read().unwrap_or_else(|e| e.into_inner());
    match r.iter().find(|(raiz, _)| raiz == root) {
        Some((_, m)) => **m == *mapa,
        None => mapa.is_empty(),
    }
}

/// La copia fuera de la bóveda de `rel`, si está registrada.
pub fn resolver(root: &Path, rel: &str) -> Option<PathBuf> {
    if !HAY.load(Ordering::Acquire) {
        return None;
    }
    let r = REGISTRO.read().unwrap_or_else(|e| e.into_inner());
    r.iter()
        .find(|(raiz, _)| raiz == root)
        .and_then(|(_, m)| m.get(rel).cloned())
}

/// Como [`resolver`], para una ruta absoluta dentro de alguna bóveda registrada (las imágenes
/// con ruta relativa a la nota, `![](foto.png)`).
pub fn redirigir(abs: &Path) -> Option<PathBuf> {
    if !HAY.load(Ordering::Acquire) {
        return None;
    }
    let r = REGISTRO.read().unwrap_or_else(|e| e.into_inner());
    r.iter().find_map(|(raiz, m)| {
        let rel = abs.strip_prefix(raiz).ok()?;
        m.get(crate::RelPath::from_path(rel).ok()?.as_str())
            .cloned()
    })
}

/// Si `abs` es la copia registrada de algún archivo «solo en Drive»: `(bóveda, ruta relativa)`.
pub fn origen(abs: &Path) -> Option<(PathBuf, String)> {
    if !HAY.load(Ordering::Acquire) {
        return None;
    }
    let r = REGISTRO.read().unwrap_or_else(|e| e.into_inner());
    r.iter().find_map(|(raiz, m)| {
        m.iter()
            .find(|(_, p)| p.as_path() == abs)
            .map(|(rel, _)| (raiz.clone(), rel.clone()))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registrar_resolver_y_quitar() {
        let root = Path::new("/tmp/orbital-prueba-ubicacion");
        let copia = PathBuf::from("/tmp/cache/copias/1/foto.png");
        registrar(
            root,
            HashMap::from([("raw/foto.png".to_owned(), copia.clone())]),
        );
        let rel = crate::RelPath::new("raw/foto.png").unwrap();
        assert_eq!(rel.to_path(root), copia);
        assert_eq!(
            crate::RelPath::new("raw/otra.png").unwrap().to_path(root),
            root.join("raw/otra.png")
        );
        assert_eq!(rel.en_boveda(root), root.join("raw/foto.png"));
        assert_eq!(redirigir(&root.join("raw/foto.png")), Some(copia.clone()));
        assert_eq!(
            origen(&copia),
            Some((root.to_path_buf(), "raw/foto.png".into()))
        );
        registrar(root, HashMap::new());
        assert_eq!(rel.to_path(root), root.join("raw/foto.png"));
    }
}
