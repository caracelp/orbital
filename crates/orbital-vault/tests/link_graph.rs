//! Tests del grafo de enlaces y retroenlaces en memoria (especificación 003).

mod common;

use common::TempDir;
use orbital_vault::entry::RelPath;
use orbital_vault::link_graph::LinkGraph;

#[test]
fn link_graph_basic_and_incremental() {
    let tmp = TempDir::new("link-graph");
    let root = tmp.path();

    // Crear estructura básica de bóveda
    std::fs::create_dir_all(root.join("wiki")).unwrap();

    let a_path = root.join("wiki/a.md");
    let b_path = root.join("wiki/b.md");
    let c_path = root.join("wiki/c.md");
    let d_path = root.join("wiki/d.md");

    std::fs::write(&a_path, "Nota A citando a [[b]] y a [[c#seccion]].\n").unwrap();
    std::fs::write(&b_path, "Nota B citando también a [[c]].\n").unwrap();
    std::fs::write(&c_path, "Nota C sin enlaces salientes.\n").unwrap();
    std::fs::write(&d_path, "Nota D citando a [[nota_pendiente]].\n").unwrap();

    let vault = orbital_vault::open(root).unwrap();
    let mut graph = LinkGraph::build(&vault).unwrap();

    let rel_a = RelPath::new("wiki/a.md").unwrap();
    let rel_b = RelPath::new("wiki/b.md").unwrap();
    let rel_c = RelPath::new("wiki/c.md").unwrap();
    let rel_d = RelPath::new("wiki/d.md").unwrap();

    // 1. Verificar retroenlaces iniciales
    let c_backlinks = graph.backlinks_for(&rel_c);
    assert_eq!(c_backlinks.len(), 2);
    assert!(c_backlinks.iter().any(|b| b.from == rel_a));
    assert!(c_backlinks.iter().any(|b| b.from == rel_b));

    let b_backlinks = graph.backlinks_for(&rel_b);
    assert_eq!(b_backlinks.len(), 1);
    assert_eq!(b_backlinks[0].from, rel_a);

    let a_backlinks = graph.backlinks_for(&rel_a);
    assert!(a_backlinks.is_empty());

    // 2. Verificar enlaces pendientes (no resueltos)
    assert!(graph.is_unresolved("nota_pendiente"));
    assert!(!graph.is_unresolved("b"));

    // 3. Actualización incremental (editar A para que ya no cite a C)
    let new_a_text = "Nota A editada, ahora solo cita a [[b]].\n";
    std::fs::write(&a_path, new_a_text).unwrap();
    graph.update_file(&vault, &rel_a, new_a_text, false);

    let c_backlinks_after = graph.backlinks_for(&rel_c);
    assert_eq!(c_backlinks_after.len(), 1);
    assert_eq!(c_backlinks_after[0].from, rel_b);

    // 4. Eliminación de archivo (borrar B)
    graph.remove_file(&rel_b);
    let c_backlinks_final = graph.backlinks_for(&rel_c);
    assert!(c_backlinks_final.is_empty());

    // 5. Creación de nota pendiente
    let pendiente_path = root.join("wiki/nota_pendiente.md");
    let pendiente_text = "Nota que antes estaba pendiente.\n";
    std::fs::write(&pendiente_path, pendiente_text).unwrap();
    // Reabrir o refrescar vault para que reconozca la nueva entrada
    let vault = orbital_vault::open(root).unwrap();
    let rel_pendiente = RelPath::new("wiki/nota_pendiente.md").unwrap();

    graph.update_file(&vault, &rel_pendiente, pendiente_text, false);
    // Ahora nota_pendiente ya existe en la bóveda, D debe figurar como retroenlace
    // al volver a indexar D o resolver
    graph.update_file(
        &vault,
        &rel_d,
        "Nota D citando a [[nota_pendiente]].\n",
        false,
    );
    let pend_backlinks = graph.backlinks_for(&rel_pendiente);
    assert_eq!(pend_backlinks.len(), 1);
    assert_eq!(pend_backlinks[0].from, rel_d);
    assert!(!graph.is_unresolved("nota_pendiente"));
}
