//! Íconos personalizados (062): `.orbital/iconos.yaml`, SVG locales en `.orbital/iconos/` y
//! resolución por prioridad (frontmatter → explícito → herencia → reglas automáticas).

use std::collections::HashSet;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::entry::RelPath;
use crate::fold::fold;
use crate::iconos_catalogo::{self, normalizar_color};
use crate::props::{PropValue, emit, yaml};
use crate::write::{atomic_write, backup, hash};
use crate::{Vault, VaultError, WriteCtx};

/// Ruta del archivo de configuración de íconos dentro de la bóveda.
pub const ICONOS_PATH: &str = ".orbital/iconos.yaml";
/// Carpeta de SVG locales propios dentro de la bóveda.
pub const ICONOS_SVG_DIR: &str = ".orbital/iconos";
/// Versión actual del formato.
const FORMAT_VERSION: u64 = 1;

/// Asignación explícita de un ícono a una ruta (carpeta o archivo).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AsignacionIcono {
    pub ruta: RelPath,
    pub icono: String,
    pub color: Option<String>,
    /// Solo relevante para carpetas: aplica también a sus descendientes sin ícono propio.
    pub heredar: bool,
}

/// Criterio de una regla automática de íconos.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TipoRegla {
    Carpeta,
    Patron,
    PropTipo,
    Etiqueta,
}

impl TipoRegla {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Carpeta => "carpeta",
            Self::Patron => "patron",
            Self::PropTipo => "prop_tipo",
            Self::Etiqueta => "etiqueta",
        }
    }

    pub fn etiqueta_ui(self) -> &'static str {
        match self {
            Self::Carpeta => "Carpeta",
            Self::Patron => "Patrón de nombre",
            Self::PropTipo => "Propiedad type",
            Self::Etiqueta => "Etiqueta",
        }
    }

    pub fn from_str_clave(s: &str) -> Option<Self> {
        match fold(s.trim()).as_str() {
            "carpeta" | "folder" => Some(Self::Carpeta),
            "patron" | "pattern" => Some(Self::Patron),
            "prop_tipo" | "tipo" | "type" => Some(Self::PropTipo),
            "etiqueta" | "tag" => Some(Self::Etiqueta),
            _ => None,
        }
    }
}

/// Regla automática que asigna un ícono sin modificar notas.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReglaIcono {
    pub tipo: TipoRegla,
    pub valor: String,
    pub icono: String,
    pub color: Option<String>,
}

/// Estado completo de `.orbital/iconos.yaml`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct IconosConfig {
    pub asignaciones: Vec<AsignacionIcono>,
    pub reglas: Vec<ReglaIcono>,
}

/// Resultado de resolver el ícono de una entrada.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IconoResuelto {
    pub icono: String,
    pub color: Option<String>,
}

/// Metadatos livianos de una nota que participan en la resolución de íconos.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MetaNotaIcono {
    pub prop_icon: Option<String>,
    pub prop_color: Option<String>,
    pub prop_tipo: Option<String>,
    pub tags: Vec<String>,
}

/// Un SVG local descubierto en `.orbital/iconos/`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SvgLocal {
    /// Nombre sin `.svg` (en minúsculas normalizadas).
    pub nombre: String,
    /// Ruta absoluta en disco.
    pub ruta: PathBuf,
}

/// Lo leído del disco (`.orbital/iconos.yaml` + lista de SVGs locales).
#[derive(Clone, Debug)]
pub struct IconosSnapshot {
    pub config: IconosConfig,
    pub svgs_locales: Vec<SvgLocal>,
    pub hash: Option<u64>,
    pub warning: Option<String>,
    root: Option<PathBuf>,
    writable: bool,
    propio: bool,
}

impl IconosSnapshot {
    pub fn writable(&self) -> bool {
        self.writable
    }

    pub fn propio(&self) -> bool {
        self.propio
    }

    pub fn ruta_svg_local(&self, nombre: &str) -> Option<&Path> {
        let clave = nombre.trim().to_ascii_lowercase();
        self.svgs_locales
            .iter()
            .find(|s| s.nombre == clave)
            .map(|s| s.ruta.as_path())
    }

    pub fn existe_icono(&self, nombre: &str) -> bool {
        self.ruta_svg_local(nombre).is_some() || iconos_catalogo::buscar(nombre).is_some()
    }
}

/// Valida un nombre de ícono (solo letras minúsculas, dígitos, `-` y `_`).
pub fn nombre_icono_valido(s: &str) -> bool {
    let s = s.trim();
    let len = s.len();
    (2..=48).contains(&len)
        && s.bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_')
        && s.as_bytes()[0].is_ascii_alphanumeric()
}

/// Normaliza un nombre de ícono de entrada.
pub fn normalizar_nombre_icono(s: &str) -> Option<String> {
    let n = s
        .trim()
        .trim_start_matches(':')
        .trim_end_matches(':')
        .to_ascii_lowercase();
    nombre_icono_valido(&n).then_some(n)
}

fn validar_ruta(s: &str) -> Result<RelPath, String> {
    let r = RelPath::new(s.trim()).map_err(|_| format!("«{s}» no es una ruta válida"))?;
    if r.as_str().split('/').any(|c| c.starts_with('.')) {
        return Err(format!("«{s}» apunta a un elemento oculto"));
    }
    Ok(r)
}

impl IconosConfig {
    /// Asigna o actualiza el ícono explícito para `ruta`.
    pub fn asignar(&mut self, ruta: RelPath, icono: String, color: Option<String>, heredar: bool) {
        let color = color.and_then(|c| normalizar_color(&c));
        if let Some(pos) = self.asignaciones.iter().position(|a| a.ruta == ruta) {
            self.asignaciones[pos] = AsignacionIcono {
                ruta,
                icono,
                color,
                heredar,
            };
        } else {
            self.asignaciones.push(AsignacionIcono {
                ruta,
                icono,
                color,
                heredar,
            });
        }
    }

    /// Quita la asignación explícita de `ruta`. Devuelve la que había, si existía.
    pub fn quitar(&mut self, ruta: &str) -> Option<AsignacionIcono> {
        let pos = self
            .asignaciones
            .iter()
            .position(|a| a.ruta.as_str() == ruta)?;
        Some(self.asignaciones.remove(pos))
    }

    /// Devuelve la asignación explícita de `ruta`, si existe.
    pub fn asignacion_de(&self, ruta: &str) -> Option<&AsignacionIcono> {
        self.asignaciones.iter().find(|a| a.ruta.as_str() == ruta)
    }

    /// Invierte `heredar` en una carpeta ya asignada. Devuelve el nuevo estado.
    pub fn alternar_heredar(&mut self, ruta: &str) -> Option<bool> {
        let a = self
            .asignaciones
            .iter_mut()
            .find(|a| a.ruta.as_str() == ruta)?;
        a.heredar = !a.heredar;
        Some(a.heredar)
    }

    /// Actualiza rutas al renombrar o mover un archivo o carpeta.
    pub fn renombrar_ruta(&mut self, viejo: &RelPath, nuevo: &RelPath) -> bool {
        let prefijo = format!("{}/", viejo.as_str());
        let mut cambio = false;
        let mut vistas = HashSet::new();
        let mut nuevas = Vec::with_capacity(self.asignaciones.len());
        for mut a in self.asignaciones.drain(..) {
            if a.ruta == *viejo {
                a.ruta = nuevo.clone();
                cambio = true;
            } else if let Some(cola) = a.ruta.as_str().strip_prefix(&prefijo)
                && let Ok(r) = RelPath::new(&format!("{}/{cola}", nuevo.as_str()))
            {
                a.ruta = r;
                cambio = true;
            }
            if vistas.insert(a.ruta.clone()) {
                nuevas.push(a);
            }
        }
        self.asignaciones = nuevas;
        cambio
    }

    /// Quita las asignaciones de una ruta (o carpeta y sus hijos) borrada.
    pub fn borrar_ruta(&mut self, ruta: &RelPath) -> Vec<AsignacionIcono> {
        let prefijo = format!("{}/", ruta.as_str());
        let mut quitadas = Vec::new();
        let mut quedan = Vec::with_capacity(self.asignaciones.len());
        for a in self.asignaciones.drain(..) {
            if a.ruta == *ruta || a.ruta.as_str().starts_with(&prefijo) {
                quitadas.push(a);
            } else {
                quedan.push(a);
            }
        }
        self.asignaciones = quedan;
        quitadas
    }

    /// Resuelve el ícono para `ruta` según la prioridad de la 062:
    /// 1. Propiedad `icon:` del frontmatter (si existe y resuelve a un ícono conocido).
    /// 2. Asignación explícita de `ruta`.
    /// 3. Carpeta ancestra más cercana con `heredar: true`.
    /// 4. Primera regla coincidente en `reglas`.
    pub fn resolver(
        &self,
        ruta: &str,
        meta: Option<&MetaNotaIcono>,
        existe: impl Fn(&str) -> bool,
    ) -> Option<IconoResuelto> {
        // 1. Frontmatter `icon:`
        if let Some(m) = meta
            && let Some(raw) = &m.prop_icon
            && let Some(nombre) = normalizar_nombre_icono(raw)
            && existe(&nombre)
        {
            return Some(IconoResuelto {
                icono: nombre,
                color: m.prop_color.as_deref().and_then(normalizar_color),
            });
        }
        // 2. Asignación explícita
        if let Some(a) = self.asignacion_de(ruta)
            && existe(&a.icono)
        {
            return Some(IconoResuelto {
                icono: a.icono.clone(),
                color: a.color.clone(),
            });
        }
        // 3. Herencia desde la carpeta ancestra más cercana
        let mut prefijo = ruta;
        while let Some((padre, _)) = prefijo.rsplit_once('/') {
            if let Some(a) = self.asignacion_de(padre)
                && a.heredar
                && existe(&a.icono)
            {
                return Some(IconoResuelto {
                    icono: a.icono.clone(),
                    color: a.color.clone(),
                });
            }
            prefijo = padre;
        }
        // 4. Reglas automáticas en orden
        for regla in &self.reglas {
            if !existe(&regla.icono) {
                continue;
            }
            if regla.coincide(ruta, meta) {
                return Some(IconoResuelto {
                    icono: regla.icono.clone(),
                    color: regla.color.clone(),
                });
            }
        }
        None
    }
}

impl ReglaIcono {
    pub fn coincide(&self, ruta: &str, meta: Option<&MetaNotaIcono>) -> bool {
        let val = fold(self.valor.trim());
        if val.is_empty() {
            return false;
        }
        match self.tipo {
            TipoRegla::Carpeta => {
                let dir = val.trim_matches('/');
                let r = fold(ruta);
                r == dir || r.starts_with(&format!("{dir}/"))
            }
            TipoRegla::Patron => {
                let nombre = ruta.rsplit('/').next().unwrap_or(ruta);
                let r_fold = fold(ruta);
                let n_fold = fold(nombre);
                if val.contains('*') {
                    coincide_glob(&val, &n_fold) || coincide_glob(&val, &r_fold)
                } else {
                    n_fold.contains(&val) || r_fold.contains(&val)
                }
            }
            TipoRegla::PropTipo => meta
                .and_then(|m| m.prop_tipo.as_deref())
                .is_some_and(|t| fold(t.trim()) == val),
            TipoRegla::Etiqueta => {
                let buscada = val.trim_start_matches('#');
                meta.is_some_and(|m| {
                    m.tags.iter().any(|t| {
                        let tf = fold(t.trim_start_matches('#'));
                        tf == buscada || tf.starts_with(&format!("{buscada}/"))
                    })
                })
            }
        }
    }
}

/// Coincidencia glob simple con `*` (lineal/iterativa, sin backtracking exponencial).
fn coincide_glob(patron: &str, texto: &str) -> bool {
    let (p, t) = (patron.as_bytes(), texto.as_bytes());
    let (mut pi, mut ti) = (0usize, 0usize);
    let mut star_p = None;
    let mut star_t = 0usize;
    while ti < t.len() {
        if pi < p.len() && p[pi] == t[ti] {
            pi += 1;
            ti += 1;
        } else if pi < p.len() && p[pi] == b'*' {
            star_p = Some(pi);
            pi += 1;
            star_t = ti;
        } else if let Some(sp) = star_p {
            pi = sp + 1;
            star_t += 1;
            ti = star_t;
        } else {
            return false;
        }
    }
    while pi < p.len() && p[pi] == b'*' {
        pi += 1;
    }
    pi == p.len()
}

/// Parsea el texto YAML de `.orbital/iconos.yaml`.
pub fn parse_iconos(source: &str) -> Result<IconosConfig, String> {
    let trimmed = source.trim_start_matches('\u{feff}').trim();
    if trimmed.is_empty() {
        return Ok(IconosConfig::default());
    }
    let entries =
        yaml::parse_block(trimmed).map_err(|e| format!("Línea {}: {}", e.line, e.message))?;
    let mut cfg = IconosConfig::default();
    let mut vistas_rutas = HashSet::new();

    for entry in entries {
        match entry.key.as_str() {
            "format_version" => {
                let v = match &entry.value {
                    PropValue::Number(n) => n.parse::<u64>().ok(),
                    _ => None,
                };
                match v {
                    Some(n) if n <= FORMAT_VERSION => {}
                    Some(n) => return Err(format!("Versión de formato no soportada: {n}")),
                    None => return Err("format_version debe ser un número entero".into()),
                }
            }
            "asignaciones" => {
                let PropValue::List(items) = entry.value else {
                    return Err("«asignaciones» debe ser una lista".into());
                };
                for item in items {
                    let PropValue::Map(m) = item else {
                        return Err("Cada asignación debe ser un mapa".into());
                    };
                    let mut ruta = None;
                    let mut icono = None;
                    let mut color = None;
                    let mut heredar = false;
                    for (k, v) in m {
                        match k.as_str() {
                            "ruta" => {
                                let s = v.as_text().ok_or("«ruta» debe ser texto")?;
                                ruta = Some(validar_ruta(s)?);
                            }
                            "icono" => {
                                let s = v.as_text().ok_or("«icono» debe ser texto")?;
                                icono =
                                    Some(normalizar_nombre_icono(s).ok_or_else(|| {
                                        format!("Nombre de ícono inválido: «{s}»")
                                    })?);
                            }
                            "color" => {
                                if let Some(s) = v.as_text() {
                                    color = Some(
                                        normalizar_color(s)
                                            .ok_or_else(|| format!("Color inválido: «{s}»"))?,
                                    );
                                }
                            }
                            "heredar" => {
                                let PropValue::Bool(b) = v else {
                                    return Err("«heredar» debe ser verdadero o falso".into());
                                };
                                heredar = b;
                            }
                            otra => {
                                return Err(format!("Clave desconocida en asignación: «{otra}»"));
                            }
                        }
                    }
                    let ruta = ruta.ok_or("Falta «ruta» en una asignación")?;
                    let icono = icono.ok_or("Falta «icono» en una asignación")?;
                    if vistas_rutas.insert(ruta.clone()) {
                        cfg.asignaciones.push(AsignacionIcono {
                            ruta,
                            icono,
                            color,
                            heredar,
                        });
                    }
                }
            }
            "reglas" => {
                let PropValue::List(items) = entry.value else {
                    return Err("«reglas» debe ser una lista".into());
                };
                for item in items {
                    let PropValue::Map(m) = item else {
                        return Err("Cada regla debe ser un mapa".into());
                    };
                    let mut tipo = None;
                    let mut valor = None;
                    let mut icono = None;
                    let mut color = None;
                    for (k, v) in m {
                        match k.as_str() {
                            "tipo" => {
                                let s = v.as_text().ok_or("«tipo» debe ser texto")?;
                                tipo =
                                    Some(TipoRegla::from_str_clave(s).ok_or_else(|| {
                                        format!("Tipo de regla desconocido: «{s}»")
                                    })?);
                            }
                            "valor" => {
                                let s = v.as_text().ok_or("«valor» debe ser texto")?;
                                if s.trim().is_empty() {
                                    return Err("«valor» de regla no puede estar vacío".into());
                                }
                                valor = Some(s.trim().to_owned());
                            }
                            "icono" => {
                                let s = v.as_text().ok_or("«icono» debe ser texto")?;
                                icono =
                                    Some(normalizar_nombre_icono(s).ok_or_else(|| {
                                        format!("Nombre de ícono inválido: «{s}»")
                                    })?);
                            }
                            "color" => {
                                if let Some(s) = v.as_text() {
                                    color = Some(
                                        normalizar_color(s)
                                            .ok_or_else(|| format!("Color inválido: «{s}»"))?,
                                    );
                                }
                            }
                            otra => return Err(format!("Clave desconocida en regla: «{otra}»")),
                        }
                    }
                    cfg.reglas.push(ReglaIcono {
                        tipo: tipo.ok_or("Falta «tipo» en una regla")?,
                        valor: valor.ok_or("Falta «valor» en una regla")?,
                        icono: icono.ok_or("Falta «icono» en una regla")?,
                        color,
                    });
                }
            }
            otra => return Err(format!("Clave desconocida: «{otra}»")),
        }
    }
    Ok(cfg)
}

/// Serializa [`IconosConfig`] al formato YAML canónico de `.orbital/iconos.yaml`.
pub fn emit_iconos(cfg: &IconosConfig) -> String {
    let mut out = format!("format_version: {FORMAT_VERSION}\n");
    if cfg.asignaciones.is_empty() {
        out.push_str("asignaciones: []\n");
    } else {
        out.push_str("asignaciones:\n");
        for a in &cfg.asignaciones {
            out.push_str(&format!("  - ruta: {}\n", emit::scalar(a.ruta.as_str())));
            out.push_str(&format!("    icono: {}\n", emit::scalar(&a.icono)));
            if let Some(c) = &a.color {
                out.push_str(&format!("    color: {}\n", emit::scalar(c)));
            }
            if a.heredar {
                out.push_str("    heredar: true\n");
            }
        }
    }
    if cfg.reglas.is_empty() {
        out.push_str("reglas: []\n");
    } else {
        out.push_str("reglas:\n");
        for r in &cfg.reglas {
            out.push_str(&format!("  - tipo: {}\n", r.tipo.as_str()));
            out.push_str(&format!("    valor: {}\n", emit::scalar(&r.valor)));
            out.push_str(&format!("    icono: {}\n", emit::scalar(&r.icono)));
            if let Some(c) = &r.color {
                out.push_str(&format!("    color: {}\n", emit::scalar(c)));
            }
        }
    }
    out
}

/// Descubre los archivos `.svg` locales en `.orbital/iconos/`.
pub fn escanear_svgs_locales(root: &Path) -> Vec<SvgLocal> {
    let dir = root.join(ICONOS_SVG_DIR);
    let Ok(rd) = fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for entry in rd.flatten() {
        let p = entry.path();
        if !p.is_file() {
            continue;
        }
        let Some(ext) = p.extension().and_then(|e| e.to_str()) else {
            continue;
        };
        if !ext.eq_ignore_ascii_case("svg") {
            continue;
        }
        let Some(stem) = p.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        if let Some(nombre) = normalizar_nombre_icono(stem) {
            out.push(SvgLocal { nombre, ruta: p });
        }
    }
    out.sort_by(|a, b| a.nombre.cmp(&b.nombre));
    out.dedup_by(|a, b| a.nombre == b.nombre);
    out
}

/// Copia un archivo `.svg` local a `.orbital/iconos/<nombre>.svg` y devuelve su nombre normalizado.
pub fn importar_svg_local(root: &Path, origen: &Path) -> Result<SvgLocal, VaultError> {
    let bytes = fs::read(origen)?;
    let texto = std::str::from_utf8(&bytes)
        .map_err(|_| VaultError::InvalidPath(origen.display().to_string()))?;
    if !texto.contains("<svg") {
        return Err(VaultError::InvalidPath(
            "El archivo elegido no parece un SVG válido".into(),
        ));
    }
    let stem = origen
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("icono");
    let limpio: String = stem
        .to_ascii_lowercase()
        .chars()
        .map(|c| {
            if c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_' {
                c
            } else {
                '-'
            }
        })
        .collect();
    let base = limpio.trim_matches('-');
    let base = if base.len() >= 2 && base.as_bytes()[0].is_ascii_alphanumeric() {
        base.to_owned()
    } else {
        format!("svg-{base}")
    };
    let dir = root.join(ICONOS_SVG_DIR);
    fs::create_dir_all(&dir)?;
    let mut nombre = base.clone();
    let mut n = 2usize;
    while dir.join(format!("{nombre}.svg")).exists() {
        nombre = format!("{base}-{n}");
        n += 1;
    }
    let destino = dir.join(format!("{nombre}.svg"));
    fs::write(&destino, &bytes)?;
    Ok(SvgLocal {
        nombre,
        ruta: destino,
    })
}

impl Vault {
    /// Lee `.orbital/iconos.yaml` y los SVGs de `.orbital/iconos/`. Ausente = vacío sin aviso.
    pub fn load_iconos(&self) -> IconosSnapshot {
        let svgs_locales = escanear_svgs_locales(self.root());
        let abs = self.root().join(ICONOS_PATH);
        let bytes = match fs::read(&abs) {
            Ok(b) => b,
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                return IconosSnapshot {
                    config: IconosConfig::default(),
                    svgs_locales,
                    hash: None,
                    warning: None,
                    root: Some(self.root().to_path_buf()),
                    writable: true,
                    propio: false,
                };
            }
            Err(e) => {
                return IconosSnapshot {
                    config: IconosConfig::default(),
                    svgs_locales,
                    hash: None,
                    warning: Some(format!("No se pudo leer {ICONOS_PATH}: {e}")),
                    root: Some(self.root().to_path_buf()),
                    writable: false,
                    propio: false,
                };
            }
        };
        let h = hash(&bytes);
        let Ok(text) = std::str::from_utf8(&bytes) else {
            return IconosSnapshot {
                config: IconosConfig::default(),
                svgs_locales,
                hash: Some(h),
                warning: Some(format!("{ICONOS_PATH} no es UTF-8 válido")),
                root: Some(self.root().to_path_buf()),
                writable: true,
                propio: false,
            };
        };
        match parse_iconos(text) {
            Ok(config) => IconosSnapshot {
                config,
                svgs_locales,
                hash: Some(h),
                warning: None,
                root: Some(self.root().to_path_buf()),
                writable: true,
                propio: false,
            },
            Err(e) => {
                let writable = !e.starts_with("Versión de formato");
                IconosSnapshot {
                    config: IconosConfig::default(),
                    svgs_locales,
                    hash: Some(h),
                    warning: Some(format!("Error en {ICONOS_PATH}: {e}")),
                    root: Some(self.root().to_path_buf()),
                    writable,
                    propio: false,
                }
            }
        }
    }

    /// Guarda `.orbital/iconos.yaml` de forma atómica con comprobación de hash y respaldo previo.
    pub fn save_iconos(
        &self,
        config: &IconosConfig,
        snapshot: &IconosSnapshot,
        ctx: &WriteCtx,
    ) -> Result<IconosSnapshot, VaultError> {
        let rel = RelPath::from_static(ICONOS_PATH);
        if !snapshot.writable || snapshot.root.as_deref() != Some(self.root()) {
            return Err(VaultError::ReadOnly(rel));
        }
        let abs = self.root().join(ICONOS_PATH);
        let actual = match fs::read(&abs) {
            Ok(b) => Some((hash(&b), b)),
            Err(e) if e.kind() == io::ErrorKind::NotFound => None,
            Err(e) => return Err(e.into()),
        };
        if actual.as_ref().map(|(h, _)| *h) != snapshot.hash {
            return Err(VaultError::Conflict { path: rel });
        }
        let body = emit_iconos(config);
        let new_hash = hash(body.as_bytes());
        if let Some((_, bytes)) = &actual
            && !snapshot.propio
        {
            backup(self.root(), &rel, bytes, new_hash, ctx)?;
        }
        if let Some(parent) = abs.parent() {
            fs::create_dir_all(parent)?;
        }
        atomic_write(&abs, body.as_bytes())?;
        Ok(IconosSnapshot {
            config: config.clone(),
            svgs_locales: escanear_svgs_locales(self.root()),
            hash: Some(new_hash),
            warning: None,
            root: Some(self.root().to_path_buf()),
            writable: true,
            propio: true,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::write::{Actor, Date, Offset, Time};

    fn ctx() -> WriteCtx {
        WriteCtx {
            date: Date {
                year: 2026,
                month: 11,
                day: 27,
            },
            time: Time {
                hour: 14,
                minute: 30,
                second: 0,
            },
            utc_offset: Offset { minutes: -180 },
            actor: Actor::human("prueba"),
        }
    }

    #[test]
    fn roundtrip_yaml_y_resolucion_por_prioridad() {
        let mut cfg = IconosConfig::default();
        cfg.asignar(
            RelPath::new("Proyectos").unwrap(),
            "cohete".into(),
            Some("#ff8cc6".into()),
            true,
        );
        cfg.asignar(
            RelPath::new("Proyectos/especial.md").unwrap(),
            "estrella".into(),
            None,
            false,
        );
        cfg.reglas.push(ReglaIcono {
            tipo: TipoRegla::PropTipo,
            valor: "diario".into(),
            icono: "calendario".into(),
            color: Some("#7dcfff".into()),
        });
        cfg.reglas.push(ReglaIcono {
            tipo: TipoRegla::Etiqueta,
            valor: "musica".into(),
            icono: "musica".into(),
            color: None,
        });

        let yaml = emit_iconos(&cfg);
        let parsed = parse_iconos(&yaml).expect("parsea YAML emitido");
        assert_eq!(parsed, cfg);

        let existe = |n: &str| iconos_catalogo::buscar(n).is_some();

        // 1. Gana frontmatter `icon:`
        let meta_fm = MetaNotaIcono {
            prop_icon: Some("atomo".into()),
            prop_color: Some("#9ece6a".into()),
            ..Default::default()
        };
        assert_eq!(
            parsed.resolver("Proyectos/especial.md", Some(&meta_fm), existe),
            Some(IconoResuelto {
                icono: "atomo".into(),
                color: Some("#9ece6a".into()),
            })
        );
        // 2. Gana asignación explícita sobre herencia
        assert_eq!(
            parsed.resolver("Proyectos/especial.md", None, existe),
            Some(IconoResuelto {
                icono: "estrella".into(),
                color: None,
            })
        );
        // 3. Hereda de carpeta ancestra con heredar: true
        assert_eq!(
            parsed.resolver("Proyectos/sub/nota.md", None, existe),
            Some(IconoResuelto {
                icono: "cohete".into(),
                color: Some("#ff8cc6".into()),
            })
        );
        // 4. Regla por prop_tipo y por etiqueta anidada
        let meta_diario = MetaNotaIcono {
            prop_tipo: Some("diario".into()),
            ..Default::default()
        };
        assert_eq!(
            parsed.resolver("wiki/2026-11-27.md", Some(&meta_diario), existe),
            Some(IconoResuelto {
                icono: "calendario".into(),
                color: Some("#7dcfff".into()),
            })
        );
        let meta_tag = MetaNotaIcono {
            tags: vec!["musica/piano".into()],
            ..Default::default()
        };
        assert_eq!(
            parsed.resolver("wiki/acordes.md", Some(&meta_tag), existe),
            Some(IconoResuelto {
                icono: "musica".into(),
                color: None,
            })
        );
    }

    #[test]
    fn renombrar_y_borrar_actualizan_asignaciones() {
        let mut cfg = IconosConfig::default();
        cfg.asignar(RelPath::new("A").unwrap(), "carpeta".into(), None, true);
        cfg.asignar(
            RelPath::new("A/nota.md").unwrap(),
            "nota".into(),
            None,
            false,
        );
        assert!(cfg.renombrar_ruta(&RelPath::new("A").unwrap(), &RelPath::new("B").unwrap()));
        assert!(cfg.asignacion_de("B").is_some());
        assert!(cfg.asignacion_de("B/nota.md").is_some());
        let quitadas = cfg.borrar_ruta(&RelPath::new("B").unwrap());
        assert_eq!(quitadas.len(), 2);
        assert!(cfg.asignaciones.is_empty());
    }

    #[test]
    fn guardado_atomico_y_svg_local() {
        let dir = std::env::temp_dir().join(format!("orbital-iconos-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let vault = crate::open(&dir).unwrap();
        let snap = vault.load_iconos();
        assert!(snap.warning.is_none());
        let mut cfg = snap.config.clone();
        cfg.asignar(RelPath::new("wiki").unwrap(), "libro".into(), None, true);
        let snap2 = vault.save_iconos(&cfg, &snap, &ctx()).unwrap();
        assert!(snap2.propio());

        let svg_src = dir.join("origen.svg");
        fs::write(
            &svg_src,
            "<svg viewBox=\"0 0 24 24\"><circle cx=\"12\" cy=\"12\" r=\"8\"/></svg>",
        )
        .unwrap();
        let imp = importar_svg_local(&dir, &svg_src).unwrap();
        assert_eq!(imp.nombre, "origen");
        let recargado = vault.load_iconos();
        assert!(recargado.ruta_svg_local("origen").is_some());

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn inline_iconos_en_render_y_falsos_positivos() {
        let doc = crate::render::render_markdown(
            "Ver :cohete: a las 12:30:00, clave:: valor y :no_existe:.",
        );
        let crate::render::BlockKind::Paragraph(inlines) = &doc.blocks[0].kind else {
            panic!("esperaba párrafo");
        };
        assert!(inlines.contains(&crate::render::Inline::Icon("cohete".into())));
        assert!(inlines.contains(&crate::render::Inline::Icon("no_existe".into())));
        // 12:30:00 y clave:: valor no se confunden con íconos.
        let txt = crate::render::plain_text(inlines);
        assert!(txt.contains("12:30:00"));
        assert!(txt.contains("clave:: valor"));
    }
}
