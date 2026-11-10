//! Pruebas unitarias y de integración para la navegación de enlaces y encabezados (US1).

use gtk::glib;
use orbital_vault::links::{LinkKind, extract_links, parse_link_target};

use super::*;

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
        let prepared = Prepared::new(markdown);
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
