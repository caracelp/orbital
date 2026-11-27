//! Editor de ecuaciones desde Código (051): con el cursor en una fórmula (`$…$`, `$$…$$`,
//! ` ```math `) la abre en el editor y, al aceptar, reescribe solo su LaTeX (una sola acción de
//! deshacer); sin fórmula, inserta una nueva en el cursor. Se abre con Ctrl+M, desde el menú
//! contextual de Código y desde la paleta de comandos («Insertar ecuación»). Solo en notas `.md`,
//! en Código y en la mitad Código de Dividida.
//!
//! Gráficos (052): con el cursor en un bloque `grafico`, el editor abre su página «Gráfico» y al
//! aceptar reescribe los renglones de adentro; al insertar, la página «Gráfico» inserta un
//! bloque nuevo en su propio párrafo. También una acción de deshacer.

use super::*;
use crate::ecuaciones::{Aceptado, Apertura, EditorEcuaciones};
use crate::formulas::Modo;

/// Qué hace el editor al aceptar.
enum Destino {
    /// Reescribe el LaTeX entre las dos marcas.
    Reemplazar { modo: Modo },
    /// Reescribe los renglones de un bloque `grafico` entre las dos marcas.
    Grafico,
    /// Inserta una fórmula o un gráfico nuevo entre las marcas (lo seleccionado, si había).
    Insertar,
}

/// Qué hay bajo el cursor o el puntero, para el rótulo del menú contextual.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Sobre {
    Nada,
    Formula,
    Grafico,
}

const ROTULO_EDITAR: &str = "Editar ecuación…";
const ROTULO_EDITAR_GRAFICO: &str = "Editar gráfico…";
const ROTULO_INSERTAR: &str = "Insertar ecuación…";

impl Editor {
    /// El menú contextual de Código lleva «Insertar ecuación…» o, sobre una fórmula, «Editar
    /// ecuación…». Un menú de un ítem y una acción: nada más se arma al crear el editor.
    pub(super) fn instalar_ecuaciones(&self) {
        let i = &self.0;
        let grupo = gio::SimpleActionGroup::new();
        let accion = gio::SimpleAction::new("ecuacion", None);
        let weak = Rc::downgrade(&self.0);
        accion.connect_activate(move |_, _| {
            if let Some(inner) = weak.upgrade() {
                Editor(inner).abrir_ecuacion(false);
            }
        });
        grupo.add_action(&accion);
        for (nombre, f) in [
            ("color-texto", Editor::abrir_selector_color as fn(&Editor)),
            ("resaltar-texto", Editor::resaltar_texto_rapido),
            ("quitar-color-texto", Editor::quitar_color_texto),
        ] {
            let a = gio::SimpleAction::new(nombre, None);
            let weak = Rc::downgrade(&self.0);
            a.connect_activate(move |_, _| {
                if let Some(inner) = weak.upgrade() {
                    f(&Editor(inner));
                }
            });
            grupo.add_action(&a);
        }
        i.view.insert_action_group("editor", Some(&grupo));
        self.rotular_menu(Sobre::Nada);
        // Con el clic derecho, el rótulo según lo que hay bajo el puntero (el teclado usa el de
        // la última pausa del cursor, ver `actualizar_vista_previa`).
        let clic = gtk::GestureClick::builder()
            .button(gtk::gdk::BUTTON_SECONDARY)
            .propagation_phase(gtk::PropagationPhase::Capture)
            .build();
        let weak = Rc::downgrade(&self.0);
        clic.connect_pressed(move |g, _, x, y| {
            let Some(inner) = weak.upgrade() else { return };
            let e = Editor(inner);
            let v = e.0.view.upcast_ref::<gtk::TextView>();
            let (bx, by) = posicion::al_buffer(v, x, y);
            let it = (e.0.format.get() == Format::Md)
                .then(|| posicion::iter_en(v, bx, by))
                .flatten();
            let sobre = match it {
                Some(it) if e.formula_en_offset(it.offset()).is_some() => Sobre::Formula,
                Some(it) if e.grafico_en_offset(it.offset()).is_some() => Sobre::Grafico,
                _ => Sobre::Nada,
            };
            e.rotular_menu(sobre);
            // Que GTK siga con su menú.
            g.set_state(gtk::EventSequenceState::None);
        });
        i.view.add_controller(clic);
    }

    /// Cambia el rótulo del ítem del menú contextual.
    pub(super) fn rotular_menu(&self, sobre: Sobre) {
        let i = &self.0;
        let rotulo = match sobre {
            Sobre::Formula => ROTULO_EDITAR,
            Sobre::Grafico => ROTULO_EDITAR_GRAFICO,
            Sobre::Nada => ROTULO_INSERTAR,
        };
        let actual = i
            .view
            .extra_menu()
            .and_then(|m| m.item_attribute_value(0, "label", Some(glib::VariantTy::STRING)))
            .and_then(|v| v.get::<String>());
        if actual.as_deref() == Some(rotulo) {
            return;
        }
        let menu = gio::Menu::new();
        let item = gio::MenuItem::new(Some(rotulo), Some("editor.ecuacion"));
        item.set_attribute_value("accel", Some(&"<Control>m".to_variant()));
        menu.append_item(&item);
        let item_color = gio::MenuItem::new(Some("Color y resaltado…"), Some("editor.color-texto"));
        item_color.set_attribute_value("accel", Some(&"<Control><Alt>c".to_variant()));
        menu.append_item(&item_color);
        let item_resaltar =
            gio::MenuItem::new(Some("Resaltar texto"), Some("editor.resaltar-texto"));
        item_resaltar.set_attribute_value("accel", Some(&"<Control><Alt>h".to_variant()));
        menu.append_item(&item_resaltar);
        menu.append(Some("Quitar color"), Some("editor.quitar-color-texto"));
        i.view.set_extra_menu(Some(&menu));
    }

    /// Por qué no se puede abrir el editor ahora, si no se puede.
    fn motivo_sin_ecuacion(&self) -> Option<&'static str> {
        let i = &self.0;
        if self.current_path().is_none() {
            return Some("Abrí una nota para insertar una ecuación");
        }
        if i.format.get() == Format::Html {
            return Some("El editor de ecuaciones es para las notas .md");
        }
        if !matches!(i.mode.get(), View::Codigo | View::Dividida) {
            return Some("El editor de ecuaciones se abre desde Código o Dividida");
        }
        None
    }

    /// Abre el editor de ecuaciones: con `solo_insertar`, siempre una fórmula nueva en el cursor;
    /// si no, la fórmula o el gráfico bajo el cursor (o una nueva si no hay).
    pub fn abrir_ecuacion(&self, solo_insertar: bool) -> Option<EditorEcuaciones> {
        self.abrir_editor(solo_insertar, false)
    }

    /// Abre el editor en su página «Gráfico» para insertar uno nuevo en el cursor (052).
    pub fn insertar_grafico(&self) -> Option<EditorEcuaciones> {
        self.abrir_editor(true, true)
    }

    fn abrir_editor(&self, solo_insertar: bool, grafico: bool) -> Option<EditorEcuaciones> {
        if let Some(m) = self.motivo_sin_ecuacion() {
            self.error(m);
            return None;
        }
        let i = &self.0;
        let b = &i.buffer;
        self.ocultar_vista_previa();
        let hallada = (!solo_insertar)
            .then(|| self.formula_en_offset(b.cursor_position()))
            .flatten();
        let grafico_hallado = (!solo_insertar && hallada.is_none())
            .then(|| self.grafico_en_offset(b.cursor_position()))
            .flatten();
        let (desde, hasta, inicial, destino) = match (hallada, grafico_hallado) {
            (Some(f), _) => (
                f.contenido.start,
                f.contenido.end,
                f.tex.trim().to_owned(),
                Destino::Reemplazar { modo: f.modo },
            ),
            (None, Some(g)) => (
                g.contenido.start,
                g.contenido.end,
                g.fuente.trim_end().to_owned(),
                Destino::Grafico,
            ),
            (None, None) => {
                let (a, z) = b
                    .selection_bounds()
                    .map_or((b.cursor_position(), b.cursor_position()), |(a, z)| {
                        (a.offset(), z.offset())
                    });
                let sel = b.text(&b.iter_at_offset(a), &b.iter_at_offset(z), true);
                // Lo seleccionado va al editor si no tiene renglones en blanco (un párrafo).
                let inicial = if sel.contains("\n\n") {
                    String::new()
                } else {
                    sel.trim().trim_matches('$').to_owned()
                };
                let (a, z) = if inicial.is_empty() { (a, a) } else { (a, z) };
                (a, z, inicial, Destino::Insertar)
            }
        };
        let marca_desde = b.create_mark(None, &b.iter_at_offset(desde), true);
        let marca_hasta = b.create_mark(None, &b.iter_at_offset(hasta), false);
        let (apertura, editando) = match destino {
            Destino::Reemplazar { modo } => (Apertura::Formula { tex: inicial, modo }, true),
            Destino::Grafico => (Apertura::Grafico { fuente: inicial }, true),
            Destino::Insertar => (
                Apertura::Insertar {
                    tex: inicial,
                    modo: Modo::EnLinea,
                    grafico,
                },
                false,
            ),
        };
        let padre = i.view.clone();
        let (aceptar, cerrar) = (self.clone(), self.clone());
        let (md, mh) = (marca_desde.clone(), marca_hasta.clone());
        Some(EditorEcuaciones::presentar_con(
            &padre,
            apertura,
            move |r| match r {
                Aceptado::Formula(tex, modo_elegido) => {
                    aceptar.aplicar_ecuacion(&md, &mh, &tex, editando, modo_elegido);
                }
                Aceptado::Grafico(fuente) => aceptar.aplicar_grafico(&md, &mh, &fuente, editando),
            },
            move || {
                let b = &cerrar.0.buffer;
                for m in [&marca_desde, &marca_hasta] {
                    if !m.is_deleted() {
                        b.delete_mark(m);
                    }
                }
                cerrar.0.view.grab_focus();
            },
        ))
    }

    /// Escribe el resultado del editor en la nota, como una sola acción de deshacer.
    fn aplicar_ecuacion(
        &self,
        desde: &gtk::TextMark,
        hasta: &gtk::TextMark,
        tex: &str,
        editando: bool,
        modo: Modo,
    ) {
        let b = &self.0.buffer;
        let mut a = b.iter_at_mark(desde);
        let mut z = b.iter_at_mark(hasta);
        let tex = tex.trim();
        let nuevo = if editando {
            let viejo = b.text(&a, &z, true);
            // Se conservan los espacios y saltos de alrededor (`$$\n…\n$$`, la cerca).
            let lleno = viejo.trim();
            let (izq, der) = if lleno.is_empty() {
                (
                    if viejo.starts_with('\n') { "\n" } else { "" },
                    if viejo.ends_with('\n') { "\n" } else { "" },
                )
            } else {
                let k = viejo.find(lleno).unwrap_or(0);
                (&viejo[..k], &viejo[k + lleno.len()..])
            };
            // `$…$` va en un solo renglón (el modo de una fórmula que se edita no cambia).
            let tex = if modo == Modo::EnLinea {
                tex.split_whitespace().collect::<Vec<_>>().join(" ")
            } else {
                tex.to_owned()
            };
            format!("{izq}{tex}{der}")
        } else {
            match modo {
                Modo::EnLinea => {
                    format!("${}$", tex.split_whitespace().collect::<Vec<_>>().join(" "))
                }
                // `$$` en su propio párrafo: renglones en blanco antes y después si hace falta.
                Modo::Bloque => en_su_parrafo(b.upcast_ref(), &a, &z, &format!("$$\n{tex}\n$$")),
            }
        };
        let inicio = a.offset();
        b.begin_user_action();
        b.delete(&mut a, &mut z);
        let mut en = b.iter_at_offset(inicio);
        b.insert(&mut en, &nuevo);
        b.end_user_action();
        b.place_cursor(&b.iter_at_mark(hasta));
    }

    /// Escribe un gráfico (052): los renglones de adentro del bloque que se editaba o un bloque
    /// `grafico` nuevo en su propio párrafo. Una sola acción de deshacer.
    fn aplicar_grafico(
        &self,
        desde: &gtk::TextMark,
        hasta: &gtk::TextMark,
        fuente: &str,
        editando: bool,
    ) {
        let b = &self.0.buffer;
        let mut a = b.iter_at_mark(desde);
        let mut z = b.iter_at_mark(hasta);
        let fuente = fuente.trim_end();
        let nuevo = if editando {
            format!("{fuente}\n")
        } else {
            let cerca = "`".repeat(3);
            en_su_parrafo(
                b.upcast_ref(),
                &a,
                &z,
                &format!("{cerca}grafico\n{fuente}\n{cerca}"),
            )
        };
        let inicio = a.offset();
        b.begin_user_action();
        b.delete(&mut a, &mut z);
        let mut en = b.iter_at_offset(inicio);
        b.insert(&mut en, &nuevo);
        b.end_user_action();
        b.place_cursor(&b.iter_at_mark(hasta));
    }
}

/// `cuerpo` como bloque en su propio párrafo entre `a` y `z`: con los renglones en blanco que
/// falten antes y después.
pub(super) fn en_su_parrafo(
    b: &gtk::TextBuffer,
    a: &gtk::TextIter,
    z: &gtk::TextIter,
    cuerpo: &str,
) -> String {
    let antes = {
        let mut ini = *a;
        ini.set_line_offset(0);
        b.text(&ini, a, true)
    };
    let despues = {
        let mut fin = *z;
        if !fin.ends_line() {
            fin.forward_to_line_end();
        }
        b.text(z, &fin, true)
    };
    let linea_vacia = |l: i32| {
        b.iter_at_line(l).is_none_or(|ini| {
            let mut fin = ini;
            if !fin.ends_line() {
                fin.forward_to_line_end();
            }
            b.text(&ini, &fin, true).trim().is_empty()
        })
    };
    let pre = if !antes.trim().is_empty() {
        "\n\n"
    } else if a.line() > 0 && !linea_vacia(a.line() - 1) {
        "\n"
    } else {
        ""
    };
    let ultima = b.end_iter().line();
    let post = if !despues.trim().is_empty() {
        "\n\n"
    } else if z.line() < ultima && !linea_vacia(z.line() + 1) {
        "\n"
    } else {
        ""
    };
    format!("{pre}{cuerpo}{post}")
}
