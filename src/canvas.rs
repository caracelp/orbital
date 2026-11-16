//! Spec 038: Editor de Canvas (`.canvas`) y activación del botón «Crear canva» del riel.
//!
//! Soporta tarjetas de texto editable (`type: text`), tarjetas de nota de la bóveda (`type: file`,
//! con clic para abrir la nota), colores por tarjeta y conexiones/flechas dirigidas (`edges`) con
//! etiqueta opcional. Se lee con `Vault::read_aux` y se escribe con `Vault::save_aux` (`format_version: 1`).
//! Si el archivo `.canvas` tiene errores al abrirse, no se sobrescribe sin confirmación explícita.

use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::rc::{Rc, Weak};

use adw::prelude::*;
use gtk::subclass::prelude::*;
use gtk::{glib, gsk};
use orbital_vault::props::emit::scalar;
use orbital_vault::props::{PropValue, yaml};
use orbital_vault::{AuxFile, RelPath};

/// Color de una tarjeta del lienzo (paleta rosa y negro con contraste WCAG AA).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ColorCanvas {
    #[default]
    Rosa,
    Violeta,
    Cian,
    Verde,
    Ambar,
    Neutro,
}

impl ColorCanvas {
    pub const TODOS: [Self; 6] = [
        Self::Rosa,
        Self::Violeta,
        Self::Cian,
        Self::Verde,
        Self::Ambar,
        Self::Neutro,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Rosa => "rosa",
            Self::Violeta => "violeta",
            Self::Cian => "cian",
            Self::Verde => "verde",
            Self::Ambar => "ambar",
            Self::Neutro => "neutro",
        }
    }

    pub fn nombre_visible(self) -> &'static str {
        match self {
            Self::Rosa => "Rosa",
            Self::Violeta => "Violeta",
            Self::Cian => "Cian",
            Self::Verde => "Verde",
            Self::Ambar => "Ámbar",
            Self::Neutro => "Neutro",
        }
    }

    pub fn parse(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().as_str() {
            "rosa" | "pink" | "1" => Self::Rosa,
            "violeta" | "purple" | "6" => Self::Violeta,
            "cian" | "cyan" | "5" => Self::Cian,
            "verde" | "green" | "4" => Self::Verde,
            "ambar" | "amber" | "yellow" | "3" => Self::Ambar,
            "neutro" | "neutral" | "gray" | "0" => Self::Neutro,
            _ => Self::Rosa,
        }
    }

    /// Color de acento/borde y color de fondo oscuro de la tarjeta.
    fn rgba_acento_y_fondo(self) -> (gtk::gdk::RGBA, gtk::gdk::RGBA) {
        let (acento, fondo) = match self {
            Self::Rosa => ("#ff8cc6", "#2b1924"),
            Self::Violeta => ("#c792ea", "#231b2e"),
            Self::Cian => ("#89ddff", "#17262e"),
            Self::Verde => ("#a3e635", "#1c2819"),
            Self::Ambar => ("#fbbf24", "#2a2215"),
            Self::Neutro => ("#a89ca4", "#211c20"),
        };
        (
            gtk::gdk::RGBA::parse(acento).expect("color de acento válido"),
            gtk::gdk::RGBA::parse(fondo).expect("color de fondo válido"),
        )
    }
}

/// Clase de tarjeta del lienzo: texto libre o referencia a una nota/archivo de la bóveda.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TipoNodoCanvas {
    Texto(String),
    Nota(String),
}

/// Tarjeta posicionada en el lienzo.
#[derive(Clone, Debug, PartialEq)]
pub struct NodoCanvas {
    pub id: String,
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    pub color: ColorCanvas,
    pub tipo: TipoNodoCanvas,
}

impl NodoCanvas {
    pub fn titulo_corto(&self) -> String {
        match &self.tipo {
            TipoNodoCanvas::Texto(t) => {
                let primera = t.lines().next().unwrap_or("").trim();
                if primera.is_empty() {
                    format!("Texto ({})", self.id)
                } else {
                    primera.chars().take(28).collect()
                }
            }
            TipoNodoCanvas::Nota(ruta) => {
                let arch = ruta.rsplit_once('/').map_or(ruta.as_str(), |(_, f)| f);
                format!("📄 {arch}")
            }
        }
    }
}

/// Conexión dirigida (`from_node -> to_node`) entre dos tarjetas del lienzo.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AristaCanvas {
    pub id: String,
    pub from_node: String,
    pub to_node: String,
    pub label: String,
}

/// Modelo completo de un archivo `.canvas`.
#[derive(Clone, Debug, PartialEq)]
pub struct DocumentoCanvas {
    pub format_version: u32,
    pub nodes: Vec<NodoCanvas>,
    pub edges: Vec<AristaCanvas>,
}

impl Default for DocumentoCanvas {
    fn default() -> Self {
        Self {
            format_version: 1,
            nodes: Vec::new(),
            edges: Vec::new(),
        }
    }
}

fn val_str(v: &PropValue) -> Option<String> {
    match v {
        PropValue::Text(s)
        | PropValue::Link(s)
        | PropValue::Number(s)
        | PropValue::Date(s)
        | PropValue::DateTime(s) => Some(s.clone()),
        PropValue::Bool(b) => Some(b.to_string()),
        PropValue::Null => Some(String::new()),
        _ => None,
    }
}

fn val_f64(v: &PropValue) -> Option<f64> {
    match v {
        PropValue::Number(n) | PropValue::Text(n) => n.trim().parse::<f64>().ok(),
        _ => None,
    }
}

fn map_get<'a>(mapa: &'a [(String, PropValue)], clave: &str) -> Option<&'a PropValue> {
    mapa.iter().find(|(k, _)| k == clave).map(|(_, v)| v)
}

impl DocumentoCanvas {
    /// Analiza el texto YAML o JSON de un `.canvas`. Exige `format_version: 1`.
    pub fn parse(text: &str) -> Result<Self, String> {
        let entries =
            yaml::parse_block(text).map_err(|e| format!("Línea {}: {}", e.line, e.message))?;
        let mut version: Option<u32> = None;
        let mut nodes: Vec<NodoCanvas> = Vec::new();
        let mut edges: Vec<AristaCanvas> = Vec::new();

        for entry in &entries {
            match entry.key.as_str() {
                "format_version" => {
                    let n = match &entry.value {
                        PropValue::Number(s) => s.parse::<u32>().ok(),
                        _ => None,
                    }
                    .ok_or_else(|| {
                        "«format_version» tiene que ser un número entero positivo".to_owned()
                    })?;
                    if n != 1 {
                        return Err(format!(
                            "Versión de formato no soportada («format_version: {n}»); esta versión de Orbital soporta la 1"
                        ));
                    }
                    version = Some(n);
                }
                "nodes" => match &entry.value {
                    PropValue::Null => {}
                    PropValue::List(lista) => {
                        let mut ids = HashSet::new();
                        for item in lista {
                            let PropValue::Map(m) = item else {
                                return Err("Cada elemento de «nodes» debe ser un mapa".into());
                            };
                            let id = map_get(m, "id")
                                .and_then(val_str)
                                .filter(|s| !s.trim().is_empty())
                                .ok_or_else(|| "Un nodo de «nodes» no tiene «id»".to_owned())?;
                            if !ids.insert(id.clone()) {
                                return Err(format!("El id de nodo «{id}» está repetido"));
                            }
                            let tipo_str = map_get(m, "type")
                                .and_then(val_str)
                                .unwrap_or_else(|| "text".into());
                            let x = map_get(m, "x").and_then(val_f64).unwrap_or(0.0);
                            let y = map_get(m, "y").and_then(val_f64).unwrap_or(0.0);
                            let width = map_get(m, "width")
                                .and_then(val_f64)
                                .unwrap_or(240.0)
                                .clamp(100.0, 1200.0);
                            let height = map_get(m, "height")
                                .and_then(val_f64)
                                .unwrap_or(130.0)
                                .clamp(64.0, 900.0);
                            let color = map_get(m, "color")
                                .and_then(val_str)
                                .map(|c| ColorCanvas::parse(&c))
                                .unwrap_or_default();
                            let tipo = match tipo_str.as_str() {
                                "file" | "nota" => {
                                    let file = map_get(m, "file")
                                        .and_then(val_str)
                                        .filter(|s| !s.trim().is_empty())
                                        .ok_or_else(|| {
                                            format!(
                                                "El nodo «{id}» de tipo «file» requiere el campo «file»"
                                            )
                                        })?;
                                    TipoNodoCanvas::Nota(file)
                                }
                                "text" | "texto" => {
                                    let texto =
                                        map_get(m, "text").and_then(val_str).unwrap_or_default();
                                    TipoNodoCanvas::Texto(texto)
                                }
                                otro => {
                                    return Err(format!(
                                        "Tipo de nodo desconocido «{otro}» en «{id}» (se admite text o file)"
                                    ));
                                }
                            };
                            nodes.push(NodoCanvas {
                                id,
                                x,
                                y,
                                width,
                                height,
                                color,
                                tipo,
                            });
                        }
                    }
                    _ => return Err("«nodes» tiene que ser una lista".into()),
                },
                "edges" => match &entry.value {
                    PropValue::Null => {}
                    PropValue::List(lista) => {
                        for item in lista {
                            let PropValue::Map(m) = item else {
                                return Err("Cada elemento de «edges» debe ser un mapa".into());
                            };
                            let id = map_get(m, "id")
                                .and_then(val_str)
                                .filter(|s| !s.trim().is_empty())
                                .unwrap_or_else(|| format!("e{}", edges.len() + 1));
                            let from_node = map_get(m, "fromNode")
                                .or_else(|| map_get(m, "from"))
                                .and_then(val_str)
                                .filter(|s| !s.trim().is_empty())
                                .ok_or_else(|| format!("La conexión «{id}» requiere «fromNode»"))?;
                            let to_node = map_get(m, "toNode")
                                .or_else(|| map_get(m, "to"))
                                .and_then(val_str)
                                .filter(|s| !s.trim().is_empty())
                                .ok_or_else(|| format!("La conexión «{id}» requiere «toNode»"))?;
                            let label = map_get(m, "label").and_then(val_str).unwrap_or_default();
                            edges.push(AristaCanvas {
                                id,
                                from_node,
                                to_node,
                                label,
                            });
                        }
                    }
                    _ => return Err("«edges» tiene que ser una lista".into()),
                },
                otra => {
                    return Err(format!("Clave desconocida «{otra}» en el archivo .canvas"));
                }
            }
        }

        let Some(format_version) = version else {
            return Err("Falta «format_version: 1» en el archivo .canvas".into());
        };
        Ok(Self {
            format_version,
            nodes,
            edges,
        })
    }

    /// Serializa el lienzo en YAML determinista compatible con `Vault::read_aux` y `Vault::save_aux`.
    pub fn serialize(&self) -> String {
        let mut out = String::with_capacity(128 + self.nodes.len() * 140 + self.edges.len() * 80);
        out.push_str(&format!("format_version: {}\n", self.format_version));
        if self.nodes.is_empty() {
            out.push_str("nodes: []\n");
        } else {
            out.push_str("nodes:\n");
            for n in &self.nodes {
                out.push_str(&format!("  - id: {}\n", scalar(&n.id)));
                match &n.tipo {
                    TipoNodoCanvas::Texto(t) => {
                        out.push_str("    type: text\n");
                        out.push_str(&format!("    x: {}\n", n.x.round() as i64));
                        out.push_str(&format!("    y: {}\n", n.y.round() as i64));
                        out.push_str(&format!("    width: {}\n", n.width.round() as i64));
                        out.push_str(&format!("    height: {}\n", n.height.round() as i64));
                        out.push_str(&format!("    color: {}\n", n.color.as_str()));
                        out.push_str(&format!("    text: {}\n", scalar(t)));
                    }
                    TipoNodoCanvas::Nota(ruta) => {
                        out.push_str("    type: file\n");
                        out.push_str(&format!("    x: {}\n", n.x.round() as i64));
                        out.push_str(&format!("    y: {}\n", n.y.round() as i64));
                        out.push_str(&format!("    width: {}\n", n.width.round() as i64));
                        out.push_str(&format!("    height: {}\n", n.height.round() as i64));
                        out.push_str(&format!("    color: {}\n", n.color.as_str()));
                        out.push_str(&format!("    file: {}\n", scalar(ruta)));
                    }
                }
            }
        }
        if self.edges.is_empty() {
            out.push_str("edges: []\n");
        } else {
            out.push_str("edges:\n");
            for e in &self.edges {
                out.push_str(&format!("  - id: {}\n", scalar(&e.id)));
                out.push_str(&format!("    fromNode: {}\n", scalar(&e.from_node)));
                out.push_str(&format!("    toNode: {}\n", scalar(&e.to_node)));
                if !e.label.is_empty() {
                    out.push_str(&format!("    label: {}\n", scalar(&e.label)));
                }
            }
        }
        out
    }

    /// Genera un identificador `n1`, `n2`… libre para un nodo nuevo.
    pub fn nuevo_id_nodo(&self) -> String {
        let mut k = self.nodes.len() + 1;
        loop {
            let cand = format!("n{k}");
            if !self.nodes.iter().any(|n| n.id == cand) {
                return cand;
            }
            k += 1;
        }
    }

    /// Genera un identificador `e1`, `e2`… libre para una conexión nueva.
    pub fn nuevo_id_arista(&self) -> String {
        let mut k = self.edges.len() + 1;
        loop {
            let cand = format!("e{k}");
            if !self.edges.iter().any(|e| e.id == cand) {
                return cand;
            }
            k += 1;
        }
    }

    /// Elimina el nodo en el índice `idx` y todas las conexiones incidentes.
    pub fn eliminar_nodo(&mut self, idx: usize) {
        if idx < self.nodes.len() {
            let id = self.nodes.remove(idx).id;
            self.edges.retain(|e| e.from_node != id && e.to_node != id);
        }
    }
}

#[derive(Clone, Copy)]
struct VistaTransform {
    pan_x: f64,
    pan_y: f64,
    zoom: f64,
}

impl Default for VistaTransform {
    fn default() -> Self {
        Self {
            pan_x: 0.0,
            pan_y: 0.0,
            zoom: 1.0,
        }
    }
}

#[derive(Default)]
enum ArrastreLienzo {
    #[default]
    Ninguno,
    Fondo(f64, f64, f64, f64),
    Nodo(usize, f64, f64),
}

type SeleccionFn = Box<dyn Fn(Option<usize>)>;
type CambioFn = Box<dyn Fn()>;
type AbrirNotaFn = Box<dyn Fn(&str)>;

mod imp {
    use super::*;

    #[derive(Default)]
    pub struct LienzoCanvas {
        pub doc: RefCell<DocumentoCanvas>,
        pub(super) vista: Cell<VistaTransform>,
        pub seleccionado: Cell<Option<usize>>,
        pub(super) arrastre: RefCell<ArrastreLienzo>,
        pub al_seleccionar: RefCell<Option<SeleccionFn>>,
        pub al_cambiar: RefCell<Option<CambioFn>>,
        pub al_abrir_nota: RefCell<Option<AbrirNotaFn>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for LienzoCanvas {
        const NAME: &'static str = "OrbitalLienzoCanvas";
        type Type = super::LienzoCanvas;
        type ParentType = gtk::Widget;
    }

    impl ObjectImpl for LienzoCanvas {}

    impl WidgetImpl for LienzoCanvas {
        fn snapshot(&self, snapshot: &gtk::Snapshot) {
            let width = self.obj().width() as f64;
            let height = self.obj().height() as f64;
            if width <= 0.0 || height <= 0.0 {
                return;
            }
            let v = self.vista.get();
            let doc = self.doc.borrow();
            let sel = self.seleccionado.get();
            let widget = self.obj();

            let color_flecha = gtk::gdk::RGBA::parse("#b89eb0").expect("color flecha");
            let color_rosa = gtk::gdk::RGBA::parse("#ff8cc6").expect("rosa");
            let color_texto = gtk::gdk::RGBA::parse("#f3edf1").expect("texto");
            let color_sub = gtk::gdk::RGBA::parse("#b5a7b0").expect("subtexto");

            let a_pantalla = |wx: f64, wy: f64| (wx * v.zoom - v.pan_x, wy * v.zoom - v.pan_y);

            // 1. Conexiones dirigidas (flechas con punta y rótulo opcional).
            let path_lineas = gsk::PathBuilder::new();
            let path_puntas = gsk::PathBuilder::new();
            let mut hay_aristas = false;
            let mut etiquetas_aristas: Vec<(f64, f64, &str)> = Vec::new();

            for edge in &doc.edges {
                let Some(na) = doc.nodes.iter().find(|n| n.id == edge.from_node) else {
                    continue;
                };
                let Some(nb) = doc.nodes.iter().find(|n| n.id == edge.to_node) else {
                    continue;
                };
                let (ax, ay) = (na.x + na.width * 0.5, na.y + na.height * 0.5);
                let (bx, by) = (nb.x + nb.width * 0.5, nb.y + nb.height * 0.5);
                let (sx0, sy0) = a_pantalla(ax, ay);
                let (sx1, sy1) = a_pantalla(bx, by);
                if (sx0 < 0.0 && sx1 < 0.0)
                    || (sy0 < 0.0 && sy1 < 0.0)
                    || (sx0 > width && sx1 > width)
                    || (sy0 > height && sy1 > height)
                {
                    continue;
                }
                let dx = sx1 - sx0;
                let dy = sy1 - sy0;
                let dist = (dx * dx + dy * dy).sqrt();
                if dist < 4.0 {
                    continue;
                }
                let (ux, uy) = (dx / dist, dy / dist);
                // Recortar el extremo final al borde aproximado de la tarjeta destino.
                let recorte = ((nb.width.min(nb.height) * 0.45) * v.zoom).min(dist * 0.45);
                let (tx, ty) = (sx1 - ux * recorte, sy1 - uy * recorte);

                path_lineas.move_to(sx0 as f32, sy0 as f32);
                path_lineas.line_to(tx as f32, ty as f32);

                // Punta de flecha triangular.
                let largo_punta = (10.0 * v.zoom).clamp(6.0, 14.0);
                let ancho_punta = largo_punta * 0.55;
                let (px_base, py_base) = (tx - ux * largo_punta, ty - uy * largo_punta);
                let (nx, ny) = (-uy * ancho_punta, ux * ancho_punta);
                path_puntas.move_to(tx as f32, ty as f32);
                path_puntas.line_to((px_base + nx) as f32, (py_base + ny) as f32);
                path_puntas.line_to((px_base - nx) as f32, (py_base - ny) as f32);
                path_puntas.close();
                hay_aristas = true;

                if !edge.label.is_empty() && v.zoom >= 0.4 {
                    etiquetas_aristas.push(((sx0 + tx) * 0.5, (sy0 + ty) * 0.5, &edge.label));
                }
            }
            if hay_aristas {
                let trazo_arista = gsk::Stroke::new((2.0 * v.zoom as f32).clamp(1.2, 3.0));
                snapshot.append_stroke(&path_lineas.to_path(), &trazo_arista, &color_flecha);
                snapshot.append_fill(
                    &path_puntas.to_path(),
                    gsk::FillRule::Winding,
                    &color_flecha,
                );
            }
            for (mx, my, rotulo) in etiquetas_aristas {
                let layout = widget.create_pango_layout(Some(rotulo));
                let mut f = gtk::pango::FontDescription::new();
                f.set_size(((9.5 * v.zoom).clamp(7.0, 14.0) * f64::from(gtk::pango::SCALE)) as i32);
                layout.set_font_description(Some(&f));
                let (lw, lh) = layout.pixel_size();
                snapshot.save();
                snapshot.translate(&gtk::graphene::Point::new(
                    (mx - f64::from(lw) * 0.5) as f32,
                    (my - f64::from(lh) - 4.0) as f32,
                ));
                snapshot.append_layout(&layout, &color_rosa);
                snapshot.restore();
            }

            // 2. Tarjetas (nodos).
            let radio_esquina = (10.0 * v.zoom) as f32;
            for (idx, nodo) in doc.nodes.iter().enumerate() {
                let (x, y) = a_pantalla(nodo.x, nodo.y);
                let (w, h) = (nodo.width * v.zoom, nodo.height * v.zoom);
                if x + w < 0.0 || y + h < 0.0 || x > width || y > height {
                    continue;
                }
                let (acento, fondo) = nodo.color.rgba_acento_y_fondo();
                let rect = gtk::graphene::Rect::new(x as f32, y as f32, w as f32, h as f32);
                let rrect = gsk::RoundedRect::from_rect(rect, radio_esquina);

                let builder_caja = gsk::PathBuilder::new();
                builder_caja.add_rounded_rect(&rrect);
                let path_caja = builder_caja.to_path();
                snapshot.append_fill(&path_caja, gsk::FillRule::Winding, &fondo);

                // Franja superior de color.
                let alto_franja = (6.0 * v.zoom).clamp(3.0, 10.0) as f32;
                snapshot.push_rounded_clip(&rrect);
                snapshot.append_color(
                    &acento,
                    &gtk::graphene::Rect::new(x as f32, y as f32, w as f32, alto_franja),
                );

                // Contenido textual de la tarjeta.
                if v.zoom >= 0.3 {
                    let pad = (12.0 * v.zoom).max(4.0);
                    let ancho_txt =
                        ((w - pad * 2.0).max(20.0) * f64::from(gtk::pango::SCALE)) as i32;
                    match &nodo.tipo {
                        TipoNodoCanvas::Texto(texto) => {
                            let mostrar = if texto.trim().is_empty() {
                                "(Tarjeta vacía)"
                            } else {
                                texto.as_str()
                            };
                            let layout = widget.create_pango_layout(Some(mostrar));
                            layout.set_width(ancho_txt);
                            layout.set_wrap(gtk::pango::WrapMode::WordChar);
                            let mut f = gtk::pango::FontDescription::new();
                            f.set_size(
                                ((10.5 * v.zoom).clamp(6.0, 18.0) * f64::from(gtk::pango::SCALE))
                                    as i32,
                            );
                            layout.set_font_description(Some(&f));
                            snapshot.save();
                            snapshot.translate(&gtk::graphene::Point::new(
                                (x + pad) as f32,
                                (y + f64::from(alto_franja) + pad * 0.8) as f32,
                            ));
                            snapshot.append_layout(&layout, &color_texto);
                            snapshot.restore();
                        }
                        TipoNodoCanvas::Nota(ruta) => {
                            let arch = ruta.rsplit_once('/').map_or(ruta.as_str(), |(_, f)| f);
                            let titulo = arch.rsplit_once('.').map_or(arch, |(s, _)| s);
                            let layout_t =
                                widget.create_pango_layout(Some(&format!("📄 {titulo}")));
                            layout_t.set_width(ancho_txt);
                            layout_t.set_ellipsize(gtk::pango::EllipsizeMode::End);
                            let mut ft = gtk::pango::FontDescription::new();
                            ft.set_weight(gtk::pango::Weight::Bold);
                            ft.set_size(
                                ((11.0 * v.zoom).clamp(6.0, 18.0) * f64::from(gtk::pango::SCALE))
                                    as i32,
                            );
                            layout_t.set_font_description(Some(&ft));
                            let (_, th) = layout_t.pixel_size();
                            snapshot.save();
                            snapshot.translate(&gtk::graphene::Point::new(
                                (x + pad) as f32,
                                (y + f64::from(alto_franja) + pad * 0.8) as f32,
                            ));
                            snapshot.append_layout(&layout_t, &color_texto);
                            snapshot.restore();

                            let layout_r = widget.create_pango_layout(Some(ruta));
                            layout_r.set_width(ancho_txt);
                            layout_r.set_ellipsize(gtk::pango::EllipsizeMode::Middle);
                            let mut fr = gtk::pango::FontDescription::new();
                            fr.set_size(
                                ((9.0 * v.zoom).clamp(5.0, 15.0) * f64::from(gtk::pango::SCALE))
                                    as i32,
                            );
                            layout_r.set_font_description(Some(&fr));
                            snapshot.save();
                            snapshot.translate(&gtk::graphene::Point::new(
                                (x + pad) as f32,
                                (y + f64::from(alto_franja) + pad * 0.8 + f64::from(th) + 6.0)
                                    as f32,
                            ));
                            snapshot.append_layout(&layout_r, &color_sub);
                            snapshot.restore();
                        }
                    }
                }
                snapshot.pop();

                // Borde de la tarjeta (más grueso y rosa brillante si está seleccionada).
                let es_sel = sel == Some(idx);
                let grosor = if es_sel { 2.6 } else { 1.3 };
                let color_borde = if es_sel { &color_rosa } else { &acento };
                snapshot.append_stroke(&path_caja, &gsk::Stroke::new(grosor), color_borde);
            }
        }
    }
}

glib::wrapper! {
    pub struct LienzoCanvas(ObjectSubclass<imp::LienzoCanvas>)
        @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl LienzoCanvas {
    pub fn new() -> Self {
        let this: Self = glib::Object::builder().build();
        this.set_can_focus(true);
        this.set_focusable(true);
        this.set_vexpand(true);
        this.set_hexpand(true);
        this.update_property(&[gtk::accessible::Property::Label(
            "Lienzo de Canvas: tarjetas de texto, notas y conexiones",
        )]);
        this.setup_gestos();
        this
    }

    pub fn nodo_en(&self, px: f64, py: f64) -> Option<usize> {
        let v = self.imp().vista.get();
        let wx = (px + v.pan_x) / v.zoom;
        let wy = (py + v.pan_y) / v.zoom;
        let doc = self.imp().doc.borrow();
        // Recorrer de atrás hacia adelante (la última dibujada está arriba).
        for (idx, n) in doc.nodes.iter().enumerate().rev() {
            if wx >= n.x && wx <= n.x + n.width && wy >= n.y && wy <= n.y + n.height {
                return Some(idx);
            }
        }
        None
    }

    pub fn seleccionar(&self, idx: Option<usize>) {
        if self.imp().seleccionado.get() != idx {
            self.imp().seleccionado.set(idx);
            if let Some(f) = self.imp().al_seleccionar.borrow().as_ref() {
                f(idx);
            }
            self.queue_draw();
        }
    }

    pub fn centrar_vista(&self) {
        self.imp().vista.set(VistaTransform::default());
        self.queue_draw();
    }

    fn notificar_cambio(&self) {
        if let Some(f) = self.imp().al_cambiar.borrow().as_ref() {
            f();
        }
    }

    fn setup_gestos(&self) {
        // Rueda: zoom centrado en el puntero.
        let scroll = gtk::EventControllerScroll::new(gtk::EventControllerScrollFlags::VERTICAL);
        let this = self.clone();
        let ctl_scroll = scroll.clone();
        scroll.connect_scroll(move |_, _dx, dy| {
            let imp = this.imp();
            let mut v = imp.vista.get();
            let factor = if dy > 0.0 { 0.9 } else { 1.1 };
            let nuevo = (v.zoom * factor).clamp(0.2, 4.0);
            if let Some(e) = ctl_scroll.current_event()
                && let Some((mx, my)) = e.position()
            {
                let wx = (mx + v.pan_x) / v.zoom;
                let wy = (my + v.pan_y) / v.zoom;
                v.pan_x = wx * nuevo - mx;
                v.pan_y = wy * nuevo - my;
            }
            v.zoom = nuevo;
            imp.vista.set(v);
            this.queue_draw();
            glib::Propagation::Stop
        });
        self.add_controller(scroll);

        // Arrastre: tarjeta o fondo.
        let drag = gtk::GestureDrag::new();
        drag.set_button(1);
        let this = self.clone();
        drag.connect_drag_begin(move |_, x, y| {
            let imp = this.imp();
            if let Some(idx) = this.nodo_en(x, y) {
                this.seleccionar(Some(idx));
                let (nx, ny) = {
                    let doc = imp.doc.borrow();
                    (doc.nodes[idx].x, doc.nodes[idx].y)
                };
                imp.arrastre.replace(ArrastreLienzo::Nodo(idx, nx, ny));
            } else {
                this.seleccionar(None);
                let v = imp.vista.get();
                imp.arrastre
                    .replace(ArrastreLienzo::Fondo(x, y, v.pan_x, v.pan_y));
            }
        });
        let this = self.clone();
        drag.connect_drag_update(move |_, dx, dy| {
            let imp = this.imp();
            match *imp.arrastre.borrow() {
                ArrastreLienzo::Fondo(_sx, _sy, px, py) => {
                    let mut v = imp.vista.get();
                    v.pan_x = px - dx;
                    v.pan_y = py - dy;
                    imp.vista.set(v);
                    this.queue_draw();
                }
                ArrastreLienzo::Nodo(idx, nx0, ny0) => {
                    let zoom = imp.vista.get().zoom;
                    let mut doc = imp.doc.borrow_mut();
                    if let Some(n) = doc.nodes.get_mut(idx) {
                        n.x = (nx0 + dx / zoom).round();
                        n.y = (ny0 + dy / zoom).round();
                    }
                    drop(doc);
                    this.queue_draw();
                }
                ArrastreLienzo::Ninguno => {}
            }
        });
        let this = self.clone();
        drag.connect_drag_end(move |_, dx, dy| {
            let movio_nodo = matches!(
                *this.imp().arrastre.borrow(),
                ArrastreLienzo::Nodo(..) if dx.abs() + dy.abs() >= 2.0
            );
            this.imp().arrastre.replace(ArrastreLienzo::Ninguno);
            if movio_nodo {
                this.notificar_cambio();
            }
        });
        self.add_controller(drag);

        // Doble clic sobre una tarjeta de nota: abrir la nota.
        let click = gtk::GestureClick::new();
        click.set_button(1);
        let this = self.clone();
        click.connect_released(move |_, n_press, x, y| {
            if n_press == 2
                && let Some(idx) = this.nodo_en(x, y)
            {
                let ruta = {
                    let doc = this.imp().doc.borrow();
                    doc.nodes.get(idx).and_then(|n| match &n.tipo {
                        TipoNodoCanvas::Nota(r) => Some(r.clone()),
                        TipoNodoCanvas::Texto(_) => None,
                    })
                };
                if let Some(r) = ruta
                    && let Some(f) = this.imp().al_abrir_nota.borrow().as_ref()
                {
                    f(&r);
                }
            }
        });
        self.add_controller(click);
    }
}

type GuardarCanvasFn = Box<dyn Fn(AuxFile, String, bool)>;

struct Inner {
    root: gtk::Box,
    stack: gtk::Stack,
    lienzo: LienzoCanvas,
    barra_contextual: gtk::Box,
    entrada_texto: gtk::Entry,
    boton_abrir_nota: gtk::Button,
    nota_sel_ruta: RefCell<Option<String>>,
    actualizando_ui: Cell<bool>,
    aux_actual: RefCell<Option<AuxFile>>,
    error_banner: adw::Banner,
    error_fuente: gtk::TextView,
    al_guardar: RefCell<Option<GuardarCanvasFn>>,
    al_abrir_nota: RefCell<Option<AbrirNotaFn>>,
    al_pedir_nota: RefCell<Option<Box<dyn Fn()>>>,
}

#[derive(Clone)]
pub struct VistaCanvas(Rc<Inner>);

thread_local! {
    static INSTANCIAS: RefCell<Vec<(gtk::Box, Weak<Inner>)>> = const { RefCell::new(Vec::new()) };
}

impl VistaCanvas {
    pub fn new() -> Self {
        let b_add_txt = gtk::Button::builder()
            .label("+ Texto")
            .tooltip_text("Agregar tarjeta de texto al lienzo")
            .css_classes(["flat"])
            .build();
        b_add_txt.update_property(&[gtk::accessible::Property::Label("Agregar tarjeta de texto")]);

        let b_add_nota = gtk::Button::builder()
            .label("+ Nota…")
            .tooltip_text("Agregar tarjeta vinculada a una nota de la bóveda")
            .css_classes(["flat"])
            .build();
        b_add_nota.update_property(&[gtk::accessible::Property::Label("Agregar tarjeta de nota")]);

        let b_conectar = gtk::Button::builder()
            .label("+ Conectar…")
            .tooltip_text("Conectar dos tarjetas con una flecha")
            .css_classes(["flat"])
            .build();
        b_conectar.update_property(&[gtk::accessible::Property::Label("Conectar dos tarjetas")]);

        let b_centrar = gtk::Button::builder()
            .icon_name("zoom-fit-best-symbolic")
            .tooltip_text("Restablecer vista del lienzo")
            .css_classes(["flat"])
            .build();
        b_centrar.update_property(&[gtk::accessible::Property::Label(
            "Restablecer vista del lienzo",
        )]);

        let barra_herramientas = adw::HeaderBar::builder()
            .show_start_title_buttons(false)
            .show_end_title_buttons(false)
            .css_classes(["flat"])
            .build();
        barra_herramientas.pack_start(&b_add_txt);
        barra_herramientas.pack_start(&b_add_nota);
        barra_herramientas.pack_start(&b_conectar);
        barra_herramientas.pack_end(&b_centrar);

        // Barra contextual para editar la tarjeta seleccionada.
        let entrada_texto = gtk::Entry::builder()
            .placeholder_text("Texto de la tarjeta…")
            .hexpand(true)
            .build();
        entrada_texto.update_property(&[gtk::accessible::Property::Label(
            "Texto de la tarjeta seleccionada",
        )]);

        let boton_abrir_nota = gtk::Button::builder()
            .label("Abrir nota")
            .tooltip_text("Abrir la nota vinculada en el editor")
            .css_classes(["suggested-action"])
            .visible(false)
            .build();
        boton_abrir_nota
            .update_property(&[gtk::accessible::Property::Label("Abrir la nota vinculada")]);

        let caja_colores = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(4)
            .build();
        caja_colores.update_property(&[gtk::accessible::Property::Label("Color de la tarjeta")]);

        let b_eliminar = gtk::Button::builder()
            .icon_name("user-trash-symbolic")
            .tooltip_text("Eliminar tarjeta seleccionada")
            .css_classes(["flat", "destructive-action"])
            .build();
        b_eliminar.update_property(&[gtk::accessible::Property::Label(
            "Eliminar tarjeta seleccionada",
        )]);

        let barra_contextual = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(8)
            .margin_start(12)
            .margin_end(12)
            .margin_top(6)
            .margin_bottom(6)
            .visible(false)
            .build();
        barra_contextual.append(&entrada_texto);
        barra_contextual.append(&boton_abrir_nota);
        barra_contextual.append(&caja_colores);
        barra_contextual.append(&b_eliminar);

        let lienzo = LienzoCanvas::new();
        let pagina_lienzo = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .vexpand(true)
            .build();
        pagina_lienzo.append(&barra_herramientas);
        pagina_lienzo.append(&barra_contextual);
        pagina_lienzo.append(&lienzo);

        // Página de protección ante archivo .canvas con errores de sintaxis (US3).
        let error_banner = adw::Banner::builder()
            .title("Este archivo .canvas tiene errores y no se sobrescribirá automáticamente")
            .button_label("Reiniciar lienzo…")
            .revealed(true)
            .build();
        let error_fuente = gtk::TextView::builder()
            .editable(false)
            .monospace(true)
            .top_margin(16)
            .bottom_margin(16)
            .left_margin(16)
            .right_margin(16)
            .vexpand(true)
            .build();
        error_fuente.update_property(&[gtk::accessible::Property::Label(
            "Contenido fuente del archivo .canvas con errores",
        )]);
        let error_scroll = gtk::ScrolledWindow::builder()
            .vexpand(true)
            .child(&error_fuente)
            .build();
        let pagina_error = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .vexpand(true)
            .build();
        pagina_error.append(&error_banner);
        pagina_error.append(&error_scroll);

        let stack = gtk::Stack::builder().vexpand(true).build();
        stack.add_named(&pagina_lienzo, Some("lienzo"));
        stack.add_named(&pagina_error, Some("error"));

        let root = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .vexpand(true)
            .hexpand(true)
            .build();
        root.append(&stack);

        let this = Self(Rc::new(Inner {
            root: root.clone(),
            stack,
            lienzo: lienzo.clone(),
            barra_contextual,
            entrada_texto: entrada_texto.clone(),
            boton_abrir_nota: boton_abrir_nota.clone(),
            nota_sel_ruta: RefCell::new(None),
            actualizando_ui: Cell::new(false),
            aux_actual: RefCell::new(None),
            error_banner: error_banner.clone(),
            error_fuente,
            al_guardar: RefCell::new(None),
            al_abrir_nota: RefCell::new(None),
            al_pedir_nota: RefCell::new(None),
        }));

        INSTANCIAS.with(|inst| {
            inst.borrow_mut().retain(|(_, w)| w.strong_count() > 0);
            inst.borrow_mut().push((root, Rc::downgrade(&this.0)));
        });

        // Botones de color para la tarjeta seleccionada.
        for color in ColorCanvas::TODOS {
            let b_col = gtk::Button::builder()
                .label(color.nombre_visible())
                .tooltip_text(format!("Color {}", color.nombre_visible()))
                .css_classes(["flat", "caption"])
                .build();
            b_col.update_property(&[gtk::accessible::Property::Label(&format!(
                "Color {}",
                color.nombre_visible()
            ))]);
            let weak = Rc::downgrade(&this.0);
            b_col.connect_clicked(move |_| {
                if let Some(i) = weak.upgrade() {
                    Self(i).cambiar_color_seleccionado(color);
                }
            });
            caja_colores.append(&b_col);
        }

        // Conectar señales del lienzo y la barra.
        {
            let weak = Rc::downgrade(&this.0);
            lienzo
                .imp()
                .al_seleccionar
                .replace(Some(Box::new(move |sel| {
                    if let Some(i) = weak.upgrade() {
                        Self(i).actualizar_barra_contextual(sel);
                    }
                })));
        }
        {
            let weak = Rc::downgrade(&this.0);
            lienzo.imp().al_cambiar.replace(Some(Box::new(move || {
                if let Some(i) = weak.upgrade() {
                    Self(i).guardar_automatico(false);
                }
            })));
        }
        {
            let weak = Rc::downgrade(&this.0);
            lienzo
                .imp()
                .al_abrir_nota
                .replace(Some(Box::new(move |ruta| {
                    if let Some(i) = weak.upgrade()
                        && let Some(f) = i.al_abrir_nota.borrow().as_ref()
                    {
                        f(ruta);
                    }
                })));
        }
        {
            let weak = Rc::downgrade(&this.0);
            b_add_txt.connect_clicked(move |_| {
                if let Some(i) = weak.upgrade() {
                    Self(i).agregar_tarjeta_texto("Nueva tarjeta");
                }
            });
        }
        {
            let weak = Rc::downgrade(&this.0);
            b_add_nota.connect_clicked(move |_| {
                if let Some(i) = weak.upgrade()
                    && let Some(f) = i.al_pedir_nota.borrow().as_ref()
                {
                    f();
                }
            });
        }
        {
            let weak = Rc::downgrade(&this.0);
            b_conectar.connect_clicked(move |btn| {
                if let Some(i) = weak.upgrade() {
                    Self(i).dialogo_conectar(btn.upcast_ref::<gtk::Widget>());
                }
            });
        }
        {
            let l = lienzo.clone();
            b_centrar.connect_clicked(move |_| l.centrar_vista());
        }
        {
            let weak = Rc::downgrade(&this.0);
            entrada_texto.connect_changed(move |e| {
                let Some(i) = weak.upgrade() else { return };
                if i.actualizando_ui.get() {
                    return;
                }
                let Some(idx) = i.lienzo.imp().seleccionado.get() else {
                    return;
                };
                let nuevo = e.text().to_string();
                let mut cambio = false;
                {
                    let mut doc = i.lienzo.imp().doc.borrow_mut();
                    if let Some(n) = doc.nodes.get_mut(idx)
                        && let TipoNodoCanvas::Texto(t) = &mut n.tipo
                        && *t != nuevo
                    {
                        *t = nuevo;
                        cambio = true;
                    }
                }
                if cambio {
                    i.lienzo.queue_draw();
                    Self(i).guardar_automatico(false);
                }
            });
        }
        {
            let weak = Rc::downgrade(&this.0);
            boton_abrir_nota.connect_clicked(move |_| {
                if let Some(i) = weak.upgrade()
                    && let Some(ruta) = i.nota_sel_ruta.borrow().clone()
                    && let Some(f) = i.al_abrir_nota.borrow().as_ref()
                {
                    f(&ruta);
                }
            });
        }
        {
            let weak = Rc::downgrade(&this.0);
            b_eliminar.connect_clicked(move |_| {
                if let Some(i) = weak.upgrade() {
                    Self(i).eliminar_seleccionada();
                }
            });
        }
        {
            let weak = Rc::downgrade(&this.0);
            error_banner.connect_button_clicked(move |banner| {
                if let Some(i) = weak.upgrade() {
                    Self(i).confirmar_reinicio_con_error(banner.upcast_ref::<gtk::Widget>());
                }
            });
        }

        this
    }

    pub fn desde_stack(stack: &gtk::Stack) -> Option<Self> {
        let child = stack.child_by_name("canvas")?;
        let caja = child.downcast::<gtk::Box>().ok()?;
        INSTANCIAS.with(|inst| {
            inst.borrow()
                .iter()
                .find(|(b, _)| *b == caja)
                .and_then(|(_, w)| w.upgrade())
                .map(Self)
        })
    }

    pub fn widget(&self) -> &gtk::Box {
        &self.0.root
    }

    pub fn connect_guardar(&self, f: impl Fn(AuxFile, String, bool) + 'static) {
        *self.0.al_guardar.borrow_mut() = Some(Box::new(f));
    }

    pub fn connect_abrir_nota(&self, f: impl Fn(&str) + 'static) {
        *self.0.al_abrir_nota.borrow_mut() = Some(Box::new(f));
    }

    pub fn connect_pedir_nota(&self, f: impl Fn() + 'static) {
        *self.0.al_pedir_nota.borrow_mut() = Some(Box::new(f));
    }

    /// Ruta del archivo `.canvas` actualmente cargado, si hay uno.
    pub fn ruta_actual(&self) -> Option<RelPath> {
        self.0.aux_actual.borrow().as_ref().map(|a| a.path.clone())
    }

    /// Carga un `AuxFile` leído con `Vault::read_aux`. Si tiene errores de sintaxis o esquema,
    /// activa la vista de error sin sobrescribir el archivo.
    pub fn cargar(&self, aux: AuxFile) {
        let i = &self.0;
        let resultado = match &aux.error {
            Some(err) => Err(format!("Línea {}: {}", err.line, err.message)),
            None => DocumentoCanvas::parse(&aux.source),
        };
        *i.aux_actual.borrow_mut() = Some(aux.clone());
        match resultado {
            Ok(doc) => {
                *i.lienzo.imp().doc.borrow_mut() = doc;
                i.lienzo.seleccionar(None);
                i.stack.set_visible_child_name("lienzo");
                i.lienzo.queue_draw();
            }
            Err(msg) => {
                i.error_banner.set_title(&format!(
                    "Error en «{}» ({msg}). No se sobrescribirá sin confirmación.",
                    aux.path
                ));
                i.error_fuente.buffer().set_text(&aux.source);
                i.stack.set_visible_child_name("error");
            }
        }
    }

    /// Indica si la vista actual está en modo de error protegido.
    #[cfg(test)]
    pub fn en_modo_error(&self) -> bool {
        self.0.stack.visible_child_name().as_deref() == Some("error")
    }

    /// Copia del `DocumentoCanvas` actual en memoria.
    #[cfg(test)]
    pub fn documento(&self) -> DocumentoCanvas {
        self.0.lienzo.imp().doc.borrow().clone()
    }

    /// Actualiza el `AuxFile` base tras un guardado exitoso en disco.
    pub fn actualizar_aux(&self, nuevo_aux: AuxFile) {
        *self.0.aux_actual.borrow_mut() = Some(nuevo_aux);
    }

    pub fn agregar_tarjeta_texto(&self, texto: &str) {
        let i = &self.0;
        let idx = {
            let mut doc = i.lienzo.imp().doc.borrow_mut();
            let id = doc.nuevo_id_nodo();
            let n = doc.nodes.len();
            let x = 60.0 + ((n % 4) as f64) * 260.0;
            let y = 60.0 + ((n / 4) as f64) * 160.0;
            doc.nodes.push(NodoCanvas {
                id,
                x,
                y,
                width: 240.0,
                height: 130.0,
                color: ColorCanvas::Rosa,
                tipo: TipoNodoCanvas::Texto(texto.to_owned()),
            });
            doc.nodes.len() - 1
        };
        i.lienzo.seleccionar(Some(idx));
        i.lienzo.queue_draw();
        self.guardar_automatico(false);
    }

    pub fn agregar_tarjeta_nota(&self, ruta: &str) {
        let i = &self.0;
        let idx = {
            let mut doc = i.lienzo.imp().doc.borrow_mut();
            let id = doc.nuevo_id_nodo();
            let n = doc.nodes.len();
            let x = 80.0 + ((n % 4) as f64) * 260.0;
            let y = 80.0 + ((n / 4) as f64) * 160.0;
            doc.nodes.push(NodoCanvas {
                id,
                x,
                y,
                width: 240.0,
                height: 110.0,
                color: ColorCanvas::Violeta,
                tipo: TipoNodoCanvas::Nota(ruta.to_owned()),
            });
            doc.nodes.len() - 1
        };
        i.lienzo.seleccionar(Some(idx));
        i.lienzo.queue_draw();
        self.guardar_automatico(false);
    }

    pub fn conectar_nodos(&self, from_id: &str, to_id: &str, label: &str) -> bool {
        if from_id == to_id {
            return false;
        }
        let i = &self.0;
        let mut doc = i.lienzo.imp().doc.borrow_mut();
        let existe_from = doc.nodes.iter().any(|n| n.id == from_id);
        let existe_to = doc.nodes.iter().any(|n| n.id == to_id);
        if !existe_from || !existe_to {
            return false;
        }
        let id = doc.nuevo_id_arista();
        doc.edges.push(AristaCanvas {
            id,
            from_node: from_id.to_owned(),
            to_node: to_id.to_owned(),
            label: label.trim().to_owned(),
        });
        drop(doc);
        i.lienzo.queue_draw();
        self.guardar_automatico(false);
        true
    }

    pub fn cambiar_color_seleccionado(&self, color: ColorCanvas) {
        let i = &self.0;
        let Some(idx) = i.lienzo.imp().seleccionado.get() else {
            return;
        };
        {
            let mut doc = i.lienzo.imp().doc.borrow_mut();
            if let Some(n) = doc.nodes.get_mut(idx) {
                n.color = color;
            }
        }
        i.lienzo.queue_draw();
        self.guardar_automatico(false);
    }

    pub fn eliminar_seleccionada(&self) {
        let i = &self.0;
        let Some(idx) = i.lienzo.imp().seleccionado.get() else {
            return;
        };
        i.lienzo.imp().doc.borrow_mut().eliminar_nodo(idx);
        i.lienzo.seleccionar(None);
        i.lienzo.queue_draw();
        self.guardar_automatico(false);
    }

    fn actualizar_barra_contextual(&self, sel: Option<usize>) {
        let i = &self.0;
        let Some(idx) = sel else {
            i.barra_contextual.set_visible(false);
            *i.nota_sel_ruta.borrow_mut() = None;
            return;
        };
        let doc = i.lienzo.imp().doc.borrow();
        let Some(nodo) = doc.nodes.get(idx) else {
            i.barra_contextual.set_visible(false);
            return;
        };
        i.actualizando_ui.set(true);
        match &nodo.tipo {
            TipoNodoCanvas::Texto(t) => {
                i.entrada_texto.set_visible(true);
                i.entrada_texto.set_text(t);
                i.boton_abrir_nota.set_visible(false);
                *i.nota_sel_ruta.borrow_mut() = None;
            }
            TipoNodoCanvas::Nota(r) => {
                i.entrada_texto.set_visible(false);
                i.boton_abrir_nota.set_visible(true);
                i.boton_abrir_nota.set_label(&format!("Abrir «{r}»"));
                *i.nota_sel_ruta.borrow_mut() = Some(r.clone());
            }
        }
        i.actualizando_ui.set(false);
        i.barra_contextual.set_visible(true);
    }

    fn guardar_automatico(&self, confirm: bool) {
        let i = &self.0;
        let Some(aux) = i.aux_actual.borrow().clone() else {
            return;
        };
        if aux.error.is_some() && !confirm {
            return;
        }
        let yaml = i.lienzo.imp().doc.borrow().serialize();
        if let Some(f) = i.al_guardar.borrow().as_ref() {
            f(aux, yaml, confirm);
        }
    }

    fn dialogo_conectar(&self, parent: &gtk::Widget) {
        let nodos: Vec<(String, String)> = {
            let doc = self.0.lienzo.imp().doc.borrow();
            doc.nodes
                .iter()
                .map(|n| (n.id.clone(), n.titulo_corto()))
                .collect()
        };
        if nodos.len() < 2 {
            return;
        }
        let titulos: Vec<&str> = nodos.iter().map(|(_, t)| t.as_str()).collect();
        let dd_origen = gtk::DropDown::from_strings(&titulos);
        dd_origen.set_selected(0);
        dd_origen.update_property(&[gtk::accessible::Property::Label("Tarjeta de origen")]);
        let dd_destino = gtk::DropDown::from_strings(&titulos);
        dd_destino.set_selected(1);
        dd_destino.update_property(&[gtk::accessible::Property::Label("Tarjeta de destino")]);
        let entrada_rotulo = gtk::Entry::builder()
            .placeholder_text("Etiqueta opcional de la flecha…")
            .build();
        entrada_rotulo
            .update_property(&[gtk::accessible::Property::Label("Etiqueta de la conexión")]);

        let caja = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(8)
            .build();
        caja.append(&dd_origen);
        caja.append(&dd_destino);
        caja.append(&entrada_rotulo);

        let dialog = adw::AlertDialog::builder()
            .heading("Conectar dos tarjetas")
            .body("Elegí la tarjeta de origen, la de destino y una etiqueta opcional.")
            .extra_child(&caja)
            .default_response("connect")
            .close_response("cancel")
            .build();
        dialog.add_response("cancel", "Cancelar");
        dialog.add_response("connect", "Conectar");
        dialog.set_response_appearance("connect", adw::ResponseAppearance::Suggested);

        let this = self.clone();
        dialog.choose(Some(parent), None::<&gtk::gio::Cancellable>, move |resp| {
            if resp == "connect" {
                let i_orig = dd_origen.selected() as usize;
                let i_dest = dd_destino.selected() as usize;
                if let (Some((id_o, _)), Some((id_d, _))) = (nodos.get(i_orig), nodos.get(i_dest)) {
                    this.conectar_nodos(id_o, id_d, &entrada_rotulo.text());
                }
            }
        });
    }

    fn confirmar_reinicio_con_error(&self, parent: &gtk::Widget) {
        let dialog = adw::AlertDialog::builder()
            .heading("¿Sobrescribir el archivo .canvas con un lienzo limpio?")
            .body("El archivo actual tiene errores de sintaxis. Al confirmar se respaldará y se reemplazará por un lienzo válido.")
            .default_response("cancel")
            .close_response("cancel")
            .build();
        dialog.add_response("cancel", "Cancelar");
        dialog.add_response("overwrite", "Sobrescribir");
        dialog.set_response_appearance("overwrite", adw::ResponseAppearance::Destructive);

        let this = self.clone();
        dialog.choose(Some(parent), None::<&gtk::gio::Cancellable>, move |resp| {
            if resp == "overwrite" {
                this.forzar_reinicio_confirmado();
            }
        });
    }

    /// Reemplaza un lienzo dañado por uno válido con `confirm = true` (tras confirmación).
    pub fn forzar_reinicio_confirmado(&self) {
        let i = &self.0;
        *i.lienzo.imp().doc.borrow_mut() = DocumentoCanvas::default();
        i.lienzo.seleccionar(None);
        i.stack.set_visible_child_name("lienzo");
        i.lienzo.queue_draw();
        self.guardar_automatico(true);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// T002/T005: ida y vuelta de DocumentoCanvas en YAML y compatibilidad con JSON y Vault::read_aux.
    #[test]
    fn us38_parse_y_serialize_yaml_y_json() {
        let mut doc = DocumentoCanvas::default();
        doc.nodes.push(NodoCanvas {
            id: "n1".into(),
            x: 80.0,
            y: 60.0,
            width: 240.0,
            height: 140.0,
            color: ColorCanvas::Rosa,
            tipo: TipoNodoCanvas::Texto("Idea central: A -> B".into()),
        });
        doc.nodes.push(NodoCanvas {
            id: "n2".into(),
            x: 420.0,
            y: 60.0,
            width: 240.0,
            height: 120.0,
            color: ColorCanvas::Violeta,
            tipo: TipoNodoCanvas::Nota("wiki/nota.md".into()),
        });
        doc.edges.push(AristaCanvas {
            id: "e1".into(),
            from_node: "n1".into(),
            to_node: "n2".into(),
            label: "fundamenta".into(),
        });

        let yaml_emitido = doc.serialize();
        let re_leido = DocumentoCanvas::parse(&yaml_emitido).expect("YAML válido");
        assert_eq!(re_leido, doc);

        // También acepta formato JSON con format_version: 1.
        let json = r#"{
            "format_version": 1,
            "nodes": [
                { "id": "a", "type": "text", "x": 10, "y": 20, "width": 200, "height": 100, "color": "cian", "text": "Hola" }
            ],
            "edges": []
        }"#;
        let desde_json = DocumentoCanvas::parse(json).expect("JSON válido");
        assert_eq!(desde_json.nodes.len(), 1);
        assert_eq!(desde_json.nodes[0].color, ColorCanvas::Cian);

        // Sin format_version o con YAML dañado falla de forma segura.
        assert!(DocumentoCanvas::parse("nodes: []\n").is_err());
        assert!(DocumentoCanvas::parse("format_version: [\n").is_err());
    }

    /// SC-002 (038): mide parse y serialize de un lienzo con 200 tarjetas y 200 conexiones.
    #[test]
    fn sc_canvas_200() {
        let mut doc = DocumentoCanvas::default();
        for k in 0..200 {
            doc.nodes.push(NodoCanvas {
                id: format!("n{k}"),
                x: (k % 15) as f64 * 260.0,
                y: (k / 15) as f64 * 160.0,
                width: 240.0,
                height: 130.0,
                color: ColorCanvas::TODOS[k % 6],
                tipo: if k % 3 == 0 {
                    TipoNodoCanvas::Nota(format!("wiki/nota_{k}.md"))
                } else {
                    TipoNodoCanvas::Texto(format!("Contenido de la tarjeta número {k}"))
                },
            });
            if k > 0 {
                doc.edges.push(AristaCanvas {
                    id: format!("e{k}"),
                    from_node: format!("n{}", k - 1),
                    to_node: format!("n{k}"),
                    label: "sigue".into(),
                });
            }
        }
        let t0 = std::time::Instant::now();
        let s = doc.serialize();
        let parsed = DocumentoCanvas::parse(&s).expect("parse 200");
        let ms = t0.elapsed().as_secs_f64() * 1000.0;
        assert_eq!(parsed.nodes.len(), 200);
        assert_eq!(parsed.edges.len(), 199);
        println!("CANVAS 200 tarjetas (serialize + parse): {ms:.2} ms");
        if !cfg!(debug_assertions) {
            assert!(
                ms < 5.0,
                "serialize + parse en release debe ser < 5 ms: {ms:.2} ms"
            );
        }
    }

    /// T005 (038): prueba GTK en proceso de VistaCanvas, tarjetas, conexiones y protección de errores.
    #[test]
    #[ignore = "requiere acceso GTK; tools/pruebas-gtk.sh"]
    fn us38_vista_canvas_edicion_y_error_gtk() {
        gtk::init().expect("se necesita acceso GTK");
        let root = std::env::temp_dir().join(format!("orbital-038-{}", glib::uuid_string_random()));
        std::fs::create_dir_all(root.join("wiki")).unwrap();
        std::fs::create_dir_all(root.join("raw")).unwrap();
        let path_ok = RelPath::new("wiki/mapa.canvas").unwrap();
        std::fs::write(
            path_ok.to_path(&root),
            "format_version: 1\nnodes: []\nedges: []\n",
        )
        .unwrap();
        let path_mal = RelPath::new("wiki/roto.canvas").unwrap();
        std::fs::write(path_mal.to_path(&root), "nodes: [sin cerrar\n").unwrap();

        let vault = orbital_vault::open(&root).unwrap();
        let vista = VistaCanvas::new();
        let guardados: Rc<RefCell<Vec<(String, bool)>>> = Rc::new(RefCell::new(Vec::new()));
        {
            let g = guardados.clone();
            vista.connect_guardar(move |_aux, yaml, confirm| {
                g.borrow_mut().push((yaml, confirm));
            });
        }

        // 1. Cargar archivo válido, agregar tarjeta de texto, de nota y conectarlas.
        let aux_ok = vault.read_aux(&path_ok).unwrap();
        vista.cargar(aux_ok);
        assert!(!vista.en_modo_error());
        vista.agregar_tarjeta_texto("Hipótesis");
        vista.agregar_tarjeta_nota("wiki/experimento.md");
        assert!(vista.conectar_nodos("n1", "n2", "prueba"));
        let doc = vista.documento();
        assert_eq!(doc.nodes.len(), 2);
        assert_eq!(doc.edges.len(), 1);
        assert!(!guardados.borrow().is_empty());

        // 2. Cargar archivo con error de sintaxis: NO escribe nada y queda en modo error.
        guardados.borrow_mut().clear();
        let aux_mal = vault.read_aux(&path_mal).unwrap();
        vista.cargar(aux_mal);
        assert!(vista.en_modo_error());
        assert!(
            guardados.borrow().is_empty(),
            "un .canvas dañado no se sobrescribe al abrirlo"
        );
        // Al confirmar el reinicio explícito, escribe con confirm = true.
        vista.forzar_reinicio_confirmado();
        assert!(!vista.en_modo_error());
        assert_eq!(guardados.borrow().len(), 1);
        assert!(guardados.borrow()[0].1, "usa confirm = true");

        // 3. Medición de snapshot GTK con 200 tarjetas y 199 flechas (SC-001: < 16 ms).
        for k in 0..200 {
            vista
                .0
                .lienzo
                .imp()
                .doc
                .borrow_mut()
                .nodes
                .push(NodoCanvas {
                    id: format!("m{k}"),
                    x: (k % 15) as f64 * 260.0,
                    y: (k / 15) as f64 * 160.0,
                    width: 220.0,
                    height: 120.0,
                    color: ColorCanvas::TODOS[k % 6],
                    tipo: TipoNodoCanvas::Texto(format!("Tarjeta {k}")),
                });
            if k > 0 {
                vista
                    .0
                    .lienzo
                    .imp()
                    .doc
                    .borrow_mut()
                    .edges
                    .push(AristaCanvas {
                        id: format!("me{k}"),
                        from_node: format!("m{}", k - 1),
                        to_node: format!("m{k}"),
                        label: "enlace".into(),
                    });
            }
        }
        let snap = gtk::Snapshot::new();
        let t0 = std::time::Instant::now();
        WidgetImpl::snapshot(vista.0.lienzo.imp(), &snap);
        let snap_ms = t0.elapsed().as_secs_f64() * 1000.0;
        assert!(
            snap_ms < 16.0,
            "el snapshot de 200 tarjetas debe ser < 16 ms: {snap_ms:.2} ms"
        );

        let _ = std::fs::remove_dir_all(root);
    }
}
