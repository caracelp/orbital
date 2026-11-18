//! Fórmulas LaTeX en la app (050): el motor es `orbital_formulas` (sin GTK); acá viven la caché
//! de imágenes, las texturas y el pintable que la Lectura mete en el texto.
//!
//! - Las imágenes se dibujan fuera del hilo de GTK: al preparar el documento
//!   ([`precalentar`], en el hilo de fondo de la Lectura) o, si falta alguna al volcar, por tandas
//!   en el pool de `gio` ([`Formula::pedir`]). Al volcar solo se crean texturas de lo ya dibujado.
//! - La caché es por (LaTeX, modo, tamaño, escala, color) y vale para toda la app: reabrir una
//!   nota o volver a una diapositiva no dibuja nada.
//! - Sin fórmulas no se carga nada: las fuentes del motor se leen con la primera.

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, LazyLock, Mutex};

use gtk::{gdk, gio, glib, graphene, prelude::*, subclass::prelude::*};
pub use orbital_formulas::Modo;

/// Las fórmulas se ven un poco más grandes que el texto: las letras de TeX son más chicas que las
/// de la interfaz con el mismo tamaño.
pub const FACTOR: f32 = 1.12;
/// Límite de la caché de imágenes, en bytes de píxeles.
const MAX_BYTES: usize = 48 * 1024 * 1024;

/// Tamaño, escala y color de las fórmulas de una vista.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Estilo {
    /// Tamaño de la letra del texto, en píxeles lógicos.
    pub tamano: f32,
    pub escala: f32,
    pub color: [u8; 4],
}

impl Estilo {
    /// El de una vista: su letra, su color y el factor de escala de su monitor.
    pub fn de(w: &impl IsA<gtk::Widget>) -> Self {
        let w = w.as_ref();
        let fd = w.pango_context().font_description();
        let tamano = fd.map_or(15.0, |fd| {
            let s = fd.size() as f32 / gtk::pango::SCALE as f32;
            // Un tamaño en puntos se pasa a píxeles (96 ppp).
            if fd.is_size_absolute() {
                s
            } else {
                s * 96.0 / 72.0
            }
        });
        let c = w.color();
        let b = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
        Self {
            tamano: if tamano > 1.0 { tamano } else { 15.0 },
            escala: w.scale_factor().max(1) as f32,
            color: [b(c.red()), b(c.green()), b(c.blue()), b(c.alpha())],
        }
    }

    /// El mismo estilo con la letra multiplicada (encabezados, partes de la nota).
    pub fn por(self, factor: f32) -> Self {
        Self {
            tamano: self.tamano * factor,
            ..self
        }
    }
}

/// Lo que identifica una imagen en la caché.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Clave {
    tex: Arc<str>,
    modo: Modo,
    /// Tamaño en centésimas de píxel.
    tamano: u32,
    escala: u32,
    color: [u8; 4],
}

impl Clave {
    pub fn new(tex: &str, modo: Modo, estilo: &Estilo) -> Self {
        Self {
            tex: Arc::from(tex),
            modo,
            tamano: (estilo.tamano * FACTOR * 100.0).round() as u32,
            escala: estilo.escala.round() as u32,
            color: estilo.color,
        }
    }

    fn estilo(&self) -> orbital_formulas::Estilo {
        orbital_formulas::Estilo {
            tamano: self.tamano as f32 / 100.0,
            escala: self.escala as f32,
            color: self.color,
        }
    }

    pub fn tex(&self) -> &str {
        &self.tex
    }
}

/// Una imagen dibujada, compartida entre hilos (los píxeles van en `glib::Bytes`: la textura los
/// usa sin copiarlos).
pub struct Dibujo {
    pub ancho: u32,
    pub alto: u32,
    pub escala: f32,
    /// Lo que baja de la línea base, en píxeles lógicos.
    pub profundidad: f32,
    bytes: glib::Bytes,
}

impl Dibujo {
    pub fn ancho_logico(&self) -> f64 {
        f64::from(self.ancho) / f64::from(self.escala)
    }
    pub fn alto_logico(&self) -> f64 {
        f64::from(self.alto) / f64::from(self.escala)
    }
    /// La textura (en el hilo de GTK).
    pub fn textura(&self) -> gdk::Texture {
        gdk::MemoryTexture::new(
            self.ancho as i32,
            self.alto as i32,
            gdk::MemoryFormat::R8g8b8a8,
            &self.bytes,
            self.ancho as usize * 4,
        )
        .upcast()
    }
}

pub type Resultado = Result<Arc<Dibujo>, orbital_formulas::Error>;

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

/// Dibuja (si hace falta) y guarda en la caché. Se puede llamar desde cualquier hilo; no desde el
/// de GTK salvo para una sola fórmula.
pub fn dibujar(clave: &Clave) -> Resultado {
    if let Some(r) = en_cache(clave) {
        return r;
    }
    let r = orbital_formulas::dibujar(&clave.tex, clave.modo, &clave.estilo()).map(|img| {
        Arc::new(Dibujo {
            ancho: img.ancho,
            alto: img.alto,
            escala: img.escala,
            profundidad: img.profundidad(),
            bytes: glib::Bytes::from_owned(img.rgba),
        })
    });
    if let Ok(mut c) = CACHE.lock() {
        if let Ok(d) = &r {
            c.bytes += d.bytes.len();
        }
        c.orden.push_back(clave.clone());
        c.mapa.insert(clave.clone(), r.clone());
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

type CacheSvg = HashMap<(Arc<str>, Modo), Result<String, orbital_formulas::Error>>;
static SVGS: LazyLock<Mutex<CacheSvg>> = LazyLock::new(Mutex::default);

/// El SVG de una fórmula (notas `.html`), con caché: al editar en Visual se vuelven a pedir todas.
pub fn svg(tex: &str, modo: Modo) -> Result<String, orbital_formulas::Error> {
    let clave = (Arc::from(tex), modo);
    if let Some(r) = SVGS.lock().ok().and_then(|c| c.get(&clave).cloned()) {
        return r;
    }
    let r = orbital_formulas::svg(tex, modo).map(|s| s.marcado);
    if let Ok(mut c) = SVGS.lock() {
        if c.len() > 4096 {
            c.clear();
        }
        c.insert(clave, r.clone());
    }
    r
}

/// Dibuja de antemano las fórmulas de un documento (en el hilo de fondo que lo prepara).
pub fn precalentar(claves: impl IntoIterator<Item = Clave>) {
    for c in claves {
        let _ = dibujar(&c);
    }
}

thread_local! {
    /// Fórmulas que esperan su imagen, para pedirlas en una sola tanda.
    static PENDIENTES: RefCell<Vec<(Clave, Formula)>> = const { RefCell::new(Vec::new()) };
    static PEDIDO: Cell<bool> = const { Cell::new(false) };
}

mod imp {
    use super::*;

    #[derive(Default)]
    pub struct Formula {
        pub ancho: Cell<f64>,
        pub alto: Cell<f64>,
        pub textura: RefCell<Option<gdk::Texture>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for Formula {
        const NAME: &'static str = "OrbitalFormula";
        type Type = super::Formula;
        type Interfaces = (gdk::Paintable,);
    }

    impl ObjectImpl for Formula {}

    impl PaintableImpl for Formula {
        fn intrinsic_width(&self) -> i32 {
            self.ancho.get().ceil() as i32
        }
        fn intrinsic_height(&self) -> i32 {
            self.alto.get().ceil() as i32
        }
        fn flags(&self) -> gdk::PaintableFlags {
            if self.textura.borrow().is_some() {
                gdk::PaintableFlags::STATIC_SIZE | gdk::PaintableFlags::STATIC_CONTENTS
            } else {
                gdk::PaintableFlags::STATIC_SIZE
            }
        }
        fn snapshot(&self, snapshot: &gdk::Snapshot, _w: f64, _h: f64) {
            let Some(t) = self.textura.borrow().clone() else {
                return;
            };
            let Some(s) = snapshot.downcast_ref::<gtk::Snapshot>() else {
                return;
            };
            s.append_texture(
                &t,
                &graphene::Rect::new(0.0, 0.0, self.ancho.get() as f32, self.alto.get() as f32),
            );
        }
    }
}

glib::wrapper! {
    /// Una fórmula dibujada dentro de un `GtkTextView` (`insert_paintable`): una textura de la
    /// caché, sin widgets anclados. Si la imagen todavía no está, se ve vacía con su tamaño final
    /// y se completa sola.
    pub struct Formula(ObjectSubclass<imp::Formula>) @implements gdk::Paintable;
}

impl Formula {
    /// Con la imagen ya dibujada.
    pub fn lista(d: &Dibujo) -> Self {
        let f: Self = glib::Object::new();
        let imp = f.imp();
        imp.ancho.set(d.ancho_logico());
        imp.alto.set(d.alto_logico());
        imp.textura.replace(Some(d.textura()));
        f
    }

    /// Vacía, del tamaño dado (en píxeles lógicos), hasta que llegue la imagen.
    pub fn esperando(ancho: f64, alto: f64) -> Self {
        let f: Self = glib::Object::new();
        f.imp().ancho.set(ancho);
        f.imp().alto.set(alto);
        f
    }

    fn completar(&self, d: &Dibujo) {
        let imp = self.imp();
        imp.textura.replace(Some(d.textura()));
        let cambia_tamano = (imp.ancho.get() - d.ancho_logico()).abs() > 0.5
            || (imp.alto.get() - d.alto_logico()).abs() > 0.5;
        imp.ancho.set(d.ancho_logico());
        imp.alto.set(d.alto_logico());
        if cambia_tamano {
            self.invalidate_size();
        }
        self.invalidate_contents();
    }

    /// Pide la imagen de `clave` fuera del hilo de GTK; las pedidas en la misma vuelta del bucle
    /// principal van en una sola tanda.
    pub fn pedir(&self, clave: Clave) {
        PENDIENTES.with_borrow_mut(|p| p.push((clave, self.clone())));
        if PEDIDO.replace(true) {
            return;
        }
        glib::idle_add_local_once(|| {
            PEDIDO.set(false);
            let tanda = PENDIENTES.take();
            let (claves, pintables): (Vec<Clave>, Vec<Formula>) = tanda.into_iter().unzip();
            glib::spawn_future_local(async move {
                let Ok(hechos) =
                    gio::spawn_blocking(move || claves.iter().map(dibujar).collect::<Vec<_>>())
                        .await
                else {
                    return;
                };
                for (r, f) in hechos.into_iter().zip(pintables) {
                    if let Ok(d) = r {
                        f.completar(&d);
                    }
                }
            });
        });
    }
}

/// El tamaño que tendrá una fórmula (sin dibujarla), en píxeles lógicos, o su error.
pub fn medir(clave: &Clave) -> Result<(f64, f64, f32), orbital_formulas::Error> {
    let m = orbital_formulas::medir(&clave.tex, clave.modo, &clave.estilo())?;
    Ok((
        f64::from(m.ancho) / f64::from(m.escala),
        f64::from(m.alto) / f64::from(m.escala),
        m.profundidad(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn la_cache_devuelve_lo_mismo_y_guarda_errores() {
        let e = Estilo {
            tamano: 15.0,
            escala: 1.0,
            color: [255, 255, 255, 255],
        };
        let c = Clave::new(r"\frac{1}{2}", Modo::EnLinea, &e);
        let a = dibujar(&c).unwrap();
        let b = en_cache(&c).unwrap().unwrap();
        assert!(Arc::ptr_eq(&a, &b));
        let (w, h, prof) = medir(&c).unwrap();
        assert!((w - a.ancho_logico()).abs() < 0.01 && (h - a.alto_logico()).abs() < 0.01);
        assert!((prof - a.profundidad).abs() < 0.01);
        let mal = Clave::new(r"\frac{1", Modo::EnLinea, &e);
        assert!(dibujar(&mal).is_err());
        assert!(en_cache(&mal).unwrap().is_err());
    }
}
