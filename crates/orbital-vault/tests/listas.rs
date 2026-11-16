//! Favoritos y Guardados (027): formato, archivo dañado, conflictos, respaldo y cambios puros.
mod common;
use common::{TempDir, ctx};
use orbital_vault::listas::{self, ErrorNombre, LISTAS_PATH, Lista, faltantes};
use orbital_vault::{Coleccion, Listas, RelPath, VaultError};
use std::fs;
use std::path::Path;

/// El ejemplo del contrato (`contracts/formato-listas.md`).
const EJEMPLO: &str = "format_version: 1
favoritos:
  - wiki/entropia.md
  - raw/callen-termodinamica.pdf
colecciones:
  - nombre: Para leer
    notas:
      - wiki/segundo-principio.html
      - wiki/entropia.md
  - nombre: \"Tesis: capítulo 2\"
    notas: []
";

fn r(s: &str) -> RelPath {
    RelPath::new(s).unwrap()
}

fn boveda(tag: &str) -> TempDir {
    let tmp = TempDir::new(tag);
    fs::create_dir_all(tmp.path().join("wiki")).unwrap();
    fs::create_dir_all(tmp.path().join("raw")).unwrap();
    fs::write(tmp.path().join("wiki/a.md"), "Nota a\n").unwrap();
    fs::write(tmp.path().join("wiki/b.md"), "Nota b\n").unwrap();
    tmp
}

fn respaldos(root: &Path) -> Vec<Vec<u8>> {
    let dir = root
        .join(".orbital/respaldo")
        .join(ctx().date.to_string())
        .join(".orbital");
    let Ok(rd) = fs::read_dir(dir) else {
        return vec![];
    };
    let mut out: Vec<_> = rd
        .filter_map(Result::ok)
        .filter(|e| e.path().extension().is_some_and(|x| x == "yaml"))
        .filter(|e| !e.path().to_string_lossy().ends_with(".yaml.yaml"))
        .map(|e| fs::read(e.path()).unwrap())
        .collect();
    out.sort();
    out
}

#[test]
fn ejemplo_del_contrato_ida_y_vuelta_byte_a_byte() {
    let l = listas::parse(EJEMPLO).unwrap();
    assert_eq!(
        l,
        Listas {
            favoritos: vec![r("wiki/entropia.md"), r("raw/callen-termodinamica.pdf")],
            colecciones: vec![
                Coleccion {
                    nombre: "Para leer".into(),
                    notas: vec![r("wiki/segundo-principio.html"), r("wiki/entropia.md")],
                },
                Coleccion {
                    nombre: "Tesis: capítulo 2".into(),
                    notas: vec![],
                },
            ],
        }
    );
    assert_eq!(listas::serialize(&l), EJEMPLO);
    assert_eq!(
        listas::serialize(&Listas::default()),
        "format_version: 1\nfavoritos: []\ncolecciones: []\n"
    );
}

#[test]
fn vacio_ausente_y_claves_que_faltan() {
    for texto in [
        "",
        "\n",
        "# solo un comentario\n",
        "~\n",
        "format_version: 1\n",
    ] {
        assert_eq!(
            listas::parse(texto).unwrap(),
            Listas::default(),
            "{texto:?}"
        );
    }
    let solo = listas::parse("favoritos:\n  - wiki/a.md\n").unwrap();
    assert_eq!(solo.favoritos, [r("wiki/a.md")]);
    assert!(solo.colecciones.is_empty());
    // Comillas y nombres raros sobreviven la ida y vuelta.
    let raro = Listas {
        favoritos: vec![
            r("wiki/con: dos puntos.md"),
            r("wiki/123"),
            r("wiki/canción #1.md"),
        ],
        colecciones: vec![Coleccion {
            nombre: "true".into(),
            notas: vec![r("wiki/a.md")],
        }],
    };
    assert_eq!(listas::parse(&listas::serialize(&raro)).unwrap(), raro);
}

#[test]
fn casos_de_archivo_danado() {
    for texto in [
        "favoritos: [",                                                       // YAML inválido
        "favoritos: []\notra: 1\n",                                           // clave desconocida
        "favoritos:\n  - ../x.md\n",                                          // ruta con ..
        "favoritos:\n  - wiki/.oculta.md\n",                                  // ruta oculta
        "favoritos:\n  - /abs.md\n",                                          // ruta absoluta
        "colecciones:\n  - nombre: \"  \"\n    notas: []\n",                  // nombre vacío
        "colecciones:\n  - notas: []\n",                                      // sin nombre
        "colecciones:\n  - nombre: Para leer\n  - nombre: \" para LEER \"\n", // repetidos
        "format_version: 2\n",                                                // versión nueva
        "format_version: uno\n",
        "- wiki/a.md\n",                  // no es un mapa
        "favoritos: wiki/a.md\n",         // no es lista
        "favoritos: []\nfavoritos: []\n", // clave repetida
        "colecciones:\n  - nombre: X\n    notas: []\n    extra: 1\n",
        "favoritos:\n  - &a wiki/a.md\n", // ancla
    ] {
        assert!(
            listas::parse(texto).is_err(),
            "debería ser dañado: {texto:?}"
        );
    }
}

#[test]
fn repetidos_dentro_de_una_lista_se_toman_una_vez() {
    let l = listas::parse(
        "favoritos:\n  - wiki/a.md\n  - wiki/b.md\n  - wiki/a.md\ncolecciones:\n  - nombre: X\n    notas: [wiki/b.md, wiki/b.md]\n",
    )
    .unwrap();
    assert_eq!(l.favoritos, [r("wiki/a.md"), r("wiki/b.md")]);
    assert_eq!(l.colecciones[0].notas, [r("wiki/b.md")]);
}

#[test]
fn leer_no_crea_orbital_y_guardar_lo_crea() {
    let tmp = boveda("listas-crear");
    let vault = orbital_vault::open(tmp.path()).unwrap();
    let snap = vault.listas_snapshot();
    assert_eq!(snap.listas, Listas::default());
    assert!(snap.warning.is_none() && snap.hash.is_none() && snap.writable());
    assert!(
        !tmp.path().join(".orbital").exists(),
        "leer no crea .orbital/"
    );
    let mut l = Listas::default();
    l.alternar_favorito(&r("wiki/a.md"));
    let guardado = vault.guardar_listas(&l, &snap, &ctx()).unwrap();
    assert!(guardado.propio());
    assert_eq!(vault.listas_snapshot().listas, l);
    assert!(
        respaldos(tmp.path()).is_empty(),
        "un archivo nuevo no se respalda"
    );
    assert_eq!(
        fs::read_to_string(tmp.path().join("wiki/a.md")).unwrap(),
        "Nota a\n"
    );
}

#[test]
fn orbital_como_enlace_simbolico_se_rechaza() {
    let tmp = boveda("listas-symlink");
    let otro = TempDir::new("listas-symlink-destino");
    std::os::unix::fs::symlink(otro.path(), tmp.path().join(".orbital")).unwrap();
    let vault = orbital_vault::open(tmp.path()).unwrap();
    let snap = vault.listas_snapshot();
    assert!(snap.warning.is_some());
    assert!(!snap.writable());
    let mut l = Listas::default();
    l.alternar_favorito(&r("wiki/a.md"));
    assert!(vault.guardar_listas(&l, &snap, &ctx()).is_err());
    assert!(!otro.path().join("listas.yaml").exists());
}

#[test]
fn conflicto_si_el_archivo_cambio_afuera() {
    let tmp = boveda("listas-conflicto");
    let vault = orbital_vault::open(tmp.path()).unwrap();
    let snap = vault.listas_snapshot();
    let mut l = Listas::default();
    l.alternar_favorito(&r("wiki/a.md"));
    let snap = vault.guardar_listas(&l, &snap, &ctx()).unwrap();
    fs::write(tmp.path().join(LISTAS_PATH), "favoritos:\n  - wiki/b.md\n").unwrap();
    l.alternar_favorito(&r("wiki/b.md"));
    assert!(matches!(
        vault.guardar_listas(&l, &snap, &ctx()),
        Err(VaultError::Conflict { .. })
    ));
    assert_eq!(
        fs::read_to_string(tmp.path().join(LISTAS_PATH)).unwrap(),
        "favoritos:\n  - wiki/b.md\n",
        "el cambio de afuera no se pisa"
    );
}

/// Decisión 4 (cambiada): se respalda solo lo que no escribió esta sesión.
#[test]
fn respaldo_solo_de_lo_que_no_escribio_esta_sesion() {
    let tmp = boveda("listas-respaldo");
    fs::create_dir_all(tmp.path().join(".orbital")).unwrap();
    let externo = "favoritos:\n  - wiki/b.md\n";
    fs::write(tmp.path().join(LISTAS_PATH), externo).unwrap();
    let vault = orbital_vault::open(tmp.path()).unwrap();
    let snap = vault.listas_snapshot();
    assert!(!snap.propio());
    // 1. Primera escritura de la sesión sobre un archivo que ya existía: se respalda.
    let mut l = snap.listas.clone();
    l.alternar_favorito(&r("wiki/a.md"));
    let snap = vault.guardar_listas(&l, &snap, &ctx()).unwrap();
    assert!(snap.propio());
    assert_eq!(respaldos(tmp.path()), [externo.as_bytes().to_vec()]);
    // 2. Las siguientes (reordenar, quitar) no dejan más copias.
    let mut snap = snap;
    for _ in 0..5 {
        l.mover(Lista::Favoritos, 0, 1);
        snap = vault.guardar_listas(&l, &snap, &ctx()).unwrap();
    }
    assert_eq!(
        respaldos(tmp.path()).len(),
        1,
        "reordenar no llena el respaldo"
    );
    // 3. Un cambio hecho afuera: el conflicto obliga a releer, y esa escritura sí respalda.
    let afuera = "favoritos:\n  - wiki/a.md\n";
    fs::write(tmp.path().join(LISTAS_PATH), afuera).unwrap();
    assert!(vault.guardar_listas(&l, &snap, &ctx()).is_err());
    let releido = vault.listas_snapshot();
    assert!(!releido.propio());
    let mut l2 = releido.listas.clone();
    l2.alternar_favorito(&r("wiki/b.md"));
    vault.guardar_listas(&l2, &releido, &ctx()).unwrap();
    assert_eq!(respaldos(tmp.path()).len(), 2);
    assert!(respaldos(tmp.path()).contains(&afuera.as_bytes().to_vec()));
}

#[test]
fn danado_se_avisa_queda_intacto_y_el_primer_cambio_lo_respalda() {
    let tmp = boveda("listas-danado");
    fs::create_dir_all(tmp.path().join(".orbital")).unwrap();
    let danado = "favoritos: [\n";
    fs::write(tmp.path().join(LISTAS_PATH), danado).unwrap();
    let vault = orbital_vault::open(tmp.path()).unwrap();
    let snap = vault.listas_snapshot();
    assert!(snap.warning.is_some());
    assert_eq!(snap.listas, Listas::default());
    assert!(snap.writable(), "dañado sigue escribible (FR-011)");
    assert_eq!(
        fs::read_to_string(tmp.path().join(LISTAS_PATH)).unwrap(),
        danado
    );
    let mut l = Listas::default();
    l.alternar_favorito(&r("wiki/a.md"));
    vault.guardar_listas(&l, &snap, &ctx()).unwrap();
    assert_eq!(respaldos(tmp.path()), [danado.as_bytes().to_vec()]);
    assert_eq!(vault.listas_snapshot().listas, l);
}

#[test]
fn version_nueva_no_se_pisa() {
    let tmp = boveda("listas-version");
    fs::create_dir_all(tmp.path().join(".orbital")).unwrap();
    fs::write(tmp.path().join(LISTAS_PATH), "format_version: 2\n").unwrap();
    let vault = orbital_vault::open(tmp.path()).unwrap();
    let snap = vault.listas_snapshot();
    assert!(snap.warning.is_some() && !snap.writable());
    assert!(
        vault
            .guardar_listas(&Listas::default(), &snap, &ctx())
            .is_err()
    );
}

#[test]
fn cambios_puros() {
    let (a, b, c) = (r("wiki/a.md"), r("wiki/b.md"), r("raw/c.pdf"));
    let mut l = Listas::default();
    assert!(l.alternar_favorito(&a) && l.alternar_favorito(&b));
    assert!(l.es_favorito(&a));
    assert!(l.alternar_favorito(&a));
    assert!(!l.es_favorito(&a));
    assert_eq!(l.favoritos, std::slice::from_ref(&b));

    assert_eq!(l.crear_coleccion("Para leer", Some(a.clone())), Ok(true));
    assert_eq!(
        l.crear_coleccion(" para LEER ", None),
        Err(ErrorNombre::Repetido)
    );
    assert_eq!(l.crear_coleccion("   ", None), Err(ErrorNombre::Vacio));
    assert_eq!(l.crear_coleccion("  Tesis  ", None), Ok(true));
    assert_eq!(l.colecciones[1].nombre, "Tesis");
    assert_eq!(l.buscar_coleccion("tesis"), Some(1));
    assert!(l.poner_en(1, &a, true));
    assert!(!l.poner_en(1, &a, true), "una sola vez por lista");
    assert_eq!(l.colecciones_de(&a), 2);
    assert!(l.poner_en(0, &a, false));
    assert_eq!(l.colecciones_de(&a), 1);
    assert!(
        l.colecciones[0].notas.is_empty(),
        "la colección vacía se conserva"
    );
    assert!(!l.poner_en(9, &a, true));

    assert_eq!(l.renombrar_coleccion(1, "Lectura"), Ok(true));
    assert_eq!(
        l.renombrar_coleccion(0, "lectura"),
        Err(ErrorNombre::Repetido)
    );
    assert_eq!(l.renombrar_coleccion(1, "Lectura"), Ok(false));
    assert_eq!(
        l.renombrar_coleccion(1, "LECTURA"),
        Ok(true),
        "cambiar mayúsculas"
    );
    assert_eq!(l.renombrar_coleccion(1, ""), Err(ErrorNombre::Vacio));
    assert_eq!(
        l.colecciones[1].notas,
        std::slice::from_ref(&a),
        "renombrar conserva las notas"
    );

    l.poner_en(1, &b, true);
    l.poner_en(1, &c, true);
    assert!(l.mover(Lista::Coleccion(1), 2, 0));
    assert_eq!(l.colecciones[1].notas, [c.clone(), a.clone(), b.clone()]);
    assert!(
        l.mover(Lista::Coleccion(1), 0, 99),
        "más allá del final = al final"
    );
    assert_eq!(l.colecciones[1].notas, [a.clone(), b.clone(), c.clone()]);
    assert!(!l.mover(Lista::Coleccion(1), 1, 1));
    assert!(!l.mover(Lista::Coleccion(7), 0, 1));
    assert!(l.quitar(Lista::Coleccion(1), 1));
    assert_eq!(l.colecciones[1].notas, [a.clone(), c.clone()]);
    assert!(!l.quitar(Lista::Favoritos, 5));
    assert_eq!(
        l.favoritos,
        std::slice::from_ref(&b),
        "quitar de una colección no toca Favoritos"
    );

    assert!(l.borrar_coleccion(0));
    assert!(!l.borrar_coleccion(5));
    assert_eq!(l.colecciones.len(), 1);
}

#[test]
fn renombrar_ruta_de_archivo_y_de_carpeta() {
    let mut l = Listas {
        favoritos: vec![
            r("wiki/a/x.md"),
            r("wiki/ab.md"),
            r("wiki/a/sub/y.md"),
            r("wiki/z.md"),
        ],
        colecciones: vec![Coleccion {
            nombre: "C".into(),
            notas: vec![r("wiki/z.md"), r("wiki/a/x.md")],
        }],
    };
    assert!(l.renombrar_ruta(&r("wiki/a"), &r("wiki/b")));
    assert_eq!(
        l.favoritos,
        [
            r("wiki/b/x.md"),
            r("wiki/ab.md"),
            r("wiki/b/sub/y.md"),
            r("wiki/z.md")
        ],
        "wiki/ab.md no está dentro de wiki/a/"
    );
    assert_eq!(l.colecciones[0].notas, [r("wiki/z.md"), r("wiki/b/x.md")]);
    assert!(l.renombrar_ruta(&r("wiki/z.md"), &r("wiki/otra/z2.md")));
    assert_eq!(l.favoritos[3], r("wiki/otra/z2.md"));
    assert_eq!(l.colecciones[0].notas[0], r("wiki/otra/z2.md"));
    assert!(!l.renombrar_ruta(&r("wiki/nada.md"), &r("wiki/x.md")));
    // Si la ruta nueva ya estaba, queda una sola, en la posición de la primera.
    assert!(l.renombrar_ruta(&r("wiki/ab.md"), &r("wiki/b/x.md")));
    assert_eq!(
        l.favoritos,
        [r("wiki/b/x.md"), r("wiki/b/sub/y.md"), r("wiki/otra/z2.md")]
    );
}

#[test]
fn quitar_ruta_y_reponer_en_las_mismas_posiciones() {
    let original = Listas {
        favoritos: vec![
            r("wiki/a.md"),
            r("wiki/d/1.md"),
            r("wiki/b.md"),
            r("wiki/d/2.md"),
        ],
        colecciones: vec![
            Coleccion {
                nombre: "X".into(),
                notas: vec![r("wiki/b.md"), r("wiki/a.md")],
            },
            Coleccion {
                nombre: "Y".into(),
                notas: vec![r("wiki/a.md")],
            },
        ],
    };
    let mut l = original.clone();
    let q = l.quitar_ruta(&r("wiki/a.md"));
    assert_eq!(q.0.len(), 3);
    assert_eq!(
        l.favoritos,
        [r("wiki/d/1.md"), r("wiki/b.md"), r("wiki/d/2.md")]
    );
    assert!(l.colecciones[1].notas.is_empty());
    assert!(l.reponer(&q));
    assert_eq!(l, original);
    assert!(!l.reponer(&q), "lo que ya está no se repite");
    // Carpeta: todo lo de adentro, en sus posiciones.
    let q = l.quitar_ruta(&r("wiki/d"));
    assert_eq!(l.favoritos, [r("wiki/a.md"), r("wiki/b.md")]);
    assert!(l.reponer(&q));
    assert_eq!(l, original);
    assert!(l.quitar_ruta(&r("wiki/nada.md")).is_empty());
}

#[test]
fn faltantes_por_busqueda_binaria() {
    let tmp = boveda("listas-faltantes");
    fs::write(tmp.path().join("raw/c.pdf"), b"%PDF").unwrap();
    let vault = orbital_vault::open(tmp.path()).unwrap();
    let l = Listas {
        favoritos: vec![r("wiki/a.md"), r("wiki/no.md"), r("raw/c.pdf")],
        colecciones: vec![Coleccion {
            nombre: "X".into(),
            notas: vec![r("wiki/b.md"), r("raw/borrado.pdf")],
        }],
    };
    let f = faltantes(&l, vault.entries());
    assert_eq!(f.len(), 2);
    assert!(f.contains(&r("wiki/no.md")) && f.contains(&r("raw/borrado.pdf")));
}
