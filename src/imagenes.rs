//! Imágenes del carrusel (032) y de la galería (033): se decodifican fuera del hilo de GTK,
//! achicadas al tamaño en que se muestran (nunca agrandadas), y quedan en una caché chica por
//! (archivo, tamaño, ajuste) para volver a una diapositiva o a una miniatura sin decodificar de
//! nuevo. La caché es por hilo y está vacía hasta la primera imagen: nada se prepara en el
//! arranque ni en las notas sin carrusel ni galería.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::io::Read;
use std::path::{Path, PathBuf};

use gtk::prelude::*;
use gtk::{gdk, gio, glib};

/// Bytes de imágenes decodificadas como mucho en la caché (RGBA).
const CACHE_BYTES: usize = 48 << 20;
/// Lado máximo de una imagen decodificada, aunque la pantalla sea más grande.
pub const LADO_MAXIMO: i32 = 4096;

/// Cómo entra la imagen en el rectángulo pedido.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ajuste {
    /// Entera adentro (el escenario del carrusel, el visor).
    Contener,
    /// Lo cubre entero, para recortarla (las celdas de la galería).
    Cubrir,
}

type Clave = (PathBuf, i32, i32, Ajuste);
/// Quien espera una imagen.
type Listo = Box<dyn FnOnce(Option<gdk::Texture>)>;

#[derive(Default)]
struct Cache {
    orden: VecDeque<(Clave, gdk::Texture, usize)>,
    bytes: usize,
}

thread_local! {
    static CACHE: RefCell<Cache> = RefCell::default();
    /// Pedidos en curso: no se decodifica dos veces lo mismo a la vez.
    static EN_CURSO: RefCell<Vec<(Clave, Vec<Listo>)>> =
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

/// La orientación EXIF de un JPEG (1 a 8; 1 si no tiene). Lee solo la cabecera.
pub fn orientacion(path: &Path) -> u16 {
    let es_jpeg = path
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("jpg") || e.eq_ignore_ascii_case("jpeg"));
    if !es_jpeg {
        return 1;
    }
    let mut buf = Vec::with_capacity(64 << 10);
    let leido = std::fs::File::open(path).and_then(|f| f.take(64 << 10).read_to_end(&mut buf));
    if leido.is_err() {
        return 1;
    }
    orientacion_exif(&buf).unwrap_or(1)
}

/// La orientación del primer bloque EXIF de los bytes de un JPEG.
fn orientacion_exif(b: &[u8]) -> Option<u16> {
    if b.get(..2)? != [0xFF, 0xD8] {
        return None;
    }
    let mut k = 2;
    while k + 4 <= b.len() {
        if b[k] != 0xFF {
            return None;
        }
        let marca = b[k + 1];
        let largo = usize::from(u16::from_be_bytes([b[k + 2], b[k + 3]]));
        // El comienzo de la imagen: ya no hay cabeceras.
        if marca == 0xDA || largo < 2 {
            return None;
        }
        let seg = b.get(k + 4..k + 2 + largo)?;
        if marca == 0xE1 && seg.starts_with(b"Exif\0\0") {
            let t = &seg[6..];
            let le = match t.get(..2)? {
                b"II" => true,
                b"MM" => false,
                _ => return None,
            };
            let u16_en = |o: usize| -> Option<u16> {
                let x = t.get(o..o + 2)?;
                Some(if le {
                    u16::from_le_bytes([x[0], x[1]])
                } else {
                    u16::from_be_bytes([x[0], x[1]])
                })
            };
            let u32_en = |o: usize| -> Option<u32> {
                let x = t.get(o..o + 4)?;
                let a = [x[0], x[1], x[2], x[3]];
                Some(if le {
                    u32::from_le_bytes(a)
                } else {
                    u32::from_be_bytes(a)
                })
            };
            let ifd = u32_en(4)? as usize;
            let n = usize::from(u16_en(ifd)?);
            for e in 0..n.min(64) {
                let o = ifd + 2 + e * 12;
                if u16_en(o)? == 0x0112 {
                    let v = u16_en(o + 8)?;
                    return (1..=8).contains(&v).then_some(v);
                }
            }
            return None;
        }
        k += 2 + largo;
    }
    None
}

/// Si la orientación gira la imagen un cuarto de vuelta (ancho y alto se cambian).
fn gira(o: u16) -> bool {
    matches!(o, 5..=8)
}

/// Ancho y alto con que se ve la imagen (la orientación de la cámara aplicada), sin
/// decodificarla: lee la cabecera. Para el hilo de fondo.
pub fn medida_de_archivo(path: &Path) -> Option<(i32, i32)> {
    let (_, w, h) = gtk::gdk_pixbuf::Pixbuf::file_info(path)?;
    if w <= 0 || h <= 0 {
        return None;
    }
    Some(if gira(orientacion(path)) {
        (h, w)
    } else {
        (w, h)
    })
}

/// Las medidas de varios archivos ([`medida_de_archivo`]), en el pool de `gio`.
pub async fn medidas(paths: Vec<PathBuf>) -> Vec<Option<(i32, i32)>> {
    let n = paths.len();
    gio::spawn_blocking(move || paths.iter().map(|p| medida_de_archivo(p)).collect())
        .await
        .unwrap_or_else(|_| vec![None; n])
}

fn decodificar(path: &Path, ancho: i32, alto: i32, ajuste: Ajuste) -> Option<Pixeles> {
    use gtk::gdk_pixbuf::Pixbuf;
    let (_, w, h) = Pixbuf::file_info(path)?;
    // El rectángulo en la orientación del archivo (la de la cámara se aplica al final).
    let (ancho, alto) = if gira(orientacion(path)) {
        (alto, ancho)
    } else {
        (ancho, alto)
    };
    let pb = if w <= 0 || h <= 0 {
        Pixbuf::from_file_at_scale(path, ancho, alto, true).ok()?
    } else {
        let escala = match ajuste {
            Ajuste::Contener => {
                (f64::from(ancho) / f64::from(w)).min(f64::from(alto) / f64::from(h))
            }
            Ajuste::Cubrir => (f64::from(ancho) / f64::from(w)).max(f64::from(alto) / f64::from(h)),
        };
        if escala >= 1.0 {
            Pixbuf::from_file(path).ok()?
        } else {
            let tw = ((f64::from(w) * escala).ceil() as i32).clamp(1, LADO_MAXIMO);
            let th = ((f64::from(h) * escala).ceil() as i32).clamp(1, LADO_MAXIMO);
            Pixbuf::from_file_at_scale(path, tw, th, true).ok()?
        }
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

/// La imagen ya decodificada para entrar (o cubrir, según `ajuste`) ese tamaño, si está en la
/// caché.
pub fn en_cache_con(path: &Path, ancho: i32, alto: i32, ajuste: Ajuste) -> Option<gdk::Texture> {
    let (ancho, alto) = acotar(ancho, alto);
    CACHE.with_borrow_mut(|c| {
        let n = c
            .orden
            .iter()
            .position(|(k, _, _)| k.0 == path && k.1 == ancho && k.2 == alto && k.3 == ajuste)?;
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
    pedir_con(path, ancho, alto, Ajuste::Contener, listo);
}

/// Como [`pedir`], con el ajuste dado.
pub fn pedir_con(
    path: &Path,
    ancho: i32,
    alto: i32,
    ajuste: Ajuste,
    listo: impl FnOnce(Option<gdk::Texture>) + 'static,
) {
    if let Some(t) = en_cache_con(path, ancho, alto, ajuste) {
        listo(Some(t));
        return;
    }
    let (ancho, alto) = acotar(ancho, alto);
    let clave: Clave = (path.to_path_buf(), ancho, alto, ajuste);
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
        let r = gio::spawn_blocking(move || decodificar(&ruta, ancho, alto, ajuste))
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

/// Bytes decodificados en la caché (mediciones).
#[cfg(test)]
pub fn en_cache_bytes() -> usize {
    CACHE.with_borrow(|c| c.bytes)
}

/// Vacía la caché (pruebas).
#[cfg(test)]
pub fn vaciar() {
    CACHE.with_borrow_mut(|c| {
        c.orden.clear();
        c.bytes = 0;
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Un JPEG mínimo con solo la cabecera EXIF (orden de bytes de Intel o de Motorola).
    fn jpeg_con_orientacion(o: u16, le: bool) -> Vec<u8> {
        let mut tiff = Vec::new();
        let u16b = |v: u16| if le { v.to_le_bytes() } else { v.to_be_bytes() };
        let u32b = |v: u32| if le { v.to_le_bytes() } else { v.to_be_bytes() };
        tiff.extend_from_slice(if le { b"II" } else { b"MM" });
        tiff.extend_from_slice(&u16b(42));
        tiff.extend_from_slice(&u32b(8));
        tiff.extend_from_slice(&u16b(1));
        tiff.extend_from_slice(&u16b(0x0112));
        tiff.extend_from_slice(&u16b(3));
        tiff.extend_from_slice(&u32b(1));
        tiff.extend_from_slice(&u16b(o));
        tiff.extend_from_slice(&[0, 0]);
        tiff.extend_from_slice(&u32b(0));
        let mut seg = b"Exif\0\0".to_vec();
        seg.extend_from_slice(&tiff);
        let mut b = vec![0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x04, 0x00, 0x00, 0xFF, 0xE1];
        b.extend_from_slice(&((seg.len() + 2) as u16).to_be_bytes());
        b.extend_from_slice(&seg);
        b.extend_from_slice(&[0xFF, 0xDA, 0x00, 0x02]);
        b
    }

    #[test]
    fn lee_la_orientacion_exif() {
        assert_eq!(orientacion_exif(&jpeg_con_orientacion(6, true)), Some(6));
        assert_eq!(orientacion_exif(&jpeg_con_orientacion(8, false)), Some(8));
        assert_eq!(orientacion_exif(&jpeg_con_orientacion(9, true)), None);
        assert_eq!(orientacion_exif(b"\x89PNG"), None);
        assert_eq!(orientacion_exif(&[0xFF, 0xD8, 0xFF]), None);
        assert!(gira(6) && gira(8) && !gira(1) && !gira(3));
    }
}
