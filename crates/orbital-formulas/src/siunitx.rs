//! Comandos de unidades de siunitx (051) dentro de una fórmula: `\qty{9.8}{m/s^2}`,
//! `\SI{9.8}{\metre\per\second\squared}`, `\unit{km/h}`, `\si{\kilo\gram}`, `\num{6.022e23}` y
//! `\ang{30}`.
//!
//! El motor no los conoce: antes de disponer la fórmula se cambian por LaTeX que sí conoce, con el
//! espaciado de siunitx: número y unidad separados por un espacio fino (`\,`), igual que las
//! unidades entre sí; las unidades en redonda; `\per` como potencia negativa (`km\,h^{-1}`) y la
//! barra literal (`km/h`) como barra; el exponente `e` como `\times 10^{n}`; `+-` como `\pm`; los
//! dígitos de a tres con un espacio fino desde los cinco dígitos; el grado (`°`), pegado al número.
//! Las opciones entre corchetes (`\qty[…]{…}{…}`) se aceptan y no cambian nada.

use std::borrow::Cow;

use crate::unidades::{Factor, Pieza, piezas};

/// Los comandos que se expanden.
const COMANDOS: &[&str] = &["qty", "SI", "unit", "si", "num", "ang"];

/// La fórmula con los comandos de siunitx expandidos; si no tiene ninguno, la misma.
pub fn expandir(tex: &str) -> Cow<'_, str> {
    if !tex.contains('\\') || !COMANDOS.iter().any(|c| tex.contains(c)) {
        return Cow::Borrowed(tex);
    }
    let b = tex.as_bytes();
    let mut out = String::with_capacity(tex.len() + 32);
    let mut copiado = 0;
    let mut i = 0;
    let mut cambio = false;
    while i < b.len() {
        if b[i] != b'\\' {
            i += 1;
            continue;
        }
        let inicio = i + 1;
        let mut fin = inicio;
        while fin < b.len() && b[fin].is_ascii_alphabetic() {
            fin += 1;
        }
        let cmd = &tex[inicio..fin];
        if fin == inicio {
            // `\\`, `\{`: el carácter escapado no empieza un comando.
            i = fin + 1;
            continue;
        }
        if !COMANDOS.contains(&cmd) {
            i = fin;
            continue;
        }
        let mut j = fin;
        saltear_opciones(tex, &mut j);
        let Some(a) = argumento(tex, &mut j) else {
            i = fin;
            continue;
        };
        let reemplazo = match cmd {
            "qty" | "SI" => {
                // `\SI{1}[\$]{…}` (unidad previa de siunitx 2) también vale.
                saltear_opciones(tex, &mut j);
                let Some(u) = argumento(tex, &mut j) else {
                    i = fin;
                    continue;
                };
                cantidad(a, u)
            }
            "unit" | "si" => unidad(a),
            "num" => numero(a),
            _ => format!("{}^{{\\circ}}", numero(a)),
        };
        out.push_str(&tex[copiado..i]);
        out.push('{');
        out.push_str(&reemplazo);
        out.push('}');
        copiado = j;
        i = j;
        cambio = true;
    }
    if !cambio {
        return Cow::Borrowed(tex);
    }
    out.push_str(&tex[copiado..]);
    Cow::Owned(out)
}

fn saltear_espacios(s: &str, i: &mut usize) {
    while s.as_bytes().get(*i).is_some_and(u8::is_ascii_whitespace) {
        *i += 1;
    }
}

/// `[…]` opcional.
fn saltear_opciones(s: &str, i: &mut usize) {
    let mut j = *i;
    saltear_espacios(s, &mut j);
    if s.as_bytes().get(j) == Some(&b'[')
        && let Some(n) = s[j..].find(']')
    {
        *i = j + n + 1;
    }
}

/// `{…}` con llaves anidadas; devuelve lo de adentro.
fn argumento<'a>(s: &'a str, i: &mut usize) -> Option<&'a str> {
    let b = s.as_bytes();
    let mut j = *i;
    saltear_espacios(s, &mut j);
    if b.get(j) != Some(&b'{') {
        return None;
    }
    let mut nivel = 0usize;
    for (k, &c) in b.iter().enumerate().skip(j) {
        match c {
            b'\\' => {}
            b'{' if k == 0 || b[k - 1] != b'\\' => nivel += 1,
            b'}' if k == 0 || b[k - 1] != b'\\' => {
                nivel -= 1;
                if nivel == 0 {
                    *i = k + 1;
                    return Some(&s[j + 1..k]);
                }
            }
            _ => {}
        }
    }
    None
}

/// Número y unidad: `9.8\,\mathrm{m}/\mathrm{s}^{2}`. El grado va pegado.
fn cantidad(num: &str, uni: &str) -> String {
    let n = numero(num);
    let u = unidad(uni);
    let pegada = matches!(
        piezas(uni).ok().as_deref(),
        Some([Pieza::Factor(Factor { unidad: Some(u), .. }), ..]) if u.pegada
    );
    if pegada || u.is_empty() {
        format!("{n}{u}")
    } else {
        format!("{n}\\,{u}")
    }
}

/// Las unidades en redonda, separadas por espacios finos.
pub fn unidad(s: &str) -> String {
    let Ok(ps) = piezas(s) else {
        // No es una expresión de unidades (por ejemplo, un comando desconocido): se deja tal
        // cual, en redonda, para que el motor diga qué falla.
        return format!("\\mathrm{{{s}}}");
    };
    let mut out = String::with_capacity(s.len() * 4);
    for p in &ps {
        match p {
            Pieza::Factor(f) => {
                let simbolo = match f.unidad {
                    Some(u) if u.tex.is_empty() => Some(u.simbolo),
                    Some(_) => None,
                    None => Some(f.crudo.as_str()),
                };
                match (f.prefijo, simbolo) {
                    // `\mathrm{km}`: prefijo y unidad en un solo grupo.
                    (Some(pr), Some(sim)) if pr.tex.is_empty() => {
                        out.push_str("\\mathrm{");
                        out.push_str(pr.simbolo);
                        out.push_str(sim);
                        out.push('}');
                    }
                    (pr, sim) => {
                        if let Some(pr) = pr {
                            if pr.tex.is_empty() {
                                out.push_str("\\mathrm{");
                                out.push_str(pr.simbolo);
                                out.push('}');
                            } else {
                                out.push_str(pr.tex);
                            }
                        }
                        match (sim, f.unidad) {
                            (Some(sim), _) => {
                                out.push_str("\\mathrm{");
                                out.push_str(sim);
                                out.push('}');
                            }
                            (None, Some(u)) => out.push_str(u.tex),
                            (None, None) => {}
                        }
                    }
                }
                if f.exponente != 1 {
                    out.push_str(&format!("^{{{}}}", f.exponente));
                }
            }
            Pieza::Producto => out.push_str("\\,"),
            Pieza::Barra => out.push('/'),
            Pieza::Abre => out.push('('),
            Pieza::Cierra => out.push(')'),
        }
    }
    out
}

/// Un número de siunitx: `1.5e3` → `1.5\times 10^{3}`, `+-` → `\pm`, `,` decimal sin espacio
/// detrás, dígitos agrupados de a tres desde los cinco.
pub fn numero(s: &str) -> String {
    let s = s.trim();
    let mut out = String::with_capacity(s.len() + 16);
    let b = s.as_bytes();
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        if c.is_ascii_digit()
            || ((c == b'.' || c == b',') && b.get(i + 1).is_some_and(u8::is_ascii_digit))
        {
            // Un número: parte entera, decimal y exponente.
            let inicio = i;
            while i < b.len() && b[i].is_ascii_digit() {
                i += 1;
            }
            let entera = &s[inicio..i];
            out.push_str(&agrupar(entera, false));
            if i < b.len()
                && (b[i] == b'.' || b[i] == b',')
                && b.get(i + 1).is_some_and(u8::is_ascii_digit)
            {
                out.push_str(if b[i] == b',' { "{,}" } else { "." });
                i += 1;
                let d = i;
                while i < b.len() && b[i].is_ascii_digit() {
                    i += 1;
                }
                out.push_str(&agrupar(&s[d..i], true));
            }
            if i < b.len() && (b[i] == b'e' || b[i] == b'E' || b[i] == b'd' || b[i] == b'D') {
                let mut j = i + 1;
                if matches!(b.get(j), Some(b'+' | b'-')) {
                    j += 1;
                }
                let d = j;
                while j < b.len() && b[j].is_ascii_digit() {
                    j += 1;
                }
                if j > d {
                    let exp = s[i + 1..j].trim_start_matches('+');
                    // `e3` solo (sin mantisa) es `10^{3}`.
                    if entera.is_empty() && inicio == i {
                        out.push_str(&format!("10^{{{exp}}}"));
                    } else {
                        out.push_str(&format!("\\times 10^{{{exp}}}"));
                    }
                    i = j;
                }
            }
            continue;
        }
        match c {
            b'+' if b.get(i + 1) == Some(&b'-') => {
                out.push_str("\\pm ");
                i += 2;
            }
            b'x' | b'*' => {
                out.push_str("\\times ");
                i += 1;
            }
            b' ' => i += 1,
            _ => {
                let ch = s[i..].chars().next().unwrap_or(' ');
                match ch {
                    '±' => out.push_str("\\pm "),
                    '×' => out.push_str("\\times "),
                    _ => out.push(ch),
                }
                i += ch.len_utf8();
            }
        }
    }
    out
}

/// Grupos de tres dígitos con `\,` cuando hay cinco o más (siunitx: `group-minimum-digits = 5`).
fn agrupar(d: &str, decimal: bool) -> String {
    if d.len() < 5 {
        return d.to_owned();
    }
    let mut out = String::with_capacity(d.len() * 2);
    if decimal {
        for (k, c) in d.chars().enumerate() {
            if k > 0 && k.is_multiple_of(3) {
                out.push_str("\\,");
            }
            out.push(c);
        }
    } else {
        let primero = d.len() % 3;
        for (k, c) in d.chars().enumerate() {
            if k > 0 && (k + 3 - primero).is_multiple_of(3) {
                out.push_str("\\,");
            }
            out.push(c);
        }
    }
    out
}

/// Una cantidad escrita con siunitx dentro de una fórmula (para convertirla).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CantidadEn {
    /// Rango de todo el comando, de la barra a la última llave.
    pub rango: std::ops::Range<usize>,
    /// `qty` o `SI`.
    pub comando: String,
    pub numero: String,
    pub unidad: String,
}

/// Las cantidades (`\qty{…}{…}`, `\SI{…}{…}`) de una fórmula, en orden.
pub fn cantidades(tex: &str) -> Vec<CantidadEn> {
    let mut out = Vec::new();
    let b = tex.as_bytes();
    let mut i = 0;
    while let Some(n) = tex[i..].find('\\') {
        let inicio = i + n;
        let mut fin = inicio + 1;
        while fin < b.len() && b[fin].is_ascii_alphabetic() {
            fin += 1;
        }
        let cmd = &tex[inicio + 1..fin];
        i = fin.max(inicio + 1).min(tex.len());
        if cmd != "qty" && cmd != "SI" {
            continue;
        }
        let mut j = fin;
        saltear_opciones(tex, &mut j);
        let Some(num) = argumento(tex, &mut j) else {
            continue;
        };
        saltear_opciones(tex, &mut j);
        let Some(uni) = argumento(tex, &mut j) else {
            continue;
        };
        out.push(CantidadEn {
            rango: inicio..j,
            comando: cmd.to_owned(),
            numero: num.trim().to_owned(),
            unidad: uni.trim().to_owned(),
        });
        i = j;
    }
    out
}

/// La cantidad que contiene el byte `offset` (o lo toca), si hay una; si no, la única de la
/// fórmula.
pub fn cantidad_en(tex: &str, offset: usize) -> Option<CantidadEn> {
    let todas = cantidades(tex);
    if let Some(c) = todas
        .iter()
        .find(|c| c.rango.start <= offset && offset <= c.rango.end)
    {
        return Some(c.clone());
    }
    match todas.as_slice() {
        [una] => Some(una.clone()),
        _ => None,
    }
}

/// Lee el número de una cantidad para convertirlo: `1,5`, `1.5e3`, `12\,345`, `−4`.
pub fn leer_numero(s: &str) -> Option<f64> {
    let limpio: String = s
        .replace(r"\,", "")
        .replace(r"\times10^", "e")
        .replace(r"\times 10^", "e")
        .chars()
        .filter(|c| !c.is_whitespace() && *c != '{' && *c != '}')
        .map(|c| match c {
            ',' => '.',
            '\u{2212}' => '-',
            'd' | 'D' => 'e',
            otro => otro,
        })
        .collect();
    limpio.parse::<f64>().ok().filter(|v| v.is_finite())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn busca_cantidades() {
        let t = r"v = \qty{90}{km/h} + \SI[x]{3}{\metre\per\second}";
        let todas = cantidades(t);
        assert_eq!(todas.len(), 2);
        assert_eq!(
            (todas[0].numero.as_str(), todas[0].unidad.as_str()),
            ("90", "km/h")
        );
        assert_eq!(todas[1].comando, "SI");
        assert_eq!(&t[todas[1].rango.clone()], r"\SI[x]{3}{\metre\per\second}");
        assert_eq!(cantidad_en(t, t.find("km").unwrap()).unwrap().numero, "90");
        assert_eq!(cantidad_en(t, 0), None, "dos y ninguna bajo el cursor");
        assert_eq!(cantidad_en(r"\qty{1}{m}", 0).unwrap().unidad, "m");
        assert_eq!(leer_numero("1,5"), Some(1.5));
        assert_eq!(leer_numero("6.022e23"), Some(6.022e23));
        assert_eq!(leer_numero(r"12\,345"), Some(12345.0));
        assert_eq!(leer_numero("1.2+-0.1"), None);
    }

    #[test]
    fn cantidades_y_unidades() {
        assert_eq!(
            expandir(r"v = \qty{90}{km/h}"),
            r"v = {90\,\mathrm{km}/\mathrm{h}}"
        );
        assert_eq!(
            expandir(r"\SI{9.81}{\metre\per\second\squared}"),
            r"{9.81\,\mathrm{m}\,\mathrm{s}^{-2}}"
        );
        assert_eq!(
            expandir(r"\unit{kg.m.s^{-2}}"),
            r"{\mathrm{kg}\,\mathrm{m}\,\mathrm{s}^{-2}}"
        );
        assert_eq!(expandir(r"\si{\kilo\ohm}"), r"{\mathrm{k}\Omega}");
        assert_eq!(
            expandir(r"\qty{3}{\micro\metre}"),
            r"{3\,\text{µ}\mathrm{m}}"
        );
        assert_eq!(
            expandir(r"\qty{25}{\degreeCelsius}"),
            r"{25\,{}^{\circ}\mathrm{C}}"
        );
        assert_eq!(expandir(r"\ang{30}"), r"{30^{\circ}}");
        assert_eq!(expandir(r"\qty{30}{°}"), r"{30^{\circ}}");
        assert_eq!(expandir(r"\qty[mode=math]{2}{m}"), r"{2\,\mathrm{m}}");
    }

    #[test]
    fn numeros() {
        assert_eq!(numero("6.022e23"), r"6.022\times 10^{23}");
        assert_eq!(numero("1.5E-3"), r"1.5\times 10^{-3}");
        assert_eq!(numero("12345.678"), r"12\,345.678");
        assert_eq!(numero("1234"), "1234");
        assert_eq!(numero("0.123456"), r"0.123\,456");
        assert_eq!(numero("3,14"), "3{,}14");
        assert_eq!(numero("1.2+-0.1"), r"1.2\pm 0.1");
        assert_eq!(numero("-40"), "-40");
        assert_eq!(numero("2x3"), r"2\times 3");
    }

    #[test]
    fn no_toca_lo_demas() {
        for t in [
            r"\sin x",
            r"\sigma",
            r"\unitlength",
            r"\numberwithin",
            "x^2",
            r"\qty",
        ] {
            assert!(matches!(expandir(t), Cow::Borrowed(_)), "{t}");
        }
    }
}
