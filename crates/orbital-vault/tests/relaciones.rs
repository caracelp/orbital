//! Conexiones con tipo y motivo (049): índice de enlaces, búsqueda `rel:`, consultas vivas y la
//! convivencia con los campos `clave:: valor` de la 004.

mod common;

use common::TempDir;
use orbital_vault::entry::RelPath;
use orbital_vault::link_graph::LinkGraph;
use orbital_vault::{SearchIndex, SearchOptions, parse_query};

const DERIVADAS_REGLA: &str = "---\nid: AP-001\ntype: apunte\n---\n<!-- block 1 AP-001 -->\n# Regla de la cadena\nestado:: activo\n<!---->\n\n<!-- connections AP-001 -->\n- amplía:: [[Derivadas]] — agrega la regla de la cadena\n- Contradice A:: [[Límites intuitivos]] - no vale para x→0\n- es fuente de:: [[Integrales]]\n- refuta:: [[Derivadas]]\n- [[Cálculo]]\n<!---->\n";

fn boveda(tmp: &TempDir) -> orbital_vault::Vault {
    let root = tmp.path();
    std::fs::create_dir_all(root.join("wiki")).unwrap();
    std::fs::write(root.join("wiki/regla.md"), DERIVADAS_REGLA).unwrap();
    for n in ["Derivadas", "Límites intuitivos", "Integrales", "Cálculo"] {
        std::fs::write(root.join(format!("wiki/{n}.md")), format!("# {n}\n")).unwrap();
    }
    std::fs::write(
        root.join("wiki/otra.md"),
        "Texto con [[Derivadas]].\n\n<!-- connections X -->\n- amplía:: [[Integrales]]\n<!---->\n",
    )
    .unwrap();
    orbital_vault::open(root).unwrap()
}

fn rel(p: &str) -> RelPath {
    RelPath::new(p).unwrap()
}

#[test]
fn retroenlaces_llevan_tipo_y_motivo() {
    let tmp = TempDir::new("rel-grafo");
    let vault = boveda(&tmp);
    let mut graph = LinkGraph::build(&vault).unwrap();

    let refs = graph.backlinks_for(&rel("wiki/Derivadas.md"));
    let mut vistos: Vec<(&str, Option<&str>, Option<&str>)> = refs
        .iter()
        .map(|b| {
            (
                b.from.as_str(),
                b.relacion.as_ref().map(|r| r.tipo.as_str()),
                b.relacion.as_ref().and_then(|r| r.motivo.as_deref()),
            )
        })
        .collect();
    vistos.sort();
    assert_eq!(
        vistos,
        [
            ("wiki/otra.md", None, None),
            (
                "wiki/regla.md",
                Some("amplía"),
                Some("agrega la regla de la cadena")
            ),
            ("wiki/regla.md", Some("refuta"), None),
        ]
    );
    // Un enlace sin tipo sigue como hoy.
    let calc = graph.backlinks_for(&rel("wiki/Cálculo.md"));
    assert_eq!(calc.len(), 1);
    assert!(calc[0].relacion.is_none());

    // El par con tipo gana al par sin tipo; el tipo va en su nombre canónico.
    let aristas = graph.enlaces_resueltos_con_tipo();
    let tipo_de = |a: &str, b: &str| {
        aristas
            .iter()
            .find(|(x, y, _)| x.as_str() == a && y.as_str() == b)
            .map(|(_, _, t)| t.clone())
    };
    assert_eq!(
        tipo_de("wiki/regla.md", "wiki/Derivadas.md"),
        Some(Some("amplía".into()))
    );
    assert_eq!(
        tipo_de("wiki/regla.md", "wiki/Límites intuitivos.md"),
        Some(Some("contradice a".into()))
    );
    assert_eq!(tipo_de("wiki/otra.md", "wiki/Derivadas.md"), Some(None));
    assert_eq!(aristas.len(), graph.enlaces_resueltos().len());

    let tipos = graph.tipos_usados();
    assert_eq!(tipos[0], "amplía", "{tipos:?}");
    assert!(tipos.contains(&"refuta".to_owned()));
    assert_eq!(tipos.len(), 4);

    // Incremental: al quitar el tipo, el retroenlace queda sin tipo.
    graph.update_file(
        &vault,
        &rel("wiki/otra.md"),
        "<!-- connections X -->\n- [[Integrales]]\n<!---->\n",
        false,
    );
    let integrales = graph.backlinks_for(&rel("wiki/Integrales.md"));
    assert!(
        integrales
            .iter()
            .any(|b| b.from.as_str() == "wiki/otra.md" && b.relacion.is_none())
    );
    // En HTML no hay partes de la 041: los enlaces no llevan tipo.
    graph.update_file(
        &vault,
        &rel("wiki/otra.md"),
        "<p><!-- connections X --></p><ul><li>amplía:: <a href=\"Integrales.md\">I</a></li></ul>",
        true,
    );
    assert!(
        graph
            .backlinks_for(&rel("wiki/Integrales.md"))
            .iter()
            .all(|b| b.relacion.is_none() || b.from.as_str() != "wiki/otra.md")
    );
}

fn buscar(index: &SearchIndex, vault: &orbital_vault::Vault, q: &str) -> Vec<String> {
    let query = parse_query(q).unwrap();
    let mut v: Vec<String> = index
        .search(vault, &query, &SearchOptions::default())
        .notes
        .into_iter()
        .map(|h| h.path.to_string())
        .collect();
    v.sort();
    v
}

#[test]
fn operador_rel_en_busqueda_y_consultas() {
    let tmp = TempDir::new("rel-busqueda");
    let vault = boveda(&tmp);
    let index = SearchIndex::build(&vault);

    assert_eq!(buscar(&index, &vault, "rel:contradice"), ["wiki/regla.md"]);
    assert_eq!(
        buscar(&index, &vault, "rel:\"CONTRADICE A\""),
        ["wiki/regla.md"]
    );
    assert_eq!(
        buscar(&index, &vault, "rel:amplia"),
        ["wiki/otra.md", "wiki/regla.md"]
    );
    assert_eq!(
        buscar(&index, &vault, "rel:amplía:Derivadas"),
        ["wiki/regla.md"]
    );
    assert_eq!(
        buscar(&index, &vault, "rel:amplía:[[Integrales]]"),
        ["wiki/otra.md"]
    );
    assert_eq!(
        buscar(&index, &vault, "rel::\"límites intuitivos\""),
        ["wiki/regla.md"]
    );
    assert!(buscar(&index, &vault, "rel:ejemplifica").is_empty());
    assert_eq!(
        buscar(&index, &vault, "rel:amplia -rel:refuta"),
        ["wiki/otra.md"]
    );
    assert!(parse_query("rel:").is_err());
    assert!(parse_query("rel:\"sin cerrar").is_err());

    // Las relaciones no son campos: `[amplía]` no las encuentra; el campo del cuerpo sigue.
    assert!(buscar(&index, &vault, "[amplía]").is_empty());
    assert!(buscar(&index, &vault, "[refuta]").is_empty());
    assert_eq!(buscar(&index, &vault, "[estado:activo]"), ["wiki/regla.md"]);
    let campos = orbital_vault::inline_fields(DERIVADAS_REGLA);
    assert_eq!(campos, [("estado".to_owned(), "activo".to_owned())]);

    // Dentro de un codeblock `consulta` (048) funciona igual.
    let c = orbital_vault::consulta::parse("rel:contradice OR rel:amplía:Integrales").unwrap();
    let r = orbital_vault::consulta::ejecutar(&index, Some(vault.root()), &c);
    let mut filas: Vec<String> = r.filas.iter().map(|f| f.path.to_string()).collect();
    filas.sort();
    assert_eq!(filas, ["wiki/otra.md", "wiki/regla.md"]);
}
