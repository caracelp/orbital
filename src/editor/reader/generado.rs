//! Contenido generado de la vista formateada (005): índice por etiqueta, tabla de contenidos,
//! consultas vivas (048) y la preparación del documento (`Prepared`) con marcadores y transclusiones resueltos.

use super::*;

/// Valores por defecto de la tabla de contenidos (035), los de Preferencias. Se leen en el hilo
/// del análisis; sin nada guardado, los de fábrica (no se arma nada al arrancar).
static TOC_DEFAULTS: std::sync::RwLock<Option<orbital_vault::toc::TocDefaults>> =
    std::sync::RwLock::new(None);

/// Los valores por defecto vigentes de la tabla de contenidos.
pub fn toc_defaults() -> orbital_vault::toc::TocDefaults {
    TOC_DEFAULTS
        .read()
        .ok()
        .and_then(|d| d.clone())
        .unwrap_or_default()
}

/// Cambia los valores por defecto de la tabla de contenidos (Preferencias y el arranque).
pub fn set_toc_defaults(d: orbital_vault::toc::TocDefaults) {
    if let Ok(mut g) = TOC_DEFAULTS.write() {
        *g = (d != orbital_vault::toc::TocDefaults::default()).then_some(d);
    }
}

/// Completa los codeblocks generados del doc (005, T015 y T019): `toc` con los encabezados del
/// propio doc, `indice` con las notas de cada tag desde el índice y `consulta` (048) con su
/// resultado (`raiz`, la carpeta de la bóveda, la usa la vista de coincidencias). Reemplaza cada bloque
/// manteniendo su rango fuente (el codeblock completo) y actualiza su hash, así la actualización
/// en el sitio ve el contenido nuevo aunque el fuente no cambie. Devuelve true si completó algo.
pub fn complete_generated(
    doc: &mut RenderDoc,
    hashes: &mut [u64],
    index: Option<&orbital_vault::SearchIndex>,
    raiz: Option<&Path>,
    path: Option<&orbital_vault::RelPath>,
) -> bool {
    let encabezados = headings_flat(&doc.blocks);
    let mut completado = false;
    // Los valores por defecto se leen una vez, y solo si hay una tabla de contenidos.
    let mut defaults: Option<orbital_vault::toc::TocDefaults> = None;
    for (n, block) in doc.blocks.iter_mut().enumerate() {
        // Los generados dentro de transclusiones (a cualquier profundidad) se completan con los
        // encabezados y la ruta de la nota incrustada; el hash del bloque ya incluye la huella
        // de lo incrustado.
        if !matches!(block.kind, BlockKind::Generated(_)) {
            let mut huella = String::new();
            if completar_incrustados(std::slice::from_mut(block), index, raiz, &mut huella) {
                completado = true;
                // Un índice incrustado depende del índice de la bóveda: entra en el hash.
                if let Some(h) = hashes.get_mut(n) {
                    let mut bytes = h.to_le_bytes().to_vec();
                    bytes.extend_from_slice(huella.as_bytes());
                    *h = orbital_vault::write::hash(&bytes);
                }
            }
            continue;
        }
        let BlockKind::Generated(g) = &block.kind else {
            continue;
        };
        let items = match g.kind {
            orbital_vault::render::GeneratedKind::Toc => {
                let d = defaults.get_or_insert_with(toc_defaults);
                block.kind = toc_block(&g.raw, &encabezados, path, d);
                if let Some(h) = hashes.get_mut(n) {
                    *h = orbital_vault::write::hash(format!("{:?}", block.kind).as_bytes());
                }
                completado = true;
                continue;
            }
            orbital_vault::render::GeneratedKind::Consulta => {
                block.kind = super::consulta::consulta_block(&g.raw, index, raiz);
                if let Some(h) = hashes.get_mut(n) {
                    *h = orbital_vault::write::hash(format!("{:?}", block.kind).as_bytes());
                }
                completado = true;
                continue;
            }
            orbital_vault::render::GeneratedKind::CardLink => {
                // La tarjeta se muestra como bloque de texto enlazado (013).
                let url = g
                    .params
                    .iter()
                    .find(|e| e.key == "url")
                    .and_then(|e| e.value.as_text())
                    .unwrap_or("");
                let title = g
                    .params
                    .iter()
                    .find(|e| e.key == "title")
                    .and_then(|e| e.value.as_text())
                    .unwrap_or(url);
                let description = g
                    .params
                    .iter()
                    .find(|e| e.key == "description")
                    .and_then(|e| e.value.as_text())
                    .unwrap_or("");
                let host = g
                    .params
                    .iter()
                    .find(|e| e.key == "host")
                    .and_then(|e| e.value.as_text())
                    .unwrap_or("");
                let mut items = vec![ListItem {
                    task: None,
                    blocks: vec![Block {
                        line: 0,
                        id: None,
                        source: 0..0,
                        kind: BlockKind::Paragraph(vec![Inline::Link {
                            url: url.to_owned(),
                            children: vec![Inline::Text(title.to_owned())],
                        }]),
                    }],
                }];
                if !description.is_empty() {
                    items.push(ListItem {
                        task: None,
                        blocks: vec![Block {
                            line: 0,
                            id: None,
                            source: 0..0,
                            kind: BlockKind::Paragraph(vec![Inline::Text(description.to_owned())]),
                        }],
                    });
                }
                if !host.is_empty() {
                    items.push(ListItem {
                        task: None,
                        blocks: vec![Block {
                            line: 0,
                            id: None,
                            source: 0..0,
                            kind: BlockKind::Paragraph(vec![Inline::Text(host.to_owned())]),
                        }],
                    });
                }
                items
            }
            orbital_vault::render::GeneratedKind::Indice => {
                let tag = g
                    .params
                    .iter()
                    .find(|e| e.key == "tag")
                    .and_then(|e| e.value.as_text())
                    .map(str::to_owned);
                match (tag, index) {
                    (Some(tag), Some(index)) => {
                        let fold_tag = orbital_vault::fold(tag.trim_start_matches('#'));
                        let tree = index.tags();
                        match nodo_del_tag(&tree, &fold_tag) {
                            Some(node) => indice_items(index, node),
                            None => Vec::new(),
                        }
                    }
                    _ => Vec::new(),
                }
            }
        };
        let nuevo = if items.is_empty() {
            let tag = g
                .params
                .iter()
                .find(|e| e.key == "tag")
                .and_then(|e| e.value.as_text())
                .map(|t| t.trim_start_matches('#').to_owned())
                .unwrap_or_default();
            BlockKind::Paragraph(vec![Inline::Text(format!("«sin notas con #{tag}»"))])
        } else {
            BlockKind::List { start: None, items }
        };
        block.kind = nuevo;
        if let Some(h) = hashes.get_mut(n) {
            *h = orbital_vault::write::hash(format!("{:?}", block.kind).as_bytes());
        }
        completado = true;
    }
    completado
}

/// Completa los bloques generados de las notas incrustadas en `blocks` (y en las que estas
/// incrustan): cada una con sus propios encabezados y su ruta. En `huella` deja algo que cambia
/// si cambia lo completado. Devuelve true si completó algo.
fn completar_incrustados(
    blocks: &mut [Block],
    index: Option<&orbital_vault::SearchIndex>,
    raiz: Option<&Path>,
    huella: &mut String,
) -> bool {
    use orbital_vault::render::Embed;
    let mut completado = false;
    for b in blocks {
        match &mut b.kind {
            BlockKind::Transclusion(t) => {
                if let Embed::Note { path, blocks, .. } = &mut t.content {
                    let mut sub = RenderDoc {
                        blocks: std::mem::take(blocks),
                    };
                    let mut hashes = vec![0; sub.blocks.len()];
                    if complete_generated(&mut sub, &mut hashes, index, raiz, Some(path)) {
                        completado = true;
                        for h in hashes.iter().filter(|h| **h != 0) {
                            huella.push_str(&format!("{h:x}"));
                        }
                    }
                    *blocks = sub.blocks;
                }
            }
            BlockKind::Quote(body)
            | BlockKind::Callout { body, .. }
            | BlockKind::Seccion { body, .. }
            | BlockKind::Group(body) => {
                completado |= completar_incrustados(body, index, raiz, huella)
            }
            BlockKind::Flashcard(f) => {
                completado |= completar_incrustados(&mut f.question, index, raiz, huella);
                completado |= completar_incrustados(&mut f.answer, index, raiz, huella);
            }
            BlockKind::List { items, .. } => {
                for it in items {
                    completado |= completar_incrustados(&mut it.blocks, index, raiz, huella);
                }
            }
            _ => {}
        }
    }
    completado
}

/// Encabezados de primer nivel (y dentro de citas), en orden del documento.
pub(super) fn headings_flat(blocks: &[Block]) -> Vec<(u8, String)> {
    let mut out = Vec::new();
    fn walk(out: &mut Vec<(u8, String)>, blocks: &[Block]) {
        for b in blocks {
            match &b.kind {
                BlockKind::Heading { level, inlines } => {
                    out.push((*level, plain_text(inlines)));
                }
                BlockKind::Quote(body) => walk(out, body),
                _ => {}
            }
        }
    }
    walk(&mut out, blocks);
    out
}

/// La tabla de contenidos de un codeblock `table-of-contents` o `toc` (opciones en
/// [`orbital_vault::toc`]): el título (Markdown) y la lista anidada, numerada o en una línea.
/// Sin encabezados, un aviso o nada (`hideWhenEmpty`); con opciones inválidas, el error.
pub(super) fn toc_block(
    raw: &str,
    encabezados: &[(u8, String)],
    path: Option<&orbital_vault::RelPath>,
    defaults: &orbital_vault::toc::TocDefaults,
) -> BlockKind {
    use orbital_vault::toc::TocStyle;
    let parrafo = |inlines: Vec<Inline>| Block {
        line: 0,
        id: None,
        source: 0..0,
        kind: BlockKind::Paragraph(inlines),
    };
    let o = match orbital_vault::toc::parse_options_with(raw, defaults.options()) {
        Ok(o) => o,
        Err(e) => {
            return BlockKind::Paragraph(vec![Inline::Emph(vec![Inline::Text(format!(
                "No se pudo mostrar la tabla de contenidos (valor inválido en `{}`)",
                e.0
            ))])]);
        }
    };
    let pares: Vec<(u8, &str)> = encabezados.iter().map(|(n, t)| (*n, t.as_str())).collect();
    let elegidos: Vec<(u8, &str)> = o.select(&pares).into_iter().map(|i| pares[i]).collect();
    // El título admite Markdown en línea: el primer párrafo de su análisis.
    let titulo: Vec<Inline> = if o.title.is_empty() {
        Vec::new()
    } else {
        orbital_vault::render::render_markdown(&o.title)
            .blocks
            .into_iter()
            .find_map(|b| match b.kind {
                BlockKind::Paragraph(p) => Some(p),
                BlockKind::Heading { inlines, .. } => Some(inlines),
                _ => None,
            })
            .unwrap_or_default()
    };
    let entrada = |titulo: &str| -> Inline {
        match path.filter(|_| o.include_links) {
            Some(path) => Inline::Link {
                url: format!("/{}#{}", ruta_sin_wiki(path), titulo),
                children: vec![Inline::Text(titulo.to_owned())],
            },
            None => Inline::Text(titulo.to_owned()),
        }
    };
    let mut bloques: Vec<Block> = Vec::new();
    if elegidos.is_empty() {
        if o.hide_when_empty {
            return BlockKind::Group(Vec::new());
        }
        if !titulo.is_empty() {
            bloques.push(parrafo(titulo));
        }
        bloques.push(parrafo(vec![Inline::Emph(vec![Inline::Text(
            "Tabla de contenidos: no hay encabezados".into(),
        )])]));
        return BlockKind::Group(bloques);
    }
    if o.style == TocStyle::InlineFirstLevel {
        // El título va en la misma línea, separado por un espacio.
        let mut linea = titulo;
        if !linea.is_empty() {
            linea.push(Inline::Text(" ".into()));
        }
        for (k, (_, t)) in elegidos.iter().enumerate() {
            if k > 0 {
                linea.push(Inline::Text(" | ".into()));
            }
            linea.push(entrada(t));
        }
        return BlockKind::Paragraph(linea);
    }
    if !titulo.is_empty() {
        bloques.push(parrafo(titulo));
    }
    let ordenada = o.style == TocStyle::NestedOrderedList;
    bloques.push(Block {
        line: 0,
        id: None,
        source: 0..0,
        kind: BlockKind::List {
            start: ordenada.then_some(1),
            items: toc_items(&elegidos, ordenada, entrada),
        },
    });
    BlockKind::Group(bloques)
}

/// Ítems del TOC, anidados por nivel; `entrada` arma el texto (o el enlace) de cada uno.
pub(super) fn toc_items(
    encabezados: &[(u8, &str)],
    ordenada: bool,
    entrada: impl Fn(&str) -> Inline,
) -> Vec<ListItem> {
    // Una pila de listas abiertas, de la menos a la más profunda. Un encabezado más profundo
    // abre una sublista; uno de nivel igual o menor cierra las más profundas, que quedan
    // anidadas en el último ítem de su lista madre.
    struct Pila {
        nivel: u8,
        items: Vec<ListItem>,
    }
    fn cerrar(pilas: &mut Vec<Pila>, ordenada: bool) {
        let Some(cerrada) = pilas.pop() else { return };
        let lista = Block {
            line: 0,
            id: None,
            source: 0..0,
            kind: BlockKind::List {
                start: ordenada.then_some(1),
                items: cerrada.items,
            },
        };
        if let Some(ultimo) = pilas.last_mut().and_then(|p| p.items.last_mut()) {
            ultimo.blocks.push(lista);
        }
    }
    let mut pilas: Vec<Pila> = Vec::new();
    for (nivel, titulo) in encabezados {
        let item = ListItem {
            task: None,
            blocks: vec![Block {
                line: 0,
                id: None,
                source: 0..0,
                kind: BlockKind::Paragraph(vec![entrada(titulo)]),
            }],
        };
        loop {
            let madre = pilas.len().checked_sub(2).map(|k| pilas[k].nivel);
            match pilas.last_mut() {
                None => pilas.push(Pila {
                    nivel: *nivel,
                    items: vec![item],
                }),
                Some(p) if p.nivel < *nivel => pilas.push(Pila {
                    nivel: *nivel,
                    items: vec![item],
                }),
                Some(p) if p.nivel == *nivel => p.items.push(item),
                // Más profunda que este encabezado: se cierra si la madre llega a su nivel;
                // si no (saltos de nivel, o la nota empieza con `##`), sube a su nivel.
                Some(_) if madre.is_some_and(|m| m >= *nivel) => {
                    cerrar(&mut pilas, ordenada);
                    continue;
                }
                Some(p) => {
                    p.nivel = *nivel;
                    p.items.push(item);
                }
            }
            break;
        }
    }
    while pilas.len() > 1 {
        cerrar(&mut pilas, ordenada);
    }
    pilas.pop().map(|p| p.items).unwrap_or_default()
}

/// `wiki/x.md` -> `x.md`: las URLs `/x.md` se resuelven desde la raíz de `wiki/`.
pub(super) fn ruta_sin_wiki(path: &orbital_vault::RelPath) -> &str {
    path.as_str().strip_prefix("wiki/").unwrap_or(path.as_str())
}

/// Ítems del índice: notas propias del tag (alfabéticas) y debajo, anidado, cada hija con sus
/// notas. La fila del tag abre la búsqueda por etiqueta (como el panel).
pub(super) fn indice_items(
    index: &orbital_vault::SearchIndex,
    node: &orbital_vault::TagNode,
) -> Vec<ListItem> {
    let mut items: Vec<ListItem> = index
        .notes_with_tag(&node.path)
        .into_iter()
        .map(|p| ListItem {
            task: None,
            blocks: vec![nota_item(&p)],
        })
        .collect();
    for child in &node.children {
        let hijos = indice_items(index, child);
        if hijos.is_empty() {
            continue;
        }
        let mut blocks = vec![Block {
            line: 0,
            id: None,
            source: 0..0,
            kind: BlockKind::Paragraph(vec![Inline::Link {
                url: format!("orbital-tag:{}", child.path),
                children: vec![Inline::Text(format!("#{}", child.name))],
            }]),
        }];
        blocks.push(Block {
            line: 0,
            id: None,
            source: 0..0,
            kind: BlockKind::List {
                start: None,
                items: hijos,
            },
        });
        items.push(ListItem { task: None, blocks });
    }
    items
}

/// La fila de una nota del índice: enlace a `/ruta` con el nombre del archivo.
pub(super) fn nota_item(path: &orbital_vault::RelPath) -> Block {
    Block {
        line: 0,
        id: None,
        source: 0..0,
        kind: BlockKind::Paragraph(vec![Inline::Link {
            url: format!("/{}", ruta_sin_wiki(path)),
            children: vec![Inline::Text(path.file_name().to_owned())],
        }]),
    }
}

/// El nodo del subárbol con esta ruta plegada, si existe.
pub(super) fn nodo_del_tag<'t>(
    tree: &'t orbital_vault::TagTree,
    path: &str,
) -> Option<&'t orbital_vault::TagNode> {
    let mut actuales: &[orbital_vault::TagNode] = &tree.roots;
    let mut restante = path;
    loop {
        let (tramo, resto) = restante.split_once('/').unwrap_or((restante, ""));
        let node = actuales
            .iter()
            .find(|n| orbital_vault::fold(&n.name) == tramo)?;
        if resto.is_empty() {
            return Some(node);
        }
        actuales = &node.children;
        restante = resto;
    }
}

impl Prepared {
    /// Con las propiedades del frontmatter para resolver los marcadores `[%clave]` (005).
    pub fn new(text: &str, yaml: &[orbital_vault::props::PropEntry]) -> Self {
        Self::build(text, None, yaml)
    }

    /// Como [`Prepared::new`], con las transclusiones expandidas (lee las notas incrustadas: va
    /// en el hilo de fondo) y los marcadores resueltos.
    pub fn with_vault(
        text: &str,
        vault: &orbital_vault::Vault,
        from: &orbital_vault::RelPath,
        yaml: &[orbital_vault::props::PropEntry],
    ) -> Self {
        Self::build(text, Some((vault, from)), yaml)
    }

    fn build(
        text: &str,
        vault: Option<(&orbital_vault::Vault, &orbital_vault::RelPath)>,
        yaml: &[orbital_vault::props::PropEntry],
    ) -> Self {
        let repl = orbital_vault::markers::resolve_markers(text, yaml);
        let mut doc = orbital_vault::render::render_markdown_interpolated(text, &repl);
        let deps = vault.map_or_else(Vec::new, |(v, from)| v.expand_transclusions(from, &mut doc));
        // Un bloque con transclusiones cambia también si cambia lo incrustado: la huella entra en
        // su hash, y la actualización en el sitio reemplaza solo esa tarjeta.
        let hashes = doc
            .blocks
            .iter()
            .map(|b| {
                let h = orbital_vault::write::hash(text[b.source.clone()].as_bytes());
                match orbital_vault::transclusion::fingerprint(b) {
                    0 => h,
                    fp => orbital_vault::write::hash(&[h.to_le_bytes(), fp.to_le_bytes()].concat()),
                }
            })
            .collect();
        let header = orbital_vault::props::markdown::locate(text)
            .map(|r| orbital_vault::write::hash(text[r].as_bytes()))
            .unwrap_or(0);
        Self {
            doc,
            hashes,
            header,
            deps,
            plegables: Vec::new(),
        }
    }

    /// Las flechitas del plegado (028): las líneas plegables y, en el hash de cada bloque, cuáles
    /// de sus líneas lo son (relativas al bloque: un bloque que gana o pierde una flechita se
    /// vuelve a dibujar; editar más arriba no cambia nada).
    pub fn con_plegables(&mut self, plegables: &[orbital_vault::Plegable]) {
        self.plegables = plegables.iter().map(|p| p.linea).collect();
        if self.plegables.is_empty() {
            return;
        }
        let lineas: Vec<u32> = self.doc.blocks.iter().map(|b| b.line as u32).collect();
        for (n, h) in self.hashes.iter_mut().enumerate() {
            let desde = lineas[n];
            let hasta = lineas.get(n + 1).copied().unwrap_or(u32::MAX);
            let a = self.plegables.partition_point(|&l| l < desde);
            let z = self.plegables.partition_point(|&l| l < hasta);
            if a == z {
                continue;
            }
            let mut bytes = h.to_le_bytes().to_vec();
            for l in &self.plegables[a..z] {
                bytes.extend_from_slice(&(l - desde).to_le_bytes());
            }
            *h = orbital_vault::write::hash(&bytes);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// La forma del TOC: cada ítem con su título y, entre paréntesis, sus hijos.
    fn forma(items: &[ListItem]) -> String {
        items
            .iter()
            .map(|it| {
                let mut s = String::new();
                for b in &it.blocks {
                    match &b.kind {
                        BlockKind::Paragraph(p) => s.push_str(&plain_text(p)),
                        BlockKind::List { items, .. } => {
                            s.push('(');
                            s.push_str(&forma(items));
                            s.push(')');
                        }
                        _ => {}
                    }
                }
                s
            })
            .collect::<Vec<_>>()
            .join(" ")
    }

    fn toc(encabezados: &[(u8, &str)]) -> String {
        forma(&toc_items(encabezados, false, |t| {
            Inline::Text(t.to_owned())
        }))
    }

    #[test]
    fn toc_anida_por_nivel() {
        assert_eq!(
            toc(&[(1, "A"), (2, "B"), (3, "C"), (2, "D"), (1, "E")]),
            "A(B(C) D) E"
        );
        // Saltos de nivel y una nota que empieza más profunda.
        assert_eq!(toc(&[(1, "A"), (3, "B"), (2, "C")]), "A(B C)");
        assert_eq!(toc(&[(2, "A"), (3, "B"), (1, "C"), (2, "D")]), "A(B) C(D)");
    }

    fn enc(v: &[(u8, &str)]) -> Vec<(u8, String)> {
        v.iter().map(|(n, t)| (*n, (*t).to_owned())).collect()
    }

    #[test]
    fn toc_bloque_con_opciones() {
        let path = orbital_vault::RelPath::new("wiki/x.md").unwrap();
        let h = enc(&[(2, "A"), (3, "B"), (2, "C")]);
        let d = orbital_vault::toc::TocDefaults::default();
        // Título y lista numerada, con enlaces a la propia nota.
        let BlockKind::Group(g) = toc_block(
            "title: **Índice**\nstyle: nestedOrderedList # numerada",
            &h,
            Some(&path),
            &d,
        ) else {
            panic!("se esperaba un grupo");
        };
        assert!(matches!(&g[0].kind, BlockKind::Paragraph(p) if plain_text(p) == "Índice"));
        let BlockKind::List { start, items } = &g[1].kind else {
            panic!("se esperaba la lista");
        };
        assert_eq!(*start, Some(1));
        assert_eq!(forma(items), "A(B) C");
        let BlockKind::Paragraph(p) = &items[0].blocks[0].kind else {
            panic!()
        };
        assert!(matches!(&p[0], Inline::Link { url, .. } if url == "/x.md#A"));
        // En una línea y sin enlaces.
        let BlockKind::Paragraph(p) = toc_block(
            "style: inlineFirstLevel\nincludeLinks: false",
            &h,
            Some(&path),
            &d,
        ) else {
            panic!()
        };
        assert_eq!(plain_text(&p), "A | C");
        assert!(p.iter().all(|i| !matches!(i, Inline::Link { .. })));
        // Sin encabezados: aviso u oculto; opción inválida: el error.
        assert!(matches!(toc_block("", &[], Some(&path), &d), BlockKind::Group(g) if g.len() == 1));
        assert!(
            matches!(toc_block("hideWhenEmpty: true", &[], Some(&path), &d), BlockKind::Group(g) if g.is_empty())
        );
        assert!(matches!(
            toc_block("style: arbol", &h, Some(&path), &d),
            BlockKind::Paragraph(_)
        ));
    }

    /// Los valores por defecto de Preferencias (035): un bloque sin opciones los usa y lo que el
    /// bloque escribe manda.
    #[test]
    fn toc_con_valores_por_defecto() {
        use orbital_vault::toc::{TocDefaults, TocStyle};
        let path = orbital_vault::RelPath::new("wiki/x.md").unwrap();
        let h = enc(&[(1, "A"), (2, "B"), (3, "C"), (1, "D")]);
        let d = TocDefaults {
            title: "En esta nota".into(),
            style: TocStyle::NestedOrderedList,
            max_level: 2,
            include_links: false,
            ..TocDefaults::default()
        };
        let BlockKind::Group(g) = toc_block("", &h, Some(&path), &d) else {
            panic!("se esperaba un grupo");
        };
        assert!(matches!(&g[0].kind, BlockKind::Paragraph(p) if plain_text(p) == "En esta nota"));
        let BlockKind::List { start, items } = &g[1].kind else {
            panic!("se esperaba la lista");
        };
        assert_eq!(*start, Some(1));
        assert_eq!(forma(items), "A(B) D");
        let BlockKind::Paragraph(p) = &items[0].blocks[0].kind else {
            panic!()
        };
        assert!(matches!(&p[0], Inline::Text(_)), "sin enlaces por defecto");
        // El bloque manda.
        let BlockKind::Group(g) = toc_block(
            "title: ''\nstyle: lista\nmaxLevel: 0\nincludeLinks: true",
            &h,
            Some(&path),
            &d,
        ) else {
            panic!("se esperaba un grupo");
        };
        assert_eq!(g.len(), 1, "sin título");
        let BlockKind::List { start, items } = &g[0].kind else {
            panic!("se esperaba la lista");
        };
        assert_eq!(*start, None);
        assert_eq!(forma(items), "A(B(C)) D");
    }

    /// SC-001/SC-002 (035): analizar y completar una nota de 300 encabezados con una tabla de
    /// contenidos, y solo completar la tabla. `cargo test --release --bin orbital medir_toc --
    /// --ignored --nocapture`.
    #[test]
    #[ignore = "medición"]
    fn medir_toc() {
        let mut texto = String::from("```toc\n```\n\n");
        for k in 0..300 {
            let nivel = "#".repeat(1 + k % 3);
            texto.push_str(&format!(
                "{nivel} Sección {k} con **formato** ñ\n\nTexto {k}.\n\n"
            ));
        }
        let path = orbital_vault::RelPath::new("wiki/x.md").unwrap();
        let mut mejor_total = f64::MAX;
        let mut mejor_tabla = f64::MAX;
        for _ in 0..20 {
            let t = std::time::Instant::now();
            let mut p = Prepared::new(&texto, &[]);
            let analizar = t.elapsed();
            assert!(complete_generated(
                &mut p.doc,
                &mut p.hashes,
                None,
                None,
                Some(&path)
            ));
            mejor_total = mejor_total.min(t.elapsed().as_secs_f64() * 1e3);
            mejor_tabla = mejor_tabla.min((t.elapsed() - analizar).as_secs_f64() * 1e3);
        }
        println!(
            "300 encabezados: análisis + tabla {mejor_total:.2} ms; solo la tabla {mejor_tabla:.3} ms"
        );
    }
}
