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

// Callouts
pub const CALLOUT_NOTA_BG: Rgb = Rgb(0x1b, 0x24, 0x33);
#[allow(dead_code)]
pub const CALLOUT_NOTA_FG: Rgb = Rgb(0xcb, 0xe0, 0xff);

pub const CALLOUT_CONSEJO_BG: Rgb = Rgb(0x16, 0x29, 0x1f);
#[allow(dead_code)]
pub const CALLOUT_CONSEJO_FG: Rgb = Rgb(0xa6, 0xf5, 0xc0);

pub const CALLOUT_IMPORTANTE_BG: Rgb = Rgb(0x2a, 0x1a, 0x33);
#[allow(dead_code)]
pub const CALLOUT_IMPORTANTE_FG: Rgb = Rgb(0xf0, 0xc2, 0xff);

pub const CALLOUT_ADVERTENCIA_BG: Rgb = Rgb(0x33, 0x29, 0x0f);
#[allow(dead_code)]
pub const CALLOUT_ADVERTENCIA_FG: Rgb = Rgb(0xff, 0xe0, 0x82);

// Búsqueda en la nota (US3)
/// Resaltado de coincidencias de búsqueda (paleta cálida sobre fondo oscuro, WCAG AA ≥ 4,5:1).
pub const SEARCH_MATCH_BG: Rgb = Rgb(0x5c, 0x48, 0x14);
pub const SEARCH_MATCH_FG: Rgb = Rgb(0xff, 0xe0, 0x82);
/// Coincidencia actual seleccionada.
pub const SEARCH_CURRENT_BG: Rgb = Rgb(0x80, 0x58, 0x0a);
pub const SEARCH_CURRENT_FG: Rgb = Rgb(0xff, 0xff, 0xff);

pub const CALLOUT_PELIGRO_BG: Rgb = Rgb(0x36, 0x1a, 0x1d);
#[allow(dead_code)]
pub const CALLOUT_PELIGRO_FG: Rgb = Rgb(0xff, 0xb3, 0xba);

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
         listview.orbital-completion-list, list.orbital-completion-list { background: transparent; }\n",
    );
    for (name, c) in vars {
        out.push_str(&format!(
            "@define-color {} {};\n",
            name.replace('-', "_"),
            c.hex()
        ));
    }
    out
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
        let pairs = [
            (CALLOUT_NOTA_FG, CALLOUT_NOTA_BG, "nota"),
            (CALLOUT_CONSEJO_FG, CALLOUT_CONSEJO_BG, "consejo"),
            (CALLOUT_IMPORTANTE_FG, CALLOUT_IMPORTANTE_BG, "importante"),
            (
                CALLOUT_ADVERTENCIA_FG,
                CALLOUT_ADVERTENCIA_BG,
                "advertencia",
            ),
            (CALLOUT_PELIGRO_FG, CALLOUT_PELIGRO_BG, "peligro"),
        ];
        for (fg, bg, name) in pairs {
            let r = contrast_ratio(fg, bg);
            assert!(r >= 4.5, "callout {name}: {r:.2} < 4.5");
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
