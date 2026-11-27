//! Tablas avanzadas en Código (064): navegación con Tab/Shift+Tab/Enter, barra contextual
//! discreta (visible solo con el cursor dentro de una tabla), operaciones de estructura, orden,
//! CSV y evaluación de fórmulas `<!-- TBLFM: ... -->`.

use std::rc::Weak;

use gtk::prelude::*;
use orbital_vault::tablas::{self, Alineacion, EdicionTabla, TablaEnTexto};

use super::*;

/// Barra contextual que aparece arriba del editor únicamente cuando el cursor está dentro de una
/// tabla Markdown (en Código o Dividida). Fuera de tablas queda oculta (`visible = false`).
pub(super) struct BarraTabla {
    caja: gtk::Box,
}

impl BarraTabla {
    pub(super) fn new(editor: Weak<Inner>) -> Self {
        let caja = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(2)
            .margin_start(8)
            .margin_end(8)
            .margin_top(4)
            .margin_bottom(4)
            .visible(false)
            .css_classes(["toolbar"])
            .build();
        caja.update_property(&[gtk::accessible::Property::Label("Herramientas de tabla")]);

        let boton = |rotulo: &str, tooltip: &str, f: fn(&Editor)| -> gtk::Button {
            let b = gtk::Button::builder()
                .label(rotulo)
                .tooltip_text(tooltip)
                .css_classes(["flat", "caption"])
                .focusable(false)
                .build();
            b.update_property(&[gtk::accessible::Property::Label(tooltip)]);
            let weak = editor.clone();
            b.connect_clicked(move |_| {
                if let Some(inner) = weak.upgrade() {
                    f(&Editor(inner));
                }
            });
            b
        };

        caja.append(&boton(
            "Formatear",
            "Alinear columnas de la tabla",
            Editor::tabla_formatear,
        ));
        caja.append(&gtk::Separator::new(gtk::Orientation::Vertical));
        caja.append(&boton(
            "+Fila",
            "Insertar fila debajo",
            Editor::tabla_insertar_fila_abajo,
        ));
        caja.append(&boton(
            "−Fila",
            "Borrar fila actual",
            Editor::tabla_borrar_fila,
        ));
        caja.append(&boton(
            "↑F",
            "Mover fila arriba",
            Editor::tabla_mover_fila_arriba,
        ));
        caja.append(&boton(
            "↓F",
            "Mover fila abajo",
            Editor::tabla_mover_fila_abajo,
        ));
        caja.append(&gtk::Separator::new(gtk::Orientation::Vertical));
        caja.append(&boton(
            "+Col",
            "Insertar columna a la derecha",
            Editor::tabla_insertar_col_der,
        ));
        caja.append(&boton(
            "−Col",
            "Borrar columna actual",
            Editor::tabla_borrar_col,
        ));
        caja.append(&boton(
            "←C",
            "Mover columna a la izquierda",
            Editor::tabla_mover_col_izq,
        ));
        caja.append(&boton(
            "→C",
            "Mover columna a la derecha",
            Editor::tabla_mover_col_der,
        ));
        caja.append(&gtk::Separator::new(gtk::Orientation::Vertical));
        caja.append(&boton(
            "Izq",
            "Alinear columna a la izquierda",
            Editor::tabla_alinear_izq,
        ));
        caja.append(&boton(
            "Cen",
            "Centrar columna",
            Editor::tabla_alinear_centro,
        ));
        caja.append(&boton(
            "Der",
            "Alinear columna a la derecha",
            Editor::tabla_alinear_der,
        ));
        caja.append(&gtk::Separator::new(gtk::Orientation::Vertical));
        caja.append(&boton(
            "A→Z",
            "Ordenar columna ascendente",
            Editor::tabla_ordenar_asc,
        ));
        caja.append(&boton(
            "Z→A",
            "Ordenar columna descendente",
            Editor::tabla_ordenar_desc,
        ));
        caja.append(&gtk::Separator::new(gtk::Orientation::Vertical));
        caja.append(&boton(
            "ƒ(x)",
            "Recalcular fórmulas TBLFM",
            Editor::tabla_recalcular_formulas,
        ));
        caja.append(&boton(
            "CSV",
            "Copiar tabla como CSV",
            Editor::tabla_copiar_csv,
        ));

        Self { caja }
    }

    pub(super) fn widget(&self) -> &gtk::Box {
        &self.caja
    }

    pub(super) fn set_visible(&self, visible: bool) {
        if self.caja.is_visible() != visible {
            self.caja.set_visible(visible);
        }
    }
}

fn inicio_linea(buf: &sourceview5::Buffer, linea: usize) -> gtk::TextIter {
    let mut it = buf.start_iter();
    for _ in 0..linea {
        if !it.forward_line() {
            break;
        }
    }
    it
}

fn fin_linea(buf: &sourceview5::Buffer, linea: usize) -> gtk::TextIter {
    let mut it = inicio_linea(buf, linea);
    while !it.ends_line() && !it.is_end() {
        it.forward_char();
    }
    it
}

impl Editor {
    fn tabla_en_cursor(&self) -> Option<TablaEnTexto> {
        let i = &self.0;
        if i.format.get() != Format::Md || !matches!(i.mode.get(), View::Codigo | View::Dividida) {
            return None;
        }
        let buf = &i.buffer;
        let cur = buf.iter_at_mark(&buf.get_insert());
        let linea = cur.line().max(0) as usize;
        let col_char = cur.line_offset().max(0) as usize;
        let texto = buf.text(&buf.start_iter(), &buf.end_iter(), true);
        let lineas: Vec<&str> = texto.as_str().split('\n').collect();
        tablas::detectar_tabla(&lineas, linea, col_char)
    }

    /// Actualiza la visibilidad de la barra contextual de tabla según la posición del cursor.
    pub(super) fn actualizar_barra_tabla(&self) {
        let i = &self.0;
        if i.loading.get()
            || i.format.get() != Format::Md
            || !matches!(i.mode.get(), View::Codigo | View::Dividida)
        {
            i.barra_tabla.set_visible(false);
            return;
        }
        // Chequeo O(1) rápido: si la línea actual no tiene `|` ni `TBLFM`, salimos enseguida.
        let cur = i.buffer.iter_at_mark(&i.buffer.get_insert());
        let mut a = cur;
        a.set_line_offset(0);
        let mut b = cur;
        if !b.ends_line() {
            b.forward_to_line_end();
        }
        let linea = i.buffer.text(&a, &b, true);
        if !linea.contains('|') && !tablas::es_linea_formula(linea.as_str()) {
            i.barra_tabla.set_visible(false);
            return;
        }
        i.barra_tabla.set_visible(self.tabla_en_cursor().is_some());
    }

    fn aplicar_edicion_tabla(&self, t: &TablaEnTexto, ed: EdicionTabla) {
        let buf = &self.0.buffer;
        buf.begin_user_action();
        let mut a = inicio_linea(buf, t.linea_inicio);
        let mut b = fin_linea(buf, t.linea_fin_total);
        buf.delete(&mut a, &mut b);
        let mut en = inicio_linea(buf, t.linea_inicio);
        buf.insert(&mut en, &ed.texto);
        let mut dest = inicio_linea(buf, t.linea_inicio + ed.linea_rel);
        for _ in 0..ed.col_char {
            if dest.ends_line() || dest.is_end() {
                break;
            }
            dest.forward_char();
        }
        buf.place_cursor(&dest);
        buf.end_user_action();
        self.0.view.grab_focus();
    }

    /// Navegación con `Tab` / `Shift+Tab` dentro de una tabla. Devuelve `true` si la manejó.
    pub(super) fn tabla_tab(&self, adelante: bool) -> bool {
        let Some(mut t) = self.tabla_en_cursor() else {
            return false;
        };
        let (f, c) = (t.fila_cursor, t.col_cursor);
        let ed = t.tabla.navegar_tab(f, c, adelante);
        self.aplicar_edicion_tabla(&t, ed);
        true
    }

    /// Navegación con `Enter` dentro de una tabla. Devuelve `true` si la manejó.
    pub(super) fn tabla_enter(&self) -> bool {
        let Some(mut t) = self.tabla_en_cursor() else {
            return false;
        };
        let (f, c) = (t.fila_cursor, t.col_cursor);
        let ed = t.tabla.navegar_enter(f, c);
        self.aplicar_edicion_tabla(&t, ed);
        true
    }

    pub fn tabla_formatear(&self) {
        let Some(t) = self.tabla_en_cursor() else {
            self.error("Colocá el cursor dentro de una tabla Markdown");
            return;
        };
        let ed = t.tabla.edicion(t.fila_cursor, t.col_cursor);
        self.aplicar_edicion_tabla(&t, ed);
    }

    pub fn tabla_insertar_fila_abajo(&self) {
        let Some(mut t) = self.tabla_en_cursor() else {
            return;
        };
        let ed = t.tabla.insertar_fila(t.fila_cursor, true);
        self.aplicar_edicion_tabla(&t, ed);
    }

    pub fn tabla_borrar_fila(&self) {
        let Some(mut t) = self.tabla_en_cursor() else {
            return;
        };
        let ed = t.tabla.borrar_fila(t.fila_cursor);
        self.aplicar_edicion_tabla(&t, ed);
    }

    pub fn tabla_mover_fila_arriba(&self) {
        let Some(mut t) = self.tabla_en_cursor() else {
            return;
        };
        let ed = t.tabla.mover_fila(t.fila_cursor, true, t.col_cursor);
        self.aplicar_edicion_tabla(&t, ed);
    }

    pub fn tabla_mover_fila_abajo(&self) {
        let Some(mut t) = self.tabla_en_cursor() else {
            return;
        };
        let ed = t.tabla.mover_fila(t.fila_cursor, false, t.col_cursor);
        self.aplicar_edicion_tabla(&t, ed);
    }

    pub fn tabla_insertar_col_der(&self) {
        let Some(mut t) = self.tabla_en_cursor() else {
            return;
        };
        let ed = t.tabla.insertar_columna(t.col_cursor, true, t.fila_cursor);
        self.aplicar_edicion_tabla(&t, ed);
    }

    pub fn tabla_borrar_col(&self) {
        let Some(mut t) = self.tabla_en_cursor() else {
            return;
        };
        let ed = t.tabla.borrar_columna(t.col_cursor, t.fila_cursor);
        self.aplicar_edicion_tabla(&t, ed);
    }

    pub fn tabla_mover_col_izq(&self) {
        let Some(mut t) = self.tabla_en_cursor() else {
            return;
        };
        let ed = t.tabla.mover_columna(t.col_cursor, true, t.fila_cursor);
        self.aplicar_edicion_tabla(&t, ed);
    }

    pub fn tabla_mover_col_der(&self) {
        let Some(mut t) = self.tabla_en_cursor() else {
            return;
        };
        let ed = t.tabla.mover_columna(t.col_cursor, false, t.fila_cursor);
        self.aplicar_edicion_tabla(&t, ed);
    }

    pub fn tabla_alinear_izq(&self) {
        self.tabla_alinear(Alineacion::Izquierda);
    }

    pub fn tabla_alinear_centro(&self) {
        self.tabla_alinear(Alineacion::Centro);
    }

    pub fn tabla_alinear_der(&self) {
        self.tabla_alinear(Alineacion::Derecha);
    }

    fn tabla_alinear(&self, align: Alineacion) {
        let Some(mut t) = self.tabla_en_cursor() else {
            return;
        };
        let ed = t.tabla.alinear_columna(t.col_cursor, align, t.fila_cursor);
        self.aplicar_edicion_tabla(&t, ed);
    }

    pub fn tabla_ordenar_asc(&self) {
        self.tabla_ordenar(false);
    }

    pub fn tabla_ordenar_desc(&self) {
        self.tabla_ordenar(true);
    }

    fn tabla_ordenar(&self, desc: bool) {
        let Some(mut t) = self.tabla_en_cursor() else {
            self.error("Colocá el cursor dentro de una tabla Markdown");
            return;
        };
        let ed = t.tabla.ordenar_por_columna(t.col_cursor, desc);
        self.aplicar_edicion_tabla(&t, ed);
    }

    pub fn tabla_recalcular_formulas(&self) {
        let Some(mut t) = self.tabla_en_cursor() else {
            self.error("Colocá el cursor dentro de una tabla Markdown");
            return;
        };
        match t.tabla.evaluar_formulas() {
            Ok(0) => self.error("Esta tabla no tiene líneas <!-- TBLFM: ... -->"),
            Ok(_) => {
                let ed = t.tabla.edicion(t.fila_cursor, t.col_cursor);
                self.aplicar_edicion_tabla(&t, ed);
            }
            Err(e) => self.error(&e),
        }
    }

    /// Inserta una tabla Markdown 3×3 en la posición del cursor.
    pub fn tabla_insertar_nueva(&self) {
        let i = &self.0;
        if self.current_path().is_none() || i.format.get() != Format::Md {
            self.error("Abrí una nota .md en Código o Dividida para insertar una tabla");
            return;
        }
        let texto = tablas::nueva_tabla(3, 2).formatear();
        let buf = &i.buffer;
        buf.begin_user_action();
        let mut it = buf.iter_at_mark(&buf.get_insert());
        let prefijo = if it.line_offset() > 0 { "\n\n" } else { "" };
        buf.insert(&mut it, &format!("{prefijo}{texto}\n"));
        buf.end_user_action();
        i.view.grab_focus();
        self.actualizar_barra_tabla();
    }

    /// Copia la tabla bajo el cursor como CSV al portapapeles.
    pub fn tabla_copiar_csv(&self) {
        let Some(t) = self.tabla_en_cursor() else {
            self.error("Colocá el cursor dentro de una tabla Markdown");
            return;
        };
        let csv = t.tabla.a_csv();
        self.0.view.clipboard().set_text(&csv);
        self.error("Tabla copiada como CSV al portapapeles");
    }

    /// Pega texto CSV del portapapeles convertido en una tabla Markdown formateada.
    pub fn tabla_pegar_csv(&self) {
        let i = &self.0;
        if self.current_path().is_none() || i.format.get() != Format::Md {
            self.error("Abrí una nota .md en Código o Dividida para pegar CSV como tabla");
            return;
        }
        let this = self.clone();
        i.view
            .clipboard()
            .read_text_async(None::<&gio::Cancellable>, move |res| {
                let Ok(Some(txt)) = res else {
                    this.error("No hay texto CSV en el portapapeles");
                    return;
                };
                let Some(tabla) = tablas::csv_a_tabla(txt.as_str()) else {
                    this.error("El portapapeles no tiene datos CSV válidos");
                    return;
                };
                let formateada = tabla.formatear();
                let buf = &this.0.buffer;
                buf.begin_user_action();
                let mut it = buf.iter_at_mark(&buf.get_insert());
                let prefijo = if it.line_offset() > 0 { "\n\n" } else { "" };
                buf.insert(&mut it, &format!("{prefijo}{formateada}\n"));
                buf.end_user_action();
                this.0.view.grab_focus();
                this.actualizar_barra_tabla();
            });
    }
}
