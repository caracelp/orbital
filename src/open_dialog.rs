//! Diálogo de «Abrir como bóveda de Orbital»: muestra qué se va a crear antes de crearlo.

use adw::prelude::*;
use orbital_vault::{CreationKind, OpenAsPlan};

/// Respuesta del usuario.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Answer {
    Create,
    Cancel,
}

/// Texto del cuerpo del diálogo.
fn body(plan: &OpenAsPlan) -> String {
    let mut out = String::new();
    if plan.creations.is_empty() {
        out.push_str(
            "La carpeta ya tiene todo lo que Orbital puede crear. No se va a cambiar nada.",
        );
    } else {
        out.push_str("Se van a crear:\n");
        for c in &plan.creations {
            let suffix = if c.kind == CreationKind::Folder {
                "/"
            } else {
                ""
            };
            out.push_str(&format!("  • {}{suffix}\n", c.path));
        }
        out.push_str("\nNo se modifica ningún archivo existente.");
    }
    if !plan.conflicts.is_empty() {
        out.push_str("\n\nNo se van a tocar (existen con otro tipo):\n");
        for p in &plan.conflicts {
            out.push_str(&format!("  • {p}\n"));
        }
    }
    out
}

/// Pregunta si se crea la estructura.
pub async fn confirm(parent: &impl IsA<gtk::Widget>, plan: &OpenAsPlan) -> Answer {
    let dialog = adw::AlertDialog::new(Some("Abrir como bóveda de Orbital"), Some(&body(plan)));
    dialog.add_response("cancel", "Cancelar");
    let label = if plan.creations.is_empty() {
        "Abrir"
    } else {
        "Crear y abrir"
    };
    dialog.add_response("create", label);
    dialog.set_response_appearance("create", adw::ResponseAppearance::Suggested);
    dialog.set_default_response(Some("create"));
    dialog.set_close_response("cancel");
    match dialog.choose_future(Some(parent)).await.as_str() {
        "create" => Answer::Create,
        _ => Answer::Cancel,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use orbital_vault::{Creation, RelPath};

    #[test]
    fn body_lists_creations_and_conflicts() {
        let plan = OpenAsPlan {
            creations: vec![
                Creation {
                    path: RelPath::new("raw").unwrap(),
                    kind: CreationKind::Folder,
                },
                Creation {
                    path: RelPath::new("wiki/log.md").unwrap(),
                    kind: CreationKind::File,
                },
            ],
            conflicts: vec![RelPath::new("wiki").unwrap()],
        };
        let b = body(&plan);
        assert!(b.contains("• raw/"));
        assert!(b.contains("• wiki/log.md\n"));
        assert!(b.contains("No se van a tocar"));
        assert!(body(&OpenAsPlan::default()).contains("No se va a cambiar nada"));
    }
}
