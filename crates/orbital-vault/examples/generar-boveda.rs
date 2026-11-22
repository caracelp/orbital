//! Genera una bóveda de prueba con la estructura de referencia.
//!
//! Uso: `cargo run -p orbital-vault --example generar-boveda -- <destino> <n>`
//!
//! Crea `raw/` con 10 fuentes y `wiki/` con `n` notas (90 % `.md`, 10 % `.html`) en subcarpetas
//! de hasta 3 niveles, con frontmatter OKF, más `wiki/index.md` y `wiki/log.md`. Es determinista.
//! Nunca escribe dentro de una bóveda real: rechaza todo destino dentro de `~/Documentos` y de las
//! carpetas que nombre `ORBITAL_BOVEDAS_PROTEGIDAS` (separadas por `:`).

use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

const TOPICS: [&str; 8] = [
    "fisica",
    "historia",
    "musica",
    "filosofia",
    "rust",
    "biologia",
    "lenguas",
    "matematica",
];

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (Some(dest), Some(n)) = (
        args.first(),
        args.get(1).and_then(|s| s.parse::<usize>().ok()),
    ) else {
        eprintln!("Uso: generar-boveda <destino> <cantidad de notas>");
        return ExitCode::FAILURE;
    };
    let dest = PathBuf::from(dest);
    if let Some(protegida) = protected_root(&dest) {
        eprintln!(
            "Me niego a escribir dentro de una carpeta protegida ({}).",
            protegida.display()
        );
        return ExitCode::FAILURE;
    }
    if dest.exists()
        && fs::read_dir(&dest)
            .map(|mut d| d.next().is_some())
            .unwrap_or(true)
    {
        eprintln!("«{}» ya existe y no está vacía.", dest.display());
        return ExitCode::FAILURE;
    }
    match generate(&dest, n) {
        Ok(()) => {
            println!("Bóveda de {n} notas creada en {}", dest.display());
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("Error: {e}");
            ExitCode::FAILURE
        }
    }
}

/// Carpetas donde nunca se escribe: `~/Documentos` (donde suelen vivir las bóvedas reales) y las
/// que nombre `ORBITAL_BOVEDAS_PROTEGIDAS`, separadas por `:`.
fn protected_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Some(home) = std::env::var_os("HOME").filter(|h| !h.is_empty()) {
        roots.push(PathBuf::from(home).join("Documentos"));
    }
    if let Some(extra) = std::env::var_os("ORBITAL_BOVEDAS_PROTEGIDAS") {
        roots.extend(std::env::split_paths(&extra).filter(|p| !p.as_os_str().is_empty()));
    }
    roots
}

/// La carpeta protegida que contiene a `dest`, si hay alguna.
fn protected_root(dest: &Path) -> Option<PathBuf> {
    let abs = if dest.is_absolute() {
        dest.to_path_buf()
    } else {
        std::env::current_dir().unwrap_or_default().join(dest)
    };
    // Normaliza `..` sin exigir que la ruta exista.
    let mut norm = PathBuf::new();
    for c in abs.components() {
        match c {
            std::path::Component::ParentDir => {
                norm.pop();
            }
            std::path::Component::CurDir => {}
            other => norm.push(other),
        }
    }
    let canon = fs::canonicalize(&norm).ok();
    protected_roots().into_iter().find(|root| {
        let root_canon = fs::canonicalize(root).ok();
        let inside = |p: &Path| {
            p.starts_with(root) || root_canon.as_deref().is_some_and(|r| p.starts_with(r))
        };
        inside(&norm) || canon.as_deref().is_some_and(inside)
    })
}

/// Generador congruencial simple: suficiente para datos de prueba deterministas.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0 >> 33
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

fn generate(dest: &Path, n: usize) -> std::io::Result<()> {
    let raw = dest.join("raw");
    let wiki = dest.join("wiki");
    fs::create_dir_all(&raw)?;
    fs::create_dir_all(&wiki)?;
    for i in 0..10 {
        fs::write(
            raw.join(format!("fuente-{i:02}.txt")),
            format!("Fuente original {i}.\n"),
        )?;
    }

    let mut rng = Rng(0x0b17a1);
    let mut index_md: Vec<(String, String)> = Vec::new();
    let mut index_html: Vec<(String, String)> = Vec::new();
    for i in 0..n {
        let topic = TOPICS[rng.below(TOPICS.len() as u64) as usize];
        let depth = rng.below(4);
        let mut rel = String::new();
        let mut dir = wiki.clone();
        for level in 0..depth {
            let folder = format!("{topic}-{level}{}", rng.below(3));
            dir.push(&folder);
            rel.push_str(&folder);
            rel.push('/');
        }
        fs::create_dir_all(&dir)?;
        let is_html = i % 10 == 9;
        let title = format!("Nota {i} sobre {topic}");
        let description = format!("Resumen breve de la nota {i}.");
        let mut body = String::with_capacity(1500);
        for p in 0..(3 + rng.below(6)) {
            let _ = writeln!(
                body,
                "Párrafo {p} de la nota {i}. Enlaza con [[Nota {}]] y trata sobre #{topic}.\n",
                rng.below(n.max(1) as u64)
            );
        }
        let props = format!(
            "type: {}\ntitle: {title}\ndescription: {description}\ntags: [{topic}, prueba]\ncreated: 2026-11-09T12:00:00-03:00\ngenerated: {{ by: human:prueba, at: 2026-11-09T12:00:00-03:00 }}\n",
            if is_html { "sintesis" } else { "atomo" }
        );
        let file = if is_html {
            format!("nota-{i:05}.html")
        } else {
            format!("nota-{i:05}.md")
        };
        let content = if is_html {
            format!(
                "<!doctype html>\n<html lang=\"es\">\n<head>\n  <meta charset=\"utf-8\">\n  <title>{title}</title>\n  <script type=\"application/yaml\">\n{props}  </script>\n</head>\n<body>\n<h1>{title}</h1>\n<p>{}</p>\n</body>\n</html>\n",
                body.replace('\n', " ")
            )
        } else {
            format!("---\n{props}---\n# {title}\n\n{body}")
        };
        fs::write(dir.join(&file), content)?;
        let link = format!("/{rel}{file}");
        let line = format!("* [{title}]({link}) - {description}");
        if is_html {
            index_html.push((link, line));
        } else {
            index_md.push((link, line));
        }
    }

    let mut index = String::from("# atomo\n\n");
    for (_, l) in &index_md {
        index.push_str(l);
        index.push('\n');
    }
    index.push_str("\n# sintesis\n\n");
    for (_, l) in &index_html {
        index.push_str(l);
        index.push('\n');
    }
    fs::write(wiki.join("index.md"), index)?;
    fs::write(
        wiki.join("log.md"),
        "# Registro de la wiki\n\n## 2026-11-09\n* **Creación**: Estructura de la bóveda\n",
    )?;
    Ok(())
}
