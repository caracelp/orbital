//! Preferencias de la bóveda (`.orbital/config.yaml`, FR-018).

use std::path::PathBuf;

use adw::prelude::*;
use gtk::{gio, glib};
use orbital_vault::{LinkStyle, VaultConfig};

/// Muestra el diálogo. Al cerrarlo, si algo cambió, guarda la configuración (creando
/// `.orbital/` recién ahora, FR-004) y llama a `on_saved`.
pub fn show(
    parent: &impl IsA<gtk::Widget>,
    root: PathBuf,
    current: VaultConfig,
    on_saved: impl Fn(Result<(), String>) + 'static,
) {
    let dialog = adw::PreferencesDialog::builder()
        .title("Preferencias de la bóveda")
        .build();
    let page = adw::PreferencesPage::new();
    let group = adw::PreferencesGroup::builder()
        .title("Notas")
        .description("Se guardan en .orbital/config.yaml, dentro de la bóveda.")
        .build();

    let links = adw::ComboRow::builder()
        .title("Enlaces al autocompletar")
        .subtitle("Cómo escribe Orbital un enlace a otra nota")
        .model(&gtk::StringList::new(&[
            "Wikilink: [[nota]]",
            "Markdown: [nota](/nota.md)",
        ]))
        .selected(match current.link_style {
            LinkStyle::Wikilink => 0,
            LinkStyle::Markdown => 1,
        })
        .build();
    let journal = adw::EntryRow::builder()
        .title("Nombre del diario (AAAA, MM y DD se reemplazan)")
        .text(&current.journal_name)
        .build();
    let actor = adw::EntryRow::builder()
        .title("Autor de las notas (vacío: el usuario del sistema)")
        .text(current.actor.as_deref().unwrap_or(""))
        .build();
    let ask_missing = adw::SwitchRow::builder()
        .title("Confirmar antes de crear notas pendientes")
        .subtitle("Preguntar antes de crear una nota al hacer clic en un enlace inexistente")
        .active(current.ask_create_missing_notes)
        .build();
    group.add(&links);
    group.add(&journal);
    group.add(&actor);
    group.add(&ask_missing);
    page.add(&group);
    dialog.add(&page);

    let on_saved = std::rc::Rc::new(on_saved);
    dialog.connect_closed(move |_| {
        let actor_text = actor.text().trim().to_owned();
        let journal_text = journal.text().trim().to_owned();
        let next = VaultConfig {
            link_style: if links.selected() == 1 {
                LinkStyle::Markdown
            } else {
                LinkStyle::Wikilink
            },
            journal_name: if journal_text.is_empty() {
                VaultConfig::default().journal_name
            } else {
                journal_text
            },
            actor: (!actor_text.is_empty()).then(|| {
                if actor_text.contains(':') {
                    actor_text
                } else {
                    format!("human:{actor_text}")
                }
            }),
            ask_create_missing_notes: ask_missing.is_active(),
            ..current.clone()
        };
        if next == current {
            return;
        }
        let (root, on_saved) = (root.clone(), on_saved.clone());
        glib::spawn_future_local(async move {
            let r = gio::spawn_blocking(move || next.save(&root))
                .await
                .map_err(|_| "No se pudo guardar".to_owned())
                .and_then(|r| r.map_err(|e| e.to_string()));
            on_saved(r);
        });
    });
    dialog.present(Some(parent));
}
