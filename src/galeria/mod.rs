//! Galería en la nota (033): la grilla de una carpeta de la bóveda en la Lectura, en Dividida y
//! en OnStage (usan el mismo `Reader`).
//!
//! Un solo widget anclado por galería: un `GtkFixed` del alto de la grilla entera, con celdas
//! solo para lo que se ve y una pantalla de margen arriba y abajo (al desplazar, las que salen se
//! reutilizan para las que entran). Las miniaturas se decodifican fuera del hilo de GTK, de a
//! pocas a la vez y empezando por lo visible ([`crate::imagenes`]); las proporciones de
//! `masonry` y `justificadas` se leen de las cabeceras, también afuera. Nada de esto ocurre en
//! el arranque ni en las notas sin galería.
//!
//! Teclado: Tab entra a la foto elegida; las flechas recorren la grilla, Inicio y Fin van a la
//! primera y a la última, Av Pág y Re Pág cambian de página (con `paginar`), Enter o Espacio
//! abren el visor a pantalla completa, que es el del carrusel (032).

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use adw::prelude::*;
use gtk::{gdk, glib};
use orbital_vault::carrusel::Clase;
use orbital_vault::galeria::{Galeria, Layout};

use crate::imagenes::{self, Ajuste};

pub mod disposicion;

use disposicion::{Direccion, Disposicion, Pedido, Rect};

pub use crate::carrusel::AbrirFn;

/// Miniaturas que se decodifican a la vez (el Celeron de referencia tiene 4 núcleos: uno queda
/// para la interfaz).
const EN_VUELO: usize = 3;
/// Proporciones que se leen antes de mostrar la primera tanda de `masonry` y `justificadas`.
const PRIMERA_TANDA: usize = 48;
/// Con más elementos, la disposición `carrusel` no lleva tira de miniaturas (cientos de botones).
const MINIATURAS_HASTA: usize = 200;
/// Proporción de un video (no se lee el archivo).
const PROPORCION_VIDEO: f32 = 16.0 / 9.0;

/// Una celda viva: un botón con la miniatura.
struct Celda {
    boton: gtk::Button,
    pic: gtk::Picture,
    marca: gtk::Image,
    /// El tamaño en píxeles del dispositivo de la textura pedida o puesta.
    pedida: Cell<(i32, i32)>,
}

struct Inner {
    g: Rc<Galeria>,
    abrir: AbrirFn,
    /// Débil: el widget es dueño de la galería (la guarda en sus datos), no al revés.
    raiz: glib::WeakRef<gtk::Box>,
    lienzo: gtk::Fixed,
    paginas: Option<Paginacion>,
    pagina: Cell<usize>,
    ancho: Cell<i32>,
    /// Ancho sobre alto de cada elemento (`None`: todavía no se leyó).
    proporciones: RefCell<Vec<Option<f32>>>,
    leyendo: Cell<bool>,
    disp: RefCell<Disposicion>,
    /// Elemento (índice en la galería) → su celda.
    vivas: RefCell<Vec<(usize, Celda)>>,
    libres: RefCell<Vec<Celda>>,
    titulos: RefCell<Vec<gtk::Label>>,
    /// El elemento con el foco (o que lo recibe al entrar con Tab).
    activo: Cell<usize>,
    cola: RefCell<Vec<usize>>,
    en_vuelo: Cell<usize>,
    pendiente: Cell<bool>,
    vigia: RefCell<Option<(gtk::Adjustment, Vec<glib::SignalHandlerId>)>>,
    visor: RefCell<Option<Rc<orbital_vault::carrusel::Carrusel>>>,
}

struct Paginacion {
    anterior: gtk::Button,
    siguiente: gtk::Button,
    rotulo: gtk::Label,
}

impl Inner {
    fn raiz(&self) -> gtk::Box {
        self.raiz.upgrade().unwrap_or_default()
    }
}

/// Una galería: el widget es el dueño (la guarda en sus datos).
#[derive(Clone)]
struct Vista(Rc<Inner>);

/// Si el foco de la ventana está dentro de una galería (sus teclas no las toma OnStage).
pub fn tiene_foco(ventana: &gtk::Window) -> bool {
    let mut w = gtk::prelude::GtkWindowExt::focus(ventana);
    while let Some(x) = w {
        if x.has_css_class("orbital-galeria") {
            return true;
        }
        w = x.parent();
    }
    false
}

/// La galería de la nota, del ancho de la columna `ancho`.
pub fn widget(g: Rc<Galeria>, ancho: i32, abrir: AbrirFn) -> gtk::Widget {
    let raiz = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(6)
        .css_classes(["orbital-galeria"])
        .build();
    raiz.set_accessible_role(gtk::AccessibleRole::Group);
    raiz.update_property(&[gtk::accessible::Property::Label(&g.descripcion())]);
    let n = g.elementos.len();
    if let Some(motivo) = &g.vacia {
        raiz.append(&vacia(motivo));
        return raiz.upcast();
    }
    // El rótulo: la carpeta y cuántas hay.
    let rotulo = gtk::Label::builder()
        .label(g.descripcion().trim_start_matches("Galería: "))
        .xalign(0.0)
        .ellipsize(gtk::pango::EllipsizeMode::Middle)
        .css_classes(["dim-label", "caption", "orbital-galeria-rotulo"])
        .build();
    rotulo.set_accessible_role(gtk::AccessibleRole::Presentation);
    raiz.append(&rotulo);
    if g.opciones.layout == Layout::Carrusel {
        // El carrusel de la 032 es el mismo componente.
        let c = crate::carrusel::widget(
            Rc::new(g.a_carrusel(n <= MINIATURAS_HASTA)),
            ancho - 16,
            abrir,
        );
        c.set_width_request(ancho - 16);
        raiz.append(&c);
        let c = c.downgrade();
        raiz.connect_width_request_notify(move |r| {
            if let Some(c) = c.upgrade()
                && r.width_request() > 0
            {
                c.set_width_request(r.width_request() - 16);
            }
        });
        return raiz.upcast();
    }
    let lienzo = gtk::Fixed::builder()
        .css_classes(["orbital-galeria-lienzo"])
        .build();
    lienzo.set_accessible_role(gtk::AccessibleRole::Grid);
    lienzo.update_property(&[gtk::accessible::Property::Label(&g.descripcion())]);
    raiz.append(&lienzo);
    let paginas = (g.paginas() > 1).then(|| {
        let barra = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(8)
            .halign(gtk::Align::Center)
            .build();
        let boton = |icono: &str, nombre: &str| {
            let b = gtk::Button::builder()
                .icon_name(icono)
                .tooltip_text(nombre)
                .css_classes(["flat", "circular"])
                .build();
            b.update_property(&[gtk::accessible::Property::Label(nombre)]);
            b
        };
        let anterior = boton("go-previous-symbolic", "Página anterior (Re Pág)");
        let siguiente = boton("go-next-symbolic", "Página siguiente (Av Pág)");
        let rotulo = gtk::Label::builder().css_classes(["numeric"]).build();
        barra.append(&anterior);
        barra.append(&rotulo);
        barra.append(&siguiente);
        raiz.append(&barra);
        Paginacion {
            anterior,
            siguiente,
            rotulo,
        }
    });
    let proporciones = g
        .elementos
        .iter()
        .map(|e| {
            (e.clase == Clase::Video || g.opciones.layout == Layout::Miniaturas)
                .then_some(PROPORCION_VIDEO)
        })
        .collect();
    let this = Vista(Rc::new(Inner {
        abrir,
        raiz: raiz.downgrade(),
        lienzo,
        paginas,
        pagina: Cell::new(0),
        ancho: Cell::new(ancho.max(120)),
        proporciones: RefCell::new(proporciones),
        leyendo: Cell::new(false),
        disp: RefCell::default(),
        vivas: RefCell::default(),
        libres: RefCell::default(),
        titulos: RefCell::default(),
        activo: Cell::new(0),
        cola: RefCell::default(),
        en_vuelo: Cell::new(0),
        pendiente: Cell::new(false),
        vigia: RefCell::default(),
        visor: RefCell::default(),
        g,
    }));
    this.conectar();
    this.disponer();
    // SAFETY: la clave es propia de este módulo y solo se guarda un `Rc<Inner>`.
    unsafe { raiz.set_data("orbital-galeria", this.0.clone()) };
    raiz.upcast()
}

/// El estado vacío: el motivo, en vez de un hueco.
fn vacia(motivo: &str) -> gtk::Widget {
    let b = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(10)
        .css_classes(["orbital-galeria-vacia"])
        .build();
    let im = gtk::Image::from_icon_name("folder-pictures-symbolic");
    im.set_pixel_size(24);
    im.add_css_class("dim-label");
    im.set_accessible_role(gtk::AccessibleRole::Presentation);
    b.append(&im);
    b.append(
        &gtk::Label::builder()
            .label(motivo)
            .wrap(true)
            .xalign(0.0)
            .css_classes(["dim-label"])
            .build(),
    );
    b.upcast()
}

impl Vista {
    fn conectar(&self) {
        let i = &self.0;
        let debil = || Rc::downgrade(&self.0);
        let raiz = i.raiz();
        if let Some(p) = &i.paginas {
            let d = debil();
            p.anterior.connect_clicked(move |_| {
                if let Some(i) = d.upgrade() {
                    let v = Vista(i);
                    v.ir_a_pagina(v.0.pagina.get().saturating_sub(1), true);
                }
            });
            let d = debil();
            p.siguiente.connect_clicked(move |_| {
                if let Some(i) = d.upgrade() {
                    let v = Vista(i);
                    v.ir_a_pagina(v.0.pagina.get() + 1, true);
                }
            });
        }
        // Teclado (en burbuja: la celda con el foco no usa las flechas).
        let teclas = gtk::EventControllerKey::new();
        {
            let d = debil();
            teclas.connect_key_pressed(move |_, k, _, mods| {
                d.upgrade()
                    .map_or(glib::Propagation::Proceed, |i| Vista(i).tecla(k, mods))
            });
        }
        raiz.add_controller(teclas);
        {
            let d = debil();
            raiz.connect_map(move |_| {
                if let Some(i) = d.upgrade() {
                    let v = Vista(i);
                    v.vigilar_vista();
                    v.leer_proporciones();
                    v.programar();
                }
            });
        }
        {
            let d = debil();
            raiz.connect_unmap(move |_| {
                if let Some(i) = d.upgrade() {
                    if let Some((adj, ids)) = i.vigia.borrow_mut().take() {
                        for id in ids {
                            adj.disconnect(id);
                        }
                    }
                    i.cola.borrow_mut().clear();
                }
            });
        }
        // La columna cambió de ancho: se vuelve a disponer (en un idle: puede llegar durante
        // la disposición de la vista).
        {
            let d = debil();
            raiz.connect_width_request_notify(move |r| {
                let Some(i) = d.upgrade() else { return };
                let w = r.width_request();
                if w <= 0 || w == i.ancho.get() {
                    return;
                }
                i.ancho.set(w);
                let d = Rc::downgrade(&i);
                glib::idle_add_local_once(move || {
                    if let Some(i) = d.upgrade() {
                        Vista(i).disponer();
                    }
                });
            });
        }
    }

    /// El rango de elementos de la página vigente.
    fn rango(&self) -> std::ops::Range<usize> {
        self.0.g.rango_de_pagina(self.0.pagina.get())
    }

    /// Calcula la grilla de la página, acomoda el alto del lienzo, los títulos y las celdas vivas.
    fn disponer(&self) {
        let i = &self.0;
        let g = &i.g;
        let r = self.rango();
        let props = i.proporciones.borrow();
        let grupos: Vec<usize> = if g.con_titulos() {
            g.elementos[r.clone()].iter().map(|e| e.grupo).collect()
        } else {
            Vec::new()
        };
        let d = disposicion::disponer(&Pedido {
            layout: g.opciones.layout,
            ancho: i.ancho.get(),
            proporciones: &props[r.clone()],
            grupos: (!grupos.is_empty()).then_some(&grupos[..]),
            columnas: g.opciones.columnas,
            alto_fila: g.opciones.alto as i32,
        });
        drop(props);
        i.lienzo.set_size_request(i.ancho.get(), d.alto.max(1));
        // Los títulos de grupo (pocos): todos.
        for t in i.titulos.borrow_mut().drain(..) {
            i.lienzo.remove(&t);
        }
        for (grupo, rect) in &d.titulos {
            let nombre = &g.grupos[*grupo].nombre;
            let texto = if nombre.is_empty() {
                g.opciones
                    .carpeta
                    .as_deref()
                    .unwrap_or("")
                    .trim_start_matches("./")
            } else {
                nombre
            };
            let l = gtk::Label::builder()
                .label(texto)
                .xalign(0.0)
                .valign(gtk::Align::End)
                .ellipsize(gtk::pango::EllipsizeMode::Middle)
                .css_classes(["heading", "orbital-galeria-titulo"])
                .build();
            l.set_accessible_role(gtk::AccessibleRole::Heading);
            l.set_size_request(rect.w, rect.h);
            i.lienzo.put(&l, f64::from(rect.x), f64::from(rect.y));
            i.titulos.borrow_mut().push(l);
        }
        *i.disp.borrow_mut() = d;
        // Las celdas vivas van a su lugar nuevo (o se sueltan si ya no tienen).
        let vivas = std::mem::take(&mut *i.vivas.borrow_mut());
        let mut quedan = Vec::with_capacity(vivas.len());
        for (k, c) in vivas {
            match self.rect(k) {
                Some(rect) => {
                    ubicar(&i.lienzo, &c, rect);
                    quedan.push((k, c));
                }
                None => self.soltar(c),
            }
        }
        *i.vivas.borrow_mut() = quedan;
        if let Some(p) = &i.paginas {
            let (n, total) = (i.pagina.get(), g.paginas());
            p.rotulo.set_label(&format!("Página {} de {total}", n + 1));
            p.anterior.set_sensitive(n > 0);
            p.siguiente.set_sensitive(n + 1 < total);
        }
        self.programar();
    }

    /// El rectángulo de un elemento si la página lo tiene dispuesto.
    fn rect(&self, k: usize) -> Option<Rect> {
        let r = self.rango();
        if !r.contains(&k) {
            return None;
        }
        self.0.disp.borrow().celdas.get(k - r.start).copied()
    }

    /// Pide actualizar las celdas en el próximo idle (nunca durante la disposición).
    fn programar(&self) {
        if self.0.pendiente.replace(true) {
            return;
        }
        let d = Rc::downgrade(&self.0);
        glib::idle_add_local_once(move || {
            if let Some(i) = d.upgrade() {
                i.pendiente.set(false);
                Vista(i).actualizar();
            }
        });
    }

    /// La vista de desplazamiento que contiene la galería.
    fn scroller(&self) -> Option<gtk::ScrolledWindow> {
        self.0
            .raiz()
            .ancestor(gtk::ScrolledWindow::static_type())
            .and_downcast::<gtk::ScrolledWindow>()
    }

    /// La franja visible del lienzo (en sus coordenadas) y el alto de la pantalla.
    fn franja(&self) -> (i32, i32) {
        let i = &self.0;
        let Some(sw) = self.scroller() else {
            return (0, 1200);
        };
        let alto = sw.height().max(400);
        match i.lienzo.compute_bounds(&sw) {
            Some(b) => {
                let arriba = (-b.y()) as i32;
                (arriba, arriba + alto)
            }
            None => (0, alto),
        }
    }

    /// Crea las celdas de lo visible y de una pantalla de margen, suelta las que salieron y
    /// pide las miniaturas que faltan, de lo visible hacia afuera.
    fn actualizar(&self) {
        let i = &self.0;
        let raiz = i.raiz();
        if !raiz.is_mapped() {
            return;
        }
        let (a, b) = self.franja();
        let margen = b - a;
        let (ma, mb) = (a - margen, b + margen);
        let r = self.rango();
        let activo = i.activo.get();
        let mut necesarias: Vec<(usize, Rect)> = i
            .disp
            .borrow()
            .celdas
            .iter()
            .enumerate()
            .filter(|(_, c)| c.en_franja(ma, mb))
            .map(|(k, c)| (r.start + k, *c))
            .collect();
        // Soltar las que salieron (menos la que tiene o recibe el foco).
        let vivas = std::mem::take(&mut *i.vivas.borrow_mut());
        let mut quedan = Vec::with_capacity(necesarias.len() + 1);
        for (k, c) in vivas {
            if k == activo || necesarias.iter().any(|(n, _)| *n == k) {
                quedan.push((k, c));
            } else {
                self.soltar(c);
            }
        }
        *i.vivas.borrow_mut() = quedan;
        if r.contains(&activo) && !necesarias.iter().any(|(k, _)| *k == activo) {
            if let Some(rect) = self.rect(activo) {
                necesarias.push((activo, rect));
            }
        }
        let e = raiz.scale_factor().max(1);
        for (k, rect) in &necesarias {
            if !i.vivas.borrow().iter().any(|(n, _)| n == k) {
                let c = self.celda(*k, *rect);
                i.vivas.borrow_mut().push((*k, c));
            }
        }
        // El foco va y viene por una sola celda (Tab no recorre cien fotos).
        if !r.contains(&activo) {
            i.activo.set(r.start);
        }
        for (k, c) in i.vivas.borrow().iter() {
            c.boton.set_focusable(*k == i.activo.get());
        }
        // Las miniaturas que faltan, primero lo visible y lo más cerca del centro.
        let centro = (a + b) / 2;
        let mut cola: Vec<(i32, usize)> = necesarias
            .iter()
            .filter(|(k, _)| i.g.elementos[*k].clase == Clase::Imagen)
            .filter(|(k, rect)| {
                i.vivas
                    .borrow()
                    .iter()
                    .find(|(n, _)| n == k)
                    .is_some_and(|(_, c)| falta_textura(c, *rect, e))
            })
            .map(|(k, rect)| {
                let visible = rect.en_franja(a, b);
                let dist = (rect.y + rect.h / 2 - centro).abs();
                (if visible { dist } else { dist + 100_000 }, *k)
            })
            .collect();
        cola.sort_unstable();
        *i.cola.borrow_mut() = cola.into_iter().map(|(_, k)| k).collect();
        self.bombear();
    }

    /// Una celda para el elemento `k`: del depósito o nueva; con su textura si ya está en la
    /// caché.
    fn celda(&self, k: usize, rect: Rect) -> Celda {
        let i = &self.0;
        let c = i.libres.borrow_mut().pop().unwrap_or_else(|| self.celda_nueva());
        let el = &i.g.elementos[k];
        let total = i.g.elementos.len();
        let tipo = el.clase.nombre();
        c.boton.update_property(&[gtk::accessible::Property::Label(&format!(
            "{tipo} {} de {total}: {}",
            k + 1,
            el.nombre()
        ))]);
        c.boton.set_tooltip_text(Some(el.nombre()));
        // SAFETY: la clave es propia de este módulo y solo se guarda un `usize`.
        unsafe { c.boton.set_data("orbital-galeria-indice", k) };
        let video = el.clase == Clase::Video;
        c.marca.set_visible(video);
        if video {
            c.boton.add_css_class("orbital-galeria-video");
        } else {
            c.boton.remove_css_class("orbital-galeria-video");
        }
        c.pic.set_alternative_text(Some(el.nombre()));
        c.pic.set_paintable(gdk::Paintable::NONE);
        c.pedida.set((0, 0));
        if !video {
            let e = i.raiz().scale_factor().max(1);
            let (w, h) = medida_pedida(rect, e);
            if let Some(t) = imagenes::en_cache_con(&el.abs, w, h, Ajuste::Cubrir) {
                c.pic.set_paintable(Some(&t));
                c.pedida.set((w, h));
            }
        }
        ubicar(&i.lienzo, &c, rect);
        c.boton.set_visible(true);
        c
    }

    fn celda_nueva(&self) -> Celda {
        let pic = gtk::Picture::builder()
            .can_shrink(true)
            .content_fit(gtk::ContentFit::Cover)
            .hexpand(true)
            .vexpand(true)
            .build();
        pic.set_accessible_role(gtk::AccessibleRole::Presentation);
        let marca = gtk::Image::builder()
            .icon_name("media-playback-start-symbolic")
            .pixel_size(32)
            .halign(gtk::Align::Center)
            .valign(gtk::Align::Center)
            .css_classes(["osd", "circular", "orbital-galeria-marca"])
            .visible(false)
            .build();
        marca.set_accessible_role(gtk::AccessibleRole::Presentation);
        let sobre = gtk::Overlay::builder().child(&pic).build();
        sobre.add_overlay(&marca);
        let boton = gtk::Button::builder()
            .child(&sobre)
            .focusable(false)
            .css_classes(["orbital-galeria-celda"])
            .build();
        let d = Rc::downgrade(&self.0);
        boton.connect_clicked(move |b| {
            let Some(i) = d.upgrade() else { return };
            // SAFETY: la clave solo guarda un `usize` (ver `celda`).
            let Some(k) = (unsafe { b.data::<usize>("orbital-galeria-indice") })
                .map(|p| unsafe { *p.as_ref() })
            else {
                return;
            };
            let v = Vista(i);
            v.enfocar(k, false);
            v.abrir_visor(k);
        });
        i_put(&self.0.lienzo, &boton);
        Celda {
            boton,
            pic,
            marca,
            pedida: Cell::new((0, 0)),
        }
    }

    /// Devuelve una celda al depósito (oculta, sin imagen).
    fn soltar(&self, c: Celda) {
        if c.boton.has_focus() {
            self.0.raiz().grab_focus();
        }
        c.boton.set_visible(false);
        c.pic.set_paintable(gdk::Paintable::NONE);
        c.pedida.set((0, 0));
        self.0.libres.borrow_mut().push(c);
    }

    /// Pide las miniaturas de la cola, de a [`EN_VUELO`].
    fn bombear(&self) {
        let i = &self.0;
        let e = i.raiz().scale_factor().max(1);
        while i.en_vuelo.get() < EN_VUELO {
            let Some(k) = (!i.cola.borrow().is_empty()).then(|| i.cola.borrow_mut().remove(0))
            else {
                return;
            };
            let Some(rect) = self.rect(k) else { continue };
            let (w, h) = medida_pedida(rect, e);
            {
                let vivas = i.vivas.borrow();
                let Some((_, c)) = vivas.iter().find(|(n, _)| *n == k) else {
                    continue;
                };
                c.pedida.set((w, h));
            }
            i.en_vuelo.set(i.en_vuelo.get() + 1);
            let d = Rc::downgrade(&self.0);
            imagenes::pedir_con(&i.g.elementos[k].abs, w, h, Ajuste::Cubrir, move |t| {
                let Some(i) = d.upgrade() else { return };
                i.en_vuelo.set(i.en_vuelo.get().saturating_sub(1));
                if let Some((_, c)) = i.vivas.borrow().iter().find(|(n, _)| *n == k)
                    && c.pedida.get() == (w, h)
                {
                    match &t {
                        Some(t) => c.pic.set_paintable(Some(t)),
                        None => c.boton.add_css_class("orbital-galeria-rota"),
                    }
                }
                // Lo que sigue, después de volver al bucle (no en el mismo cuadro).
                let d = Rc::downgrade(&i);
                glib::idle_add_local_once(move || {
                    if let Some(i) = d.upgrade() {
                        Vista(i).bombear();
                    }
                });
            });
        }
    }

    /// Sigue el desplazamiento de la vista que contiene la galería.
    fn vigilar_vista(&self) {
        let i = &self.0;
        if i.vigia.borrow().is_some() {
            return;
        }
        let Some(sw) = self.scroller() else { return };
        let adj = sw.vadjustment();
        let mut ids = Vec::with_capacity(2);
        let d = Rc::downgrade(&self.0);
        ids.push(adj.connect_value_changed(move |_| {
            if let Some(i) = d.upgrade() {
                Vista(i).programar();
            }
        }));
        let d = Rc::downgrade(&self.0);
        ids.push(adj.connect_page_size_notify(move |_| {
            if let Some(i) = d.upgrade() {
                Vista(i).programar();
            }
        }));
        *i.vigia.borrow_mut() = Some((adj, ids));
    }

    /// Lee en el pool de `gio` las proporciones que faltan de la página (solo `masonry` y
    /// `justificadas`): primero una tanda chica para mostrar algo enseguida, después el resto.
    fn leer_proporciones(&self) {
        let i = &self.0;
        if i.leyendo.get() {
            return;
        }
        let faltan: Vec<usize> = {
            let props = i.proporciones.borrow();
            self.rango().filter(|k| props[*k].is_none()).collect()
        };
        if faltan.is_empty() {
            return;
        }
        i.leyendo.set(true);
        let d = Rc::downgrade(&self.0);
        glib::spawn_future_local(async move {
            let mut desde = 0;
            while desde < faltan.len() {
                let tanda = if desde == 0 {
                    PRIMERA_TANDA
                } else {
                    faltan.len()
                };
                let hasta = (desde + tanda).min(faltan.len());
                let paths: Vec<std::path::PathBuf> = {
                    let Some(i) = d.upgrade() else { return };
                    faltan[desde..hasta]
                        .iter()
                        .map(|k| i.g.elementos[*k].abs.clone())
                        .collect()
                };
                let medidas = imagenes::medidas(paths).await;
                let Some(i) = d.upgrade() else { return };
                {
                    let mut props = i.proporciones.borrow_mut();
                    for (k, m) in faltan[desde..hasta].iter().zip(medidas) {
                        props[*k] = Some(m.map_or(1.0, |(w, h)| w as f32 / h as f32));
                    }
                }
                Vista(i).disponer();
                desde = hasta;
            }
            if let Some(i) = d.upgrade() {
                i.leyendo.set(false);
                // Si mientras tanto se cambió de página.
                Vista(i).leer_proporciones();
            }
        });
    }

    fn ir_a_pagina(&self, p: usize, foco: bool) {
        let i = &self.0;
        let total = i.g.paginas();
        let p = p.min(total.saturating_sub(1));
        if p == i.pagina.get() {
            return;
        }
        i.pagina.set(p);
        i.activo.set(self.rango().start);
        i.cola.borrow_mut().clear();
        self.disponer();
        self.leer_proporciones();
        // Que se vea el principio de la página nueva.
        self.mostrar_rect(Rect {
            x: 0,
            y: 0,
            w: 1,
            h: 1,
        });
        if foco {
            let k = i.activo.get();
            let d = Rc::downgrade(&self.0);
            glib::idle_add_local_once(move || {
                if let Some(i) = d.upgrade() {
                    Vista(i).enfocar(k, true);
                }
            });
        }
    }

    fn tecla(&self, k: gdk::Key, mods: gdk::ModifierType) -> glib::Propagation {
        use gdk::Key;
        if mods.intersects(
            gdk::ModifierType::CONTROL_MASK
                | gdk::ModifierType::ALT_MASK
                | gdk::ModifierType::SUPER_MASK
                | gdk::ModifierType::SHIFT_MASK,
        ) {
            return glib::Propagation::Proceed;
        }
        let i = &self.0;
        let r = self.rango();
        if r.is_empty() {
            return glib::Propagation::Proceed;
        }
        let actual = i.activo.get().clamp(r.start, r.end - 1);
        let mover = |dir| {
            disposicion::vecina(&i.disp.borrow().celdas, actual - r.start, dir).map(|n| r.start + n)
        };
        let destino = match k {
            Key::Left | Key::KP_Left => mover(Direccion::Izquierda),
            Key::Right | Key::KP_Right => mover(Direccion::Derecha),
            Key::Up | Key::KP_Up => mover(Direccion::Arriba),
            Key::Down | Key::KP_Down => mover(Direccion::Abajo),
            Key::Home | Key::KP_Home => Some(r.start),
            Key::End | Key::KP_End => Some(r.start + i.disp.borrow().celdas.len().saturating_sub(1)),
            Key::Page_Down | Key::KP_Page_Down if i.paginas.is_some() => {
                self.ir_a_pagina(i.pagina.get() + 1, true);
                return glib::Propagation::Stop;
            }
            Key::Page_Up | Key::KP_Page_Up if i.paginas.is_some() => {
                self.ir_a_pagina(i.pagina.get().saturating_sub(1), true);
                return glib::Propagation::Stop;
            }
            _ => return glib::Propagation::Proceed,
        };
        // En un borde la tecla queda acá (el foco no se escapa de la grilla).
        if let Some(n) = destino {
            self.enfocar(n, true);
        }
        glib::Propagation::Stop
    }

    /// Pone el foco (o lo deja listo para Tab) en el elemento `k` de la página vigente: crea su
    /// celda si hace falta y la trae a la vista.
    fn enfocar(&self, k: usize, foco: bool) {
        let i = &self.0;
        let Some(rect) = self.rect(k) else { return };
        i.activo.set(k);
        if !i.vivas.borrow().iter().any(|(n, _)| *n == k) {
            let c = self.celda(k, rect);
            i.vivas.borrow_mut().push((k, c));
        }
        for (n, c) in i.vivas.borrow().iter() {
            c.boton.set_focusable(*n == k);
        }
        if foco {
            self.mostrar_rect(rect);
            if let Some((_, c)) = i.vivas.borrow().iter().find(|(n, _)| *n == k) {
                c.boton.grab_focus();
            }
        }
        self.programar();
    }

    /// Desplaza la vista lo justo para que se vea `rect` (en coordenadas del lienzo).
    fn mostrar_rect(&self, rect: Rect) {
        let i = &self.0;
        let Some(sw) = self.scroller() else { return };
        let Some(b) = i.lienzo.compute_bounds(&sw) else {
            return;
        };
        let adj = sw.vadjustment();
        let arriba = f64::from(b.y()) + f64::from(rect.y);
        let abajo = arriba + f64::from(rect.h);
        let alto = f64::from(sw.height());
        if arriba < 0.0 {
            adj.set_value(adj.value() + arriba - 12.0);
        } else if abajo > alto {
            adj.set_value(adj.value() + abajo - alto + 12.0);
        }
    }

    /// Abre el visor a pantalla completa en el elemento `k`; al cerrarlo, el foco vuelve a la
    /// foto en que se salió (en su página).
    fn abrir_visor(&self, k: usize) {
        let i = &self.0;
        let c = i
            .visor
            .borrow_mut()
            .get_or_insert_with(|| Rc::new(i.g.a_carrusel(i.g.elementos.len() <= MINIATURAS_HASTA)))
            .clone();
        let d = Rc::downgrade(&self.0);
        crate::carrusel::abrir_visor(
            i.raiz().upcast_ref(),
            c,
            k,
            i.abrir.clone(),
            true,
            move |n| {
                let Some(i) = d.upgrade() else { return };
                let v = Vista(i);
                let p = v.0.g.opciones.paginar.map_or(0, |por| n / por.max(1));
                if p != v.0.pagina.get() {
                    v.ir_a_pagina(p, false);
                }
                let d = Rc::downgrade(&v.0);
                glib::idle_add_local_once(move || {
                    if let Some(i) = d.upgrade() {
                        Vista(i).enfocar(n, true);
                    }
                });
            },
        );
    }
}

/// El tamaño en píxeles del dispositivo en que se pide una miniatura: el de la celda, redondeado
/// hacia arriba de a 32 (un cambio chico de ancho no vuelve a decodificar).
fn medida_pedida(rect: Rect, e: i32) -> (i32, i32) {
    let q = |v: i32| ((v * e + 31) / 32) * 32;
    (q(rect.w), q(rect.h))
}

/// Si la celda no tiene textura o la que tiene es bastante más chica que la celda.
fn falta_textura(c: &Celda, rect: Rect, e: i32) -> bool {
    if c.pic.paintable().is_none() {
        return c.pedida.get() == (0, 0);
    }
    let (w, h) = c.pedida.get();
    let (nw, nh) = medida_pedida(rect, e);
    nw > w * 5 / 4 || nh > h * 5 / 4
}

fn i_put(lienzo: &gtk::Fixed, b: &gtk::Button) {
    lienzo.put(b, 0.0, 0.0);
}

fn ubicar(lienzo: &gtk::Fixed, c: &Celda, r: Rect) {
    c.boton.set_size_request(r.w, r.h);
    lienzo.move_(&c.boton, f64::from(r.x), f64::from(r.y));
}
