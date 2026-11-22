//! Ampliar y copiar un diagrama Mermaid desde la Lectura (059).
//!
//! - **Ampliar**: una ventana propia, modal y a pantalla completa (como el visor de fotos de la
//!   033), con la imagen ampliable del carrusel ([`crate::carrusel::zoom::Zoom`]): rueda, `+`,
//!   `-` y doble clic acercan; arrastrar recorre; `0` ajusta. La imagen se vuelve a dibujar del
//!   tamaño de la pantalla y, al acercar, más grande (en el pool de `gio`, sin la caché: son
//!   imágenes grandes que se usan una vez).
//! - **Copiar como imagen**: PNG con los colores de papel sobre blanco, al doble de resolución.
//! - **Copiar como SVG**: el SVG con los colores de papel (`image/svg+xml` y como texto).
//! - En la nota, el diagrama se enfoca con Tab; Intro o un clic lo amplían; el menú contextual
//!   (clic derecho, mantener apretado, la tecla Menú o Mayús+F10) ofrece las tres cosas;
//!   Ctrl+C copia la imagen y Ctrl+Mayús+C el SVG.

use std::rc::Rc;
use std::sync::Arc;

use gtk::prelude::*;
use gtk::{gdk, gio, glib};

use crate::diagramas::{self, Clave, Estilo, Tema};

/// Escala de la imagen copiada (píxeles por píxel del diagrama).
const ESCALA_COPIA: f32 = 2.0;
/// Fondo de la imagen copiada: papel.
const PAPEL: [u8; 3] = [255, 255, 255];

/// Da al diagrama de la nota el clic, el teclado y el menú para ampliarlo y copiarlo.
pub fn interactivo(pic: &gtk::Picture, fuente: &str, estilo: Estilo) {
    let fuente: Arc<str> = Arc::from(fuente.strip_suffix('\n').unwrap_or(fuente));
    pic.set_focusable(true);
    pic.set_cursor_from_name(Some("zoom-in"));
    pic.set_tooltip_text(Some(
        "Clic o Intro para ampliar; menú contextual para copiar",
    ));
    let acciones = gio::SimpleActionGroup::new();
    let accion = |nombre: &str, f: Rc<dyn Fn(&gtk::Widget)>| {
        let a = gio::SimpleAction::new(nombre, None);
        let p = pic.downgrade();
        a.connect_activate(move |_, _| {
            if let Some(p) = p.upgrade() {
                f(p.upcast_ref());
            }
        });
        acciones.add_action(&a);
    };
    {
        let f = fuente.clone();
        accion(
            "ampliar",
            Rc::new(move |w| {
                abrir(w, f.clone(), estilo);
            }),
        );
    }
    {
        let f = fuente.clone();
        accion(
            "copiar-imagen",
            Rc::new(move |w| copiar_imagen(w, f.clone())),
        );
    }
    {
        let f = fuente.clone();
        accion("copiar-svg", Rc::new(move |w| copiar_svg(w, &f)));
    }
    pic.insert_action_group("diagrama", Some(&acciones));

    let clic = gtk::GestureClick::new();
    clic.set_button(0);
    clic.connect_released(|g, n, x, y| {
        let Some(w) = g.widget() else { return };
        match g.current_button() {
            gdk::BUTTON_PRIMARY if n == 1 => {
                let _ = w.activate_action("diagrama.ampliar", None);
            }
            gdk::BUTTON_SECONDARY => menu(&w, Some((x, y))),
            _ => return,
        }
        g.set_state(gtk::EventSequenceState::Claimed);
    });
    pic.add_controller(clic);
    let largo = gtk::GestureLongPress::new();
    largo.set_touch_only(true);
    largo.connect_pressed(|g, x, y| {
        if let Some(w) = g.widget() {
            menu(&w, Some((x, y)));
        }
    });
    pic.add_controller(largo);
    let teclas = gtk::EventControllerKey::new();
    teclas.connect_key_pressed(|c, k, _, m| {
        let Some(w) = c.widget() else {
            return glib::Propagation::Proceed;
        };
        let ctrl = m.contains(gdk::ModifierType::CONTROL_MASK);
        let mayus = m.contains(gdk::ModifierType::SHIFT_MASK);
        let accion = match k {
            gdk::Key::Return | gdk::Key::KP_Enter | gdk::Key::space => "diagrama.ampliar",
            gdk::Key::c | gdk::Key::C if ctrl && mayus => "diagrama.copiar-svg",
            gdk::Key::c | gdk::Key::C if ctrl => "diagrama.copiar-imagen",
            gdk::Key::Menu => {
                menu(&w, None);
                return glib::Propagation::Stop;
            }
            gdk::Key::F10 if mayus => {
                menu(&w, None);
                return glib::Propagation::Stop;
            }
            _ => return glib::Propagation::Proceed,
        };
        let _ = w.activate_action(accion, None);
        glib::Propagation::Stop
    });
    pic.add_controller(teclas);
}

/// El menú contextual del diagrama, en el puntero o sobre el diagrama.
pub(crate) fn menu(w: &gtk::Widget, en: Option<(f64, f64)>) {
    let modelo = gio::Menu::new();
    modelo.append(Some("Ampliar"), Some("diagrama.ampliar"));
    modelo.append(Some("Copiar como imagen"), Some("diagrama.copiar-imagen"));
    modelo.append(Some("Copiar como SVG"), Some("diagrama.copiar-svg"));
    let pop = gtk::PopoverMenu::from_model(Some(&modelo));
    pop.set_parent(w);
    let (x, y) = en.unwrap_or((f64::from(w.width()) / 2.0, f64::from(w.height()) / 2.0));
    pop.set_pointing_to(Some(&gdk::Rectangle::new(x as i32, y as i32, 1, 1)));
    let debil = w.downgrade();
    pop.connect_closed(move |p| {
        let p = p.clone();
        let debil = debil.clone();
        // Después de que corra la acción elegida; el foco vuelve al diagrama.
        glib::idle_add_local_once(move || {
            p.unparent();
            if let Some(w) = debil.upgrade() {
                w.grab_focus();
            }
        });
    });
    pop.popup();
}

/// Avisa al lector de pantalla (y a quien mira: el aviso de la ventana ampliada).
fn avisar(w: &gtk::Widget, texto: &str) {
    w.announce(texto, gtk::AccessibleAnnouncementPriority::Medium);
    if let Some(t) = w
        .ancestor(adw::ToastOverlay::static_type())
        .and_downcast::<adw::ToastOverlay>()
    {
        t.add_toast(adw::Toast::new(texto));
    }
}

/// Copia el diagrama como imagen PNG (papel, al doble de resolución). Se dibuja en el pool de
/// `gio`; el portapapeles la convierte a PNG cuando otra aplicación la pide.
pub fn copiar_imagen(w: &gtk::Widget, fuente: Arc<str>) {
    let debil = w.downgrade();
    glib::spawn_future_local(async move {
        let r = gio::spawn_blocking(move || {
            diagramas::dibujar_a(&fuente, Tema::Claro, ESCALA_COPIA, Some(PAPEL))
        })
        .await;
        let Some(w) = debil.upgrade() else { return };
        match r {
            Ok(Ok(d)) => {
                w.clipboard().set_texture(&d.textura());
                avisar(&w, "Diagrama copiado como imagen");
            }
            _ => avisar(&w, "No se pudo copiar el diagrama"),
        }
    });
}

/// El SVG del diagrama para copiar (colores de papel).
pub fn svg_para_copiar(fuente: &str) -> Option<String> {
    diagramas::svg(fuente, Tema::Claro)
        .ok()
        .map(|d| d.marcado.clone())
}

/// Copia el diagrama como SVG: `image/svg+xml` (lo pegan los programas de dibujo) y texto.
pub fn copiar_svg(w: &gtk::Widget, fuente: &str) {
    let Some(svg) = svg_para_copiar(fuente) else {
        avisar(w, "No se pudo copiar el diagrama");
        return;
    };
    let bytes = glib::Bytes::from_owned(svg.clone().into_bytes());
    let contenido = gdk::ContentProvider::new_union(&[
        gdk::ContentProvider::for_bytes("image/svg+xml", &bytes),
        gdk::ContentProvider::for_value(&svg.to_value()),
    ]);
    if w.clipboard().set_content(Some(&contenido)).is_ok() {
        avisar(w, "Diagrama copiado como SVG");
    } else {
        avisar(w, "No se pudo copiar el diagrama");
    }
}

/// Abre el diagrama ampliado: una ventana modal a pantalla completa con la imagen ampliable y
/// los botones de copiar y cerrar.
pub fn abrir(desde: &gtk::Widget, fuente: Arc<str>, estilo: Estilo) -> Option<gtk::Window> {
    let d = diagramas::svg(&fuente, estilo.tema).ok()?;
    let nombre = d.nombre();
    let (ancho, alto) = (d.ancho, d.alto);
    let tema = estilo.tema;
    let pedir: crate::carrusel::zoom::Pedir = {
        let fuente = fuente.clone();
        Rc::new(move |lado, listo| {
            let fuente = fuente.clone();
            // `lado` píxeles en el lado mayor del diagrama (con el tope de la imagen).
            let escala = (lado as f32 / ancho.max(alto).max(1.0)).clamp(0.25, 16.0);
            glib::spawn_future_local(async move {
                let r =
                    gio::spawn_blocking(move || diagramas::dibujar_a(&fuente, tema, escala, None))
                        .await;
                listo(match r {
                    Ok(Ok(d)) => Some(d.textura()),
                    _ => None,
                });
            });
        })
    };
    let zoom = crate::carrusel::zoom::Zoom::con_fuente(pedir, &nombre, Rc::new(|_| {}));
    let z = zoom.widget();
    z.set_focusable(true);
    z.update_property(&[
        gtk::accessible::Property::Label(&nombre),
        gtk::accessible::Property::Description(
            "Rueda, más y menos para acercar; doble clic o cero para ajustar; arrastrar para recorrer; Ctrl+C copia la imagen y Ctrl+Mayús+C el SVG; Escape para salir",
        ),
    ]);
    // Mientras llega la del tamaño de la pantalla, la de la nota.
    if let Some(Ok(d)) = diagramas::en_cache(&Clave::new(&fuente, estilo)) {
        zoom.picture().set_paintable(Some(&d.textura()));
    }

    let boton = |icono: &str, texto: &str, accion: &str| {
        let b = gtk::Button::builder()
            .icon_name(icono)
            .tooltip_text(texto)
            .action_name(accion)
            .css_classes(["circular", "osd"])
            .build();
        b.update_property(&[gtk::accessible::Property::Label(texto)]);
        b
    };
    let barra = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(8)
        .halign(gtk::Align::End)
        .valign(gtk::Align::Start)
        .margin_top(12)
        .margin_end(12)
        .build();
    barra.append(&boton(
        "edit-copy-symbolic",
        "Copiar como imagen",
        "diagrama.copiar-imagen",
    ));
    barra.append(&boton(
        "text-x-generic-symbolic",
        "Copiar como SVG",
        "diagrama.copiar-svg",
    ));
    barra.append(&boton("window-close-symbolic", "Cerrar", "diagrama.cerrar"));
    let capas = gtk::Overlay::builder().child(z).build();
    capas.add_overlay(&barra);
    let tostadas = adw::ToastOverlay::new();
    tostadas.set_child(Some(&capas));
    let ventana = gtk::Window::builder()
        .title(nombre.as_str())
        .modal(true)
        .child(&tostadas)
        .css_classes(["orbital-diagrama-ventana"])
        .build();
    if let Some(madre) = desde.root().and_downcast::<gtk::Window>() {
        ventana.set_transient_for(Some(&madre));
        ventana.set_application(madre.application().as_ref());
    }

    let acciones = gio::SimpleActionGroup::new();
    {
        let a = gio::SimpleAction::new("copiar-imagen", None);
        let (f, w) = (fuente.clone(), z.downgrade());
        a.connect_activate(move |_, _| {
            if let Some(w) = w.upgrade() {
                copiar_imagen(w.upcast_ref(), f.clone());
            }
        });
        acciones.add_action(&a);
        let a = gio::SimpleAction::new("copiar-svg", None);
        let (f, w) = (fuente.clone(), z.downgrade());
        a.connect_activate(move |_, _| {
            if let Some(w) = w.upgrade() {
                copiar_svg(w.upcast_ref(), &f);
            }
        });
        acciones.add_action(&a);
        let a = gio::SimpleAction::new("cerrar", None);
        let v = ventana.downgrade();
        a.connect_activate(move |_, _| {
            if let Some(v) = v.upgrade() {
                v.close();
            }
        });
        acciones.add_action(&a);
    }
    ventana.insert_action_group("diagrama", Some(&acciones));

    let teclas = gtk::EventControllerKey::new();
    {
        let zoom = zoom.clone();
        let v = ventana.downgrade();
        teclas.connect_key_pressed(move |_, k, _, m| {
            let Some(v) = v.upgrade() else {
                return glib::Propagation::Proceed;
            };
            let ctrl = m.contains(gdk::ModifierType::CONTROL_MASK);
            let mayus = m.contains(gdk::ModifierType::SHIFT_MASK);
            let accion = match k {
                gdk::Key::Escape => "diagrama.cerrar",
                gdk::Key::c | gdk::Key::C if ctrl && mayus => "diagrama.copiar-svg",
                gdk::Key::c | gdk::Key::C if ctrl => "diagrama.copiar-imagen",
                _ if !ctrl && zoom.tecla(k) => return glib::Propagation::Stop,
                _ => return glib::Propagation::Proceed,
            };
            let _ = v.activate_action(accion, None);
            glib::Propagation::Stop
        });
    }
    ventana.add_controller(teclas);
    ventana.fullscreen();
    ventana.present();
    z.grab_focus();
    // La imagen del tamaño de la pantalla (el lado mayor del monitor, en píxeles físicos).
    let lado = desde
        .display()
        .monitor_at_surface(&desde.native()?.surface()?)
        .map(|m| {
            let g = m.geometry();
            g.width().max(g.height()) * m.scale_factor().max(1)
        })
        .unwrap_or(1920);
    zoom.cargar(lado);
    Some(ventana)
}
