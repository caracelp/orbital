mod common;
use common::{TempDir, ctx};
use orbital_vault::{RelPath, VaultError, bases::*, props::PropValue};
use std::fs;
fn path(s: &str) -> RelPath {
    RelPath::new(s).unwrap()
}
fn fixture() -> TempDir {
    let d = TempDir::new("bases");
    fs::create_dir_all(d.path().join("wiki/proyectos")).unwrap();
    fs::write(d.path().join("wiki/proyectos/a.md"),"---\ntitle: Uno\nprecio: 10\nestado: activo\ntags: [trabajo]\n---\nCuerpo intacto\ninline:: valor\n").unwrap();
    fs::write(d.path().join("wiki/proyectos/b.html"),"<html><head><script type=\"application/yaml\">\ntitle: Dos\nprecio: 2\nestado: activo\n</script></head><body>HTML intacto</body></html>").unwrap();
    d
}
#[test]
fn format_roundtrip_and_reject_unknown() {
    let mut d = BaseDefinition {
        view: BaseView::Cards,
        ..Default::default()
    };
    d.filters.push(BaseFilter::Property {
        key: "estado".into(),
        value: Some(PropValue::Text("a\n\"<b>".into())),
    });
    d.sort = Some(BaseSort {
        property: "precio".into(),
        descending: true,
    });
    assert_eq!(parse(&emit(&d)).unwrap(), d);
    for source in [
        "format_version: 2",
        "format_version: 1\nunknown: x",
        "version: 1",
        "format_version: 1\nview: x",
        "format_version: 1\ncolumns: [{property: a, wat: x}]",
        "format_version: 1\nfilters: [{tag: x, property: a}]",
    ] {
        assert!(parse(source).is_err(), "{source}");
    }
}
#[test]
fn query_filters_sort_and_incremental() {
    let d = fixture();
    let v = orbital_vault::open(d.path()).unwrap();
    let mut index = BaseIndex::build(&v);
    let mut base = BaseDefinition::default();
    base.filters
        .push(BaseFilter::Folder("wiki/proyectos".into()));
    base.sort = Some(BaseSort {
        property: "precio".into(),
        descending: false,
    });
    let rows = index.query(&base).rows;
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].path, path("wiki/proyectos/b.html"));
    assert_eq!(
        rows[1].value("inline"),
        Some(&PropValue::Text("valor".into()))
    );
    assert!(rows[0].value("inline").is_none());
    base.filters.push(BaseFilter::Tag("trabajo".into()));
    assert_eq!(index.query(&base).rows.len(), 1);
    fs::remove_file(d.path().join("wiki/proyectos/a.md")).unwrap();
    index.update(&v, &path("wiki/proyectos/a.md"));
    assert!(index.query(&base).rows.is_empty());
    fs::write(d.path().join("wiki/nueva.md"), "---\nprecio: null\n---\n").unwrap();
    index.update(&v, &path("wiki/nueva.md"));
    assert!(
        index
            .query(&BaseDefinition::default())
            .rows
            .iter()
            .any(|r| r.value("precio") == Some(&PropValue::Null))
    );
}
#[test]
fn safe_cell_md_html_and_conflicts() {
    let d = fixture();
    let v = orbital_vault::open(d.path()).unwrap();
    for p in [path("wiki/proyectos/a.md"), path("wiki/proyectos/b.html")] {
        let note = v.read_note(&p).unwrap();
        let expected = note
            .with_prop("precio", &PropValue::Number("3".into()))
            .unwrap();
        edit_cell(
            &v,
            &p,
            note.hash,
            "precio",
            Some(&PropValue::Number("3".into())),
            &ctx(),
        )
        .unwrap();
        assert_eq!(fs::read_to_string(p.to_path(d.path())).unwrap(), expected);
        assert!(matches!(
            edit_cell(&v, &p, note.hash, "precio", Some(&PropValue::Null), &ctx()),
            Err(VaultError::Conflict { .. })
        ));
    }
}
#[test]
fn typed_values_dates_and_null() {
    assert!(parse_value(PropertyType::Date, "2024-02-29").is_ok());
    for date in ["2023-02-29", "2024-04-31"] {
        assert!(parse_value(PropertyType::Date, date).is_err());
    }
    assert!(parse_value(PropertyType::Number, "abc").is_err());
    assert!(parse_value(PropertyType::Bool, "false").is_ok());
    assert_eq!(
        parse_value(PropertyType::Text, "false").unwrap(),
        PropValue::Text("false".into())
    );
    assert!(validate_value(PropertyType::Number, &PropValue::Null));
    assert!(parse_value(PropertyType::List, "[a, b]").is_ok());
}
#[test]
fn save_creation_conflict_and_invalid_preserved() {
    let d = fixture();
    let v = orbital_vault::open(d.path()).unwrap();
    let p = path("wiki/vista.base");
    let base = BaseDefinition::default();
    save(&v, &p, &base, None, &ctx()).unwrap();
    assert_eq!(read(&v, &p).unwrap().definition, base);
    assert!(save(&v, &p, &base, None, &ctx()).is_err());
    let doc = read(&v, &p).unwrap();
    fs::write(p.to_path(d.path()), "format_version: 2\n").unwrap();
    assert!(save(&v, &p, &base, Some(doc.hash), &ctx()).is_err());
    assert_eq!(
        fs::read_to_string(p.to_path(d.path())).unwrap(),
        "format_version: 2\n"
    );
    assert!(save(&v, &path("wiki/test.md"), &base, None, &ctx()).is_err());
}
#[test]
fn raw_invalid_and_symlinks() {
    let d = fixture();
    fs::create_dir(d.path().join("raw")).unwrap();
    fs::write(d.path().join("raw/a.md"), "---\nx: 1\n---\n").unwrap();
    fs::write(d.path().join("wiki/bad.md"), "---\nx: [\n---\n").unwrap();
    let v = orbital_vault::open(d.path()).unwrap();
    for p in [path("raw/a.md"), path("wiki/bad.md")] {
        let note = v.read_note(&p).unwrap();
        assert!(edit_cell(&v, &p, note.hash, "x", Some(&PropValue::Null), &ctx()).is_err());
    }
    #[cfg(unix)]
    {
        let outside = TempDir::new("base-outside");
        std::os::unix::fs::symlink(
            outside.path().join("missing.base"),
            d.path().join("wiki/escape.base"),
        )
        .unwrap();
        assert!(
            save(
                &v,
                &path("wiki/escape.base"),
                &BaseDefinition::default(),
                None,
                &ctx()
            )
            .is_err()
        );
        assert!(!outside.path().join("missing.base").exists());
    }
}
#[test]
fn rename_merge_preview_apply_undo_partial() {
    let d = fixture();
    for (name, body) in [
        ("solo", "origen: 1 # comentario\r\n"),
        ("igual", "origen: 1\r\ndestino: 1\r\n"),
        ("conflicto", "origen: 1\r\ndestino: 2\r\n"),
    ] {
        fs::write(
            d.path().join(format!("wiki/{name}.md")),
            format!("---\r\n{body}---\r\nCuerpo\r\n"),
        )
        .unwrap();
    }
    let base = BaseDefinition {
        columns: vec![BaseColumn {
            property: "origen".into(),
            label: "Origen".into(),
            kind: PropertyType::Number,
        }],
        ..Default::default()
    };
    fs::write(d.path().join("wiki/renombre.base"), emit(&base)).unwrap();
    let v = orbital_vault::open(d.path()).unwrap();
    let plan = plan_property_rename(&v, "origen", "destino").unwrap();
    assert_eq!(plan.conflicts.len(), 1);
    assert_eq!(plan.edits.files(), 3);
    assert!(plan.edits.errors.is_empty(), "{:?}", plan.edits.errors);
    let original = fs::read(d.path().join("wiki/solo.md")).unwrap();
    fs::write(d.path().join("wiki/igual.md"), "cambio externo").unwrap();
    let (receipt, errors) = v.apply_edits(&plan.edits, &ctx());
    assert_eq!(errors.len(), 1);
    assert!(
        fs::read_to_string(d.path().join("wiki/solo.md"))
            .unwrap()
            .contains("destino: 1 # comentario\r\n")
    );
    let (_, errors) = v.revert_edits(&plan.edits.applied(&receipt), &ctx());
    assert!(errors.is_empty());
    assert_eq!(fs::read(d.path().join("wiki/solo.md")).unwrap(), original);
    assert_eq!(
        fs::read_to_string(d.path().join("wiki/igual.md")).unwrap(),
        "cambio externo"
    );
    assert!(plan_property_rename(&v, "title", "otro").is_err());
}

#[test]
fn exact_numeric_order_and_datetime_offsets() {
    let d = TempDir::new("bases-sort");
    for (name, value) in [
        ("a", "9007199254740993"),
        ("b", "9007199254740992"),
        ("c", "9007199254740992.000000000000000001"),
    ] {
        fs::write(
            d.path().join(format!("{name}.md")),
            format!("---\nx: {value}\n---\n"),
        )
        .unwrap();
    }
    let v = orbital_vault::open(d.path()).unwrap();
    let mut index = BaseIndex::build(&v);
    let base = BaseDefinition {
        sort: Some(BaseSort {
            property: "x".into(),
            descending: false,
        }),
        ..Default::default()
    };
    assert_eq!(
        index
            .query(&base)
            .rows
            .iter()
            .map(|r| r.path.file_name())
            .collect::<Vec<_>>(),
        vec!["b.md", "c.md", "a.md"]
    );
    fs::write(
        d.path().join("a.md"),
        "---\nx: 2026-01-01T09:00:00-03:00\n---\n",
    )
    .unwrap();
    fs::write(d.path().join("b.md"), "---\nx: 2026-01-01T10:00:00Z\n---\n").unwrap();
    fs::remove_file(d.path().join("c.md")).unwrap();
    for p in ["a.md", "b.md", "c.md"] {
        index.update(&v, &path(p));
    }
    assert_eq!(index.query(&base).rows[0].path, path("b.md"));
    for date in [
        "2026-01-01T25:00:00Z",
        "2026-01-01T10:61:00Z",
        "2026-01-01T10:00:00+99:00",
    ] {
        assert!(parse_value(PropertyType::DateTime, date).is_err(), "{date}");
    }
    assert!(parse_value(PropertyType::Link, "[[nota]]").is_ok());
}

#[test]
fn rename_never_creates_invalid_definitions_or_rewrites_flow_yaml() {
    let d = fixture();
    fs::write(
        d.path().join("wiki/flow.md"),
        "---\n{origen: 1, vecino: 2}\n---\nIntacto\n",
    )
    .unwrap();
    fs::write(d.path().join("wiki/inline.md"), "origen:: uno\n").unwrap();
    let base = BaseDefinition {
        columns: ["origen", "destino"]
            .into_iter()
            .map(|p| BaseColumn {
                property: p.into(),
                label: p.into(),
                kind: PropertyType::Auto,
            })
            .collect(),
        ..Default::default()
    };
    fs::write(d.path().join("wiki/duplicada.base"), emit(&base)).unwrap();
    let v = orbital_vault::open(d.path()).unwrap();
    let plan = plan_property_rename(&v, "origen", "destino").unwrap();
    assert!(
        plan.conflicts
            .iter()
            .any(|(p, _)| p.file_name() == "duplicada.base")
    );
    assert!(
        plan.edits
            .errors
            .iter()
            .any(|(p, _)| p.file_name() == "flow.md")
    );
    assert!(
        plan.edits
            .errors
            .iter()
            .any(|(p, _)| p.file_name() == "inline.md")
    );
    assert!(plan.edits.changes.is_empty());
}

#[test]
fn reload_transfers_hot_index_and_reconciles_new_modified_deleted_notes() {
    let d = fixture();
    let v = orbital_vault::open(d.path()).unwrap();
    assert_eq!(v.base_snapshot(&BaseDefinition::default()).rows.len(), 2);
    fs::remove_file(d.path().join("wiki/proyectos/a.md")).unwrap();
    fs::write(
        d.path().join("wiki/nueva.md"),
        "---\nprecio: 2.0\n---\nNueva",
    )
    .unwrap();
    let mut next = orbital_vault::open(d.path()).unwrap();
    next.inherit_base_index(&v);
    let b = BaseDefinition {
        filters: vec![BaseFilter::Property {
            key: "precio".into(),
            value: Some(PropValue::Number("2".into())),
        }],
        ..Default::default()
    };
    let rows = next.base_snapshot(&b).rows;
    assert_eq!(rows.len(), 2);
    assert!(rows.iter().any(|r| r.path.file_name() == "nueva.md"));
    assert!(!rows.iter().any(|r| r.path.file_name() == "a.md"));
}

#[test]
fn invalid_and_extra_yaml_documents_cannot_be_saved() {
    for source in [
        "format_version: 1\n---\nother: true",
        "format_version: 1\ncolumns: []",
        "format_version: 1\nformat_version: 1",
        "format_version: 1\ncolumns: [{property: ' '}]",
    ] {
        assert!(parse(source).is_err(), "{source}");
    }
}

#[test]
fn flow_cells_and_equal_merge_preserve_other_values() {
    let d = TempDir::new("bases-flow-conservation");
    let source = "---\n{origen: 1, destino: 1, otra: conservar}\n---\nCuerpo intacto\n";
    fs::write(d.path().join("flow.md"), source).unwrap();
    let v = orbital_vault::open(d.path()).unwrap();
    let p = path("flow.md");
    let note = v.read_note(&p).unwrap();
    for value in [None, Some(PropValue::Number("2".into()))] {
        assert!(edit_cell(&v, &p, note.hash, "origen", value.as_ref(), &ctx()).is_err());
        assert_eq!(fs::read_to_string(p.to_path(d.path())).unwrap(), source);
    }
    let plan = plan_property_rename(&v, "origen", "destino").unwrap();
    assert!(plan.edits.changes.is_empty());
    assert_eq!(plan.edits.errors.len(), 1);
    assert_eq!(fs::read_to_string(p.to_path(d.path())).unwrap(), source);
}
#[test]
fn rename_respects_effective_inline_destination_and_origin() {
    let d = TempDir::new("bases-inline-conflicts");
    fs::write(
        d.path().join("conflict.md"),
        "---\norigen: uno\n---\ndestino:: dos\n",
    )
    .unwrap();
    fs::write(
        d.path().join("resurface.md"),
        "---\norigen: uno\n---\norigen:: anterior\n",
    )
    .unwrap();
    let v = orbital_vault::open(d.path()).unwrap();
    let plan = plan_property_rename(&v, "origen", "destino").unwrap();
    assert!(plan.edits.changes.is_empty());
    assert_eq!(plan.conflicts.len(), 1);
    assert_eq!(plan.edits.errors.len(), 1);
    assert_eq!(plan.conflicts[0].0, path("conflict.md"));
    assert_eq!(plan.edits.errors[0].0, path("resurface.md"));
}
#[test]
fn numeric_sort_with_unsupported_exponent_stays_total() {
    let d = TempDir::new("bases-order-total");
    for (name, value) in [("a", "2"), ("b", "10"), ("c", "1e999999999999999999999")] {
        fs::write(
            d.path().join(format!("{name}.md")),
            format!("---\nx: {value}\n---\n"),
        )
        .unwrap();
    }
    let v = orbital_vault::open(d.path()).unwrap();
    let index = BaseIndex::build(&v);
    let mut base = BaseDefinition {
        sort: Some(BaseSort {
            property: "x".into(),
            descending: false,
        }),
        ..Default::default()
    };
    assert_eq!(
        index
            .query(&base)
            .rows
            .iter()
            .map(|r| r.path.file_name())
            .collect::<Vec<_>>(),
        vec!["a.md", "b.md", "c.md"]
    );
    base.sort.as_mut().unwrap().descending = true;
    assert_eq!(
        index
            .query(&base)
            .rows
            .iter()
            .map(|r| r.path.file_name())
            .collect::<Vec<_>>(),
        vec!["c.md", "b.md", "a.md"]
    );
}
