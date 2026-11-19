//! La paleta del editor de ecuaciones (051): símbolos y plantillas que insertan LaTeX, por
//! categoría. Sin GTK: la interfaz arma un botón por elemento con su `etiqueta` visible, su
//! `nombre` como nombre accesible y la `plantilla` en el tooltip.
//!
//! En una plantilla, cada `◌` es un hueco: el primero recibe lo seleccionado y el cursor queda en
//! el primer hueco que quede vacío (ver [`aplicar`]).

/// El hueco de una plantilla.
pub const HUECO: char = '◌';

/// Un botón de la paleta.
#[derive(Debug)]
pub struct Elemento {
    /// Lo que se ve en el botón (Unicode, sin dibujar con el motor: barato de armar).
    pub etiqueta: &'static str,
    /// Nombre accesible, en español.
    pub nombre: &'static str,
    pub plantilla: &'static str,
}

/// Una página de la paleta.
#[derive(Debug)]
pub struct Categoria {
    pub id: &'static str,
    pub nombre: &'static str,
    pub elementos: &'static [Elemento],
}

macro_rules! e {
    ($et:literal, $n:literal, $p:literal) => {
        Elemento {
            etiqueta: $et,
            nombre: $n,
            plantilla: $p,
        }
    };
}

pub static CATEGORIAS: &[Categoria] = &[
    Categoria {
        id: "estructuras",
        nombre: "Estructuras",
        elementos: &[
            e!("a⁄b", "Fracción", r"\frac{◌}{◌}"),
            e!("√x", "Raíz cuadrada", r"\sqrt{◌}"),
            e!("ⁿ√x", "Raíz enésima", r"\sqrt[◌]{◌}"),
            e!("xⁿ", "Potencia", r"{◌}^{◌}"),
            e!("xᵢ", "Subíndice", r"{◌}_{◌}"),
            e!("xᵢⁿ", "Subíndice y potencia", r"{◌}_{◌}^{◌}"),
            e!("eˣ", "Exponencial", r"e^{◌}"),
            e!("∑", "Sumatoria", r"\sum_{◌}^{◌}"),
            e!("∏", "Productoria", r"\prod_{◌}^{◌}"),
            e!("∫", "Integral", r"\int_{◌}^{◌} ◌ \, d◌"),
            e!("∬", "Integral doble", r"\iint_{◌} ◌ \, dA"),
            e!("∮", "Integral de contorno", r"\oint_{◌} ◌"),
            e!("lim", "Límite", r"\lim_{◌ \to ◌}"),
            e!("d/dx", "Derivada", r"\frac{d◌}{d◌}"),
            e!("∂/∂x", "Derivada parcial", r"\frac{\partial ◌}{\partial ◌}"),
            e!("( )", "Paréntesis ajustables", r"\left( ◌ \right)"),
            e!("[ ]", "Corchetes ajustables", r"\left[ ◌ \right]"),
            e!("{ }", "Llaves ajustables", r"\left\{ ◌ \right\}"),
            e!("|x|", "Valor absoluto", r"\left| ◌ \right|"),
            e!("‖x‖", "Norma", r"\left\| ◌ \right\|"),
            e!("⌊x⌋", "Parte entera", r"\lfloor ◌ \rfloor"),
            e!("x⃗", "Vector", r"\vec{◌}"),
            e!("x̄", "Barra", r"\overline{◌}"),
            e!("x̂", "Sombrero", r"\hat{◌}"),
            e!("ẋ", "Punto (derivada temporal)", r"\dot{◌}"),
            e!("x̃", "Tilde", r"\tilde{◌}"),
            e!("⏞", "Llave arriba", r"\overbrace{◌}^{◌}"),
            e!("⏟", "Llave abajo", r"\underbrace{◌}_{◌}"),
            e!("(ⁿₖ)", "Coeficiente binomial", r"\binom{◌}{◌}"),
            e!("abc", "Texto", r"\text{◌}"),
            e!("𝐱", "Negrita", r"\mathbf{◌}"),
            e!("ℝ", "Conjunto (pizarra)", r"\mathbb{◌}"),
            e!("𝒜", "Caligráfica", r"\mathcal{◌}"),
            e!("☐", "Recuadro", r"\boxed{◌}"),
        ],
    },
    Categoria {
        id: "matrices",
        nombre: "Matrices y sistemas",
        elementos: &[
            e!(
                "(2×2)",
                "Matriz 2 por 2 entre paréntesis",
                r"\begin{pmatrix} ◌ & ◌ \\ ◌ & ◌ \end{pmatrix}"
            ),
            e!(
                "[2×2]",
                "Matriz 2 por 2 entre corchetes",
                r"\begin{bmatrix} ◌ & ◌ \\ ◌ & ◌ \end{bmatrix}"
            ),
            e!(
                "|2×2|",
                "Determinante 2 por 2",
                r"\begin{vmatrix} ◌ & ◌ \\ ◌ & ◌ \end{vmatrix}"
            ),
            e!(
                "(3×3)",
                "Matriz 3 por 3",
                r"\begin{pmatrix} ◌ & ◌ & ◌ \\ ◌ & ◌ & ◌ \\ ◌ & ◌ & ◌ \end{pmatrix}"
            ),
            e!(
                "(n×1)",
                "Vector columna",
                r"\begin{pmatrix} ◌ \\ ◌ \\ ◌ \end{pmatrix}"
            ),
            e!(
                "Iₙ",
                "Matriz con puntos suspensivos",
                r"\begin{pmatrix} a_{11} & \cdots & a_{1n} \\ \vdots & \ddots & \vdots \\ a_{m1} & \cdots & a_{mn} \end{pmatrix}"
            ),
            e!(
                "{ casos",
                "Por casos",
                r"\begin{cases} ◌ & \text{si } ◌ \\ ◌ & \text{si no} \end{cases}"
            ),
            e!(
                "=\n=",
                "Igualdades alineadas",
                r"\begin{aligned} ◌ &= ◌ \\ &= ◌ \end{aligned}"
            ),
            e!(
                "{ sistema",
                "Sistema de ecuaciones",
                r"\left\{ \begin{aligned} ◌ &= ◌ \\ ◌ &= ◌ \end{aligned} \right."
            ),
            e!(
                "a|b",
                "Tabla",
                r"\begin{array}{c|c} ◌ & ◌ \\ \hline ◌ & ◌ \end{array}"
            ),
            e!("⋯", "Puntos horizontales", r"\cdots"),
            e!("⋮", "Puntos verticales", r"\vdots"),
            e!("⋱", "Puntos diagonales", r"\ddots"),
            e!("Aᵀ", "Traspuesta", r"{◌}^{\mathsf{T}}"),
            e!("A⁻¹", "Inversa", r"{◌}^{-1}"),
        ],
    },
    Categoria {
        id: "griegas",
        nombre: "Letras griegas",
        elementos: &[
            e!("α", "alfa", r"\alpha"),
            e!("β", "beta", r"\beta"),
            e!("γ", "gamma", r"\gamma"),
            e!("δ", "delta", r"\delta"),
            e!("ε", "épsilon", r"\varepsilon"),
            e!("ζ", "zeta", r"\zeta"),
            e!("η", "eta", r"\eta"),
            e!("θ", "theta", r"\theta"),
            e!("ι", "iota", r"\iota"),
            e!("κ", "kappa", r"\kappa"),
            e!("λ", "lambda", r"\lambda"),
            e!("μ", "mu", r"\mu"),
            e!("ν", "nu", r"\nu"),
            e!("ξ", "xi", r"\xi"),
            e!("π", "pi", r"\pi"),
            e!("ρ", "rho", r"\rho"),
            e!("σ", "sigma", r"\sigma"),
            e!("τ", "tau", r"\tau"),
            e!("υ", "ípsilon", r"\upsilon"),
            e!("φ", "fi", r"\varphi"),
            e!("χ", "ji", r"\chi"),
            e!("ψ", "psi", r"\psi"),
            e!("ω", "omega", r"\omega"),
            e!("Γ", "Gamma mayúscula", r"\Gamma"),
            e!("Δ", "Delta mayúscula", r"\Delta"),
            e!("Θ", "Theta mayúscula", r"\Theta"),
            e!("Λ", "Lambda mayúscula", r"\Lambda"),
            e!("Ξ", "Xi mayúscula", r"\Xi"),
            e!("Π", "Pi mayúscula", r"\Pi"),
            e!("Σ", "Sigma mayúscula", r"\Sigma"),
            e!("Φ", "Fi mayúscula", r"\Phi"),
            e!("Ψ", "Psi mayúscula", r"\Psi"),
            e!("Ω", "Omega mayúscula", r"\Omega"),
        ],
    },
    Categoria {
        id: "operadores",
        nombre: "Operadores y relaciones",
        elementos: &[
            e!("±", "Más o menos", r"\pm"),
            e!("∓", "Menos o más", r"\mp"),
            e!("×", "Por", r"\times"),
            e!("÷", "Dividido", r"\div"),
            e!("·", "Producto (punto)", r"\cdot"),
            e!("∘", "Composición", r"\circ"),
            e!("≠", "Distinto", r"\neq"),
            e!("≤", "Menor o igual", r"\leq"),
            e!("≥", "Mayor o igual", r"\geq"),
            e!("≪", "Mucho menor", r"\ll"),
            e!("≫", "Mucho mayor", r"\gg"),
            e!("≈", "Aproximadamente", r"\approx"),
            e!("≡", "Equivalente", r"\equiv"),
            e!("≅", "Congruente", r"\cong"),
            e!("∼", "Semejante", r"\sim"),
            e!("∝", "Proporcional", r"\propto"),
            e!("∞", "Infinito", r"\infty"),
            e!("∂", "Parcial", r"\partial"),
            e!("∇", "Nabla", r"\nabla"),
            e!("∈", "Pertenece", r"\in"),
            e!("∉", "No pertenece", r"\notin"),
            e!("⊂", "Subconjunto", r"\subset"),
            e!("⊆", "Subconjunto o igual", r"\subseteq"),
            e!("∪", "Unión", r"\cup"),
            e!("∩", "Intersección", r"\cap"),
            e!("∖", "Diferencia de conjuntos", r"\setminus"),
            e!("∅", "Conjunto vacío", r"\emptyset"),
            e!("∀", "Para todo", r"\forall"),
            e!("∃", "Existe", r"\exists"),
            e!("¬", "No", r"\neg"),
            e!("∧", "Y", r"\land"),
            e!("∨", "O", r"\lor"),
            e!("⊕", "Suma directa", r"\oplus"),
            e!("⊗", "Producto tensorial", r"\otimes"),
            e!("⊥", "Perpendicular", r"\perp"),
            e!("∥", "Paralelo", r"\parallel"),
            e!("∠", "Ángulo", r"\angle"),
            e!("°", "Grado", r"^{\circ}"),
            e!("ℏ", "Constante de Planck reducida", r"\hbar"),
            e!("ℓ", "Ele cursiva", r"\ell"),
            e!("sin", "Seno", r"\sin"),
            e!("cos", "Coseno", r"\cos"),
            e!("tan", "Tangente", r"\tan"),
            e!("ln", "Logaritmo natural", r"\ln"),
            e!("log", "Logaritmo", r"\log_{◌}"),
            e!("max", "Máximo", r"\max"),
        ],
    },
    Categoria {
        id: "flechas",
        nombre: "Flechas",
        elementos: &[
            e!("→", "Flecha a la derecha", r"\to"),
            e!("←", "Flecha a la izquierda", r"\leftarrow"),
            e!("↔", "Flecha doble", r"\leftrightarrow"),
            e!("⇒", "Implica", r"\Rightarrow"),
            e!("⇐", "Implicado por", r"\Leftarrow"),
            e!("⇔", "Si y solo si", r"\Leftrightarrow"),
            e!("↦", "Se asigna a", r"\mapsto"),
            e!("↑", "Flecha arriba", r"\uparrow"),
            e!("↓", "Flecha abajo", r"\downarrow"),
            e!("⟶", "Flecha larga", r"\longrightarrow"),
            e!("⟹", "Implica (larga)", r"\Longrightarrow"),
            e!("⇀", "Arpón", r"\rightharpoonup"),
            e!("⇌", "Arpones en equilibrio", r"\rightleftharpoons"),
            e!("↗", "Flecha en diagonal", r"\nearrow"),
            e!("→ₓ", "Flecha con texto", r"\xrightarrow{◌}"),
            e!("⇒ₓ", "Implica con texto", r"\xRightarrow{◌}"),
        ],
    },
    Categoria {
        id: "quimica",
        nombre: "Química",
        elementos: &[
            e!("H₂O", "Fórmula química", r"\ce{◌}"),
            e!("A→B", "Reacción", r"\ce{◌ -> ◌}"),
            e!("A⇌B", "Equilibrio", r"\ce{◌ <=> ◌}"),
            e!("A←B", "Reacción inversa", r"\ce{◌ <- ◌}"),
            e!("A→ᶜB", "Reacción con condición", r"\ce{◌ ->[◌] ◌}"),
            e!(
                "2H₂+O₂",
                "Ejemplo: síntesis de agua",
                r"\ce{2H2 + O2 -> 2H2O}"
            ),
            e!("SO₄²⁻", "Ion con carga", r"\ce{SO4^2-}"),
            e!("Na⁺", "Catión", r"\ce{Na+}"),
            e!("(aq)", "Estado acuoso", r"\ce{◌(aq)}"),
            e!("(s)", "Estado sólido", r"\ce{◌(s)}"),
            e!("(g)", "Estado gaseoso", r"\ce{◌(g)}"),
            e!("↑", "Desprende gas", r"\ce{◌ ^}"),
            e!("↓", "Precipita", r"\ce{◌ v}"),
            e!("¹⁴C", "Isótopo", r"\ce{^{14}_{6}C}"),
            e!("·5H₂O", "Hidrato", r"\ce{CuSO4.5H2O}"),
            e!("ΔH", "Entalpía con unidades", r"\Delta H = \pu{◌ kJ/mol}"),
            e!("[ ]", "Concentración", r"[\ce{◌}]"),
            e!("Kₑq", "Constante de equilibrio", r"K_{\mathrm{eq}}"),
            e!("pH", "pH", r"\mathrm{pH} = -\log[\ce{H+}]"),
        ],
    },
    Categoria {
        id: "unidades",
        nombre: "Unidades",
        elementos: &[
            e!("9,8 m/s²", "Cantidad con unidad", r"\qty{◌}{◌}"),
            e!("m/s", "Unidad sola", r"\unit{◌}"),
            e!("1,2×10³", "Número", r"\num{◌}"),
            e!("30°", "Ángulo en grados", r"\ang{◌}"),
            e!("m/s", "Metros por segundo", r"\unit{m/s}"),
            e!("km/h", "Kilómetros por hora", r"\unit{km/h}"),
            e!("m/s²", "Aceleración", r"\unit{m/s^2}"),
            e!("N", "Newton", r"\unit{N}"),
            e!("kg·m/s²", "Newton en unidades base", r"\unit{kg.m/s^2}"),
            e!("J", "Joule", r"\unit{J}"),
            e!("W", "Watt", r"\unit{W}"),
            e!("kWh", "Kilowatt hora", r"\unit{kW.h}"),
            e!("Pa", "Pascal", r"\unit{Pa}"),
            e!("°C", "Grado Celsius", r"\unit{°C}"),
            e!("K", "Kelvin", r"\unit{K}"),
            e!("mol/L", "Molaridad", r"\unit{mol/L}"),
            e!("g/mol", "Masa molar", r"\unit{g/mol}"),
            e!("V", "Volt", r"\unit{V}"),
            e!("A", "Ampere", r"\unit{A}"),
            e!("Ω", "Ohm", r"\unit{\ohm}"),
            e!("Hz", "Hertz", r"\unit{Hz}"),
            e!("µm", "Micrómetro", r"\unit{\micro\metre}"),
            e!("k", "Prefijo kilo", r"\kilo"),
            e!("M", "Prefijo mega", r"\mega"),
            e!("m", "Prefijo mili", r"\milli"),
            e!("µ", "Prefijo micro", r"\micro"),
            e!("n", "Prefijo nano", r"\nano"),
            e!("/", "Por (potencia negativa)", r"\per"),
            e!("²", "Al cuadrado", r"\squared"),
            e!("³", "Al cubo", r"\cubed"),
        ],
    },
];

/// Aplica una plantilla: el primer hueco recibe `seleccion`; los demás quedan vacíos. Devuelve el
/// texto y dónde va el cursor (en caracteres desde el comienzo del texto devuelto): en el primer
/// hueco vacío o, si no hay, al final.
pub fn aplicar(plantilla: &str, seleccion: &str) -> (String, usize) {
    let mut out = String::with_capacity(plantilla.len() + seleccion.len());
    let mut cursor = None;
    let mut primero = true;
    let mut chars = 0usize;
    for c in plantilla.chars() {
        if c == HUECO {
            if primero && !seleccion.is_empty() {
                out.push_str(seleccion);
                chars += seleccion.chars().count();
            } else if cursor.is_none() {
                cursor = Some(chars);
            }
            primero = false;
        } else {
            out.push(c);
            chars += 1;
        }
    }
    (out, cursor.unwrap_or(chars))
}

/// La plantilla con cada hueco cambiado por `x` (para comprobar que todas se dibujan).
pub fn ejemplo(plantilla: &str) -> String {
    plantilla.replace(HUECO, "x")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aplica_huecos() {
        assert_eq!(aplicar(r"\frac{◌}{◌}", ""), (r"\frac{}{}".into(), 6));
        assert_eq!(aplicar(r"\frac{◌}{◌}", "a+b"), (r"\frac{a+b}{}".into(), 11));
        assert_eq!(aplicar(r"\alpha", "x"), (r"\alpha".into(), 6));
        assert_eq!(aplicar(r"\sqrt{◌}", "ñ"), (r"\sqrt{ñ}".into(), 8));
    }

    #[test]
    fn todas_las_plantillas_se_dibujan() {
        let estilo = crate::Estilo {
            tamano: 16.0,
            escala: 1.0,
            color: [255, 255, 255, 255],
        };
        let mut nombres = std::collections::HashSet::new();
        for cat in CATEGORIAS {
            for el in cat.elementos {
                assert!(
                    nombres.insert((cat.id, el.nombre)),
                    "nombre repetido: {}",
                    el.nombre
                );
                let mut tex = ejemplo(el.plantilla);
                // Los prefijos y modificadores sueltos de siunitx van dentro de `\unit`.
                if tex.starts_with(r"\kilo")
                    || tex.starts_with(r"\mega")
                    || tex.starts_with(r"\milli")
                    || tex.starts_with(r"\micro")
                    || tex.starts_with(r"\nano")
                {
                    tex = format!(r"\unit{{{tex}\metre}}");
                } else if tex == r"\per" {
                    tex = r"\unit{\metre\per\second}".into();
                } else if tex == r"\squared" || tex == r"\cubed" {
                    tex = format!(r"\unit{{\metre{tex}}}");
                } else if tex.starts_with(r"\qty") {
                    tex = r"\qty{9.8}{m/s^2}".into();
                } else if tex.starts_with(r"\unit{x}") {
                    tex = r"\unit{m}".into();
                }
                crate::dibujar(&tex, crate::Modo::Bloque, &estilo)
                    .unwrap_or_else(|e| panic!("{} ({tex}): {e}", el.nombre));
            }
        }
    }
}
