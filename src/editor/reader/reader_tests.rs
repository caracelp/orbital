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

/// 049: las relaciones con tipo se agrupan por tipo (en el orden en que aparece cada uno), con
/// el nombre del tipo y el motivo como texto visible; las que no tienen tipo siguen como píldoras.
#[test]
#[ignore = "requiere acceso GTK; tools/pruebas-gtk.sh"]
fn conexiones_con_tipo_en_lectura() {
    gtk::init().expect("el proceso necesita conectarse a una sesión GTK");
    glib::MainContext::default().block_on(async {
        let reader = Reader::new();
        let text = "---\nid: AP-001\n---\n\n<!-- connections AP-001 -->\n## Conexiones\n\n- amplía:: [[Derivadas]] — agrega la regla de la cadena\n- contradice a:: [[Límites intuitivos]] - no vale para x→0\n- [[Cálculo]]\n- AMPLIA:: [[Series]], [[Taylor]]\n- refuta:: [[Otra]]\n<!---->\n";
        reader.render(Prepared::new(text, &[]), || None, None);
        esperar_volcado(&reader).await;
        let buf = reader.buffer();
        let visible = buf
            .text(&buf.start_iter(), &buf.end_iter(), false)
            .to_string();
        assert!(!visible.contains("::"), "{visible:?}");
        let pos = |t: &str| visible.find(t).unwrap_or_else(|| panic!("falta {t:?} en {visible:?}"));
        // Un rótulo por tipo, con su nombre canónico; los dos «amplía» van juntos.
        assert_eq!(visible.matches("amplía").count(), 1, "{visible:?}");
        assert!(pos("amplía") < pos("Derivadas"));
        assert!(pos("Derivadas") < pos("Series") && pos("Series") < pos("contradice a"));
        assert!(pos("contradice a") < pos("refuta") && pos("refuta") < pos("Cálculo"));
        assert!(visible.contains("— agrega la regla de la cadena"));
        assert!(visible.contains("— no vale para x→0"));
        let items = reader.all_link_items();
        for url in ["[[Derivadas]]", "[[Series]]", "[[Taylor]]", "[[Cálculo]]", "[[Otra]]"] {
            assert!(items.iter().any(|l| l.url == url), "{url}");
        }
        let pildoras: usize = reader
            .0
            .blocks
            .borrow()
            .iter()
            .flat_map(|b| b.capas.iter())
            .map(|c| match c {
                capas::Capa::Seccion { chips, .. } => chips.len(),
                _ => 0,
            })
            .sum();
        assert_eq!(pildoras, 6);
        assert!(reader.0.pending.borrow().is_empty(), "sin widgets anclados");
    });
}

/// Cuenta las capas de cada clase en el lector.
fn contar_capas(reader: &Reader, f: impl Fn(&capas::Capa) -> bool) -> usize {
    reader
        .0
        .blocks
        .borrow()
        .iter()
        .flat_map(|b| b.capas.iter())
        .filter(|c| f(c))
        .count()
}

/// Las transclusiones (nota entera, encabezado y bloque) se dibujan con capas y sin widgets: la
/// incrustada lleva su encabezado enlazado, sus callouts y tarjetas con su capa y sus bloques
/// generados completos (el índice muestra sus encabezados); su frontmatter no se ve.
#[test]
#[ignore = "requiere acceso GTK; tools/pruebas-gtk.sh"]
fn transclusiones_en_lectura() {
    gtk::init().expect("el proceso necesita conectarse a una sesión GTK");
    let root = std::env::temp_dir().join(format!("orbital-044-{}", glib::uuid_string_random()));
    std::fs::create_dir_all(root.join("wiki")).unwrap();
    std::fs::write(
        root.join("wiki/Origen.md"),
        "---\ntype: atomo\nsecreto: no-se-ve\n---\n```table-of-contents\ntitle: \nstyle: nestedList\ndebugInConsole: false # depuración\n```\n\n## Primera parte\n\nUn párrafo con definición. ^def\n\n> [!warning] Cuidado\n> Texto del callout.\n\n## Segunda parte\n\n:::flashcard id=fc-1\n¿Pregunta?\n---\nRespuesta.\n:::\n",
    )
    .unwrap();
    let vault = orbital_vault::open(&root).unwrap();
    let from = orbital_vault::RelPath::new("wiki/Nota.md").unwrap();
    let text = "# Nota\n\n![[Origen]]\n\n![[Origen#Segunda parte]]\n\n![[Origen#^def]]\n";
    let mut prepared = Prepared::with_vault(text, &vault, &from, &[]);
    complete_generated(
        &mut prepared.doc,
        &mut prepared.hashes,
        None,
        None,
        Some(&from),
    );
    glib::MainContext::default().block_on(async {
        let reader = Reader::new();
        reader.render(prepared, || None, None);
        esperar_volcado(&reader).await;
        let buf = reader.buffer();
        let visible = buf
            .text(&buf.start_iter(), &buf.end_iter(), false)
            .to_string();
        assert!(!visible.contains("secreto"), "{visible:?}");
        assert!(
            !visible.contains("debugInConsole"),
            "índice crudo: {visible:?}"
        );
        // El índice de la incrustada lista sus encabezados (además de los títulos mismos).
        assert_eq!(visible.matches("Primera parte").count(), 2, "{visible:?}");
        assert!(visible.contains("Origen  ›  Segunda parte"), "{visible:?}");
        assert!(visible.contains("bloque def"), "{visible:?}");
        assert!(
            visible.contains("Un párrafo con definición."),
            "{visible:?}"
        );
        let items = reader.all_link_items();
        assert!(items.iter().filter(|l| l.url == "[[Origen]]").count() == 1);
        assert!(items.iter().any(|l| l.url == "[[Origen#Segunda parte]]"));
        let incrustaciones = contar_capas(&reader, |c| {
            matches!(
                c,
                capas::Capa::Incrustacion {
                    cuerpo: Some(_),
                    ..
                }
            )
        });
        assert_eq!(incrustaciones, 3);
        // El callout y las tarjetas de adentro, metidos por la derecha dentro del borde.
        assert_eq!(
            contar_capas(
                &reader,
                |c| matches!(c, capas::Capa::Callout { der, .. } if *der > 0)
            ),
            1
        );
        assert_eq!(
            contar_capas(
                &reader,
                |c| matches!(c, capas::Capa::Tarjeta { der, .. } if *der > 0)
            ),
            2
        );
        assert!(reader.0.pending.borrow().is_empty(), "sin widgets anclados");
        let mut it = buf.start_iter();
        let mut anclados = 0;
        loop {
            if it.child_anchor().is_some() {
                anclados += 1;
            }
            if !it.forward_char() {
                break;
            }
        }
        assert_eq!(anclados, 0, "ningún widget anclado al texto");
    });
    let _ = std::fs::remove_dir_all(&root);
}

/// 024: cada tarjeta de estudio es una tarjeta propia (capa, sin widgets) con su rótulo, la
/// pregunta a la vista y la respuesta oculta hasta accionar «Mostrar respuesta» (Tab e Intro).
#[test]
#[ignore = "requiere acceso GTK; tools/pruebas-gtk.sh"]
fn tarjetas_de_estudio_en_lectura() {
    gtk::init().expect("el proceso necesita conectarse a una sesión GTK");
    glib::MainContext::default().block_on(async {
        let reader = Reader::new();
        let text = "## Flashcards\n\n:::flashcard id=fc-001\n¿Qué dice el PBO?\n---\nTodo subconjunto no vacío tiene mínimo.\n:::\n\n:::flashcard id=fc-002 \n¿Y en $\\mathbb{Z}$?\n---\nNo vale.\n:::\n";
        reader.render(Prepared::new(text, &[]), || None, None);
        esperar_volcado(&reader).await;
        let buf = reader.buffer();
        let visible = || {
            buf.text(&buf.start_iter(), &buf.end_iter(), false)
                .to_string()
        };
        let v = visible();
        assert!(v.contains("Tarjeta 1 de 2  ·  fc-001"), "{v:?}");
        assert!(v.contains("Tarjeta 2 de 2  ·  fc-002"), "{v:?}");
        assert!(v.contains("¿Qué dice el PBO?"), "{v:?}");
        assert!(!v.contains("Todo subconjunto"), "la respuesta empieza oculta: {v:?}");
        assert!(!v.contains(":::") && !v.contains("---"), "{v:?}");
        assert_eq!(v.matches("Mostrar respuesta").count(), 2, "{v:?}");
        assert!(!v.contains("Ocultar respuesta"), "{v:?}");
        let items = reader.all_link_items();
        let n = items
            .iter()
            .position(|l| l.url.starts_with(bloques::URL_CALLOUT))
            .expect("el botón se recorre con Tab");
        reader.set_focused_link(Some(n));
        assert!(reader.activate_focused_link());
        let v = visible();
        assert!(v.contains("Todo subconjunto"), "{v:?}");
        assert!(v.contains("Ocultar respuesta"), "{v:?}");
        assert!(!v.contains("No vale."), "la otra sigue cerrada: {v:?}");
        reader.set_focused_link(Some(n));
        assert!(reader.activate_focused_link());
        assert!(!visible().contains("Todo subconjunto"));
        assert_eq!(
            contar_capas(&reader, |c| matches!(c, capas::Capa::Tarjeta { .. })),
            2
        );
        assert!(reader.0.pending.borrow().is_empty(), "sin widgets anclados");
    });
}

/// Medición (no es prueba de comportamiento): abrir en Lectura una nota con muchas
/// transclusiones. `ORBITAL_MEDIR_BOVEDA=<bóveda> ORBITAL_MEDIR_NOTA=<ruta> cargo test --release
/// --bin orbital medir_lectura_con_transclusiones -- --ignored --nocapture`. Imprime la mediana
/// de 9 aperturas: la preparación (hilo de fondo: análisis, transclusiones y generados) y el
/// volcado en la vista hasta el primer cuadro dibujado.
#[test]
#[ignore = "medición; correr a mano con --release"]
fn medir_lectura_con_transclusiones() {
    use gtk::prelude::*;
    gtk::init().expect("el proceso necesita conectarse a una sesión GTK");
    let (Some(root), Ok(nota)) = (
        std::env::var_os("ORBITAL_MEDIR_BOVEDA"),
        std::env::var("ORBITAL_MEDIR_NOTA"),
    ) else {
        eprintln!("Faltan ORBITAL_MEDIR_BOVEDA y ORBITAL_MEDIR_NOTA");
        return;
    };
    let root = std::path::PathBuf::from(root);
    let vault = orbital_vault::open(&root).unwrap();
    let from = orbital_vault::RelPath::new(&nota).unwrap();
    let text = std::fs::read_to_string(root.join(&nota)).unwrap();
    glib::MainContext::default().block_on(async {
        let mut preparar = Vec::new();
        let mut volcar = Vec::new();
        for _ in 0..9 {
            let t0 = std::time::Instant::now();
            let mut p = Prepared::with_vault(&text, &vault, &from, &[]);
            complete_generated(&mut p.doc, &mut p.hashes, None, None, Some(&from));
            preparar.push(t0.elapsed().as_secs_f64() * 1000.0);
            let reader = Reader::new();
            let window = gtk::Window::builder()
                .default_width(1000)
                .default_height(1200)
                .child(reader.widget())
                .build();
            window.present();
            glib::timeout_future(Duration::from_millis(200)).await;
            let t1 = std::time::Instant::now();
            reader.render(p, || None, None);
            while reader.0.blocks.borrow().is_empty() {
                glib::timeout_future(Duration::from_millis(1)).await;
            }
            // Hasta que se dibuja el cuadro (disposición y capas de lo visible): el segundo
            // tic del reloj de cuadros llega después de pintar el primero.
            let tics = std::rc::Rc::new(std::cell::Cell::new(0));
            let d = tics.clone();
            reader.text_view().add_tick_callback(move |_, _| {
                d.set(d.get() + 1);
                if d.get() < 2 {
                    glib::ControlFlow::Continue
                } else {
                    glib::ControlFlow::Break
                }
            });
            while tics.get() < 2 {
                glib::timeout_future(Duration::from_millis(1)).await;
            }
            volcar.push(t1.elapsed().as_secs_f64() * 1000.0);
            window.close();
        }
        for v in [&mut preparar, &mut volcar] {
            v.sort_by(f64::total_cmp);
        }
        println!(
            "Preparar: mediana {:.1} ms (mín {:.1}, máx {:.1}); volcado hasta el cuadro: mediana {:.1} ms (mín {:.1}, máx {:.1})",
            preparar[4], preparar[0], preparar[8], volcar[4], volcar[0], volcar[8]
        );
    });
}

/// Bóveda temporal con un cuaderno de `capitulos` capítulos hechos de transclusiones (como los
/// cuadernos de la usuaria): cada átomo trae Síntesis, callouts (plegables abiertos y cerrados),
/// índice, lista anidada, tareas, tarjetas de estudio y Conexiones. Devuelve la raíz y el texto
/// del cuaderno; el capítulo 2 tiene además callouts, una tarjeta y una lista directos.
fn boveda_cuaderno(capitulos: usize) -> (std::path::PathBuf, String) {
    let root = std::env::temp_dir().join(format!("orbital-046-{}", glib::uuid_string_random()));
    std::fs::create_dir_all(root.join("wiki/HMA")).unwrap();
    for n in 1..=capitulos {
        let mut t = format!(
            "---\nid: AT-{n:03}\ntype: atomo\n---\n<!-- summary AT-{n:03} -->\n## Síntesis\n\nIdea central del átomo {n}.\n<!---->\n\n> [!abstract] Ficha de la nota\n> **Tipo:** átomo\n\n```table-of-contents\ntitle: \nstyle: nestedList\n```\n\n## 1. Definiciones\n\n> [!definition] Demostración\n> Una cadena de pasos.\n\n> [!warning]- Plegable cerrado\n> Oculto adentro {n}.\n\n> [!tip]+ Plegable abierto\n> Visible adentro {n}.\n\n- uno\n  - sub uno\n  - sub dos\n- dos\n\n- [ ] tarea pendiente {n}\n- [x] tarea hecha\n\n## 2. Flashcards\n\n"
        );
        for k in 1..=6 {
            t.push_str(&format!(
                ":::flashcard id=fc-{n}0{k}\n¿Pregunta {k} del {n}?\n---\nRespuesta {k} del {n}.\n:::\n\n"
            ));
        }
        t.push_str(&format!(
            "<!-- connections AT-{n:03} -->\n## Conexiones\n\n- [[Atomo 1]]\n- [[Atomo 2]]\n<!---->\n"
        ));
        std::fs::write(root.join(format!("wiki/HMA/Atomo {n}.md")), t).unwrap();
    }
    let mut text = String::from("# Cuaderno\n\n### Módulo 1\n\n#### Tema 1\n\n");
    for n in 1..=capitulos {
        text.push_str(&format!("##### Capítulo {n}\n\n![[Atomo {n}]]\n\n"));
        if n == 2 {
            text.push_str("> [!note] Callout directo\n> Con texto directo.\n\n> [!faq]- Callout directo plegable\n> Cuerpo directo.\n\n:::flashcard id=fc-901\n¿Directa?\n---\nRespuesta directa.\n:::\n\n- lista\n  - anidada directa\n\nTareas:\n\n- [ ] tarea directa\n- [x] otra tarea directa\n\n");
        }
    }
    text.push_str("##### Final\n\nTexto final.\n");
    (root, text)
}

/// Guarda la vista del lector en `<ORBITAL_CAPTURA_DIR>/<nombre>.png` (si está la variable).
pub(super) fn capturar_lector(reader: &Reader, nombre: &str) {
    use gtk::prelude::*;
    let Some(dir) = std::env::var_os("ORBITAL_CAPTURA_DIR") else {
        return;
    };
    let w = reader.widget();
    let (ancho, alto) = (w.width(), w.height());
    let pintable = gtk::WidgetPaintable::new(Some(w));
    let snap = gtk::Snapshot::new();
    snap.append_color(
        &crate::style::VIEW_BG.rgba(1.0),
        &gtk::graphene::Rect::new(0.0, 0.0, ancho as f32, alto as f32),
    );
    pintable.snapshot(&snap, f64::from(ancho), f64::from(alto));
    let node = snap.to_node().expect("cuadro");
    let png = std::path::Path::new(&dir).join(format!("{nombre}.png"));
    w.root()
        .and_then(|r| r.renderer())
        .expect("renderizador")
        .render_texture(&node, None)
        .save_to_png(&png)
        .unwrap();
    println!("Captura: {}", png.display());
}

/// Prepara el cuaderno de prueba en un lector dentro de una ventana y espera el volcado.
async fn lector_con_cuaderno(
    root: &std::path::Path,
    text: &str,
    alto: i32,
) -> (Reader, gtk::Window) {
    use gtk::prelude::*;
    let vault = orbital_vault::open(root).unwrap();
    let from = orbital_vault::RelPath::new("wiki/Cuaderno.md").unwrap();
    let mut prepared = Prepared::with_vault(text, &vault, &from, &[]);
    complete_generated(
        &mut prepared.doc,
        &mut prepared.hashes,
        None,
        None,
        Some(&from),
    );
    prepared.con_plegables(&orbital_vault::plegado::plegables(text));
    let reader = Reader::new();
    let window = gtk::Window::builder()
        .default_width(1000)
        .default_height(alto)
        .child(reader.widget())
        .build();
    window.present();
    reader.render(prepared, || None, None);
    esperar_volcado(&reader).await;
    (reader, window)
}

/// Deja pasar unos cuadros (GTK calcula la disposición de a tramos, en *idles*).
async fn esperar_cuadros() {
    for _ in 0..15 {
        glib::timeout_future(Duration::from_millis(60)).await;
    }
}

/// 046: una sección plegada queda guardada del todo. Sus transclusiones, callouts, tarjetas
/// (aun con la respuesta a la vista), listas y partes especiales no ocupan alto ni se dibujan, y
/// lo abierto adentro (un callout o una respuesta) no se asoma. Al desplegar vuelve todo.
#[test]
#[ignore = "requiere acceso GTK; tools/pruebas-gtk.sh"]
fn seccion_plegada_no_deja_cajas_ni_alto() {
    use gtk::prelude::*;
    gtk::init().expect("el proceso necesita conectarse a una sesión GTK");
    let (root, text) = boveda_cuaderno(3);
    let linea = |t: &str| text.lines().position(|l| l == t).unwrap() as u32;
    let (cap1, cap2) = (linea("##### Capítulo 1"), linea("##### Capítulo 2"));
    glib::MainContext::default().block_on(async {
        let (reader, window) = lector_con_cuaderno(&root, &text, 1100).await;
        let buf = reader.buffer();
        let visible = || {
            buf.text(&buf.start_iter(), &buf.end_iter(), false)
                .to_string()
        };
        // Lo abierto adentro de la sección que se va a plegar: una respuesta y un callout.
        assert!(reader.mostrar_respuesta("fc-901"));
        let n = reader
            .all_link_items()
            .iter()
            .position(|l| {
                l.url.starts_with(bloques::URL_CALLOUT)
                    && buf.text(&buf.iter_at_mark(&l.start), &buf.iter_at_mark(&l.end), true)
                        == "Callout directo plegable"
            })
            .unwrap();
        reader.set_focused_link(Some(n));
        assert!(reader.activate_focused_link());
        reader.set_focused_link(None);
        let v = visible();
        assert!(
            v.contains("Respuesta directa.") && v.contains("Cuerpo directo."),
            "{v:?}"
        );
        // Posición (en caracteres) de un texto en el buffer de ahora: lo plegado sale de él.
        let off = |t: &str| {
            let todo = buf
                .text(&buf.start_iter(), &buf.end_iter(), true)
                .to_string();
            todo[..todo.find(t).unwrap()].chars().count() as i32
        };
        // Alto entre el comienzo de un capítulo y el del siguiente, y el de su renglón.
        let alto_seccion = |a: &str, b: &str| {
            let view = reader.text_view();
            let (ya, ha) = view.line_yrange(&buf.iter_at_offset(off(a)));
            let (yb, _) = view.line_yrange(&buf.iter_at_offset(off(b)));
            (yb - ya, ha)
        };
        esperar_cuadros().await;
        let (abierto, _) = alto_seccion("Capítulo 2", "Capítulo 3");
        capturar_lector(&reader, "046-desplegado");
        // Las casillas de las tareas incrustadas son widgets anclados: vuelven al desplegar.
        let anclas = || {
            let mut it = buf.start_iter();
            let mut n = 0;
            loop {
                n += usize::from(it.child_anchor().is_some());
                if !it.forward_char() {
                    return n;
                }
            }
        };
        let casillas = anclas();
        assert!(casillas >= 2, "{casillas}");

        reader.set_plegado(Rc::new(vec![cap1, cap2]));
        esperar_cuadros().await;
        capturar_lector(&reader, "046-plegado");
        let v = visible();
        for oculto in [
            "Idea central del átomo 1",
            "Respuesta directa.",
            "Cuerpo directo.",
            "Ocultar respuesta",
            "¿Pregunta 1 del 1?",
            "¿Directa?",
            "Con texto directo.",
            "anidada directa",
            "Visible adentro 2",
        ] {
            assert!(!v.contains(oculto), "se asoma «{oculto}»: {v:?}");
        }
        for (a, b) in [("Capítulo 1", "Capítulo 2"), ("Capítulo 2", "Capítulo 3")] {
            let (alto, renglon) = alto_seccion(a, b);
            assert!(
                alto <= renglon,
                "«{a}» plegado ocupa {alto} px (su renglón, {renglon} px; abierto, {abierto})"
            );
        }
        // Ninguna capa de lo plegado se dibuja (las del capítulo 3, a la vista, sí).
        let plegado = off("Capítulo 1")..off("Capítulo 3");
        let capas = reader.capas_dibujables();
        assert!(
            !capas.iter().any(|c| plegado.contains(c)),
            "capas dibujadas en lo plegado: {capas:?} ({plegado:?})"
        );
        assert!(capas.iter().any(|c| *c > plegado.end));
        // Lo plegado no está en el buffer, pero se encuentra en el texto oculto.
        assert!(
            !buf.text(&buf.start_iter(), &buf.end_iter(), true)
                .contains("Idea central del átomo 1")
        );
        assert!(
            reader
                .texto_oculto()
                .iter()
                .any(|o| o.texto.contains("Idea central del átomo 1"))
        );

        // Al desplegar vuelve todo como estaba (lo abierto, abierto).
        reader.set_plegado(Rc::new(Vec::new()));
        esperar_cuadros().await;
        capturar_lector(&reader, "046-redesplegado");
        let v = visible();
        assert!(
            v.contains("Respuesta directa.") && v.contains("Cuerpo directo."),
            "{v:?}"
        );
        assert!(
            !v.contains("Oculto adentro 1"),
            "lo cerrado sigue cerrado: {v:?}"
        );
        assert!(!v.contains("Respuesta 1 del 1"), "{v:?}");
        let (alto, _) = alto_seccion("Capítulo 2", "Capítulo 3");
        assert_eq!(alto, abierto);
        let plegado = off("Capítulo 1")..off("Capítulo 3");
        assert!(
            reader
                .capas_dibujables()
                .iter()
                .any(|c| plegado.contains(c))
        );
        assert!(reader.texto_oculto().is_empty());
        assert_eq!(anclas(), casillas, "las casillas vuelven como anclas");
        window.close();
    });
    let _ = std::fs::remove_dir_all(&root);
}

/// Medición (no es prueba de comportamiento): desplazarse por un cuaderno de 40 capítulos con
/// todos plegados, de a 60 px. `cargo test --release --bin orbital
/// medir_lectura_scroll_plegado -- --ignored --nocapture`. Imprime el tiempo de armar el cuadro
/// de la vista (disposición de lo visible, texto y capas) en cada paso y cuántas veces cambió el
/// alto total del documento mientras tanto (cada cambio es un salto de la barra: un tirón).
#[test]
#[ignore = "medición; correr a mano con --release"]
fn medir_lectura_scroll_plegado() {
    use gtk::prelude::*;
    gtk::init().expect("el proceso necesita conectarse a una sesión GTK");
    let capitulos = std::env::var("ORBITAL_MEDIR_CAPITULOS")
        .ok()
        .and_then(|n| n.parse().ok())
        .unwrap_or(40);
    // `ORBITAL_MEDIR_PLEGAR=impares`: solo uno de cada dos capítulos (lo plegado entre texto).
    let impares = std::env::var("ORBITAL_MEDIR_PLEGAR").is_ok_and(|v| v == "impares");
    let (root, text) = boveda_cuaderno(capitulos);
    let plegados: Vec<u32> = text
        .lines()
        .enumerate()
        .filter(|(_, l)| l.starts_with("##### Capítulo"))
        .map(|(n, _)| n as u32)
        .enumerate()
        .filter(|(k, _)| !impares || k % 2 == 0)
        .map(|(_, n)| n)
        .collect();
    glib::MainContext::default().block_on(async {
        // `ORBITAL_MEDIR_SIN_ESPERA=1`: plegar y desplazarse apenas se vuelca, mientras GTK todavía
        // calcula la disposición (como al abrir la nota y bajar enseguida).
        let sin_espera = std::env::var_os("ORBITAL_MEDIR_SIN_ESPERA").is_some();
        let (reader, window) = lector_con_cuaderno(&root, &text, 500).await;
        if !sin_espera {
            esperar_cuadros().await;
        }
        reader.set_plegado(Rc::new(plegados));
        let view = reader.text_view().clone();
        let adj = reader.widget().vadjustment();
        adj.set_value(0.0);
        if !sin_espera {
            esperar_cuadros().await;
        }
        while adj.upper() <= adj.page_size() + 60.0 {
            glib::timeout_future(Duration::from_millis(2)).await;
        }
        let saltos = Rc::new(std::cell::Cell::new(0u32));
        let s = saltos.clone();
        adj.connect_upper_notify(move |_| s.set(s.get() + 1));
        // Cada cuadro, de la disposición al final del dibujo (lo que hace la CPU por cuadro).
        let reloj = view.frame_clock().expect("reloj de cuadros");
        let inicio: Rc<std::cell::Cell<Option<std::time::Instant>>> = Rc::default();
        let cuadros: Rc<RefCell<Vec<f64>>> = Rc::default();
        let i = inicio.clone();
        reloj.connect_before_paint(move |_| i.set(Some(std::time::Instant::now())));
        let (i, c) = (inicio.clone(), cuadros.clone());
        reloj.connect_after_paint(move |_| {
            if let Some(t) = i.take() {
                c.borrow_mut().push(t.elapsed().as_secs_f64() * 1000.0);
            }
        });
        let mut paso = 0.0;
        let mut pasos = 0;
        while paso + adj.page_size() < adj.upper() && pasos < 400 {
            paso += 60.0;
            pasos += 1;
            adj.set_value(paso);
            glib::timeout_future(Duration::from_millis(20)).await;
        }
        let resumen = |ms: &mut Vec<f64>| {
            if ms.is_empty() {
                return "sin cuadros".to_owned();
            }
            ms.sort_by(f64::total_cmp);
            let at = |q: f64| ms[((ms.len() - 1) as f64 * q).round() as usize];
            format!(
                "{} cuadros, mediana {:.2} ms, p95 {:.2} ms, máx {:.2} ms, {} de más de 16,7 ms",
                ms.len(),
                at(0.5),
                at(0.95),
                ms[ms.len() - 1],
                ms.iter().filter(|m| **m > 16.7).count()
            )
        };
        let mut ms = std::mem::take(&mut *cuadros.borrow_mut());
        println!(
            "Desplazamiento: {pasos} pasos de 60 px, alto {:.0} px, cambios del alto total: {}; {}",
            adj.upper(),
            saltos.get(),
            resumen(&mut ms)
        );
        // Quieto en la mitad: los cuadros solo vuelven a dibujar lo mismo.
        adj.set_value((adj.upper() - adj.page_size()) / 2.0);
        glib::timeout_future(Duration::from_millis(200)).await;
        cuadros.borrow_mut().clear();
        for _ in 0..30 {
            view.queue_draw();
            glib::timeout_future(Duration::from_millis(20)).await;
        }
        let mut ms = std::mem::take(&mut *cuadros.borrow_mut());
        println!("Quieto, redibujando: {}", resumen(&mut ms));
        // Desplegar y volver a plegar un capítulo (los demás quedan como estaban): lo que tarda
        // `set_plegado` y el cuadro siguiente.
        let todos = reader.0.plegado.borrow().clone();
        let mut alternar = Vec::new();
        let mut cuadro = Vec::new();
        for k in 0..todos.len().min(10) {
            for lineas in [
                todos.iter().copied().filter(|l| *l != todos[k]).collect(),
                (*todos).clone(),
            ] {
                cuadros.borrow_mut().clear();
                let t = std::time::Instant::now();
                reader.set_plegado(Rc::new(lineas));
                alternar.push(t.elapsed().as_secs_f64() * 1000.0);
                while cuadros.borrow().is_empty() {
                    view.queue_draw();
                    glib::timeout_future(Duration::from_millis(5)).await;
                }
                cuadro.push(cuadros.borrow()[0]);
                glib::timeout_future(Duration::from_millis(60)).await;
            }
        }
        println!(
            "Plegar o desplegar un capítulo: {}; cuadro siguiente: {}",
            resumen(&mut alternar),
            resumen(&mut cuadro)
        );
        window.close();
    });
    let _ = std::fs::remove_dir_all(&root);
}

/// 048: una consulta en tabla se dibuja con la nota como enlace activable y los valores tipados;
/// al cambiar una nota y volver a generar (lo que hace la ventana tras cada cambio del índice) la
/// tabla se actualiza; las coincidencias y el conteo se ven como texto.
#[test]
#[ignore = "requiere acceso GTK; tools/pruebas-gtk.sh"]
fn consulta_tabla_coincidencias_y_conteo() {
    gtk::init().expect("el proceso necesita conectarse a una sesión GTK");
    let context = glib::MainContext::default();
    context.block_on(async {
        use std::fs;
        let root =
            std::env::temp_dir().join(format!("orbital-048-{}", glib::uuid_string_random()));
        fs::create_dir_all(root.join("wiki")).unwrap();
        fs::write(
            root.join("wiki/a.md"),
            "---\nestado: activo\nprioridad: 10\n---\n#proyecto\nEl motor arranca.\n",
        )
        .unwrap();
        fs::write(
            root.join("wiki/b.md"),
            "---\nprioridad: 9\n---\n#proyecto\nestado:: pausado\n",
        )
        .unwrap();
        let fuente = "# Tablero\n\n```consulta\ntag:#proyecto\nvista: tabla\ncolumnas: estado, prioridad\norden: prioridad\n```\n\n```consulta\nmotor\nvista: coincidencias\n```\n\n```consulta\ntag:#proyecto\nvista: conteo\n```\n";
        fs::write(root.join("wiki/tablero.md"), fuente).unwrap();
        let path = orbital_vault::RelPath::new("wiki/tablero.md").unwrap();
        let vault = orbital_vault::open(&root).unwrap();
        let mut index = orbital_vault::SearchIndex::build(&vault);
        let preparar = |index: &orbital_vault::SearchIndex| {
            let mut p = Prepared::new(fuente, &[]);
            assert!(complete_generated(
                &mut p.doc,
                &mut p.hashes,
                Some(index),
                Some(vault.root()),
                Some(&path),
            ));
            p
        };
        let texto = |reader: &Reader| {
            let b = reader.buffer();
            b.text(&b.start_iter(), &b.end_iter(), false).to_string()
        };
        let reader = Reader::new();
        let elegida = Rc::new(RefCell::new(String::new()));
        let got = elegida.clone();
        reader.set_on_link(move |url| *got.borrow_mut() = url.into());
        reader.render(preparar(&index), || None, None);
        for _ in 0..200 {
            if texto(&reader).contains("2 notas") {
                break;
            }
            glib::timeout_future(Duration::from_millis(10)).await;
        }
        let visible = texto(&reader);
        assert!(!visible.contains("```"), "{visible:?}");
        // Tabla ordenada por prioridad (9 antes que 10), con el campo inline de b.
        assert!(
            visible.contains("\tNota\testado\tprioridad\n\tb.md\tpausado\t9\n\ta.md\tactivo\t10\n"),
            "{visible:?}"
        );
        assert!(visible.contains("6: El motor arranca."), "{visible:?}");
        assert!(visible.contains("2 notas"), "{visible:?}");
        let urls: Vec<String> = reader.all_link_items().into_iter().map(|i| i.url).collect();
        // El propio bloque contiene «motor»: la nota del tablero también coincide (excluir la
        // nota actual quedó fuera de alcance).
        assert_eq!(urls, ["/b.md", "/a.md", "/a.md", "/tablero.md"]);
        reader.set_focused_link(Some(0));
        assert!(reader.activate_focused_link());
        assert_eq!(*elegida.borrow(), "/b.md");

        // Un cambio en otra nota: la consulta se vuelve a generar con el índice nuevo.
        let b = orbital_vault::RelPath::new("wiki/b.md").unwrap();
        let nuevo = "---\nprioridad: 11\nestado: listo\n---\n#proyecto\n";
        fs::write(b.to_path(vault.root()), nuevo).unwrap();
        index.update_file(&vault, &b, nuevo);
        reader.render(preparar(&index), || None, None);
        for _ in 0..200 {
            if texto(&reader).contains("listo") {
                break;
            }
            glib::timeout_future(Duration::from_millis(10)).await;
        }
        let visible = texto(&reader);
        assert!(
            visible.contains("\ta.md\tactivo\t10\n\tb.md\tlisto\t11\n"),
            "{visible:?}"
        );
        assert_eq!(
            fs::read_to_string(root.join("wiki/tablero.md")).unwrap(),
            fuente
        );
        fs::remove_dir_all(root).unwrap();
    });
}

/// Medición (050): abrir en Lectura una nota con 200 fórmulas (la primera vez dibuja todas en el
/// hilo de fondo; después salen de la caché). Imprime, por apertura, la preparación en el hilo de
/// fondo (análisis + dibujo de las fórmulas), el volcado en el hilo de GTK y el tramo más largo
/// sin volver al bucle principal. `cargo test --release --bin orbital medir_lectura_formulas --
/// --ignored --nocapture`; `ORBITAL_MEDIR_SIN_FORMULAS=1` mide la misma nota sin los `$`.
#[test]
#[ignore = "medición; correr a mano con --release"]
fn medir_lectura_formulas() {
    gtk::init().expect("el proceso necesita conectarse a una sesión GTK");
    let corpus = [
        r"x^2 + y^2 = z^2",
        r"\frac{a}{b} + \sqrt{x^2+1}",
        r"\sum_{n=1}^{\infty} \frac{1}{n^2} = \frac{\pi^2}{6}",
        r"\int_0^1 x^2\,dx",
        r"f(x) = \begin{cases} x & x \ge 0 \\ -x & x < 0 \end{cases}",
        r"\begin{pmatrix} a & b \\ c & d \end{pmatrix}",
        r"\begin{aligned} a &= b + c \\ d &= e + f \end{aligned}",
        r"\alpha \beta \gamma \Delta \Omega",
    ];
    let mut text = String::from("---\ntype: atomo\n---\n# Muchas fórmulas\n\n");
    for n in 0..200 {
        let f = format!("{} + {n}", corpus[n % corpus.len()]);
        if n % 4 == 3 {
            text.push_str(&format!("$$\n{f}\n$$\n\n"));
        } else {
            text.push_str(&format!(
                "Párrafo {n} con ${f}$ en el renglón y más texto.\n\n"
            ));
        }
    }
    if std::env::var_os("ORBITAL_MEDIR_SIN_FORMULAS").is_some() {
        text = text.replace('$', "");
    }
    let window = gtk::Window::builder()
        .default_width(900)
        .default_height(800)
        .build();
    glib::MainContext::default().block_on(async {
        // `ORBITAL_MEDIR_TEXTURA=1`: dibujar antes una textura cualquiera (separa el costo de la
        // primera textura del proceso del de las fórmulas).
        if std::env::var_os("ORBITAL_MEDIR_TEXTURA").is_some() {
            let bytes = glib::Bytes::from_owned(vec![255u8; 16 * 16 * 4]);
            let t = gtk::gdk::MemoryTexture::new(16, 16, gtk::gdk::MemoryFormat::R8g8b8a8, &bytes, 64);
            window.set_child(Some(&gtk::Picture::for_paintable(&t)));
            window.present();
            glib::timeout_future(Duration::from_millis(300)).await;
        }
        for vuelta in 0..6 {
            let reader = Reader::new();
            window.set_child(Some(reader.widget()));
            window.present();
            let estilo = reader.estilo_formulas();
            let t0 = std::time::Instant::now();
            let texto = text.clone();
            let prepared = gtk::gio::spawn_blocking(move || {
                let p = Prepared::new(&texto, &[]);
                crate::formulas::precalentar(Reader::claves_formulas(&p.doc, &estilo));
                p
            })
            .await
            .unwrap();
            let prep = t0.elapsed().as_secs_f64() * 1000.0;
            // El tramo más largo del hilo de GTK: un reloj que avanza cada 1 ms.
            let max = Rc::new(Cell::new(0.0f64));
            let (m, ultimo) = (max.clone(), Rc::new(Cell::new(std::time::Instant::now())));
            let tic = glib::timeout_add_local(Duration::from_millis(1), move || {
                let ahora = std::time::Instant::now();
                m.set(m.get().max((ahora - ultimo.get()).as_secs_f64() * 1000.0));
                ultimo.set(ahora);
                glib::ControlFlow::Continue
            });
            let t1 = std::time::Instant::now();
            reader.render(prepared, || None, None);
            while reader.0.blocks.borrow().is_empty() {
                glib::timeout_future(Duration::from_millis(1)).await;
            }
            let volcado = t1.elapsed().as_secs_f64() * 1000.0;
            glib::timeout_future(Duration::from_millis(50)).await;
            tic.remove();
            println!(
                "Apertura {}: preparación {prep:.1} ms, volcado {volcado:.1} ms, tramo más largo {:.1} ms",
                vuelta + 1,
                max.get()
            );
        }
    });
}

/// Medios en Lectura (057): `![[tono.wav#t=1]]` y `![](medios/tono.wav)` son reproductores
/// anclados (sin abrir nada: no hay controles hasta reproducir), `![[foto.png]]` es la imagen y
/// lo que no existe dice «Archivo inexistente», no «Nota inexistente». La bóveda tiene una ruta
/// con letras no ASCII.
#[test]
#[ignore = "requiere acceso GTK; tools/pruebas-gtk.sh"]
fn medios_en_lectura() {
    gtk::init().expect("el proceso necesita conectarse a una sesión GTK");
    let root = std::env::temp_dir().join(format!(
        "orbital-057-bóveda-ñ-{}",
        glib::uuid_string_random()
    ));
    std::fs::create_dir_all(root.join("wiki/medios")).unwrap();
    std::fs::write(
        root.join("wiki/medios/tono ñ.wav"),
        crate::medios::tests::wav_de_prueba(2.0),
    )
    .unwrap();
    // Un PNG de 1×1.
    std::fs::write(
        root.join("wiki/medios/foto.png"),
        [
            0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0, 0, 0, 0x0d, 0x49, 0x48, 0x44, 0x52,
            0, 0, 0, 1, 0, 0, 0, 1, 8, 6, 0, 0, 0, 0x1f, 0x15, 0xc4, 0x89, 0, 0, 0, 0x0d, 0x49,
            0x44, 0x41, 0x54, 0x78, 0x9c, 0x63, 0xf8, 0xcf, 0xc0, 0xf0, 0x1f, 0, 0x05, 0x00, 0x01,
            0xff, 0x89, 0x99, 0x3d, 0x1d, 0, 0, 0, 0, 0x49, 0x45, 0x4e, 0x44, 0xae, 0x42, 0x60,
            0x82,
        ],
    )
    .unwrap();
    let vault = orbital_vault::open(&root).unwrap();
    let from = orbital_vault::RelPath::new("wiki/Clase.md").unwrap();
    let text = "# Clase\n\n![[tono ñ.wav#t=1]]\n\nEn línea: ![](medios/tono%20ñ.wav) y sigue.\n\n![[foto.png]]\n\n![[falta.mp3]]\n\n![](medios/nada.webm)\n";
    let prepared = Prepared::with_vault(text, &vault, &from, &[]);
    glib::MainContext::default().block_on(async {
        let reader = Reader::new();
        let ventana = gtk::Window::builder()
            .child(reader.widget())
            .default_width(900)
            .default_height(700)
            .build();
        ventana.present();
        reader.render(prepared, || None, Some(root.join("wiki")));
        esperar_volcado(&reader).await;
        for _ in 0..200 {
            if reader.0.pending.borrow().is_empty() {
                break;
            }
            glib::timeout_future(Duration::from_millis(10)).await;
        }
        let buf = reader.buffer();
        let visible = buf
            .text(&buf.start_iter(), &buf.end_iter(), false)
            .to_string();
        assert!(
            visible.contains("Archivo inexistente: «falta.mp3»"),
            "{visible:?}"
        );
        assert!(
            visible.contains("Archivo inexistente: «medios/nada.webm»"),
            "{visible:?}"
        );
        assert!(!visible.contains("Nota inexistente"), "{visible:?}");
        assert!(visible.contains("En línea: "), "{visible:?}");
        let mut it = buf.start_iter();
        let mut anclados = Vec::new();
        loop {
            if let Some(a) = it.child_anchor() {
                anclados.extend(a.widgets());
            }
            if !it.forward_char() {
                break;
            }
        }
        let reproductores: Vec<_> = anclados
            .iter()
            .filter(|w| w.has_css_class("orbital-medio"))
            .collect();
        assert_eq!(reproductores.len(), 2, "{anclados:?}");
        assert_eq!(
            anclados.iter().filter(|w| w.is::<gtk::Picture>()).count(),
            1,
            "la imagen de ![[foto.png]]"
        );
        // Nada se abrió: ningún control de medio todavía.
        fn hay_controles(w: &gtk::Widget) -> bool {
            if w.is::<gtk::MediaControls>() {
                return true;
            }
            let mut c = w.first_child();
            while let Some(h) = c {
                if hay_controles(&h) {
                    return true;
                }
                c = h.next_sibling();
            }
            false
        }
        assert!(reproductores.iter().all(|w| !hay_controles(w)));
        // El primero muestra desde dónde empieza.
        fn rotulos(w: &gtk::Widget, out: &mut Vec<String>) {
            if let Some(l) = w.downcast_ref::<gtk::Label>() {
                out.push(l.label().to_string());
            }
            let mut c = w.first_child();
            while let Some(h) = c {
                rotulos(&h, out);
                c = h.next_sibling();
            }
        }
        let mut r = Vec::new();
        rotulos(reproductores[0], &mut r);
        assert!(r.iter().any(|l| l == "tono ñ.wav  ·  desde 0:01"), "{r:?}");
        // Dividida: editar otro bloque no rearma el reproductor, que sigue sonando.
        fn buscar_boton(w: &gtk::Widget) -> Option<gtk::Button> {
            if let Some(b) = w.downcast_ref::<gtk::Button>()
                && b.tooltip_text().as_deref() == Some("Reproducir")
            {
                return Some(b.clone());
            }
            let mut c = w.first_child();
            while let Some(h) = c {
                if let Some(b) = buscar_boton(&h) {
                    return Some(b);
                }
                c = h.next_sibling();
            }
            None
        }
        fn controles(w: &gtk::Widget) -> Option<gtk::MediaControls> {
            if let Some(c) = w.downcast_ref::<gtk::MediaControls>() {
                return Some(c.clone());
            }
            let mut c = w.first_child();
            while let Some(h) = c {
                if let Some(x) = controles(&h) {
                    return Some(x);
                }
                c = h.next_sibling();
            }
            None
        }
        buscar_boton(reproductores[0]).unwrap().emit_clicked();
        let m = controles(reproductores[0])
            .and_then(|c| c.media_stream())
            .expect("controles");
        for _ in 0..300 {
            if m.is_playing() {
                break;
            }
            glib::timeout_future(Duration::from_millis(10)).await;
        }
        assert!(m.is_playing());
        let editado = text.replace("En línea:", "Editado en línea:");
        reader.render(
            Prepared::with_vault(&editado, &vault, &from, &[]),
            || None,
            Some(root.join("wiki")),
        );
        glib::timeout_future(Duration::from_millis(300)).await;
        let visible = buf.text(&buf.start_iter(), &buf.end_iter(), false);
        assert!(visible.contains("Editado en línea:"), "{visible}");
        assert!(m.is_playing(), "editar otro bloque cortó el audio");
        ventana.destroy();
    });
    std::fs::remove_dir_all(&root).ok();
}

/// Capturas de los medios en Lectura (057), de una nota de una bóveda dada: sin reproducir
/// (`057-lectura.png`) y con todo sonando (`057-reproduciendo.png`). `ORBITAL_CAPTURA_DIR`,
/// `ORBITAL_CAPTURA_BOVEDA` y `ORBITAL_CAPTURA_NOTA` (relativa a la bóveda).
#[test]
#[ignore = "captura de desarrollo (057)"]
fn captura_medios() {
    gtk::init().expect("el proceso necesita conectarse a una sesión GTK");
    adw::init().expect("libadwaita");
    crate::style::apply();
    let root = std::path::PathBuf::from(std::env::var_os("ORBITAL_CAPTURA_BOVEDA").unwrap());
    let nota = std::env::var("ORBITAL_CAPTURA_NOTA").unwrap();
    let text = std::fs::read_to_string(root.join(&nota)).unwrap();
    let vault = orbital_vault::open(&root).unwrap();
    let from = orbital_vault::RelPath::new(&nota).unwrap();
    let prepared = Prepared::with_vault(&text, &vault, &from, &[]);
    let dir = from.to_path(&root).parent().unwrap().to_path_buf();
    glib::MainContext::default().block_on(async {
        let reader = Reader::new();
        let ventana = gtk::Window::builder()
            .child(reader.widget())
            .default_width(900)
            .default_height(1000)
            .build();
        ventana.present();
        reader.render(prepared, || None, Some(dir));
        esperar_volcado(&reader).await;
        glib::timeout_future(Duration::from_millis(800)).await;
        capturar_lector(&reader, "057-lectura");
        fn botones(w: &gtk::Widget, out: &mut Vec<gtk::Button>) {
            if let Some(b) = w.downcast_ref::<gtk::Button>()
                && b.tooltip_text().as_deref() == Some("Reproducir")
            {
                out.push(b.clone());
            }
            let mut c = w.first_child();
            while let Some(h) = c {
                botones(&h, out);
                c = h.next_sibling();
            }
        }
        let mut b = Vec::new();
        botones(reader.widget().upcast_ref(), &mut b);
        for x in b {
            x.emit_clicked();
        }
        glib::timeout_future(Duration::from_millis(2500)).await;
        capturar_lector(&reader, "057-reproduciendo");
        ventana.destroy();
    });
}
