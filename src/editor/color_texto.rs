//! Color rápido de texto y resaltado en Código (063): selector rápido (`Ctrl+Alt+C`),
//! resaltado rápido (`Ctrl+Alt+H`), quitar color y paleta personalizable en `estado.yaml`.

use std::cell::RefCell;
use std::rc::Rc;

use adw::prelude::*;
use orbital_vault::colores::{
    self, PALETA_BASE, alternar_resaltado, aplicar_estilo, nombre_color_valido, quitar_color,
};
use orbital_vault::iconos_catalogo::normalizar_color;

use crate::state::AppState;

use super::*;

impl Editor {
    /// Rango `(inicio, fin)` en offsets de caracteres sobre el que operan los comandos de color:
    /// si hay selección, esa selección; si no, el tramo `{spec:…}` o `==…==` bajo el cursor (o la
    /// palabra bajo el cursor).
    fn rango_color_en_cursor(&self) -> (i32, i32) {
        let buf = &self.0.buffer;
        if let Some((a, b)) = buf.selection_bounds() {
            return (a.offset(), b.offset());
        }
        let cursor = buf.iter_at_offset(buf.cursor_position());
        let mut line_start = cursor;
        line_start.set_line_offset(0);
        let mut line_end = cursor;
        if !line_end.ends_line() {
            line_end.forward_to_line_end();
        }
        let linea = buf.text(&line_start, &line_end, true);
        let linea_str = linea.as_str();
        let cur_char = (cursor.offset() - line_start.offset()).max(0) as usize;
        let cur_byte = linea_str
            .char_indices()
            .nth(cur_char)
            .map_or(linea_str.len(), |(b, _)| b);

        // 1. Buscar un tramo `{spec:…}` o `==…==` que contenga el cursor.
        if let Some((b0, b1)) = tramo_coloreado_en(linea_str, cur_byte) {
            let c0 = linea_str[..b0].chars().count() as i32;
            let c1 = linea_str[..b1].chars().count() as i32;
            return (line_start.offset() + c0, line_start.offset() + c1);
        }

        // 2. Si no, tomar la palabra bajo el cursor (si está dentro de una).
        let mut a = cursor;
        let mut b = cursor;
        if !a.starts_word() && a.inside_word() {
            a.backward_word_start();
        }
        if !b.ends_word() && b.inside_word() {
            b.forward_word_end();
        }
        (a.offset(), b.offset())
    }

    fn reemplazar_rango(&self, inicio: i32, fin: i32, nuevo: &str) {
        let buf = &self.0.buffer;
        buf.begin_user_action();
        let mut a = buf.iter_at_offset(inicio);
        let mut b = buf.iter_at_offset(fin);
        buf.delete(&mut a, &mut b);
        let mut en = buf.iter_at_offset(inicio);
        buf.insert(&mut en, nuevo);
        let sel_inicio = buf.iter_at_offset(inicio);
        let sel_fin = buf.iter_at_offset(inicio + nuevo.chars().count() as i32);
        buf.select_range(&sel_inicio, &sel_fin);
        buf.end_user_action();
        self.0.view.grab_focus();
    }

    fn motivo_sin_color(&self) -> Option<&'static str> {
        let i = &self.0;
        if self.current_path().is_none() {
            return Some("Abrí una nota para colorear texto");
        }
        if i.format.get() == Format::Html {
            return Some("En notas .html usá el botón Colores de la vista Visual");
        }
        if !matches!(i.mode.get(), View::Codigo | View::Dividida) {
            return Some("El color rápido en Markdown se aplica desde Código o Dividida");
        }
        None
    }

    /// Aplica un color de texto (`fg`) y/o fondo (`bg`) sobre la selección o el cursor.
    pub fn aplicar_color_texto(&self, fg: Option<&str>, bg: Option<&str>) {
        if let Some(m) = self.motivo_sin_color() {
            self.error(m);
            return;
        }
        let (inicio, fin) = self.rango_color_en_cursor();
        let buf = &self.0.buffer;
        let a = buf.iter_at_offset(inicio);
        let b = buf.iter_at_offset(fin);
        let texto = buf.text(&a, &b, true);
        let resultado = aplicar_estilo(texto.as_str(), fg, bg);
        self.reemplazar_rango(inicio, fin, &resultado);
    }

    /// Alterna el resaltado rápido `==texto==` (`Ctrl+Alt+H`).
    pub fn resaltar_texto_rapido(&self) {
        if let Some(m) = self.motivo_sin_color() {
            self.error(m);
            return;
        }
        let (inicio, fin) = self.rango_color_en_cursor();
        let buf = &self.0.buffer;
        let a = buf.iter_at_offset(inicio);
        let b = buf.iter_at_offset(fin);
        let texto = buf.text(&a, &b, true);
        let resultado = alternar_resaltado(texto.as_str());
        self.reemplazar_rango(inicio, fin, &resultado);
    }

    /// Quita `{color:…}` o `==…==` de la selección o el tramo bajo el cursor.
    pub fn quitar_color_texto(&self) {
        if let Some(m) = self.motivo_sin_color() {
            self.error(m);
            return;
        }
        let (inicio, fin) = self.rango_color_en_cursor();
        let buf = &self.0.buffer;
        let a = buf.iter_at_offset(inicio);
        let b = buf.iter_at_offset(fin);
        let texto = buf.text(&a, &b, true);
        let resultado = quitar_color(texto.as_str());
        if resultado != texto.as_str() {
            self.reemplazar_rango(inicio, fin, &resultado);
        }
    }

    /// Actualiza los colores propios en la vista Lectura y fuerza su redibujo.
    pub fn actualizar_colores_propios(&self, colores: Vec<(String, String)>) {
        self.0.reader.set_colores_propios(colores);
        if matches!(self.0.mode.get(), View::Lectura | View::Dividida) {
            self.render_now();
        }
    }

    /// Abre el selector rápido de color de texto y resaltado (`Ctrl+Alt+C`).
    pub fn abrir_selector_color(&self) {
        if let Some(m) = self.motivo_sin_color() {
            self.error(m);
            return;
        }
        let dialog = adw::Dialog::builder()
            .title("Color y resaltado de texto")
            .content_width(480)
            .content_height(460)
            .build();

        let propios = Rc::new(RefCell::new(AppState::load().text_colors));

        let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
        let header = adw::HeaderBar::new();
        root.append(&header);

        let cuerpo = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(12)
            .margin_start(16)
            .margin_end(16)
            .margin_top(12)
            .margin_bottom(14)
            .build();

        // --- Sección 1: Color de texto ---
        cuerpo.append(
            &gtk::Label::builder()
                .label("Color de texto  {color:texto}")
                .xalign(0.0)
                .css_classes(["heading"])
                .build(),
        );
        let fg_flow = gtk::FlowBox::builder()
            .selection_mode(gtk::SelectionMode::None)
            .min_children_per_line(5)
            .max_children_per_line(5)
            .row_spacing(6)
            .column_spacing(6)
            .build();
        for c in PALETA_BASE {
            let b = boton_color(c.nombre, c.fg, false);
            let ed = self.clone();
            let d = dialog.clone();
            let clave = c.clave;
            b.connect_clicked(move |_| {
                d.close();
                ed.aplicar_color_texto(Some(clave), None);
            });
            fg_flow.insert(&b, -1);
        }
        cuerpo.append(&fg_flow);

        // Hex rápido para texto.
        let hex_fg_row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        let hex_fg_entry = gtk::Entry::builder()
            .placeholder_text("#rrggbb (color de texto)")
            .hexpand(true)
            .build();
        hex_fg_entry.update_property(&[gtk::accessible::Property::Label(
            "Color hexadecimal de texto",
        )]);
        let btn_hex_fg = gtk::Button::with_label("Aplicar");
        {
            let e = hex_fg_entry.clone();
            let ed = self.clone();
            let d = dialog.clone();
            btn_hex_fg.connect_clicked(move |_| {
                if let Some(norm) = normalizar_color(&e.text()) {
                    d.close();
                    ed.aplicar_color_texto(Some(&norm), None);
                }
            });
        }
        hex_fg_row.append(&hex_fg_entry);
        hex_fg_row.append(&btn_hex_fg);
        cuerpo.append(&hex_fg_row);

        // --- Sección 2: Resaltado / fondo ---
        cuerpo.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
        let bg_head = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        bg_head.append(
            &gtk::Label::builder()
                .label("Resaltado  {fondo-color:texto}")
                .xalign(0.0)
                .hexpand(true)
                .css_classes(["heading"])
                .build(),
        );
        let btn_rapido = gtk::Button::builder()
            .label("==Resaltado rápido==")
            .css_classes(["suggested-action"])
            .build();
        {
            let ed = self.clone();
            let d = dialog.clone();
            btn_rapido.connect_clicked(move |_| {
                d.close();
                ed.resaltar_texto_rapido();
            });
        }
        bg_head.append(&btn_rapido);
        cuerpo.append(&bg_head);

        let bg_flow = gtk::FlowBox::builder()
            .selection_mode(gtk::SelectionMode::None)
            .min_children_per_line(5)
            .max_children_per_line(5)
            .row_spacing(6)
            .column_spacing(6)
            .build();
        for c in PALETA_BASE {
            let b = boton_color(c.nombre, c.fg, true);
            let ed = self.clone();
            let d = dialog.clone();
            let clave = c.clave;
            b.connect_clicked(move |_| {
                d.close();
                ed.aplicar_color_texto(None, Some(clave));
            });
            bg_flow.insert(&b, -1);
        }
        cuerpo.append(&bg_flow);

        // --- Sección 3: Paleta propia (Mis colores) ---
        cuerpo.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
        cuerpo.append(
            &gtk::Label::builder()
                .label("Mis colores personalizados")
                .xalign(0.0)
                .css_classes(["heading"])
                .build(),
        );
        let propios_box = gtk::Box::new(gtk::Orientation::Vertical, 4);
        cuerpo.append(&propios_box);

        type RepintarFn = Rc<RefCell<Option<Rc<dyn Fn()>>>>;
        let repintar: RepintarFn = Rc::new(RefCell::new(None));
        let repintar_fn: Rc<dyn Fn()> = {
            let propios_box = propios_box.clone();
            let propios = propios.clone();
            let ed = self.clone();
            let d = dialog.clone();
            let repintar = repintar.clone();
            Rc::new(move || {
                while let Some(c) = propios_box.first_child() {
                    propios_box.remove(&c);
                }
                let lista = propios.borrow().clone();
                if lista.is_empty() {
                    propios_box.append(
                        &gtk::Label::builder()
                            .label("Todavía no creaste colores propios. Agregá uno abajo.")
                            .xalign(0.0)
                            .css_classes(["dim-label", "caption"])
                            .build(),
                    );
                    return;
                }
                for (nombre, hex) in lista {
                    let fila = gtk::Box::new(gtk::Orientation::Horizontal, 6);
                    let lbl = gtk::Label::new(None);
                    lbl.set_markup(&format!(
                        "<span foreground=\"{hex}\">●</span>  <b>{nombre}</b>  <span alpha=\"70%\">{hex}</span>"
                    ));
                    lbl.set_xalign(0.0);
                    lbl.set_hexpand(true);
                    fila.append(&lbl);

                    let b_fg = gtk::Button::builder()
                        .label("Texto")
                        .css_classes(["flat"])
                        .build();
                    {
                        let ed = ed.clone();
                        let d = d.clone();
                        let n = nombre.clone();
                        b_fg.connect_clicked(move |_| {
                            d.close();
                            ed.aplicar_color_texto(Some(&n), None);
                        });
                    }
                    let b_bg = gtk::Button::builder()
                        .label("Fondo")
                        .css_classes(["flat"])
                        .build();
                    {
                        let ed = ed.clone();
                        let d = d.clone();
                        let n = nombre.clone();
                        b_bg.connect_clicked(move |_| {
                            d.close();
                            ed.aplicar_color_texto(None, Some(&n));
                        });
                    }
                    let b_del = gtk::Button::builder()
                        .icon_name("user-trash-symbolic")
                        .css_classes(["flat"])
                        .tooltip_text("Borrar color")
                        .build();
                    b_del.update_property(&[gtk::accessible::Property::Label(&format!(
                        "Borrar color {nombre}"
                    ))]);
                    {
                        let propios = propios.clone();
                        let ed = ed.clone();
                        let repintar = repintar.clone();
                        let n = nombre.clone();
                        b_del.connect_clicked(move |_| {
                            propios.borrow_mut().retain(|(k, _)| k != &n);
                            let copia = propios.borrow().clone();
                            guardar_colores_propios(&ed, copia);
                            if let Some(f) = repintar.borrow().as_ref() {
                                f();
                            }
                        });
                    }
                    fila.append(&b_fg);
                    fila.append(&b_bg);
                    fila.append(&b_del);
                    propios_box.append(&fila);
                }
            })
        };
        *repintar.borrow_mut() = Some(repintar_fn.clone());
        repintar_fn();

        // Formulario para agregar un color propio.
        let add_row = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        let nuevo_nombre = gtk::Entry::builder()
            .placeholder_text("nombre (ej. menta)")
            .hexpand(true)
            .build();
        nuevo_nombre
            .update_property(&[gtk::accessible::Property::Label("Nombre del color propio")]);
        let nuevo_hex = gtk::Entry::builder()
            .placeholder_text("#88ffcc")
            .width_chars(9)
            .max_length(7)
            .build();
        nuevo_hex.update_property(&[gtk::accessible::Property::Label("Código hexadecimal")]);
        let btn_add = gtk::Button::with_label("Guardar color");
        {
            let nuevo_nombre = nuevo_nombre.clone();
            let nuevo_hex = nuevo_hex.clone();
            let propios = propios.clone();
            let ed = self.clone();
            let repintar_fn = repintar_fn.clone();
            btn_add.connect_clicked(move |_| {
                let n = nuevo_nombre.text().trim().to_ascii_lowercase();
                let Some(h) = normalizar_color(&nuevo_hex.text()) else {
                    return;
                };
                if !nombre_color_valido(&n) {
                    return;
                }
                {
                    let mut v = propios.borrow_mut();
                    if let Some(slot) = v.iter_mut().find(|(k, _)| k == &n) {
                        slot.1 = h;
                    } else {
                        v.push((n, h));
                    }
                }
                nuevo_nombre.set_text("");
                nuevo_hex.set_text("");
                let copia = propios.borrow().clone();
                guardar_colores_propios(&ed, copia);
                repintar_fn();
            });
        }
        add_row.append(&nuevo_nombre);
        add_row.append(&nuevo_hex);
        add_row.append(&btn_add);
        cuerpo.append(&add_row);

        // Pie: Quitar color.
        let pie = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        let btn_quitar = gtk::Button::builder()
            .label("Quitar color")
            .css_classes(["destructive-action"])
            .build();
        {
            let ed = self.clone();
            let d = dialog.clone();
            btn_quitar.connect_clicked(move |_| {
                d.close();
                ed.quitar_color_texto();
            });
        }
        pie.append(&btn_quitar);
        cuerpo.append(&pie);

        let scroll = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vexpand(true)
            .child(&cuerpo)
            .build();
        root.append(&scroll);
        dialog.set_child(Some(&root));
        dialog.present(Some(&self.0.view));
    }
}

fn boton_color(nombre: &str, hex: &str, es_fondo: bool) -> gtk::Button {
    let marca = if es_fondo { "■" } else { "●" };
    let lbl = gtk::Label::new(None);
    lbl.set_markup(&format!(
        "<span foreground=\"{hex}\">{marca}</span> {nombre}"
    ));
    let b = gtk::Button::builder()
        .child(&lbl)
        .css_classes(["flat"])
        .build();
    b.update_property(&[gtk::accessible::Property::Label(&format!(
        "{} {nombre}",
        if es_fondo { "Fondo" } else { "Color" }
    ))]);
    b
}

fn guardar_colores_propios(editor: &Editor, colores: Vec<(String, String)>) {
    editor.actualizar_colores_propios(colores.clone());
    gio::spawn_blocking(move || {
        let mut st = AppState::load();
        st.text_colors = colores;
        let _ = st.save();
    });
}

/// Busca el rango en bytes `(inicio, fin)` del tramo `{spec:…}` o `==…==` que envuelve a `pos`
/// dentro de una línea.
fn tramo_coloreado_en(linea: &str, pos: usize) -> Option<(usize, usize)> {
    // 1. Buscar `{spec:…}` que incluya `pos`.
    let bytes = linea.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'{'
            && let Some(c_rel) = linea[i + 1..].find(':')
        {
            let colon = i + 1 + c_rel;
            let spec = &linea[i + 1..colon];
            if !spec.contains('}')
                && colores::parse_spec(spec).is_some()
                && let Some(end_rel) = linea[colon + 1..].find('}')
            {
                let end = colon + 1 + end_rel + 1;
                if pos >= i && pos <= end {
                    return Some((i, end));
                }
                i = end;
                continue;
            }
        }
        if i + 4 <= bytes.len()
            && bytes[i] == b'='
            && bytes[i + 1] == b'='
            && let Some(end_rel) = linea[i + 2..].find("==")
        {
            let end = i + 2 + end_rel + 2;
            if pos >= i && pos <= end {
                return Some((i, end));
            }
            i = end;
            continue;
        }
        i += 1;
    }
    None
}
