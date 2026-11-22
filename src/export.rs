//! Exportación de notas (007): HTML autónomo del render de Lectura.
//!
//! Tema claro (papel blanco) para imprimir y compartir; sin recursos externos. Sin red: todo es
//! render local.

use pulldown_cmark::{CodeBlockKind, CowStr, Event, Options, Parser, Tag, TagEnd, html::push_html};
use std::ops::Range;

/// CSS de la página exportada: papel blanco, tipografía limpia, ancho de lectura.
const ESTILOS: &str = "body{font-family:sans-serif;line-height:1.5;color:#1a1a1c;background:#ffffff;\
max-width:860px;margin:2em auto;padding:0 1.5em}\
h1,h2,h3,h4,h5,h6{line-height:1.25}code{background:#f2f2f4;border-radius:3px;padding:0.05em 0.3em;\
font-family:monospace}pre{background:#f2f2f4;border-radius:6px;padding:0.75em 1em;overflow-x:auto}\
pre code{background:none;padding:0}blockquote{border-left:4px solid #ff8cc6;margin:0.75em 0;\
padding:0.25em 1em;color:#55555c;background:#fff7fb}table{border-collapse:collapse}\
th,td{border:1px solid #d0d0d4;padding:0.35em 0.6em;text-align:left}\
img{max-width:100%}input[type=checkbox]{margin-right:0.4em}\
.formula-bloque{display:block;text-align:center;margin:0.6em 0;overflow-x:auto}\
.formula-error{text-decoration:underline wavy #c2185b}\
figure.grafico{margin:1em 0}figure.grafico svg{max-width:100%;height:auto}\
.grafico-aviso{color:#c2185b}\
figure.diagrama{margin:1em 0}figure.diagrama svg{max-width:100%;height:auto}\
figure.medio{margin:1em 0}figure.medio video{max-width:100%}figure.medio audio{width:100%}\
figure.medio figcaption,p.medio{color:#55555c;font-size:0.9em}\
p.medio{border:1px solid #d0d0d4;border-radius:6px;padding:0.5em 0.8em}\
nav.orbital-indice,nav.orbital-toc{margin:0.75em 0}";

/// Título de la página: el título de la nota (frontmatter) o el nombre del archivo.
fn titulo(texto: &str, defecto: &str) -> String {
    orbital_vault::props::markdown::locate(texto)
        .and_then(|r| orbital_vault::props::yaml::parse_block(&texto[r]).ok())
        .and_then(|entries| {
            orbital_vault::props::find(&entries, "titulo")
                .or_else(|| orbital_vault::props::find(&entries, "title"))
                .and_then(|e| e.value.as_text().map(str::to_owned))
        })
        .unwrap_or_else(|| defecto.to_owned())
}

/// HTML autónomo de una nota `.md`: cuerpo renderizado sin frontmatter (FR-3), con las mismas
/// extensiones que Lectura (tablas, tachado, casillas).
#[cfg(test)]
pub fn html_md(texto: &str, nombre_archivo: &str) -> String {
    html_md_con(texto, nombre_archivo, &Medios::default())
}

/// Resuelve lo escrito en `![[…]]` a la ruta del archivo.
pub type Resolver<'a> = dyn Fn(&str) -> Option<std::path::PathBuf> + 'a;

/// Cómo exportar los medios incrustados (057).
#[derive(Default)]
pub struct Medios<'a> {
    /// Carpeta de la nota, para `![](clip.mp4)`.
    pub carpeta: Option<std::path::PathBuf>,
    /// Resuelve `![[clip.mp3]]` a la ruta del archivo (la misma resolución que la Lectura).
    pub adjunto: Option<&'a Resolver<'a>>,
    /// HTML: `<audio>`/`<video>` con controles; PDF (`false`): el nombre y la ruta, estáticos.
    pub reproducibles: bool,
    /// Resuelve los medios de un carrusel (032) contra la bóveda.
    pub carrusel: Option<&'a dyn Fn(&mut orbital_vault::carrusel::Carrusel)>,
    /// Arma la lista de una galería (033) con la bóveda.
    pub galeria: Option<&'a dyn Fn(&mut orbital_vault::galeria::Galeria)>,
    /// Los bloques generados (036): por lenguaje (`indice`, `toc`) y cuerpo, su HTML estático;
    /// `None` deja el bloque como código.
    pub generado: Option<&'a dyn Fn(&str, &str) -> Option<String>>,
}

/// Lenguajes de los bloques generados que se exportan estáticos (036).
fn es_generado(lang: &str) -> bool {
    matches!(
        lang,
        "indice" | "toc" | "table-of-contents" | "tabla-de-contenidos"
    )
}

impl Medios<'_> {
    /// La ruta de un medio escrito como imagen (`![](…)`), relativa a la nota.
    fn de_imagen(&self, url: &str) -> Option<std::path::PathBuf> {
        let ruta = orbital_vault::links::percent_decode(url.split('#').next().unwrap_or(url));
        self.carpeta
            .as_ref()
            .map(|d| d.join(ruta))
            .filter(|p| p.is_file())
    }
}

/// Un medio exportado (057). Existe: con controles (HTML) o estático (PDF), con el nombre y un
/// enlace al archivo como respaldo si el lector no lo reproduce. No existe: el aviso.
fn medio_html(
    escrito: &str,
    ruta: Option<&std::path::Path>,
    tipo: orbital_vault::medios::Tipo,
    fragmento: Option<&str>,
    reproducible: bool,
) -> String {
    use orbital_formulas::html::escapar_atributo as esc;
    use orbital_vault::medios::Tipo;
    let nombre = escrito.rsplit('/').next().unwrap_or(escrito);
    let Some(ruta) = ruta else {
        return format!(
            "<p class=\"medio medio-falta\">Archivo inexistente: «{}»</p>",
            esc(nombre)
        );
    };
    use gtk::gio::prelude::FileExt;
    let uri = gtk::gio::File::for_path(ruta).uri().to_string();
    if tipo == Tipo::Imagen {
        return format!("<img src=\"{}\" alt=\"{}\">", esc(&uri), esc(nombre));
    }
    let inicio = fragmento.and_then(orbital_vault::medios::inicio);
    let (clase, etiqueta) = match tipo {
        Tipo::Video => ("Video", "video"),
        _ => ("Audio", "audio"),
    };
    let desde = inicio
        .filter(|t| *t > 0.0)
        .map(|t| format!(" (desde {})", crate::medios::tiempo(t)))
        .unwrap_or_default();
    let enlace = format!(
        "{clase}: <a href=\"{}\">{}</a>{desde}",
        esc(&uri),
        esc(nombre)
    );
    if !reproducible {
        return format!("<p class=\"medio\">{enlace}</p>");
    }
    let src = match inicio {
        Some(t) if t > 0.0 => format!("{uri}#t={t}"),
        _ => uri,
    };
    format!(
        "<figure class=\"medio\"><{etiqueta} controls preload=\"none\" src=\"{}\" aria-label=\"{clase}: {}\">{enlace}</{etiqueta}><figcaption>{enlace}</figcaption></figure>",
        esc(&src),
        esc(nombre)
    )
}

/// Como [`html_md`], con los medios incrustados (057) según `medios`.
pub fn html_md_con(texto: &str, nombre_archivo: &str, medios: &Medios) -> String {
    // Los ids de bloque (`^id`, 026) no se exportan.
    let cuerpo = orbital_vault::bloques::sin_ids(cuerpo_sin_frontmatter(texto));
    pagina(
        &fragmento_html(cuerpo.as_ref(), medios),
        &titulo(texto, nombre_archivo),
    )
}

/// Markdown a HTML con los medios (057), las fórmulas (050), los gráficos y el código resaltado.
fn fragmento_html(cuerpo: &str, medios: &Medios) -> String {
    let mut opts = Options::empty();
    opts.insert(Options::ENABLE_TABLES);
    opts.insert(Options::ENABLE_STRIKETHROUGH);
    opts.insert(Options::ENABLE_TASKLISTS);
    opts.insert(Options::ENABLE_MATH);
    let mut body = String::with_capacity(cuerpo.len() * 2);
    push_html(
        &mut body,
        con_formulas(
            cuerpo,
            con_medios(
                cuerpo,
                Parser::new_ext(cuerpo, opts).into_offset_iter(),
                medios,
            ),
        ),
    );
    body
}

/// Los medios (057): un párrafo que es solo `![[clip.mp3]]` y las imágenes `![](clip.mp4)` que
/// son audio o video.
fn con_medios<'a>(
    cuerpo: &'a str,
    eventos: impl Iterator<Item = (Event<'a>, Range<usize>)> + 'a,
    medios: &'a Medios<'a>,
) -> impl Iterator<Item = (Event<'a>, Range<usize>)> + 'a {
    // Lo que se saltea hasta el cierre: un párrafo o una imagen ya reemplazados.
    let mut saltar: Option<TagEnd> = None;
    // El cuerpo de un carrusel (032) que se está leyendo.
    let mut carrusel: Option<String> = None;
    // El cuerpo de una galería (033) que se está leyendo.
    let mut galeria: Option<String> = None;
    // Un bloque generado (036): lenguaje y cuerpo.
    let mut generado: Option<(String, String)> = None;
    eventos.filter_map(move |(ev, rango)| {
        if let Some((_, texto)) = &mut generado {
            return match ev {
                Event::Text(t) => {
                    texto.push_str(&t);
                    None
                }
                Event::End(TagEnd::CodeBlock) => {
                    let (lang, texto) = generado.take()?;
                    let html = medios
                        .generado
                        .and_then(|f| f(&lang, &texto))
                        .unwrap_or_else(|| {
                            format!(
                                "<pre><code class=\"language-{lang}\">{}</code></pre>",
                                orbital_formulas::html::escapar_atributo(&texto)
                            )
                        });
                    Some((Event::Html((html + "\n").into()), rango))
                }
                _ => None,
            };
        }
        if let Some(texto) = &mut galeria {
            return match ev {
                Event::Text(t) => {
                    texto.push_str(&t);
                    None
                }
                Event::End(TagEnd::CodeBlock) => {
                    let texto = galeria.take()?;
                    Some((Event::Html(galeria_html(&texto, medios).into()), rango))
                }
                _ => None,
            };
        }
        if let Some(texto) = &mut carrusel {
            return match ev {
                Event::Text(t) => {
                    texto.push_str(&t);
                    None
                }
                Event::End(TagEnd::CodeBlock) => {
                    let texto = carrusel.take()?;
                    Some((Event::Html(carrusel_html(&texto, medios).into()), rango))
                }
                _ => None,
            };
        }
        if let Event::Start(Tag::CodeBlock(CodeBlockKind::Fenced(info))) = &ev
            && orbital_vault::galeria::es_lenguaje(info.split_whitespace().next().unwrap_or(""))
        {
            galeria = Some(String::new());
            return None;
        }
        if let Event::Start(Tag::CodeBlock(CodeBlockKind::Fenced(info))) = &ev
            && medios.generado.is_some()
            && es_generado(info.split_whitespace().next().unwrap_or(""))
        {
            let lang = info.split_whitespace().next().unwrap_or("").to_owned();
            generado = Some((lang, String::new()));
            return None;
        }
        if let Event::Start(Tag::CodeBlock(CodeBlockKind::Fenced(info))) = &ev
            && orbital_vault::carrusel::es_lenguaje(info.split_whitespace().next().unwrap_or(""))
        {
            carrusel = Some(String::new());
            return None;
        }
        if let Some(fin) = &saltar {
            if matches!(&ev, Event::End(t) if t == fin) {
                saltar = None;
            }
            return None;
        }
        let (html, fin) = match &ev {
            Event::Start(Tag::Paragraph) => (
                medio_de_parrafo(&cuerpo[rango.clone()], medios)
                    .or_else(|| parrafo_con_medios(&cuerpo[rango.clone()], medios)),
                TagEnd::Paragraph,
            ),
            Event::Start(Tag::Image { dest_url, .. }) => {
                (medio_de_imagen(dest_url, medios), TagEnd::Image)
            }
            _ => (None, TagEnd::Paragraph),
        };
        match html {
            Some(h) => {
                saltar = Some(fin);
                let ev = if fin == TagEnd::Image {
                    Event::InlineHtml(h.into())
                } else {
                    Event::Html((h + "\n").into())
                };
                Some((ev, rango))
            }
            None => Some((ev, rango)),
        }
    })
}

/// Un carrusel (032) para papel o para el navegador: la grilla estática con cada medio y su
/// leyenda (las imágenes como `file://`, como las demás).
fn carrusel_html(cuerpo: &str, medios: &Medios) -> String {
    use gtk::gio::prelude::FileExt;
    let mut c = orbital_vault::carrusel::parse(cuerpo);
    match medios.carrusel {
        Some(resolver) => resolver(&mut c),
        None => {
            // Sin bóveda: lo escrito, relativo a la carpeta de la nota.
            for d in &mut c.diapositivas {
                let abs = medios.carpeta.as_ref().map(|dir| dir.join(&d.escrito));
                if let (Some(abs), Ok(path)) = (abs, orbital_vault::RelPath::new(&d.escrito))
                    && abs.is_file()
                {
                    d.estado = orbital_vault::carrusel::Estado::Encontrado { path, abs };
                }
            }
        }
    }
    let src = |d: &orbital_vault::carrusel::Diapositiva| {
        d.abs()
            .map(|p| gtk::gio::File::for_path(p).uri().to_string())
    };
    let mut html = orbital_vault::carrusel::html(&c, &src, medios.reproducibles);
    html.push('\n');
    html
}

/// Una galería (033) para papel o para el navegador: la grilla estática de su carpeta, con la
/// misma disposición (CSS, sin JavaScript) y las imágenes como `file://`. Sin bóveda (exportar
/// un archivo suelto) no hay índice para listar la carpeta: queda el motivo.
fn galeria_html(cuerpo: &str, medios: &Medios) -> String {
    use gtk::gio::prelude::FileExt;
    let mut g = orbital_vault::galeria::parse(cuerpo);
    match medios.galeria {
        Some(resolver) => resolver(&mut g),
        None if g.vacia.is_none() => {
            g.vacia = Some("La galería se muestra con la bóveda abierta".into());
        }
        None => {}
    }
    let src = |e: &orbital_vault::galeria::Elemento| {
        Some(gtk::gio::File::for_path(&e.abs).uri().to_string())
    };
    let mut html = orbital_vault::galeria::html(&g, &src, medios.reproducibles);
    html.push('\n');
    html
}

/// `![[clip.mp3]]` solo en su párrafo.
fn medio_de_parrafo(fuente: &str, medios: &Medios) -> Option<String> {
    let adjunto = medios.adjunto?;
    let (escrito, ancla) = orbital_vault::medios::incrustacion(fuente)?;
    let tipo = orbital_vault::medios::tipo_de(&escrito)?;
    let ruta = adjunto(&escrito);
    Some(medio_html(
        &escrito,
        ruta.as_deref(),
        tipo,
        ancla.as_deref(),
        medios.reproducibles,
    ))
}

/// Un párrafo con algún `![[medio]]` entre otras cosas (varios en un renglón, o con texto):
/// cada medio en su lugar y lo de alrededor como Markdown, igual que en la Lectura. Antes
/// quedaban como texto («![[a.png]] ![[b.png]]»). `None` si no tiene ninguno.
fn parrafo_con_medios(fuente: &str, medios: &Medios) -> Option<String> {
    let adjunto = medios.adjunto?;
    if !fuente.contains("![[") {
        return None;
    }
    let mut out = String::new();
    let (mut desde, mut buscar, mut alguno) = (0, 0, false);
    while let Some(r) = fuente[buscar..].find("![[") {
        let ini = buscar + r;
        let Some(fin_rel) = fuente[ini + 3..].find("]]") else {
            break;
        };
        let fin = ini + 3 + fin_rel + 2;
        let inner = &fuente[ini + 3..fin - 2];
        let t = orbital_vault::links::parse_link_target(inner);
        // Escapado o dentro de un `código`: queda como texto.
        let literal = (ini > 0 && fuente.as_bytes()[ini - 1] == b'\\')
            || fuente[desde..ini].matches('`').count() % 2 == 1
            || inner.contains('\n')
            || inner.contains("[[");
        let tipo = (!literal)
            .then(|| orbital_vault::medios::tipo_de(&t.path))
            .flatten();
        let Some(tipo) = tipo else {
            buscar = ini + 3;
            continue;
        };
        if !fuente[desde..ini].trim().is_empty() {
            out.push_str(&fragmento_html(&fuente[desde..ini], medios));
        }
        out.push_str(&medio_html(
            &t.path,
            adjunto(&t.path).as_deref(),
            tipo,
            t.anchor.as_deref().filter(|a| !a.is_empty()),
            medios.reproducibles,
        ));
        out.push('\n');
        alguno = true;
        desde = fin;
        buscar = fin;
    }
    if !alguno {
        return None;
    }
    if !fuente[desde..].trim().is_empty() {
        out.push_str(&fragmento_html(&fuente[desde..], medios));
    }
    Some(out)
}

/// `![](clip.mp4#t=30)`: audio o video local escrito como imagen.
fn medio_de_imagen(url: &str, medios: &Medios) -> Option<String> {
    if url.contains("://") || !orbital_vault::medios::es_reproducible(url) {
        return None;
    }
    let tipo = orbital_vault::medios::tipo_de(url)?;
    let (ruta, fragmento) = url
        .split_once('#')
        .map_or((url, None), |(r, f)| (r, Some(f)));
    let escrito = orbital_vault::links::percent_decode(ruta);
    Some(medio_html(
        &escrito,
        medios.de_imagen(url).as_deref(),
        tipo,
        fragmento,
        medios.reproducibles,
    ))
}

/// Las fórmulas (050) como SVG del mismo motor que la Lectura: `$…$`, `$$…$$` y ` ```math `. Un
/// `$` de cierre seguido de un dígito no cierra (dinero), igual que en Lectura. Los gráficos
/// (052, ` ```grafico `), como SVG estático para papel. Los bloques de código con lenguaje
/// conocido, resaltados con los colores de Código pasados a papel (055).
fn con_formulas<'a>(
    cuerpo: &'a str,
    eventos: impl Iterator<Item = (Event<'a>, Range<usize>)> + 'a,
) -> impl Iterator<Item = Event<'a>> + 'a {
    let mut en_bloque: Option<(String, Cercado)> = None;
    eventos.filter_map(move |(ev, rango)| {
        if let Some((texto, _)) = &mut en_bloque {
            return match ev {
                Event::Text(t) => {
                    texto.push_str(&t);
                    None
                }
                Event::End(TagEnd::CodeBlock) => {
                    let (texto, cercado) = en_bloque.take()?;
                    Some(Event::Html(
                        match cercado {
                            Cercado::Grafico => grafico_html(&texto),
                            Cercado::Diagrama => diagrama_html(&texto),
                            Cercado::Formula => formula_html(&texto, true, true),
                            Cercado::Codigo(lang) => codigo_html(&lang, &texto),
                        }
                        .into(),
                    ))
                }
                _ => None,
            };
        }
        match ev {
            Event::Start(Tag::CodeBlock(CodeBlockKind::Fenced(ref lang)))
                if matches!(
                    lang.split_whitespace().next(),
                    Some("math" | "grafico" | "mermaid")
                ) =>
            {
                en_bloque = Some((
                    String::new(),
                    match lang.split_whitespace().next() {
                        Some("grafico") => Cercado::Grafico,
                        Some("mermaid") => Cercado::Diagrama,
                        _ => Cercado::Formula,
                    },
                ));
                None
            }
            Event::Start(Tag::CodeBlock(CodeBlockKind::Fenced(ref info)))
                if crate::resaltado::puede(info.split_whitespace().next().unwrap_or("")) =>
            {
                let lang = info.split_whitespace().next().unwrap_or("").to_owned();
                en_bloque = Some((String::new(), Cercado::Codigo(lang)));
                None
            }
            Event::InlineMath(t) => {
                if cuerpo
                    .as_bytes()
                    .get(rango.end)
                    .is_some_and(u8::is_ascii_digit)
                {
                    Some(Event::Text(CowStr::from(cuerpo[rango].to_owned())))
                } else {
                    Some(Event::InlineHtml(formula_html(&t, false, false).into()))
                }
            }
            Event::DisplayMath(t) => Some(Event::InlineHtml(formula_html(&t, true, false).into())),
            ev => Some(ev),
        }
    })
}

/// Una fórmula como SVG accesible; con error, su fuente subrayada y el motivo como título.
fn formula_html(tex: &str, bloque: bool, sola: bool) -> String {
    let modo = if bloque {
        orbital_formulas::Modo::Bloque
    } else {
        orbital_formulas::Modo::EnLinea
    };
    let (etiqueta, clase) = if sola {
        ("div", "formula formula-bloque")
    } else if bloque {
        ("span", "formula formula-bloque")
    } else {
        ("span", "formula")
    };
    match orbital_formulas::svg(tex, modo) {
        Ok(svg) => format!("<{etiqueta} class=\"{clase}\">{}</{etiqueta}>", svg.marcado),
        Err(e) => {
            let d = if bloque { "$$" } else { "$" };
            format!(
                "<code class=\"formula-error\" title=\"{}\">{}</code>",
                orbital_formulas::html::escapar_atributo(&e.mensaje),
                orbital_formulas::html::escapar_atributo(&format!("{d}{tex}{d}"))
            )
        }
    }
}

/// Qué se junta de un bloque cercado para dibujarlo aparte.
enum Cercado {
    Formula,
    Grafico,
    /// Un diagrama Mermaid (058).
    Diagrama,
    /// Código para resaltar (055), con su lenguaje.
    Codigo(String),
}

/// Un bloque de código resaltado (055); con un lenguaje que GtkSourceView no conoce, como lo
/// escribe `pulldown-cmark`.
fn codigo_html(lang: &str, texto: &str) -> String {
    let clase = orbital_formulas::html::escapar_atributo(lang);
    // Sin tramos, el texto escapado igual que `pulldown-cmark` (`&`, `<` y `>`).
    let cuerpo = crate::resaltado::html(lang, texto)
        .unwrap_or_else(|| crate::resaltado::html_de(texto, &[]));
    format!("<pre><code class=\"language-{clase}\">{cuerpo}</code></pre>\n")
}

/// Ancho de los gráficos exportados, en píxeles (el SVG se achica si la página es más angosta).
const ANCHO_GRAFICO: u32 = 760;

/// Un gráfico (052) como SVG accesible; con error, su fuente y el motivo.
fn grafico_html(fuente: &str) -> String {
    let fuente = fuente.strip_suffix('\n').unwrap_or(fuente);
    match crate::graficos::svg(fuente, ANCHO_GRAFICO) {
        Ok(svg) => format!("<figure class=\"grafico\">{svg}</figure>\n"),
        Err(e) => {
            let e = orbital_formulas::html::escapar_atributo(&e.to_string());
            format!(
                "<pre class=\"grafico-error\" title=\"{e}\"><code>{}</code></pre>\n<p class=\"grafico-aviso\">⚠ {e}</p>\n",
                orbital_formulas::html::escapar_atributo(fuente)
            )
        }
    }
}

/// Un diagrama Mermaid (058) como SVG con los colores de papel; con error, su fuente y el motivo.
fn diagrama_html(fuente: &str) -> String {
    let fuente = fuente.strip_suffix('\n').unwrap_or(fuente);
    match crate::diagramas::svg(fuente, crate::diagramas::Tema::Claro) {
        Ok(d) => format!("<figure class=\"diagrama\">{}</figure>\n", d.marcado),
        Err(e) => {
            let e = orbital_formulas::html::escapar_atributo(&e.to_string());
            format!(
                "<pre class=\"grafico-error\" title=\"{e}\"><code class=\"language-mermaid\">{}</code></pre>\n<p class=\"grafico-aviso\">⚠ {e}</p>\n",
                orbital_formulas::html::escapar_atributo(fuente)
            )
        }
    }
}

/// Formato de exportación.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Formato {
    Html,
    Pdf,
}

/// El cuerpo de la nota: después del frontmatter si lo hay (igual que el render).
fn cuerpo_sin_frontmatter(texto: &str) -> &str {
    let base = match orbital_vault::props::markdown::locate(texto) {
        Some(region) => {
            let rest = &texto[region.end..];
            region.end + rest.find('\n').map_or(rest.len(), |i| i + 1)
        }
        None => orbital_vault::props::markdown::bom_len(texto),
    };
    &texto[base..]
}

/// Las transclusiones de medios `![[clip.mp3]]` del cuerpo de una nota `.html`, reemplazadas
/// por el medio (057). `None` si no hay ninguna.
fn medios_en_html(texto: &str, medios: &Medios) -> Option<String> {
    medios.adjunto?;
    let cuerpo = orbital_vault::html_edit::body_range(texto)?;
    let mut cambios: Vec<(Range<usize>, String)> = Vec::new();
    for occ in orbital_vault::links::extract_links(&texto[cuerpo.clone()], true) {
        if occ.kind != orbital_vault::links::LinkKind::Transclusion {
            continue;
        }
        let Some(tipo) = orbital_vault::medios::tipo_de(&occ.target.path) else {
            continue;
        };
        let ruta = medios.adjunto.and_then(|f| f(&occ.target.path));
        let html = medio_html(
            &occ.target.path,
            ruta.as_deref(),
            tipo,
            occ.target.anchor.as_deref(),
            medios.reproducibles,
        );
        let r = occ.byte_range;
        cambios.push((cuerpo.start + r.start..cuerpo.start + r.end, html));
    }
    if cambios.is_empty() {
        return None;
    }
    let mut out = texto.to_owned();
    for (r, h) in cambios.into_iter().rev() {
        out.replace_range(r, &h);
    }
    Some(out)
}

/// Página completa con los estilos embebidos (FR-2).
pub fn pagina(body: &str, titulo: &str) -> String {
    format!(
        "<!doctype html>\n<html lang=\"es\">\n<head>\n<meta charset=\"utf-8\">\n\
<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n\
<title>{titulo}</title>\n<style>{ESTILOS}{sintaxis}</style>\n</head>\n<body>\n{body}\n</body>\n</html>\n",
        // Los colores del código (055), solo si hay código resaltado.
        sintaxis = if body.contains("<span class=\"sx-") {
            crate::resaltado::CSS_PAPEL
        } else {
            ""
        }
    )
}

/// HTML de una nota `.html`: el contenido tal cual (FR-3), con las fórmulas (050) dibujadas como
/// SVG en el texto visible. Sin fórmulas, una copia exacta.
#[cfg(test)]
pub fn html_passthrough(texto: &str) -> String {
    html_passthrough_con(texto, &Medios::default())
}

/// Como [`html_passthrough`], con cada `![[clip.mp3]]` del cuerpo como el medio (057).
pub fn html_passthrough_con(texto: &str, medios: &Medios) -> String {
    let con_medios = medios_en_html(texto, medios);
    let texto = con_medios.as_deref().unwrap_or(texto);
    let con_formulas = orbital_formulas::html::reemplazar(texto, |tex, modo| {
        formula_html(tex, modo == orbital_formulas::Modo::Bloque, false)
    });
    // Los gráficos (052): `<pre><code class="language-grafico">`.
    let con_graficos = orbital_graficos::html::reemplazar(&con_formulas, grafico_html);
    // Los diagramas Mermaid (058): `<pre><code class="language-mermaid">`.
    let con_diagramas =
        orbital_graficos::html::reemplazar_bloques(&con_graficos, "mermaid", diagrama_html);
    // Los carruseles (032), como la grilla estática.
    let mut out = con_diagramas;
    for lang in ["carrusel", "media-slider"] {
        if out.contains(&format!("language-{lang}")) {
            out = orbital_graficos::html::reemplazar_bloques(&out, lang, |f| {
                carrusel_html(f, medios)
            });
        }
    }
    // Las galerías (033), también.
    for lang in ["galeria", "galería", "gallery"] {
        if out.contains(&format!("language-{lang}")) {
            out =
                orbital_graficos::html::reemplazar_bloques(&out, lang, |f| galeria_html(f, medios));
        }
    }
    // Los índices por etiqueta (036), estáticos.
    if let Some(f) = medios.generado
        && out.contains("language-indice")
    {
        out = orbital_graficos::html::reemplazar_bloques(&out, "indice", |cuerpo| {
            f("indice", cuerpo).unwrap_or_default()
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exporta_cuerpo_sin_frontmatter_con_tabla() {
        let nota = "---\ntitulo: Mi nota\ntype: nota\n---\n\n# Encabezado\n\n| a | b |\n|---|---|\n| 1 | 2 |\n\n- [ ] tarea\n";
        let out = html_md(nota, "mi-nota.md");
        assert!(out.contains("<h1>Encabezado</h1>"), "{out}");
        assert!(out.contains("<table>"), "{out}");
        assert!(out.contains("checkbox"), "{out}");
        assert!(out.contains("<title>Mi nota</title>"), "{out}");
        // El frontmatter no aparece (FR-3).
        assert!(!out.contains("type: nota"), "{out}");
        // Autónomo: estilos embebidos, sin recursos externos (FR-2).
        assert!(out.contains("<style>"), "{out}");
        assert!(!out.contains("http://"), "{out}");
        assert!(!out.contains("https://"), "{out}");
    }

    #[test]
    fn exporta_sin_ids_de_bloque() {
        let nota = "Un párrafo ^abc123\n\n| a |\n|---|\n| 1 |\n^tab001\n\n- ítem ^def456\n";
        let out = html_md(nota, "n.md");
        assert!(!out.contains('^'), "{out}");
        assert!(out.contains("<p>Un párrafo</p>"), "{out}");
        assert!(!out.contains("tab001"), "{out}");
    }

    #[test]
    fn exporta_formulas_como_svg() {
        let nota = "Sea $x^2$ y cuesta $5 o $10.\n\n$$\n\\frac{a}{b}\n$$\n\n```math\n\\sqrt{2}\n```\n\nMal: $x^^2$ y `$code$`\n";
        let out = html_md(nota, "n.md");
        assert_eq!(out.matches("<svg").count(), 3, "{out}");
        assert!(out.contains("aria-label=\"Fórmula: x^2\""), "{out}");
        assert!(out.contains("cuesta $5 o $10"), "{out}");
        assert!(
            out.contains("<div class=\"formula formula-bloque\">"),
            "{out}"
        );
        assert!(out.contains("class=\"formula-error\""), "{out}");
        assert!(out.contains("<code>$code$</code>"), "{out}");
        assert!(!out.contains("http://"), "{out}");
        let html = "<html><body><p>Sea $x$ y <code>$y$</code></p></body></html>";
        let out = html_passthrough(html);
        assert_eq!(out.matches("<svg").count(), 1, "{out}");
        assert!(out.contains("<code>$y$</code>"), "{out}");
    }

    #[test]
    fn exporta_graficos_como_svg() {
        let nota = "# G\n\n```grafico\ntítulo: Ondas\ny = sin(x)\nr = 2\n```\n\n```grafico\ny = (x\n```\n\n```rust\nlet y = 1;\n```\n";
        let out = html_md(nota, "n.md");
        assert_eq!(
            out.matches("<figure class=\"grafico\"><svg").count(),
            1,
            "{out}"
        );
        assert!(
            out.contains("aria-label=\"Gráfico: Ondas. y = sin(x); r = 2."),
            "{out}"
        );
        assert!(out.contains("class=\"grafico-error\""), "{out}");
        assert!(out.contains("Error en el gráfico (renglón 1)"), "{out}");
        assert!(out.contains("<code class=\"language-rust\">"), "{out}");
        assert!(!out.contains("http"), "{out}");
        let html = "<html><body><pre><code class=\"language-grafico\">y = x^2</code></pre><p>$x$</p></body></html>";
        let out = html_passthrough(html);
        assert_eq!(out.matches("<svg").count(), 2, "{out}");
        assert!(!out.contains("language-grafico"), "{out}");
    }

    #[test]
    fn exporta_diagramas_como_svg() {
        let nota = "# D\n\n```mermaid\ngraph TD\n  A[Año] --> B\n```\n\n```mermaid\ngraph TD\n  A -->> B[\n```\n";
        let out = html_md(nota, "d.md");
        assert_eq!(
            out.matches("<figure class=\"diagrama\"><svg").count(),
            1,
            "{out}"
        );
        assert!(
            out.contains("aria-label=\"Diagrama Mermaid: de flujo\""),
            "{out}"
        );
        assert!(out.contains("Año"), "{out}");
        assert!(out.contains("Error en el diagrama (renglón 2)"), "{out}");
        assert!(!out.contains("currentColor"), "papel: colores fijos");
        let html = "<html><body><pre><code class=\"language-mermaid\">pie\n  &quot;A&quot; : 1</code></pre></body></html>";
        let out = html_passthrough(html);
        assert!(out.contains("<figure class=\"diagrama\"><svg"), "{out}");
        assert!(!out.contains("language-mermaid"), "{out}");
    }

    #[test]
    fn nota_html_pasa_tal_cual() {
        let nota = "<!doctype html><html><body><p>hola</p></body></html>";
        assert_eq!(html_passthrough(nota), nota);
    }

    #[test]
    fn exporta_galeria_estatica() {
        let root =
            std::env::temp_dir().join(format!("orbital-033-exportar-ñ-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("fotos ñ")).unwrap();
        for n in ["b.png", "a.jpg", "clip.mp4", "notas.txt"] {
            std::fs::write(root.join("fotos ñ").join(n), b"x").unwrap();
        }
        std::fs::write(root.join("n.md"), "").unwrap();
        let vault = orbital_vault::open(&root).unwrap();
        let rel = orbital_vault::RelPath::new("n.md").unwrap();
        let resolver = |g: &mut orbital_vault::galeria::Galeria| vault.resolver_galeria(&rel, g);
        let nota = "# G\n\n```galeria\ncarpeta: fotos ñ\nlayout: masonry\n```\n";
        for reproducibles in [true, false] {
            let m = Medios {
                reproducibles,
                galeria: Some(&resolver),
                ..Medios::default()
            };
            let out = html_md_con(nota, "n.md", &m);
            assert!(out.contains("orbital-galeria-masonry"), "{out}");
            assert_eq!(out.matches("<img ").count(), 2, "{out}");
            assert!(out.find("a.jpg") < out.find("b.png"), "orden alfabético");
            assert!(out.contains("column-width"), "{out}");
            assert!(!out.contains("language-galeria"), "{out}");
            assert_eq!(out.contains("<video controls"), reproducibles, "{out}");
        }
        // Sin bóveda: el motivo, no un hueco.
        let out = html_md(nota, "n.md");
        assert!(out.contains("con la bóveda abierta"), "{out}");
        // En una nota `.html`.
        let h = "<html><body><pre><code class=\"language-galeria\">carpeta: fotos ñ</code></pre></body></html>";
        let m = Medios {
            galeria: Some(&resolver),
            ..Medios::default()
        };
        let out = html_passthrough_con(h, &m);
        assert_eq!(out.matches("<img ").count(), 2, "{out}");
        assert!(!out.contains("language-galeria"), "{out}");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn exporta_carrusel_estatico() {
        let dir =
            std::env::temp_dir().join(format!("orbital-032-exportar ñ-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("a.png"), b"x").unwrap();
        std::fs::write(dir.join("b.mp3"), b"x").unwrap();
        let nota = "# C\n\n```media-slider\nautoplay: true\n![[a.png|Uno]]\n![[b.mp3]]\n![[falta.png]]\n```\n";
        let html = |reproducibles| {
            let m = Medios {
                carpeta: Some(dir.clone()),
                adjunto: None,
                reproducibles,
                carrusel: None,
                galeria: None,
                generado: None,
            };
            html_md_con(nota, "c.md", &m)
        };
        let out = html(true);
        assert!(out.contains("aria-label=\"Carrusel: 3 medios\""), "{out}");
        assert!(out.contains("<img src=\"file://"), "{out}");
        assert!(out.contains("<figcaption"), "{out}");
        assert!(out.contains("<audio controls"), "{out}");
        assert!(out.contains("Archivo inexistente: «falta.png»"), "{out}");
        assert!(!out.contains("media-slider"), "{out}");
        let papel = html(false);
        assert!(papel.contains("Audio: <a href=\"file://"), "{papel}");
        // Una nota `.html` con el bloque.
        let h = "<html><body><pre><code class=\"language-carrusel\">![[a.png]]</code></pre></body></html>";
        let m = Medios {
            carpeta: Some(dir.clone()),
            adjunto: None,
            reproducibles: true,
            carrusel: None,
            galeria: None,
            generado: None,
        };
        let out = html_passthrough_con(h, &m);
        assert!(out.contains("orbital-carrusel"), "{out}");
        assert!(!out.contains("language-carrusel"), "{out}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn exporta_medios() {
        let dir = std::env::temp_dir().join(format!("orbital-057-exp-ñ-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("clip ñ.mp4"), b"x").unwrap();
        std::fs::write(dir.join("voz.mp3"), b"x").unwrap();
        let adjunto = |e: &str| (e == "voz.mp3").then(|| dir.join("voz.mp3"));
        let nota = "# M\n\n![[voz.mp3#t=1:30]]\n\nTexto ![video](clip%20ñ.mp4#t=5) fin.\n\n![[falta.ogg]]\n\n![](no.webm)\n\n```\n![[voz.mp3]]\n```\n";
        let html = |reproducibles| {
            let m = Medios {
                carpeta: Some(dir.clone()),
                adjunto: Some(&adjunto),
                reproducibles,
                carrusel: None,
                galeria: None,
                generado: None,
            };
            html_md_con(nota, "m.md", &m)
        };
        let out = html(true);
        assert!(
            out.contains("<audio controls preload=\"none\" src=\"file://"),
            "{out}"
        );
        assert!(out.contains("voz.mp3#t=90\""), "{out}");
        assert!(out.contains("Audio: <a href=\"file://"), "{out}");
        assert!(out.contains("(desde 1:30)"), "{out}");
        assert!(out.contains("<video controls"), "{out}");
        assert!(out.contains("aria-label=\"Video: clip ñ.mp4\""), "{out}");
        assert!(out.contains("Archivo inexistente: «falta.ogg»"), "{out}");
        assert!(out.contains("Archivo inexistente: «no.webm»"), "{out}");
        // En un bloque de código queda como texto.
        assert!(out.contains("<code>![[voz.mp3]]"), "{out}");
        assert!(!out.contains("<img"), "{out}");
        let foto = |e: &str| (e == "foto.png").then(|| dir.join("foto.png"));
        let m = Medios {
            carpeta: None,
            adjunto: Some(&foto),
            reproducibles: true,
            carrusel: None,
            galeria: None,
            generado: None,
        };
        let out = html_md_con("![[foto.png]]\n", "f.md", &m);
        assert!(out.contains("<img src=\"file://"), "{out}");
        // PDF: nada que reproducir, el nombre y la ruta.
        let pdf = html(false);
        assert!(!pdf.contains("<audio") && !pdf.contains("<video"), "{pdf}");
        assert!(
            pdf.contains("<p class=\"medio\">Audio: <a href=\"file://"),
            "{pdf}"
        );
        // Varios en un renglón, y uno entre texto (antes quedaban como texto).
        let m = Medios {
            carpeta: Some(dir.clone()),
            adjunto: Some(&adjunto),
            reproducibles: true,
            carrusel: None,
            galeria: None,
            generado: None,
        };
        let out = html_md_con(
            "![[voz.mp3]] ![[falta.png]]\n![[voz.mp3]]\n\nOí ![[voz.mp3]] y `![[voz.mp3]]` fin.\n",
            "r.md",
            &m,
        );
        assert_eq!(out.matches("<audio").count(), 3, "{out}");
        assert!(out.contains("Archivo inexistente: «falta.png»"), "{out}");
        assert!(out.contains("<p>Oí</p>"), "{out}");
        assert!(out.contains("<code>![[voz.mp3]]</code> fin.</p>"), "{out}");
        // Sin medios, lo de siempre.
        assert_eq!(
            html_md("# A\n", "a.md"),
            html_md_con("# A\n", "a.md", &Medios::default())
        );
        // Una nota `.html` con `![[voz.mp3]]` en el cuerpo.
        let h = "<html><head></head><body><p>Oí:</p><p>![[voz.mp3]]</p></body></html>";
        let m = Medios {
            carpeta: Some(dir.clone()),
            adjunto: Some(&adjunto),
            reproducibles: true,
            carrusel: None,
            galeria: None,
            generado: None,
        };
        let out = html_passthrough_con(h, &m);
        assert!(out.contains("<p><figure class=\"medio\"><audio"), "{out}");
        assert_eq!(html_passthrough(h), h);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn sin_titulo_usa_el_nombre_de_archivo() {
        let out = html_md("# Solo encabezado\n", "apunte.md");
        assert!(out.contains("<title>apunte.md</title>"), "{out}");
    }
}
