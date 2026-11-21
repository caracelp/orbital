//! Galería lateral de la nota abierta (034): un modo de la barra derecha con las imágenes y los
//! videos de la nota, agrupados por encabezado, con búsqueda, filtro por tipo, tres tamaños de
//! mosaico y el visor compartido con el carrusel (032) y la galería en la nota (033).
//!
//! Solo widgets: la lista ([`MediosNota`]) la arma la ventana en otro hilo
//! (`src/window/galeria_lateral.rs`) y los pedidos («Ir al embed», «Mostrar en explorador») le
//! vuelven por [`GaleriaLateral::connect_pedido`]. Nada de esto existe hasta que se elige el
//! modo por primera vez.
//!
//! La grilla es un `GtkFixed` del alto entero dentro de su `ScrolledWindow`, con widgets solo
//! para lo visible y una pantalla de margen (reutilizados al desplazar), como la galería de la
//! 033; las miniaturas se decodifican afuera ([`crate::imagenes`]), de a tres y de lo visible
//! hacia afuera. El título del grupo que se está recorriendo queda fijo arriba.
//!
//! Teclado: Tab entra a un solo elemento (un título o un mosaico) y las flechas recorren;
//! Inicio y Fin; Enter o Espacio pliegan un grupo o abren el visor; Ctrl+Enter va al embed;
//! Mayús+F10 o la tecla de menú abren el menú del mosaico.

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use adw::prelude::*;
use gtk::{gdk, gio, glib};
use orbital_vault::RelPath;
use orbital_vault::carrusel::{Carrusel, Clase, Diapositiva, Estado};
use orbital_vault::medios_nota::{Medio, MediosNota};

use crate::imagenes::{self, Ajuste};

pub mod disposicion;

use disposicion::{Direccion, Disposicion, Filtro, Pieza, Rect, Tamano};

/// Miniaturas que se decodifican a la vez (como la galería de la 033).
const EN_VUELO: usize = 3;
/// Con más medios, el visor no lleva tira de miniaturas (cientos de botones).
const MINIATURAS_HASTA: usize = 200;
/// Ancho de la grilla mientras el panel no tiene lugar (la barra derecha mide 280).
const ANCHO_INICIAL: i32 = 248;

/// Lo que el panel le pide a la ventana.
#[derive(Clone, Debug)]
pub enum Pedido {
    IrAlEmbed(Medio),
    MostrarEnExplorador(RelPath),
    Aviso(String),
}

type PedidoFn = Rc<dyn Fn(Pedido)>;

/// La búsqueda, el filtro y los grupos plegados de una nota, mientras corre la app.
#[derive(Clone, Default)]
struct EstadoNota {
    consulta: String,
    filtro: Filtro,
    /// Título y nivel de los grupos plegados.
    plegadas: HashSet<(String, u8)>,
}

struct Celda {
    boton: gtk::Button,
    pic: gtk::Picture,
    marca: gtk::Image,
    nombre: gtk::Label,
    /// El tamaño en píxeles del dispositivo de la textura pedida o puesta.
    pedida: Cell<(i32, i32)>,
}

struct Titulo {
    boton: gtk::Button,
    flecha: gtk::Image,
    texto: gtk::Label,
    conteo: gtk::Label,
}

struct Inner {
    root: gtk::Box,
    paginas: gtk::Stack,
    vacio: gtk::Label,
    busqueda: gtk::SearchEntry,
    filtro_grupo: adw::ToggleGroup,
    resumen: gtk::Label,
    scroller: gtk::ScrolledWindow,
    lienzo: gtk::Fixed,
    fijo: gtk::Button,
    fijo_texto: gtk::Label,
    acciones: gio::SimpleActionGroup,
    ruta: RefCell<Option<String>>,
    medios: RefCell<Rc<MediosNota>>,
    estados: RefCell<HashMap<String, EstadoNota>>,
    consulta: RefCell<Vec<char>>,
    filtro: Cell<Filtro>,
    tamano: Cell<Tamano>,
    plegadas: RefCell<Vec<bool>>,
    disp: RefCell<Disposicion>,
    ancho: Cell<i32>,
    vivas: RefCell<Vec<(usize, Celda)>>,
    libres: RefCell<Vec<Celda>>,
    titulos: RefCell<Vec<(usize, Titulo)>>,
    titulos_libres: RefCell<Vec<Titulo>>,
    activo: Cell<Option<Pieza>>,
    cola: RefCell<Vec<usize>>,
    en_vuelo: Cell<usize>,
    pendiente: Cell<bool>,
    /// La lista del visor (los medios filtrados), armada al abrirlo.
    visor: RefCell<Option<(Rc<Carrusel>, Vec<usize>)>>,
    /// «Ir al embed» desde el visor: al cerrarse, el foco no vuelve a la grilla.
    saltando: Cell<bool>,
    pedido: RefCell<Option<PedidoFn>>,
    /// El medio del menú contextual abierto.
    menu: Cell<Option<usize>>,
    /// Se están poniendo la búsqueda y el filtro guardados (sus señales no redisponen).
    ignorar_cambios: Cell<bool>,
    /// Un desplazamiento que espera a que el lienzo crezca (la disposición nueva todavía no
    /// llegó al ajuste del `ScrolledWindow`).
    desplazar: Cell<Option<f64>>,
    /// El último pedido de lista de la ventana (los anteriores que lleguen tarde no cuentan).
    generacion: Cell<u64>,
}

/// El panel: se crea la primera vez que se elige el modo Galería.
#[derive(Clone)]
pub struct GaleriaLateral(Rc<Inner>);

impl GaleriaLateral {
    pub fn new() -> Self {
        let busqueda = gtk::SearchEntry::builder()
            .placeholder_text("Buscar por nombre o encabezado")
            .hexpand(true)
            .build();
        busqueda.update_property(&[gtk::accessible::Property::Label(
            "Buscar medios por nombre o encabezado",
        )]);
        let filtro_grupo = adw::ToggleGroup::new();
        for (nombre, rotulo, ayuda) in [
            ("todo", "Todo", "Imágenes y videos"),
            ("imagenes", "Imágenes", "Solo imágenes"),
            ("videos", "Videos", "Solo videos"),
        ] {
            filtro_grupo.add(
                adw::Toggle::builder()
                    .name(nombre)
                    .label(rotulo)
                    .tooltip(ayuda)
                    .build(),
            );
        }
        filtro_grupo.set_active_name(Some("todo"));
        filtro_grupo.set_hexpand(true);
        filtro_grupo.add_css_class("round");
        filtro_grupo
            .upcast_ref::<gtk::Widget>()
            .update_property(&[gtk::accessible::Property::Label("Tipo de medio")]);
        let menu = gio::Menu::new();
        let grupos = gio::Menu::new();
        grupos.append(Some("Plegar todo"), Some("galeria.plegar-todo"));
        grupos.append(Some("Desplegar todo"), Some("galeria.desplegar-todo"));
        menu.append_section(None, &grupos);
        let tamanos = gio::Menu::new();
        tamanos.append(Some("Mosaicos chicos"), Some("galeria.tamano::chico"));
        tamanos.append(Some("Mosaicos medianos"), Some("galeria.tamano::mediano"));
        tamanos.append(Some("Mosaicos grandes"), Some("galeria.tamano::grande"));
        menu.append_section(Some("Tamaño"), &tamanos);
        let opciones = gtk::MenuButton::builder()
            .icon_name("view-more-symbolic")
            .tooltip_text("Opciones de la galería")
            .menu_model(&menu)
            .css_classes(["flat"])
            .build();
        opciones.update_property(&[gtk::accessible::Property::Label("Opciones de la galería")]);
        let fila = gtk::Box::builder().spacing(4).build();
        fila.append(&filtro_grupo);
        fila.append(&opciones);
        let resumen = gtk::Label::builder()
            .xalign(0.0)
            .css_classes(["dim-label", "caption"])
            .build();
        let cabecera = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(6)
            .css_classes(["orbital-galeria-lateral-cabecera"])
            .build();
        cabecera.append(&busqueda);
        cabecera.append(&fila);
        cabecera.append(&resumen);

        let lienzo = gtk::Fixed::builder()
            .css_classes(["orbital-galeria-lateral-lienzo"])
            .build();
        lienzo.set_accessible_role(gtk::AccessibleRole::Grid);
        lienzo.update_property(&[gtk::accessible::Property::Label("Medios de la nota")]);
        let scroller = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vexpand(true)
            .child(&lienzo)
            .build();
        let fijo_texto = gtk::Label::builder()
            .xalign(0.0)
            .hexpand(true)
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .build();
        let fijo_caja = gtk::Box::builder().spacing(6).build();
        let fijo_flecha = gtk::Image::from_icon_name("pan-down-symbolic");
        fijo_flecha.set_accessible_role(gtk::AccessibleRole::Presentation);
        fijo_caja.append(&fijo_flecha);
        fijo_caja.append(&fijo_texto);
        let fijo = gtk::Button::builder()
            .child(&fijo_caja)
            .valign(gtk::Align::Start)
            .focusable(false)
            .visible(false)
            .css_classes(["orbital-galeria-lateral-fijo"])
            .build();
        fijo.update_property(&[gtk::accessible::Property::Label(
            "Grupo actual: plegar y volver a su título",
        )]);
        let sobre = gtk::Overlay::builder().child(&scroller).build();
        sobre.add_overlay(&fijo);

        let vacio = gtk::Label::builder()
            .wrap(true)
            .xalign(0.0)
            .css_classes(["orbital-context-empty"])
            .build();
        let caja_vacia = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(8)
            .css_classes(["orbital-right-block"])
            .build();
        let icono_vacio = gtk::Image::from_icon_name("folder-pictures-symbolic");
        icono_vacio.set_pixel_size(32);
        icono_vacio.set_halign(gtk::Align::Start);
        icono_vacio.add_css_class("dim-label");
        icono_vacio.set_accessible_role(gtk::AccessibleRole::Presentation);
        caja_vacia.append(&icono_vacio);
        caja_vacia.append(&vacio);
        let grilla = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .build();
        grilla.append(&cabecera);
        grilla.append(&sobre);
        let paginas = gtk::Stack::builder().vexpand(true).build();
        paginas.add_named(&grilla, Some("grilla"));
        paginas.add_named(&caja_vacia, Some("vacio"));
        paginas.set_visible_child_name("vacio");

        let root = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .vexpand(true)
            .css_classes(["orbital-galeria-lateral"])
            .build();
        root.set_accessible_role(gtk::AccessibleRole::Group);
        root.update_property(&[gtk::accessible::Property::Label("Galería de la nota")]);
        root.append(&paginas);
        let acciones = gio::SimpleActionGroup::new();
        root.insert_action_group("galeria", Some(&acciones));

        let this = Self(Rc::new(Inner {
            root,
            paginas,
            vacio,
            busqueda,
            filtro_grupo,
            resumen,
            scroller,
            lienzo,
            fijo,
            fijo_texto,
            acciones,
            ruta: RefCell::new(None),
            medios: RefCell::default(),
            estados: RefCell::default(),
            consulta: RefCell::default(),
            filtro: Cell::new(Filtro::Todo),
            tamano: Cell::new(Tamano::Mediano),
            plegadas: RefCell::default(),
            disp: RefCell::default(),
            ancho: Cell::new(ANCHO_INICIAL),
            vivas: RefCell::default(),
            libres: RefCell::default(),
            titulos: RefCell::default(),
            titulos_libres: RefCell::default(),
            activo: Cell::new(None),
            cola: RefCell::default(),
            en_vuelo: Cell::new(0),
            pendiente: Cell::new(false),
            visor: RefCell::default(),
            saltando: Cell::new(false),
            pedido: RefCell::default(),
            menu: Cell::new(None),
            ignorar_cambios: Cell::new(false),
            generacion: Cell::new(0),
            desplazar: Cell::new(None),
        }));
        this.conectar();
        this.mostrar_vacio("Abrí una nota para ver sus imágenes y videos.");
        this
    }

    pub fn widget(&self) -> &gtk::Box {
        &self.0.root
    }

    pub fn connect_pedido(&self, f: impl Fn(Pedido) + 'static) {
        *self.0.pedido.borrow_mut() = Some(Rc::new(f));
    }

    fn pedir(&self, p: Pedido) {
        let f = self.0.pedido.borrow().clone();
        if let Some(f) = f {
            f(p);
        }
    }

    /// Un pedido nuevo de lista (la ventana lo arma en otro hilo).
    pub fn nuevo_pedido(&self) -> u64 {
        let g = self.0.generacion.get() + 1;
        self.0.generacion.set(g);
        g
    }

    /// Si `g` sigue siendo el último pedido.
    pub fn vigente(&self, g: u64) -> bool {
        self.0.generacion.get() == g
    }

    /// La nota de la que se muestran los medios.
    pub fn ruta(&self) -> Option<String> {
        self.0.ruta.borrow().clone()
    }

    /// Sin nota (o una que no es `.md`): el estado vacío con el motivo.
    pub fn mostrar_vacio(&self, motivo: &str) {
        let i = &self.0;
        self.guardar_estado();
        *i.ruta.borrow_mut() = None;
        *i.medios.borrow_mut() = Rc::default();
        *i.visor.borrow_mut() = None;
        i.vacio.set_label(motivo);
        i.paginas.set_visible_child_name("vacio");
        self.limpiar_piezas();
    }

    /// Los medios de la nota `ruta`. Si es la misma nota con la misma lista, no hace nada (la
    /// edición vuelve a llamar con cada pausa); si es otra, recupera su búsqueda, su filtro y
    /// sus pliegues.
    pub fn set_medios(&self, ruta: &str, m: Rc<MediosNota>) {
        let i = &self.0;
        let misma = i.ruta.borrow().as_deref() == Some(ruta);
        if misma && *i.medios.borrow().as_ref() == *m {
            return;
        }
        if !misma {
            self.guardar_estado();
            *i.ruta.borrow_mut() = Some(ruta.to_owned());
            let e = i.estados.borrow().get(ruta).cloned().unwrap_or_default();
            i.ignorar_cambios.set(true);
            i.busqueda.set_text(&e.consulta);
            i.filtro_grupo.set_active_name(Some(match e.filtro {
                Filtro::Todo => "todo",
                Filtro::Imagenes => "imagenes",
                Filtro::Videos => "videos",
            }));
            i.ignorar_cambios.set(false);
            *i.consulta.borrow_mut() = disposicion::plegar(e.consulta.trim());
            i.filtro.set(e.filtro);
            *i.plegadas.borrow_mut() = m
                .secciones
                .iter()
                .map(|s| e.plegadas.contains(&(s.titulo.clone(), s.nivel)))
                .collect();
            i.activo.set(None);
            i.scroller.vadjustment().set_value(0.0);
        } else {
            // La misma nota editada: los pliegues siguen a su grupo por el título.
            let antes = i.medios.borrow().clone();
            let plegados: HashSet<(String, u8)> = antes
                .secciones
                .iter()
                .zip(i.plegadas.borrow().iter())
                .filter(|(_, p)| **p)
                .map(|(s, _)| (s.titulo.clone(), s.nivel))
                .collect();
            *i.plegadas.borrow_mut() = m
                .secciones
                .iter()
                .map(|s| plegados.contains(&(s.titulo.clone(), s.nivel)))
                .collect();
        }
        *i.visor.borrow_mut() = None;
        let vacia = m.medios.is_empty();
        *i.medios.borrow_mut() = m;
        if vacia {
            i.vacio.set_label(
                "Esta nota no tiene imágenes ni videos. Incrustá uno con ![[archivo.png]] o \
                 ![texto](archivo.png), o agregá un bloque carrusel o galeria.",
            );
            i.paginas.set_visible_child_name("vacio");
            self.limpiar_piezas();
            return;
        }
        i.paginas.set_visible_child_name("grilla");
        // Los widgets que quedaron se rearman con la lista nueva.
        self.limpiar_piezas();
        self.disponer();
    }

    fn guardar_estado(&self) {
        let i = &self.0;
        let Some(ruta) = i.ruta.borrow().clone() else {
            return;
        };
        let m = i.medios.borrow();
        let e = EstadoNota {
            consulta: i.busqueda.text().to_string(),
            filtro: i.filtro.get(),
            plegadas: m
                .secciones
                .iter()
                .zip(i.plegadas.borrow().iter())
                .filter(|(_, p)| **p)
                .map(|(s, _)| (s.titulo.clone(), s.nivel))
                .collect(),
        };
        i.estados.borrow_mut().insert(ruta, e);
    }

    fn conectar(&self) {
        let i = &self.0;
        let debil = || Rc::downgrade(&self.0);
        {
            let d = debil();
            i.busqueda.connect_search_changed(move |e| {
                let Some(i) = d.upgrade() else { return };
                if i.ignorar_cambios.get() {
                    return;
                }
                *i.consulta.borrow_mut() = disposicion::plegar(e.text().trim());
                i.scroller.vadjustment().set_value(0.0);
                GaleriaLateral(i).disponer();
            });
        }
        {
            let d = debil();
            i.filtro_grupo.connect_active_name_notify(move |g| {
                let Some(i) = d.upgrade() else { return };
                let f = match g.active_name().as_deref() {
                    Some("imagenes") => Filtro::Imagenes,
                    Some("videos") => Filtro::Videos,
                    _ => Filtro::Todo,
                };
                if i.ignorar_cambios.get() || f == i.filtro.get() {
                    i.filtro.set(f);
                    return;
                }
                i.filtro.set(f);
                GaleriaLateral(i).disponer();
            });
        }
        // Las acciones del menú de opciones y del menú de cada mosaico.
        let accion = |nombre: &str, f: fn(&GaleriaLateral)| {
            let a = gio::SimpleAction::new(nombre, None);
            let d = debil();
            a.connect_activate(move |_, _| {
                if let Some(i) = d.upgrade() {
                    f(&GaleriaLateral(i));
                }
            });
            i.acciones.add_action(&a);
        };
        accion("plegar-todo", |g| g.plegar_todo(true));
        accion("desplegar-todo", |g| g.plegar_todo(false));
        accion("ir-al-embed", |g| {
            if let Some(k) = g.0.menu.get() {
                g.ir_al_embed(k);
            }
        });
        accion("copiar-ruta", |g| {
            if let Some(k) = g.0.menu.get() {
                g.copiar_ruta(k);
            }
        });
        accion("mostrar-explorador", |g| {
            let Some(k) = g.0.menu.get() else { return };
            let p =
                g.0.medios
                    .borrow()
                    .medios
                    .get(k)
                    .and_then(|m| m.path.clone());
            match p {
                Some(p) => g.pedir(Pedido::MostrarEnExplorador(p)),
                None => g.pedir(Pedido::Aviso("El archivo no está en la bóveda".into())),
            }
        });
        let tamano = gio::SimpleAction::new_stateful(
            "tamano",
            Some(glib::VariantTy::STRING),
            &Tamano::Mediano.nombre().to_variant(),
        );
        {
            let d = debil();
            tamano.connect_activate(move |a, v| {
                let (Some(i), Some(v)) = (d.upgrade(), v.and_then(|v| v.get::<String>())) else {
                    return;
                };
                a.set_state(&v.to_variant());
                let t = Tamano::de(&v);
                if t != i.tamano.get() {
                    i.tamano.set(t);
                    let g = GaleriaLateral(i);
                    // Las miniaturas cambian de tamaño: se vuelven a pedir.
                    g.limpiar_piezas();
                    g.disponer();
                }
            });
        }
        i.acciones.add_action(&tamano);
        // Teclado (en burbuja desde el elemento con el foco).
        let teclas = gtk::EventControllerKey::new();
        {
            let d = debil();
            teclas.connect_key_pressed(move |_, k, _, mods| {
                d.upgrade().map_or(glib::Propagation::Proceed, |i| {
                    GaleriaLateral(i).tecla(k, mods)
                })
            });
        }
        i.lienzo.add_controller(teclas);
        let adj = i.scroller.vadjustment();
        {
            let d = debil();
            adj.connect_value_changed(move |_| {
                if let Some(i) = d.upgrade() {
                    GaleriaLateral(i).programar();
                }
            });
        }
        {
            let d = debil();
            adj.connect_upper_notify(move |a| {
                if let Some(i) = d.upgrade()
                    && let Some(v) = i.desplazar.get()
                {
                    if v <= a.upper() - a.page_size() {
                        i.desplazar.set(None);
                    }
                    a.set_value(v.min(a.upper() - a.page_size()));
                }
            });
        }
        {
            let d = debil();
            adj.connect_page_size_notify(move |_| {
                if let Some(i) = d.upgrade() {
                    GaleriaLateral(i).programar();
                }
            });
        }
        {
            let d = debil();
            i.root.connect_map(move |_| {
                if let Some(i) = d.upgrade() {
                    GaleriaLateral(i).programar();
                }
            });
        }
        {
            let d = debil();
            i.root.connect_unmap(move |_| {
                if let Some(i) = d.upgrade() {
                    i.cola.borrow_mut().clear();
                }
            });
        }
        {
            let d = debil();
            i.fijo.connect_clicked(move |_| {
                let Some(i) = d.upgrade() else { return };
                let s = i.fijo_seccion();
                if let Some(s) = s {
                    let g = GaleriaLateral(i);
                    g.alternar(s);
                    g.mostrar(Pieza::Titulo(s), true);
                }
            });
        }
    }

    /// Calcula la grilla con la búsqueda, el filtro y los pliegues; ubica los widgets vivos.
    fn disponer(&self) {
        let i = &self.0;
        let m = i.medios.borrow().clone();
        let d = disposicion::disponer(&disposicion::Pedido {
            medios: &m,
            consulta: &i.consulta.borrow(),
            filtro: i.filtro.get(),
            plegadas: &i.plegadas.borrow(),
            ancho: i.ancho.get(),
            tamano: i.tamano.get(),
        });
        i.lienzo.set_size_request(i.ancho.get(), d.alto.max(1));
        let total = m.medios.len();
        let n = d.filtrados.len();
        let grupos = d.visibles.iter().filter(|v| **v > 0).count();
        let plural = |n: usize, uno: &str, varios: &str| {
            format!("{n} {}", if n == 1 { uno } else { varios })
        };
        let texto = if n == total {
            format!(
                "{} en {}",
                plural(n, "medio", "medios"),
                plural(grupos, "grupo", "grupos")
            )
        } else if n == 0 {
            format!("Ninguno de los {total} medios coincide")
        } else {
            format!(
                "{n} de {total} medios en {}",
                plural(grupos, "grupo", "grupos")
            )
        };
        i.resumen.set_label(&texto);
        i.lienzo
            .update_property(&[gtk::accessible::Property::Label(&format!(
                "Medios de la nota: {texto}"
            ))]);
        *i.disp.borrow_mut() = d;
        *i.visor.borrow_mut() = None;
        // El elemento activo se mantiene si sigue en la grilla.
        if let Some(a) = i.activo.get()
            && i.disp.borrow().rect(a).is_none()
        {
            i.activo.set(None);
        }
        // Los vivos van a su lugar nuevo (o se sueltan).
        let vivas = std::mem::take(&mut *i.vivas.borrow_mut());
        let mut quedan = Vec::with_capacity(vivas.len());
        for (k, c) in vivas {
            match i.disp.borrow().rect(Pieza::Mosaico(k)) {
                Some(r) => {
                    ubicar(&i.lienzo, c.boton.upcast_ref(), r);
                    self.ajustar_celda(&c, r);
                    quedan.push((k, c));
                }
                None => self.soltar(c),
            }
        }
        *i.vivas.borrow_mut() = quedan;
        let titulos = std::mem::take(&mut *i.titulos.borrow_mut());
        let mut quedan = Vec::with_capacity(titulos.len());
        for (s, t) in titulos {
            match i.disp.borrow().rect(Pieza::Titulo(s)) {
                Some(r) => {
                    ubicar(&i.lienzo, t.boton.upcast_ref(), r);
                    self.llenar_titulo(&t, s);
                    quedan.push((s, t));
                }
                None => self.soltar_titulo(t),
            }
        }
        *i.titulos.borrow_mut() = quedan;
        self.programar();
    }

    /// Suelta todos los widgets (otra nota, otro tamaño).
    fn limpiar_piezas(&self) {
        let i = &self.0;
        for (_, c) in std::mem::take(&mut *i.vivas.borrow_mut()) {
            self.soltar(c);
        }
        for (_, t) in std::mem::take(&mut *i.titulos.borrow_mut()) {
            self.soltar_titulo(t);
        }
        i.cola.borrow_mut().clear();
        i.fijo.set_visible(false);
    }

    /// Pide actualizar los widgets en el próximo idle (nunca durante la disposición).
    fn programar(&self) {
        if self.0.pendiente.replace(true) {
            return;
        }
        let d = Rc::downgrade(&self.0);
        glib::idle_add_local_once(move || {
            if let Some(i) = d.upgrade() {
                i.pendiente.set(false);
                GaleriaLateral(i).actualizar();
            }
        });
    }

    /// Crea los widgets de lo visible y de una pantalla de margen, suelta los que salieron,
    /// pone el título fijo y pide las miniaturas que faltan.
    fn actualizar(&self) {
        let i = &self.0;
        if !i.root.is_mapped() || i.paginas.visible_child_name().as_deref() != Some("grilla") {
            return;
        }
        // El ancho real de la grilla (la barra derecha es fija: casi nunca cambia).
        // Menos los márgenes del lienzo (12 de cada lado) y un respiro para la barra de
        // desplazamiento superpuesta.
        let ancho = i.scroller.width() - 28;
        if ancho > 100 && ancho != i.ancho.get() {
            i.ancho.set(ancho);
            self.disponer();
            return;
        }
        let adj = i.scroller.vadjustment();
        let a = adj.value() as i32;
        let alto = (adj.page_size() as i32).max(300);
        let b = a + alto;
        let (ma, mb) = (a - alto, b + alto);
        let activo = i.activo.get();
        let necesarias: Vec<(Pieza, Rect)> = i
            .disp
            .borrow()
            .piezas
            .iter()
            .filter(|(p, r)| r.en_franja(ma, mb) || Some(*p) == activo)
            .copied()
            .collect();
        // Soltar lo que salió.
        let vivas = std::mem::take(&mut *i.vivas.borrow_mut());
        let mut quedan = Vec::with_capacity(vivas.len());
        for (k, c) in vivas {
            if necesarias.iter().any(|(p, _)| *p == Pieza::Mosaico(k)) {
                quedan.push((k, c));
            } else {
                self.soltar(c);
            }
        }
        *i.vivas.borrow_mut() = quedan;
        let titulos = std::mem::take(&mut *i.titulos.borrow_mut());
        let mut quedan = Vec::with_capacity(titulos.len());
        for (s, t) in titulos {
            if necesarias.iter().any(|(p, _)| *p == Pieza::Titulo(s)) {
                quedan.push((s, t));
            } else {
                self.soltar_titulo(t);
            }
        }
        *i.titulos.borrow_mut() = quedan;
        for (p, r) in &necesarias {
            match *p {
                Pieza::Mosaico(k) => {
                    if !i.vivas.borrow().iter().any(|(n, _)| *n == k) {
                        let c = self.celda(k, *r);
                        i.vivas.borrow_mut().push((k, c));
                    }
                }
                Pieza::Titulo(s) => {
                    if !i.titulos.borrow().iter().any(|(n, _)| *n == s) {
                        let t = self.titulo(s, *r);
                        i.titulos.borrow_mut().push((s, t));
                    }
                }
            }
        }
        // Tab entra a un solo elemento: el activo o, si no hay, el primero.
        let activo = activo.or_else(|| i.disp.borrow().piezas.first().map(|(p, _)| *p));
        i.activo.set(activo);
        for (k, c) in i.vivas.borrow().iter() {
            c.boton.set_focusable(activo == Some(Pieza::Mosaico(*k)));
        }
        for (s, t) in i.titulos.borrow().iter() {
            t.boton.set_focusable(activo == Some(Pieza::Titulo(*s)));
        }
        self.poner_fijo(a);
        // Las miniaturas que faltan: primero lo visible, de arriba hacia abajo.
        let e = i.root.scale_factor().max(1);
        let m = i.medios.borrow().clone();
        let mut cola: Vec<(i32, usize)> = necesarias
            .iter()
            .filter_map(|(p, r)| match p {
                Pieza::Mosaico(k) => Some((*k, *r)),
                Pieza::Titulo(_) => None,
            })
            .filter(|(k, _)| m.medios[*k].clase == Clase::Imagen && m.medios[*k].abs.is_some())
            .filter(|(k, r)| {
                i.vivas
                    .borrow()
                    .iter()
                    .find(|(n, _)| n == k)
                    .is_some_and(|(_, c)| falta_textura(c, *r, e))
            })
            .map(|(k, r)| {
                let visible = r.en_franja(a, b);
                (if visible { r.y - a } else { r.y + 100_000 }, k)
            })
            .collect();
        cola.sort_unstable();
        *i.cola.borrow_mut() = cola.into_iter().map(|(_, k)| k).collect();
        self.bombear();
    }

    /// El grupo cuyo título quedó arriba de la vista (el que se está recorriendo).
    fn poner_fijo(&self, arriba: i32) {
        let i = &self.0;
        let s = i
            .disp
            .borrow()
            .piezas
            .iter()
            .filter_map(|(p, r)| match p {
                Pieza::Titulo(s) if r.y < arriba => Some(*s),
                _ => None,
            })
            .next_back();
        match s {
            Some(s) => {
                let m = i.medios.borrow();
                let sec = &m.secciones[s];
                let n = i.disp.borrow().visibles[s];
                i.fijo_texto.set_label(&format!("{} · {n}", sec.titulo));
                i.fijo.set_tooltip_text(Some(&sec.titulo));
                i.fijo.set_visible(true);
                // SAFETY: la clave es propia de este módulo y solo guarda un `usize`.
                unsafe { i.fijo.set_data("orbital-galeria-lateral-seccion", s) };
            }
            None => i.fijo.set_visible(false),
        }
    }

    /// Una celda para el medio `k`: del depósito o nueva, con su textura si ya está en la caché.
    fn celda(&self, k: usize, r: Rect) -> Celda {
        let i = &self.0;
        let c = i
            .libres
            .borrow_mut()
            .pop()
            .unwrap_or_else(|| self.celda_nueva());
        let m = i.medios.borrow();
        let x = &m.medios[k];
        let consulta = i.consulta.borrow();
        c.nombre
            .set_markup(&disposicion::resaltado(x.nombre(), &consulta));
        let seccion = &m.secciones[x.seccion].titulo;
        let tipo = x.clase.nombre();
        let falta = x.abs.is_none();
        let pos = i
            .disp
            .borrow()
            .filtrados
            .iter()
            .position(|n| *n == k)
            .unwrap_or(0);
        let total = i.disp.borrow().filtrados.len();
        let mut etiqueta = format!("{tipo} {} de {total}: {}", pos + 1, x.nombre());
        if falta {
            etiqueta.push_str(" (falta el archivo)");
        }
        etiqueta.push_str(&format!(", en {seccion}"));
        c.boton
            .update_property(&[gtk::accessible::Property::Label(&etiqueta)]);
        c.boton
            .update_property(&[gtk::accessible::Property::Description(
                "Enter abre el visor; Control+Enter va al embed; Mayúsculas+F10 abre el menú",
            )]);
        let ayuda = match &x.leyenda {
            Some(l) => format!("{} — {l}", x.escrito),
            None => x.escrito.clone(),
        };
        c.boton.set_tooltip_text(Some(&if falta {
            format!("Falta: {ayuda}")
        } else {
            ayuda
        }));
        // SAFETY: la clave es propia de este módulo y solo se guarda un `usize`.
        unsafe { c.boton.set_data("orbital-galeria-lateral-indice", k) };
        let video = x.clase == Clase::Video;
        c.marca.set_icon_name(Some(if falta {
            "image-missing-symbolic"
        } else {
            "media-playback-start-symbolic"
        }));
        c.marca.set_visible(video || falta);
        for (clase, si) in [
            ("orbital-galeria-lateral-video", video && !falta),
            ("orbital-galeria-lateral-rota", falta),
        ] {
            if si {
                c.boton.add_css_class(clase);
            } else {
                c.boton.remove_css_class(clase);
            }
        }
        c.pic.set_alternative_text(Some(x.nombre()));
        c.pic.set_paintable(gdk::Paintable::NONE);
        c.pedida.set((0, 0));
        if !video && let Some(abs) = &x.abs {
            let e = i.root.scale_factor().max(1);
            let (w, h) = medida_pedida(r, e);
            if let Some(t) = imagenes::en_cache_con(abs, w, h, Ajuste::Cubrir) {
                c.pic.set_paintable(Some(&t));
                c.pedida.set((w, h));
            }
        }
        drop(m);
        drop(consulta);
        ubicar(&i.lienzo, c.boton.upcast_ref(), r);
        self.ajustar_celda(&c, r);
        c.boton.set_visible(true);
        c
    }

    fn ajustar_celda(&self, c: &Celda, r: Rect) {
        c.pic.set_size_request(r.w, r.w);
    }

    fn celda_nueva(&self) -> Celda {
        let pic = gtk::Picture::builder()
            .can_shrink(true)
            .content_fit(gtk::ContentFit::Cover)
            .build();
        pic.set_accessible_role(gtk::AccessibleRole::Presentation);
        let marca = gtk::Image::builder()
            .icon_name("media-playback-start-symbolic")
            .pixel_size(20)
            .halign(gtk::Align::Center)
            .valign(gtk::Align::Center)
            .css_classes(["osd", "circular", "orbital-galeria-marca"])
            .visible(false)
            .build();
        marca.set_accessible_role(gtk::AccessibleRole::Presentation);
        let sobre = gtk::Overlay::builder()
            .child(&pic)
            .css_classes(["orbital-galeria-lateral-cuadro"])
            .overflow(gtk::Overflow::Hidden)
            .build();
        sobre.add_overlay(&marca);
        let nombre = gtk::Label::builder()
            .ellipsize(gtk::pango::EllipsizeMode::Middle)
            .xalign(0.5)
            .css_classes(["caption", "orbital-galeria-lateral-nombre"])
            .build();
        nombre.set_accessible_role(gtk::AccessibleRole::Presentation);
        let caja = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(2)
            .build();
        caja.append(&sobre);
        caja.append(&nombre);
        let boton = gtk::Button::builder()
            .child(&caja)
            .focusable(false)
            .css_classes(["flat", "orbital-galeria-lateral-mosaico"])
            .build();
        let d = Rc::downgrade(&self.0);
        boton.connect_clicked(move |b| {
            let Some(i) = d.upgrade() else { return };
            let Some(k) = indice_de(b) else { return };
            let g = GaleriaLateral(i);
            g.0.activo.set(Some(Pieza::Mosaico(k)));
            g.abrir_visor(k);
        });
        // Clic secundario: el menú del mosaico.
        let clic = gtk::GestureClick::builder()
            .button(gdk::BUTTON_SECONDARY)
            .build();
        let d = Rc::downgrade(&self.0);
        let bd = boton.downgrade();
        clic.connect_pressed(move |g, _, x, y| {
            let (Some(i), Some(b)) = (d.upgrade(), bd.upgrade()) else {
                return;
            };
            let Some(k) = indice_de(&b) else { return };
            g.set_state(gtk::EventSequenceState::Claimed);
            GaleriaLateral(i).abrir_menu(k, Some((x, y)));
        });
        boton.add_controller(clic);
        self.0.lienzo.put(&boton, 0.0, 0.0);
        Celda {
            boton,
            pic,
            marca,
            nombre,
            pedida: Cell::new((0, 0)),
        }
    }

    fn soltar(&self, c: Celda) {
        if c.boton.has_focus() {
            self.0.lienzo.grab_focus();
        }
        c.boton.set_visible(false);
        c.pic.set_paintable(gdk::Paintable::NONE);
        c.pedida.set((0, 0));
        self.0.libres.borrow_mut().push(c);
    }

    fn titulo(&self, s: usize, r: Rect) -> Titulo {
        let i = &self.0;
        let t = i
            .titulos_libres
            .borrow_mut()
            .pop()
            .unwrap_or_else(|| self.titulo_nuevo());
        self.llenar_titulo(&t, s);
        ubicar(&i.lienzo, t.boton.upcast_ref(), r);
        t.boton.set_visible(true);
        t
    }

    fn llenar_titulo(&self, t: &Titulo, s: usize) {
        let i = &self.0;
        let m = i.medios.borrow();
        let sec = &m.secciones[s];
        let n = i.disp.borrow().visibles[s];
        let plegada = i.plegadas.borrow().get(s).copied().unwrap_or(false);
        t.texto
            .set_markup(&disposicion::resaltado(&sec.titulo, &i.consulta.borrow()));
        t.conteo.set_label(&n.to_string());
        t.flecha.set_icon_name(Some(if plegada {
            "pan-end-symbolic"
        } else {
            "pan-down-symbolic"
        }));
        t.boton.set_tooltip_text(Some(&sec.titulo));
        let medios = if n == 1 { "medio" } else { "medios" };
        t.boton.update_property(&[
            gtk::accessible::Property::Label(&format!("{}, {n} {medios}", sec.titulo)),
            gtk::accessible::Property::Description("Enter o Espacio pliegan o despliegan el grupo"),
        ]);
        t.boton
            .update_state(&[gtk::accessible::State::Expanded(Some(!plegada))]);
        // SAFETY: la clave es propia de este módulo y solo se guarda un `usize`.
        unsafe { t.boton.set_data("orbital-galeria-lateral-indice", s) };
    }

    fn titulo_nuevo(&self) -> Titulo {
        let flecha = gtk::Image::from_icon_name("pan-down-symbolic");
        flecha.set_accessible_role(gtk::AccessibleRole::Presentation);
        let texto = gtk::Label::builder()
            .xalign(0.0)
            .hexpand(true)
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .css_classes(["heading"])
            .build();
        let conteo = gtk::Label::builder()
            .css_classes(["dim-label", "caption", "numeric"])
            .build();
        let caja = gtk::Box::builder().spacing(6).build();
        caja.append(&flecha);
        caja.append(&texto);
        caja.append(&conteo);
        let boton = gtk::Button::builder()
            .child(&caja)
            .focusable(false)
            .css_classes(["flat", "orbital-galeria-lateral-titulo"])
            .build();
        let d = Rc::downgrade(&self.0);
        boton.connect_clicked(move |b| {
            let Some(i) = d.upgrade() else { return };
            let Some(s) = indice_de(b) else { return };
            let g = GaleriaLateral(i);
            g.0.activo.set(Some(Pieza::Titulo(s)));
            g.alternar(s);
        });
        self.0.lienzo.put(&boton, 0.0, 0.0);
        Titulo {
            boton,
            flecha,
            texto,
            conteo,
        }
    }

    fn soltar_titulo(&self, t: Titulo) {
        if t.boton.has_focus() {
            self.0.lienzo.grab_focus();
        }
        t.boton.set_visible(false);
        self.0.titulos_libres.borrow_mut().push(t);
    }

    /// Pliega o despliega el grupo `s`.
    fn alternar(&self, s: usize) {
        {
            let mut p = self.0.plegadas.borrow_mut();
            if let Some(x) = p.get_mut(s) {
                *x = !*x;
            }
        }
        self.disponer();
        self.mostrar(
            Pieza::Titulo(s),
            self.0
                .titulos
                .borrow()
                .iter()
                .any(|(n, t)| *n == s && t.boton.has_focus()),
        );
    }

    /// Pliega (o despliega) todos los grupos.
    pub fn plegar_todo(&self, plegar: bool) {
        for x in self.0.plegadas.borrow_mut().iter_mut() {
            *x = plegar;
        }
        if plegar {
            self.0.scroller.vadjustment().set_value(0.0);
        }
        self.disponer();
    }

    /// Pide las miniaturas de la cola, de a [`EN_VUELO`].
    fn bombear(&self) {
        let i = &self.0;
        let e = i.root.scale_factor().max(1);
        while i.en_vuelo.get() < EN_VUELO {
            let siguiente = {
                let mut cola = i.cola.borrow_mut();
                (!cola.is_empty()).then(|| cola.remove(0))
            };
            let Some(k) = siguiente else { return };
            let Some(r) = i.disp.borrow().rect(Pieza::Mosaico(k)) else {
                continue;
            };
            let Some(abs) = i.medios.borrow().medios[k].abs.clone() else {
                continue;
            };
            let (w, h) = medida_pedida(r, e);
            {
                let vivas = i.vivas.borrow();
                let Some((_, c)) = vivas.iter().find(|(n, _)| *n == k) else {
                    continue;
                };
                c.pedida.set((w, h));
            }
            i.en_vuelo.set(i.en_vuelo.get() + 1);
            let d = Rc::downgrade(&self.0);
            let lista = Rc::downgrade(&i.medios.borrow());
            imagenes::pedir_con(&abs, w, h, Ajuste::Cubrir, move |t| {
                let Some(i) = d.upgrade() else { return };
                i.en_vuelo.set(i.en_vuelo.get().saturating_sub(1));
                // La lista pudo cambiar mientras tanto (otra nota).
                let misma = lista
                    .upgrade()
                    .is_some_and(|l| Rc::ptr_eq(&l, &i.medios.borrow()));
                if misma
                    && let Some((_, c)) = i.vivas.borrow().iter().find(|(n, _)| *n == k)
                    && c.pedida.get() == (w, h)
                {
                    match &t {
                        Some(t) => c.pic.set_paintable(Some(t)),
                        None => c.boton.add_css_class("orbital-galeria-lateral-rota"),
                    }
                }
                let d = Rc::downgrade(&i);
                glib::idle_add_local_once(move || {
                    if let Some(i) = d.upgrade() {
                        GaleriaLateral(i).bombear();
                    }
                });
            });
        }
    }

    fn tecla(&self, k: gdk::Key, mods: gdk::ModifierType) -> glib::Propagation {
        use gdk::Key;
        let i = &self.0;
        let activo = i.activo.get();
        let ctrl = mods.contains(gdk::ModifierType::CONTROL_MASK);
        let mayus = mods.contains(gdk::ModifierType::SHIFT_MASK);
        if let Some(Pieza::Mosaico(n)) = activo {
            if ctrl && matches!(k, Key::Return | Key::KP_Enter) {
                self.ir_al_embed(n);
                return glib::Propagation::Stop;
            }
            if k == Key::Menu || (mayus && k == Key::F10) {
                self.abrir_menu(n, None);
                return glib::Propagation::Stop;
            }
        }
        if mods.intersects(
            gdk::ModifierType::CONTROL_MASK
                | gdk::ModifierType::ALT_MASK
                | gdk::ModifierType::SUPER_MASK
                | gdk::ModifierType::SHIFT_MASK,
        ) {
            return glib::Propagation::Proceed;
        }
        let disp = i.disp.borrow();
        let piezas = &disp.piezas;
        if piezas.is_empty() {
            return glib::Propagation::Proceed;
        }
        let actual = activo
            .and_then(|a| piezas.iter().position(|(p, _)| *p == a))
            .unwrap_or(0);
        let destino = match k {
            Key::Left | Key::KP_Left => disposicion::vecina(piezas, actual, Direccion::Izquierda),
            Key::Right | Key::KP_Right => disposicion::vecina(piezas, actual, Direccion::Derecha),
            Key::Up | Key::KP_Up => disposicion::vecina(piezas, actual, Direccion::Arriba),
            Key::Down | Key::KP_Down => disposicion::vecina(piezas, actual, Direccion::Abajo),
            Key::Home | Key::KP_Home => Some(0),
            Key::End | Key::KP_End => Some(piezas.len() - 1),
            _ => return glib::Propagation::Proceed,
        };
        let destino = destino.map(|n| piezas[n].0);
        drop(disp);
        if let Some(p) = destino {
            self.mostrar(p, true);
        }
        glib::Propagation::Stop
    }

    /// Activa la pieza `p`: crea su widget si hace falta, la trae a la vista y, con `foco`, le
    /// pone el foco.
    fn mostrar(&self, p: Pieza, foco: bool) {
        let i = &self.0;
        let Some(r) = i.disp.borrow().rect(p) else {
            return;
        };
        i.activo.set(Some(p));
        match p {
            Pieza::Mosaico(k) => {
                if !i.vivas.borrow().iter().any(|(n, _)| *n == k) {
                    let c = self.celda(k, r);
                    i.vivas.borrow_mut().push((k, c));
                }
            }
            Pieza::Titulo(s) => {
                if !i.titulos.borrow().iter().any(|(n, _)| *n == s) {
                    let t = self.titulo(s, r);
                    i.titulos.borrow_mut().push((s, t));
                }
            }
        }
        for (n, c) in i.vivas.borrow().iter() {
            c.boton.set_focusable(p == Pieza::Mosaico(*n));
        }
        for (n, t) in i.titulos.borrow().iter() {
            t.boton.set_focusable(p == Pieza::Titulo(*n));
        }
        if foco {
            // Que se vea, sin quedar tapada por el título fijo.
            let adj = i.scroller.vadjustment();
            let arriba = f64::from(r.y);
            let abajo = arriba + f64::from(r.h);
            let tapa = f64::from(disposicion::ALTO_TITULO);
            let valor = if arriba - tapa < adj.value() {
                Some((arriba - tapa - 4.0).max(0.0))
            } else if abajo > adj.value() + adj.page_size() {
                Some(abajo - adj.page_size() + 4.0)
            } else {
                None
            };
            if let Some(v) = valor {
                let tope = (f64::from(i.disp.borrow().alto) - adj.page_size()).max(0.0);
                let v = v.min(tope);
                if v > adj.upper() - adj.page_size() {
                    i.desplazar.set(Some(v));
                }
                adj.set_value(v);
            }
            if let Some(w) = self.widget_de(p) {
                w.grab_focus();
            }
        }
        self.programar();
    }

    fn widget_de(&self, p: Pieza) -> Option<gtk::Button> {
        let i = &self.0;
        match p {
            Pieza::Mosaico(k) => i
                .vivas
                .borrow()
                .iter()
                .find(|(n, _)| *n == k)
                .map(|(_, c)| c.boton.clone()),
            Pieza::Titulo(s) => i
                .titulos
                .borrow()
                .iter()
                .find(|(n, _)| *n == s)
                .map(|(_, t)| t.boton.clone()),
        }
    }

    /// El visor a pantalla completa con los medios que pasan la búsqueda y el filtro, desde el
    /// medio `k`. Al cerrarlo, el foco vuelve al mosaico en que se salió.
    fn abrir_visor(&self, k: usize) {
        let i = &self.0;
        let (c, lista) = {
            let mut v = i.visor.borrow_mut();
            v.get_or_insert_with(|| {
                let lista = i.disp.borrow().filtrados.clone();
                (Rc::new(carrusel_de(&i.medios.borrow(), &lista)), lista)
            })
            .clone()
        };
        let Some(n) = lista.iter().position(|x| *x == k) else {
            return;
        };
        let d = Rc::downgrade(&self.0);
        let l = lista.clone();
        let al_cerrar = move |n: usize| {
            let Some(i) = d.upgrade() else { return };
            if i.saltando.replace(false) {
                return;
            }
            let Some(k) = l.get(n).copied() else { return };
            let d = Rc::downgrade(&i);
            glib::idle_add_local_once(move || {
                if let Some(i) = d.upgrade() {
                    let g = GaleriaLateral(i);
                    // Si estaba plegado, se despliega su grupo.
                    let s = g.0.medios.borrow().medios.get(k).map(|m| m.seccion);
                    if let Some(s) = s
                        && g.0.plegadas.borrow().get(s).copied().unwrap_or(false)
                    {
                        g.alternar(s);
                    }
                    g.mostrar(Pieza::Mosaico(k), true);
                }
            });
        };
        let d = Rc::downgrade(&self.0);
        let ir = Rc::new(move |n: usize| {
            let Some(i) = d.upgrade() else { return };
            let Some(k) = lista.get(n).copied() else {
                return;
            };
            i.saltando.set(true);
            let d = Rc::downgrade(&i);
            // Después de cerrar el visor (el foco tiene que quedar en el editor).
            glib::idle_add_local_once(move || {
                if let Some(i) = d.upgrade() {
                    GaleriaLateral(i).ir_al_embed(k);
                }
            });
        });
        crate::carrusel::abrir_visor_con_embed(&i.root, c, n, Rc::new(|_: &str| {}), al_cerrar, ir);
    }

    fn ir_al_embed(&self, k: usize) {
        let m = self.0.medios.borrow().medios.get(k).cloned();
        if let Some(m) = m {
            self.pedir(Pedido::IrAlEmbed(m));
        }
    }

    fn copiar_ruta(&self, k: usize) {
        let m = self.0.medios.borrow().medios.get(k).cloned();
        let Some(m) = m else { return };
        match &m.path {
            Some(p) => {
                self.0.root.clipboard().set_text(p.as_str());
                self.pedir(Pedido::Aviso(format!("Ruta copiada: {}", p.as_str())));
            }
            None => self.pedir(Pedido::Aviso(format!(
                "«{}» no está en la bóveda",
                m.escrito
            ))),
        }
    }

    /// El menú de un mosaico, en el puntero o sobre el mosaico.
    fn abrir_menu(&self, k: usize, en: Option<(f64, f64)>) {
        let i = &self.0;
        i.menu.set(Some(k));
        let Some(r) = i.disp.borrow().rect(Pieza::Mosaico(k)) else {
            return;
        };
        let modelo = gio::Menu::new();
        modelo.append(Some("Ir al embed"), Some("galeria.ir-al-embed"));
        modelo.append(
            Some("Copiar ruta en la bóveda"),
            Some("galeria.copiar-ruta"),
        );
        modelo.append(
            Some("Mostrar en explorador"),
            Some("galeria.mostrar-explorador"),
        );
        let pop = gtk::PopoverMenu::from_model(Some(&modelo));
        pop.set_parent(&i.lienzo);
        let (x, y) = en.map_or((r.x + r.w / 2, r.y + r.h / 2), |(x, y)| {
            (r.x + x as i32, r.y + y as i32)
        });
        pop.set_pointing_to(Some(&gdk::Rectangle::new(x, y, 1, 1)));
        pop.set_has_arrow(true);
        let d = Rc::downgrade(&self.0);
        pop.connect_closed(move |p| {
            let p = p.clone();
            // Después de que corra la acción elegida.
            glib::idle_add_local_once(move || p.unparent());
            if let Some(i) = d.upgrade()
                && let Some(Pieza::Mosaico(k)) = i.activo.get()
                && let Some(b) = GaleriaLateral(i).widget_de(Pieza::Mosaico(k))
            {
                b.grab_focus();
            }
        });
        pop.popup();
    }

    /// Medios con widget vivo (pruebas).
    #[cfg(test)]
    pub fn vivas(&self) -> Vec<usize> {
        self.0.vivas.borrow().iter().map(|(k, _)| *k).collect()
    }

    /// Las piezas dispuestas (pruebas).
    #[cfg(test)]
    pub fn piezas(&self) -> Vec<Pieza> {
        self.0
            .disp
            .borrow()
            .piezas
            .iter()
            .map(|(p, _)| *p)
            .collect()
    }

    #[cfg(test)]
    pub fn buscar(&self, texto: &str) {
        self.0.busqueda.set_text(texto);
        // `search-changed` llega con una pausa: se aplica ya.
        *self.0.consulta.borrow_mut() = disposicion::plegar(texto.trim());
        self.disponer();
    }

    #[cfg(test)]
    pub fn filtrar(&self, nombre: &str) {
        self.0.filtro_grupo.set_active_name(Some(nombre));
    }

    #[cfg(test)]
    pub fn activar(&self, p: Pieza) {
        self.mostrar(p, true);
        if let Some(b) = self.widget_de(p) {
            b.emit_clicked();
        }
    }

    #[cfg(test)]
    pub fn pulsar(&self, k: gdk::Key, mods: gdk::ModifierType) {
        let _ = self.tecla(k, mods);
    }

    #[cfg(test)]
    pub fn activo(&self) -> Option<Pieza> {
        self.0.activo.get()
    }

    #[cfg(test)]
    pub fn con_textura(&self) -> usize {
        self.0
            .vivas
            .borrow()
            .iter()
            .filter(|(_, c)| c.pic.paintable().is_some())
            .count()
    }

    #[cfg(test)]
    pub fn resumen(&self) -> String {
        self.0.resumen.label().to_string()
    }

    #[cfg(test)]
    pub fn pagina(&self) -> String {
        self.0
            .paginas
            .visible_child_name()
            .map(|s| s.to_string())
            .unwrap_or_default()
    }

    #[cfg(test)]
    pub fn titulo_fijo(&self) -> Option<String> {
        self.0
            .fijo
            .is_visible()
            .then(|| self.0.fijo_texto.label().to_string())
    }

    #[cfg(test)]
    pub fn scroller(&self) -> gtk::ScrolledWindow {
        self.0.scroller.clone()
    }

    #[cfg(test)]
    pub fn accion(&self, nombre: &str, k: Option<usize>, valor: Option<&glib::Variant>) {
        if k.is_some() {
            self.0.menu.set(k);
        }
        self.0.acciones.activate_action(nombre, valor);
    }
}

impl Inner {
    fn fijo_seccion(&self) -> Option<usize> {
        // SAFETY: la clave solo guarda un `usize` (ver `poner_fijo`).
        unsafe { self.fijo.data::<usize>("orbital-galeria-lateral-seccion") }
            .map(|p| unsafe { *p.as_ref() })
    }
}

/// El índice que guarda un mosaico o un título reciclado.
fn indice_de(b: &gtk::Button) -> Option<usize> {
    // SAFETY: la clave solo guarda un `usize` (ver `celda` y `llenar_titulo`).
    unsafe { b.data::<usize>("orbital-galeria-lateral-indice") }.map(|p| unsafe { *p.as_ref() })
}

/// Los medios `lista` como un carrusel para el visor (032).
fn carrusel_de(m: &MediosNota, lista: &[usize]) -> Carrusel {
    let mut c = Carrusel {
        diapositivas: lista
            .iter()
            .map(|k| {
                let x = &m.medios[*k];
                Diapositiva {
                    escrito: x
                        .path
                        .as_ref()
                        .map_or_else(|| x.escrito.clone(), |p| p.to_string()),
                    fragmento: x.fragmento.clone(),
                    leyenda: x.leyenda.clone(),
                    clase: x.clase,
                    estado: match (&x.path, &x.abs) {
                        (Some(path), Some(abs)) => Estado::Encontrado {
                            path: path.clone(),
                            abs: abs.clone(),
                        },
                        _ => Estado::Faltante,
                    },
                    de_carpeta: false,
                    enlace: None,
                }
            })
            .collect(),
        resuelto: true,
        ..Carrusel::default()
    };
    let o = &mut c.opciones;
    o.miniaturas = lista.len() <= MINIATURAS_HASTA;
    o.ampliada = true;
    o.autoplay = true;
    c
}

/// El tamaño en píxeles del dispositivo de la miniatura de una celda (el cuadro es de `r.w` de
/// lado), redondeado de a 32 como en la galería de la 033.
fn medida_pedida(r: Rect, e: i32) -> (i32, i32) {
    let q = ((r.w * e + 31) / 32) * 32;
    (q, q)
}

fn falta_textura(c: &Celda, r: Rect, e: i32) -> bool {
    if c.pic.paintable().is_none() {
        return c.pedida.get() == (0, 0);
    }
    let (w, _) = c.pedida.get();
    let (nw, _) = medida_pedida(r, e);
    nw > w * 5 / 4
}

fn ubicar(lienzo: &gtk::Fixed, w: &gtk::Widget, r: Rect) {
    w.set_size_request(r.w, r.h);
    lienzo.move_(w, f64::from(r.x), f64::from(r.y));
}
