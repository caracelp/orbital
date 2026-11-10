//! Variables de las plantillas (`type: plantilla`, FR-024).

/// Valores de las variables al crear una nota desde una plantilla.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Vars {
    /// `{{fecha}}`: `AAAA-MM-DD`.
    pub fecha: String,
    /// `{{hora}}`: `HH:MM`.
    pub hora: String,
    /// `{{titulo}}`: nombre de la nota nueva.
    pub titulo: String,
    /// `{{carpeta}}`: carpeta de destino, relativa a la bóveda.
    pub carpeta: String,
}

impl Vars {
    fn get(&self, name: &str) -> Option<&str> {
        match name {
            "fecha" => Some(&self.fecha),
            "hora" => Some(&self.hora),
            "titulo" | "título" => Some(&self.titulo),
            "carpeta" => Some(&self.carpeta),
            _ => None,
        }
    }
}

/// Reemplaza `{{variable}}` (con o sin espacios adentro). Las variables desconocidas quedan tal
/// cual y se devuelven, sin repetir, para avisar.
pub fn render(text: &str, vars: &Vars) -> (String, Vec<String>) {
    let mut out = String::with_capacity(text.len());
    let mut unknown: Vec<String> = Vec::new();
    let mut rest = text;
    while let Some(open) = rest.find("{{") {
        let Some(close) = rest[open + 2..].find("}}").map(|c| open + 2 + c) else {
            break;
        };
        let name = rest[open + 2..close].trim();
        out.push_str(&rest[..open]);
        match vars.get(name) {
            Some(value) => out.push_str(value),
            None => {
                out.push_str(&rest[open..close + 2]);
                if !name.is_empty() && !unknown.iter().any(|u| u == name) {
                    unknown.push(name.to_owned());
                }
            }
        }
        rest = &rest[close + 2..];
    }
    out.push_str(rest);
    (out, unknown)
}
