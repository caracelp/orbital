//! Vista de notas HTML con WebKitGTK (constitución, principio III).
//!
//! - Se crea recién al abrir la primera nota HTML, y una sola vista sirve para todas: así WebKit
//!   no consume nada si solo se usan notas `.md`.
//! - Sesión de red efímera (nada en disco) y una política CSP que bloquea toda carga externa: solo
//!   se permiten archivos locales y `data:` (principio V).
//! - El JavaScript de la nota nunca corre. La vista de lectura lo tiene apagado; la vista Visual
//!   (002) lo enciende solo para el script propio de Orbital, en un *world* aislado, con
//!   `enable-javascript-markup` apagado: los `<script>` de la nota se descartan al cargar.
//! - Un enlace se abre en el navegador del sistema solo si el usuario hace clic.

use std::rc::Rc;

use gtk::{gdk, gio};
use webkit6::prelude::*;

pub type LinkHandler = Rc<dyn Fn(&str)>;

thread_local! {
    /// Ya se creó una vista del contexto por defecto: su proceso web arrancó y no se le pueden
    /// agregar carpetas al sandbox.
    static SANDBOX_CERRADO: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Antes de crear una vista web del contexto por defecto (057): la primera vez, si la bóveda
/// está fuera de la carpeta personal (`/tmp`, otro disco), se monta de solo lectura en el sandbox
/// del proceso web; si no, ese proceso no ve sus imágenes ni sus medios. WebKit solo lo permite
/// antes de arrancar el primer proceso web (después es un error fatal): una bóveda de afuera que
/// se abra más tarde no se agrega. Sin vistas web (bóvedas sin `.html`) no corre nunca.
pub fn preparar_sandbox(raiz: Option<&std::path::Path>) {
    if SANDBOX_CERRADO.with(|c| c.replace(true)) {
        return;
    }
    if let Some(r) = raiz.filter(|r| hace_falta_montar(r))
        && let Some(c) = webkit6::WebContext::default()
    {
        c.add_path_to_sandbox(r, true);
    }
}

/// Si el proceso web no ve la carpeta por su cuenta y se la puede agregar: la carpeta personal
/// ya se ve (y WebKit no deja agregarla entera); `/`, `/sys`, `/proc` y `/dev` no se aceptan.
fn hace_falta_montar(r: &std::path::Path) -> bool {
    let casa = gtk::glib::home_dir();
    r.is_absolute()
        && r.parent().is_some()
        && !r.starts_with(&casa)
        && !["/sys", "/proc", "/dev"].iter().any(|p| r.starts_with(p))
        && r.is_dir()
}

/// Política de contenido que se inserta al principio del `<head>`.
const CSP: &str = "<meta http-equiv=\"Content-Security-Policy\" content=\"default-src 'none'; \
img-src file: data:; media-src file: data:; style-src 'unsafe-inline' file:; font-src file: data:\">";

/// Estilos base de Orbital (oscuro y rosa). Van antes que los de la nota, así que la nota manda.
const BASE_STYLE: &str = "<meta name=\"color-scheme\" content=\"dark\"><style>\
:root{color-scheme:dark}body{font-family:sans-serif;line-height:1.5;color:#eeeeee;background:#111113}\
a{color:#ff8cc6}</style>";

/// Inserta la política CSP justo después de `<head ...>`, o al principio si no hay `<head>`.
pub fn with_csp(html: &str) -> String {
    let lower = html.to_ascii_lowercase();
    let at = lower
        .find("<head")
        .and_then(|p| html[p..].find('>').map(|e| p + e + 1))
        .unwrap_or(0);
    let mut out = String::with_capacity(html.len() + CSP.len());
    out.push_str(&html[..at]);
    out.push_str(CSP);
    out.push_str(BASE_STYLE);
    out.push_str(&html[at..]);
    out
}

/// Crea la vista web de lectura, ya configurada.
pub fn new_view(on_link: Option<LinkHandler>) -> webkit6::WebView {
    build(None, on_link)
}

/// Crea la vista web de edición (vista Visual): solo corre el script de Orbital, que se agrega
/// en `content` en un *world* aislado (research R6 de la 002).
pub fn new_editable_view(
    content: &webkit6::UserContentManager,
    on_link: Option<LinkHandler>,
) -> webkit6::WebView {
    let view = build(Some(content), on_link);
    view.set_editable(true);
    view
}

fn build(
    content: Option<&webkit6::UserContentManager>,
    on_link: Option<LinkHandler>,
) -> webkit6::WebView {
    let settings = webkit6::Settings::new();
    settings.set_enable_javascript(content.is_some());
    settings.set_enable_javascript_markup(false);
    settings.set_enable_webaudio(false);
    settings.set_enable_webgl(false);
    settings.set_enable_html5_database(false);
    settings.set_enable_page_cache(false);
    settings.set_allow_file_access_from_file_urls(false);
    let session = webkit6::NetworkSession::new_ephemeral();
    let mut builder = webkit6::WebView::builder()
        .settings(&settings)
        .network_session(&session)
        .vexpand(true)
        .hexpand(true);
    if let Some(c) = content {
        builder = builder.user_content_manager(c);
    }
    let view = builder.build();
    // Fondo igual al de la app mientras carga.
    view.set_background_color(&gdk::RGBA::new(0.067, 0.067, 0.075, 1.0));
    view.connect_decide_policy(move |_, decision, kind| {
        if kind != webkit6::PolicyDecisionType::NavigationAction {
            return false;
        }
        let Some(nav) = decision.downcast_ref::<webkit6::NavigationPolicyDecision>() else {
            return false;
        };
        let Some(action) = nav.navigation_action() else {
            return false;
        };
        if action.navigation_type() == webkit6::NavigationType::LinkClicked {
            if let Some(uri) = action.request().and_then(|r| r.uri()) {
                if uri.starts_with("http://")
                    || uri.starts_with("https://")
                    || uri.starts_with("mailto:")
                {
                    gtk::UriLauncher::new(&uri).launch(
                        None::<&gtk::Window>,
                        None::<&gio::Cancellable>,
                        |_| {},
                    );
                } else if let Some(cb) = on_link.as_ref() {
                    cb(&uri);
                }
            }
            decision.ignore();
            return true;
        }
        false
    });
    view
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sandbox_solo_para_bovedas_de_afuera() {
        use std::path::Path;
        let casa = gtk::glib::home_dir();
        assert!(!hace_falta_montar(&casa));
        assert!(!hace_falta_montar(&casa.join("Documentos")));
        assert!(!hace_falta_montar(Path::new("/")));
        assert!(!hace_falta_montar(Path::new("/proc/self")));
        assert!(!hace_falta_montar(Path::new("relativa")));
        assert!(!hace_falta_montar(Path::new("/no-existe-orbital-057")));
        assert!(hace_falta_montar(&std::env::temp_dir()));
    }

    #[test]
    fn csp_goes_inside_head() {
        let out = with_csp("<!doctype html><html><HEAD lang=\"es\"><title>x</title></head>");
        assert!(out.starts_with("<!doctype html><html><HEAD lang=\"es\"><meta http-equiv"));
        assert!(with_csp("<p>sin head</p>").starts_with("<meta http-equiv"));
    }
}
