mod common;

use common::{TempDir, ctx};
use orbital_vault::{RelPath, Vault};
use std::fs;

fn rel(s: &str) -> RelPath {
    RelPath::new(s).unwrap()
}

fn fixture(files: &[(&str, &str)]) -> (TempDir, Vault, Vec<RelPath>) {
    let tmp = TempDir::new("rename-tags");
    let mut paths = Vec::new();
    for (path, text) in files {
        let dest = tmp.path().join(path);
        fs::create_dir_all(dest.parent().unwrap()).unwrap();
        fs::write(dest, text).unwrap();
        paths.push(rel(path));
    }
    let vault = orbital_vault::open(tmp.path()).unwrap();
    (tmp, vault, paths)
}

#[test]
fn rename_parent_and_children_in_body_and_props_then_restore_exact_bytes() {
    let md = "\u{feff}---\r\ntitle: Intacto #comentario\r\ntags: [Física, Física/Termodinámica, otra]\r\n---\r\n# Título\r\n#FÍSICA #Física/Hija #fisicas #ciencia/fisica\r\n`#fisica` [#fisica](nota) [[nota#fisica]]\r\n\r\n```\r\n#fisica\r\n```\r\n";
    let html = "<!doctype html><html><head><script type=\"application/yaml\">\ntags: 'Física; otra'\ntitle: Conservar\n</script></head><body>İ<p>#física/hija #fisicas</p><code>#fisica</code><a href=\"#fisica\">#fisica</a></body></html>";
    let (tmp, vault, paths) = fixture(&[("wiki/a.md", md), ("wiki/b.html", html)]);
    let plan = vault.plan_tag_rename("fisica", "ciencia/física", &paths);
    assert!(plan.errors.is_empty(), "{:?}", plan.errors);
    assert_eq!(plan.files(), 2);
    assert_eq!(
        fs::read_to_string(tmp.path().join("wiki/a.md")).unwrap(),
        md
    );
    let (receipt, failed) = vault.apply_edits(&plan, &ctx());
    assert!(failed.is_empty(), "{failed:?}");
    let renamed_md = fs::read_to_string(tmp.path().join("wiki/a.md")).unwrap();
    assert!(
        renamed_md.contains("#ciencia/física #ciencia/física/Hija #fisicas #ciencia/fisica"),
        "{renamed_md}"
    );
    assert!(renamed_md.contains("`#fisica` [#fisica](nota) [[nota#fisica]]"));
    assert!(renamed_md.contains("```\r\n#fisica\r\n```"));
    assert!(renamed_md.contains("title: Intacto #comentario\r\n"));
    let renamed_html = fs::read_to_string(tmp.path().join("wiki/b.html")).unwrap();
    assert!(renamed_html.contains("#ciencia/física/hija #fisicas"));
    assert!(renamed_html.contains("<code>#fisica</code><a href=\"#fisica\">#fisica</a>"));
    let applied = plan.applied(&receipt);
    let (_, failed) = vault.revert_edits(&applied, &ctx());
    assert!(failed.is_empty(), "{failed:?}");
    assert_eq!(
        fs::read(tmp.path().join("wiki/a.md")).unwrap(),
        md.as_bytes()
    );
    assert_eq!(
        fs::read(tmp.path().join("wiki/b.html")).unwrap(),
        html.as_bytes()
    );
}

#[test]
fn merge_existing_destination_and_keep_other_tags_and_inline_contexts() {
    let original = "---\ntags: [fisica, ciencia, fisica/hija, fisicas]\n---\n#fisica #ciencia #fisicas\n\n    #fisica\n\n[#fisica][ref]\n\n[ref]: nota\n";
    let (tmp, vault, paths) = fixture(&[("wiki/a.md", original)]);
    let plan = vault.plan_tag_rename("fisica", "ciencia", &paths);
    let (receipt, failed) = vault.apply_edits(&plan, &ctx());
    assert!(failed.is_empty());
    let text = fs::read_to_string(tmp.path().join("wiki/a.md")).unwrap();
    assert!(text.contains("#ciencia #ciencia #fisicas"));
    assert!(text.contains("    #fisica"));
    assert!(text.contains("[#fisica][ref]"));
    let parsed = orbital_vault::extract_tags(&text, false);
    assert!(parsed.iter().any(|t| t.tag == "ciencia/hija"));
    let note = vault.read_note(&rel("wiki/a.md")).unwrap();
    assert_eq!(
        note.prop("tags"),
        Some(&orbital_vault::props::PropValue::List(
            ["ciencia", "ciencia/hija", "fisicas"]
                .into_iter()
                .map(|name| orbital_vault::props::PropValue::Text(name.into()))
                .collect()
        ))
    );
    vault.revert_edits(&plan.applied(&receipt), &ctx());
    assert_eq!(
        fs::read_to_string(tmp.path().join("wiki/a.md")).unwrap(),
        original
    );
}

#[test]
fn merge_preserves_unrelated_duplicates_and_accepts_final_hyphen() {
    let (tmp, vault, paths) = fixture(&[(
        "wiki/a.md",
        "---\ntags: [vieja, otra, otra, nueva-]\n---\n#vieja",
    )]);
    let plan = vault.plan_tag_rename("vieja", "nueva-", &paths);
    assert!(plan.errors.is_empty());
    let (_, failed) = vault.apply_edits(&plan, &ctx());
    assert!(failed.is_empty());
    let note = vault.read_note(&paths[0]).unwrap();
    assert_eq!(
        note.prop("tags"),
        Some(&orbital_vault::props::PropValue::List(
            ["nueva-", "otra", "otra"]
                .into_iter()
                .map(|name| orbital_vault::props::PropValue::Text(name.into()))
                .collect()
        ))
    );
    assert!(
        fs::read_to_string(tmp.path().join("wiki/a.md"))
            .unwrap()
            .ends_with("#nueva-")
    );
    assert!(
        orbital_vault::extract_tags(&note.source, false)
            .iter()
            .any(|tag| tag.tag == "nueva-")
    );
}

#[test]
fn partial_application_receipt_limits_undo_and_conflicts_are_preserved() {
    let (tmp, vault, paths) = fixture(&[("wiki/a.md", "#vieja\n"), ("wiki/b.md", "#vieja\n")]);
    let plan = vault.plan_tag_rename("vieja", "nueva", &paths);
    // Externo escribe exactamente el resultado previsto: el apply debe fallar por hash viejo.
    fs::write(tmp.path().join("wiki/b.md"), "#nueva\n").unwrap();
    let (receipt, failed) = vault.apply_edits(&plan, &ctx());
    assert_eq!(receipt.written.len(), 1);
    assert_eq!(failed.len(), 1);
    let applied = plan.applied(&receipt);
    assert_eq!(applied.files(), 1);
    fs::write(tmp.path().join("wiki/a.md"), "#nueva con edición externa\n").unwrap();
    let (undone, failed) = vault.revert_edits(&applied, &ctx());
    assert!(undone.written.is_empty());
    assert_eq!(failed.len(), 1);
    assert_eq!(
        fs::read_to_string(tmp.path().join("wiki/b.md")).unwrap(),
        "#nueva\n"
    );
    assert_eq!(
        fs::read_to_string(tmp.path().join("wiki/a.md")).unwrap(),
        "#nueva con edición externa\n"
    );
}

#[test]
fn reject_invalid_sources_symlinks_and_report_missing_files() {
    use std::os::unix::fs::symlink;
    let (tmp, _vault, paths) = fixture(&[("wiki/a.md", "#vieja"), ("raw/fuente.md", "#vieja")]);
    let outside = TempDir::new("rename-outside");
    fs::write(outside.path().join("nota.md"), "#vieja").unwrap();
    symlink(
        outside.path().join("nota.md"),
        tmp.path().join("wiki/link.md"),
    )
    .unwrap();
    let vault = orbital_vault::open(tmp.path()).unwrap();
    let sources = [
        paths[0].clone(),
        paths[1].clone(),
        rel("wiki/link.md"),
        rel("wiki/no-existe.md"),
    ];
    let plan = vault.plan_tag_rename("vieja", "nueva", &sources);
    assert_eq!(plan.files(), 1);
    assert_eq!(plan.errors.len(), 3);
    // Cambiar el archivo ya planificado por un enlace externo tampoco permite escribir afuera.
    fs::remove_file(tmp.path().join("wiki/a.md")).unwrap();
    symlink(outside.path().join("nota.md"), tmp.path().join("wiki/a.md")).unwrap();
    let (receipt, failed) = vault.apply_edits(&plan, &ctx());
    assert!(receipt.written.is_empty());
    assert_eq!(failed.len(), 1);
    assert_eq!(
        fs::read_to_string(outside.path().join("nota.md")).unwrap(),
        "#vieja"
    );
    drop(vault);
}

#[test]
fn plan_cannot_be_applied_to_another_vault() {
    let (_tmp, vault, paths) = fixture(&[("wiki/a.md", "#vieja")]);
    let plan = vault.plan_tag_rename("vieja", "nueva", &paths);
    let (other, other_vault, _) = fixture(&[("wiki/a.md", "#vieja")]);
    let (receipt, failed) = other_vault.apply_edits(&plan, &ctx());
    assert!(receipt.written.is_empty());
    assert_eq!(failed.len(), 1);
    assert_eq!(
        fs::read_to_string(other.path().join("wiki/a.md")).unwrap(),
        "#vieja"
    );
}

#[test]
#[ignore = "medición release: cargo test --release -p orbital-vault --test rename_tags rename_200_notes -- --ignored --nocapture"]
fn rename_200_notes() {
    let tmp = TempDir::new("rename-tags-200");
    fs::create_dir(tmp.path().join("wiki")).unwrap();
    let original = "---\ntags: [Física, Física/Termodinámica, otra]\n---\nTexto #física #física/hija #fisicas y `#física`.\n";
    let paths: Vec<_> = (0..200)
        .map(|number| {
            let path = rel(&format!("wiki/nota-{number:03}.md"));
            fs::write(path.to_path(tmp.path()), original).unwrap();
            path
        })
        .collect();
    let vault = orbital_vault::open(tmp.path()).unwrap();
    let started = std::time::Instant::now();
    let plan = vault.plan_tag_rename("fisica", "ciencia/fisica", &paths);
    let planned = started.elapsed();
    let (receipt, failed) = vault.apply_edits(&plan, &ctx());
    let elapsed = started.elapsed();
    assert!(plan.errors.is_empty());
    assert!(failed.is_empty());
    assert_eq!(receipt.written.len(), 200);
    println!("Renombrar 200 notas: plan {planned:?}, plan+GuardadoSeguro {elapsed:?}");
    let (_, failed) = vault.revert_edits(&plan.applied(&receipt), &ctx());
    assert!(failed.is_empty());
    for path in paths {
        assert_eq!(
            fs::read(path.to_path(tmp.path())).unwrap(),
            original.as_bytes()
        );
    }
    assert!(
        elapsed < std::time::Duration::from_secs(2),
        "objetivo SC-005: {elapsed:?}"
    );
}
