//! Import from folder / removable drive (TASK-01-017, PLAN §2A/§6 P1).
//!
//! Copies media files into the catalog location and catalogs them — the
//! destination becomes a normal library folder afterwards. Per PLAN §2:
//! "Exclude duplicates" (default on) skips files whose content hash is
//! already in the `files` table and counts them in the report; destination
//! folders are named by date or a user-typed custom pattern.
//!
//! Cancellation is cooperative and per-file (§3 rule 4): the caller passes
//! an [`AtomicBool`], checked before every file *and* per chunk inside a
//! copy. Copies go through a `.part` temp file renamed into place on
//! success, so a cancel can never leave a half-copied file at the
//! destination — the per-file cleanup the task spec demands.
//!
//! Originals are only ever read here; the source is never modified, and no
//! image bytes travel anywhere but the local filesystem copy.

use std::fs::{self, File};
use std::io::{BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use fotobase_catalog::Catalog;
use sha2::{Digest, Sha256};

use crate::walk_dir;

/// Copy chunk: cancel checks happen at most this often during a copy.
const COPY_CHUNK: usize = 64 * 1024;

/// Destination folder naming (PLAN §6 P1 "destination folder naming
/// (date / custom)").
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Naming {
    /// One `YYYY-MM-DD` folder per capture date. The date comes from EXIF
    /// `taken_at` once the metadata crate provides readers (03-08 / exiv2
    /// bindings); until then the file's mtime is the date source.
    DateFolders,
    /// A user-typed pattern: a literal subfolder under the destination root,
    /// possibly several components deep (`2026/Vacation`). Never absolute
    /// and never escaping the root.
    Custom(String),
}

/// Import behavior knobs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportOptions {
    /// "Exclude duplicates" (PLAN §2, default enabled in the UI): files
    /// whose content hash is already cataloged are skipped and reported.
    pub exclude_duplicates: bool,
    pub naming: Naming,
}

/// What one import did.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ImportReport {
    pub copied: usize,
    /// Files skipped because their content hash was already cataloged.
    pub duplicates: usize,
    /// Files that could not be read/copied (vanished, permissions).
    pub failed: usize,
    /// True if the cancel flag stopped the import early.
    pub cancelled: bool,
}

/// Per-file progress for the UI. `files_done` counts fully handled files
/// (copied, skipped as duplicates, or failed); `files_total` is known up
/// front from the source walk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportProgress {
    pub files_done: usize,
    pub files_total: usize,
    pub current_file: String,
}

/// Content hash in the catalog's / render cache-key convention: the first
/// 16 bytes of the SHA-256, lowercase hex (same as `render::cache_key`, so
/// an imported file's `thumb://` key matches its stored hash). Streams the
/// file — camera cards hold multi-megabyte files, nothing is read whole.
pub fn content_hash(path: &Path) -> std::io::Result<String> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; COPY_CHUNK];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hasher.finalize()[..16].iter().fold(String::new(), |mut acc, b| {
        use std::fmt::Write as _;
        let _ = write!(acc, "{b:02x}");
        acc
    }))
}

/// Import every media file under `source` into `dest_root`, cataloging each
/// copy as it lands (per-file commits: a cancelled import leaves the
/// catalog consistent with the destination). Progress fires once per file.
pub fn import_folder(
    catalog: &mut Catalog,
    source: &Path,
    dest_root: &Path,
    options: &ImportOptions,
    cancel: &AtomicBool,
    progress: &dyn Fn(ImportProgress),
) -> fotobase_catalog::Result<ImportReport> {
    if cancel.load(Ordering::Relaxed) {
        return Ok(ImportReport { cancelled: true, ..ImportReport::default() });
    }

    // Walk the source first: totals are known up front and the destination
    // side stays free of source-side surprises.
    let mut dirs = Vec::new();
    let mut files = Vec::new();
    let mut walk_cancelled = false;
    walk_dir(source, &mut dirs, &mut files, cancel, &mut walk_cancelled, &|_| {});
    if walk_cancelled {
        return Ok(ImportReport { cancelled: true, ..ImportReport::default() });
    }
    let total = files.len();

    let mut report = ImportReport::default();
    for f in &files {
        if cancel.load(Ordering::Relaxed) {
            report.cancelled = true;
            return Ok(report);
        }
        progress(ImportProgress {
            files_done: report.copied + report.duplicates + report.failed,
            files_total: total,
            current_file: f.filename.clone(),
        });
        // The cancel flag may have been flipped by the progress consumer.
        if cancel.load(Ordering::Relaxed) {
            report.cancelled = true;
            return Ok(report);
        }

        let hash = match content_hash(Path::new(&f.path)) {
            Ok(hash) => hash,
            Err(_) => {
                report.failed += 1;
                continue;
            }
        };
        if options.exclude_duplicates && catalog.has_hash(&hash)? {
            report.duplicates += 1;
            continue;
        }

        let dest_dir = dest_dir_for(dest_root, &options.naming, f.mtime)?;
        if fs::create_dir_all(&dest_dir).is_err() {
            report.failed += 1;
            continue;
        }
        let final_name = unique_dest_name(&dest_dir, &f.filename);
        let copied = match File::open(&f.path) {
            Ok(mut file) => copy_file(&mut file, &dest_dir, &final_name, cancel),
            Err(_) => Err(std::io::Error::other("open source failed")),
        };
        match copied {
            Ok(true) => {}
            Ok(false) => {
                report.cancelled = true;
                return Ok(report);
            }
            Err(_) => {
                report.failed += 1;
                continue;
            }
        }

        let final_path = dest_dir.join(&final_name);
        upsert_imported(
            catalog,
            &dest_dir.to_string_lossy(),
            &final_path.to_string_lossy(),
            f,
            &final_name,
            &hash,
        )?;
        report.copied += 1;
    }
    Ok(report)
}

/// Copy `reader` into `dest_dir/filename` through a `.part` temp file that
/// is renamed into place only on success. Returns `Ok(true)` when the file
/// was fully copied, `Ok(false)` when the cancel flag surfaced mid-copy
/// (the temp file is removed — no partial file at the destination).
pub fn copy_file(
    reader: &mut impl Read,
    dest_dir: &Path,
    filename: &str,
    cancel: &AtomicBool,
) -> std::io::Result<bool> {
    let tmp = dest_dir.join(format!("{filename}.part"));
    let result = (|| -> std::io::Result<()> {
        let out = File::create(&tmp)?;
        let mut out = BufWriter::new(out);
        let mut buf = vec![0u8; COPY_CHUNK];
        loop {
            if cancel.load(Ordering::Relaxed) {
                return Err(std::io::Error::other("cancelled"));
            }
            let n = reader.read(&mut buf)?;
            if n == 0 {
                break;
            }
            out.write_all(&buf[..n])?;
        }
        out.flush()?;
        Ok(())
    })();
    match result {
        Ok(()) => {
            fs::rename(&tmp, dest_dir.join(filename))?;
            Ok(true)
        }
        Err(_) if cancel.load(Ordering::Relaxed) => {
            let _ = fs::remove_file(&tmp);
            Ok(false)
        }
        Err(e) => {
            let _ = fs::remove_file(&tmp);
            Err(e)
        }
    }
}

/// A name that does not collide at the destination: `name.ext` when free,
/// otherwise `name-1.ext`, `name-2.ext`, … An existing destination file is
/// never overwritten — an import must not destroy what is already there.
fn unique_dest_name(dest_dir: &Path, filename: &str) -> String {
    if !dest_dir.join(filename).exists() {
        return filename.to_string();
    }
    let stem = Path::new(filename).file_stem().and_then(|s| s.to_str()).unwrap_or(filename);
    let ext = Path::new(filename).extension().and_then(|s| s.to_str());
    for n in 1.. {
        let candidate = match ext {
            Some(ext) => format!("{stem}-{n}.{ext}"),
            None => format!("{stem}-{n}"),
        };
        if !dest_dir.join(&candidate).exists() {
            return candidate;
        }
    }
    unreachable!("the loop only exits by returning")
}

/// Destination directory for one file: `dest_root/<date>` for date naming
/// (the file's date: EXIF `taken_at` once available, the mtime until then)
/// or `dest_root/<pattern>` for custom naming. The pattern is validated —
/// absolute paths and `..` components would write outside the destination
/// root and are rejected.
fn dest_dir_for(
    dest_root: &Path,
    naming: &Naming,
    mtime_secs: f64,
) -> fotobase_catalog::Result<PathBuf> {
    let rel: PathBuf = match naming {
        Naming::DateFolders => PathBuf::from(date_folder(mtime_secs)),
        Naming::Custom(pattern) => {
            let p = PathBuf::from(pattern);
            let bad = p.is_absolute()
                || pattern.is_empty()
                || p.components().any(|c| {
                    matches!(
                        c,
                        std::path::Component::ParentDir
                            | std::path::Component::RootDir
                            | std::path::Component::Prefix(_)
                    )
                });
            if bad {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "custom pattern must be a relative subfolder",
                )
                .into());
            }
            p
        }
    };
    Ok(dest_root.join(rel))
}

/// `YYYY-MM-DD` for a unix timestamp (UTC — the file date fallback; EXIF
/// dates carry their own offset and will replace this path entirely).
fn date_folder(mtime_secs: f64) -> String {
    let secs = mtime_secs.floor().max(0.0) as i64;
    let days = secs.div_euclid(86_400);
    let (y, m, d) = civil_from_days(days);
    format!("{y:04}-{m:02}-{d:02}")
}

/// Days since the unix epoch → (year, month, day). Howard Hinnant's
/// civil-from-days algorithm, same as the catalog's backup stamping.
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// Catalog the copied file (folder row + file row with the content hash).
/// Per-file: each insert commits on its own, so the catalog always mirrors
/// what is actually at the destination.
fn upsert_imported(
    catalog: &mut Catalog,
    dir: &str,
    path: &str,
    f: &crate::WalkedFile,
    filename: &str,
    hash: &str,
) -> fotobase_catalog::Result<()> {
    catalog.seed_folder(dir)?;
    let folder_id: i64 =
        catalog.conn().query_row("SELECT id FROM folders WHERE path = ?1", [dir], |r| r.get(0))?;
    catalog.conn().execute(
        "INSERT INTO files(folder_id, path, filename, size, mtime, hash, media_type)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
         ON CONFLICT(path) DO UPDATE SET
            folder_id = excluded.folder_id,
            filename = excluded.filename,
            size = excluded.size,
            mtime = excluded.mtime,
            hash = excluded.hash,
            media_type = excluded.media_type",
        rusqlite::params![folder_id, path, filename, f.size, f.mtime, hash, f.media_type],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn date_folder_formats_the_file_date() {
        // 2023-01-15 12:00:00 UTC
        assert_eq!(date_folder(1_673_784_000.0), "2023-01-15");
        // Pre-epoch (clock skew): clamps to the epoch, never panics.
        assert_eq!(date_folder(-5.0), "1970-01-01");
    }

    #[test]
    fn custom_patterns_may_not_escape_the_destination() {
        let root = Path::new("/dest");
        assert!(dest_dir_for(root, &Naming::Custom("trip".into()), 0.0).is_ok());
        assert!(dest_dir_for(root, &Naming::Custom("2026/Vacation".into()), 0.0).is_ok());
        assert!(dest_dir_for(root, &Naming::Custom("../escape".into()), 0.0).is_err());
        assert!(dest_dir_for(root, &Naming::Custom("/etc".into()), 0.0).is_err());
        assert!(dest_dir_for(root, &Naming::Custom(String::new()), 0.0).is_err());
    }
}
