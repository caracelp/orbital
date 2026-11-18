mod common;

use std::time::{Duration, SystemTime};

use common::TempDir;
use orbital_vault::RelPath;
use orbital_vault::consulta::{Vista, ejecutar, mostrar, parse};
use orbital_vault::props::PropValue;
use orbital_vault::search::SearchIndex;

fn boveda() -> (TempDir, orbital_vault::Vault) {
    let tmp = TempDir::new("consulta");
    let root = tmp.path();
    std::fs::create_dir_all(root.join("wiki")).unwrap();
    std::fs::create_dir_all(root.join("raw")).unwrap();
    let notas = [
        (
            "wiki/alfa.md",
            "---\nestado: activo\nprioridad: 10\n---\n#proyecto\nEl motor arranca.\n",
            3,
        ),
        (
            "wiki/beta.md",
            "---\nestado: pausado\nprioridad: 9\n---\n#proyecto\nNada del motor por ahora.\nOtra línea con motor.\n",
            1,
        ),
        ("wiki/Gamma.md", "#proyecto\nestado:: activo\n", 2),
        (
            "wiki/delta.html",
            "<html><body>\n<p>#proyecto</p>\n<p>prioridad:: 1</p>\n</body></html>\n",
            4,
        ),
        ("wiki/otra.md", "#ajena\nmotor\n", 5),
        ("raw/fuente.md", "#proyecto\n", 6),
    ];
    let base = SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000);
    for (ruta, texto, dias) in notas {
        let full = root.join(ruta);
        std::fs::write(&full, texto).unwrap();
        let f = std::fs::File::options().write(true).open(&full).unwrap();
        f.set_modified(base + Duration::from_secs(86_400 * dias))
            .unwrap();
    }
    let vault = orbital_vault::open(root).unwrap();
    (tmp, vault)
}

fn nombres(r: &orbital_vault::consulta::Resultado) -> Vec<&str> {
    r.filas.iter().map(|f| f.path.file_name()).collect()
}

#[test]
fn lista_por_nombre_sin_raw() {
    let (_tmp, vault) = boveda();
    let index = SearchIndex::build(&vault);
    let r = ejecutar(&index, None, &parse("tag:#proyecto").unwrap());
    assert_eq!(r.total, 4);
    assert_eq!(
        nombres(&r),
        ["alfa.md", "beta.md", "delta.html", "Gamma.md"]
    );
    let r = ejecutar(
        &index,
        None,
        &parse("tag:#proyecto -[estado:activo]").unwrap(),
    );
    assert_eq!(nombres(&r), ["beta.md", "delta.html"]);
}

#[test]
fn orden_por_fecha_y_por_propiedad_tipada() {
    let (_tmp, vault) = boveda();
    let index = SearchIndex::build(&vault);
    let r = ejecutar(
        &index,
        None,
        &parse("tag:#proyecto\norden: modificado desc").unwrap(),
    );
    assert_eq!(
        nombres(&r),
        ["delta.html", "alfa.md", "Gamma.md", "beta.md"]
    );
    // 9 < 10 por valor numérico (no por texto). Como en Bases, un campo `clave:: valor` es texto
    // y los textos van después de los números.
    let r = ejecutar(
        &index,
        None,
        &parse("[prioridad]\norden: prioridad").unwrap(),
    );
    assert_eq!(nombres(&r), ["beta.md", "alfa.md", "delta.html"]);
    let r = ejecutar(
        &index,
        None,
        &parse("tag:#proyecto\norden: prioridad desc").unwrap(),
    );
    // Descendente, la nota sin la propiedad queda al final.
    assert_eq!(
        nombres(&r),
        ["delta.html", "alfa.md", "beta.md", "Gamma.md"]
    );
}

#[test]
fn tabla_con_valores_de_yaml_e_inline() {
    let (_tmp, vault) = boveda();
    let index = SearchIndex::build(&vault);
    let c = parse("tag:#proyecto\nvista: tabla\ncolumnas: Estado, prioridad").unwrap();
    assert_eq!(c.vista, Vista::Tabla);
    let r = ejecutar(&index, None, &c);
    let alfa = &r.filas[0];
    assert_eq!(alfa.valores[0], Some(PropValue::Text("activo".into())));
    assert_eq!(alfa.valores[1], Some(PropValue::Number("10".into())));
    let gamma = &r.filas[3];
    assert_eq!(gamma.valores[0], Some(PropValue::Text("activo".into())));
    assert_eq!(gamma.valores[1], None);
    assert_eq!(mostrar(&PropValue::Bool(true)), "sí");
}

#[test]
#[allow(clippy::single_range_in_vec_init)]
fn coincidencias_y_conteo() {
    let (_tmp, vault) = boveda();
    let index = SearchIndex::build(&vault);
    let c = parse("motor tag:#proyecto\nvista: coincidencias").unwrap();
    let r = ejecutar(&index, Some(vault.root()), &c);
    assert_eq!(nombres(&r), ["alfa.md", "beta.md"]);
    let beta = &r.filas[1];
    assert_eq!(beta.coincidencias.len(), 2);
    assert_eq!(beta.coincidencias[0].snippet, "Nada del motor por ahora.");
    assert_eq!(beta.coincidencias[0].highlights, vec![9..14]);
    let r = ejecutar(&index, None, &parse("motor\nvista: conteo").unwrap());
    assert_eq!(r.total, 3);
    assert!(r.filas.is_empty());
}

#[test]
fn se_actualiza_con_el_indice() {
    let (_tmp, vault) = boveda();
    let mut index = SearchIndex::build(&vault);
    let c = parse("[estado:activo]").unwrap();
    assert_eq!(ejecutar(&index, None, &c).total, 2);
    let ruta = RelPath::new("wiki/beta.md").unwrap();
    let texto = "---\nestado: activo\n---\n";
    std::fs::write(ruta.to_path(vault.root()), texto).unwrap();
    index.update_file(&vault, &ruta, texto);
    assert_eq!(ejecutar(&index, None, &c).total, 3);
    index.remove_file(&ruta);
    assert_eq!(ejecutar(&index, None, &c).total, 2);
}

#[test]
fn el_bloque_no_etiqueta_ni_enlaza_su_nota() {
    let (_tmp, vault) = boveda();
    let ruta = RelPath::new("wiki/tablero.md").unwrap();
    let texto = "# Tablero\n\n```consulta\ntag:#proyecto OR #ajena [[alfa]]\nvista: tabla\n```\n";
    std::fs::write(ruta.to_path(vault.root()), texto).unwrap();
    let vault = orbital_vault::open(vault.root()).unwrap();
    let index = SearchIndex::build(&vault);
    let r = ejecutar(&index, None, &parse("tag:#proyecto OR tag:#ajena").unwrap());
    assert!(!nombres(&r).contains(&"tablero.md"));
    assert!(orbital_vault::extract_links(texto, false).is_empty());
}
