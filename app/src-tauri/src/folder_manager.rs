//! Folder Manager backend (TASK-01-016, PLAN §2A).
//!
//! Per folder: **Scan Once** (scan now, don't watch), **Scan Always** (hand
//! the folder to the scanner's `notify` watcher, 01-007), **Remove from
//! catalog** (drop the catalog rows; files on disk are never touched — the
//! library mirrors real disk folders, nothing is copied or deleted).
//!
//! The manager owns the watcher plus a dedicated catalog connection: WAL
//! lets the UI read through the shell's own connection while a scan holds
//! this one, so a long scan never blocks IPC reads. All work here is plain
//! Rust — Tauri types stay in `lib.rs` so these paths are testable without
//! a running app.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use fotobase_catalog::{Catalog, ScanMode};
use fotobase_scanner::{scan_folder, ScanHook, ScanReport, Watcher};

/// Crate identity, used by the workspace smoke test.
pub const CRATE_NAME: &str = "fotobase-folder-manager";

/// Parse the `mode` string a Tauri command receives into a [`ScanMode`].
pub fn parse_mode(mode: &str) -> Option<ScanMode> {
    match mode {
        "once" => Some(ScanMode::Once),
        "always" => Some(ScanMode::Always),
        _ => None,
    }
}

/// Watched folders plus their owned catalog connection, driven by the Folder
/// Manager commands.
pub struct FolderManager {
    watcher: Watcher,
    catalog: Arc<Mutex<Catalog>>,
    /// Scan-completion hook (see [`Watcher::spawn_with`]): the shell emits
    /// `library-changed` through it so the UI re-lists after a scan that
    /// changed something — also fired by `scan_now` (Scan Once).
    on_change: Option<ScanHook>,
}

impl FolderManager {
    /// Open the manager: a catalog connection of its own plus a watcher
    /// armed for every top-level `scan_mode = 'always'` folder already in
    /// the catalog (so a restart resumes watching without any dialog
    /// action). Nested rows fold into their parent — watching the parent
    /// covers the whole tree.
    pub fn open(catalog_path: &Path, debounce: Duration) -> Result<Self, String> {
        Self::open_with(catalog_path, debounce, None)
    }

    /// [`open`] with a scan-completion hook, called after every scan that
    /// changed the catalog (watcher-driven and `scan_now` alike).
    pub fn open_with(
        catalog_path: &Path,
        debounce: Duration,
        on_change: Option<ScanHook>,
    ) -> Result<Self, String> {
        let catalog = Arc::new(Mutex::new(
            Catalog::open(catalog_path).map_err(|e| format!("open catalog: {e}"))?,
        ));
        let roots = {
            let cat = catalog.lock().map_err(|e| format!("catalog mutex poisoned: {e}"))?;
            top_level_roots(&cat)
        };
        let watcher = Watcher::spawn_with(&roots, catalog.clone(), debounce, on_change.clone())
            .map_err(|e| e.to_string())?;
        Ok(Self { watcher, catalog, on_change })
    }

    /// Number of scans the watcher has started (test observability).
    pub fn scan_count(&self) -> usize {
        self.watcher.scan_count()
    }

    /// Apply a folder's new scan mode. `Always` arms the watcher first — if
    /// the folder cannot be watched (vanished, permission), the catalog mode
    /// is not persisted either, so the UI never shows a mode the backend
    /// does not honor. `Once` un-watches (a no-op when not watched) and
    /// scans immediately, so a newly added folder shows up on apply.
    pub fn set_mode(&self, path: &str, mode: ScanMode) -> Result<(), String> {
        match mode {
            ScanMode::Always => {
                self.watcher.add_root(Path::new(path)).map_err(|e| e.to_string())?;
                self.persist_mode(path, mode)?;
            }
            ScanMode::Once => {
                self.persist_mode(path, mode)?;
                self.watcher.remove_root(Path::new(path));
                self.scan_now(path)?;
            }
        }
        Ok(())
    }

    /// "Scan Once" semantics: one incremental rescan of a folder right now,
    /// regardless of its mode.
    pub fn scan_now(&self, path: &str) -> Result<ScanReport, String> {
        let mut cat = self.catalog.lock().map_err(|e| format!("catalog mutex poisoned: {e}"))?;
        let report =
            scan_folder(&mut cat, Path::new(path), &std::sync::atomic::AtomicBool::new(false))
                .map_err(|e| format!("scan {path}: {e}"))?;
        if report.changed() {
            if let Some(hook) = &self.on_change {
                hook(&report);
            }
        }
        Ok(report)
    }

    /// Remove a folder from the catalog (with its nested folder rows); also
    /// stops watching it. Returns how many folder rows were removed (`0`
    /// when the folder was never cataloged). Files on disk are untouched.
    pub fn remove(&self, path: &str) -> Result<usize, String> {
        self.watcher.remove_root(Path::new(path));
        let mut cat = self.catalog.lock().map_err(|e| format!("catalog mutex poisoned: {e}"))?;
        cat.remove_folder_tree(path).map_err(|e| format!("remove folder {path}: {e}"))
    }

    fn persist_mode(&self, path: &str, mode: ScanMode) -> Result<(), String> {
        let mut cat = self.catalog.lock().map_err(|e| format!("catalog mutex poisoned: {e}"))?;
        let changed =
            cat.set_folder_scan_mode(path, mode).map_err(|e| format!("set scan mode: {e}"))?;
        if !changed {
            // First time this folder is managed: seed a row for it, then set
            // the mode. The scan (watcher's closing scan or `scan_now`)
            // fills in the files.
            cat.seed_folder(path).map_err(|e| format!("seed folder {path}: {e}"))?;
            cat.set_folder_scan_mode(path, mode).map_err(|e| format!("set scan mode: {e}"))?;
        }
        Ok(())
    }

    /// Shut the watcher down and wait for its threads.
    pub fn stop(self) {
        self.watcher.stop();
    }
}

/// Top-level `scan_mode = 'always'` folders: nested rows fold into their
/// parent (watching the parent covers the whole tree; a nested root would
/// double-report events and double-scan).
fn top_level_roots(cat: &Catalog) -> Vec<PathBuf> {
    let mut roots: Vec<String> = Vec::new();
    for path in cat.watched_folder_paths().unwrap_or_default() {
        if !roots.iter().any(|r| r == &path || fotobase_catalog::is_within(&path, r)) {
            roots.push(path);
        }
    }
    roots.into_iter().map(PathBuf::from).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mode_strings_parse_strictly() {
        assert_eq!(parse_mode("once"), Some(ScanMode::Once));
        assert_eq!(parse_mode("always"), Some(ScanMode::Always));
        assert_eq!(parse_mode("Always"), None);
        assert_eq!(parse_mode(""), None);
        assert_eq!(parse_mode("never"), None);
    }

    #[test]
    fn nested_watched_folders_fold_into_their_top_level_root() {
        let dir = tempfile::tempdir().unwrap();
        let catalog_path = dir.path().join("catalog.db");
        let mut cat = Catalog::open(&catalog_path).unwrap();
        cat.seed_folder("/photos").unwrap();
        cat.seed_folder("/photos/2023").unwrap();
        cat.seed_folder("/other").unwrap();
        cat.set_folder_scan_mode("/photos", ScanMode::Always).unwrap();
        cat.set_folder_scan_mode("/photos/2023", ScanMode::Always).unwrap();
        cat.set_folder_scan_mode("/other", ScanMode::Always).unwrap();

        let roots = top_level_roots(&cat);
        assert_eq!(roots.len(), 2);
        assert!(roots.contains(&std::path::PathBuf::from("/photos")));
        assert!(roots.contains(&std::path::PathBuf::from("/other")));
    }
}
