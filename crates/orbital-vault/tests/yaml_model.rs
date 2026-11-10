//! T013 y T015: lector de YAML (tipos y rangos de líneas) e ida y vuelta con el emisor.

use orbital_vault::props::emit::{self, LineEnding};
use orbital_vault::props::yaml::parse_block;
use orbital_vault::props::{PropValue, find};

fn v(text: &str, key: &str) -> PropValue {
    find(&parse_block(text).unwrap(), key)
        .unwrap()
        .value
        .clone()
}

#[test]
fn infers_types() {
    let text = "\
t: hola
n: 42
f: -3.5e2
b: true
d: 2026-11-09
dt: 2026-06-30T14:00:00Z
l: \"[[Entropía]]\"
p: /fisica/entropia.md
q: \"42\"
nada:
lista: [a, 1, false]
bloque:
  - uno
  - dos
mapa: { by: human:ejemplo, at: 2026-11-09T18:30:00-03:00 }
";
    assert_eq!(v(text, "t"), PropValue::Text("hola".into()));
    assert_eq!(v(text, "n"), PropValue::Number("42".into()));
    assert_eq!(v(text, "f"), PropValue::Number("-3.5e2".into()));
    assert_eq!(v(text, "b"), PropValue::Bool(true));
    assert_eq!(v(text, "d"), PropValue::Date("2026-11-09".into()));
    assert_eq!(
        v(text, "dt"),
        PropValue::DateTime("2026-06-30T14:00:00Z".into())
    );
    assert_eq!(v(text, "l"), PropValue::Link("[[Entropía]]".into()));
    assert_eq!(v(text, "p"), PropValue::Link("/fisica/entropia.md".into()));
    assert_eq!(
        v(text, "q"),
        PropValue::Text("42".into()),
        "entre comillas es texto"
    );
    assert_eq!(v(text, "nada"), PropValue::Null);
    assert_eq!(
        v(text, "lista"),
        PropValue::List(vec![
            PropValue::Text("a".into()),
            PropValue::Number("1".into()),
            PropValue::Bool(false)
        ])
    );
    assert_eq!(
        v(text, "bloque"),
        PropValue::List(vec![
            PropValue::Text("uno".into()),
            PropValue::Text("dos".into())
        ])
    );
    assert_eq!(
        v(text, "mapa"),
        PropValue::Map(vec![
            ("by".into(), PropValue::Text("human:ejemplo".into())),
            (
                "at".into(),
                PropValue::DateTime("2026-11-09T18:30:00-03:00".into())
            ),
        ])
    );
}

#[test]
fn line_ranges_cover_values_and_inner_comments() {
    let text = "\
# comentario inicial
type: atomo
tags:
  # comentario dentro del valor
  - a
  - b

# sobre title
title: Hola
sources:
  - id: x
    resource: /raw/x.pdf
";
    let e = parse_block(text).unwrap();
    let keys: Vec<_> = e
        .iter()
        .map(|p| (p.key.as_str(), p.lines.clone()))
        .collect();
    assert_eq!(
        keys,
        vec![
            ("type", 1..2),
            ("tags", 2..6),
            ("title", 8..9),
            ("sources", 9..12)
        ]
    );
}

#[test]
fn reports_error_line() {
    let err = parse_block("type: atomo\ntags: [a, b\ntitle: x\n").unwrap_err();
    assert!(err.line >= 2, "línea {}", err.line);
    assert!(parse_block("- solo\n- una lista\n").is_err());
}

#[test]
fn empty_blocks() {
    assert!(parse_block("").unwrap().is_empty());
    assert!(parse_block("# solo comentarios\n\n").unwrap().is_empty());
}

#[test]
fn emitter_roundtrip() {
    let values = vec![
        ("type", PropValue::Text("atomo".into())),
        ("title", PropValue::Text("Canción: «sí» # no".into())),
        ("falso", PropValue::Text("true".into())),
        ("fecha_texto", PropValue::Text("2026-11-09".into())),
        ("n", PropValue::Number("7".into())),
        ("d", PropValue::Date("2026-11-09".into())),
        (
            "dt",
            PropValue::DateTime("2026-11-09T18:30:00-03:00".into()),
        ),
        ("enlace", PropValue::Link("[[Entropía]]".into())),
        ("html", PropValue::Text("</script><b>".into())),
        (
            "tags",
            PropValue::List(vec![
                PropValue::Text("a, b".into()),
                PropValue::Text("c".into()),
            ]),
        ),
        (
            "generated",
            PropValue::Map(vec![
                ("by".into(), PropValue::Text("human:ejemplo".into())),
                (
                    "at".into(),
                    PropValue::DateTime("2026-11-09T18:30:00-03:00".into()),
                ),
            ]),
        ),
        ("vacio", PropValue::Null),
    ];
    for eol in [LineEnding::Lf, LineEnding::CrLf] {
        let doc = emit::document(&values, eol);
        assert!(!doc.contains("</"), "nunca escribe </: {doc}");
        let parsed = parse_block(&doc).unwrap();
        let back: Vec<_> = parsed
            .iter()
            .map(|e| (e.key.as_str(), e.value.clone()))
            .collect();
        assert_eq!(back, values);
    }
}
