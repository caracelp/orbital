//! Spec 039: Mapa mental desde una nota.
//!
//! Construye un árbol jerárquico a partir de la nota abierta (título como raíz, encabezados
//! `#`–`######` como ramas principales y listas anidadas debajo de cada sección como subramas,
//! estilo Markmap) y lo dibuja como un mapa mental interactivo con curvas de Bézier, zoom, paneo,
//! plegado/desplegado por rama y salto directo a la línea de la nota al hacer clic en un nodo.

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use adw::prelude::*;
use gtk::subclass::prelude::*;
use gtk::{glib, gsk};

///Separación horizontal entre niveles del mapa mental (unidades del mundo).
const PASO_X: f64 = 280.0;
/// Alto reservado por hoja visible (unidades del mundo).
const PASO_Y: f64 = 52.0;
/// Alto de cada caja de nodo.
const ALTO_CAJA: f64 = 34.0;
/// Radio del botón circular de plegado al costado derecho del nodo.
const RADIO_TOGGLE: f64 = 10.0;

/// Paleta de 6 colores para distinguir las ramas principales (contraste WCAG AA sobre negro).
const COLORES_RAMA: [(&str, &str); 6] = [
    ("#ff8cc6", "#2b1924"), // Rosa
    ("#c792ea", "#231b2e"), // Violeta
    ("#89ddff", "#17262e"), // Cian
    ("#a3e635", "#1c2819"), // Verde
    ("#fbbf24", "#2a2215"), // Ámbar
    ("#f472b6", "#291824"), // Magenta suave
];

/// Nodo del árbol del mapa mental extraído de una nota.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NodoMapaMental {
    /// Identificador único dentro del árbol (`0` = raíz).
    pub id: usize,
    /// Texto visible limpio del nodo.
    pub texto: String,
    /// Número de línea en la nota fuente (base 1).
    pub linea: usize,
    /// Indica si sus subramas están plegadas (ocultas).
    pub plegado: bool,
    /// Subramas directas.
    pub hijos: Vec<NodoMapaMental>,
}

/// Caja posicionada en el plano del mundo tras calcular la disposición del árbol visible.
#[derive(Clone, Debug)]
pub struct CajaPosicionada {
    pub id: usize,
    pub padre_id: Option<usize>,
    pub texto: String,
    pub linea: usize,
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    pub color_rama: usize,
    pub tiene_hijos: bool,
    pub plegado: bool,
    pub cantidad_hijos: usize,
}

fn limpiar_texto_en_linea(s: &str) -> String {
    let mut t = s.trim();
    // Quitar identificadores de bloque al final (` ^id`).
    if let Some((antes, id)) = t.rsplit_once(" ^")
        && !id.is_empty()
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        t = antes.trim_end();
    }
    // Simplificar wikilinks `[[ruta|alias]]` -> `alias` o `[[ruta]]` -> `ruta`.
    let mut out = String::with_capacity(t.len());
    let mut resto = t;
    while let Some(ini) = resto.find("[[") {
        out.push_str(&resto[..ini]);
        let desp = &resto[ini + 2..];
        if let Some(fin) = desp.find("]]") {
            let interior = &desp[..fin];
            let visible = interior.rsplit_once('|').map_or(interior, |(_, a)| a);
            out.push_str(visible);
            resto = &desp[fin + 2..];
        } else {
            out.push_str("[[");
            resto = desp;
        }
    }
    out.push_str(resto);
    let limpio = out.replace("**", "").replace('`', "");
    let limpio = limpio.trim();
    if limpio.chars().count() > 56 {
        let recorte: String = limpio.chars().take(55).collect();
        format!("{recorte}…")
    } else {
        limpio.to_owned()
    }
}

fn parse_encabezado_atx(linea: &str) -> Option<(u16, String)> {
    let sin_esp = linea.trim_start_matches(' ');
    if linea.len() - sin_esp.len() > 3 {
        return None;
    }
    let gatos = sin_esp.bytes().take_while(|&b| b == b'#').count();
    if !(1..=6).contains(&gatos) {
        return None;
    }
    let resto = &sin_esp[gatos..];
    if !resto.starts_with(' ') && !resto.starts_with('\t') {
        return None;
    }
    let cuerpo = resto.trim().trim_end_matches('#').trim();
    if cuerpo.is_empty() {
        return None;
    }
    Some((gatos as u16, limpiar_texto_en_linea(cuerpo)))
}

fn parse_item_lista(linea: &str) -> Option<(u16, String)> {
    let mut indent = 0u16;
    let mut idx_bytes = 0usize;
    for c in linea.chars() {
        if c == ' ' {
            indent = indent.saturating_add(1);
            idx_bytes += 1;
        } else if c == '\t' {
            indent = indent.saturating_add(2);
            idx_bytes += 1;
        } else {
            break;
        }
    }
    let resto = &linea[idx_bytes..];
    let cuerpo = if let Some(c) = resto
        .strip_prefix("- ")
        .or_else(|| resto.strip_prefix("* "))
        .or_else(|| resto.strip_prefix("+ "))
    {
        c
    } else {
        let digitos = resto.bytes().take_while(|b| b.is_ascii_digit()).count();
        if digitos == 0 || digitos > 9 {
            return None;
        }
        let desp = &resto[digitos..];
        desp.strip_prefix(". ")
            .or_else(|| desp.strip_prefix(") "))?
    };
    let cuerpo = cuerpo.trim();
    if cuerpo.is_empty() {
        return None;
    }
    let cuerpo_casilla = if let Some(r) = cuerpo.strip_prefix("[ ] ") {
        format!("☐ {r}")
    } else if let Some(r) = cuerpo
        .strip_prefix("[x] ")
        .or_else(|| cuerpo.strip_prefix("[X] "))
    {
        format!("☑ {r}")
    } else {
        cuerpo.to_owned()
    };
    let limpio = limpiar_texto_en_linea(&cuerpo_casilla);
    if limpio.is_empty() {
        return None;
    }
    Some((10 + indent, limpio))
}

fn sin_etiquetas_html(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut en_tag = false;
    for c in s.chars() {
        match c {
            '<' => en_tag = true,
            '>' => en_tag = false,
            _ if !en_tag => out.push(c),
            _ => {}
        }
    }
    limpiar_texto_en_linea(&out)
}

fn insertar_en_ruta(raiz: &mut NodoMapaMental, ruta: &[usize], nuevo: NodoMapaMental) -> usize {
    let mut actual = raiz;
    for &idx in ruta {
        actual = &mut actual.hijos[idx];
    }
    actual.hijos.push(nuevo);
    actual.hijos.len() - 1
}

impl NodoMapaMental {
    /// Extrae el árbol del mapa mental desde el texto fuente de una nota `.md` o `.html`.
    pub fn extraer(titulo_raiz: &str, fuente: &str, es_html: bool) -> Self {
        let titulo = if titulo_raiz.trim().is_empty() {
            "Nota".to_owned()
        } else {
            limpiar_texto_en_linea(titulo_raiz)
        };
        let mut raiz = Self {
            id: 0,
            texto: titulo,
            linea: 1,
            plegado: false,
            hijos: Vec::new(),
        };
        let mut proximo_id = 1usize;
        // Pila de `(peso_jerarquico, indice_en_hijos_del_padre)`.
        let mut pila: Vec<(u16, usize)> = Vec::new();

        if es_html {
            let mut en_script_o_style = false;
            for (idx_linea, linea) in fuente.lines().enumerate() {
                let num_linea = idx_linea + 1;
                let min = linea.to_ascii_lowercase();
                if min.contains("<script") || min.contains("<style") {
                    en_script_o_style = true;
                }
                if en_script_o_style {
                    if min.contains("</script>") || min.contains("</style>") {
                        en_script_o_style = false;
                    }
                    continue;
                }
                let mut hallado: Option<(u16, String)> = None;
                for nivel in 1..=6u16 {
                    let abrir = format!("<h{nivel}");
                    if let Some(pos) = min.find(&abrir)
                        && let Some(cierre) = min[pos..].find('>')
                    {
                        let txt = sin_etiquetas_html(&linea[pos + cierre + 1..]);
                        if !txt.is_empty() {
                            hallado = Some((nivel, txt));
                            break;
                        }
                    }
                }
                if hallado.is_none()
                    && let Some(pos) = min.find("<li")
                    && let Some(cierre) = min[pos..].find('>')
                {
                    let txt = sin_etiquetas_html(&linea[pos + cierre + 1..]);
                    if !txt.is_empty() {
                        hallado = Some((10, txt));
                    }
                }
                if let Some((peso, texto)) = hallado {
                    while pila.last().is_some_and(|&(p, _)| p >= peso) {
                        pila.pop();
                    }
                    let ruta: Vec<usize> = pila.iter().map(|&(_, i)| i).collect();
                    let id = proximo_id;
                    proximo_id += 1;
                    let idx_hijo = insertar_en_ruta(
                        &mut raiz,
                        &ruta,
                        Self {
                            id,
                            texto,
                            linea: num_linea,
                            plegado: false,
                            hijos: Vec::new(),
                        },
                    );
                    pila.push((peso, idx_hijo));
                }
            }
            return raiz;
        }

        // Markdown: ignorar frontmatter inicial (`---`) y cercas de código (``` o ~~~).
        let mut lineas = fuente.lines().enumerate().peekable();
        if let Some((_, primera)) = lineas.peek()
            && primera.trim_start_matches('\u{feff}').trim_end() == "---"
        {
            lineas.next();
            for (_, l) in lineas.by_ref() {
                let t = l.trim_end();
                if t == "---" || t == "..." {
                    break;
                }
            }
        }

        let mut cerca: Option<char> = None;
        for (idx_linea, linea) in lineas {
            let num_linea = idx_linea + 1;
            let trim = linea.trim_start();
            if trim.starts_with("```") || trim.starts_with("~~~") {
                let ch = trim.chars().next().unwrap_or('`');
                match cerca {
                    None => {
                        cerca = Some(ch);
                        continue;
                    }
                    Some(c) if c == ch => {
                        cerca = None;
                        continue;
                    }
                    _ => continue,
                }
            }
            if cerca.is_some() {
                continue;
            }

            let hallado = parse_encabezado_atx(linea).or_else(|| parse_item_lista(linea));
            if let Some((peso, texto)) = hallado {
                while pila.last().is_some_and(|&(p, _)| p >= peso) {
                    pila.pop();
                }
                let ruta: Vec<usize> = pila.iter().map(|&(_, i)| i).collect();
                let id = proximo_id;
                proximo_id += 1;
                let idx_hijo = insertar_en_ruta(
                    &mut raiz,
                    &ruta,
                    Self {
                        id,
                        texto,
                        linea: num_linea,
                        plegado: false,
                        hijos: Vec::new(),
                    },
                );
                pila.push((peso, idx_hijo));
            }
        }

        raiz
    }

    /// Alterna el estado `plegado` del nodo con identificador `id_buscado`.
    pub fn alternar_plegado(&mut self, id_buscado: usize) -> bool {
        if self.id == id_buscado {
            if !self.hijos.is_empty() {
                self.plegado = !self.plegado;
                return true;
            }
            return false;
        }
        for h in &mut self.hijos {
            if h.alternar_plegado(id_buscado) {
                return true;
            }
        }
        false
    }

    /// Pliega o despliega todos los nodos descendientes de la raíz (la raíz siempre queda abierta).
    pub fn set_plegado_recursivo(&mut self, plegar: bool) {
        for h in &mut self.hijos {
            h.aplicar_plegado(plegar);
        }
    }

    fn aplicar_plegado(&mut self, plegar: bool) {
        if !self.hijos.is_empty() {
            self.plegado = plegar;
        }
        for h in &mut self.hijos {
            h.aplicar_plegado(plegar);
        }
    }
}

fn ancho_estimado(texto: &str) -> f64 {
    let chars = texto.chars().count() as f64;
    (chars * 7.4 + 32.0).clamp(120.0, 248.0)
}

fn alto_subarbol(nodo: &NodoMapaMental) -> f64 {
    if nodo.hijos.is_empty() || nodo.plegado {
        PASO_Y
    } else {
        nodo.hijos.iter().map(alto_subarbol).sum()
    }
}

#[allow(clippy::too_many_arguments)]
fn posicionar_recursivo(
    nodo: &NodoMapaMental,
    padre_id: Option<usize>,
    prof: usize,
    y_tope: f64,
    color_rama: usize,
    out: &mut Vec<CajaPosicionada>,
) {
    let span = alto_subarbol(nodo);
    let x = 40.0 + (prof as f64) * PASO_X;
    let y = y_tope + (span - ALTO_CAJA) * 0.5;
    let width = ancho_estimado(&nodo.texto);
    out.push(CajaPosicionada {
        id: nodo.id,
        padre_id,
        texto: nodo.texto.clone(),
        linea: nodo.linea,
        x,
        y,
        width,
        height: ALTO_CAJA,
        color_rama,
        tiene_hijos: !nodo.hijos.is_empty(),
        plegado: nodo.plegado,
        cantidad_hijos: nodo.hijos.len(),
    });

    if !nodo.plegado && !nodo.hijos.is_empty() {
        let mut cursor_y = y_tope;
        for (idx, h) in nodo.hijos.iter().enumerate() {
            let color_hijo = if prof == 0 {
                idx % COLORES_RAMA.len()
            } else {
                color_rama
            };
            let h_span = alto_subarbol(h);
            posicionar_recursivo(h, Some(nodo.id), prof + 1, cursor_y, color_hijo, out);
            cursor_y += h_span;
        }
    }
}

/// Calcula la disposición sin solapamiento de todos los nodos visibles del mapa mental.
pub fn calcular_layout(raiz: &NodoMapaMental) -> Vec<CajaPosicionada> {
    let mut out = Vec::new();
    posicionar_recursivo(raiz, None, 0, 40.0, 0, &mut out);
    out
}

#[derive(Clone, Copy)]
struct VistaMM {
    pan_x: f64,
    pan_y: f64,
    zoom: f64,
}

impl Default for VistaMM {
    fn default() -> Self {
        Self {
            pan_x: 0.0,
            pan_y: 0.0,
            zoom: 1.0,
        }
    }
}

type IrALineaFn = Box<dyn Fn(usize)>;

mod imp {
    use super::*;

    #[derive(Default)]
    pub struct LienzoMapaMental {
        pub arbol: RefCell<Option<NodoMapaMental>>,
        pub cajas: RefCell<Vec<CajaPosicionada>>,
        pub(super) vista: Cell<VistaMM>,
        pub drag_origen: Cell<Option<(f64, f64)>>,
        pub hover_id: Cell<Option<usize>>,
        pub al_ir_a_linea: RefCell<Option<IrALineaFn>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for LienzoMapaMental {
        const NAME: &'static str = "OrbitalLienzoMapaMental";
        type Type = super::LienzoMapaMental;
        type ParentType = gtk::Widget;
    }

    impl ObjectImpl for LienzoMapaMental {}

    impl WidgetImpl for LienzoMapaMental {
        fn snapshot(&self, snapshot: &gtk::Snapshot) {
            let width = self.obj().width() as f64;
            let height = self.obj().height() as f64;
            if width <= 0.0 || height <= 0.0 {
                return;
            }
            let v = self.vista.get();
            let cajas = self.cajas.borrow();
            if cajas.is_empty() {
                return;
            }
            let widget = self.obj();
            let hovered = self.hover_id.get();
            let color_texto = gtk::gdk::RGBA::parse("#f3edf1").expect("texto");
            let color_negro = gtk::gdk::RGBA::parse("#141013").expect("negro");

            let a_pantalla = |wx: f64, wy: f64| (wx * v.zoom - v.pan_x, wy * v.zoom - v.pan_y);

            // 1. Curvas de Bézier cúbicas entre cada padre y sus hijos visibles.
            for c in cajas.iter() {
                let Some(pid) = c.padre_id else {
                    continue;
                };
                let Some(padre) = cajas.iter().find(|p| p.id == pid) else {
                    continue;
                };
                let (x0, y0) = a_pantalla(padre.x + padre.width, padre.y + padre.height * 0.5);
                let (x1, y1) = a_pantalla(c.x, c.y + c.height * 0.5);
                if (x0 < 0.0 && x1 < 0.0)
                    || (y0 < 0.0 && y1 < 0.0)
                    || (x0 > width && x1 > width)
                    || (y0 > height && y1 > height)
                {
                    continue;
                }
                let mx = (x0 + x1) * 0.5;
                let builder = gsk::PathBuilder::new();
                builder.move_to(x0 as f32, y0 as f32);
                builder.cubic_to(
                    mx as f32, y0 as f32, mx as f32, y1 as f32, x1 as f32, y1 as f32,
                );
                let (acento_hex, _) = COLORES_RAMA[c.color_rama % COLORES_RAMA.len()];
                let col = gtk::gdk::RGBA::parse(acento_hex).expect("color rama");
                let trazo = gsk::Stroke::new((2.0 * v.zoom as f32).clamp(1.2, 3.2));
                snapshot.append_stroke(&builder.to_path(), &trazo, &col);
            }

            // 2. Cajas de nodos y botones circulares de plegado.
            let r_esquina = (9.0 * v.zoom) as f32;
            for c in cajas.iter() {
                let (x, y) = a_pantalla(c.x, c.y);
                let (w, h) = (c.width * v.zoom, c.height * v.zoom);
                let extra_der = (RADIO_TOGGLE * 3.0) * v.zoom;
                if x + w + extra_der < 0.0 || y + h < 0.0 || x > width || y > height {
                    continue;
                }
                let (acento_hex, fondo_hex) = COLORES_RAMA[c.color_rama % COLORES_RAMA.len()];
                let acento = gtk::gdk::RGBA::parse(acento_hex).expect("acento");
                let fondo = gtk::gdk::RGBA::parse(fondo_hex).expect("fondo");

                let rect = gtk::graphene::Rect::new(x as f32, y as f32, w as f32, h as f32);
                let rrect = gsk::RoundedRect::from_rect(rect, r_esquina);
                let b_caja = gsk::PathBuilder::new();
                b_caja.add_rounded_rect(&rrect);
                let path_caja = b_caja.to_path();
                snapshot.append_fill(&path_caja, gsk::FillRule::Winding, &fondo);

                let grosor = if hovered == Some(c.id) || c.id == 0 {
                    2.4
                } else {
                    1.4
                };
                snapshot.append_stroke(&path_caja, &gsk::Stroke::new(grosor), &acento);

                if v.zoom >= 0.35 {
                    let pad_x = (10.0 * v.zoom).max(4.0);
                    let layout = widget.create_pango_layout(Some(&c.texto));
                    layout.set_width(
                        ((w - pad_x * 2.0).max(20.0) * f64::from(gtk::pango::SCALE)) as i32,
                    );
                    layout.set_ellipsize(gtk::pango::EllipsizeMode::End);
                    let mut f = gtk::pango::FontDescription::new();
                    if c.id == 0 {
                        f.set_weight(gtk::pango::Weight::Bold);
                    }
                    f.set_size(
                        ((10.0 * v.zoom).clamp(6.0, 18.0) * f64::from(gtk::pango::SCALE)) as i32,
                    );
                    layout.set_font_description(Some(&f));
                    let (_, lh) = layout.pixel_size();
                    snapshot.save();
                    snapshot.translate(&gtk::graphene::Point::new(
                        (x + pad_x) as f32,
                        (y + (h - f64::from(lh)) * 0.5) as f32,
                    ));
                    snapshot.append_layout(&layout, &color_texto);
                    snapshot.restore();
                }

                // Indicador circular de plegado / desplegado para ramas con hijos (salvo la raíz).
                if c.tiene_hijos && c.id != 0 {
                    let (tx, ty) =
                        a_pantalla(c.x + c.width + RADIO_TOGGLE + 3.0, c.y + c.height * 0.5);
                    let rt = (RADIO_TOGGLE * v.zoom).max(4.0);
                    let b_circ = gsk::PathBuilder::new();
                    b_circ.add_circle(&gtk::graphene::Point::new(tx as f32, ty as f32), rt as f32);
                    let path_circ = b_circ.to_path();
                    if c.plegado {
                        snapshot.append_fill(&path_circ, gsk::FillRule::Winding, &acento);
                    } else {
                        snapshot.append_fill(&path_circ, gsk::FillRule::Winding, &fondo);
                        snapshot.append_stroke(&path_circ, &gsk::Stroke::new(1.3), &acento);
                    }
                    if v.zoom >= 0.45 {
                        let simbolo = if c.plegado {
                            format!("+{}", c.cantidad_hijos)
                        } else {
                            "−".to_owned()
                        };
                        let layout_s = widget.create_pango_layout(Some(&simbolo));
                        let mut fs = gtk::pango::FontDescription::new();
                        fs.set_weight(gtk::pango::Weight::Bold);
                        fs.set_size(
                            ((8.0 * v.zoom).clamp(5.0, 13.0) * f64::from(gtk::pango::SCALE)) as i32,
                        );
                        layout_s.set_font_description(Some(&fs));
                        let (sw, sh) = layout_s.pixel_size();
                        snapshot.save();
                        snapshot.translate(&gtk::graphene::Point::new(
                            (tx - f64::from(sw) * 0.5) as f32,
                            (ty - f64::from(sh) * 0.5) as f32,
                        ));
                        snapshot.append_layout(
                            &layout_s,
                            if c.plegado { &color_negro } else { &acento },
                        );
                        snapshot.restore();
                    }
                }
            }
        }
    }
}

glib::wrapper! {
    pub struct LienzoMapaMental(ObjectSubclass<imp::LienzoMapaMental>)
        @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

enum HitMapaMental {
    TogglePlegado(usize),
    CajaNodo { id: usize, linea: usize },
}

impl LienzoMapaMental {
    pub fn new() -> Self {
        let this: Self = glib::Object::builder().build();
        this.set_can_focus(true);
        this.set_focusable(true);
        this.set_vexpand(true);
        this.set_hexpand(true);
        this.update_property(&[gtk::accessible::Property::Label(
            "Mapa mental de la nota: encabezados y listas jerárquicas",
        )]);
        this.setup_gestos();
        this
    }

    pub fn set_arbol(&self, arbol: NodoMapaMental) {
        let cajas = calcular_layout(&arbol);
        *self.imp().arbol.borrow_mut() = Some(arbol);
        *self.imp().cajas.borrow_mut() = cajas;
        self.queue_draw();
    }

    pub fn recalcular(&self) {
        let nuevas = self
            .imp()
            .arbol
            .borrow()
            .as_ref()
            .map(calcular_layout)
            .unwrap_or_default();
        *self.imp().cajas.borrow_mut() = nuevas;
        self.queue_draw();
    }

    pub fn alternar_nodo(&self, id: usize) -> bool {
        let cambio = self
            .imp()
            .arbol
            .borrow_mut()
            .as_mut()
            .is_some_and(|a| a.alternar_plegado(id));
        if cambio {
            self.recalcular();
        }
        cambio
    }

    pub fn set_plegado_todo(&self, plegar: bool) {
        if let Some(a) = self.imp().arbol.borrow_mut().as_mut() {
            a.set_plegado_recursivo(plegar);
        }
        self.recalcular();
    }

    pub fn centrar_vista(&self) {
        self.imp().vista.set(VistaMM::default());
        self.queue_draw();
    }

    fn hit_en(&self, px: f64, py: f64) -> Option<HitMapaMental> {
        let v = self.imp().vista.get();
        let wx = (px + v.pan_x) / v.zoom;
        let wy = (py + v.pan_y) / v.zoom;
        let cajas = self.imp().cajas.borrow();
        for c in cajas.iter().rev() {
            if c.tiene_hijos && c.id != 0 {
                let tx = c.x + c.width + RADIO_TOGGLE + 3.0;
                let ty = c.y + c.height * 0.5;
                let dist = ((wx - tx).powi(2) + (wy - ty).powi(2)).sqrt();
                if dist <= RADIO_TOGGLE + 4.0 {
                    return Some(HitMapaMental::TogglePlegado(c.id));
                }
            }
            if wx >= c.x && wx <= c.x + c.width && wy >= c.y && wy <= c.y + c.height {
                return Some(HitMapaMental::CajaNodo {
                    id: c.id,
                    linea: c.linea,
                });
            }
        }
        None
    }

    fn setup_gestos(&self) {
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

        let drag = gtk::GestureDrag::new();
        drag.set_button(1);
        let this = self.clone();
        drag.connect_drag_begin(move |_, _x, _y| {
            let v = this.imp().vista.get();
            this.imp().drag_origen.set(Some((v.pan_x, v.pan_y)));
        });
        let this = self.clone();
        drag.connect_drag_update(move |_, dx, dy| {
            if let Some((px, py)) = this.imp().drag_origen.get() {
                let mut v = this.imp().vista.get();
                v.pan_x = px - dx;
                v.pan_y = py - dy;
                this.imp().vista.set(v);
                this.queue_draw();
            }
        });
        let this = self.clone();
        drag.connect_drag_end(move |g, dx, dy| {
            this.imp().drag_origen.set(None);
            if dx.abs() + dy.abs() < 5.0
                && let Some((sx, sy)) = g.start_point()
                && let Some(hit) = this.hit_en(sx, sy)
            {
                match hit {
                    HitMapaMental::TogglePlegado(id) => {
                        this.alternar_nodo(id);
                    }
                    HitMapaMental::CajaNodo { linea, .. } => {
                        if let Some(f) = this.imp().al_ir_a_linea.borrow().as_ref() {
                            f(linea);
                        }
                    }
                }
            }
        });
        self.add_controller(drag);

        let motion = gtk::EventControllerMotion::new();
        let this = self.clone();
        motion.connect_motion(move |_, x, y| {
            let nuevo = match this.hit_en(x, y) {
                Some(HitMapaMental::CajaNodo { id, .. } | HitMapaMental::TogglePlegado(id)) => {
                    Some(id)
                }
                None => None,
            };
            if nuevo != this.imp().hover_id.get() {
                this.imp().hover_id.set(nuevo);
                this.queue_draw();
            }
        });
        let this = self.clone();
        motion.connect_leave(move |_| {
            this.imp().hover_id.set(None);
            this.queue_draw();
        });
        self.add_controller(motion);
    }
}

type SaltarLineaNotaFn = Box<dyn Fn(&str, usize)>;
type CerrarMMFn = Box<dyn Fn()>;

struct Inner {
    root: gtk::Box,
    titulo_barra: adw::WindowTitle,
    lienzo: LienzoMapaMental,
    ruta_nota: RefCell<Option<String>>,
    al_saltar: RefCell<Option<SaltarLineaNotaFn>>,
    al_cerrar: RefCell<Option<CerrarMMFn>>,
}

#[derive(Clone)]
pub struct VistaMapaMental(Rc<Inner>);

thread_local! {
    static INSTANCIAS: RefCell<Vec<(gtk::Box, Weak<Inner>)>> = const { RefCell::new(Vec::new()) };
}

impl VistaMapaMental {
    pub fn new() -> Self {
        let titulo_barra = adw::WindowTitle::new("Mapa mental de la nota", "");

        let b_plegar = gtk::Button::builder()
            .label("Plegar ramas")
            .tooltip_text("Plegar todas las subramas del mapa mental")
            .css_classes(["flat"])
            .build();
        b_plegar.update_property(&[gtk::accessible::Property::Label("Plegar todas las ramas")]);

        let b_desplegar = gtk::Button::builder()
            .label("Desplegar todo")
            .tooltip_text("Desplegar todas las ramas del mapa mental")
            .css_classes(["flat"])
            .build();
        b_desplegar.update_property(&[gtk::accessible::Property::Label(
            "Desplegar todas las ramas",
        )]);

        let b_centrar = gtk::Button::builder()
            .icon_name("zoom-fit-best-symbolic")
            .tooltip_text("Restablecer vista del mapa mental")
            .css_classes(["flat"])
            .build();
        b_centrar.update_property(&[gtk::accessible::Property::Label(
            "Restablecer vista del mapa mental",
        )]);

        let b_cerrar = gtk::Button::builder()
            .icon_name("window-close-symbolic")
            .tooltip_text("Cerrar el mapa mental (Esc)")
            .css_classes(["flat"])
            .build();
        b_cerrar.update_property(&[gtk::accessible::Property::Label("Cerrar el mapa mental")]);

        let barra = adw::HeaderBar::new();
        barra.set_title_widget(Some(&titulo_barra));
        barra.pack_start(&b_plegar);
        barra.pack_start(&b_desplegar);
        barra.pack_end(&b_cerrar);
        barra.pack_end(&b_centrar);

        let lienzo = LienzoMapaMental::new();
        let root = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .vexpand(true)
            .hexpand(true)
            .build();
        root.append(&barra);
        root.append(&lienzo);

        let this = Self(Rc::new(Inner {
            root: root.clone(),
            titulo_barra,
            lienzo: lienzo.clone(),
            ruta_nota: RefCell::new(None),
            al_saltar: RefCell::new(None),
            al_cerrar: RefCell::new(None),
        }));

        INSTANCIAS.with(|inst| {
            inst.borrow_mut().retain(|(_, w)| w.strong_count() > 0);
            inst.borrow_mut().push((root, Rc::downgrade(&this.0)));
        });

        {
            let l = lienzo.clone();
            b_plegar.connect_clicked(move |_| l.set_plegado_todo(true));
        }
        {
            let l = lienzo.clone();
            b_desplegar.connect_clicked(move |_| l.set_plegado_todo(false));
        }
        {
            let l = lienzo.clone();
            b_centrar.connect_clicked(move |_| l.centrar_vista());
        }
        {
            let weak = Rc::downgrade(&this.0);
            b_cerrar.connect_clicked(move |_| {
                if let Some(i) = weak.upgrade()
                    && let Some(f) = i.al_cerrar.borrow().as_ref()
                {
                    f();
                }
            });
        }
        {
            let weak = Rc::downgrade(&this.0);
            lienzo
                .imp()
                .al_ir_a_linea
                .replace(Some(Box::new(move |linea| {
                    if let Some(i) = weak.upgrade()
                        && let Some(ruta) = i.ruta_nota.borrow().clone()
                        && let Some(f) = i.al_saltar.borrow().as_ref()
                    {
                        f(&ruta, linea);
                    }
                })));
        }
        {
            let teclas = gtk::EventControllerKey::new();
            let weak = Rc::downgrade(&this.0);
            teclas.connect_key_pressed(move |_, keyval, _, _| {
                if keyval == gtk::gdk::Key::Escape
                    && let Some(i) = weak.upgrade()
                    && let Some(f) = i.al_cerrar.borrow().as_ref()
                {
                    f();
                    return glib::Propagation::Stop;
                }
                glib::Propagation::Proceed
            });
            this.0.root.add_controller(teclas);
        }

        this
    }

    pub fn desde_stack(stack: &gtk::Stack) -> Option<Self> {
        let child = stack.child_by_name("mapa-mental")?;
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

    pub fn connect_saltar_linea(&self, f: impl Fn(&str, usize) + 'static) {
        *self.0.al_saltar.borrow_mut() = Some(Box::new(f));
    }

    pub fn connect_cerrar(&self, f: impl Fn() + 'static) {
        *self.0.al_cerrar.borrow_mut() = Some(Box::new(f));
    }

    pub fn cargar_nota(&self, ruta: &str, titulo: &str, fuente: &str, es_html: bool) {
        let i = &self.0;
        *i.ruta_nota.borrow_mut() = Some(ruta.to_owned());
        i.titulo_barra.set_subtitle(ruta);
        let arbol = NodoMapaMental::extraer(titulo, fuente, es_html);
        i.lienzo.set_arbol(arbol);
    }

    #[cfg(test)]
    pub fn cantidad_cajas_visibles(&self) -> usize {
        self.0.lienzo.imp().cajas.borrow().len()
    }

    #[cfg(test)]
    pub fn alternar_nodo(&self, id: usize) -> bool {
        self.0.lienzo.alternar_nodo(id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// T002/T005: extrae encabezados ATX y listas anidadas ignorando frontmatter y codeblocks.
    #[test]
    fn us39_extrae_jerarquia_md_y_html() {
        let md = "\
---
type: atomo
title: Termodinámica
---
# Principios
- Conservación de la energía ^b1
  - Primera ley
- Entropía
```python
# Esto es un comentario en código, no un encabezado
- ni un ítem
```
## Ciclos
1. Carnot
2. Rankine
";
        let arbol = NodoMapaMental::extraer("Termodinámica", md, false);
        assert_eq!(arbol.texto, "Termodinámica");
        assert_eq!(arbol.hijos.len(), 1);
        let h1 = &arbol.hijos[0];
        assert_eq!(h1.texto, "Principios");
        assert_eq!(h1.linea, 5);
        // Hijos de # Principios: dos ítems de lista y un subencabezado ## Ciclos.
        assert_eq!(h1.hijos.len(), 3);
        assert_eq!(h1.hijos[0].texto, "Conservación de la energía");
        assert_eq!(h1.hijos[0].hijos.len(), 1);
        assert_eq!(h1.hijos[0].hijos[0].texto, "Primera ley");
        assert_eq!(h1.hijos[1].texto, "Entropía");
        assert_eq!(h1.hijos[2].texto, "Ciclos");
        assert_eq!(h1.hijos[2].hijos.len(), 2);

        // Plegar una rama oculta a sus descendientes en el layout.
        let cajas_abiertas = calcular_layout(&arbol);
        assert_eq!(cajas_abiertas.len(), 8);
        let mut clon = arbol.clone();
        assert!(clon.alternar_plegado(h1.hijos[2].id));
        let cajas_plegadas = calcular_layout(&clon);
        assert_eq!(cajas_plegadas.len(), 6);

        // HTML también extrae h1..h6 y li.
        let html = "<h1>Síntesis</h1>\n<ul>\n<li>Punto A</li>\n</ul>\n";
        let arbol_html = NodoMapaMental::extraer("Síntesis", html, true);
        assert_eq!(arbol_html.hijos.len(), 1);
        assert_eq!(arbol_html.hijos[0].hijos.len(), 1);
        assert_eq!(arbol_html.hijos[0].hijos[0].texto, "Punto A");
    }

    /// SC-001 (039): mide extracción y cálculo de layout para una nota de 500 nodos.
    #[test]
    fn sc_mapa_mental_500() {
        let mut md = String::from("---\ntype: atomo\n---\n");
        for s in 0..50 {
            md.push_str(&format!("# Sección {s}\n"));
            md.push_str(&format!("## Subsección {s}\n"));
            for i in 0..8 {
                md.push_str(&format!("- Ítem de lista {s}.{i}\n"));
            }
        }
        let t0 = std::time::Instant::now();
        let arbol = NodoMapaMental::extraer("Nota grande", &md, false);
        let cajas = calcular_layout(&arbol);
        let ms = t0.elapsed().as_secs_f64() * 1000.0;
        assert_eq!(cajas.len(), 501);
        println!("MAPA MENTAL 500 nodos (extraer + layout): {ms:.2} ms");
        if !cfg!(debug_assertions) {
            assert!(
                ms < 5.0,
                "extraer + layout en release debe ser < 5 ms: {ms:.2} ms"
            );
        }
    }

    /// T005 (039): prueba GTK en proceso de VistaMapaMental, plegado y snapshot.
    #[test]
    #[ignore = "requiere acceso GTK; tools/pruebas-gtk.sh"]
    fn us39_vista_mapa_mental_gtk() {
        gtk::init().expect("se necesita acceso GTK");
        let vista = VistaMapaMental::new();
        let md = "# Uno\n- A\n- B\n# Dos\n- C\n";
        vista.cargar_nota("wiki/prueba.md", "Prueba", md, false);
        assert_eq!(vista.cantidad_cajas_visibles(), 6);

        // Plegar la rama # Uno (id = 1) oculta sus 2 hijos.
        assert!(vista.alternar_nodo(1));
        assert_eq!(vista.cantidad_cajas_visibles(), 4);
        assert!(vista.alternar_nodo(1));
        assert_eq!(vista.cantidad_cajas_visibles(), 6);
    }
}
