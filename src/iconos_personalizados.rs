//! Renderizado, caché y diálogos de íconos personalizados (062).
//!
//! - Los íconos vectoriales del catálogo embebido se dibujan directo con `gsk::Path` (sin
//!   rasterizar para `gdk::Paintable`, nítidos a cualquier escala) y se rasterizan bajo demanda a
//!   `gdk::MemoryTexture` cuando hace falta un `gio::Icon` (por ejemplo en `adw::TabPage`).
//! - Los `.svg` locales de `.orbital/iconos/` se rasterizan una sola vez con `resvg` (a través de
//!   `orbital_diagramas::rasterizar_svg_icono`) y se guardan en caché como `gdk::MemoryTexture`.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use adw::prelude::*;
use gtk::{gdk, gio, glib, graphene, gsk, subclass::prelude::*};
use orbital_vault::iconos::{ReglaIcono, SvgLocal, TipoRegla};
use orbital_vault::iconos_catalogo::{self, COLORES, normalizar_color};

/// Color predeterminado de un ícono sin color explícito (legible sobre la base oscura de Orbital).
const COLOR_DEFECTO: &str = "#d8dce8";

// ---------------------------------------------------------------------------
// Paintable vectorial con gsk::Path (cero rasterización)
// ---------------------------------------------------------------------------

mod imp {
    use super::*;

    #[derive(Default)]
    pub struct TrazoPaintable {
        pub path: RefCell<Option<gsk::Path>>,
        pub color: RefCell<Option<gdk::RGBA>>,
        pub lado: Cell<i32>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for TrazoPaintable {
        const NAME: &'static str = "OrbitalTrazoPaintable";
        type Type = super::TrazoPaintable;
        type Interfaces = (gdk::Paintable,);
    }

    impl ObjectImpl for TrazoPaintable {}

    impl PaintableImpl for TrazoPaintable {
        fn intrinsic_width(&self) -> i32 {
            self.lado.get().max(1)
        }

        fn intrinsic_height(&self) -> i32 {
            self.lado.get().max(1)
        }

        fn snapshot(&self, snapshot: &gdk::Snapshot, width: f64, height: f64) {
            let Some(path) = self.path.borrow().clone() else {
                return;
            };
            let lado = (width.min(height) as f32).max(1.0);
            let escala = lado / 24.0;
            let color = self
                .color
                .borrow()
                .unwrap_or_else(|| gdk::RGBA::parse(COLOR_DEFECTO).unwrap_or(gdk::RGBA::WHITE));
            snapshot.save();
            snapshot.translate(&graphene::Point::new(
                (width as f32 - lado) / 2.0,
                (height as f32 - lado) / 2.0,
            ));
            snapshot.scale(escala, escala);
            let stroke = gsk::Stroke::new(if lado <= 16.0 { 1.65 } else { 1.8 });
            stroke.set_line_cap(gsk::LineCap::Round);
            stroke.set_line_join(gsk::LineJoin::Round);
            snapshot.append_stroke(&path, &stroke, &color);
            snapshot.restore();
        }
    }
}

glib::wrapper! {
    pub struct TrazoPaintable(ObjectSubclass<imp::TrazoPaintable>)
        @implements gdk::Paintable;
}

impl TrazoPaintable {
    fn new(path: gsk::Path, color: Option<gdk::RGBA>, lado: i32) -> Self {
        let this: Self = glib::Object::new();
        let imp = this.imp();
        *imp.path.borrow_mut() = Some(path);
        *imp.color.borrow_mut() = color;
        imp.lado.set(lado);
        this
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct ClaveCache {
    nombre: String,
    color: Option<String>,
    lado: i32,
    svg: Option<PathBuf>,
}

thread_local! {
    static PATHS: RefCell<HashMap<&'static str, gsk::Path>> = RefCell::new(HashMap::new());
    static PAINTABLES: RefCell<HashMap<ClaveCache, gdk::Paintable>> = RefCell::new(HashMap::new());
    static TEXTURAS: RefCell<HashMap<ClaveCache, gdk::MemoryTexture>> = RefCell::new(HashMap::new());
}

/// Limpia la caché de texturas SVG locales (por ejemplo al importar un SVG o cambiar de bóveda).
pub fn limpiar_cache() {
    PAINTABLES.with(|c| c.borrow_mut().clear());
    TEXTURAS.with(|c| c.borrow_mut().clear());
}

fn path_de_catalogo(nombre: &str) -> Option<gsk::Path> {
    let ic = iconos_catalogo::buscar(nombre)?;
    PATHS.with(|c| {
        let mut map = c.borrow_mut();
        if let Some(p) = map.get(ic.nombre) {
            return Some(p.clone());
        }
        let p = gsk::Path::parse(ic.trazo).ok()?;
        map.insert(ic.nombre, p.clone());
        Some(p)
    })
}

fn textura_desde_svg(svg: &str, lado: i32, color: Option<&str>) -> Option<gdk::MemoryTexture> {
    // 2× para pantallas HiDPI.
    let img = orbital_diagramas::rasterizar_svg_icono(svg, (lado.max(12) as u32) * 2, color)?;
    let bytes = glib::Bytes::from_owned(img.rgba);
    Some(gdk::MemoryTexture::new(
        img.ancho as i32,
        img.alto as i32,
        gdk::MemoryFormat::R8g8b8a8Premultiplied,
        &bytes,
        (img.ancho * 4) as usize,
    ))
}

/// Devuelve un `gdk::Paintable` listo para usar en `gtk::Image` o `TextBuffer::insert_paintable`.
pub fn paintable(
    nombre: &str,
    color: Option<&str>,
    lado: i32,
    svg_local: Option<&Path>,
) -> Option<gdk::Paintable> {
    let clave = ClaveCache {
        nombre: nombre.to_ascii_lowercase(),
        color: color.map(str::to_owned),
        lado,
        svg: svg_local.map(Path::to_path_buf),
    };
    if let Some(p) = PAINTABLES.with(|c| c.borrow().get(&clave).cloned()) {
        return Some(p);
    }
    let result: gdk::Paintable = if let Some(ruta_svg) = svg_local {
        let svg = std::fs::read_to_string(ruta_svg).ok()?;
        let tex = textura_desde_svg(&svg, lado, color.or(Some(COLOR_DEFECTO)))?;
        tex.upcast()
    } else {
        let path = path_de_catalogo(nombre)?;
        let rgba = color.and_then(|c| gdk::RGBA::parse(c).ok());
        TrazoPaintable::new(path, rgba, lado).upcast()
    };
    PAINTABLES.with(|c| {
        c.borrow_mut().insert(clave, result.clone());
    });
    Some(result)
}

/// Devuelve un `gio::Icon` (respaldado por `gdk::MemoryTexture`) para `adw::TabPage::set_icon`.
pub fn gicon(
    nombre: &str,
    color: Option<&str>,
    lado: i32,
    svg_local: Option<&Path>,
) -> Option<gio::Icon> {
    let clave = ClaveCache {
        nombre: nombre.to_ascii_lowercase(),
        color: color.map(str::to_owned),
        lado,
        svg: svg_local.map(Path::to_path_buf),
    };
    if let Some(t) = TEXTURAS.with(|c| c.borrow().get(&clave).cloned()) {
        return Some(t.upcast());
    }
    let tex = if let Some(ruta_svg) = svg_local {
        let svg = std::fs::read_to_string(ruta_svg).ok()?;
        textura_desde_svg(&svg, lado, color.or(Some(COLOR_DEFECTO)))?
    } else {
        let ic = iconos_catalogo::buscar(nombre)?;
        let svg = iconos_catalogo::svg_de_trazo(ic.trazo, color.unwrap_or(COLOR_DEFECTO), None);
        textura_desde_svg(&svg, lado, None)?
    };
    TEXTURAS.with(|c| {
        c.borrow_mut().insert(clave, tex.clone());
    });
    Some(tex.upcast())
}

/// Crea un `gtk::Image` pequeño de `lado` px mostrando el ícono personalizado.
pub fn imagen(
    nombre: &str,
    color: Option<&str>,
    lado: i32,
    svg_local: Option<&Path>,
) -> Option<gtk::Image> {
    let p = paintable(nombre, color, lado, svg_local)?;
    let img = gtk::Image::from_paintable(Some(&p));
    img.set_pixel_size(lado);
    Some(img)
}

// ---------------------------------------------------------------------------
// Diálogo selector de ícono (búsqueda, paleta, heredar, importar SVG)
// ---------------------------------------------------------------------------

/// Resultado del diálogo selector de ícono.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AccionSelector {
    Aplicar {
        icono: String,
        color: Option<String>,
        heredar: bool,
    },
    Quitar,
}

/// Opciones iniciales del diálogo selector de ícono.
pub struct OpcionesSelector {
    pub titulo: String,
    pub icono_actual: Option<String>,
    pub color_actual: Option<String>,
    pub heredar_actual: bool,
    pub es_carpeta: bool,
    pub mostrar_quitar: bool,
    pub svgs_locales: Vec<SvgLocal>,
    pub raiz_boveda: PathBuf,
}

/// Presenta el diálogo de selección de ícono y llama a `al_elegir` con el resultado y `al_importar`
/// si la usuaria importó un nuevo `.svg` a `.orbital/iconos/`.
pub fn mostrar_selector(
    parent: &impl IsA<gtk::Widget>,
    opts: OpcionesSelector,
    al_elegir: impl Fn(AccionSelector) + 'static,
    al_importar: impl Fn(SvgLocal) + 'static,
) {
    let dialog = adw::Dialog::builder()
        .title(&opts.titulo)
        .content_width(540)
        .content_height(560)
        .build();

    let elegido_icono: Rc<RefCell<Option<String>>> =
        Rc::new(RefCell::new(opts.icono_actual.clone()));
    let elegido_color: Rc<RefCell<Option<String>>> =
        Rc::new(RefCell::new(opts.color_actual.clone()));
    let svgs_locales: Rc<RefCell<Vec<SvgLocal>>> = Rc::new(RefCell::new(opts.svgs_locales));
    let al_elegir = Rc::new(al_elegir);
    let al_importar = Rc::new(al_importar);

    let root = gtk::Box::new(gtk::Orientation::Vertical, 0);

    let header = adw::HeaderBar::builder()
        .show_end_title_buttons(false)
        .build();
    let btn_cancel = gtk::Button::with_label("Cancelar");
    let d_cancel = dialog.clone();
    btn_cancel.connect_clicked(move |_| {
        d_cancel.close();
    });
    let btn_aplicar = gtk::Button::builder()
        .label("Aplicar")
        .css_classes(["suggested-action"])
        .sensitive(opts.icono_actual.is_some())
        .build();
    header.pack_start(&btn_cancel);
    header.pack_end(&btn_aplicar);
    root.append(&header);

    let cuerpo = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(10)
        .margin_start(16)
        .margin_end(16)
        .margin_top(12)
        .margin_bottom(12)
        .build();

    // Vista previa + buscador.
    let top_row = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    let preview = gtk::Image::builder().pixel_size(24).build();
    let preview_lbl = gtk::Label::builder()
        .xalign(0.0)
        .css_classes(["dim-label", "caption"])
        .build();
    let search = gtk::SearchEntry::builder()
        .placeholder_text("Buscar ícono (ej. cohete, libro, música)…")
        .hexpand(true)
        .build();
    search.update_property(&[gtk::accessible::Property::Label("Buscar ícono")]);
    top_row.append(&preview);
    top_row.append(&search);
    cuerpo.append(&top_row);
    cuerpo.append(&preview_lbl);

    // Selector de color.
    let color_row = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    color_row.set_valign(gtk::Align::Center);
    let color_entry = gtk::Entry::builder()
        .placeholder_text("#rrggbb")
        .width_chars(9)
        .max_length(7)
        .text(opts.color_actual.as_deref().unwrap_or(""))
        .build();
    color_entry.update_property(&[gtk::accessible::Property::Label(
        "Color hexadecimal del ícono",
    )]);

    let btn_tema = gtk::Button::builder()
        .label("Tema")
        .tooltip_text("Usar color del tema")
        .css_classes(["flat", "caption"])
        .build();
    btn_tema.update_property(&[gtk::accessible::Property::Label("Usar color del tema")]);
    color_row.append(&btn_tema);

    let mut swatch_btns: Vec<(gtk::Button, &'static str)> = Vec::new();
    for c in COLORES {
        let lbl = gtk::Label::new(None);
        lbl.set_markup(&format!("<span foreground=\"{}\">●</span>", c.hex));
        let b = gtk::Button::builder()
            .child(&lbl)
            .tooltip_text(c.nombre)
            .css_classes(["flat"])
            .build();
        b.update_property(&[gtk::accessible::Property::Label(&format!(
            "Color {}",
            c.nombre
        ))]);
        color_row.append(&b);
        swatch_btns.push((b, c.hex));
    }
    color_row.append(&color_entry);
    cuerpo.append(&color_row);

    // Grilla de íconos.
    let flow = gtk::FlowBox::builder()
        .selection_mode(gtk::SelectionMode::Single)
        .homogeneous(true)
        .min_children_per_line(6)
        .max_children_per_line(9)
        .row_spacing(6)
        .column_spacing(6)
        .valign(gtk::Align::Start)
        .build();
    flow.update_property(&[gtk::accessible::Property::Label("Catálogo de íconos")]);
    let scroll = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vexpand(true)
        .child(&flow)
        .build();
    cuerpo.append(&scroll);

    // Heredar (solo carpetas).
    let check_heredar = gtk::CheckButton::builder()
        .label("Heredar este ícono a los archivos y subcarpetas")
        .active(opts.heredar_actual)
        .visible(opts.es_carpeta)
        .build();
    cuerpo.append(&check_heredar);

    // Barra inferior: Importar SVG y Quitar ícono.
    let pie = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let btn_importar = gtk::Button::with_label("Importar SVG…");
    btn_importar.update_property(&[gtk::accessible::Property::Label(
        "Importar archivo SVG local a la bóveda",
    )]);
    pie.append(&btn_importar);
    let espaciador = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    espaciador.set_hexpand(true);
    pie.append(&espaciador);
    let btn_quitar = gtk::Button::builder()
        .label("Quitar ícono")
        .css_classes(["destructive-action"])
        .visible(opts.mostrar_quitar)
        .build();
    pie.append(&btn_quitar);
    cuerpo.append(&pie);

    root.append(&cuerpo);
    dialog.set_child(Some(&root));

    // Función para refrescar la vista previa.
    let actualizar_preview = {
        let preview = preview.clone();
        let preview_lbl = preview_lbl.clone();
        let elegido_icono = elegido_icono.clone();
        let elegido_color = elegido_color.clone();
        let svgs_locales = svgs_locales.clone();
        let btn_aplicar = btn_aplicar.clone();
        Rc::new(move || {
            let ic = elegido_icono.borrow().clone();
            let col = elegido_color.borrow().clone();
            btn_aplicar.set_sensitive(ic.is_some());
            if let Some(nombre) = ic {
                let ruta_svg = svgs_locales
                    .borrow()
                    .iter()
                    .find(|s| s.nombre == nombre)
                    .map(|s| s.ruta.clone());
                if let Some(p) = paintable(&nombre, col.as_deref(), 24, ruta_svg.as_deref()) {
                    preview.set_paintable(Some(&p));
                }
                let sufijo = if ruta_svg.is_some() {
                    " (SVG local)"
                } else {
                    ""
                };
                preview_lbl.set_label(&format!(":{nombre}:{sufijo}"));
            } else {
                preview.set_icon_name(Some("image-missing-symbolic"));
                preview_lbl.set_label("Ningún ícono seleccionado");
            }
        })
    };

    // Función para poblar la grilla según filtro y color actual.
    let poblar_grilla = {
        let flow = flow.clone();
        let search = search.clone();
        let elegido_icono = elegido_icono.clone();
        let elegido_color = elegido_color.clone();
        let svgs_locales = svgs_locales.clone();
        Rc::new(move || {
            while let Some(child) = flow.first_child() {
                flow.remove(&child);
            }
            let q = orbital_vault::fold(search.text().trim());
            let col = elegido_color.borrow().clone();
            let actual = elegido_icono.borrow().clone();
            let mut seleccionar_child: Option<gtk::FlowBoxChild> = None;

            // 1. SVGs locales propios.
            for svg in svgs_locales.borrow().iter() {
                if !q.is_empty() && !orbital_vault::fold(&svg.nombre).contains(&q) {
                    continue;
                }
                let Some(img) = imagen(&svg.nombre, col.as_deref(), 22, Some(&svg.ruta)) else {
                    continue;
                };
                img.set_margin_top(6);
                img.set_margin_bottom(6);
                img.set_margin_start(6);
                img.set_margin_end(6);
                let child = gtk::FlowBoxChild::builder()
                    .child(&img)
                    .tooltip_text(format!("{} (SVG local · :{}:)", svg.nombre, svg.nombre))
                    .build();
                child.set_widget_name(&svg.nombre);
                child.update_property(&[gtk::accessible::Property::Label(&format!(
                    "Ícono SVG local {}",
                    svg.nombre
                ))]);
                if actual.as_deref() == Some(svg.nombre.as_str()) {
                    seleccionar_child = Some(child.clone());
                }
                flow.insert(&child, -1);
            }

            // 2. Catálogo curado vectorial.
            for ic in iconos_catalogo::CATALOGO {
                if !q.is_empty() {
                    let hay = orbital_vault::fold(ic.nombre).contains(&q)
                        || orbital_vault::fold(ic.etiqueta).contains(&q)
                        || orbital_vault::fold(ic.categoria).contains(&q)
                        || orbital_vault::fold(ic.claves).contains(&q);
                    if !hay {
                        continue;
                    }
                }
                let Some(img) = imagen(ic.nombre, col.as_deref(), 22, None) else {
                    continue;
                };
                img.set_margin_top(6);
                img.set_margin_bottom(6);
                img.set_margin_start(6);
                img.set_margin_end(6);
                let child = gtk::FlowBoxChild::builder()
                    .child(&img)
                    .tooltip_text(format!("{} (:{}:)", ic.etiqueta, ic.nombre))
                    .build();
                child.set_widget_name(ic.nombre);
                child.update_property(&[gtk::accessible::Property::Label(&format!(
                    "{} ({})",
                    ic.etiqueta, ic.nombre
                ))]);
                if actual.as_deref() == Some(ic.nombre) {
                    seleccionar_child = Some(child.clone());
                }
                flow.insert(&child, -1);
            }
            if let Some(c) = seleccionar_child {
                flow.select_child(&c);
            }
        })
    };

    {
        let elegido_icono = elegido_icono.clone();
        let actualizar_preview = actualizar_preview.clone();
        flow.connect_selected_children_changed(move |f| {
            if let Some(c) = f.selected_children().first() {
                let nombre = c.widget_name().to_string();
                if !nombre.is_empty() {
                    *elegido_icono.borrow_mut() = Some(nombre);
                    actualizar_preview();
                }
            }
        });
    }
    {
        let elegido_icono = elegido_icono.clone();
        let elegido_color = elegido_color.clone();
        let check_heredar = check_heredar.clone();
        let al_elegir = al_elegir.clone();
        let d = dialog.clone();
        flow.connect_child_activated(move |_, c| {
            let nombre = c.widget_name().to_string();
            if !nombre.is_empty() {
                *elegido_icono.borrow_mut() = Some(nombre.clone());
                let color = elegido_color.borrow().clone();
                let heredar = check_heredar.is_active();
                d.close();
                al_elegir(AccionSelector::Aplicar {
                    icono: nombre,
                    color,
                    heredar,
                });
            }
        });
    }

    {
        let poblar = poblar_grilla.clone();
        search.connect_search_changed(move |_| poblar());
    }

    // Colores.
    {
        let elegido_color = elegido_color.clone();
        let color_entry = color_entry.clone();
        let actualizar_preview = actualizar_preview.clone();
        let poblar = poblar_grilla.clone();
        btn_tema.connect_clicked(move |_| {
            *elegido_color.borrow_mut() = None;
            color_entry.set_text("");
            actualizar_preview();
            poblar();
        });
    }
    for (btn, hex) in swatch_btns {
        let elegido_color = elegido_color.clone();
        let color_entry = color_entry.clone();
        let actualizar_preview = actualizar_preview.clone();
        let poblar = poblar_grilla.clone();
        btn.connect_clicked(move |_| {
            *elegido_color.borrow_mut() = Some(hex.to_owned());
            color_entry.set_text(hex);
            actualizar_preview();
            poblar();
        });
    }
    {
        let elegido_color = elegido_color.clone();
        let actualizar_preview = actualizar_preview.clone();
        let poblar = poblar_grilla.clone();
        color_entry.connect_changed(move |e| {
            let txt = e.text();
            let t = txt.trim();
            if t.is_empty() {
                *elegido_color.borrow_mut() = None;
                actualizar_preview();
                poblar();
            } else if let Some(norm) = normalizar_color(t) {
                *elegido_color.borrow_mut() = Some(norm);
                actualizar_preview();
                poblar();
            }
        });
    }

    // Importar SVG local.
    {
        let root_vault = opts.raiz_boveda.clone();
        let svgs_locales = svgs_locales.clone();
        let elegido_icono = elegido_icono.clone();
        let actualizar_preview = actualizar_preview.clone();
        let poblar = poblar_grilla.clone();
        let al_importar = al_importar.clone();
        let parent_win = parent.root().and_downcast::<gtk::Window>();
        btn_importar.connect_clicked(move |_| {
            let filter = gtk::FileFilter::new();
            filter.set_name(Some("Imágenes SVG"));
            filter.add_pattern("*.svg");
            filter.add_pattern("*.SVG");
            let filters = gio::ListStore::new::<gtk::FileFilter>();
            filters.append(&filter);
            let fd = gtk::FileDialog::builder()
                .title("Importar ícono SVG")
                .filters(&filters)
                .build();
            let root_vault = root_vault.clone();
            let svgs_locales = svgs_locales.clone();
            let elegido_icono = elegido_icono.clone();
            let actualizar_preview = actualizar_preview.clone();
            let poblar = poblar.clone();
            let al_importar = al_importar.clone();
            let win = parent_win.clone();
            glib::spawn_future_local(async move {
                let Ok(file) = fd.open_future(win.as_ref()).await else {
                    return;
                };
                let Some(src) = file.path() else { return };
                let imported = gio::spawn_blocking(move || {
                    orbital_vault::iconos::importar_svg_local(&root_vault, &src)
                })
                .await
                .ok()
                .and_then(Result::ok);
                if let Some(svg) = imported {
                    limpiar_cache();
                    svgs_locales.borrow_mut().push(svg.clone());
                    *elegido_icono.borrow_mut() = Some(svg.nombre.clone());
                    al_importar(svg);
                    actualizar_preview();
                    poblar();
                }
            });
        });
    }

    // Quitar y Aplicar.
    {
        let d = dialog.clone();
        let al_elegir = al_elegir.clone();
        btn_quitar.connect_clicked(move |_| {
            d.close();
            al_elegir(AccionSelector::Quitar);
        });
    }
    {
        let d = dialog.clone();
        let elegido_icono = elegido_icono.clone();
        let elegido_color = elegido_color.clone();
        let check_heredar = check_heredar.clone();
        let al_elegir = al_elegir.clone();
        btn_aplicar.connect_clicked(move |_| {
            if let Some(icono) = elegido_icono.borrow().clone() {
                let color = elegido_color.borrow().clone();
                let heredar = check_heredar.is_active();
                d.close();
                al_elegir(AccionSelector::Aplicar {
                    icono,
                    color,
                    heredar,
                });
            }
        });
    }

    actualizar_preview();
    poblar_grilla();
    dialog.present(Some(parent));
    let s = search.clone();
    glib::idle_add_local_once(move || {
        s.grab_focus();
    });
}

// ---------------------------------------------------------------------------
// Diálogo de reglas automáticas de íconos
// ---------------------------------------------------------------------------

/// Presenta el gestor de reglas automáticas de íconos.
pub fn mostrar_reglas(
    parent: &impl IsA<gtk::Widget>,
    reglas_iniciales: Vec<ReglaIcono>,
    svgs_locales: Vec<SvgLocal>,
    raiz_boveda: PathBuf,
    al_cambiar: impl Fn(Vec<ReglaIcono>) + 'static,
    al_importar: impl Fn(SvgLocal) + 'static,
) {
    let dialog = adw::Dialog::builder()
        .title("Reglas automáticas de íconos")
        .content_width(580)
        .content_height(540)
        .build();

    let reglas = Rc::new(RefCell::new(reglas_iniciales));
    let svgs = Rc::new(RefCell::new(svgs_locales));
    let al_cambiar = Rc::new(al_cambiar);
    let al_importar = Rc::new(al_importar);

    let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
    let header = adw::HeaderBar::new();
    root.append(&header);

    let cuerpo = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(12)
        .margin_start(16)
        .margin_end(16)
        .margin_top(12)
        .margin_bottom(12)
        .build();

    let desc = gtk::Label::builder()
        .label("Se evalúan en orden cuando una nota o carpeta no tiene ícono propio ni heredado.")
        .wrap(true)
        .xalign(0.0)
        .css_classes(["dim-label", "caption"])
        .build();
    cuerpo.append(&desc);

    let list_box = gtk::ListBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .css_classes(["boxed-list"])
        .build();
    list_box.update_property(&[gtk::accessible::Property::Label("Reglas configuradas")]);
    let scroll = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vexpand(true)
        .child(&list_box)
        .build();
    cuerpo.append(&scroll);

    // Formulario para agregar una regla nueva.
    let form_sep = gtk::Separator::new(gtk::Orientation::Horizontal);
    cuerpo.append(&form_sep);

    let form_lbl = gtk::Label::builder()
        .label("Nueva regla")
        .xalign(0.0)
        .css_classes(["heading"])
        .build();
    cuerpo.append(&form_lbl);

    let tipos = [
        TipoRegla::Carpeta,
        TipoRegla::Patron,
        TipoRegla::PropTipo,
        TipoRegla::Etiqueta,
    ];
    let tipo_dropdown =
        gtk::DropDown::from_strings(&["Carpeta", "Patrón de nombre", "Propiedad type", "Etiqueta"]);
    tipo_dropdown.update_property(&[gtk::accessible::Property::Label("Tipo de regla")]);

    let valor_entry = gtk::Entry::builder()
        .placeholder_text("Ej. proyectos, *diario*, atomo, fisica")
        .hexpand(true)
        .build();
    valor_entry.update_property(&[gtk::accessible::Property::Label("Valor de la regla")]);

    let fila1 = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    fila1.append(&tipo_dropdown);
    fila1.append(&valor_entry);
    cuerpo.append(&fila1);

    let nuevo_icono: Rc<RefCell<String>> = Rc::new(RefCell::new("estrella".to_owned()));
    let nuevo_color: Rc<RefCell<Option<String>>> = Rc::new(RefCell::new(None));

    let fila2 = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let preview_nueva = gtk::Image::builder().pixel_size(18).build();
    let btn_elegir_icono = gtk::Button::with_label("Elegir ícono…");
    let btn_agregar = gtk::Button::builder()
        .label("Agregar regla")
        .css_classes(["suggested-action"])
        .build();
    let esp = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    esp.set_hexpand(true);
    fila2.append(&preview_nueva);
    fila2.append(&btn_elegir_icono);
    fila2.append(&esp);
    fila2.append(&btn_agregar);
    cuerpo.append(&fila2);

    root.append(&cuerpo);
    dialog.set_child(Some(&root));

    let pintar_preview_nueva = {
        let preview_nueva = preview_nueva.clone();
        let nuevo_icono = nuevo_icono.clone();
        let nuevo_color = nuevo_color.clone();
        let svgs = svgs.clone();
        Rc::new(move || {
            let ic = nuevo_icono.borrow().clone();
            let col = nuevo_color.borrow().clone();
            let ruta_svg = svgs
                .borrow()
                .iter()
                .find(|s| s.nombre == ic)
                .map(|s| s.ruta.clone());
            if let Some(p) = paintable(&ic, col.as_deref(), 18, ruta_svg.as_deref()) {
                preview_nueva.set_paintable(Some(&p));
            }
        })
    };
    pintar_preview_nueva();

    // Reconstruir la lista de reglas.
    type ReconstruirFn = Rc<RefCell<Option<Rc<dyn Fn()>>>>;
    let reconstruir: ReconstruirFn = Rc::new(RefCell::new(None));
    let reconstruir_fn: Rc<dyn Fn()> = {
        let list_box = list_box.clone();
        let reglas = reglas.clone();
        let svgs = svgs.clone();
        let al_cambiar = al_cambiar.clone();
        let reconstruir = reconstruir.clone();
        Rc::new(move || {
            while let Some(child) = list_box.first_child() {
                list_box.remove(&child);
            }
            let lista = reglas.borrow().clone();
            if lista.is_empty() {
                let vacio = gtk::Label::builder()
                    .label("Todavía no hay reglas automáticas.")
                    .margin_top(16)
                    .margin_bottom(16)
                    .css_classes(["dim-label"])
                    .build();
                list_box.append(&vacio);
                return;
            }
            let total = lista.len();
            for (idx, r) in lista.iter().enumerate() {
                let row = gtk::Box::builder()
                    .orientation(gtk::Orientation::Horizontal)
                    .spacing(10)
                    .margin_start(12)
                    .margin_end(12)
                    .margin_top(8)
                    .margin_bottom(8)
                    .build();
                let ruta_svg = svgs
                    .borrow()
                    .iter()
                    .find(|s| s.nombre == r.icono)
                    .map(|s| s.ruta.clone());
                if let Some(img) = imagen(&r.icono, r.color.as_deref(), 18, ruta_svg.as_deref()) {
                    row.append(&img);
                }
                let texto = gtk::Label::builder()
                    .label(format!(
                        "{}: {}  →  :{}:",
                        r.tipo.etiqueta_ui(),
                        r.valor,
                        r.icono
                    ))
                    .xalign(0.0)
                    .hexpand(true)
                    .ellipsize(gtk::pango::EllipsizeMode::End)
                    .build();
                row.append(&texto);

                let btn_up = gtk::Button::builder()
                    .icon_name("go-up-symbolic")
                    .css_classes(["flat"])
                    .sensitive(idx > 0)
                    .tooltip_text("Subir prioridad")
                    .build();
                btn_up.update_property(&[gtk::accessible::Property::Label("Subir regla")]);
                {
                    let reglas = reglas.clone();
                    let al_cambiar = al_cambiar.clone();
                    let reconstruir = reconstruir.clone();
                    btn_up.connect_clicked(move |_| {
                        let mut v = reglas.borrow_mut();
                        if idx > 0 && idx < v.len() {
                            v.swap(idx, idx - 1);
                            let copia = v.clone();
                            drop(v);
                            al_cambiar(copia);
                            if let Some(f) = reconstruir.borrow().as_ref() {
                                f();
                            }
                        }
                    });
                }

                let btn_down = gtk::Button::builder()
                    .icon_name("go-down-symbolic")
                    .css_classes(["flat"])
                    .sensitive(idx + 1 < total)
                    .tooltip_text("Bajar prioridad")
                    .build();
                btn_down.update_property(&[gtk::accessible::Property::Label("Bajar regla")]);
                {
                    let reglas = reglas.clone();
                    let al_cambiar = al_cambiar.clone();
                    let reconstruir = reconstruir.clone();
                    btn_down.connect_clicked(move |_| {
                        let mut v = reglas.borrow_mut();
                        if idx + 1 < v.len() {
                            v.swap(idx, idx + 1);
                            let copia = v.clone();
                            drop(v);
                            al_cambiar(copia);
                            if let Some(f) = reconstruir.borrow().as_ref() {
                                f();
                            }
                        }
                    });
                }

                let btn_del = gtk::Button::builder()
                    .icon_name("user-trash-symbolic")
                    .css_classes(["flat"])
                    .tooltip_text("Borrar regla")
                    .build();
                btn_del.update_property(&[gtk::accessible::Property::Label("Borrar regla")]);
                {
                    let reglas = reglas.clone();
                    let al_cambiar = al_cambiar.clone();
                    let reconstruir = reconstruir.clone();
                    btn_del.connect_clicked(move |_| {
                        let mut v = reglas.borrow_mut();
                        if idx < v.len() {
                            v.remove(idx);
                            let copia = v.clone();
                            drop(v);
                            al_cambiar(copia);
                            if let Some(f) = reconstruir.borrow().as_ref() {
                                f();
                            }
                        }
                    });
                }

                row.append(&btn_up);
                row.append(&btn_down);
                row.append(&btn_del);
                list_box.append(&row);
            }
        })
    };
    *reconstruir.borrow_mut() = Some(reconstruir_fn.clone());
    reconstruir_fn();

    // Elegir ícono para la nueva regla.
    {
        let d_parent = dialog.clone();
        let nuevo_icono = nuevo_icono.clone();
        let nuevo_color = nuevo_color.clone();
        let svgs = svgs.clone();
        let raiz_boveda = raiz_boveda.clone();
        let pintar = pintar_preview_nueva.clone();
        let al_importar = al_importar.clone();
        btn_elegir_icono.connect_clicked(move |_| {
            let opts = OpcionesSelector {
                titulo: "Ícono para la regla".into(),
                icono_actual: Some(nuevo_icono.borrow().clone()),
                color_actual: nuevo_color.borrow().clone(),
                heredar_actual: false,
                es_carpeta: false,
                mostrar_quitar: false,
                svgs_locales: svgs.borrow().clone(),
                raiz_boveda: raiz_boveda.clone(),
            };
            let ni = nuevo_icono.clone();
            let nc = nuevo_color.clone();
            let pintar = pintar.clone();
            let svgs_imp = svgs.clone();
            let al_imp = al_importar.clone();
            mostrar_selector(
                &d_parent,
                opts,
                move |acc| {
                    if let AccionSelector::Aplicar { icono, color, .. } = acc {
                        *ni.borrow_mut() = icono;
                        *nc.borrow_mut() = color;
                        pintar();
                    }
                },
                move |svg| {
                    svgs_imp.borrow_mut().push(svg.clone());
                    al_imp(svg);
                },
            );
        });
    }

    // Agregar regla.
    {
        let reglas = reglas.clone();
        let tipo_dropdown = tipo_dropdown.clone();
        let valor_entry = valor_entry.clone();
        let nuevo_icono = nuevo_icono.clone();
        let nuevo_color = nuevo_color.clone();
        let al_cambiar = al_cambiar.clone();
        let reconstruir_fn = reconstruir_fn.clone();
        btn_agregar.connect_clicked(move |_| {
            let val = valor_entry.text().trim().to_owned();
            if val.is_empty() {
                return;
            }
            let tipo = tipos
                .get(tipo_dropdown.selected() as usize)
                .copied()
                .unwrap_or(TipoRegla::Carpeta);
            let regla = ReglaIcono {
                tipo,
                valor: val,
                icono: nuevo_icono.borrow().clone(),
                color: nuevo_color.borrow().clone(),
            };
            let copia = {
                let mut v = reglas.borrow_mut();
                v.push(regla);
                v.clone()
            };
            valor_entry.set_text("");
            al_cambiar(copia);
            reconstruir_fn();
        });
    }

    dialog.present(Some(parent));
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Todos los trazos del catálogo embebido son válidos para `gsk::Path::parse`.
    #[test]
    fn catalogo_trazos_validos() {
        assert!(iconos_catalogo::CATALOGO.len() >= 60);
        for ic in iconos_catalogo::CATALOGO {
            assert!(
                gsk::Path::parse(ic.trazo).is_ok(),
                "trazo inválido en «{}»: {}",
                ic.nombre,
                ic.trazo
            );
        }
    }
}
