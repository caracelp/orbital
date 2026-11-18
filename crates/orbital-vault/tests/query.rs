use orbital_vault::query::{Atom, parse_query};

#[test]
fn query_words_and_phrases() {
    let q = parse_query("palabra1 \"frase exacta\" palabra2").unwrap();
    assert_eq!(q.any_of.len(), 1);
    let and_group = &q.any_of[0];
    assert_eq!(and_group.len(), 3);

    assert!(!and_group[0].negated);
    assert_eq!(and_group[0].atom, Atom::Word("palabra1".to_string()));

    assert!(!and_group[1].negated);
    assert_eq!(and_group[1].atom, Atom::Phrase("frase exacta".to_string()));

    assert!(!and_group[2].negated);
    assert_eq!(and_group[2].atom, Atom::Word("palabra2".to_string()));
}

#[test]
fn tag_literals_round_trip_without_becoming_operators() {
    use orbital_vault::query::{Atom, tag_query};
    for name in [
        "física/hija",
        "con espacios",
        "a OR b",
        "a(b)",
        "a]b",
        "a\"b",
        "a\\b",
    ] {
        let query = parse_query(&tag_query(name)).unwrap();
        assert_eq!(query.any_of.len(), 1);
        assert_eq!(query.any_of[0].len(), 1);
        assert_eq!(query.any_of[0][0].atom, Atom::Tag(name.into()));
    }
    for input in ["tag:\"sin cerrar", "tag:\"escape\\"] {
        let error = parse_query(input).unwrap_err();
        assert_eq!(error.at, 4);
        assert!(error.message.contains("Comillas sin cerrar"));
    }
}

#[test]
fn query_negation() {
    let q = parse_query("-excluir -\"no incluir\" -tag:#viejo").unwrap();
    assert_eq!(q.any_of.len(), 1);
    let and_group = &q.any_of[0];
    assert_eq!(and_group.len(), 3);

    assert!(and_group[0].negated);
    assert_eq!(and_group[0].atom, Atom::Word("excluir".to_string()));

    assert!(and_group[1].negated);
    assert_eq!(and_group[1].atom, Atom::Phrase("no incluir".to_string()));

    assert!(and_group[2].negated);
    assert_eq!(and_group[2].atom, Atom::Tag("viejo".to_string()));
}

#[test]
fn query_or_alternatives() {
    let q = parse_query("a b OR c d OR e").unwrap();
    assert_eq!(q.any_of.len(), 3);

    assert_eq!(q.any_of[0].len(), 2);
    assert_eq!(q.any_of[0][0].atom, Atom::Word("a".to_string()));
    assert_eq!(q.any_of[0][1].atom, Atom::Word("b".to_string()));

    assert_eq!(q.any_of[1].len(), 2);
    assert_eq!(q.any_of[1][0].atom, Atom::Word("c".to_string()));
    assert_eq!(q.any_of[1][1].atom, Atom::Word("d".to_string()));

    assert_eq!(q.any_of[2].len(), 1);
    assert_eq!(q.any_of[2][0].atom, Atom::Word("e".to_string()));
}

#[test]
fn query_parentheses_and_groups() {
    let q = parse_query("(a OR b) c").unwrap();
    assert_eq!(q.any_of.len(), 1);
    let and_group = &q.any_of[0];
    assert_eq!(and_group.len(), 2);

    match &and_group[0].atom {
        Atom::Group(inner) => {
            assert_eq!(inner.any_of.len(), 2);
            assert_eq!(inner.any_of[0][0].atom, Atom::Word("a".to_string()));
            assert_eq!(inner.any_of[1][0].atom, Atom::Word("b".to_string()));
        }
        other => panic!("Esperaba Group, obtuve {other:?}"),
    }

    assert_eq!(and_group[1].atom, Atom::Word("c".to_string()));
}

#[test]
fn query_specifiers() {
    let q = parse_query("tag:#fisica/mecanica path:proyectos file:nota tag:astronomia").unwrap();
    let and_group = &q.any_of[0];
    assert_eq!(and_group.len(), 4);

    assert_eq!(and_group[0].atom, Atom::Tag("fisica/mecanica".to_string()));
    assert_eq!(and_group[1].atom, Atom::Path("proyectos".to_string()));
    assert_eq!(and_group[2].atom, Atom::File("nota".to_string()));
    assert_eq!(and_group[3].atom, Atom::Tag("astronomia".to_string()));
}

#[test]
fn query_line_and_section() {
    let q = parse_query("line:(a b) section:(c OR d)").unwrap();
    let and_group = &q.any_of[0];
    assert_eq!(and_group.len(), 2);

    match &and_group[0].atom {
        Atom::Line(inner) => {
            assert_eq!(inner.any_of.len(), 1);
            assert_eq!(inner.any_of[0].len(), 2);
        }
        other => panic!("Esperaba Line, obtuve {other:?}"),
    }

    match &and_group[1].atom {
        Atom::Section(inner) => {
            assert_eq!(inner.any_of.len(), 2);
        }
        other => panic!("Esperaba Section, obtuve {other:?}"),
    }
}

#[test]
fn query_properties() {
    let q = parse_query("[estado] [prioridad:alta]").unwrap();
    let and_group = &q.any_of[0];
    assert_eq!(and_group.len(), 2);

    assert_eq!(
        and_group[0].atom,
        Atom::Prop {
            key: "estado".to_string(),
            value: None,
        }
    );
    assert_eq!(
        and_group[1].atom,
        Atom::Prop {
            key: "prioridad".to_string(),
            value: Some("alta".to_string()),
        }
    );
}

#[test]
fn query_regex_like_is_plain_text() {
    // FR-008: sin regex
    let q = parse_query("/regex/ a.*b").unwrap();
    let and_group = &q.any_of[0];
    assert_eq!(and_group.len(), 2);
    assert_eq!(and_group[0].atom, Atom::Word("/regex/".to_string()));
    assert_eq!(and_group[1].atom, Atom::Word("a.*b".to_string()));
}

#[test]
fn query_errors_with_position() {
    // Consulta vacía
    let err = parse_query("   ").unwrap_err();
    assert_eq!(err.at, 0);
    assert!(err.message.to_lowercase().contains("vacía"));

    // Comillas sin cerrar
    let err = parse_query("palabra \"frase sin cerrar").unwrap_err();
    assert_eq!(err.at, 8);
    assert!(err.message.to_lowercase().contains("comilla"));

    // Paréntesis sin cerrar
    let err = parse_query("(a OR b").unwrap_err();
    assert_eq!(err.at, 0);
    assert!(err.message.to_lowercase().contains("paréntesis"));

    // Corchete sin cerrar
    let err = parse_query("[clave:valor").unwrap_err();
    assert_eq!(err.at, 0);
    assert!(err.message.to_lowercase().contains("corchete"));

    // Operador desconocido
    let err = parse_query("desconocido:valor").unwrap_err();
    assert_eq!(err.at, 0);
    assert!(err.message.to_lowercase().contains("desconocido"));

    // Grupo vacío
    let err = parse_query("()").unwrap_err();
    assert!(err.message.to_lowercase().contains("vacía"));

    // Un grupo abierto al final: el error va donde termina, no al principio.
    let err = parse_query("tag:#a (").unwrap_err();
    assert_eq!(err.at, 8);
    assert!(err.message.to_lowercase().contains("paréntesis"));
}
