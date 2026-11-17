//! Notas en tres partes (especificación 041): diagnóstico, estructuración sin pérdida y barra
//! interactiva para las 3 partes obligatorias (`Propiedades YAML`, `Cuerpo en bloques`,
//! `Conexiones`) y las 2 partes opcionales (`Síntesis`, `Fuentes`).
//!
//! Orden canónico en el archivo `.md`: Propiedades siempre primero, Conexiones siempre última y el
//! Cuerpo en el medio; Síntesis y Fuentes nunca quedan antes de las Propiedades ni después de las
//! Conexiones.
//! 1. `--- ... ---` (Parte 1 obligatoria: Propiedades YAML con `id` y `type`)
//! 2. `<!-- summary <ID> --> ... <!---->` (Parte opcional A: Síntesis / Resumen)
//! 3. `<!-- block <jerarquía> <ID> --> ... <!---->` (Parte 2 obligatoria: Cuerpo en bloques)
//! 4. `<!-- sources <ID> --> ... <!---->` (Parte opcional B: Fuentes / Bibliografía)
//! 5. `<!-- connections <ID> --> ... <!---->` (Parte 3 obligatoria: Conexiones)

use std::ops::Range;
use std::rc::Rc;

use adw::prelude::*;

use crate::grafo::ramas_libres::{
    ArbolRamas, extraer_bloques_delimitados, parse_apertura_bloque, siguiente_id_nota,
};

/// Secciones especiales delimitadas por comentarios HTML (además de `<!-- block ... -->`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TipoSeccionNota {
    /// `<!-- summary <ID> --> ... <!---->` (Opcional A: Síntesis / Resumen).
    Summary,
    /// `<!-- connections <ID> --> ... <!---->` (Parte 3 obligatoria: Conexiones).
    Connections,
    /// `<!-- sources <ID> --> ... <!---->` (Opcional B: Fuentes / Bibliografía).
    Sources,
}

impl TipoSeccionNota {
    /// Etiqueta discreta que reemplaza al comentario de apertura en la vista Lectura.
    pub fn etiqueta_lectura(self) -> &'static str {
        match self {
            Self::Summary => "▸ SÍNTESIS",
            Self::Connections => "▸ CONEXIONES",
            Self::Sources => "▸ FUENTES",
        }
    }
}

/// Partes opcionales que el usuario puede insertar con un clic desde la barra de estructura.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ParteOpcional {
    Sintesis,
    Fuentes,
}

/// Sección delimitada hallada en el texto de una nota.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SeccionDelimitada {
    pub tipo: TipoSeccionNota,
    pub id_nota: String,
    /// Rango en bytes del bloque completo (desde la línea `<!-- ... -->` hasta `<!---->`).
    pub rango_total: Range<usize>,
    /// Rango en bytes del contenido interior.
    pub rango_contenido: Range<usize>,
    /// Línea (base 1) donde comienza la marca `<!-- ... -->`.
    pub linea: usize,
    /// Contenido interior limpio de la sección.
    pub contenido: String,
}

/// Diagnóstico completo de las 3 partes obligatorias y 2 opcionales de una nota `.md`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiagnosticoTresPartes {
    /// Identificador único de la nota (`"AP-001"`).
    pub id_nota: String,
    /// Parte 1 (Obligatoria): tiene frontmatter YAML con `id` y `type` no vacíos.
    pub tiene_propiedades: bool,
    /// Línea (base 1) donde inician las propiedades YAML.
    pub linea_propiedades: usize,
    /// Parte opcional A: `<!-- summary <ID> -->`.
    pub sintesis: Option<SeccionDelimitada>,
    /// Parte 2 (Obligatoria): cantidad de bloques `<!-- block ... -->` en el cuerpo.
    pub cantidad_bloques_cuerpo: usize,
    /// Línea (base 1) del primer bloque del cuerpo.
    pub linea_cuerpo: Option<usize>,
    /// Parte 3 (Obligatoria): `<!-- connections <ID> -->`.
    pub conexiones: Option<SeccionDelimitada>,
    /// Destinos de enlaces `[[...]]` declarados dentro de `connections`.
    pub enlaces_en_conexiones: Vec<String>,
    /// Parte opcional B: `<!-- sources <ID> -->`.
    pub fuentes: Option<SeccionDelimitada>,
}

impl DiagnosticoTresPartes {
    /// Devuelve `true` si la nota cumple las 3 partes obligatorias (`Propiedades`, `Cuerpo`, `Conexiones`).
    pub fn es_completa_tres_partes(&self) -> bool {
        self.tiene_propiedades && self.cantidad_bloques_cuerpo > 0 && self.conexiones.is_some()
    }

    /// Cantidad de partes obligatorias presentes (de 0 a 3).
    pub fn partes_obligatorias_completas(&self) -> usize {
        usize::from(self.tiene_propiedades)
            + usize::from(self.cantidad_bloques_cuerpo > 0)
            + usize::from(self.conexiones.is_some())
    }

    /// Analiza el texto completo de una nota `.md`.
    pub fn analizar(texto: &str, id_sugerido: Option<&str>) -> Self {
        let (tiene_propiedades, id_fm, tipo_fm) = analizar_frontmatter_nota(texto);
        let bloques = extraer_bloques_delimitados(texto);
        let secciones = extraer_secciones_delimitadas(texto);

        let id_de_bloque = bloques
            .iter()
            .find_map(|b| (!b.id_nota.is_empty()).then(|| b.id_nota.clone()))
            .or_else(|| {
                secciones
                    .iter()
                    .find_map(|s| (!s.id_nota.is_empty()).then(|| s.id_nota.clone()))
            });

        let id_nota = id_fm
            .or(id_de_bloque)
            .or_else(|| id_sugerido.map(str::to_owned))
            .unwrap_or_else(|| {
                // Solo para mostrar: el id que se escribe lo da `Window::con_id_de_nota`.
                let t = tipo_fm.as_deref().unwrap_or("atomo");
                siguiente_id_nota(t, std::iter::empty())
            });

        let sintesis = secciones
            .iter()
            .find(|s| s.tipo == TipoSeccionNota::Summary)
            .cloned();
        let conexiones = secciones
            .iter()
            .find(|s| s.tipo == TipoSeccionNota::Connections)
            .cloned();
        let fuentes = secciones
            .iter()
            .find(|s| s.tipo == TipoSeccionNota::Sources)
            .cloned();

        let enlaces_en_conexiones = conexiones
            .as_ref()
            .map(|c| extraer_enlaces_wikilink(&c.contenido))
            .unwrap_or_default();

        let linea_cuerpo = bloques.first().map(|b| b.linea + 1);

        Self {
            id_nota,
            tiene_propiedades,
            linea_propiedades: 1,
            sintesis,
            cantidad_bloques_cuerpo: bloques.len(),
            linea_cuerpo,
            conexiones,
            enlaces_en_conexiones,
            fuentes,
        }
    }
}

/// Analiza si una línea es un comentario de apertura `<!-- summary [ID] -->`,
/// `<!-- connections [ID] -->` o `<!-- sources [ID] -->` (aceptando también sus alias en español).
pub fn parse_apertura_seccion(linea: &str) -> Option<(TipoSeccionNota, String)> {
    let t = linea.trim();
    if t == "<!---->" || !t.starts_with("<!--") {
        return None;
    }
    let interior = t.strip_prefix("<!--")?.strip_suffix("-->")?.trim();
    if interior.is_empty() || interior.starts_with("block") {
        return None;
    }
    let mut partes = interior.split_whitespace();
    let clave = partes.next()?;
    let tipo = if clave.eq_ignore_ascii_case("summary")
        || clave.eq_ignore_ascii_case("sintesis")
        || clave == "síntesis"
        || clave == "SÍNTESIS"
    {
        TipoSeccionNota::Summary
    } else if clave.eq_ignore_ascii_case("connections") || clave.eq_ignore_ascii_case("conexiones")
    {
        TipoSeccionNota::Connections
    } else if clave.eq_ignore_ascii_case("sources")
        || clave.eq_ignore_ascii_case("fuentes")
        || clave.eq_ignore_ascii_case("bibliography")
    {
        TipoSeccionNota::Sources
    } else {
        return None;
    };
    let id_nota = partes.collect::<Vec<_>>().join(" ");
    Some((tipo, id_nota))
}

/// Si la línea es la apertura de una sección (`summary`, `connections`, `sources`), devuelve su
/// etiqueta legible para la vista Lectura (`"▸ SÍNTESIS"`, `"▸ CONEXIONES"`, `"▸ FUENTES"`).
pub fn etiqueta_apertura_seccion_lectura(linea: &str) -> Option<&'static str> {
    parse_apertura_seccion(linea).map(|(t, _)| t.etiqueta_lectura())
}

/// Extrae las secciones `summary`, `connections` y `sources` delimitadas en `texto` (con líneas base 1).
pub fn extraer_secciones_delimitadas(texto: &str) -> Vec<SeccionDelimitada> {
    let mut out = Vec::new();
    let mut offset = 0usize;
    // (tipo, id_nota, inicio_total, inicio_contenido, linea_1_based)
    let mut actual: Option<(TipoSeccionNota, String, usize, usize, usize)> = None;

    for (idx_linea, l) in texto.split_inclusive('\n').enumerate() {
        let sin_eol = l.trim_end_matches(['\n', '\r']);
        let sig_offset = offset + l.len();
        let linea_1 = idx_linea + 1;

        if let Some((tipo, id_nota)) = parse_apertura_seccion(sin_eol) {
            if let Some((t_ant, id_ant, tot_ant, ini_ant, lin_ant)) = actual.take() {
                let cuerpo = texto[ini_ant..offset].trim_matches(['\n', '\r']).to_owned();
                out.push(SeccionDelimitada {
                    tipo: t_ant,
                    id_nota: id_ant,
                    rango_total: tot_ant..offset,
                    rango_contenido: ini_ant..offset,
                    linea: lin_ant,
                    contenido: cuerpo,
                });
            }
            actual = Some((tipo, id_nota, offset, sig_offset, linea_1));
        } else if parse_apertura_bloque(sin_eol).is_some() {
            if let Some((t_ant, id_ant, tot_ant, ini_ant, lin_ant)) = actual.take() {
                let cuerpo = texto[ini_ant..offset].trim_matches(['\n', '\r']).to_owned();
                out.push(SeccionDelimitada {
                    tipo: t_ant,
                    id_nota: id_ant,
                    rango_total: tot_ant..offset,
                    rango_contenido: ini_ant..offset,
                    linea: lin_ant,
                    contenido: cuerpo,
                });
            }
        } else if sin_eol.trim() == "<!---->"
            && let Some((t_ant, id_ant, tot_ant, ini_ant, lin_ant)) = actual.take()
        {
            let fin_cuerpo = offset.max(ini_ant);
            let cuerpo = texto[ini_ant..fin_cuerpo]
                .trim_matches(['\n', '\r'])
                .to_owned();
            out.push(SeccionDelimitada {
                tipo: t_ant,
                id_nota: id_ant,
                rango_total: tot_ant..sig_offset,
                rango_contenido: ini_ant..fin_cuerpo,
                linea: lin_ant,
                contenido: cuerpo,
            });
        }
        offset = sig_offset;
    }

    if let Some((t_ant, id_ant, tot_ant, ini_ant, lin_ant)) = actual.take() {
        let cuerpo = texto[ini_ant..].trim_matches(['\n', '\r']).to_owned();
        out.push(SeccionDelimitada {
            tipo: t_ant,
            id_nota: id_ant,
            rango_total: tot_ant..texto.len(),
            rango_contenido: ini_ant..texto.len(),
            linea: lin_ant,
            contenido: cuerpo,
        });
    }
    out
}

/// Devuelve una copia de `texto` omitiendo los bloques delimitados `summary`, `connections` y `sources`.
pub fn quitar_secciones_delimitadas(texto: &str) -> String {
    let secciones = extraer_secciones_delimitadas(texto);
    if secciones.is_empty() {
        return texto.to_owned();
    }
    let mut out = String::with_capacity(texto.len());
    let mut cursor = 0usize;
    for s in secciones {
        if s.rango_total.start > cursor {
            out.push_str(&texto[cursor..s.rango_total.start]);
        }
        cursor = s.rango_total.end;
    }
    if cursor < texto.len() {
        out.push_str(&texto[cursor..]);
    }
    out
}

/// Extrae los destinos de todos los wikilinks `[[destino]]` o `[[destino|alias]]` presentes en `texto`.
pub fn extraer_enlaces_wikilink(texto: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut resto = texto;
    while let Some(ini) = resto.find("[[") {
        let despues = &resto[ini + 2..];
        let Some(fin) = despues.find("]]") else {
            break;
        };
        let interior = &despues[..fin];
        let destino = interior.split('|').next().unwrap_or(interior).trim();
        if !destino.is_empty() && !out.iter().any(|e| e == destino) {
            out.push(destino.to_owned());
        }
        resto = &despues[fin + 2..];
    }
    out
}

fn analizar_frontmatter_nota(texto: &str) -> (bool, Option<String>, Option<String>) {
    let sin_bom = texto.strip_prefix('\u{feff}').unwrap_or(texto);
    let Some(resto) = sin_bom
        .strip_prefix("---\n")
        .or_else(|| sin_bom.strip_prefix("---\r\n"))
    else {
        return (false, None, None);
    };
    let mut fin = None;
    let mut offset = 0usize;
    for l in resto.split_inclusive('\n') {
        let t = l.trim();
        if t == "---" || t == "..." {
            fin = Some(offset);
            break;
        }
        offset += l.len();
    }
    let cerrado = fin.is_some();
    // El tipo es `type` o su alias `tipo` (gana `type` si están las dos).
    let (id, tipo) = crate::grafo::ramas_libres::extraer_id_y_tipo_de_frontmatter(
        &resto[..fin.unwrap_or(resto.len())],
    );
    let tiene_props = cerrado && id.is_some() && tipo.is_some();
    (tiene_props, id, tipo)
}

/// Estructura una nota `.md` garantizando sus **3 partes obligatorias** (`Propiedades YAML` con `id` y
/// `type`, `Cuerpo` en bloques `<!-- block ... -->` y sección `<!-- connections <ID> -->` al pie) y
/// preservando cualquier contenido o parte opcional (`summary`, `sources`) existente. Reescribe las
/// partes en el orden canónico: Propiedades → Síntesis → Cuerpo → Fuentes → Conexiones.
///
/// Devuelve `(nuevo_texto, linea_destino_1_based)`.
pub fn estructurar_en_tres_partes(texto: &str, id_sugerido: Option<&str>) -> (String, usize) {
    let mut arbol = ArbolRamas::parse(texto, id_sugerido);
    if arbol.connections.is_none() {
        arbol.connections = Some("## Conexiones\n\n- ".to_owned());
    }
    let nuevo = arbol.serialize();
    let diag = DiagnosticoTresPartes::analizar(&nuevo, Some(&arbol.id_nota));
    let linea = diag
        .conexiones
        .as_ref()
        .map(|c| c.linea)
        .or(diag.linea_cuerpo)
        .unwrap_or(1);
    (nuevo, linea)
}

/// Inserta una de las 2 partes opcionales (`ParteOpcional::Sintesis` o `ParteOpcional::Fuentes`) en su
/// posición canónica dentro de la nota (la Síntesis después de las Propiedades; las Fuentes después
/// del Cuerpo y antes de las Conexiones), estructurando las partes si fuera necesario.
///
/// Devuelve `(nuevo_texto, linea_de_la_parte_1_based)`.
pub fn insertar_parte_opcional(
    texto: &str,
    parte: ParteOpcional,
    id_sugerido: Option<&str>,
) -> (String, usize) {
    // Si la nota ya tiene bloques delimitados usamos ArbolRamas para mantener todo canónico;
    // si no, igual ArbolRamas estructura sin pérdida.
    let mut arbol = ArbolRamas::parse(texto, id_sugerido);
    match parte {
        ParteOpcional::Sintesis => {
            if arbol.summary.is_none() {
                arbol.summary = Some("## Síntesis\n\nIdea central de la nota.".to_owned());
            }
        }
        ParteOpcional::Fuentes => {
            if arbol.sources.is_none() {
                arbol.sources = Some("## Fuentes\n\n- ".to_owned());
            }
        }
    }
    let nuevo = arbol.serialize();
    let diag = DiagnosticoTresPartes::analizar(&nuevo, Some(&arbol.id_nota));
    let linea = match parte {
        ParteOpcional::Sintesis => diag.sintesis.map(|s| s.linea).unwrap_or(1),
        ParteOpcional::Fuentes => diag.fuentes.map(|f| f.linea).unwrap_or(1),
    };
    (nuevo, linea)
}

/// Callbacks de la barra de estructura de la nota en la cabecera de la vista Lectura.
pub struct AccionesTresPartes {
    /// Desplaza el editor a la línea (base 1) indicada.
    pub ir_a_linea: Box<dyn Fn(usize)>,
    /// Estructura la nota asegurando las 3 partes obligatorias.
    pub estructurar: Box<dyn Fn()>,
    /// Inserta una parte opcional (`Síntesis` o `Fuentes`) si aún no existe.
    pub insertar_opcional: Box<dyn Fn(ParteOpcional)>,
    /// Abre un enlace `[[destino]]` desde las píldoras de conexiones.
    pub abrir_enlace: Box<dyn Fn(&str)>,
}

/// Construye la barra y resumen interactivo de las 3 partes obligatorias y 2 opcionales para la
/// cabecera de una nota `.md`.
pub fn construir_barra_tres_partes(
    diag: &DiagnosticoTresPartes,
    retroenlaces: &[String],
    solo_lectura: bool,
    acciones: Rc<AccionesTresPartes>,
) -> gtk::Widget {
    let contenedor = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(6)
        .margin_top(4)
        .build();
    contenedor.add_css_class("orbital-tres-partes");

    let fila_chips = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(6)
        .build();

    let completas = diag.partes_obligatorias_completas();
    let insignia = gtk::Label::builder()
        .label(format!("{} · {completas}/3 partes", diag.id_nota))
        .css_classes(["caption", "heading"])
        .valign(gtk::Align::Center)
        .margin_end(4)
        .tooltip_text(
            "Estructura de la nota: 1) Propiedades YAML, 2) Cuerpo en bloques, 3) Conexiones",
        )
        .build();
    fila_chips.append(&insignia);

    // Chip 1: Propiedades YAML (Obligatoria 1)
    let btn_props = gtk::Button::builder()
        .label(if diag.tiene_propiedades {
            "1 · Propiedades ✓"
        } else {
            "1 · Propiedades ⚠"
        })
        .tooltip_text(if diag.tiene_propiedades {
            "Parte 1 (obligatoria): Propiedades YAML con id y type (clic para ir)"
        } else {
            "Parte 1 (obligatoria): Falta id o type en las propiedades YAML (clic para completar)"
        })
        .css_classes(if diag.tiene_propiedades {
            vec!["flat", "caption"]
        } else {
            vec!["suggested-action", "caption"]
        })
        .build();
    {
        let acc = acciones.clone();
        let tiene = diag.tiene_propiedades;
        let lin = diag.linea_propiedades;
        btn_props.connect_clicked(move |_| {
            if tiene {
                (acc.ir_a_linea)(lin);
            } else {
                (acc.estructurar)();
            }
        });
    }
    fila_chips.append(&btn_props);

    // Chip 2: Cuerpo en bloques (Obligatoria 2)
    let etq_cuerpo = if diag.cantidad_bloques_cuerpo > 0 {
        format!("2 · Cuerpo ({}) ✓", diag.cantidad_bloques_cuerpo)
    } else {
        "2 · Cuerpo ⚠".to_owned()
    };
    let btn_cuerpo = gtk::Button::builder()
        .label(&etq_cuerpo)
        .tooltip_text(if diag.cantidad_bloques_cuerpo > 0 {
            "Parte 2 (obligatoria): Cuerpo dividido en bloques con id (clic para ir)"
        } else {
            "Parte 2 (obligatoria): El cuerpo aún no está delimitado en bloques <!-- block ... --> (clic para estructurar)"
        })
        .css_classes(if diag.cantidad_bloques_cuerpo > 0 {
            vec!["flat", "caption"]
        } else {
            vec!["suggested-action", "caption"]
        })
        .build();
    {
        let acc = acciones.clone();
        let lin = diag.linea_cuerpo;
        btn_cuerpo.connect_clicked(move |_| {
            if let Some(l) = lin {
                (acc.ir_a_linea)(l);
            } else {
                (acc.estructurar)();
            }
        });
    }

    // Chip 3: Conexiones (Obligatoria 3)
    let tiene_con = diag.conexiones.is_some();
    let etq_con = if tiene_con {
        format!("3 · Conexiones ({}) ✓", diag.enlaces_en_conexiones.len())
    } else {
        "3 · Conexiones ⚠".to_owned()
    };
    let btn_con = gtk::Button::builder()
        .label(&etq_con)
        .tooltip_text(if tiene_con {
            "Parte 3 (obligatoria): Bloque <!-- connections ID --> al pie de la nota (clic para ir)"
        } else {
            "Parte 3 (obligatoria): Falta el bloque <!-- connections ID --> al pie (clic para crearlo)"
        })
        .css_classes(if tiene_con {
            vec!["flat", "caption"]
        } else {
            vec!["suggested-action", "caption"]
        })
        .build();
    {
        let acc = acciones.clone();
        let lin = diag.conexiones.as_ref().map(|c| c.linea);
        btn_con.connect_clicked(move |_| {
            if let Some(l) = lin {
                (acc.ir_a_linea)(l);
            } else {
                (acc.estructurar)();
            }
        });
    }

    // Chip Opcional A: Síntesis
    let lin_sintesis = diag.sintesis.as_ref().map(|s| s.linea);
    let btn_sintesis = gtk::Button::builder()
        .label(if lin_sintesis.is_some() {
            "Síntesis ✓"
        } else {
            "+ Síntesis"
        })
        .tooltip_text(if lin_sintesis.is_some() {
            "Parte opcional A: Ir a la Síntesis (<!-- summary ID -->)"
        } else {
            "Parte opcional A: Insertar sección de Síntesis / Resumen arriba del cuerpo"
        })
        .sensitive(!solo_lectura || lin_sintesis.is_some())
        .css_classes(["flat", "caption"])
        .build();
    {
        let acc = acciones.clone();
        btn_sintesis.connect_clicked(move |_| {
            if let Some(l) = lin_sintesis {
                (acc.ir_a_linea)(l);
            } else {
                (acc.insertar_opcional)(ParteOpcional::Sintesis);
            }
        });
    }

    // Chip Opcional B: Fuentes
    let lin_fuentes = diag.fuentes.as_ref().map(|f| f.linea);
    let btn_fuentes = gtk::Button::builder()
        .label(if lin_fuentes.is_some() {
            "Fuentes ✓"
        } else {
            "+ Fuentes"
        })
        .tooltip_text(if lin_fuentes.is_some() {
            "Parte opcional B: Ir a las Fuentes / Bibliografía (<!-- sources ID -->)"
        } else {
            "Parte opcional B: Insertar sección de Fuentes / Bibliografía después del cuerpo, antes de las Conexiones"
        })
        .sensitive(!solo_lectura || lin_fuentes.is_some())
        .css_classes(["flat", "caption"])
        .build();
    {
        let acc = acciones.clone();
        btn_fuentes.connect_clicked(move |_| {
            if let Some(l) = lin_fuentes {
                (acc.ir_a_linea)(l);
            } else {
                (acc.insertar_opcional)(ParteOpcional::Fuentes);
            }
        });
    }
    // En el orden de la nota: Propiedades → Síntesis → Cuerpo → Fuentes → Conexiones.
    for b in [&btn_sintesis, &btn_cuerpo, &btn_fuentes, &btn_con] {
        fila_chips.append(b);
    }

    // Espaciador y botón «Estructurar en 3 partes» cuando falta alguna de las 3 obligatorias
    let espaciador = gtk::Box::builder().hexpand(true).build();
    fila_chips.append(&espaciador);

    if !diag.es_completa_tres_partes() && !solo_lectura {
        let btn_estructurar = gtk::Button::builder()
            .label("Estructurar en 3 partes")
            .tooltip_text(
                "Completa las 3 partes obligatorias (Propiedades con id, Cuerpo en bloques y Conexiones al pie)",
            )
            .css_classes(["suggested-action", "pill", "caption"])
            .build();
        let acc = acciones.clone();
        btn_estructurar.connect_clicked(move |_| (acc.estructurar)());
        fila_chips.append(&btn_estructurar);
    }

    let scroll_chips = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Automatic)
        .vscrollbar_policy(gtk::PolicyType::Never)
        .child(&fila_chips)
        .build();
    contenedor.append(&scroll_chips);

    // Si hay conexiones declaradas en la Parte 3 o retroenlaces entrantes, mostrarlos como píldoras rápidas
    if !diag.enlaces_en_conexiones.is_empty() || !retroenlaces.is_empty() {
        let fila_red = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(6)
            .build();
        let lbl_red = gtk::Label::builder()
            .label("Red:")
            .css_classes(["dim-label", "caption"])
            .build();
        fila_red.append(&lbl_red);

        for destino in diag.enlaces_en_conexiones.iter().take(6) {
            let btn_link = gtk::Button::builder()
                .label(format!("→ {destino}"))
                .tooltip_text(format!("Conexión declarada en Parte 3: [[{destino}]]"))
                .css_classes(["flat", "caption"])
                .build();
            let acc = acciones.clone();
            let dest = destino.clone();
            btn_link.connect_clicked(move |_| (acc.abrir_enlace)(&format!("[[{dest}]]")));
            fila_red.append(&btn_link);
        }

        for entrante in retroenlaces.iter().take(4) {
            let btn_in = gtk::Button::builder()
                .label(format!("← {entrante}"))
                .tooltip_text(format!("Retroenlace entrante desde [[{entrante}]]"))
                .css_classes(["flat", "caption"])
                .build();
            let acc = acciones.clone();
            let ent = entrante.clone();
            btn_in.connect_clicked(move |_| (acc.abrir_enlace)(&format!("[[{ent}]]")));
            fila_red.append(&btn_in);
        }

        let scroll_red = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Automatic)
            .vscrollbar_policy(gtk::PolicyType::Never)
            .child(&fila_red)
            .build();
        contenedor.append(&scroll_red);
    }

    contenedor.upcast()
}

#[cfg(test)]
mod tests {
    use super::*;

    const DERIVADAS_AP001: &str = "---\n\
id: AP-001\n\
type: apunte\n\
title: Derivadas de funciones reales\n\
tags:\n\
  - matematicas\n\
  - calculo\n\
  - derivadas\n\
---\n\
\n\
<!-- block 1 AP-001 -->\n\
# Derivadas de funciones reales\n\
<!---->\n\
\n\
<!-- block 1.1 AP-001 -->\n\
## 1. Concepto de derivada\n\
<!---->\n\
\n\
<!-- block 1.1.1 AP-001 -->\n\
### Tasa de variación media\n\
\n\
Para una función $f$, la tasa de variación media entre $a$ y $b$ es:\n\
\n\
$$\\frac{f(b)-f(a)}{b-a},\\qquad a\\ne b.$$\n\
<!---->\n";

    #[test]
    fn us41_diagnostico_y_estructuracion_derivadas_ap001() {
        let diag_inicial = DiagnosticoTresPartes::analizar(DERIVADAS_AP001, None);
        assert_eq!(diag_inicial.id_nota, "AP-001");
        assert!(diag_inicial.tiene_propiedades);
        assert_eq!(diag_inicial.cantidad_bloques_cuerpo, 3);
        assert!(diag_inicial.conexiones.is_none());
        assert!(!diag_inicial.es_completa_tres_partes());
        assert_eq!(diag_inicial.partes_obligatorias_completas(), 2);

        // Estructurar en 3 partes agrega <!-- connections AP-001 --> preservando todos los bloques
        let (estructurada, linea_con) = estructurar_en_tres_partes(DERIVADAS_AP001, None);
        assert!(linea_con > 10);
        let diag_3 = DiagnosticoTresPartes::analizar(&estructurada, None);
        assert!(diag_3.es_completa_tres_partes());
        assert_eq!(diag_3.partes_obligatorias_completas(), 3);
        assert_eq!(diag_3.cantidad_bloques_cuerpo, 3);
        assert!(estructurada.contains("<!-- connections AP-001 -->"));

        // Insertar partes opcionales A (Síntesis) y B (Fuentes)
        let (con_sintesis, lin_sin) =
            insertar_parte_opcional(&estructurada, ParteOpcional::Sintesis, None);
        assert!(lin_sin > 1);
        let (con_cinco, lin_fue) =
            insertar_parte_opcional(&con_sintesis, ParteOpcional::Fuentes, None);
        assert!(lin_fue > lin_sin);
        assert!(
            lin_fue
                < DiagnosticoTresPartes::analizar(&con_cinco, None)
                    .conexiones
                    .unwrap()
                    .linea
        );

        let diag_5 = DiagnosticoTresPartes::analizar(&con_cinco, None);
        assert!(diag_5.es_completa_tres_partes());
        assert!(diag_5.sintesis.is_some());
        assert!(diag_5.fuentes.is_some());
        assert_eq!(diag_5.cantidad_bloques_cuerpo, 3);

        // Orden canónico en el archivo: YAML -> summary -> block -> sources -> connections
        let pos_yaml = con_cinco.find("id: AP-001").unwrap();
        let pos_summary = con_cinco.find("<!-- summary AP-001 -->").unwrap();
        let pos_block = con_cinco.find("<!-- block 1 AP-001 -->").unwrap();
        let pos_conn = con_cinco.find("<!-- connections AP-001 -->").unwrap();
        let pos_sources = con_cinco.find("<!-- sources AP-001 -->").unwrap();
        assert!(pos_yaml < pos_summary);
        assert!(pos_summary < pos_block);
        assert!(pos_block < pos_sources);
        assert!(pos_sources < pos_conn);
        // Conexiones siempre última: después de su cierre no queda otra parte.
        assert!(!con_cinco[pos_conn..].contains("<!-- sources"));
        assert!(con_cinco.trim_end().ends_with("<!---->"));
    }

    #[test]
    fn us41_preserva_cinco_partes_al_editar_en_ramas_libres() {
        let (paso1, _) = estructurar_en_tres_partes(DERIVADAS_AP001, None);
        let (paso2, _) = insertar_parte_opcional(&paso1, ParteOpcional::Sintesis, None);
        let (paso3, _) = insertar_parte_opcional(&paso2, ParteOpcional::Fuentes, None);
        let enriquecida = paso3.replace(
            "## Conexiones\n\n- ",
            "## Conexiones\n\n- Prerrequisito: [[Límites|Límites de funciones]]\n- Continúa en: [[Integrales]]",
        );

        let mut arbol = ArbolRamas::parse(&enriquecida, None);
        let uid_raiz = arbol.raices[0].uid;
        arbol.agregar_hijo(uid_raiz, "## 2. Reglas de derivación");
        let serializado = arbol.serialize();

        let diag = DiagnosticoTresPartes::analizar(&serializado, None);
        assert!(diag.es_completa_tres_partes());
        assert!(diag.sintesis.is_some());
        assert!(diag.fuentes.is_some());
        assert_eq!(diag.cantidad_bloques_cuerpo, 4);
        assert_eq!(
            diag.enlaces_en_conexiones,
            vec!["Límites".to_owned(), "Integrales".to_owned()]
        );
    }

    /// Una nota con las Fuentes después de las Conexiones (el orden viejo) vuelve al orden
    /// canónico al estructurarla: Conexiones siempre última.
    #[test]
    fn fuentes_despues_de_conexiones_se_reordenan() {
        let vieja = format!(
            "{DERIVADAS_AP001}\n<!-- connections AP-001 -->\n- [[Límites]]\n<!---->\n\n<!-- sources AP-001 -->\n- Apostol\n<!---->\n"
        );
        let (nueva, _) = estructurar_en_tres_partes(&vieja, None);
        let fuentes = nueva.find("<!-- sources AP-001 -->").unwrap();
        let conexiones = nueva.find("<!-- connections AP-001 -->").unwrap();
        assert!(fuentes < conexiones, "{nueva}");
        assert!(nueva.contains("- Apostol") && nueva.contains("[[Límites]]"));
    }

    #[test]
    fn sc_notas_tres_partes_200() {
        let mut md = String::from("---\nid: AP-009\ntype: apunte\ntitle: Prueba 200\n---\n\n");
        md.push_str("<!-- summary AP-009 -->\nResumen general.\n<!---->\n\n");
        md.push_str("<!-- block 1 AP-009 -->\n# Raíz\n<!---->\n\n");
        for i in 1..=20 {
            md.push_str(&format!(
                "<!-- block {i}.1 AP-009 -->\n## Sección {i}\n<!---->\n\n"
            ));
            for j in 1..=9 {
                md.push_str(&format!(
                    "<!-- block {i}.1.{j} AP-009 -->\n### Subsección {i}.{j}\nTexto.\n<!---->\n\n"
                ));
            }
        }
        md.push_str("<!-- connections AP-009 -->\n- [[Nota A]]\n- [[Nota B]]\n<!---->\n");

        let mut muestras = Vec::with_capacity(25);
        for _ in 0..25 {
            let t0 = std::time::Instant::now();
            let diag = DiagnosticoTresPartes::analizar(&md, Some("AP-009"));
            assert!(diag.es_completa_tres_partes());
            assert_eq!(diag.cantidad_bloques_cuerpo, 201);
            muestras.push(t0.elapsed().as_secs_f64() * 1000.0);
        }
        muestras.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let mediana = muestras[muestras.len() / 2];
        println!("SC-001 (diagnóstico 3 partes 201 bloques) mediana = {mediana:.2} ms");
        let limite = if cfg!(debug_assertions) { 25.0 } else { 5.0 };
        assert!(
            mediana < limite,
            "El análisis de las 3 partes superó los {limite} ms: {mediana:.2} ms"
        );
    }

    #[test]
    #[ignore = "requiere servidor gráfico GTK"]
    fn us41_notas_tres_partes_gtk() {
        gtk::init().expect("GTK debe inicializar");
        let diag = DiagnosticoTresPartes::analizar(DERIVADAS_AP001, None);
        let estructurado = std::rc::Rc::new(std::cell::Cell::new(false));
        let est_clon = estructurado.clone();
        let acciones = Rc::new(AccionesTresPartes {
            ir_a_linea: Box::new(|_| {}),
            estructurar: Box::new(move || est_clon.set(true)),
            insertar_opcional: Box::new(|_| {}),
            abrir_enlace: Box::new(|_| {}),
        });
        let w = construir_barra_tres_partes(&diag, &["Límites".to_owned()], false, acciones);
        assert!(w.has_css_class("orbital-tres-partes"));
        assert!(!estructurado.get());
    }
}
