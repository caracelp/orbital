use super::*;

const TIPOS: &[(&str, &str)] = &[
    (
        "flowchart",
        "flowchart TD\n    A[Idea en una nota] --> B{¿Tiene enlaces?}\n    B -->|Sí| C[Conectar]\n    B -->|No| D[Etiquetar]\n    C --> E((Listo))\n    D --> E\n",
    ),
    ("graph", "graph LR\n  A --> B"),
    (
        "sequenceDiagram",
        "sequenceDiagram\n    participant U as Usuaria\n    U->>O: Escribe\n    O-->>U: Muestra\n    Note over U,O: ñandú",
    ),
    (
        "classDiagram",
        "classDiagram\n    class Nota {\n        +String titulo\n        +guardar() bool\n    }\n    Nota <|-- NotaHtml",
    ),
    (
        "stateDiagram-v2",
        "stateDiagram-v2\n    [*] --> Cerrada\n    Cerrada --> Abierta : abrir\n    Abierta --> [*]",
    ),
    (
        "erDiagram",
        "erDiagram\n    BOVEDA ||--o{ NOTA : contiene\n    NOTA {\n        string titulo\n    }",
    ),
    (
        "gantt",
        "gantt\n    title Plan\n    dateFormat YYYY-MM-DD\n    section Fórmulas\n    LaTeX :a1, 2026-11-01, 5d\n    Editor :after a1, 4d",
    ),
    (
        "pie",
        "pie title Notas\n    \"Markdown\" : 68\n    \"HTML\" : 22",
    ),
    (
        "mindmap",
        "mindmap\n  root((Orbital))\n    Notas\n      Markdown\n    Vistas",
    ),
    (
        "timeline",
        "timeline\n    title Historia\n    2026-10 : Bóveda : Editor\n    2026-11 : Fórmulas",
    ),
];

#[test]
fn los_nueve_tipos_con_cada_tema() {
    for (tipo_esperado, fuente) in TIPOS {
        assert_eq!(tipo(fuente), Some(*tipo_esperado));
        for tema in [Tema::Oscuro, Tema::Claro, Tema::Html] {
            let d = svg(fuente, tema).unwrap_or_else(|e| panic!("{tipo_esperado}: {e}"));
            assert!(d.marcado.starts_with("<svg"), "{tipo_esperado}");
            assert!(d.marcado.contains("role=\"img\""), "{tipo_esperado}");
            assert!(
                d.marcado.contains("aria-label=\"Diagrama Mermaid: "),
                "{tipo_esperado}"
            );
            assert!(
                d.ancho > 10.0 && d.alto > 10.0,
                "{tipo_esperado}: {}x{}",
                d.ancho,
                d.alto
            );
            assert!(
                !d.marcado.contains("#ECECFF"),
                "{tipo_esperado}: color de mermaid"
            );
            assert!(
                !d.marcado.contains("#8a90dd"),
                "{tipo_esperado}: color de mermaid"
            );
        }
    }
}

#[test]
fn html_usa_el_color_del_texto() {
    let d = svg(TIPOS[0].1, Tema::Html).unwrap();
    assert!(d.marcado.contains("currentColor"));
    let d = svg(TIPOS[0].1, Tema::Oscuro).unwrap();
    assert!(!d.marcado.contains("currentColor"));
    assert!(d.marcado.contains("#FF8CC6"));
}

#[test]
fn errores_con_renglon() {
    let e = svg("flowchart TD\n    A --> B\n    B -->> C[\n", Tema::Oscuro).unwrap_err();
    assert_eq!(e.renglon, Some(3), "{e}");
    assert!(
        e.to_string()
            .starts_with("Error en el diagrama (renglón 3): "),
        "{e}"
    );
    let e = svg("fooDiagram\n  a", Tema::Oscuro).unwrap_err();
    assert!(
        e.mensaje
            .contains("tipo de diagrama desconocido: «fooDiagram"),
        "{e}"
    );
    let e = svg("  \n%% nada\n", Tema::Oscuro).unwrap_err();
    assert_eq!(e.mensaje, "el diagrama está vacío");
}

#[test]
fn tipo_salta_comentarios_y_frontmatter() {
    assert_eq!(
        tipo("---\ntitle: Hola\n---\n%%{init: {}}%%\n%% c\ngraph TD;A-->B"),
        Some("graph")
    );
    assert_eq!(tipo("pie"), Some("pie"));
    assert_eq!(tipo(""), None);
    assert_eq!(nombre_tipo("sequenceDiagram"), "de secuencia");
}

#[test]
fn imagen_con_tope() {
    let f = Fuentes::para("sans-serif");
    let d = svg(TIPOS[0].1, Tema::Oscuro).unwrap();
    let i = imagen(&d, 2.0, &f).unwrap();
    assert_eq!(i.rgba.len(), (i.ancho * i.alto * 4) as usize);
    assert!((i.ancho_logico() - d.ancho).abs() < 1.0);
    // Algo se dibujó (no todo transparente).
    assert!(i.rgba.chunks(4).any(|p| p[3] > 0));
    let i = imagen(&d, 100.0, &f).unwrap();
    assert!(i.ancho <= MAX_LADO && i.alto <= MAX_LADO);
}

/// Entradas rotas no pueden tirar la app: miles de variaciones de cada ejemplo (cortes,
/// caracteres borrados o metidos, renglones repetidos), con letras no ASCII.
#[test]
fn entradas_rotas_no_entran_en_panico() {
    let mut semilla: u64 = 0x5eed_0058;
    let mut azar = move |n: usize| {
        semilla ^= semilla << 13;
        semilla ^= semilla >> 7;
        semilla ^= semilla << 17;
        (semilla % n.max(1) as u64) as usize
    };
    let raros = [
        '[', ']', '{', '}', '(', ')', '"', ':', '|', '-', '>', '<', 'ñ', 'ó', '%', '\n', ';', ' ',
        '#', '1', '&',
    ];
    let vueltas: usize = std::env::var("ORBITAL_VUELTAS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(300);
    let mut panicos = Vec::new();
    for (_, fuente) in TIPOS {
        let chars: Vec<char> = fuente.chars().collect();
        for _ in 0..vueltas {
            let mut v = chars.clone();
            for _ in 0..1 + azar(4) {
                match azar(4) {
                    0 if !v.is_empty() => {
                        let i = azar(v.len());
                        v.remove(i);
                    }
                    1 => {
                        let i = azar(v.len() + 1);
                        v.insert(i, raros[azar(raros.len())]);
                    }
                    2 if !v.is_empty() => v.truncate(azar(v.len())),
                    _ => {
                        let i = azar(v.len() + 1);
                        let j = (i + azar(20)).min(v.len());
                        let trozo: Vec<char> = v[i.min(j)..j].to_vec();
                        let k = azar(v.len() + 1);
                        v.splice(k..k, trozo);
                    }
                }
            }
            let s: String = v.into_iter().collect();
            let r = std::panic::catch_unwind(|| {
                let _ = svg(&s, Tema::Oscuro);
            });
            if r.is_err() {
                panicos.push(s);
            }
        }
    }
    assert!(
        panicos.is_empty(),
        "{} pánicos; el primero:\n{}",
        panicos.len(),
        panicos[0]
    );
}

/// Las secciones de un Gantt alternan tonos del tema: ninguna franja queda con los colores fijos
/// del motor (la blanca al 20 % salía gris, #444, sobre la base oscura), y dos seguidas no son
/// iguales.
#[test]
fn gantt_secciones_con_tonos_del_tema() {
    let fuente = "gantt\n    dateFormat YYYY-MM-DD\n    section Uno\n    A :a1, 2026-03-01, 3d\n    section Dos\n    B :2026-03-02, 3d\n    section Tres\n    C :2026-03-03, 3d\n    section Cuatro\n    D :2026-03-04, 3d\n    section Cinco\n    E :2026-03-05, 3d";
    for tema in [Tema::Oscuro, Tema::Claro, Tema::Html] {
        let d = svg(fuente, tema).unwrap();
        let franjas: Vec<&str> = d
            .marcado
            .split("<rect ")
            .skip(1)
            .filter_map(|r| r.split("/>").next())
            .filter(|r| r.contains("stroke=\"none\"") && r.contains("fill-opacity"))
            .map(|r| {
                r.split("fill=\"")
                    .nth(1)
                    .unwrap()
                    .split('"')
                    .next()
                    .unwrap()
            })
            .collect();
        assert!(franjas.len() >= 5, "{tema:?}: {franjas:?}");
        for f in &franjas {
            let f = f.to_ascii_lowercase();
            assert!(
                !["#ffffff", "#6666ff", "#fff400"].contains(&f.as_str()),
                "{tema:?}: {franjas:?}"
            );
        }
        for par in franjas[..5].windows(2) {
            assert_ne!(par[0], par[1], "{tema:?}: {franjas:?}");
        }
    }
}
