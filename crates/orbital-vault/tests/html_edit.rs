//! T022: edición de `.html` desde la vista Visual (SC-004).

use orbital_vault::html_edit::{
    body_has_scripts, body_range, orbital_style, splice_body, upsert_orbital_style,
};

const NOTA: &str = "<!doctype html>\r\n<html lang=\"es\">\r\n<HEAD>\r\n<script type=\"application/yaml\">\r\ntype: sintesis\r\ntitle: \"<body> falso\"\r\n</script>\r\n<!-- <body> en un comentario -->\r\n<style>p { color: red } /* </body> */</style>\r\n</HEAD>\r\n<BODY class=\"nota\" data-x='a>b'>\r\n<h1>Título</h1>\r\n<p>Hola</p>\r\n</BODY>\r\n</html>\r\n";

#[test]
fn splice_replaces_only_the_body() {
    let r = body_range(NOTA).unwrap();
    assert_eq!(&NOTA[r.clone()], "\r\n<h1>Título</h1>\r\n<p>Hola</p>\r\n");
    let out = splice_body(NOTA, "<p>Nuevo</p>").unwrap();
    assert_eq!(&out[..r.start], &NOTA[..r.start]);
    assert_eq!(&out[r.start + "<p>Nuevo</p>".len()..], &NOTA[r.end..]);
    // Ida y vuelta: el archivo queda igual byte a byte.
    assert_eq!(splice_body(&out, &NOTA[r.clone()]).unwrap(), NOTA);
}

#[test]
fn no_body() {
    assert!(body_range("<p>suelto</p>").is_none());
    assert!(splice_body("<html><head></head></html>", "x").is_none());
    // Sin cierre: hasta el final.
    assert_eq!(body_range("<body><p>a").unwrap(), 6..10);
}

#[test]
fn orbital_style_is_created_then_replaced() {
    assert_eq!(orbital_style(NOTA), None);
    // Sin CSS ni estilo previo: no cambia nada.
    assert_eq!(upsert_orbital_style(NOTA, "").unwrap(), NOTA);
    let one = upsert_orbital_style(NOTA, ".o-rosa{color:#ff8cc6}").unwrap();
    assert_eq!(orbital_style(&one), Some(".o-rosa{color:#ff8cc6}"));
    assert!(one.contains("<style data-orbital>.o-rosa{color:#ff8cc6}</style>\n</HEAD>"));
    // Lo único que cambió es el estilo agregado.
    assert_eq!(
        one.replace("<style data-orbital>.o-rosa{color:#ff8cc6}</style>\n", ""),
        NOTA
    );
    let two = upsert_orbital_style(&one, ".o-centro{text-align:center}").unwrap();
    assert_eq!(two.matches("data-orbital").count(), 1);
    assert_eq!(orbital_style(&two), Some(".o-centro{text-align:center}"));
    assert_eq!(
        two.len(),
        one.len() - ".o-rosa{color:#ff8cc6}".len() + ".o-centro{text-align:center}".len()
    );
    // Sin </head> no hay dónde crearlo.
    assert!(upsert_orbital_style("<body>x</body>", ".a{}").is_none());
}

#[test]
fn scripts_in_body_only() {
    // El bloque de propiedades es un <script> del <head>: no cuenta.
    assert!(!body_has_scripts(NOTA));
    assert!(body_has_scripts(
        &splice_body(NOTA, "<script>alert(1)</script>").unwrap()
    ));
    assert!(body_has_scripts(
        &splice_body(NOTA, "<p ONCLICK=\"x()\">a</p>").unwrap()
    ));
    assert!(!body_has_scripts(
        &splice_body(NOTA, "<p data-on=\"x\">once</p>").unwrap()
    ));
    assert!(!body_has_scripts(
        &splice_body(NOTA, "<!-- <script> -->").unwrap()
    ));
}

#[test]
fn scripts_on_body_are_detected() {
    assert!(body_has_scripts(
        "<html><head></head><body onload='x()'><p>Texto</p></body></html>"
    ));
}

#[test]
fn raw_text_does_not_define_the_body() {
    let source = "<html><head><title>Texto <body> falso</title></head><body><textarea></body></textarea><p>Real</p></body></html>";
    assert_eq!(
        splice_body(source, "nuevo").unwrap(),
        "<html><head><title>Texto <body> falso</title></head><body>nuevo</body></html>"
    );
}
