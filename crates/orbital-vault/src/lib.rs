//! Núcleo de la bóveda de Orbital.
//!
//! Lee y escribe la bóveda según el contrato `docs/formato-boveda.md`, sin depender de GTK ni del
//! reloj: la app le pasa la fecha, la hora y el actor en un [`WriteCtx`]. Ninguna función está
//! pensada para el hilo principal de la interfaz; la app las llama desde un hilo aparte.

#![forbid(unsafe_code)]

pub mod config;
pub mod entry;
pub mod error;
pub mod links;
pub mod okf_files;
pub mod props;
pub mod scan;
pub mod template;
pub mod vault;
pub mod write;

pub use config::{LinkStyle, VaultConfig};
pub use entry::{Entry, EntryKind, RelPath, ReservedKind, Zone};
pub use error::VaultError;
pub use vault::{
    Creation, CreationKind, Layout, OpenAsPlan, Presence, Vault, apply_open_as, open, plan_open_as,
};
pub use write::{Actor, Date, Offset, Time, WriteCtx, WriteReceipt};
