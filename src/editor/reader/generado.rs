//! Contenido generado de la vista formateada (005): índice por etiqueta, tabla de contenidos y
//! la preparación del documento (`Prepared`) con marcadores y transclusiones resueltos.

use super::*;

/// Completa los codeblocks generados del doc (005, T015 y T019): `toc` con los encabezados del
/// propio doc y `indice` con las notas de cada tag desde el índice. Reemplaza cada bloque
/// manteniendo su rango fuente (el codeblock completo) y actualiza su hash, así la actualización
/// en el sitio ve el contenido nuevo aunque el fuente no cambie. Devuelve true si completó algo.
pub fn complete_generated(
    doc: &mut RenderDoc,
    hashes: &mut [u64],
    index: Option<&orbital_vault::SearchIndex>,
    path: Option<&orbital_vault::RelPath>,
) -> bool {
    let encabezados = headings_flat(&doc.blocks);
    let mut completado = false;
    for (n, block) in doc.blocks.iter_mut().enumerate() {
        let BlockKind::Generated(g) = &block.kind else {
            continue;
        };
        let items = match g.kind {
            orbital_vault::render::GeneratedKind::Toc => {
                let (min, max) = niveles(&g.params);
                toc_items(&encabezados, path, min, max)
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

/// `minLevel` y `maxLevel` del codeblock `toc`; fuera de rango o inválido: sin límite. El
/// escalar YAML de un número llega como `PropValue::Number` (no como texto), así que se
/// aceptan ambos sin perder el texto original.
pub(super) fn niveles(params: &[orbital_vault::props::PropEntry]) -> (u8, u8) {
    let nivel = |key: &str, defecto: u8| -> u8 {
        params
            .iter()
            .find(|e| e.key == key)
            .and_then(|e| match &e.value {
                orbital_vault::props::PropValue::Text(v)
                | orbital_vault::props::PropValue::Number(v)
                | orbital_vault::props::PropValue::Date(v)
                | orbital_vault::props::PropValue::DateTime(v)
                | orbital_vault::props::PropValue::Link(v) => Some(v.as_str()),
                _ => None,
            })
            .and_then(|v| v.trim().parse::<u8>().ok())
            .unwrap_or(defecto)
    };
    (nivel("minLevel", 0), nivel("maxLevel", 6).min(6))
}

/// Ítems del TOC: enlaces al encabezado de la propia nota, anidados por nivel.
pub(super) fn toc_items(
    encabezados: &[(u8, String)],
    path: Option<&orbital_vault::RelPath>,
    min: u8,
    max: u8,
) -> Vec<ListItem> {
    let Some(path) = path else {
        return Vec::new();
    };
    // Una pila de listas abiertas, de la menos a la más profunda. Un encabezado más profundo
    // abre una sublista; uno de nivel igual o menor cierra las más profundas, que quedan
    // anidadas en el último ítem de su lista madre.
    struct Pila {
        nivel: u8,
        items: Vec<ListItem>,
    }
    fn cerrar(pilas: &mut Vec<Pila>) {
        let Some(cerrada) = pilas.pop() else { return };
        let lista = Block {
            line: 0,
            id: None,
            source: 0..0,
            kind: BlockKind::List {
                start: None,
                items: cerrada.items,
            },
        };
        if let Some(ultimo) = pilas.last_mut().and_then(|p| p.items.last_mut()) {
            ultimo.blocks.push(lista);
        }
    }
    let mut pilas: Vec<Pila> = Vec::new();
    for (nivel, titulo) in encabezados {
        if *nivel < min || *nivel > max || titulo.is_empty() {
            continue;
        }
        let item = ListItem {
            task: None,
            blocks: vec![Block {
                line: 0,
                id: None,
                source: 0..0,
                kind: BlockKind::Paragraph(vec![Inline::Link {
                    url: format!("/{}#{}", ruta_sin_wiki(path), titulo),
                    children: vec![Inline::Text(titulo.clone())],
                }]),
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
                    cerrar(&mut pilas);
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
        cerrar(&mut pilas);
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
        let e: Vec<(u8, String)> = encabezados
            .iter()
            .map(|(n, t)| (*n, (*t).to_owned()))
            .collect();
        let path = orbital_vault::RelPath::new("wiki/x.md").unwrap();
        forma(&toc_items(&e, Some(&path), 0, 6))
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
}
