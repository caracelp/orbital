//! Pruebas GTK en proceso del plegado en Código (028, T016): flechitas, plegar y desplegar, y
//! las reglas que nunca esconden lo que se edita o se busca.

use std::fs;
use std::sync::Arc;
use std::time::Instant;

use super::oculto;
use crate::editor::flechas::Flecha;
use crate::editor::{Editor, View};
use gtk::glib;
use gtk::prelude::*;
use orbital_vault::{RelPath, Vault};

/// La nota de prueba del quickstart (la misma que `crates/orbital-vault/tests/plegado.rs`).
pub(crate) const OUTLINE: &str = "---
title: x
---
# Uno
Párrafo.

## Historia
Texto.
- Frutas
  - Manzana
    - Verde
  - Pera

## Ejemplos
uno

## Ejemplos
dos

```
# no es encabezado
- a
  - b
```

> - cita
>   - sub

| a | b |
|---|---|
| 1 | 2 |

- suelto

## Vacío
## Final
";

/// Líneas de los elementos plegables de `OUTLINE`.
const UNO: u32 = 3;
const HISTORIA: u32 = 6;
const FRUTAS: u32 = 8;
const MANZANA: u32 = 9;
const EJEMPLOS2: u32 = 16;

pub(crate) fn boveda(nombre: &str, texto: &str) -> (std::path::PathBuf, Arc<Vault>, RelPath) {
    let root = std::env::temp_dir().join(format!("orbital-028-{nombre}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join("wiki")).unwrap();
    fs::create_dir_all(root.join("raw")).unwrap();
    let rel = RelPath::new("wiki/outline.md").unwrap();
    fs::write(rel.to_path(&root), texto).unwrap();
    (
        root.clone(),
        Arc::new(orbital_vault::open(&root).unwrap()),
        rel,
    )
}

fn abrir(nombre: &str, texto: &str) -> (std::path::PathBuf, Arc<Vault>, RelPath, Editor) {
    adw::init().expect("Se necesita una sesión gráfica GTK");
    let (root, vault, rel) = boveda(nombre, texto);
    let editor = Editor::new();
    editor.load(vault.clone(), &vault.read_note(&rel).unwrap());
    editor.set_view(View::Codigo);
    // Como si ya hubiera llegado el árbol que se calcula en otro hilo al cargar.
    let _ = editor.arbol();
    (root, vault, rel, editor)
}

/// La línea `n` está oculta en Código.
fn oculta(e: &Editor, n: u32) -> bool {
    let b = e.buffer();
    match b.iter_at_line(n as i32) {
        Some(it) => oculto(&it, &e.0.tag_plegado),
        None => false,
    }
}

/// Las líneas ocultas de Código.
fn ocultas(e: &Editor) -> Vec<u32> {
    (0..e.buffer().line_count() as u32)
        .filter(|&n| oculta(e, n))
        .collect()
}

fn cursor_a(e: &Editor, linea: u32, columna: i32) {
    let b = e.buffer();
    let it = b.iter_at_line_offset(linea as i32, columna).unwrap();
    b.place_cursor(&it);
}

fn fin_de_linea(e: &Editor, linea: u32) {
    let b = e.buffer();
    let mut it = b.iter_at_line(linea as i32).unwrap();
    if !it.ends_line() {
        it.forward_to_line_end();
    }
    b.place_cursor(&it);
}

fn linea_cursor(e: &Editor) -> u32 {
    let b = e.buffer();
    b.iter_at_mark(&b.get_insert()).line() as u32
}

fn sha(path: &std::path::Path) -> u64 {
    orbital_vault::write::hash(&fs::read(path).unwrap())
}

/// Escenarios 1, 2 y 5 del quickstart: qué tiene flechita, plegar, desplegar, y la nota en
/// disco nunca cambia por plegar (SC-004).
#[test]
#[ignore = "requiere una sesión gráfica; tools/pruebas-gtk.sh"]
fn flechitas_plegar_y_desplegar_sin_tocar_la_nota() {
    let (root, _vault, rel, e) = abrir("flechitas", OUTLINE);
    glib::MainContext::default().block_on(async {
        let antes = sha(&rel.to_path(&root));
        let con: Vec<u32> = (0..e.buffer().line_count() as u32)
            .filter(|&n| e.flecha(n) != Flecha::Ninguna)
            .collect();
        assert_eq!(con, [UNO, HISTORIA, FRUTAS, MANZANA, 13, EJEMPLOS2]);

        assert!(e.alternar_plegado(HISTORIA));
        assert_eq!(e.flecha(HISTORIA), Flecha::Cerrada);
        assert_eq!(ocultas(&e), (7..12).collect::<Vec<_>>());
        assert!(!oculta(&e, HISTORIA) && !oculta(&e, 12) && !oculta(&e, 13));
        assert!(e.alternar_plegado(HISTORIA));
        assert!(ocultas(&e).is_empty());
        assert_eq!(e.flecha(HISTORIA), Flecha::Abierta);
        // Un ítem: sus subítems.
        assert!(e.plegar(FRUTAS));
        assert_eq!(ocultas(&e), [9, 10, 11]);
        // La última sección, que llega al final del texto.
        assert!(e.plegar(UNO));
        let total = e.buffer().line_count() as u32;
        assert!((4..total).all(|n| oculta(&e, n)), "{:?}", ocultas(&e));
        e.desplegar_todo();
        assert!(ocultas(&e).is_empty());
        // Sin plegables: nada.
        assert!(!e.plegar(7) && !e.plegar(20) && !e.plegar(32));

        e.plegar_todo();
        assert!(!ocultas(&e).is_empty());
        e.flush().await;
        assert_eq!(sha(&rel.to_path(&root)), antes, "plegar no toca la nota");
        let _ = fs::remove_dir_all(&root);
    });
}

/// Escenario 3: desplegar un elemento conserva los pliegues internos.
#[test]
#[ignore = "requiere una sesión gráfica; tools/pruebas-gtk.sh"]
fn anidados_conservan_su_estado() {
    let (root, _v, _r, e) = abrir("anidados", OUTLINE);
    assert!(e.plegar(MANZANA));
    assert!(e.plegar(HISTORIA));
    assert_eq!(ocultas(&e), (7..12).collect::<Vec<_>>());
    assert!(e.desplegar(HISTORIA));
    assert_eq!(ocultas(&e), [10], "Manzana sigue plegado");
    assert!(e.esta_plegado(MANZANA));
    let _ = fs::remove_dir_all(&root);
}

/// Escenario 2 y decisión 7: Ctrl+Arriba/Abajo solo en la línea de un elemento; plegar con el
/// cursor adentro lo lleva al final de la línea del elemento.
#[test]
#[ignore = "requiere una sesión gráfica; tools/pruebas-gtk.sh"]
fn teclas_y_cursor_adentro() {
    let (root, _v, _r, e) = abrir("teclas", OUTLINE);
    cursor_a(&e, FRUTAS, 3);
    assert!(e.tecla_plegar(true));
    assert!(e.esta_plegado(FRUTAS));
    assert!(e.tecla_plegar(false));
    assert!(!e.esta_plegado(FRUTAS));
    cursor_a(&e, 7, 1);
    assert!(
        !e.tecla_plegar(true),
        "en un párrafo, la tecla sigue su camino"
    );
    // Cursor en «Verde» y plegar «Historia» desde la flechita.
    cursor_a(&e, 10, 6);
    assert!(e.alternar_plegado(HISTORIA));
    assert_eq!(linea_cursor(&e), HISTORIA);
    let b = e.buffer();
    assert!(b.iter_at_mark(&b.get_insert()).ends_line());
    // `win.alternar-plegado` en una línea de contenido pliega el más interno que la contiene.
    e.desplegar_todo();
    cursor_a(&e, 11, 3);
    assert!(e.alternar_plegado_en_cursor());
    assert!(e.esta_plegado(FRUTAS));
    let _ = fs::remove_dir_all(&root);
}

/// Escenario 4: escribir en la línea plegada, Enter, mover, sangrar, borrar junto al pliegue.
#[test]
#[ignore = "requiere una sesión gráfica; tools/pruebas-gtk.sh"]
fn editar_alrededor_de_un_pliegue() {
    let (root, _v, _r, e) = abrir("editar", OUTLINE);
    let b = e.buffer().clone();
    assert!(e.plegar(FRUTAS));
    // Escribir al final de la línea del elemento no lo despliega.
    fin_de_linea(&e, FRUTAS);
    b.insert_at_cursor(" frescas");
    e.reconciliar();
    assert!(e.esta_plegado(FRUTAS));
    assert_eq!(ocultas(&e), [9, 10, 11]);
    // Enter: el ítem nuevo va después de los subítems ocultos.
    assert!(e.enter_en_plegado());
    assert!(e.text().contains("  - Pera\n- \n"), "{}", e.text());
    assert_eq!(linea_cursor(&e), 12);
    assert!(e.esta_plegado(FRUTAS));
    assert_eq!(ocultas(&e), [9, 10, 11]);
    b.insert_at_cursor("Dátiles");
    // Alt+Abajo: Frutas se mueve con sus subítems y sigue plegado.
    cursor_a(&e, FRUTAS, 2);
    assert!(e.mover_item_plegado(false));
    assert!(
        e.text()
            .contains("- Dátiles\n- Frutas frescas\n  - Manzana"),
        "{}",
        e.text()
    );
    assert!(e.esta_plegado(9), "{:?}", e.lineas_plegadas());
    assert_eq!(ocultas(&e), [10, 11, 12]);
    // Tab y Mayús+Tab: cambia la sangría de todo el bloque y sigue plegado.
    cursor_a(&e, 9, 2);
    assert!(e.indentar_plegado(true));
    assert!(e.text().contains("  - Frutas frescas\n    - Manzana"));
    assert!(e.esta_plegado(9));
    assert_eq!(ocultas(&e), [10, 11, 12]);
    assert!(e.indentar_plegado(false));
    assert_eq!(ocultas(&e), [10, 11, 12]);
    // Supr al final de la línea del elemento (borra el salto que la separa de lo oculto):
    // se despliega antes de borrar.
    let mut a = b.iter_at_line(9).unwrap();
    a.forward_to_line_end();
    let mut z = a;
    z.forward_char();
    b.delete(&mut a, &mut z);
    assert!(!e.esta_plegado(9));
    assert!(ocultas(&e).is_empty());
    let _ = fs::remove_dir_all(&root);
}

/// Retroceso al comienzo de la línea que sigue a lo plegado: despliega antes de borrar.
#[test]
#[ignore = "requiere una sesión gráfica; tools/pruebas-gtk.sh"]
fn retroceso_despues_del_pliegue() {
    let (root, _v, _r, e) = abrir("retroceso", OUTLINE);
    let b = e.buffer().clone();
    assert!(e.plegar(HISTORIA));
    let mut z = b.iter_at_line(12).unwrap();
    let mut a = z;
    a.backward_char();
    b.delete(&mut a, &mut z);
    assert!(!e.esta_plegado(HISTORIA));
    assert!(ocultas(&e).is_empty());
    let _ = fs::remove_dir_all(&root);
}

/// Escenario 5: buscar y navegar hacia algo oculto lo despliega (SC-005).
#[test]
#[ignore = "requiere una sesión gráfica; tools/pruebas-gtk.sh"]
fn buscar_y_navegar_despliegan() {
    let (root, _v, _r, e) = abrir("navegar", OUTLINE);
    e.plegar_todo();
    e.open_find(false);
    e.find_search("Verde");
    assert_eq!(linea_cursor(&e), 10);
    assert!(!oculta(&e, 10), "la coincidencia se ve: {:?}", ocultas(&e));
    for l in [UNO, HISTORIA, FRUTAS, MANZANA] {
        assert!(!e.esta_plegado(l), "{l}");
    }
    e.close_find();
    // reveal_line (retroenlaces, búsqueda de la bóveda): desde 1.
    e.plegar_todo();
    e.reveal_line(15);
    assert!(!oculta(&e, 14));
    // Encabezado y bloque.
    e.plegar_todo();
    e.scroll_to_heading("Ejemplos");
    assert!(!oculta(&e, 13));
    let (root2, vault2, rel2) = boveda("bloque", "# A\n- x\n  - y ^b1\n    - z\n");
    let e2 = Editor::new();
    e2.load(vault2.clone(), &vault2.read_note(&rel2).unwrap());
    e2.plegar_todo();
    assert!(oculta(&e2, 2));
    assert!(e2.scroll_to_block("b1"));
    assert!(!oculta(&e2, 2), "{:?}", ocultas(&e2));
    // La posición guardada de una pestaña.
    e.plegar_todo();
    let off = e.buffer().iter_at_line_offset(10, 2).unwrap().offset();
    e.restaurar_vista(off, 0.0, None);
    assert!(!oculta(&e, 10));
    let _ = fs::remove_dir_all(&root);
    let _ = fs::remove_dir_all(&root2);
}

/// «Reemplazar todo» despliega antes los elementos con coincidencias.
#[test]
#[ignore = "requiere una sesión gráfica; tools/pruebas-gtk.sh"]
fn reemplazar_todo_despliega() {
    let (root, _v, _r, e) = abrir("reemplazar", OUTLINE);
    e.plegar(FRUTAS);
    e.plegar(EJEMPLOS2);
    e.open_find(true);
    e.find_search("Pera");
    e.find_replace_all("Pomelo");
    assert!(e.text().contains("  - Pomelo\n"));
    assert!(!e.esta_plegado(FRUTAS));
    assert!(
        e.esta_plegado(EJEMPLOS2),
        "lo que no tiene coincidencias sigue plegado"
    );
    assert!(!ocultas(&e).contains(&11));
    let _ = fs::remove_dir_all(&root);
}

/// Un cursor en columna sobre una línea oculta despliega; plegar quita los cursores extra de
/// adentro.
#[test]
#[ignore = "requiere una sesión gráfica; tools/pruebas-gtk.sh"]
fn cursores_extra_y_lo_oculto() {
    let (root, _v, _r, e) = abrir("cursores", OUTLINE);
    e.plegar(MANZANA);
    cursor_a(&e, MANZANA, 4);
    assert!(e.cursor_en_columna(1));
    assert!(
        !e.esta_plegado(MANZANA),
        "la línea del cursor nuevo se despliega"
    );
    assert!(!oculta(&e, 10));
    // Plegar con el cursor extra adentro lo quita.
    assert!(e.plegar(MANZANA));
    assert!(e.0.extra.borrow().is_empty());
    let _ = fs::remove_dir_all(&root);
}

/// Escenario 6: el autoguardado pone ids dentro de lo plegado; nada queda suelto a la vista y
/// lo plegado sigue plegado.
#[test]
#[ignore = "requiere una sesión gráfica; tools/pruebas-gtk.sh"]
fn ids_del_guardado_dentro_de_un_pliegue() {
    let (root, _v, rel, e) = abrir("ids", OUTLINE);
    glib::MainContext::default().block_on(async {
        assert!(e.plegar(HISTORIA));
        cursor_a(&e, 0, 0);
        e.replace_text(&e.text().replacen("# Uno", "# Uno!", 1));
        e.flush().await;
        let disco = fs::read_to_string(rel.to_path(&root)).unwrap();
        assert_eq!(disco, e.text());
        assert!(disco.contains("- Manzana ^"), "{disco}");
        assert!(e.esta_plegado(HISTORIA));
        // Toda la línea con id dentro de lo plegado está oculta, entera.
        for n in 7..12 {
            assert!(oculta(&e, n), "{n}: {:?}", ocultas(&e));
            let b = e.buffer();
            let mut it = b.iter_at_line(n as i32).unwrap();
            let fin = {
                let mut f = it;
                f.forward_to_line_end();
                f
            };
            while it.offset() < fin.offset() {
                assert!(
                    it.has_tag(&e.0.tag_plegado),
                    "carácter visible en la línea {n}"
                );
                it.forward_char();
            }
        }
        // La línea del elemento sigue visible.
        assert!(!oculta(&e, HISTORIA));
        let _ = fs::remove_dir_all(&root);
    });
}

/// Un elemento que deja de ser plegable se despliega solo; deshacer que cambia texto dentro de
/// un pliegue lo despliega antes (el texto nunca cambia escondido).
#[test]
#[ignore = "requiere una sesión gráfica; tools/pruebas-gtk.sh"]
fn deja_de_ser_plegable_y_deshacer() {
    let (root, _v, _r, e) = abrir("deja", "- a\n  - b\n- c\n");
    let b = e.buffer().clone();
    // Deshacer dentro de lo plegado.
    fin_de_linea(&e, 1);
    b.insert_at_cursor("X");
    assert!(e.plegar(0));
    assert!(oculta(&e, 1));
    e.undo();
    assert_eq!(e.text(), "- a\n  - b\n- c\n");
    assert!(!e.esta_plegado(0));
    assert!(!oculta(&e, 1));
    // Se borra el subítem: deja de ser plegable y la flechita desaparece.
    assert!(e.plegar(0));
    let (mut x, mut y) = (b.iter_at_line(1).unwrap(), b.iter_at_line(2).unwrap());
    b.delete(&mut x, &mut y);
    e.reconciliar();
    assert!(!e.tiene_pliegues());
    assert_eq!(e.flecha(0), Flecha::Ninguna);
    let _ = fs::remove_dir_all(&root);
}

/// SC-001: plegar o desplegar un elemento en una nota de 100 KB se ve en menos de 50 ms (hasta
/// el cuadro siguiente, con la vista en una ventana).
#[test]
#[ignore = "requiere una sesión gráfica; tools/pruebas-gtk.sh"]
fn sc001_plegar_en_100k() {
    let mut t = String::new();
    let mut s = 0;
    while t.len() < 100 * 1024 {
        t.push_str(&format!("## Sección {s}\n\nUn párrafo.\n\n"));
        for k in 0..40 {
            t.push_str(&format!("{}- Ítem {k} con texto\n", "  ".repeat(k % 3)));
        }
        s += 1;
    }
    let (root, _v, _r, e) = abrir("sc001", &t);
    let win = gtk::Window::new();
    win.set_default_size(900, 700);
    win.set_child(Some(e.widget()));
    win.present();
    glib::MainContext::default().block_on(async {
        for _ in 0..50 {
            glib::timeout_future(std::time::Duration::from_millis(20)).await;
        }
        let clock = e.0.view.frame_clock().expect("la vista se dibuja");
        let mut peor: f64 = 0.0;
        for linea in [0u32, 44 * 10, 44 * 30] {
            for _ in 0..2 {
                let pintado = std::rc::Rc::new(std::cell::Cell::new(None));
                let p = pintado.clone();
                let h = clock.connect_after_paint(move |_| {
                    if p.get().is_none() {
                        p.set(Some(Instant::now()));
                    }
                });
                let inicio = Instant::now();
                assert!(e.alternar_plegado(linea), "{linea}");
                e.0.view.queue_draw();
                while pintado.get().is_none() && inicio.elapsed().as_secs() < 2 {
                    glib::timeout_future(std::time::Duration::from_millis(1)).await;
                }
                clock.disconnect(h);
                let ms = pintado
                    .get()
                    .map_or(f64::MAX, |t: Instant| (t - inicio).as_secs_f64() * 1000.0);
                eprintln!("SC-001 línea {linea}: {ms:.1} ms");
                peor = peor.max(ms);
            }
        }
        assert!(peor < 50.0, "plegar tardó {peor:.1} ms");
        win.close();
        let _ = fs::remove_dir_all(&root);
    });
}

/// Decisión 14: copiar una selección que abarca algo plegado copia también lo oculto; cortarla
/// no pierde nada (lo cortado se puede pegar entero).
#[test]
#[ignore = "requiere una sesión gráfica; tools/pruebas-gtk.sh"]
fn copiar_y_cortar_incluyen_lo_plegado() {
    let (root, _v, _r, e) = abrir("copiar", OUTLINE);
    glib::MainContext::default().block_on(async {
        let b = e.buffer().clone();
        assert!(e.plegar(HISTORIA));
        let esperado: String = OUTLINE
            .lines()
            .skip(6)
            .take(7)
            .map(|l| format!("{l}\n"))
            .collect();
        let seleccionar = || {
            let (a, z) = (b.iter_at_line(6).unwrap(), b.iter_at_line(13).unwrap());
            b.select_range(&a, &z);
        };
        let portapapeles = e.0.view.clipboard();
        seleccionar();
        e.0.view.emit_copy_clipboard();
        let copiado = portapapeles.read_text_future().await.unwrap().unwrap();
        assert_eq!(copiado.as_str(), esperado, "copiar incluye lo oculto");
        assert_eq!(e.selected_text().as_deref(), Some(esperado.as_str()));
        seleccionar();
        e.0.view.emit_cut_clipboard();
        let cortado = portapapeles.read_text_future().await.unwrap().unwrap();
        assert_eq!(cortado.as_str(), esperado, "cortar no pierde lo oculto");
        assert!(!e.text().contains("Manzana"));
        // Pegar lo cortado (pegar del sistema depende del foco de la ventana: se inserta el
        // texto leído) deja la nota como estaba.
        b.insert_at_cursor(cortado.as_str());
        assert_eq!(e.text(), OUTLINE);
        let _ = fs::remove_dir_all(&root);
    });
}

async fn pausa(ms: u64) {
    glib::timeout_future(std::time::Duration::from_millis(ms)).await;
}

/// Espera a que la vista formateada termine de dibujar (en otro hilo y por tramos).
async fn esperar_lectura(e: &Editor) {
    for _ in 0..250 {
        if !e.0.reader.lineas_con_flecha().is_empty() {
            pausa(50).await;
            return;
        }
        pausa(20).await;
    }
    panic!("la vista formateada no se dibujó");
}

/// El texto `aguja` de la vista formateada está oculto por el plegado.
fn oculto_en_lector(e: &Editor, aguja: &str) -> bool {
    let b = e.0.reader.buffer();
    let texto = b.slice(&b.start_iter(), &b.end_iter(), true).to_string();
    let byte = texto
        .find(aguja)
        .unwrap_or_else(|| panic!("no está «{aguja}»"));
    let off = texto[..byte].chars().count() as i32;
    let tag = b.tag_table().lookup("plegado").unwrap();
    b.iter_at_offset(off).has_tag(&tag)
}

fn anclados(e: &Editor) -> usize {
    let b = e.0.reader.buffer();
    b.slice(&b.start_iter(), &b.end_iter(), true)
        .chars()
        .filter(|c| *c == '\u{fffc}')
        .count()
}

/// Escenario 7: las mismas flechitas en Lectura, el clic alterna, y Lectura y Código comparten
/// el estado; sin widgets anclados nuevos.
#[test]
#[ignore = "requiere una sesión gráfica; tools/pruebas-gtk.sh"]
fn lectura_flechitas_y_estado_compartido() {
    let (root, _v, _r, e) = abrir("lectura", OUTLINE);
    glib::MainContext::default().block_on(async {
        e.set_view(View::Lectura);
        esperar_lectura(&e).await;
        let r = &e.0.reader;
        assert_eq!(
            r.lineas_con_flecha(),
            [UNO, HISTORIA, FRUTAS, MANZANA, 13, EJEMPLOS2]
        );
        let widgets = anclados(&e);
        // Clic en la flechita de «Historia»: la flechita está en el comienzo de su línea.
        let b = r.buffer();
        let texto = b.slice(&b.start_iter(), &b.end_iter(), true).to_string();
        let byte = texto
            .find("▾▸Historia")
            .or_else(|| texto.find("▾▸ Historia"))
            .unwrap();
        let off = texto[..byte].chars().count() as i32;
        assert_eq!(r.clic_en(off), Some(HISTORIA));
        assert!(e.esta_plegado(HISTORIA));
        assert!(oculto_en_lector(&e, "Texto."));
        assert!(oculto_en_lector(&e, "Manzana"));
        assert!(!oculto_en_lector(&e, "uno"));
        assert_eq!(anclados(&e), widgets, "plegar no ancla widgets");
        // En Código, lo mismo.
        e.set_view(View::Codigo);
        assert!(oculta(&e, 7) && oculta(&e, 11) && !oculta(&e, 13));
        // Y al revés: plegar en Código se ve en Lectura.
        assert!(e.desplegar(HISTORIA));
        assert!(e.plegar(MANZANA));
        e.set_view(View::Lectura);
        pausa(400).await;
        assert!(oculto_en_lector(&e, "Verde"));
        assert!(!oculto_en_lector(&e, "Texto."));
        let _ = fs::remove_dir_all(&root);
    });
}

/// Tab llega a las flechitas como a los enlaces; Enter las alterna.
#[test]
#[ignore = "requiere una sesión gráfica; tools/pruebas-gtk.sh"]
fn lectura_teclado() {
    let (root, _v, _r, e) = abrir("lectura-tab", OUTLINE);
    glib::MainContext::default().block_on(async {
        e.set_view(View::Lectura);
        esperar_lectura(&e).await;
        let r = &e.0.reader;
        let n = r
            .urls_tab()
            .iter()
            .position(|u| *u == format!("orbital-plegar:{FRUTAS}"))
            .expect("la flechita de Frutas está en la lista de Tab");
        assert!(r.activar_tab(n));
        assert!(e.esta_plegado(FRUTAS));
        // Lo plegado sale de la lista que recorre Tab.
        let urls = r.urls_tab();
        assert!(
            !urls.contains(&format!("orbital-plegar:{MANZANA}")),
            "{urls:?}"
        );
        let _ = fs::remove_dir_all(&root);
    });
}

/// Dividida: plegar de un lado se ve del otro; escribir en una sección vecina (actualización en
/// el sitio) no muestra lo plegado; buscar en Lectura hacia algo oculto despliega.
#[test]
#[ignore = "requiere una sesión gráfica; tools/pruebas-gtk.sh"]
fn dividida_y_buscar_en_lectura() {
    let (root, _v, _r, e) = abrir("dividida", OUTLINE);
    glib::MainContext::default().block_on(async {
        e.set_view(View::Dividida);
        esperar_lectura(&e).await;
        assert!(e.plegar(HISTORIA));
        assert!(oculto_en_lector(&e, "Manzana"));
        // Escribir en «## Ejemplos» (la sección vecina).
        let b = e.buffer().clone();
        let mut it = b.iter_at_line(14).unwrap();
        it.forward_to_line_end();
        b.place_cursor(&it);
        b.insert_at_cursor(" más texto");
        pausa(700).await;
        assert!(e.text().contains("uno más texto"));
        assert!(!oculto_en_lector(&e, "uno más texto"));
        assert!(
            oculto_en_lector(&e, "Manzana"),
            "la actualización en el sitio no la muestra"
        );
        // Lectura y buscar «Verde» con todo plegado.
        e.set_view(View::Lectura);
        pausa(400).await;
        e.plegar_todo();
        assert!(oculto_en_lector(&e, "Verde"));
        e.open_find(false);
        e.find_search("Verde");
        assert!(!oculto_en_lector(&e, "Verde"));
        assert!(!e.esta_plegado(HISTORIA) && !e.esta_plegado(MANZANA));
        let _ = fs::remove_dir_all(&root);
    });
}

/// Una nota incrustada con secciones no tiene flechitas (se pliega en su propia nota).
#[test]
#[ignore = "requiere una sesión gráfica; tools/pruebas-gtk.sh"]
fn incrustado_sin_flechitas() {
    adw::init().expect("Se necesita una sesión gráfica GTK");
    let (root, vault, _) = boveda("incrustado", "# Otra\ntexto\n- a\n  - b\n");
    fs::write(root.join("wiki/otra.md"), "# Otra\ntexto\n- a\n  - b\n").unwrap();
    fs::write(root.join("wiki/nota.md"), "# Propia\nalgo\n\n![[otra]]\n").unwrap();
    let vault = Arc::new(orbital_vault::open(&root).unwrap_or_else(|_| (*vault).clone()));
    let rel = RelPath::new("wiki/nota.md").unwrap();
    let e = Editor::new();
    e.load(vault.clone(), &vault.read_note(&rel).unwrap());
    glib::MainContext::default().block_on(async {
        e.set_view(View::Lectura);
        esperar_lectura(&e).await;
        assert_eq!(e.0.reader.lineas_con_flecha(), [0]);
        let b = e.0.reader.buffer();
        let texto = b.slice(&b.start_iter(), &b.end_iter(), true).to_string();
        assert!(texto.contains("Otra"), "se incrustó: {texto}");
        assert_eq!(texto.matches('▾').count(), 1, "{texto}");
        let _ = fs::remove_dir_all(&root);
    });
}
