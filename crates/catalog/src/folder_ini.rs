//! Folder ini ingest (TASK-03-022, PLAN §1 onboarding, §5 mapping).
//!
//! A folder's ini holds the user's stars, albums, keywords
//! and named faces from an earlier organizer. Ingest turns them into standard XMP through the normal
//! write-through path (sidecar first, catalog mirror second, 00-010 §3):
//!
//! - `star=yes` → `xmp:Rating` 5 (ADR-0002), only where the photo has no
//!   rating yet;
//! - `keywords` → `dc:subject` (unioned), `caption` → `dc:description`
//!   (only where empty);
//! - album tokens → `fb:Albums`, with the album itself created under an id
//!   derived from the token, so a rerun (or a rebuilt catalog) finds the
//!   same album instead of a duplicate;
//! - named faces → MWG `mwg-rs:Regions`; faces never named are
//!   unconfirmed detections, not user data, and are skipped.
//!
//! Existing user data always wins and nothing is rewritten when nothing
//! would change. The ini is only ever read.
//!
//! Which folders are *offered* the migration is catalog state
//! (`folder_ini_ingest`): the scanner records a folder `pending` when it first
//! sees the ini, and the user answers by ingesting (`done`) or declining.

use std::collections::HashMap;
use std::path::Path;

use rusqlite::{params, OptionalExtension};
use serde::Serialize;
use uuid::Uuid;

use fotobase_metadata::{
    folder_ini_files, FaceRegion, FolderIni, FolderIniAlbum, FolderIniPhoto, Sidecar, UNNAMED_ALBUM,
};

use crate::sync::{sidecar_path, SyncError, SyncResult};
use crate::{Catalog, Result};

/// Fixed namespace for deriving album ids from ini tokens.
const TOKEN_NAMESPACE: Uuid = Uuid::from_u128(0x3b8f_7c1e_5a42_4d6b_9e0f_a1c2_d3e4_f506);

/// Where a folder stands with the migration offer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum FolderIniStatus {
    /// Ini seen by a scan; the user has not answered.
    Pending,
    /// The user said no; scans never offer it again.
    Declined,
    /// Migrated.
    Done,
}

impl FolderIniStatus {
    fn as_str(self) -> &'static str {
        match self {
            FolderIniStatus::Pending => "pending",
            FolderIniStatus::Declined => "declined",
            FolderIniStatus::Done => "done",
        }
    }

    fn from_text(text: &str) -> Option<Self> {
        match text {
            "pending" => Some(FolderIniStatus::Pending),
            "declined" => Some(FolderIniStatus::Declined),
            "done" => Some(FolderIniStatus::Done),
            _ => None,
        }
    }
}

/// What one ingest run did, for the post-migration summary.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize)]
pub struct FolderIniReport {
    /// Ini files read in the folder.
    pub ini_files: usize,
    /// Photo sections that matched a catalogued photo.
    pub matched: usize,
    /// Sidecars written.
    pub written: usize,
    /// Matched photos whose sidecar already held everything.
    pub unchanged: usize,
    /// Photo sections with no catalogued photo (not scanned, or gone).
    pub unmatched: usize,
    /// Ratings set from a star.
    pub stars: usize,
    /// Face regions added.
    pub faces: usize,
    /// Faces left out: never named, or an existing `Regions`
    /// structure we will not rewrite.
    pub faces_skipped: usize,
    /// Albums created (including placeholders for undefined tokens).
    pub albums_created: usize,
    /// Photos (or ini files) that could not be processed; the run stays
    /// open for a retry while this is not empty.
    pub failed: Vec<String>,
}

/// The album id an ini token migrates to.
fn token_uuid(token: &str) -> String {
    Uuid::new_v5(&TOKEN_NAMESPACE, token.trim().to_lowercase().as_bytes()).to_string()
}

struct Applied {
    changed: bool,
    star: bool,
    faces_added: usize,
    faces_skipped: usize,
}

/// Merge one photo section into its sidecar. Pure over the sidecar, so a
/// dry run on a copy tells whether a write is needed at all.
fn apply_photo(
    sc: &mut Sidecar,
    photo: &FolderIniPhoto,
    album_uuids: &[String],
    faces: &[FaceRegion],
    dims: &dyn Fn() -> Option<(u32, u32)>,
) -> Applied {
    let mut out = Applied { changed: false, star: false, faces_added: 0, faces_skipped: 0 };

    if photo.star && sc.rating().unwrap_or(0) == 0 {
        sc.set_rating(5);
        out.star = true;
        out.changed = true;
    }

    let mut tags = sc.tags();
    let before = tags.len();
    for keyword in &photo.keywords {
        if !tags.contains(keyword) {
            tags.push(keyword.clone());
        }
    }
    if tags.len() != before {
        sc.set_tags(tags);
        out.changed = true;
    }

    if let Some(caption) = &photo.caption {
        if sc.caption().is_none_or(|c| c.trim().is_empty()) {
            sc.set_caption(caption);
            out.changed = true;
        }
    }

    let mut albums = sc.albums();
    let before = albums.len();
    for uuid in album_uuids {
        if !albums.contains(uuid) {
            albums.push(uuid.clone());
        }
    }
    if albums.len() != before {
        sc.set_albums(albums);
        out.changed = true;
    }

    if !faces.is_empty() {
        // Only look at the file's size if a region will actually be written.
        let known = sc.face_regions();
        let all_known = faces.iter().all(|f| known.iter().any(|k| k.same_face(f)));
        let size = if all_known { None } else { dims() };
        match sc.add_face_regions(faces, size) {
            Some(n) => {
                out.faces_added = n;
                out.changed |= n > 0;
            }
            None => out.faces_skipped = faces.len(),
        }
    }
    out
}

impl Catalog {
    /// Where a folder stands with the migration offer, if it has been seen.
    pub fn folder_ini_status(&self, dir: &str) -> Result<Option<FolderIniStatus>> {
        let status: Option<String> = self
            .conn()
            .query_row("SELECT status FROM folder_ini_ingest WHERE folder_path = ?1", [dir], |r| {
                r.get(0)
            })
            .optional()?;
        Ok(status.as_deref().and_then(FolderIniStatus::from_text))
    }

    /// Record that a scan met this folder's ini. Only a folder never seen
    /// before becomes `pending`; a declined or migrated folder is never
    /// re-offered. Returns whether the offer is new.
    pub fn offer_folder_ini_ingest(&mut self, dir: &str) -> Result<bool> {
        let n = self.conn().execute(
            "INSERT OR IGNORE INTO folder_ini_ingest (folder_path, status) VALUES (?1, 'pending')",
            [dir],
        )?;
        Ok(n == 1)
    }

    /// The user declined migrating this folder; scans stop offering it.
    pub fn decline_folder_ini_ingest(&mut self, dir: &str) -> Result<()> {
        self.set_folder_ini_status(dir, FolderIniStatus::Declined)
    }

    /// Folders with an ini whose offer is still unanswered, path-ordered.
    pub fn folder_ini_pending_folders(&self) -> Result<Vec<String>> {
        let mut stmt = self.conn().prepare_cached(
            "SELECT folder_path FROM folder_ini_ingest WHERE status = 'pending' ORDER BY folder_path",
        )?;
        let rows = stmt.query_map([], |r| r.get(0))?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }

    fn set_folder_ini_status(&mut self, dir: &str, status: FolderIniStatus) -> Result<()> {
        self.conn().execute(
            "INSERT INTO folder_ini_ingest (folder_path, status) VALUES (?1, ?2)
             ON CONFLICT(folder_path) DO UPDATE
                SET status = excluded.status, updated_at = datetime('now')",
            params![dir, status.as_str()],
        )?;
        Ok(())
    }

    /// Migrate one folder's ini file(s) into sidecars (see
    /// the module docs). The folder's photos must already be catalogued
    /// (scanned); sections for photos that are not are counted, not
    /// created. Safe to rerun: nothing changes when nothing is new. A
    /// photo that fails (unreadable sidecar) is reported and skipped; the
    /// folder is marked `done` only when every photo went through.
    pub fn ingest_folder_ini(&mut self, dir: &str) -> SyncResult<FolderIniReport> {
        let folder_id: i64 = self
            .conn()
            .query_row("SELECT id FROM folders WHERE path = ?1", [dir], |r| r.get(0))
            .optional()?
            .ok_or_else(|| SyncError::UnknownFolder(dir.to_string()))?;

        let mut report = FolderIniReport::default();
        let mut inis: Vec<FolderIni> = Vec::new();
        for path in folder_ini_files(Path::new(dir)) {
            report.ini_files += 1;
            match std::fs::read(&path) {
                Ok(bytes) => inis.push(FolderIni::parse(&String::from_utf8_lossy(&bytes))),
                Err(err) => {
                    tracing::warn!(path = %path.display(), error = %err, "cannot read folder ini");
                    report.failed.push(path.file_name().map_or_else(
                        || "folder.ini".to_string(),
                        |n| n.to_string_lossy().into_owned(),
                    ));
                }
            }
        }
        if report.ini_files == 0 {
            return Ok(report);
        }

        // Album definitions first, so memberships mirror into real albums.
        let mut defined: HashMap<&str, &FolderIniAlbum> = HashMap::new();
        for album in inis.iter().flat_map(|i| &i.albums) {
            defined.insert(album.token.as_str(), album);
        }
        for album in defined.values() {
            let created = self.ensure_album(
                &token_uuid(&album.token),
                &album.name,
                album.date.as_deref(),
                album.place.as_deref(),
                album.description.as_deref(),
            )?;
            report.albums_created += usize::from(created);
        }

        let files = self.folder_files(folder_id)?;
        let exact: HashMap<&str, &FolderFile> =
            files.iter().map(|f| (f.filename.as_str(), f)).collect();
        let mut lower: HashMap<String, &FolderFile> = HashMap::new();
        for f in &files {
            lower.entry(f.filename.to_lowercase()).or_insert(f);
        }

        for photo in inis.iter().flat_map(|i| &i.photos) {
            let Some(file) = exact
                .get(photo.filename.as_str())
                .or_else(|| lower.get(&photo.filename.to_lowercase()))
            else {
                report.unmatched += 1;
                continue;
            };
            report.matched += 1;

            let mut album_uuids = Vec::new();
            for token in &photo.albums {
                let uuid = token_uuid(token);
                if !defined.contains_key(token.as_str()) {
                    let created = self.ensure_album(&uuid, UNNAMED_ALBUM, None, None, None)?;
                    report.albums_created += usize::from(created);
                }
                album_uuids.push(uuid);
            }
            let faces: Vec<FaceRegion> = photo
                .faces
                .iter()
                .filter_map(|f| f.name.as_deref().map(|name| f.rect.to_region(name)))
                .collect();
            report.faces_skipped += photo.faces.len() - faces.len();

            let dims = || match (file.width, file.height) {
                (Some(w), Some(h)) if w > 0 && h > 0 => Some((w as u32, h as u32)),
                _ => imagesize::size(&file.path).ok().map(|s| (s.width as u32, s.height as u32)),
            };

            let sc_path = sidecar_path(&file.path);
            let mut probe = match Sidecar::read(&sc_path) {
                Ok(sc) => sc,
                Err(err) => {
                    tracing::warn!(photo = %file.path, error = %err, "folder ini ingest: unreadable sidecar");
                    report.failed.push(photo.filename.clone());
                    continue;
                }
            };
            let planned = apply_photo(&mut probe, photo, &album_uuids, &faces, &dims);
            report.faces_skipped += planned.faces_skipped;
            if !planned.changed {
                report.unchanged += 1;
                continue;
            }
            let write = self.mutate_user_data(file.id, |sc| {
                apply_photo(sc, photo, &album_uuids, &faces, &dims);
            });
            match write {
                Ok(()) => {
                    report.written += 1;
                    report.stars += usize::from(planned.star);
                    report.faces += planned.faces_added;
                }
                Err(err) => {
                    tracing::warn!(photo = %file.path, error = %err, "folder ini ingest: write failed");
                    report.failed.push(photo.filename.clone());
                }
            }
        }

        if report.failed.is_empty() {
            self.set_folder_ini_status(dir, FolderIniStatus::Done)?;
        }
        Ok(report)
    }

    fn folder_files(&self, folder_id: i64) -> Result<Vec<FolderFile>> {
        let mut stmt = self
            .conn()
            .prepare("SELECT id, filename, path, width, height FROM files WHERE folder_id = ?1")?;
        let rows = stmt.query_map([folder_id], |r| {
            Ok(FolderFile {
                id: r.get(0)?,
                filename: r.get(1)?,
                path: r.get(2)?,
                width: r.get(3)?,
                height: r.get(4)?,
            })
        })?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }
}

struct FolderFile {
    id: i64,
    filename: String,
    path: String,
    width: Option<i64>,
    height: Option<i64>,
}
