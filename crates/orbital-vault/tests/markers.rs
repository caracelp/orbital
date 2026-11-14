//! Tests de marcadores `[%clave]`, secciones y resolución (005, T002–T004 y T006).

use orbital_vault::{find_markers, resolve_markers, section_ranges};
use std::ops::Range;

fn claves(text: &str) -> Vec<String> {
    find_markers(text, false)
        .into_iter()
        .map(|m| m.key)
        .collect()
}

fn claves_html(text: &str) -> Vec<String> {
    find_markers(text, true)
        .into_iter()
        .map(|m| m.key)
        .collect()
}

fn rangos(text: &str) -> Vec<Range<usize>> {
    find_markers(text, false)
        .into_iter()
        .map(|m| m.range)
        .collect()
}

#[test]
fn marker_basico_y_varios_por_linea() {
    let text = "Estado: [%estado] y luego [%prioridad].\n";
    assert_eq!(claves(text), ["estado", "prioridad"]);
    let rs = rangos(text);
    assert_eq!(&text[rs[0].clone()], "[%estado]");
    assert_eq!(&text[rs[1].clone()], "[%prioridad]");
}

#[test]
fn marker_en_codigo_y_enlace_excluidos() {
    // Código en línea, cerca y enlace: ninguno cuenta.
    let text = "`[%no]`\n\n```rust\nlet x = \"[%tampoco]\";\n```\n\n[ver](wiki/nota.md) [%si]\n";
    assert_eq!(claves(text), ["si"]);
}

#[test]
fn marker_incompleto_y_con_espacios_no_es_marcador() {
    // Sin `]` en la misma línea, con espacios en la clave y vacío: nada cuenta.
    let text = "[[%sin cerrar en la línea\ncon espacios [%clave con espacios] y vacío [%].";
    assert!(claves(text).is_empty());
}

#[test]
fn marker_con_ventana_unicode() {
    let text = "¡Atención! [%estado] — Güemes ñandú";
    assert_eq!(claves(text), ["estado"]);
    assert_eq!(&text[rangos(text)[0].clone()], "[%estado]");
}

#[test]
fn marcador_escapado_no_cuenta() {
    let text = r"\[%estado] y [%estado]";
    assert_eq!(claves(text), ["estado"]);
}

#[test]
fn marcador_en_frontmatter_no_cuenta() {
    let text = "---\ntitulo: [%no]\n---\n\nCuerpo [%si].\n";
    assert_eq!(claves(text), ["si"]);
}

#[test]
fn marcador_en_html_excluye_code_y_enlace() {
    let text = "<p>[%si]</p><code>[%no]</code><a href='x'>[%tampoco]</a>";
    let out: Vec<String> = find_markers(text, true)
        .into_iter()
        .map(|m| m.key)
        .collect();
    assert_eq!(out, ["si"]);
}

#[test]
fn secciones_con_rangos_de_bytes() {
    let text = "Lead.\n\n# Uno\ntexto uno\n## Uno-uno\nmás\n\n# Dos\nfin\n";
    let ss = section_ranges(text);
    let nombres: Vec<&str> = ss.iter().map(|r| &text[r.clone()]).collect();
    assert_eq!(
        nombres,
        [
            "Lead.\n\n",
            "# Uno\ntexto uno\n",
            "## Uno-uno\nmás\n\n",
            "# Dos\nfin\n"
        ]
    );
}

#[test]
fn secciones_ignoran_hash_en_cerca_y_frontmatter() {
    let text = "---\na: 1\n---\n\n```md\n# no es encabezado\n```\n\n# Real\n\nx\n";
    let ss = section_ranges(text);
    let nombres: Vec<&str> = ss.iter().map(|r| &text[r.clone()]).collect();
    assert_eq!(
        nombres,
        ["\n```md\n# no es encabezado\n```\n\n", "# Real\n\nx\n"]
    );
}

#[test]
fn resuelve_inline_de_la_seccion_primero() {
    let text = "# A\nestado:: activo\nmarcador [%estado]\n\n# B\n[%estado]\n";
    let out = resolve_markers(text, &[]);
    assert_eq!(out.len(), 2);
    assert_eq!(out[0].1, "activo");
    // La sección B no define la clave: faltante.
    assert_eq!(out[1].1, "«faltante»");
}

#[test]
fn resuelve_con_yaml_cuando_la_seccion_no_define() {
    let text = "# A\n[%estado]\n";
    let yaml = orbital_vault::props::yaml::parse_block("estado: pausado").unwrap();
    let out = resolve_markers(text, &yaml);
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].1, "pausado");
}

#[test]
fn clave_repetida_en_la_seccion_gana_el_primero() {
    let text = "# A\nestado:: uno\nestado:: dos\n[%estado]\n";
    let out = resolve_markers(text, &[]);
    assert_eq!(out[0].1, "uno");
}

#[test]
fn yaml_no_pisa_al_inline_de_la_seccion() {
    let text = "# A\nestado:: local\n[%estado]\n\n# B\n[%estado]\n";
    let yaml = orbital_vault::props::yaml::parse_block("estado: global").unwrap();
    let out = resolve_markers(text, &yaml);
    assert_eq!(out[0].1, "local");
    assert_eq!(out[1].1, "global");
}

// --- 005 T010: resolución en notas .html ---

#[test]
fn marcador_en_html_excluye_el_bloque_de_props() {
    let text = "<html><head><script type=\"application/yaml\">\ntitulo: [%no]\n</script></head>\
<body><p>[%si]</p></body></html>";
    assert_eq!(claves_html(text), ["si"]);
}

#[test]
fn resolucion_html_da_pares_clave_valor_sin_duplicar() {
    let text = "<body><p>[%estado] y de nuevo [%estado], luego [%otro].</p></body>";
    let yaml = orbital_vault::props::yaml::parse_block("estado: activo").unwrap();
    let out = orbital_vault::resolve_markers_html(text, &yaml);
    assert_eq!(
        out,
        [
            ("estado".to_owned(), "activo".to_owned()),
            ("otro".to_owned(), "«faltante»".to_owned())
        ]
    );
}

#[test]
fn resolucion_html_prefiere_el_inline_del_cuerpo() {
    let text = "<body><p>estado:: local</p>\n<p>[%estado]</p></body>";
    let yaml = orbital_vault::props::yaml::parse_block("estado: global").unwrap();
    let out = orbital_vault::resolve_markers_html(text, &yaml);
    assert_eq!(out, [("estado".to_owned(), "local".to_owned())]);
}

// --- 005 T013: escenarios de US2 (resolución por sección) ---

/// 2.1: dos secciones con la misma clave y distinto valor; cada marcador muestra el suyo.
#[test]
fn escenario_2_1_misma_clave_distinta_seccion() {
    let text =
        "# A\nestado:: activo\nresultado [%estado]\n\n# B\nestado:: pausado\nresultado [%estado]\n";
    let out = resolve_markers(text, &[]);
    assert_eq!(out[0].1, "activo");
    assert_eq!(out[1].1, "pausado");
}

/// 2.2: sección sin campo inline cae a la propiedad YAML.
#[test]
fn escenario_2_2_seccion_sin_campo_cae_a_yaml() {
    let text = "# Con campo\nestado:: local\n[%estado]\n\n# Sin campo\n[%estado]\n";
    let yaml = orbital_vault::props::yaml::parse_block("estado: global").unwrap();
    let out = resolve_markers(text, &yaml);
    assert_eq!(out[0].1, "local");
    assert_eq!(out[1].1, "global");
}

/// 2.3: un marcador no toma el campo inline de una sección ajena.
#[test]
fn escenario_2_3_no_toma_el_campo_ajeno() {
    let text = "# A\nestado:: de A\n\n# B\n[%estado]\n";
    let out = resolve_markers(text, &[]);
    assert_eq!(out[0].1, "«faltante»");
}

/// Regresión (2026-11-14, `tools/verificar-005-us1.sh`): en una lista, todos los marcadores se
/// resuelven: inline de la sección, YAML y «faltante».
#[test]
fn list_markers_all_resolved() {
    let text = "---\ntype: nota\nestado: pausado\notra: pausado\n---\n# Principal\n\nestado:: activo\n\n- Inline de la sección: [%estado]\n- Solo en YAML: [%otra]\n- Sin valor: [%nada]\n";
    let yaml =
        orbital_vault::props::yaml::parse_block("type: nota\nestado: pausado\notra: pausado")
            .unwrap();
    let repl = orbital_vault::markers::resolve_markers(text, &yaml);
    let values: Vec<&str> = repl.iter().map(|(_, v)| v.as_str()).collect();
    assert_eq!(values, ["activo", "pausado", "«faltante»"]);
    let doc = orbital_vault::render::render_markdown_interpolated(text, &repl);
    let all: String = format!("{doc:?}");
    assert!(
        !all.contains("[%otra]"),
        "quedó [%otra] sin resolver: {all}"
    );
    assert!(all.contains("«faltante»"), "falta «faltante»: {all}");
}
