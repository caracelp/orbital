//! Multi-cursor en la vista Código (006).
//!
//! El cursor principal sigue siendo la selección nativa del buffer (AT-SPI y lector de pantalla
//! intactos, research R1). Los cursores extra son pares de `GtkTextMark` (inserción con gravedad
//! derecha, límite con gravedad izquierda) y se dibujan en un overlay que no recibe eventos.
//! Sin cursores extra, el editor se comporta exactamente como siempre (FR-7).

use gtk::glib;
use gtk::glib::subclass::prelude::ObjectSubclassIsExt;
use gtk::prelude::*;

const MAX_CURSORES: usize = 1000;

pub enum EditKind {
    Insert(String),
    Backspace,
    Delete,
}

/// Un cursor extra: `(inserción, límite)`. La inserción tiene gravedad derecha y el límite
/// izquierda, así el par abraza la selección aunque se edite alrededor.
pub struct ExtraCursors {
    cursors: Vec<(gtk::TextMark, gtk::TextMark)>,
}

impl ExtraCursors {
    pub fn new() -> Self {
        Self {
            cursors: Vec::new(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.cursors.is_empty()
    }

    /// Dónde está la inserción de cada cursor extra.
    pub fn inserciones<'a>(
        &'a self,
        buf: &'a impl IsA<gtk::TextBuffer>,
    ) -> impl Iterator<Item = gtk::TextIter> + 'a {
        self.cursors
            .iter()
            .map(move |(insert, _)| buf.iter_at_mark(insert))
    }

    /// Quita todos los cursores extra (Esc, cambio de vista, nota o bóveda).
    pub fn clear(&mut self, buf: &impl IsA<gtk::TextBuffer>) {
        for (insert, bound) in self.cursors.drain(..) {
            buf.delete_mark(&insert);
            buf.delete_mark(&bound);
        }
    }

    /// Agrega un cursor en `iter`. Devuelve false si se alcanzó el límite.
    pub fn add_cursor(&mut self, buf: &impl IsA<gtk::TextBuffer>, iter: &gtk::TextIter) -> bool {
        if self.cursors.len() >= MAX_CURSORES {
            return false;
        }
        let insert = buf.create_mark(None, iter, false); // gravedad derecha
        let bound = buf.create_mark(None, iter, true); // gravedad izquierda
        self.cursors.push((insert, bound));
        true
    }

    /// Agrega una selección extra (el orden de `a` y `b` se normaliza).
    pub fn add_selection(
        &mut self,
        buf: &impl IsA<gtk::TextBuffer>,
        a: &gtk::TextIter,
        b: &gtk::TextIter,
    ) -> bool {
        if self.cursors.len() >= MAX_CURSORES {
            return false;
        }
        let (insert, bound) = if a.offset() <= b.offset() {
            (b, a)
        } else {
            (a, b)
        };
        let insert = buf.create_mark(None, insert, false);
        let bound = buf.create_mark(None, bound, true);
        self.cursors.push((insert, bound));
        true
    }

    /// Quita el cursor cuya barra o selección contenga `iter` (Alt+clic sobre uno existente).
    pub fn remove_at(&mut self, buf: &impl IsA<gtk::TextBuffer>, iter: &gtk::TextIter) -> bool {
        let pos = iter.offset();
        let Some(idx) = self.cursors.iter().position(|(insert, bound)| {
            let (ins, lim) = (buf.iter_at_mark(insert), buf.iter_at_mark(bound));
            let lo = ins.offset().min(lim.offset());
            let hi = ins.offset().max(lim.offset());
            pos == ins.offset() || (lo <= pos && pos <= hi)
        }) else {
            return false;
        };
        let (insert, bound) = self.cursors.remove(idx);
        buf.delete_mark(&insert);
        buf.delete_mark(&bound);
        true
    }
}

fn aplicar(
    buf: &impl IsA<gtk::TextBuffer>,
    insert: &mut gtk::TextIter,
    bound: &mut gtk::TextIter,
    edit: &EditKind,
) {
    match edit {
        EditKind::Insert(texto) => {
            if insert.offset() != bound.offset() {
                buf.delete(insert, bound);
            }
            buf.insert(insert, texto);
        }
        EditKind::Backspace => {
            if insert.offset() != bound.offset() {
                buf.delete(insert, bound);
            } else if insert.backward_char() {
                let mut desde = *insert;
                desde.forward_char();
                buf.delete(&mut desde, insert);
            }
        }
        EditKind::Delete => {
            if insert.offset() != bound.offset() {
                buf.delete(insert, bound);
            } else {
                let mut hasta = *insert;
                hasta.forward_char();
                if hasta.offset() != insert.offset() {
                    buf.delete(insert, &mut hasta);
                }
            }
        }
    }
}

/// Tanda de edición que incluye el cursor principal y todos los extra (FR-3/FR-4): se aplica de
/// atrás hacia adelante y toda junta, dentro de una sola acción de deshacer.
pub fn editar_tanda(
    buf: &impl IsA<gtk::TextBuffer>,
    extra: &ExtraCursors,
    edit: &EditKind,
) -> bool {
    buf.begin_user_action();
    // Orden global descendente (principal + extras): los TextIter se invalidan con cada
    // mutación, así que cada aplicación relee su posición (research R4).
    let main_off = buf.iter_at_mark(&buf.get_insert()).offset();
    let mut orden: Vec<(i32, Option<usize>)> = extra
        .cursors
        .iter()
        .enumerate()
        .map(|(n, (insert, _))| (buf.iter_at_mark(insert).offset(), Some(n)))
        .collect();
    orden.push((main_off, None));
    orden.sort_by_key(|(off, _)| std::cmp::Reverse(*off));
    for (_, cual) in orden {
        match cual {
            None => {
                let mut insert = buf.iter_at_mark(&buf.get_insert());
                let mut bound = buf.iter_at_mark(&buf.selection_bound());
                aplicar(buf, &mut insert, &mut bound, edit);
            }
            Some(n) => {
                let (insert, bound) = &extra.cursors[n];
                let mut i = buf.iter_at_mark(insert);
                let mut b = buf.iter_at_mark(bound);
                aplicar(buf, &mut i, &mut b, edit);
            }
        }
    }
    buf.end_user_action();
    true
}

/// Cursor en la misma columna de la línea adyacente, clampeado al final de la línea (US2).
/// `None` en los bordes de la nota.
pub fn iter_columna_adyacente(
    buf: &impl IsA<gtk::TextBuffer>,
    base: &gtk::TextIter,
    delta: i32,
) -> Option<gtk::TextIter> {
    let linea = base.line();
    let columna = base.line_index();
    let destino = linea + delta;
    if destino < 0 || destino >= buf.line_count() {
        return None;
    }
    // NOTA GTK4: `iter_at_line` salta las líneas vacías; avanzar con `forward_line` las respeta.
    let mut it = buf.start_iter();
    for _ in 0..destino {
        if !it.forward_line() {
            break;
        }
    }
    // NOTA GTK4: forward_to_line_end desde una línea vacía salta a la siguiente línea; avanzar
    // manualmente respeta la línea actual.
    let mut fin = it;
    while !fin.ends_line() && !fin.is_end() {
        fin.forward_char();
    }
    let col = columna.min(fin.line_index());
    it.forward_chars(col);
    Some(it)
}

/// Base para columnas: el cursor más al norte al subir y el más al sur al bajar, entre el
/// principal y los extra (como VS Code).
pub fn base_columna(
    buf: &impl IsA<gtk::TextBuffer>,
    extra: &ExtraCursors,
    delta: i32,
) -> gtk::TextIter {
    let mut candidatos: Vec<gtk::TextIter> = extra
        .cursors
        .iter()
        .map(|(insert, _)| buf.iter_at_mark(insert))
        .collect();
    candidatos.push(buf.iter_at_mark(&buf.get_insert()));
    candidatos.sort_by_key(|i| i.line());
    if delta < 0 {
        candidatos[0]
    } else {
        *candidatos.last().expect("siempre está el principal")
    }
}

/// Rectángulos en coordenadas del widget de la vista para el overlay (research R2).
#[derive(Clone)]
pub struct Rects {
    pub barras: Vec<(f64, f64, f64)>,
    pub selecciones: Vec<(f64, f64, f64, f64)>,
}

/// Calcula los rectángulos de los cursores extra en coordenadas del widget de la vista.
/// Pasa rectángulos en coordenadas del buffer a coordenadas de `capa` (la capa de cursores, que
/// cubre el scroller): primero a la ventana de la vista (que ya descuenta el desplazamiento) y
/// después a la capa. Los que quedan fuera de lo visible igual se dibujan recortados por la capa.
pub fn a_capa(rects: Rects, view: &impl IsA<gtk::TextView>, capa: &impl IsA<gtk::Widget>) -> Rects {
    let punto = |x: f64, y: f64| {
        let (wx, wy) =
            view.buffer_to_window_coords(gtk::TextWindowType::Widget, x as i32, y as i32);
        view.upcast_ref::<gtk::TextView>()
            .upcast_ref::<gtk::Widget>()
            .compute_point(capa, &gtk::graphene::Point::new(wx as f32, wy as f32))
            .map_or((x, y), |p| (f64::from(p.x()), f64::from(p.y())))
    };
    Rects {
        barras: rects
            .barras
            .into_iter()
            .map(|(x, y, h)| {
                let (x, y) = punto(x, y);
                (x, y, h)
            })
            .collect(),
        selecciones: rects
            .selecciones
            .into_iter()
            .map(|(x, y, w, h)| {
                let (x, y) = punto(x, y);
                (x, y, w, h)
            })
            .collect(),
    }
}

pub fn rects_para_dibujar(
    view: &impl IsA<gtk::TextView>,
    buf: &impl IsA<gtk::TextBuffer>,
    extra: &ExtraCursors,
) -> Rects {
    let mut barras = Vec::new();
    let mut selecciones = Vec::new();
    for (insert, bound) in extra.cursors.iter() {
        let ins = buf.iter_at_mark(insert);
        let lim = buf.iter_at_mark(bound);
        let barra = view.iter_location(&ins);
        barras.push((
            barra.x() as f64,
            barra.y() as f64,
            barra.height().max(18) as f64,
        ));
        if ins.offset() == lim.offset() {
            continue;
        }
        let (lo, hi) = if ins.offset() < lim.offset() {
            (ins, lim)
        } else {
            (lim, ins)
        };
        // La selección puede abarcar varias líneas: una tira por línea.
        let mut linea = lo.line();
        loop {
            let inicio = if linea == lo.line() {
                lo
            } else {
                buf.iter_at_line(linea).unwrap_or(lo)
            };
            let fin = if linea == hi.line() {
                hi
            } else {
                buf.iter_at_line(linea)
                    .map(|mut it| {
                        it.forward_to_line_end();
                        it
                    })
                    .unwrap_or(hi)
            };
            let a = view.iter_location(&inicio);
            let b = view.iter_location(&fin);
            selecciones.push((
                a.x() as f64,
                a.y() as f64,
                (b.x() - a.x()).max(4) as f64,
                b.height().max(18) as f64,
            ));
            if linea >= hi.line() {
                break;
            }
            linea += 1;
        }
    }
    Rects {
        barras,
        selecciones,
    }
}

/// El color del cursor extra: el rosa de la paleta (constitución: rosa y negro).
pub const COLOR_CURSOR: &str = "#ff8cc6";

pub fn color_rgba() -> gtk::gdk::RGBA {
    gtk::gdk::RGBA::parse(COLOR_CURSOR).expect("color de paleta válido")
}

pub mod imp {
    use std::cell::RefCell;
    use std::rc::Rc;

    use gtk::glib;
    use gtk::glib::subclass::prelude::*;
    use gtk::prelude::*;
    use gtk::subclass::prelude::*;

    use super::Rects;

    #[derive(Default)]
    pub struct CarenOverlay {
        /// Rectángulos en coordenadas del buffer y la vista que los ubica: se pasan a
        /// coordenadas de la capa al dibujar, cuando el desplazamiento de la vista ya está al día.
        pub rects: RefCell<Option<Rc<Rects>>>,
        pub vista: RefCell<Option<gtk::TextView>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for CarenOverlay {
        const NAME: &'static str = "OrbitalCarenOverlay";
        type Type = super::CarenOverlay;
        type ParentType = gtk::Widget;
    }

    impl ObjectImpl for CarenOverlay {}

    impl WidgetImpl for CarenOverlay {
        fn snapshot(&self, snapshot: &gtk::Snapshot) {
            let Some(rects) = self.rects.borrow().clone() else {
                return;
            };
            let Some(vista) = self.vista.borrow().clone() else {
                return;
            };
            let rects = &super::a_capa((*rects).clone(), &vista, &*self.obj());
            let color = super::color_rgba();
            // Selecciones primero (debajo de las barras).
            for (x, y, w, h) in &rects.selecciones {
                let mut fondo = color;
                fondo.set_alpha(0.20);
                let bounds = gtk::graphene::Rect::new(*x as f32, *y as f32, *w as f32, *h as f32);
                snapshot.append_color(&fondo, &bounds);
            }
            for (x, y, h) in &rects.barras {
                let bounds = gtk::graphene::Rect::new(*x as f32, *y as f32, 2.0, *h as f32);
                snapshot.append_color(&color, &bounds);
            }
        }
    }
}

glib::wrapper! {
    pub struct CarenOverlay(ObjectSubclass<imp::CarenOverlay>)
        @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl CarenOverlay {
    pub fn new() -> Self {
        let this: Self = glib::Object::builder().build();
        // No recibe ni bloquea eventos: el mouse y el teclado pasan al editor (research R2).
        this.set_can_target(false);
        this.set_can_focus(false);
        this.set_halign(gtk::Align::Fill);
        this.set_valign(gtk::Align::Fill);
        this.set_hexpand(true);
        this.set_vexpand(true);
        this
    }

    /// Rectángulos nuevos en coordenadas del widget; el overlay se redibuja solo.
    /// Rectángulos en coordenadas del buffer de `vista` (se ubican al dibujar).
    pub fn set_rects(&self, vista: &impl IsA<gtk::TextView>, rects: std::rc::Rc<Rects>) {
        *self.imp().vista.borrow_mut() = Some(vista.clone().upcast());
        *self.imp().rects.borrow_mut() = Some(rects);
        self.queue_draw();
    }

    pub fn limpiar(&self) {
        *self.imp().rects.borrow_mut() = None;
        self.queue_draw();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn texto(buf: &impl IsA<gtk::TextBuffer>) -> String {
        buf.text(&buf.start_iter(), &buf.end_iter(), true)
            .to_string()
    }

    /// T002/T005: agregar cursores, editar tanda con el principal incluido y deshacer en un paso.
    #[test]
    #[ignore = "requiere acceso GTK; tools/verificar-006-us1.sh"]
    fn us6_cursors_edit_tanda_and_undo() {
        gtk::init().expect("se necesita acceso GTK");
        let buf = sourceview5::Buffer::new(None);
        buf.set_text("uno dos tres");
        let mut extra = ExtraCursors::new();
        extra.add_cursor(&buf, &buf.iter_at_offset(3)); // fin de «uno»
        extra.add_cursor(&buf, &buf.iter_at_offset(7)); // fin de «dos»
        assert_eq!(extra.cursors.len(), 2);
        // El principal al final de «tres»: la tanda inserta en los tres.
        buf.select_range(&buf.iter_at_offset(12), &buf.iter_at_offset(12));
        assert!(editar_tanda(&buf, &extra, &EditKind::Insert("-".into())));
        // El principal y los extra insertan en la misma tanda (FR-3).
        assert_eq!(texto(&buf), "uno- dos- tres-");
        // Deshacer: toda la tanda en un solo paso (FR-4).
        assert!(buf.can_undo());
        buf.undo();
        assert_eq!(texto(&buf), "uno dos tres");
        buf.redo();
        assert_eq!(texto(&buf), "uno- dos- tres-");
    }

    /// T005: Backspace y Delete con selecciones extra (reemplazo) y sin ellas.
    #[test]
    #[ignore = "requiere acceso GTK"]
    fn us6_cursors_backspace_delete_selections() {
        gtk::init().expect("se necesita acceso GTK");
        let buf = sourceview5::Buffer::new(None);
        buf.set_text("alfa bravo charlie");
        let mut extra = ExtraCursors::new();
        // Selección extra «alfa» (0..4) y cursor extra en 10 (entre «bravo» y «charlie»).
        extra.add_selection(&buf, &buf.iter_at_offset(0), &buf.iter_at_offset(4));
        extra.add_cursor(&buf, &buf.iter_at_offset(10));
        // Principal: selección «charlie».
        buf.select_range(&buf.iter_at_offset(11), &buf.iter_at_offset(18));
        editar_tanda(&buf, &extra, &EditKind::Insert("X".into()));
        assert_eq!(texto(&buf), "X bravoX X");
        // Backspace: cada cursor borra la «X» que acaba de insertar (de atrás hacia adelante).
        editar_tanda(&buf, &extra, &EditKind::Backspace);
        assert_eq!(texto(&buf), " bravo ");
    }

    /// T006: quitar con Alt+clic (remove_at) y límite de 1000.
    #[test]
    #[ignore = "requiere acceso GTK"]
    fn us6_cursors_remove_and_limit() {
        gtk::init().expect("se necesita acceso GTK");
        let buf = sourceview5::Buffer::new(None);
        buf.set_text("abc");
        let mut extra = ExtraCursors::new();
        assert!(extra.add_cursor(&buf, &buf.iter_at_offset(1)));
        assert!(extra.remove_at(&buf, &buf.iter_at_offset(1)));
        assert!(extra.is_empty());
        assert!(!extra.remove_at(&buf, &buf.iter_at_offset(0)));
        // Límite.
        for n in 0..MAX_CURSORES {
            assert!(
                extra.add_cursor(&buf, &buf.iter_at_offset(0)),
                "falló en {n}"
            );
        }
        assert_eq!(extra.cursors.len(), MAX_CURSORES);
        assert!(!extra.add_cursor(&buf, &buf.iter_at_offset(0)));
        extra.clear(&buf);
        assert!(extra.is_empty());
    }

    /// T007: columna clampeada al final de líneas cortas y bordes de nota.
    #[test]
    #[ignore = "requiere acceso GTK"]
    fn us6_column_clamp_and_edges() {
        gtk::init().expect("se necesita acceso GTK");
        let buf = sourceview5::Buffer::new(None);
        buf.set_text("larga-abc\nxy\n\ncorta");
        // Columna 8 sobre «xy» (línea 1, largo 2): cae al final de la línea.
        let base = buf.iter_at_offset(8); // «c» de larga-abc
        let destino = iter_columna_adyacente(&buf, &base, 1).expect("hay línea 1");
        assert_eq!(destino.line(), 1);
        assert_eq!(destino.line_index(), 2, "clampeado al final de «xy»");
        // Línea vacía (2): columna 0.
        let destino = iter_columna_adyacente(&buf, &destino, 1).expect("hay línea 2");
        assert_eq!(destino.line(), 2);
        assert_eq!(destino.line_index(), 0);
        // Borde superior: desde la línea 0 hacia arriba no hay nada.
        let base = buf.iter_at_offset(0);
        assert!(iter_columna_adyacente(&buf, &base, -1).is_none());
        // Borde inferior: desde la última línea hacia abajo no hay nada.
        let base = buf.iter_at_offset(18);
        assert!(iter_columna_adyacente(&buf, &base, 1).is_none());
    }

    /// T003: los rectángulos para el overlay salen dentro de la vista y reflejan los cursores.
    #[test]
    #[ignore = "requiere acceso GTK"]
    fn us6_rects_for_overlay() {
        gtk::init().expect("se necesita acceso GTK");
        let buf = sourceview5::Buffer::new(None);
        buf.set_text("uno\ndos");
        let view = gtk::TextView::with_buffer(&buf);
        let mut extra = ExtraCursors::new();
        extra.add_cursor(&buf, &buf.iter_at_offset(3));
        extra.add_selection(&buf, &buf.iter_at_offset(4), &buf.iter_at_offset(7));
        let rects = rects_para_dibujar(&view, &buf, &extra);
        assert_eq!(rects.barras.len(), 2);
        assert_eq!(rects.selecciones.len(), 1);
        for (x, y, h) in &rects.barras {
            assert!(*x >= 0.0 && *y >= 0.0 && *h > 0.0);
        }
        let (_, _, w, h) = rects.selecciones[0];
        assert!(w > 0.0 && h > 0.0);
    }

    /// Regla de CLAUDE.md: la vista de Código es hija directa de su `ScrolledWindow`. Envuelta en
    /// otro widget (como pasó con la capa de cursores de la 006), GTK dispone el documento entero en
    /// cada tecla: 8,6 ms con 100 KB contra 2,4 ms.
    #[test]
    #[ignore = "requiere acceso GTK"]
    fn vista_de_codigo_directa_en_su_scroller() {
        gtk::init().expect("se necesita acceso GTK");
        let editor = super::super::Editor::new();
        let padre = editor.0.view.parent().expect("la vista tiene padre");
        assert!(
            padre.is::<gtk::ScrolledWindow>(),
            "la vista de Código cuelga de {}, no de un ScrolledWindow",
            padre.type_().name()
        );
    }

    /// La capa de los cursores va afuera del scroller (la vista sigue siendo hija directa del
    /// `ScrolledWindow`): con la nota desplazada, un cursor de la línea de arriba de lo visible
    /// queda arriba de la capa, no a la altura que tiene en el documento.
    #[test]
    #[ignore = "requiere acceso GTK"]
    fn us6_capa_sigue_al_desplazamiento() {
        gtk::init().expect("se necesita acceso GTK");
        let buf = sourceview5::Buffer::new(None);
        let texto: String = (0..400).map(|n| format!("línea {n}\n")).collect();
        buf.set_text(&texto);
        let view = gtk::TextView::with_buffer(&buf);
        let scroller = gtk::ScrolledWindow::builder().child(&view).build();
        let capa = gtk::Overlay::new();
        capa.set_child(Some(&scroller));
        let ventana = gtk::Window::builder()
            .default_width(500)
            .default_height(300)
            .child(&capa)
            .build();
        ventana.present();
        let ctx = glib::MainContext::default();
        let pasar = || {
            for _ in 0..40 {
                ctx.iteration(false);
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
        };
        pasar();
        let linea = buf.iter_at_line(300).unwrap();
        // `scroll_to_iter` valida el alto de las líneas hasta ahí (el `TextView` lo calcula de a
        // poco; en Xvfb, `set_value` quedaba limitado por el alto calculado hasta ese momento).
        let mut it = linea;
        for _ in 0..50 {
            view.scroll_to_iter(&mut it, 0.0, true, 0.0, 0.0);
            pasar();
            if view.visible_rect().y() > 1000 {
                break;
            }
        }
        let visible = f64::from(view.visible_rect().y());
        assert!(visible > 1000.0, "la nota se desplazó lejos: {visible}");
        let y_doc = f64::from(view.iter_location(&linea).y());
        let mut extra = ExtraCursors::new();
        extra.add_cursor(&buf, &linea);
        let rects = a_capa(rects_para_dibujar(&view, &buf, &extra), &view, &capa);
        let (_, y, _) = rects.barras[0];
        // En la capa, el cursor está donde está en el documento menos lo desplazado (más el
        // margen de la vista), no a la altura que tiene en el documento.
        let esperado = y_doc - visible;
        assert!(
            (y - esperado).abs() < 40.0,
            "el cursor queda en y = {y}; se esperaba ≈ {esperado} (documento {y_doc}, visible desde {visible})"
        );
        ventana.destroy();
    }
}

/// SC-1: costo por tecla con 0, 100 y 1000 cursores extra sobre 100 KiB. Medir con
/// `cargo test --release --bin orbital sc1_cursor_latency -- --ignored --nocapture`.
#[test]
#[ignore]
fn sc1_cursor_latency() {
    gtk::init().expect("se necesita acceso GTK");
    let parrafo = "Relleno del párrafo para medir la edición con varios cursores abiertos.\n\n";
    let text = parrafo.repeat((100 * 1024usize).div_ceil(parrafo.len()));
    for &n in &[0usize, 100, 1000] {
        let buf = sourceview5::Buffer::new(None);
        buf.set_text(&text);
        let mut extra = ExtraCursors::new();
        let paso = (text.len() as i32 / (n.max(1) as i32)).max(1);
        for k in 0..n {
            let it = buf.iter_at_offset(((k as i32 + 1) * paso).clamp(0, text.len() as i32));
            extra.add_cursor(&buf, &it);
        }
        // 20 teclas con la tanda completa; medir el promedio por tecla.
        let t = std::time::Instant::now();
        for _ in 0..20 {
            editar_tanda(&buf, &extra, &EditKind::Insert("x".into()));
        }
        let por_tecla = t.elapsed() / 20;
        println!(
            "cursores extra: {n:>4} | por tecla: {:?} | teclas en sub-ms: {}",
            por_tecla,
            por_tecla.as_millis() < 1
        );
        if n > 0 {
            extra.clear(&buf);
        }
    }
}
