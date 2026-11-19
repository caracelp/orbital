//! Editor de ecuaciones (051): un diálogo con el LaTeX en un campo de texto, la fórmula dibujada
//! en vivo (motor de la 050) y una paleta de símbolos y plantillas que insertan LaTeX. La última
//! página convierte cantidades entre unidades.
//!
//! No es un editor estructural: se edita el LaTeX; la paleta solo escribe en el campo. Se arma
//! recién cuando se abre (nada al arrancar) y sus estilos se cargan con el primero.
//!
//! La página «Gráfico» (052) edita un bloque `grafico`: las expresiones y opciones en un campo,
//! el gráfico en vivo (interactivo, con sus deslizadores) y plantillas para cada tipo, también
//! las del espacio (053: superficies, curvas, implícitas y nubes de puntos). Se arma
//! al mostrarse por primera vez.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

use adw::prelude::*;
use gtk::{gio, glib};
use orbital_formulas::{paleta, siunitx, unidades};

use crate::formulas::{self, Clave, Formula, Modo};

/// Pausa después de la última tecla antes de volver a dibujar.
const PAUSA: Duration = Duration::from_millis(90);
/// La fórmula de la vista previa, más grande que el texto.
const AUMENTO: f32 = 1.9;

/// Con qué se abre el editor.
pub enum Apertura {
    /// Editar una fórmula de la nota: solo la página «Ecuación».
    Formula { tex: String, modo: Modo },
    /// Editar un bloque `grafico` (052): solo la página «Gráfico».
    Grafico { fuente: String },
    /// Insertar algo nuevo: las dos páginas; `grafico` dice con cuál empieza.
    Insertar {
        tex: String,
        modo: Modo,
        grafico: bool,
    },
}

/// Lo que se aceptó.
pub enum Aceptado {
    /// El LaTeX y el modo elegido.
    Formula(String, Modo),
    /// Los renglones de un bloque `grafico` (052).
    Grafico(String),
}

type AlAceptar = Box<dyn Fn(Aceptado)>;

/// Pausa entre la última tecla y volver a dibujar el gráfico.
const PAUSA_GRAFICO: Duration = Duration::from_millis(150);
/// Alto máximo del gráfico de la página.
const ALTO_GRAFICO: i32 = 250;
/// Lo que trae un gráfico nuevo.
const GRAFICO_NUEVO: &str = "y = x^2";

/// Plantillas de la página «Gráfico»: (etiqueta, nombre accesible, texto).
const PLANTILLAS: &[(&str, &str, &str)] = &[
    ("y = f(x)", "Función", "y = x^2"),
    ("r = f(θ)", "Polar", "r = 1 + \\cos\\theta"),
    ("(x(t), y(t))", "Paramétrica", "(\\cos t, \\sin 2t)"),
    ("F(x, y) = 0", "Implícita", "x^2 + y^2 = 4"),
    ("(a, b)", "Puntos", "(1, 2), (2, 3), (3, 5)"),
    (
        "datos",
        "Tabla de datos",
        "datos: líneas\n0 0\n1 2\n2 3\n3 5",
    ),
    ("z = f(x, y)", "Superficie", "z = \\sin(x)\\cos(y)"),
    (
        "(x(t), y(t), z(t))",
        "Curva en el espacio",
        "(\\cos t, \\sin t, t/6) ; t: 0..6π",
    ),
    (
        "(x(u, v), y(u, v), z(u, v))",
        "Superficie paramétrica",
        "(\\cos u \\sin v, \\sin u \\sin v, \\cos v) ; v: 0..π",
    ),
    (
        "F(x, y, z) = 0",
        "Superficie implícita",
        "x^2 + y^2 + z^2 = 4",
    ),
    ("(a, b, c)", "Puntos en el espacio", "(1, 2, 3), (2, 1, 0)"),
    (
        "datos x y z",
        "Nube de puntos",
        "datos: nube\n0 0 0\n1 2 3\n2 1 1",
    ),
    ("a = 1 (0..5)", "Deslizador", "a = 1 (0..5)"),
    ("x: a..b", "Rango de x", "x: -10..10"),
    ("y: a..b", "Rango de y", "y: -5..5"),
    ("θ: a..b", "Rango de θ", "θ: 0..2π"),
    ("t: a..b", "Rango de t", "t: 0..2π"),
    ("z: a..b", "Rango de z", "z: -5..5"),
    ("cámara", "Ángulo de la cámara", "cámara: -60, 28"),
    ("resolución", "Resolución de la malla", "resolución: 80"),
    ("título", "Título", "título: "),
    ("grilla polar", "Grilla polar", "grilla: polar"),
    ("; color", "Color de la serie", " ; color: celeste"),
    ("; nombre", "Nombre de la serie", " ; nombre: "),
    (
        "; discontinua",
        "Línea discontinua",
        " ; estilo: discontinua",
    ),
];

/// La página «Gráfico».
struct PaginaGrafico {
    raiz: gtk::Widget,
    texto: gtk::TextView,
    vista: gtk::Box,
    error: gtk::Label,
    generacion: Cell<u64>,
    actual: RefCell<Option<crate::graficos::GraficoVista>>,
}

struct Inner {
    dialogo: adw::Dialog,
    texto: gtk::TextView,
    imagen: gtk::Picture,
    error: gtk::Label,
    aceptar: gtk::Button,
    modo: adw::ToggleGroup,
    paginas: gtk::Stack,
    /// Páginas de la paleta que todavía no tienen sus botones.
    sin_llenar: RefCell<Vec<(&'static paleta::Categoria, gtk::FlowBox)>>,
    conversor: Conversor,
    generacion: Cell<u64>,
    al_aceptar: RefCell<Option<AlAceptar>>,
    /// Las páginas «Ecuación» y «Gráfico» (052).
    secciones: adw::ViewStack,
    /// Dónde va la página «Gráfico» (se arma al mostrarse).
    contenedor_grafico: gtk::Box,
    grafico: RefCell<Option<Rc<PaginaGrafico>>>,
    grafico_inicial: RefCell<String>,
}

/// La página de conversión de unidades.
struct Conversor {
    valor: gtk::Entry,
    desde: gtk::Entry,
    hacia: gtk::Entry,
    resultado: gtk::Label,
    sugerencias: gtk::FlowBox,
    insertar: gtk::Button,
    /// La cantidad del campo que se está convirtiendo (rango en bytes y comando).
    origen: RefCell<Option<siunitx::CantidadEn>>,
    /// Las unidades sugeridas, en el orden de sus botones.
    lista: RefCell<Vec<&'static str>>,
}

/// El editor abierto.
#[derive(Clone)]
pub struct EditorEcuaciones(Rc<Inner>);

static CSS: std::sync::Once = std::sync::Once::new();

fn cargar_estilos() {
    CSS.call_once(|| {
        use crate::style::*;
        let css = format!(
            ".orbital-ecuacion-vista {{ background: {view}; border-radius: 10px; border: 1px solid {line}; }}\n\
             .orbital-ecuacion-texto, .orbital-ecuacion-texto text {{ background: {view}; }}\n\
             .orbital-ecuacion-texto {{ border-radius: 8px; border: 1px solid {line}; padding: 8px; }}\n\
             flowbox.orbital-paleta {{ padding: 4px; }}\n\
             flowboxchild.orbital-simbolo {{ min-width: 44px; min-height: 36px; padding: 2px 8px; border-radius: 8px; background: {card}; }}\n\
             flowboxchild.orbital-simbolo:hover {{ background: {hover}; color: {pink}; }}\n\
             flowboxchild.orbital-simbolo:active {{ background: {pressed}; }}\n\
             .orbital-ecuacion-error {{ color: {pink}; }}\n",
            view = VIEW_BG.hex(),
            line = LINE.hex(),
            card = CARD_BG.hex(),
            hover = HOVER_BG.hex(),
            pressed = PRESSED_BG.hex(),
            pink = ACCENT_TEXT.hex(),
        );
        let provider = gtk::CssProvider::new();
        provider.load_from_string(&css);
        if let Some(display) = gtk::gdk::Display::default() {
            gtk::style_context_add_provider_for_display(
                &display,
                &provider,
                gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
            );
        }
    });
}

fn etiquetado(w: &impl IsA<gtk::Accessible>, nombre: &str) {
    w.update_property(&[gtk::accessible::Property::Label(nombre)]);
}

impl EditorEcuaciones {
    /// Abre el editor con `inicial`. `editando`: se está cambiando una fórmula que ya está en la
    /// nota (su modo no cambia); si no, se elige en línea o en bloque. `al_aceptar` recibe el
    /// LaTeX; `al_cerrar` corre siempre al cerrar (aceptado o no).
    #[cfg(test)]
    pub fn presentar(
        padre: &impl IsA<gtk::Widget>,
        inicial: &str,
        modo: Modo,
        editando: bool,
        al_aceptar: impl Fn(String, Modo) + 'static,
        al_cerrar: impl Fn() + 'static,
    ) -> Self {
        let apertura = if editando {
            Apertura::Formula {
                tex: inicial.to_owned(),
                modo,
            }
        } else {
            Apertura::Insertar {
                tex: inicial.to_owned(),
                modo,
                grafico: false,
            }
        };
        Self::presentar_con(
            padre,
            apertura,
            move |r| {
                if let Aceptado::Formula(tex, modo) = r {
                    al_aceptar(tex, modo);
                }
            },
            al_cerrar,
        )
    }

    /// Abre el editor según `apertura`. `al_aceptar` recibe la fórmula o el gráfico;
    /// `al_cerrar` corre siempre al cerrar (aceptado o no).
    pub fn presentar_con(
        padre: &impl IsA<gtk::Widget>,
        apertura: Apertura,
        al_aceptar: impl Fn(Aceptado) + 'static,
        al_cerrar: impl Fn() + 'static,
    ) -> Self {
        cargar_estilos();
        let (inicial, modo, editando, grafico_inicial, con_ecuacion, con_grafico, en_grafico) =
            match apertura {
                Apertura::Formula { tex, modo } => {
                    (tex, modo, true, String::new(), true, false, false)
                }
                Apertura::Grafico { fuente } => (
                    String::new(),
                    Modo::EnLinea,
                    true,
                    fuente,
                    false,
                    true,
                    true,
                ),
                Apertura::Insertar { tex, modo, grafico } => (
                    tex,
                    modo,
                    false,
                    GRAFICO_NUEVO.to_owned(),
                    true,
                    true,
                    grafico,
                ),
            };
        let inicial = inicial.as_str();
        let titulo = match (editando, con_ecuacion) {
            (true, true) => "Editar ecuación",
            (true, false) => "Editar gráfico",
            (false, _) if en_grafico => "Insertar gráfico",
            _ => "Insertar ecuación",
        };
        let dialogo = adw::Dialog::builder()
            .title(titulo)
            .content_width(780)
            .content_height(660)
            .build();

        let cancelar = gtk::Button::with_mnemonic("_Cancelar");
        let aceptar = gtk::Button::builder()
            .label(if editando { "Aplicar" } else { "Insertar" })
            .css_classes(["suggested-action"])
            .tooltip_text("Ctrl+Intro")
            .build();
        let cabecera = adw::HeaderBar::builder()
            .show_start_title_buttons(false)
            .show_end_title_buttons(false)
            .build();
        cabecera.pack_start(&cancelar);
        cabecera.pack_end(&aceptar);

        // Vista previa.
        let imagen = gtk::Picture::builder()
            .can_shrink(false)
            .halign(gtk::Align::Center)
            .valign(gtk::Align::Center)
            .margin_top(12)
            .margin_bottom(12)
            .margin_start(12)
            .margin_end(12)
            .build();
        let marco = gtk::ScrolledWindow::builder()
            .vscrollbar_policy(gtk::PolicyType::Automatic)
            .hscrollbar_policy(gtk::PolicyType::Automatic)
            .min_content_height(120)
            .max_content_height(220)
            .propagate_natural_height(true)
            .child(&imagen)
            .css_classes(["orbital-ecuacion-vista"])
            .build();
        let error = gtk::Label::builder()
            .wrap(true)
            .xalign(0.0)
            .visible(false)
            .css_classes(["orbital-ecuacion-error"])
            .build();
        error.update_property(&[gtk::accessible::Property::Label("Error en la fórmula")]);

        // El LaTeX.
        let texto = gtk::TextView::builder()
            .monospace(true)
            .wrap_mode(gtk::WrapMode::WordChar)
            .accepts_tab(false)
            .top_margin(4)
            .bottom_margin(4)
            .left_margin(4)
            .right_margin(4)
            .css_classes(["orbital-ecuacion-texto"])
            .build();
        etiquetado(&texto, "LaTeX de la ecuación");
        texto.buffer().set_text(inicial);
        let desplazar_texto = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .min_content_height(72)
            .max_content_height(140)
            .propagate_natural_height(true)
            .child(&texto)
            .build();
        let rotulo_texto = gtk::Label::builder()
            .label("_LaTeX")
            .use_underline(true)
            .mnemonic_widget(&texto)
            .xalign(0.0)
            .css_classes(["heading"])
            .build();

        let modo_grupo = adw::ToggleGroup::new();
        modo_grupo.add(
            adw::Toggle::builder()
                .name("linea")
                .label("En línea")
                .tooltip("En el renglón: $…$")
                .build(),
        );
        modo_grupo.add(
            adw::Toggle::builder()
                .name("bloque")
                .label("En bloque")
                .tooltip("En su propio renglón: $$…$$")
                .build(),
        );
        modo_grupo.set_active_name(Some(match modo {
            Modo::EnLinea => "linea",
            Modo::Bloque => "bloque",
        }));
        modo_grupo.set_visible(!editando);
        etiquetado(
            modo_grupo.upcast_ref::<gtk::Widget>(),
            "Cómo va la ecuación",
        );
        let fila_texto = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        rotulo_texto.set_hexpand(true);
        fila_texto.append(&rotulo_texto);
        fila_texto.append(&modo_grupo);

        // La paleta: categorías a la izquierda, botones a la derecha.
        let paginas = gtk::Stack::builder()
            .transition_type(gtk::StackTransitionType::None)
            .hexpand(true)
            .vexpand(true)
            .build();
        let barra = gtk::StackSidebar::builder().stack(&paginas).build();
        barra.set_size_request(170, -1);
        etiquetado(&barra, "Categorías de la paleta");
        let mut sin_llenar = Vec::new();
        let mut primera = None;
        for (k, cat) in paleta::CATEGORIAS.iter().enumerate() {
            let caja = gtk::FlowBox::builder()
                .selection_mode(gtk::SelectionMode::None)
                .activate_on_single_click(true)
                .homogeneous(false)
                .row_spacing(6)
                .column_spacing(6)
                .max_children_per_line(30)
                .valign(gtk::Align::Start)
                .css_classes(["orbital-paleta"])
                .build();
            etiquetado(&caja, cat.nombre);
            let desplazar = gtk::ScrolledWindow::builder()
                .hscrollbar_policy(gtk::PolicyType::Never)
                .child(&caja)
                .build();
            paginas.add_titled(&desplazar, Some(cat.id), cat.nombre);
            if k == 0 {
                llenar(&caja, cat);
                primera = Some((cat, caja));
            } else {
                sin_llenar.push((cat, caja));
            }
        }
        let conversor = Conversor::new();
        paginas.add_titled(&conversor.widget(), Some("convertir"), "Convertir unidades");
        let paleta_caja = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        paleta_caja.append(&barra);
        paleta_caja.append(&gtk::Separator::new(gtk::Orientation::Vertical));
        paleta_caja.append(&paginas);
        let paleta_marco = gtk::Frame::builder()
            .child(&paleta_caja)
            .vexpand(true)
            .build();

        let cuerpo = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(10)
            .margin_top(6)
            .margin_bottom(16)
            .margin_start(16)
            .margin_end(16)
            .build();
        cuerpo.append(&marco);
        cuerpo.append(&error);
        cuerpo.append(&fila_texto);
        cuerpo.append(&desplazar_texto);
        cuerpo.append(&paleta_marco);

        // Las páginas «Ecuación» y «Gráfico» (052): con las dos, un selector en la cabecera.
        let secciones = adw::ViewStack::new();
        let contenedor_grafico = gtk::Box::new(gtk::Orientation::Vertical, 0);
        if con_ecuacion {
            secciones.add_titled(&cuerpo, Some("ecuacion"), "Ecuación");
        }
        if con_grafico {
            secciones.add_titled(&contenedor_grafico, Some("grafico"), "Gráfico");
        }
        if con_ecuacion && con_grafico {
            let selector = adw::ViewSwitcher::builder()
                .stack(&secciones)
                .policy(adw::ViewSwitcherPolicy::Wide)
                .build();
            etiquetado(selector.upcast_ref::<gtk::Widget>(), "Qué insertar");
            cabecera.set_title_widget(Some(&selector));
        }

        let vista = adw::ToolbarView::new();
        vista.add_top_bar(&cabecera);
        vista.set_content(Some(&secciones));
        dialogo.set_child(Some(&vista));

        let this = Self(Rc::new(Inner {
            dialogo: dialogo.clone(),
            texto: texto.clone(),
            imagen,
            error,
            aceptar: aceptar.clone(),
            modo: modo_grupo,
            paginas: paginas.clone(),
            sin_llenar: RefCell::new(sin_llenar),
            conversor,
            generacion: Cell::new(0),
            al_aceptar: RefCell::new(Some(Box::new(al_aceptar))),
            secciones: secciones.clone(),
            contenedor_grafico,
            grafico: RefCell::new(None),
            grafico_inicial: RefCell::new(grafico_inicial),
        }));

        // Señales.
        {
            let d = dialogo.clone();
            cancelar.connect_clicked(move |_| {
                d.close();
            });
        }
        {
            let this = this.clone();
            aceptar.connect_clicked(move |_| this.aceptar());
        }
        {
            let this = this.clone();
            texto.buffer().connect_changed(move |_| this.programar());
        }
        {
            let this = this.clone();
            paginas.connect_visible_child_name_notify(move |_| this.al_cambiar_pagina());
        }
        if let Some((cat, caja)) = primera {
            this.conectar_caja(&caja, cat);
        }
        this.conectar_conversor();
        let atajos = gtk::ShortcutController::new();
        atajos.set_scope(gtk::ShortcutScope::Managed);
        {
            let this = this.clone();
            atajos.add_shortcut(gtk::Shortcut::new(
                gtk::ShortcutTrigger::parse_string("<Control>Return"),
                Some(gtk::CallbackAction::new(move |_, _| {
                    this.aceptar();
                    glib::Propagation::Stop
                })),
            ));
        }
        dialogo.add_controller(atajos);
        dialogo.connect_closed(move |_| al_cerrar());
        {
            let this = this.clone();
            secciones.connect_visible_child_name_notify(move |_| this.al_cambiar_seccion());
        }

        if en_grafico {
            secciones.set_visible_child_name("grafico");
            this.al_cambiar_seccion();
        }
        this.actualizar_aceptar();
        if con_ecuacion {
            this.dibujar_ya();
        }
        dialogo.present(Some(padre));
        if en_grafico {
            this.enfocar_grafico();
        } else {
            // El foco, en el LaTeX, con el cursor al final.
            let b = texto.buffer();
            b.place_cursor(&b.end_iter());
            dialogo.set_focus(Some(&texto));
        }
        this
    }

    fn en_grafico(&self) -> bool {
        self.0.secciones.visible_child_name().as_deref() == Some("grafico")
    }

    /// Se mostró otra página: la del gráfico se arma la primera vez.
    fn al_cambiar_seccion(&self) {
        if self.en_grafico() {
            if self.0.grafico.borrow().is_none() {
                let inicial = self.0.grafico_inicial.borrow().clone();
                let p = self.armar_grafico(&inicial);
                self.0.contenedor_grafico.append(&p.raiz);
                self.0.grafico.replace(Some(p));
                self.dibujar_grafico();
            }
            self.enfocar_grafico();
        } else {
            self.0.dialogo.set_focus(Some(&self.0.texto));
        }
        self.actualizar_aceptar();
    }

    fn enfocar_grafico(&self) {
        if let Some(p) = self.0.grafico.borrow().as_ref() {
            let b = p.texto.buffer();
            b.place_cursor(&b.end_iter());
            self.0.dialogo.set_focus(Some(&p.texto));
        }
    }

    fn armar_grafico(&self, inicial: &str) -> Rc<PaginaGrafico> {
        let vista = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .height_request(ALTO_GRAFICO)
            .build();
        let error = gtk::Label::builder()
            .wrap(true)
            .xalign(0.0)
            .visible(false)
            .css_classes(["orbital-ecuacion-error"])
            .build();
        error.update_property(&[gtk::accessible::Property::Label("Error en el gráfico")]);
        let texto = gtk::TextView::builder()
            .monospace(true)
            .wrap_mode(gtk::WrapMode::WordChar)
            .accepts_tab(false)
            .top_margin(4)
            .bottom_margin(4)
            .left_margin(4)
            .right_margin(4)
            .css_classes(["orbital-ecuacion-texto"])
            .build();
        etiquetado(&texto, "Expresiones y opciones del gráfico");
        texto.buffer().set_text(inicial);
        let desplazar_texto = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .min_content_height(96)
            .max_content_height(180)
            .propagate_natural_height(true)
            .child(&texto)
            .build();
        let rotulo = gtk::Label::builder()
            .label("_Expresiones y opciones (una por renglón)")
            .use_underline(true)
            .mnemonic_widget(&texto)
            .xalign(0.0)
            .css_classes(["heading"])
            .build();
        let plantillas = gtk::FlowBox::builder()
            .selection_mode(gtk::SelectionMode::None)
            .activate_on_single_click(true)
            .row_spacing(6)
            .column_spacing(6)
            .max_children_per_line(30)
            .valign(gtk::Align::Start)
            .css_classes(["orbital-paleta"])
            .build();
        etiquetado(&plantillas, "Plantillas del gráfico");
        for (etiqueta, nombre, plantilla) in PLANTILLAS {
            let ayuda = format!("{nombre}: {}", plantilla.trim());
            plantillas.append(&simbolo(etiqueta, nombre, &ayuda));
        }
        let ayuda = gtk::Label::builder()
            .label(
                "En notación simple (sin(x)/x, 2x^2, |x|) o LaTeX (\\sin x, \\frac{1}{x}), \
                 incluso mezcladas. Funciones y = f(x), polares r = f(θ), curvas (x(t), y(t)), \
                 ecuaciones F(x, y) = 0, puntos (a, b) y tablas de datos. Arrastrá el gráfico \
                 para moverlo, la rueda acerca y aleja; la nota no cambia hasta aceptar.",
            )
            .wrap(true)
            .xalign(0.0)
            .css_classes(["dim-label", "caption"])
            .build();
        let caja = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(10)
            .margin_top(6)
            .margin_bottom(16)
            .margin_start(16)
            .margin_end(16)
            .build();
        caja.append(&vista);
        caja.append(&error);
        caja.append(&rotulo);
        caja.append(&desplazar_texto);
        caja.append(&plantillas);
        caja.append(&ayuda);
        let raiz = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vexpand(true)
            .child(&caja)
            .build();
        let p = Rc::new(PaginaGrafico {
            raiz: raiz.upcast(),
            texto: texto.clone(),
            vista,
            error,
            generacion: Cell::new(0),
            actual: RefCell::new(None),
        });
        {
            let this = self.clone();
            texto
                .buffer()
                .connect_changed(move |_| this.programar_grafico());
        }
        {
            let this = self.clone();
            plantillas.connect_child_activated(move |_, hijo| {
                if let Some((_, _, t)) = usize::try_from(hijo.index())
                    .ok()
                    .and_then(|k| PLANTILLAS.get(k))
                {
                    this.insertar_plantilla_grafico(t);
                }
            });
        }
        p
    }

    /// El texto del gráfico (vacío si la página no se armó).
    pub fn fuente_grafico(&self) -> String {
        self.0
            .grafico
            .borrow()
            .as_ref()
            .map_or_else(String::new, |p| {
                let b = p.texto.buffer();
                b.text(&b.start_iter(), &b.end_iter(), true).to_string()
            })
    }

    /// Escribe una plantilla en el campo del gráfico: en un renglón nuevo debajo del cursor, o
    /// al final del renglón si es un atributo (`; color: …`).
    pub fn insertar_plantilla_grafico(&self, plantilla: &str) {
        let Some(p) = self.0.grafico.borrow().clone() else {
            return;
        };
        let b = p.texto.buffer();
        let mut en = b.iter_at_mark(&b.get_insert());
        if !en.ends_line() {
            en.forward_to_line_end();
        }
        let mut ini = en;
        ini.set_line_offset(0);
        let renglon_vacio = b.text(&ini, &en, true).trim().is_empty();
        let nuevo = if plantilla.starts_with(' ') || renglon_vacio {
            plantilla.to_owned()
        } else {
            format!("\n{plantilla}")
        };
        let inicio = en.offset();
        b.begin_user_action();
        b.insert(&mut en, &nuevo);
        b.end_user_action();
        b.place_cursor(&b.iter_at_offset(inicio + nuevo.chars().count() as i32));
        p.texto.grab_focus();
    }

    fn programar_grafico(&self) {
        self.actualizar_aceptar();
        let Some(p) = self.0.grafico.borrow().clone() else {
            return;
        };
        let g = p.generacion.get() + 1;
        p.generacion.set(g);
        let this = self.clone();
        glib::timeout_add_local_once(PAUSA_GRAFICO, move || {
            if p.generacion.get() == g {
                this.dibujar_grafico();
            }
        });
    }

    /// Lee el campo y muestra el gráfico (el muestreo va fuera del hilo de GTK); con un error,
    /// el motivo, y queda el último gráfico bueno.
    pub fn dibujar_grafico(&self) {
        let Some(p) = self.0.grafico.borrow().clone() else {
            return;
        };
        let fuente = self.fuente_grafico();
        if fuente.trim().is_empty() {
            p.error.set_visible(false);
            return;
        }
        match crate::graficos::leer(&fuente) {
            Ok(g) => {
                let alto = (orbital_graficos::alto(&g) as i32).min(ALTO_GRAFICO);
                let vista = crate::graficos::GraficoVista::new(g, &fuente, true);
                vista.lienzo().fijar_alto(alto);
                while let Some(h) = p.vista.first_child() {
                    p.vista.remove(&h);
                }
                p.vista.append(vista.widget());
                p.actual.replace(Some(vista));
                p.error.set_visible(false);
            }
            Err(e) => {
                p.error.set_label(&e.to_string());
                p.error.set_visible(true);
            }
        }
    }

    /// El gráfico de la página y si hay un error (para las pruebas).
    #[cfg(test)]
    pub fn estado_grafico(&self) -> (Option<crate::graficos::GraficoVista>, Option<String>) {
        let p = self.0.grafico.borrow();
        let Some(p) = p.as_ref() else {
            return (None, None);
        };
        (
            p.actual.borrow().clone(),
            p.error.is_visible().then(|| p.error.label().to_string()),
        )
    }

    /// El campo del gráfico (para las pruebas).
    #[cfg(test)]
    pub fn texto_grafico(&self) -> Option<gtk::TextView> {
        self.0.grafico.borrow().as_ref().map(|p| p.texto.clone())
    }

    /// Muestra la página «Ecuación» o «Gráfico» (para las pruebas).
    #[cfg(test)]
    pub fn ir_a_seccion(&self, nombre: &str) {
        self.0.secciones.set_visible_child_name(nombre);
    }

    #[cfg(test)]
    pub fn dialogo(&self) -> &adw::Dialog {
        &self.0.dialogo
    }

    /// El LaTeX del campo.
    pub fn tex(&self) -> String {
        let b = self.0.texto.buffer();
        b.text(&b.start_iter(), &b.end_iter(), true).to_string()
    }

    fn modo(&self) -> Modo {
        if self.0.modo.active_name().as_deref() == Some("bloque") {
            Modo::Bloque
        } else {
            Modo::EnLinea
        }
    }

    fn actualizar_aceptar(&self) {
        let lleno = if self.en_grafico() {
            !self.fuente_grafico().trim().is_empty()
        } else {
            !self.tex().trim().is_empty()
        };
        self.0.aceptar.set_sensitive(lleno);
    }

    fn aceptar(&self) {
        let r = if self.en_grafico() {
            let fuente = self.fuente_grafico();
            if fuente.trim().is_empty() {
                return;
            }
            Aceptado::Grafico(fuente)
        } else {
            let tex = self.tex();
            if tex.trim().is_empty() {
                return;
            }
            Aceptado::Formula(tex, self.modo())
        };
        if let Some(f) = self.0.al_aceptar.take() {
            f(r);
        }
        self.0.dialogo.close();
    }

    /// Inserta una plantilla de la paleta en el campo, con lo seleccionado en el primer hueco.
    pub fn insertar_plantilla(&self, plantilla: &str) {
        let b = self.0.texto.buffer();
        let (mut desde, mut hasta) = b.selection_bounds().unwrap_or_else(|| {
            let c = b.iter_at_mark(&b.get_insert());
            (c, c)
        });
        let seleccion = b.text(&desde, &hasta, true);
        let (nuevo, cursor) = paleta::aplicar(plantilla, &seleccion);
        let inicio = desde.offset();
        b.begin_user_action();
        b.delete(&mut desde, &mut hasta);
        let mut en = b.iter_at_offset(inicio);
        b.insert(&mut en, &nuevo);
        b.end_user_action();
        b.place_cursor(&b.iter_at_offset(inicio + cursor as i32));
        self.0.texto.grab_focus();
    }

    fn programar(&self) {
        self.actualizar_aceptar();
        let g = self.0.generacion.get() + 1;
        self.0.generacion.set(g);
        let this = self.clone();
        glib::timeout_add_local_once(PAUSA, move || {
            if this.0.generacion.get() == g {
                this.dibujar_ya();
            }
        });
    }

    /// Dibuja el LaTeX del campo (fuera del hilo de GTK si no está en la caché).
    pub fn dibujar_ya(&self) {
        let tex = self.tex();
        let i = &self.0;
        if tex.trim().is_empty() {
            i.imagen.set_paintable(None::<&gtk::gdk::Paintable>);
            i.error.set_visible(false);
            return;
        }
        let mut estilo = formulas::Estilo::de(&i.imagen).por(AUMENTO);
        let c = crate::style::TEXT_FG;
        estilo.color = [c.0, c.1, c.2, 255];
        let clave = Clave::new(&tex, Modo::Bloque, &estilo);
        let g = i.generacion.get();
        match formulas::en_cache(&clave) {
            Some(r) => self.mostrar(&tex, r),
            None => {
                let this = self.clone();
                glib::spawn_future_local(async move {
                    let Ok(r) = gio::spawn_blocking(move || formulas::dibujar(&clave)).await else {
                        return;
                    };
                    // Se siguió escribiendo: lo dibuja la próxima vuelta.
                    if this.0.generacion.get() == g {
                        this.mostrar(&tex, r);
                    }
                });
            }
        }
    }

    fn mostrar(&self, tex: &str, r: formulas::Resultado) {
        let i = &self.0;
        match r {
            Ok(d) => {
                i.imagen.set_paintable(Some(&Formula::lista(&d)));
                i.imagen
                    .set_alternative_text(Some(&orbital_formulas::lectura(tex)));
                i.error.set_visible(false);
            }
            Err(e) => {
                i.error.set_label(&e.mensaje);
                i.error.set_visible(true);
            }
        }
    }

    /// La imagen de la vista previa (para medir cuándo cambia).
    #[cfg(test)]
    pub fn imagen(&self) -> Option<gtk::gdk::Paintable> {
        self.0.imagen.paintable()
    }

    /// Lo que se ve: la imagen (si hay) y el error (si hay). Para las pruebas.
    #[cfg(test)]
    pub fn estado(&self) -> (bool, Option<String>) {
        let i = &self.0;
        (
            i.imagen.paintable().is_some(),
            i.error.is_visible().then(|| i.error.label().to_string()),
        )
    }

    /// Muestra una página de la paleta (`estructuras`, `quimica`, `convertir`…).
    #[cfg(test)]
    pub fn ir_a(&self, pagina: &str) {
        self.0.paginas.set_visible_child_name(pagina);
    }

    fn al_cambiar_pagina(&self) {
        let Some(nombre) = self.0.paginas.visible_child_name() else {
            return;
        };
        let pendiente = {
            let mut s = self.0.sin_llenar.borrow_mut();
            s.iter()
                .position(|(c, _)| c.id == nombre.as_str())
                .map(|k| s.remove(k))
        };
        if let Some((cat, caja)) = pendiente {
            llenar(&caja, cat);
            self.conectar_caja(&caja, cat);
        } else if nombre == "convertir" {
            self.preparar_conversor();
        }
    }

    fn conectar_caja(&self, caja: &gtk::FlowBox, cat: &'static paleta::Categoria) {
        let this = self.clone();
        caja.connect_child_activated(move |_, hijo| {
            if let Some(el) = usize::try_from(hijo.index())
                .ok()
                .and_then(|k| cat.elementos.get(k))
            {
                this.insertar_plantilla(el.plantilla);
            }
        });
    }

    // --- Conversión de unidades ---

    fn conectar_conversor(&self) {
        let c = &self.0.conversor;
        for e in [&c.valor, &c.desde, &c.hacia] {
            let this = self.clone();
            e.connect_changed(move |_| this.convertir());
        }
        {
            let this = self.clone();
            c.desde.connect_changed(move |_| this.llenar_sugerencias());
        }
        {
            let this = self.clone();
            c.sugerencias.connect_child_activated(move |_, hijo| {
                let u = usize::try_from(hijo.index())
                    .ok()
                    .and_then(|k| this.0.conversor.lista.borrow().get(k).copied());
                if let Some(u) = u {
                    this.0.conversor.hacia.set_text(u);
                }
            });
        }
        {
            let this = self.clone();
            c.insertar
                .connect_clicked(move |_| this.insertar_conversion());
        }
        {
            let this = self.clone();
            c.hacia
                .connect_activate(move |_| this.insertar_conversion());
        }
    }

    /// Al abrir la página: toma la cantidad bajo el cursor del campo (o la única).
    fn preparar_conversor(&self) {
        let tex = self.tex();
        let b = self.0.texto.buffer();
        let cursor = b.iter_at_mark(&b.get_insert()).offset() as usize;
        let byte = tex.char_indices().nth(cursor).map_or(tex.len(), |(i, _)| i);
        let c = &self.0.conversor;
        let hallada = siunitx::cantidad_en(&tex, byte);
        if let Some(q) = &hallada {
            c.valor.set_text(&q.numero);
            c.desde.set_text(&q.unidad);
        }
        c.insertar.set_label(if hallada.is_some() {
            "Reemplazar"
        } else {
            "Insertar"
        });
        c.origen.replace(hallada);
        self.llenar_sugerencias();
        self.convertir();
    }

    fn llenar_sugerencias(&self) {
        let c = &self.0.conversor;
        while let Some(h) = c.sugerencias.first_child() {
            c.sugerencias.remove(&h);
        }
        let lista = unidades::sugerencias(&c.desde.text());
        for u in &lista {
            c.sugerencias.append(&simbolo(
                u,
                &format!("Convertir a {u}"),
                &format!("Unidad: {u}"),
            ));
        }
        c.lista.replace(lista);
    }

    /// El resultado, o por qué no se puede.
    fn resultado(&self) -> Result<(f64, String), String> {
        let c = &self.0.conversor;
        let v = siunitx::leer_numero(&c.valor.text())
            .ok_or_else(|| "Escribí un número (por ejemplo 90 o 1,5e3)".to_owned())?;
        let desde = c.desde.text();
        let hacia = c.hacia.text();
        if desde.trim().is_empty() || hacia.trim().is_empty() {
            return Err("Elegí la unidad de origen y la de destino".into());
        }
        let r = unidades::convertir(v, &desde, &hacia).map_err(|e| {
            let mut m = e.0;
            if let Some(p) = m.get_mut(0..1) {
                p.make_ascii_uppercase();
            }
            m
        })?;
        Ok((r, hacia.trim().to_owned()))
    }

    fn convertir(&self) {
        let c = &self.0.conversor;
        match self.resultado() {
            Ok((v, hacia)) => {
                c.resultado.set_label(&format!(
                    "= {} {hacia}",
                    unidades::formatear(v).replace('.', ",")
                ));
                c.resultado.remove_css_class("orbital-ecuacion-error");
                c.insertar.set_sensitive(true);
            }
            Err(m) => {
                c.resultado.set_label(&m);
                c.resultado.add_css_class("orbital-ecuacion-error");
                c.insertar.set_sensitive(false);
            }
        }
    }

    /// Escribe el resultado en el campo: reemplaza la cantidad de origen o lo inserta en el
    /// cursor, como `\qty{…}{…}`.
    pub fn insertar_conversion(&self) {
        let Ok((v, hacia)) = self.resultado() else {
            return;
        };
        let c = &self.0.conversor;
        let origen = c.origen.borrow().clone();
        let comando = origen.as_ref().map_or("qty", |q| q.comando.as_str());
        let nuevo = format!("\\{comando}{{{}}}{{{hacia}}}", unidades::formatear(v));
        let b = self.0.texto.buffer();
        let tex = self.tex();
        let reemplazado = origen.and_then(|q| {
            // La cantidad sigue ahí (el campo pudo cambiar desde que se abrió la página).
            let actual = tex.get(q.rango.clone())?;
            actual.starts_with('\\').then_some(q.rango)
        });
        b.begin_user_action();
        let inicio = match reemplazado {
            Some(r) => {
                let a = tex[..r.start].chars().count() as i32;
                let z = tex[..r.end].chars().count() as i32;
                let mut desde = b.iter_at_offset(a);
                let mut hasta = b.iter_at_offset(z);
                b.delete(&mut desde, &mut hasta);
                a
            }
            None => {
                let (mut desde, mut hasta) = b.selection_bounds().unwrap_or_else(|| {
                    let c = b.iter_at_mark(&b.get_insert());
                    (c, c)
                });
                let a = desde.offset();
                b.delete(&mut desde, &mut hasta);
                a
            }
        };
        let mut en = b.iter_at_offset(inicio);
        b.insert(&mut en, &nuevo);
        b.end_user_action();
        b.place_cursor(&b.iter_at_offset(inicio + nuevo.chars().count() as i32));
        // Lo insertado pasa a ser la cantidad de origen: se puede volver a convertir.
        self.preparar_conversor();
        self.0.texto.grab_focus();
    }

    /// Los campos del conversor, para las pruebas.
    #[cfg(test)]
    pub fn conversor(&self) -> (gtk::Entry, gtk::Entry, gtk::Entry, gtk::Label, gtk::Button) {
        let c = &self.0.conversor;
        (
            c.valor.clone(),
            c.desde.clone(),
            c.hacia.clone(),
            c.resultado.clone(),
            c.insertar.clone(),
        )
    }

    #[cfg(test)]
    pub fn boton_aceptar(&self) -> gtk::Button {
        self.0.aceptar.clone()
    }

    #[cfg(test)]
    pub fn texto(&self) -> gtk::TextView {
        self.0.texto.clone()
    }

    #[cfg(test)]
    pub fn elegir_modo(&self, modo: Modo) {
        self.0.modo.set_active_name(Some(match modo {
            Modo::EnLinea => "linea",
            Modo::Bloque => "bloque",
        }));
    }
}

/// Un botón de la paleta: una etiqueta en un hijo del `FlowBox` (que se enfoca y se activa con
/// el teclado), con nombre accesible y tooltip. Al activarlo, el `FlowBox` dice su índice.
fn simbolo(etiqueta: &str, nombre: &str, ayuda: &str) -> gtk::FlowBoxChild {
    let rotulo = gtk::Label::new(Some(etiqueta));
    let hijo = gtk::FlowBoxChild::builder()
        .child(&rotulo)
        .tooltip_text(ayuda)
        .css_classes(["orbital-simbolo"])
        .build();
    etiquetado(&hijo, nombre);
    hijo
}

fn llenar(caja: &gtk::FlowBox, cat: &'static paleta::Categoria) {
    for el in cat.elementos {
        let ayuda = format!(
            "{}: {}",
            el.nombre,
            el.plantilla.replace(paleta::HUECO, "…")
        );
        caja.append(&simbolo(el.etiqueta, el.nombre, &ayuda));
    }
}

impl Conversor {
    fn new() -> Self {
        let campo = |marcador: &str, nombre: &str| {
            let e = gtk::Entry::builder()
                .placeholder_text(marcador)
                .hexpand(true)
                .build();
            etiquetado(&e, nombre);
            e
        };
        let resultado = gtk::Label::builder()
            .xalign(0.0)
            .wrap(true)
            .selectable(true)
            .css_classes(["title-4"])
            .build();
        let insertar = gtk::Button::builder()
            .label("Insertar")
            .css_classes(["suggested-action"])
            .halign(gtk::Align::Start)
            .sensitive(false)
            .build();
        let sugerencias = gtk::FlowBox::builder()
            .selection_mode(gtk::SelectionMode::None)
            .activate_on_single_click(true)
            .row_spacing(6)
            .column_spacing(6)
            .max_children_per_line(30)
            .valign(gtk::Align::Start)
            .css_classes(["orbital-paleta"])
            .build();
        etiquetado(&sugerencias, "Unidades del mismo tipo");
        Self {
            valor: campo("90", "Cantidad"),
            desde: campo("km/h", "Unidad de origen"),
            hacia: campo("m/s", "Unidad de destino"),
            resultado,
            sugerencias,
            insertar,
            origen: RefCell::new(None),
            lista: RefCell::new(Vec::new()),
        }
    }

    fn widget(&self) -> gtk::Widget {
        let rejilla = gtk::Grid::builder()
            .row_spacing(8)
            .column_spacing(10)
            .build();
        let fila = |k: i32, texto: &str, w: &gtk::Entry| {
            let r = gtk::Label::builder()
                .label(texto)
                .use_underline(true)
                .mnemonic_widget(w)
                .xalign(1.0)
                .build();
            rejilla.attach(&r, 0, k, 1, 1);
            rejilla.attach(w, 1, k, 1, 1);
        };
        fila(0, "_Cantidad", &self.valor);
        fila(1, "_De", &self.desde);
        fila(2, "_A", &self.hacia);
        let ayuda = gtk::Label::builder()
            .label(
                "Unidades del SI con prefijos y compuestas (km/h, kg m/s^2, J/(mol K), \
                 \\kilo\\metre\\per\\hour) y las comunes (°C, °F, mi, lb, atm, psi, kWh, cal…). \
                 Si el cursor está en un \\qty{…}{…}, se convierte esa cantidad.",
            )
            .wrap(true)
            .xalign(0.0)
            .css_classes(["dim-label", "caption"])
            .build();
        let caja = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(10)
            .margin_top(12)
            .margin_bottom(12)
            .margin_start(12)
            .margin_end(12)
            .build();
        caja.append(&rejilla);
        caja.append(&self.resultado);
        caja.append(&self.insertar);
        caja.append(&self.sugerencias);
        caja.append(&ayuda);
        gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .child(&caja)
            .build()
            .upcast()
    }
}
