//! Pruebas del resaltado de los bloques de código (055). Las que necesitan GtkSourceView son
//! `#[ignore]` (`tools/pruebas-gtk.sh`).

use super::*;
use crate::style::{Rgb, contrast_ratio};

/// Fondo de los bloques de código en la vista formateada (`codeblock`).
const FONDO_BLOQUE: Rgb = Rgb(0x1e, 0x1e, 0x22);
/// Fondo de `pre` en lo exportado y el papel.
const FONDOS_PAPEL: [Rgb; 2] = [Rgb(0xf2, 0xf2, 0xf4), Rgb(0xff, 0xff, 0xff)];

#[test]
fn colores_legibles_en_la_vista_y_en_papel() {
    for rol in ROLES {
        let r = contrast_ratio(rol.color(), FONDO_BLOQUE);
        assert!(r >= 4.5, "{rol:?} sobre el bloque: {r:.2}");
        for bg in FONDOS_PAPEL {
            let r = contrast_ratio(rol.color_papel(), bg);
            assert!(r >= 4.5, "{rol:?} sobre papel {bg:?}: {r:.2}");
        }
    }
}

#[test]
fn css_de_papel_con_los_colores_de_cada_rol() {
    for rol in [Rol::Clave, Rol::Comentario, Rol::Cadena, Rol::Tipo] {
        let regla = format!(".{}{{color:{}}}", rol.clase(), rol.color_papel().hex());
        assert!(CSS_PAPEL.contains(&regla), "{regla} en {CSS_PAPEL}");
    }
}

#[test]
fn html_escapa_y_envuelve_cada_tramo() {
    let tramos = [
        Tramo {
            inicio: 0,
            fin: 3,
            rol: Rol::Clave,
            negrita: true,
            cursiva: false,
        },
        Tramo {
            inicio: 8,
            fin: 14,
            rol: Rol::Cadena,
            negrita: false,
            cursiva: false,
        },
        Tramo {
            inicio: 16,
            fin: 22,
            rol: Rol::Comentario,
            negrita: false,
            cursiva: true,
        },
    ];
    // Con un carácter de varios bytes: los tramos van en caracteres.
    let out = html_de("let ñ = \"a<b&\"; // café\n", &tramos);
    assert_eq!(
        out,
        "<span class=\"sx-c sx-b\">let</span> ñ = <span class=\"sx-s\">\"a&lt;b&amp;\"</span>; \
<span class=\"sx-m sx-i\">// caf</span>é\n"
    );
}

#[test]
fn sin_gtk_ni_lenguaje_no_hay_resaltado() {
    // Las pruebas comunes no inicializan GTK: el resaltado se apaga sin romper nada.
    assert_eq!(en_cache("rust", "fn a() {}"), Some(None));
    assert_eq!(en_cache("", "x"), Some(None));
    assert!(!puede("rust"));
    assert!(tramos("rust", "fn a() {}").is_none());
}

/// Los roles de un texto ya resaltado, como (texto, rol) de cada tramo.
fn roles(texto: &str, t: &[Tramo]) -> Vec<(String, Rol)> {
    let chars: Vec<char> = texto.chars().collect();
    t.iter()
        .map(|t| {
            (
                chars[t.inicio as usize..t.fin as usize].iter().collect(),
                t.rol,
            )
        })
        .collect()
}

#[test]
#[ignore = "requiere una sesión gráfica; tools/pruebas-gtk.sh"]
fn resalta_con_los_lenguajes_de_codigo() {
    gtk::init().expect("Se necesita una sesión gráfica GTK");
    let rust = "fn principal() {\n    let s = \"hola\"; // saludo\n    return 42;\n}";
    let t = tramos("rust", rust).expect("rust tiene resaltado");
    let r = roles(rust, &t);
    assert!(r.contains(&("fn".into(), Rol::Clave)), "{r:?}");
    assert!(r.contains(&("\"hola\"".into(), Rol::Cadena)), "{r:?}");
    assert!(r.contains(&("// saludo".into(), Rol::Comentario)), "{r:?}");
    // Los alias y las extensiones van al mismo lenguaje que el nombre.
    assert_eq!(tramos("rs", rust).as_deref(), Some(&*t));
    assert_eq!(tramos("Rust", rust).as_deref(), Some(&*t));
    for (alias, texto) in [
        ("py", "def f(x):\n    return \"x\"  # c\n"),
        ("python", "def f(x):\n    return \"x\"  # c\n"),
        ("js", "const a = 'x'; // c\n"),
        ("javascript", "const a = 'x'; // c\n"),
        ("ts", "let a: number = 1; // c\n"),
        ("sh", "echo \"x\" # c\n"),
        ("bash", "echo \"x\" # c\n"),
        ("yml", "clave: \"valor\" # c\n"),
        ("toml", "[tabla]\nclave = \"valor\" # c\n"),
        ("json", "{\"a\": 1}\n"),
        ("c", "int main(void) { return 0; } /* c */\n"),
        ("cpp", "int main() { return 0; } // c\n"),
        ("html", "<p class=\"x\">hola</p>\n"),
        ("css", "p { color: red; } /* c */\n"),
        ("sql", "SELECT * FROM t; -- c\n"),
        ("go", "func main() { return } // c\n"),
    ] {
        assert!(conocido(alias), "{alias}");
        assert!(tramos(alias, texto).is_some(), "{alias}: {texto}");
    }
    // Sin lenguaje, desconocido o especial: sin color, y sin cargar nada para el especial.
    for lang in [
        "", "xyz", "mermaid", "math", "grafico", "consulta", "indice", "toc",
    ] {
        assert!(tramos(lang, "fn a() { 1 }").is_none(), "{lang}");
    }
    assert!(!puede("xyz"), "desconocido ya visto");
    assert!(puede("rust"));
}

#[test]
#[ignore = "requiere una sesión gráfica; tools/pruebas-gtk.sh"]
fn resalta_por_partes_igual_que_de_una_vez() {
    gtk::init().expect("Se necesita una sesión gráfica GTK");
    // Un comentario de varios renglones que cruza el corte entre tandas.
    let mut texto = String::new();
    for n in 0..300 {
        if n == RENGLONES_POR_PASO - 2 {
            texto.push_str("/* empieza\n sigue\n sigue\n termina */\n");
        }
        texto.push_str(&format!("let x{n} = \"valor {n}\"; // nota {n}\n"));
    }
    let entera = tramos("rs", &texto).expect("resaltado");
    // Con un límite ya vencido: una tanda por llamada (y la carga del lenguaje, aparte).
    let mut vueltas = 0;
    while !avanzar("rust", &texto, std::time::Instant::now()) {
        vueltas += 1;
        assert!(vueltas < 1000);
    }
    assert!(vueltas >= 300 / RENGLONES_POR_PASO, "{vueltas} vueltas");
    let partes = en_cache("rust", &texto).flatten().expect("en la caché");
    assert_eq!(&*partes, &*entera);
    // Contra el texto entero resaltado de una vez: el mismo rol en cada carácter visible (el
    // salto de renglón del final de una tanda puede quedar sin etiqueta, y no se ve).
    let b = buffer_de("rust").unwrap();
    b.set_text(&texto);
    let (a, z) = b.bounds();
    b.ensure_highlight(&a, &z);
    let mut referencia = Vec::new();
    leer(&a, &z, &mut referencia, &mut HashMap::new());
    b.set_text("");
    let por_caracter = |t: &[Tramo]| {
        let mut v = vec![None; texto.chars().count()];
        for t in t {
            for c in &mut v[t.inicio as usize..t.fin as usize] {
                *c = Some((t.rol, t.negrita, t.cursiva));
            }
        }
        texto
            .chars()
            .zip(v)
            .filter(|(c, _)| *c != '\n')
            .map(|(_, r)| r)
            .collect::<Vec<_>>()
    };
    assert_eq!(por_caracter(&partes), por_caracter(&referencia));
    let r = roles(&texto, &partes);
    assert!(
        r.iter()
            .any(|(t, rol)| t.starts_with("/* empieza\n sigue") && *rol == Rol::Comentario),
        "{r:?}"
    );
    // Otro texto en medio descarta el trabajo a medias sin mezclarlo.
    let otro = texto.replace("valor", "otro");
    assert!(!avanzar("rust", &otro, std::time::Instant::now()));
    let corto = "let a = 1;";
    assert!(avanzar("rust", corto, std::time::Instant::now()));
    while !avanzar("rust", &otro, std::time::Instant::now()) {}
    assert_eq!(
        en_cache("rust", &otro).flatten().map(|t| t.len()),
        Some(entera.len())
    );
}
