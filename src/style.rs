//! Paleta de Orbital: rosa sobre una base oscura, con contraste WCAG AA (constitución §IV).

use gtk::gdk;

/// Color sRGB de 8 bits por canal.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rgb(pub u8, pub u8, pub u8);

/// Acento rosa para fondos (botones sugeridos, selección).
pub const ACCENT_BG: Rgb = Rgb(0xb8, 0x46, 0x7c);
/// Texto sobre el acento.
pub const ACCENT_FG: Rgb = Rgb(0xff, 0xff, 0xff);
/// Acento rosa para texto e íconos sobre la base oscura.
pub const ACCENT_TEXT: Rgb = Rgb(0xff, 0x8c, 0xc6);
/// Anillo de foco (no textual: necesita 3:1 contra lo que lo rodea).
pub const FOCUS_RING: Rgb = ACCENT_TEXT;
/// Anillo de foco sobre botones de acento (el rosa sobre rosa queda en 2,3:1).
pub const FOCUS_ON_ACCENT: Rgb = Rgb(0xff, 0xff, 0xff);
/// Base casi negra.
pub const WINDOW_BG: Rgb = Rgb(0x11, 0x11, 0x13);
pub const VIEW_BG: Rgb = Rgb(0x16, 0x16, 0x18);
pub const SIDEBAR_BG: Rgb = Rgb(0x14, 0x14, 0x17);
pub const HEADERBAR_BG: Rgb = Rgb(0x18, 0x18, 0x1b);
pub const CARD_BG: Rgb = Rgb(0x1e, 0x1e, 0x22);
/// Fondo de las transclusiones `![[nota]]` en la vista formateada (rosado muy oscuro).
pub const EMBED_BG: Rgb = Rgb(0x23, 0x18, 0x20);

// Diseño de interfaz (025): tonos de la maqueta `orbital_estructura_base_v13.html`.
/// Texto atenuado (etiquetas de sección, barra inferior, pestañas inactivas).
pub const MUTED_FG: Rgb = Rgb(0x99, 0x94, 0x9e);
/// Texto principal de la maqueta.
pub const TEXT_FG: Rgb = Rgb(0xee, 0xe9, 0xed);
/// Íconos en reposo.
pub const ICON_FG: Rgb = Rgb(0xbd, 0xb7, 0xc0);
/// Líneas divisorias.
pub const LINE: Rgb = Rgb(0x30, 0x30, 0x36);
/// Fondo del riel de navegación.
pub const RAIL_BG: Rgb = Rgb(0x14, 0x13, 0x16);
/// Botón de ícono bajo el puntero, presionado y elegido en el riel.
pub const HOVER_BG: Rgb = Rgb(0x32, 0x25, 0x30);
pub const PRESSED_BG: Rgb = Rgb(0x36, 0x23, 0x30);
pub const RAIL_ACTIVE_BG: Rgb = Rgb(0x40, 0x23, 0x36);
/// Fila elegida (árbol, resultados, menús).
pub const SELECTED_BG: Rgb = Rgb(0x39, 0x25, 0x32);
/// Texto de la fila elegida.
pub const SELECTED_FG: Rgb = Rgb(0xff, 0xd2, 0xe6);

/// Barra derecha (025): títulos de bloque, chips de etiquetas y tarjetas de contexto.
pub const HEADING_FG: Rgb = Rgb(0xc5, 0xb9, 0xc5);
pub const CHIP_BG: Rgb = Rgb(0x38, 0x25, 0x32);
pub const CHIP_FG: Rgb = Rgb(0xff, 0xad, 0xd5);
pub const CHIP_BORDER: Rgb = Rgb(0x6b, 0x3b, 0x56);
pub const CARD_CTX_BG: Rgb = Rgb(0x21, 0x1c, 0x25);
pub const CARD_CTX_BORDER: Rgb = Rgb(0x3e, 0x30, 0x40);

/// Enlaces a notas existentes.
pub const LINK_FG: Rgb = ACCENT_TEXT;
/// Enlaces a notas pendientes o inexistentes (rosa atenuado con contraste WCAG AA ≥ 4,5:1).
pub const LINK_MISSING_FG: Rgb = Rgb(0xc9, 0x82, 0xa7);

// Colores de comparación (diff)
pub const DIFF_REMOVED_BG: Rgb = Rgb(0x36, 0x1a, 0x24);
#[allow(dead_code)]
pub const DIFF_REMOVED_FG: Rgb = Rgb(0xff, 0xb3, 0xd9);
pub const DIFF_REMOVED_MARKER: Rgb = Rgb(0xff, 0x8c, 0xc6);

pub const DIFF_ADDED_BG: Rgb = Rgb(0x16, 0x29, 0x1f);
#[allow(dead_code)]
pub const DIFF_ADDED_FG: Rgb = Rgb(0xa6, 0xf5, 0xc0);
pub const DIFF_ADDED_MARKER: Rgb = Rgb(0x8f, 0xf0, 0xa4);

// Callouts: los colores de Obsidian en modo oscuro, aclarados lo justo para que el título (en
// ese color) llegue a 4,5:1 sobre el fondo, que es el mismo color al 10 % sobre la vista.
pub const CALLOUT_AZUL: Rgb = Rgb(0x4d, 0x9b, 0xff);
pub const CALLOUT_CIAN: Rgb = Rgb(0x53, 0xdf, 0xdd);
pub const CALLOUT_VERDE: Rgb = Rgb(0x44, 0xcf, 0x6e);
pub const CALLOUT_NARANJA: Rgb = Rgb(0xe9, 0x97, 0x3f);
pub const CALLOUT_ROJO: Rgb = Rgb(0xff, 0x5c, 0x62);
pub const CALLOUT_VIOLETA: Rgb = Rgb(0xa8, 0x82, 0xff);
pub const CALLOUT_GRIS: Rgb = Rgb(0x9e, 0x9e, 0x9e);
/// Opacidad del fondo de un callout.
pub const CALLOUT_ALFA: f64 = 0.10;
/// Todos los colores de callout (para las pruebas de contraste).
#[cfg(test)]
const CALLOUTS: [Rgb; 7] = [
    CALLOUT_AZUL,
    CALLOUT_CIAN,
    CALLOUT_VERDE,
    CALLOUT_NARANJA,
    CALLOUT_ROJO,
    CALLOUT_VIOLETA,
    CALLOUT_GRIS,
];

// Búsqueda en la nota (US3)
/// Resaltado de coincidencias de búsqueda (paleta cálida sobre fondo oscuro, WCAG AA ≥ 4,5:1).
pub const SEARCH_MATCH_BG: Rgb = Rgb(0x5c, 0x48, 0x14);
pub const SEARCH_MATCH_FG: Rgb = Rgb(0xff, 0xe0, 0x82);
/// Coincidencia actual seleccionada.
pub const SEARCH_CURRENT_BG: Rgb = Rgb(0x80, 0x58, 0x0a);
pub const SEARCH_CURRENT_FG: Rgb = Rgb(0xff, 0xff, 0xff);

// Resaltado de sintaxis (paleta rosa y negro)
pub const SYNTAX_TEXT: Rgb = Rgb(0xe6, 0xe6, 0xe6);
pub const SYNTAX_KEYWORD: Rgb = ACCENT_TEXT;
pub const SYNTAX_STRING: Rgb = Rgb(0xf0, 0xa8, 0xd0);
pub const SYNTAX_TYPE: Rgb = Rgb(0xff, 0xb3, 0xd9);
pub const SYNTAX_COMMENT: Rgb = Rgb(0x94, 0x94, 0x98);

// Paleta de formato
#[allow(dead_code)]
pub const FORMAT_ROSA: Rgb = ACCENT_TEXT;
#[allow(dead_code)]
pub const FORMAT_ROJO: Rgb = Rgb(0xff, 0x6b, 0x6b);
#[allow(dead_code)]
pub const FORMAT_NARANJA: Rgb = Rgb(0xff, 0xa9, 0x4d);
#[allow(dead_code)]
pub const FORMAT_AMARILLO: Rgb = Rgb(0xff, 0xd4, 0x3b);
#[allow(dead_code)]
pub const FORMAT_VERDE: Rgb = Rgb(0x69, 0xdb, 0x7c);
#[allow(dead_code)]
pub const FORMAT_AZUL: Rgb = Rgb(0x74, 0xc0, 0xfc);
#[allow(dead_code)]
pub const FORMAT_VIOLETA: Rgb = Rgb(0xb1, 0x97, 0xfc);
#[allow(dead_code)]
pub const FORMAT_GRIS: Rgb = Rgb(0xad, 0xb5, 0xbd);

impl Rgb {
    /// El color con opacidad, para dibujar con `gtk::Snapshot`.
    pub fn rgba(self, alpha: f32) -> gtk::gdk::RGBA {
        gtk::gdk::RGBA::new(
            f32::from(self.0) / 255.0,
            f32::from(self.1) / 255.0,
            f32::from(self.2) / 255.0,
            alpha,
        )
    }

    pub fn hex(self) -> String {
        format!("#{:02x}{:02x}{:02x}", self.0, self.1, self.2)
    }

    /// Luminancia relativa de WCAG 2.x.
    fn luminance(self) -> f64 {
        let lin = |c: u8| {
            let c = f64::from(c) / 255.0;
            if c <= 0.04045 {
                c / 12.92
            } else {
                ((c + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * lin(self.0) + 0.7152 * lin(self.1) + 0.0722 * lin(self.2)
    }
}

/// Relación de contraste de WCAG entre dos colores (de 1 a 21).
pub fn contrast_ratio(fg: Rgb, bg: Rgb) -> f64 {
    let (a, b) = (fg.luminance(), bg.luminance());
    let (hi, lo) = if a > b { (a, b) } else { (b, a) };
    (hi + 0.05) / (lo + 0.05)
}

fn css() -> String {
    // Variables de libadwaita (1.6+) y, por compatibilidad, los colores con nombre.
    let vars = [
        ("accent-bg-color", ACCENT_BG),
        ("accent-fg-color", ACCENT_FG),
        ("accent-color", ACCENT_TEXT),
        ("window-bg-color", WINDOW_BG),
        ("view-bg-color", VIEW_BG),
        ("sidebar-bg-color", SIDEBAR_BG),
        ("headerbar-bg-color", HEADERBAR_BG),
        ("card-bg-color", CARD_BG),
        ("link-fg", LINK_FG),
        ("link-missing-fg", LINK_MISSING_FG),
    ];
    let mut out = String::from(":root {\n");
    for (name, c) in vars {
        out.push_str(&format!("  --{name}: {};\n", c.hex()));
    }
    out.push_str("}\n");
    // Anillo de foco opaco: el de libadwaita (acento al 50 %) queda en 2,9:1 sobre las tarjetas y
    // no llega al 3:1 de WCAG 1.4.11. Con prioridad de aplicación, esta regla gana siempre.
    out.push_str(&format!(
        "*:focus-visible {{ outline-color: {}; }}\n\
         .suggested-action:focus-visible, .destructive-action:focus-visible {{ outline-color: {}; }}\n",
        FOCUS_RING.hex(),
        FOCUS_ON_ACCENT.hex()
    ));
    // Texto de lectura: un poco más grande que la interfaz.
    out.push_str(
        ".orbital-reader { font-size: 1.08em; } \
         .orbital-reader, .orbital-reader text { background: transparent; }\n\
         a.orbital-link-missing, .orbital-link-missing { color: var(--link-missing-fg); text-decoration: underline; }\n\
         popover.orbital-completion-popover > contents { padding: 4px; border-radius: 8px; }\n\
         listview.orbital-completion-list, list.orbital-completion-list { background: transparent; }\n\
         .orbital-medio { background: var(--card-bg-color); border-radius: 10px; padding: 8px 10px; }\n\
         .orbital-medio-pantalla { background: #000000; border-radius: 6px; }\n\
         .orbital-carrusel { background: var(--card-bg-color); border-radius: 10px; padding: 8px; }\n\
         .orbital-carrusel:focus-visible { outline: 2px solid var(--accent-color); outline-offset: 2px; }\n\
         .orbital-carrusel-escenario { background: #000000; border-radius: 6px; }\n\
         .orbital-carrusel-completo { background: #000000; border-radius: 0; padding: 12px; }\n\
         .orbital-carrusel-leyenda { padding: 4px 10px; border-radius: 6px; }\n\
         .orbital-carrusel-miniatura { padding: 2px; border-radius: 6px; border: 2px solid transparent; }\n\
         .orbital-carrusel-miniatura:checked { border-color: var(--accent-color); background: transparent; }\n\
         .orbital-carrusel-marcador { background: alpha(currentColor, 0.08); border-radius: 4px; }\n",
    );
    out.push_str(&props_css());
    out.push_str(&v13_css());
    for (name, c) in vars {
        out.push_str(&format!(
            "@define-color {} {};\n",
            name.replace('-', "_"),
            c.hex()
        ));
    }
    out
}

/// Propiedades arriba de la vista formateada, como las de Obsidian: filas bajas sin caja, la clave
/// en gris con el ícono de su tipo y el valor editable sin borde hasta que se lo toca.
fn props_css() -> String {
    format!(
        ".orbital-props {{ margin-bottom: 14px; }}\n\
         button.orbital-props-titulo {{ min-height: 26px; padding: 0 6px; margin-bottom: 2px; color: {muted}; font-weight: 700; }}\n\
         button.orbital-props-titulo:hover {{ color: {text}; }}\n\
         .orbital-prop {{ min-height: 32px; padding: 0 4px 0 8px; border-radius: 6px; }}\n\
         .orbital-prop:hover, .orbital-prop:focus-within {{ background: alpha(white, 0.04); }}\n\
         .orbital-prop-clave {{ color: {muted}; }}\n\
         .orbital-prop orbital-icono {{ color: {muted}; }}\n\
         entry.orbital-prop-valor {{ background: transparent; border: none; box-shadow: none; outline: none; min-height: 28px; padding: 0 6px; }}\n\
         entry.orbital-prop-valor:focus-within {{ background: alpha(white, 0.06); }}\n\
         .orbital-prop-texto {{ padding: 0 6px; }}\n\
         .orbital-prop button.orbital-prop-boton, .orbital-prop menubutton.orbital-prop-boton > button {{ min-height: 26px; padding: 0 6px; }}\n\
         .orbital-prop-boton label, .orbital-props-agregar label {{ font-weight: normal; }}\n\
         .orbital-prop button.orbital-prop-quitar {{ opacity: 0; min-height: 26px; min-width: 26px; padding: 0; color: {muted}; }}\n\
         .orbital-prop:hover button.orbital-prop-quitar, button.orbital-prop-quitar:focus-visible {{ opacity: 1; }}\n\
         .orbital-pastilla {{ background: {chip_bg}; color: {chip_fg}; border-radius: 999px; padding: 1px 2px 1px 10px; }}\n\
         .orbital-pastilla.solo {{ padding: 1px 10px; }}\n\
         .orbital-pastilla button {{ min-height: 20px; min-width: 20px; padding: 0; color: {chip_fg}; }}\n\
         .orbital-props button.orbital-props-agregar {{ min-height: 28px; padding: 0 8px; color: {muted}; font-weight: normal; }}\n\
         .orbital-props button.orbital-props-agregar:hover {{ color: {text}; }}\n\
         .orbital-props-error {{ padding: 6px 8px; border-radius: 6px; background: alpha({warn}, 0.10); }}\n\
         .orbital-props-sep {{ min-height: 1px; background: {line}; margin-top: 10px; }}\n",
        muted = MUTED_FG.hex(),
        text = TEXT_FG.hex(),
        chip_bg = CHIP_BG.hex(),
        chip_fg = CHIP_FG.hex(),
        warn = CALLOUT_NARANJA.hex(),
        line = LINE.hex(),
    )
}

/// Estilos del diseño de interfaz (025): medidas y tonos de la maqueta. Los pesos de letra son los
/// estándar (normal y 700): pesos intermedios como 650 o 750, y `font-feature-settings`, obligan a
/// cargar otras caras de la fuente en la primera disposición (≈ 50 ms de arranque, medido). Por lo
/// mismo, el texto chico usa un solo tamaño (11 px) y el planeta de la marca es un ícono de trazo.
fn v13_css() -> String {
    format!(
        "headerbar.orbital-top {{ min-height: 48px; background: {top}; box-shadow: inset 0 -1px {line}; padding-left: 0; }}\n\
         headerbar.orbital-top > windowhandle > box {{ padding-left: 0; margin-left: 0; }}\n\
         headerbar.orbital-top box.start {{ padding-left: 0; margin-left: 0; }}\n\
         .orbital-left-slot {{ min-width: 58px; max-width: 58px; padding: 0; margin: 0; }}\n\
         .orbital-left-slot button.orbital-iconbtn, button.orbital-rail-toggle {{ min-width: 38px; min-height: 38px; margin: 0 10px; }}\n\
         .orbital-brand {{ font-size: 18px; font-weight: 700; letter-spacing: -0.5px; }}\n\
         .orbital-planet {{ color: {pink}; }}\n\
         .orbital-pink {{ color: {pink}; }}\n\
         .orbital-left-modes {{ min-width: 250px; padding-left: 10px; }}\n\
         button.orbital-iconbtn {{ min-width: 32px; min-height: 32px; padding: 0; border-radius: 9px; color: {icon}; }}\n\
         button.orbital-iconbtn:hover {{ background: {hover}; color: {pink}; }}\n\
         button.orbital-iconbtn:checked {{ background: {pressed}; color: {pink}; }}\n\
         button.orbital-iconbtn:disabled {{ color: alpha({icon}, 0.4); }}\n\
         .orbital-rail {{ min-width: 58px; background: {rail}; box-shadow: inset -1px 0 {line}; padding: 14px 0; }}\n\
         .orbital-rail button.orbital-iconbtn {{ min-width: 38px; min-height: 38px; margin: 0 10px; }}\n\
         .orbital-rail button.orbital-iconbtn:checked {{ background: {rail_active}; }}\n\
         .orbital-left {{ background: {side}; }}\n\
         .orbital-left-head {{ padding: 12px 10px 10px; box-shadow: inset 0 -1px {line}; }}\n\
         .orbital-left-head button.orbital-iconbtn, .orbital-left-head menubutton > button {{ min-height: 34px; }}\n\
         .orbital-vault-footer {{ padding: 12px; box-shadow: inset 0 1px {line}; }}\n\
         .orbital-section {{ font-size: 11px; letter-spacing: 1.5px; color: {muted}; }}\n\
         .orbital-section:hover {{ color: {pink}; }}\n\
         .orbital-count {{ font-size: 11px; color: {muted}; }}\n\
         .orbital-split > .sidebar-pane {{ box-shadow: inset -1px 0 {line}; }}\n\
         .orbital-right > .sidebar-pane {{ background: {side}; box-shadow: inset 1px 0 {line}; }}\n\
         headerbar.orbital-note-toolbar {{ background: transparent; box-shadow: none; min-height: 40px; padding: 0 9px; }}\n\
         .orbital-note-path {{ color: {muted}; font-size: 11px; padding-left: 10px; }}\n\
         .orbital-status {{ min-height: 34px; padding: 0 6px 0 16px; background: {top}; box-shadow: inset 0 1px {line}; font-size: 11px; color: {muted}; }}\n\
         .orbital-status-tab {{ padding: 0 0 0 12px; background: {top}; border: 1px solid {line}; border-bottom: none; border-radius: 9px 9px 0 0; font-size: 11px; color: {muted}; margin-right: 4px; }}\n\
         .navigation-sidebar > row:selected {{ background: {selected}; color: {selected_fg}; }}\n\
         .orbital-right-head {{ padding: 12px 14px; box-shadow: inset 0 -1px {line}; }}\n\
         .orbital-right-block {{ padding: 20px 20px 8px; }}\n\
         .orbital-right-heading {{ font-size: 11px; font-weight: 700; letter-spacing: 1.2px; color: {heading}; }}\n\
         .orbital-meta-row {{ padding: 13px 0; box-shadow: inset 0 -1px {line}; font-size: 11px; }}\n\
         .orbital-meta-label {{ color: {muted}; }}\n\
         .orbital-meta-value {{ color: {text}; font-weight: 700; }}\n\
         .orbital-tag-chip {{ border: 1px solid {chip_border}; background: {chip_bg}; color: {chip_fg}; padding: 7px 10px; border-radius: 9px; font-size: 11px; }}\n\
         button.orbital-context-card {{ border: 1px solid {card_border}; background: {card_bg}; color: {text}; border-radius: 10px; padding: 12px; font-weight: 700; font-size: 11px; }}\n\
         button.orbital-context-card:hover {{ border-color: #9d577b; }}\n\
         .orbital-context-empty {{ font-size: 11px; color: {muted}; }}\n\
         tabbar.orbital-tabs > .box {{ background: {side}; box-shadow: none; min-height: 42px; padding: 0 12px; }}\n\
         tabbar.orbital-tabs tab {{ border-radius: 9px 9px 0 0; color: {muted}; min-width: 110px; }}\n\
         tabbar.orbital-tabs tab:selected {{ background: {view}; color: {text}; }}\n\
         tabbar.orbital-tabs tab:hover:not(:selected) {{ background: {hover}; }}\n\
         .orbital-explorer-section {{ padding: 6px 8px 4px; }}\n\
         .orbital-explorer-section + .orbital-explorer-section {{ box-shadow: inset 0 1px {line}; }}\n\
         button.orbital-section {{ min-height: 30px; padding: 0 6px; }}\n\
         button.orbital-list-item {{ color: {text}; padding: 6px 7px; font-weight: normal; }}\n\
         button.orbital-list-item:hover {{ background: {selected}; color: {pink}; }}\n",
        top = HEADERBAR_BG.hex(),
        line = LINE.hex(),
        pink = ACCENT_TEXT.hex(),
        icon = ICON_FG.hex(),
        hover = HOVER_BG.hex(),
        pressed = PRESSED_BG.hex(),
        rail = RAIL_BG.hex(),
        rail_active = RAIL_ACTIVE_BG.hex(),
        side = SIDEBAR_BG.hex(),
        muted = MUTED_FG.hex(),
        selected = SELECTED_BG.hex(),
        selected_fg = SELECTED_FG.hex(),
        text = TEXT_FG.hex(),
        view = VIEW_BG.hex(),
        heading = HEADING_FG.hex(),
        chip_bg = CHIP_BG.hex(),
        chip_fg = CHIP_FG.hex(),
        chip_border = CHIP_BORDER.hex(),
        card_bg = CARD_CTX_BG.hex(),
        card_border = CARD_CTX_BORDER.hex(),
    )
}

/// Aplica la base oscura y el acento rosa a toda la app.
pub fn apply() {
    debug_assert!(contrast_ratio(ACCENT_FG, ACCENT_BG) >= 4.5);
    debug_assert!(contrast_ratio(ACCENT_TEXT, WINDOW_BG) >= 4.5);
    adw::StyleManager::default().set_color_scheme(adw::ColorScheme::ForceDark);
    let provider = gtk::CssProvider::new();
    provider.load_from_string(&css());
    if let Some(display) = gdk::Display::default() {
        gtk::style_context_add_provider_for_display(
            &display,
            &provider,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn palette_meets_wcag_aa() {
        for bg in [
            WINDOW_BG,
            VIEW_BG,
            SIDEBAR_BG,
            HEADERBAR_BG,
            CARD_BG,
            EMBED_BG,
        ] {
            let r = contrast_ratio(ACCENT_TEXT, bg);
            assert!(r >= 4.5, "acento sobre {bg:?}: {r:.2}");
            let w = contrast_ratio(Rgb(0xff, 0xff, 0xff), bg);
            assert!(w >= 4.5, "texto sobre {bg:?}: {w:.2}");
        }
        let r = contrast_ratio(ACCENT_FG, ACCENT_BG);
        assert!(r >= 4.5, "texto sobre el acento: {r:.2}");
    }

    #[test]
    fn focus_ring_meets_non_text_contrast() {
        for bg in [WINDOW_BG, VIEW_BG, SIDEBAR_BG, HEADERBAR_BG, CARD_BG] {
            let r = contrast_ratio(FOCUS_RING, bg);
            assert!(r >= 3.0, "foco sobre {bg:?}: {r:.2}");
        }
        // En botones de acento el anillo es blanco: contra el botón y contra la ventana.
        for bg in [ACCENT_BG, WINDOW_BG, CARD_BG] {
            let r = contrast_ratio(FOCUS_ON_ACCENT, bg);
            assert!(r >= 3.0, "foco en acento sobre {bg:?}: {r:.2}");
        }
    }

    #[test]
    fn known_ratios() {
        assert!((contrast_ratio(Rgb(0, 0, 0), Rgb(255, 255, 255)) - 21.0).abs() < 1e-9);
        assert!((contrast_ratio(Rgb(9, 9, 9), Rgb(9, 9, 9)) - 1.0).abs() < 1e-9);
    }

    #[test]
    fn syntax_colors_meet_wcag_aa() {
        for fg in [
            SYNTAX_TEXT,
            SYNTAX_KEYWORD,
            SYNTAX_STRING,
            SYNTAX_TYPE,
            SYNTAX_COMMENT,
        ] {
            for bg in [WINDOW_BG, VIEW_BG] {
                let r = contrast_ratio(fg, bg);
                assert!(r >= 4.5, "sintaxis {fg:?} sobre {bg:?}: {r:.2} < 4.5");
            }
        }
    }

    #[test]
    fn diff_colors_meet_wcag_aa() {
        let r_rem = contrast_ratio(DIFF_REMOVED_FG, DIFF_REMOVED_BG);
        assert!(r_rem >= 4.5, "diff removed texto: {r_rem:.2}");
        let r_rem_m = contrast_ratio(DIFF_REMOVED_MARKER, DIFF_REMOVED_BG);
        assert!(r_rem_m >= 3.0, "diff removed marker: {r_rem_m:.2}");

        let r_add = contrast_ratio(DIFF_ADDED_FG, DIFF_ADDED_BG);
        assert!(r_add >= 4.5, "diff added texto: {r_add:.2}");
        let r_add_m = contrast_ratio(DIFF_ADDED_MARKER, DIFF_ADDED_BG);
        assert!(r_add_m >= 3.0, "diff added marker: {r_add_m:.2}");
    }

    #[test]
    fn callout_colors_meet_wcag_aa() {
        for c in CALLOUTS {
            let fondo = blend(c, CALLOUT_ALFA, VIEW_BG);
            let titulo = contrast_ratio(c, fondo);
            assert!(titulo >= 4.5, "título {}: {titulo:.2} < 4.5", c.hex());
            let cuerpo = contrast_ratio(TEXT_FG, fondo);
            assert!(cuerpo >= 4.5, "cuerpo sobre {}: {cuerpo:.2} < 4.5", c.hex());
        }
    }

    #[test]
    fn link_colors_meet_wcag_aa() {
        // Fondos donde aparecen enlaces: la ventana, la vista, las tarjetas y las transclusiones.
        for fg in [LINK_FG, LINK_MISSING_FG] {
            for bg in [WINDOW_BG, VIEW_BG, CARD_BG, EMBED_BG] {
                let r = contrast_ratio(fg, bg);
                assert!(r >= 4.5, "enlace {fg:?} sobre {bg:?}: {r:.2} < 4.5");
            }
        }
    }

    #[test]
    fn format_colors_meet_wcag_aa() {
        let colors = [
            FORMAT_ROSA,
            FORMAT_ROJO,
            FORMAT_NARANJA,
            FORMAT_AMARILLO,
            FORMAT_VERDE,
            FORMAT_AZUL,
            FORMAT_VIOLETA,
            FORMAT_GRIS,
        ];
        for fg in colors {
            for bg in [WINDOW_BG, VIEW_BG] {
                let r = contrast_ratio(fg, bg);
                assert!(r >= 4.5, "formato {fg:?} sobre {bg:?}: {r:.2} < 4.5");
            }
        }
    }

    /// Mezcla `fg` con opacidad `alpha` sobre `bg`, como dibuja GTK un texto semitransparente.
    fn blend(fg: Rgb, alpha: f64, bg: Rgb) -> Rgb {
        let ch = |f: u8, b: u8| (f64::from(f) * alpha + f64::from(b) * (1.0 - alpha)).round() as u8;
        Rgb(ch(fg.0, bg.0), ch(fg.1, bg.1), ch(fg.2, bg.2))
    }

    /// 004 T046: la marca «fuente» de `raw/`, las cantidades del panel de etiquetas y los
    /// fragmentos de resultados usan `.dim-label` (texto blanco al 55 % en libadwaita) sobre la
    /// barra lateral, las tarjetas y la vista; las etiquetas de Lectura usan el color de enlace.
    #[test]
    fn search_and_tag_labels_meet_wcag_aa() {
        let white = Rgb(0xff, 0xff, 0xff);
        for bg in [WINDOW_BG, VIEW_BG, SIDEBAR_BG, HEADERBAR_BG, CARD_BG] {
            let dim = blend(white, 0.55, bg);
            let r = contrast_ratio(dim, bg);
            assert!(r >= 4.5, "dim-label sobre {bg:?}: {r:.2} < 4.5");
        }
        for bg in [SIDEBAR_BG, CARD_BG] {
            let r = contrast_ratio(LINK_FG, bg);
            assert!(r >= 4.5, "etiqueta sobre {bg:?}: {r:.2} < 4.5");
        }
    }

    /// 025: los tonos de la maqueta v13 cumplen WCAG AA (texto 4,5:1; íconos 3:1).
    #[test]
    fn v13_colors_meet_wcag_aa() {
        for bg in [HEADERBAR_BG, SIDEBAR_BG, RAIL_BG, VIEW_BG, WINDOW_BG] {
            for fg in [MUTED_FG, TEXT_FG, ICON_FG, ACCENT_TEXT] {
                let r = contrast_ratio(fg, bg);
                assert!(r >= 4.5, "{fg:?} sobre {bg:?}: {r:.2} < 4.5");
            }
        }
        for bg in [HOVER_BG, PRESSED_BG, RAIL_ACTIVE_BG, SELECTED_BG] {
            let r = contrast_ratio(ACCENT_TEXT, bg);
            assert!(r >= 4.5, "rosa sobre {bg:?}: {r:.2} < 4.5");
        }
        let r = contrast_ratio(SELECTED_FG, SELECTED_BG);
        assert!(r >= 4.5, "fila elegida: {r:.2} < 4.5");
        for (fg, bg) in [
            (HEADING_FG, SIDEBAR_BG),
            (CHIP_FG, CHIP_BG),
            (TEXT_FG, CARD_CTX_BG),
            (MUTED_FG, CARD_CTX_BG),
        ] {
            let r = contrast_ratio(fg, bg);
            assert!(r >= 4.5, "{fg:?} sobre {bg:?}: {r:.2} < 4.5");
        }
    }

    #[test]
    fn search_match_colors_meet_wcag_aa() {
        let r_match = contrast_ratio(SEARCH_MATCH_FG, SEARCH_MATCH_BG);
        assert!(
            r_match >= 4.5,
            "search match fg sobre bg: {r_match:.2} < 4.5"
        );
        let r_curr = contrast_ratio(SEARCH_CURRENT_FG, SEARCH_CURRENT_BG);
        assert!(
            r_curr >= 4.5,
            "search current fg sobre bg: {r_curr:.2} < 4.5"
        );
    }
}
