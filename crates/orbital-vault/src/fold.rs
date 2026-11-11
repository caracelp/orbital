//! Normalización sin tildes ni mayúsculas (R2, spec 004).
//!
//! Pasa el texto a minúsculas y quita diacríticos del alfabeto latino (Latin-1 y Latin Extended-A).
//! Invariante fundamental: **un carácter de entrada siempre produce exactamente un carácter de salida**,
//! preservando los índices de carácter para resaltado y rangos sin necesidad de tablas de mapeo.

/// Pliega un carácter individual a minúscula y sin diacrítico.
pub fn fold_char(c: char) -> char {
    match c {
        // ASCII
        'A'..='Z' => c.to_ascii_lowercase(),

        // Latin-1 Supplement: A
        'À'..='Å' | 'à'..='å' => 'a',

        // Latin-1 Supplement: C
        'Ç' | 'ç' => 'c',

        // Latin-1 Supplement: E
        'È'..='Ë' | 'è'..='ë' => 'e',

        // Latin-1 Supplement: I
        'Ì'..='Ï' | 'ì'..='ï' => 'i',

        // Latin-1 Supplement: D
        'Ð' | 'ð' => 'd',

        // Latin-1 Supplement: N
        'Ñ' | 'ñ' => 'n',

        // Latin-1 Supplement: O
        'Ò'..='Ö' | 'Ø' | 'ò'..='ö' | 'ø' => 'o',

        // Latin-1 Supplement: U
        'Ù'..='Ü' | 'ù'..='ü' => 'u',

        // Latin-1 Supplement: Y
        'Ý' | 'ý' | 'ÿ' => 'y',

        // Latin-1 Supplement: S (scharfes S)
        // Se pliega a un solo carácter 's' para mantener el invariante 1 char -> 1 char.
        'ß' => 's',

        // Latin Extended-A: A
        'Ā' | 'ā' | 'Ă' | 'ă' | 'Ą' | 'ą' => 'a',

        // Latin Extended-A: C
        'Ć' | 'ć' | 'Ĉ' | 'ĉ' | 'Ċ' | 'ċ' | 'Č' | 'č' => 'c',

        // Latin Extended-A: D
        'Ď' | 'ď' | 'Đ' | 'đ' => 'd',

        // Latin Extended-A: E
        'Ē' | 'ē' | 'Ĕ' | 'ĕ' | 'Ė' | 'ė' | 'Ę' | 'ę' | 'Ě' | 'ě' => 'e',

        // Latin Extended-A: G
        'Ĝ' | 'ĝ' | 'Ğ' | 'ğ' | 'Ġ' | 'ġ' | 'Ģ' | 'ģ' => 'g',

        // Latin Extended-A: H
        'Ĥ' | 'ĥ' | 'Ħ' | 'ħ' => 'h',

        // Latin Extended-A: I
        'Ĩ' | 'ĩ' | 'Ī' | 'ī' | 'Ĭ' | 'ĭ' | 'Į' | 'į' | 'İ' | 'ı' => 'i',

        // Latin Extended-A: J
        'Ĵ' | 'ĵ' => 'j',

        // Latin Extended-A: K
        'Ķ' | 'ķ' | 'ĸ' => 'k',

        // Latin Extended-A: L
        'Ĺ' | 'ĺ' | 'Ļ' | 'ļ' | 'Ľ' | 'ľ' | 'Ŀ' | 'ŀ' | 'Ł' | 'ł' => 'l',

        // Latin Extended-A: N
        'Ń' | 'ń' | 'Ņ' | 'ņ' | 'Ň' | 'ň' | 'ŉ' | 'Ŋ' | 'ŋ' => 'n',

        // Latin Extended-A: O
        'Ō' | 'ō' | 'Ŏ' | 'ŏ' | 'Ő' | 'ő' => 'o',

        // Latin Extended-A: R
        'Ŕ' | 'ŕ' | 'Ŗ' | 'ŗ' | 'Ř' | 'ř' => 'r',

        // Latin Extended-A: S
        'Ś' | 'ś' | 'Ŝ' | 'ŝ' | 'Ş' | 'ş' | 'Š' | 'š' => 's',

        // Latin Extended-A: T
        'Ţ' | 'ţ' | 'Ť' | 'ť' | 'Ŧ' | 'ŧ' => 't',

        // Latin Extended-A: U
        'Ũ' | 'ũ' | 'Ū' | 'ū' | 'Ŭ' | 'ŭ' | 'Ů' | 'ů' | 'Ű' | 'ű' | 'Ų' | 'ų' => 'u',

        // Latin Extended-A: W
        'Ŵ' | 'ŵ' => 'w',

        // Latin Extended-A: Y
        'Ŷ' | 'ŷ' | 'Ÿ' => 'y',

        // Latin Extended-A: Z
        'Ź' | 'ź' | 'Ż' | 'ż' | 'Ž' | 'ž' => 'z',

        // Latin Extended-B (comunes en rumano y otros alfabetos latinos)
        'Ș' | 'ș' => 's',
        'Ț' | 'ț' => 't',

        // Todo lo demás: minúscula si da 1 solo char, sino dejar igual
        _ => {
            let mut lower = c.to_lowercase();
            let first = lower.next().unwrap_or(c);
            if lower.next().is_none() { first } else { c }
        }
    }
}

/// Normaliza una cadena de texto pasando a minúsculas y quitando diacríticos latinos.
///
/// Conserva siempre la cantidad exacta de caracteres (en `chars().count()`).
pub fn fold(text: &str) -> String {
    text.chars().map(fold_char).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fold_lowercase_and_ascii() {
        assert_eq!(fold("HOLA MUNDO"), "hola mundo");
        assert_eq!(fold("orbital"), "orbital");
        assert_eq!(fold("123 ABC !?"), "123 abc !?");
    }

    #[test]
    fn fold_accents_and_diaeresis() {
        assert_eq!(fold("canción"), "cancion");
        assert_eq!(fold("CANCIÓN"), "cancion");
        assert_eq!(fold("pingüino"), "pinguino");
        assert_eq!(fold("PINGÜINO"), "pinguino");
        assert_eq!(
            fold("árbol épico íntimo ómnibus"),
            "arbol epico intimo omnibus"
        );
    }

    #[test]
    fn fold_enie_and_cedilla() {
        assert_eq!(fold("año español"), "ano espanol");
        assert_eq!(fold("AÑO ESPAÑOL"), "ano espanol");
        assert_eq!(fold("façade"), "facade");
        assert_eq!(fold("GARÇON"), "garcon");
    }

    #[test]
    fn fold_preserves_char_count() {
        let examples = [
            "Straße",
            "Canción de otoño con pingüinos en el río",
            "Ça va très bien, señor",
            "12345!@#$%",
            "日本語テキスト",
            "🚀 orbital 🛸",
            "Crème brûlée",
        ];
        for s in examples {
            let folded = fold(s);
            assert_eq!(
                folded.chars().count(),
                s.chars().count(),
                "La longitud en caracteres debe conservarse para {s:?}"
            );
        }
        // Verificar específicamente que 'ß' no se expanda a "ss"
        assert_eq!(fold("Straße"), "strase");
        assert_eq!(fold("Straße").chars().count(), 6);
    }

    #[test]
    fn fold_leaves_non_latin_intact_or_lowercased() {
        // CJK queda intacto
        assert_eq!(fold("日本語"), "日本語");
        // Símbolos y emojis quedan intactos
        assert_eq!(fold("★ 100% #tag"), "★ 100% #tag");
        // Cirílico se pasa a minúsculas preservando cantidad de caracteres
        assert_eq!(fold("МОСКВА"), "москва");
        assert_eq!(fold("МОСКВА").chars().count(), "МОСКВА".chars().count());
    }

    #[test]
    fn fold_latin_extended_a() {
        // Polaco, checo, rumano, turco
        assert_eq!(fold("Zażółć gęślą jaźń"), "zazolc gesla jazn");
        assert_eq!(fold("Příliš žluťoučký kůň"), "prilis zlutoucky kun");
        assert_eq!(fold("Chișinău"), "chisinau");
        assert_eq!(fold("İstanbul"), "istanbul");
    }
}
