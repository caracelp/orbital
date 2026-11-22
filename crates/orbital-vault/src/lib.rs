//! Núcleo de la bóveda de Orbital.
//!
//! Lee y escribe la bóveda según el contrato `docs/formato-boveda.md`, sin depender de GTK ni del
//! reloj: la app le pasa la fecha, la hora y el actor en un [`WriteCtx`]. Ninguna función está
//! pensada para el hilo principal de la interfaz; la app las llama desde un hilo aparte.

#![forbid(unsafe_code)]

pub mod aux_files;
pub mod bases;
mod bloque_medios;
pub mod bloques;
pub mod carrusel;
pub mod config;
pub mod consulta;
pub mod diff;
pub mod edit_plan;
pub mod enlaces;
pub mod entry;
pub mod error;
pub mod fold;
pub mod formulas;
pub mod galeria;
pub mod historial;
pub mod html_edit;
pub mod html_text;
pub mod importar;
pub mod indice;
pub mod link_graph;
pub mod links;
pub mod listas;
pub mod markers;
pub mod medios;
pub mod medios_nota;
pub mod note;
pub mod note_ids;
pub mod okf_files;
pub mod ops;
pub mod plegado;
pub mod props;
pub mod query;
pub mod relaciones;
pub mod rename_links;
pub mod render;
pub mod saved;
pub mod scan;
pub mod search;
pub mod session;
pub mod tags;
pub mod tarjetas;
pub mod template;
pub mod toc;
pub mod transclusion;
pub mod ubicacion;
pub mod vault;
pub mod write;

pub use aux_files::AuxFile;
pub use config::{LinkStyle, VaultConfig};
pub use diff::{DiffKind, DiffLine, diff_lines};
pub use edit_plan::{EditPlan, FileEdit};
pub use enlaces::Enlace;
pub use entry::{Entry, EntryKind, RelPath, ReservedKind, Zone};
pub use error::VaultError;
pub use fold::fold;
pub use historial::{leer_version, versiones as versiones_historial};
pub use html_text::html_text;
pub use link_graph::LinkGraph;
pub use links::{
    BacklinkRef, LinkKind, LinkOccurrence, LinkTarget, Resolution, extract_links,
    parse_link_target, percent_decode,
};
pub use listas::{Coleccion, Lista, Listas, ListasSnapshot};
pub use markers::{Marker, find_markers, resolve_markers, resolve_markers_html, section_ranges};
pub use note::{Format, Note, NoteMeta};
pub use ops::DeletePlan;
pub use plegado::{Clase, Clave, Plegable, Plegado, PlegadoSnapshot};
pub use query::{Atom, Clause, Query, QueryError, parse_query};
pub use relaciones::{Relacion, RelacionInfo, TIPOS_BASE, TipoBase};
pub use rename_links::{LinkChange, LinkUpdatePlan};
pub use saved::{SavedSearch, SavedSearchSnapshot};
pub use search::{Match, NoteHit, SearchIndex, SearchOptions, SearchResults};
pub use tags::{TagNode, TagOccurrence, TagTree, extract_tags, inline_fields};
pub use vault::{
    Creation, CreationKind, ExternalChange, Layout, OpenAsPlan, Presence, Vault, apply_open_as,
    open, plan_open_as,
};
pub use write::{Actor, Date, Offset, Time, WriteCtx, WriteReceipt};
