//! Gráficos 2D y polares de Orbital (052), sin GTK.
//!
//! Un bloque ` ```grafico ` se lee ([`leer`]) en un [`Grafico`]: opciones, parámetros con
//! deslizador y series (funciones, polares, paramétricas, implícitas, puntos y datos). Las
//! expresiones aceptan la notación simple y la de LaTeX ([`expr`]). Para una vista, [`muestreo`]
//! da las líneas y los puntos; [`guias`], la grilla, los ejes y los rótulos; [`svg`], el gráfico
//! estático para exportar y para las notas `.html`. Lo usan la Lectura (interactiva), el globito
//! de Código, el editor de ecuaciones, OnStage y exportar.
//!
//! Todo es puro y se puede llamar desde cualquier hilo: las expresiones compiladas no tienen
//! estado y se evalúan sin asignar memoria.

pub mod bloque;
pub mod ejes;
pub mod expr;
pub mod guias;
pub mod html;
pub mod muestreo;
pub mod svg;

pub use bloque::{Color, Estilo, Grafico, Grilla, Parametro, Serie, Tipo, leer};
pub use ejes::Vista;
pub use muestreo::{Trazo, muestrear};

/// Alto de un gráfico si el bloque no pide otro, en píxeles.
pub const ALTO: u32 = 300;

/// Los colores de las series según el fondo.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Paleta {
    /// Sobre la base oscura de Orbital (Lectura, globito, editor, OnStage): tonos claros, ≥ 6:1.
    Oscura,
    /// Sobre papel blanco (exportar) y en las notas `.html` (fondo desconocido): tonos medios
    /// que dan ≥ 4,3:1 sobre blanco y ≥ 3:1 sobre la base oscura.
    Papel,
}

/// Rosa, celeste, amarillo, verde, violeta, naranja, azul, rojo, gris (el orden de
/// [`bloque::NOMBRES_COLORES`]); las series sin color toman los primeros seis en orden.
const OSCURA: [[u8; 3]; 9] = [
    [0xff, 0x8c, 0xc6],
    [0x5c, 0xcf, 0xe6],
    [0xff, 0xd1, 0x66],
    [0x7b, 0xd8, 0x8f],
    [0xb3, 0x92, 0xf0],
    [0xff, 0x9e, 0x64],
    [0x6c, 0xb6, 0xff],
    [0xff, 0x6b, 0x6b],
    [0xa8, 0xa3, 0xad],
];
const PAPEL: [[u8; 3]; 9] = [
    [0xd6, 0x33, 0x6c],
    [0x0b, 0x7f, 0x99],
    [0xa8, 0x6b, 0x00],
    [0x2b, 0x8a, 0x3e],
    [0x70, 0x48, 0xe8],
    [0xd9, 0x48, 0x0f],
    [0x1c, 0x6e, 0xd6],
    [0xd4, 0x2a, 0x2a],
    [0x6c, 0x68, 0x70],
];

/// Las series sin color pedido van en este orden (los más distinguibles primero).
const ORDEN: [usize; 8] = [0, 1, 2, 3, 4, 5, 6, 7];

/// El color de la serie `k`.
pub fn color_de_serie(g: &Grafico, k: usize, paleta: Paleta) -> [u8; 3] {
    let tabla = match paleta {
        Paleta::Oscura => &OSCURA,
        Paleta::Papel => &PAPEL,
    };
    match g.series.get(k).and_then(|s| s.color) {
        Some(Color::Propio(c)) => c,
        Some(Color::Paleta(i)) => tabla[i.min(tabla.len() - 1)],
        None => {
            // Las series sin color siguen el orden saltando los pedidos por otras.
            let pedidos: Vec<usize> = g
                .series
                .iter()
                .filter_map(|s| match s.color {
                    Some(Color::Paleta(i)) => Some(i),
                    _ => None,
                })
                .collect();
            let libres: Vec<usize> = ORDEN
                .iter()
                .copied()
                .filter(|i| !pedidos.contains(i))
                .collect();
            let libres = if libres.is_empty() {
                ORDEN.to_vec()
            } else {
                libres
            };
            let n = g.series[..k].iter().filter(|s| s.color.is_none()).count();
            tabla[libres[n % libres.len()]]
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colores_sin_repetir_los_pedidos() {
        let g = leer("y = x ; rosa\ny = 2x\ny = 3x").unwrap();
        assert_eq!(color_de_serie(&g, 0, Paleta::Oscura), OSCURA[0]);
        assert_eq!(color_de_serie(&g, 1, Paleta::Oscura), OSCURA[1]);
        assert_eq!(color_de_serie(&g, 2, Paleta::Papel), PAPEL[2]);
    }
}
