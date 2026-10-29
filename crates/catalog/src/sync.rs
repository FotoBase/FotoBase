//! Sidecar sync engine (TASK-03-002, PLAN §5): catalog ↔ sidecar authority.
//!
//! Single mutation pathway for user data: every mutation writes the sidecar
//! first (merge-on-write per 00-010 §3), then updates the catalog row inside
//! the same logical transaction. On sidecar-write failure the mutation fails
//! loudly and the catalog is unchanged (no silent divergence). On read,
//! a sidecar whose (mtime, size) changed since the last sync wins over the
//! catalog — the catalog is a rebuildable cache.

use std::path::PathBuf;

use rusqlite::OptionalExtension;

use fotobase_metadata::{FolderMetaError, Sidecar, SidecarError, StackMembership};

use crate::{Catalog, CatalogError, Result};

#[derive(Debug, thiserror::Error)]
pub enum SyncError {
    #[error("no catalog file with id {0}")]
    UnknownFile(i64),
    #[error("rating {0} out of range 0-5")]
    InvalidRating(u8),
    #[error("tag name is empty")]
    EmptyTag,
    #[error("album name is empty")]
    EmptyAlbumName,
    #[error("album date {0:?} is not a YYYY-MM-DD date")]
    InvalidAlbumDate(String),
    #[error("no album with id {0}")]
    UnknownAlbum(i64),
    #[error("file {file_id} is not in album {album_id}")]
    NotInAlbum { album_id: i64, file_id: i64 },
    #[error("no stack with id {0}")]
    UnknownStack(String),
    #[error("a stack needs at least two different photos")]
    StackTooSmall,
    #[error("the photos already belong to different stacks; unstack one first")]
    StackConflict,
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Sidecar(#[from] SidecarError),
    #[error(transparent)]
    Adjust(#[from] crate::datetime::AdjustError),
    #[error(transparent)]
    FolderMeta(#[from] FolderMetaError),
    #[error("no trash entry with id {0}")]
    UnknownTrashEntry(i64),
    #[error("no folder {0} in the catalog")]
    UnknownFolder(String),
    #[error("malformed trash payload: {0}")]
    TrashPayload(String),
    #[error(transparent)]
    Catalog(#[from] CatalogError),
    #[error(transparent)]
    Sql(#[from] rusqlite::Error),
}

pub type SyncResult<T> = std::result::Result<T, SyncError>;

/// Result of a rebuild pass over the catalog files.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SyncStats {
    pub checked: usize,
    pub converged: usize,
    pub skipped: usize,
}

/// Sidecar path for an original: `IMG_1234.jpg` → `IMG_1234.jpg.xmp` (§5).
pub fn sidecar_path(original: &str) -> PathBuf {
    PathBuf::from(format!("{original}.xmp"))
}

#[derive(Debug, Clone, PartialEq)]
struct UserData {
    rating: Option<i64>,
    caption: Option<String>,
    tags: Vec<String>,
    gps: Option<(f64, f64)>,
    /// `dc:creator` / `dc:rights` (TASK-03-020).
    artist: Option<String>,
    copyright: Option<String>,
    /// `fb:Albums` ids (TASK-03-008).
    albums: Vec<String>,
    /// `fb:Hidden` (TASK-03-009).
    hidden: bool,
    /// `exif:DateTimeOriginal` as ISO text (TASK-03-019); `None` leaves the
    /// catalog's scan-time value alone.
    taken_at: Option<String>,
    /// `fb:StackId` + `fb:StackRole` (TASK-03-021).
    stack: Option<StackMembership>,
    /// `fb:Recipe` present: an edited version (TASK-03-021).
    has_recipe: bool,
}

impl UserData {
    fn from_sidecar(sc: &Sidecar) -> Self {
        UserData {
            // The catalog column is CHECK 0..=5; other tools write -1
            // (rejected) or junk, which must not abort convergence.
            rating: sc.rating().filter(|r| *r <= 5).map(i64::from),
            caption: sc.caption(),
            tags: sc.tags(),
            gps: sc.gps(),
            artist: sc.artist(),
            copyright: sc.copyright(),
            albums: sc.albums(),
            hidden: sc.hidden(),
            taken_at: sc.capture_time().map(|t| t.to_xmp()),
            stack: sc.stack(),
            has_recipe: sc.has_recipe(),
        }
    }
}

impl Catalog {
    // ---- write-through mutations (single pathway, 00-010 §3.6) -----------

    pub fn set_rating(&mut self, file_id: i64, rating: u8) -> SyncResult<()> {
        if rating > 5 {
            return Err(SyncError::InvalidRating(rating));
        }
        self.mutate_user_data(file_id, |sc| sc.set_rating(rating))
    }

    /// Unstar: drop `xmp:Rating` from the sidecar and clear the mirror.
    pub fn clear_rating(&mut self, file_id: i64) -> SyncResult<()> {
        self.mutate_user_data(file_id, |sc| sc.clear_rating())
    }

    /// Write the caption to `dc:description` (TASK-03-007). Blank input
    /// clears the field, so an emptied editor never leaves an empty
    /// `dc:description` behind.
    pub fn set_caption(&mut self, file_id: i64, caption: &str) -> SyncResult<()> {
        if caption.trim().is_empty() {
            return self.clear_caption(file_id);
        }
        self.mutate_user_data(file_id, |sc| sc.set_caption(caption))
    }

    /// Remove `dc:description` from the sidecar and clear the mirror + FTS.
    pub fn clear_caption(&mut self, file_id: i64) -> SyncResult<()> {
        self.mutate_user_data(file_id, |sc| sc.clear_caption())
    }

    /// A photo's caption from the catalog mirror (derived cache, PLAN §5).
    pub fn file_caption(&self, file_id: i64) -> Result<Option<String>> {
        let caption = self
            .conn()
            .query_row("SELECT caption FROM metadata WHERE file_id = ?1", [file_id], |r| r.get(0))
            .optional()?;
        Ok(caption.flatten())
    }

    pub fn set_tags(&mut self, file_id: i64, tags: Vec<String>) -> SyncResult<()> {
        self.mutate_user_data(file_id, |sc| sc.set_tags(tags))
    }

    // ---- tags (TASK-03-006, PLAN §5) ------------------------------------

    /// Add one tag: write-through to `dc:subject`; a tag containing `|`
    /// (nested, e.g. `Vacation|2024`) is also written to
    /// `lr:hierarchicalSubject` so darktable sees the nesting. Returns the
    /// photo's tag list after the write.
    pub fn add_tag(&mut self, file_id: i64, tag: &str) -> SyncResult<Vec<String>> {
        let tag = tag.trim().to_string();
        if tag.is_empty() {
            return Err(SyncError::EmptyTag);
        }
        let tags = self.mutate_tags(file_id, |sc| {
            let mut tags = sc.tags();
            if !tags.contains(&tag) {
                tags.push(tag.clone());
            }
            sc.set_tags(tags.clone());
            if tag.contains('|') {
                let mut hier = sc.hierarchical_tags();
                if !hier.contains(&tag) {
                    hier.push(tag.clone());
                }
                sc.set_hierarchical_tags(hier);
            }
            tags
        })?;
        Ok(tags)
    }

    /// Remove one tag from `dc:subject` (and the matching
    /// `lr:hierarchicalSubject` entry). Returns the remaining tags.
    pub fn remove_tag(&mut self, file_id: i64, tag: &str) -> SyncResult<Vec<String>> {
        let tag = tag.trim().to_string();
        let tags = self.mutate_tags(file_id, |sc| {
            let tags: Vec<String> = sc.tags().into_iter().filter(|t| *t != tag).collect();
            sc.set_tags(tags.clone());
            let hier: Vec<String> =
                sc.hierarchical_tags().into_iter().filter(|t| *t != tag).collect();
            sc.set_hierarchical_tags(hier);
            tags
        })?;
        Ok(tags)
    }

    /// Rename a tag across the whole library (functional spec: tag rename
    /// rewrites affected sidecars as a batch job). For each affected photo
    /// the sidecar's `dc:subject` entry is replaced exactly and each
    /// `lr:hierarchicalSubject` path segment is renamed, so `Vacation|2024`
    /// follows a `Vacation` → `Trips` rename. Returns the number of
    /// sidecars rewritten.
    pub fn rename_tag(&mut self, old: &str, new: &str) -> SyncResult<usize> {
        let (old, new) = (old.trim().to_string(), new.trim().to_string());
        if old.is_empty() || new.is_empty() {
            return Err(SyncError::EmptyTag);
        }
        let pattern: Vec<String> =
            vec![format!("%|{old}|%"), format!("{old}|%"), format!("%|{old}")];
        let ids: Vec<i64> = {
            let mut stmt = self.conn().prepare_cached(
                "SELECT DISTINCT ft.file_id FROM file_tags ft JOIN tags t ON t.id = ft.tag_id
                 WHERE t.name = ?1 OR t.name LIKE ?2 OR t.name LIKE ?3 OR t.name LIKE ?4
                 ORDER BY ft.file_id",
            )?;
            let rows = stmt
                .query_map(rusqlite::params![old, pattern[0], pattern[1], pattern[2]], |r| {
                    r.get(0)
                })?;
            rows.flatten().collect()
        };
        for id in &ids {
            self.mutate_tags(*id, |sc| {
                let rename_item = |t: String| {
                    t.split('|')
                        .map(|seg| if seg == old { new.clone() } else { seg.to_string() })
                        .collect::<Vec<_>>()
                        .join("|")
                };
                let tags: Vec<String> = sc.tags().into_iter().map(rename_item).collect();
                sc.set_tags(tags);
                let hier: Vec<String> =
                    sc.hierarchical_tags().into_iter().map(rename_item).collect();
                sc.set_hierarchical_tags(hier);
                sc.tags()
            })?;
        }
        // The derived cache: stale name rows drop out with their last refs.
        if !ids.is_empty() {
            self.conn_mut()
                .execute("DELETE FROM tags WHERE id NOT IN (SELECT tag_id FROM file_tags)", [])?;
        }
        Ok(ids.len())
    }

    /// A photo's tags from the catalog mirror (derived cache, PLAN §5).
    pub fn file_tags(&self, file_id: i64) -> Result<Vec<String>> {
        let mut stmt = self.conn().prepare_cached(
            "SELECT t.name FROM tags t JOIN file_tags ft ON ft.tag_id = t.id
             WHERE ft.file_id = ?1 ORDER BY t.name",
        )?;
        let rows = stmt.query_map([file_id], |r| r.get(0))?;
        Ok(rows.flatten().collect())
    }

    /// Every tag in the library with its usage count, name-ordered — the
    /// rename dialog's source list.
    pub fn all_tags(&self) -> Result<Vec<(String, i64)>> {
        let mut stmt = self.conn().prepare_cached(
            "SELECT t.name, COUNT(ft.file_id) FROM tags t
             LEFT JOIN file_tags ft ON ft.tag_id = t.id
             GROUP BY t.id HAVING COUNT(ft.file_id) > 0 ORDER BY t.name",
        )?;
        let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
        Ok(rows.flatten().collect())
    }

    /// Read-modify-write the sidecar's tags via the single mutation pathway;
    /// the catalog mirror is updated from the written sidecar.
    fn mutate_tags(
        &mut self,
        file_id: i64,
        apply: impl FnOnce(&mut Sidecar) -> Vec<String>,
    ) -> SyncResult<Vec<String>> {
        let mut tags = Vec::new();
        self.mutate_user_data(file_id, |sc| {
            tags = apply(sc);
        })?;
        Ok(tags)
    }

    pub fn set_gps(&mut self, file_id: i64, lat: f64, lon: f64) -> SyncResult<()> {
        self.mutate_user_data(file_id, |sc| sc.set_gps(lat, lon))
    }

    /// Write-through: sidecar write succeeds first, catalog update second,
    /// both inside one logical transaction. Sidecar failure → error, catalog
    /// untouched.
    pub(crate) fn mutate_user_data(
        &mut self,
        file_id: i64,
        apply: impl FnOnce(&mut Sidecar),
    ) -> SyncResult<()> {
        self.try_mutate_user_data(file_id, |sc| {
            apply(sc);
            Ok(())
        })
    }

    /// [`Self::mutate_user_data`] for edits that can be refused: an `Err`
    /// from `apply` leaves the sidecar and the catalog untouched.
    pub(crate) fn try_mutate_user_data(
        &mut self,
        file_id: i64,
        apply: impl FnOnce(&mut Sidecar) -> SyncResult<()>,
    ) -> SyncResult<()> {
        let tx = self.conn_mut().transaction()?;
        let path: String = tx
            .query_row("SELECT path FROM files WHERE id = ?1", [file_id], |r| r.get(0))
            .optional()?
            .ok_or(SyncError::UnknownFile(file_id))?;
        let sc_path = sidecar_path(&path);

        let mut sc = Sidecar::read(&sc_path)?;
        apply(&mut sc)?;
        sc.write(&sc_path)?;

        let data = UserData::from_sidecar(&sc);
        let placeholders = apply_user_data(&tx, file_id, &data)?;
        record_sidecar_stat(&tx, file_id, &sc_path)?;
        tx.commit()?;
        if placeholders {
            self.persist_album_definitions()?;
        }
        Ok(())
    }

    // ---- read-side convergence (sidecar wins, §5 conflict rule) ----------

    /// Re-read one file's sidecar if it changed since the last sync
    /// (mtime+size probe) and overwrite the catalog user data.
    /// Returns `true` when the catalog was converged from the sidecar.
    pub fn sync_file(&mut self, file_id: i64) -> SyncResult<bool> {
        let (path, stored_mtime, stored_size): (String, Option<i64>, Option<i64>) = self
            .conn()
            .query_row(
                "SELECT path, sidecar_mtime, sidecar_size FROM files WHERE id = ?1",
                [file_id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()?
            .ok_or(SyncError::UnknownFile(file_id))?;
        self.converge(file_id, &path, stored_mtime, stored_size)
    }

    /// Rebuild user data from sidecars (§5). Incremental by default:
    /// files whose sidecar (mtime, size) is unchanged since the last sync
    /// are skipped. `full = true` forces a re-read of every sidecar.
    pub fn rebuild_from_sidecars(&mut self, full: bool) -> SyncResult<SyncStats> {
        let mut stats = SyncStats::default();
        // Folder-level hide lives in `.fotobase`, not in any sidecar (ADR-0007).
        self.sync_all_folder_flags()?;
        let files: Vec<(i64, String, Option<i64>, Option<i64>)> = {
            let mut stmt =
                self.conn().prepare("SELECT id, path, sidecar_mtime, sidecar_size FROM files")?;
            let rows = stmt
                .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?
                .collect::<std::result::Result<Vec<_>, _>>()?;
            rows
        };
        for (file_id, path, mtime, size) in files {
            stats.checked += 1;
            if let (false, Some(sz), Some(mt), Ok(md)) =
                (full, size, mtime, std::fs::metadata(sidecar_path(&path)))
            {
                if sidecar_unchanged(&md, sz, mt) {
                    stats.skipped += 1;
                    continue;
                }
            }
            if self.converge_forced(file_id, &path, mtime, size, full)? {
                stats.converged += 1;
            }
        }
        Ok(stats)
    }

    /// Scan hook: converge the sidecars of just-scanned files so user data
    /// survives a catalog delete + re-scan (§5 rebuild). Unlike
    /// [`Self::rebuild_from_sidecars`] a bad sidecar is logged and skipped —
    /// one malformed XMP must never fail a library scan.
    pub fn sync_paths<'a>(&mut self, paths: impl IntoIterator<Item = &'a str>) -> SyncStats {
        let mut stats = SyncStats::default();
        for path in paths {
            stats.checked += 1;
            let row = self
                .conn()
                .query_row(
                    "SELECT id, sidecar_mtime, sidecar_size FROM files WHERE path = ?1",
                    [path],
                    |r| Ok((r.get::<_, i64>(0)?, r.get(1)?, r.get(2)?)),
                )
                .optional();
            let Ok(Some((id, mtime, size))) = row else {
                stats.skipped += 1;
                continue;
            };
            match self.converge(id, path, mtime, size) {
                Ok(true) => stats.converged += 1,
                Ok(false) => stats.skipped += 1,
                Err(err) => {
                    tracing::warn!(path, error = %err, "sidecar sync failed; keeping catalog row");
                    stats.skipped += 1;
                }
            }
        }
        stats
    }
}

impl Catalog {
    /// Converge one file: if the sidecar exists and differs from the recorded
    /// sync state, apply its user data to the catalog and record the new
    /// sync state. Missing sidecar → `Ok(false)`, catalog left as-is.
    fn converge(
        &mut self,
        file_id: i64,
        path: &str,
        stored_mtime: Option<i64>,
        stored_size: Option<i64>,
    ) -> SyncResult<bool> {
        self.converge_forced(file_id, path, stored_mtime, stored_size, false)
    }

    fn converge_forced(
        &mut self,
        file_id: i64,
        path: &str,
        stored_mtime: Option<i64>,
        stored_size: Option<i64>,
        force: bool,
    ) -> SyncResult<bool> {
        let sc_path = sidecar_path(path);
        let md = match std::fs::metadata(&sc_path) {
            Ok(md) => md,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(false),
            Err(err) => return Err(err.into()),
        };
        if force || !matches!((stored_mtime, stored_size), (Some(_), Some(_))) {
            // Never synced (or mid-crash state): always re-read.
        } else if sidecar_unchanged(&md, stored_size.unwrap(), stored_mtime.unwrap()) {
            return Ok(false);
        }

        let sc = Sidecar::read(&sc_path)?;
        let data = UserData::from_sidecar(&sc);
        let tx = self.conn_mut().transaction()?;
        let placeholders = apply_user_data(&tx, file_id, &data)?;
        record_sidecar_stat(&tx, file_id, &sc_path)?;
        tx.commit()?;
        if placeholders {
            self.persist_album_definitions()?;
        }
        Ok(true)
    }
}

/// Sidecar (mtime, size) recorded at last sync, in whole nanoseconds since
/// the epoch. Float seconds would round-trip imprecisely and make unchanged
/// sidecars look changed.
fn mtime_nanos(md: &std::fs::Metadata) -> Result<i64> {
    Ok(md
        .modified()?
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(std::io::Error::other)?
        .as_nanos() as i64)
}

/// Cheap (mtime, size) probe: is the sidecar on disk the one we last synced?
fn sidecar_unchanged(md: &std::fs::Metadata, stored_size: i64, stored_mtime_nanos: i64) -> bool {
    md.len() as i64 == stored_size && mtime_nanos(md).ok() == Some(stored_mtime_nanos)
}

/// Mirror one sidecar's user data into the catalog. Returns `true` when an
/// album id without a definition forced a placeholder album (the caller
/// re-persists `albums.json`).
fn apply_user_data(tx: &rusqlite::Transaction<'_>, file_id: i64, data: &UserData) -> Result<bool> {
    tx.execute("INSERT OR IGNORE INTO metadata (file_id) VALUES (?1)", [file_id])?;
    tx.execute(
        "UPDATE metadata SET rating = ?1, caption = ?2, latitude = ?3, longitude = ?4,
                artist = ?5, copyright = ?6
         WHERE file_id = ?7",
        rusqlite::params![
            data.rating,
            data.caption,
            data.gps.map(|(lat, _)| lat),
            data.gps.map(|(_, lon)| lon),
            data.artist,
            data.copyright,
            file_id
        ],
    )?;

    tx.execute(
        "UPDATE files SET hidden = ?1, taken_at = COALESCE(?2, taken_at),
                stack_id = ?3, stack_role = ?4, has_recipe = ?5
         WHERE id = ?6",
        rusqlite::params![
            data.hidden,
            data.taken_at,
            data.stack.as_ref().map(|s| s.id.as_str()),
            data.stack.as_ref().map(|s| s.role.as_str()),
            data.has_recipe,
            file_id
        ],
    )?;

    tx.execute("DELETE FROM file_tags WHERE file_id = ?1", [file_id])?;
    for tag in &data.tags {
        tx.execute("INSERT OR IGNORE INTO tags (name) VALUES (?1)", [tag])?;
        let tag_id: i64 =
            tx.query_row("SELECT id FROM tags WHERE name = ?1", [tag], |r| r.get(0))?;
        tx.execute(
            "INSERT OR IGNORE INTO file_tags (file_id, tag_id) VALUES (?1, ?2)",
            [file_id, tag_id],
        )?;
    }
    crate::albums::apply_membership(tx, file_id, &data.albums)
}

/// Record (mtime, size) of the sidecar we just synced, so later reads can
/// skip unchanged sidecars (incremental rebuild, §5).
fn record_sidecar_stat(
    tx: &rusqlite::Transaction<'_>,
    file_id: i64,
    sc_path: &std::path::Path,
) -> Result<()> {
    let md = std::fs::metadata(sc_path)?;
    let mtime = mtime_nanos(&md)?;
    tx.execute(
        "UPDATE files SET sidecar_mtime = ?1, sidecar_size = ?2 WHERE id = ?3",
        rusqlite::params![mtime, md.len() as i64, file_id],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::SystemTime;

    fn catalog_with_photo(dir: &std::path::Path, name: &str) -> (Catalog, i64, PathBuf, PathBuf) {
        let db = Catalog::open(dir.join("catalog.db")).unwrap();
        let mut cat = db;
        let original = dir.join(name);
        std::fs::write(&original, b"fake image bytes").unwrap();
        let folder = dir.to_str().unwrap().to_string();
        let folder_id = cat.seed_folder(&folder).unwrap();
        let path = original.to_str().unwrap().to_string();
        cat.conn_mut()
            .execute(
                "INSERT INTO files (folder_id, path, filename, size, mtime, media_type)
                 VALUES (?1, ?2, ?3, 10, 0.0, 'image')",
                rusqlite::params![folder_id, path, name],
            )
            .unwrap();
        let file_id = cat.conn_mut().last_insert_rowid();
        let sc = sidecar_path(&path);
        (cat, file_id, original, sc)
    }

    fn rating_in(cat: &Catalog, file_id: i64) -> Option<i64> {
        cat.conn()
            .query_row("SELECT rating FROM metadata WHERE file_id = ?1", [file_id], |r| r.get(0))
            .optional()
            .unwrap()
            .flatten()
    }

    fn set_sidecar_mtime(sc: &std::path::Path, t: SystemTime) {
        let ft = filetime::FileTime::from_system_time(t);
        filetime::set_file_mtime(sc, ft).unwrap();
    }

    // ---- TDD: unit — sidecar wins on conflict ---------------------------

    #[test]
    fn catalog_rating_3_sidecar_rating_5_converges_to_5() {
        let dir = tempfile::tempdir().unwrap();
        let (mut cat, file_id, _orig, sc) = catalog_with_photo(dir.path(), "IMG_0001.jpg");

        // Catalog says 3 (stale cache).
        cat.conn_mut()
            .execute("INSERT INTO metadata (file_id, rating) VALUES (?1, 3)", [file_id])
            .unwrap();
        // Sidecar says 5 (authoritative).
        let mut side = Sidecar::read(&sc).unwrap();
        side.set_rating(5);
        side.write(&sc).unwrap();

        let converged = cat.sync_file(file_id).unwrap();
        assert!(converged);
        assert_eq!(rating_in(&cat, file_id), Some(5));
    }

    #[test]
    fn unchanged_sidecar_is_not_re_read() {
        let dir = tempfile::tempdir().unwrap();
        let (mut cat, file_id, _orig, sc) = catalog_with_photo(dir.path(), "IMG_0002.jpg");
        let mut side = Sidecar::read(&sc).unwrap();
        side.set_rating(4);
        side.write(&sc).unwrap();
        assert!(cat.sync_file(file_id).unwrap());
        assert_eq!(rating_in(&cat, file_id), Some(4));

        // Untouched sidecar: the (mtime, size) probe must skip the re-read.
        assert!(!cat.sync_file(file_id).unwrap());
        assert_eq!(rating_in(&cat, file_id), Some(4));

        // A changed sidecar (mtime AND size differ) is re-read — sidecar wins.
        let mut side2 = Sidecar::read(&sc).unwrap();
        side2.set_rating(1);
        side2.write(&sc).unwrap();
        assert!(cat.sync_file(file_id).unwrap());
        assert_eq!(rating_in(&cat, file_id), Some(1));
    }

    // ---- TDD: integration — write-through -------------------------------

    #[test]
    fn star_photo_writes_sidecar_and_catalog_in_same_operation() {
        let dir = tempfile::tempdir().unwrap();
        let (mut cat, file_id, _orig, sc) = catalog_with_photo(dir.path(), "IMG_0003.jpg");

        let before = std::fs::metadata(&sc).ok().map(|m| m.modified().unwrap());
        cat.set_rating(file_id, 5).unwrap();

        // Sidecar changed in the same operation.
        let after = std::fs::metadata(&sc).unwrap().modified().unwrap();
        assert_ne!(Some(after), before, "sidecar must be written by the mutation");
        let side = Sidecar::read(&sc).unwrap();
        assert_eq!(side.rating(), Some(5));
        // Catalog follows.
        assert_eq!(rating_in(&cat, file_id), Some(5));
    }

    #[test]
    fn write_through_captions_tags_and_gps() {
        let dir = tempfile::tempdir().unwrap();
        let (mut cat, file_id, _orig, sc) = catalog_with_photo(dir.path(), "IMG_0004.jpg");

        cat.set_caption(file_id, "sunset over the bay").unwrap();
        cat.set_tags(file_id, vec!["sea".into(), "trip2026".into()]).unwrap();
        cat.set_gps(file_id, 52.52, 13.405).unwrap();

        let side = Sidecar::read(&sc).unwrap();
        assert_eq!(side.caption().as_deref(), Some("sunset over the bay"));
        assert_eq!(side.tags(), vec!["sea", "trip2026"]);
        let (lat, lon) = side.gps().unwrap();
        assert!((lat - 52.52).abs() < 1e-4, "lat {lat}");
        assert!((lon - 13.405).abs() < 1e-4, "lon {lon}");

        let (c_lat, c_lon): (Option<f64>, Option<f64>) = cat
            .conn()
            .query_row(
                "SELECT latitude, longitude FROM metadata WHERE file_id = ?1",
                [file_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(c_lat.map(|v| (v * 1e4).round() / 1e4), Some(52.52));
        assert_eq!(c_lon.map(|v| (v * 1e4).round() / 1e4), Some(13.405));
        let tags: Vec<String> = cat
            .conn()
            .prepare(
                "SELECT t.name FROM tags t JOIN file_tags ft ON ft.tag_id = t.id
                      WHERE ft.file_id = ?1 ORDER BY t.name",
            )
            .unwrap()
            .query_map([file_id], |r| r.get(0))
            .unwrap()
            .collect::<std::result::Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(tags, vec!["sea", "trip2026"]);
    }

    #[test]
    fn external_digikam_edit_converges_on_next_read() {
        let dir = tempfile::tempdir().unwrap();
        let (mut cat, file_id, _orig, sc) = catalog_with_photo(dir.path(), "IMG_0005.jpg");
        cat.set_rating(file_id, 2).unwrap();

        // "digiKam" edits the sidecar behind our back, keeping its own fields.
        let mut external = Sidecar::read(&sc).unwrap();
        external.set_rating(5);
        external.set_tags(vec!["digiKam-tag".into()]);
        external.write(&sc).unwrap();

        assert!(cat.sync_file(file_id).unwrap());
        assert_eq!(rating_in(&cat, file_id), Some(5));
        let tags: Vec<String> = cat
            .conn()
            .prepare(
                "SELECT t.name FROM tags t JOIN file_tags ft ON ft.tag_id = t.id
                      WHERE ft.file_id = ?1",
            )
            .unwrap()
            .query_map([file_id], |r| r.get(0))
            .unwrap()
            .collect::<std::result::Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(tags, vec!["digiKam-tag"]);
    }

    // ---- TDD: integration — loud failure, catalog unchanged --------------

    #[test]
    fn sidecar_failure_fails_loudly_and_catalog_unchanged() {
        let dir = tempfile::tempdir().unwrap();
        let (mut cat, file_id, _orig, sc) = catalog_with_photo(dir.path(), "IMG_0006.jpg");

        // Portability: make the sidecar path itself a directory — rename over
        // a directory fails on every OS (EISDIR), unlike chmod which is
        // advisory on Windows.
        std::fs::create_dir(&sc).unwrap();

        let err = cat.set_rating(file_id, 5).unwrap_err();
        assert!(matches!(err, SyncError::Sidecar(_)), "got {err:?}");
        assert_eq!(rating_in(&cat, file_id), None, "catalog must be unchanged");
        // No stray tmp file left behind.
        let tmp = fotobase_metadata::tmp_path(&sc);
        assert!(!tmp.exists(), "tmp file must be cleaned up on failure");
    }

    // ---- TDD: integration — rebuild from sidecars ------------------------

    #[test]
    fn rebuild_from_sidecars_restores_user_data() {
        let dir = tempfile::tempdir().unwrap();
        let (mut cat, file_id, _orig, _sc) = catalog_with_photo(dir.path(), "IMG_0007.jpg");
        cat.set_rating(file_id, 3).unwrap();
        cat.set_caption(file_id, "keep me").unwrap();
        cat.set_tags(file_id, vec!["family".into()]).unwrap();

        // Simulate catalog corruption/loss: wipe all user data + sync state.
        cat.conn_mut().execute("UPDATE metadata SET rating = NULL, caption = NULL", []).unwrap();
        cat.conn_mut().execute("DELETE FROM file_tags", []).unwrap();
        cat.conn_mut()
            .execute("UPDATE files SET sidecar_mtime = NULL, sidecar_size = NULL", [])
            .unwrap();

        let stats = cat.rebuild_from_sidecars(false).unwrap();
        assert_eq!(stats.converged, 1);
        assert_eq!(rating_in(&cat, file_id), Some(3));
        let caption: Option<String> = cat
            .conn()
            .query_row("SELECT caption FROM metadata WHERE file_id = ?1", [file_id], |r| r.get(0))
            .unwrap();
        assert_eq!(caption.as_deref(), Some("keep me"));
        let tags: i64 = cat
            .conn()
            .query_row("SELECT COUNT(*) FROM file_tags WHERE file_id = ?1", [file_id], |r| r.get(0))
            .unwrap();
        assert_eq!(tags, 1);
    }

    #[test]
    fn rebuild_is_incremental_by_mtime_and_size() {
        let dir = tempfile::tempdir().unwrap();
        let (mut cat, file_a, _o1, sc_a) = catalog_with_photo(dir.path(), "IMG_0008.jpg");
        let (_cat_b, file_b, _o2, sc_b) = catalog_with_photo(dir.path(), "IMG_0009.jpg");

        cat.set_rating(file_a, 2).unwrap();
        cat.set_rating(file_b, 4).unwrap();

        // Touch only sidecar A afterwards.
        let mut side = Sidecar::read(&sc_a).unwrap();
        side.set_rating(5);
        side.write(&sc_a).unwrap();
        assert!(sc_b.exists());

        let stats = cat.rebuild_from_sidecars(false).unwrap();
        assert_eq!(stats.checked, 2);
        assert_eq!(stats.skipped, 1, "untouched sidecar B must be skipped");
        assert_eq!(stats.converged, 1);
        assert_eq!(rating_in(&cat, file_a), Some(5));
        assert_eq!(rating_in(&cat, file_b), Some(4));
    }

    #[test]
    fn full_rebuild_re_reads_every_sidecar() {
        let dir = tempfile::tempdir().unwrap();
        let (mut cat, file_id, _orig, sc) = catalog_with_photo(dir.path(), "IMG_0010.jpg");
        cat.set_rating(file_id, 2).unwrap();

        let mut side = Sidecar::read(&sc).unwrap();
        side.set_rating(5);
        side.write(&sc).unwrap();
        set_sidecar_mtime(&sc, SystemTime::UNIX_EPOCH); // stale-looking probe

        let stats = cat.rebuild_from_sidecars(true).unwrap();
        assert_eq!(stats.skipped, 0);
        assert_eq!(stats.converged, 1);
        assert_eq!(rating_in(&cat, file_id), Some(5));
    }

    // ---- derived data never written to sidecars --------------------------

    #[test]
    fn derived_data_never_written_to_sidecars() {
        let dir = tempfile::tempdir().unwrap();
        let (mut cat, file_id, _orig, sc) = catalog_with_photo(dir.path(), "IMG_0011.jpg");
        cat.set_rating(file_id, 3).unwrap();

        // Catalog-side derived data changes (hash, FTS) — sidecar untouched.
        cat.set_file_hash(file_id, "abc123").unwrap();
        let stats = cat.rebuild_from_sidecars(false).unwrap();
        assert_eq!(stats.skipped, 1);

        let raw = std::fs::read_to_string(&sc).unwrap();
        assert!(!raw.contains("abc123"), "hash must never reach the sidecar");
        assert!(!raw.contains("thumb"), "no derived data in sidecar");
    }

    #[test]
    fn unknown_file_id_errors() {
        let dir = tempfile::tempdir().unwrap();
        let (mut cat, _id, _o, _sc) = catalog_with_photo(dir.path(), "IMG_0012.jpg");
        let err = cat.set_rating(9999, 5).unwrap_err();
        assert!(matches!(err, SyncError::UnknownFile(9999)));
    }
}
