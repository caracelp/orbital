//! Archivos externos: URL asociadas, PDF, importar, exportar, tarjetas de URL y OnStage.

use super::*;

impl Window {
    /// El webview de la URL asociada (009), creado al primer uso con una sesión de red efímera:
    /// nada de lo que cargue (cookies, caché, almacenamiento) queda en disco.
    pub(super) fn url_web(i: &Inner) -> webkit6::WebView {
        if let Some(web) = i.url_web.borrow().as_ref() {
            return web.clone();
        }
        crate::html_view::preparar_sandbox(i.vault.borrow().as_ref().map(|v| v.root()));
        let session = webkit6::NetworkSession::new_ephemeral();
        let web = webkit6::WebView::builder()
            .network_session(&session)
            .vexpand(true)
            .hexpand(true)
            .build();
        i.url_page.append(&web);
        *i.url_web.borrow_mut() = Some(web.clone());
        web
    }

    /// Pide la URL y la asocia al archivo (009, US1): precargada si ya tiene (FR-1.3).
    pub fn asociar_url_dialog(&self, ruta: &RelPath) {
        let i = &self.0;
        let Some(vault) = i.vault.borrow().clone() else {
            return;
        };
        let this = self.clone();
        let ruta = ruta.clone();
        glib::spawn_future_local(async move {
            let actual = gio::spawn_blocking({
                let ruta = ruta.clone();
                move || vault.url_de(&ruta)
            })
            .await
            .ok()
            .flatten();
            let campo = gtk::Entry::builder()
                .text(actual.unwrap_or_default())
                .activates_default(true)
                .build();
            campo.update_property(&[gtk::accessible::Property::Label("URL del archivo")]);
            let dialog = adw::AlertDialog::new(
                Some("Asociar URL al archivo"),
                Some("Solo http y https. Queda en .orbital/enlaces.yaml."),
            );
            dialog.set_extra_child(Some(&campo));
            dialog.add_response("cancel", "Cancelar");
            dialog.add_response("asociar", "Asociar");
            dialog.set_default_response(Some("asociar"));
            dialog.set_response_appearance("asociar", adw::ResponseAppearance::Suggested);
            dialog.set_close_response("cancel");
            let weak = dialog.downgrade();
            campo.connect_changed(move |e| {
                if let Some(d) = weak.upgrade() {
                    let ok = e.text().trim().starts_with("http://")
                        || e.text().trim().starts_with("https://");
                    d.set_response_enabled("asociar", ok);
                }
            });
            dialog.set_response_enabled("asociar", false);
            if dialog.choose_future(Some(&this.0.window)).await != "asociar" {
                return;
            }
            let url = campo.text().trim().to_owned();
            let Some(v) = this.0.vault.borrow().clone() else {
                return;
            };
            let r = ruta.clone();
            let resultado =
                gio::spawn_blocking(move || v.asociar_url(&r, &url, &write_ctx::now(None))).await;
            match resultado {
                Ok(Ok(())) => this.reload(None).await,
                Ok(Err(error)) => this.toast(&format!("No se pudo asociar la URL: {error}")),
                Err(_) => this.toast("No se pudo asociar la URL"),
            }
        });
    }

    /// Card link (013): lee la URL del portapapeles, arma el bloque `cardlink` con los
    /// metadatos disponibles (host derivado de la URL; título/descr/image por red si gvfs
    /// lo resuelve; sin red, el bloque queda con la URL y el host) y lo inserta en la nota.
    pub fn card_link(&self) {
        let i = &self.0;
        if i.code_editor.current_path().is_none() {
            self.toast("Abrí una nota antes de insertar un card link");
            return;
        }
        let this = self.clone();
        glib::spawn_future_local(async move {
            let clip = this
                .0
                .window
                .clipboard()
                .read_text_future()
                .await
                .map(|t| t.map(String::from).unwrap_or_default())
                .unwrap_or_default();
            let url = clip.trim().to_owned();
            if !url.starts_with("http://") && !url.starts_with("https://") {
                this.toast("El portapapeles no tiene una URL http(s)");
                return;
            }
            let host = url
                .strip_prefix("https://")
                .or_else(|| url.strip_prefix("http://"))
                .and_then(|r| r.split('/').next())
                .unwrap_or("")
                .to_owned();
            let (title, description, image) =
                match gio::File::for_uri(&url).load_contents_future().await {
                    Ok((bytes, _)) => {
                        let bytes = bytes.to_vec();
                        let texto = String::from_utf8_lossy(&bytes).to_string();
                        let extraer = |tag: &str| -> Option<String> {
                            let marker = format!("{tag}\" content=\"");
                            let start = texto.find(&marker)? + marker.len();
                            let end = texto[start..].find('"')? + start;
                            Some(texto[start..end].to_owned())
                        };
                        let title = extraer("property=\"og:title\"")
                            .or_else(|| extraer("name=\"og:title\""))
                            .or_else(|| {
                                texto.find("<title>").and_then(|s| {
                                    texto[s + 7..]
                                        .find("</title>")
                                        .map(|e| texto[s + 7..s + 7 + e].to_owned())
                                })
                            });
                        let description = extraer("property=\"og:description\"")
                            .or_else(|| extraer("name=\"og:description\""));
                        let image = extraer("property=\"og:image\"")
                            .or_else(|| extraer("name=\"og:image\""));
                        (title, description, image)
                    }
                    Err(_) => (None, None, None),
                };
            let mut bloque = format!(
                "```cardlink\nurl: \"{url}\"\ntitle: \"{}\"\n",
                title.as_deref().unwrap_or(&url)
            );
            if let Some(d) = &description {
                bloque.push_str(&format!("description: \"{d}\"\n"));
            }
            bloque.push_str(&format!("host: \"{host}\"\n"));
            if let Some(img) = &image {
                bloque.push_str(&format!("image: \"{img}\"\n"));
            }
            bloque.push_str("```\n");
            let actual = this.0.code_editor.text();
            let nuevo = if actual.ends_with('\n') || actual.is_empty() {
                format!("{actual}\n{bloque}\n")
            } else {
                format!("{actual}\n\n{bloque}\n")
            };
            this.0.code_editor.replace_text(&nuevo);
            this.toast(&format!("Card link insertado: {host}"));
        });
    }

    /// Exporta la nota abierta (007): HTML autónomo o PDF vía impresión de WebKit. Lee el texto
    /// del editor (lo que se ve, con ediciones sin guardar); sin nota abierta, aviso sin diálogo.
    pub fn exportar(&self, formato: crate::export::Formato) {
        let i = &self.0;
        let Some(entry) = i.current_entry.borrow().clone() else {
            self.toast("Abrí una nota antes de exportar");
            return;
        };
        let text = i.code_editor.text();
        let nombre = entry.path.file_name().to_owned();
        let stem = nombre
            .strip_suffix(".md")
            .or_else(|| nombre.strip_suffix(".html"))
            .unwrap_or(&nombre)
            .to_owned();
        let (titulo, defecto) = match formato {
            crate::export::Formato::Html => ("Exportar nota como HTML", format!("{stem}.html")),
            crate::export::Formato::Pdf => ("Exportar nota como PDF", format!("{stem}.pdf")),
        };
        let dialog = gtk::FileDialog::builder()
            .title(titulo)
            .initial_name(&defecto)
            .build();
        let this = self.clone();
        dialog.save(
            Some(&i.window),
            None::<&gio::Cancellable>,
            move |resultado| {
                let Ok(file) = resultado else {
                    return; // cancelado (SC-3: no se escribe nada)
                };
                let Some(destino) = file.path() else {
                    return;
                };
                let this = this.clone();
                let text = text.clone();
                let formato = formato;
                glib::spawn_future_local(async move {
                    match formato {
                        crate::export::Formato::Html => {
                            let out = this.exportar_html(&text, &destino, true);
                            let out_clon = out.clone();
                            let destino_clon = destino.clone();
                            let escritura = gio::spawn_blocking(move || {
                                std::fs::write(&destino_clon, out_clon)
                            })
                            .await;
                            match escritura {
                                Ok(Ok(())) => {
                                    this.toast(&format!("Nota exportada: {}", destino.display()))
                                }
                                _ => this.toast("No se pudo escribir el archivo exportado"),
                            }
                        }
                        crate::export::Formato::Pdf => this.exportar_pdf(&text, &destino),
                    }
                });
            },
        );
    }

    /// El HTML exportado de la nota abierta. Los medios (057) se resuelven como en la Lectura:
    /// `![[clip.mp3]]` desde la nota y `![](clip.mp4)` desde su carpeta; con `reproducibles`
    /// (HTML) van con controles, si no (PDF), estáticos.
    fn exportar_html(&self, text: &str, destino: &std::path::Path, reproducibles: bool) -> String {
        let vault = self.0.vault.borrow().clone();
        let ruta = self.0.code_editor.current_path();
        let carpeta = vault
            .as_ref()
            .zip(ruta.as_ref())
            .and_then(|(v, r)| r.to_path(v.root()).parent().map(std::path::PathBuf::from));
        let adjunto = |escrito: &str| -> Option<std::path::PathBuf> {
            let (v, r) = vault.as_ref().zip(ruta.as_ref())?;
            match v.resolve_attachment(r, escrito) {
                orbital_vault::Resolution::Found(p) => Some(p.to_path(v.root())),
                _ => None,
            }
        };
        // Los carruseles (032) se resuelven como en la Lectura: medios y carpetas de la bóveda.
        let carrusel = |c: &mut orbital_vault::carrusel::Carrusel| {
            if let Some((v, r)) = vault.as_ref().zip(ruta.as_ref()) {
                v.resolver_carrusel(r, c);
            }
        };
        // Y las galerías (033): la carpeta con el índice.
        let galeria = |g: &mut orbital_vault::galeria::Galeria| {
            if let Some((v, r)) = vault.as_ref().zip(ruta.as_ref()) {
                v.resolver_galeria(r, g);
            }
        };
        // Los índices por etiqueta y las tablas de contenidos (036), estáticos: del índice en
        // memoria y del texto de la nota. Cada nota, como `file://` (como los medios: la
        // página exportada puede ir a cualquier carpeta).
        let indice = self.0.search_index.borrow().clone();
        let href = |p: &RelPath| -> String {
            use gtk::gio::prelude::FileExt;
            vault.as_ref().map_or_else(String::new, |v| {
                gtk::gio::File::for_path(p.to_path(v.root()))
                    .uri()
                    .to_string()
            })
        };
        let generado = |lang: &str, cuerpo: &str| {
            let idx = indice.as_ref().and_then(|i| i.read().ok());
            crate::editor::reader::generado_html(lang, cuerpo, text, idx.as_deref(), &href)
        };
        let medios = crate::export::Medios {
            carpeta,
            adjunto: Some(&adjunto),
            reproducibles,
            carrusel: Some(&carrusel),
            galeria: Some(&galeria),
            generado: Some(&generado),
        };
        let es_html = text.trim_start().starts_with("<!doctype html>")
            || text.trim_start().starts_with("<!DOCTYPE html>")
            || text.trim_start().starts_with("<html");
        if es_html {
            crate::export::html_passthrough_con(text, &medios)
        } else {
            crate::export::html_md_con(text, &destino.to_string_lossy(), &medios)
        }
    }

    /// PDF vía impresión de WebKit del mismo HTML autónomo: el diálogo de impresión aparece
    /// preconfigurado con «Salida a archivo» y el destino elegido (WebKit no permite exportación
    /// silenciosa). La vista vive en `exports_pendientes` hasta que la impresión termina.
    pub(super) fn exportar_pdf(&self, text: &str, destino: &std::path::Path) {
        let html = self.exportar_html(text, destino, false);
        crate::html_view::preparar_sandbox(self.0.vault.borrow().as_ref().map(|v| v.root()));
        let web = webkit6::WebView::new();
        web.load_html(&html, None);
        let destino = destino.to_path_buf();
        web.connect_load_changed(move |web, ev| {
            if ev != webkit6::LoadEvent::Finished {
                return;
            }
            let op = webkit6::PrintOperation::new(web);
            let settings = gtk::PrintSettings::new();
            settings.set("output-uri", Some(&format!("file://{}", destino.display())));
            settings.set("output-file-format", Some("pdf"));
            op.set_print_settings(&settings);
            op.print();
        });
        self.0.exports_pendientes.borrow_mut().push(web);
    }

    /// Presenta la nota abierta como diapositivas (OnStage, 014): solo notas `.md`.
    pub fn presentar(&self) {
        let Some(ruta) = self.0.code_editor.current_path() else {
            self.toast("Abrí una nota antes de presentar");
            return;
        };
        if ruta.as_str().ends_with(".html") {
            self.toast("OnStage presenta notas .md; esta nota es HTML");
            return;
        }
        self.0.onstage.presentar();
    }

    /// Abre el visor del PDF abierto de `raw/` (016, acción de ventana y botón).
    pub(super) fn ver_pdf_accion(&self) {
        let Some(ruta) = self.0.externo_ruta.borrow().clone() else {
            self.toast("Abrí un PDF de raw/ antes de verlo");
            return;
        };
        if !ruta.as_str().ends_with(".pdf") {
            self.toast("Solo se ve así un PDF");
            return;
        }
        self.ver_pdf(&ruta, &crate::visor_pdf::Ancla::default());
    }

    /// Muestra el PDF en el visor embebido, con ancla si la hay (016).
    pub(super) fn ver_pdf(&self, ruta: &RelPath, ancla: &crate::visor_pdf::Ancla) {
        let Some(vault) = self.0.vault.borrow().clone() else {
            return;
        };
        *self.0.visor_ruta.borrow_mut() = Some(ruta.clone());
        self.0.visor.abrir(vault, ruta.clone(), ancla);
        self.0.content_stack.set_visible_child_name("pdf");
    }

    /// Importa el archivo abierto de `raw/` a una nota Markdown nueva en `wiki/` (015).
    pub(super) fn importar_md(&self) {
        let Some(vault) = self.0.vault.borrow().clone() else {
            return;
        };
        let Some(ruta) = self.0.externo_ruta.borrow().clone() else {
            self.toast("Abrí un archivo de raw/ antes de importar");
            return;
        };
        let extension = ruta
            .file_name()
            .rsplit_once('.')
            .map(|(_, e)| e.to_ascii_lowercase())
            .unwrap_or_default();
        if orbital_vault::importar::formato_de_ext(&extension).is_none() {
            self.toast("Solo se importan archivos PDF, DOCX o HTML");
            return;
        }
        let this = self.clone();
        glib::spawn_future_local(async move {
            let result = gio::spawn_blocking(move || {
                let ctx = write_ctx::now(Some(vault.config()));
                orbital_vault::importar::crear_nota_importada(&vault, &ruta, &ctx)
            })
            .await;
            match result {
                Ok(Ok(path)) => {
                    this.reload(Some(path.clone())).await;
                    this.toast(&format!("Nota creada: «{path}»"));
                }
                Ok(Err(e)) => this.toast(&e.to_string()),
                Err(_) => this.toast("No se pudo importar el archivo"),
            }
        });
    }
}
