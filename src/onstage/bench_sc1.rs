use super::*;

/// SC-1: el análisis de una nota de ~100 KiB con 40 diapositivas es lineal y queda por
/// debajo del cuadro de 16 ms (corre en el hilo de fondo). La parte de hilo principal por
/// refresco es el volcado de la diapositiva, acotado por tramos de 8 ms (`SLICE` en
/// editor/reader.rs); el peor caso por diapositiva se mide acá con `Prepared::new`.
#[test]
fn sc1_split_y_preparacion_en_budget() {
    let mut nota = String::from("---\ntype: atomo\ntitle: Grande\n---\n");
    for n in 0..40 {
        nota.push_str(&format!("\n## Diapositiva {}\n\n", n));
        for p in 0..25 {
            nota.push_str(&format!(
                "Párrafo {p} con [[enlace {n}]] y #etiqueta, texto de relleno para llegar \
                 al tamaño de una nota real con muchas diapositivas.\n\n"
            ));
        }
    }
    assert!(nota.len() > 100_000, "{} bytes", nota.len());
    let t0 = std::time::Instant::now();
    let diapositivas = dividir_diapositivas(&nota);
    let split = t0.elapsed();
    assert_eq!(diapositivas.len(), 40);
    println!(
        "SC-1 dividir_diapositivas en {} KiB, 40 diapositivas: {:.2} ms",
        nota.len() / 1024,
        split.as_secs_f64() * 1000.0
    );
    let (mas_larga, largo) = diapositivas
        .iter()
        .map(|d| (d, d.texto.len()))
        .fold((&diapositivas[0], 0), |a, b| if b.1 > a.1 { b } else { a });
    let t1 = std::time::Instant::now();
    let _prepared = Prepared::new(&mas_larga.texto, &[]);
    let prep = t1.elapsed();
    println!(
        "SC-1 Prepared::new de la diapositiva más larga ({} KiB): {:.2} ms",
        largo / 1024,
        prep.as_secs_f64() * 1000.0
    );
    assert!(
        split.as_millis() < 16,
        "split {:?} fuera de presupuesto",
        split
    );
    assert!(
        prep.as_millis() < 16,
        "prepared {:?} fuera de presupuesto",
        prep
    );
}
