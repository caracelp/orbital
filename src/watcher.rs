//! Vigilancia de la bóveda (research R5): un `gio::FileMonitor` por carpeta, con los avisos
//! agrupados cada 100 ms. No lee nada en el hilo principal: solo junta rutas y llama a `on_batch`.

use std::cell::{Cell, RefCell};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::Duration;

use gtk::prelude::*;
use gtk::{gio, glib};

/// Lo que el monitor le avisa a la ventana.
pub enum Batch {
    /// Rutas absolutas que cambiaron.
    Changed(Vec<PathBuf>),
    /// La carpeta de la bóveda dejó de estar disponible (disco desconectado o borrada).
    Gone,
}

/// Monitores activos; se sueltan (y dejan de vigilar) al reemplazarlos.
pub struct Watcher {
    _monitors: Vec<gio::FileMonitor>,
}

impl Watcher {
    /// Vigila `root` y sus `folders` (rutas absolutas).
    pub fn new(
        root: &Path,
        folders: impl Iterator<Item = PathBuf>,
        on_batch: impl Fn(Batch) + 'static,
    ) -> Self {
        let pending: Rc<RefCell<BTreeSet<PathBuf>>> = Rc::default();
        let scheduled = Rc::new(Cell::new(false));
        let on_batch = Rc::new(on_batch);
        let root = root.to_path_buf();
        let mut monitors = Vec::new();
        for dir in std::iter::once(root.clone()).chain(folders) {
            let Ok(monitor) = gio::File::for_path(&dir).monitor_directory(
                gio::FileMonitorFlags::WATCH_MOVES,
                None::<&gio::Cancellable>,
            ) else {
                continue;
            };
            let (pending, scheduled, on_batch, root) = (
                pending.clone(),
                scheduled.clone(),
                on_batch.clone(),
                root.clone(),
            );
            monitor.connect_changed(move |_, file, other, event| {
                if matches!(event, gio::FileMonitorEvent::Unmounted)
                    || (matches!(event, gio::FileMonitorEvent::Deleted)
                        && file.path().as_deref() == Some(root.as_path()))
                {
                    on_batch(Batch::Gone);
                    return;
                }
                let mut p = pending.borrow_mut();
                p.extend(file.path());
                if let Some(o) = other {
                    p.extend(o.path());
                }
                drop(p);
                if !scheduled.replace(true) {
                    let (pending, scheduled, on_batch) =
                        (pending.clone(), scheduled.clone(), on_batch.clone());
                    glib::timeout_add_local_once(Duration::from_millis(100), move || {
                        scheduled.set(false);
                        let paths: Vec<PathBuf> = std::mem::take(&mut *pending.borrow_mut())
                            .into_iter()
                            .collect();
                        if !paths.is_empty() {
                            on_batch(Batch::Changed(paths));
                        }
                    });
                }
            });
            monitors.push(monitor);
        }
        Self {
            _monitors: monitors,
        }
    }
}
