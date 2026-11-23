//! `.orbital/drive.yaml`: la conexión de la bóveda con Drive y las reglas «solo en este equipo».
//!
//! Viaja con la bóveda (también se sincroniza) y no tiene secretos. Formato:
//!
//! ```yaml
//! # Orbital: conexión con Google Drive (061). No contiene secretos.
//! cuenta: nombre@gmail.com
//! carpeta: 1AbC…
//! solo_local:
//!   - wiki/privado/
//!   - borrador.md
//! ```
//!
//! Las reglas «en ambos» / «solo en Drive» son de cada equipo (decisión 2) y van en el estado.

use std::io;
use std::path::Path;

/// Archivos de `.orbital/` que se comparten entre equipos (el resto es de cada uno).
pub const ORBITAL_COMPARTIDOS: &[&str] = &[
    "config.yaml",
    "drive.yaml",
    "listas.yaml",
    "plegado.yaml",
    "enlaces.yaml",
    "busquedas.yaml",
];

/// Conexión de una bóveda.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Conexion {
    pub cuenta: String,
    /// Id de la carpeta raíz en Drive.
    pub carpeta: String,
    /// Rutas (archivos, o carpetas terminadas en `/`) que nunca se suben.
    pub solo_local: Vec<String>,
}

impl Conexion {
    /// Si `rel` (ruta relativa con `/`) es «solo en este equipo»: gana la regla más específica,
    /// y como hoy solo hay reglas `local`, basta con que alguna la cubra.
    pub fn es_local(&self, rel: &str) -> bool {
        self.solo_local.iter().any(|r| cubre(r, rel))
    }

    pub fn leer(root: &Path) -> io::Result<Option<Self>> {
        match std::fs::read_to_string(root.join(crate::ARCHIVO_CONEXION)) {
            Ok(t) => Ok(Some(Self::desde_texto(&t))),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e),
        }
    }

    pub fn desde_texto(t: &str) -> Self {
        let mut c = Self::default();
        let mut en_lista = false;
        for linea in t.lines() {
            let l = linea.trim_end();
            if l.trim_start().starts_with('#') || l.trim().is_empty() {
                continue;
            }
            if let Some(item) = l.trim_start().strip_prefix("- ")
                && en_lista
            {
                let v = sin_comillas(item.trim());
                if !v.is_empty() {
                    c.solo_local.push(v);
                }
                continue;
            }
            en_lista = false;
            if let Some((k, v)) = l.split_once(':') {
                let v = sin_comillas(v.trim());
                match k.trim() {
                    "cuenta" => c.cuenta = v,
                    "carpeta" => c.carpeta = v,
                    "solo_local" => en_lista = true,
                    _ => {}
                }
            }
        }
        c
    }

    pub fn a_texto(&self) -> String {
        let mut s =
            String::from("# Orbital: conexión con Google Drive (061). No contiene secretos.\n");
        s.push_str(&format!("cuenta: {}\n", comillas(&self.cuenta)));
        s.push_str(&format!("carpeta: {}\n", comillas(&self.carpeta)));
        s.push_str("solo_local:\n");
        for r in &self.solo_local {
            s.push_str(&format!("  - {}\n", comillas(r)));
        }
        s
    }

    pub fn guardar(&self, root: &Path) -> io::Result<()> {
        let ruta = root.join(crate::ARCHIVO_CONEXION);
        if let Some(p) = ruta.parent() {
            std::fs::create_dir_all(p)?;
        }
        orbital_vault::write::atomic_write(&ruta, self.a_texto().as_bytes()).map(|_| ())
    }

    /// Agrega o quita una regla «solo en este equipo».
    pub fn poner_local(&mut self, rel: &str, local: bool) {
        self.solo_local.retain(|r| r != rel);
        if local {
            self.solo_local.push(rel.to_owned());
            self.solo_local.sort();
        }
    }
}

/// Dónde vive un archivo o carpeta (061b): el selector «Ubicación» del explorador.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Ubicacion {
    /// Solo en este equipo: nunca se sube (regla compartida en `drive.yaml`).
    Local,
    /// En este equipo y en Drive.
    #[default]
    Ambos,
    /// Solo en Drive: no hay copia en la bóveda (un marcador en el explorador).
    SoloDrive,
}

impl Ubicacion {
    pub fn clave(self) -> &'static str {
        match self {
            Self::Local => "local",
            Self::Ambos => "ambos",
            Self::SoloDrive => "solo-drive",
        }
    }

    pub fn desde_clave(t: &str) -> Option<Self> {
        match t {
            "local" => Some(Self::Local),
            "ambos" => Some(Self::Ambos),
            "solo-drive" => Some(Self::SoloDrive),
            _ => None,
        }
    }

    /// Texto del selector.
    pub fn texto(self) -> &'static str {
        match self {
            Self::Local => "Solo en este equipo",
            Self::Ambos => "En ambos",
            Self::SoloDrive => "Solo en Drive",
        }
    }
}

/// La regla más específica que cubre `rel` (la de la ruta más larga; con la misma ruta, gana la
/// compartida «solo en este equipo»). `None`: ninguna regla la cubre.
pub fn ubicacion<'a, I>(reglas: I, rel: &str) -> Option<Ubicacion>
where
    I: IntoIterator<Item = (&'a str, Ubicacion)>,
{
    let mut mejor: Option<(usize, Ubicacion)> = None;
    for (r, u) in reglas {
        if !cubre(r, rel) {
            continue;
        }
        let largo = r.trim_end_matches('/').len();
        let gana = match mejor {
            None => true,
            Some((l, v)) => largo > l || (largo == l && u == Ubicacion::Local && v != u),
        };
        if gana {
            mejor = Some((largo, u));
        }
    }
    mejor.map(|(_, u)| u)
}

/// Si la regla `r` cubre `rel`: igual, o una carpeta (con o sin `/` final) que la contiene.
pub fn cubre(r: &str, rel: &str) -> bool {
    let base = r.trim_end_matches('/');
    !base.is_empty()
        && (rel == base
            || rel
                .strip_prefix(base)
                .is_some_and(|resto| resto.starts_with('/')))
}

/// Si una ruta se sincroniza por su forma: nada oculto (`.git/`, `.orbital/`…), salvo la
/// configuración compartida de `.orbital/`.
pub fn sincronizable(rel: &str) -> bool {
    if let Some(n) = rel.strip_prefix(".orbital/") {
        return ORBITAL_COMPARTIDOS.contains(&n);
    }
    !rel.split('/').any(|c| c.starts_with('.'))
}

fn sin_comillas(v: &str) -> String {
    let v = v.trim();
    if v.len() >= 2
        && ((v.starts_with('"') && v.ends_with('"')) || (v.starts_with('\'') && v.ends_with('\'')))
    {
        v[1..v.len() - 1].replace("\\\"", "\"")
    } else {
        v.to_owned()
    }
}

fn comillas(v: &str) -> String {
    format!("\"{}\"", v.replace('"', "\\\""))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ida_y_vuelta() {
        let mut c = Conexion {
            cuenta: "a@b.c".into(),
            carpeta: "1Ab".into(),
            solo_local: vec![],
        };
        c.poner_local("wiki/privado/", true);
        c.poner_local("borrador: \"x\".md", true);
        let t = c.a_texto();
        assert_eq!(Conexion::desde_texto(&t), c);
        c.poner_local("wiki/privado/", false);
        assert_eq!(c.solo_local.len(), 1);
    }

    #[test]
    fn reglas_por_carpeta_y_archivo() {
        let c = Conexion {
            solo_local: vec!["wiki/privado/".into(), "raw/a.png".into(), "wiki/x".into()],
            ..Default::default()
        };
        assert!(c.es_local("wiki/privado"));
        assert!(c.es_local("wiki/privado/nota.md"));
        assert!(!c.es_local("wiki/privadoX/nota.md"));
        assert!(c.es_local("raw/a.png"));
        assert!(!c.es_local("raw/a.png.bak"));
        assert!(c.es_local("wiki/x/y.md"));
    }

    #[test]
    fn gana_la_mas_especifica() {
        let reglas = [
            ("raw/", Ubicacion::SoloDrive),
            ("raw/fotos/", Ubicacion::Ambos),
            ("raw/fotos/grande.png", Ubicacion::SoloDrive),
            ("wiki/privado", Ubicacion::Local),
            ("wiki/privado/", Ubicacion::Ambos),
        ];
        let u = |rel| ubicacion(reglas.iter().map(|(r, u)| (*r, *u)), rel);
        assert_eq!(u("raw/video.mp4"), Some(Ubicacion::SoloDrive));
        assert_eq!(u("raw/fotos/a.png"), Some(Ubicacion::Ambos));
        assert_eq!(u("raw/fotos/grande.png"), Some(Ubicacion::SoloDrive));
        assert_eq!(u("wiki/privado/x.md"), Some(Ubicacion::Local));
        assert_eq!(u("wiki/a.md"), None);
        for v in [Ubicacion::Local, Ubicacion::Ambos, Ubicacion::SoloDrive] {
            assert_eq!(Ubicacion::desde_clave(v.clave()), Some(v));
        }
    }

    #[test]
    fn ocultos_fuera_salvo_la_configuracion() {
        assert!(sincronizable("wiki/a.md"));
        assert!(!sincronizable(".git/config"));
        assert!(!sincronizable("wiki/.borrador.md"));
        assert!(sincronizable(".orbital/listas.yaml"));
        assert!(!sincronizable(".orbital/historial/wiki/a.md/x.txt"));
        assert!(!sincronizable(".orbital/conflictos/a.md"));
    }
}
