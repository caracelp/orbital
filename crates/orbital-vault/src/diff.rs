//! Comparación de líneas con `similar` (US5, data-model de la 002).

use std::borrow::Cow;

/// Tipo de cambio en una línea.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DiffKind {
    /// La línea está en las dos versiones.
    Same,
    /// La línea estaba en la versión izquierda pero no en la derecha.
    Removed,
    /// La línea está en la versión derecha pero no en la izquierda.
    Added,
}

/// Una línea dentro de una comparación.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiffLine {
    pub kind: DiffKind,
    /// Índice de la línea en la versión izquierda (0-indexado), si corresponde.
    pub left: Option<usize>,
    /// Índice de la línea en la versión derecha (0-indexado), si corresponde.
    pub right: Option<usize>,
    /// Texto de la línea (sin `\r` ni `\n` al final).
    pub text: String,
}

fn normalize_newlines(s: &str) -> Cow<'_, str> {
    if s.contains('\r') {
        Cow::Owned(s.replace("\r\n", "\n").replace('\r', "\n"))
    } else {
        Cow::Borrowed(s)
    }
}

/// Compara dos textos línea por línea.
///
/// Los saltos de línea CRLF se normalizan y el campo `text` de cada [`DiffLine`] no incluye
/// el salto de línea final.
pub fn diff_lines(left: &str, right: &str) -> Vec<DiffLine> {
    let left_norm = normalize_newlines(left);
    let right_norm = normalize_newlines(right);
    let diff = similar::TextDiff::from_lines(&left_norm, &right_norm);
    let mut out = Vec::new();

    for change in diff.iter_all_changes() {
        let kind = match change.tag() {
            similar::ChangeTag::Equal => DiffKind::Same,
            similar::ChangeTag::Delete => DiffKind::Removed,
            similar::ChangeTag::Insert => DiffKind::Added,
        };
        let text = change
            .value()
            .strip_suffix("\r\n")
            .or_else(|| change.value().strip_suffix('\n'))
            .unwrap_or(change.value())
            .to_string();

        out.push(DiffLine {
            kind,
            left: change.old_index(),
            right: change.new_index(),
            text,
        });
    }

    out
}
