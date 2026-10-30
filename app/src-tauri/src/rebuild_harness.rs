//! Rebuild acceptance harness (TASK-03-026, PLAN §9 "Rebuildability" row):
//! snapshot user data → delete the catalog → rebuild from sidecars →
//! per-field semantic diff. This is the regression gate for every
//! sidecar-writing feature in EPIC-03: the catalog is a rebuildable cache
//! (§3), so deleting it must lose nothing the sidecars carry.
//!
//! The snapshot is an export of the catalog's user-data tables plus a
//! SHA-256 of each sidecar's bytes; the diff compares semantically (not a
//! byte-diff of the DB) and reports one entry per lost/changed field as
//! machine-readable JSON. AI-derived data (faces, embeddings) is out of
//! scope — PLAN §5 allows recompute. Originals and sidecars are only ever
//! read here, never written.

use std::collections::BTreeMap;
use std::path::Path;

use rusqlite::OptionalExtension;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use fotobase_catalog::{sidecar_path, Catalog, ScanMode};

/// One photo's user data, keyed by its original path in the snapshot.
/// Every field here must survive a catalog rebuild (§9).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FileSnapshot {
    pub rating: Option<i64>,
    pub caption: Option<String>,
    pub tags: Vec<String>,
    pub gps: Option<[f64; 2]>,
    pub artist: Option<String>,
    pub copyright: Option<String>,
    pub hidden: bool,
    pub taken_at: Option<String>,
    pub stack_id: Option<String>,
    pub stack_role: Option<String>,
    /// `fb:Recipe` present: an edited version (TASK-03-021).
    pub has_recipe: bool,
    /// Album UUIDs the photo belongs to, sorted.
    pub albums: Vec<String>,
}

/// One album definition (`albums.json` lives next to the catalog and is
/// re-imported on reopen; the harness diffs it anyway — belt and braces).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AlbumSnapshot {
    pub uuid: String,
    pub name: String,
    pub date: Option<String>,
    pub place: Option<String>,
    pub description: Option<String>,
}

/// A watched folder, restored from the snapshot to drive the headless
/// rebuild. Folders are catalog configuration, not user data, but a wiped
/// DB forgets them, so the harness re-seeds them before rebuilding.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FolderSnapshot {
    pub path: String,
    pub scan_mode: String,
}

/// The pre-delete state: catalog user data plus on-disk sidecar hashes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Snapshot {
    pub folders: Vec<FolderSnapshot>,
    /// Keyed by original path.
    pub files: BTreeMap<String, FileSnapshot>,
    pub albums: Vec<AlbumSnapshot>,
    /// Keyed by sidecar path: SHA-256 of the sidecar's bytes. A rebuild
    /// must never rewrite a sidecar, so any hash change is a regression.
    pub sidecars: BTreeMap<String, Option<String>>,
}

/// One semantic difference between two snapshots. `diff` emits these for
/// per-field losses (user data the rebuild failed to restore) and for
/// sidecar hash mismatches; the report is machine-readable JSON.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FieldDiff {
    /// Original path (sidecar path for `sidecar_sha256`).
    pub path: String,
    pub field: String,
    /// `None` when the value is absent on that side (e.g. the file itself).
    pub before: Option<String>,
    pub after: Option<String>,
}

/// Result of a diff: `pass` is true only when nothing was lost or changed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DiffReport {
    pub diffs: Vec<FieldDiff>,
    pub pass: bool,
}

/// Snapshot the user data of `cat` plus the sidecar bytes on disk.
pub fn snapshot(cat: &Catalog) -> fotobase_catalog::Result<Snapshot> {
    let conn = cat.conn();

    let mut folders = Vec::new();
    let mut stmt = conn.prepare("SELECT path, scan_mode FROM folders ORDER BY path")?;
    let mut rows = stmt.query([])?;
    while let Some(row) = rows.next()? {
        folders.push(FolderSnapshot {
            path: row.get(0)?,
            scan_mode: ScanMode::from_db(&row.get::<_, String>(1)?).as_str().to_owned(),
        });
    }

    let mut files = BTreeMap::new();
    let mut stmt = conn.prepare(
        "SELECT f.path, m.rating, m.caption, m.latitude, m.longitude, m.artist, m.copyright,
                f.hidden, f.taken_at, f.stack_id, f.stack_role, f.has_recipe
         FROM files f LEFT JOIN metadata m ON m.file_id = f.id
         ORDER BY f.path",
    )?;
    let mut rows = stmt.query([])?;
    while let Some(row) = rows.next()? {
        let path: String = row.get(0)?;
        let rating: Option<i64> = row.get(1)?;
        let gps = match (row.get::<_, Option<f64>>(3)?, row.get::<_, Option<f64>>(4)?) {
            (Some(lat), Some(lon)) => Some([lat, lon]),
            _ => None,
        };
        files.insert(
            path.clone(),
            FileSnapshot {
                rating,
                caption: row.get(2)?,
                tags: Vec::new(),
                gps,
                artist: row.get(5)?,
                copyright: row.get(6)?,
                hidden: row.get::<_, i64>(7)? != 0,
                taken_at: row.get(8)?,
                stack_id: row.get(9)?,
                stack_role: row.get(10)?,
                has_recipe: row.get::<_, i64>(11)? != 0,
                albums: Vec::new(),
            },
        );
    }

    // Tags per file (small tables; separate query keeps the row mapping simple).
    let mut stmt = conn.prepare(
        "SELECT f.path, t.name FROM file_tags ft JOIN files f ON f.id = ft.file_id
                      JOIN tags t ON t.id = ft.tag_id ORDER BY f.path, t.name",
    )?;
    let mut rows = stmt.query([])?;
    while let Some(row) = rows.next()? {
        if let Some(file) = files.get_mut(&row.get::<_, String>(0)?) {
            file.tags.push(row.get(1)?);
        }
    }

    // Album membership: sidecars carry the album UUID (fb:Albums).
    let mut stmt = conn.prepare(
        "SELECT f.path, a.id FROM album_files af JOIN files f ON f.id = af.file_id
         JOIN albums a ON a.id = af.album_id ORDER BY f.path",
    )?;
    let mut rows = stmt.query([])?;
    while let Some(row) = rows.next()? {
        let path: String = row.get(0)?;
        let album_id: i64 = row.get(1)?;
        let uuid: Option<String> = conn
            .query_row("SELECT uuid FROM albums WHERE id = ?1", [album_id], |r| r.get(0))
            .optional()?;
        if let Some(uuid) = uuid {
            if let Some(file) = files.get_mut(&path) {
                file.albums.push(uuid);
            }
        }
    }
    for file in files.values_mut() {
        file.albums.sort();
    }

    let mut albums = Vec::new();
    for info in cat.list_albums()? {
        albums.push(AlbumSnapshot {
            uuid: info.uuid,
            name: info.name,
            date: info.date,
            place: info.place,
            description: info.description,
        });
    }
    albums.sort_by(|a, b| a.uuid.cmp(&b.uuid));

    // Sidecar contents, hashed from disk — the source of truth itself.
    let mut sidecars = BTreeMap::new();
    for path in files.keys() {
        let sc = sidecar_path(path);
        let hash = std::fs::metadata(&sc).ok().and_then(|_| hash_file(&sc));
        sidecars.insert(sc.to_string_lossy().into_owned(), hash);
    }

    Ok(Snapshot { folders, files, albums, sidecars })
}

fn hash_file(path: &Path) -> Option<String> {
    let bytes = std::fs::read(path).ok()?;
    Some(format!("{:x}", Sha256::digest(&bytes)))
}

/// Semantic per-field diff of two snapshots. Only differences are reported;
/// an empty diff means zero loss (§9).
pub fn diff(before: &Snapshot, after: &Snapshot) -> DiffReport {
    let mut diffs = Vec::new();
    let mut push = |path: &str, field: &str, before: Option<String>, after: Option<String>| {
        if before != after {
            diffs.push(FieldDiff { path: path.to_owned(), field: field.to_owned(), before, after });
        }
    };

    for (path, file) in &before.files {
        let Some(after_file) = after.files.get(path) else {
            push(path, "file", Some("present".into()), None);
            continue;
        };
        push(path, "rating", opt_str(&file.rating), opt_str(&after_file.rating));
        push(path, "caption", file.caption.clone(), after_file.caption.clone());
        push(path, "tags", Some(file.tags.join(", ")), Some(after_file.tags.join(", ")));
        push(path, "gps", opt_str(&file.gps), opt_str(&after_file.gps));
        push(path, "artist", file.artist.clone(), after_file.artist.clone());
        push(path, "copyright", file.copyright.clone(), after_file.copyright.clone());
        push(path, "hidden", Some(file.hidden.to_string()), Some(after_file.hidden.to_string()));
        push(path, "taken_at", file.taken_at.clone(), after_file.taken_at.clone());
        push(path, "stack_id", file.stack_id.clone(), after_file.stack_id.clone());
        push(path, "stack_role", file.stack_role.clone(), after_file.stack_role.clone());
        push(
            path,
            "has_recipe",
            Some(file.has_recipe.to_string()),
            Some(after_file.has_recipe.to_string()),
        );
        push(path, "albums", Some(file.albums.join(", ")), Some(after_file.albums.join(", ")));
    }
    for path in after.files.keys() {
        if !before.files.contains_key(path) {
            push(path, "file", None, Some("present".into()));
        }
    }

    for album in &before.albums {
        let Some(after_album) = after.albums.iter().find(|a| a.uuid == album.uuid) else {
            push(&album.uuid, "album", Some(album.name.clone()), None);
            continue;
        };
        push(&album.uuid, "album.name", Some(album.name.clone()), Some(after_album.name.clone()));
        push(&album.uuid, "album.date", album.date.clone(), after_album.date.clone());
        push(&album.uuid, "album.place", album.place.clone(), after_album.place.clone());
        push(
            &album.uuid,
            "album.description",
            album.description.clone(),
            after_album.description.clone(),
        );
    }
    for album in &after.albums {
        if !before.albums.iter().any(|a| a.uuid == album.uuid) {
            push(&album.uuid, "album", None, Some(album.name.clone()));
        }
    }

    for (sc, hash) in &before.sidecars {
        push(sc, "sidecar_sha256", hash.clone(), after.sidecars.get(sc).cloned().flatten());
    }

    DiffReport { pass: diffs.is_empty(), diffs }
}

fn opt_str<T: std::fmt::Debug>(value: &Option<T>) -> Option<String> {
    value.as_ref().map(|v| format!("{v:?}"))
}

/// Delete the catalog database (and its WAL/SHM side files). Only the
/// rebuildable cache is removed — `albums.json`, settings, originals and
/// sidecars are never touched (§3, §5).
pub fn delete_catalog_db(db_path: &Path) -> std::io::Result<()> {
    for suffix in ["", "-wal", "-shm"] {
        let file = format!("{}{suffix}", db_path.display());
        match std::fs::remove_file(&file) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e),
        }
    }
    Ok(())
}

/// Re-seed the watched folders a wiped catalog forgot, from the snapshot.
/// Folder paths and scan modes are catalog configuration, so restoring them
/// from the snapshot is what lets the existing rebuild command re-walk the
/// library headless.
pub fn restore_folders(cat: &mut Catalog, snapshot: &Snapshot) -> fotobase_catalog::Result<()> {
    for folder in &snapshot.folders {
        cat.seed_folder(&folder.path)?;
        let mode = if folder.scan_mode == "always" { ScanMode::Always } else { ScanMode::Once };
        cat.set_folder_scan_mode(&folder.path, mode)?;
    }
    Ok(())
}
