//! Importar archivos a Markdown (015): PDF, DOCX y HTML → nota nueva en `wiki/`.
//!
//! La conversión es offline y pura (sin GTK, sin binarios externos): HTML con `htmd`, PDF con
//! `pdf-extract` (solo texto) y DOCX con `zip` + `roxmltree` (es un ZIP con
//! `word/document.xml`). La nota sale por el camino de escritura normal: `check_new`,
//! `safe_write` (GuardadoSeguro) y `after_create` (índice y registro al día).

use std::fs;
use std::io::Cursor;
use std::io::Read;

use crate::props::PropValue;
use crate::props::emit::{self, LineEnding};
use crate::write::{WriteCtx, safe_write};
use crate::{RelPath, Vault, VaultError};

/// Formatos soportados por la importación.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FormatoImport {
    Html,
    Pdf,
    Docx,
}

/// Extensión (sin punto, minúscula) → formato, si está soportada.
pub fn formato_de_ext(ext: &str) -> Option<FormatoImport> {
    match ext.to_ascii_lowercase().as_str() {
        "html" | "htm" | "xhtml" => Some(FormatoImport::Html),
        "pdf" => Some(FormatoImport::Pdf),
        "docx" => Some(FormatoImport::Docx),
        _ => None,
    }
}

/// Convierte el contenido del archivo a un cuerpo Markdown (sin frontmatter).
pub fn importar(bytes: &[u8], formato: FormatoImport) -> Result<String, VaultError> {
    let cuerpo = match formato {
        FormatoImport::Html => html_a_md(bytes)?,
        FormatoImport::Pdf => normalizar_pdf(&pdf_a_md(bytes)?),
        FormatoImport::Docx => docx_a_md(bytes)?,
    };
    Ok(cuerpo.trim().to_owned() + "\n")
}

/// Crea la nota Markdown de `wiki/` para el archivo de `raw/` dado y devuelve la ruta. Si el
/// nombre ya existe, prueba `<nombre>-2`, `-3`, …: nunca pisa una nota existente.
pub fn crear_nota_importada(
    vault: &Vault,
    ruta_raw: &RelPath,
    ctx: &WriteCtx,
) -> Result<RelPath, VaultError> {
    let extension = ruta_raw
        .file_name()
        .rsplit_once('.')
        .map(|(_, e)| e.to_owned())
        .unwrap_or_default();
    let Some(formato) = formato_de_ext(&extension) else {
        return Err(VaultError::Importar(format!(
            "«{}» no es PDF, DOCX ni HTML",
            ruta_raw.file_name()
        )));
    };
    let bytes = fs::read(ruta_raw.to_path(vault.root()))?;
    let cuerpo = importar(&bytes, formato)?;
    let nombre_base = ruta_raw
        .file_name()
        .rsplit_once('.')
        .map(|(n, _)| n.to_owned())
        .unwrap_or_else(|| ruta_raw.file_name().to_owned());
    // La nota vive en wiki/ si la bóveda la tiene (igual que el diario); el fuente queda en raw/.
    let prefijo = if vault.root().join("wiki").is_dir() {
        "wiki"
    } else {
        ""
    };
    let mut ruta_nota = None;
    for intento in 1..=999u32 {
        let nombre = if intento == 1 {
            format!("{nombre_base}.md")
        } else {
            format!("{nombre_base}-{intento}.md")
        };
        let ruta_relativa = format!("{prefijo}/{nombre}")
            .trim_start_matches('/')
            .to_owned();
        let path = RelPath::new(&ruta_relativa)?;
        if vault.root().join(path.as_str()).exists() {
            continue;
        }
        ruta_nota = Some(path);
        break;
    }
    let Some(path) = ruta_nota else {
        return Err(VaultError::Importar(format!(
            "no hay nombre libre para «{nombre_base}»"
        )));
    };
    let stamp = PropValue::DateTime(ctx.timestamp());
    let entries = [
        ("type", PropValue::Text("atomo".to_owned())),
        ("title", PropValue::Text(nombre_base.clone())),
        (
            "description",
            PropValue::Text(format!("Importado de /{ruta_raw}.")),
        ),
        ("created", stamp.clone()),
        (
            "generated",
            PropValue::Map(vec![
                ("by".to_owned(), PropValue::Text(ctx.actor.0.clone())),
                ("at".to_owned(), stamp),
            ]),
        ),
        (
            "sources",
            PropValue::List(vec![PropValue::Map(vec![
                ("id".to_owned(), PropValue::Text(nombre_base.clone())),
                (
                    "resource".to_owned(),
                    PropValue::Text(format!("/{ruta_raw}")),
                ),
            ])]),
        ),
    ];
    let block = emit::document(&entries, LineEnding::Lf);
    let content = format!("---\n{block}---\n# {nombre_base}\n\n{cuerpo}");
    safe_write(vault.root(), &path, content.as_bytes(), None, ctx)?;
    vault.after_create(&path, ctx)?;
    Ok(path)
}

fn html_a_md(bytes: &[u8]) -> Result<String, VaultError> {
    let text = String::from_utf8_lossy(bytes);
    htmd::convert(&text).map_err(|e| VaultError::Importar(format!("HTML: {e}")))
}

fn pdf_a_md(bytes: &[u8]) -> Result<String, VaultError> {
    pdf_extract::extract_text_from_mem(bytes).map_err(|e| VaultError::Importar(format!("PDF: {e}")))
}

/// Atributo por nombre local (los prefijos de namespace de los DOCX varían).
fn atributo<'a>(nodo: &roxmltree::Node<'a, 'a>, nombre: &str) -> Option<&'a str> {
    nodo.attributes()
        .find(|a| a.name() == nombre)
        .map(|a| a.value())
}

fn docx_a_md(bytes: &[u8]) -> Result<String, VaultError> {
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes))
        .map_err(|e| VaultError::Importar(format!("DOCX: {e}")))?;
    let rels = hiperenlaces(&mut zip)?;
    let document = xml_del_zip(&mut zip, "word/document.xml")?;
    let doc = roxmltree::Document::parse(&document)
        .map_err(|e| VaultError::Importar(format!("DOCX: {e}")))?;
    let cuerpo = doc
        .root_element()
        .descendants()
        .find(|n| n.tag_name().name() == "body")
        .ok_or_else(|| VaultError::Importar("DOCX sin cuerpo".to_owned()))?;
    let mut out = String::new();
    for nodo in cuerpo.children() {
        match nodo.tag_name().name() {
            "p" => {
                let texto = parrafo_docx(&nodo, &rels);
                if !texto.trim().is_empty() {
                    out.push_str(&texto);
                }
            }
            "tbl" => {
                for fila in nodo.children().filter(|f| f.tag_name().name() == "tr") {
                    let celdas: Vec<String> = fila
                        .children()
                        .filter(|c| c.tag_name().name() == "tc")
                        .map(|c| {
                            c.descendants()
                                .filter(|n| n.tag_name().name() == "p")
                                .map(|p| parrafo_docx(&p, &rels).trim().to_owned())
                                .collect::<Vec<_>>()
                                .join(" ")
                        })
                        .collect();
                    out.push_str(&celdas.join("\t"));
                    out.push('\n');
                }
            }
            _ => {}
        }
    }
    Ok(out)
}

/// Texto plano de un elemento: el contenido de todos sus `w:t`.
fn texto_plano(nodo: &roxmltree::Node) -> String {
    let mut out = String::new();
    for d in nodo.descendants() {
        if d.tag_name().name() == "t" {
            out.push_str(d.text().unwrap_or_default());
        }
    }
    out
}

/// Párrafo DOCX → Markdown: título por estilo, lista por numeración, énfasis por propiedades
/// de corrida y enlaces por `w:hyperlink` resueltos contra las relaciones del documento.
fn parrafo_docx(
    nodo: &roxmltree::Node,
    rels: &std::collections::HashMap<String, String>,
) -> String {
    let mut nivel_titulo = 0;
    let mut nivel_lista = 0;
    let mut hay_lista = false;
    if let Some(ppr) = nodo.children().find(|n| n.tag_name().name() == "pPr") {
        if let Some(estilo) = ppr
            .children()
            .find(|n| n.tag_name().name() == "pStyle")
            .and_then(|s| atributo(&s, "val"))
        {
            nivel_titulo = nivel_de_estilo(estilo).unwrap_or(0);
        }
        if let Some(numpr) = ppr.children().find(|n| n.tag_name().name() == "numPr") {
            hay_lista = true;
            nivel_lista = numpr
                .children()
                .find(|n| n.tag_name().name() == "ilvl")
                .and_then(|il| atributo(&il, "val"))
                .and_then(|v| v.parse::<usize>().ok())
                .unwrap_or(0);
        }
    }
    let mut texto = String::new();
    for hijo in nodo.children() {
        match hijo.tag_name().name() {
            "hyperlink" => {
                let destino = hijo
                    .attribute("id")
                    .and_then(|id| rels.get(id).cloned())
                    .unwrap_or_default();
                let cuerpo = texto_plano(&hijo);
                if destino.is_empty() || cuerpo.trim().is_empty() {
                    texto.push_str(&cuerpo);
                } else {
                    texto.push_str(&format!("[{cuerpo}]({destino})"));
                }
            }
            "r" => {
                let mut negrita = false;
                let mut cursiva = false;
                if let Some(rpr) = hijo.children().find(|n| n.tag_name().name() == "rPr") {
                    negrita = rpr.children().any(|n| n.tag_name().name() == "b");
                    cursiva = rpr.children().any(|n| n.tag_name().name() == "i");
                }
                let mut corrida = String::new();
                for pieza in hijo.children() {
                    match pieza.tag_name().name() {
                        "t" => corrida.push_str(pieza.text().unwrap_or_default()),
                        "tab" => corrida.push('\t'),
                        "br" => corrida.push(' '),
                        _ => {}
                    }
                }
                if corrida.is_empty() {
                    continue;
                }
                if negrita {
                    texto.push_str("**");
                }
                if cursiva {
                    texto.push('*');
                }
                texto.push_str(&corrida);
                if cursiva {
                    texto.push('*');
                }
                if negrita {
                    texto.push_str("**");
                }
            }
            _ => {}
        }
    }
    if nivel_titulo > 0 {
        format!("{} {}\n", "#".repeat(nivel_titulo), texto.trim())
    } else if hay_lista {
        let sangria = "  ".repeat(nivel_lista);
        format!("{sangria}- {}\n", texto.trim())
    } else {
        texto.trim().to_owned() + "\n"
    }
}

/// Estilo de párrafo DOCX → nivel de encabezado (1-6). Cubre los `HeadingN` y los `TtuloN`
/// (identificadores localizados de Word en español).
fn nivel_de_estilo(val: &str) -> Option<usize> {
    let resto = val
        .strip_prefix("Heading")
        .or_else(|| val.strip_prefix("Ttulo"))?;
    let n = resto.trim().parse::<usize>().ok()?;
    (1..=6).contains(&n).then_some(n)
}

/// Id → destino de las relaciones con hipervínculo externo (solo http/https y mailto).
fn hiperenlaces(
    zip: &mut zip::ZipArchive<Cursor<&[u8]>>,
) -> Result<std::collections::HashMap<String, String>, VaultError> {
    let mut out = std::collections::HashMap::new();
    let Ok(xml) = xml_del_zip(zip, "word/_rels/document.xml.rels") else {
        return Ok(out);
    };
    let Ok(doc) = roxmltree::Document::parse(&xml) else {
        return Ok(out);
    };
    for rel in doc.descendants() {
        if rel.tag_name().name() != "Relationship" {
            continue;
        }
        let destino = rel.attribute("Target").unwrap_or_default();
        let externo = destino.starts_with("http://")
            || destino.starts_with("https://")
            || destino.starts_with("mailto:");
        if externo && let Some(id) = rel.attribute("Id") {
            out.insert(id.to_owned(), destino.to_owned());
        }
    }
    Ok(out)
}

fn xml_del_zip(
    zip: &mut zip::ZipArchive<Cursor<&[u8]>>,
    nombre: &str,
) -> Result<String, VaultError> {
    let mut archivo = zip
        .by_name(nombre)
        .map_err(|e| VaultError::Importar(format!("DOCX: {nombre}: {e}")))?;
    let mut bytes = Vec::with_capacity(archivo.size() as usize);
    archivo
        .read_to_end(&mut bytes)
        .map_err(|e| VaultError::Importar(format!("DOCX: {nombre}: {e}")))?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

/// El texto de un PDF es corrido: un salto simple une el párrafo (los PDF cortan por ancho de
/// página) y dos o más lo separan. Las líneas que abren estructura Markdown se agrupan como
/// bloque propio (las viñetas consecutivas quedan juntas, con aire entre bloques).
fn normalizar_pdf(texto: &str) -> String {
    let estructural = |l: &str| {
        l.starts_with("# ")
            || l.starts_with("## ")
            || l.starts_with("- ")
            || l.starts_with("* ")
            || l.starts_with("> ")
    };
    let mut bloques: Vec<String> = Vec::new();
    let mut parrafo = String::new();
    let mut lista = String::new();
    for linea in texto.lines() {
        let l = linea.trim_end();
        if l.trim().is_empty() {
            if !parrafo.trim().is_empty() {
                bloques.push(parrafo.trim().to_owned());
                parrafo.clear();
            }
            if !lista.is_empty() {
                bloques.push(std::mem::take(&mut lista));
            }
            continue;
        }
        if estructural(l) {
            if !parrafo.trim().is_empty() {
                bloques.push(parrafo.trim().to_owned());
                parrafo.clear();
            }
            if !lista.is_empty() {
                lista.push('\n');
            }
            lista.push_str(l);
        } else {
            if !lista.is_empty() {
                bloques.push(std::mem::take(&mut lista));
            }
            if parrafo.is_empty() {
                parrafo.push_str(l.trim_start());
            } else {
                parrafo.push(' ');
                parrafo.push_str(l.trim_start());
            }
        }
    }
    if !parrafo.trim().is_empty() {
        bloques.push(parrafo.trim().to_owned());
    }
    if !lista.is_empty() {
        bloques.push(lista);
    }
    bloques.join("\n\n") + "\n"
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::write::{Actor, Date, Offset, Time};

    /// Contexto de escritura fijo (igual que tests/common/mod.rs, que no se ve desde acá).
    fn ctx() -> WriteCtx {
        WriteCtx {
            date: Date {
                year: 2026,
                month: 11,
                day: 9,
            },
            time: Time {
                hour: 18,
                minute: 30,
                second: 5,
            },
            utc_offset: Offset { minutes: -180 },
            actor: Actor::human("prueba"),
        }
    }

    /// US2.1: HTML con encabezados, párrafos, listas, énfasis y enlaces pasa a Markdown.
    #[test]
    fn t002_html_a_markdown() {
        let html = "<html><body><h1>Título</h1><p>Párrafo con \
            <strong>negrita</strong> y <a href=\"https://ejemplo.com\">enlace</a>.</p>\
            <ul><li>uno</li><li>dos</li></ul></body></html>"
            .as_bytes();
        let md = importar(html, FormatoImport::Html).unwrap();
        assert!(md.contains("# Título"), "{md}");
        assert!(md.contains("**negrita**"), "{md}");
        assert!(md.contains("[enlace](https://ejemplo.com)"), "{md}");
        let items = md
            .lines()
            .filter(|l| l.trim_start().starts_with('*') && l.contains("uno"))
            .count();
        assert!(items == 1, "lista sin «uno»: {md}");
        assert!(md.contains("dos"), "{md}");
    }

    /// El DOCX mínimo para los tests: un ZIP con los archivos que se le pasan.
    fn docx_minimo(archivos: &[(&str, &str)]) -> Vec<u8> {
        let mut buffer = Cursor::new(Vec::new());
        {
            let mut zip = zip::ZipWriter::new(&mut buffer);
            for (nombre, contenido) in archivos {
                zip.start_file(*nombre, zip::write::SimpleFileOptions::default())
                    .unwrap();
                std::io::Write::write_all(&mut zip, contenido.as_bytes()).unwrap();
            }
            zip.finish().unwrap();
        }
        buffer.into_inner()
    }

    /// US2.2: DOCX con título, énfasis y lista (con nivel) pasa a Markdown.
    #[test]
    fn t002_docx_a_markdown() {
        let document = "<w:document xmlns:w=\"x\"><w:body>\
            <w:p><w:pPr><w:pStyle w:val=\"Heading1\"/></w:pPr><w:r><w:t>Portada</w:t></w:r></w:p>\
            <w:p><w:r><w:rPr><w:b/></w:rPr><w:t>Importante</w:t></w:r><w:r><w:t> y </w:t></w:r>\
            <w:r><w:rPr><w:i/></w:rPr><w:t>cursiva</w:t></w:r></w:p>\
            <w:p><w:pPr><w:numPr><w:ilvl w:val=\"1\"/></w:numPr></w:pPr><w:r><w:t>primer ítem</w:t></w:r></w:p>\
            </w:body></w:document>";
        let docx = docx_minimo(&[("word/document.xml", document)]);
        let md = importar(&docx, FormatoImport::Docx).unwrap();
        assert!(md.contains("# Portada"), "{md}");
        assert!(md.contains("**Importante** y *cursiva*"), "{md}");
        assert!(md.contains("  - primer ítem"), "{md}");
    }

    /// US2.2: los hipervínculos del DOCX se resuelven contra las relaciones del documento.
    #[test]
    fn t002_docx_hipervinculos() {
        let document = "<w:document xmlns:w=\"x\" xmlns:r=\"y\"><w:body>\
            <w:p><w:hyperlink r:id=\"rId5\"><w:r><w:t>Enlace</w:t></w:r></w:hyperlink></w:p>\
            </w:body></w:document>";
        let rels = "<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">\
            <Relationship Id=\"rId5\" Target=\"https://ejemplo.com/doc\" TargetMode=\"External\"/>\
            </Relationships>";
        let docx = docx_minimo(&[
            ("word/document.xml", document),
            ("word/_rels/document.xml.rels", rels),
        ]);
        let md = importar(&docx, FormatoImport::Docx).unwrap();
        assert!(md.contains("[Enlace](https://ejemplo.com/doc)"), "{md}");
    }

    /// US2.2: los títulos localizados de Word en español (`Ttulo1`) también son encabezados.
    #[test]
    fn t002_docx_estilo_localizado() {
        let document = "<w:document xmlns:w=\"x\"><w:body>\
            <w:p><w:pPr><w:pStyle w:val=\"Ttulo2\"/></w:pPr><w:r><w:t>Sección</w:t></w:r></w:p>\
            </w:body></w:document>";
        let docx = docx_minimo(&[("word/document.xml", document)]);
        let md = importar(&docx, FormatoImport::Docx).unwrap();
        assert!(md.contains("## Sección"), "{md}");
    }

    /// US2.3: el texto del PDF se normaliza en párrafos (un salto une, dos separan).
    #[test]
    fn t003_pdf_normalizado() {
        let texto = "Página uno:\n\nlínea uno del párrafo\nlínea dos del\nmismo párrafo.\n\n\
            Segundo párrafo.\n\n- viñeta\n\nOtro párrafo.";
        let normalizado = normalizar_pdf(texto);
        assert!(
            normalizado.contains("línea uno del párrafo línea dos del mismo párrafo."),
            "{normalizado}"
        );
        assert!(
            normalizado.contains("\n\nSegundo párrafo."),
            "{normalizado}"
        );
        assert!(normalizado.contains("\n- viñeta\n\n"), "{normalizado}");
    }

    /// US2.4: un archivo corrupto no genera pánico ni nota.
    #[test]
    fn t004_corrupto_da_error() {
        assert!(importar(b"esto no es un pdf", FormatoImport::Pdf).is_err());
        assert!(importar(b"PK basura", FormatoImport::Docx).is_err());
        assert!(importar(&[0xff, 0xfe], FormatoImport::Pdf).is_err());
    }

    /// US1.4: extensiones soportadas y no soportadas.
    #[test]
    fn t001_extensiones() {
        assert_eq!(formato_de_ext("pdf"), Some(FormatoImport::Pdf));
        assert_eq!(formato_de_ext("DOCX"), Some(FormatoImport::Docx));
        assert_eq!(formato_de_ext("xhtml"), Some(FormatoImport::Html));
        assert_eq!(formato_de_ext("png"), None);
        assert_eq!(formato_de_ext("canvas"), None);
    }

    /// US1.1-1.3: crear la nota desde un archivo de raw/, con `sources`, colisión `-2` y el
    /// fuente intacto; índice y registro al día.
    #[test]
    fn t003_crear_nota_importada() {
        let raiz = std::env::temp_dir().join(format!("orbital-importar-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&raiz);
        std::fs::create_dir_all(raiz.join("raw")).unwrap();
        std::fs::create_dir_all(raiz.join("wiki")).unwrap();
        // index.md y log.md existen solo en bóvedas inicializadas por Orbital (contrato OKF).
        std::fs::write(raiz.join("wiki/index.md"), "# atomo\n\n# sintesis\n").unwrap();
        std::fs::write(raiz.join("wiki/log.md"), "# Registro de la wiki\n").unwrap();
        let html =
            "<html><body><h1>Artículo</h1><p>Texto del artículo.</p></body></html>".as_bytes();
        std::fs::write(raiz.join("raw/articulo.html"), html).unwrap();
        let vault = crate::open(&raiz).unwrap();
        let ruta = RelPath::new("raw/articulo.html").unwrap();
        let nota = crear_nota_importada(&vault, &ruta, &ctx()).unwrap();
        assert_eq!(nota.as_str(), "wiki/articulo.md");
        let texto = std::fs::read_to_string(raiz.join(nota.as_str())).unwrap();
        assert!(texto.starts_with("---\n"), "{texto}");
        assert!(texto.contains("resource: /raw/articulo.html"), "{texto}");
        assert!(texto.contains("# Artículo"), "{texto}");
        // Colisión: la segunda importación va a -2 y nunca pisa.
        let segunda = crear_nota_importada(&vault, &ruta, &ctx()).unwrap();
        assert_eq!(segunda.as_str(), "wiki/articulo-2.md");
        // El fuente de raw/ queda byte a byte (SC-2).
        assert_eq!(std::fs::read(raiz.join("raw/articulo.html")).unwrap(), html);
        // índice y registro al día (after_create).
        let indice = std::fs::read_to_string(raiz.join("wiki/index.md")).unwrap();
        assert!(indice.contains("articulo.md"), "{indice}");
        let _ = std::fs::remove_dir_all(&raiz);
    }

    /// US1.4: una extensión no soportada da error sin escribir nada.
    #[test]
    fn t004_extension_no_soportada() {
        let raiz =
            std::env::temp_dir().join(format!("orbital-importar-ext-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&raiz);
        std::fs::create_dir_all(raiz.join("raw")).unwrap();
        std::fs::write(raiz.join("raw/imagen.png"), b"png").unwrap();
        let vault = crate::open(&raiz).unwrap();
        let ruta = RelPath::new("raw/imagen.png").unwrap();
        let error = crear_nota_importada(&vault, &ruta, &ctx()).unwrap_err();
        assert!(error.to_string().contains("no es PDF"), "{error}");
        assert!(!raiz.join("wiki").exists());
        let _ = std::fs::remove_dir_all(&raiz);
    }
}

#[cfg(test)]
mod bench_sc1 {
    use super::*;

    /// SC-1: convertir un HTML de ~1 MiB tarda menos de un segundo (corre en el hilo de fondo).
    #[test]
    fn sc1_html_1mib_en_menos_de_un_segundo() {
        let mut html = String::from("<html><body>");
        for n in 0..7400 {
            html.push_str(&format!(
                "<h2>Sección {n}</h2><p>Párrafo con <strong>énfasis</strong> y un \
                 <a href=\"https://ejemplo.com/{n}\">enlace</a> más texto de relleno.</p>"
            ));
        }
        html.push_str("</body></html>");
        assert!(html.len() > 1_000_000, "{} bytes", html.len());
        let t0 = std::time::Instant::now();
        let md = importar(html.as_bytes(), FormatoImport::Html).unwrap();
        let duracion = t0.elapsed();
        assert!(md.contains("# Sección 1999"));
        println!(
            "SC-1 HTML de {} KiB → Markdown en {:.2} s",
            html.len() / 1024,
            duracion.as_secs_f64()
        );
        // El presupuesto (SC-1) mide en release; el debug corre sin optimizar y acá solo
        // se verifica que no se descontrola.
        let presupuesto = if cfg!(debug_assertions) { 4.0 } else { 1.0 };
        assert!(
            duracion.as_secs_f64() < presupuesto,
            "la conversión de 1 MiB tomó {duracion:?}"
        );
    }
}
