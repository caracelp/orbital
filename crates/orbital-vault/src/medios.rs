//! Medios incrustados (057): reconocer imágenes, audio y video por la extensión, el tiempo de
//! inicio `#t=` y resolver `![[archivo.ext]]` a un archivo de la bóveda que no es una nota.
//!
//! Sin GTK: la app decide cómo mostrar cada tipo (la Lectura, la Visual de `.html`, exportar).

use crate::RelPath;
use crate::links::Resolution;
use crate::vault::Vault;

/// Qué clase de medio es un archivo.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Tipo {
    Imagen,
    Audio,
    Video,
}

/// Extensiones de audio que se reproducen en el lugar (lo que reproduzca depende del sistema).
pub const AUDIO: &[&str] = &["mp3", "ogg", "oga", "opus", "wav", "flac", "m4a"];
/// Extensiones de video.
pub const VIDEO: &[&str] = &["mp4", "webm", "ogv", "mkv", "mov"];
/// Extensiones de imagen.
pub const IMAGEN: &[&str] = &["png", "jpg", "jpeg", "gif", "webp", "svg", "bmp", "avif"];

/// El tipo de medio de un nombre o una ruta (sin el fragmento `#…`), por su extensión sin
/// distinguir mayúsculas. `None` si no es un medio.
pub fn tipo_de(nombre: &str) -> Option<Tipo> {
    let nombre = nombre.split(['#', '?']).next().unwrap_or(nombre);
    let archivo = nombre.rsplit('/').next().unwrap_or(nombre);
    let (_, ext) = archivo.rsplit_once('.')?;
    // Las extensiones conocidas son cortas: sin asignar para la mayoría de los nombres.
    if ext.is_empty() || ext.len() > 4 {
        return None;
    }
    let mut buf = [0u8; 4];
    let ext = &mut buf[..ext.len()];
    ext.copy_from_slice(&archivo.as_bytes()[archivo.len() - ext.len()..]);
    ext.make_ascii_lowercase();
    let ext = std::str::from_utf8(ext).ok()?;
    if AUDIO.contains(&ext) {
        Some(Tipo::Audio)
    } else if VIDEO.contains(&ext) {
        Some(Tipo::Video)
    } else if IMAGEN.contains(&ext) {
        Some(Tipo::Imagen)
    } else {
        None
    }
}

/// Si el nombre es de audio o video (lo que tiene reproductor).
pub fn es_reproducible(nombre: &str) -> bool {
    matches!(tipo_de(nombre), Some(Tipo::Audio | Tipo::Video))
}

/// Segundos de inicio de un fragmento de medio (`t=30`, `t=1:30`, `t=1:02:03.5`, `t=npt:30`,
/// `t=30,60`: el final se ignora). Acepta el fragmento con o sin `#` y otros pares separados por
/// `&`. `None` si no hay un `t=` válido.
pub fn inicio(fragmento: &str) -> Option<f64> {
    let f = fragmento.trim().trim_start_matches('#');
    let valor = f
        .split('&')
        .find_map(|par| par.trim().strip_prefix("t="))?
        .trim();
    let valor = valor.strip_prefix("npt:").unwrap_or(valor);
    let desde = valor.split(',').next()?.trim();
    if desde.is_empty() {
        return None;
    }
    let mut total = 0.0f64;
    let partes: Vec<&str> = desde.split(':').collect();
    if partes.len() > 3 {
        return None;
    }
    for (n, p) in partes.iter().enumerate() {
        let ultimo = n + 1 == partes.len();
        let v: f64 = if ultimo {
            p.parse().ok()?
        } else {
            f64::from(p.parse::<u32>().ok()?)
        };
        if !v.is_finite() || v < 0.0 {
            return None;
        }
        total = total * 60.0 + v;
    }
    Some(total)
}

/// El destino de un párrafo que es solo `![[medio.ext]]` (con fragmento o alias opcionales):
/// (ruta escrita, fragmento). `None` si no es un medio solo en su párrafo.
pub fn incrustacion(fuente: &str) -> Option<(String, Option<String>)> {
    let inner = fuente.trim().strip_prefix("![[")?.strip_suffix("]]")?;
    if inner.contains("]]") || inner.contains("[[") || inner.contains('\n') {
        return None;
    }
    let t = crate::links::parse_link_target(inner);
    tipo_de(&t.path)?;
    Some((t.path, t.anchor.filter(|a| !a.is_empty())))
}

impl Vault {
    /// Resuelve un archivo que no es nota (imagen, audio, video) visto desde la nota `from`:
    ///
    /// 1. relativo a la carpeta de la nota (`clip.mp3`, `medios/clip.mp3`, `../clip.mp3`);
    /// 2. relativo a la raíz de la bóveda (`raw/clip.mp3`) o de `wiki/` (`/medios/clip.mp3`);
    /// 3. por nombre en toda la bóveda, sin distinguir mayúsculas, con la carpeta escrita como
    ///    final de la ruta (`medios/clip.mp3`). Con varios, gana el de la carpeta de la nota y si
    ///    no el primero en orden de ruta.
    ///
    /// Las carpetas y archivos ocultos nunca se resuelven.
    pub fn resolve_attachment(&self, from: &RelPath, target: &str) -> Resolution {
        let target = target.split(['#', '|']).next().unwrap_or("").trim();
        let target =
            crate::links::percent_decode(target.trim_start_matches('<').trim_end_matches('>'));
        if target.is_empty() || target.contains("://") {
            return Resolution::Missing;
        }
        let dir = from.as_str().rsplit_once('/').map_or("", |(d, _)| d);
        let existe = |rel: &str| -> Option<RelPath> {
            let rel = RelPath::new(rel).ok()?;
            (!rel.as_str().split('/').any(|c| c.starts_with('.'))
                && rel.to_path(self.root()).is_file())
            .then_some(rel)
        };
        let candidatos = if let Some(rest) = target.strip_prefix('/') {
            vec![join(rest, ""), join("wiki", rest)]
        } else {
            vec![join(dir, &target), join("", &target)]
        };
        for c in candidatos.into_iter().flatten() {
            if let Some(p) = existe(&c) {
                return Resolution::Found(p);
            }
        }
        // Por nombre (el final de la ruta escrita).
        let escrito = target.trim_start_matches('/').to_lowercase();
        let (carpeta, nombre) = match escrito.rsplit_once('/') {
            Some((c, n)) => (Some(c.trim_start_matches("./")), n),
            None => (None, escrito.as_str()),
        };
        let mut hits: Vec<&RelPath> = self
            .entries()
            .iter()
            .filter(|e| e.kind != crate::EntryKind::Folder)
            .filter(|e| e.path.file_name().eq_ignore_ascii_case(nombre))
            .filter(|e| {
                carpeta.is_none_or(|c| {
                    let d = e.path.as_str().rsplit_once('/').map_or("", |(d, _)| d);
                    d.to_lowercase().ends_with(c)
                })
            })
            .map(|e| &e.path)
            .collect();
        if hits.is_empty() {
            return Resolution::Missing;
        }
        hits.sort();
        let elegido = hits
            .iter()
            .find(|p| p.as_str().rsplit_once('/').map_or("", |(d, _)| d) == dir)
            .unwrap_or(&hits[0]);
        Resolution::Found((*elegido).clone())
    }
}

/// Une `base` con `rel` resolviendo `.` y `..`; `None` si se sale de la bóveda.
fn join(base: &str, rel: &str) -> Option<String> {
    let mut parts: Vec<&str> = base.split('/').filter(|p| !p.is_empty()).collect();
    for p in rel.split('/') {
        match p {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            otro => parts.push(otro),
        }
    }
    (!parts.is_empty()).then(|| parts.join("/"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reconoce_por_extension() {
        for n in [
            "a.mp3", "A.OGG", "x.oga", "v.opus", "s.wav", "f.FLAC", "m.m4a",
        ] {
            assert_eq!(tipo_de(n), Some(Tipo::Audio), "{n}");
        }
        for n in ["c.mp4", "c.webm", "c.ogv", "c.MKV", "c.mov"] {
            assert_eq!(tipo_de(n), Some(Tipo::Video), "{n}");
        }
        for n in ["i.png", "i.JPG", "i.jpeg", "i.webp", "i.svg"] {
            assert_eq!(tipo_de(n), Some(Tipo::Imagen), "{n}");
        }
        assert_eq!(tipo_de("medios/clip.mp4#t=30"), Some(Tipo::Video));
        assert_eq!(tipo_de("clip.mp3?x=1"), Some(Tipo::Audio));
        for n in [
            "nota.md",
            "nota",
            "x.html",
            "mp3",
            ".mp3x",
            "a.mp3.md",
            "a.",
            "carpeta.mp3/n",
        ] {
            assert_eq!(tipo_de(n), None, "{n}");
        }
        assert!(es_reproducible("a.webm"));
        assert!(!es_reproducible("a.png"));
        // Nombres con letras de varios bytes.
        assert_eq!(tipo_de("canción ñandú.mp3"), Some(Tipo::Audio));
        assert_eq!(tipo_de("ñ.ñ"), None);
    }

    #[test]
    fn tiempo_de_inicio() {
        assert_eq!(inicio("t=30"), Some(30.0));
        assert_eq!(inicio("#t=30"), Some(30.0));
        assert_eq!(inicio("t=1:30"), Some(90.0));
        assert_eq!(inicio("t=1:02:03.5"), Some(3723.5));
        assert_eq!(inicio("t=npt:12.5"), Some(12.5));
        assert_eq!(inicio("t=10,20"), Some(10.0));
        assert_eq!(inicio("x=1&t=5"), Some(5.0));
        for malo in [
            "",
            "t=",
            "t=abc",
            "t=-3",
            "t=1:2:3:4",
            "encabezado",
            "t=,5",
            "t=inf",
        ] {
            assert_eq!(inicio(malo), None, "{malo}");
        }
    }

    #[test]
    fn incrustacion_de_medio_sola() {
        assert_eq!(
            incrustacion("![[clip.mp4#t=30]]"),
            Some(("clip.mp4".into(), Some("t=30".into())))
        );
        assert_eq!(
            incrustacion("  ![[medios/a.mp3|alias]] "),
            Some(("medios/a.mp3".into(), None))
        );
        assert_eq!(incrustacion("![[nota]]"), None);
        assert_eq!(incrustacion("texto ![[a.mp3]]"), None);
        assert_eq!(incrustacion("![[a.mp3]] y ![[b.mp3]]"), None);
    }
}
