//! Lo «solo en Drive» (061b): entradas que no están en la bóveda, con su copia (o su extracto) en
//! otra carpeta. La búsqueda, el grafo, las etiquetas y las propiedades las leen igual.

use std::collections::HashMap;
use std::fs;

use orbital_vault::{LinkGraph, RelPath, SearchIndex, SearchOptions, parse_query, ubicacion};

#[test]
fn el_indice_ve_lo_que_esta_solo_en_drive() {
    let base = std::env::temp_dir().join(format!("orbital-remotos-{}", std::process::id()));
    let root = base.join("boveda");
    let cache = base.join("cache");
    fs::create_dir_all(root.join("wiki")).unwrap();
    fs::create_dir_all(root.join("raw")).unwrap();
    fs::write(root.join("wiki/local.md"), "# Local\n\n[[remota]]\n").unwrap();
    fs::create_dir_all(cache.join("1")).unwrap();
    fs::write(
        cache.join("1/remota.md"),
        "---\ntitle: La remota\ntags: [viaje]\n---\n# Remota\n\npalabraunica y [[local]] #nube\n",
    )
    .unwrap();
    let mut v = orbital_vault::open(&root).unwrap();
    let remota = RelPath::new("wiki/sub/remota.md").unwrap();
    let video = RelPath::new("raw/video.mp4").unwrap();
    v.agregar_remotos(&[(remota.clone(), 90, None), (video.clone(), 5 << 20, None)]);
    ubicacion::registrar(
        &root,
        HashMap::from([(remota.as_str().to_owned(), cache.join("1/remota.md"))]),
    );
    // Las entradas: la nota, el video y la carpeta que falta en el disco.
    let rutas: Vec<&str> = v.entries().iter().map(|e| e.path.as_str()).collect();
    assert!(rutas.contains(&"wiki/sub") && rutas.contains(&"wiki/sub/remota.md"));
    assert!(rutas.contains(&"raw/video.mp4"));
    // Búsqueda por texto y por etiqueta.
    let idx = SearchIndex::build(&v);
    let r = idx.search(
        &v,
        &parse_query("palabraunica").unwrap(),
        &SearchOptions::default(),
    );
    assert_eq!(r.notes.len(), 1);
    assert_eq!(r.notes[0].path, remota);
    assert!(
        !r.notes[0].matches.is_empty(),
        "con el renglón que coincide"
    );
    assert!(idx.tag_names().iter().any(|t| t == "nube"));
    // Grafo: los enlaces en los dos sentidos.
    let g = LinkGraph::build(&v).unwrap();
    assert!(
        g.backlinks_for(&RelPath::new("wiki/local.md").unwrap())
            .iter()
            .any(|b| b.from == remota)
    );
    assert!(
        g.backlinks_for(&remota)
            .iter()
            .any(|b| b.from.as_str() == "wiki/local.md")
    );
    // Propiedades.
    let n = v.read_note(&remota).unwrap();
    assert!(n.source.contains("La remota"));
    ubicacion::registrar(&root, HashMap::new());
    fs::remove_dir_all(&base).unwrap();
}
