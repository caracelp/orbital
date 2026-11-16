//! Spec 040: Ramas libres (escritura y organización en ramas estilo Branch Writing / Gingko /
//! Lineage) e identificadores jerárquicos de bloques (`<!-- block <jerarquía> <ID_NOTA> --> ... <!---->`).
//!
//! Cada nota `.md` tiene una propiedad `id` en su frontmatter YAML formada por el prefijo de su
//! `type` (tabla fija de prefijos o las 2 primeras letras en mayúsculas), un guion y un número
//! correlativo de 3 dígitos (por ejemplo `AP-001` para `type: apunte`).
//! Los bloques de la nota se delimitan con `<!-- block <jerarquía> <ID_NOTA> -->` al abrir y
//! `<!---->` al cerrar (ocultos en la vista Lectura, visibles en Código).

use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::ops::Range;
use std::rc::{Rc, Weak};

use adw::prelude::*;
use gtk::glib;
use orbital_vault::bloques::BloqueConId;

/// Tabla fija de prefijos de `id` según la propiedad `type` de la nota.
const TABLA_PREFIJOS: &[(&str, &str)] = &[
    ("apunte", "AP"),
    ("atomo", "AT"),
    ("sintesis", "SI"),
    ("diario", "DI"),
    ("plantilla", "PL"),
    ("libro", "LI"),
    ("ficha", "FI"),
    ("fuente", "FU"),
    ("manuscrito", "MA"),
    ("proyecto", "PR"),
    ("concepto", "CO"),
    ("ensayo", "EN"),
];

/// Devuelve el prefijo en mayúsculas para construir el `id` (`<PREFIJO>-NNN`) a partir de `type`.
/// Si el `type` no está en la tabla fija, toma sus 2 primeras letras ASCII en mayúsculas.
pub fn prefijo_de_tipo(tipo: &str) -> String {
    let doblado = orbital_vault::fold(tipo.trim());
    for &(clave, pref) in TABLA_PREFIJOS {
        if doblado == clave {
            return pref.to_owned();
        }
    }
    let letras: String = doblado
        .chars()
        .filter(|c| c.is_ascii_alphabetic())
        .take(2)
        .map(|c| c.to_ascii_uppercase())
        .collect();
    match letras.len() {
        2 => letras,
        1 => format!("{letras}X"),
        _ => "NT".to_owned(),
    }
}

/// Calcula el siguiente identificador `<PREFIJO>-NNN` libre para `tipo`, dados los IDs existentes.
pub fn siguiente_id_nota<'a>(tipo: &str, existentes: impl IntoIterator<Item = &'a str>) -> String {
    let pref = prefijo_de_tipo(tipo);
    let usados: HashSet<String> = existentes
        .into_iter()
        .map(|s| s.trim().to_ascii_uppercase())
        .collect();
    let mut n = 1u32;
    loop {
        let cand = format!("{pref}-{n:03}");
        if !usados.contains(&cand) {
            return cand;
        }
        n += 1;
    }
}

/// Indica si una línea es un delimitador de apertura `<!-- block <jerarquía> <ID> -->` o cierre `<!---->`.
pub fn es_delimitador_bloque_comentario(linea: &str) -> bool {
    let t = linea.trim();
    t == "<!---->" || parse_apertura_bloque(t).is_some()
}

/// Analiza una línea `<!-- block <jerarquía> <ID> -->` (tolerando la falta de espacio entre
/// `<jerarquía>` e `<ID>`, como `<!-- block 1.1.2AP-001 -->`). Devuelve `(jerarquia, id_nota)`.
pub fn parse_apertura_bloque(linea: &str) -> Option<(String, String)> {
    let t = linea.trim();
    let interior = t.strip_prefix("<!--")?.strip_suffix("-->")?.trim();
    let resto = interior.strip_prefix("block")?.trim_start();
    if resto.is_empty() {
        return None;
    }
    let fin_codigo = resto
        .bytes()
        .take_while(|b| b.is_ascii_digit() || *b == b'.')
        .count();
    let codigo = resto[..fin_codigo].trim_end_matches('.');
    if codigo.is_empty() || !codigo.bytes().any(|b| b.is_ascii_digit()) {
        return None;
    }
    let id_nota = resto[fin_codigo..].trim();
    Some((codigo.to_owned(), id_nota.to_owned()))
}

/// Indica si el texto de la nota utiliza el sistema de bloques delimitados `<!-- block ... -->`.
pub fn usa_bloques_delimitados(texto: &str) -> bool {
    texto.lines().any(|l| parse_apertura_bloque(l).is_some())
}

/// Bloque delimitado hallado en el texto fuente de una nota.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BloqueDelimitado {
    /// Código jerárquico (`"1"`, `"1.1"`, `"1.1.1"`…).
    pub codigo: String,
    /// Identificador de la nota en la marca (`"AP-001"`).
    pub id_nota: String,
    /// Rango de bytes del contenido interior del bloque.
    pub rango_contenido: Range<usize>,
    /// Línea (base 0) donde empieza la marca `<!-- block ... -->`.
    pub linea: usize,
    /// Contenido interior limpio del bloque.
    pub contenido: String,
}

/// Extrae todos los bloques `<!-- block <jerarquía> <ID> --> ... <!---->` de `texto` en orden.
pub fn extraer_bloques_delimitados(texto: &str) -> Vec<BloqueDelimitado> {
    let mut out = Vec::new();
    let mut offset = 0usize;
    let mut actual: Option<(String, String, usize, usize)> = None;

    for (num_linea, l) in texto.split_inclusive('\n').enumerate() {
        let sin_eol = l.trim_end_matches(['\n', '\r']);
        let sig_offset = offset + l.len();
        if let Some((codigo, id_nota)) = parse_apertura_bloque(sin_eol) {
            if let Some((c_ant, id_ant, ini_ant, lin_ant)) = actual.take() {
                let cuerpo = texto[ini_ant..offset].trim_matches(['\n', '\r']).to_owned();
                out.push(BloqueDelimitado {
                    codigo: c_ant,
                    id_nota: id_ant,
                    rango_contenido: ini_ant..offset,
                    linea: lin_ant,
                    contenido: cuerpo,
                });
            }
            actual = Some((codigo, id_nota, sig_offset, num_linea));
        } else if crate::grafo::notas_tres_partes::parse_apertura_seccion(sin_eol).is_some()
            && let Some((c_ant, id_ant, ini_ant, lin_ant)) = actual.take()
        {
            let fin_cuerpo = offset.max(ini_ant);
            let cuerpo = texto[ini_ant..fin_cuerpo]
                .trim_matches(['\n', '\r'])
                .to_owned();
            out.push(BloqueDelimitado {
                codigo: c_ant,
                id_nota: id_ant,
                rango_contenido: ini_ant..fin_cuerpo,
                linea: lin_ant,
                contenido: cuerpo,
            });
        } else if sin_eol.trim() == "<!---->"
            && let Some((c_ant, id_ant, ini_ant, lin_ant)) = actual.take()
        {
            let fin_cuerpo = offset.max(ini_ant);
            let cuerpo = texto[ini_ant..fin_cuerpo]
                .trim_matches(['\n', '\r'])
                .to_owned();
            out.push(BloqueDelimitado {
                codigo: c_ant,
                id_nota: id_ant,
                rango_contenido: ini_ant..fin_cuerpo,
                linea: lin_ant,
                contenido: cuerpo,
            });
        }
        offset = sig_offset;
    }

    if let Some((c_ant, id_ant, ini_ant, lin_ant)) = actual.take() {
        let cuerpo = texto[ini_ant..].trim_matches(['\n', '\r']).to_owned();
        out.push(BloqueDelimitado {
            codigo: c_ant,
            id_nota: id_ant,
            rango_contenido: ini_ant..texto.len(),
            linea: lin_ant,
            contenido: cuerpo,
        });
    }
    out
}

/// Busca un bloque delimitado por su código (`"1.1.1"`) o por su clave completa (`"1.1.1 AP-001"`).
pub fn buscar_bloque_delimitado(texto: &str, id_buscado: &str) -> Option<(Range<usize>, usize)> {
    let q = id_buscado.trim();
    for b in extraer_bloques_delimitados(texto) {
        let completo = if b.id_nota.is_empty() {
            b.codigo.clone()
        } else {
            format!("{} {}", b.codigo, b.id_nota)
        };
        let sin_espacio = format!("{}{}", b.codigo, b.id_nota);
        if b.codigo == q || completo.eq_ignore_ascii_case(q) || sin_espacio.eq_ignore_ascii_case(q)
        {
            return Some((b.rango_contenido, b.linea));
        }
    }
    None
}

/// Lista los bloques delimitados de `texto` como `BloqueConId` para el autocompletado de `[[nota#^`.
pub fn listar_bloques_delimitados(texto: &str) -> Vec<BloqueConId> {
    extraer_bloques_delimitados(texto)
        .into_iter()
        .map(|b| {
            let primera = b
                .contenido
                .lines()
                .map(|l| l.trim().trim_start_matches('#').trim())
                .find(|l| !l.is_empty())
                .unwrap_or("(Bloque vacío)");
            let resumen: String = primera.chars().take(72).collect();
            BloqueConId {
                id: b.codigo,
                linea: b.linea,
                resumen,
            }
        })
        .collect()
}

/// Nodo del árbol de Ramas libres.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NodoRama {
    pub uid: usize,
    pub codigo: String,
    pub contenido: String,
    pub linea: usize,
    pub hijos: Vec<NodoRama>,
}

/// Árbol editable de Ramas libres de una nota `.md`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ArbolRamas {
    /// Líneas del frontmatter YAML (sin las líneas `---`), con `id` asegurado.
    pub frontmatter: String,
    /// Identificador de la nota (p. ej. `"AP-001"`).
    pub id_nota: String,
    /// Parte opcional A: Síntesis (`<!-- summary <ID> --> ... <!---->`).
    pub summary: Option<String>,
    /// Raíces de nivel 1.
    pub raices: Vec<NodoRama>,
    /// Parte 3 obligatoria: Conexiones (`<!-- connections <ID> --> ... <!---->`).
    pub connections: Option<String>,
    /// Parte opcional B: Fuentes (`<!-- sources <ID> --> ... <!---->`).
    pub sources: Option<String>,
    proximo_uid: usize,
}

fn separar_frontmatter(texto: &str) -> (Option<String>, &str) {
    let sin_bom = texto.strip_prefix('\u{feff}').unwrap_or(texto);
    let Some(resto) = sin_bom
        .strip_prefix("---\n")
        .or_else(|| sin_bom.strip_prefix("---\r\n"))
    else {
        return (None, texto);
    };
    let mut offset = 0usize;
    for l in resto.split_inclusive('\n') {
        let t = l.trim_end_matches(['\n', '\r']);
        if t == "---" || t == "..." {
            let fm = resto[..offset].trim_end_matches(['\n', '\r']).to_owned();
            let cuerpo = &resto[offset + l.len()..];
            return (Some(fm), cuerpo);
        }
        offset += l.len();
    }
    (None, texto)
}

fn extraer_id_y_tipo_de_frontmatter(fm: &str) -> (Option<String>, Option<String>) {
    let mut id = None;
    let mut tipo = None;
    for l in fm.lines() {
        let t = l.trim();
        if let Some(v) = t.strip_prefix("id:") {
            let val = v.trim().trim_matches(['"', '\'']);
            if !val.is_empty() {
                id = Some(val.to_owned());
            }
        } else if let Some(v) = t.strip_prefix("type:") {
            let val = v.trim().trim_matches(['"', '\'']);
            if !val.is_empty() {
                tipo = Some(val.to_owned());
            }
        }
    }
    (id, tipo)
}

fn profundidad_de_codigo(codigo: &str) -> usize {
    codigo.split('.').filter(|s| !s.is_empty()).count().max(1)
}

impl ArbolRamas {
    /// Analiza el texto completo de una nota `.md` y construye su `ArbolRamas`.
    /// Si la nota no tiene `id` en su frontmatter, usa `id_sugerido` (o lo genera desde `type`).
    /// Si la nota todavía no tenía bloques `<!-- block ... -->`, los crea a partir de sus
    /// encabezados ATX (`#`, `##`, `###`…).
    pub fn parse(texto: &str, id_sugerido: Option<&str>) -> Self {
        let (fm_opt, cuerpo) = separar_frontmatter(texto);
        let fm_raw = fm_opt.unwrap_or_default();
        let (id_fm, tipo_fm) = extraer_id_y_tipo_de_frontmatter(&fm_raw);

        let secciones_especiales =
            crate::grafo::notas_tres_partes::extraer_secciones_delimitadas(cuerpo);
        let summary = secciones_especiales
            .iter()
            .find(|s| s.tipo == crate::grafo::notas_tres_partes::TipoSeccionNota::Summary)
            .map(|s| s.contenido.clone());
        let connections = secciones_especiales
            .iter()
            .find(|s| s.tipo == crate::grafo::notas_tres_partes::TipoSeccionNota::Connections)
            .map(|s| s.contenido.clone());
        let sources = secciones_especiales
            .iter()
            .find(|s| s.tipo == crate::grafo::notas_tres_partes::TipoSeccionNota::Sources)
            .map(|s| s.contenido.clone());

        let bloques_delim = extraer_bloques_delimitados(cuerpo);
        let id_de_bloques = bloques_delim
            .iter()
            .find_map(|b| (!b.id_nota.is_empty()).then(|| b.id_nota.clone()))
            .or_else(|| {
                secciones_especiales
                    .iter()
                    .find_map(|s| (!s.id_nota.is_empty()).then(|| s.id_nota.clone()))
            });

        let id_nota = id_fm
            .clone()
            .or(id_de_bloques)
            .or_else(|| id_sugerido.map(str::to_owned))
            .unwrap_or_else(|| {
                let t = tipo_fm.as_deref().unwrap_or("atomo");
                siguiente_id_nota(t, std::iter::empty())
            });

        let frontmatter = match (id_fm.is_some(), tipo_fm.is_some()) {
            (true, true) => fm_raw,
            (true, false) => format!("{fm_raw}\ntype: atomo"),
            (false, true) => format!("id: {id_nota}\n{fm_raw}"),
            (false, false) => {
                if fm_raw.trim().is_empty() {
                    format!("id: {id_nota}\ntype: atomo")
                } else {
                    format!("id: {id_nota}\ntype: atomo\n{fm_raw}")
                }
            }
        };

        let mut proximo_uid = 1usize;
        let mut raices: Vec<NodoRama> = Vec::new();

        if !bloques_delim.is_empty() {
            // Reconstruir árbol según la profundidad del código (`1` -> prof 1, `1.1`/`2.1` -> prof 2,
            // `1.1.1` -> prof 3, etc.).
            let mut pila_ruta: Vec<(usize, usize)> = Vec::new();
            for b in bloques_delim {
                let prof = profundidad_de_codigo(&b.codigo);
                while pila_ruta.last().is_some_and(|&(p, _)| p >= prof) {
                    pila_ruta.pop();
                }
                let uid = proximo_uid;
                proximo_uid += 1;
                let nodo = NodoRama {
                    uid,
                    codigo: b.codigo,
                    contenido: b.contenido,
                    linea: b.linea,
                    hijos: Vec::new(),
                };
                let indices: Vec<usize> = pila_ruta.iter().map(|&(_, idx)| idx).collect();
                let nuevo_idx = insertar_en_bosque(&mut raices, &indices, nodo);
                pila_ruta.push((prof, nuevo_idx));
            }
        } else {
            // La nota todavía no tenía `<!-- block ... -->`: partir por encabezados ATX (`#`..`######`).
            let cuerpo_sin_especiales =
                crate::grafo::notas_tres_partes::quitar_secciones_delimitadas(cuerpo);
            let secciones = partir_por_encabezados(&cuerpo_sin_especiales);
            let mut pila_ruta: Vec<(usize, usize)> = Vec::new();
            for (nivel, lin, cont) in secciones {
                while pila_ruta.last().is_some_and(|&(p, _)| p >= nivel) {
                    pila_ruta.pop();
                }
                let uid = proximo_uid;
                proximo_uid += 1;
                let nodo = NodoRama {
                    uid,
                    codigo: String::new(),
                    contenido: cont,
                    linea: lin,
                    hijos: Vec::new(),
                };
                let indices: Vec<usize> = pila_ruta.iter().map(|&(_, idx)| idx).collect();
                let nuevo_idx = insertar_en_bosque(&mut raices, &indices, nodo);
                pila_ruta.push((nivel, nuevo_idx));
            }
        }

        if raices.is_empty() {
            raices.push(NodoRama {
                uid: proximo_uid,
                codigo: "1".into(),
                contenido: "# Nueva nota".into(),
                linea: 0,
                hijos: Vec::new(),
            });
            proximo_uid += 1;
        }

        let mut arbol = Self {
            frontmatter,
            id_nota,
            summary,
            raices,
            connections,
            sources,
            proximo_uid,
        };
        arbol.renumerar();
        arbol
    }

    /// Renumera todos los nodos siguiendo la convención de `Derivadas_AP001.md`:
    /// - Raíces (nivel 1): `"1"` (o `"1"`, `"2"`… si hubiera más de una raíz).
    /// - Nivel 2 (bajo la primera raíz `"1"`): `"{k}.1"` (`"1.1"`, `"2.1"`, `"3.1"`…).
    /// - Nivel $\ge 3$: `"{codigo_padre}.{m}"` (`"1.1.1"`, `"1.1.2"`, `"2.1.1"`…).
    pub fn renumerar(&mut self) {
        let unica_raiz = self.raices.len() == 1;
        for (i, r) in self.raices.iter_mut().enumerate() {
            let cod_raiz = (i + 1).to_string();
            r.codigo = cod_raiz.clone();
            for (j, h2) in r.hijos.iter_mut().enumerate() {
                let cod_h2 = if unica_raiz {
                    format!("{}.1", j + 1)
                } else {
                    format!("{cod_raiz}.{}", j + 1)
                };
                renumerar_subarbol(h2, cod_h2);
            }
        }
    }

    /// Serializa el árbol completo de vuelta al formato Markdown con frontmatter y bloques
    /// `<!-- block <jerarquía> <ID_NOTA> --> ... <!---->`.
    pub fn serialize(&self) -> String {
        let mut out = String::with_capacity(512);
        if !self.frontmatter.trim().is_empty() {
            out.push_str("---\n");
            out.push_str(self.frontmatter.trim_end());
            out.push_str("\n---\n\n");
        }
        if let Some(s) = &self.summary {
            out.push_str(&format!(
                "<!-- summary {} -->\n{}\n<!---->\n\n",
                self.id_nota,
                s.trim_matches(['\n', '\r'])
            ));
        }
        let mut primero = true;
        for r in &self.raices {
            serializar_nodo(r, &self.id_nota, &mut out, &mut primero);
        }
        if let Some(c) = &self.connections {
            if !out.ends_with("\n\n") {
                out.push('\n');
            }
            out.push_str(&format!(
                "<!-- connections {} -->\n{}\n<!---->\n",
                self.id_nota,
                c.trim_matches(['\n', '\r'])
            ));
        }
        if let Some(src) = &self.sources {
            if !out.ends_with("\n\n") {
                out.push('\n');
            }
            out.push_str(&format!(
                "<!-- sources {} -->\n{}\n<!---->\n",
                self.id_nota,
                src.trim_matches(['\n', '\r'])
            ));
        }
        out
    }

    /// Cantidad máxima de columnas (profundidad del árbol).
    pub fn profundidad_maxima(&self) -> usize {
        fn prof(n: &NodoRama) -> usize {
            1 + n.hijos.iter().map(prof).max().unwrap_or(0)
        }
        self.raices.iter().map(prof).max().unwrap_or(1)
    }

    /// Todos los nodos del nivel `nivel` (0 = raíces, 1 = segundo nivel, etc.) con el UID de su padre.
    pub fn nodos_en_nivel(&self, nivel: usize) -> Vec<(Option<usize>, NodoRama)> {
        let mut out = Vec::new();
        for r in &self.raices {
            recolectar_nivel(r, None, 0, nivel, &mut out);
        }
        out
    }

    /// Actualiza el contenido textual del bloque con `uid`.
    pub fn actualizar_contenido(&mut self, uid: usize, nuevo: String) -> bool {
        for r in &mut self.raices {
            if actualizar_en_nodo(r, uid, &nuevo) {
                return true;
            }
        }
        false
    }

    /// Agrega un bloque hermano inmediatamente debajo de `uid` y devuelve el `uid` nuevo.
    pub fn agregar_hermano(&mut self, uid: usize, contenido_inicial: &str) -> Option<usize> {
        let nuevo_uid = self.proximo_uid;
        let nuevo = NodoRama {
            uid: nuevo_uid,
            codigo: String::new(),
            contenido: contenido_inicial.to_owned(),
            linea: 0,
            hijos: Vec::new(),
        };
        if let Some(pos) = self.raices.iter().position(|r| r.uid == uid) {
            self.proximo_uid += 1;
            self.raices.insert(pos + 1, nuevo);
            self.renumerar();
            return Some(nuevo_uid);
        }
        for r in &mut self.raices {
            if insertar_hermano_en(r, uid, &nuevo) {
                self.proximo_uid += 1;
                self.renumerar();
                return Some(nuevo_uid);
            }
        }
        None
    }

    /// Agrega una subrama hija al final de `uid` y devuelve el `uid` nuevo.
    pub fn agregar_hijo(&mut self, uid: usize, contenido_inicial: &str) -> Option<usize> {
        let nuevo_uid = self.proximo_uid;
        let nuevo = NodoRama {
            uid: nuevo_uid,
            codigo: String::new(),
            contenido: contenido_inicial.to_owned(),
            linea: 0,
            hijos: Vec::new(),
        };
        for r in &mut self.raices {
            if insertar_hijo_en(r, uid, &nuevo) {
                self.proximo_uid += 1;
                self.renumerar();
                return Some(nuevo_uid);
            }
        }
        None
    }

    /// Mueve el nodo `uid` una posición hacia arriba (`delta = -1`) o abajo (`delta = 1`) entre sus hermanos.
    pub fn mover_hermano(&mut self, uid: usize, hacia_arriba: bool) -> bool {
        if mover_en_lista(&mut self.raices, uid, hacia_arriba) {
            self.renumerar();
            return true;
        }
        for r in &mut self.raices {
            if mover_en_subarbol(r, uid, hacia_arriba) {
                self.renumerar();
                return true;
            }
        }
        false
    }

    /// Elimina el nodo `uid` y todas sus subramas (salvo que sea el único nodo raíz).
    pub fn eliminar_nodo(&mut self, uid: usize) -> bool {
        if self.raices.len() == 1 && self.raices[0].uid == uid {
            return false;
        }
        let len_antes = self.raices.len();
        self.raices.retain(|r| r.uid != uid);
        if self.raices.len() != len_antes {
            self.renumerar();
            return true;
        }
        for r in &mut self.raices {
            if eliminar_en_subarbol(r, uid) {
                self.renumerar();
                return true;
            }
        }
        false
    }
}

fn insertar_en_bosque(raices: &mut Vec<NodoRama>, ruta: &[usize], nodo: NodoRama) -> usize {
    if ruta.is_empty() {
        raices.push(nodo);
        return raices.len() - 1;
    }
    let mut actual = &mut raices[ruta[0]];
    for &idx in &ruta[1..] {
        actual = &mut actual.hijos[idx];
    }
    actual.hijos.push(nodo);
    actual.hijos.len() - 1
}

fn renumerar_subarbol(nodo: &mut NodoRama, codigo: String) {
    nodo.codigo = codigo.clone();
    for (m, h) in nodo.hijos.iter_mut().enumerate() {
        renumerar_subarbol(h, format!("{codigo}.{}", m + 1));
    }
}

fn serializar_nodo(nodo: &NodoRama, id_nota: &str, out: &mut String, primero: &mut bool) {
    if !*primero {
        out.push('\n');
    }
    *primero = false;
    out.push_str(&format!("<!-- block {} {} -->\n", nodo.codigo, id_nota));
    out.push_str(nodo.contenido.trim_matches(['\n', '\r']));
    out.push_str("\n<!---->\n");
    for h in &nodo.hijos {
        serializar_nodo(h, id_nota, out, primero);
    }
}

fn recolectar_nivel(
    nodo: &NodoRama,
    padre_uid: Option<usize>,
    actual: usize,
    objetivo: usize,
    out: &mut Vec<(Option<usize>, NodoRama)>,
) {
    if actual == objetivo {
        out.push((padre_uid, nodo.clone()));
        return;
    }
    for h in &nodo.hijos {
        recolectar_nivel(h, Some(nodo.uid), actual + 1, objetivo, out);
    }
}

fn actualizar_en_nodo(nodo: &mut NodoRama, uid: usize, nuevo: &str) -> bool {
    if nodo.uid == uid {
        nodo.contenido = nuevo.to_owned();
        return true;
    }
    for h in &mut nodo.hijos {
        if actualizar_en_nodo(h, uid, nuevo) {
            return true;
        }
    }
    false
}

fn insertar_hermano_en(nodo: &mut NodoRama, uid: usize, nuevo: &NodoRama) -> bool {
    if let Some(pos) = nodo.hijos.iter().position(|h| h.uid == uid) {
        nodo.hijos.insert(pos + 1, nuevo.clone());
        return true;
    }
    for h in &mut nodo.hijos {
        if insertar_hermano_en(h, uid, nuevo) {
            return true;
        }
    }
    false
}

fn insertar_hijo_en(nodo: &mut NodoRama, uid: usize, nuevo: &NodoRama) -> bool {
    if nodo.uid == uid {
        nodo.hijos.push(nuevo.clone());
        return true;
    }
    for h in &mut nodo.hijos {
        if insertar_hijo_en(h, uid, nuevo) {
            return true;
        }
    }
    false
}

fn mover_en_lista(lista: &mut [NodoRama], uid: usize, hacia_arriba: bool) -> bool {
    let Some(pos) = lista.iter().position(|n| n.uid == uid) else {
        return false;
    };
    if hacia_arriba {
        if pos == 0 {
            return false;
        }
        lista.swap(pos - 1, pos);
        true
    } else {
        if pos + 1 >= lista.len() {
            return false;
        }
        lista.swap(pos, pos + 1);
        true
    }
}

fn mover_en_subarbol(nodo: &mut NodoRama, uid: usize, hacia_arriba: bool) -> bool {
    if mover_en_lista(&mut nodo.hijos, uid, hacia_arriba) {
        return true;
    }
    for h in &mut nodo.hijos {
        if mover_en_subarbol(h, uid, hacia_arriba) {
            return true;
        }
    }
    false
}

fn eliminar_en_subarbol(nodo: &mut NodoRama, uid: usize) -> bool {
    let antes = nodo.hijos.len();
    nodo.hijos.retain(|h| h.uid != uid);
    if nodo.hijos.len() != antes {
        return true;
    }
    for h in &mut nodo.hijos {
        if eliminar_en_subarbol(h, uid) {
            return true;
        }
    }
    false
}

fn partir_por_encabezados(cuerpo: &str) -> Vec<(usize, usize, String)> {
    let mut out: Vec<(usize, usize, String)> = Vec::new();
    let mut actual_nivel = 1usize;
    let mut actual_linea = 0usize;
    let mut actual_lineas: Vec<&str> = Vec::new();
    let mut en_cerca = false;

    for (idx, l) in cuerpo.lines().enumerate() {
        let trim = l.trim_start();
        if trim.starts_with("```") || trim.starts_with("~~~") {
            en_cerca = !en_cerca;
        }
        if !en_cerca && trim.starts_with('#') {
            let gatos = trim.bytes().take_while(|&b| b == b'#').count();
            if (1..=6).contains(&gatos) && trim[gatos..].starts_with(' ') {
                let acumulado = actual_lineas.join("\n").trim().to_owned();
                if !acumulado.is_empty() {
                    out.push((actual_nivel, actual_linea, acumulado));
                }
                actual_nivel = gatos;
                actual_linea = idx;
                actual_lineas.clear();
            }
        }
        actual_lineas.push(l);
    }
    let acumulado = actual_lineas.join("\n").trim().to_owned();
    if !acumulado.is_empty() {
        out.push((actual_nivel, actual_linea, acumulado));
    }
    out
}

type SincronizarFn = Box<dyn Fn(String)>;
type CerrarRamasFn = Box<dyn Fn()>;

struct Inner {
    root: gtk::Box,
    titulo_barra: adw::WindowTitle,
    chip_id: gtk::Label,
    contenedor_columnas: gtk::Box,
    arbol: RefCell<ArbolRamas>,
    uid_activo: Cell<Option<usize>>,
    sincronizando_ui: Cell<bool>,
    al_sincronizar: RefCell<Option<SincronizarFn>>,
    al_cerrar: RefCell<Option<CerrarRamasFn>>,
}

#[derive(Clone)]
pub struct VistaRamasLibres(Rc<Inner>);

thread_local! {
    static INSTANCIAS: RefCell<Vec<(gtk::Box, Weak<Inner>)>> = const { RefCell::new(Vec::new()) };
}

impl VistaRamasLibres {
    pub fn new() -> Self {
        let titulo_barra = adw::WindowTitle::new("Ramas libres (escritura por bloques)", "");
        let chip_id = gtk::Label::builder()
            .label("ID: —")
            .css_classes(["orbital-tag-chip"])
            .build();
        chip_id.update_property(&[gtk::accessible::Property::Label(
            "Identificador de la nota abierta",
        )]);

        let b_cerrar = gtk::Button::builder()
            .icon_name("window-close-symbolic")
            .tooltip_text("Cerrar ramas libres y volver al editor (Esc)")
            .css_classes(["flat"])
            .build();
        b_cerrar.update_property(&[gtk::accessible::Property::Label("Cerrar ramas libres")]);

        let barra = adw::HeaderBar::new();
        barra.set_title_widget(Some(&titulo_barra));
        barra.pack_start(&chip_id);
        barra.pack_end(&b_cerrar);

        let contenedor_columnas = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(16)
            .margin_top(16)
            .margin_bottom(16)
            .margin_start(16)
            .margin_end(16)
            .vexpand(true)
            .build();
        contenedor_columnas.update_property(&[gtk::accessible::Property::Label(
            "Columnas jerárquicas de ramas de la nota",
        )]);

        let scroll_horizontal = gtk::ScrolledWindow::builder()
            .vscrollbar_policy(gtk::PolicyType::Never)
            .hscrollbar_policy(gtk::PolicyType::Automatic)
            .vexpand(true)
            .hexpand(true)
            .child(&contenedor_columnas)
            .build();

        let root = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .vexpand(true)
            .hexpand(true)
            .build();
        root.append(&barra);
        root.append(&scroll_horizontal);

        let this = Self(Rc::new(Inner {
            root: root.clone(),
            titulo_barra,
            chip_id,
            contenedor_columnas,
            arbol: RefCell::new(ArbolRamas::parse("", Some("AT-001"))),
            uid_activo: Cell::new(None),
            sincronizando_ui: Cell::new(false),
            al_sincronizar: RefCell::new(None),
            al_cerrar: RefCell::new(None),
        }));

        INSTANCIAS.with(|inst| {
            inst.borrow_mut().retain(|(_, w)| w.strong_count() > 0);
            inst.borrow_mut().push((root, Rc::downgrade(&this.0)));
        });

        {
            let weak = Rc::downgrade(&this.0);
            b_cerrar.connect_clicked(move |_| {
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
                    && let Some(f) = i.al_cerrar.borrow().as_ref()
                {
                    f();
                    return glib::Propagation::Stop;
                }
                glib::Propagation::Proceed
            });
            this.0.root.add_controller(teclas);
        }

        this
    }

    pub fn desde_stack(stack: &gtk::Stack) -> Option<Self> {
        let child = stack.child_by_name("ramas-libres")?;
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

    pub fn connect_sincronizar(&self, f: impl Fn(String) + 'static) {
        *self.0.al_sincronizar.borrow_mut() = Some(Box::new(f));
    }

    pub fn connect_cerrar(&self, f: impl Fn() + 'static) {
        *self.0.al_cerrar.borrow_mut() = Some(Box::new(f));
    }

    /// Carga una nota en el editor de Ramas libres. Si el texto ganó `id` o delimitadores
    /// `<!-- block ... -->`, sincroniza con el editor.
    pub fn cargar_nota(&self, ruta: &str, texto: &str, id_sugerido: Option<&str>) {
        let i = &self.0;
        let arbol = ArbolRamas::parse(texto, id_sugerido);
        i.titulo_barra.set_subtitle(ruta);
        i.chip_id.set_label(&format!("id: {}", arbol.id_nota));
        let primer_uid = arbol.raices.first().map(|r| r.uid);
        i.uid_activo.set(primer_uid);
        *i.arbol.borrow_mut() = arbol;
        self.reconstruir_columnas();
    }

    #[cfg(test)]
    pub fn arbol(&self) -> ArbolRamas {
        self.0.arbol.borrow().clone()
    }

    fn emitir_sincronizacion(&self) {
        let md = self.0.arbol.borrow().serialize();
        if let Some(f) = self.0.al_sincronizar.borrow().as_ref() {
            f(md);
        }
    }

    pub fn reconstruir_columnas(&self) {
        let i = &self.0;
        i.sincronizando_ui.set(true);
        while let Some(c) = i.contenedor_columnas.first_child() {
            i.contenedor_columnas.remove(&c);
        }
        let prof_max = i.arbol.borrow().profundidad_maxima();
        let id_nota = i.arbol.borrow().id_nota.clone();

        for nivel in 0..prof_max {
            let nodos = i.arbol.borrow().nodos_en_nivel(nivel);
            let col_titulo = gtk::Label::builder()
                .label(format!("NIVEL {} ({})", nivel + 1, nodos.len()))
                .xalign(0.0)
                .css_classes(["orbital-right-heading"])
                .build();

            let lista_tarjetas = gtk::Box::builder()
                .orientation(gtk::Orientation::Vertical)
                .spacing(12)
                .build();

            for (_padre_uid, nodo) in nodos {
                let tarjeta = self.construir_tarjeta(&nodo, &id_nota, nivel);
                lista_tarjetas.append(&tarjeta);
            }

            let scroller_col = gtk::ScrolledWindow::builder()
                .hscrollbar_policy(gtk::PolicyType::Never)
                .vscrollbar_policy(gtk::PolicyType::Automatic)
                .vexpand(true)
                .width_request(330)
                .child(&lista_tarjetas)
                .build();

            let col_box = gtk::Box::builder()
                .orientation(gtk::Orientation::Vertical)
                .spacing(10)
                .width_request(330)
                .vexpand(true)
                .css_classes(["orbital-right-block"])
                .build();
            col_box.update_property(&[gtk::accessible::Property::Label(&format!(
                "Columna de ramas de nivel {}",
                nivel + 1
            ))]);
            col_box.append(&col_titulo);
            col_box.append(&scroller_col);
            i.contenedor_columnas.append(&col_box);
        }
        i.sincronizando_ui.set(false);
    }

    fn construir_tarjeta(&self, nodo: &NodoRama, id_nota: &str, nivel: usize) -> gtk::Box {
        let uid = nodo.uid;
        let codigo = nodo.codigo.clone();
        let cab = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(4)
            .build();
        let badge = gtk::Label::builder()
            .label(format!("block {codigo} {id_nota}"))
            .xalign(0.0)
            .hexpand(true)
            .css_classes(["orbital-tag-chip"])
            .build();

        let b_subir = gtk::Button::builder()
            .icon_name("go-up-symbolic")
            .tooltip_text("Subir esta rama entre sus hermanas")
            .css_classes(["flat", "orbital-iconbtn"])
            .build();
        b_subir.update_property(&[gtk::accessible::Property::Label(&format!(
            "Subir bloque {codigo}"
        ))]);

        let b_bajar = gtk::Button::builder()
            .icon_name("go-down-symbolic")
            .tooltip_text("Bajar esta rama entre sus hermanas")
            .css_classes(["flat", "orbital-iconbtn"])
            .build();
        b_bajar.update_property(&[gtk::accessible::Property::Label(&format!(
            "Bajar bloque {codigo}"
        ))]);

        let b_hermano = gtk::Button::builder()
            .label("+ Abajo")
            .tooltip_text("Agregar un bloque hermano debajo")
            .css_classes(["flat", "caption"])
            .build();
        b_hermano.update_property(&[gtk::accessible::Property::Label(&format!(
            "Agregar bloque hermano debajo de {codigo}"
        ))]);

        let b_hijo = gtk::Button::builder()
            .label("+ Rama →")
            .tooltip_text("Ramificar hacia la derecha (crear subrama hija)")
            .css_classes(["flat", "caption"])
            .build();
        b_hijo.update_property(&[gtk::accessible::Property::Label(&format!(
            "Crear subrama hija de {codigo}"
        ))]);

        let b_borrar = gtk::Button::builder()
            .icon_name("user-trash-symbolic")
            .tooltip_text("Eliminar este bloque y sus subramas")
            .css_classes(["flat", "orbital-iconbtn"])
            .build();
        b_borrar.update_property(&[gtk::accessible::Property::Label(&format!(
            "Eliminar bloque {codigo}"
        ))]);

        cab.append(&badge);
        cab.append(&b_subir);
        cab.append(&b_bajar);
        cab.append(&b_hermano);
        cab.append(&b_hijo);
        cab.append(&b_borrar);

        let tv = gtk::TextView::builder()
            .wrap_mode(gtk::WrapMode::WordChar)
            .top_margin(8)
            .bottom_margin(8)
            .left_margin(8)
            .right_margin(8)
            .build();
        tv.buffer().set_text(&nodo.contenido);
        tv.update_property(&[gtk::accessible::Property::Label(&format!(
            "Contenido del bloque {codigo} {id_nota}"
        ))]);
        let tv_scroll = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .min_content_height(84)
            .max_content_height(190)
            .propagate_natural_height(true)
            .child(&tv)
            .build();

        let tarjeta = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(6)
            .css_classes(["orbital-context-card"])
            .build();
        tarjeta.append(&cab);
        tarjeta.append(&tv_scroll);

        {
            let weak = Rc::downgrade(&self.0);
            tv.buffer().connect_changed(move |buf| {
                let Some(i) = weak.upgrade() else { return };
                if i.sincronizando_ui.get() {
                    return;
                }
                let txt = buf
                    .text(&buf.start_iter(), &buf.end_iter(), false)
                    .to_string();
                if i.arbol.borrow_mut().actualizar_contenido(uid, txt) {
                    Self(i).emitir_sincronizacion();
                }
            });
        }
        {
            let weak = Rc::downgrade(&self.0);
            b_subir.connect_clicked(move |_| {
                if let Some(i) = weak.upgrade() {
                    let s = Self(i);
                    if s.0.arbol.borrow_mut().mover_hermano(uid, true) {
                        s.reconstruir_columnas();
                        s.emitir_sincronizacion();
                    }
                }
            });
        }
        {
            let weak = Rc::downgrade(&self.0);
            b_bajar.connect_clicked(move |_| {
                if let Some(i) = weak.upgrade() {
                    let s = Self(i);
                    if s.0.arbol.borrow_mut().mover_hermano(uid, false) {
                        s.reconstruir_columnas();
                        s.emitir_sincronizacion();
                    }
                }
            });
        }
        {
            let weak = Rc::downgrade(&self.0);
            let gatos = "#".repeat((nivel + 1).clamp(1, 6));
            b_hermano.connect_clicked(move |_| {
                if let Some(i) = weak.upgrade() {
                    let s = Self(i);
                    let plantilla = format!("{gatos} Nuevo bloque");
                    if s.0
                        .arbol
                        .borrow_mut()
                        .agregar_hermano(uid, &plantilla)
                        .is_some()
                    {
                        s.reconstruir_columnas();
                        s.emitir_sincronizacion();
                    }
                }
            });
        }
        {
            let weak = Rc::downgrade(&self.0);
            let gatos = "#".repeat((nivel + 2).clamp(1, 6));
            b_hijo.connect_clicked(move |_| {
                if let Some(i) = weak.upgrade() {
                    let s = Self(i);
                    let plantilla = format!("{gatos} Nueva subrama");
                    if s.0
                        .arbol
                        .borrow_mut()
                        .agregar_hijo(uid, &plantilla)
                        .is_some()
                    {
                        s.reconstruir_columnas();
                        s.emitir_sincronizacion();
                    }
                }
            });
        }
        {
            let weak = Rc::downgrade(&self.0);
            b_borrar.connect_clicked(move |_| {
                if let Some(i) = weak.upgrade() {
                    let s = Self(i);
                    if s.0.arbol.borrow_mut().eliminar_nodo(uid) {
                        s.reconstruir_columnas();
                        s.emitir_sincronizacion();
                    }
                }
            });
        }

        tarjeta
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DERIVADAS_AP001: &str = "\
---
id: AP-001
type: apunte
title: Derivadas de funciones reales
tags:
  - matematicas
  - calculo
  - derivadas
---

<!-- block 1 AP-001 -->
# Derivadas de funciones reales
<!---->

<!-- block 1.1 AP-001 -->
## 1. Concepto de derivada
<!---->


<!-- block 1.1.1 AP-001 -->
### Tasa de variación media

Para una función $f$, la tasa de variación media entre $a$ y $b$ es:

$$\\frac{f(b)-f(a)}{b-a},\\qquad a\\ne b.$$

Representa la pendiente de la recta secante que pasa por $(a,f(a))$ y $(b,f(b))$.
<!---->

<!-- block 1.1.2AP-001 -->
### Definición de derivada

La derivada de $f$ en $a$ es el límite, si existe y es finito:

$$f'(a)=\\lim_{h\\to 0}\\frac{f(a+h)-f(a)}{h}.$$

Geométricamente, $f'(a)$ es la pendiente de la recta tangente a la gráfica en $x=a$.
<!---->

<!-- block 1.1.3 AP-001 -->
### Derivabilidad y continuidad

Si una función es derivable en $a$, entonces es continua en $a$. La recíproca es falsa: $f(x)=|x|$ es continua en $0$, pero no es derivable allí.
<!---->

<!-- block 2.1 AP-001 -->
## 2. Reglas de derivación
<!---->

<!-- block 2.1.1 AP-001 -->
### Regla de la potencia

Para $n$ entero positivo, si $f(x)=x^n$, entonces:

$$f'(x)=nx^{n-1}.$$

Por ejemplo, la derivada de $x^4$ es $4x^3$.
<!---->
";

    /// T002/T005: tabla fija de prefijos por tipo y respaldo de 2 primeras letras en mayúsculas.
    #[test]
    fn us40_prefijos_y_siguiente_id() {
        assert_eq!(prefijo_de_tipo("apunte"), "AP");
        assert_eq!(prefijo_de_tipo("atomo"), "AT");
        assert_eq!(prefijo_de_tipo("diario"), "DI");
        // Tipo fuera de la tabla fija -> 2 primeras letras en mayúsculas.
        assert_eq!(prefijo_de_tipo("teorema"), "TE");
        assert_eq!(prefijo_de_tipo("resumen"), "RE");

        let id1 = siguiente_id_nota("apunte", ["AP-001", "AP-002"]);
        assert_eq!(id1, "AP-003");
    }

    /// T002/T005 (SC-002): parsea el formato real de Derivadas_AP001.md (incluido `1.1.2AP-001`),
    /// reconstruye los 3 niveles de ramas y renumera al mover o agregar ramas.
    #[test]
    fn us40_parse_y_edicion_derivadas_ap001() {
        let mut arbol = ArbolRamas::parse(DERIVADAS_AP001, None);
        assert_eq!(arbol.id_nota, "AP-001");
        assert_eq!(arbol.profundidad_maxima(), 3);
        assert_eq!(arbol.raices.len(), 1);
        let r = &arbol.raices[0];
        assert_eq!(r.codigo, "1");
        assert_eq!(r.hijos.len(), 2); // 1.1 y 2.1 en el extracto
        assert_eq!(r.hijos[0].codigo, "1.1");
        assert_eq!(r.hijos[0].hijos.len(), 3);
        assert_eq!(r.hijos[0].hijos[0].codigo, "1.1.1");
        assert_eq!(r.hijos[0].hijos[1].codigo, "1.1.2");
        assert_eq!(r.hijos[0].hijos[2].codigo, "1.1.3");
        assert_eq!(r.hijos[1].codigo, "2.1");
        assert_eq!(r.hijos[1].hijos[0].codigo, "2.1.1");

        // Mover la rama 2.1 arriba de 1.1 renumera 2.1 -> 1.1 y sus hijos 2.1.1 -> 1.1.1.
        let uid_2_1 = r.hijos[1].uid;
        assert!(arbol.mover_hermano(uid_2_1, true));
        assert_eq!(arbol.raices[0].hijos[0].codigo, "1.1");
        assert!(
            arbol.raices[0].hijos[0]
                .contenido
                .contains("Reglas de derivación")
        );
        assert_eq!(arbol.raices[0].hijos[0].hijos[0].codigo, "1.1.1");
        assert_eq!(arbol.raices[0].hijos[1].codigo, "2.1");
        assert_eq!(arbol.raices[0].hijos[1].hijos.len(), 3);
        assert_eq!(arbol.raices[0].hijos[1].hijos[0].codigo, "2.1.1");

        // Serializar y volver a parsear conserva todo el árbol y corrige `1.1.2AP-001` a `<!-- block ... AP-001 -->`.
        let emitido = arbol.serialize();
        assert!(emitido.contains("<!-- block 1 AP-001 -->"));
        assert!(emitido.contains("<!-- block 2.1.2 AP-001 -->"));
        let re_leido = ArbolRamas::parse(&emitido, None);
        assert_eq!(re_leido.id_nota, "AP-001");
        assert_eq!(re_leido.raices[0].hijos.len(), 2);

        // Búsqueda de bloque por código jerárquico.
        assert!(buscar_bloque_delimitado(&emitido, "2.1.2").is_some());
        assert_eq!(listar_bloques_delimitados(&emitido).len(), 7);
    }

    /// SC-001 (040): mide parse, renumeración y serialize con 200 bloques jerárquicos.
    #[test]
    fn sc_ramas_libres_200() {
        let mut md = String::from(
            "---\nid: AP-001\ntype: apunte\n---\n\n<!-- block 1 AP-001 -->\n# Raíz\n<!---->\n",
        );
        for i in 1..=20 {
            md.push_str(&format!(
                "\n<!-- block {i}.1 AP-001 -->\n## Sección {i}\n<!---->\n"
            ));
            for j in 1..=9 {
                md.push_str(&format!(
                    "\n<!-- block {i}.1.{j} AP-001 -->\n### Subrama {i}.{j}\n\nTexto del bloque {i}.{j}.\n<!---->\n"
                ));
            }
        }
        let t0 = std::time::Instant::now();
        let mut arbol = ArbolRamas::parse(&md, None);
        arbol.renumerar();
        let s = arbol.serialize();
        let ms = t0.elapsed().as_secs_f64() * 1000.0;
        assert!(s.contains("<!-- block 20.1.9 AP-001 -->"));
        println!("RAMAS LIBRES 201 bloques (parse + renumerar + serialize): {ms:.2} ms");
        if !cfg!(debug_assertions) {
            assert!(
                ms < 5.0,
                "parse + renumerar + serialize en release debe ser < 5 ms: {ms:.2} ms"
            );
        }
    }

    /// T005 (040): prueba GTK en proceso de VistaRamasLibres.
    #[test]
    #[ignore = "requiere acceso GTK; tools/pruebas-gtk.sh"]
    fn us40_vista_ramas_libres_gtk() {
        gtk::init().expect("se necesita acceso GTK");
        let vista = VistaRamasLibres::new();
        let sincronizados: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(Vec::new()));
        {
            let s = sincronizados.clone();
            vista.connect_sincronizar(move |md| s.borrow_mut().push(md));
        }
        vista.cargar_nota("wiki/Derivadas_AP001.md", DERIVADAS_AP001, Some("AP-001"));
        let arbol = vista.arbol();
        assert_eq!(arbol.id_nota, "AP-001");
        assert_eq!(arbol.profundidad_maxima(), 3);
    }
}
