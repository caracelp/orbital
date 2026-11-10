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
/// Base casi negra.
pub const WINDOW_BG: Rgb = Rgb(0x11, 0x11, 0x13);
pub const VIEW_BG: Rgb = Rgb(0x16, 0x16, 0x18);
pub const SIDEBAR_BG: Rgb = Rgb(0x14, 0x14, 0x17);
pub const HEADERBAR_BG: Rgb = Rgb(0x18, 0x18, 0x1b);
pub const CARD_BG: Rgb = Rgb(0x1e, 0x1e, 0x22);

impl Rgb {
    fn hex(self) -> String {
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
    ];
    let mut out = String::from(":root {\n");
    for (name, c) in vars {
        out.push_str(&format!("  --{name}: {};\n", c.hex()));
    }
    out.push_str("}\n");
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
        for bg in [WINDOW_BG, VIEW_BG, SIDEBAR_BG, HEADERBAR_BG, CARD_BG] {
            let r = contrast_ratio(ACCENT_TEXT, bg);
            assert!(r >= 4.5, "acento sobre {bg:?}: {r:.2}");
            let w = contrast_ratio(Rgb(0xff, 0xff, 0xff), bg);
            assert!(w >= 4.5, "texto sobre {bg:?}: {w:.2}");
        }
        let r = contrast_ratio(ACCENT_FG, ACCENT_BG);
        assert!(r >= 4.5, "texto sobre el acento: {r:.2}");
    }

    #[test]
    fn known_ratios() {
        assert!((contrast_ratio(Rgb(0, 0, 0), Rgb(255, 255, 255)) - 21.0).abs() < 1e-9);
        assert!((contrast_ratio(Rgb(9, 9, 9), Rgb(9, 9, 9)) - 1.0).abs() < 1e-9);
    }
}
