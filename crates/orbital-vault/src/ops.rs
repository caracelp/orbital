//! Operaciones sobre archivos de la bóveda que mantienen `index.md` y `log.md` (FR-020 a FR-023).

use std::fs;
use std::io;

use crate::entry::RelPath;
use crate::error::VaultError;
use crate::note::Format;
use crate::okf_files::{self, IndexEntry, Verb};
use crate::props::{PropValue, find};
use crate::vault::Vault;
use crate::write::{WriteCtx, WriteReceipt, hash, safe_write};

const INDEX: &str = "wiki/index.md";
const LOG: &str = "wiki/log.md";

/// Lo que hace falta para registrar un borrado después de que la app mandó el archivo a la papelera.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct DeletePlan {
    pub path: RelPath,
    pub title: String,
    pub link: Option<String>,
    /// Sección y línea exacta que tenía en `index.md`, para reponerla tal cual al deshacer.
    pub index_line: Option<(String, String)>,
}

fn rel(s: &'static str) -> RelPath {
    RelPath::from_static(s)
}

fn stem(path: &RelPath) -> &str {
    let name = path.file_name();
    name.rsplit_once('.').map_or(name, |(s, _)| s)
}

fn merge(into: &mut WriteReceipt, other: WriteReceipt) {
    into.written.extend(other.written);
}

impl Vault {
    /// Aplica `f` a un archivo de la wiki que ya existe, releyéndolo si otro programa lo cambió
    /// entre la lectura y la escritura. Si no existe, no hace nada (FR-023).
    fn update_okf(
        &self,
        file: &'static str,
        ctx: &WriteCtx,
        f: impl Fn(&str) -> String,
    ) -> Result<WriteReceipt, VaultError> {
        let path = rel(file);
        let abs = path.to_path(self.root());
        for _ in 0..3 {
            let bytes = match fs::read(&abs) {
                Ok(b) => b,
                Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(WriteReceipt::default()),
                Err(e) => return Err(e.into()),
            };
            let text = String::from_utf8(bytes).map_err(|_| VaultError::NotUtf8(path.clone()))?;
            let new = f(&text);
            if new == text {
                return Ok(WriteReceipt::default());
            }
            match safe_write(
                self.root(),
                &path,
                new.as_bytes(),
                Some(hash(text.as_bytes())),
                ctx,
            ) {
                Err(VaultError::Conflict { .. }) => continue,
                other => return other,
            }
        }
        Err(VaultError::Conflict { path })
    }

    /// Entrada del índice para una nota de `wiki/`. `Ok(None)` si no va en el índice (no es una
    /// nota de la wiki, es un archivo reservado o está archivada).
    fn index_entry(&self, path: &RelPath) -> Result<Option<IndexEntry>, VaultError> {
        let Some(link) = okf_files::index_link(path) else {
            return Ok(None);
        };
        let Some(format) = Format::from_name(path.file_name()) else {
            return Ok(None);
        };
        if matches!(path.as_str(), INDEX | LOG) || matches!(path.file_name(), "index.md" | "log.md")
        {
            return Ok(None);
        }
        let props = self.read_props(path)?;
        let entries = props.as_ref().map_or(&[][..], |b| &b.entries[..]);
        let text = |k: &str| {
            find(entries, k)
                .and_then(|e| e.value.as_text())
                .map(str::to_owned)
        };
        if find(entries, "archived").is_some_and(|e| e.value == PropValue::Bool(true)) {
            return Ok(None);
        }
        Ok(Some(IndexEntry {
            title: text("title").unwrap_or_else(|| stem(path).to_owned()),
            link,
            description: text("description").unwrap_or_default(),
            section: text("type").unwrap_or_else(|| {
                match format {
                    Format::Md => "atomo",
                    Format::Html => "sintesis",
                }
                .to_owned()
            }),
        }))
    }

    /// Pone al día la entrada de una nota en `index.md` (sin tocar el registro).
    pub(crate) fn sync_index(
        &self,
        path: &RelPath,
        ctx: &WriteCtx,
    ) -> Result<WriteReceipt, VaultError> {
        let Some(link) = okf_files::index_link(path) else {
            return Ok(WriteReceipt::default());
        };
        if !rel(INDEX).to_path(self.root()).exists() {
            return Ok(WriteReceipt::default());
        }
        let entry = self.index_entry(path)?;
        self.update_okf(INDEX, ctx, |t| match &entry {
            Some(e) => okf_files::index_upsert(t, e),
            None => okf_files::index_remove(t, &link),
        })
    }

    fn log(&self, verb: Verb, detail: &str, ctx: &WriteCtx) -> Result<WriteReceipt, VaultError> {
        let date = ctx.date.to_string();
        self.update_okf(LOG, ctx, |t| okf_files::log_add(t, &date, verb, detail))
    }

    /// Título y enlace de una nota para el registro.
    fn log_ref(&self, path: &RelPath) -> String {
        let title = self
            .read_props(path)
            .ok()
            .flatten()
            .and_then(|b| {
                find(&b.entries, "title")
                    .and_then(|e| e.value.as_text())
                    .map(str::to_owned)
            })
            .unwrap_or_else(|| stem(path).to_owned());
        match okf_files::index_link(path) {
            Some(link) => okf_files::md_ref(&title, &link),
            None => format!("`{path}`"),
        }
    }

    fn is_wiki_note(path: &RelPath) -> bool {
        okf_files::index_link(path).is_some() && Format::from_name(path.file_name()).is_some()
    }

    /// Después de crear una nota: índice y registro.
    pub(crate) fn after_create(
        &self,
        path: &RelPath,
        ctx: &WriteCtx,
    ) -> Result<WriteReceipt, VaultError> {
        let mut r = self.sync_index(path, ctx)?;
        if Self::is_wiki_note(path) {
            merge(&mut r, self.log(Verb::Creation, &self.log_ref(path), ctx)?);
        }
        Ok(r)
    }

    /// Renombra o mueve un archivo o carpeta y pone al día `index.md` y `log.md`.
    /// Deshacer es hacer el renombre inverso.
    pub fn rename(
        &self,
        from: &RelPath,
        to: &RelPath,
        ctx: &WriteCtx,
    ) -> Result<WriteReceipt, VaultError> {
        for p in [from, to] {
            if p.as_str().split('/').any(|c| c.starts_with('.')) {
                return Err(VaultError::Hidden(p.to_string()));
            }
            if p.first_component() == "raw" {
                return Err(VaultError::ReadOnly(p.clone()));
            }
        }
        if to.first_component() == "wiki" && matches!(to.file_name(), "index.md" | "log.md") {
            return Err(VaultError::ReservedName(to.file_name().to_owned()));
        }
        let (src, dst) = (from.to_path(self.root()), to.to_path(self.root()));
        let is_dir = fs::metadata(&src)?.is_dir();
        if fs::symlink_metadata(&dst).is_ok() {
            return Err(VaultError::Conflict { path: to.clone() });
        }
        if let Some(parent) = dst.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::rename(&src, &dst)?;

        let mut receipt = WriteReceipt::default();
        let same_dir = from.as_str().rsplit_once('/').map(|p| p.0)
            == to.as_str().rsplit_once('/').map(|p| p.0);
        let verb = if same_dir { Verb::Rename } else { Verb::Move };
        let old_shown = okf_files::index_link(from)
            .map(|l| l.trim_start_matches('/').to_owned())
            .unwrap_or_else(|| from.to_string());

        if is_dir {
            if let (Some(old), Some(new)) = (okf_files::index_link(from), okf_files::index_link(to))
            {
                let (old, new) = (format!("{old}/"), format!("{new}/"));
                merge(
                    &mut receipt,
                    self.update_okf(INDEX, ctx, |t| {
                        okf_files::index_relink_prefix(t, &old, &new)
                    })?,
                );
            }
            let detail = format!(
                "carpeta `{}` (antes `{old_shown}`)",
                to.as_str().trim_start_matches("wiki/")
            );
            merge(&mut receipt, self.log(verb, &detail, ctx)?);
            return Ok(receipt);
        }

        if let Some(old_link) = okf_files::index_link(from) {
            let entry = self.index_entry(to)?;
            merge(
                &mut receipt,
                self.update_okf(INDEX, ctx, |t| match &entry {
                    Some(e) => okf_files::index_upsert_from(t, &old_link, e),
                    None => okf_files::index_remove(t, &old_link),
                })?,
            );
        } else {
            merge(&mut receipt, self.sync_index(to, ctx)?);
        }
        if Self::is_wiki_note(from) || Self::is_wiki_note(to) {
            let detail = format!("{} (antes `{old_shown}`)", self.log_ref(to));
            merge(&mut receipt, self.log(verb, &detail, ctx)?);
        }
        Ok(receipt)
    }

    /// Archiva (`archived: true`) o desarchiva una nota, sin moverla (FR-027).
    pub fn archive(
        &self,
        path: &RelPath,
        archived: bool,
        ctx: &WriteCtx,
    ) -> Result<WriteReceipt, VaultError> {
        let note = self.read_note(path)?;
        let mut r = if archived {
            self.set_prop(&note, "archived", &PropValue::Bool(true), ctx)?
        } else {
            self.remove_prop(&note, "archived", ctx)?
        };
        let reference = self.log_ref(path);
        let (verb, detail) = if archived {
            (Verb::Archive, reference)
        } else {
            (Verb::Update, format!("{reference} vuelve del archivo"))
        };
        merge(&mut r, self.log(verb, &detail, ctx)?);
        Ok(r)
    }

    /// Prepara un borrado. **No borra nada**: la app manda el archivo a la papelera (para poder
    /// deshacerlo) y después llama a [`Vault::after_delete`].
    pub fn delete(&self, path: &RelPath) -> Result<DeletePlan, VaultError> {
        if path.first_component() == "raw" {
            return Err(VaultError::ReadOnly(path.clone()));
        }
        if path.as_str().split('/').any(|c| c.starts_with('.')) {
            return Err(VaultError::Hidden(path.to_string()));
        }
        fs::symlink_metadata(path.to_path(self.root()))?;
        let title = self
            .read_props(path)
            .ok()
            .flatten()
            .and_then(|b| {
                find(&b.entries, "title")
                    .and_then(|e| e.value.as_text())
                    .map(str::to_owned)
            })
            .unwrap_or_else(|| stem(path).to_owned());
        let link = okf_files::index_link(path);
        let index_line = link.as_ref().and_then(|l| {
            fs::read_to_string(rel(INDEX).to_path(self.root()))
                .ok()
                .and_then(|t| okf_files::index_find(&t, l))
        });
        Ok(DeletePlan {
            path: path.clone(),
            title,
            link,
            index_line,
        })
    }

    /// Papelera propia de la bóveda, para cuando el sistema no tiene papelera en ese disco (tmpfs,
    /// FAT, algunos discos de red). Mueve `path` a `.orbital/papelera/<fecha>-<hora>/<ruta>` y
    /// devuelve dónde quedó, para poder deshacerlo con [`Vault::restore_from`].
    pub fn move_to_vault_trash(
        &self,
        path: &RelPath,
        ctx: &WriteCtx,
    ) -> Result<std::path::PathBuf, VaultError> {
        if path.first_component() == "raw" {
            return Err(VaultError::ReadOnly(path.clone()));
        }
        let stamp = format!(
            "{}-{:02}{:02}{:02}",
            ctx.date, ctx.time.hour, ctx.time.minute, ctx.time.second
        );
        let base = self.root().join(crate::write::ORBITAL_DIR).join("papelera");
        let mut n = 0u32;
        let dir = loop {
            let d = base.join(if n == 0 {
                stamp.clone()
            } else {
                format!("{stamp}-{n}")
            });
            if !d.exists() {
                break d;
            }
            n += 1;
        };
        let dest = path.to_path(&dir);
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::rename(path.to_path(self.root()), &dest)?;
        Ok(dest)
    }

    /// Devuelve a `path` un archivo que estaba en `trashed` (deshacer un borrado).
    pub fn restore_from(
        &self,
        trashed: &std::path::Path,
        path: &RelPath,
    ) -> Result<(), VaultError> {
        let dest = path.to_path(self.root());
        if fs::symlink_metadata(&dest).is_ok() {
            return Err(VaultError::Conflict { path: path.clone() });
        }
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::rename(trashed, &dest)?;
        Ok(())
    }

    /// Copia un archivo de afuera a `raw/` sin modificarlo (la única forma de escribir en `raw/`,
    /// FR-006). Si el nombre existe, agrega `-1`, `-2`…; nunca pisa una fuente.
    pub fn copy_into_raw(
        &self,
        src: &std::path::Path,
        _ctx: &WriteCtx,
    ) -> Result<RelPath, VaultError> {
        let raw = self.root().join("raw");
        if !raw.is_dir() {
            return Err(VaultError::InvalidPath("raw".into()));
        }
        let name = src
            .file_name()
            .and_then(|n| n.to_str())
            .map(|n| n.trim_start_matches('.').to_owned())
            .filter(|n| !n.is_empty())
            .unwrap_or_else(|| "fuente".to_owned());
        let (stem, ext) = match name.rsplit_once('.') {
            Some((s, e)) if !s.is_empty() => (s.to_owned(), format!(".{e}")),
            _ => (name.clone(), String::new()),
        };
        let bytes = fs::read(src)?;
        let mut n = 0u32;
        loop {
            let file = if n == 0 {
                name.clone()
            } else {
                format!("{stem}-{n}{ext}")
            };
            let dest = raw.join(&file);
            if fs::symlink_metadata(&dest).is_err() {
                crate::write::atomic_write(&dest, &bytes)?;
                return RelPath::new(&format!("raw/{file}"));
            }
            n += 1;
        }
    }

    /// Registra un borrado ya hecho: quita la entrada del índice y lo anota en el registro.
    pub fn after_delete(
        &self,
        plan: &DeletePlan,
        ctx: &WriteCtx,
    ) -> Result<WriteReceipt, VaultError> {
        let Some(link) = &plan.link else {
            return Ok(WriteReceipt::default());
        };
        let mut r = self.update_okf(INDEX, ctx, |t| okf_files::index_remove(t, link))?;
        if Self::is_wiki_note(&plan.path) {
            merge(
                &mut r,
                self.log(Verb::Deletion, &okf_files::md_ref(&plan.title, link), ctx)?,
            );
        }
        Ok(r)
    }

    /// Registra que una nota volvió de la papelera (deshacer un borrado): repone su línea del
    /// índice tal como estaba, o la arma de nuevo si no estaba.
    pub fn after_restore(
        &self,
        plan: &DeletePlan,
        ctx: &WriteCtx,
    ) -> Result<WriteReceipt, VaultError> {
        let path = &plan.path;
        let mut r = match (&plan.link, &plan.index_line) {
            (Some(link), Some((section, line))) => self.update_okf(INDEX, ctx, |t| {
                okf_files::index_put_line(t, link, section, line.clone())
            })?,
            _ => self.sync_index(path, ctx)?,
        };
        if Self::is_wiki_note(path) {
            let detail = format!("{} vuelve de la papelera", self.log_ref(path));
            merge(&mut r, self.log(Verb::Update, &detail, ctx)?);
        }
        Ok(r)
    }
}
