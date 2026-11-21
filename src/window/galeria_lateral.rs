//! Galería lateral (034): la lista de medios de la nota abierta para el modo Galería de la barra
//! derecha, armada en otro hilo, y lo que el panel pide (ir al embed, mostrar en el explorador).
//!
//! Con la barra cerrada o en otro modo no se calcula nada: `refresh_panel` solo llama acá con la
//! galería a la vista, y el aviso por cada cambio del texto (conectado recién al crear el panel)
//! sale enseguida si el panel no está a la vista.

use super::*;
use crate::galeria_lateral::Pedido;

/// La misma pausa que el refresco por edición del editor.
const PAUSA_MS: u64 = 250;

impl Window {
    /// Conecta la galería lateral la primera vez que se elige su modo.
    pub(super) fn setup_galeria_lateral(&self) {
        let weak = Rc::downgrade(&self.0);
        self.0.panel.connect_galeria(move |g| {
            let Some(i) = weak.upgrade() else { return };
            let w = Window(i);
            let weak = Rc::downgrade(&w.0);
            g.connect_pedido(move |p| {
                let Some(i) = weak.upgrade() else { return };
                let w = Window(i);
                match p {
                    Pedido::IrAlEmbed(m) => w.ir_al_embed_de_galeria(&m),
                    Pedido::MostrarEnExplorador(p) => w.mostrar_en_explorador(p.as_str()),
                    Pedido::Aviso(t) => w.toast(&t),
                }
            });
            // Editar la nota: la grilla se actualiza con una pausa.
            let pendiente = Rc::new(Cell::new(0u64));
            let weak = Rc::downgrade(&w.0);
            let gd = g.widget().downgrade();
            w.0.code_editor.buffer().connect_changed(move |_| {
                if !gd.upgrade().is_some_and(|g| g.is_mapped()) {
                    return;
                }
                let n = pendiente.get() + 1;
                pendiente.set(n);
                let pendiente = pendiente.clone();
                let weak = weak.clone();
                glib::timeout_add_local_once(
                    std::time::Duration::from_millis(PAUSA_MS),
                    move || {
                        if pendiente.get() == n
                            && let Some(i) = weak.upgrade()
                        {
                            Window(i).refresh_galeria_lateral();
                        }
                    },
                );
            });
        });
    }

    /// Arma en otro hilo la lista de medios de la nota abierta y se la pasa al panel.
    pub(super) fn refresh_galeria_lateral(&self) {
        let i = &self.0;
        let Some(g) = i.panel.galeria() else { return };
        let Some(entry) = i.current_entry.borrow().clone() else {
            g.mostrar_vacio("Abrí una nota para ver sus imágenes y videos.");
            return;
        };
        match Format::from_name(entry.path.file_name()) {
            Some(Format::Md) => {}
            Some(Format::Html) => {
                g.mostrar_vacio(
                    "Por ahora la galería muestra los medios de las notas Markdown (.md).",
                );
                return;
            }
            None => {
                g.mostrar_vacio("Este archivo no es una nota.");
                return;
            }
        }
        let Some(vault) = i.vault.borrow().clone() else {
            return;
        };
        // El texto del editor (con lo que no se guardó todavía) o el de la nota de solo lectura.
        let texto = if i.code_editor.current_path().as_ref() == Some(&entry.path) {
            i.code_editor.text()
        } else {
            match i
                .current_note
                .borrow()
                .as_ref()
                .filter(|n| n.path == entry.path)
            {
                Some(n) => n.source.clone(),
                None => return,
            }
        };
        let generacion = g.nuevo_pedido();
        let ruta = entry.path.clone();
        glib::spawn_future_local(async move {
            let r = ruta.clone();
            let m = gio::spawn_blocking(move || {
                let mut m = orbital_vault::medios_nota::extraer(&texto);
                vault.resolver_medios_nota(&r, &mut m);
                m
            })
            .await;
            if let Ok(m) = m
                && g.vigente(generacion)
            {
                g.set_medios(ruta.as_str(), Rc::new(m));
            }
        });
    }

    /// «Ir al embed»: el editor muestra la nota con el embed resaltado y el foco.
    fn ir_al_embed_de_galeria(&self, m: &orbital_vault::medios_nota::Medio) {
        let i = &self.0;
        let ruta = i.panel.galeria().and_then(|g| g.ruta());
        let en_editor = i.code_editor.current_path().map(|p| p.to_string());
        if ruta.is_none() || ruta != en_editor {
            self.toast("El embed está en una nota de solo lectura");
            return;
        }
        if i.content_stack.visible_child_name().as_deref() != Some("codigo") {
            i.content_stack.set_visible_child_name("codigo");
        }
        if !i.code_editor.ir_al_embed(&m.ubicacion) {
            self.toast("El embed ya no está en la nota");
        }
    }
}
