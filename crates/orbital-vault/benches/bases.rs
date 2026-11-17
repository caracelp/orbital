//! Índice frío, consulta caliente y actualización aislada de 5000 notas temporales.
use orbital_vault::bases::*;
use std::sync::OnceLock;
fn main() {
    divan::main();
}
fn vault() -> &'static orbital_vault::Vault {
    static V: OnceLock<orbital_vault::Vault> = OnceLock::new();
    V.get_or_init(|| {
        let dir = std::env::temp_dir().join(format!("orbital-bench-bases-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        for n in 0..5000 {
            std::fs::write(
                dir.join(format!("nota-{n}.md")),
                format!("---\ntitle: Nota {n}\nvalor: {n}\ntags: [trabajo]\n---\nTexto\n"),
            )
            .unwrap();
        }
        orbital_vault::open(&dir).unwrap()
    })
}
#[divan::bench(sample_count = 10)]
fn build_5000(b: divan::Bencher) {
    let v = vault();
    b.bench(|| BaseIndex::build(divan::black_box(v)));
}
#[divan::bench(sample_count = 20)]
fn query_sort_5000(b: divan::Bencher) {
    let index = BaseIndex::build(vault());
    let d = BaseDefinition {
        filters: vec![BaseFilter::Tag("trabajo".into())],
        sort: Some(BaseSort {
            property: "valor".into(),
            descending: true,
        }),
        ..Default::default()
    };
    b.bench(|| index.query(divan::black_box(&d)));
}
#[divan::bench(sample_count = 20)]
fn update_one_5000(b: divan::Bencher) {
    let v = vault();
    let mut index = BaseIndex::build(v);
    let path = orbital_vault::RelPath::new("nota-42.md").unwrap();
    b.bench_local(|| index.update(v, divan::black_box(&path)));
}
