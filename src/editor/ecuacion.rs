//! Editor de ecuaciones desde Código (051): con el cursor en una fórmula (`$…$`, `$$…$$`,
//! ` ```math `) la abre en el editor y, al aceptar, reescribe solo su LaTeX (una sola acción de
//! deshacer); sin fórmula, inserta una nueva en el cursor. Se abre con Ctrl+M, desde el menú
//! contextual de Código y desde la paleta de comandos («Insertar ecuación»). Solo en notas `.md`,
//! en Código y en la mitad Código de Dividida.

use super::*;
use crate::ecuaciones::EditorEcuaciones;
use crate::formulas::Modo;

/// Qué hace el editor al aceptar.
enum Destino {
    /// Reescribe el LaTeX entre las dos marcas.
    Reemplazar { modo: Modo },
    /// Inserta una fórmula nueva entre las marcas (lo seleccionado, si había).
    Insertar,
}

const ROTULO_EDITAR: &str = "Editar ecuación…";
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
        i.view.insert_action_group("editor", Some(&grupo));
        self.rotular_menu(false);
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
            let sobre = e.0.format.get() == Format::Md
                && posicion::iter_en(v, bx, by)
                    .and_then(|it| e.formula_en_offset(it.offset()))
                    .is_some();
            e.rotular_menu(sobre);
            // Que GTK siga con su menú.
            g.set_state(gtk::EventSequenceState::None);
        });
        i.view.add_controller(clic);
    }

    /// Cambia el rótulo del ítem del menú contextual.
    pub(super) fn rotular_menu(&self, sobre_formula: bool) {
        let i = &self.0;
        let rotulo = if sobre_formula {
            ROTULO_EDITAR
        } else {
            ROTULO_INSERTAR
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
    /// si no, la fórmula bajo el cursor (o una nueva si no hay).
    pub fn abrir_ecuacion(&self, solo_insertar: bool) -> Option<EditorEcuaciones> {
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
        let (desde, hasta, inicial, destino) = match hallada {
            Some(f) => (
                f.contenido.start,
                f.contenido.end,
                f.tex.trim().to_owned(),
                Destino::Reemplazar { modo: f.modo },
            ),
            None => {
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
        let (modo, editando) = match destino {
            Destino::Reemplazar { modo } => (modo, true),
            Destino::Insertar => (Modo::EnLinea, false),
        };
        let padre = i.view.clone();
        let (aceptar, cerrar) = (self.clone(), self.clone());
        let (md, mh) = (marca_desde.clone(), marca_hasta.clone());
        Some(EditorEcuaciones::presentar(
            &padre,
            &inicial,
            modo,
            editando,
            move |tex, modo_elegido| {
                aceptar.aplicar_ecuacion(&md, &mh, &tex, editando, modo_elegido);
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
                Modo::Bloque => {
                    // `$$` en su propio párrafo: renglones en blanco antes y después si hace falta.
                    let antes = {
                        let mut ini = a;
                        ini.set_line_offset(0);
                        b.text(&ini, &a, true)
                    };
                    let despues = {
                        let mut fin = z;
                        if !fin.ends_line() {
                            fin.forward_to_line_end();
                        }
                        b.text(&z, &fin, true)
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
                    format!("{pre}$$\n{tex}\n$${post}")
                }
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
}
