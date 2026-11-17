//! Pruebas unitarias y de integración para la navegación de enlaces y encabezados (US1).

use gtk::glib;
use orbital_vault::links::{LinkKind, extract_links, parse_link_target};

use super::*;

#[test]
#[ignore = "requiere acceso GTK; tools/verificar-004-us5.sh"]
fn us5_tag_widgets_reader() {
    gtk::init().expect("el proceso necesita conectarse a una sesión GTK");
    let context = glib::MainContext::default();
    context.block_on(async {
        let reader = Reader::new();
        let chosen = Rc::new(RefCell::new(String::new()));
        let got = chosen.clone();
        reader.set_on_link(move |url| *got.borrow_mut() = url.into());
        reader.render(
            Prepared::new("😀 #Física y `#código` #123 #ff00ff", &[]),
            || None,
            None,
        );
        for _ in 0..100 {
            if !reader.all_link_items().is_empty() {
                break;
            }
            glib::timeout_future(Duration::from_millis(10)).await;
        }
        let items = reader.all_link_items();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].url, "orbital-tag:Física");
        reader.set_focused_link(Some(0));
        assert!(reader.activate_focused_link());
        assert_eq!(*chosen.borrow(), "orbital-tag:Física");
    });
}

#[test]
fn us1_parse_and_extract_links() {
    let raw = "nota-b#Mi Seccion|Mi Alias";
    let target = parse_link_target(raw);
    assert_eq!(target.path, "nota-b");
    assert_eq!(target.anchor.as_deref(), Some("Mi Seccion"));
    assert_eq!(target.alias.as_deref(), Some("Mi Alias"));

    let md = "# Titulo\n\nVer [[otra-nota#Subseccion]] y [sitio](https://gnome.org).";
    let occs = extract_links(md, false);
    assert_eq!(occs.len(), 2);
    assert_eq!(occs[0].kind, LinkKind::Wikilink);
    assert_eq!(occs[0].target.path, "otra-nota");
    assert_eq!(occs[0].target.anchor.as_deref(), Some("Subseccion"));
    assert_eq!(occs[1].kind, LinkKind::Markdown);
    assert_eq!(occs[1].target.path, "https://gnome.org");
}

#[test]
#[ignore = "requiere una sesión gráfica; ejecutar tools/verificar-us1.sh"]
fn us1_reader_scrolling_and_links() {
    glib::set_prgname(Some("orbital"));
    glib::set_application_name("Orbital");
    adw::init().expect("Se necesita acceso a una sesión gráfica GTK");
    let context = glib::MainContext::default();

    context.block_on(async {
        let reader = Reader::new();
        let markdown = "\
# Introducción

Primer párrafo con un enlace a [[nota-b]] y otro [web](https://gnome.org).

# Mi Encabezado

Texto de la sección de destino.
";
        let prepared = Prepared::new(markdown, &[]);
        reader.render(prepared, || None, None);

        // Esperar a que los bloques se vuelquen en el idle loop
        let mut iters = 0;
        while reader.0.blocks.borrow().is_empty() && iters < 50 {
            context.iteration(true);
            iters += 1;
        }

        // 1. Desplazamiento a encabezados
        assert!(reader.scroll_to_heading("Mi Encabezado"));
        assert!(reader.scroll_to_heading("Introducción"));
        assert!(!reader.scroll_to_heading("Sección Inexistente"));

        // 2. Extracción de enlaces en orden de documento
        let items = reader.all_link_items();
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].url, "[[nota-b]]");
        assert_eq!(items[1].url, "https://gnome.org");

        // 3. Foco por teclado en enlaces
        assert_eq!(reader.0.focused_link.get(), None);
        reader.set_focused_link(Some(0));
        assert_eq!(reader.0.focused_link.get(), Some(0));
        reader.set_focused_link(Some(1));
        assert_eq!(reader.0.focused_link.get(), Some(1));
        reader.set_focused_link(None);
        assert_eq!(reader.0.focused_link.get(), None);
    });
}

#[test]
#[ignore = "requiere una sesión gráfica; ejecutar tools/verificar-us2.sh"]
fn us2_reader_missing_link_styling() {
    glib::set_prgname(Some("orbital"));
    glib::set_application_name("Orbital");
    adw::init().expect("Se necesita acceso a una sesión gráfica GTK");
    let context = glib::MainContext::default();

    context.block_on(async {
        let reader = Reader::new();
        reader.set_is_missing(|url| url.contains("pendiente"));

        let markdown = "Enlace a [[nota-existente]] y a [[nota-pendiente]].";
        let prepared = Prepared::new(markdown, &[]);
        reader.render(prepared, || None, None);

        let mut iters = 0;
        while reader.0.blocks.borrow().is_empty() && iters < 50 {
            context.iteration(true);
            iters += 1;
        }

        let items = reader.all_link_items();
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].url, "[[nota-existente]]");
        assert_eq!(items[1].url, "[[nota-pendiente]]");

        // Verificar que el segundo enlace tiene la etiqueta "link-missing"
        let buf = reader.buffer();
        let it = buf.iter_at_mark(&items[1].start);
        let tag_names: Vec<String> = it
            .tags()
            .into_iter()
            .filter_map(|t| t.name())
            .map(|s| s.to_string())
            .collect();
        assert!(tag_names.contains(&"link-missing".to_string()));
    });
}

#[test]
fn us2_missing_note_creation_and_config() {
    let root = std::env::temp_dir().join(format!("orbital-us2-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("wiki")).expect("crear directorio temporal");

    let vault = orbital_vault::open(&root).unwrap();
    assert!(vault.config().ask_create_missing_notes);

    // 1. Crear nota atómicamente en wiki/
    let dir = orbital_vault::RelPath::new("wiki").unwrap();
    let ctx = crate::write_ctx::now(Some(vault.config()));
    let created_path = vault
        .create_note(&dir, "mi-nota-nueva", orbital_vault::Format::Md, &ctx)
        .expect("crear nota pendiente");
    assert_eq!(created_path.as_str(), "wiki/mi-nota-nueva.md");

    // 2. Verificar contenido y frontmatter
    let note = vault.read_note(&created_path).expect("leer nota creada");
    assert_eq!(note.format, orbital_vault::Format::Md);
    assert!(note.source.contains("type: atomo"));
    assert!(note.source.contains("title: mi-nota-nueva"));

    // 3. Verificar persistencia de «No volver a preguntar» en config.yaml
    let mut cfg = vault.config().clone();
    cfg.ask_create_missing_notes = false;
    cfg.save(&root).expect("guardar configuración");

    let (loaded_cfg, _) = orbital_vault::VaultConfig::load(&root);
    assert!(!loaded_cfg.ask_create_missing_notes);

    // 4. Volver a activar la confirmación previa (Preferencias)
    let mut cfg_restored = loaded_cfg;
    cfg_restored.ask_create_missing_notes = true;
    cfg_restored.save(&root).expect("restaurar preferencia");

    let (final_cfg, _) = orbital_vault::VaultConfig::load(&root);
    assert!(final_cfg.ask_create_missing_notes);

    let _ = std::fs::remove_dir_all(&root);
}

/// 005 T009: la Lectura dibuja el valor interpolado y la casilla sigue mapeando al fuente.
#[test]
#[ignore = "requiere acceso GTK; tools/verificar-005-us1.sh"]
fn us5_markers_reader_shows_value() {
    gtk::init().expect("el proceso necesita conectarse a una sesión GTK");
    let context = glib::MainContext::default();
    context.block_on(async {
        let text = "estado:: activo\n\n- [ ] Tarea [%estado] y rótulo [%ausente]\n";
        let yaml = orbital_vault::props::yaml::parse_block("estado: activo").unwrap();
        let reader = Reader::new();
        reader.render(Prepared::new(text, &yaml), || None, None);
        for _ in 0..100 {
            if reader
                .buffer()
                .text(
                    &reader.buffer().start_iter(),
                    &reader.buffer().end_iter(),
                    false,
                )
                .contains("activo")
            {
                break;
            }
            glib::timeout_future(Duration::from_millis(10)).await;
        }
        let visible = reader
            .buffer()
            .text(
                &reader.buffer().start_iter(),
                &reader.buffer().end_iter(),
                false,
            )
            .to_string();
        assert!(
            visible.contains("Tarea activo y rótulo «faltante»"),
            "{visible:?}"
        );
        assert!(!visible.contains("[%estado]"));
        assert!(!visible.contains("[%ausente]"));
    });
}

/// 005 T015/T016/T017: el codeblock `indice` lista las notas del tag anidadas, las entradas
/// activan (abren la nota) y leer no escribe en el disco.
#[test]
#[ignore = "requiere acceso GTK; tools/verificar-005-us3.sh"]
fn us5_indice_reader_lists_and_activates() {
    gtk::init().expect("el proceso necesita conectarse a una sesión GTK");
    let context = glib::MainContext::default();
    context.block_on(async {
        use std::{fs, sync::Arc};
        let root =
            std::env::temp_dir().join(format!("orbital-us5i-{}", glib::uuid_string_random()));
        fs::create_dir_all(root.join("wiki")).unwrap();
        fs::write(root.join("wiki/a.md"), "#fisica\nTexto de A.\n").unwrap();
        fs::write(root.join("wiki/b.md"), "#fisica/termo\nTexto de B.\n").unwrap();
        let nota = root.join("wiki/principal.md");
        let fuente = "```indice\ntag: #fisica\n```\n";
        fs::write(&nota, fuente).unwrap();
        let hash_antes = orbital_vault::write::hash(fs::read(&nota).unwrap().as_slice());
        let path = orbital_vault::RelPath::new("wiki/principal.md").unwrap();
        let vault = Arc::new(orbital_vault::open(&root).unwrap());
        let index = orbital_vault::SearchIndex::build(&vault);

        let mut prepared = Prepared::new(fuente, &[]);
        assert!(complete_generated(
            &mut prepared.doc,
            &mut prepared.hashes,
            Some(&index),
            Some(&path),
        ));

        let reader = Reader::new();
        let elegida = Rc::new(RefCell::new(String::new()));
        let got = elegida.clone();
        reader.set_on_link(move |url| *got.borrow_mut() = url.into());
        reader.render(prepared, || None, None);
        for _ in 0..100 {
            let visible = reader
                .buffer()
                .text(
                    &reader.buffer().start_iter(),
                    &reader.buffer().end_iter(),
                    false,
                )
                .to_string();
            if visible.contains("a.md") && visible.contains("#termo") {
                break;
            }
            glib::timeout_future(Duration::from_millis(10)).await;
        }
        let visible = reader
            .buffer()
            .text(
                &reader.buffer().start_iter(),
                &reader.buffer().end_iter(),
                false,
            )
            .to_string();
        assert!(visible.contains("a.md"), "{visible:?}");
        assert!(visible.contains("b.md"), "{visible:?}");
        assert!(visible.contains("#termo"), "{visible:?}");
        assert!(!visible.contains("```"), "{visible:?}");

        // T016: elegir la primera entrada abre la nota (url /a.md desde la raíz de wiki/).
        let items = reader.all_link_items();
        assert_eq!(
            items.len(),
            3,
            "{:?}\"",
            items.iter().map(|i| i.url.clone()).collect::<Vec<_>>()
        );
        reader.set_focused_link(Some(0));
        assert!(reader.activate_focused_link());
        assert_eq!(*elegida.borrow(), "/a.md");

        // T017: leer no escribe; el archivo queda byte a byte.
        assert_eq!(
            hash_antes,
            orbital_vault::write::hash(fs::read(&nota).unwrap().as_slice())
        );
        fs::remove_dir_all(root).unwrap();
    });
}

/// 005 T015/T020 (TOC): los encabezados del doc se listan anidados, con enlaces al ancla y
/// activables: elegir una entrada manda la URL del ancla (el scroll lo hace `open_link`).
#[test]
#[ignore = "requiere acceso GTK; tools/verificar-005-us4.sh"]
fn us5_toc_reader_lists_headings() {
    gtk::init().expect("el proceso necesita conectarse a una sesión GTK");
    let context = glib::MainContext::default();
    context.block_on(async {
        let text = "# Principal\n\n```toc\n```\n\n## Primera\n\n### Adentro\n\n## Segunda\n";
        let path = orbital_vault::RelPath::new("wiki/principal.md").unwrap();
        let mut prepared = Prepared::new(text, &[]);
        assert!(complete_generated(
            &mut prepared.doc,
            &mut prepared.hashes,
            None,
            Some(&path),
        ));
        let reader = Reader::new();
        let elegida = Rc::new(RefCell::new(String::new()));
        let got = elegida.clone();
        reader.set_on_link(move |url| *got.borrow_mut() = url.into());
        reader.render(prepared, || None, None);
        for _ in 0..100 {
            let visible = reader
                .buffer()
                .text(
                    &reader.buffer().start_iter(),
                    &reader.buffer().end_iter(),
                    false,
                )
                .to_string();
            if visible.contains("Primera") && visible.contains("Adentro") {
                break;
            }
            glib::timeout_future(Duration::from_millis(10)).await;
        }
        let visible = reader
            .buffer()
            .text(
                &reader.buffer().start_iter(),
                &reader.buffer().end_iter(),
                false,
            )
            .to_string();
        assert!(visible.contains("Primera"), "{visible:?}");
        assert!(visible.contains("Adentro"), "{visible:?}");
        assert!(visible.contains("Segunda"), "{visible:?}");
        // Los enlaces llevan a la propia nota con el ancla del encabezado.
        let items = reader.all_link_items();
        assert!(
            items.iter().any(|i| i.url == "/principal.md#Primera"),
            "{:?}",
            items.iter().map(|i| i.url.clone()).collect::<Vec<_>>()
        );

        // T020: activar la entrada «Primera» del TOC manda la URL del ancla; el scroll a la
        // sección lo hace Editor::scroll_to_heading vía window.open_link. La primera entrada
        // de la lista es el propio «Principal» (nivel 1, dentro del rango por defecto).
        let idx = items
            .iter()
            .position(|i| i.url == "/principal.md#Primera")
            .expect("la entrada Primera no está en el TOC");
        reader.set_focused_link(Some(idx));
        assert!(reader.activate_focused_link());
        assert_eq!(*elegida.borrow(), "/principal.md#Primera");

        // Escenario 4.2: minLevel/maxLevel dejan fuera el nivel 1 de la lista; el encabezado
        // de nivel 1 sigue visible en el cuerpo. (El escalar YAML «2» llega como Number.)
        let texto_filtro =
            "# Topo\n\n```toc\nminLevel: 2\nmaxLevel: 3\n```\n\n## Segundo\n\n### Profundo\n";
        let mut filtro = Prepared::new(texto_filtro, &[]);
        assert!(complete_generated(
            &mut filtro.doc,
            &mut filtro.hashes,
            None,
            Some(&path)
        ));
        let lector_filtro = Reader::new();
        lector_filtro.render(filtro, || None, None);
        for _ in 0..100 {
            let visible = lector_filtro
                .buffer()
                .text(
                    &lector_filtro.buffer().start_iter(),
                    &lector_filtro.buffer().end_iter(),
                    false,
                )
                .to_string();
            if visible.contains("Profundo") {
                break;
            }
            glib::timeout_future(Duration::from_millis(10)).await;
        }
        let visible = lector_filtro
            .buffer()
            .text(
                &lector_filtro.buffer().start_iter(),
                &lector_filtro.buffer().end_iter(),
                false,
            )
            .to_string();
        assert_eq!(visible.matches("Topo").count(), 1, "{visible:?}");
        assert_eq!(visible.matches("Segundo").count(), 2, "{visible:?}");
        assert!(!visible.contains("```"), "{visible:?}");
    });
}

/// Espera a que el lector termine de volcar el documento.
async fn esperar_volcado(reader: &Reader) {
    for _ in 0..200 {
        if !reader.0.blocks.borrow().is_empty() {
            return;
        }
        glib::timeout_future(Duration::from_millis(10)).await;
    }
    panic!("el lector no volcó el documento");
}

/// Un callout plegable se abre y cierra desde su título (Tab llega a él), sin widgets anclados,
/// y deja su fondo para la capa de dibujo.
#[test]
#[ignore = "requiere acceso GTK; tools/pruebas-gtk.sh"]
fn callout_plegable_se_alterna_desde_el_titulo() {
    gtk::init().expect("el proceso necesita conectarse a una sesión GTK");
    glib::MainContext::default().block_on(async {
        let reader = Reader::new();
        let text = "> [!warning]- Cuidado\n> Oculto.\n\n> [!tip] Abierto\n> Visible.\n";
        reader.render(Prepared::new(text, &[]), || None, None);
        esperar_volcado(&reader).await;
        let items = reader.all_link_items();
        let n = items
            .iter()
            .position(|l| l.url.starts_with(bloques::URL_CALLOUT))
            .expect("el título del callout plegable se recorre con Tab");
        let nombre = items[n].url[bloques::URL_CALLOUT.len()..].to_owned();
        let tag = reader.buffer().tag_table().lookup(&nombre).unwrap();
        assert!(tag.is_invisible(), "empieza plegado (`-`)");
        reader.set_focused_link(Some(n));
        assert!(reader.activate_focused_link());
        assert!(!tag.is_invisible());
        reader.set_focused_link(Some(n));
        assert!(reader.activate_focused_link());
        assert!(tag.is_invisible());
        // Solo uno es plegable; los dos tienen su fondo.
        assert_eq!(
            items
                .iter()
                .filter(|l| l.url.starts_with(bloques::URL_CALLOUT))
                .count(),
            1
        );
        let fondos: usize = reader
            .0
            .blocks
            .borrow()
            .iter()
            .map(|b| {
                b.capas
                    .iter()
                    .filter(|c| matches!(c, capas::Capa::Callout { .. }))
                    .count()
            })
            .sum();
        assert_eq!(fondos, 2);
        assert!(reader.0.pending.borrow().is_empty(), "sin widgets anclados");
    });
}

/// La flechita sugerida con el mouse es la del elemento más interno: el ítem con subítems sobre
/// su texto y la sección del encabezado más cercano en el resto.
#[test]
#[ignore = "requiere acceso GTK; tools/pruebas-gtk.sh"]
fn flecha_sugerida_es_la_del_elemento_mas_interno() {
    gtk::init().expect("el proceso necesita conectarse a una sesión GTK");
    glib::MainContext::default().block_on(async {
        let reader = Reader::new();
        let text = "# A\n\nUno.\n\n## B\n\n- ítem\n  - sub\n- otro\n\nDos.\n";
        let mut prepared = Prepared::new(text, &[]);
        prepared.con_plegables(&orbital_vault::plegado::plegables(text));
        reader.render(prepared, || None, None);
        esperar_volcado(&reader).await;
        let buf = reader.buffer();
        let visible = buf
            .text(&buf.start_iter(), &buf.end_iter(), true)
            .to_string();
        let en = |t: &str| {
            visible
                .find(t)
                .map(|b| visible[..b].chars().count() as i32)
                .unwrap()
        };
        assert_eq!(reader.plegable_en(en("Uno")), Some(0));
        assert_eq!(reader.plegable_en(en("ítem")), Some(6));
        assert_eq!(reader.plegable_en(en("sub")), Some(6));
        assert_eq!(reader.plegable_en(en("otro")), Some(4));
        assert_eq!(reader.plegable_en(en("Dos")), Some(4));
        // La viñeta queda aunque el ítem tenga flechita.
        assert!(visible.contains("•  ítem"), "{visible:?}");
    });
}

/// Como en Obsidian, un salto simple dentro de un párrafo es un salto de línea en Lectura, y la
/// línea siguiente conserva las etiquetas del contexto (la sangría del callout).
#[test]
#[ignore = "requiere acceso GTK; tools/pruebas-gtk.sh"]
fn salto_simple_es_salto_de_linea_con_la_sangria_del_contexto() {
    gtk::init().expect("el proceso necesita conectarse a una sesión GTK");
    glib::MainContext::default().block_on(async {
        let reader = Reader::new();
        let text =
            "> [!abstract] Información general\n> **Asignatura:** Uno\n> **Código:** Dos\n\nA\nB\n";
        reader.render(Prepared::new(text, &[]), || None, None);
        esperar_volcado(&reader).await;
        let buf = reader.buffer();
        let visible = buf
            .text(&buf.start_iter(), &buf.end_iter(), true)
            .to_string();
        assert!(
            visible.contains("Asignatura: Uno\nCódigo: Dos"),
            "{visible:?}"
        );
        assert!(visible.contains("A\nB"), "{visible:?}");
        let en = |t: &str| {
            let b = visible.find(t).unwrap();
            buf.iter_at_offset(visible[..b].chars().count() as i32)
        };
        let sangria = |it: &gtk::TextIter| {
            it.tags()
                .iter()
                .filter_map(|t| t.name())
                .filter(|n| n.starts_with("indent-"))
                .map(|n| n.to_string())
                .collect::<Vec<_>>()
        };
        let primera = sangria(&en("Asignatura"));
        assert!(!primera.is_empty(), "el cuerpo del callout lleva sangría");
        assert_eq!(primera, sangria(&en("Código")));
    });
}

/// Medición (no es prueba de comportamiento): volcar en Lectura una nota de ≈ 100 KB con las
/// cinco partes (041). `cargo test --release --bin orbital medir_lectura_nota_grande -- --ignored
/// --nocapture`. Imprime la mediana de 9 volcados.
#[test]
#[ignore = "medición; correr a mano con --release"]
fn medir_lectura_nota_grande() {
    gtk::init().expect("el proceso necesita conectarse a una sesión GTK");
    let mut text = String::from("---\nid: AT-001\ntype: atomo\n---\n\n");
    text.push_str("<!-- summary AT-001 -->\n## Síntesis\n\nIdea central de la nota, con **negrita** y [[Otra]].\n<!---->\n\n");
    for i in 0..300 {
        text.push_str(&format!(
            "<!-- block {i} AT-001 -->\n## Sección {i}\n\nPárrafo con **negrita**, *cursiva*, `código` y [enlace](/otra.md).\nSegunda línea del párrafo.\n\n- uno\n- [ ] tarea\n  - sub\n\n> [!nota] Título\n> cuerpo\n> más cuerpo\n\n| a | b |\n|---|---|\n| 1 | 2 |\n<!---->\n\n"
        ));
    }
    text.push_str("<!-- sources AT-001 -->\n## Fuentes\n\n");
    for i in 0..20 {
        text.push_str(&format!("- Autor {i}, *Libro {i}*, 2020.\n"));
    }
    text.push_str("<!---->\n\n<!-- connections AT-001 -->\n## Conexiones\n\n");
    for i in 0..40 {
        text.push_str(&format!("- [[Nota {i}]]\n"));
    }
    text.push_str("<!---->\n");
    println!("Nota de {} KB", text.len() / 1024);
    glib::MainContext::default().block_on(async {
        let mut tiempos = Vec::new();
        for _ in 0..9 {
            let reader = Reader::new();
            let t0 = std::time::Instant::now();
            reader.render(Prepared::new(&text, &[]), || None, None);
            while reader.0.blocks.borrow().is_empty() {
                glib::timeout_future(Duration::from_millis(1)).await;
            }
            tiempos.push(t0.elapsed().as_secs_f64() * 1000.0);
        }
        tiempos.sort_by(f64::total_cmp);
        println!(
            "Volcado en Lectura: mediana {:.1} ms (mín {:.1}, máx {:.1})",
            tiempos[4], tiempos[0], tiempos[8]
        );
    });
}

/// 041: las partes especiales se dibujan como estructuras propias (capas, sin widgets): sin los
/// comentarios ni el encabezado que repite el nombre, Fuentes numeradas y cada conexión una
/// píldora que es un enlace (Tab llega a ella).
#[test]
#[ignore = "requiere acceso GTK; tools/pruebas-gtk.sh"]
fn partes_especiales_en_lectura() {
    gtk::init().expect("el proceso necesita conectarse a una sesión GTK");
    glib::MainContext::default().block_on(async {
        let reader = Reader::new();
        let text = "---\nid: AP-001\ntype: apunte\n---\n\n<!-- summary AP-001 -->\n## Síntesis\n\nIdea central.\n<!---->\n\n<!-- block 1 AP-001 -->\n# Cuerpo\n<!---->\n\n<!-- sources AP-001 -->\n## Fuentes\n\n- Apostol\n- Spivak\n<!---->\n\n<!-- connections AP-001 -->\n## Conexiones\n\n- Prerrequisito: [[Límites]]\n- [[Integrales]]\n<!---->\n";
        reader.render(Prepared::new(text, &[]), || None, None);
        esperar_volcado(&reader).await;
        let buf = reader.buffer();
        let visible = buf
            .text(&buf.start_iter(), &buf.end_iter(), false)
            .to_string();
        assert!(!visible.contains("<!--"), "{visible:?}");
        assert!(!visible.contains("SÍNTESIS"), "{visible:?}");
        assert_eq!(visible.matches("Síntesis").count(), 1, "{visible:?}");
        assert_eq!(visible.matches("Fuentes").count(), 1, "{visible:?}");
        assert!(visible.contains("1. Apostol") && visible.contains("2. Spivak"));
        assert!(visible.contains("Prerrequisito"), "{visible:?}");
        let items = reader.all_link_items();
        assert!(items.iter().any(|l| l.url == "[[Límites]]"));
        assert!(items.iter().any(|l| l.url == "[[Integrales]]"));
        let (secciones, pildoras) = reader
            .0
            .blocks
            .borrow()
            .iter()
            .flat_map(|b| b.capas.iter())
            .fold((0, 0), |(s, p), c| match c {
                capas::Capa::Seccion { chips, .. } => (s + 1, p + chips.len()),
                _ => (s, p),
            });
        assert_eq!((secciones, pildoras), (3, 2));
        assert!(reader.0.pending.borrow().is_empty(), "sin widgets anclados");
    });
}
