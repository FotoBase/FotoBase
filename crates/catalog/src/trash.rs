//! Catalog trash (TASK-03-012, PLAN §6 P2a): soft-delete with an undo window
//! for the organizational actions that would otherwise vanish — remove from
//! album, hide, and unlink (remove a folder from the library).
//!
//! Nothing here deletes anything on disk. Each recorded action writes one
//! `trash` row (action, the photo or folder it hit, a timestamp and a JSON
//! restore payload) *after* the action itself succeeded. Undo replays the
//! reverse operation through the same mutation paths the forward action
//! used — album add, unhide, re-seed + rescan — so sidecars stay the source
//! of truth and the catalog follows. Entries older than the configured
//! window are swept on startup; an expired entry only loses its undo, the
//! action itself stays in effect.

use std::path::Path;

use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::sync::{SyncError, SyncResult};
use crate::{is_within, Catalog, ScanMode};

/// Default undo window, in days (PLAN §6 P2a).
pub const DEFAULT_TRASH_WINDOW_DAYS: u32 = 30;

const DAY_SECS: i64 = 86_400;

/// The action a trash entry can undo.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrashAction {
    AlbumRemove,
    Hide,
    Unlink,
}

impl TrashAction {
    pub fn as_str(self) -> &'static str {
        match self {
            TrashAction::AlbumRemove => "album_remove",
            TrashAction::Hide => "hide",
            TrashAction::Unlink => "unlink",
        }
    }

    pub fn from_db(value: &str) -> Option<TrashAction> {
        match value {
            "album_remove" => Some(TrashAction::AlbumRemove),
            "hide" => Some(TrashAction::Hide),
            "unlink" => Some(TrashAction::Unlink),
            _ => None,
        }
    }
}

/// One pending, undoable action as the trash view lists it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TrashEntry {
    pub id: i64,
    pub action: TrashAction,
    /// Photo path, or folder path for folder hide / unlink: the stable key
    /// that re-resolves after a rescan (row ids do not).
    pub path: String,
    /// Display name: file name, or the folder's last path component.
    pub name: String,
    /// Album name for `album_remove`; `"folder"` for a folder hide.
    pub detail: Option<String>,
    /// Unix seconds.
    pub created_at: i64,
}

/// Outcome of one or more undos.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct UndoReport {
    /// Entries whose action was reversed.
    pub restored: usize,
    /// Entries that could not be reversed any more (album deleted, photo or
    /// folder gone, already restored). They are dropped from the trash.
    pub skipped: usize,
    /// Folder roots re-added by an undone unlink; the caller rescans them so
    /// photos and their sidecar user data come back (PLAN §5 rebuild).
    pub rescan: Vec<String>,
}

impl UndoReport {
    fn absorb(&mut self, other: UndoReport) {
        self.restored += other.restored;
        self.skipped += other.skipped;
        self.rescan.extend(other.rescan);
    }
}

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn folder_name(path: &str) -> String {
    path.trim_end_matches(['/', '\\'])
        .rsplit(['/', '\\'])
        .next()
        .filter(|n| !n.is_empty())
        .unwrap_or(path)
        .to_string()
}

struct Stored {
    entry: TrashEntry,
    payload: Value,
}

impl Catalog {
    // ---- recorded actions ---------------------------------------------------

    /// Remove photos from an album, soft-deleted: each photo that really
    /// left the album lands in the trash. Returns how many were removed.
    pub fn trash_remove_from_album(
        &mut self,
        album_id: i64,
        file_ids: &[i64],
    ) -> SyncResult<usize> {
        let album = self.album(album_id)?.ok_or(SyncError::UnknownAlbum(album_id))?;
        let mut removed = 0;
        for &file_id in file_ids {
            let (path, name) = self.file_path_and_name(file_id)?;
            if self.remove_from_album(album_id, &[file_id])? == 0 {
                continue;
            }
            self.record_trash(
                TrashAction::AlbumRemove,
                &path,
                &name,
                Some(&album.name),
                &json!({ "album": album.uuid }),
            )?;
            removed += 1;
        }
        Ok(removed)
    }

    /// Hide photos (`fb:Hidden`), soft-deleted. Already-hidden photos are
    /// left alone and record nothing. Returns how many were hidden.
    pub fn hide_photos(&mut self, file_ids: &[i64]) -> SyncResult<usize> {
        let mut hidden = 0;
        for &file_id in file_ids {
            if self.file_hidden_own(file_id)? {
                continue;
            }
            let (path, name) = self.file_path_and_name(file_id)?;
            self.set_file_hidden(file_id, true)?;
            self.record_trash(TrashAction::Hide, &path, &name, None, &json!({ "scope": "photo" }))?;
            hidden += 1;
        }
        Ok(hidden)
    }

    /// Hide a folder (`.fotobase` flag; members and subfolders inherit),
    /// soft-deleted. Returns `false` when it was already hidden.
    pub fn hide_folder(&mut self, path: &str) -> SyncResult<bool> {
        if !self.set_folder_hidden_flag(path, true)? {
            return Ok(false);
        }
        self.record_trash(
            TrashAction::Hide,
            path,
            &folder_name(path),
            Some("folder"),
            &json!({ "scope": "folder" }),
        )?;
        Ok(true)
    }

    /// Unlink a folder: remove it (and its nested folder rows) from the
    /// library, soft-deleted. The files on disk are never touched; the
    /// restore payload keeps each folder's path and scan mode. Returns how
    /// many folder rows were removed.
    pub fn unlink_folder(&mut self, path: &str) -> SyncResult<usize> {
        let folders: Vec<Value> = self
            .list_folder_rows()?
            .into_iter()
            .filter(|(p, _)| p == path || is_within(p, path))
            .map(|(p, mode)| json!({ "path": p, "scan_mode": mode.as_str() }))
            .collect();
        if folders.is_empty() {
            return Ok(0);
        }
        let removed = self.remove_folder_tree(path)?;
        self.record_trash(
            TrashAction::Unlink,
            path,
            &folder_name(path),
            None,
            &json!({ "folders": folders }),
        )?;
        Ok(removed)
    }

    // ---- the reverse operations (also usable directly) ----------------------

    /// Unhide photos; any pending `hide` entry for them is dropped, there is
    /// nothing left to undo. Returns how many were actually hidden before.
    pub fn unhide_photos(&mut self, file_ids: &[i64]) -> SyncResult<usize> {
        let mut unhidden = 0;
        for &file_id in file_ids {
            if !self.file_hidden_own(file_id)? {
                continue;
            }
            let (path, _) = self.file_path_and_name(file_id)?;
            self.set_file_hidden(file_id, false)?;
            self.conn().execute(
                "DELETE FROM trash WHERE action = 'hide' AND path = ?1 AND detail IS NULL",
                [&path],
            )?;
            unhidden += 1;
        }
        Ok(unhidden)
    }

    /// Unhide a folder; its pending `hide` entry is dropped. Returns `false`
    /// when it was not hidden.
    pub fn unhide_folder(&mut self, path: &str) -> SyncResult<bool> {
        if !self.set_folder_hidden_flag(path, false)? {
            return Ok(false);
        }
        self.conn().execute(
            "DELETE FROM trash WHERE action = 'hide' AND path = ?1 AND detail = 'folder'",
            [path],
        )?;
        Ok(true)
    }

    // ---- trash view -----------------------------------------------------------

    /// Pending entries, newest first.
    pub fn trash_list(&self) -> SyncResult<Vec<TrashEntry>> {
        Ok(self.stored_entries(None)?.into_iter().map(|s| s.entry).collect())
    }

    /// Undo one entry. A reversal that can no longer apply is reported as
    /// `skipped` and the entry is dropped; an I/O or sidecar failure is an
    /// error and the entry stays, so the undo can be retried.
    pub fn undo_trash(&mut self, id: i64) -> SyncResult<UndoReport> {
        let stored = self
            .stored_entries(None)?
            .into_iter()
            .find(|s| s.entry.id == id)
            .ok_or(SyncError::UnknownTrashEntry(id))?;
        self.undo_stored(stored)
    }

    /// Undo every pending entry of one action type, newest first.
    pub fn undo_trash_all(&mut self, action: TrashAction) -> SyncResult<UndoReport> {
        let mut report = UndoReport::default();
        for stored in self.stored_entries(Some(action))? {
            report.absorb(self.undo_stored(stored)?);
        }
        Ok(report)
    }

    /// Drop entries older than `window_days` as of `now` (unix seconds): the
    /// undo window expired. Returns how many were dropped.
    pub fn sweep_trash(&mut self, window_days: u32, now: i64) -> SyncResult<usize> {
        let cutoff = now - i64::from(window_days) * DAY_SECS;
        Ok(self.conn().execute("DELETE FROM trash WHERE created_at < ?1", [cutoff])?)
    }

    // ---- internals --------------------------------------------------------------

    fn undo_stored(&mut self, stored: Stored) -> SyncResult<UndoReport> {
        let Stored { entry, payload } = stored;
        let mut report = UndoReport::default();
        let restored = match entry.action {
            TrashAction::AlbumRemove => self.undo_album_remove(&entry, &payload)?,
            TrashAction::Hide => self.undo_hide(&entry, &payload)?,
            TrashAction::Unlink => self.undo_unlink(&entry, &payload)?,
        };
        self.conn().execute("DELETE FROM trash WHERE id = ?1", [entry.id])?;
        if restored {
            report.restored = 1;
            if entry.action == TrashAction::Unlink {
                report.rescan.push(entry.path);
            }
        } else {
            report.skipped = 1;
        }
        Ok(report)
    }

    fn undo_album_remove(&mut self, entry: &TrashEntry, payload: &Value) -> SyncResult<bool> {
        let uuid = payload_str(payload, "album")?;
        let album_id: Option<i64> = self
            .conn()
            .query_row("SELECT id FROM albums WHERE uuid = ?1", [uuid], |r| r.get(0))
            .optional()?;
        let (Some(album_id), Some(file_id)) = (album_id, self.file_id_by_path(&entry.path)?) else {
            return Ok(false);
        };
        Ok(self.add_to_album(album_id, &[file_id])? > 0)
    }

    fn undo_hide(&mut self, entry: &TrashEntry, payload: &Value) -> SyncResult<bool> {
        match payload_str(payload, "scope")? {
            "folder" => match self.unhide_folder(&entry.path) {
                Ok(changed) => Ok(changed),
                Err(SyncError::UnknownFolder(_)) => Ok(false),
                Err(err) => Err(err),
            },
            _ => match self.file_id_by_path(&entry.path)? {
                Some(file_id) => Ok(self.unhide_photos(&[file_id])? > 0),
                None => Ok(false),
            },
        }
    }

    fn undo_unlink(&mut self, entry: &TrashEntry, payload: &Value) -> SyncResult<bool> {
        if !Path::new(&entry.path).is_dir() {
            return Ok(false);
        }
        let folders = payload
            .get("folders")
            .and_then(Value::as_array)
            .ok_or_else(|| SyncError::TrashPayload("unlink payload has no folders".to_string()))?;
        for folder in folders {
            let path = payload_str(folder, "path")?;
            let mode = ScanMode::from_db(payload_str(folder, "scan_mode")?);
            self.seed_folder(path)?;
            self.set_folder_scan_mode(path, mode)?;
        }
        Ok(true)
    }

    fn record_trash(
        &self,
        action: TrashAction,
        path: &str,
        name: &str,
        detail: Option<&str>,
        payload: &Value,
    ) -> SyncResult<()> {
        self.conn().execute(
            "INSERT INTO trash (action, path, name, detail, payload, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![action.as_str(), path, name, detail, payload.to_string(), now_secs()],
        )?;
        Ok(())
    }

    fn stored_entries(&self, action: Option<TrashAction>) -> SyncResult<Vec<Stored>> {
        let mut stmt = self.conn().prepare_cached(
            "SELECT id, action, path, name, detail, created_at, payload FROM trash
             WHERE ?1 IS NULL OR action = ?1 ORDER BY id DESC",
        )?;
        let rows = stmt.query_map([action.map(TrashAction::as_str)], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, Option<String>>(4)?,
                r.get::<_, i64>(5)?,
                r.get::<_, String>(6)?,
            ))
        })?;
        let mut out = Vec::new();
        for row in rows {
            let (id, action, path, name, detail, created_at, payload) = row?;
            let Some(action) = TrashAction::from_db(&action) else { continue };
            let payload = serde_json::from_str(&payload)
                .map_err(|e| SyncError::TrashPayload(e.to_string()))?;
            out.push(Stored {
                entry: TrashEntry { id, action, path, name, detail, created_at },
                payload,
            });
        }
        Ok(out)
    }

    fn file_path_and_name(&self, file_id: i64) -> SyncResult<(String, String)> {
        self.conn()
            .query_row("SELECT path, filename FROM files WHERE id = ?1", [file_id], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .optional()?
            .ok_or(SyncError::UnknownFile(file_id))
    }

    fn list_folder_rows(&self) -> SyncResult<Vec<(String, ScanMode)>> {
        let mut stmt = self.conn().prepare_cached("SELECT path, scan_mode FROM folders")?;
        let rows = stmt.query_map([], |r| {
            let mode: String = r.get(1)?;
            Ok((r.get::<_, String>(0)?, ScanMode::from_db(&mode)))
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }
}

fn payload_str<'a>(value: &'a Value, key: &str) -> SyncResult<&'a str> {
    value
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| SyncError::TrashPayload(format!("missing {key}")))
}
