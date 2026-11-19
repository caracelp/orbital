//! Los gráficos 3D (053) en el lienzo: el mismo widget que los 2D ([`Lienzo`]) cuando el bloque
//! tiene una serie del espacio.
//!
//! - El muestreo (`orbital_graficos::espacio`) va al pool de `gio`, de a uno por gráfico (el
//!   último pedido gana). Con implícitas, primero una grilla gruesa y después la fina: el primer
//!   cuadro sale enseguida y nunca traba.
//! - El pintado de los triángulos (`orbital_graficos::raster`, con búfer de profundidad) también
//!   va al pool, de a uno por gráfico: mientras se gira, a tamaño justo; quieto, al doble y
//!   promediado (bordes suaves). En el hilo de GTK solo queda poner la textura y dibujar con GSK
//!   la caja, las curvas y los puntos (tapados donde la superficie está delante) y los rótulos.
//! - Arrastrar gira, la rueda acerca, el doble clic y 0 vuelven a la cámara inicial; con el
//!   teclado, flechas, + y −. Bajo el puntero, las coordenadas del punto de la superficie.
//!   Nada toca la nota.

use std::time::Duration;

use orbital_graficos::camara::{Camara, Proyeccion, guias3};
use orbital_graficos::espacio::{self, Caja, Escena};
use orbital_graficos::raster::{self, Cuadro, Relleno};

use super::*;

/// Después del último giro, cuánto esperar para pintar la imagen suavizada.
const ESPERA_SUAVE: Duration = Duration::from_millis(140);
/// Grados por píxel arrastrado.
const GRADOS_POR_PX: f64 = 0.45;

/// Lo pintado que se muestra, con la cámara y el tamaño con que se pintó.
pub(super) struct Mostrado {
    proy: Proyeccion,
    /// Píxeles del dispositivo por píxel del widget.
    escala: f32,
    textura: gdk::MemoryTexture,
    cuadro: Cuadro,
    escena: Arc<Escena>,
}

/// El estado de un gráfico 3D.
pub(super) struct Estado {
    pub camara: Camara,
    inicial: Camara,
    escena: Option<Arc<Escena>>,
    /// La caja del primer muestreo: los deslizadores no la hacen saltar.
    caja: Option<Caja>,
    niveles: Vec<u32>,
    nivel: usize,
    muestreando: bool,
    muestreo_pendiente: bool,
    pintando: bool,
    /// Un pedido de pintado que llegó mientras se pintaba (con su factor).
    pintar_pendiente: Option<usize>,
    mostrado: Option<Rc<Mostrado>>,
    suavizar: Option<glib::SourceId>,
    rellenos: Arc<Vec<Relleno>>,
    pub arrastre: Option<Camara>,
    pub pintados: u64,
}

impl Estado {
    pub fn new(g: &Grafico) -> Estado {
        let c = Camara::inicial(g);
        Estado {
            camara: c,
            inicial: c,
            escena: None,
            caja: None,
            niveles: if espacio::tiene_implicitas(g) {
                espacio::niveles_implicitas(g)
            } else {
                vec![espacio::RESOLUCION_IMPLICITAS]
            },
            nivel: 0,
            muestreando: false,
            muestreo_pendiente: false,
            pintando: false,
            pintar_pendiente: None,
            mostrado: None,
            suavizar: None,
            rellenos: Arc::new(raster::rellenos(g, Paleta::Oscura)),
            arrastre: None,
            pintados: 0,
        }
    }
}

/// (fuente) → la escena de la vista inicial, ya refinada, con los valores iniciales.
type CacheEscenas = Mutex<Lru<Arc<str>, Arc<Escena>>>;
static ESCENAS: LazyLock<CacheEscenas> = LazyLock::new(|| Mutex::new(Lru::new()));

impl Lienzo {
    pub(super) fn tres(&self) -> std::cell::RefMut<'_, Estado> {
        std::cell::RefMut::map(self.imp().tres.borrow_mut(), |t| {
            t.as_deref_mut().expect("gráfico 3D")
        })
    }

    /// ¿Es un gráfico 3D?
    pub fn es_3d(&self) -> bool {
        self.imp().tres.borrow().is_some()
    }

    /// La cámara (3D, para las pruebas).
    pub fn camara(&self) -> Option<Camara> {
        self.imp().tres.borrow().as_ref().map(|t| t.camara)
    }

    /// Cuántas imágenes se pintaron (3D, para las pruebas y las mediciones).
    pub fn pintados(&self) -> u64 {
        self.imp().tres.borrow().as_ref().map_or(0, |t| t.pintados)
    }

    /// La resolución con que se muestrearon las implícitas de lo que se ve (0 si no hay).
    pub fn resolucion_implicitas(&self) -> u32 {
        self.imp()
            .tres
            .borrow()
            .as_ref()
            .and_then(|t| t.escena.as_ref().map(|e| e.resolucion_implicitas))
            .unwrap_or(0)
    }

    pub(super) fn ocupado_3d(&self) -> bool {
        self.imp().tres.borrow().as_ref().is_some_and(|t| {
            t.muestreando || t.muestreo_pendiente || t.pintando || t.pintar_pendiente.is_some()
        })
    }

    pub(super) fn al_cambiar_tamano_3d(&self) {
        let hay = self.tres().escena.is_some();
        if hay {
            self.pedir_cuadro(2);
        } else {
            self.muestrear_3d();
        }
    }

    /// Muestrea la escena fuera del hilo de GTK (uno por vez; el último pedido gana), en el nivel
    /// de refinamiento actual; al terminar, pide el siguiente nivel.
    pub(super) fn muestrear_3d(&self) {
        let imp = self.imp();
        let Some(g) = self.grafico() else { return };
        let fuente = imp.fuente.borrow().clone().unwrap_or_else(|| Arc::from(""));
        let iniciales = imp.params_iniciales.get();
        let (nivel, implicitas, ultimo, caja) = {
            let mut t = self.tres();
            if t.muestreando {
                t.muestreo_pendiente = true;
                return;
            }
            if iniciales
                && t.escena.is_none()
                && let Some(e) = ESCENAS.lock().ok().and_then(|c| c.get(&fuente))
            {
                t.nivel = t.niveles.len() - 1;
                t.caja = Some(e.caja);
                t.escena = Some(e);
                drop(t);
                imp.muestreos.set(imp.muestreos.get() + 1);
                self.pedir_cuadro(2);
                return;
            }
            t.muestreando = true;
            let ultimo = t.nivel + 1 >= t.niveles.len();
            (t.nivel, t.niveles[t.nivel], ultimo, t.caja)
        };
        let params = imp.params.borrow().clone();
        let generacion = imp.generacion.get();
        let weak = self.downgrade();
        glib::spawn_future_local(async move {
            let g2 = g.clone();
            let r = gio::spawn_blocking(move || {
                Arc::new(espacio::escena(
                    &g2,
                    &params,
                    &espacio::Opciones {
                        superficies: None,
                        implicitas,
                        caja,
                    },
                ))
            })
            .await;
            let Some(this) = weak.upgrade() else { return };
            let imp = this.imp();
            let pendiente;
            {
                let mut t = this.tres();
                t.muestreando = false;
                pendiente = std::mem::take(&mut t.muestreo_pendiente);
                if let Ok(e) = r
                    && imp.generacion.get() == generacion
                    && !pendiente
                {
                    if t.caja.is_none() {
                        t.caja = Some(e.caja);
                    }
                    if ultimo
                        && iniciales
                        && imp.params_iniciales.get()
                        && let Ok(mut c) = ESCENAS.lock()
                    {
                        c.put(fuente, e.clone());
                    }
                    t.escena = Some(e);
                    if !ultimo {
                        t.nivel = nivel + 1;
                    }
                }
            }
            if pendiente {
                // Llegó otro pedido (un deslizador): de nuevo desde la grilla gruesa.
                this.tres().nivel = 0;
                this.muestrear_3d();
                return;
            }
            imp.muestreos.set(imp.muestreos.get() + 1);
            this.pedir_cuadro(if ultimo { 2 } else { 1 });
            if !ultimo {
                this.muestrear_3d();
            }
        });
    }

    pub(super) fn fijar_parametro_3d(&self) {
        {
            let mut t = self.tres();
            t.nivel = 0;
        }
        self.muestrear_3d();
    }

    /// Pinta la escena con la cámara actual fuera del hilo de GTK (uno por vez; si llegan más
    /// pedidos mientras tanto, se hace solo el último). `factor` 2: suavizado.
    fn pedir_cuadro(&self, factor: usize) {
        let (w, h) = (self.width() as f32, self.height() as f32);
        if w < 1.0 || h < 1.0 {
            return;
        }
        let escala = self.scale_factor().max(1) as f32;
        let (escena, rellenos, camara) = {
            let mut t = self.tres();
            if let Some(id) = t.suavizar.take() {
                id.remove();
            }
            let Some(e) = t.escena.clone() else { return };
            if t.pintando {
                t.pintar_pendiente = Some(t.pintar_pendiente.map_or(factor, |f| f.min(factor)));
                return;
            }
            t.pintando = true;
            (e, t.rellenos.clone(), t.camara)
        };
        let proy = Proyeccion::new(&camara, w, h);
        let dispositivo = proy.por(escala);
        let weak = self.downgrade();
        glib::spawn_future_local(async move {
            let e2 = escena.clone();
            let r = gio::spawn_blocking(move || {
                raster::pintar(&e2, &rellenos, &dispositivo, Paleta::Oscura, factor)
            })
            .await;
            let Some(this) = weak.upgrade() else { return };
            let pendiente;
            {
                let mut t = this.tres();
                t.pintando = false;
                pendiente = t.pintar_pendiente.take();
                if let Ok(cuadro) = r {
                    let bytes = glib::Bytes::from(&cuadro.bytes[..]);
                    let textura = gdk::MemoryTexture::new(
                        cuadro.ancho as i32,
                        cuadro.alto as i32,
                        gdk::MemoryFormat::B8g8r8a8Premultiplied,
                        &bytes,
                        cuadro.ancho * 4,
                    );
                    t.mostrado = Some(Rc::new(Mostrado {
                        proy,
                        escala,
                        textura,
                        cuadro,
                        escena,
                    }));
                    t.pintados += 1;
                }
            }
            this.queue_draw();
            if let Some(f) = this.imp().al_puntero.borrow().as_ref() {
                f(this.imp().puntero.get());
            }
            match pendiente {
                Some(f) => this.pedir_cuadro(f),
                None if factor == 1 => this.programar_suavizado(),
                None => {}
            }
        });
    }

    /// Después de un rato quieto, la imagen suavizada.
    fn programar_suavizado(&self) {
        let weak = self.downgrade();
        let id = glib::timeout_add_local_once(ESPERA_SUAVE, move || {
            if let Some(l) = weak.upgrade() {
                l.tres().suavizar = None;
                l.pedir_cuadro(2);
            }
        });
        if let Some(viejo) = self.tres().suavizar.replace(id) {
            viejo.remove();
        }
    }

    /// Mira desde otra cámara (girar, acercar).
    pub fn fijar_camara(&self, c: Camara) {
        let era_inicial = {
            let mut t = self.tres();
            if t.camara == c {
                return;
            }
            let era = t.camara == t.inicial;
            t.camara = c;
            era
        };
        if era_inicial && let Some(f) = self.imp().al_cambiar_vista.borrow().as_ref() {
            f(false);
        }
        self.pedir_cuadro(1);
    }

    pub(super) fn restablecer_3d(&self) {
        {
            let mut t = self.tres();
            t.camara = t.inicial;
        }
        if let Some(f) = self.imp().al_cambiar_vista.borrow().as_ref() {
            f(true);
        }
        self.pedir_cuadro(2);
    }

    pub(super) fn arrastrar_3d(&self, dx: f64, dy: f64) {
        let Some(c) = self.tres().arrastre else {
            return;
        };
        self.fijar_camara(c.girar(-dx * GRADOS_POR_PX, dy * GRADOS_POR_PX));
    }

    pub(super) fn acercar_3d(&self, factor: f64) {
        let c = self.tres().camara;
        self.fijar_camara(c.acercar(1.0 / factor));
    }

    pub(super) fn tecla_3d(&self, tecla: gdk::Key) -> bool {
        let c = self.tres().camara;
        let paso = 10.0;
        match tecla {
            gdk::Key::Left | gdk::Key::KP_Left => self.fijar_camara(c.girar(paso, 0.0)),
            gdk::Key::Right | gdk::Key::KP_Right => self.fijar_camara(c.girar(-paso, 0.0)),
            gdk::Key::Up | gdk::Key::KP_Up => self.fijar_camara(c.girar(0.0, -paso)),
            gdk::Key::Down | gdk::Key::KP_Down => self.fijar_camara(c.girar(0.0, paso)),
            gdk::Key::plus | gdk::Key::equal | gdk::Key::KP_Add => {
                self.fijar_camara(c.acercar(1.25))
            }
            gdk::Key::minus | gdk::Key::KP_Subtract => self.fijar_camara(c.acercar(1.0 / 1.25)),
            gdk::Key::_0 | gdk::Key::KP_0 | gdk::Key::Home => self.restablecer(),
            _ => return false,
        }
        true
    }

    /// El punto de la superficie bajo el píxel (x, y) del widget, en las unidades del gráfico.
    pub fn punto_bajo(&self, x: f64, y: f64) -> Option<[f64; 3]> {
        let t = self.imp().tres.borrow();
        let m = t.as_ref()?.mostrado.clone()?;
        let (x, y) = (x as f32, y as f32);
        let d = m.cuadro.profundidad(x * m.escala, y * m.escala)?;
        Some(m.escena.caja.mundo(m.proy.desde_px(x, y, d)))
    }

    /// Lo que se dice bajo el puntero: el punto de la superficie o, si no hay, la cámara.
    pub(super) fn texto_puntero_3d(&self, p: Option<(f64, f64)>) -> String {
        let f = |v: f64| orbital_graficos::ejes::rotulo(v, 1e-3);
        if let Some((x, y)) = p
            && let Some(q) = self.punto_bajo(x, y)
        {
            return format!("x = {}   y = {}   z = {}", f(q[0]), f(q[1]), f(q[2]));
        }
        let c = self.tres().camara;
        format!(
            "Giro {}°   elevación {}°",
            f(c.giro.round()),
            f(c.elevacion.round())
        )
    }

    pub(super) fn dibujar_3d(&self, s: &gtk::Snapshot, g: &Grafico, w: f32, h: f32) {
        use crate::style;
        let mostrado = self.tres().mostrado.clone();
        let Some(m) = mostrado else { return };
        let p = &m.proy;
        let gu = guias3(&m.escena.caja, p, g.ejes);
        let fino = gsk::Stroke::new(1.0);
        let lineas = |ls: &[[f32; 4]], color: gdk::RGBA| {
            if ls.is_empty() {
                return;
            }
            let pb = gsk::PathBuilder::new();
            for l in ls {
                pb.move_to(l[0], l[1]);
                pb.line_to(l[2], l[3]);
            }
            s.append_stroke(&pb.to_path(), &fino, &color);
        };
        lineas(&gu.grilla, rgba(style::TEXT_FG, 0.07));
        lineas(&gu.bordes, rgba(style::TEXT_FG, 0.24));
        // Lo pintado se estira si el widget cambió de tamaño mientras se pintaba otro.
        s.append_texture(&m.textura, &graphene::Rect::new(0.0, 0.0, w, h));
        let (fx, fy) = (w / p.ancho, h / p.alto);
        let visible = |q: [f32; 3]| m.cuadro.visible(q[0] * m.escala, q[1] * m.escala, q[2]);
        for (k, (serie, pz)) in g.series.iter().zip(&m.escena.piezas).enumerate() {
            let color = rgba3(color_de_serie(g, k, Paleta::Oscura));
            if serie.estilo != Estilo::Puntos && !pz.lineas.is_empty() {
                let pb = gsk::PathBuilder::new();
                let mut algo = false;
                for l in &pz.lineas {
                    let mut abierto = false;
                    let mut previo = p.px(l[0]);
                    for q in &l[1..] {
                        let actual = p.px(*q);
                        let medio = [
                            (previo[0] + actual[0]) / 2.0,
                            (previo[1] + actual[1]) / 2.0,
                            (previo[2] + actual[2]) / 2.0,
                        ];
                        if visible(medio) {
                            if !abierto {
                                pb.move_to(previo[0] * fx, previo[1] * fy);
                                abierto = true;
                            }
                            pb.line_to(actual[0] * fx, actual[1] * fy);
                            algo = true;
                        } else {
                            abierto = false;
                        }
                        previo = actual;
                    }
                }
                if algo {
                    let trazo = gsk::Stroke::new(serie.grosor.unwrap_or(2.0) as f32);
                    trazo.set_line_join(gsk::LineJoin::Round);
                    trazo.set_line_cap(gsk::LineCap::Round);
                    if serie.estilo == Estilo::Discontinua {
                        trazo.set_dash(&[7.0, 5.0]);
                    }
                    s.append_stroke(&pb.to_path(), &trazo, &color);
                }
            }
            if !pz.puntos.is_empty() && (serie.estilo != Estilo::Linea || pz.lineas.is_empty()) {
                let pb = gsk::PathBuilder::new();
                let mut algo = false;
                for q in &pz.puntos {
                    let a = p.px(*q);
                    if visible(a) {
                        pb.add_circle(&graphene::Point::new(a[0] * fx, a[1] * fy), 3.5);
                        algo = true;
                    }
                }
                if algo {
                    s.append_fill(&pb.to_path(), gsk::FillRule::Winding, &color);
                }
            }
        }
        // Rótulos de la caja.
        let mut fd = self.pango_context().font_description().unwrap_or_default();
        fd.set_size(9 * pango::SCALE);
        let tenue = rgba(style::MUTED_FG, 1.0);
        let mut fdn = fd.clone();
        fdn.set_weight(pango::Weight::Bold);
        for (r, fuerte) in gu
            .rotulos
            .iter()
            .map(|r| (r, false))
            .chain(gu.nombres.iter().map(|r| (r, true)))
        {
            let layout = self.create_pango_layout(Some(&r.texto));
            layout.set_font_description(Some(if fuerte { &fdn } else { &fd }));
            let (lw, lh) = layout.pixel_size();
            let (lw, lh) = (lw as f32, lh as f32);
            let (rx, ry) = (r.x as f32 * fx, r.y as f32 * fy);
            let (x, y) = match r.ancla {
                guias::Ancla::Arriba => (rx - lw / 2.0, ry),
                guias::Ancla::Abajo => (rx - lw / 2.0, ry - lh),
                guias::Ancla::Derecha => (rx - lw, ry - lh / 2.0),
                guias::Ancla::Izquierda => (rx, ry - lh / 2.0),
            };
            s.save();
            s.translate(&graphene::Point::new(x, y));
            s.append_layout(&layout, &tenue);
            s.restore();
        }
    }

    /// La muestra de color de la serie `k` en la leyenda (el degradé para las superficies por
    /// altura).
    pub(super) fn muestra_3d(&self, s: &gtk::Snapshot, g: &Grafico, k: usize, x: f32, y: f32) {
        let relleno = self.tres().rellenos.get(k).copied();
        match relleno {
            Some(Relleno::Altura) => {
                for i in 0..7 {
                    let c = raster::color_altura(i as f32 / 6.0, Paleta::Oscura);
                    s.append_color(
                        &gdk::RGBA::new(c[0] / 255.0, c[1] / 255.0, c[2] / 255.0, 1.0),
                        &graphene::Rect::new(x + i as f32 * 2.0, y, 2.0, 3.0),
                    );
                }
            }
            _ => s.append_color(
                &rgba3(color_de_serie(g, k, Paleta::Oscura)),
                &graphene::Rect::new(x, y, 14.0, 3.0),
            ),
        }
    }
}
