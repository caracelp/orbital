//! Spec 017: K-Plex — Brújula de la nota abierta.
//!
//! Muestra la nota abierta en el centro y sus relaciones existentes clasificadas en los cuatro
//! puntos cardinales:
//! - **Norte (Entrantes / Padres)**: notas que enlazan hacia la nota abierta (`u -> centro`).
//! - **Sur (Salientes / Hijos)**: notas a las que la nota abierta enlaza (`centro -> v`).
//! - **Oeste (Mutuos ↔)**: notas con enlace bidireccional (`u <-> centro`).
//! - **Este (Hermanas / 2.º salto)**: notas a distancia 2 en el grafo que comparten al menos un
//!   padre, hijo o vecino directo con la nota abierta, sin enlace directo.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::{Rc, Weak};

use adw::prelude::*;
use gtk::glib;

/// Entrada de un cuadrante cardinal de la Brújula.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ItemBrujula {
    /// Ruta relativa en la bóveda (p. ej. `wiki/nota.md`).
    pub ruta: String,
    /// Nombre corto sin extensión para mostrar.
    pub nombre: String,
    /// Clave doblada (`orbital_vault::fold`) de nombre y ruta para el filtro rápido en vivo.
    pub clave_filtro: String,
    /// Detalle adicional (en Este: el vecino puente por el que se conecta).
    pub detalle: Option<String>,
}

impl ItemBrujula {
    fn nuevo(ruta: String, detalle: Option<String>) -> Self {
        let nombre = nombre_corto(&ruta).to_owned();
        let clave_filtro = match &detalle {
            Some(d) => orbital_vault::fold(&format!("{nombre} {ruta} {d}")),
            None => orbital_vault::fold(&format!("{nombre} {ruta}")),
        };
        Self {
            ruta,
            nombre,
            clave_filtro,
            detalle,
        }
    }

    /// Indica si esta nota coincide con la consulta ya doblada con `orbital_vault::fold`.
    pub fn coincide(&self, consulta_doblada: &str) -> bool {
        consulta_doblada.is_empty() || self.clave_filtro.contains(consulta_doblada)
    }
}

/// Vecindario cardinal completo de una nota.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct VecindarioBrujula {
    /// Ruta de la nota central.
    pub centro: String,
    /// Nombre corto de la nota central.
    pub nombre_centro: String,
    /// Norte: enlaces entrantes (`u -> centro`).
    pub norte: Vec<ItemBrujula>,
    /// Sur: enlaces salientes (`centro -> v`).
    pub sur: Vec<ItemBrujula>,
    /// Oeste: enlaces mutuos (`u <-> centro`).
    pub oeste: Vec<ItemBrujula>,
    /// Este: notas hermanas / a 2 saltos sin enlace directo con `centro`.
    pub este: Vec<ItemBrujula>,
}

fn nombre_corto(ruta: &str) -> &str {
    let archivo = ruta.rsplit_once('/').map_or(ruta, |(_, f)| f);
    archivo.rsplit_once('.').map_or(archivo, |(s, _)| s)
}

impl VecindarioBrujula {
    /// Calcula los cuatro puntos cardinales de `centro` a partir de los enlaces dirigidos
    /// resueltos `(origen, destino)` del `LinkGraph`.
    pub fn calcular(centro: &str, enlaces: &[(String, String)]) -> Self {
        let mut entrantes: Vec<&str> = Vec::new();
        let mut salientes: Vec<&str> = Vec::new();

        for (orig, dest) in enlaces {
            if orig == dest {
                continue;
            }
            if dest == centro {
                entrantes.push(orig.as_str());
            } else if orig == centro {
                salientes.push(dest.as_str());
            }
        }
        entrantes.sort_unstable();
        entrantes.dedup();
        salientes.sort_unstable();
        salientes.dedup();

        let set_salientes: HashSet<&str> = salientes.iter().copied().collect();

        let mut mutuos: Vec<&str> = entrantes
            .iter()
            .copied()
            .filter(|u| set_salientes.contains(u))
            .collect();
        mutuos.sort_unstable();

        // Vecinos directos (1 salto): entrantes ∪ salientes.
        let mut vecinos_1: HashSet<&str> =
            HashSet::with_capacity(entrantes.len() + salientes.len() + 1);
        vecinos_1.insert(centro);
        for &u in &entrantes {
            vecinos_1.insert(u);
        }
        for &v in &salientes {
            vecinos_1.insert(v);
        }

        // Este (2.º salto / hermanas): notas conectadas con algún vecino directo `z` que no estén
        // en `vecinos_1`. Guardamos los puentes `z` ordenados para el detalle.
        let mut puentes_por_hermana: HashMap<&str, Vec<&str>> = HashMap::new();
        for (a, b) in enlaces {
            if a == b {
                continue;
            }
            let a_str = a.as_str();
            let b_str = b.as_str();
            if a_str != centro && vecinos_1.contains(a_str) && !vecinos_1.contains(b_str) {
                puentes_por_hermana.entry(b_str).or_default().push(a_str);
            }
            if b_str != centro && vecinos_1.contains(b_str) && !vecinos_1.contains(a_str) {
                puentes_por_hermana.entry(a_str).or_default().push(b_str);
            }
        }

        let mut hermanas_claves: Vec<&str> = puentes_por_hermana.keys().copied().collect();
        hermanas_claves.sort_unstable();

        let norte = entrantes
            .into_iter()
            .map(|r| ItemBrujula::nuevo(r.to_owned(), None))
            .collect();
        let sur = salientes
            .into_iter()
            .map(|r| ItemBrujula::nuevo(r.to_owned(), None))
            .collect();
        let oeste = mutuos
            .into_iter()
            .map(|r| ItemBrujula::nuevo(r.to_owned(), None))
            .collect();
        let este = hermanas_claves
            .into_iter()
            .map(|r| {
                let mut puentes = puentes_por_hermana.remove(r).unwrap_or_default();
                puentes.sort_unstable();
                puentes.dedup();
                let detalle = puentes.first().map(|p| {
                    let corto = nombre_corto(p);
                    if puentes.len() > 1 {
                        format!("vía {corto} (+{})", puentes.len() - 1)
                    } else {
                        format!("vía {corto}")
                    }
                });
                ItemBrujula::nuevo(r.to_owned(), detalle)
            })
            .collect();

        Self {
            centro: centro.to_owned(),
            nombre_centro: nombre_corto(centro).to_owned(),
            norte,
            sur,
            oeste,
            este,
        }
    }
}

type RutaFn = Box<dyn Fn(&str)>;
type CerrarFn = Box<dyn Fn()>;

struct CuadranteWidgets {
    titulo: gtk::Label,
    lista: gtk::Box,
    prefijo: &'static str,
    vacio_texto: &'static str,
}

struct Inner {
    root: gtk::Box,
    subtitulo_barra: adw::WindowTitle,
    buscador: gtk::SearchEntry,
    stack_contenido: gtk::Stack,
    vacio: adw::StatusPage,
    centro_nombre: gtk::Label,
    centro_ruta: gtk::Label,
    centro_resumen: gtk::Label,
    cuadrantes: [CuadranteWidgets; 4], // 0: Norte, 1: Sur, 2: Oeste, 3: Este
    datos: RefCell<Option<VecindarioBrujula>>,
    al_abrir: RefCell<Option<RutaFn>>,
    al_centrar: RefCell<Option<RutaFn>>,
    al_cerrar: RefCell<Option<CerrarFn>>,
}

#[derive(Clone)]
pub struct VistaBrujula(Rc<Inner>);

thread_local! {
    static INSTANCIAS: RefCell<Vec<(gtk::Box, Weak<Inner>)>> = const { RefCell::new(Vec::new()) };
}

fn construir_cuadrante(
    prefijo: &'static str,
    etiqueta_accesible: &str,
    vacio_texto: &'static str,
) -> (gtk::Box, CuadranteWidgets) {
    let titulo = gtk::Label::builder()
        .label(format!("{prefijo} (0)"))
        .xalign(0.0)
        .css_classes(["orbital-right-heading"])
        .build();
    let lista = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(6)
        .build();
    lista.update_property(&[gtk::accessible::Property::Label(etiqueta_accesible)]);
    let scroller = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .min_content_height(120)
        .max_content_height(210)
        .propagate_natural_height(true)
        .vexpand(true)
        .child(&lista)
        .build();
    let contenedor = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(8)
        .hexpand(true)
        .vexpand(true)
        .css_classes(["orbital-right-block"])
        .build();
    contenedor.append(&titulo);
    contenedor.append(&scroller);
    (
        contenedor,
        CuadranteWidgets {
            titulo,
            lista,
            prefijo,
            vacio_texto,
        },
    )
}

impl VistaBrujula {
    pub fn new() -> Self {
        let subtitulo_barra = adw::WindowTitle::new("Brújula de la nota", "");
        let buscador = gtk::SearchEntry::builder()
            .placeholder_text("Filtrar en la brújula…")
            .width_chars(24)
            .build();
        buscador.update_property(&[gtk::accessible::Property::Label(
            "Filtrar notas en la brújula",
        )]);
        let cerrar = gtk::Button::builder()
            .icon_name("window-close-symbolic")
            .tooltip_text("Cerrar la brújula (Esc)")
            .css_classes(["flat"])
            .build();
        cerrar.update_property(&[gtk::accessible::Property::Label("Cerrar la brújula")]);

        let barra = adw::HeaderBar::new();
        barra.set_title_widget(Some(&subtitulo_barra));
        barra.pack_start(&buscador);
        barra.pack_end(&cerrar);

        let (caja_norte, w_norte) = construir_cuadrante(
            "↑ NORTE · ENTRANTES",
            "Norte: notas entrantes que enlazan a esta nota",
            "Sin enlaces entrantes",
        );
        let (caja_sur, w_sur) = construir_cuadrante(
            "↓ SUR · SALIENTES",
            "Sur: notas salientes enlazadas desde esta nota",
            "Sin enlaces salientes",
        );
        let (caja_oeste, w_oeste) = construir_cuadrante(
            "← OESTE · MUTUOS ↔",
            "Oeste: notas con enlace mutuo bidireccional",
            "Sin enlaces mutuos",
        );
        let (caja_este, w_este) = construir_cuadrante(
            "→ ESTE · HERMANAS / 2.º SALTO",
            "Este: notas hermanas a dos saltos en el grafo",
            "Sin notas a 2 saltos",
        );

        // Tarjeta central con la nota abierta.
        let centro_etiqueta = gtk::Label::builder()
            .label("◎ NOTA CENTRAL")
            .xalign(0.5)
            .css_classes(["orbital-right-heading"])
            .build();
        let centro_nombre = gtk::Label::builder()
            .label("—")
            .xalign(0.5)
            .wrap(true)
            .css_classes(["title-3"])
            .build();
        let centro_ruta = gtk::Label::builder()
            .label("")
            .xalign(0.5)
            .ellipsize(gtk::pango::EllipsizeMode::Middle)
            .css_classes(["dim-label", "caption"])
            .build();
        let centro_resumen = gtk::Label::builder()
            .label("")
            .xalign(0.5)
            .css_classes(["orbital-tag-chip"])
            .build();
        let caja_centro = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(6)
            .valign(gtk::Align::Center)
            .halign(gtk::Align::Center)
            .width_request(220)
            .css_classes(["orbital-right-block"])
            .build();
        caja_centro.update_property(&[gtk::accessible::Property::Label(
            "Nota central de la brújula",
        )]);
        caja_centro.append(&centro_etiqueta);
        caja_centro.append(&centro_nombre);
        caja_centro.append(&centro_ruta);
        caja_centro.append(&centro_resumen);

        // Disposición en cruz: Norte arriba, Oeste - Centro - Este en la franja media, Sur abajo.
        let fila_norte = adw::Clamp::builder()
            .maximum_size(520)
            .child(&caja_norte)
            .build();
        let fila_media = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(16)
            .vexpand(true)
            .build();
        fila_media.append(&caja_oeste);
        fila_media.append(&caja_centro);
        fila_media.append(&caja_este);
        let fila_sur = adw::Clamp::builder()
            .maximum_size(520)
            .child(&caja_sur)
            .build();

        let cruz = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(16)
            .margin_top(16)
            .margin_bottom(16)
            .margin_start(16)
            .margin_end(16)
            .vexpand(true)
            .build();
        cruz.append(&fila_norte);
        cruz.append(&fila_media);
        cruz.append(&fila_sur);

        let cruz_scroll = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vexpand(true)
            .child(&cruz)
            .build();

        let vacio = adw::StatusPage::builder()
            .icon_name("compass-symbolic")
            .title("Sin nota activa para la brújula")
            .description("Abrí una nota de la bóveda para explorar sus conexiones cardinales.")
            .build();

        let stack_contenido = gtk::Stack::builder()
            .transition_type(gtk::StackTransitionType::Crossfade)
            .vexpand(true)
            .build();
        stack_contenido.add_named(&vacio, Some("vacio"));
        stack_contenido.add_named(&cruz_scroll, Some("cruz"));
        stack_contenido.set_visible_child_name("vacio");

        let root = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .vexpand(true)
            .hexpand(true)
            .build();
        root.update_property(&[gtk::accessible::Property::Label(
            "Brújula de relaciones de la nota",
        )]);
        root.append(&barra);
        root.append(&stack_contenido);

        let this = Self(Rc::new(Inner {
            root: root.clone(),
            subtitulo_barra,
            buscador: buscador.clone(),
            stack_contenido,
            vacio,
            centro_nombre,
            centro_ruta,
            centro_resumen,
            cuadrantes: [w_norte, w_sur, w_oeste, w_este],
            datos: RefCell::new(None),
            al_abrir: RefCell::new(None),
            al_centrar: RefCell::new(None),
            al_cerrar: RefCell::new(None),
        }));

        INSTANCIAS.with(|inst| {
            inst.borrow_mut().retain(|(_, w)| w.strong_count() > 0);
            inst.borrow_mut().push((root, Rc::downgrade(&this.0)));
        });

        {
            let weak = Rc::downgrade(&this.0);
            buscador.connect_search_changed(move |_| {
                if let Some(i) = weak.upgrade() {
                    Self(i).repoblar_cuadrantes();
                }
            });
        }
        {
            let weak = Rc::downgrade(&this.0);
            cerrar.connect_clicked(move |_| {
                if let Some(i) = weak.upgrade()
                    && let Some(f) = i.al_cerrar.borrow().as_ref()
                {
                    f();
                }
            });
        }
        {
            let teclas = gtk::EventControllerKey::new();
            let weak = Rc::downgrade(&this.0);
            teclas.connect_key_pressed(move |_, keyval, _, _| {
                if keyval == gtk::gdk::Key::Escape
                    && let Some(i) = weak.upgrade()
                {
                    if !i.buscador.text().is_empty() {
                        i.buscador.set_text("");
                        return glib::Propagation::Stop;
                    }
                    if let Some(f) = i.al_cerrar.borrow().as_ref() {
                        f();
                        return glib::Propagation::Stop;
                    }
                }
                glib::Propagation::Proceed
            });
            this.0.root.add_controller(teclas);
        }

        this
    }

    /// Recupera la `VistaBrujula` registrada como página `"brujula"` dentro de `stack`.
    pub fn desde_stack(stack: &gtk::Stack) -> Option<Self> {
        let child = stack.child_by_name("brujula")?;
        let caja = child.downcast::<gtk::Box>().ok()?;
        INSTANCIAS.with(|inst| {
            inst.borrow()
                .iter()
                .find(|(b, _)| *b == caja)
                .and_then(|(_, w)| w.upgrade())
                .map(Self)
        })
    }

    pub fn widget(&self) -> &gtk::Box {
        &self.0.root
    }

    pub fn connect_abrir(&self, f: impl Fn(&str) + 'static) {
        *self.0.al_abrir.borrow_mut() = Some(Box::new(f));
    }

    pub fn connect_centrar(&self, f: impl Fn(&str) + 'static) {
        *self.0.al_centrar.borrow_mut() = Some(Box::new(f));
    }

    pub fn connect_cerrar(&self, f: impl Fn() + 'static) {
        *self.0.al_cerrar.borrow_mut() = Some(Box::new(f));
    }

    /// Muestra un estado vacío con el mensaje indicado.
    pub fn set_vacio(&self, descripcion: &str) {
        let i = &self.0;
        *i.datos.borrow_mut() = None;
        i.subtitulo_barra.set_subtitle("");
        i.vacio.set_description(Some(descripcion));
        i.stack_contenido.set_visible_child_name("vacio");
    }

    /// Carga el vecindario calculado y puebla los cuatro puntos cardinales.
    pub fn set_vecindario(&self, vecindario: VecindarioBrujula) {
        let i = &self.0;
        i.subtitulo_barra.set_subtitle(&vecindario.centro);
        i.centro_nombre.set_label(&vecindario.nombre_centro);
        i.centro_ruta.set_label(&vecindario.centro);
        i.centro_resumen.set_label(&format!(
            "↑ {} · ↓ {} · ↔ {} · → {}",
            vecindario.norte.len(),
            vecindario.sur.len(),
            vecindario.oeste.len(),
            vecindario.este.len()
        ));
        *i.datos.borrow_mut() = Some(vecindario);
        i.stack_contenido.set_visible_child_name("cruz");
        self.repoblar_cuadrantes();
    }

    /// Cambia el texto del filtro rápido (usado en pruebas).
    #[cfg(test)]
    pub fn set_filtro(&self, texto: &str) {
        self.0.buscador.set_text(texto);
    }

    /// Cantidad de filas actualmente visibles en `[Norte, Sur, Oeste, Este]`.
    #[cfg(test)]
    pub fn visibles_por_cuadrante(&self) -> [usize; 4] {
        let consulta = orbital_vault::fold(self.0.buscador.text().trim());
        let Some(d) = self.0.datos.borrow().clone() else {
            return [0; 4];
        };
        [
            d.norte.iter().filter(|x| x.coincide(&consulta)).count(),
            d.sur.iter().filter(|x| x.coincide(&consulta)).count(),
            d.oeste.iter().filter(|x| x.coincide(&consulta)).count(),
            d.este.iter().filter(|x| x.coincide(&consulta)).count(),
        ]
    }

    fn repoblar_cuadrantes(&self) {
        let i = &self.0;
        let Some(datos) = i.datos.borrow().clone() else {
            return;
        };
        let consulta = orbital_vault::fold(i.buscador.text().trim());
        let listas_datos = [&datos.norte, &datos.sur, &datos.oeste, &datos.este];

        for (cuad, items) in i.cuadrantes.iter().zip(listas_datos) {
            while let Some(hijo) = cuad.lista.first_child() {
                cuad.lista.remove(&hijo);
            }
            let filtrados: Vec<&ItemBrujula> =
                items.iter().filter(|it| it.coincide(&consulta)).collect();
            if consulta.is_empty() {
                cuad.titulo
                    .set_label(&format!("{} ({})", cuad.prefijo, items.len()));
            } else {
                cuad.titulo.set_label(&format!(
                    "{} ({}/{})",
                    cuad.prefijo,
                    filtrados.len(),
                    items.len()
                ));
            }
            if filtrados.is_empty() {
                let msg = if consulta.is_empty() {
                    cuad.vacio_texto
                } else {
                    "Sin coincidencias con el filtro"
                };
                cuad.lista.append(
                    &gtk::Label::builder()
                        .label(msg)
                        .xalign(0.0)
                        .css_classes(["orbital-context-empty"])
                        .build(),
                );
                continue;
            }
            for item in filtrados {
                let fila = gtk::Box::builder()
                    .orientation(gtk::Orientation::Horizontal)
                    .spacing(4)
                    .build();
                let texto_boton = match &item.detalle {
                    Some(d) => format!("{}  ·  {}", item.nombre, d),
                    None => item.nombre.clone(),
                };
                let b_abrir = gtk::Button::builder()
                    .child(
                        &gtk::Label::builder()
                            .label(&texto_boton)
                            .xalign(0.0)
                            .ellipsize(gtk::pango::EllipsizeMode::End)
                            .build(),
                    )
                    .hexpand(true)
                    .tooltip_text(&item.ruta)
                    .css_classes(["orbital-context-card"])
                    .build();
                b_abrir.update_property(&[gtk::accessible::Property::Label(&format!(
                    "Abrir {}",
                    item.nombre
                ))]);
                let weak = Rc::downgrade(&self.0);
                let ruta_abrir = item.ruta.clone();
                b_abrir.connect_clicked(move |_| {
                    if let Some(inner) = weak.upgrade()
                        && let Some(f) = inner.al_abrir.borrow().as_ref()
                    {
                        f(&ruta_abrir);
                    }
                });

                let b_centrar = gtk::Button::builder()
                    .icon_name("find-location-symbolic")
                    .tooltip_text(format!("Centrar la brújula en {}", item.nombre))
                    .css_classes(["flat", "orbital-iconbtn"])
                    .build();
                b_centrar.update_property(&[gtk::accessible::Property::Label(&format!(
                    "Centrar la brújula en {}",
                    item.nombre
                ))]);
                let weak_c = Rc::downgrade(&self.0);
                let ruta_centrar = item.ruta.clone();
                b_centrar.connect_clicked(move |_| {
                    if let Some(inner) = weak_c.upgrade()
                        && let Some(f) = inner.al_centrar.borrow().as_ref()
                    {
                        f(&ruta_centrar);
                    }
                });

                fila.append(&b_abrir);
                fila.append(&b_centrar);
                cuad.lista.append(&fila);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// T005: verifica que Norte, Sur, Oeste (mutuos) y Este (hermanas / 2.º salto) se calculan
    /// exactamente sobre las relaciones dirigidas existentes.
    #[test]
    fn us17_clasifica_norte_sur_oeste_este() {
        let enlaces = vec![
            // Padre puro -> centro
            ("wiki/padre.md".to_owned(), "wiki/centro.md".to_owned()),
            // Centro -> hijo puro
            ("wiki/centro.md".to_owned(), "wiki/hijo.md".to_owned()),
            // Enlace mutuo <-> centro
            ("wiki/mutua.md".to_owned(), "wiki/centro.md".to_owned()),
            ("wiki/centro.md".to_owned(), "wiki/mutua.md".to_owned()),
            // Hermana (comparte padre con centro: padre -> hermana)
            ("wiki/padre.md".to_owned(), "wiki/hermana_p.md".to_owned()),
            // Segundo salto por hijo (hijo -> nieta)
            ("wiki/hijo.md".to_owned(), "wiki/nieta_h.md".to_owned()),
            // Tercer salto (no debe entrar en Este)
            ("wiki/nieta_h.md".to_owned(), "wiki/lejana.md".to_owned()),
        ];

        let v = VecindarioBrujula::calcular("wiki/centro.md", &enlaces);
        assert_eq!(v.nombre_centro, "centro");

        fn rutas(items: &[ItemBrujula]) -> Vec<&str> {
            items.iter().map(|i| i.ruta.as_str()).collect()
        }
        assert_eq!(rutas(&v.norte), ["wiki/mutua.md", "wiki/padre.md"]);
        assert_eq!(rutas(&v.sur), ["wiki/hijo.md", "wiki/mutua.md"]);
        assert_eq!(rutas(&v.oeste), ["wiki/mutua.md"]);
        assert_eq!(rutas(&v.este), ["wiki/hermana_p.md", "wiki/nieta_h.md"]);
        assert_eq!(v.este[0].detalle.as_deref(), Some("vía padre"));
        assert_eq!(v.este[1].detalle.as_deref(), Some("vía hijo"));
    }

    /// T005: filtro rápido insensible a mayúsculas y tildes.
    #[test]
    fn us17_filtro_insensible_a_mayusculas_y_tildes() {
        let item = ItemBrujula::nuevo(
            "wiki/Mecánica Cuántica.md".into(),
            Some("vía Física".into()),
        );
        let q1 = orbital_vault::fold("mecanica");
        let q2 = orbital_vault::fold("FISICA");
        let q3 = orbital_vault::fold("biologia");
        assert!(item.coincide(&q1));
        assert!(item.coincide(&q2));
        assert!(!item.coincide(&q3));
    }

    /// SC-001 (017): mide el cálculo del vecindario y el filtrado con 5000 notas y ~15 000 aristas.
    #[test]
    fn sc_brujula_5000() {
        let n_max = 5000usize;
        let enlaces: Vec<(String, String)> = (0..n_max)
            .flat_map(|n| {
                [n * 7 + 13, n * 31 + 5, n + 1].map(|m| {
                    (
                        format!("wiki/n{n:05}.md"),
                        format!("wiki/n{:05}.md", m % n_max),
                    )
                })
            })
            .collect();
        let t0 = std::time::Instant::now();
        let v = VecindarioBrujula::calcular("wiki/n00013.md", &enlaces);
        let ms = t0.elapsed().as_secs_f64() * 1000.0;
        println!(
            "BRÚJULA 5000 notas ({} aristas): {:.2} ms (N={}, S={}, O={}, E={})",
            enlaces.len(),
            ms,
            v.norte.len(),
            v.sur.len(),
            v.oeste.len(),
            v.este.len()
        );
        if !cfg!(debug_assertions) {
            assert!(
                ms < 10.0,
                "el cálculo en release debe ser < 10 ms: {ms:.2} ms"
            );
        }
    }

    /// T005: prueba GTK en proceso de VistaBrujula, filtro rápido y callbacks de abrir/recentrar.
    #[test]
    #[ignore = "requiere acceso GTK; tools/pruebas-gtk.sh"]
    fn us17_vista_brujula_y_filtro_gtk() {
        gtk::init().expect("se necesita acceso GTK");
        let vista = VistaBrujula::new();
        let enlaces = vec![
            ("wiki/alfa.md".to_owned(), "wiki/centro.md".to_owned()),
            ("wiki/centro.md".to_owned(), "wiki/beta.md".to_owned()),
            ("wiki/gamma.md".to_owned(), "wiki/centro.md".to_owned()),
            ("wiki/centro.md".to_owned(), "wiki/gamma.md".to_owned()),
            ("wiki/alfa.md".to_owned(), "wiki/delta.md".to_owned()),
        ];
        let vecindario = VecindarioBrujula::calcular("wiki/centro.md", &enlaces);
        vista.set_vecindario(vecindario);
        assert_eq!(vista.visibles_por_cuadrante(), [2, 2, 1, 1]);

        vista.set_filtro("GAMMA");
        assert_eq!(vista.visibles_por_cuadrante(), [1, 1, 1, 0]);

        vista.set_filtro("delta");
        assert_eq!(vista.visibles_por_cuadrante(), [0, 0, 0, 1]);

        vista.set_filtro("");
        assert_eq!(vista.visibles_por_cuadrante(), [2, 2, 1, 1]);
    }
}
