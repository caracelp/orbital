//! Diagramas Mermaid en la app (058): el motor es `orbital_diagramas` (sin GTK); acá viven las
//! cachés, el tema según la vista y el widget que muestra la imagen.
//!
//! - El SVG (análisis y disposición: décimas de milisegundo) se arma al volcar o se toma de la
//!   caché; la imagen (`resvg`, unos milisegundos) se dibuja fuera del hilo de GTK: al preparar
//!   el documento ([`precalentar`], en el hilo de fondo de la Lectura, con un tope de tiempo) o,
//!   si falta, en el pool de `gio` ([`picture`] la completa al llegar).
//! - Las cachés son por contenido: (fuente, tema) para el SVG y (fuente, tema, escala) para la
//!   imagen. Reabrir la nota, pasar a Dividida o volver a una diapositiva no dibuja nada.
//! - Sin diagramas no se carga nada: las fuentes de la imagen se leen con el primero.

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, LazyLock, Mutex, OnceLock};
use std::time::{Duration, Instant};

use gtk::{gdk, gio, glib, prelude::*};
pub use orbital_diagramas::{Diagrama, Error, Tema};

/// Límite de la caché de imágenes, en bytes de píxeles.
const MAX_BYTES: usize = 64 * 1024 * 1024;
/// Entradas de la caché de SVG.
const MAX_SVG: usize = 256;
/// Tiempo que [`precalentar`] puede gastar por documento: lo que no entra se dibuja después, ya
/// con la nota en pantalla.
const TOPE_PRECALENTAR: Duration = Duration::from_millis(250);

/// Tema y escala de los diagramas de una vista.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Estilo {
    pub tema: Tema,
    pub escala: u32,
}

impl Estilo {
    /// El de una vista: claro u oscuro según su color de texto, y la escala de su monitor. De
    /// paso anota la letra de la interfaz para las imágenes.
    pub fn de(w: &impl IsA<gtk::Widget>) -> Self {
        let w = w.as_ref();
        fijar_familia(w);
        Self::del_color(w.color(), w.scale_factor())
    }

    /// Con el color del texto de la vista (letra clara: fondo oscuro).
    pub fn del_color(c: gdk::RGBA, escala: i32) -> Self {
        let luz = 0.2126 * c.red() + 0.7152 * c.green() + 0.0722 * c.blue();
        Self {
            tema: if luz > 0.5 { Tema::Oscuro } else { Tema::Claro },
            escala: escala.clamp(1, 4) as u32,
        }
    }
}

static FAMILIA: OnceLock<String> = OnceLock::new();

/// La familia de letra de la interfaz (la primera vista que dibuja un diagrama la anota).
fn fijar_familia(w: &gtk::Widget) {
    if FAMILIA.get().is_none() {
        let fam = w
            .pango_context()
            .font_description()
            .and_then(|fd| fd.family().map(|f| f.to_string()))
            .unwrap_or_else(|| "sans-serif".to_owned());
        let _ = FAMILIA.set(fam);
    }
}

fn fuentes() -> Arc<orbital_diagramas::Fuentes> {
    orbital_diagramas::Fuentes::para(FAMILIA.get().map_or("sans-serif", String::as_str))
}

// --- SVG ------------------------------------------------------------------------------------

type ResultadoSvg = Result<Arc<Diagrama>, Error>;

#[derive(Default)]
struct CacheSvg {
    mapa: HashMap<(Arc<str>, Tema), ResultadoSvg>,
    orden: VecDeque<(Arc<str>, Tema)>,
}

static SVGS: LazyLock<Mutex<CacheSvg>> = LazyLock::new(Mutex::default);

/// El diagrama como SVG, con caché por (fuente, tema). Cualquier hilo.
pub fn svg(fuente: &str, tema: Tema) -> ResultadoSvg {
    let fuente = fuente.strip_suffix('\n').unwrap_or(fuente);
    let clave = (Arc::<str>::from(fuente), tema);
    if let Some(r) = SVGS.lock().ok().and_then(|c| c.mapa.get(&clave).cloned()) {
        return r;
    }
    let r = orbital_diagramas::svg(fuente, tema).map(Arc::new);
    if let Ok(mut c) = SVGS.lock() {
        if c.mapa.insert(clave.clone(), r.clone()).is_none() {
            c.orden.push_back(clave);
        }
        while c.orden.len() > MAX_SVG {
            if let Some(viejo) = c.orden.pop_front() {
                c.mapa.remove(&viejo);
            }
        }
    }
    r
}

// --- Imágenes -------------------------------------------------------------------------------

/// Lo que identifica una imagen.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Clave {
    fuente: Arc<str>,
    estilo: Estilo,
}

impl Clave {
    pub fn new(fuente: &str, estilo: Estilo) -> Self {
        let fuente = fuente.strip_suffix('\n').unwrap_or(fuente);
        Self {
            fuente: Arc::from(fuente),
            estilo,
        }
    }

    pub fn fuente(&self) -> &str {
        &self.fuente
    }
}

/// Una imagen dibujada, compartida entre hilos (los píxeles van en `glib::Bytes`: la textura los
/// usa sin copiarlos).
pub struct Dibujo {
    pub ancho: u32,
    pub alto: u32,
    bytes: glib::Bytes,
}

impl Dibujo {
    /// La textura (en el hilo de GTK).
    pub fn textura(&self) -> gdk::Texture {
        gdk::MemoryTexture::new(
            self.ancho as i32,
            self.alto as i32,
            gdk::MemoryFormat::R8g8b8a8Premultiplied,
            &self.bytes,
            self.ancho as usize * 4,
        )
        .upcast()
    }
}

pub type Resultado = Result<Arc<Dibujo>, Error>;

#[derive(Default)]
struct Cache {
    mapa: HashMap<Clave, Resultado>,
    orden: VecDeque<Clave>,
    bytes: usize,
}

static CACHE: LazyLock<Mutex<Cache>> = LazyLock::new(Mutex::default);

/// Lo que ya está dibujado (o falló).
pub fn en_cache(clave: &Clave) -> Option<Resultado> {
    CACHE.lock().ok()?.mapa.get(clave).cloned()
}

/// Dibuja (si hace falta) y guarda en la caché. Desde cualquier hilo menos el de GTK (la primera
/// vez lee las fuentes).
pub fn dibujar(clave: &Clave) -> Resultado {
    if let Some(r) = en_cache(clave) {
        return r;
    }
    let r = svg(&clave.fuente, clave.estilo.tema).and_then(|d| {
        let img = orbital_diagramas::imagen(&d, clave.estilo.escala as f32, &fuentes())?;
        Ok(Arc::new(Dibujo {
            ancho: img.ancho,
            alto: img.alto,
            bytes: glib::Bytes::from_owned(img.rgba),
        }))
    });
    if let Ok(mut c) = CACHE.lock() {
        if let Ok(d) = &r {
            c.bytes += d.bytes.len();
        }
        if c.mapa.insert(clave.clone(), r.clone()).is_none() {
            c.orden.push_back(clave.clone());
        }
        while c.bytes > MAX_BYTES {
            let Some(vieja) = c.orden.pop_front() else {
                break;
            };
            if let Some(Ok(d)) = c.mapa.remove(&vieja) {
                c.bytes -= d.bytes.len();
            }
        }
    }
    r
}

/// Dibuja de antemano los diagramas de un documento (en el hilo de fondo que lo prepara), hasta
/// [`TOPE_PRECALENTAR`]: los que quedan se dibujan con la nota ya en pantalla.
pub fn precalentar(claves: impl IntoIterator<Item = Clave>) {
    let desde = Instant::now();
    for c in claves {
        if desde.elapsed() > TOPE_PRECALENTAR {
            break;
        }
        let _ = dibujar(&c);
    }
}

/// El widget de un diagrama: la imagen (ya en la caché o pedida al pool de `gio`) con su nombre
/// accesible y el fuente como descripción. Mide `ancho`×`alto` (lo decide quien lo pone).
pub fn picture(clave: Clave, nombre: &str) -> gtk::Picture {
    let pic = gtk::Picture::builder()
        .can_shrink(true)
        .content_fit(gtk::ContentFit::Contain)
        .halign(gtk::Align::Start)
        .css_classes(["orbital-diagrama"])
        .build();
    pic.set_alternative_text(Some(nombre));
    pic.update_property(&[
        gtk::accessible::Property::Label(nombre),
        gtk::accessible::Property::Description(clave.fuente()),
    ]);
    match en_cache(&clave) {
        Some(Ok(d)) => pic.set_paintable(Some(&d.textura())),
        Some(Err(_)) => {}
        None => {
            let debil = pic.downgrade();
            glib::spawn_future_local(async move {
                let Ok(Ok(d)) = gio::spawn_blocking(move || dibujar(&clave)).await else {
                    return;
                };
                if let Some(pic) = debil.upgrade() {
                    pic.set_paintable(Some(&d.textura()));
                }
            });
        }
    }
    pic
}

/// Ancho y alto con que se muestra un diagrama de tamaño natural `ancho`×`alto` en una columna
/// de `columna` píxeles: el natural, o achicado hasta entrar.
pub fn medida(ancho: f32, alto: f32, columna: i32) -> (i32, i32) {
    let col = columna.max(80) as f32;
    let k = (col / ancho.max(1.0)).min(1.0);
    (
        (ancho * k).round().max(1.0) as i32,
        (alto * k).round().max(1.0) as i32,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn medida_achica_lo_ancho() {
        assert_eq!(medida(400.0, 200.0, 800), (400, 200));
        assert_eq!(medida(1600.0, 400.0, 800), (800, 200));
    }

    #[test]
    fn cache_por_contenido() {
        let estilo = Estilo {
            tema: Tema::Oscuro,
            escala: 1,
        };
        let c = Clave::new("pie\n  \"A\" : 1\n", estilo);
        assert!(en_cache(&c).is_none());
        let d = dibujar(&c).unwrap();
        assert!(d.ancho > 0 && d.alto > 0);
        assert!(Arc::ptr_eq(&d, &en_cache(&c).unwrap().unwrap()));
        let Err(e) = dibujar(&Clave::new("graph TD\n A -->> B[\n", estilo)) else {
            panic!("debía fallar");
        };
        assert_eq!(e.renglon, Some(2));
    }
}
