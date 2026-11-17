//! Explorador de la barra izquierda: árbol, recientes, orden, carpetas y archivos soltados.

use super::*;

impl Window {
    pub(super) fn setup_list(&self) {
        let factory = gtk::SignalListItemFactory::new();
        factory.connect_setup(|_, item| {
            let item = item.downcast_ref::<gtk::ListItem>().expect("ListItem");
            let icon = gtk::Image::new();
            let label = gtk::Label::builder()
                .xalign(0.0)
                .ellipsize(gtk::pango::EllipsizeMode::End)
                .build();
            let row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
            row.append(&icon);
            row.append(&label);
            let expander = gtk::TreeExpander::new();
            expander.set_child(Some(&row));
            item.set_child(Some(&expander));
        });
        let tree_for_bind = self.clone();
        factory.connect_bind(move |_, item| {
            let item = item.downcast_ref::<gtk::ListItem>().expect("ListItem");
            let Some(row) = item.item().and_downcast::<gtk::TreeListRow>() else {
                return;
            };
            let Some(path) = row.item().and_downcast::<gtk::StringObject>() else {
                return;
            };
            let Some(expander) = item.child().and_downcast::<gtk::TreeExpander>() else {
                return;
            };
            expander.set_list_row(Some(&row));
            let Some(entry) = tree_for_bind.entry(path.string().as_str()) else {
                return;
            };
            let name = entry.path.file_name().to_owned();
            let content = expander.child().and_downcast::<gtk::Box>().expect("Box");
            let icon = content
                .first_child()
                .and_downcast::<gtk::Image>()
                .expect("Image");
            let label = content
                .last_child()
                .and_downcast::<gtk::Label>()
                .expect("Label");
            icon.set_icon_name(Some(icon_for(entry.kind)));
            label.set_label(&name);
            let what = match entry.kind {
                EntryKind::Folder => "Carpeta",
                EntryKind::AtomNote | EntryKind::Agents | EntryKind::Reserved(_) => "Nota",
                EntryKind::SynthesisNote => "Nota HTML",
                _ => "Archivo",
            };
            let ro = if entry.read_only {
                ", solo lectura"
            } else {
                ""
            };
            item.set_accessible_label(&format!("{what} {name}{ro}"));
        });
        self.0.list_view.set_factory(Some(&factory));

        let this = self.clone();
        self.0.list_view.connect_activate(move |view, pos| {
            let Some(model) = view.model() else { return };
            let Some(row) = model.item(pos).and_downcast::<gtk::TreeListRow>() else {
                return;
            };
            let Some(path) = row.item().and_downcast::<gtk::StringObject>() else {
                return;
            };
            // Las notas se abren al elegirlas (ver `show_vault`); activar solo expande carpetas.
            if this
                .entry(path.string().as_str())
                .is_some_and(|e| e.kind == EntryKind::Folder)
            {
                row.set_expanded(!row.is_expanded());
            }
        });
    }

    /// Busca una entrada por ruta en el árbol cargado.
    pub(super) fn entry(&self, path: &str) -> Option<Arc<Entry>> {
        let parent = path.rsplit_once('/').map_or("", |(p, _)| p);
        let tree = self.0.tree.borrow();
        tree.get(parent)?
            .iter()
            .find(|e| e.path.as_str() == path)
            .cloned()
    }

    /// Arma el modelo del árbol de la bóveda (con el orden elegido) y conserva las carpetas que
    /// estaban abiertas.
    pub(super) fn set_list_model(&self, tree: Rc<Tree>) {
        let i = &self.0;
        // Pestañas de notas que ya no existen (borradas, archivadas o renombradas afuera).
        {
            let this = self.clone();
            i.pestanas.quitar(move |clave| this.entry(clave).is_none());
        }
        let reversed = i.tree_reversed.get();
        let root = children_model(&tree, "", reversed);
        let tree_model = gtk::TreeListModel::new(root, false, false, move |item| {
            let path = item.downcast_ref::<gtk::StringObject>()?.string();
            tree.get(path.as_str())
                .map(|_| children_model(&tree, path.as_str(), reversed).upcast::<gio::ListModel>())
        });
        let selection = gtk::SingleSelection::builder()
            .model(&tree_model)
            .autoselect(false)
            .build();
        // Elegir un archivo lo abre (clic, flechas o AT-SPI), como en las barras laterales de GNOME.
        let this = self.clone();
        selection.connect_selection_changed(move |sel, _, _| {
            let Some(row) = sel.selected_item().and_downcast::<gtk::TreeListRow>() else {
                return;
            };
            let Some(path) = row.item().and_downcast::<gtk::StringObject>() else {
                return;
            };
            if let Some(e) = this
                .entry(path.string().as_str())
                .filter(|e| e.kind != EntryKind::Folder)
            {
                this.show_entry(e);
            }
        });
        // Las carpetas que estaban abiertas siguen abiertas después de recargar.
        let expanded: std::collections::HashSet<String> = i
            .list_view
            .model()
            .map(|old| {
                (0..old.n_items())
                    .filter_map(|n| old.item(n).and_downcast::<gtk::TreeListRow>())
                    .filter(|r| r.is_expanded())
                    .filter_map(|r| r.item().and_downcast::<gtk::StringObject>())
                    .map(|o| o.string().to_string())
                    .collect()
            })
            .unwrap_or_default();
        i.list_view.set_model(Some(&selection));
        let mut n = 0;
        while n < selection.n_items() {
            if let Some(row) = selection.item(n).and_downcast::<gtk::TreeListRow>()
                && let Some(o) = row.item().and_downcast::<gtk::StringObject>()
                && expanded.contains(o.string().as_str())
            {
                row.set_expanded(true);
            }
            n += 1;
        }
    }

    /// Sección «Recientes» del explorador: las cinco últimas notas abiertas.
    pub(super) fn refresh_recent(&self) {
        let top: Vec<String> = self.0.recientes.borrow().iter().take(5).cloned().collect();
        if self.0.recientes_vistos.borrow().as_ref() == Some(&top) {
            return;
        }
        *self.0.recientes_vistos.borrow_mut() = Some(top);
        let list = &self.0.recent_list;
        while let Some(c) = list.first_child() {
            list.remove(&c);
        }
        let recientes = self.0.recientes.borrow();
        if recientes.is_empty() {
            list.append(
                &gtk::Label::builder()
                    .label("Sin notas recientes")
                    .xalign(0.0)
                    .css_classes(["orbital-count"])
                    .build(),
            );
            return;
        }
        for ruta in recientes.iter().take(5) {
            let nombre = ruta.rsplit('/').next().unwrap_or(ruta);
            let fila = gtk::Box::new(gtk::Orientation::Horizontal, 6);
            fila.append(&crate::iconos::Icono::new("archivo", 16));
            fila.append(
                &gtk::Label::builder()
                    .label(nombre)
                    .xalign(0.0)
                    .ellipsize(gtk::pango::EllipsizeMode::End)
                    .build(),
            );
            let b = gtk::Button::builder()
                .child(&fila)
                .tooltip_text(ruta.as_str())
                .css_classes(["flat", "orbital-list-item"])
                .build();
            b.update_property(&[gtk::accessible::Property::Label(&format!("Abrir {nombre}"))]);
            let weak = Rc::downgrade(&self.0);
            let ruta = ruta.clone();
            b.connect_clicked(move |_| {
                if let Some(i) = weak.upgrade() {
                    Window(i).show_path(&ruta);
                }
            });
            list.append(&b);
        }
    }

    /// «Cambiar orden» (maqueta): invierte el orden del árbol.
    pub(super) fn sort_tree(&self) {
        self.0.tree_reversed.set(!self.0.tree_reversed.get());
        let tree = self.0.tree.borrow().clone();
        self.set_list_model(tree);
    }

    /// «Colapsar carpetas» (maqueta): si hay carpetas abiertas, las cierra todas; si no, abre
    /// todas.
    pub(super) fn collapse_folders(&self) {
        let Some(model) = self.0.list_view.model() else {
            return;
        };
        let rows = || {
            (0..model.n_items()).filter_map(|n| model.item(n).and_downcast::<gtk::TreeListRow>())
        };
        if rows().any(|r| r.is_expanded()) {
            // De abajo hacia arriba: cerrar una carpeta saca a sus hijas de la lista.
            for n in (0..model.n_items()).rev() {
                if let Some(r) = model.item(n).and_downcast::<gtk::TreeListRow>() {
                    r.set_expanded(false);
                }
            }
        } else {
            let mut n = 0;
            while n < model.n_items() {
                if let Some(r) = model.item(n).and_downcast::<gtk::TreeListRow>()
                    && r.is_expandable()
                {
                    r.set_expanded(true);
                }
                n += 1;
            }
        }
    }

    /// «Crear carpeta»: pide el nombre y la crea donde se crean las notas nuevas.
    pub(super) fn ask_new_folder(&self) {
        let Some(vault) = self.0.vault.borrow().clone() else {
            return;
        };
        let this = self.clone();
        glib::spawn_future_local(async move {
            let field = gtk::Entry::builder().activates_default(true).build();
            field.update_property(&[gtk::accessible::Property::Label("Nombre de la carpeta")]);
            let dialog = adw::AlertDialog::new(Some("Carpeta nueva"), Some("¿Cómo se llama?"));
            dialog.set_extra_child(Some(&field));
            dialog.add_response("cancel", "Cancelar");
            dialog.add_response("create", "Crear");
            dialog.set_response_appearance("create", adw::ResponseAppearance::Suggested);
            dialog.set_default_response(Some("create"));
            dialog.set_close_response("cancel");
            let f = field.clone();
            glib::idle_add_local_once(move || {
                f.grab_focus();
            });
            if dialog.choose_future(Some(&this.0.window)).await != "create" {
                return;
            }
            let name = field.text().trim().to_owned();
            if name.is_empty() || name.contains('/') || name.starts_with('.') {
                return this.toast("El nombre no puede estar vacío, tener «/» ni empezar con «.»");
            }
            warn_portable(&this, &name);
            let dir = this
                .target_dir()
                .map_or_else(|| "wiki".to_owned(), |d| d.as_str().to_owned());
            let abs = vault.root().join(&dir).join(&name);
            let result = gio::spawn_blocking(move || std::fs::create_dir(abs)).await;
            match result {
                Ok(Ok(())) => this.toast(&format!("Carpeta «{name}» creada en {dir}/")),
                Ok(Err(e)) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                    this.toast(&format!("Ya existe «{name}» en {dir}/"))
                }
                _ => this.toast("No se pudo crear la carpeta"),
            }
        });
    }

    /// Copia archivos soltados sobre la lista a `raw/` (la única forma de agregar fuentes).
    pub(super) fn setup_raw_drop(&self) {
        let drop = gtk::DropTarget::new(
            gtk::gdk::FileList::static_type(),
            gtk::gdk::DragAction::COPY,
        );
        let this = self.clone();
        drop.connect_drop(move |_, value, _, _| {
            let Ok(list) = value.get::<gtk::gdk::FileList>() else {
                return false;
            };
            let Some(vault) = this.0.vault.borrow().clone() else {
                return false;
            };
            let paths: Vec<PathBuf> = list.files().iter().filter_map(|f| f.path()).collect();
            let this = this.clone();
            glib::spawn_future_local(async move {
                let result = gio::spawn_blocking(move || {
                    let ctx = write_ctx::now(Some(vault.config()));
                    paths
                        .iter()
                        .map(|p| vault.copy_into_raw(p, &ctx))
                        .collect::<Result<Vec<_>, _>>()
                })
                .await;
                match result {
                    Ok(Ok(copied)) => {
                        let first = copied.first().cloned();
                        this.reload(first).await;
                        this.toast(&format!("{} fuente(s) copiada(s) a raw/", copied.len()));
                    }
                    Ok(Err(e)) => this.toast(&format!("No se pudo copiar a raw/: {e}")),
                    Err(_) => this.toast("No se pudo copiar a raw/"),
                }
            });
            true
        });
        self.0.list_view.add_controller(drop);
    }

    /// Guarda archivos como adjuntos de la nota abierta y la vuelve a mostrar.
    pub(super) async fn attach_files(&self, files: Vec<(String, Vec<u8>)>) {
        self.0.code_editor.flush().await;
        if self.0.code_editor.has_pending() {
            return;
        }
        let Some(vault) = self.0.vault.borrow().clone() else {
            return;
        };
        let Some(note) = self
            .0
            .current_entry
            .borrow()
            .as_ref()
            .map(|e| e.path.clone())
        else {
            return self.toast("Abrí una nota antes de agregarle adjuntos");
        };
        let n = note.clone();
        let result = gio::spawn_blocking(move || {
            let ctx = write_ctx::now(Some(vault.config()));
            files
                .iter()
                .map(|(name, bytes)| vault.attach(&n, name, bytes, &ctx))
                .collect::<Result<Vec<_>, _>>()
        })
        .await;
        match result {
            Ok(Ok(saved)) => {
                self.reload(Some(note)).await;
                let names: Vec<&str> = saved.iter().map(RelPath::file_name).collect();
                self.toast(&format!("Adjunto guardado: {}", names.join(", ")));
            }
            Ok(Err(e)) => self.toast(&e.to_string()),
            Err(_) => self.toast("No se pudo guardar el adjunto"),
        }
    }

    /// «Pegar imagen como adjunto»: la imagen del portapapeles se guarda como PNG.
    pub(super) fn paste_image(&self) {
        let this = self.clone();
        glib::spawn_future_local(async move {
            let clipboard = this.0.window.clipboard();
            let texture = match clipboard.read_texture_future().await {
                Ok(Some(t)) => t,
                _ => return this.toast("El portapapeles no tiene una imagen"),
            };
            let bytes = texture.save_to_png_bytes().to_vec();
            let ctx = write_ctx::now(None);
            let name = format!(
                "pegado-{}-{:02}{:02}{:02}.png",
                ctx.date, ctx.time.hour, ctx.time.minute, ctx.time.second
            );
            this.attach_files(vec![(name, bytes)]).await;
        });
    }

    /// Soltar archivos sobre la nota los guarda como adjuntos.
    pub(super) fn setup_drop(&self, target: &impl IsA<gtk::Widget>) {
        let drop = gtk::DropTarget::new(
            gtk::gdk::FileList::static_type(),
            gtk::gdk::DragAction::COPY,
        );
        let this = self.clone();
        drop.connect_drop(move |_, value, _, _| {
            let Ok(list) = value.get::<gtk::gdk::FileList>() else {
                return false;
            };
            let paths: Vec<PathBuf> = list.files().iter().filter_map(|f| f.path()).collect();
            let this = this.clone();
            glib::spawn_future_local(async move {
                let read = gio::spawn_blocking(move || {
                    paths
                        .iter()
                        .filter_map(|p| {
                            let name = p.file_name()?.to_string_lossy().into_owned();
                            std::fs::read(p).ok().map(|b| (name, b))
                        })
                        .collect::<Vec<_>>()
                })
                .await
                .unwrap_or_default();
                if !read.is_empty() {
                    this.attach_files(read).await;
                }
            });
            true
        });
        target.add_controller(drop);
    }

    pub fn show_path(&self, rel: &str) {
        // Elegir la fila la abre (ver `selection-changed`); si no está en la lista (por ejemplo,
        // archivada) o ya estaba elegida (otra coincidencia de la nota abierta, o volver desde el
        // grafo), se muestra directamente.
        if !self.reveal(rel)
            && let Some(e) = self.entry(rel)
        {
            self.show_entry(e);
        }
    }

    /// Expande las carpetas hasta `rel` y elige su fila. Devuelve si cambió la selección (y con
    /// eso `selection-changed` abre la nota).
    pub(super) fn reveal(&self, rel: &str) -> bool {
        let Some(selection) = self
            .0
            .list_view
            .model()
            .and_downcast::<gtk::SingleSelection>()
        else {
            return false;
        };
        let find = |path: &str| -> Option<(u32, gtk::TreeListRow)> {
            (0..selection.n_items()).find_map(|i| {
                let row = selection.item(i).and_downcast::<gtk::TreeListRow>()?;
                let item = row.item().and_downcast::<gtk::StringObject>()?;
                (item.string() == path).then_some((i, row))
            })
        };
        // Cada carpeta antepasada, de afuera hacia adentro.
        let mut prefix = String::new();
        let parts: Vec<&str> = rel.split('/').collect();
        for part in &parts[..parts.len().saturating_sub(1)] {
            if !prefix.is_empty() {
                prefix.push('/');
            }
            prefix.push_str(part);
            if let Some((_, row)) = find(&prefix) {
                row.set_expanded(true);
            }
        }
        match find(rel) {
            Some((pos, _)) => {
                let changed = selection.selected() != pos;
                selection.set_selected(pos);
                self.0
                    .list_view
                    .scroll_to(pos, gtk::ListScrollFlags::NONE, None);
                changed
            }
            None => false,
        }
    }
}
