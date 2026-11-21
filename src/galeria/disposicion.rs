//! Dónde va cada foto de la galería (033): cuentas puras, sin GTK, para los tres layouts de
//! grilla. Determinista: el mismo ancho, las mismas proporciones y el mismo orden dan siempre la
//! misma grilla.

use orbital_vault::galeria::Layout;

/// Separación entre celdas, en píxeles lógicos.
pub const HUECO: i32 = 6;
/// Lado buscado de las celdas de `miniaturas`.
pub const LADO: i32 = 150;
/// Ancho buscado de las columnas de `masonry`.
pub const COLUMNA: i32 = 210;
/// Alto de un título de grupo.
pub const TITULO: i32 = 34;

/// Un rectángulo en píxeles lógicos, relativo a la grilla.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl Rect {
    fn centro_x(&self) -> i32 {
        self.x + self.w / 2
    }

    /// Si se cruza con la franja vertical `[a, b)`.
    pub fn en_franja(&self, a: i32, b: i32) -> bool {
        self.y < b && self.y + self.h > a
    }
}

/// La grilla dispuesta: un rectángulo por elemento (en el orden de la página) y uno por título.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Disposicion {
    pub celdas: Vec<Rect>,
    /// (grupo, rectángulo del título).
    pub titulos: Vec<(usize, Rect)>,
    pub alto: i32,
}

/// Lo que hace falta para disponer una página.
pub struct Pedido<'a> {
    pub layout: Layout,
    pub ancho: i32,
    /// Ancho sobre alto de cada elemento de la página (las que no se conocen, `None`: se
    /// dispone hasta la primera que falte).
    pub proporciones: &'a [Option<f32>],
    /// El grupo de cada elemento, si hay títulos.
    pub grupos: Option<&'a [usize]>,
    pub columnas: Option<u32>,
    pub alto_fila: i32,
}

/// Columnas y su ancho para un ancho total y un ancho buscado.
fn columnas(ancho: i32, buscado: i32, fijas: Option<u32>) -> (i32, i32) {
    let n = match fijas {
        Some(n) => n as i32,
        None => ((ancho + HUECO) / (buscado + HUECO)).max(1),
    }
    .max(1);
    let w = ((ancho - HUECO * (n - 1)) / n).max(24);
    (n, w)
}

fn proporcion(p: f32) -> f32 {
    if p.is_finite() && p > 0.0 {
        p.clamp(0.2, 5.0)
    } else {
        1.0
    }
}

/// Dispone una página. Con proporciones que faltan, se detiene antes de la primera (los
/// layouts que las usan): lo dispuesto ya no cambia cuando llegan las demás, salvo la última
/// fila de `justificadas`.
pub fn disponer(p: &Pedido) -> Disposicion {
    let mut d = Disposicion::default();
    let ancho = p.ancho.max(48);
    let total = p.proporciones.len();
    // Los tramos de un mismo grupo (con títulos) o uno solo.
    let mut tramos: Vec<(Option<usize>, std::ops::Range<usize>)> = Vec::new();
    match p.grupos {
        Some(gr) if !gr.is_empty() => {
            let mut a = 0;
            for k in 1..=total {
                if k == total || gr[k] != gr[a] {
                    tramos.push((Some(gr[a]), a..k));
                    a = k;
                }
            }
        }
        _ => tramos.push((None, 0..total)),
    }
    let mut y = 0;
    for (grupo, r) in tramos {
        if let Some(g) = grupo {
            if y > 0 {
                y += HUECO * 2;
            }
            d.titulos.push((
                g,
                Rect {
                    x: 0,
                    y,
                    w: ancho,
                    h: TITULO,
                },
            ));
            y += TITULO;
        }
        let (fin, completo) = match p.layout {
            Layout::Miniaturas | Layout::Carrusel => miniaturas(p, ancho, r.clone(), y, &mut d),
            Layout::Masonry => masonry(p, ancho, r.clone(), y, &mut d),
            Layout::Justificadas => justificadas(p, ancho, r.clone(), y, &mut d),
        };
        y = fin;
        if !completo {
            break;
        }
    }
    d.alto = y;
    d
}

fn miniaturas(
    p: &Pedido,
    ancho: i32,
    r: std::ops::Range<usize>,
    y0: i32,
    d: &mut Disposicion,
) -> (i32, bool) {
    let (n, lado) = columnas(ancho, LADO, p.columnas);
    let cuantos = r.len() as i32;
    for (k, _) in r.enumerate() {
        let (fila, col) = (k as i32 / n, k as i32 % n);
        d.celdas.push(Rect {
            x: col * (lado + HUECO),
            y: y0 + fila * (lado + HUECO),
            w: lado,
            h: lado,
        });
    }
    let filas = (cuantos + n - 1) / n;
    (y0 + (filas * (lado + HUECO) - HUECO).max(0), true)
}

fn masonry(
    p: &Pedido,
    ancho: i32,
    r: std::ops::Range<usize>,
    y0: i32,
    d: &mut Disposicion,
) -> (i32, bool) {
    let (n, w) = columnas(ancho, COLUMNA, p.columnas);
    let mut altos = vec![y0; n as usize];
    for k in r {
        let Some(prop) = p.proporciones[k] else {
            return (altos.iter().copied().max().unwrap_or(y0), false);
        };
        // La columna más baja (la primera si empatan).
        let (c, &top) = altos
            .iter()
            .enumerate()
            .min_by_key(|(i, h)| (**h, *i))
            .unwrap_or((0, &y0));
        let h = ((w as f32) / proporcion(prop)).round().max(16.0) as i32;
        d.celdas.push(Rect {
            x: c as i32 * (w + HUECO),
            y: top,
            w,
            h,
        });
        altos[c] = top + h + HUECO;
    }
    let fin = altos.iter().copied().max().unwrap_or(y0);
    ((fin - HUECO).max(y0), true)
}

fn justificadas(
    p: &Pedido,
    ancho: i32,
    r: std::ops::Range<usize>,
    y0: i32,
    d: &mut Disposicion,
) -> (i32, bool) {
    let alto = p.alto_fila.max(40) as f32;
    let mut y = y0;
    let mut fila: Vec<f32> = Vec::new();
    let mut completo = true;
    let cerrar = |fila: &mut Vec<f32>, y: &mut i32, d: &mut Disposicion, llena: bool| {
        if fila.is_empty() {
            return;
        }
        let huecos = HUECO * (fila.len() as i32 - 1);
        let natural: f32 = fila.iter().map(|r| r * alto).sum();
        // Una fila llena reparte el ancho exacto; la última queda con el alto buscado (salvo que
        // no entre).
        let escala = if llena || natural > (ancho - huecos) as f32 {
            (ancho - huecos) as f32 / natural
        } else {
            1.0
        };
        let h = (alto * escala).round().max(16.0) as i32;
        let mut x = 0;
        let n = fila.len();
        for (i, r) in fila.iter().enumerate() {
            let w = if llena && i + 1 == n {
                // La última de una fila llena toca el borde (sin restos del redondeo).
                (ancho - x).max(8)
            } else {
                (r * alto * escala).round().max(8.0) as i32
            };
            d.celdas.push(Rect { x, y: *y, w, h });
            x += w + HUECO;
        }
        *y += h + HUECO;
        fila.clear();
    };
    for k in r {
        let Some(prop) = p.proporciones[k] else {
            completo = false;
            break;
        };
        fila.push(proporcion(prop));
        let natural: f32 = fila.iter().map(|r| r * alto).sum();
        if natural + (HUECO * (fila.len() as i32 - 1)) as f32 >= ancho as f32 {
            cerrar(&mut fila, &mut y, d, true);
        }
    }
    // La fila sin llenar: la última de la página o la que espera más proporciones.
    cerrar(&mut fila, &mut y, d, false);
    ((y - HUECO).max(y0), completo)
}

/// Hacia dónde se mueve el foco con las flechas.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direccion {
    Izquierda,
    Derecha,
    Arriba,
    Abajo,
}

/// La celda vecina de `actual` en esa dirección: ← y → siguen el orden de la grilla; ↑ y ↓, la
/// más cercana arriba o abajo (con poco corrimiento de costado).
pub fn vecina(celdas: &[Rect], actual: usize, dir: Direccion) -> Option<usize> {
    let a = celdas.get(actual)?;
    match dir {
        Direccion::Izquierda => actual.checked_sub(1),
        Direccion::Derecha => (actual + 1 < celdas.len()).then_some(actual + 1),
        Direccion::Arriba | Direccion::Abajo => {
            let abajo = dir == Direccion::Abajo;
            celdas
                .iter()
                .enumerate()
                .filter(|(_, c)| {
                    if abajo {
                        c.y >= a.y + a.h
                    } else {
                        c.y + c.h <= a.y
                    }
                })
                .min_by_key(|(_, c)| {
                    let dy = if abajo {
                        c.y - (a.y + a.h)
                    } else {
                        a.y - (c.y + c.h)
                    };
                    dy + 3 * (c.centro_x() - a.centro_x()).abs()
                })
                .map(|(k, _)| k)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pedido(layout: Layout, ancho: i32, props: &[Option<f32>]) -> Disposicion {
        disponer(&Pedido {
            layout,
            ancho,
            proporciones: props,
            grupos: None,
            columnas: None,
            alto_fila: 180,
        })
    }

    #[test]
    fn miniaturas_cuadradas_que_llenan_el_ancho() {
        let d = pedido(Layout::Miniaturas, 800, &[None; 11]);
        // (800 + 6) / 156 = 5 columnas de (800 - 24) / 5 = 155.
        assert_eq!(d.celdas.len(), 11);
        assert_eq!(
            d.celdas[0],
            Rect {
                x: 0,
                y: 0,
                w: 155,
                h: 155
            }
        );
        assert_eq!(d.celdas[4].x, 4 * 161);
        assert_eq!(
            d.celdas[5],
            Rect {
                x: 0,
                y: 161,
                w: 155,
                h: 155
            }
        );
        assert_eq!(d.alto, 3 * 161 - 6);
        for c in &d.celdas {
            assert!(c.x + c.w <= 800);
        }
    }

    #[test]
    fn masonry_sin_recorte_en_la_columna_mas_baja() {
        let props = [Some(1.0), Some(2.0), Some(0.5), Some(1.0), Some(2.0)];
        let d = pedido(Layout::Masonry, 640, &props);
        // (640 + 6) / 216 = 2 columnas de 317.
        assert_eq!(
            d.celdas[0],
            Rect {
                x: 0,
                y: 0,
                w: 317,
                h: 317
            }
        );
        assert_eq!(
            d.celdas[1],
            Rect {
                x: 323,
                y: 0,
                w: 317,
                h: 159
            }
        );
        // La tercera va a la columna más baja (la segunda).
        assert_eq!(d.celdas[2].x, 323);
        assert_eq!(d.celdas[2].y, 165);
        assert_eq!(d.celdas[2].h, 634);
        // La proporción se respeta (sin recorte).
        for (c, p) in d.celdas.iter().zip(props) {
            let r = c.w as f32 / c.h as f32;
            assert!((r - p.unwrap()).abs() < 0.02, "{r} {p:?}");
        }
        // Con proporciones que faltan, se dispone hasta la primera.
        let parcial = pedido(Layout::Masonry, 640, &[Some(1.0), None, Some(1.0)]);
        assert_eq!(parcial.celdas.len(), 1);
    }

    #[test]
    fn justificadas_llenan_cada_fila() {
        let props = [Some(1.5); 7];
        let d = pedido(Layout::Justificadas, 1000, &props);
        // 1,5 × 180 = 270 por foto: entran 4 (1080 + 18 ≥ 1000) en la primera fila.
        let fila: Vec<&Rect> = d.celdas.iter().filter(|c| c.y == 0).collect();
        assert_eq!(fila.len(), 4);
        let ultima = fila.last().unwrap();
        assert_eq!(ultima.x + ultima.w, 1000, "toca el borde");
        assert!(fila[0].h < 180, "la fila llena se achica para entrar");
        // La última fila (3 fotos) queda con el alto buscado.
        let resto: Vec<&Rect> = d.celdas.iter().filter(|c| c.y > 0).collect();
        assert_eq!(resto.len(), 3);
        assert_eq!(resto[0].h, 180);
        assert!(resto[2].x + resto[2].w < 1000);
        // Una foto más ancha que la columna entra sola, achicada.
        let ancha = pedido(Layout::Justificadas, 300, &[Some(5.0)]);
        assert_eq!(ancha.celdas[0].w, 300);
        assert_eq!(ancha.celdas[0].h, 60);
    }

    #[test]
    fn titulos_por_grupo_y_determinismo() {
        let props = [Some(1.0); 5];
        let grupos = [0, 0, 1, 1, 1];
        let p = Pedido {
            layout: Layout::Miniaturas,
            ancho: 500,
            proporciones: &props,
            grupos: Some(&grupos),
            columnas: Some(2),
            alto_fila: 180,
        };
        let d = disponer(&p);
        assert_eq!(d.titulos.len(), 2);
        assert_eq!(d.titulos[0].1.y, 0);
        assert_eq!(d.celdas[0].y, TITULO);
        // El segundo grupo empieza en una fila nueva, debajo de su título.
        assert_eq!(d.celdas[2].x, 0);
        assert_eq!(d.celdas[2].y, d.titulos[1].1.y + TITULO);
        assert_eq!(d, disponer(&p), "la misma grilla siempre");
    }

    #[test]
    fn vecinas_con_las_flechas() {
        let d = pedido(Layout::Miniaturas, 800, &[None; 11]);
        assert_eq!(vecina(&d.celdas, 0, Direccion::Izquierda), None);
        assert_eq!(vecina(&d.celdas, 0, Direccion::Derecha), Some(1));
        assert_eq!(vecina(&d.celdas, 1, Direccion::Abajo), Some(6));
        assert_eq!(vecina(&d.celdas, 6, Direccion::Arriba), Some(1));
        assert_eq!(vecina(&d.celdas, 9, Direccion::Abajo), Some(10));
        assert_eq!(vecina(&d.celdas, 10, Direccion::Abajo), None);
        assert_eq!(vecina(&d.celdas, 10, Direccion::Derecha), None);
        let m = pedido(
            Layout::Masonry,
            640,
            &[Some(1.0), Some(2.0), Some(0.5), Some(1.0)],
        );
        // Abajo de la primera (columna izquierda) está la cuarta, en la misma columna.
        assert_eq!(vecina(&m.celdas, 0, Direccion::Abajo), Some(3));
    }
}
