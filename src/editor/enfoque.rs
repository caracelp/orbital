//! Enfoque de un elemento del outliner y miga de pan (028, T030–T031).
//!
//! Muestra solo el elemento elegido con su contenido (ocultando el resto con la etiqueta
//! `fuera-de-enfoque`, tanto en Código como en Lectura) y coloca una miga de pan arriba del
//! editor para subir de nivel o salir del enfoque. El enfoque no se recuerda entre notas ni entre
//! sesiones.

use super::*;
use orbital_vault::Plegable;
use orbital_vault::plegado as nucleo;
use std::ops::Range;

/// Barra de miga de pan que aparece arriba del editor mientras hay un elemento enfocado.
pub(super) struct Migas {
    root: gtk::Box,
    ruta: gtk::Box,
}

impl Migas {
    pub(super) fn new(editor_weak: std::rc::Weak<Inner>) -> Self {
        let root = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        root.add_css_class("toolbar");
        root.set_margin_start(8);
        root.set_margin_end(8);
        root.set_margin_top(2);
        root.set_margin_bottom(2);
        root.set_visible(false);
        root.set_accessible_role(gtk::AccessibleRole::Group);
        root.update_property(&[gtk::accessible::Property::Label("Miga de pan del enfoque")]);

        let ruta = gtk::Box::new(gtk::Orientation::Horizontal, 2);
        ruta.set_hexpand(true);
        root.append(&ruta);

        let salir = gtk::Button::from_icon_name("window-close-symbolic");
        salir.add_css_class("flat");
        salir.set_tooltip_text(Some("Salir del enfoque"));
        salir.update_property(&[gtk::accessible::Property::Label("Salir del enfoque")]);
        salir.connect_clicked(move |_| {
            if let Some(i) = editor_weak.upgrade() {
                Editor(i).salir_enfoque();
            }
        });
        root.append(&salir);

        Self { root, ruta }
    }

    pub(super) fn widget(&self) -> &gtk::Box {
        &self.root
    }

    pub(super) fn ocultar(&self) {
        self.limpiar();
        self.root.set_visible(false);
    }

    fn limpiar(&self) {
        while let Some(child) = self.ruta.first_child() {
            self.ruta.remove(&child);
        }
    }

    /// Reconstruye los niveles de la miga: nota › ancestros › elemento actual.
    pub(super) fn actualizar(
        &self,
        editor_weak: &std::rc::Weak<Inner>,
        nombre_nota: &str,
        texto: &str,
        ps: &[Plegable],
        indice: u32,
    ) {
        self.limpiar();

        let btn_nota = Self::boton_nivel(nombre_nota);
        let w = editor_weak.clone();
        btn_nota.connect_clicked(move |_| {
            if let Some(i) = w.upgrade() {
                Editor(i).salir_enfoque();
            }
        });
        self.ruta.append(&btn_nota);

        for anc in nucleo::camino(ps, indice) {
            self.ruta.append(&Self::separador());
            let p = &ps[anc as usize];
            let etiqueta = Self::titulo_elemento(texto, p);
            let btn = Self::boton_nivel(&etiqueta);
            let linea = p.linea;
            let w = editor_weak.clone();
            btn.connect_clicked(move |_| {
                if let Some(i) = w.upgrade() {
                    Editor(i).enfocar(linea);
                }
            });
            self.ruta.append(&btn);
        }

        self.ruta.append(&Self::separador());
        let actual = Self::titulo_elemento(texto, &ps[indice as usize]);
        let lbl = gtk::Label::new(Some(&actual));
        lbl.set_ellipsize(gtk::pango::EllipsizeMode::End);
        lbl.set_max_width_chars(32);
        lbl.set_tooltip_text(Some(&actual));
        lbl.set_margin_start(4);
        self.ruta.append(&lbl);

        self.root.set_visible(true);
    }

    fn boton_nivel(texto: &str) -> gtk::Button {
        let lbl = gtk::Label::new(Some(texto));
        lbl.set_ellipsize(gtk::pango::EllipsizeMode::End);
        lbl.set_max_width_chars(24);
        let btn = gtk::Button::new();
        btn.set_child(Some(&lbl));
        btn.add_css_class("flat");
        btn.set_tooltip_text(Some(texto));
        btn
    }

    fn separador() -> gtk::Label {
        let sep = gtk::Label::new(Some("›"));
        sep.add_css_class("dim-label");
        sep.set_accessible_role(gtk::AccessibleRole::Presentation);
        sep
    }

    fn titulo_elemento(texto: &str, p: &Plegable) -> String {
        let s = texto.get(p.texto.clone()).unwrap_or("").trim();
        if s.is_empty() {
            format!("Línea {}", p.linea + 1)
        } else {
            s.to_owned()
        }
    }
}

impl Editor {
    /// Hay un elemento enfocado en el editor.
    pub fn esta_enfocado(&self) -> bool {
        self.0.enfoque.borrow().is_some()
    }

    /// Avisa cuando entra o sale del enfoque (para habilitar `win.salir-enfoque` y el menú).
    pub fn set_on_enfoque(&self, f: impl Fn() + 'static) {
        *self.0.on_enfoque.borrow_mut() = Some(Box::new(f));
    }

    fn avisar_enfoque(&self) {
        if let Some(f) = self.0.on_enfoque.borrow().as_ref() {
            f();
        }
    }

    /// Rango de líneas `linea..=ultima` del elemento enfocado, si hay uno válido.
    pub(super) fn rango_enfocado(&self) -> Option<Range<u32>> {
        let linea = self.linea_enfocada()?;
        let (_, ps) = self.arbol();
        let idx = nucleo::en_linea(&ps, linea)?;
        let p = &ps[idx as usize];
        Some(p.linea..p.ultima + 1)
    }

    fn linea_enfocada(&self) -> Option<u32> {
        let m = self.0.enfoque.borrow().clone()?;
        Some(self.0.buffer.iter_at_mark(&m).line() as u32)
    }

    /// Enfoca el elemento plegable de `linea` o, si no es plegable, el más cercano que lo
    /// contiene. Devuelve `false` si no hay ninguno o si la nota no admite plegado.
    pub fn enfocar(&self, linea: u32) -> bool {
        if !self.plegado_disponible() {
            return false;
        }
        let (texto, ps) = self.arbol();
        let idx =
            nucleo::en_linea(&ps, linea).or_else(|| nucleo::contienen(&ps, linea).last().copied());
        let Some(idx) = idx else {
            return false;
        };
        let p = &ps[idx as usize];
        let linea_elem = p.linea;
        let ultima_elem = p.ultima;

        // Si el propio elemento estaba plegado, se despliega para mostrar su contenido al enfocar.
        if self.esta_plegado(linea_elem) {
            self.desplegar(linea_elem);
        }

        let i = &self.0;
        let buf = &i.buffer;
        // Mover el cursor adentro si quedó fuera del elemento enfocado.
        let cursor_linea = buf.iter_at_mark(&buf.get_insert()).line() as u32;
        if (cursor_linea < linea_elem || cursor_linea > ultima_elem)
            && let Some(it) = buf.iter_at_line(linea_elem as i32)
        {
            self.sin_desplegar(|| buf.place_cursor(&it));
        }
        // Quitar cursores extra fuera del elemento enfocado.
        let extra_fuera: Vec<gtk::TextIter> = i
            .extra
            .borrow()
            .inserciones(buf)
            .filter(|it| {
                let l = it.line() as u32;
                l < linea_elem || l > ultima_elem
            })
            .collect();
        if !extra_fuera.is_empty() {
            let mut extra = i.extra.borrow_mut();
            for it in &extra_fuera {
                extra.remove_at(buf, it);
            }
            drop(extra);
            self.actualizar_carenes();
        }

        let vieja = i.enfoque.borrow_mut().take();
        if let Some(m) = vieja {
            buf.delete_mark(&m);
        }
        if let Some(it) = buf.iter_at_line(linea_elem as i32) {
            let m = buf.create_mark(None, &it, false);
            *i.enfoque.borrow_mut() = Some(m);
        }

        let total = buf.line_count() as u32;
        let fuera = nucleo::fuera_de(&ps, idx, total);
        plegado::aplicar_ocultas(buf, &i.tag_fuera, &fuera);

        let nombre_nota = self
            .current_path()
            .and_then(|r| {
                r.as_str()
                    .rsplit('/')
                    .next()
                    .map(|s| s.strip_suffix(".md").unwrap_or(s).to_owned())
            })
            .unwrap_or_else(|| "Nota".to_owned());
        i.migas
            .actualizar(&Rc::downgrade(i), &nombre_nota, &texto, &ps, idx);
        i.reader.set_enfoque(Some((linea_elem, ultima_elem)));
        self.avisar_enfoque();
        true
    }

    /// Enfoca el elemento de la línea del cursor (en Código y Dividida).
    pub fn enfocar_en_cursor(&self) -> bool {
        if !self.plegado_disponible() {
            return false;
        }
        let b = &self.0.buffer;
        let linea = b.iter_at_mark(&b.get_insert()).line() as u32;
        self.enfocar(linea)
    }

    /// Enfoca en Lectura: la flechita con el foco o la sección de lo primero visible (decisión 11).
    pub fn enfocar_en_lectura(&self) -> bool {
        if !self.plegado_disponible() {
            return false;
        }
        if let Some(linea) = self.0.reader.linea_para_enfocar() {
            return self.enfocar(linea);
        }
        false
    }

    /// Sube un nivel en la jerarquía del enfoque o sale si está en el primer nivel.
    pub fn subir_enfoque(&self) -> bool {
        let Some(linea) = self.linea_enfocada() else {
            return false;
        };
        let (_, ps) = self.arbol();
        if let Some(idx) = nucleo::en_linea(&ps, linea)
            && let Some(padre) = ps[idx as usize].padre
        {
            return self.enfocar(ps[padre as usize].linea);
        }
        self.salir_enfoque()
    }

    /// Sale del enfoque y vuelve a mostrar la nota entera.
    pub fn salir_enfoque(&self) -> bool {
        let i = &self.0;
        let Some(m) = i.enfoque.borrow_mut().take() else {
            return false;
        };
        i.buffer.delete_mark(&m);
        plegado::aplicar_ocultas(&i.buffer, &i.tag_fuera, &[]);
        i.migas.ocultar();
        i.reader.set_enfoque(None);
        self.avisar_enfoque();
        true
    }

    /// Reconcilia el enfoque después de editar el texto (si el elemento desaparece, sale).
    pub(super) fn reconciliar_enfoque(&self) {
        let Some(linea) = self.linea_enfocada() else {
            return;
        };
        let (texto, ps) = self.arbol();
        let Some(idx) = nucleo::en_linea(&ps, linea) else {
            self.salir_enfoque();
            return;
        };
        let p = &ps[idx as usize];
        let total = self.0.buffer.line_count() as u32;
        let fuera = nucleo::fuera_de(&ps, idx, total);
        plegado::aplicar_ocultas(&self.0.buffer, &self.0.tag_fuera, &fuera);
        let nombre_nota = self
            .current_path()
            .and_then(|r| {
                r.as_str()
                    .rsplit('/')
                    .next()
                    .map(|s| s.strip_suffix(".md").unwrap_or(s).to_owned())
            })
            .unwrap_or_else(|| "Nota".to_owned());
        self.0
            .migas
            .actualizar(&Rc::downgrade(&self.0), &nombre_nota, &texto, &ps, idx);
        self.0.reader.set_enfoque(Some((p.linea, p.ultima)));
    }

    /// Si un borrado cruza el comienzo de la línea enfocada, sale del enfoque.
    pub(super) fn olvidar_enfoque_borrado(
        &self,
        buf: &sourceview5::Buffer,
        a: &gtk::TextIter,
        b: &gtk::TextIter,
    ) {
        let (a_off, b_off) = (a.offset(), b.offset());
        let limpio = a.starts_line();
        let borrado = self.0.enfoque.borrow().as_ref().is_some_and(|m| {
            let p = buf.iter_at_mark(m).offset();
            a_off < p && (p < b_off || (p == b_off && !limpio))
        });
        if borrado {
            self.salir_enfoque();
        }
    }
}
