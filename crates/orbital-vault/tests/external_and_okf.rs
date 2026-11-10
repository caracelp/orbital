//! T070 (cambios externos) y T076 (conformidad con OKF v0.2 §11, SC-003).

mod common;

use std::fs;
use std::path::Path;

use common::{TempDir, copy_tree, ctx, fixture};
use orbital_vault::okf_files::line_link;
use orbital_vault::props::markdown;
use orbital_vault::props::yaml::parse_block;
use orbital_vault::{ExternalChange, Format, RelPath, VaultConfig};

fn rel(s: &str) -> RelPath {
    RelPath::new(s).unwrap()
}

#[test]
fn external_changes_are_detected() {
    let t = TempDir::new("externo");
    copy_tree(&fixture("completa"), t.path());
    let v = orbital_vault::open(t.path()).unwrap();
    let root = t.path();
    fs::write(root.join("wiki/nueva.md"), "---\ntype: atomo\n---\n").unwrap();
    fs::write(
        root.join("wiki/fisica/entropia.md"),
        "cambiada por otro editor\n",
    )
    .unwrap();
    fs::remove_file(root.join("wiki/termodinamica.html")).unwrap();
    fs::create_dir_all(root.join(".orbital")).unwrap();
    fs::write(root.join(".orbital/config.yaml"), "format_version: 1\n").unwrap();
    let paths: Vec<_> = [
        "wiki/nueva.md",
        "wiki/fisica/entropia.md",
        "wiki/termodinamica.html",
        ".orbital/config.yaml",
        "wiki/index.md",
    ]
    .iter()
    .map(|p| root.join(p))
    .collect();
    let changes = v.on_external_change(&paths);
    assert_eq!(
        changes,
        vec![
            ExternalChange::Added(rel("wiki/nueva.md")),
            ExternalChange::Modified(rel("wiki/fisica/entropia.md")),
            ExternalChange::Removed(rel("wiki/termodinamica.html")),
        ],
        "lo oculto y lo que no cambió se ignoran"
    );
    // Después de volver a leer la bóveda, ya no hay nada que reportar.
    let v2 = orbital_vault::open(root).unwrap();
    assert!(v2.on_external_change(&paths).is_empty());
}

#[test]
fn config_is_saved_atomically() {
    let t = TempDir::new("config");
    let c = VaultConfig {
        link_style: orbital_vault::LinkStyle::Markdown,
        journal_name: "diario/AAAA-MM-DD".into(),
        ..VaultConfig::default()
    };
    c.save(t.path()).unwrap();
    assert_eq!(VaultConfig::load(t.path()).0, c);
}

/// Verifica OKF v0.2 §11 sobre una carpeta `wiki/`.
fn assert_okf_conformant(wiki: &Path) {
    fn walk(dir: &Path, out: &mut Vec<std::path::PathBuf>) {
        for e in fs::read_dir(dir).unwrap() {
            let p = e.unwrap().path();
            let hidden = p.file_name().unwrap().to_string_lossy().starts_with('.');
            if hidden {
                continue;
            }
            if p.is_dir() {
                walk(&p, out);
            } else {
                out.push(p);
            }
        }
    }
    let mut files = Vec::new();
    walk(wiki, &mut files);
    for f in files
        .iter()
        .filter(|f| f.extension().is_some_and(|e| e == "md"))
    {
        let text = fs::read_to_string(f).unwrap();
        let name = f.file_name().unwrap().to_string_lossy();
        match name.as_ref() {
            // §8: sin frontmatter (salvo okf_version en la raíz); secciones con `* [t](u)`.
            "index.md" => {
                for line in text.lines().filter(|l| l.starts_with("* ")) {
                    assert!(
                        line_link(line).is_some(),
                        "{f:?}: entrada mal formada: {line}"
                    );
                }
            }
            // §9: encabezados de fecha AAAA-MM-DD.
            "log.md" => {
                for line in text.lines().filter(|l| l.starts_with("## ")) {
                    let date = &line[3..];
                    assert!(
                        orbital_vault::props::yaml::is_date(date),
                        "{f:?}: fecha inválida {date}"
                    );
                }
            }
            _ => {
                let region =
                    markdown::locate(&text).unwrap_or_else(|| panic!("{f:?}: sin frontmatter"));
                let entries = parse_block(&text[region]).unwrap();
                let ty = entries
                    .iter()
                    .find(|e| e.key == "type")
                    .and_then(|e| e.value.as_text());
                assert!(ty.is_some_and(|t| !t.is_empty()), "{f:?}: sin type");
            }
        }
    }
}

#[test]
fn wiki_made_with_orbital_is_okf_conformant() {
    let t = TempDir::new("okf");
    let plan = orbital_vault::plan_open_as(t.path()).unwrap();
    let v = orbital_vault::apply_open_as(t.path(), &plan, &ctx()).unwrap();
    let c = ctx();
    let a = v
        .create_note(&rel("wiki"), "Calor", Format::Md, &c)
        .unwrap();
    v.create_note(&rel("wiki"), "Panorama", Format::Html, &c)
        .unwrap();
    v.open_journal(&c).unwrap();
    let v = orbital_vault::open(t.path()).unwrap();
    v.rename(&a, &rel("wiki/fisica/calor.md"), &c).unwrap();
    v.archive(&rel("wiki/fisica/calor.md"), true, &c).unwrap();
    assert_okf_conformant(&t.path().join("wiki"));
}

/// Con `ORBITAL_OKF_DIR=<bóveda>` verifica una bóveda cualquiera (por ejemplo, una generada).
#[test]
fn optional_external_vault_is_okf_conformant() {
    if let Ok(dir) = std::env::var("ORBITAL_OKF_DIR") {
        assert_okf_conformant(&Path::new(&dir).join("wiki"));
    }
}
