//! Fusión de tres vías de notas de texto (decisión 4): si los cambios de este equipo y los de Drive
//! no tocan los mismos renglones, se unen sin preguntar. Las dos versiones originales van al
//! historial antes de escribir el resultado (lo hace el motor).

use similar::TextMerge;

/// Une `local` y `remota` a partir de `base`. `None` si los cambios se pisan.
pub fn fusionar(base: &str, local: &str, remota: &str) -> Option<String> {
    let m = TextMerge::from_lines(base, local, remota);
    if m.is_conflicted() {
        return None;
    }
    Some(m.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn une_cambios_separados() {
        let base = "# Título\n\nprincipio\n\nmedio\n\nfinal\n";
        let local = "# Título\n\nPRINCIPIO cambiado acá\n\nmedio\n\nfinal\n";
        let remota = "# Título\n\nprincipio\n\nmedio\n\nfinal cambiado en la otra\n";
        assert_eq!(
            fusionar(base, local, remota).as_deref(),
            Some("# Título\n\nPRINCIPIO cambiado acá\n\nmedio\n\nfinal cambiado en la otra\n")
        );
    }

    #[test]
    fn no_une_si_se_pisan() {
        let base = "a\nb\nc\n";
        assert_eq!(fusionar(base, "a\nB1\nc\n", "a\nB2\nc\n"), None);
    }

    #[test]
    fn el_mismo_cambio_en_los_dos_lados() {
        let base = "a\nb\n";
        assert_eq!(
            fusionar(base, "a\nX\n", "a\nX\n").as_deref(),
            Some("a\nX\n")
        );
    }

    #[test]
    fn sin_salto_final_y_letras_no_ascii() {
        let base = "ñandú\nb\nc";
        assert_eq!(
            fusionar(base, "ÑANDÚ\nb\nc", "ñandú\nb\nc — ¡sí!").as_deref(),
            Some("ÑANDÚ\nb\nc — ¡sí!")
        );
    }
}
