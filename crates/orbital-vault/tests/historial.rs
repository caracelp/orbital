//! 010: snapshot automático al guardar en .orbital/historial/.

mod common;

use common::TempDir;
use orbital_vault::{RelPath, leer_version, versiones_historial};

fn crear_vault() -> (TempDir, orbital_vault::Vault) {
    let tmp = TempDir::new("historial");
    let vault = orbital_vault::open(tmp.path()).unwrap();
    (tmp, vault)
}

#[test]
fn guardar_leer_y_ordenar() {
    let (_tmp, vault) = crear_vault();
    let ruta = RelPath::new("wiki/sub/nota.md").unwrap();
    assert!(versiones_historial(&vault, &ruta).unwrap().is_empty());
    assert!(orbital_vault::historial::guardar_version(&vault, &ruta, "v1", "").unwrap());
    // Contenido igual: no crea duplicado.
    assert!(!orbital_vault::historial::guardar_version(&vault, &ruta, "v1", "").unwrap());
    assert!(orbital_vault::historial::guardar_version(&vault, &ruta, "v2", "").unwrap());
    let versiones = versiones_historial(&vault, &ruta).unwrap();
    assert_eq!(versiones.len(), 2);
    // Más recientes primero (por fecha de modificación; el sufijo anti-colisión puede romper
    // el orden lexicográfico del nombre).
    assert_eq!(versiones[0].1, "v2");
    assert_eq!(versiones[1].1, "v1");
    // Lectura por nombre.
    assert_eq!(leer_version(&vault, &ruta, &versiones[0].0).unwrap(), "v2");
    // Traversal rechazado.
    assert!(leer_version(&vault, &ruta, "../x").is_err());
}

#[test]
fn limite_de_veinte() {
    let (_tmp, vault) = crear_vault();
    let ruta = RelPath::new("wiki/nota.md").unwrap();
    for n in 0..25 {
        orbital_vault::historial::guardar_version(&vault, &ruta, &format!("v{n}"), "").unwrap();
    }
    let versiones = versiones_historial(&vault, &ruta).unwrap();
    assert_eq!(versiones.len(), 20);
    // Quedan las 20 más nuevas (v5..v24) y se descartaron v0..v4.
    assert_eq!(versiones.last().unwrap().1, "v5");
    assert_eq!(versiones.first().unwrap().1, "v24");
}

/// SC-1: el costo del guardado extra no se tiene que sentir. Mide 40 versiones de una nota
/// de 100 KiB (leer previo + escribir atómica + recorte ya con 20 versiones acumuladas).
#[test]
#[ignore = "medición; tools/verificar-010-us1.sh o manual con --ignored"]
fn costo_de_guardar_version() {
    let (_tmp, vault) = crear_vault();
    let ruta = RelPath::new("wiki/nota.md").unwrap();
    let texto = format!("{}\n", "x".repeat(100 * 1024));
    let mut muestras: Vec<u128> = Vec::new();
    for _ in 0..40 {
        let t0 = std::time::Instant::now();
        orbital_vault::historial::guardar_version(&vault, &ruta, &texto, "").unwrap();
        muestras.push(t0.elapsed().as_micros());
    }
    muestras.sort();
    let mediana = muestras[muestras.len() / 2];
    let peor = muestras[muestras.len() - 1];
    println!("mediana {mediana} µs, peor {peor} µs sobre 40 versiones de 100 KiB");
    // SC-1: la escritura extra atómica es sub-milisegundo.
    assert!(mediana < 1_000, "mediana {mediana} µs supera 1 ms");
}
