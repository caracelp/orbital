//! Outliner plegable (028): árbol de plegables, claves, rangos, diferencia y el archivo
//! `.orbital/plegado.yaml` (formato, dañado, conflicto, respaldo y cambios puros).
// Las listas de un solo rango son a propósito: son conjuntos de líneas ocultas.
#![allow(clippy::single_range_in_vec_init)]
mod common;
use common::{TempDir, ctx};
use orbital_vault::plegado::{
    self, Clase, Clave, PLEGADO_PATH, Plegable, Plegado, camino, claves, contienen, diferencia,
    en_linea, fuera_de, ocultas, plegables, resolver,
};
use orbital_vault::{RelPath, VaultError};
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

/// La nota de prueba del quickstart.
const OUTLINE: &str = "---
title: x
---
# Uno
Párrafo.

## Historia
Texto.
- Frutas
  - Manzana
    - Verde
  - Pera

## Ejemplos
uno

## Ejemplos
dos

```
# no es encabezado
- a
  - b
```

> - cita
>   - sub

| a | b |
|---|---|
| 1 | 2 |

- suelto

## Vacío
## Final
";

fn textos<'a>(texto: &'a str, ps: &[Plegable]) -> Vec<(u32, &'a str)> {
    ps.iter()
        .map(|p| (p.linea, &texto[p.texto.clone()]))
        .collect()
}

#[test]
fn que_es_plegable_en_la_nota_de_prueba() {
    let ps = plegables(OUTLINE);
    assert_eq!(
        textos(OUTLINE, &ps),
        [
            (3, "Uno"),
            (6, "Historia"),
            (8, "Frutas"),
            (9, "Manzana"),
            (13, "Ejemplos"),
            (16, "Ejemplos"),
        ],
        "sin el código, la cita, la tabla, `## Vacío`, `## Final` ni el ítem suelto"
    );
    let ultimas: Vec<u32> = ps.iter().map(|p| p.ultima).collect();
    assert_eq!(ultimas, [35, 11, 11, 10, 14, 32]);
    let padres: Vec<Option<u32>> = ps.iter().map(|p| p.padre).collect();
    assert_eq!(padres, [None, Some(0), Some(1), Some(2), Some(0), Some(0)]);
    assert_eq!(ps[0].clase, Clase::Encabezado { nivel: 1 });
    assert_eq!(ps[3].clase, Clase::Item { sangria: 2 });
}

#[test]
fn frontmatter_setext_y_final_sin_salto() {
    assert!(plegables("---\n# a\n- b\n  - c\n---\ntexto\n").is_empty());
    assert!(plegables("Título\n===\ntexto\n").is_empty());
    let ps = plegables("# A\ntexto");
    assert_eq!(ps.len(), 1);
    assert_eq!((ps[0].linea, ps[0].ultima), (0, 1));
    // Un `#` sin espacio no es encabezado; uno con cuatro espacios de sangría tampoco.
    assert!(plegables("#etiqueta\ntexto\n    # código\nmás\n").is_empty());
    // Las líneas vacías del final de la sección no se ocultan.
    let ps = plegables("# A\ntexto\n\n\n# B\n");
    assert_eq!(ps[0].ultima, 1);
}

#[test]
fn items_con_y_sin_subitems() {
    // Líneas vacías intermedias si lo siguiente tiene más sangría.
    let ps = plegables("- a\n\n  - b\n- c\n");
    assert_eq!(
        ps.iter().map(|p| (p.linea, p.ultima)).collect::<Vec<_>>(),
        [(0, 2)]
    );
    // Un hermano corta el bloque aunque haya una línea vacía.
    assert!(plegables("- a\n\n- b\n").is_empty());
    // El párrafo con sangría dentro del ítem se oculta con el bloque; solo texto no basta.
    let ps = plegables("- a\n  - b\n  texto\n");
    assert_eq!((ps[0].linea, ps[0].ultima), (0, 2));
    assert!(plegables("- a\n  texto\n").is_empty());
    // Numerados, tareas, `*` y `+`, con tabs (un tab cuenta 4).
    let t = "1. uno\n   - sub\n* [ ] tarea\n\t+ hija\n";
    let ps = plegables(t);
    assert_eq!(textos(t, &ps), [(0, "uno"), (2, "tarea")]);
    // Un encabezado termina los ítems.
    let ps = plegables("# S\n- a\n  - b\n# T\n");
    assert_eq!(ps.iter().map(|p| p.ultima).collect::<Vec<_>>(), [2, 2]);
    assert_eq!(ps[1].padre, Some(0));
    // Una regla no es un ítem.
    assert!(plegables("* * *\n  - b\n").is_empty());
}

#[test]
fn id_y_texto_del_item() {
    let t = "- uno ^abc123\n  - dos\n- [x] hecha ^z-9\n  - sub\n";
    let ps = plegables(t);
    assert_eq!(&t[ps[0].texto.clone()], "uno");
    assert_eq!(&t[ps[0].id.clone().unwrap()], "abc123");
    assert_eq!(&t[ps[1].texto.clone()], "hecha");
    assert_eq!(&t[ps[1].id.clone().unwrap()], "z-9");
    assert_eq!(
        claves(t, &ps, &[0, 1]),
        [
            Clave::Bloque {
                id: "abc123".into()
            },
            Clave::Bloque { id: "z-9".into() }
        ]
    );
}

#[test]
fn claves_y_resolver() {
    let ps = plegables(OUTLINE);
    let todas: Vec<u32> = (0..ps.len() as u32).collect();
    let c = claves(OUTLINE, &ps, &todas);
    assert_eq!(
        c[5],
        Clave::Encabezado {
            texto: "Ejemplos".into(),
            aparicion: 2
        }
    );
    assert_eq!(
        c[2],
        Clave::Item {
            texto: "Frutas".into(),
            aparicion: 1
        }
    );
    for (i, k) in c.iter().enumerate() {
        assert_eq!(&plegado::clave(OUTLINE, &ps, i), k);
    }
    assert_eq!(resolver(OUTLINE, &ps, &c), todas);
    // Solo el segundo «Ejemplos».
    assert_eq!(resolver(OUTLINE, &ps, &c[5..]), [5]);
    // Una clave perdida no pliega nada; repetida, una vez.
    let perdidas = [
        Clave::Encabezado {
            texto: "Ejemplos".into(),
            aparicion: 3,
        },
        Clave::Bloque { id: "nada".into() },
        Clave::Item {
            texto: "Historia".into(),
            aparicion: 1,
        },
        c[1].clone(),
        c[1].clone(),
    ];
    assert_eq!(resolver(OUTLINE, &ps, &perdidas), [1]);
    // Con el texto del ítem cambiado y su id, se lo sigue encontrando.
    let t = "- viejo ^abc\n  - x\n";
    let k = claves(t, &plegables(t), &[0]);
    let t2 = "- nuevo texto ^abc\n  - x\n";
    assert_eq!(resolver(t2, &plegables(t2), &k), [0]);
}

#[test]
fn en_linea_contienen_y_camino() {
    let ps = plegables(OUTLINE);
    assert_eq!(en_linea(&ps, 8), Some(2));
    assert_eq!(en_linea(&ps, 7), None);
    assert_eq!(contienen(&ps, 10), [0, 1, 2, 3]);
    assert_eq!(contienen(&ps, 8), [0, 1]);
    assert_eq!(contienen(&ps, 12), [0]);
    assert_eq!(contienen(&ps, 3), Vec::<u32>::new());
    assert_eq!(contienen(&ps, 20), [0, 5]);
    assert_eq!(camino(&ps, 3), [0, 1, 2]);
    assert_eq!(camino(&ps, 0), Vec::<u32>::new());
}

#[test]
fn ocultas_fuera_de_y_diferencia() {
    let ps = plegables(OUTLINE);
    assert_eq!(ocultas(&ps, &[3, 1]), [7..12]);
    assert_eq!(ocultas(&ps, &[3, 4]), [10..11, 14..15]);
    assert_eq!(ocultas(&ps, &[]), Vec::<std::ops::Range<u32>>::new());
    assert_eq!(fuera_de(&ps, 2, 36), [0..8, 12..36]);
    // Iguales: nada.
    assert_eq!(diferencia(&[1..3, 5..9], &[1..3, 5..9]), (vec![], vec![]));
    // Disjuntos.
    assert_eq!(diferencia(&[1..3], &[5..9]), (vec![5..9], vec![1..3]));
    // Solapados.
    assert_eq!(diferencia(&[1..6], &[4..9]), (vec![6..9], vec![1..4]));
    // Contenido.
    assert_eq!(diferencia(&[1..10], &[3..5]), (vec![], vec![1..3, 5..10]));
    assert_eq!(diferencia(&[3..5], &[1..10]), (vec![1..3, 5..10], vec![]));
    assert_eq!(
        diferencia(&[0..2, 4..6, 8..10], &[1..9]),
        (vec![2..4, 6..8], vec![0..1, 9..10])
    );
}

/// El ejemplo del contrato (`contracts/formato-plegado.md`).
const EJEMPLO: &str = "format_version: 1
notas:
  - ruta: wiki/entropia.md
    plegados:
      - encabezado: Historia
      - encabezado: Ejemplos
        aparicion: 2
      - bloque: p2m8q4
      - item: Comprar pan
  - ruta: \"wiki/proyectos/tesis: capítulo 2.md\"
    plegados:
      - encabezado: Bibliografía
";

fn r(s: &str) -> RelPath {
    RelPath::new(s).unwrap()
}

fn enc(t: &str) -> Clave {
    Clave::Encabezado {
        texto: t.into(),
        aparicion: 1,
    }
}

#[test]
fn ejemplo_del_contrato_ida_y_vuelta_byte_a_byte() {
    let p = plegado::parse(EJEMPLO).unwrap();
    let mut esperado = BTreeMap::new();
    esperado.insert(
        r("wiki/entropia.md"),
        vec![
            enc("Historia"),
            Clave::Encabezado {
                texto: "Ejemplos".into(),
                aparicion: 2,
            },
            Clave::Bloque {
                id: "p2m8q4".into(),
            },
            Clave::Item {
                texto: "Comprar pan".into(),
                aparicion: 1,
            },
        ],
    );
    esperado.insert(
        r("wiki/proyectos/tesis: capítulo 2.md"),
        vec![enc("Bibliografía")],
    );
    assert_eq!(p.notas, esperado);
    assert_eq!(plegado::serialize(&p), EJEMPLO);
    assert_eq!(
        plegado::serialize(&Plegado::default()),
        "format_version: 1\nnotas: []\n"
    );
    // Un encabezado que parece número sigue siendo texto.
    let p2 = plegado::parse("notas:\n  - ruta: a.md\n    plegados:\n      - encabezado: 2024\n")
        .unwrap();
    assert_eq!(p2.de(&r("a.md")), [enc("2024")]);
    assert!(plegado::serialize(&p2).contains("encabezado: \"2024\""));
}

#[test]
fn vacio_y_casos_de_archivo_danado() {
    assert_eq!(plegado::parse("").unwrap(), Plegado::default());
    assert_eq!(plegado::parse("notas:\n").unwrap(), Plegado::default());
    assert_eq!(plegado::parse("notas: []\n").unwrap(), Plegado::default());
    let malos = [
        "notas: [",
        "otra: 1\n",
        "notas: 3\n",
        "format_version: x\n",
        "notas:\n  - ruta: ../x.md\n    plegados:\n      - item: a\n",
        "notas:\n  - ruta: .oculta/x.md\n    plegados:\n      - item: a\n",
        "notas:\n  - ruta: wiki/x.pdf\n    plegados:\n      - item: a\n",
        "notas:\n  - ruta: a.md\n    plegados:\n      - item: a\n  - ruta: a.md\n    plegados:\n      - item: b\n",
        "notas:\n  - ruta: a.md\n    plegados:\n      - {}\n",
        "notas:\n  - ruta: a.md\n    plegados:\n      - bloque: a\n        item: b\n",
        "notas:\n  - ruta: a.md\n    plegados:\n      - item: a\n        aparicion: x\n",
        "notas:\n  - ruta: a.md\n    plegados:\n      - item: a\n        aparicion: 0\n",
        "notas:\n  - ruta: a.md\n    plegados: []\n",
        "notas:\n  - ruta: a.md\n",
        "notas:\n  - ruta: a.md\n    plegados:\n      - bloque: \"a b\"\n",
        "notas:\n  - ruta: a.md\n    plegados:\n      - otra: a\n",
    ];
    for m in malos {
        assert!(plegado::parse(m).is_err(), "debería estar dañado: {m:?}");
    }
    let nuevo = plegado::parse("format_version: 2\n").unwrap_err();
    assert!(nuevo.contains("versión más nueva"));
}

fn boveda(tag: &str) -> TempDir {
    let tmp = TempDir::new(tag);
    fs::create_dir_all(tmp.path().join("wiki/a")).unwrap();
    fs::write(tmp.path().join("wiki/x.md"), "# X\ntexto\n").unwrap();
    fs::write(tmp.path().join("wiki/a/y.md"), "# Y\ntexto\n").unwrap();
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
        .filter(|e| {
            let n = e.file_name().to_string_lossy().into_owned();
            n.starts_with("plegado") && !n.ends_with(".yaml.yaml")
        })
        .map(|e| fs::read(e.path()).unwrap())
        .collect();
    out.sort();
    out
}

fn uno(ruta: &str, t: &str) -> Plegado {
    let mut p = Plegado::default();
    p.poner(&r(ruta), vec![enc(t)]);
    p
}

#[test]
fn leer_no_crea_orbital_y_guardar_lo_crea() {
    let tmp = boveda("plegado-crear");
    let vault = orbital_vault::open(tmp.path()).unwrap();
    let snap = vault.plegado_snapshot();
    assert_eq!(snap.plegado, Plegado::default());
    assert!(snap.warning.is_none() && snap.hash.is_none() && snap.writable());
    assert!(!tmp.path().join(".orbital").exists());
    // Guardar nada no crea nada.
    let vacio = vault
        .guardar_plegado(&Plegado::default(), &snap, &ctx())
        .unwrap();
    assert!(!tmp.path().join(".orbital").exists());
    let p = uno("wiki/x.md", "X");
    let snap = vault.guardar_plegado(&p, &vacio, &ctx()).unwrap();
    assert!(snap.propio());
    assert_eq!(vault.plegado_snapshot().plegado, p);
    assert!(respaldos(tmp.path()).is_empty());
    assert_eq!(
        fs::read_to_string(tmp.path().join("wiki/x.md")).unwrap(),
        "# X\ntexto\n"
    );
}

#[test]
fn orbital_como_enlace_simbolico_se_rechaza() {
    let tmp = boveda("plegado-symlink");
    let otro = TempDir::new("plegado-symlink-destino");
    std::os::unix::fs::symlink(otro.path(), tmp.path().join(".orbital")).unwrap();
    let vault = orbital_vault::open(tmp.path()).unwrap();
    let snap = vault.plegado_snapshot();
    assert!(snap.warning.is_some() && !snap.writable());
    assert!(
        vault
            .guardar_plegado(&uno("wiki/x.md", "X"), &snap, &ctx())
            .is_err()
    );
    assert!(!otro.path().join("plegado.yaml").exists());
}

#[test]
fn conflicto_si_el_archivo_cambio_afuera() {
    let tmp = boveda("plegado-conflicto");
    let vault = orbital_vault::open(tmp.path()).unwrap();
    let snap = vault.plegado_snapshot();
    let snap = vault
        .guardar_plegado(&uno("wiki/x.md", "X"), &snap, &ctx())
        .unwrap();
    let afuera = "notas: []\n";
    fs::write(tmp.path().join(PLEGADO_PATH), afuera).unwrap();
    assert!(matches!(
        vault.guardar_plegado(&uno("wiki/x.md", "Y"), &snap, &ctx()),
        Err(VaultError::Conflict { .. })
    ));
    assert_eq!(
        fs::read_to_string(tmp.path().join(PLEGADO_PATH)).unwrap(),
        afuera
    );
}

#[test]
fn danado_avisa_queda_intacto_y_respalda_una_vez() {
    let tmp = boveda("plegado-danado");
    fs::create_dir_all(tmp.path().join(".orbital")).unwrap();
    let danado = "notas: [\n";
    fs::write(tmp.path().join(PLEGADO_PATH), danado).unwrap();
    let vault = orbital_vault::open(tmp.path()).unwrap();
    let snap = vault.plegado_snapshot();
    assert!(snap.warning.is_some() && snap.writable());
    assert_eq!(snap.plegado, Plegado::default());
    assert_eq!(
        fs::read_to_string(tmp.path().join(PLEGADO_PATH)).unwrap(),
        danado
    );
    let mut snap = vault
        .guardar_plegado(&uno("wiki/x.md", "X"), &snap, &ctx())
        .unwrap();
    assert_eq!(respaldos(tmp.path()), [danado.as_bytes().to_vec()]);
    // Plegar y desplegar muchas veces en la sesión no deja más copias.
    for t in ["A", "B", "C"] {
        snap = vault
            .guardar_plegado(&uno("wiki/x.md", t), &snap, &ctx())
            .unwrap();
    }
    assert_eq!(respaldos(tmp.path()).len(), 1);
}

#[test]
fn version_nueva_no_se_pisa() {
    let tmp = boveda("plegado-version");
    fs::create_dir_all(tmp.path().join(".orbital")).unwrap();
    fs::write(tmp.path().join(PLEGADO_PATH), "format_version: 2\n").unwrap();
    let vault = orbital_vault::open(tmp.path()).unwrap();
    let snap = vault.plegado_snapshot();
    assert!(snap.warning.is_some() && !snap.writable());
    assert!(
        vault
            .guardar_plegado(&Plegado::default(), &snap, &ctx())
            .is_err()
    );
}

#[test]
fn cambios_puros() {
    let mut p = Plegado::default();
    assert!(p.poner(&r("wiki/a/y.md"), vec![enc("Y")]));
    assert!(!p.poner(&r("wiki/a/y.md"), vec![enc("Y")]));
    assert!(p.poner(&r("wiki/ab.md"), vec![enc("Z")]));
    assert!(p.poner(&r("wiki/x.md"), vec![enc("X")]));
    // Carpeta: `wiki/a/` se mueve y `wiki/ab.md` no.
    assert!(p.renombrar_ruta(&r("wiki/a"), &r("wiki/b")));
    assert_eq!(p.de(&r("wiki/b/y.md")), [enc("Y")]);
    assert!(p.de(&r("wiki/a/y.md")).is_empty());
    assert_eq!(p.de(&r("wiki/ab.md")), [enc("Z")]);
    // Archivo exacto; si el destino tenía estado, gana el de origen.
    assert!(p.renombrar_ruta(&r("wiki/x.md"), &r("wiki/ab.md")));
    assert_eq!(p.de(&r("wiki/ab.md")), [enc("X")]);
    let q = p.quitar_ruta(&r("wiki/b"));
    assert_eq!(q.0.len(), 1);
    assert!(p.de(&r("wiki/b/y.md")).is_empty());
    assert!(p.reponer(&q));
    assert_eq!(p.de(&r("wiki/b/y.md")), [enc("Y")]);
    assert!(!p.reponer(&q));
    assert!(p.poner(&r("wiki/ab.md"), vec![]));
    assert!(!p.poner(&r("wiki/ab.md"), vec![]));
    // Podar contra la bóveda: solo queda lo que existe.
    let tmp = boveda("plegado-podar");
    let vault = orbital_vault::open(tmp.path()).unwrap();
    let mut p = Plegado::default();
    p.poner(&r("wiki/x.md"), vec![enc("X")]);
    p.poner(&r("wiki/borrada.md"), vec![enc("B")]);
    assert!(p.podar(vault.entries()));
    assert_eq!(p.notas.len(), 1);
    assert!(!p.podar(vault.entries()));
}

/// El recorrido de líneas del formato canónico da lo mismo que el analizador de eventos.
#[test]
fn camino_rapido_igual_al_estricto() {
    let mut p = Plegado::default();
    let textos = [
        "Historia",
        "2024",
        "true",
        "a: b",
        "con \"comillas\"",
        "#numeral",
        "- guion",
        "fin:",
        "ñandú ☃",
        "barra\\invertida",
    ];
    for (n, t) in textos.iter().enumerate() {
        p.poner(
            &r(&format!("wiki/n{n}: x.md")),
            vec![
                enc(t),
                Clave::Item {
                    texto: (*t).into(),
                    aparicion: 3,
                },
                Clave::Bloque { id: "a-1".into() },
            ],
        );
    }
    let texto = plegado::serialize(&p);
    assert_eq!(plegado::parse_estricto(&texto).unwrap(), p);
    assert_eq!(plegado::parse(&texto).unwrap(), p);
    assert_eq!(
        plegado::parse(EJEMPLO).unwrap(),
        plegado::parse_estricto(EJEMPLO).unwrap()
    );
    // Lo que no es canónico pasa por el estricto (y da lo mismo, o el mismo error).
    for t in [
        "format_version: 1\nnotas:\n  - ruta: a.md\n    plegados:\n      - item: x\n        aparicion: 1\n",
        "format_version: 1\nnotas:\n  - ruta: 'a.md'\n    plegados:\n      - item: x # comentario\n",
        "format_version: 1\nnotas:\n  - ruta: a.md\n    plegados:\n      - bloque: x\n        aparicion: 2\n",
        "format_version: 1\nnotas:\n  - ruta: a.md\n    plegados:\n",
        "format_version: 1\nnotas:\n  - ruta: a.md\n    plegados:\n      - item: x\n  - ruta: a.md\n    plegados:\n      - item: y\n",
    ] {
        assert_eq!(plegado::parse(t), plegado::parse_estricto(t), "{t:?}");
    }
}
