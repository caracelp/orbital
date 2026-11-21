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
/// decodificarla: PNG, JPEG, GIF, WebP y BMP desde los primeros 64 KiB (la cabecera); el resto,
/// con `gdk-pixbuf` (más lento: ≈ 25 ms por PNG grande en el equipo de referencia). Para el hilo
/// de fondo.
pub fn medida_de_archivo(path: &Path) -> Option<(i32, i32)> {
    let ((w, h), o) = cabecera(path)?;
    Some(if gira(o) { (h, w) } else { (w, h) })
}

/// Las medidas guardadas en el archivo (sin girar) y su orientación EXIF.
fn cabecera(path: &Path) -> Option<((i32, i32), u16)> {
    let mut buf = Vec::with_capacity(64 << 10);
    std::fs::File::open(path)
        .and_then(|f| f.take(64 << 10).read_to_end(&mut buf))
        .ok()?;
    let (w, h) = match medida_de_cabecera(&buf) {
        Some(m) => m,
        None => {
            let (_, w, h) = gtk::gdk_pixbuf::Pixbuf::file_info(path)?;
            (w, h)
        }
    };
    (w > 0 && h > 0).then(|| ((w, h), orientacion_exif(&buf).unwrap_or(1)))
}

/// Ancho y alto de la cabecera de una imagen PNG, JPEG, GIF, WebP o BMP.
fn medida_de_cabecera(b: &[u8]) -> Option<(i32, i32)> {
    let be32 =
        |o: usize| -> Option<u32> { Some(u32::from_be_bytes(b.get(o..o + 4)?.try_into().ok()?)) };
    let be16 =
        |o: usize| -> Option<u16> { Some(u16::from_be_bytes(b.get(o..o + 2)?.try_into().ok()?)) };
    let le16 =
        |o: usize| -> Option<u16> { Some(u16::from_le_bytes(b.get(o..o + 2)?.try_into().ok()?)) };
    let le32 =
        |o: usize| -> Option<u32> { Some(u32::from_le_bytes(b.get(o..o + 4)?.try_into().ok()?)) };
    let a_i32 = |w: u32, h: u32| Some((i32::try_from(w).ok()?, i32::try_from(h).ok()?));
    if b.starts_with(b"\x89PNG\r\n\x1a\n") && b.get(12..16)? == b"IHDR" {
        return a_i32(be32(16)?, be32(20)?);
    }
    if b.starts_with(b"GIF8") {
        return a_i32(u32::from(le16(6)?), u32::from(le16(8)?));
    }
    if b.starts_with(b"BM") {
        let w = le32(18)? as i32;
        let h = le32(22)? as i32;
        return Some((w.abs(), h.abs()));
    }
    if b.starts_with(b"RIFF") && b.get(8..12)? == b"WEBP" {
        return match b.get(12..16)? {
            b"VP8 " => a_i32(u32::from(le16(26)? & 0x3fff), u32::from(le16(28)? & 0x3fff)),
            b"VP8L" => {
                let v = le32(21)?;
                a_i32((v & 0x3fff) + 1, ((v >> 14) & 0x3fff) + 1)
            }
            b"VP8X" => {
                let w = u32::from_le_bytes([*b.get(24)?, *b.get(25)?, *b.get(26)?, 0]) + 1;
                let h = u32::from_le_bytes([*b.get(27)?, *b.get(28)?, *b.get(29)?, 0]) + 1;
                a_i32(w, h)
            }
            _ => None,
        };
    }
    if b.starts_with(&[0xFF, 0xD8]) {
        let mut k = 2;
        while k + 9 < b.len() {
            if b[k] != 0xFF {
                return None;
            }
            let marca = b[k + 1];
            if marca == 0xFF {
                k += 1;
                continue;
            }
            let largo = usize::from(be16(k + 2)?);
            // SOF0–SOF15 menos DHT (C4), JPG (C8) y DAC (CC).
            if (0xC0..=0xCF).contains(&marca) && !matches!(marca, 0xC4 | 0xC8 | 0xCC) {
                return a_i32(u32::from(be16(k + 7)?), u32::from(be16(k + 5)?));
            }
            if marca == 0xDA || largo < 2 {
                return None;
            }
            k += 2 + largo;
        }
    }
    None
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
    let ((w, h), o) = cabecera(path).unwrap_or(((0, 0), 1));
    // El rectángulo en la orientación del archivo (la de la cámara se aplica al final).
    let (ancho, alto) = if gira(o) {
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

    #[test]
    fn medidas_de_la_cabecera() {
        let mut png = b"\x89PNG\r\n\x1a\n\0\0\0\x0dIHDR".to_vec();
        png.extend_from_slice(&1600u32.to_be_bytes());
        png.extend_from_slice(&1000u32.to_be_bytes());
        assert_eq!(medida_de_cabecera(&png), Some((1600, 1000)));
        let gif = b"GIF89a\x40\x01\xf0\x00";
        assert_eq!(medida_de_cabecera(gif), Some((320, 240)));
        // Un JPEG: SOI, APP0 vacío, SOF0 con 480 de alto y 640 de ancho.
        let jpeg = [
            0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x02, 0xFF, 0xC0, 0x00, 0x11, 0x08, 0x01, 0xE0, 0x02,
            0x80, 0x03, 0, 0, 0, 0, 0, 0,
        ];
        assert_eq!(medida_de_cabecera(&jpeg), Some((640, 480)));
        assert_eq!(medida_de_cabecera(b"nada"), None);
    }
}
