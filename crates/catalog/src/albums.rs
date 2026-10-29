//! Virtual albums (TASK-03-008, PLAN §2 B Albums, §5 mapping).
//!
//! An album is a named, dated collection that never copies or moves a photo.
//! Two pieces of data, two homes (00-010 §2):
//!
//! - **Membership** is user data: each member's sidecar carries the album's
//!   UUID in the custom `fb:Albums` bag. It is written through the sync
//!   engine's single mutation pathway (sidecar first, catalog mirror second).
//! - **Definitions** (name, date, place, description, cover pick) are not
//!   per-photo, so they live in `albums.json` next to the catalog file,
//!   written with the same discipline as sidecars (backup, `.tmp` + rename,
//!   unknown fields preserved). Deleting the catalog therefore loses
//!   nothing: reopening re-imports the definitions and the next
//!   rebuild-from-sidecars restores the membership.
//!
//! The `albums` / `album_files` tables are a derived cache of both.

use std::io::Write;
use std::path::{Path, PathBuf};

use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::sync::{SyncError, SyncResult};
use crate::{file_info_row, fts_match_expr, Catalog, FileInfo, Result, FILE_INFO_COLS};

/// File name of the definitions document, next to the catalog file.
pub const ALBUMS_FILE: &str = "albums.json";

const FORMAT_VERSION: u32 = 1;

/// Name given to an album whose UUID is found in a sidecar but has no
/// definition (`albums.json` lost): membership must not be dropped.
const RECOVERED_NAME: &str = "Recovered album";

/// Name of an album imported with a member reference but no definition.
pub(crate) const IMPORTED_PLACEHOLDER_NAME: &str = fotobase_metadata::UNNAMED_ALBUM;

/// One album with its derived list stats (album-list UI row).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AlbumInfo {
    pub id: i64,
    /// Stable id written to members' `fb:Albums`.
    pub uuid: String,
    pub name: String,
    /// ISO `YYYY-MM-DD`.
    pub date: Option<String>,
    pub place: Option<String>,
    pub description: Option<String>,
    /// Number of member photos.
    pub count: i64,
    /// The user-picked cover if it is still a member, else the first-added
    /// member; `None` for an empty album.
    pub cover_file_id: Option<i64>,
    /// Content hash of the cover, the key of its `thumb://` URI.
    pub cover_hash: Option<String>,
}

// ---- albums.json ----------------------------------------------------------

#[derive(Debug, Serialize, Deserialize)]
struct AlbumsDoc {
    #[serde(default = "default_version")]
    version: u32,
    #[serde(default)]
    albums: Vec<AlbumDef>,
    /// Fields written by a newer Fotobase: kept verbatim.
    #[serde(flatten)]
    extra: Map<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AlbumDef {
    id: String,
    name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    date: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    place: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    description: Option<String>,
    /// Path of the picked cover photo (paths re-resolve after a re-scan,
    /// row ids do not).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    cover: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    created_at: Option<String>,
    #[serde(flatten)]
    extra: Map<String, Value>,
}

fn default_version() -> u32 {
    FORMAT_VERSION
}

enum DocState {
    Missing,
    /// Present but unreadable: never overwritten silently (the next write
    /// backs it up to `.fb-bak` first).
    Invalid,
    Ok(AlbumsDoc),
}

fn read_doc(path: &Path) -> DocState {
    let bytes = match std::fs::read(path) {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return DocState::Missing,
        Err(e) => {
            tracing::warn!(path = %path.display(), error = %e, "cannot read albums.json");
            return DocState::Invalid;
        }
    };
    match serde_json::from_slice(&bytes) {
        Ok(doc) => DocState::Ok(doc),
        Err(e) => {
            tracing::warn!(path = %path.display(), error = %e, "malformed albums.json");
            DocState::Invalid
        }
    }
}

/// Write the definitions of every album row to `path` (merge-on-write:
/// unknown fields of the existing document survive; the previous version is
/// backed up; the replace is atomic).
fn persist_definitions(conn: &Connection, path: &Path) -> Result<()> {
    let existing = match read_doc(path) {
        DocState::Ok(doc) => Some(doc),
        _ => None,
    };
    let mut stmt = conn.prepare_cached(
        "SELECT uuid, name, date, place, description, cover_path, created_at
         FROM albums WHERE uuid IS NOT NULL ORDER BY id",
    )?;
    let defs = stmt
        .query_map([], |r| {
            Ok(AlbumDef {
                id: r.get(0)?,
                name: r.get(1)?,
                date: r.get(2)?,
                place: r.get(3)?,
                description: r.get(4)?,
                cover: r.get(5)?,
                created_at: r.get(6)?,
                extra: Map::new(),
            })
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    let albums = defs
        .into_iter()
        .map(|mut def| {
            if let Some(old) =
                existing.as_ref().and_then(|d| d.albums.iter().find(|a| a.id == def.id))
            {
                def.extra = old.extra.clone();
            }
            def
        })
        .collect();
    let doc = AlbumsDoc {
        version: existing.as_ref().map_or(FORMAT_VERSION, |d| d.version.max(FORMAT_VERSION)),
        albums,
        extra: existing.map(|d| d.extra).unwrap_or_default(),
    };
    let mut bytes = serde_json::to_vec_pretty(&doc).map_err(std::io::Error::other)?;
    bytes.push(b'\n');
    write_atomic(path, &bytes)?;
    Ok(())
}

fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    if path.exists() {
        if let Err(err) = std::fs::copy(path, fotobase_metadata::backup_path(path)) {
            tracing::warn!(path = %path.display(), error = %err, "albums.json backup failed; continuing");
        }
    }
    let tmp = fotobase_metadata::tmp_path(path);
    {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
    }
    if let Err(err) = std::fs::rename(&tmp, path) {
        let _ = std::fs::remove_file(&tmp);
        return Err(err);
    }
    Ok(())
}

// ---- membership mirror (called by the sync engine) --------------------------

/// Make `album_files` for `file_id` match the sidecar's `fb:Albums` ids.
/// An id with no definition becomes a placeholder album so membership is
/// never silently dropped. Returns `true` when a placeholder was created
/// (the caller then re-persists the definitions).
pub(crate) fn apply_membership(conn: &Connection, file_id: i64, uuids: &[String]) -> Result<bool> {
    let mut created = false;
    let mut wanted: Vec<i64> = Vec::new();
    for uuid in uuids.iter().map(|u| u.trim()).filter(|u| !u.is_empty()) {
        let existing: Option<i64> = conn
            .query_row("SELECT id FROM albums WHERE uuid = ?1", [uuid], |r| r.get(0))
            .optional()?;
        let id = match existing {
            Some(id) => id,
            None => {
                conn.execute(
                    "INSERT INTO albums (uuid, name) VALUES (?1, ?2)",
                    params![uuid, RECOVERED_NAME],
                )?;
                created = true;
                conn.last_insert_rowid()
            }
        };
        if !wanted.contains(&id) {
            wanted.push(id);
        }
    }

    let current: Vec<i64> = {
        let mut stmt =
            conn.prepare_cached("SELECT album_id FROM album_files WHERE file_id = ?1")?;
        let rows = stmt.query_map([file_id], |r| r.get(0))?;
        rows.collect::<std::result::Result<_, _>>()?
    };
    for album_id in current.iter().filter(|id| !wanted.contains(id)) {
        conn.execute(
            "DELETE FROM album_files WHERE album_id = ?1 AND file_id = ?2",
            [album_id, &file_id],
        )?;
    }
    for album_id in wanted.iter().filter(|id| !current.contains(id)) {
        conn.execute(
            "INSERT INTO album_files (album_id, file_id, position)
             VALUES (?1, ?2, COALESCE((SELECT MAX(position) + 1 FROM album_files WHERE album_id = ?1), 0))",
            [album_id, &file_id],
        )?;
    }
    Ok(created)
}

// ---- validation -------------------------------------------------------------

fn clean_name(name: &str) -> SyncResult<String> {
    let name = name.trim();
    if name.is_empty() {
        return Err(SyncError::EmptyAlbumName);
    }
    Ok(name.to_string())
}

fn clean_optional(value: Option<&str>) -> Option<String> {
    value.map(str::trim).filter(|v| !v.is_empty()).map(str::to_string)
}

/// `YYYY-MM-DD` with a real calendar day.
fn clean_date(date: &str) -> SyncResult<String> {
    let date = date.trim();
    let invalid = || SyncError::InvalidAlbumDate(date.to_string());
    let parts: Vec<&str> = date.split('-').collect();
    let [y, m, d] = parts.as_slice() else { return Err(invalid()) };
    let digits = |s: &str, len: usize| s.len() == len && s.bytes().all(|b| b.is_ascii_digit());
    if !(digits(y, 4) && digits(m, 2) && digits(d, 2)) {
        return Err(invalid());
    }
    let (y, m, d): (u32, u32, u32) = (
        y.parse().map_err(|_| invalid())?,
        m.parse().map_err(|_| invalid())?,
        d.parse().map_err(|_| invalid())?,
    );
    let leap = (y % 4 == 0 && y % 100 != 0) || y % 400 == 0;
    let days = match m {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap => 29,
        2 => 28,
        _ => return Err(invalid()),
    };
    if d == 0 || d > days {
        return Err(invalid());
    }
    Ok(date.to_string())
}

// ---- Catalog API ------------------------------------------------------------

/// Album-list columns. Hidden photos (TASK-03-009) count toward neither the
/// member total nor the cover pick unless "show hidden" is on.
fn album_cols(not_hidden: &str) -> String {
    format!(
        "a.id, a.uuid, a.name, a.date, a.place, a.description,
     (SELECT COUNT(*) FROM album_files af JOIN files f ON f.id = af.file_id
       WHERE af.album_id = a.id{not_hidden}),
     COALESCE(
        (SELECT f.id FROM album_files af JOIN files f ON f.id = af.file_id
          WHERE af.album_id = a.id AND f.path = a.cover_path{not_hidden}),
        (SELECT af.file_id FROM album_files af JOIN files f ON f.id = af.file_id
          WHERE af.album_id = a.id{not_hidden}
          ORDER BY af.position, af.file_id LIMIT 1))"
    )
}

fn album_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<AlbumInfo> {
    Ok(AlbumInfo {
        id: r.get(0)?,
        uuid: r.get(1)?,
        name: r.get(2)?,
        date: r.get(3)?,
        place: r.get(4)?,
        description: r.get(5)?,
        count: r.get(6)?,
        cover_file_id: r.get(7)?,
        cover_hash: None,
    })
}

impl Catalog {
    /// Where the album definitions live: `albums.json` beside the catalog.
    pub fn albums_json_path(&self) -> PathBuf {
        self.path().with_file_name(ALBUMS_FILE)
    }

    /// Create an album. `date` is required (`YYYY-MM-DD`); blank place and
    /// description are stored as none.
    pub fn create_album(
        &mut self,
        name: &str,
        date: &str,
        place: Option<&str>,
        description: Option<&str>,
    ) -> SyncResult<AlbumInfo> {
        let name = clean_name(name)?;
        let date = clean_date(date)?;
        let (place, description) = (clean_optional(place), clean_optional(description));
        let uuid = uuid::Uuid::new_v4().to_string();
        let json = self.albums_json_path();

        let tx = self.conn_mut().transaction()?;
        tx.execute(
            "INSERT INTO albums (uuid, name, date, place, description) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![uuid, name, date, place, description],
        )?;
        let id = tx.last_insert_rowid();
        persist_definitions(&tx, &json)?;
        tx.commit()?;
        self.album(id)?.ok_or(SyncError::UnknownAlbum(id))
    }

    /// Create an album with a caller-chosen id (an ini import, whose id is
    /// derived from the ini token so a rerun finds the same album). An
    /// existing album is left as the user shaped it — except a placeholder
    /// name, which a real `name` replaces. Returns whether it was created.
    pub(crate) fn ensure_album(
        &mut self,
        uuid: &str,
        name: &str,
        date: Option<&str>,
        place: Option<&str>,
        description: Option<&str>,
    ) -> SyncResult<bool> {
        let existing: Option<String> = self
            .conn()
            .query_row("SELECT name FROM albums WHERE uuid = ?1", [uuid], |r| r.get(0))
            .optional()?;
        let json = self.albums_json_path();
        let tx = self.conn_mut().transaction()?;
        match existing {
            None => {
                tx.execute(
                    "INSERT INTO albums (uuid, name, date, place, description)
                     VALUES (?1, ?2, ?3, ?4, ?5)",
                    params![uuid, name, date, place, description],
                )?;
            }
            Some(old) if old == IMPORTED_PLACEHOLDER_NAME && name != old => {
                tx.execute(
                    "UPDATE albums SET name = ?2, date = COALESCE(date, ?3),
                            place = COALESCE(place, ?4), description = COALESCE(description, ?5)
                     WHERE uuid = ?1",
                    params![uuid, name, date, place, description],
                )?;
                persist_definitions(&tx, &json)?;
                tx.commit()?;
                return Ok(false);
            }
            Some(_) => return Ok(false),
        }
        persist_definitions(&tx, &json)?;
        tx.commit()?;
        Ok(true)
    }

    /// Change an album's name/date/place/description. Membership and the
    /// sidecars are untouched: they reference the stable UUID.
    pub fn update_album(
        &mut self,
        album_id: i64,
        name: &str,
        date: &str,
        place: Option<&str>,
        description: Option<&str>,
    ) -> SyncResult<AlbumInfo> {
        let name = clean_name(name)?;
        let date = clean_date(date)?;
        let (place, description) = (clean_optional(place), clean_optional(description));
        self.album(album_id)?.ok_or(SyncError::UnknownAlbum(album_id))?;
        let json = self.albums_json_path();

        let tx = self.conn_mut().transaction()?;
        tx.execute(
            "UPDATE albums SET name = ?1, date = ?2, place = ?3, description = ?4 WHERE id = ?5",
            params![name, date, place, description, album_id],
        )?;
        persist_definitions(&tx, &json)?;
        tx.commit()?;
        self.album(album_id)?.ok_or(SyncError::UnknownAlbum(album_id))
    }

    /// Delete an album only: its id leaves every member's sidecar, the
    /// definition leaves `albums.json`, and the photos stay exactly as they
    /// were. A failing sidecar aborts with the album still present (members
    /// already processed are already out of it), so the delete can be
    /// retried.
    pub fn delete_album(&mut self, album_id: i64) -> SyncResult<()> {
        let album = self.album(album_id)?.ok_or(SyncError::UnknownAlbum(album_id))?;
        for file_id in self.album_file_ids(album_id)? {
            self.mutate_user_data(file_id, |sc| {
                let rest = sc.albums().into_iter().filter(|u| *u != album.uuid).collect();
                sc.set_albums(rest);
            })?;
        }
        let json = self.albums_json_path();
        let tx = self.conn_mut().transaction()?;
        tx.execute("DELETE FROM albums WHERE id = ?1", [album_id])?;
        persist_definitions(&tx, &json)?;
        tx.commit()?;
        Ok(())
    }

    /// Add photos to an album: one sidecar write per photo not already in
    /// it. Returns how many were added.
    pub fn add_to_album(&mut self, album_id: i64, file_ids: &[i64]) -> SyncResult<usize> {
        let album = self.album(album_id)?.ok_or(SyncError::UnknownAlbum(album_id))?;
        let mut added = 0;
        for &file_id in file_ids {
            if self.is_member(album_id, file_id)? {
                continue;
            }
            self.mutate_user_data(file_id, |sc| {
                let mut uuids = sc.albums();
                if !uuids.contains(&album.uuid) {
                    uuids.push(album.uuid.clone());
                }
                sc.set_albums(uuids);
            })?;
            added += 1;
        }
        Ok(added)
    }

    /// Remove photos from an album (the photos themselves stay). Returns how
    /// many were removed.
    pub fn remove_from_album(&mut self, album_id: i64, file_ids: &[i64]) -> SyncResult<usize> {
        let album = self.album(album_id)?.ok_or(SyncError::UnknownAlbum(album_id))?;
        let mut removed = 0;
        for &file_id in file_ids {
            if !self.is_member(album_id, file_id)? {
                continue;
            }
            self.mutate_user_data(file_id, |sc| {
                let rest = sc.albums().into_iter().filter(|u| *u != album.uuid).collect();
                sc.set_albums(rest);
            })?;
            removed += 1;
        }
        Ok(removed)
    }

    /// Pick the album's cover. Only a member can be the cover.
    pub fn set_album_cover(&mut self, album_id: i64, file_id: i64) -> SyncResult<AlbumInfo> {
        self.album(album_id)?.ok_or(SyncError::UnknownAlbum(album_id))?;
        if !self.is_member(album_id, file_id)? {
            return Err(SyncError::NotInAlbum { album_id, file_id });
        }
        let json = self.albums_json_path();
        let tx = self.conn_mut().transaction()?;
        tx.execute(
            "UPDATE albums SET cover_path = (SELECT path FROM files WHERE id = ?1) WHERE id = ?2",
            [file_id, album_id],
        )?;
        persist_definitions(&tx, &json)?;
        tx.commit()?;
        self.album(album_id)?.ok_or(SyncError::UnknownAlbum(album_id))
    }

    /// Every album, newest date first (album-list UI, with counts).
    pub fn list_albums(&self) -> Result<Vec<AlbumInfo>> {
        let cols = album_cols(self.not_hidden());
        let mut stmt = self.conn().prepare_cached(&format!(
            "SELECT {cols} FROM albums a
             ORDER BY a.date DESC, a.name COLLATE NOCASE, a.id"
        ))?;
        let rows = stmt.query_map([], album_row)?;
        let albums: Vec<AlbumInfo> = rows.collect::<std::result::Result<_, _>>()?;
        albums.into_iter().map(|a| self.with_cover_hash(a)).collect()
    }

    pub fn album(&self, album_id: i64) -> Result<Option<AlbumInfo>> {
        let cols = album_cols(self.not_hidden());
        let mut stmt =
            self.conn().prepare_cached(&format!("SELECT {cols} FROM albums a WHERE a.id = ?1"))?;
        let album = stmt.query_row([album_id], album_row).optional()?;
        album.map(|a| self.with_cover_hash(a)).transpose()
    }

    fn with_cover_hash(&self, mut album: AlbumInfo) -> Result<AlbumInfo> {
        if let Some(cover) = album.cover_file_id {
            album.cover_hash = self
                .conn()
                .query_row("SELECT hash FROM files WHERE id = ?1", [cover], |r| r.get(0))
                .optional()?
                .flatten();
        }
        Ok(album)
    }

    /// Member file ids in the order they were added.
    pub fn album_file_ids(&self, album_id: i64) -> Result<Vec<i64>> {
        let mut stmt = self.conn().prepare_cached(
            "SELECT file_id FROM album_files WHERE album_id = ?1 ORDER BY position, file_id",
        )?;
        let rows = stmt.query_map([album_id], |r| r.get(0))?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }

    /// Albums a photo belongs to, in album-list order.
    pub fn file_album_ids(&self, file_id: i64) -> Result<Vec<i64>> {
        let mut stmt = self.conn().prepare_cached(
            "SELECT a.id FROM albums a JOIN album_files af ON af.album_id = a.id
             WHERE af.file_id = ?1 ORDER BY a.date DESC, a.name COLLATE NOCASE, a.id",
        )?;
        let rows = stmt.query_map([file_id], |r| r.get(0))?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }

    /// The album's photos as grid items, in the order they were added.
    pub fn list_album_files(&self, album_id: i64, limit: usize) -> Result<Vec<FileInfo>> {
        let mut stmt = self.conn().prepare_cached(&format!(
            "SELECT {FILE_INFO_COLS} FROM album_files af JOIN files f ON f.id = af.file_id
             WHERE af.album_id = ?1{} ORDER BY af.position, f.path LIMIT ?2",
            self.not_hidden()
        ))?;
        let rows = stmt.query_map(params![album_id, limit as i64], file_info_row)?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }

    /// Live search restricted to one album's photos: the same matching rules
    /// as the library search (filename, folder, caption), best match first.
    pub fn search_album_files(
        &self,
        album_id: i64,
        query: &str,
        limit: usize,
    ) -> Result<Vec<FileInfo>> {
        let expr = fts_match_expr(query);
        if expr.is_empty() {
            return Ok(Vec::new());
        }
        let mut stmt = self.conn().prepare_cached(&format!(
            "SELECT {FILE_INFO_COLS} FROM files_fts
             JOIN files f ON f.id = files_fts.rowid
             WHERE files_fts MATCH ?1
               AND f.id IN (SELECT file_id FROM album_files WHERE album_id = ?2){}
             ORDER BY rank LIMIT ?3",
            self.not_hidden()
        ))?;
        let rows = stmt.query_map(params![expr, album_id, limit as i64], file_info_row)?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }

    fn is_member(&self, album_id: i64, file_id: i64) -> Result<bool> {
        Ok(self
            .conn()
            .query_row(
                "SELECT 1 FROM album_files WHERE album_id = ?1 AND file_id = ?2",
                [album_id, file_id],
                |_| Ok(()),
            )
            .optional()?
            .is_some())
    }

    /// Bring the album cache in line with `albums.json` (called on open):
    /// the document is the source of truth for definitions, so a catalog
    /// deleted and recreated gets its albums back. A missing document is
    /// written from the cache; an unreadable one is left alone.
    pub(crate) fn restore_album_definitions(&mut self) -> Result<()> {
        let json = self.albums_json_path();
        match read_doc(&json) {
            DocState::Invalid => Ok(()),
            DocState::Missing => {
                let has_albums: bool =
                    self.conn()
                        .query_row("SELECT EXISTS(SELECT 1 FROM albums)", [], |r| r.get(0))?;
                if has_albums {
                    persist_definitions(self.conn(), &json)?;
                }
                Ok(())
            }
            DocState::Ok(doc) => {
                let tx = self.conn_mut().transaction()?;
                for def in &doc.albums {
                    tx.execute(
                        "INSERT INTO albums (uuid, name, date, place, description, cover_path, created_at)
                         VALUES (?1, ?2, ?3, ?4, ?5, ?6, COALESCE(?7, datetime('now')))
                         ON CONFLICT(uuid) DO UPDATE SET
                            name = excluded.name, date = excluded.date, place = excluded.place,
                            description = excluded.description, cover_path = excluded.cover_path",
                        params![
                            def.id,
                            def.name,
                            def.date,
                            def.place,
                            def.description,
                            def.cover,
                            def.created_at
                        ],
                    )?;
                }
                let known: Vec<String> = doc.albums.iter().map(|d| d.id.clone()).collect();
                let stale: Vec<String> = {
                    let mut stmt = tx.prepare("SELECT uuid FROM albums WHERE uuid IS NOT NULL")?;
                    let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
                    rows.collect::<std::result::Result<Vec<_>, _>>()?
                };
                for uuid in stale.iter().filter(|u| !known.contains(u)) {
                    tx.execute("DELETE FROM albums WHERE uuid = ?1", [uuid])?;
                }
                tx.commit()?;
                Ok(())
            }
        }
    }

    /// Re-persist the definitions after the mirror created placeholders.
    pub(crate) fn persist_album_definitions(&self) -> Result<()> {
        persist_definitions(self.conn(), &self.albums_json_path())
    }
}
