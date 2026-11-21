//! Los colores de Orbital para los diagramas: rosa y negro.
//!
//! - [`Tema::Oscuro`]: la base oscura de la app (Lectura, Dividida, OnStage, el globito).
//! - [`Tema::Claro`]: una vista clara y el papel (exportar a HTML y PDF).
//! - [`Tema::Html`]: la Visual de una nota `.html`, cuyo fondo no se conoce: el texto y las
//!   líneas van en `currentColor` y los rellenos son rosas translúcidos, legibles sobre blanco y
//!   sobre negro.
//!
//! El tema que pida el propio diagrama (`%%{init: {theme: …}}%%`) lo respeta el motor; Orbital no
//! lo pisa.

use std::borrow::Cow;

use mermaid_svg::Theme;

/// Con qué colores se dibuja un diagrama.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Tema {
    Oscuro,
    Claro,
    Html,
}

type Paleta = [Cow<'static, str>; 12];

const fn c(s: &'static str) -> Cow<'static, str> {
    Cow::Borrowed(s)
}

/// Rellenos por categoría (torta, línea de tiempo, mapa mental…) sobre la base oscura: tonos
/// medios, con texto claro u oscuro según el contraste (lo elige el motor).
static PALETA_OSCURA: Paleta = [
    c("#B8467C"),
    c("#4B5D8A"),
    c("#C9823F"),
    c("#2F8A88"),
    c("#6B3B56"),
    c("#8A5CB8"),
    c("#3E8A56"),
    c("#9C3D6B"),
    c("#C982A7"),
    c("#B8474C"),
    c("#5C5862"),
    c("#7A4A8C"),
];

/// Rellenos por categoría sobre papel: pasteles con texto oscuro.
static PALETA_CLARA: Paleta = [
    c("#FFC2DF"),
    c("#C9D6F5"),
    c("#FBD9B5"),
    c("#BDEBE9"),
    c("#FFD1E6"),
    c("#E0CCF5"),
    c("#C8EBD2"),
    c("#FFE1EF"),
    c("#EBC9DB"),
    c("#F9C4C6"),
    c("#E2E0E4"),
    c("#FFD6EA"),
];

/// Rellenos translúcidos para `.html`: se ven sobre cualquier fondo.
static PALETA_HTML: Paleta = [
    c("rgba(255,140,198,0.45)"),
    c("rgba(77,155,255,0.35)"),
    c("rgba(233,151,63,0.40)"),
    c("rgba(83,223,221,0.35)"),
    c("rgba(184,70,124,0.45)"),
    c("rgba(160,110,230,0.35)"),
    c("rgba(68,207,110,0.35)"),
    c("rgba(255,173,213,0.35)"),
    c("rgba(201,130,167,0.40)"),
    c("rgba(255,92,98,0.35)"),
    c("rgba(153,148,158,0.35)"),
    c("rgba(255,210,230,0.40)"),
];

impl Tema {
    pub(crate) fn theme(self) -> Theme {
        let base = Theme::default_theme();
        let mut t = match self {
            Tema::Oscuro => Theme {
                fg: c("#EEE9ED"),
                fg_muted: c("#99949E"),
                bg: c("#161618"),
                actor_fill: c("#2A1D27"),
                actor_stroke: c("#FF8CC6"),
                lifeline: c("#6B3B56"),
                arrow_stroke: c("#C5B9C5"),
                note_fill: c("#382532"),
                note_stroke: c("#B8467C"),
                activation_fill: c("#402336"),
                activation_stroke: c("#FF8CC6"),
                frame_label_fill: c("#382532"),
                flow_node_fill: c("#2A1D27"),
                flow_node_stroke: c("#FF8CC6"),
                flow_edge_stroke: c("#C5B9C5"),
                flow_label_bg: c("#1E1E22"),
                flow_cluster_fill: c("#1E1E22"),
                flow_cluster_stroke: c("#6B3B56"),
                cscale_palette: Cow::Borrowed(&PALETA_OSCURA),
                pie_palette: Cow::Borrowed(&PALETA_OSCURA),
                git_palette: Cow::Borrowed(&PALETA_OSCURA),
                xychart_palette: Cow::Borrowed(&PALETA_OSCURA[..10]),
                pie_stroke: Some(c("#161618")),
                ..base
            },
            Tema::Claro => Theme {
                fg: c("#1A1A1C"),
                fg_muted: c("#55555C"),
                bg: c("#FFFFFF"),
                actor_fill: c("#FFF0F7"),
                actor_stroke: c("#C2185B"),
                lifeline: c("#D9A0BE"),
                arrow_stroke: c("#3A3A40"),
                note_fill: c("#FFF4FA"),
                note_stroke: c("#B8467C"),
                activation_fill: c("#FFE1EF"),
                activation_stroke: c("#C2185B"),
                frame_label_fill: c("#FFE1EF"),
                flow_node_fill: c("#FFF0F7"),
                flow_node_stroke: c("#C2185B"),
                flow_edge_stroke: c("#3A3A40"),
                flow_label_bg: c("#F2F2F4"),
                flow_cluster_fill: c("#FFF7FB"),
                flow_cluster_stroke: c("#FF8CC6"),
                cscale_palette: Cow::Borrowed(&PALETA_CLARA),
                pie_palette: Cow::Borrowed(&PALETA_CLARA),
                git_palette: Cow::Borrowed(&PALETA_CLARA),
                xychart_palette: Cow::Borrowed(&PALETA_CLARA[..10]),
                pie_stroke: Some(c("#FFFFFF")),
                ..base
            },
            Tema::Html => Theme {
                fg: c("currentColor"),
                fg_muted: c("currentColor"),
                bg: c("transparent"),
                actor_fill: c("rgba(255,140,198,0.15)"),
                actor_stroke: c("#E0559A"),
                lifeline: c("#E0559A"),
                arrow_stroke: c("currentColor"),
                note_fill: c("rgba(255,140,198,0.25)"),
                note_stroke: c("#B8467C"),
                activation_fill: c("rgba(255,140,198,0.35)"),
                activation_stroke: c("#E0559A"),
                frame_label_fill: c("rgba(255,140,198,0.25)"),
                flow_node_fill: c("rgba(255,140,198,0.15)"),
                flow_node_stroke: c("#E0559A"),
                flow_edge_stroke: c("currentColor"),
                flow_label_bg: c("rgba(128,128,128,0.18)"),
                flow_cluster_fill: c("rgba(255,140,198,0.06)"),
                flow_cluster_stroke: c("#E0559A"),
                cscale_palette: Cow::Borrowed(&PALETA_HTML),
                pie_palette: Cow::Borrowed(&PALETA_HTML),
                git_palette: Cow::Borrowed(&PALETA_HTML),
                xychart_palette: Cow::Borrowed(&PALETA_HTML[..10]),
                pie_stroke: Some(c("currentColor")),
                ..base
            },
        };
        t.font_family = c("sans-serif");
        // Tamaño fijo en píxeles (`width`/`height`): la imagen y las páginas lo achican si no
        // entra (el CSS de exportar y de la Visual pone `max-width: 100%`).
        t.responsive = false;
        t
    }

    /// Los colores que el motor no toma del tema (las barras del Gantt, sus franjas), cambiados
    /// por los de Orbital.
    fn reemplazos(self) -> &'static [(&'static str, &'static str)] {
        match self {
            Tema::Oscuro => &[
                ("\"#8a90dd\"", "\"#B8467C\""),
                ("\"#534fbc\"", "\"#FF8CC6\""),
                ("\"#bfc7ff\"", "\"#6B3B56\""),
                ("\"#d3d3d3\"", "\"#3A3540\""),
                ("\"#808080\"", "\"#99949E\""),
                ("\"#6666ff\"", "\"#FF8CC6\""),
                ("\"#fff400\"", "\"#B8467C\""),
                ("\"#ffffff\"", "\"#EEE9ED\""),
                ("\"#ff8888\"", "\"#FF5C62\""),
            ],
            Tema::Claro => &[
                ("\"#8a90dd\"", "\"#E0559A\""),
                ("\"#534fbc\"", "\"#C2185B\""),
                ("\"#bfc7ff\"", "\"#FFC2DF\""),
                ("\"#6666ff\"", "\"#FF8CC6\""),
                ("\"#fff400\"", "\"#F7A8CC\""),
            ],
            Tema::Html => &[
                ("\"#8a90dd\"", "\"#E0559A\""),
                ("\"#534fbc\"", "\"#B8467C\""),
                ("\"#bfc7ff\"", "\"rgba(255,140,198,0.35)\""),
                ("\"#6666ff\"", "\"#FF8CC6\""),
                ("\"#fff400\"", "\"#B8467C\""),
            ],
        }
    }
}

/// El SVG del motor con los colores que no salen del tema cambiados (solo en los Gantt, que son
/// los que los traen fijos).
pub(crate) fn ajustar(svg: &str, tema: Tema) -> String {
    if !svg.contains("#8a90dd") && !svg.contains("#6666ff") {
        return svg.to_owned();
    }
    let mut out = svg.to_owned();
    for (de, a) in tema.reemplazos() {
        if out.contains(de) {
            out = out.replace(de, a);
        }
    }
    out
}
