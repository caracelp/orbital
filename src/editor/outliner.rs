//! Outliner (011): edición de listas que respeta la estructura, solo Código.
//!
//! Enter continúa la lista con el mismo marcador (numeración incluida), Tab/Shift+Tab indentan
//! el ítem bajo el cursor, Alt+Arriba/Abajo mueve el ítem completo (con subítems) entre hermanos.
//! Sin cursores extra activos (el multi-cursor tiene prioridad); fuera de listas, las teclas
//! hacen lo de siempre.

use gtk::prelude::*;

/// Inicio de línea (lección de la 006: `iter_at_line` salta líneas vacías).
fn inicio_de(buf: &impl IsA<gtk::TextBuffer>, linea: i32) -> gtk::TextIter {
    let mut it = buf.start_iter();
    for _ in 0..linea.max(0) {
        if !it.forward_line() {
            break;
        }
    }
    it
}

/// Fin de línea (antes del salto).
fn fin_de(buf: &impl IsA<gtk::TextBuffer>, linea: i32) -> gtk::TextIter {
    let mut fin = inicio_de(buf, linea);
    while !fin.ends_line() && !fin.is_end() {
        fin.forward_char();
    }
    fin
}

/// Texto de la línea sin el salto final.
fn texto_de(buf: &impl IsA<gtk::TextBuffer>, linea: i32) -> String {
    let inicio = inicio_de(buf, linea);
    let fin = fin_de(buf, linea);
    buf.text(&inicio, &fin, false).to_string()
}

/// Un ítem de lista detectado. `inicio`/`fin` son offsets absolutos del texto de la línea;
/// `indent` son los espacios antes del marcador y `marcador` lo que va antes del contenido
/// (`- `, `* `, `1. `, `- [ ] `, etc.).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Item {
    pub linea: i32,
    pub inicio: i32,
    pub fin: i32,
    pub indent: usize,
    pub marcador: String,
    /// Es ítem de tarea (`- [ ]` / `- [x]`).
    pub tarea: bool,
    pub vacio: bool,
}

/// Analiza una línea y devuelve el ítem si es de lista. `numero` es el número de línea y `base`
/// el offset absoluto del inicio de la línea dentro del buffer.
pub fn item_de_linea(linea: &str, numero: i32, base: i32) -> Option<Item> {
    let sin_salto = linea.trim_end_matches(['\r', '\n']);
    let trimmed = sin_salto.trim_start();
    let indent = sin_salto.len() - trimmed.len();
    let es_tarea = trimmed.starts_with("- [ ] ")
        || trimmed.starts_with("- [x] ")
        || trimmed.starts_with("- [X] ");
    let (marcador, vacio) = if es_tarea {
        ("- [ ] ".to_owned(), trimmed[6..].trim().is_empty())
    } else if let Some(resto) = trimmed.strip_prefix("- ") {
        ("- ".to_owned(), resto.trim().is_empty())
    } else if let Some(resto) = trimmed.strip_prefix("* ") {
        ("* ".to_owned(), resto.trim().is_empty())
    } else if let Some(resto) = trimmed.strip_prefix("+ ") {
        ("+ ".to_owned(), resto.trim().is_empty())
    } else {
        // Numeración: dígitos + '.' o ')' + espacio.
        let digitos = trimmed.bytes().take_while(|b| b.is_ascii_digit()).count();
        if digitos > 0
            && digitos < trimmed.len()
            && (trimmed.as_bytes()[digitos] == b'.' || trimmed.as_bytes()[digitos] == b')')
            && trimmed[digitos + 1..].starts_with(' ')
        {
            let marcador = trimmed[..digitos + 2].to_owned();
            let vacio = trimmed[digitos + 2..].trim().is_empty();
            (marcador, vacio)
        } else {
            return None;
        }
    };
    Some(Item {
        linea: numero,
        inicio: base + indent as i32,
        fin: base + sin_salto.len() as i32,
        indent,
        marcador,
        tarea: es_tarea,
        vacio,
    })
}

/// El ítem de lista bajo el cursor.
pub fn item_en_cursor(buf: &impl IsA<gtk::TextBuffer>) -> Option<Item> {
    let cursor = buf.iter_at_mark(&buf.get_insert());
    let linea = cursor.line();
    let texto = texto_de(buf, linea);
    item_de_linea(&texto, linea, inicio_de(buf, linea).offset())
}

/// El siguiente número para continuar una lista numerada (`1.` → `2.`).
pub fn marcador_continuacion(item: &Item) -> String {
    let texto = item.marcador.clone();
    let digitos = texto.bytes().take_while(|b| b.is_ascii_digit()).count();
    if digitos == 0 || digitos >= texto.len() {
        return texto;
    }
    let numero: u32 = texto[..digitos].parse().unwrap_or(1);
    format!("{}{} ", numero + 1, &texto[digitos..digitos + 1])
}

/// El bloque del ítem: sus líneas propias más los subítems (y líneas vacías intermedias).
/// Devuelve `(primera, última)` como números de línea.
fn rango_bloque(buf: &impl IsA<gtk::TextBuffer>, item: &Item) -> (i32, i32) {
    let total = buf.line_count();
    let mut ultima = item.linea;
    let mut n = item.linea + 1;
    while n < total {
        let txt = texto_de(buf, n);
        if txt.trim().is_empty() {
            // Separación: sigue siendo parte del bloque si el siguiente no vacío es subítem.
            let mut mirar = n + 1;
            let mut es_subitem = false;
            while mirar < total {
                let t2 = texto_de(buf, mirar);
                if t2.trim().is_empty() {
                    mirar += 1;
                    continue;
                }
                if let Some(sub) = item_de_linea(&t2, mirar, inicio_de(buf, mirar).offset()) {
                    es_subitem = sub.indent > item.indent;
                }
                break;
            }
            if es_subitem {
                ultima = n;
                n += 1;
                continue;
            }
            break;
        }
        match item_de_linea(&txt, n, inicio_de(buf, n).offset()) {
            Some(sub) if sub.indent > item.indent => ultima = n,
            _ => break,
        }
        n += 1;
    }
    (item.linea, ultima)
}

/// Enter en un ítem de lista: continúa con el mismo marcador (numeración +1) o sale si está
/// vacío. El cursor debe estar al final de la línea. Devuelve true si intervino.
pub fn enter(buf: &impl IsA<gtk::TextBuffer>) -> bool {
    let Some(item) = item_en_cursor(buf) else {
        return false;
    };
    let cursor = buf.iter_at_mark(&buf.get_insert());
    if cursor.offset() < item.fin {
        return false;
    }
    buf.begin_user_action();
    if item.vacio {
        // Salir de la lista: borra el marcador de esta línea y deja el salto.
        let mut desde = buf.iter_at_offset(item.inicio);
        let mut hasta = buf.iter_at_offset(item.fin);
        buf.delete(&mut desde, &mut hasta);
        buf.end_user_action();
        return true;
    }
    let marcador = if item
        .marcador
        .chars()
        .next()
        .is_some_and(|c| c.is_ascii_digit())
    {
        marcador_continuacion(&item)
    } else if item.tarea {
        "- [ ] ".to_owned()
    } else {
        item.marcador.clone()
    };
    let indent = " ".repeat(item.indent);
    let mut en = buf.iter_at_mark(&buf.get_insert());
    buf.insert(&mut en, &format!("\n{indent}{marcador}"));
    buf.end_user_action();
    true
}

/// Tab: indenta el ítem (y sus subítems) dos espacios. Shift+Tab: desindenta (mínimo 0).
pub fn indentar(buf: &impl IsA<gtk::TextBuffer>, hacia_adentro: bool) -> bool {
    let Some(item) = item_en_cursor(buf) else {
        return false;
    };
    if !hacia_adentro && item.indent == 0 {
        return false;
    }
    let (primera, ultima) = rango_bloque(buf, &item);
    let delta = if hacia_adentro {
        2
    } else {
        -2.min(item.indent as i32)
    };
    buf.begin_user_action();
    for n in primera..=ultima {
        let mut inicio = inicio_de(buf, n);
        if hacia_adentro {
            buf.insert(&mut inicio, "  ");
        } else if delta < 0 {
            let mut hasta = inicio;
            hasta.forward_chars(-delta);
            buf.delete(&mut inicio, &mut hasta);
        }
    }
    buf.end_user_action();
    true
}

/// El hermano (mismo indent) más cercano en la dirección dada; falla si en el medio hay texto
/// ajeno a la lista o si sale de la lista hacia un nivel menor.
fn hermano_de(buf: &impl IsA<gtk::TextBuffer>, item: &Item, hacia_arriba: bool) -> Option<Item> {
    let total = buf.line_count();
    let mut n = item.linea;
    loop {
        n = if hacia_arriba { n - 1 } else { n + 1 };
        if n < 0 || n >= total {
            return None;
        }
        let txt = texto_de(buf, n);
        if let Some(candidato) = item_de_linea(&txt, n, inicio_de(buf, n).offset()) {
            return match candidato.indent.cmp(&item.indent) {
                std::cmp::Ordering::Equal => Some(candidato),
                std::cmp::Ordering::Less => None,
                std::cmp::Ordering::Greater => continue, // subítem: seguir buscando
            };
        }
        if !txt.trim().is_empty() {
            return None; // texto ajeno a la lista
        }
    }
}

/// Texto de un rango de líneas `[desde, hasta)` con sus saltos.
fn texto_rango(buf: &impl IsA<gtk::TextBuffer>, desde: i32, hasta_exclusivo: i32) -> String {
    let mut out = String::new();
    for n in desde..hasta_exclusivo {
        out.push_str(&texto_de(buf, n));
        out.push('\n');
    }
    out
}

/// Mueve el bloque del ítem entre hermanos hacia arriba o abajo (con subítems). Devuelve false
/// si no hay hermano con el que intercambiar o si hay texto ajeno en el medio.
pub fn mover_item(buf: &impl IsA<gtk::TextBuffer>, hacia_arriba: bool) -> bool {
    let Some(item) = item_en_cursor(buf) else {
        return false;
    };
    let (primera, ultima) = rango_bloque(buf, &item);
    let Some(hermano) = hermano_de(buf, &item, hacia_arriba) else {
        return false;
    };
    let (h_primera, h_ultima) = rango_bloque(buf, &hermano);
    let texto_bloque = texto_rango(buf, primera, ultima + 1);
    let texto_hermano = texto_rango(buf, h_primera, h_ultima + 1);
    buf.begin_user_action();
    let (mut desde, mut hasta, reemplazo) = if hacia_arriba {
        // Original [hermano, bloque] → nuevo [bloque, hermano].
        (
            inicio_de(buf, h_primera),
            fin_de(buf, ultima),
            format!("{texto_bloque}{texto_hermano}"),
        )
    } else {
        // Original [bloque, hermano] → nuevo [hermano, bloque].
        (
            inicio_de(buf, primera),
            fin_de(buf, h_ultima),
            format!("{texto_hermano}{texto_bloque}"),
        )
    };
    buf.delete(&mut desde, &mut hasta);
    let mut en = desde;
    buf.insert(&mut en, reemplazo.trim_end_matches('\n'));
    buf.end_user_action();
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn buffer_con(texto: &str) -> sourceview5::Buffer {
        gtk::init().expect("se necesita acceso GTK");
        let buf = sourceview5::Buffer::new(None);
        buf.set_text(texto);
        buf
    }

    fn texto(buf: &impl IsA<gtk::TextBuffer>) -> String {
        buf.text(&buf.start_iter(), &buf.end_iter(), true)
            .to_string()
    }

    /// T001 US1.1/1.3: Enter continúa la lista con el mismo marcador y numeración +1.
    #[test]
    #[ignore = "requiere acceso GTK; tools/verificar-011-us1.sh"]
    fn us11_enter_continua_lista() {
        let buf = buffer_con("- primero\n- segundo\n");
        let en = buf.iter_at_offset(19); // fin de «segundo»
        buf.place_cursor(&en);
        assert!(enter(&buf));
        assert_eq!(texto(&buf), "- primero\n- segundo\n- \n");
        // Numerado: continúa con el número siguiente.
        let buf2 = buffer_con("1. uno\n");
        let en2 = buf2.iter_at_offset(6);
        buf2.place_cursor(&en2);
        assert!(enter(&buf2));
        assert_eq!(texto(&buf2), "1. uno\n2. \n");
    }

    /// T001 US1.2: Enter en ítem vacío sale de la lista.
    #[test]
    #[ignore = "requiere acceso GTK"]
    fn us11_enter_sale_de_lista_vacia() {
        let buf = buffer_con("- uno\n- \n");
        let en = buf.iter_at_offset(8); // fin de la línea vacía de lista
        buf.place_cursor(&en);
        assert!(enter(&buf));
        assert_eq!(texto(&buf), "- uno\n\n");
    }

    /// T002 US2.1/2.2: Tab indenta, Shift+Tab desindenta al nivel 0.
    #[test]
    #[ignore = "requiere acceso GTK"]
    fn us11_indentar_y_desindentar() {
        let buf = buffer_con("- uno\n- dos\n");
        let en = buf.iter_at_offset(9); // línea «- dos»
        buf.place_cursor(&en);
        assert!(indentar(&buf, true));
        assert_eq!(texto(&buf), "- uno\n  - dos\n");
        assert!(indentar(&buf, false));
        assert_eq!(texto(&buf), "- uno\n- dos\n");
        // Ya en nivel 0: desindentar no hace nada.
        assert!(!indentar(&buf, false));
    }

    /// T002 US2.3: Tab mueve el bloque con subítems.
    #[test]
    #[ignore = "requiere acceso GTK"]
    fn us11_indentar_bloque_con_subitems() {
        let buf = buffer_con("- uno\n  - sub\n- dos\n");
        let en = buf.iter_at_offset(2); // línea «- uno»
        buf.place_cursor(&en);
        assert!(indentar(&buf, true));
        assert_eq!(texto(&buf), "  - uno\n    - sub\n- dos\n");
    }

    /// T003 US3.1: Alt+Abajo intercambia el bloque completo entre hermanos.
    #[test]
    #[ignore = "requiere acceso GTK"]
    fn us11_mover_item_con_subitems() {
        let buf = buffer_con("- uno\n  - sub\n- dos\n");
        buf.place_cursor(&buf.iter_at_offset(2));
        assert!(mover_item(&buf, false));
        assert_eq!(texto(&buf), "- dos\n- uno\n  - sub\n");
        // Tras el intercambio, «uno» está en las líneas 1-2: el cursor se mueve con él.
        buf.place_cursor(&buf.iter_at_offset(8)); // línea «- uno»
        assert!(mover_item(&buf, true));
        assert_eq!(texto(&buf), "- uno\n  - sub\n- dos\n");
    }

    /// T003 US3.2: en los bordes de la lista no mueve.
    #[test]
    #[ignore = "requiere acceso GTK"]
    fn us11_mover_en_bordes_no_hace_nada() {
        let buf = buffer_con("- uno\n- dos\n");
        buf.place_cursor(&buf.iter_at_offset(2));
        assert!(!mover_item(&buf, true)); // ya es el primero
        buf.place_cursor(&buf.iter_at_offset(9));
        assert!(!mover_item(&buf, false)); // ya es el último
    }

    /// Parsing de marcadores: tareas, numeración con ) y niveles.
    #[test]
    fn us11_parse_marcadores() {
        let item = item_de_linea("  - [x] hecha", 0, 0).unwrap();
        assert_eq!(item.indent, 2);
        assert_eq!(item.marcador, "- [ ] ");
        let numerado = item_de_linea("3) tercero", 0, 0).unwrap();
        assert_eq!(marcador_continuacion(&numerado), "4) ");
        assert!(item_de_linea("texto suelto", 0, 0).is_none());
        assert!(item_de_linea("-sin espacio", 0, 0).is_none());
    }
}
