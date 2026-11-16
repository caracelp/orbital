//! Ventana principal: pantalla inicial, lista de la bóveda y visor de notas.
//!
//! Toda la E/S (abrir la bóveda, leer notas, guardar el estado) corre con `gio::spawn_blocking`;
//! el hilo principal solo arma widgets.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;

use adw::prelude::*;
use gtk::{gio, glib};
use orbital_vault::props::{PropEntry, PropValue, Warning};
use orbital_vault::{DeletePlan, Entry, EntryKind, Format, Note, NoteMeta, RelPath, Vault};
use webkit6::prelude::*;

use crate::editor::props::display;
use crate::editor::{self, Editor};
use crate::html_view;
use crate::open_dialog::{self, Answer};
use crate::state::AppState;
use crate::watcher::{Batch, Watcher};
use crate::write_ctx;

/// Hijos de cada carpeta (`""` = raíz), ordenados: primero carpetas, después archivos.
type Tree = HashMap<String, Vec<Arc<Entry>>>;

mod acciones;
mod boveda;
mod busqueda;
mod centro;
mod conexiones;
mod enlaces;
mod esqueleto;
mod explorador;
mod externos;
mod historial;
mod izquierda;
mod listas;
mod nota;
mod operaciones;
mod paneles;
mod propiedades;
#[cfg(test)]
mod tests;
mod util;
use util::*;

struct Inner {
    window: adw::ApplicationWindow,
    toasts: adw::ToastOverlay,
    stack: gtk::Stack,
    split: adw::OverlaySplitView,
    status: crate::status_bar::StatusBar,
    /// Sección «Recientes» del explorador y sus rutas (las cinco últimas notas abiertas).
    recent_list: gtk::Box,
    panel: crate::panel_derecho::PanelDerecho,
    pestanas: crate::pestanas::Pestanas,
    /// Estado de vista para reponer cuando cargue la nota de esa pestaña.
    estado_pendiente: RefCell<Option<(RelPath, crate::pestanas::Estado)>>,
    recientes: RefCell<Vec<String>>,
    /// Lo que muestra la sección ahora: se reconstruye solo si cambia (reabrir la misma nota, o
    /// recargar la lista por un cambio en disco, no toca los widgets).
    recientes_vistos: RefCell<Option<Vec<String>>>,
    /// «Cambiar orden» del explorador: árbol en orden inverso.
    tree_reversed: Cell<bool>,
    pie_boveda: crate::pie_boveda::PieBoveda,
    content_title: adw::WindowTitle,
    banner: adw::Banner,
    list_view: gtk::ListView,
    editor: gtk::TextView,
    content_stack: gtk::Stack,
    code_editor: Editor,
    /// Subtítulo de la nota sin el estado del guardado.
    subtitle_base: RefCell<String>,
    /// La ventana ya guardó lo pendiente y puede cerrarse.
    closing: Cell<bool>,
    /// Evita abrir más de un diálogo de conflicto a la vez.
    conflict_dialog_open: Cell<bool>,
    /// Escrituras propias recientes (ruta → hash), que el monitor no debe tomar como externas.
    own_writes: RefCell<HashMap<RelPath, u64>>,
    props_list: gtk::ListBox,
    note_banner: adw::Banner,
    type_banner: adw::Banner,
    current_note: RefCell<Option<Note>>,
    /// Archivo que se está mostrando (nota o no).
    current_entry: RefCell<Option<Arc<Entry>>>,
    html_props: gtk::ListBox,
    new_button: gtk::MenuButton,
    external_page: adw::StatusPage,
    html_page: gtk::Box,
    /// Vista web compartida; `None` hasta abrir la primera nota HTML.
    web: RefCell<Option<webkit6::WebView>>,
    vault: RefCell<Option<Arc<Vault>>>,
    tree: RefCell<Rc<Tree>>,
    /// Si es `false` (modo de captura), no se guarda el estado de la app.
    persist: Cell<bool>,
    watcher: RefCell<Option<Watcher>>,
    /// La carpeta de la bóveda dejó de estar disponible (disco desconectado).
    gone: Cell<bool>,
    gone_banner: adw::Banner,
    /// Panel lateral derecho con los retroenlaces (US4 de la 003).
    right: crate::editor::right_sidebar::RightSidebar,
    right_split: adw::OverlaySplitView,
    /// Grafo de enlaces de la bóveda; `None` mientras se arma en segundo plano.
    graph: RefCell<Option<Arc<std::sync::Mutex<orbital_vault::LinkGraph>>>>,
    /// Cada bóveda nueva sube la generación: un grafo viejo que termina tarde se descarta.
    graph_gen: Cell<u64>,
    /// Línea a mostrar cuando termine de abrirse la nota (al elegir un retroenlace).
    pending_line: Cell<Option<(RelPath, usize)>>,
    /// Ancla (`encabezado` o `^id`) a mostrar cuando termine de abrirse la nota (026).
    pending_ancla: RefCell<Option<(RelPath, String)>>,
    /// Panel lateral izquierdo: stack de páginas (Archivos, Búsqueda, Etiquetas).
    sidebar_stack: adw::ViewStack,
    /// Panel de búsqueda en toda la bóveda (US1 de la 004).
    search_panel: crate::search_panel::SearchPanel,
    /// Índice de búsqueda en memoria; `None` mientras se arma en segundo plano.
    search_index: RefCell<Option<Arc<std::sync::RwLock<orbital_vault::SearchIndex>>>>,
    /// Generación del índice para descartar construcciones viejas.
    search_gen: Cell<u64>,
    tag_panel: crate::tag_panel::TagPanel,
    search_updates: RefCell<HashMap<RelPath, bool>>,
    search_updating: Cell<bool>,
    tag_operation: Cell<bool>,
    vault_session: Cell<u64>,
    tag_disk_changes: RefCell<Vec<PathBuf>>,
    /// Candidatos para el selector rápido de notas (US2 de la 004).
    candidates: RefCell<Vec<crate::editor::completion::CompletionCandidate>>,
    /// Vistas WebKit de exportación a PDF en curso (007): mantenerlas vivas hasta terminar.
    exports_pendientes: RefCell<Vec<webkit6::WebView>>,
    /// Datos y vista del grafo de enlaces (008).
    grafo_datos: RefCell<Option<std::rc::Rc<crate::grafo::GrafoDatos>>>,
    grafo_vista: crate::grafo::VistaGrafo,
    grafo_cerrar: gtk::Button,
    /// Presentación OnStage de la nota abierta (014): se crea con la ventana, oculta.
    onstage: std::rc::Rc<crate::onstage::OnStage>,
    /// Archivos externos por URL (009): vista del archivo y webview de la URL asociada.
    externo_ruta: RefCell<Option<RelPath>>,
    asociar_url: gtk::Button,
    quitar_url: gtk::Button,
    /// Importar a Markdown (015): sensible solo para extensiones soportadas.
    importar_md: gtk::Button,
    /// Ver el PDF (016): visible solo para `.pdf` en la vista externa.
    ver_pdf: gtk::Button,
    /// Visor PDF embebido (016).
    visor: std::rc::Rc<crate::visor_pdf::VisorPdf>,
    /// El PDF mostrado en el visor (para la búsqueda de resaltados).
    visor_ruta: RefCell<Option<RelPath>>,
    /// Webview de la URL asociada (009): se crea al primer uso (crear un `WebView` cuesta
    /// cientos de milisegundos de arranque) y va en `url_page`, debajo de su cabecera.
    url_web: RefCell<Option<webkit6::WebView>>,
    url_page: gtk::Box,
    url_abrir: gtk::Button,
    url_ver_archivo: gtk::Button,
    /// Menú «Más opciones de la nota»: se rearma al abrirse (027).
    note_menu: gtk::Popover,
    /// Favoritos y Guardados (027): widgets de la barra izquierda.
    listas_panel: crate::listas_panel::ListasPanel,
    /// Lo leído de `.orbital/listas.yaml` con los cambios en memoria; `None` hasta que carga.
    listas: RefCell<Option<orbital_vault::ListasSnapshot>>,
    /// Cambios que todavía no llegaron al disco (se repiten si el archivo cambió afuera).
    listas_acciones: RefCell<Vec<listas::AccionListas>>,
    /// Hay una escritura de las listas en curso.
    listas_escribiendo: Cell<bool>,
    /// Llegaron cambios durante la escritura.
    listas_pendiente: Cell<bool>,
    /// Hash de la última escritura propia (para ignorar su aviso en el monitor).
    listas_eco: Cell<Option<u64>>,
    /// Sesión de bóveda (`vault_session`) a la que pertenecen las listas.
    listas_sesion: Cell<u64>,
}

/// Ventana de Orbital.
#[derive(Clone)]
pub struct Window(Rc<Inner>);

impl Window {
    pub fn new(app: &adw::Application) -> Self {
        let centro::Centro {
            editor,
            props_list,
            content_stack,
            code_editor,
            html_props,
            html_page,
            asociar_url,
            quitar_url,
            importar_md,
            ver_pdf,
            external_page,
            url_ver_archivo,
            url_abrir,
            url_page,
            grafo_vista,
            grafo_cerrar,
            content_title,
            pestanas,
            type_banner,
            note_banner,
            right,
            panel,
            right_split,
            backlinks_button,
            note_menu,
        } = centro::construir_centro();
        let izquierda::Izquierda {
            list_view,
            search_panel,
            tag_panel,
            recent_list,
            sidebar_stack,
            banner,
            new_button,
            pie_boveda,
            sidebar_view,
            gone_banner,
            listas_panel,
        } = izquierda::construir_izquierda();
        let esqueleto::Esqueleto {
            split,
            status,
            root_view,
        } = esqueleto::construir_esqueleto(
            backlinks_button.clone(),
            right_split.clone(),
            sidebar_stack.clone(),
            sidebar_view.clone(),
        );
        // --- Pantalla inicial ---
        let start = start_page();

        // Homogéneo a propósito: la primera disposición de la interfaz de la bóveda ocurre
        // mientras la bóveda se lee en otro hilo (sin homogéneo, se suma después: +60 ms).
        let stack = gtk::Stack::builder()
            .transition_type(gtk::StackTransitionType::Crossfade)
            .build();
        stack.add_named(&start, Some("inicio"));
        stack.add_named(&root_view, Some("boveda"));

        let toasts = adw::ToastOverlay::new();
        toasts.set_child(Some(&stack));

        let window = adw::ApplicationWindow::builder()
            .application(app)
            .title("Orbital")
            .default_width(1000)
            .default_height(680)
            .width_request(360)
            .height_request(300)
            .content(&toasts)
            .build();

        // Anchos de la maqueta: 260/280 px y, en ventanas más chicas, 220/220 y 190/170. La
        // maqueta lo hace a 900 y 680 px; acá el centro tiene un ancho mínimo real (la columna del
        // editor y las propiedades), así que los cortes van antes y, si no entra, la barra
        // derecha y después la izquierda se superponen al centro en vez de recortarlo.
        let ancho_maximo = |px: f64| {
            adw::Breakpoint::new(adw::BreakpointCondition::new_length(
                adw::BreakpointConditionLengthType::MaxWidth,
                px,
                adw::LengthUnit::Px,
            ))
        };
        for (px, izq, der) in [(1180.0, 220.0, 220.0), (1060.0, 190.0, 170.0)] {
            let bp = ancho_maximo(px);
            for (vista, ancho) in [(&split, izq), (&right_split, der)] {
                bp.add_setter(vista, "min-sidebar-width", Some(&ancho.to_value()));
                bp.add_setter(vista, "max-sidebar-width", Some(&ancho.to_value()));
            }
            window.add_breakpoint(bp);
        }
        let bp = ancho_maximo(1040.0);
        bp.add_setter(&right_split, "collapsed", Some(&true.to_value()));
        bp.add_setter(&split, "min-sidebar-width", Some(&190.0.to_value()));
        bp.add_setter(&split, "max-sidebar-width", Some(&190.0.to_value()));
        window.add_breakpoint(bp);
        let bp = ancho_maximo(760.0);
        bp.add_setter(&split, "collapsed", Some(&true.to_value()));
        bp.add_setter(&right_split, "collapsed", Some(&true.to_value()));
        window.add_breakpoint(bp);

        // OnStage (014): ventana separada, oculta hasta «Presentar con OnStage». El material
        // vivo (texto de la nota y bóveda) lo arma la ventana en cada refresco.
        let onstage = std::rc::Rc::new(crate::onstage::OnStage::new(&window));

        // Visor PDF (016): página del stack; el webview y el esquema se crean al primer uso.
        let visor = std::rc::Rc::new(crate::visor_pdf::VisorPdf::new());
        content_stack.add_named(visor.page(), Some("pdf"));

        let this = Self(Rc::new(Inner {
            window,
            toasts,
            stack,
            split,
            status,
            recent_list,
            panel,
            pestanas,
            estado_pendiente: RefCell::new(None),
            recientes: RefCell::new(Vec::new()),
            recientes_vistos: RefCell::new(None),
            tree_reversed: Cell::new(false),
            pie_boveda,
            content_title,
            banner,
            list_view,
            editor,
            content_stack,
            code_editor,
            subtitle_base: RefCell::new(String::new()),
            closing: Cell::new(false),
            conflict_dialog_open: Cell::new(false),
            own_writes: RefCell::new(HashMap::new()),
            props_list,
            note_banner,
            type_banner,
            current_note: RefCell::new(None),
            current_entry: RefCell::new(None),
            right,
            right_split,
            graph: RefCell::new(None),
            graph_gen: Cell::new(0),
            pending_line: Cell::new(None),
            pending_ancla: RefCell::new(None),
            sidebar_stack,
            search_panel: search_panel.clone(),
            search_index: RefCell::new(None),
            search_gen: Cell::new(0),
            tag_panel,
            search_updates: RefCell::new(HashMap::new()),
            search_updating: Cell::new(false),
            tag_operation: Cell::new(false),
            vault_session: Cell::new(0),
            tag_disk_changes: RefCell::new(Vec::new()),
            candidates: RefCell::new(Vec::new()),
            exports_pendientes: RefCell::new(Vec::new()),
            grafo_datos: RefCell::new(None),
            grafo_vista,
            grafo_cerrar,
            onstage,
            externo_ruta: RefCell::new(None),
            asociar_url,
            quitar_url,
            importar_md,
            ver_pdf,
            visor,
            visor_ruta: RefCell::new(None),
            url_web: RefCell::new(None),
            url_page: url_page.clone(),
            url_abrir,
            url_ver_archivo,
            html_props,
            new_button,
            external_page,
            html_page,
            web: RefCell::new(None),
            vault: RefCell::new(None),
            tree: RefCell::new(Rc::new(Tree::new())),
            persist: Cell::new(true),
            watcher: RefCell::new(None),
            gone: Cell::new(false),
            gone_banner,
            note_menu,
            listas_panel,
            listas: RefCell::new(None),
            listas_acciones: RefCell::new(Vec::new()),
            listas_escribiendo: Cell::new(false),
            listas_pendiente: Cell::new(false),
            listas_eco: Cell::new(None),
            listas_sesion: Cell::new(0),
        }));
        conexiones::conectar(&this, app);
        this
    }

    pub fn present(&self) {
        self.0.window.present();
    }

    /// Medición del arranque (desarrollo): fases del próximo cuadro.
    pub fn fases_del_proximo_cuadro(&self) {
        crate::medir_arranque::fases_del_proximo_cuadro(&self.0.window);
    }

    fn remember(&self, path: PathBuf) {
        if self.0.persist.get() {
            save_state_opened(path);
        }
    }

    /// Modo de captura: no guarda el estado de la app.
    pub fn set_persist(&self, persist: bool) {
        self.0.persist.set(persist);
    }

    /// Avisa con un botón «Deshacer» que ejecuta `undo`.
    fn toast_undo(&self, msg: &str, undo: impl Fn() + 'static) {
        // Sin tiempo límite (WCAG 2.2.1): queda hasta que se descarta o lo reemplaza otro aviso.
        let toast = adw::Toast::builder()
            .title(msg)
            .button_label("Deshacer")
            .timeout(0)
            .priority(adw::ToastPriority::High)
            .build();
        toast.connect_button_clicked(move |_| undo());
        self.0.toasts.add_toast(toast);
    }

    /// Desarrollo: latencia al escribir en la nota abierta (ver `Editor::measure_typing`).
    pub async fn measure_typing(&self, chars: usize) -> Vec<(f64, f64)> {
        self.0.code_editor.measure_typing(chars).await
    }

    /// Ejecuta `f` cuando termina de dibujarse el próximo cuadro de la ventana.
    pub fn after_next_frame(&self, f: impl FnOnce() + 'static) {
        let f = std::cell::Cell::new(Some(f));
        self.0.window.add_tick_callback(move |_, _| {
            if let Some(f) = f.take() {
                glib::idle_add_local_once(f);
            }
            glib::ControlFlow::Break
        });
    }

    /// Dibuja la ventana en un PNG, sin depender de capturas del compositor.
    pub fn capture(&self, out: &Path) -> Result<(), String> {
        let w = &self.0.window;
        let (width, height) = (w.width(), w.height());
        let paintable = gtk::WidgetPaintable::new(Some(w));
        let snapshot = gtk::Snapshot::new();
        paintable.snapshot(&snapshot, f64::from(width), f64::from(height));
        let node = snapshot.to_node().ok_or("la ventana no dibujó nada")?;
        let renderer = w.renderer().ok_or("la ventana no tiene renderizador")?;
        let bounds = gtk::graphene::Rect::new(0.0, 0.0, width as f32, height as f32);
        let texture = renderer.render_texture(node, Some(&bounds));
        texture.save_to_png(out).map_err(|e| e.to_string())
    }

    fn toast(&self, msg: &str) {
        self.0.toasts.add_toast(adw::Toast::new(msg));
    }
}
