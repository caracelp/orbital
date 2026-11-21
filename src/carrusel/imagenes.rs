//! Imágenes del carrusel (032): se decodifican fuera del hilo de GTK, achicadas al tamaño en que
//! se muestran (nunca agrandadas), y quedan en una caché chica por (archivo, tamaño) para volver a
//! una diapositiva o a una miniatura sin decodificar de nuevo.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::path::{Path, PathBuf};

use gtk::prelude::*;
use gtk::{gdk, gio, glib};

/// Bytes de imágenes decodificadas como mucho en la caché (RGBA).
const CACHE_BYTES: usize = 48 << 20;
/// Lado máximo de una imagen decodificada, aunque la pantalla sea más grande.
const LADO_MAXIMO: i32 = 4096;

type Clave = (PathBuf, i32, i32);

#[derive(Default)]
struct Cache {
    orden: VecDeque<(Clave, gdk::Texture, usize)>,
    bytes: usize,
}

thread_local! {
    static CACHE: RefCell<Cache> = RefCell::default();
    /// Pedidos en curso: no se decodifica dos veces lo mismo a la vez.
    static EN_CURSO: RefCell<Vec<(Clave, Vec<Box<dyn FnOnce(Option<gdk::Texture>)>>)>> =
        RefCell::default();
}

/// Lo que devuelve el hilo de fondo: los píxeles y su forma (`glib::Bytes` cruza de hilo).
struct Pixeles {
    bytes: glib::Bytes,
    ancho: i32,
    alto: i32,
    fila: usize,
    alfa: bool,
}

fn decodificar(path: &Path, ancho: i32, alto: i32) -> Option<Pixeles> {
    use gtk::gdk_pixbuf::Pixbuf;
    let (_, w, h) = Pixbuf::file_info(path)?;
    let pb = if w > 0 && h > 0 && w <= ancho && h <= alto {
        Pixbuf::from_file(path).ok()?
    } else {
        Pixbuf::from_file_at_scale(path, ancho, alto, true).ok()?
    };
    // Las imágenes de la cámara traen la orientación aparte.
    let pb = pb.apply_embedded_orientation().unwrap_or(pb);
    Some(Pixeles {
        bytes: pb.read_pixel_bytes(),
        ancho: pb.width(),
        alto: pb.height(),
        fila: pb.rowstride() as usize,
        alfa: pb.has_alpha(),
    })
}

fn textura(p: Pixeles) -> gdk::Texture {
    let formato = if p.alfa {
        gdk::MemoryFormat::R8g8b8a8
    } else {
        gdk::MemoryFormat::R8g8b8
    };
    gdk::MemoryTexture::new(p.ancho, p.alto, formato, &p.bytes, p.fila).upcast()
}

/// La imagen ya decodificada con ese tamaño máximo, si está en la caché.
pub fn en_cache(path: &Path, ancho: i32, alto: i32) -> Option<gdk::Texture> {
    let (ancho, alto) = acotar(ancho, alto);
    CACHE.with_borrow_mut(|c| {
        let n = c
            .orden
            .iter()
            .position(|(k, _, _)| k.0 == path && k.1 == ancho && k.2 == alto)?;
        // Al frente: la más reciente.
        let e = c.orden.remove(n)?;
        let t = e.1.clone();
        c.orden.push_back(e);
        Some(t)
    })
}

fn acotar(ancho: i32, alto: i32) -> (i32, i32) {
    (ancho.clamp(16, LADO_MAXIMO), alto.clamp(16, LADO_MAXIMO))
}

fn guardar(clave: Clave, t: &gdk::Texture) {
    let bytes = (t.width() * t.height() * 4) as usize;
    CACHE.with_borrow_mut(|c| {
        c.orden.push_back((clave, t.clone(), bytes));
        c.bytes += bytes;
        while c.bytes > CACHE_BYTES && c.orden.len() > 1 {
            if let Some((_, _, b)) = c.orden.pop_front() {
                c.bytes -= b;
            }
        }
    });
}

/// Pide la imagen de `path` achicada para entrar en `ancho`×`alto` píxeles: de la caché, o
/// decodificada en el pool de `gio`. `listo` corre en el hilo de GTK (con `None` si no se pudo
/// leer).
pub fn pedir(
    path: &Path,
    ancho: i32,
    alto: i32,
    listo: impl FnOnce(Option<gdk::Texture>) + 'static,
) {
    if let Some(t) = en_cache(path, ancho, alto) {
        listo(Some(t));
        return;
    }
    let (ancho, alto) = acotar(ancho, alto);
    let clave: Clave = (path.to_path_buf(), ancho, alto);
    let ya = EN_CURSO.with_borrow_mut(|e| {
        if let Some((_, esperan)) = e.iter_mut().find(|(k, _)| *k == clave) {
            esperan.push(Box::new(listo));
            true
        } else {
            e.push((clave.clone(), vec![Box::new(listo)]));
            false
        }
    });
    if ya {
        return;
    }
    glib::spawn_future_local(async move {
        let ruta = clave.0.clone();
        let r = gio::spawn_blocking(move || decodificar(&ruta, ancho, alto))
            .await
            .ok()
            .flatten()
            .map(textura);
        if let Some(t) = &r {
            guardar(clave.clone(), t);
        }
        let esperan = EN_CURSO.with_borrow_mut(|e| {
            e.iter()
                .position(|(k, _)| *k == clave)
                .map(|n| e.remove(n).1)
                .unwrap_or_default()
        });
        for f in esperan {
            f(r.clone());
        }
    });
}

/// Pide la imagen y no hace nada con ella (las diapositivas vecinas, para que estén al llegar).
pub fn adelantar(path: &Path, ancho: i32, alto: i32) {
    pedir(path, ancho, alto, |_| {});
}

/// Cuántas imágenes hay en la caché (pruebas).
#[cfg(test)]
pub fn en_cache_total() -> usize {
    CACHE.with_borrow(|c| c.orden.len())
}

/// Vacía la caché (pruebas).
#[cfg(test)]
pub fn vaciar() {
    CACHE.with_borrow_mut(|c| {
        c.orden.clear();
        c.bytes = 0;
    });
}
