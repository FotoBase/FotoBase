//! Hidden photos and folders (TASK-03-009, PLAN §2 A, ADR-0007).
//!
//! Library-only: a photo's flag is `fb:Hidden` in its own sidecar (written
//! through the sync engine's single mutation pathway), a folder's flag is the
//! `hidden` key of its `.fotobase` file. The catalog mirrors both as
//! rebuildable cache: `files.hidden`, `folders.hidden` (the folder's own
//! flag) and `folders.hidden_eff` (own flag or any ancestor's — members
//! inherit). Read paths filter on those columns; see
//! [`Catalog::set_show_hidden`].
//!
//! The recorded, undoable entry points are [`Catalog::hide_photos`] and
//! [`Catalog::hide_folder`] in `trash.rs`; the functions here are the raw
//! flag mutations both build on.

use std::path::Path;

use fotobase_metadata::FolderMeta;
use rusqlite::OptionalExtension;

use crate::sync::{SyncError, SyncResult};
use crate::{is_within, Catalog};

impl Catalog {
    /// Set one photo's `fb:Hidden`, sidecar first, catalog mirror second.
    pub(crate) fn set_file_hidden(&mut self, file_id: i64, hidden: bool) -> SyncResult<()> {
        self.mutate_user_data(file_id, |sc| sc.set_hidden(hidden))
    }

    /// Whether a photo's own flag is set (not the inherited folder state).
    pub(crate) fn file_hidden_own(&self, file_id: i64) -> SyncResult<bool> {
        self.conn()
            .query_row("SELECT hidden FROM files WHERE id = ?1", [file_id], |r| r.get(0))
            .optional()?
            .ok_or(SyncError::UnknownFile(file_id))
    }

    /// Set a folder's own flag: `.fotobase` first, catalog second, then the
    /// inherited state of the whole subtree is recomputed. Returns whether
    /// the flag changed.
    pub(crate) fn set_folder_hidden_flag(&mut self, path: &str, hidden: bool) -> SyncResult<bool> {
        let own: Option<bool> = self
            .conn()
            .query_row("SELECT hidden FROM folders WHERE path = ?1", [path], |r| r.get(0))
            .optional()?;
        let Some(own) = own else { return Err(SyncError::UnknownFolder(path.to_string())) };
        if own == hidden {
            return Ok(false);
        }
        FolderMeta::set_hidden(Path::new(path), hidden)?;
        self.conn().execute("UPDATE folders SET hidden = ?1 WHERE path = ?2", (hidden, path))?;
        self.refresh_folder_hidden()?;
        Ok(true)
    }

    /// Scan hook: re-read the `.fotobase` of the given folders (an absent
    /// file means "not hidden") and recompute inheritance. A malformed file
    /// is logged and skipped — one bad flag file must never fail a scan.
    pub fn sync_folder_flags<'a>(
        &mut self,
        paths: impl IntoIterator<Item = &'a str>,
    ) -> SyncResult<()> {
        for path in paths {
            let hidden = match FolderMeta::read(Path::new(path)) {
                Ok(meta) => meta.hidden,
                Err(err) => {
                    tracing::warn!(path, error = %err, "unreadable folder metadata; skipping");
                    continue;
                }
            };
            self.conn().execute(
                "UPDATE folders SET hidden = ?1 WHERE path = ?2 AND hidden != ?1",
                (hidden, path),
            )?;
        }
        self.refresh_folder_hidden()
    }

    /// [`Self::sync_folder_flags`] for every folder in the catalog (rebuild).
    pub fn sync_all_folder_flags(&mut self) -> SyncResult<()> {
        let paths: Vec<String> = {
            let mut stmt = self.conn().prepare("SELECT path FROM folders")?;
            let rows = stmt.query_map([], |r| r.get(0))?;
            rows.collect::<Result<_, _>>()?
        };
        self.sync_folder_flags(paths.iter().map(String::as_str))
    }

    /// Recompute `folders.hidden_eff` = own flag OR any ancestor's. Folder
    /// rows are few (one per directory), so a full pass beats incremental
    /// bookkeeping.
    fn refresh_folder_hidden(&self) -> SyncResult<()> {
        let rows: Vec<(i64, String, bool, bool)> = {
            let mut stmt =
                self.conn().prepare("SELECT id, path, hidden, hidden_eff FROM folders")?;
            let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?;
            rows.collect::<Result<_, _>>()?
        };
        let roots: Vec<&str> =
            rows.iter().filter(|(_, _, own, _)| *own).map(|(_, p, _, _)| p.as_str()).collect();
        for (id, path, own, current) in &rows {
            let effective = *own || roots.iter().any(|root| is_within(path, root));
            if effective != *current {
                self.conn()
                    .execute("UPDATE folders SET hidden_eff = ?1 WHERE id = ?2", (effective, id))?;
            }
        }
        Ok(())
    }
}
