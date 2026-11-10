//! T029: tests de diff_lines (data-model de la 002).

use orbital_vault::diff::{DiffKind, DiffLine, diff_lines};

#[test]
fn empty_inputs() {
    assert_eq!(diff_lines("", ""), vec![]);
    assert_eq!(
        diff_lines("", "hola\n"),
        vec![DiffLine {
            kind: DiffKind::Added,
            left: None,
            right: Some(0),
            text: "hola".into(),
        }]
    );
    assert_eq!(
        diff_lines("chau\n", ""),
        vec![DiffLine {
            kind: DiffKind::Removed,
            left: Some(0),
            right: None,
            text: "chau".into(),
        }]
    );
}

#[test]
fn same_lines() {
    let text = "primera línea\nsegunda línea\ntercera línea\n";
    let res = diff_lines(text, text);
    assert_eq!(res.len(), 3);
    assert_eq!(
        res[0],
        DiffLine {
            kind: DiffKind::Same,
            left: Some(0),
            right: Some(0),
            text: "primera línea".into(),
        }
    );
    assert_eq!(
        res[1],
        DiffLine {
            kind: DiffKind::Same,
            left: Some(1),
            right: Some(1),
            text: "segunda línea".into(),
        }
    );
    assert_eq!(
        res[2],
        DiffLine {
            kind: DiffKind::Same,
            left: Some(2),
            right: Some(2),
            text: "tercera línea".into(),
        }
    );
}

#[test]
fn added_lines() {
    let left = "uno\ntres\n";
    let right = "uno\ndos\ntres\n";
    let res = diff_lines(left, right);
    assert_eq!(
        res,
        vec![
            DiffLine {
                kind: DiffKind::Same,
                left: Some(0),
                right: Some(0),
                text: "uno".into(),
            },
            DiffLine {
                kind: DiffKind::Added,
                left: None,
                right: Some(1),
                text: "dos".into(),
            },
            DiffLine {
                kind: DiffKind::Same,
                left: Some(1),
                right: Some(2),
                text: "tres".into(),
            },
        ]
    );
}

#[test]
fn removed_lines() {
    let left = "uno\ndos\ntres\n";
    let right = "uno\ntres\n";
    let res = diff_lines(left, right);
    assert_eq!(
        res,
        vec![
            DiffLine {
                kind: DiffKind::Same,
                left: Some(0),
                right: Some(0),
                text: "uno".into(),
            },
            DiffLine {
                kind: DiffKind::Removed,
                left: Some(1),
                right: None,
                text: "dos".into(),
            },
            DiffLine {
                kind: DiffKind::Same,
                left: Some(2),
                right: Some(1),
                text: "tres".into(),
            },
        ]
    );
}

#[test]
fn changed_lines() {
    let left = "alfa\nbeta\ngamma\n";
    let right = "alfa\nbeta modificada\ngamma\n";
    let res = diff_lines(left, right);
    assert_eq!(
        res,
        vec![
            DiffLine {
                kind: DiffKind::Same,
                left: Some(0),
                right: Some(0),
                text: "alfa".into(),
            },
            DiffLine {
                kind: DiffKind::Removed,
                left: Some(1),
                right: None,
                text: "beta".into(),
            },
            DiffLine {
                kind: DiffKind::Added,
                left: None,
                right: Some(1),
                text: "beta modificada".into(),
            },
            DiffLine {
                kind: DiffKind::Same,
                left: Some(2),
                right: Some(2),
                text: "gamma".into(),
            },
        ]
    );
}

#[test]
fn crlf_normalization() {
    let left = "uno\r\ndos\r\ntres\r\n";
    let right = "uno\ndos\ntres\n";
    // Si solo difieren en los finales de línea CRLF vs LF, las líneas son Same y el texto no tiene \r
    let res = diff_lines(left, right);
    assert_eq!(
        res,
        vec![
            DiffLine {
                kind: DiffKind::Same,
                left: Some(0),
                right: Some(0),
                text: "uno".into(),
            },
            DiffLine {
                kind: DiffKind::Same,
                left: Some(1),
                right: Some(1),
                text: "dos".into(),
            },
            DiffLine {
                kind: DiffKind::Same,
                left: Some(2),
                right: Some(2),
                text: "tres".into(),
            },
        ]
    );

    // CRLF con un cambio real
    let right_changed = "uno\r\ndos modificado\r\ntres\r\n";
    let res2 = diff_lines(left, right_changed);
    assert_eq!(
        res2,
        vec![
            DiffLine {
                kind: DiffKind::Same,
                left: Some(0),
                right: Some(0),
                text: "uno".into(),
            },
            DiffLine {
                kind: DiffKind::Removed,
                left: Some(1),
                right: None,
                text: "dos".into(),
            },
            DiffLine {
                kind: DiffKind::Added,
                left: None,
                right: Some(1),
                text: "dos modificado".into(),
            },
            DiffLine {
                kind: DiffKind::Same,
                left: Some(2),
                right: Some(2),
                text: "tres".into(),
            },
        ]
    );
}
