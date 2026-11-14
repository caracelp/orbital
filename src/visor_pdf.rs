//! Visor PDF embebido (016): WebKit con PDF.js embebido en el binario, servido por el esquema
//! propio `orbital-pdf:` sin red. Los resaltados son enlaces de notas con ancla
//! `#page=N&sel=i,f` (offsets de caracteres en el texto de la página); el PDF nunca se modifica.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use gtk::prelude::*;
use gtk::{gio, glib};
use orbital_vault::{RelPath, Vault};
use webkit6::prelude::*;
use webkit6::{WebContext, WebView};

const PDF_JS: &[u8] = include_bytes!("../assets/pdfjs/pdf.min.mjs");
const PDF_WORKER: &[u8] = include_bytes!("../assets/pdfjs/pdf.worker.min.mjs");
const SHELL: &str = include_str!("visor_shell.html");

/// Ancla de un enlace a PDF: página y (opcional) selección.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Ancla {
    pub page: Option<usize>,
    pub sel: Option<(usize, usize)>,
}

/// Parsea el ancla de un enlace a PDF: `page=3&sel=12,45` (sin el `#`; también `page=3` sola).
pub fn parse_ancla(ancla: &str) -> Ancla {
    let mut res = Ancla::default();
    for campo in ancla.split('&') {
        let Some((clave, valor)) = campo.split_once('=') else {
            continue;
        };
        match clave {
            "page" => res.page = valor.parse::<usize>().ok().filter(|p| *p >= 1),
            "sel" => {
                let mut partes = valor.split(',');
                let i = partes.next().and_then(|v| v.parse::<usize>().ok());
                let f = partes.next().and_then(|v| v.parse::<usize>().ok());
                if let (Some(i), Some(f)) = (i, f)
                    && i < f
                {
                    res.sel = Some((i, f));
                }
            }
            _ => {}
        }
    }
    res
}

/// Ancla en formato de enlace: `page=N&sel=i,f` o `page=N` si no hay selección.
pub fn ancla_a_texto(ancla: &Ancla) -> String {
    match (ancla.page, ancla.sel) {
        (Some(p), Some((i, f))) => format!("page={p}&sel={i},{f}"),
        (Some(p), None) => format!("page={p}"),
        _ => String::new(),
    }
}

/// Arma el enlace Markdown de la selección: `[texto](/raw/ruta#page=N&sel=i,f)`. El texto va
/// recortado a ~80 caracteres y con corchetes escapados.
pub fn armar_enlace(ruta: &str, page: usize, inicio: usize, fin: usize, texto: &str) -> String {
    let mut corto = texto.split_whitespace().collect::<Vec<_>>().join(" ");
    if corto.chars().count() > 80 {
        corto = corto.chars().take(80).collect::<String>() + "…";
    }
    if corto.is_empty() {
        corto = "Selección".to_owned();
    }
    let escapado = corto.replace('[', "\\[").replace(']', "\\]");
    let ancla = ancla_a_texto(&Ancla {
        page: Some(page),
        sel: Some((inicio, fin)),
    });
    format!("[{escapado}](/raw/{ruta}#{ancla})")
}

type CopiarFn = Box<dyn Fn(&str)>;
type ResaltadosFn = Box<dyn Fn()>;
type VolverFn = Box<dyn Fn()>;
/// Selección vigente en el visor: (página, inicio, fin, texto).
type Seleccion = (usize, usize, usize, String);

struct Inner {
    page: gtk::Box,
    volver: gtk::Button,
    copiar: gtk::Button,
    resaltados: gtk::Button,
    web: RefCell<Option<WebView>>,
    vault: RefCell<Option<Arc<Vault>>>,
    ruta: RefCell<Option<RelPath>>,
    /// Selección vigente: (página, inicio, fin, texto) — se limpia al abrir o cambiar.
    seleccion: RefCell<Option<Seleccion>>,
    on_copiar: RefCell<Option<CopiarFn>>,
    on_resaltados: RefCell<Option<ResaltadosFn>>,
    on_volver: RefCell<Option<VolverFn>>,
}

#[derive(Clone)]
pub struct VisorPdf(Rc<Inner>);

impl VisorPdf {
    pub fn new() -> Self {
        let header = gtk::HeaderBar::new();
        header.set_title_widget(Some(&gtk::Label::new(Some("PDF"))));
        let volver = gtk::Button::builder()
            .label("Volver al archivo")
            .css_classes(["flat"])
            .build();
        volver.update_property(&[gtk::accessible::Property::Label("Volver al archivo")]);
        let copiar = gtk::Button::builder()
            .label("Copiar enlace a la selección")
            .css_classes(["flat"])
            .sensitive(false)
            .build();
        copiar.update_property(&[gtk::accessible::Property::Label(
            "Copiar enlace a la selección",
        )]);
        let resaltados = gtk::Button::builder()
            .label("Resaltados")
            .css_classes(["flat"])
            .build();
        resaltados.update_property(&[gtk::accessible::Property::Label(
            "Buscar las notas que enlazan a este PDF",
        )]);
        header.pack_start(&volver);
        header.pack_start(&copiar);
        header.pack_end(&resaltados);
        let page = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .build();
        page.append(&header);
        page.update_property(&[gtk::accessible::Property::Label("Visor de PDF")]);
        let this = Self(Rc::new(Inner {
            page,
            volver,
            copiar,
            resaltados,
            web: RefCell::new(None),
            vault: RefCell::new(None),
            ruta: RefCell::new(None),
            seleccion: RefCell::new(None),
            on_copiar: RefCell::new(None),
            on_resaltados: RefCell::new(None),
            on_volver: RefCell::new(None),
        }));
        // Los callbacks se leen en cada clic: los botones se conectan acá una sola vez.
        let este = this.clone();
        this.0.copiar.connect_clicked(move |_| {
            let i = &este.0;
            if let Some((p, inicio, fin, texto)) = i.seleccion.borrow().as_ref()
                && let (Some(ruta), Some(f)) = (
                    i.ruta.borrow().as_ref().map(|r| r.as_str().to_owned()),
                    i.on_copiar.borrow().as_ref(),
                )
            {
                f(&armar_enlace(&ruta, *p, *inicio, *fin, texto));
            }
        });
        let este = this.clone();
        this.0.resaltados.connect_clicked(move |_| {
            if let Some(f) = este.0.on_resaltados.borrow().as_ref() {
                f();
            }
        });
        let este = this.clone();
        this.0.volver.connect_clicked(move |_| {
            if let Some(f) = este.0.on_volver.borrow().as_ref() {
                f();
            }
        });
        let teclas = gtk::EventControllerKey::new();
        teclas.set_propagation_phase(gtk::PropagationPhase::Capture);
        let este = this.clone();
        teclas.connect_key_pressed(move |_, keyval, _, _| {
            if keyval == gtk::gdk::Key::Escape
                && let Some(f) = este.0.on_volver.borrow().as_ref()
            {
                f();
                return glib::Propagation::Stop;
            }
            glib::Propagation::Proceed
        });
        this.0.page.add_controller(teclas);
        this
    }

    pub fn set_on_copiar(&self, f: impl Fn(&str) + 'static) {
        *self.0.on_copiar.borrow_mut() = Some(Box::new(f));
    }

    pub fn set_on_resaltados(&self, f: impl Fn() + 'static) {
        *self.0.on_resaltados.borrow_mut() = Some(Box::new(f));
    }

    pub fn set_on_volver(&self, f: impl Fn() + 'static) {
        *self.0.on_volver.borrow_mut() = Some(Box::new(f));
    }

    /// La página del stack de contenido.
    pub fn page(&self) -> &gtk::Box {
        &self.0.page
    }

    /// Selección de prueba (sin mouse sintético): igual que el mensaje de la cáscara.
    pub fn seleccion_de_prueba(&self, page: usize, inicio: usize, fin: usize, texto: &str) {
        *self.0.seleccion.borrow_mut() = Some((page, inicio, fin, texto.to_owned()));
        self.0.copiar.set_sensitive(true);
    }

    /// El webview (para el diagnóstico de la cáscara en pruebas).
    #[cfg(test)]
    pub fn web_de_prueba(&self) -> Option<WebView> {
        self.0.web.borrow().clone()
    }

    /// Abre el visor con el PDF de la bóveda y, si hay, el ancla (página y selección).
    pub fn abrir(&self, vault: Arc<Vault>, ruta: RelPath, ancla: &Ancla) {
        *self.0.vault.borrow_mut() = Some(vault);
        *self.0.ruta.borrow_mut() = Some(ruta.clone());
        *self.0.seleccion.borrow_mut() = None;
        self.0.copiar.set_sensitive(false);
        let web = self.web();
        let mut query = format!("ruta={}", codificar_componente(ruta.as_str()));
        if let Some(p) = ancla.page {
            query.push_str(&format!("&page={p}"));
        }
        if let Some((i, f)) = ancla.sel {
            query.push_str(&format!("&sel={i},{f}"));
        }
        let uri = format!("orbital-pdf://v/shell?{query}");
        web.load_uri(uri.as_str());
    }

    fn web(&self) -> WebView {
        if let Some(w) = self.0.web.borrow().as_ref() {
            return w.clone();
        }
        let contexto = WebContext::new();
        let debil = Rc::downgrade(&self.0);
        contexto.register_uri_scheme("orbital-pdf", move |request| {
            if let Some(inner) = debil.upgrade() {
                VisorPdf(inner.clone()).responder(request);
            }
        });
        let ucm = webkit6::UserContentManager::new();
        ucm.register_script_message_handler("orbitalPdf", None);
        let debil = Rc::downgrade(&self.0);
        ucm.connect_script_message_received(Some("orbitalPdf"), move |_, val| {
            let Some(inner) = debil.upgrade() else {
                return;
            };
            let mensaje = val.to_str();
            let mut partes = mensaje.splitn(5, '|');
            if partes.next() == Some("seleccion")
                && let (Some(p), Some(i), Some(f), Some(texto)) = (
                    partes.next().and_then(|v| v.parse().ok()),
                    partes.next().and_then(|v| v.parse().ok()),
                    partes.next().and_then(|v| v.parse().ok()),
                    partes.next().map(orbital_vault::percent_decode),
                )
            {
                *inner.seleccion.borrow_mut() = Some((p, i, f, texto));
                inner.copiar.set_sensitive(true);
            }
        });
        let web = WebView::builder()
            .web_context(&contexto)
            .user_content_manager(&ucm)
            .build();
        web.update_property(&[gtk::accessible::Property::Label("Visor del PDF")]);
        web.set_vexpand(true);
        self.0.page.append(&web);
        *self.0.web.borrow_mut() = Some(web.clone());
        web
    }

    fn responder(&self, request: &webkit6::URISchemeRequest) {
        // El URI llega como texto: orbital-pdf://v/<ruta>[?query]
        let Some(uri) = request.uri() else {
            return;
        };
        let resto = uri.as_str().strip_prefix("orbital-pdf://v/").unwrap_or("");
        let (path, query) = match resto.split_once('?') {
            Some((p, q)) => (p.to_owned(), Some(q.to_owned())),
            None => (resto.to_owned(), None),
        };
        match path.as_str() {
            "shell" => responder(request, SHELL.as_bytes(), "text/html; charset=utf-8"),
            "pdfjs" => responder(request, PDF_JS, "text/javascript"),
            "worker" => responder(request, PDF_WORKER, "text/javascript"),
            "doc" => {
                let ruta = query
                    .as_deref()
                    .and_then(|q| q.split('&').find_map(|c| c.strip_prefix("ruta=")))
                    .map(orbital_vault::percent_decode)
                    .unwrap_or_default();
                let Ok(rel) = RelPath::new(&ruta) else {
                    return responder(request, b"ruta invalida", "text/plain; charset=utf-8");
                };
                let prestamo = self.0.vault.borrow();
                let Some(vault) = prestamo.as_ref() else {
                    return responder(request, b"sin boveda", "text/plain; charset=utf-8");
                };
                match std::fs::read(rel.to_path(vault.root())) {
                    Ok(bytes) => responder(request, &bytes, "application/pdf"),
                    Err(e) => responder(
                        request,
                        format!("no se pudo leer: {e}").as_bytes(),
                        "text/plain; charset=utf-8",
                    ),
                }
            }
            _ => responder(request, b"no encontrado", "text/plain; charset=utf-8"),
        }
    }
}

fn codificar_componente(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b'/' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

fn responder(request: &webkit6::URISchemeRequest, bytes: &[u8], tipo: &str) {
    let stream = gio::MemoryInputStream::from_bytes(&glib::Bytes::from_owned(bytes.to_vec()));
    request.finish(&stream, bytes.len() as i64, Some(tipo));
}

#[cfg(test)]
mod tests {
    use super::*;

    /// US2: el ancla `page=N&sel=i,f` se parsea y se vuelve a armar sin pérdida.
    #[test]
    fn t002_ancla_redonda() {
        let ancla = parse_ancla("page=3&sel=12,45");
        assert_eq!(ancla.page, Some(3));
        assert_eq!(ancla.sel, Some((12, 45)));
        assert_eq!(ancla_a_texto(&ancla), "page=3&sel=12,45");
        // Sin selección.
        let solo = parse_ancla("page=7");
        assert_eq!(solo.page, Some(7));
        assert_eq!(solo.sel, None);
        assert_eq!(ancla_a_texto(&solo), "page=7");
        // Valores basura: sin ancla.
        assert_eq!(parse_ancla("").page, None);
        assert_eq!(parse_ancla("page=0").page, None);
        assert_eq!(parse_ancla("page=x").page, None);
        assert_eq!(parse_ancla("sel=5,5").sel, None);
        assert_eq!(parse_ancla("sel=5,2").sel, None);
    }

    /// US2.1: el enlace Markdown de la selección queda con el texto recortado y escapado.
    #[test]
    fn t002_enlace_de_seleccion() {
        let enlace = armar_enlace("doc.pdf", 3, 12, 45, "Texto de la nota al pie");
        assert_eq!(
            enlace,
            "[Texto de la nota al pie](/raw/doc.pdf#page=3&sel=12,45)"
        );
        // Texto largo: recortado a ~80 caracteres con elipsis.
        let largo = "x".repeat(200);
        let enlace = armar_enlace("doc.pdf", 1, 0, 200, &largo);
        assert!(enlace.starts_with("[xxxx"));
        assert!(enlace.contains("…]"));
        // Corchetes escapados para no romper el enlace.
        let enlace = armar_enlace("doc.pdf", 1, 0, 10, "[corchete]");
        assert!(enlace.starts_with("[\\[corchete\\]]"));
        // Texto vacío: rótulo genérico.
        assert!(armar_enlace("doc.pdf", 1, 0, 1, "").starts_with("[Selección]"));
    }
}

#[cfg(test)]
mod shell_tests {
    use super::*;
    use std::time::Duration;

    /// Diagnóstico de la cáscara (requiere sesión gráfica): qué dice document.title y el body.
    #[test]
    #[ignore = "requiere una sesión gráfica"]
    fn visor_shell_carga() {
        gtk::init().expect("se necesita acceso GTK");
        let raiz = std::env::temp_dir().join(format!("orbital-visor-shell-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&raiz);
        std::fs::create_dir_all(raiz.join("raw")).unwrap();
        let pdf = pdf_minimo_de_prueba("Informe anual del proyecto");
        std::fs::write(raiz.join("raw/informe.pdf"), &pdf).unwrap();
        let vault = Arc::new(orbital_vault::open(&raiz).unwrap());
        let ruta = RelPath::new("raw/informe.pdf").unwrap();

        let visor = VisorPdf::new();
        let window = gtk::Window::new();
        window.set_child(Some(visor.page()));
        window.present();
        visor.abrir(vault, ruta, &Ancla::default());
        let contexto = glib::MainContext::default();
        contexto.block_on(async {
            for _ in 0..40 {
                glib::timeout_future(Duration::from_millis(250)).await;
            }
            let Some(web) = visor.web_de_prueba() else {
                panic!("el webview no se creó");
            };
            let titulo = evaluar(&web, "document.title").await;
            println!("DOCUMENT.TITLE: {titulo:?}");
            let body = evaluar(&web, "document.body.innerText.slice(0, 200)").await;
            println!("BODY: {body:?}");
            let worker = evaluar(&web, "typeof pdfjsLib !== 'undefined' ? 'si' : 'no'").await;
            println!("PDFJS: {worker:?}");
            assert!(
                titulo.contains("página") || titulo.contains("No se pudo"),
                "título: {titulo:?}"
            );
        });
        let _ = std::fs::remove_dir_all(&raiz);
    }

    async fn evaluar(web: &WebView, script: &str) -> String {
        web.evaluate_javascript_future(script, None, None)
            .await
            .expect("JavaScript de diagnóstico")
            .to_str()
            .to_string()
    }

    fn pdf_minimo_de_prueba(texto: &str) -> Vec<u8> {
        let stream = format!("BT /F1 24 Tf 72 720 Td ({texto}) Tj ET");
        let objetos = [
            (1u32, "<< /Type /Catalog /Pages 2 0 R >>".to_owned()),
            (2, "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned()),
            (
                3,
                "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents 4 0 R \
                 /Resources << /Font << /F1 5 0 R >> >> >>"
                    .to_owned(),
            ),
            (
                4,
                format!(
                    "<< /Length {} >>\nstream\n{stream}\nendstream",
                    stream.len()
                ),
            ),
            (
                5,
                "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_owned(),
            ),
        ];
        let mut out = b"%PDF-1.4\n".to_vec();
        let mut offsets = std::collections::HashMap::new();
        for (n, cuerpo) in &objetos {
            offsets.insert(n, out.len());
            out.extend(format!("{n} 0 obj\n{cuerpo}\nendobj\n").as_bytes());
        }
        let xref = out.len();
        out.extend(b"xref\n0 6\n0000000000 65535 f \n");
        for n in 1..=5u32 {
            out.extend(format!("{:010} 00000 n \n", offsets[&n]).as_bytes());
        }
        out.extend(
            format!("trailer\n<< /Size 6 /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n").as_bytes(),
        );
        out
    }
}

#[cfg(test)]
mod sc1_tests {
    use super::*;
    use std::time::{Duration, Instant};

    /// SC-1: abrir un PDF de ~5 MiB muestra la primera página en menos de 2 s.
    #[test]
    #[ignore = "requiere una sesión gráfica"]
    fn sc1_pdf_5mib_primer_pagina_en_menos_de_dos_segundos() {
        gtk::init().expect("se necesita acceso GTK");
        let raiz = std::env::temp_dir().join(format!("orbital-visor-sc1-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&raiz);
        std::fs::create_dir_all(raiz.join("raw")).unwrap();
        // Una página con un stream grande: el tamaño del archivo es la variable de SC-1.
        let mut stream = String::new();
        while stream.len() < 5_200_000 {
            stream
                .push_str("BT /F1 12 Tf 72 700 Td (Texto de relleno del informe) Tj 0 -20 Td ET\n");
        }
        let objetos = [
            (1u32, "<< /Type /Catalog /Pages 2 0 R >>".to_owned()),
            (2, "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned()),
            (
                3,
                "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents 4 0 R \
                 /Resources << /Font << /F1 5 0 R >> >> >>"
                    .to_owned(),
            ),
            (
                4,
                format!(
                    "<< /Length {} >>\nstream\n{stream}\nendstream",
                    stream.len()
                ),
            ),
            (
                5,
                "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_owned(),
            ),
        ];
        let mut pdf = b"%PDF-1.4\n".to_vec();
        let mut offsets = std::collections::HashMap::new();
        for (n, cuerpo) in &objetos {
            offsets.insert(n, pdf.len());
            pdf.extend(format!("{n} 0 obj\n{cuerpo}\nendobj\n").as_bytes());
        }
        let xref = pdf.len();
        pdf.extend(b"xref\n0 6\n0000000000 65535 f \n");
        for n in 1..=5u32 {
            pdf.extend(format!("{:010} 00000 n \n", offsets[&n]).as_bytes());
        }
        pdf.extend(
            format!("trailer\n<< /Size 6 /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n").as_bytes(),
        );
        assert!(pdf.len() > 5_000_000, "{} bytes", pdf.len());
        std::fs::write(raiz.join("raw/grande.pdf"), &pdf).unwrap();
        let vault = Arc::new(orbital_vault::open(&raiz).unwrap());
        let ruta = RelPath::new("raw/grande.pdf").unwrap();

        let visor = VisorPdf::new();
        let window = gtk::Window::new();
        window.set_child(Some(visor.page()));
        window.present();
        let t0 = Instant::now();
        visor.abrir(vault, ruta, &Ancla::default());
        let contexto = glib::MainContext::default();
        let mut listo = None;
        contexto.block_on(async {
            for _ in 0..200 {
                glib::timeout_future(Duration::from_millis(100)).await;
                if let Some(web) = visor.web_de_prueba()
                    && let Ok(v) = web
                        .evaluate_javascript_future("document.title", None, None)
                        .await
                    && v.to_str().contains("página")
                {
                    listo = Some(Instant::now() - t0);
                    return;
                }
            }
        });
        let duracion = listo.expect("el visor no terminó de cargar en 20 s");
        println!(
            "SC-1 PDF de {} KiB, primera página: {:.2} s",
            pdf.len() / 1024,
            duracion.as_secs_f64()
        );
        assert!(
            duracion.as_secs_f64() < 2.0,
            "la primera página tardó {duracion:?}"
        );
        let _ = std::fs::remove_dir_all(&raiz);
    }
}
