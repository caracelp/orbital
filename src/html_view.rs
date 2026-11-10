//! Vista de notas HTML con WebKitGTK (constitución, principio III).
//!
//! - Se crea recién al abrir la primera nota HTML, y una sola vista sirve para todas: así WebKit
//!   no consume nada si solo se usan notas `.md`.
//! - Sesión de red efímera (nada en disco) y una política CSP que bloquea toda carga externa: solo
//!   se permiten archivos locales y `data:` (principio V).
//! - JavaScript apagado hasta que exista la especificación de aislamiento de las notas HTML.
//! - Un enlace se abre en el navegador del sistema solo si el usuario hace clic.

use gtk::{gdk, gio};
use webkit6::prelude::*;

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

/// Crea la vista web, ya configurada.
pub fn new_view() -> webkit6::WebView {
    let settings = webkit6::Settings::new();
    settings.set_enable_javascript(false);
    settings.set_enable_javascript_markup(false);
    settings.set_enable_webaudio(false);
    settings.set_enable_webgl(false);
    settings.set_enable_html5_database(false);
    settings.set_enable_page_cache(false);
    settings.set_allow_file_access_from_file_urls(false);
    let session = webkit6::NetworkSession::new_ephemeral();
    let view = webkit6::WebView::builder()
        .settings(&settings)
        .network_session(&session)
        .vexpand(true)
        .hexpand(true)
        .build();
    // Fondo igual al de la app mientras carga.
    view.set_background_color(&gdk::RGBA::new(0.067, 0.067, 0.075, 1.0));
    view.connect_decide_policy(|_, decision, kind| {
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
            // Nunca se navega dentro de la vista: el enlace se abre afuera, porque el usuario lo pidió.
            if let Some(uri) = action.request().and_then(|r| r.uri()) {
                gtk::UriLauncher::new(&uri).launch(
                    None::<&gtk::Window>,
                    None::<&gio::Cancellable>,
                    |_| {},
                );
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
    fn csp_goes_inside_head() {
        let out = with_csp("<!doctype html><html><HEAD lang=\"es\"><title>x</title></head>");
        assert!(out.starts_with("<!doctype html><html><HEAD lang=\"es\"><meta http-equiv"));
        assert!(with_csp("<p>sin head</p>").starts_with("<meta http-equiv"));
    }
}
