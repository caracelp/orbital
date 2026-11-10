//! Núcleo de la bóveda de Orbital.
//!
//! Lee y escribe la bóveda según el contrato `docs/formato-boveda.md`, sin depender de GTK ni del
//! reloj: la app le pasa la fecha, la hora y el actor en un [`WriteCtx`]. Ninguna función está
//! pensada para el hilo principal de la interfaz; la app las llama desde un hilo aparte.

#![forbid(unsafe_code)]

pub mod aux_files;
pub mod config;
pub mod diff;
pub mod entry;
pub mod error;
pub mod html_edit;
pub mod link_graph;
pub mod links;
pub mod note;
pub mod okf_files;
pub mod ops;
pub mod props;
pub mod render;
pub mod scan;
pub mod session;
pub mod template;
pub mod vault;
pub mod write;

pub use aux_files::AuxFile;
pub use config::{LinkStyle, VaultConfig};
pub use diff::{DiffKind, DiffLine, diff_lines};
pub use entry::{Entry, EntryKind, RelPath, ReservedKind, Zone};
pub use error::VaultError;
pub use link_graph::LinkGraph;
pub use links::{
    BacklinkRef, LinkKind, LinkOccurrence, LinkTarget, Resolution, extract_links,
    parse_link_target, rewrite_links,
};
pub use note::{Format, Note, NoteMeta};
pub use ops::DeletePlan;
pub use vault::{
    Creation, CreationKind, ExternalChange, Layout, OpenAsPlan, Presence, Vault, apply_open_as,
    open, plan_open_as,
};
pub use write::{Actor, Date, Offset, Time, WriteCtx, WriteReceipt};
