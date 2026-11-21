//! Cuentas puras de la galería lateral (034), sin GTK: qué medios pasan la búsqueda y el filtro,
//! dónde va cada título de grupo y cada mosaico, y a qué elemento llevan las flechas.

use orbital_vault::carrusel::Clase;
use orbital_vault::medios_nota::MediosNota;

/// Qué tipos se muestran.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Filtro {
    #[default]
    Todo,
    Imagenes,
    Videos,
}

/// Niveles fijos de tamaño de mosaico: el mismo nivel es siempre el mismo lado en píxeles.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Tamano {
    Chico,
    #[default]
    Mediano,
    Grande,
}

impl Tamano {
    /// El lado del cuadro de la miniatura.
    pub fn lado(self) -> i32 {
        match self {
            Self::Chico => 56,
            Self::Mediano => 76,
            Self::Grande => 116,
        }
    }

    pub fn nombre(self) -> &'static str {
        match self {
            Self::Chico => "chico",
            Self::Mediano => "mediano",
            Self::Grande => "grande",
        }
    }

    pub fn de(nombre: &str) -> Self {
        match nombre {
            "chico" => Self::Chico,
            "grande" => Self::Grande,
            _ => Self::Mediano,
        }
    }
}

/// Separación entre mosaicos.
pub const ESPACIO: i32 = 4;
/// Alto del nombre debajo de cada mosaico.
pub const ALTO_NOMBRE: i32 = 18;
/// Alto del título de un grupo.
pub const ALTO_TITULO: i32 = 32;
/// Sangría por nivel de los títulos de los grupos de bloques.
pub const SANGRIA: i32 = 10;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl Rect {
    /// Si se cruza con la franja vertical `a..b`.
    pub fn en_franja(&self, a: i32, b: i32) -> bool {
        self.y < b && self.y + self.h > a
    }
}

/// Un elemento navegable: el título de un grupo o un mosaico.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Pieza {
    /// Índice en [`MediosNota::secciones`].
    Titulo(usize),
    /// Índice en [`MediosNota::medios`].
    Mosaico(usize),
}

/// La grilla dispuesta.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Disposicion {
    /// En orden de arriba hacia abajo.
    pub piezas: Vec<(Pieza, Rect)>,
    pub alto: i32,
    /// Cuántos medios de cada grupo pasan la búsqueda y el filtro.
    pub visibles: Vec<usize>,
    /// Los medios que pasan la búsqueda y el filtro, en orden (también los de grupos plegados:
    /// el visor los recorre todos).
    pub filtrados: Vec<usize>,
}

impl Disposicion {
    pub fn rect(&self, p: Pieza) -> Option<Rect> {
        self.piezas.iter().find(|(q, _)| *q == p).map(|(_, r)| *r)
    }
}

/// Minúsculas y sin tildes (la búsqueda no las distingue). Un carácter por carácter: las
/// posiciones en caracteres valen para el texto original.
pub fn plegar(s: &str) -> Vec<char> {
    s.chars()
        .map(|c| {
            let c = c.to_lowercase().next().unwrap_or(c);
            match c {
                'á' | 'à' | 'ä' | 'â' => 'a',
                'é' | 'è' | 'ë' | 'ê' => 'e',
                'í' | 'ì' | 'ï' | 'î' => 'i',
                'ó' | 'ò' | 'ö' | 'ô' => 'o',
                'ú' | 'ù' | 'ü' | 'û' => 'u',
                otra => otra,
            }
        })
        .collect()
}

/// Dónde aparece la consulta (ya plegada) en `texto`: rango de caracteres de la primera vez.
pub fn coincidencia(texto: &str, consulta: &[char]) -> Option<(usize, usize)> {
    if consulta.is_empty() {
        return None;
    }
    let t = plegar(texto);
    t.windows(consulta.len())
        .position(|w| w == consulta)
        .map(|a| (a, a + consulta.len()))
}

/// El texto con la coincidencia resaltada, como marcado de Pango.
pub fn resaltado(texto: &str, consulta: &[char]) -> String {
    match coincidencia(texto, consulta) {
        None => gtk::glib::markup_escape_text(texto).to_string(),
        Some((a, z)) => {
            let ba = texto.char_indices().nth(a).map_or(texto.len(), |(b, _)| b);
            let bz = texto.char_indices().nth(z).map_or(texto.len(), |(b, _)| b);
            format!(
                "{}<b><u>{}</u></b>{}",
                gtk::glib::markup_escape_text(&texto[..ba]),
                gtk::glib::markup_escape_text(&texto[ba..bz]),
                gtk::glib::markup_escape_text(&texto[bz..])
            )
        }
    }
}

/// Lo que pide la grilla.
pub struct Pedido<'a> {
    pub medios: &'a MediosNota,
    pub consulta: &'a [char],
    pub filtro: Filtro,
    /// Por grupo.
    pub plegadas: &'a [bool],
    pub ancho: i32,
    pub tamano: Tamano,
}

/// Los medios que pasan la búsqueda (por nombre o por encabezado) y el filtro por tipo.
pub fn filtrar(p: &Pedido) -> Vec<usize> {
    let m = p.medios;
    // Los grupos que coinciden por su título o por el encabezado que los contiene.
    let por_titulo: Vec<bool> = m
        .secciones
        .iter()
        .map(|s| {
            !p.consulta.is_empty()
                && (coincidencia(&s.titulo, p.consulta).is_some()
                    || coincidencia(&s.padre, p.consulta).is_some())
        })
        .collect();
    m.medios
        .iter()
        .enumerate()
        .filter(|(_, x)| match p.filtro {
            Filtro::Todo => true,
            Filtro::Imagenes => x.clase == Clase::Imagen,
            Filtro::Videos => x.clase == Clase::Video,
        })
        .filter(|(_, x)| {
            p.consulta.is_empty()
                || por_titulo[x.seccion]
                || coincidencia(x.nombre(), p.consulta).is_some()
        })
        .map(|(k, _)| k)
        .collect()
}

/// Columnas de mosaicos para un ancho.
pub fn columnas(ancho: i32, tamano: Tamano) -> i32 {
    ((ancho + ESPACIO) / (tamano.lado() + ESPACIO)).max(1)
}

/// Dispone los títulos y los mosaicos: cada grupo con medios visibles lleva su título y, si no
/// está plegado, sus mosaicos en filas del mismo lado (el ancho que sobra se reparte entre las
/// columnas, así que el número de columnas, no el lado, sigue al ancho del panel).
pub fn disponer(p: &Pedido) -> Disposicion {
    let filtrados = filtrar(p);
    let m = p.medios;
    let mut visibles = vec![0usize; m.secciones.len()];
    for k in &filtrados {
        visibles[m.medios[*k].seccion] += 1;
    }
    let cols = columnas(p.ancho, p.tamano);
    let lado = p.tamano.lado();
    // El ancho de cada columna: el lado más lo que sobra repartido.
    let celda = ((p.ancho - ESPACIO * (cols - 1)) / cols).max(lado);
    let alto_celda = celda + ALTO_NOMBRE;
    let mut piezas = Vec::with_capacity(filtrados.len() + m.secciones.len());
    let mut y = 0;
    let mut f = 0;
    for (s, sec) in m.secciones.iter().enumerate() {
        if visibles[s] == 0 {
            continue;
        }
        let sangria = SANGRIA * i32::from(sec.nivel.saturating_sub(1)).min(3);
        piezas.push((
            Pieza::Titulo(s),
            Rect {
                x: sangria,
                y,
                w: p.ancho - sangria,
                h: ALTO_TITULO,
            },
        ));
        y += ALTO_TITULO + ESPACIO;
        let plegada = p.plegadas.get(s).copied().unwrap_or(false);
        let mut col = 0;
        while f < filtrados.len() && m.medios[filtrados[f]].seccion == s {
            if !plegada {
                piezas.push((
                    Pieza::Mosaico(filtrados[f]),
                    Rect {
                        x: col * (celda + ESPACIO),
                        y,
                        w: celda,
                        h: alto_celda,
                    },
                ));
                col += 1;
                if col == cols {
                    col = 0;
                    y += alto_celda + ESPACIO;
                }
            }
            f += 1;
        }
        if col > 0 {
            y += alto_celda + ESPACIO;
        }
        y += ESPACIO;
    }
    Disposicion {
        piezas,
        alto: y,
        visibles,
        filtrados,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direccion {
    Izquierda,
    Derecha,
    Arriba,
    Abajo,
}

/// La pieza a la que lleva una flecha desde la pieza `n` (índice en `piezas`): izquierda y
/// derecha recorren en orden; arriba y abajo van a la fila anterior o siguiente, a la pieza más
/// cercana en horizontal (un título es una fila de un solo elemento).
pub fn vecina(piezas: &[(Pieza, Rect)], n: usize, dir: Direccion) -> Option<usize> {
    let (_, r) = piezas.get(n)?;
    match dir {
        Direccion::Izquierda => n.checked_sub(1),
        Direccion::Derecha => (n + 1 < piezas.len()).then_some(n + 1),
        Direccion::Arriba | Direccion::Abajo => {
            let arriba = dir == Direccion::Arriba;
            // La fila vecina: la `y` más cercana del lado pedido.
            let fila = piezas
                .iter()
                .map(|(_, q)| q.y)
                .filter(|y| if arriba { *y < r.y } else { *y > r.y })
                .min_by_key(|y| (y - r.y).abs())?;
            let centro = r.x + r.w / 2;
            piezas
                .iter()
                .enumerate()
                .filter(|(_, (_, q))| q.y == fila)
                .min_by_key(|(_, (_, q))| (q.x + q.w / 2 - centro).abs())
                .map(|(k, _)| k)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use orbital_vault::medios_nota::extraer;

    fn nota() -> MediosNota {
        extraer(
            "# Esquemas\n![[esquema-uno.png]] ![[b.png]] ![[c.png]] ![[d.png]]\n# Videos\n![[clip.mp4]] ![[Ñandú.png]]\n",
        )
    }

    fn pedido<'a>(m: &'a MediosNota, c: &'a [char], plegadas: &'a [bool]) -> Pedido<'a> {
        Pedido {
            medios: m,
            consulta: c,
            filtro: Filtro::Todo,
            plegadas,
            ancho: 248,
            tamano: Tamano::Mediano,
        }
    }

    #[test]
    fn dispone_grupos_y_filas() {
        let m = nota();
        let d = disponer(&pedido(&m, &[], &[]));
        // 248 px: tres columnas de 80.
        assert_eq!(columnas(248, Tamano::Mediano), 3);
        assert_eq!(columnas(248, Tamano::Chico), 4);
        assert_eq!(columnas(248, Tamano::Grande), 2);
        let piezas: Vec<_> = d.piezas.iter().map(|(p, _)| *p).collect();
        assert_eq!(
            piezas,
            [
                Pieza::Titulo(0),
                Pieza::Mosaico(0),
                Pieza::Mosaico(1),
                Pieza::Mosaico(2),
                Pieza::Mosaico(3),
                Pieza::Titulo(1),
                Pieza::Mosaico(4),
                Pieza::Mosaico(5),
            ]
        );
        let r = |k| d.rect(Pieza::Mosaico(k)).unwrap();
        assert_eq!(r(0).w, 80);
        assert_eq!(r(3).y, r(0).y + r(0).h + ESPACIO, "la cuarta pasa de fila");
        assert_eq!(r(3).x, 0);
        assert!(d.rect(Pieza::Titulo(1)).unwrap().y > r(3).y);
        assert_eq!(d.visibles, [4, 2]);
    }

    #[test]
    fn plegar_deja_el_titulo() {
        let m = nota();
        let d = disponer(&pedido(&m, &[], &[true, false]));
        let piezas: Vec<_> = d.piezas.iter().map(|(p, _)| *p).collect();
        assert_eq!(
            piezas,
            [
                Pieza::Titulo(0),
                Pieza::Titulo(1),
                Pieza::Mosaico(4),
                Pieza::Mosaico(5)
            ]
        );
        assert_eq!(d.filtrados.len(), 6, "el visor recorre también lo plegado");
    }

    #[test]
    fn busca_por_nombre_y_encabezado_sin_tildes() {
        let m = nota();
        let q = plegar("ESQUEMA");
        let d = disponer(&pedido(&m, &q, &[]));
        assert_eq!(d.filtrados, [0, 1, 2, 3], "el encabezado coincide");
        let q = plegar("nandu");
        assert_eq!(disponer(&pedido(&m, &q, &[])).filtrados, [5]);
        let q = plegar("uno");
        assert_eq!(disponer(&pedido(&m, &q, &[])).filtrados, [0]);
        let mut p = pedido(&m, &[], &[]);
        p.filtro = Filtro::Videos;
        assert_eq!(disponer(&p).filtrados, [4]);
        p.filtro = Filtro::Imagenes;
        assert_eq!(disponer(&p).filtrados.len(), 5);
        assert_eq!(
            resaltado("Ñandú <x>.png", &plegar("nandu")),
            "<b><u>Ñandú</u></b> &lt;x&gt;.png"
        );
    }

    #[test]
    fn flechas() {
        let m = nota();
        let d = disponer(&pedido(&m, &[], &[]));
        let p = &d.piezas;
        // Del título baja al primer mosaico; del cuarto mosaico sube al primero.
        assert_eq!(vecina(p, 0, Direccion::Abajo), Some(1));
        assert_eq!(vecina(p, 4, Direccion::Arriba), Some(1));
        assert_eq!(vecina(p, 1, Direccion::Arriba), Some(0));
        assert_eq!(vecina(p, 3, Direccion::Abajo), Some(4));
        assert_eq!(
            vecina(p, 4, Direccion::Abajo),
            Some(5),
            "al título siguiente"
        );
        assert_eq!(vecina(p, 0, Direccion::Izquierda), None);
        assert_eq!(vecina(p, 7, Direccion::Derecha), None);
    }

    #[test]
    fn tamano_fijo_con_cualquier_ancho() {
        for ancho in [200, 248, 300, 520] {
            for t in [Tamano::Chico, Tamano::Mediano, Tamano::Grande] {
                let c = columnas(ancho, t);
                let celda = (ancho - ESPACIO * (c - 1)) / c;
                assert!(celda >= t.lado(), "{ancho} {t:?}");
                assert!(celda < t.lado() * 2 + ESPACIO, "{ancho} {t:?}");
            }
        }
    }
}
