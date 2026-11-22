//! Preferencias: las de la bóveda (`.orbital/config.yaml`, FR-018) y las del editor y de la
//! tabla de contenidos (035), que valen para todas las bóvedas y se guardan con el estado de la
//! app (`estado.yaml`, 054).

use std::path::PathBuf;

use adw::prelude::*;
use gtk::{gio, glib};
use orbital_vault::{LinkStyle, VaultConfig};

/// Muestra el diálogo. Al cerrarlo, si algo de la bóveda cambió, guarda su configuración
/// (creando `.orbital/` recién ahora, FR-004) y llama a `on_saved`. Lo del editor se aplica y se
/// guarda en el acto (con `persist` en falso, como en las capturas, solo se aplica); `on_lectura`
/// avisa que cambió algo de lo que muestra la Lectura (la tabla de contenidos), para volver a
/// dibujar la nota abierta.
pub fn show(
    parent: &impl IsA<gtk::Widget>,
    root: PathBuf,
    current: VaultConfig,
    persist: bool,
    on_lectura: impl Fn() + 'static,
    on_saved: impl Fn(Result<(), String>) + 'static,
) -> adw::PreferencesDialog {
    let dialog = adw::PreferencesDialog::builder()
        .title("Preferencias")
        .build();
    let page = adw::PreferencesPage::new();
    page.add(&editor_group(persist));
    let on_lectura: std::rc::Rc<dyn Fn()> = std::rc::Rc::new(on_lectura);
    page.add(&toc_group(persist, on_lectura.clone()));
    page.add(&indice_group(persist, on_lectura));
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
    dialog
}

/// Grupo «Editor»: los números de línea de la vista Código (054).
fn editor_group(persist: bool) -> adw::PreferencesGroup {
    let group = adw::PreferencesGroup::builder()
        .title("Editor")
        .description("Valen para todas las bóvedas.")
        .build();
    let numbers = adw::SwitchRow::builder()
        .title("Números de línea")
        .subtitle("Numerar las líneas en el margen de la vista Código")
        .active(crate::editor::code::line_numbers())
        .build();
    numbers.connect_active_notify(move |row| {
        let on = row.is_active();
        if on == crate::editor::code::line_numbers() {
            return;
        }
        crate::editor::code::set_line_numbers(on);
        if persist {
            glib::spawn_future_local(async move {
                let _ = gio::spawn_blocking(move || {
                    let mut s = crate::state::AppState::load();
                    s.hide_line_numbers = !on;
                    s.save()
                })
                .await;
            });
        }
    });
    group.add(&numbers);
    group
}

/// Grupo «Tabla de contenidos» (035): los valores que toma un ` ```toc ` que no escribe esa
/// opción. Se aplican en el acto (la nota abierta se vuelve a dibujar) y se guardan con el estado
/// de la app; el título, con una pausa, para no escribir en cada tecla.
fn toc_group(persist: bool, on_lectura: std::rc::Rc<dyn Fn()>) -> adw::PreferencesGroup {
    use orbital_vault::toc::{TocDefaults, TocStyle};
    use std::cell::RefCell;
    use std::rc::Rc;
    let group = adw::PreferencesGroup::builder()
        .title("Tabla de contenidos")
        .description(
            "Valores por defecto de los bloques toc. Lo que escribe cada bloque manda sobre estos.",
        )
        .build();
    let actual = Rc::new(RefCell::new(crate::editor::reader::toc_defaults()));
    let aplicar = {
        let (actual, on_lectura) = (actual.clone(), on_lectura.clone());
        Rc::new(move |cambiar: &dyn Fn(&mut TocDefaults)| {
            let d = {
                let mut d = actual.borrow_mut();
                let antes = d.clone();
                cambiar(&mut d);
                if *d == antes {
                    return;
                }
                d.clone()
            };
            crate::editor::reader::set_toc_defaults(d.clone());
            on_lectura();
            if persist {
                glib::spawn_future_local(async move {
                    let _ = gio::spawn_blocking(move || {
                        let mut s = crate::state::AppState::load();
                        s.toc = d;
                        s.save()
                    })
                    .await;
                });
            }
        })
    };
    let d = actual.borrow().clone();
    let estilo = adw::ComboRow::builder()
        .title("Estilo")
        .model(&gtk::StringList::new(&[
            "Lista con viñetas",
            "Lista numerada",
            "Primer nivel en una línea",
        ]))
        .selected(
            TocStyle::TODOS
                .iter()
                .position(|s| *s == d.style)
                .unwrap_or(0) as u32,
        )
        .build();
    {
        let aplicar = aplicar.clone();
        estilo.connect_selected_notify(move |row| {
            let style = TocStyle::TODOS
                .get(row.selected() as usize)
                .copied()
                .unwrap_or(TocStyle::NestedList);
            aplicar(&|d| d.style = style);
        });
    }
    let nivel = |titulo: &str, subtitulo: &str, valor: u8| {
        adw::SpinRow::builder()
            .title(titulo)
            .subtitle(subtitulo)
            .adjustment(&gtk::Adjustment::new(valor as f64, 0.0, 6.0, 1.0, 1.0, 0.0))
            .digits(0)
            .numeric(true)
            .build()
    };
    let minimo = nivel(
        "Nivel mínimo",
        "Desde qué nivel de encabezado; 0: el menor de la nota",
        d.min_level,
    );
    let maximo = nivel(
        "Nivel máximo",
        "Hasta qué nivel de encabezado; 0: sin límite",
        d.max_level,
    );
    {
        let aplicar = aplicar.clone();
        minimo.connect_value_notify(move |row| {
            let n = row.value().round().clamp(0.0, 6.0) as u8;
            aplicar(&|d| d.min_level = n);
        });
    }
    {
        let aplicar = aplicar.clone();
        maximo.connect_value_notify(move |row| {
            let n = row.value().round().clamp(0.0, 6.0) as u8;
            aplicar(&|d| d.max_level = n);
        });
    }
    let titulo = adw::EntryRow::builder()
        .title("Título antes de la tabla (vacío: sin título)")
        .text(&d.title)
        .build();
    {
        let aplicar = aplicar.clone();
        al_escribir(&titulo, move |t| aplicar(&|d| d.title = t.clone()));
    }
    let enlaces = adw::SwitchRow::builder()
        .title("Enlaces a los encabezados")
        .subtitle("Si se apaga, la tabla es texto sin clic")
        .active(d.include_links)
        .build();
    {
        let aplicar = aplicar.clone();
        enlaces.connect_active_notify(move |row| {
            let on = row.is_active();
            aplicar(&|d| d.include_links = on);
        });
    }
    let ocultar = adw::SwitchRow::builder()
        .title("Ocultar si no hay encabezados")
        .subtitle("Si no, un aviso ocupa el lugar de la tabla")
        .active(d.hide_when_empty)
        .build();
    {
        let aplicar = aplicar.clone();
        ocultar.connect_active_notify(move |row| {
            let on = row.is_active();
            aplicar(&|d| d.hide_when_empty = on);
        });
    }
    group.add(&estilo);
    group.add(&minimo);
    group.add(&maximo);
    group.add(&titulo);
    group.add(&enlaces);
    group.add(&ocultar);
    group
}

/// Una pausa antes de aplicar lo escrito en una fila de texto (no se escribe `estado.yaml` por
/// tecla).
fn al_escribir(fila: &adw::EntryRow, aplicar: impl Fn(String) + 'static) {
    use std::cell::RefCell;
    use std::rc::Rc;
    let aplicar = Rc::new(aplicar);
    let pausa: Rc<RefCell<Option<glib::SourceId>>> = Rc::default();
    fila.connect_changed(move |row| {
        if let Some(id) = pausa.borrow_mut().take() {
            id.remove();
        }
        let (aplicar, texto, p) = (aplicar.clone(), row.text().trim().to_owned(), pausa.clone());
        let id = glib::timeout_add_local_once(std::time::Duration::from_millis(400), move || {
            p.borrow_mut().take();
            aplicar(texto);
        });
        *pausa.borrow_mut() = Some(id);
    });
}

/// Grupo «Índice por etiqueta» (036): la etiqueta de prioridad, los títulos al lado y las
/// carpetas excluidas que toma un ` ```indice ` que no las escribe. Como la tabla de contenidos:
/// se aplica en el acto y se guarda con el estado de la app.
fn indice_group(persist: bool, on_lectura: std::rc::Rc<dyn Fn()>) -> adw::PreferencesGroup {
    use orbital_vault::indice::IndiceDefaults;
    use std::cell::RefCell;
    use std::rc::Rc;
    let group = adw::PreferencesGroup::builder()
        .title("Índice por etiqueta")
        .description(
            "Valores por defecto de los bloques indice. Lo que escribe cada bloque manda sobre estos.",
        )
        .build();
    let actual = Rc::new(RefCell::new(crate::editor::reader::indice_defaults()));
    let aplicar = {
        let actual = actual.clone();
        Rc::new(move |cambiar: &dyn Fn(&mut IndiceDefaults)| {
            let d = {
                let mut d = actual.borrow_mut();
                let antes = d.clone();
                cambiar(&mut d);
                if *d == antes {
                    return;
                }
                d.clone()
            };
            crate::editor::reader::set_indice_defaults(d.clone());
            on_lectura();
            if persist {
                glib::spawn_future_local(async move {
                    let _ = gio::spawn_blocking(move || {
                        let mut s = crate::state::AppState::load();
                        s.indice = d;
                        s.save()
                    })
                    .await;
                });
            }
        })
    };
    let d = actual.borrow().clone();
    let prioridad = adw::EntryRow::builder()
        .title("Etiqueta de prioridad (arriba y en negrita; vacío: ninguna)")
        .text(&d.prioridad)
        .build();
    {
        let aplicar = aplicar.clone();
        al_escribir(&prioridad, move |t| {
            let t = t.trim_start_matches('#').to_owned();
            aplicar(&|d| d.prioridad = t.clone());
        });
    }
    let titulos = adw::SwitchRow::builder()
        .title("Título de cada nota")
        .subtitle("Mostrar la propiedad title junto al enlace")
        .active(d.titulo)
        .build();
    {
        let aplicar = aplicar.clone();
        titulos.connect_active_notify(move |row| {
            let on = row.is_active();
            aplicar(&|d| d.titulo = on);
        });
    }
    let excluir = adw::EntryRow::builder()
        .title("Carpetas excluidas (separadas por comas)")
        .text(d.excluir.join(", "))
        .build();
    {
        let aplicar = aplicar.clone();
        al_escribir(&excluir, move |t| {
            let lista: Vec<String> = t
                .split(',')
                .map(|s| s.trim().trim_matches('/').to_owned())
                .filter(|s| !s.is_empty())
                .collect();
            aplicar(&|d| d.excluir = lista.clone());
        });
    }
    group.add(&prioridad);
    group.add(&titulos);
    group.add(&excluir);
    group
}
