//! Acciones de la ventana, atajos, preferencias, selector rápido y paleta de comandos.

use super::*;

impl Window {
    pub(super) fn setup_actions(&self, app: &adw::Application) {
        let open = gio::ActionEntry::builder("open")
            .activate({
                let this = self.clone();
                move |_: &adw::ApplicationWindow, _, _| this.choose_and_open()
            })
            .build();
        let open_as = gio::ActionEntry::builder("open-as")
            .activate({
                let this = self.clone();
                move |_: &adw::ApplicationWindow, _, _| this.choose_and_open_as()
            })
            .build();
        let complete = gio::ActionEntry::builder("complete-structure")
            .activate({
                let this = self.clone();
                move |_: &adw::ApplicationWindow, _, _| {
                    let root = this
                        .0
                        .vault
                        .borrow()
                        .as_ref()
                        .map(|v| v.root().to_path_buf());
                    if let Some(root) = root {
                        let this = this.clone();
                        glib::spawn_future_local(async move { this.open_as_path(root).await });
                    }
                }
            })
            .build();
        let complete_type = gio::ActionEntry::builder("complete-type")
            .activate({
                let this = self.clone();
                move |_: &adw::ApplicationWindow, _, _| this.complete_type()
            })
            .build();
        let shortcuts = gio::ActionEntry::builder("shortcuts")
            .activate({
                let this = self.clone();
                move |_: &adw::ApplicationWindow, _, _| this.show_shortcuts()
            })
            .build();
        self.0
            .window
            .add_action_entries([open, open_as, complete, complete_type, shortcuts]);
        let rename = gio::ActionEntry::builder("rename")
            .activate({
                let this = self.clone();
                move |_: &adw::ApplicationWindow, _, _| this.ask_rename()
            })
            .build();
        let trash = gio::ActionEntry::builder("trash")
            .activate({
                let this = self.clone();
                move |_: &adw::ApplicationWindow, _, _| this.ask_trash()
            })
            .build();
        self.0.window.add_action_entries([rename, trash]);
        let simple = |name: &str, f: fn(&Window)| {
            let this = self.clone();
            gio::ActionEntry::builder(name)
                .activate(move |_: &adw::ApplicationWindow, _, _| f(&this))
                .build()
        };
        let from_template = gio::ActionEntry::builder("new-from-template")
            .parameter_type(Some(glib::VariantTy::STRING))
            .activate({
                let this = self.clone();
                move |_: &adw::ApplicationWindow, _, param| {
                    if let Some(p) = param.and_then(|v| v.get::<String>()) {
                        this.ask_new_note(NewKind::Template(p));
                    }
                }
            })
            .build();
        self.0.window.add_action_entries([
            simple("open-external", |w| {
                let entry = w.0.current_entry.borrow().clone();
                let root = w.0.vault.borrow().as_ref().map(|v| v.root().to_path_buf());
                if let (Some(e), Some(root)) = (entry, root) {
                    w.launch_file(&e.path.to_path(&root));
                }
            }),
            simple("preferences", Window::show_preferences),
            simple("view-code", |w| {
                w.0.code_editor.set_view(crate::editor::View::Codigo)
            }),
            simple("view-read", |w| {
                w.0.code_editor.set_view(crate::editor::View::Lectura)
            }),
            simple("view-split", |w| {
                w.0.code_editor.set_view(crate::editor::View::Dividida)
            }),
            simple("backlinks", |w| {
                let s = &w.0.right_split;
                s.set_show_sidebar(!s.shows_sidebar());
            }),
            simple("galeria-lateral", |w| {
                w.0.right_split.set_show_sidebar(true);
                w.0.panel.mostrar_galeria();
            }),
            simple("toggle-left", |w| {
                let s = &w.0.split;
                s.set_show_sidebar(!s.shows_sidebar());
            }),
            simple("toggle-right", |w| {
                let s = &w.0.right_split;
                s.set_show_sidebar(!s.shows_sidebar());
            }),
            simple("new-folder", Window::ask_new_folder),
            simple("sort-tree", Window::sort_tree),
            simple("reveal-current", |w| {
                if let Some(e) = w.0.current_entry.borrow().clone() {
                    w.reveal(e.path.as_str());
                }
            }),
            simple("collapse-folders", Window::collapse_folders),
            simple("search", Window::focus_search),
            simple("toggle-search-raw", Window::toggle_search_raw),
            simple("find", |w| w.0.code_editor.open_find(false)),
            simple("replace", |w| w.0.code_editor.open_find(true)),
            simple("undo", |w| {
                w.0.code_editor.undo();
            }),
            simple("quick-open", Window::open_switcher),
            simple("command-palette", Window::open_command_palette),
            simple("exportar-html", |w| {
                w.exportar(crate::export::Formato::Html)
            }),
            simple("exportar-pdf", |w| w.exportar(crate::export::Formato::Pdf)),
            simple("ver-grafo", Window::ver_grafo),
            simple("cerrar-grafo", Window::cerrar_grafo),
            simple("ver-brujula", Window::ver_brujula),
            simple("cerrar-brujula", Window::cerrar_brujula),
            simple("nuevo-canvas", Window::ask_new_canvas),
            simple("ver-mapa-mental", Window::ver_mapa_mental),
            simple("cerrar-mapa-mental", Window::cerrar_mapa_mental),
            simple("ver-ramas-libres", Window::ver_ramas_libres),
            simple("cerrar-ramas-libres", Window::cerrar_ramas_libres),
            simple("estructurar-nota", Window::estructurar_nota_tres_partes),
            simple("historial", Window::historial),
            simple("ecuacion", |w| {
                w.0.code_editor.abrir_ecuacion(false);
            }),
            simple("insertar-ecuacion", |w| {
                w.0.code_editor.abrir_ecuacion(true);
            }),
            simple("insertar-grafico", |w| {
                w.0.code_editor.insertar_grafico();
            }),
            simple("insertar-galeria", |w| {
                w.0.code_editor.insertar_galeria();
            }),
            simple("insertar-toc", |w| {
                w.0.code_editor.insertar_toc();
            }),
            simple("card-link", Window::card_link),
            simple("onstage", Window::presentar),
            simple("importar-md", Window::importar_md),
            simple("ver-pdf", Window::ver_pdf_accion),
            simple("test-pdf-seleccion", |w: &Window| {
                w.0.visor.seleccion_de_prueba(1, 0, 6, "Informe");
            }),
            simple("renumerar-notas-al-pie", |w| {
                if w.0.code_editor.current_path().is_none() {
                    w.toast("Abrí una nota antes de renumerar");
                    return;
                }
                let text = w.0.code_editor.text();
                let Some(nuevo) = crate::editor::notas_al_pie::renumerar(&text) else {
                    w.toast("Las notas al pie ya están en orden");
                    return;
                };
                w.0.code_editor.replace_text(&nuevo);
                w.toast("Notas al pie renumeradas (guardá para persistir)");
            }),
            simple("asociar-url", |w| {
                let Some(ruta) = w.0.externo_ruta.borrow().clone() else {
                    w.toast("Abrí un archivo de raw/ antes de asociar una URL");
                    return;
                };
                w.asociar_url_dialog(&ruta);
            }),
            simple("quitar-url", |w| {
                let Some(ruta) = w.0.externo_ruta.borrow().clone() else {
                    return;
                };
                let w = w.clone();
                let i = w.0.clone();
                let Some(v) = i.vault.borrow().clone() else {
                    return;
                };
                glib::spawn_future_local(async move {
                    let r = ruta.clone();
                    let resultado =
                        gio::spawn_blocking(move || v.quitar_url(&r, &write_ctx::now(None))).await;
                    match resultado {
                        Ok(Ok(())) => {
                            i.content_stack.set_visible_child_name("externo");
                        }
                        _ => w.toast("No se pudo quitar la URL"),
                    }
                });
            }),
            simple("new-note", |w| w.ask_new_note(NewKind::Md)),
            simple("new-html", |w| w.ask_new_note(NewKind::Html)),
            simple("journal", Window::open_journal),
            simple("archive", Window::archive_current),
            simple("paste-image", Window::paste_image),
            simple("open-missing", Window::test_open_missing),
            from_template,
        ]);
        app.set_accels_for_action("win.new-note", &["<Control>n"]);
        app.set_accels_for_action("win.preferences", &["<Control>comma"]);
        app.set_accels_for_action("win.view-code", &["<Control>1"]);
        app.set_accels_for_action("win.view-read", &["<Control>2"]);
        app.set_accels_for_action("win.view-split", &["<Control>3"]);
        app.set_accels_for_action("win.search", &["<Control><Shift>f"]);
        app.set_accels_for_action("win.find", &["<Control>f"]);
        app.set_accels_for_action("win.replace", &["<Control>h"]);
        app.set_accels_for_action("win.undo", &["<Control>z"]);
        app.set_accels_for_action("win.quick-open", &["<Control>p"]);
        app.set_accels_for_action("win.command-palette", &["<Control><Shift>p"]);
        app.set_accels_for_action("win.exportar-html", &["<Control><Shift>e"]);
        app.set_accels_for_action("win.backlinks", &["<Control><Shift>b"]);
        app.set_accels_for_action("win.toggle-left", &["<Control>bracketleft"]);
        app.set_accels_for_action("win.toggle-right", &["<Control>bracketright"]);
        app.set_accels_for_action("win.galeria-lateral", &["<Control><Alt>g"]);
        app.set_accels_for_action("win.journal", &["<Control><Alt>d"]);
        app.set_accels_for_action("win.archive", &["<Control><Shift>a"]);
        app.set_accels_for_action("win.paste-image", &["<Control><Shift>v"]);
        app.set_accels_for_action("win.rename", &["F2"]);
        app.set_accels_for_action("win.trash", &["Delete"]);
        app.set_accels_for_action("win.open", &["<Control>o"]);
        app.set_accels_for_action("win.open-as", &["<Control><Shift>o"]);
        app.set_accels_for_action("win.shortcuts", &["<Control>question"]);
        app.set_accels_for_action("win.ecuacion", &["<Control>m"]);
    }

    pub(super) fn show_shortcuts(&self) {
        let dialog = adw::ShortcutsDialog::new();
        let section = adw::ShortcutsSection::new(Some("Bóveda"));
        section.add(adw::ShortcutsItem::new("Abrir una carpeta", "<Control>o"));
        section.add(adw::ShortcutsItem::new(
            "Abrir como bóveda de Orbital",
            "<Control><Shift>o",
        ));
        section.add(adw::ShortcutsItem::new("Preferencias", "<Control>comma"));
        section.add(adw::ShortcutsItem::new("Nota nueva", "<Control>n"));
        section.add(adw::ShortcutsItem::new("Vista Código", "<Control>1"));
        section.add(adw::ShortcutsItem::new("Vista Lectura", "<Control>2"));
        section.add(adw::ShortcutsItem::new("Vista Dividida", "<Control>3"));
        section.add(adw::ShortcutsItem::new(
            "Buscar en toda la bóveda",
            "<Control><Shift>f",
        ));
        section.add(adw::ShortcutsItem::new(
            "Buscar en la nota abierta",
            "<Control>f",
        ));
        section.add(adw::ShortcutsItem::new(
            "Reemplazar en la nota abierta",
            "<Control>h",
        ));
        section.add(adw::ShortcutsItem::new(
            "Saltar a una nota (selector rápido)",
            "<Control>p",
        ));
        section.add(adw::ShortcutsItem::new(
            "Paleta de comandos",
            "<Control><Shift>p",
        ));
        section.add(adw::ShortcutsItem::new(
            "Cursor extra con Alt+clic o Alt+arrastre",
            "<Alt>click",
        ));
        section.add(adw::ShortcutsItem::new(
            "Cursor por columnas",
            "<Control><Alt>Up",
        ));
        section.add(adw::ShortcutsItem::new("Volver a un solo cursor", "Escape"));
        section.add(adw::ShortcutsItem::new(
            "Exportar la nota como HTML",
            "<Control><Shift>e",
        ));
        section.add(adw::ShortcutsItem::new(
            "Mostrar u ocultar los retroenlaces",
            "<Control><Shift>b",
        ));
        section.add(adw::ShortcutsItem::new(
            "Mostrar u ocultar la barra izquierda",
            "<Control>bracketleft",
        ));
        section.add(adw::ShortcutsItem::new(
            "Mostrar u ocultar la barra derecha",
            "<Control>bracketright",
        ));
        section.add(adw::ShortcutsItem::new(
            "Galería de la nota en la barra derecha",
            "<Control><Alt>g",
        ));
        section.add(adw::ShortcutsItem::new("Diario de hoy", "<Control><Alt>d"));
        section.add(adw::ShortcutsItem::new(
            "Archivar la nota",
            "<Control><Shift>a",
        ));
        section.add(adw::ShortcutsItem::new(
            "Pegar imagen como adjunto",
            "<Control><Shift>v",
        ));
        section.add(adw::ShortcutsItem::new("Renombrar o mover la nota", "F2"));
        section.add(adw::ShortcutsItem::new(
            "Mover la nota a la papelera",
            "Delete",
        ));
        section.add(adw::ShortcutsItem::new(
            "Editar la ecuación del cursor o insertar una",
            "<Control>m",
        ));
        section.add(adw::ShortcutsItem::new("Plegar el elemento", "<Control>Up"));
        section.add(adw::ShortcutsItem::new(
            "Desplegar el elemento",
            "<Control>Down",
        ));
        section.add(adw::ShortcutsItem::new("Enfocar el elemento", "<Alt>Right"));
        section.add(adw::ShortcutsItem::new(
            "Subir un nivel en el enfoque",
            "<Alt>Left",
        ));
        section.add(adw::ShortcutsItem::new(
            "Atajos de teclado",
            "<Control>question",
        ));
        dialog.add(section);
        // La galería lateral (034): con el foco en la grilla o en el visor.
        let galeria = adw::ShortcutsSection::new(Some("Galería de la nota"));
        galeria.add(adw::ShortcutsItem::new(
            "Recorrer títulos y mosaicos",
            "Left Right Up Down",
        ));
        galeria.add(adw::ShortcutsItem::new(
            "Plegar o desplegar un grupo; abrir el visor",
            "Return",
        ));
        galeria.add(adw::ShortcutsItem::new("Ir al embed", "<Control>Return"));
        galeria.add(adw::ShortcutsItem::new("Menú del mosaico", "<Shift>F10"));
        galeria.add(adw::ShortcutsItem::new("Ir al embed desde el visor", "e"));
        galeria.add(adw::ShortcutsItem::new(
            "Información del archivo en el visor",
            "i",
        ));
        galeria.add(adw::ShortcutsItem::new("Cerrar el visor", "Escape"));
        dialog.add(galeria);
        dialog.present(Some(&self.0.window));
    }

    pub fn open_switcher(&self) {
        let Some(vault) = self.0.vault.borrow().clone() else {
            return;
        };
        let candidates = self.0.candidates.borrow().clone();
        let state = AppState::load();
        let recent = state.recent_notes_for(vault.root()).to_vec();
        let this = self.clone();
        crate::switcher::SwitcherDialog::present(&self.0.window, candidates, recent, move |path| {
            this.show_path(path.as_str());
        });
    }

    /// Registro de comandos de la paleta (US8): nombre visible, acción `win.*`
    /// que corre, atajo mostrado y si requiere una nota abierta. El botón de
    /// los banners («Completar tipo/estructura») y las acciones internas o con
    /// parámetro quedan fuera: aparecen cuando toca, no en la paleta.
    pub(super) const COMMANDS: &[(&str, &str, Option<&str>, bool)] = &[
        ("Nota nueva", "new-note", Some("Ctrl+N"), false),
        ("Nota HTML nueva", "new-html", None, false),
        ("Diario de hoy", "journal", Some("Ctrl+Alt+D"), false),
        ("Archivar la nota", "archive", Some("Ctrl+Mayús+A"), true),
        (
            "Pegar imagen como adjunto",
            "paste-image",
            Some("Ctrl+Mayús+V"),
            true,
        ),
        ("Renombrar o mover la nota", "rename", Some("F2"), true),
        ("Mover la nota a la papelera", "trash", Some("Supr"), true),
        ("Abrir la nota con otra app", "open-external", None, true),
        ("Vista Código", "view-code", Some("Ctrl+1"), false),
        ("Vista Lectura", "view-read", Some("Ctrl+2"), false),
        ("Vista Dividida", "view-split", Some("Ctrl+3"), false),
        (
            "Buscar en toda la bóveda",
            "search",
            Some("Ctrl+Mayús+F"),
            false,
        ),
        (
            "Incluir u ocultar fuentes (raw/)",
            "toggle-search-raw",
            None,
            false,
        ),
        ("Buscar en la nota abierta", "find", Some("Ctrl+F"), true),
        (
            "Reemplazar en la nota abierta",
            "replace",
            Some("Ctrl+H"),
            true,
        ),
        ("Deshacer en la nota abierta", "undo", Some("Ctrl+Z"), true),
        ("Plegar todo", "plegar-todo", None, true),
        ("Desplegar todo", "desplegar-todo", None, true),
        (
            "Plegar o desplegar el elemento",
            "alternar-plegado",
            Some("Ctrl+Arriba"),
            true,
        ),
        ("Enfocar el elemento", "enfocar", Some("Alt+Derecha"), true),
        ("Salir del enfoque", "salir-enfoque", None, true),
        (
            "Saltar a una nota (selector rápido)",
            "quick-open",
            Some("Ctrl+P"),
            false,
        ),
        (
            "Mostrar u ocultar los retroenlaces",
            "backlinks",
            Some("Ctrl+Mayús+B"),
            false,
        ),
        (
            "Alternar la barra izquierda",
            "toggle-left",
            Some("Ctrl+["),
            false,
        ),
        (
            "Alternar la barra derecha",
            "toggle-right",
            Some("Ctrl+]"),
            false,
        ),
        (
            "Galería de la nota",
            "galeria-lateral",
            Some("Ctrl+Alt+G"),
            false,
        ),
        ("Crear carpeta", "new-folder", None, false),
        ("Cambiar el orden del explorador", "sort-tree", None, false),
        (
            "Ubicar la nota en el explorador",
            "reveal-current",
            None,
            true,
        ),
        (
            "Colapsar o expandir las carpetas",
            "collapse-folders",
            None,
            false,
        ),
        ("Abrir carpeta…", "open", Some("Ctrl+O"), false),
        (
            "Abrir como bóveda de Orbital…",
            "open-as",
            Some("Ctrl+Mayús+O"),
            false,
        ),
        ("Preferencias", "preferences", Some("Ctrl+,"), false),
        ("Atajos de teclado", "shortcuts", Some("Ctrl+?"), false),
        (
            "Exportar nota como HTML",
            "exportar-html",
            Some("Ctrl+Mayús+E"),
            true,
        ),
        ("Exportar nota como PDF", "exportar-pdf", None, true),
        ("Ver el grafo de enlaces", "ver-grafo", None, false),
        ("Ver la brújula de la nota", "ver-brujula", None, false),
        ("Nuevo lienzo (Canvas)…", "nuevo-canvas", None, false),
        ("Ver mapa mental de la nota", "ver-mapa-mental", None, true),
        ("Ramas libres de la nota", "ver-ramas-libres", None, true),
        (
            "Estructurar nota en 3 partes",
            "estructurar-nota",
            None,
            true,
        ),
        ("Historial de versiones…", "historial", None, true),
        ("Insertar ecuación", "insertar-ecuacion", None, true),
        ("Insertar gráfico", "insertar-grafico", None, true),
        ("Insertar galería…", "insertar-galeria", None, true),
        ("Insertar índice…", "insertar-toc", None, true),
        (
            "Editar o insertar ecuación",
            "ecuacion",
            Some("Ctrl+M"),
            true,
        ),
        (
            "Renumerar notas al pie",
            "renumerar-notas-al-pie",
            None,
            true,
        ),
        ("Card link desde el portapapeles", "card-link", None, true),
        ("Presentar con OnStage", "onstage", None, true),
        ("Importar a nota (Markdown)", "importar-md", None, false),
    ];

    /// Abre la paleta de comandos (US8). La lista se arma al abrir con el
    /// estado actual: los comandos que requieren nota abierta se muestran
    /// desactivados si no hay ninguna.
    pub fn open_command_palette(&self) {
        let has_note = self.0.current_entry.borrow().is_some();
        let commands: Vec<crate::switcher::CommandEntry> = Self::COMMANDS
            .iter()
            .map(|(name, action, accel, needs_note)| {
                crate::switcher::CommandEntry::new(
                    (*name).to_owned(),
                    (*action).to_owned(),
                    accel.map(|a| a.to_owned()),
                    !needs_note || has_note,
                )
            })
            .collect();
        let this = self.clone();
        crate::switcher::SwitcherDialog::present_commands(
            &self.0.window,
            commands,
            move |command| {
                gtk::prelude::ActionGroupExt::activate_action(
                    &this.0.window,
                    &command.action,
                    None,
                );
            },
        );
    }

    /// «Preferencias»: las de la bóveda y las del editor.
    pub(super) fn show_preferences(&self) {
        let Some(vault) = self.0.vault.borrow().clone() else {
            return;
        };
        let this = self.clone();
        let current = self
            .0
            .current_entry
            .borrow()
            .as_ref()
            .map(|e| e.path.clone());
        let _ = crate::preferences::show(
            &self.0.window,
            vault.root().to_path_buf(),
            vault.config().clone(),
            self.0.persist.get(),
            {
                // La tabla de contenidos cambió (035): la nota abierta se vuelve a dibujar si
                // tiene bloques generados.
                let this = self.clone();
                move || {
                    if this.0.code_editor.has_generated() {
                        this.0.code_editor.render_now();
                    }
                }
            },
            move |r| {
                let (this, current) = (this.clone(), current.clone());
                glib::spawn_future_local(async move {
                    match r {
                        Ok(()) => {
                            this.reload(current).await;
                            this.toast("Preferencias guardadas");
                        }
                        Err(e) => {
                            this.toast(&format!("No se pudieron guardar las preferencias: {e}"))
                        }
                    }
                });
            },
        );
    }
}
