//! SQLite catalog: schema, migrations, WAL, integrity checks, rolling backups
//! (PLAN §5 "SQLite robustness", TASK-01-005).
//!
//! The catalog is a rebuildable cache; XMP sidecars remain the source of truth
//! for user data. Corruption is detected on startup (`quick_check`) and callers
//! trigger the rebuild-from-sidecars recovery path.

pub mod migrations;

pub use migrations::MIGRATIONS;

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use rusqlite::Connection;

/// Crate identity, used by the workspace smoke test.
pub const CRATE_NAME: &str = "fotobase-catalog";

#[derive(Debug, thiserror::Error)]
pub enum CatalogError {
    /// The catalog file is corrupt; the caller must rebuild from sidecars (§5).
    #[error("catalog corrupted: {0}")]
    Corrupted(String),
    /// The database was created by a newer app version; forward-only migrations
    /// must never downgrade it.
    #[error("catalog schema v{found} is newer than this build (latest v{latest})")]
    NewerSchema { found: i64, latest: usize },
    #[error("backup failed: {0}")]
    Backup(String),
    #[error(transparent)]
    Sql(#[from] rusqlite::Error),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, CatalogError>;

/// One library-grid item (TASK-01-013): stable id, display name and the
/// content hash backing the `thumb://` URI (`None` until computed).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct FileInfo {
    pub id: i64,
    pub filename: String,
    pub hash: Option<String>,
}

#[derive(Debug)]
pub struct Catalog {
    conn: Connection,
    path: PathBuf,
}

impl Catalog {
    /// Open (or create) a catalog: WAL mode, foreign keys on, migrations
    /// applied forward-only, then a cheap `quick_check` for corruption.
    pub fn open(path: impl AsRef<Path>) -> Result<Catalog> {
        let path = path.as_ref().to_path_buf();
        let conn = Connection::open(&path)?;

        if let Err(e) = Self::configure(&conn) {
            let e = corruption_or_sql(e, "configuring catalog");
            return Err(e);
        }

        let version: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        if version > MIGRATIONS.len() as i64 {
            return Err(CatalogError::NewerSchema { found: version, latest: MIGRATIONS.len() });
        }

        migrations::migrate(&conn)?;

        let cat = Catalog { conn, path };
        // Cheap corruption check on startup; full `integrity_check` is opt-in.
        cat.quick_check()?;
        Ok(cat)
    }

    fn configure(conn: &Connection) -> rusqlite::Result<()> {
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        Ok(())
    }

    pub fn conn(&self) -> &Connection {
        &self.conn
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn schema_version(&self) -> Result<i64> {
        Ok(self.conn.query_row("PRAGMA user_version", [], |r| r.get(0))?)
    }

    /// Full `PRAGMA integrity_check` (maintenance dialog, §5).
    pub fn integrity_check(&self) -> Result<()> {
        self.run_check("PRAGMA integrity_check")
    }

    /// Cheap subset used at startup (§5: "`PRAGMA integrity_check` on startup
    /// (cheap subset)") — `quick_check(1)` skips index cross-verification.
    pub fn quick_check(&self) -> Result<()> {
        self.run_check("PRAGMA quick_check(1)")
    }

    fn run_check(&self, pragma: &str) -> Result<()> {
        let status: String = self.conn.query_row(pragma, [], |r| r.get(0))?;
        if status == "ok" {
            Ok(())
        } else {
            Err(CatalogError::Corrupted(status))
        }
    }

    /// Take a consistent snapshot backup now (`VACUUM INTO`), then prune to
    /// the newest `keep` backups. Returns the created backup path.
    pub fn backup_now(&self, keep: usize) -> Result<PathBuf> {
        let dir = self
            .path
            .parent()
            .ok_or_else(|| CatalogError::Backup("catalog has no parent directory".into()))?
            .to_path_buf();
        let stamp = chrono_stamp(SystemTime::now());
        let stem = file_stem(&self.path);
        let mut target = dir.join(format!("{stem}.backup-{stamp}"));
        // Timestamps can collide within a second; nudge until unique.
        let mut n = 1u32;
        while target.exists() {
            target = dir.join(format!("{stem}.backup-{stamp}.{n}"));
            n += 1;
        }

        let sql = format!("VACUUM INTO '{}'", target.to_string_lossy().replace('\'', "''"));
        self.conn.execute_batch(&sql)?;
        self.prune_backups(keep)?;
        Ok(target)
    }

    /// Daily rolling backup: at most one snapshot per calendar day, keeping the
    /// newest `keep` backups next to the catalog file.
    pub fn backup_if_stale(&self, keep: usize) -> Result<Option<PathBuf>> {
        if self.has_backup_today() {
            return Ok(None);
        }
        Ok(Some(self.backup_now(keep)?))
    }

    fn has_backup_today(&self) -> bool {
        let prefix = format!("{}.backup-{}", file_stem(&self.path), today());
        self.backup_files()
            .iter()
            .any(|p| p.file_name().is_some_and(|n| n.to_string_lossy().starts_with(&prefix)))
    }

    fn backup_files(&self) -> Vec<PathBuf> {
        let dir = match self.path.parent() {
            Some(d) => d,
            None => return Vec::new(),
        };
        let prefix = format!("{}.backup-", file_stem(&self.path));
        let mut files: Vec<PathBuf> = std::fs::read_dir(dir)
            .ok()
            .into_iter()
            .flatten()
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| p.file_name().is_some_and(|n| n.to_string_lossy().starts_with(&prefix)))
            .collect();
        files.sort();
        files
    }

    /// Delete the oldest backups beyond `keep` (newest survive).
    fn prune_backups(&self, keep: usize) -> Result<()> {
        let files = self.backup_files();
        let excess = files.len().saturating_sub(keep);
        for path in &files[..excess] {
            std::fs::remove_file(path)?;
        }
        Ok(())
    }

    /// Start a raw `BEGIN` on the connection without a guard object: the
    /// transaction stays open until the caller commits/rolls back explicitly
    /// (used for batched scans and the crash-recovery test). If the process
    /// dies first, SQLite rolls the transaction back on the next open.
    pub fn begin_uncommitted_batch(&mut self) -> Result<()> {
        self.conn.execute_batch("BEGIN")?;
        Ok(())
    }

    /// Commit a batch started with [`Catalog::begin_uncommitted_batch`].
    pub fn commit_batch(&mut self) -> Result<()> {
        self.conn.execute_batch("COMMIT")?;
        Ok(())
    }

    /// Scanning helper (TASK-01-006 builds on this): upsert a folder row.
    pub fn seed_folder(&mut self, path: &str) -> Result<i64> {
        self.conn.execute(
            "INSERT INTO folders(path) VALUES (?1) ON CONFLICT(path) DO NOTHING",
            [path],
        )?;
        Ok(self.conn.query_row("SELECT id FROM folders WHERE path = ?1", [path], |r| r.get(0))?)
    }

    pub fn folder_count(&self) -> Result<i64> {
        Ok(self.conn.query_row("SELECT COUNT(*) FROM folders", [], |r| r.get(0))?)
    }

    /// Source paths whose content hash matches (TASK-01-011): the `thumb://`
    /// handler uses this to trigger a render job when a thumbnail is not yet
    /// cached. Duplicates legitimately share a hash. `NULL` hashes (not yet
    /// computed) match nothing.
    pub fn paths_for_hash(&self, hash: &str) -> Result<Vec<String>> {
        let mut stmt =
            self.conn.prepare_cached("SELECT path FROM files WHERE hash = ?1 ORDER BY path")?;
        let paths = stmt.query_map([hash], |r| r.get(0))?;
        Ok(paths.flatten().collect())
    }

    /// Live full-text search over filename + folder (PLAN §5 `files_fts`,
    /// TASK-01-008). Returns matching file ids, best matches first, at most
    /// `limit` of them.
    ///
    /// Whitespace-separated tokens are AND-combined and prefix-matched so the
    /// UI can call this on every keystroke. Typed text is always treated as
    /// literal search terms, never as FTS5 query syntax.
    pub fn search_files(&self, query: &str, limit: usize) -> Result<Vec<i64>> {
        let expr = fts_match_expr(query);
        if expr.is_empty() {
            return Ok(Vec::new());
        }
        let mut stmt = self.conn.prepare_cached(
            "SELECT f.id FROM files_fts
             JOIN files f ON f.id = files_fts.rowid
             WHERE files_fts MATCH ?1
             ORDER BY rank
             LIMIT ?2",
        )?;
        let ids = stmt.query_map(rusqlite::params![expr, limit as i64], |r| r.get(0))?;
        Ok(ids.flatten().collect())
    }

    /// The library grid's item list (TASK-01-013, PLAN §4): id, filename and
    /// content hash, path-ordered. The hash is the `thumb://` cache key; a
    /// `None` hash (not yet computed) still lists, so the UI shows the
    /// placeholder instead of dropping the photo. No image bytes here — the
    /// grid fetches pixels through the URI scheme, never over IPC.
    pub fn list_files(&self, limit: usize) -> Result<Vec<FileInfo>> {
        let mut stmt = self
            .conn
            .prepare_cached("SELECT id, filename, hash FROM files ORDER BY path LIMIT ?1")?;
        let rows = stmt.query_map([limit as i64], |r| {
            Ok(FileInfo { id: r.get(0)?, filename: r.get(1)?, hash: r.get(2)? })
        })?;
        Ok(rows.flatten().collect())
    }
}

/// Build an FTS5 MATCH expression from user-typed text: each whitespace
/// token becomes a quoted prefix term (`"bea"*`), combined with implicit
/// AND. Quoting defeats FTS5 syntax characters (`(`, `"`, `*`, colons…).
fn fts_match_expr(query: &str) -> String {
    query
        .split_whitespace()
        .map(|token| format!("\"{}\"*", token.replace('"', "\"\"")))
        .collect::<Vec<_>>()
        .join(" ")
}

/// Map a rusqlite error that happened while opening/configuring to
/// [`CatalogError::Corrupted`] when SQLite reports a corrupt/not-a-database file.
fn corruption_or_sql(err: rusqlite::Error, ctx: &str) -> CatalogError {
    use rusqlite::ErrorCode;
    match err.sqlite_error_code() {
        Some(ErrorCode::DatabaseCorrupt) | Some(ErrorCode::NotADatabase) => {
            CatalogError::Corrupted(format!("{ctx}: {err}"))
        }
        _ => CatalogError::Sql(err),
    }
}

fn file_stem(path: &Path) -> String {
    path.file_name().map_or_else(|| "catalog.db".to_string(), |n| n.to_string_lossy().into_owned())
}

fn today() -> String {
    let (y, m, d) = civil_from_days(now_days());
    format!("{y:04}-{m:02}-{d:02}")
}

fn chrono_stamp(t: SystemTime) -> String {
    let secs = t.duration_since(SystemTime::UNIX_EPOCH).unwrap_or_default().as_secs() as i64;
    let (y, m, d) = civil_from_days(secs.div_euclid(86_400));
    let tod = secs.rem_euclid(86_400);
    format!("{y:04}-{m:02}-{d:02}T{:02}{:02}{:02}", tod / 3600, (tod % 3600) / 60, tod % 60)
}

fn now_days() -> i64 {
    SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap_or_default().as_secs() as i64
        / 86_400
}

/// Howard Hinnant's `civil_from_days`: days since 1970-01-01 -> (y, m, d).
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn civil_date_math() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(19_723), (2024, 1, 1));
        assert_eq!(civil_from_days(20_747), (2026, 10, 21));
    }
}
