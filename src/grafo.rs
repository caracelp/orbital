//! Grafo de enlaces de la bóveda (008, 025, 030): canvas con nodos por nota y aristas por enlace
//! resuelto, simulación física Barnes-Hut en otro hilo para el grafo global y grafo local por
//! saltos (1, 2 y 3) en anillos concéntricos.

#[path = "brujula.rs"]
pub mod brujula;
#[path = "canvas.rs"]
pub mod canvas;
#[path = "mapa_mental.rs"]
pub mod mapa_mental;
#[path = "notas_tres_partes.rs"]
pub mod notas_tres_partes;
#[path = "ramas_libres.rs"]
pub mod ramas_libres;

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, VecDeque};
use std::rc::Rc;
use std::sync::{Arc, Condvar, Mutex};

use gtk::prelude::*;
use gtk::subclass::prelude::*;
use gtk::{glib, gsk};

/// Corte de nodos por apertura (SC-1: la bóveda de 5000 entra completa).
pub const MAX_NODOS: usize = 5000;

/// Separación de la cuadrícula inicial (unidades del mundo).
const PASO: f64 = 220.0;
/// Radio base del nodo en unidades del mundo.
const RADIO: f64 = 9.0;

/// Parámetro de apertura Barnes-Hut ($\theta = 0{,}8$).
const THETA_CUAD: f64 = 0.64; // 0.8^2
/// Constante de repulsión entre nodos.
const K_REPULSION: f64 = 9500.0;
/// Constante elástica de las aristas.
const K_RESORTE: f64 = 0.045;
/// Longitud ideal de una arista en unidades del mundo.
const L_IDEAL: f64 = 110.0;
/// Gravedad centrípeta suave hacia el centro del grafo.
const K_GRAVEDAD: f64 = 0.012;
/// Amortiguamiento de velocidad por paso.
const AMORTIGUAMIENTO: f64 = 0.65;
/// Desplazamiento máximo por nodo a temperatura 1.0.
const PASO_MAX: f64 = 48.0;
/// Factor de enfriamiento por iteración (~135 pasos desde 1.0 hasta detenerse).
const ENFRIAMIENTO: f64 = 0.965;
/// Umbral de temperatura por debajo del cual la simulación se detiene sola (0 % CPU).
const TEMP_MIN: f64 = 0.008;
/// Temperatura suave al arrastrar un nodo para que sus vecinos se acomoden.
const TEMP_ARRASTRE: f64 = 0.28;
/// Profundidad máxima del cuadárbol para evitar ciclos con nodos coincidentes.
const MAX_PROF_QUAD: u8 = 18;

/// Datos del grafo: nodos por nota y aristas resueltas, más posiciones iniciales y saltos.
#[derive(Clone, Debug, Default)]
pub struct GrafoDatos {
    /// Rutas de los nodos, en orden determinista.
    pub rutas: Vec<String>,
    /// Índices `(origen, destino)` dentro de `rutas`.
    pub aristas: Vec<(usize, usize)>,
    /// Se recortó al máximo (aviso discreto).
    pub cortado: bool,
    /// Posiciones fijas del grafo local (barra derecha, 025/030); vacío = grafo global.
    pub fijas: Vec<(f64, f64)>,
    /// Nivel de salto en el grafo local (`0` = centro, `1..=3` = anillo); vacío = grafo global.
    pub saltos: Vec<u8>,
}

impl GrafoDatos {
    /// Arma los datos desde `LinkGraph` (ya resuelto) con corte en `MAX_NODOS`.
    pub fn desde(notas: Vec<String>, enlaces: Vec<(String, String)>) -> Self {
        // Ordenar primero y recortar después: el corte es determinista (alfabético).
        let cortado = notas.len() > MAX_NODOS;
        let mut rutas: Vec<String> = notas;
        rutas.sort();
        rutas.truncate(MAX_NODOS);
        let indice: HashMap<&str, usize> = rutas
            .iter()
            .enumerate()
            .map(|(n, r)| (r.as_str(), n))
            .collect();
        let mut aristas: Vec<(usize, usize)> = enlaces
            .into_iter()
            .filter_map(|(a, b)| {
                let ia = indice.get(a.as_str()).copied()?;
                let ib = indice.get(b.as_str()).copied()?;
                (ia != ib).then_some((ia.min(ib), ia.max(ib)))
            })
            .collect();
        aristas.sort();
        aristas.dedup();
        Self {
            rutas,
            aristas,
            cortado,
            fijas: Vec::new(),
            saltos: Vec::new(),
        }
    }

    /// Grafo local de 1 salto (025): `centro` en el origen y sus vecinos directos alrededor.
    pub fn local(centro: String, mut vecinos: Vec<String>, enlaces: &[(String, String)]) -> Self {
        vecinos.sort();
        vecinos.dedup();
        vecinos.retain(|v| *v != centro);
        let mut rutas = Vec::with_capacity(vecinos.len() + 1);
        rutas.push(centro);
        rutas.extend(vecinos);
        let indice: HashMap<&str, usize> = rutas
            .iter()
            .enumerate()
            .map(|(n, r)| (r.as_str(), n))
            .collect();
        let mut aristas: Vec<(usize, usize)> = enlaces
            .iter()
            .filter_map(|(a, b)| {
                let ia = *indice.get(a.as_str())?;
                let ib = *indice.get(b.as_str())?;
                (ia != ib).then_some((ia.min(ib), ia.max(ib)))
            })
            .collect();
        aristas.sort();
        aristas.dedup();
        let n = rutas.len() - 1;
        let mut fijas = vec![(0.0, 0.0)];
        let mut saltos = vec![0u8];
        for k in 0..n {
            let anillo = k / 16;
            let en_anillo = (n - anillo * 16).min(16);
            let angulo = (k % 16) as f64 / en_anillo as f64 * std::f64::consts::TAU
                - std::f64::consts::FRAC_PI_2;
            let radio = 90.0 + anillo as f64 * 60.0;
            fijas.push((radio * angulo.cos(), radio * angulo.sin()));
            saltos.push(1);
        }
        Self {
            rutas,
            aristas,
            cortado: false,
            fijas,
            saltos,
        }
    }

    /// Grafo local por saltos (030): búsqueda en anchura (BFS) hasta `max_saltos` (`1..=3`)
    /// desde `centro`, disponiendo cada nivel en anillos concéntricos con radios crecientes.
    pub fn local_saltos(centro: String, max_saltos: u8, enlaces: &[(String, String)]) -> Self {
        let max_saltos = max_saltos.clamp(1, 3);
        let mut adj: HashMap<&str, Vec<&str>> = HashMap::new();
        for (a, b) in enlaces {
            if a != b {
                adj.entry(a.as_str()).or_default().push(b.as_str());
                adj.entry(b.as_str()).or_default().push(a.as_str());
            }
        }
        let mut dist: HashMap<&str, u8> = HashMap::new();
        let mut cola: VecDeque<(&str, u8)> = VecDeque::new();
        dist.insert(centro.as_str(), 0);
        cola.push_back((centro.as_str(), 0));
        let mut por_nivel: [Vec<String>; 3] = [Vec::new(), Vec::new(), Vec::new()];

        while let Some((actual, d)) = cola.pop_front() {
            if d >= max_saltos {
                continue;
            }
            let sig = d + 1;
            if let Some(vecinos) = adj.get(actual) {
                for &v in vecinos {
                    if !dist.contains_key(v) {
                        dist.insert(v, sig);
                        por_nivel[(sig - 1) as usize].push(v.to_owned());
                        cola.push_back((v, sig));
                    }
                }
            }
        }
        for nivel in &mut por_nivel {
            nivel.sort();
            nivel.dedup();
        }

        let total = 1 + por_nivel.iter().map(Vec::len).sum::<usize>();
        let mut rutas = Vec::with_capacity(total);
        let mut fijas = Vec::with_capacity(total);
        let mut saltos = Vec::with_capacity(total);
        rutas.push(centro);
        fijas.push((0.0, 0.0));
        saltos.push(0);

        let mut radio_base = 90.0;
        for (idx_nivel, nodos_nivel) in por_nivel.into_iter().enumerate() {
            let d = (idx_nivel + 1) as u8;
            if nodos_nivel.is_empty() {
                radio_base += 68.0;
                continue;
            }
            let cap_anillo = 16 * (idx_nivel + 1);
            let n = nodos_nivel.len();
            let sub_anillos = n.div_ceil(cap_anillo);
            for (k, ruta) in nodos_nivel.into_iter().enumerate() {
                let sub = k / cap_anillo;
                let en_sub = (n - sub * cap_anillo).min(cap_anillo);
                let pos_en_sub = k % cap_anillo;
                let desfasaje = (idx_nivel as f64) * 0.25;
                let angulo = pos_en_sub as f64 / en_sub as f64 * std::f64::consts::TAU
                    - std::f64::consts::FRAC_PI_2
                    + desfasaje;
                let radio = radio_base + sub as f64 * 56.0;
                rutas.push(ruta);
                fijas.push((radio * angulo.cos(), radio * angulo.sin()));
                saltos.push(d);
            }
            radio_base += (sub_anillos.saturating_sub(1) as f64) * 56.0 + 68.0;
        }

        let indice: HashMap<&str, usize> = rutas
            .iter()
            .enumerate()
            .map(|(n, r)| (r.as_str(), n))
            .collect();
        let mut aristas: Vec<(usize, usize)> = enlaces
            .iter()
            .filter_map(|(a, b)| {
                let ia = *indice.get(a.as_str())?;
                let ib = *indice.get(b.as_str())?;
                (ia != ib).then_some((ia.min(ib), ia.max(ib)))
            })
            .collect();
        aristas.sort();
        aristas.dedup();

        Self {
            rutas,
            aristas,
            cortado: false,
            fijas,
            saltos,
        }
    }

    /// Posición inicial en la cuadrícula (columnas de ~70 nodos por fila).
    pub fn posicion(&self, n: usize) -> (f64, f64) {
        if let Some(p) = self.fijas.get(n) {
            return *p;
        }
        let por_fila = 70usize;
        let x = (n % por_fila) as f64 * PASO;
        let y = (n / por_fila) as f64 * PASO * 0.8;
        (x, y)
    }

    pub fn extension_mundo(&self) -> (f64, f64) {
        self.posicion(self.rutas.len().saturating_sub(1))
    }

    /// Centro geométrico inicial para la gravedad suave del grafo global.
    fn centro_gravedad(&self) -> (f64, f64) {
        if self.rutas.is_empty() {
            return (0.0, 0.0);
        }
        let mut sx = 0.0;
        let mut sy = 0.0;
        for i in 0..self.rutas.len() {
            let (x, y) = self.posicion(i);
            sx += x;
            sy += y;
        }
        let n = self.rutas.len() as f64;
        (sx / n, sy / n)
    }
}

/// Celda del cuadárbol Barnes-Hut almacenada en un `Vec` plano sin alojamientos por cuadro.
#[derive(Clone, Copy)]
struct CeldaQuad {
    cx: f64,
    cy: f64,
    mitad: f64,
    mx: f64,
    my: f64,
    masa: f64,
    hijos: [u32; 4],
    nodo: u32,
}

impl CeldaQuad {
    fn nueva(cx: f64, cy: f64, mitad: f64) -> Self {
        Self {
            cx,
            cy,
            mitad,
            mx: 0.0,
            my: 0.0,
            masa: 0.0,
            hijos: [u32::MAX; 4],
            nodo: u32::MAX,
        }
    }

    #[inline]
    fn es_hoja(&self) -> bool {
        self.hijos == [u32::MAX; 4]
    }

    #[inline]
    fn cuadrante(&self, x: f64, y: f64) -> usize {
        let der = usize::from(x >= self.cx);
        let aba = usize::from(y >= self.cy);
        (aba << 1) | der
    }

    #[inline]
    fn centro_hijo(&self, q: usize) -> (f64, f64, f64) {
        let m = self.mitad * 0.5;
        let dx = if (q & 1) != 0 { m } else { -m };
        let dy = if (q & 2) != 0 { m } else { -m };
        (self.cx + dx, self.cy + dy, m)
    }
}

/// Arena reutilizable para el cuadárbol Barnes-Hut y el acumulador de fuerzas.
#[derive(Default)]
pub(crate) struct ArenaQuad {
    celdas: Vec<CeldaQuad>,
    fuerzas: Vec<(f64, f64)>,
}

impl ArenaQuad {
    fn insertar(&mut self, idx_nodo: u32, x: f64, y: f64, pos: &[(f64, f64)]) {
        let mut actual = 0usize;
        let mut prof = 0u8;
        loop {
            // Acumular centro de masa en la celda actual.
            let c = &mut self.celdas[actual];
            let nueva_masa = c.masa + 1.0;
            c.mx = (c.mx * c.masa + x) / nueva_masa;
            c.my = (c.my * c.masa + y) / nueva_masa;
            c.masa = nueva_masa;

            if c.es_hoja() {
                if c.nodo == u32::MAX && c.masa <= 1.0 {
                    c.nodo = idx_nodo;
                    return;
                }
                if prof >= MAX_PROF_QUAD {
                    c.nodo = u32::MAX;
                    return;
                }
                // Subdividir y reubicar el nodo existente si lo había.
                let viejo = c.nodo;
                c.nodo = u32::MAX;
                if viejo != u32::MAX {
                    let (vx, vy) = pos[viejo as usize];
                    let qv = self.celdas[actual].cuadrante(vx, vy);
                    let (hcx, hcy, hm) = self.celdas[actual].centro_hijo(qv);
                    let nuevo_hijo = self.celdas.len() as u32;
                    self.celdas.push(CeldaQuad::nueva(hcx, hcy, hm));
                    self.celdas[actual].hijos[qv] = nuevo_hijo;
                    let h = &mut self.celdas[nuevo_hijo as usize];
                    h.mx = vx;
                    h.my = vy;
                    h.masa = 1.0;
                    h.nodo = viejo;
                }
            }

            let q = self.celdas[actual].cuadrante(x, y);
            let mut hijo = self.celdas[actual].hijos[q];
            if hijo == u32::MAX {
                let (hcx, hcy, hm) = self.celdas[actual].centro_hijo(q);
                hijo = self.celdas.len() as u32;
                self.celdas.push(CeldaQuad::nueva(hcx, hcy, hm));
                self.celdas[actual].hijos[q] = hijo;
            }
            actual = hijo as usize;
            prof += 1;
        }
    }

    fn fuerza_repulsion(&self, idx_nodo: u32, px: f64, py: f64) -> (f64, f64) {
        if self.celdas.is_empty() {
            return (0.0, 0.0);
        }
        let mut fx = 0.0;
        let mut fy = 0.0;
        let mut pila = [0u32; 96];
        let mut tope = 1usize;

        while tope > 0 {
            tope -= 1;
            let c = &self.celdas[pila[tope] as usize];
            if c.masa == 0.0 || c.nodo == idx_nodo {
                continue;
            }
            let dx = px - c.mx;
            let dy = py - c.my;
            let d2 = dx * dx + dy * dy + 25.0;
            let ancho = c.mitad * 2.0;
            if c.es_hoja() || (ancho * ancho) < THETA_CUAD * d2 {
                let inv_d = d2.sqrt().recip();
                let mag = (K_REPULSION * c.masa) / d2;
                fx += dx * inv_d * mag;
                fy += dy * inv_d * mag;
            } else {
                for &h in &c.hijos {
                    if h != u32::MAX && tope < pila.len() {
                        pila[tope] = h;
                        tope += 1;
                    }
                }
            }
        }
        (fx, fy)
    }
}

/// Ejecuta un paso de simulación física Barnes-Hut sobre `pos` y `vel`, respetando `fijos`.
pub(crate) fn paso_barnes_hut(
    arena: &mut ArenaQuad,
    pos: &mut [(f64, f64)],
    vel: &mut [(f64, f64)],
    fijos: &[bool],
    aristas: &[(usize, usize)],
    centro: (f64, f64),
    temp: f64,
) {
    let n = pos.len();
    if n == 0 {
        return;
    }
    let (mut min_x, mut max_x) = (pos[0].0, pos[0].0);
    let (mut min_y, mut max_y) = (pos[0].1, pos[0].1);
    for &(x, y) in &pos[1..] {
        min_x = min_x.min(x);
        max_x = max_x.max(x);
        min_y = min_y.min(y);
        max_y = max_y.max(y);
    }
    let cx = (min_x + max_x) * 0.5;
    let cy = (min_y + max_y) * 0.5;
    let mitad = ((max_x - min_x).max(max_y - min_y) * 0.5 + 10.0).max(50.0);

    arena.celdas.clear();
    if arena.celdas.capacity() < n * 2 {
        arena.celdas.reserve(n * 2);
    }
    arena.celdas.push(CeldaQuad::nueva(cx, cy, mitad));
    for (i, &(x, y)) in pos.iter().enumerate() {
        arena.insertar(i as u32, x, y, pos);
    }

    arena.fuerzas.clear();
    arena.fuerzas.resize(n, (0.0, 0.0));

    // 1. Repulsión Barnes-Hut + gravedad centrípeta hacia el centro.
    for i in 0..n {
        if fijos.get(i).copied().unwrap_or(false) {
            continue;
        }
        let (px, py) = pos[i];
        let (mut fx, mut fy) = arena.fuerzas_repulsion_con_gravedad(i as u32, px, py, centro);
        if !fx.is_finite() || !fy.is_finite() {
            fx = 0.0;
            fy = 0.0;
        }
        arena.fuerzas[i] = (fx, fy);
    }

    // 2. Atracción elástica sobre las aristas resueltas.
    for &(a, b) in aristas {
        if a >= n || b >= n {
            continue;
        }
        let dx = pos[b].0 - pos[a].0;
        let dy = pos[b].1 - pos[a].1;
        let dist = (dx * dx + dy * dy).sqrt().max(1.0);
        let mag = K_RESORTE * (dist - L_IDEAL);
        let ux = (dx / dist) * mag;
        let uy = (dy / dist) * mag;
        if !fijos.get(a).copied().unwrap_or(false) {
            arena.fuerzas[a].0 += ux;
            arena.fuerzas[a].1 += uy;
        }
        if !fijos.get(b).copied().unwrap_or(false) {
            arena.fuerzas[b].0 -= ux;
            arena.fuerzas[b].1 -= uy;
        }
    }

    // 3. Integración amortiguada limitada por temperatura.
    let paso_max = (PASO_MAX * temp).max(0.05);
    for i in 0..n {
        if fijos.get(i).copied().unwrap_or(false) {
            vel[i] = (0.0, 0.0);
            continue;
        }
        let (fx, fy) = arena.fuerzas[i];
        let mut vx = (vel[i].0 + fx) * AMORTIGUAMIENTO;
        let mut vy = (vel[i].1 + fy) * AMORTIGUAMIENTO;
        let mag2 = vx * vx + vy * vy;
        if mag2 > paso_max * paso_max {
            let esc = paso_max / mag2.sqrt();
            vx *= esc;
            vy *= esc;
        }
        pos[i].0 += vx;
        pos[i].1 += vy;
        vel[i] = (vx, vy);
    }
}

impl ArenaQuad {
    #[inline]
    fn fuerzas_repulsion_con_gravedad(
        &self,
        idx_nodo: u32,
        px: f64,
        py: f64,
        centro: (f64, f64),
    ) -> (f64, f64) {
        let (mut fx, mut fy) = self.fuerza_repulsion(idx_nodo, px, py);
        fx += (centro.0 - px) * K_GRAVEDAD;
        fy += (centro.1 - py) * K_GRAVEDAD;
        (fx, fy)
    }
}

/// Estado sincronizado entre el hilo secundario de Barnes-Hut y `VistaGrafo`.
struct EstadoFisica {
    posiciones: Vec<(f64, f64)>,
    velocidades: Vec<(f64, f64)>,
    fijos: Vec<bool>,
    aristas: Vec<(usize, usize)>,
    centro: (f64, f64),
    temp: f64,
    pausado: bool,
    visible: bool,
    corriendo: bool,
    generacion: u64,
    cerrar: bool,
}

struct SincroFisica {
    estado: Mutex<EstadoFisica>,
    cond: Condvar,
}

impl SincroFisica {
    fn nuevo(datos: &GrafoDatos) -> Arc<Self> {
        let n = datos.rutas.len();
        let posiciones: Vec<(f64, f64)> = (0..n).map(|i| datos.posicion(i)).collect();
        let centro = datos.centro_gravedad();
        let sincro = Arc::new(Self {
            estado: Mutex::new(EstadoFisica {
                posiciones,
                velocidades: vec![(0.0, 0.0); n],
                fijos: vec![false; n],
                aristas: datos.aristas.clone(),
                centro,
                temp: if n > 1 { 1.0 } else { 0.0 },
                pausado: false,
                visible: true,
                corriendo: n > 1,
                generacion: 0,
                cerrar: false,
            }),
            cond: Condvar::new(),
        });
        let hilo_sincro = Arc::clone(&sincro);
        std::thread::Builder::new()
            .name("orbital-grafo-bh".into())
            .spawn(move || bucle_hilo_fisica(hilo_sincro))
            .expect("hilo de simulación del grafo");
        sincro
    }
}

fn bucle_hilo_fisica(sincro: Arc<SincroFisica>) {
    let mut arena = ArenaQuad::default();
    let mut pos_local: Vec<(f64, f64)> = Vec::new();
    let mut vel_local: Vec<(f64, f64)> = Vec::new();
    let mut fijos_local: Vec<bool> = Vec::new();
    let mut aristas_local: Vec<(usize, usize)> = Vec::new();

    loop {
        let (centro, temp) = {
            let mut g = match sincro.estado.lock() {
                Ok(g) => g,
                Err(_) => return,
            };
            while !g.cerrar
                && (g.pausado || !g.visible || g.temp < TEMP_MIN || g.posiciones.len() <= 1)
            {
                g.corriendo = false;
                g = match sincro.cond.wait(g) {
                    Ok(g) => g,
                    Err(_) => return,
                };
            }
            if g.cerrar {
                return;
            }
            g.corriendo = true;
            pos_local.clone_from(&g.posiciones);
            vel_local.clone_from(&g.velocidades);
            fijos_local.clone_from(&g.fijos);
            if aristas_local.len() != g.aristas.len() {
                aristas_local.clone_from(&g.aristas);
            }
            (g.centro, g.temp)
        };

        let t0 = std::time::Instant::now();
        paso_barnes_hut(
            &mut arena,
            &mut pos_local,
            &mut vel_local,
            &fijos_local,
            &aristas_local,
            centro,
            temp,
        );

        {
            let Ok(mut g) = sincro.estado.lock() else {
                return;
            };
            if g.cerrar {
                return;
            }
            // Preservar cualquier nodo que la usuaria haya fijado o movido durante el paso.
            for i in 0..pos_local.len().min(g.posiciones.len()) {
                if !g.fijos[i] {
                    g.posiciones[i] = pos_local[i];
                    g.velocidades[i] = vel_local[i];
                } else {
                    g.velocidades[i] = (0.0, 0.0);
                }
            }
            g.temp *= ENFRIAMIENTO;
            if g.temp < TEMP_MIN {
                g.temp = 0.0;
                g.corriendo = false;
            }
            g.generacion = g.generacion.wrapping_add(1);
        }

        let transcurrido = t0.elapsed();
        let objetivo = std::time::Duration::from_millis(16);
        if transcurrido < objetivo {
            std::thread::sleep(objetivo - transcurrido);
        }
    }
}

/// Rectángulo en coords del widget (para detectar el nodo bajo el mouse).
#[derive(Clone, Copy, Default)]
pub(crate) struct Vista {
    /// Esquina superior izquierda del mundo visible (unidades del mundo).
    pan_x: f64,
    pan_y: f64,
    /// Escala: 1.0 = 1 unidad del mundo = 1 px.
    zoom: f64,
}

/// Estado del arrastre en curso: fondo (paneo) o nodo (reposicionar).
#[derive(Default)]
pub(crate) enum Arrastre {
    #[default]
    Ninguno,
    Fondo(f64, f64, f64, f64),
    Nodo(usize, f64, f64),
}

/// Callback al hacer clic en un nodo del grafo (recibe la ruta de la nota).
type CallbackAbrir = Box<dyn Fn(&str)>;
type CallbackEstadoFisica = Box<dyn Fn(bool)>;

mod imp {
    use super::*;

    #[derive(Default)]
    pub struct VistaGrafo {
        pub datos: RefCell<Option<Rc<GrafoDatos>>>,
        /// Desplazamientos `(dx, dy)` respecto de `datos.posicion(n)`.
        pub posiciones: RefCell<Vec<(f64, f64)>>,
        pub(crate) vista: Cell<Vista>,
        pub hover: Cell<Option<usize>>,
        pub seleccion: Cell<Option<usize>>,
        /// Callback al hacer clic en un nodo (abre la nota).
        pub on_abrir: RefCell<Option<CallbackAbrir>>,
        pub on_cerrar: RefCell<Option<Box<dyn Fn()>>>,
        pub on_estado_fisica: RefCell<Option<CallbackEstadoFisica>>,
        pub(crate) arrastre: RefCell<Arrastre>,
        /// Grafo local: el origen del mundo va al centro del lienzo en el próximo cuadro.
        pub centrar: Cell<bool>,
        /// Motor físico en segundo plano (solo para el grafo global).
        pub(super) fisica: RefCell<Option<Arc<SincroFisica>>>,
        /// Indica si hay un tick de redibujo de GTK activo.
        pub tick_activo: Cell<bool>,
        /// Última generación copiada del hilo secundario.
        pub ultima_gen: Cell<u64>,
    }

    impl Drop for VistaGrafo {
        fn drop(&mut self) {
            if let Some(sincro) = self.fisica.borrow_mut().take()
                && let Ok(mut g) = sincro.estado.lock()
            {
                g.cerrar = true;
                sincro.cond.notify_all();
            }
        }
    }

    #[glib::object_subclass]
    impl ObjectSubclass for VistaGrafo {
        const NAME: &'static str = "OrbitalVistaGrafo";
        type Type = super::VistaGrafo;
        type ParentType = gtk::Widget;
    }

    impl ObjectImpl for VistaGrafo {}

    impl WidgetImpl for VistaGrafo {
        fn snapshot(&self, snapshot: &gtk::Snapshot) {
            let Some(datos) = self.datos.borrow().clone() else {
                return;
            };
            let width = self.obj().width() as f64;
            let height = self.obj().height() as f64;
            if self.centrar.get() && width > 0.0 {
                self.centrar.set(false);
                let mut v = self.vista.get();
                v.pan_x = -width / 2.0;
                v.pan_y = -height / 2.0;
                self.vista.set(v);
            }
            let v = self.vista.get();
            let rosa = gtk::gdk::RGBA::parse("#ff8cc6").expect("rosa de paleta");
            let linea = gtk::gdk::RGBA::parse("#685061").expect("línea de la maqueta");
            let relleno = gtk::gdk::RGBA::parse("#392532").expect("relleno de la maqueta");
            let texto = gtk::gdk::RGBA::parse("#eee9ed").expect("texto de la maqueta");
            let hovered = self.hover.get();
            let vecinos: Vec<usize> = hovered
                .map(|n| {
                    let mut out: Vec<usize> = datos
                        .aristas
                        .iter()
                        .filter(|(a, b)| *a == n || *b == n)
                        .flat_map(|(a, b)| [*a, *b])
                        .filter(|m| *m != n)
                        .collect();
                    out.sort();
                    out.dedup();
                    out
                })
                .unwrap_or_default();
            let pos = |n: usize| {
                let (base_x, base_y) = datos.posicion(n);
                let mut posiciones = self.posiciones.borrow_mut();
                if posiciones.len() <= n {
                    posiciones.resize(datos.rutas.len(), (0.0, 0.0));
                }
                let (dx, dy) = posiciones[n];
                (base_x + dx, base_y + dy)
            };
            let a_pantalla = |wx: f64, wy: f64| (wx * v.zoom - v.pan_x, wy * v.zoom - v.pan_y);
            let trazo = gsk::Stroke::new(1.5);
            let es_local_con_saltos = !datos.saltos.is_empty();

            if es_local_con_saltos && hovered.is_none() {
                // Grafo local por saltos: opacidad de arista según el salto más lejano que une.
                let alphas = [1.0f32, 0.70, 0.45];
                let builders = [
                    gsk::PathBuilder::new(),
                    gsk::PathBuilder::new(),
                    gsk::PathBuilder::new(),
                ];
                let mut hay = [false; 3];
                for &(a, b) in &datos.aristas {
                    let (x0, y0) = {
                        let (x, y) = pos(a);
                        a_pantalla(x, y)
                    };
                    let (x1, y1) = {
                        let (x, y) = pos(b);
                        a_pantalla(x, y)
                    };
                    if (x0 < 0.0 && x1 < 0.0)
                        || (y0 < 0.0 && y1 < 0.0)
                        || (x0 > width && x1 > width)
                        || (y0 > height && y1 > height)
                    {
                        continue;
                    }
                    let salto_max = datos
                        .saltos
                        .get(a)
                        .copied()
                        .unwrap_or(1)
                        .max(datos.saltos.get(b).copied().unwrap_or(1))
                        .clamp(1, 3) as usize
                        - 1;
                    builders[salto_max].move_to(x0 as f32, y0 as f32);
                    builders[salto_max].line_to(x1 as f32, y1 as f32);
                    hay[salto_max] = true;
                }
                for i in 0..3 {
                    if hay[i] {
                        let col = gtk::gdk::RGBA::new(
                            linea.red(),
                            linea.green(),
                            linea.blue(),
                            alphas[i],
                        );
                        snapshot.append_stroke(&builders[i].clone().to_path(), &trazo, &col);
                    }
                }
            } else {
                // Aristas: un solo trazo por color (barato con miles de aristas).
                let aristas = gsk::PathBuilder::new();
                let aristas_rosa = gsk::PathBuilder::new();
                let (mut hay, mut hay_rosa) = (false, false);
                for (a, b) in &datos.aristas {
                    let resaltar = hovered.is_some_and(|h| *a == h || *b == h);
                    if hovered.is_some() && !resaltar {
                        continue; // con hover, el resto se atenúa (no se dibuja)
                    }
                    let (x0, y0) = {
                        let (x, y) = pos(*a);
                        a_pantalla(x, y)
                    };
                    let (x1, y1) = {
                        let (x, y) = pos(*b);
                        a_pantalla(x, y)
                    };
                    // Fuera del lienzo por completo: se omite.
                    if (x0 < 0.0 && x1 < 0.0)
                        || (y0 < 0.0 && y1 < 0.0)
                        || (x0 > width && x1 > width)
                        || (y0 > height && y1 > height)
                    {
                        continue;
                    }
                    let destino = if resaltar { &aristas_rosa } else { &aristas };
                    destino.move_to(x0 as f32, y0 as f32);
                    destino.line_to(x1 as f32, y1 as f32);
                    if resaltar {
                        hay_rosa = true
                    } else {
                        hay = true
                    }
                }
                if hay {
                    snapshot.append_stroke(&aristas.to_path(), &trazo, &linea);
                }
                if hay_rosa {
                    snapshot.append_stroke(&aristas_rosa.to_path(), &trazo, &rosa);
                }
            }

            let r_base = (RADIO * v.zoom).max(2.0);
            let mut etiquetas: Vec<(usize, f64, f64, f64, f32)> = Vec::new();

            if es_local_con_saltos && hovered.is_none() {
                // Nodos por nivel de salto (0 = nota central rosa; 1, 2, 3 = radio y opacidad decrecientes).
                let radios_salto = [1.18, 1.0, 0.84, 0.70];
                let opacidades_salto = [1.0f32, 1.0, 0.78, 0.56];
                let builders = [
                    gsk::PathBuilder::new(),
                    gsk::PathBuilder::new(),
                    gsk::PathBuilder::new(),
                    gsk::PathBuilder::new(),
                ];
                let mut hay = [false; 4];
                for n in 0..datos.rutas.len() {
                    let (wx, wy) = pos(n);
                    let (x, y) = a_pantalla(wx, wy);
                    let nivel = datos.saltos.get(n).copied().unwrap_or(1).min(3) as usize;
                    let r = (r_base * radios_salto[nivel]).max(2.0);
                    if x < -r || y < -r || x > width + r || y > height + r {
                        continue;
                    }
                    let centro = gtk::graphene::Point::new(x as f32, y as f32);
                    builders[nivel].add_circle(&centro, r as f32);
                    hay[nivel] = true;
                    if v.zoom > 0.5 {
                        etiquetas.push((n, x, y, r, opacidades_salto[nivel]));
                    }
                }
                if hay[0] {
                    snapshot.append_fill(
                        &builders[0].clone().to_path(),
                        gsk::FillRule::Winding,
                        &rosa,
                    );
                }
                for nivel in 1..=3 {
                    if hay[nivel] {
                        let path = builders[nivel].clone().to_path();
                        let alpha = opacidades_salto[nivel];
                        let col_relleno = gtk::gdk::RGBA::new(
                            relleno.red(),
                            relleno.green(),
                            relleno.blue(),
                            alpha,
                        );
                        let col_borde =
                            gtk::gdk::RGBA::new(rosa.red(), rosa.green(), rosa.blue(), alpha);
                        snapshot.append_fill(&path, gsk::FillRule::Winding, &col_relleno);
                        snapshot.append_stroke(&path, &trazo, &col_borde);
                    }
                }
            } else {
                // Nodos: círculos con relleno oscuro y borde rosa; hover y vecinos rellenos de rosa.
                let nodos = gsk::PathBuilder::new();
                let nodos_rosa = gsk::PathBuilder::new();
                for n in 0..datos.rutas.len() {
                    let (wx, wy) = pos(n);
                    let (x, y) = a_pantalla(wx, wy);
                    if x < -r_base || y < -r_base || x > width + r_base || y > height + r_base {
                        continue;
                    }
                    let resaltar = hovered.is_some_and(|h| h == n || vecinos.contains(&n));
                    if hovered.is_some() && !resaltar {
                        continue; // atenuar el resto
                    }
                    let centro = gtk::graphene::Point::new(x as f32, y as f32);
                    if resaltar { &nodos_rosa } else { &nodos }.add_circle(&centro, r_base as f32);
                    if v.zoom > 0.5 || hovered == Some(n) {
                        etiquetas.push((n, x, y, r_base, 1.0));
                    }
                }
                let nodos = nodos.to_path();
                snapshot.append_fill(&nodos, gsk::FillRule::Winding, &relleno);
                snapshot.append_stroke(&nodos, &trazo, &rosa);
                snapshot.append_fill(&nodos_rosa.to_path(), gsk::FillRule::Winding, &rosa);
            }

            // Etiquetas centradas debajo de cada nodo.
            let widget = self.obj();
            for (n, x, y, r, alpha) in etiquetas {
                let Some(nombre) = datos.rutas.get(n) else {
                    continue;
                };
                let archivo = nombre.rsplit_once('/').map(|(_, f)| f).unwrap_or(nombre);
                let etiqueta = archivo.rsplit_once('.').map_or(archivo, |(s, _)| s);
                let (w, _, layout) = pango_label(widget.upcast_ref::<gtk::Widget>(), etiqueta);
                let col_texto = if (alpha - 1.0).abs() < f32::EPSILON {
                    texto
                } else {
                    gtk::gdk::RGBA::new(texto.red(), texto.green(), texto.blue(), alpha)
                };
                snapshot.save();
                snapshot.translate(&gtk::graphene::Point::new(
                    (x - w / 2.0) as f32,
                    (y + r + 4.0) as f32,
                ));
                snapshot.append_layout(&layout, &col_texto);
                snapshot.restore();
            }
        }
    }
}

glib::wrapper! {
    pub struct VistaGrafo(ObjectSubclass<imp::VistaGrafo>)
        @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

/// Un layout de Pango y su tamaño, para las etiquetas.
fn pango_label(w: &impl IsA<gtk::Widget>, texto: &str) -> (f64, f64, gtk::pango::Layout) {
    let layout = w.create_pango_layout(Some(texto));
    let mut fuente = gtk::pango::FontDescription::new();
    fuente.set_size(10 * gtk::pango::SCALE);
    layout.set_font_description(Some(&fuente));
    let (w, h) = layout.pixel_size();
    (w as f64, h as f64, layout)
}

impl VistaGrafo {
    pub fn new() -> Self {
        let this: Self = glib::Object::builder().build();
        this.set_can_focus(true);
        this.set_focusable(true);
        // FR-6: el grafo es un complemento visual; etiqueta accesible del lienzo.
        this.update_property(&[gtk::accessible::Property::Label(
            "Grafo de enlaces de la bóveda: nodos por nota, aristas por enlace",
        )]);
        this.imp().vista.set(Vista {
            pan_x: 0.0,
            pan_y: 0.0,
            zoom: 1.0,
        });
        this.connect_map(|v| v.set_fisica_visible(true));
        this.connect_unmap(|v| v.set_fisica_visible(false));
        this.setup_gestos();
        this
    }

    pub fn set_datos(&self, datos: Rc<GrafoDatos>) {
        self.cerrar_hilo_fisica();
        let es_global = datos.fijas.is_empty();
        self.imp().centrar.set(!es_global);
        self.imp().posiciones.borrow_mut().clear();
        self.imp().ultima_gen.set(0);
        *self.imp().datos.borrow_mut() = Some(datos.clone());
        if es_global && datos.rutas.len() > 1 {
            let sincro = SincroFisica::nuevo(&datos);
            *self.imp().fisica.borrow_mut() = Some(sincro);
            self.notificar_estado_fisica(false);
            self.asegurar_tick_fisica();
        }
        self.queue_draw();
    }

    pub fn limpiar(&self) {
        self.cerrar_hilo_fisica();
        *self.imp().datos.borrow_mut() = None;
        self.imp().posiciones.borrow_mut().clear();
        self.imp().hover.set(None);
        self.queue_draw();
    }

    pub fn set_on_abrir(&self, f: impl Fn(&str) + 'static) {
        *self.imp().on_abrir.borrow_mut() = Some(Box::new(f));
    }

    pub fn set_on_cerrar(&self, f: impl Fn() + 'static) {
        *self.imp().on_cerrar.borrow_mut() = Some(Box::new(f));
    }

    /// Callback cuando cambia el estado de pausa manual de la simulación (`true` = pausada).
    pub fn set_on_estado_fisica(&self, f: impl Fn(bool) + 'static) {
        *self.imp().on_estado_fisica.borrow_mut() = Some(Box::new(f));
    }

    /// Alterna entre pausar y reanudar la simulación física del grafo global.
    /// Devuelve `true` si quedó pausada.
    pub fn pausar_reanudar(&self) -> bool {
        let Some(sincro) = self.imp().fisica.borrow().clone() else {
            return false;
        };
        let pausado = {
            let Ok(mut g) = sincro.estado.lock() else {
                return false;
            };
            g.pausado = !g.pausado;
            if !g.pausado && g.temp < TEMP_MIN {
                g.temp = 0.5;
            }
            if !g.pausado {
                g.corriendo = true;
            }
            let p = g.pausado;
            sincro.cond.notify_all();
            p
        };
        self.notificar_estado_fisica(pausado);
        if !pausado {
            self.asegurar_tick_fisica();
        }
        pausado
    }

    /// Indica si la simulación está pausada manualmente.
    #[cfg(test)]
    pub fn esta_pausado(&self) -> bool {
        self.imp()
            .fisica
            .borrow()
            .as_ref()
            .and_then(|s| s.estado.lock().ok().map(|g| g.pausado))
            .unwrap_or(false)
    }

    /// Indica si el hilo secundario está ejecutando pasos activamente.
    #[cfg(test)]
    pub fn fisica_corriendo(&self) -> bool {
        self.imp()
            .fisica
            .borrow()
            .as_ref()
            .and_then(|s| s.estado.lock().ok().map(|g| g.corriendo))
            .unwrap_or(false)
    }

    /// Reinicia posiciones, libera los nodos fijados manualmente y vuelve a correr la simulación.
    pub fn recalcular(&self) {
        let Some(datos) = self.imp().datos.borrow().clone() else {
            return;
        };
        self.imp().posiciones.borrow_mut().clear();
        if let Some(sincro) = self.imp().fisica.borrow().clone() {
            if let Ok(mut g) = sincro.estado.lock() {
                let n = datos.rutas.len();
                g.posiciones = (0..n).map(|i| datos.posicion(i)).collect();
                g.velocidades.fill((0.0, 0.0));
                g.fijos.fill(false);
                g.temp = 1.0;
                g.pausado = false;
                g.visible = true;
                g.corriendo = n > 1;
                g.generacion = g.generacion.wrapping_add(1);
                sincro.cond.notify_all();
            }
            self.notificar_estado_fisica(false);
            self.asegurar_tick_fisica();
        }
        self.queue_draw();
    }

    /// Detiene o reanuda la ejecución cuando la pestaña del grafo se oculta o se muestra (SC-002).
    pub fn set_fisica_visible(&self, visible: bool) {
        let Some(sincro) = self.imp().fisica.borrow().clone() else {
            return;
        };
        let despertar = {
            let Ok(mut g) = sincro.estado.lock() else {
                return;
            };
            g.visible = visible;
            let d = visible && !g.pausado && g.temp >= TEMP_MIN;
            if d {
                g.corriendo = true;
            }
            sincro.cond.notify_all();
            d
        };
        if despertar {
            self.asegurar_tick_fisica();
        }
    }

    fn cerrar_hilo_fisica(&self) {
        if let Some(sincro) = self.imp().fisica.borrow_mut().take()
            && let Ok(mut g) = sincro.estado.lock()
        {
            g.cerrar = true;
            sincro.cond.notify_all();
        }
    }

    fn notificar_estado_fisica(&self, pausado: bool) {
        if let Some(f) = self.imp().on_estado_fisica.borrow().as_ref() {
            f(pausado);
        }
    }

    fn asegurar_tick_fisica(&self) {
        if self.imp().tick_activo.get() {
            return;
        }
        self.imp().tick_activo.set(true);
        let weak = self.downgrade();
        glib::timeout_add_local(std::time::Duration::from_millis(16), move || {
            let Some(this) = weak.upgrade() else {
                return glib::ControlFlow::Break;
            };
            let imp = this.imp();
            let (Some(sincro), Some(datos)) =
                (imp.fisica.borrow().clone(), imp.datos.borrow().clone())
            else {
                imp.tick_activo.set(false);
                return glib::ControlFlow::Break;
            };
            let mut seguir = false;
            let mut cambio = false;
            if let Ok(g) = sincro.estado.lock() {
                seguir = g.corriendo && g.visible && !g.pausado && !g.cerrar;
                if g.generacion != imp.ultima_gen.get() {
                    imp.ultima_gen.set(g.generacion);
                    let n = datos.rutas.len().min(g.posiciones.len());
                    let mut offsets = imp.posiciones.borrow_mut();
                    offsets.resize(datos.rutas.len(), (0.0, 0.0));
                    for i in 0..n {
                        // Si la usuaria está arrastrando este nodo ahora mismo, respetar su puntero.
                        if matches!(*imp.arrastre.borrow(), Arrastre::Nodo(m, _, _) if m == i) {
                            continue;
                        }
                        let (bx, by) = datos.posicion(i);
                        offsets[i] = (g.posiciones[i].0 - bx, g.posiciones[i].1 - by);
                    }
                    cambio = true;
                }
            }
            if cambio {
                this.queue_draw();
            }
            if !seguir {
                imp.tick_activo.set(false);
                glib::ControlFlow::Break
            } else {
                glib::ControlFlow::Continue
            }
        });
    }

    /// Fija la posición del nodo `n` en la simulación física y la recalienta suavemente.
    fn fijar_nodo_en_fisica(&self, n: usize, wx: f64, wy: f64) {
        let Some(sincro) = self.imp().fisica.borrow().clone() else {
            return;
        };
        let despertar = {
            let Ok(mut g) = sincro.estado.lock() else {
                return;
            };
            if n < g.posiciones.len() {
                g.posiciones[n] = (wx, wy);
                g.velocidades[n] = (0.0, 0.0);
                g.fijos[n] = true;
            }
            if !g.pausado && g.visible {
                g.temp = g.temp.max(TEMP_ARRASTRE);
                g.corriendo = true;
                sincro.cond.notify_all();
                true
            } else {
                false
            }
        };
        if despertar {
            self.asegurar_tick_fisica();
        }
    }

    /// Índice del nodo bajo un punto del widget, si cae en el radio.
    fn nodo_en(&self, px: f64, py: f64) -> Option<usize> {
        let datos = self.imp().datos.borrow().clone()?;
        let v = self.imp().vista.get();
        let mut mejor: Option<(usize, f64)> = None;
        for n in 0..datos.rutas.len() {
            let (wx, wy) = self.pos_de(n);
            let (x, y) = (wx * v.zoom - v.pan_x, wy * v.zoom - v.pan_y);
            let dist = ((px - x).powi(2) + (py - y).powi(2)).sqrt();
            let radio = (RADIO * v.zoom).max(2.0) + 4.0;
            if dist <= radio && mejor.is_none_or(|(_, m)| dist < m) {
                mejor = Some((n, dist));
            }
        }
        mejor.map(|(n, _)| n)
    }

    fn pos_de(&self, n: usize) -> (f64, f64) {
        let Some(datos) = self.imp().datos.borrow().clone() else {
            return (0.0, 0.0);
        };
        let mut posiciones = self.imp().posiciones.borrow_mut();
        if posiciones.len() <= n {
            posiciones.resize(datos.rutas.len(), (0.0, 0.0));
        }
        let (dx, dy) = posiciones[n];
        let (bx, by) = datos.posicion(n);
        (bx + dx, by + dy)
    }

    fn setup_gestos(&self) {
        // Rueda: zoom centrado en el mouse.
        let scroll = gtk::EventControllerScroll::new(gtk::EventControllerScrollFlags::VERTICAL);
        let this = self.clone();
        let ctl_scroll = scroll.clone();
        scroll.connect_scroll(move |_, _dx, dy| {
            let g = &ctl_scroll;
            let imp = this.imp();
            let mut v = imp.vista.get();
            let factor = if dy > 0.0 { 0.9 } else { 1.1 };
            let nuevo = (v.zoom * factor).clamp(0.05, 8.0);
            // Centrar en el mouse: el punto del mundo bajo el mouse queda bajo el mouse.
            if let Some(e) = g.current_event()
                && let Some(pos) = e.position()
            {
                let (mx, my) = pos;
                let wx = (mx + v.pan_x) / v.zoom;
                let wy = (my + v.pan_y) / v.zoom;
                // pantalla = mundo × zoom − pan, así que pan = mundo × zoom − pantalla.
                v.pan_x = wx * nuevo - mx;
                v.pan_y = wy * nuevo - my;
            }
            v.zoom = nuevo;
            imp.vista.set(v);
            this.queue_draw();
            gtk::glib::Propagation::Stop
        });
        self.add_controller(scroll);

        // Arrastre: fondo = paneo; nodo = reposicionar y fijar en la simulación.
        let drag = gtk::GestureDrag::new();
        drag.set_button(1);
        let this = self.clone();
        drag.connect_drag_begin(move |g, x, y| {
            let imp = this.imp();
            if let Some(n) = this.nodo_en(x, y) {
                let (bx, by) = imp
                    .datos
                    .borrow()
                    .as_ref()
                    .map(|d| d.posicion(n))
                    .unwrap_or((0.0, 0.0));
                imp.arrastre.replace(Arrastre::Nodo(n, bx, by));
            } else {
                let v = imp.vista.get();
                imp.arrastre
                    .replace(Arrastre::Fondo(x, y, v.pan_x, v.pan_y));
            }
            let _ = g;
        });
        let this = self.clone();
        drag.connect_drag_update(move |g, dx, dy| {
            let imp = this.imp();
            let (x0, y0) = g.start_point().unwrap_or((0.0, 0.0));
            match *imp.arrastre.borrow() {
                Arrastre::Fondo(sx, sy, px, py) => {
                    let mut v = imp.vista.get();
                    v.pan_x = px - ((x0 - sx) + dx);
                    v.pan_y = py - ((y0 - sy) + dy);
                    imp.vista.set(v);
                    this.queue_draw();
                }
                Arrastre::Nodo(n, bx, by) => {
                    let v = imp.vista.get();
                    let (px, py) = ((x0 + dx), (y0 + dy));
                    let nuevo_x = (px + v.pan_x) / v.zoom;
                    let nuevo_y = (py + v.pan_y) / v.zoom;
                    let mut posiciones = imp.posiciones.borrow_mut();
                    if posiciones.len() <= n {
                        posiciones.resize(
                            imp.datos
                                .borrow()
                                .as_ref()
                                .map(|d| d.rutas.len())
                                .unwrap_or(0),
                            (0.0, 0.0),
                        );
                    }
                    posiciones[n] = (nuevo_x - bx, nuevo_y - by);
                    drop(posiciones);
                    this.fijar_nodo_en_fisica(n, nuevo_x, nuevo_y);
                    this.queue_draw();
                }
                Arrastre::Ninguno => {}
            }
        });
        let this = self.clone();
        drag.connect_drag_end(move |g, dx, dy| {
            let (sx, sy) = g.start_point().unwrap_or((0.0, 0.0));
            let quieto = (dx.abs() + dy.abs()) < 5.0;
            if quieto
                && let Some(n) = this.nodo_en(sx, sy)
                && let Some(ruta) = this
                    .imp()
                    .datos
                    .borrow()
                    .as_ref()
                    .and_then(|d| d.rutas.get(n).cloned())
                && let Some(f) = this.imp().on_abrir.borrow().as_ref()
            {
                f(&ruta);
            }
            this.imp().arrastre.replace(Arrastre::Ninguno);
        });
        self.add_controller(drag);

        // Movimiento: hover de vecinos.
        let motion = gtk::EventControllerMotion::new();
        let this = self.clone();
        motion.connect_motion(move |_m, x, y| {
            let n = this.nodo_en(x, y);
            if n != this.imp().hover.get() {
                this.imp().hover.set(n);
                this.queue_draw();
            }
        });
        let this = self.clone();
        motion.connect_leave(move |_| {
            this.imp().hover.set(None);
            this.queue_draw();
        });
        self.add_controller(motion);

        // Esc: cerrar el grafo.
        let teclas = gtk::EventControllerKey::new();
        teclas.set_propagation_phase(gtk::PropagationPhase::Capture);
        let this = self.clone();
        teclas.connect_key_pressed(move |_, keyval, _, _| {
            if keyval == gtk::gdk::Key::Escape {
                if let Some(f) = this.imp().on_cerrar.borrow().as_ref() {
                    f();
                }
                return glib::Propagation::Stop;
            }
            glib::Propagation::Proceed
        });
        self.add_controller(teclas);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// T002: orden, deduplicación de aristas y recorte en 5000.
    #[test]
    fn us8_datos_orden_dedup_y_corte() {
        let mut notas: Vec<String> = (0..(MAX_NODOS + 10))
            .map(|n| format!("wiki/n{n}.md"))
            .collect();
        notas.push("wiki/a.md".into());
        notas.push("wiki/b.md".into());
        let enlaces = vec![
            ("wiki/a.md".to_owned(), "wiki/b.md".to_owned()),
            ("wiki/b.md".to_owned(), "wiki/a.md".to_owned()), // duplicada al dedup
            ("wiki/a.md".to_owned(), "wiki/ausente.md".to_owned()), // destino fuera del corte
        ];
        let datos = GrafoDatos::desde(notas, enlaces);
        assert!(datos.cortado);
        assert_eq!(datos.rutas.len(), MAX_NODOS);
        assert!(
            datos.rutas[0] == "wiki/a.md" && datos.rutas[1] == "wiki/b.md",
            "{:?}",
            &datos.rutas[..2]
        );
        assert_eq!(datos.aristas, [(0, 1)]);
        // Cuadrícula determinista.
        assert_eq!(datos.posicion(0), (0.0, 0.0));
        assert_eq!(datos.posicion(70), (0.0, PASO * 0.8));
    }

    /// T002: sin corte cuando la bóveda es chica.
    #[test]
    fn us8_datos_sin_corte() {
        let datos = GrafoDatos::desde(vec!["wiki/a.md".into(), "wiki/b.md".into()], vec![]);
        assert!(!datos.cortado);
        assert_eq!(datos.rutas.len(), 2);
    }

    /// T005 (030): Barnes-Hut acerca nodos enlazados respecto de los no enlazados y respeta nodos fijos.
    #[test]
    fn us30_barnes_hut_converge_y_respeta_fijos() {
        let mut arena = ArenaQuad::default();
        // Nodos 0 y 1 enlazados (lejos al principio); nodo 2 desconectado; nodo 0 fijo.
        let mut pos: Vec<(f64, f64)> = vec![(0.0, 0.0), (500.0, 0.0), (0.0, 500.0)];
        let mut vel: Vec<(f64, f64)> = vec![(0.0, 0.0); 3];
        let fijos = vec![true, false, false];
        let aristas = vec![(0, 1)];
        let d_inicial = ((pos[1].0 - pos[0].0).powi(2) + (pos[1].1 - pos[0].1).powi(2)).sqrt();
        let mut temp = 1.0;
        for _ in 0..80 {
            paso_barnes_hut(
                &mut arena,
                &mut pos,
                &mut vel,
                &fijos,
                &aristas,
                (150.0, 150.0),
                temp,
            );
            temp *= ENFRIAMIENTO;
        }
        // El nodo 0 estaba fijo en (0, 0).
        assert_eq!(pos[0], (0.0, 0.0));
        let d_enlazados = ((pos[1].0 - pos[0].0).powi(2) + (pos[1].1 - pos[0].1).powi(2)).sqrt();
        let d_desconectados =
            ((pos[2].0 - pos[0].0).powi(2) + (pos[2].1 - pos[0].1).powi(2)).sqrt();
        assert!(
            d_enlazados < d_inicial * 0.6,
            "los nodos enlazados deben acercarse: antes {d_inicial:.1}, después {d_enlazados:.1}"
        );
        assert!(
            d_enlazados < d_desconectados,
            "los nodos enlazados ({d_enlazados:.1}) deben quedar más cerca que los desconectados ({d_desconectados:.1})"
        );
    }

    /// T005 y T008 (030): controles de simulación física en VistaGrafo y selector 1·2·3 en PanelDerecho.
    #[test]
    #[ignore = "requiere acceso GTK; tools/pruebas-gtk.sh"]
    fn us30_controles_fisica_y_saltos_gtk() {
        gtk::init().expect("se necesita acceso GTK");
        let vista = VistaGrafo::new();
        let datos = Rc::new(GrafoDatos::desde(
            vec!["wiki/a.md".into(), "wiki/b.md".into(), "wiki/c.md".into()],
            vec![("wiki/a.md".into(), "wiki/b.md".into())],
        ));
        let ultimo_estado = Rc::new(Cell::new(false));
        {
            let est = ultimo_estado.clone();
            vista.set_on_estado_fisica(move |pausado| est.set(pausado));
        }
        vista.set_datos(datos);
        assert!(!vista.esta_pausado());
        assert!(vista.fisica_corriendo());
        assert!(!ultimo_estado.get());

        // Pausar y reanudar.
        assert!(vista.pausar_reanudar());
        assert!(vista.esta_pausado());
        assert!(ultimo_estado.get());
        assert!(!vista.pausar_reanudar());
        assert!(!vista.esta_pausado());
        assert!(!ultimo_estado.get());

        // Fijar un nodo por arrastre y luego recalcular (libera el nodo fijo y despausa).
        vista.fijar_nodo_en_fisica(0, 123.0, 456.0);
        vista.pausar_reanudar();
        assert!(vista.esta_pausado());
        vista.recalcular();
        assert!(!vista.esta_pausado());
        vista.limpiar();

        // Selector de saltos 1 · 2 · 3 en PanelDerecho.
        let right = crate::editor::right_sidebar::RightSidebar::new();
        let panel = crate::panel_derecho::PanelDerecho::new(&right);
        let notificaciones = Rc::new(Cell::new(0u32));
        {
            let n = notificaciones.clone();
            panel.connect_modo(move |modo| {
                if modo == "grafo" {
                    n.set(n.get() + 1);
                }
            });
        }
        assert_eq!(panel.saltos(), 1);
        panel.set_saltos(2);
        assert_eq!(panel.saltos(), 2);
        assert_eq!(notificaciones.get(), 1);
        panel.set_saltos(3);
        assert_eq!(panel.saltos(), 3);
        assert_eq!(notificaciones.get(), 2);
        panel.set_saltos(1);
        assert_eq!(panel.saltos(), 1);
        assert_eq!(notificaciones.get(), 3);
    }

    /// T008 (030): BFS de 1, 2 y 3 saltos en una cadena A — B — C — D — E y radios concéntricos.
    #[test]
    fn us30_grafo_local_por_saltos_1_2_3() {
        let enlaces = vec![
            ("wiki/a.md".to_owned(), "wiki/b.md".to_owned()),
            ("wiki/b.md".to_owned(), "wiki/c.md".to_owned()),
            ("wiki/c.md".to_owned(), "wiki/d.md".to_owned()),
            ("wiki/d.md".to_owned(), "wiki/e.md".to_owned()),
        ];

        let g1 = GrafoDatos::local_saltos("wiki/a.md".into(), 1, &enlaces);
        assert_eq!(g1.rutas, ["wiki/a.md", "wiki/b.md"]);
        assert_eq!(g1.saltos, [0, 1]);
        assert_eq!(g1.aristas, [(0, 1)]);

        let g2 = GrafoDatos::local_saltos("wiki/a.md".into(), 2, &enlaces);
        assert_eq!(g2.rutas, ["wiki/a.md", "wiki/b.md", "wiki/c.md"]);
        assert_eq!(g2.saltos, [0, 1, 2]);
        assert_eq!(g2.aristas, [(0, 1), (1, 2)]);

        let g3 = GrafoDatos::local_saltos("wiki/a.md".into(), 3, &enlaces);
        assert_eq!(
            g3.rutas,
            ["wiki/a.md", "wiki/b.md", "wiki/c.md", "wiki/d.md"]
        );
        assert_eq!(g3.saltos, [0, 1, 2, 3]);
        assert_eq!(g3.aristas, [(0, 1), (1, 2), (2, 3)]);

        let radio = |p: (f64, f64)| (p.0 * p.0 + p.1 * p.1).sqrt();
        assert!(radio(g3.fijas[0]) < 1e-6);
        assert!(radio(g3.fijas[1]) < radio(g3.fijas[2]));
        assert!(radio(g3.fijas[2]) < radio(g3.fijas[3]));
    }

    /// SC-001 (030): mide el paso de simulación Barnes-Hut con 5000 nodos y ~15 000 aristas
    /// sin requerir display de GTK.
    #[test]
    fn sc_barnes_hut_5000() {
        let notas: Vec<String> = (0..MAX_NODOS).map(|n| format!("wiki/n{n:05}.md")).collect();
        let enlaces: Vec<(String, String)> = (0..MAX_NODOS)
            .flat_map(|n| {
                [n * 7 + 13, n * 31 + 5, n + 1].map(|m| {
                    (
                        format!("wiki/n{n:05}.md"),
                        format!("wiki/n{:05}.md", m % MAX_NODOS),
                    )
                })
            })
            .collect();
        let datos = GrafoDatos::desde(notas, enlaces);
        let aristas = datos.aristas.len();
        let mut arena = ArenaQuad::default();
        let mut pos: Vec<(f64, f64)> = (0..MAX_NODOS).map(|i| datos.posicion(i)).collect();
        let mut vel = vec![(0.0, 0.0); MAX_NODOS];
        let fijos = vec![false; MAX_NODOS];
        let centro = datos.centro_gravedad();
        let mut bh_ms: Vec<f64> = (0..30)
            .map(|k| {
                let t = std::time::Instant::now();
                paso_barnes_hut(
                    &mut arena,
                    &mut pos,
                    &mut vel,
                    &fijos,
                    &datos.aristas,
                    centro,
                    1.0 * ENFRIAMIENTO.powi(k),
                );
                t.elapsed().as_secs_f64() * 1000.0
            })
            .collect();
        bh_ms.sort_by(f64::total_cmp);
        println!(
            "BARNES-HUT 5000 nodos ({aristas} aristas): mediana {:.2} ms, p95 {:.2} ms, máx {:.2} ms",
            bh_ms[15], bh_ms[28], bh_ms[29]
        );
        // En debug puede tardar más; en release exigimos < 33.3 ms (≥ 30 fps).
        if !cfg!(debug_assertions) {
            assert!(
                bh_ms[15] < 33.3,
                "el paso Barnes-Hut en release debe sostener ≥ 30 fps (< 33,3 ms): mediana {:.2} ms",
                bh_ms[15]
            );
        }
    }

    /// T003/T004: el nodo bajo el mouse se encuentra por coordenadas del widget.
    #[test]
    #[ignore = "requiere acceso GTK; tools/verificar-008-us1.sh"]
    fn us8_nodo_en_por_coordenadas() {
        gtk::init().expect("se necesita acceso GTK");
        let vista = VistaGrafo::new();
        let datos = Rc::new(GrafoDatos::desde(
            vec!["wiki/a.md".into(), "wiki/b.md".into()],
            vec![("wiki/a.md".into(), "wiki/b.md".into())],
        ));
        vista.set_datos(datos);
        // Nodo 0 en (0,0) del mundo; con pan/zoom por defecto coincide con el widget.
        assert_eq!(vista.nodo_en(0.0, 0.0), Some(0));
        // Cerca de la posición del nodo 1.
        let (x, y) = {
            let d = vista.imp().datos.borrow().clone().expect("datos");
            d.posicion(1)
        };
        assert_eq!(vista.nodo_en(x, y), Some(1));
        // Lejos de ambos: nada.
        assert_eq!(vista.nodo_en(5000.0, 5000.0), None);
        vista.limpiar();
    }

    /// SC-1/SC-2 de la 008 y SC-001 de la 030: tiempo de armar el cuadro (snapshot) y tiempo de
    /// un paso de simulación Barnes-Hut con 5000 nodos y ~15000 aristas. Se corre en release:
    /// `cargo test --release --bin orbital grafo::tests::sc_cuadro_5000 -- --ignored --nocapture`.
    #[test]
    #[ignore = "medición; requiere acceso GTK"]
    fn sc_cuadro_5000() {
        gtk::init().expect("se necesita acceso GTK");
        let notas: Vec<String> = (0..MAX_NODOS).map(|n| format!("wiki/n{n:05}.md")).collect();
        let enlaces: Vec<(String, String)> = (0..MAX_NODOS)
            .flat_map(|n| {
                [n * 7 + 13, n * 31 + 5, n + 1].map(|m| {
                    (
                        format!("wiki/n{n:05}.md"),
                        format!("wiki/n{:05}.md", m % MAX_NODOS),
                    )
                })
            })
            .collect();
        let datos = Rc::new(GrafoDatos::desde(notas, enlaces));
        let aristas = datos.aristas.len();

        // Medición del paso Barnes-Hut con 5000 nodos y ~15 000 aristas.
        {
            let mut arena = ArenaQuad::default();
            let mut pos: Vec<(f64, f64)> = (0..MAX_NODOS).map(|i| datos.posicion(i)).collect();
            let mut vel = vec![(0.0, 0.0); MAX_NODOS];
            let fijos = vec![false; MAX_NODOS];
            let centro = datos.centro_gravedad();
            let mut bh_ms: Vec<f64> = (0..30)
                .map(|k| {
                    let t = std::time::Instant::now();
                    paso_barnes_hut(
                        &mut arena,
                        &mut pos,
                        &mut vel,
                        &fijos,
                        &datos.aristas,
                        centro,
                        1.0 * ENFRIAMIENTO.powi(k),
                    );
                    t.elapsed().as_secs_f64() * 1000.0
                })
                .collect();
            bh_ms.sort_by(f64::total_cmp);
            println!(
                "BARNES-HUT 5000 nodos ({aristas} aristas): mediana {:.2} ms, p95 {:.2} ms, máx {:.2} ms",
                bh_ms[15], bh_ms[28], bh_ms[29]
            );
            assert!(
                bh_ms[15] < 33.3,
                "el paso Barnes-Hut debe sostener ≥ 30 fps (< 33,3 ms): mediana {:.2} ms",
                bh_ms[15]
            );
        }

        let vista = VistaGrafo::new();
        vista.set_datos(datos);
        let ventana = gtk::Window::builder()
            .default_width(1280)
            .default_height(800)
            .child(&vista)
            .build();
        ventana.present();
        let ctx = glib::MainContext::default();
        for _ in 0..30 {
            ctx.iteration(false);
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert!(
            vista.width() > 1000 && vista.height() > 600,
            "el lienzo tiene que estar asignado: {}×{}",
            vista.width(),
            vista.height()
        );
        for (nombre, zoom, pan) in [
            ("todo el grafo", 0.25, 0.0),
            ("normal", 1.0, 900.0),
            ("con etiquetas", 2.5, 3000.0),
        ] {
            vista.imp().vista.set(Vista {
                pan_x: pan,
                pan_y: pan,
                zoom,
            });
            let mut ms: Vec<f64> = (0..60)
                .map(|_| {
                    let snap = gtk::Snapshot::new();
                    let t = std::time::Instant::now();
                    WidgetImpl::snapshot(vista.imp(), &snap);
                    let nodo = snap.to_node();
                    let ms = t.elapsed().as_secs_f64() * 1000.0;
                    assert!(nodo.is_some(), "el cuadro dibuja algo");
                    ms
                })
                .collect();
            ms.sort_by(f64::total_cmp);
            println!(
                "GRAFO {nombre} (zoom {zoom}, {aristas} aristas): mediana {:.2} ms, p95 {:.2} ms, máx {:.2} ms",
                ms[30], ms[57], ms[59]
            );
        }
        if let Some(png) = std::env::var_os("ORBITAL_CAPTURA_GRAFO") {
            vista.imp().vista.set(Vista {
                pan_x: 0.0,
                pan_y: 0.0,
                zoom: 1.0,
            });
            let snap = gtk::Snapshot::new();
            WidgetImpl::snapshot(vista.imp(), &snap);
            let node = snap.to_node().expect("cuadro");
            let renderer = ventana.renderer().expect("renderizador");
            renderer
                .render_texture(
                    &node,
                    Some(&gtk::graphene::Rect::new(0.0, 0.0, 1280.0, 800.0)),
                )
                .save_to_png(png)
                .unwrap();
        }
        let t = std::time::Instant::now();
        for k in 0..1000 {
            let _ = vista.nodo_en(f64::from(k % 1280), f64::from(k % 800));
        }
        println!(
            "GRAFO nodo_en: {:.1} µs por evento",
            t.elapsed().as_secs_f64() * 1e6 / 1000.0
        );
        vista.limpiar();
        ventana.destroy();
    }
}
