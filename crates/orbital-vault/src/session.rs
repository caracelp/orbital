//! Sesión de edición de una nota (data-model de la 002): sin reloj ni GTK.
//!
//! El editor trabaja con el texto sin BOM y con `\n`; al guardar se restauran el BOM y los finales
//! de línea del archivo (FR-016 de la 001). El guardado se divide en tres pasos para que la escritura
//! corra fuera del hilo principal: [`EditSession::prepare_save`] (hilo principal),
//! [`SaveJob::run`] (otro hilo) y [`EditSession::apply`] (hilo principal).

use std::fs;

use crate::entry::RelPath;
use crate::error::VaultError;
use crate::note::{Format, Note};
use crate::vault::Vault;
use crate::write::{Time, WriteCtx, backup, hash, safe_write};

/// Estado de la sesión.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SessionState {
    Clean,
    Dirty,
    Saving,
    /// El disco cambió mientras había cambios sin guardar; no se guarda solo.
    Conflict {
        disk_hash: u64,
    },
    Failed {
        message: String,
    },
}

/// Resultado de un guardado.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SaveOutcome {
    Saved { at: Time },
    NothingToSave,
    Conflict,
}

/// Qué hacer ante un cambio externo de la nota abierta.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExternalAction {
    /// Es el eco de un guardado propio.
    Ignore,
    /// No había cambios pendientes: recargar la nota.
    Reload,
    /// Había cambios pendientes: preguntar (y no guardar solo mientras tanto).
    AskUser,
}

/// Una nota abierta en el editor.
#[derive(Clone, Debug)]
pub struct EditSession {
    path: RelPath,
    format: Format,
    base_hash: u64,
    state: SessionState,
    last_saved: Option<Time>,
    failures: u32,
    bom: bool,
    crlf: bool,
}

/// Escritura preparada, lista para correr en otro hilo.
#[derive(Clone, Debug)]
pub struct SaveJob {
    path: RelPath,
    disk_text: String,
    expected: u64,
}

/// Lo que devuelve un [`SaveJob`].
#[derive(Debug)]
pub enum SaveResult {
    Written { hash: u64 },
    Conflict { disk_hash: u64 },
    Failed(VaultError),
}

impl SaveJob {
    /// Escribe (GuardadoSeguro + escritura atómica de la 001).
    pub fn run(&self, vault: &Vault, ctx: &WriteCtx) -> SaveResult {
        match safe_write(
            vault.root(),
            &self.path,
            self.disk_text.as_bytes(),
            Some(self.expected),
            ctx,
        ) {
            Ok(_) => SaveResult::Written {
                hash: hash(self.disk_text.as_bytes()),
            },
            Err(VaultError::Conflict { .. }) => SaveResult::Conflict {
                disk_hash: fs::read(self.path.to_path(vault.root()))
                    .map(|b| hash(&b))
                    .unwrap_or(0),
            },
            Err(e) => SaveResult::Failed(e),
        }
    }
}

impl EditSession {
    /// Abre una nota; devuelve la sesión y el texto para el editor (sin BOM, con `\n`).
    pub fn open(note: &Note) -> (Self, String) {
        let bom = note.source.starts_with('\u{feff}');
        let crlf = note.source.contains("\r\n");
        let text = Self::to_editor(&note.source);
        (
            Self {
                path: note.path.clone(),
                format: note.format,
                base_hash: note.hash,
                state: SessionState::Clean,
                last_saved: None,
                failures: 0,
                bom,
                crlf,
            },
            text,
        )
    }

    fn to_editor(source: &str) -> String {
        source.trim_start_matches('\u{feff}').replace("\r\n", "\n")
    }

    /// Texto del editor tal como va al disco.
    pub fn to_disk(&self, text: &str) -> String {
        let body = if self.crlf {
            text.replace("\r\n", "\n").replace('\n', "\r\n")
        } else {
            text.to_owned()
        };
        if self.bom {
            format!("\u{feff}{body}")
        } else {
            body
        }
    }

    pub fn path(&self) -> &RelPath {
        &self.path
    }
    pub fn format(&self) -> Format {
        self.format
    }
    pub fn state(&self) -> &SessionState {
        &self.state
    }
    pub fn last_saved(&self) -> Option<Time> {
        self.last_saved
    }
    pub fn failures(&self) -> u32 {
        self.failures
    }

    /// Hay algo que todavía no está en el disco.
    pub fn has_pending(&self) -> bool {
        !matches!(self.state, SessionState::Clean)
    }

    /// El usuario cambió el texto.
    pub fn edited(&mut self) {
        if matches!(
            self.state,
            SessionState::Clean | SessionState::Failed { .. }
        ) {
            self.state = SessionState::Dirty;
        }
    }

    /// Prepara el guardado. `None` si no hay nada que escribir o si hay un conflicto abierto.
    pub fn prepare_save(&mut self, text: &str) -> Option<SaveJob> {
        if matches!(self.state, SessionState::Conflict { .. }) {
            return None;
        }
        let disk_text = self.to_disk(text);
        if hash(disk_text.as_bytes()) == self.base_hash {
            self.state = SessionState::Clean;
            return None;
        }
        self.state = SessionState::Saving;
        Some(SaveJob {
            path: self.path.clone(),
            disk_text,
            expected: self.base_hash,
        })
    }

    /// Aplica el resultado de un [`SaveJob`]. `edited_meanwhile`: hubo teclas durante la escritura.
    pub fn apply(
        &mut self,
        result: SaveResult,
        at: Time,
        edited_meanwhile: bool,
    ) -> Result<SaveOutcome, VaultError> {
        match result {
            SaveResult::Written { hash } => {
                self.base_hash = hash;
                self.last_saved = Some(at);
                self.failures = 0;
                self.state = if edited_meanwhile {
                    SessionState::Dirty
                } else {
                    SessionState::Clean
                };
                Ok(SaveOutcome::Saved { at })
            }
            SaveResult::Conflict { disk_hash } => {
                self.state = SessionState::Conflict { disk_hash };
                Ok(SaveOutcome::Conflict)
            }
            SaveResult::Failed(e) => {
                self.failures += 1;
                self.state = SessionState::Failed {
                    message: e.to_string(),
                };
                Err(e)
            }
        }
    }

    /// Guardado completo en el hilo actual (para tests y para el cierre de la app).
    pub fn save(
        &mut self,
        vault: &Vault,
        text: &str,
        ctx: &WriteCtx,
    ) -> Result<SaveOutcome, VaultError> {
        if matches!(self.state, SessionState::Conflict { .. }) {
            return Ok(SaveOutcome::Conflict);
        }
        match self.prepare_save(text) {
            None => Ok(SaveOutcome::NothingToSave),
            Some(job) => {
                let result = job.run(vault, ctx);
                self.apply(result, ctx.time, false)
            }
        }
    }

    /// El monitor avisó que la nota cambió en el disco y ahora tiene `disk_hash`.
    pub fn external_change(&mut self, disk_hash: u64) -> ExternalAction {
        if disk_hash == self.base_hash {
            return ExternalAction::Ignore;
        }
        match self.state {
            SessionState::Clean => ExternalAction::Reload,
            SessionState::Conflict { .. } => {
                self.state = SessionState::Conflict { disk_hash };
                ExternalAction::AskUser
            }
            _ => {
                self.state = SessionState::Conflict { disk_hash };
                ExternalAction::AskUser
            }
        }
    }

    /// «Conservar la mía»: escribe la versión del editor; la del disco queda como respaldo.
    pub fn keep_mine(
        &mut self,
        vault: &Vault,
        text: &str,
        ctx: &WriteCtx,
    ) -> Result<(), VaultError> {
        let current = fs::read(self.path.to_path(vault.root())).ok();
        let disk_text = self.to_disk(text);
        // safe_write respalda la versión del disco antes de reemplazarla.
        safe_write(
            vault.root(),
            &self.path,
            disk_text.as_bytes(),
            current.as_deref().map(hash),
            ctx,
        )?;
        self.base_hash = hash(disk_text.as_bytes());
        self.last_saved = Some(ctx.time);
        self.failures = 0;
        self.state = SessionState::Clean;
        Ok(())
    }

    /// «Usar la del disco»: la versión del editor queda como respaldo; devuelve el texto del disco
    /// para el editor.
    pub fn use_disk(
        &mut self,
        vault: &Vault,
        mine: &str,
        ctx: &WriteCtx,
    ) -> Result<String, VaultError> {
        let bytes = fs::read(self.path.to_path(vault.root()))?;
        let disk = String::from_utf8(bytes).map_err(|_| VaultError::NotUtf8(self.path.clone()))?;
        let disk_hash = hash(disk.as_bytes());
        backup(
            vault.root(),
            &self.path,
            self.to_disk(mine).as_bytes(),
            disk_hash,
            ctx,
        )?;
        self.bom = disk.starts_with('\u{feff}');
        self.crlf = disk.contains("\r\n");
        self.base_hash = disk_hash;
        self.state = SessionState::Clean;
        Ok(Self::to_editor(&disk))
    }
}
