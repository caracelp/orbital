//! Medios de la nota abierta (034): las imágenes y los videos que una nota `.md` incrusta, con su
//! ubicación en el texto y el encabezado bajo el que caen, para la galería lateral.
//!
//! Sin GTK: [`extraer`] recorre el texto (sin disco) y [`Vault::resolver_medios_nota`] resuelve
//! cada archivo contra el índice de la bóveda y expande los bloques ` ```carrusel ` y
//! ` ```galeria `. La app llama a las dos en un hilo aparte.
//!
//! Qué cuenta: los embeds `![[archivo.png]]` y `![texto](archivo.png)` del texto de la nota y los
//! medios de sus carruseles y galerías. Qué no: lo que está dentro de otros codeblocks, de código
//! en línea o del frontmatter, las notas incrustadas (sus medios son de otra nota), el audio y
//! las direcciones externas (nada se pide a la red).
//!
//! Cada carrusel o galería forma su propio grupo, a continuación del encabezado que lo contiene
//! (un nivel más adentro), con el título «Carrusel» o «Galería: carpeta».

use std::path::PathBuf;

use crate::RelPath;
use crate::carrusel::{self, Clase};
use crate::galeria;
use crate::links::Resolution;
use crate::vault::Vault;

/// El título del grupo de lo que está antes del primer encabezado.
pub const INICIO: &str = "Inicio de la nota";

/// De dónde salió un medio.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Origen {
    /// Un embed del texto de la nota.
    Embed,
    Carrusel,
    Galeria,
}

/// Un grupo de la galería lateral: un encabezado de la nota, el principio de la nota o un
/// bloque de carrusel o galería.
#[derive(Clone, Debug, PartialEq)]
pub struct Seccion {
    pub titulo: String,
    /// 0 para el principio de la nota; 1–6 para los encabezados; un bloque, uno más que su
    /// encabezado.
    pub nivel: u8,
    /// Renglón (desde 0) del encabezado o del bloque.
    pub linea: usize,
    /// El encabezado que contiene al bloque (para buscar por encabezado); vacío si no es un
    /// bloque.
    pub padre: String,
    pub origen: Origen,
}

/// Dónde está el medio en el texto, para «Ir al embed».
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ubicacion {
    /// Renglón (desde 0).
    pub linea: usize,
    /// Renglones que ocupa (1 para un embed; el bloque entero para un carrusel o una galería).
    pub lineas: usize,
    /// Bytes del embed dentro del renglón (`None` en un bloque: el bloque entero).
    pub rango: Option<(usize, usize)>,
    /// El texto exacto del embed (o del renglón que abre el bloque), para volver a encontrarlo
    /// si la nota cambió mientras tanto.
    pub texto: String,
}

/// Un medio de la nota.
#[derive(Clone, Debug, PartialEq)]
pub struct Medio {
    /// La ruta como está escrita (o la ruta en la bóveda si viene de una carpeta).
    pub escrito: String,
    pub fragmento: Option<String>,
    pub leyenda: Option<String>,
    /// [`Clase::Imagen`] o [`Clase::Video`].
    pub clase: Clase,
    /// Índice en [`MediosNota::secciones`].
    pub seccion: usize,
    pub ubicacion: Ubicacion,
    /// El archivo en la bóveda (`None`: falta o todavía no se resolvió).
    pub path: Option<RelPath>,
    pub abs: Option<PathBuf>,
}

impl Medio {
    /// El nombre del archivo, sin carpetas.
    pub fn nombre(&self) -> &str {
        match &self.path {
            Some(p) => p.file_name(),
            None => self.escrito.rsplit('/').next().unwrap_or(&self.escrito),
        }
    }
}

/// Un bloque de carrusel o galería, guardado tal cual hasta resolverlo.
#[derive(Clone, Debug, PartialEq)]
struct Bloque {
    seccion: usize,
    cuerpo: String,
    ubicacion: Ubicacion,
}

/// Los medios de una nota, por grupo.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MediosNota {
    pub secciones: Vec<Seccion>,
    /// En el orden de la grilla: grupo por grupo y, adentro, en el orden del texto.
    pub medios: Vec<Medio>,
    bloques: Vec<Bloque>,
    resuelto: bool,
}

impl MediosNota {
    /// Cuántos medios tiene cada grupo.
    pub fn conteos(&self) -> Vec<usize> {
        let mut c = vec![0; self.secciones.len()];
        for m in &self.medios {
            c[m.seccion] += 1;
        }
        c
    }
}

/// Si el renglón abre o cierra una valla de código: (carácter, largo, lenguaje).
fn valla(t: &str) -> Option<(u8, usize, &str)> {
    let b = t.as_bytes();
    let c = *b.first()?;
    if c != b'`' && c != b'~' {
        return None;
    }
    let n = b.iter().take_while(|x| **x == c).count();
    if n < 3 {
        return None;
    }
    let resto = &t[n..];
    // Una valla de acentos graves no lleva otro acento grave en su información.
    if c == b'`' && resto.contains('`') {
        return None;
    }
    let lang = resto.split_whitespace().next().unwrap_or("");
    Some((c, n, lang))
}

/// El texto de un encabezado ATX (`## Título ##`), sin el id de bloque final.
fn encabezado(t: &str) -> Option<(u8, String)> {
    let n = t.bytes().take_while(|b| *b == b'#').count();
    if n == 0 || n > 6 {
        return None;
    }
    let resto = &t[n..];
    if !resto.is_empty() && !resto.starts_with([' ', '\t']) {
        return None;
    }
    let mut s = resto.trim();
    // Los `#` de cierre, si van separados.
    let sin = s.trim_end_matches('#');
    if sin.len() < s.len() && (sin.is_empty() || sin.ends_with([' ', '\t'])) {
        s = sin.trim_end();
    }
    // `^id` al final.
    if let Some((antes, id)) = s.rsplit_once(" ^")
        && !id.is_empty()
        && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
    {
        s = antes.trim_end();
    }
    Some((n as u8, s.to_owned()))
}

/// Los tramos de código en línea de un renglón (bytes), que no cuentan.
fn codigo_en_linea(r: &str) -> Vec<(usize, usize)> {
    let b = r.as_bytes();
    let mut tramos = Vec::new();
    let mut i = 0;
    while i < b.len() {
        if b[i] != b'`' {
            i += 1;
            continue;
        }
        let n = b[i..].iter().take_while(|x| **x == b'`').count();
        let abre = i;
        let mut j = i + n;
        let mut cierre = None;
        while j < b.len() {
            if b[j] == b'`' {
                let m = b[j..].iter().take_while(|x| **x == b'`').count();
                if m == n {
                    cierre = Some(j + m);
                    break;
                }
                j += m;
            } else {
                j += 1;
            }
        }
        match cierre {
            Some(fin) => {
                tramos.push((abre, fin));
                i = fin;
            }
            None => i = abre + n,
        }
    }
    tramos
}

/// Un embed encontrado en un renglón: (desde, hasta, ruta, fragmento, leyenda).
type Hallado = (usize, usize, String, Option<String>, Option<String>);

/// Los embeds de imagen y video de un renglón (fuera del código en línea).
fn embeds(r: &str) -> Vec<Hallado> {
    let mut out = Vec::new();
    if !r.contains("![") {
        return out;
    }
    let codigo = codigo_en_linea(r);
    let en_codigo = |p: usize| codigo.iter().any(|(a, z)| p >= *a && p < *z);
    let mut desde = 0;
    while let Some(rel) = r[desde..].find("![") {
        let p = desde + rel;
        if en_codigo(p) || (p > 0 && r.as_bytes()[p - 1] == b'\\') {
            desde = p + 2;
            continue;
        }
        if r[p..].starts_with("![[") {
            let Some(fin) = r[p + 3..].find("]]") else {
                break;
            };
            let inner = &r[p + 3..p + 3 + fin];
            let z = p + 3 + fin + 2;
            desde = z;
            if inner.contains("[[") {
                continue;
            }
            let t = crate::links::parse_link_target(inner);
            if es_medio(&t.path) {
                let leyenda = t.alias.filter(|a| !a.is_empty() && !es_medida(a));
                out.push((p, z, t.path, t.anchor.filter(|a| !a.is_empty()), leyenda));
            }
            continue;
        }
        // `![texto](ruta "título")`.
        let Some((alt, url, z)) = markdown(r, p) else {
            desde = p + 2;
            continue;
        };
        desde = z;
        if url.contains("://") || url.starts_with("data:") {
            continue;
        }
        let (ruta, fragmento) = url
            .split_once('#')
            .map_or((url, None), |(a, f)| (a, Some(f.to_owned())));
        let ruta = crate::links::percent_decode(ruta);
        if es_medio(&ruta) {
            let alt = alt.trim();
            let leyenda = (!alt.is_empty() && !es_medida(alt)).then(|| alt.to_owned());
            out.push((p, z, ruta, fragmento.filter(|f| !f.is_empty()), leyenda));
        }
    }
    out
}

/// `![alt](url …)` que empieza en `p`: (alt, url, fin).
fn markdown(r: &str, p: usize) -> Option<(&str, &str, usize)> {
    let resto = &r[p + 2..];
    let cierre_alt = resto.find("](")?;
    let alt = &resto[..cierre_alt];
    if alt.contains('[') || alt.contains(']') {
        return None;
    }
    let ini = p + 2 + cierre_alt + 2;
    let cola = &r[ini..];
    let (url, largo) = if let Some(dentro) = cola.strip_prefix('<') {
        let f = dentro.find('>')?;
        let tras = &dentro[f + 1..];
        let cierre = tras.find(')')?;
        (&dentro[..f], 1 + f + 1 + cierre + 1)
    } else {
        let cierre = cola.find(')')?;
        let dentro = &cola[..cierre];
        let url = dentro.split_whitespace().next().unwrap_or("");
        (url, cierre + 1)
    };
    Some((alt, url.trim(), ini + largo))
}

/// Si es una imagen o un video (lo que muestra la galería).
fn es_medio(ruta: &str) -> bool {
    matches!(
        crate::medios::tipo_de(ruta),
        Some(crate::medios::Tipo::Imagen | crate::medios::Tipo::Video)
    )
}

/// `300`, `300x200`: el ancho de `![[a.png|300]]`, no una leyenda.
fn es_medida(s: &str) -> bool {
    let s = s.trim();
    !s.is_empty()
        && s.split('x')
            .all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()))
}

/// Recorre el texto de una nota `.md` y junta sus medios por grupo. Sin disco: los archivos y
/// los bloques quedan sin resolver hasta [`Vault::resolver_medios_nota`].
pub fn extraer(fuente: &str) -> MediosNota {
    let mut m = MediosNota::default();
    let mut actual: Option<usize> = None;
    // El encabezado vigente (título y nivel), para los bloques.
    let mut vigente: (String, u8) = (String::new(), 0);
    let mut renglones = fuente.split('\n').enumerate().peekable();
    // El frontmatter no cuenta.
    if fuente.starts_with("---") && fuente.split('\n').next().map(str::trim_end) == Some("---") {
        renglones.next();
        for (_, r) in renglones.by_ref() {
            let t = r.trim_end();
            if t == "---" || t == "..." {
                break;
            }
        }
    }
    let seccion_de = |m: &mut MediosNota, actual: &mut Option<usize>| -> usize {
        *actual.get_or_insert_with(|| {
            m.secciones.push(Seccion {
                titulo: INICIO.to_owned(),
                nivel: 0,
                linea: 0,
                padre: String::new(),
                origen: Origen::Embed,
            });
            m.secciones.len() - 1
        })
    };
    while let Some((n, r)) = renglones.next() {
        let r = r.strip_suffix('\r').unwrap_or(r);
        let t = r.trim_start();
        let sangria = r.len() - t.len();
        if sangria < 4
            && let Some((c, largo, lang)) = valla(t)
        {
            let lang = lang.to_lowercase();
            let bloque = if carrusel::es_lenguaje(&lang) {
                Some(Origen::Carrusel)
            } else if galeria::es_lenguaje(&lang) {
                Some(Origen::Galeria)
            } else {
                None
            };
            let mut cuerpo = String::new();
            let mut fin = n;
            for (k, x) in renglones.by_ref() {
                fin = k;
                let x = x.strip_suffix('\r').unwrap_or(x);
                let xt = x.trim();
                if xt.len() >= largo
                    && xt.bytes().all(|b| b == c)
                    && x.len() - x.trim_start().len() < 4
                {
                    break;
                }
                if bloque.is_some() {
                    cuerpo.push_str(x);
                    cuerpo.push('\n');
                }
            }
            if let Some(origen) = bloque {
                m.secciones.push(Seccion {
                    titulo: match origen {
                        Origen::Carrusel => "Carrusel".to_owned(),
                        _ => "Galería".to_owned(),
                    },
                    nivel: vigente.1 + 1,
                    linea: n,
                    padre: if actual.is_some() && vigente.1 > 0 {
                        vigente.0.clone()
                    } else {
                        INICIO.to_owned()
                    },
                    origen,
                });
                m.bloques.push(Bloque {
                    seccion: m.secciones.len() - 1,
                    cuerpo,
                    ubicacion: Ubicacion {
                        linea: n,
                        lineas: fin - n + 1,
                        rango: None,
                        texto: r.to_owned(),
                    },
                });
            }
            continue;
        }
        if sangria < 4
            && let Some((nivel, titulo)) = encabezado(t)
        {
            m.secciones.push(Seccion {
                titulo: if titulo.is_empty() {
                    "(Sin título)".to_owned()
                } else {
                    titulo.clone()
                },
                nivel,
                linea: n,
                padre: String::new(),
                origen: Origen::Embed,
            });
            actual = Some(m.secciones.len() - 1);
            vigente = (titulo, nivel);
            continue;
        }
        for (a, z, escrito, fragmento, leyenda) in embeds(r) {
            let seccion = seccion_de(&mut m, &mut actual);
            m.medios.push(Medio {
                clase: Clase::de(&escrito),
                escrito,
                fragmento,
                leyenda,
                seccion,
                ubicacion: Ubicacion {
                    linea: n,
                    lineas: 1,
                    rango: Some((a, z)),
                    texto: r[a..z].to_owned(),
                },
                path: None,
                abs: None,
            });
        }
    }
    podar(&mut m, true);
    m
}

/// Deja solo los grupos con medios (`con_bloques`: los de los bloques sin resolver también
/// quedan) y renumera.
fn podar(m: &mut MediosNota, con_bloques: bool) {
    let mut usado = vec![false; m.secciones.len()];
    for x in &m.medios {
        usado[x.seccion] = true;
    }
    if con_bloques {
        for b in &m.bloques {
            usado[b.seccion] = true;
        }
    }
    let mut nuevo = vec![usize::MAX; m.secciones.len()];
    let mut k = 0;
    for (i, u) in usado.iter().enumerate() {
        if *u {
            nuevo[i] = k;
            k += 1;
        }
    }
    let mut i = 0;
    m.secciones.retain(|_| {
        let u = usado[i];
        i += 1;
        u
    });
    for x in &mut m.medios {
        x.seccion = nuevo[x.seccion];
    }
    m.bloques.retain(|b| usado[b.seccion]);
    for b in &mut m.bloques {
        b.seccion = nuevo[b.seccion];
    }
}

impl Vault {
    /// Resuelve los medios de la nota `from` contra el índice de la bóveda: cada embed (con la
    /// regla de `![[…]]`, ver [`Vault::resolve_attachment`]) y los medios de cada carrusel y
    /// galería. Idempotente.
    pub fn resolver_medios_nota(&self, from: &RelPath, m: &mut MediosNota) {
        if m.resuelto {
            return;
        }
        m.resuelto = true;
        for x in &mut m.medios {
            if let Resolution::Found(p) = self.resolve_attachment(from, &x.escrito) {
                x.abs = Some(p.to_path(self.root()));
                x.path = Some(p);
            }
        }
        for b in std::mem::take(&mut m.bloques) {
            match m.secciones[b.seccion].origen {
                Origen::Carrusel => {
                    let mut c = carrusel::parse(&b.cuerpo);
                    self.resolver_carrusel(from, &mut c);
                    for d in c.diapositivas {
                        if !matches!(d.clase, Clase::Imagen | Clase::Video) {
                            continue;
                        }
                        let (path, abs) = match d.estado {
                            carrusel::Estado::Encontrado { path, abs } => (Some(path), Some(abs)),
                            _ => (None, None),
                        };
                        m.medios.push(Medio {
                            escrito: d.escrito,
                            fragmento: d.fragmento,
                            leyenda: d.leyenda,
                            clase: d.clase,
                            seccion: b.seccion,
                            ubicacion: b.ubicacion.clone(),
                            path,
                            abs,
                        });
                    }
                }
                _ => {
                    let mut g = galeria::parse(&b.cuerpo);
                    self.resolver_galeria(from, &mut g);
                    if let Some(c) = g.opciones.carpeta.as_deref() {
                        let c = c.trim_start_matches("./");
                        m.secciones[b.seccion].titulo = format!("Galería: {c}");
                    }
                    for e in g.elementos {
                        m.medios.push(Medio {
                            escrito: e.path.as_str().to_owned(),
                            fragmento: None,
                            leyenda: None,
                            clase: e.clase,
                            seccion: b.seccion,
                            ubicacion: b.ubicacion.clone(),
                            path: Some(e.path),
                            abs: Some(e.abs),
                        });
                    }
                }
            }
        }
        // Grupo por grupo (estable: adentro queda el orden del texto).
        m.medios.sort_by_key(|x| x.seccion);
        podar(m, false);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn agrupa_por_encabezado() {
        let m = extraer(
            "![[portada.png]]\n\n# Uno\n\nTexto ![](fotos/a%20b.jpg \"t\") y ![[c.mp4#t=3|Clip]]\n\n## Vacío\n\nNada.\n\n## Dos ##\n![x](d.webp)\n",
        );
        let titulos: Vec<_> = m
            .secciones
            .iter()
            .map(|s| (s.titulo.as_str(), s.nivel))
            .collect();
        assert_eq!(titulos, [(INICIO, 0), ("Uno", 1), ("Dos", 2)]);
        let medios: Vec<_> = m
            .medios
            .iter()
            .map(|x| (x.escrito.as_str(), x.seccion, x.ubicacion.linea))
            .collect();
        assert_eq!(
            medios,
            [
                ("portada.png", 0, 0),
                ("fotos/a b.jpg", 1, 4),
                ("c.mp4", 1, 4),
                ("d.webp", 2, 11)
            ]
        );
        assert_eq!(m.medios[2].clase, Clase::Video);
        assert_eq!(m.medios[2].leyenda.as_deref(), Some("Clip"));
        assert_eq!(m.medios[2].fragmento.as_deref(), Some("t=3"));
        assert_eq!(m.medios[3].leyenda.as_deref(), Some("x"));
        let u = &m.medios[1].ubicacion;
        let renglon = "Texto ![](fotos/a%20b.jpg \"t\") y ![[c.mp4#t=3|Clip]]";
        let (a, z) = u.rango.unwrap();
        assert_eq!(&renglon[a..z], "![](fotos/a%20b.jpg \"t\")");
        assert_eq!(u.texto, "![](fotos/a%20b.jpg \"t\")");
        assert_eq!(m.conteos(), [1, 2, 1]);
    }

    #[test]
    fn ignora_codigo_notas_audio_y_externos() {
        let m = extraer(
            "---\nportada: ![[no.png]]\n---\n# T\n```\n![[en-codigo.png]]\n```\n`![[en-linea.png]]` ![[nota]] ![[a.mp3]] ![](https://x/y.png) \\![[esc.png]] ![[sí.png|300]]\n    ```\n![[tras-sangria.png]]\n",
        );
        let nombres: Vec<_> = m.medios.iter().map(|x| x.escrito.as_str()).collect();
        assert_eq!(nombres, ["sí.png", "tras-sangria.png"]);
        assert_eq!(m.medios[0].leyenda, None, "la medida no es una leyenda");
        assert_eq!(m.secciones.len(), 1);
    }

    #[test]
    fn el_mismo_archivo_dos_veces_son_dos() {
        let m = extraer("# A\n![[x.png]]\n# B\n![[x.png]] ![[x.png]]\n");
        assert_eq!(m.medios.len(), 3);
        assert_eq!(m.medios[1].ubicacion.rango, Some((0, 10)));
        assert_eq!(m.medios[2].ubicacion.rango, Some((11, 21)));
    }

    #[test]
    fn bloques_forman_su_grupo() {
        let m = extraer(
            "# Viaje\n![[a.png]]\n```carrusel\n![[b.png]]\n```\n![[c.png]]\n~~~galeria\ncarpeta: fotos\n~~~\n",
        );
        assert_eq!(m.secciones.len(), 3);
        assert_eq!(m.secciones[1].titulo, "Carrusel");
        assert_eq!(m.secciones[1].nivel, 2);
        assert_eq!(m.secciones[1].padre, "Viaje");
        assert_eq!(m.secciones[2].origen, Origen::Galeria);
        assert_eq!(m.bloques[0].ubicacion.lineas, 3);
        assert_eq!(m.bloques[1].ubicacion.linea, 6);
        // Los embeds del encabezado quedan juntos aunque haya un bloque en el medio.
        assert!(m.medios.iter().all(|x| x.seccion == 0));
    }

    #[test]
    fn nota_sin_medios() {
        let m = extraer("# Hola\n\nSolo texto con `código` y [[enlace]].\n");
        assert!(m.secciones.is_empty());
        assert!(m.medios.is_empty());
    }
}
