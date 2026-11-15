//! Conexiones de señales de la ventana recién armada: paneles, búsqueda, archivos externos,
//! grafo, editor, pestañas, barra derecha y pie de bóveda.

use super::*;

/// Conecta los widgets de `this` entre sí y con la ventana (se llama una vez, al final de
/// [`Window::new`]).
pub(super) fn conectar(this: &Window, app: &adw::Application) {
    let weak = Rc::downgrade(&this.0);
    this.0.tag_panel.connect_rename({
        let weak = weak.clone();
        move |tag| {
            if let Some(i) = weak.upgrade() {
                Window(i).ask_tag_rename(tag.to_owned());
            }
        }
    });
    this.0.tag_panel.connect_selected(move |tag| {
        if let Some(i) = weak.upgrade() {
            Window(i).search_tag(tag);
        }
    });
    let weak = Rc::downgrade(&this.0);
    this.0.search_panel.connect_open_match(move |path, line| {
        let Some(i) = weak.upgrade() else { return };
        let w = Window(i);
        w.0.pending_line.set(Some((path.clone(), line)));
        w.show_path(path.as_str());
    });

    let weak = Rc::downgrade(&this.0);
    this.0.search_panel.connect_raw_toggled(move |active| {
        let Some(i) = weak.upgrade() else { return };
        if i.persist.get() {
            let mut s = AppState::load();
            s.search_raw = active;
            glib::spawn_future_local(async move {
                let _ = gio::spawn_blocking(move || s.save()).await;
            });
        }
    });

    // Archivos externos por URL (009): asociar/quitar/abrir y volver al archivo.
    {
        let weak = Rc::downgrade(&this.0);
        this.0.asociar_url.connect_clicked(move |_| {
            let Some(i) = weak.upgrade() else { return };
            let Some(ruta) = i.externo_ruta.borrow().clone() else {
                return;
            };
            let w = Window(i);
            w.asociar_url_dialog(&ruta);
        });
        let weak = Rc::downgrade(&this.0);
        this.0.quitar_url.connect_clicked(move |_| {
            let Some(i) = weak.upgrade() else { return };
            let Some(ruta) = i.externo_ruta.borrow().clone() else {
                return;
            };
            let w = Window(i.clone());
            let Some(vault) = i.vault.borrow().clone() else {
                return;
            };
            glib::spawn_future_local(async move {
                let (v, r) = (vault, ruta.clone());
                let resultado =
                    gio::spawn_blocking(move || v.quitar_url(&r, &write_ctx::now(None))).await;
                match resultado {
                    Ok(Ok(())) => {
                        i.content_stack.set_visible_child_name("externo");
                    }
                    _ => w.toast("No se pudo quitar la URL"),
                }
            });
        });
        let weak = Rc::downgrade(&this.0);
        this.0.url_ver_archivo.connect_clicked(move |_| {
            if let Some(i) = weak.upgrade() {
                i.content_stack.set_visible_child_name("externo");
            }
        });
        let weak = Rc::downgrade(&this.0);
        this.0.url_abrir.connect_clicked(move |_| {
            let Some(i) = weak.upgrade() else { return };
            let Some(ruta) = i.externo_ruta.borrow().clone() else {
                return;
            };
            let Some(vault) = i.vault.borrow().clone() else {
                return;
            };
            glib::spawn_future_local(async move {
                let url = gio::spawn_blocking(move || vault.url_de(&ruta))
                    .await
                    .ok()
                    .flatten();
                if let Some(url) = url {
                    gtk::UriLauncher::new(&url).launch(
                        None::<&gtk::Window>,
                        None::<&gio::Cancellable>,
                        |_| {},
                    );
                }
            });
        });
    }

    // Grafo (008): clic abre la nota (cierra el grafo), Esc/botón vuelve, aviso de recorte.
    {
        let weak = Rc::downgrade(&this.0);
        this.0.grafo_vista.set_on_abrir(move |ruta| {
            if let Some(i) = weak.upgrade() {
                i.content_stack.set_visible_child_name("codigo");
                Window(i).show_path(ruta);
            }
        });
        let weak_cerrar = Rc::downgrade(&this.0);
        this.0.grafo_vista.set_on_cerrar(move || {
            if let Some(i) = weak_cerrar.upgrade() {
                i.content_stack.set_visible_child_name("codigo");
            }
        });
        let weak_boton = Rc::downgrade(&this.0);
        this.0.grafo_cerrar.connect_clicked(move |_| {
            if let Some(i) = weak_boton.upgrade() {
                i.content_stack.set_visible_child_name("codigo");
            }
        });
    }

    this.setup_actions(app);
    let content = this.0.content_stack.clone();
    this.setup_drop(&content);
    this.setup_raw_drop();
    this.setup_editor();
    this.setup_list();
    this.setup_status_counts();
    this.setup_pestanas();
    {
        let weak = Rc::downgrade(&this.0);
        this.0.pie_boveda.connect_elegir(move |ruta| {
            let Some(i) = weak.upgrade() else { return };
            let w = Window(i);
            glib::spawn_future_local(async move {
                // Lo pendiente de la nota abierta se guarda antes de cambiar de bóveda.
                w.0.code_editor.flush().await;
                if w.0.code_editor.has_pending() {
                    w.toast("La nota tiene cambios sin guardar: la bóveda no cambió");
                    if let Some(v) = w.0.vault.borrow().clone() {
                        w.0.pie_boveda
                            .set_boveda(v.root(), &AppState::load().recent);
                    }
                    return;
                }
                w.open_path(ruta).await;
            });
        });
    }
    {
        let weak = Rc::downgrade(&this.0);
        this.0.panel.connect_modo(move |_| {
            if let Some(i) = weak.upgrade() {
                Window(i).refresh_panel();
            }
        });
        let weak = Rc::downgrade(&this.0);
        this.0.panel.connect_abrir(move |ruta| {
            if let Some(i) = weak.upgrade() {
                Window(i).show_path(ruta);
            }
        });
    }
}
