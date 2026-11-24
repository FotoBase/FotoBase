//! Read-only-safe catalog source (TASK-09-002, PLAN §6 Phase 5).
//!
//! The mobile app browses a single catalog source: a NAS path or a
//! Syncthing/iCloud-synced folder that other processes write to at any time.
//! [`CatalogSource`] is the one open path for it:
//!
//! - **Opening never writes.** [`CatalogSource::open`] only stats the root;
//!   scanning only reads the source and writes the app's own catalog cache
//!   (PLAN §3 rules 1–2).
//! - **Concurrent external writes are tolerated.** The scanners re-check
//!   every file right before indexing it; one still changing is deferred
//!   (`requeued`), never indexed half-written. [`CatalogSource::scan_settled`]
//!   re-scans until the source is quiet or the pass budget runs out.
//! - **Writes are explicit.** [`CatalogSource::authorize_write`] is the guard
//!   every write into the source goes through: only a
//!   [`WriteIntent::UserAction`] passes. The sidecar write itself still
//!   follows `docs/sidecar-merge-on-write.md`.
//! - **Offline-friendly.** A locally synced copy is a plain folder; an
//!   unreachable network share surfaces [`SourceError::Unreachable`] and
//!   leaves the cached catalog untouched, so browsing keeps working.

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use fotobase_catalog::{Catalog, CatalogError};

use crate::{
    is_network_path, scan_folder_with, scan_network_folder, NetworkScanOptions, NetworkScanState,
    ScanOptions, ScanProgress, ScanReport,
};

/// How the source is reached, which picks the scan backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceKind {
    /// Local disk or a locally synced folder (Syncthing, iCloud Drive).
    Local,
    /// A mounted network share (SMB/NFS): throttled, pausable scans.
    Network,
}

/// Why a write into the source is being attempted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WriteIntent {
    /// The user explicitly asked for it (rate a photo, tag it, Save).
    UserAction,
    /// Anything automatic: scans, watchers, caches, repair passes.
    Background,
}

#[derive(Debug, thiserror::Error)]
pub enum SourceError {
    #[error("catalog source {} is not reachable: {reason}", path.display())]
    Unreachable { path: PathBuf, reason: String },
    #[error("catalog source {} is not a directory", path.display())]
    NotADirectory { path: PathBuf },
    #[error("background write to catalog source refused: {}", path.display())]
    WriteNotAuthorized { path: PathBuf },
    #[error(transparent)]
    Catalog(#[from] CatalogError),
}

/// A single catalog source, opened read-only-safe.
#[derive(Clone)]
pub struct CatalogSource {
    root: PathBuf,
    kind: SourceKind,
    network: NetworkScanOptions,
}

impl CatalogSource {
    /// Open `root`, classifying it as local or network by its mount.
    pub fn open(root: &Path) -> Result<Self, SourceError> {
        let kind = if is_network_path(root) { SourceKind::Network } else { SourceKind::Local };
        Self::open_as(root, kind)
    }

    /// [`CatalogSource::open`] with an explicit kind (a share the mount
    /// check cannot classify, or tests).
    pub fn open_as(root: &Path, kind: SourceKind) -> Result<Self, SourceError> {
        let meta = std::fs::metadata(root).map_err(|e| SourceError::Unreachable {
            path: root.to_path_buf(),
            reason: e.to_string(),
        })?;
        if !meta.is_dir() {
            return Err(SourceError::NotADirectory { path: root.to_path_buf() });
        }
        Ok(Self { root: root.to_path_buf(), kind, network: NetworkScanOptions::default() })
    }

    /// Replace the network scan tunables (IO budget, retries, filesystem).
    pub fn with_network_options(mut self, options: NetworkScanOptions) -> Self {
        self.network = options;
        self
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn kind(&self) -> SourceKind {
        self.kind
    }

    /// Guard for every write into the source. Background writes are refused;
    /// paths outside the source (the catalog cache, thumbnails) are not the
    /// source's to guard.
    pub fn authorize_write(&self, target: &Path, intent: WriteIntent) -> Result<(), SourceError> {
        if target.starts_with(&self.root) && intent == WriteIntent::Background {
            return Err(SourceError::WriteNotAuthorized { path: target.to_path_buf() });
        }
        Ok(())
    }

    /// One scan pass. Files still changing land in `requeued`; a network
    /// share that drops mid-scan is [`SourceError::Unreachable`] with
    /// nothing deleted from the catalog.
    pub fn scan(
        &self,
        catalog: &mut Catalog,
        cancel: &AtomicBool,
        progress: &dyn Fn(ScanProgress),
    ) -> Result<ScanReport, SourceError> {
        match self.kind {
            SourceKind::Local => Ok(scan_folder_with(
                catalog,
                &self.root,
                cancel,
                &ScanOptions::default(),
                progress,
            )?),
            SourceKind::Network => {
                let r = scan_network_folder(catalog, &self.root, cancel, &self.network, progress)?;
                match r.state {
                    NetworkScanState::Paused => Err(SourceError::Unreachable {
                        path: self.root.clone(),
                        reason: format!(
                            "scan paused at {}",
                            r.paused_at.as_deref().unwrap_or("the share root")
                        ),
                    }),
                    state => Ok(ScanReport {
                        added: r.added,
                        updated: r.updated,
                        skipped: r.skipped,
                        removed: r.removed,
                        reassociated: r.reassociated,
                        ambiguous: r.ambiguous,
                        requeued: r.requeued,
                        cancelled: state == NetworkScanState::Cancelled,
                        folder_ini_offered: Vec::new(),
                    }),
                }
            }
        }
    }

    /// Scan, then re-scan while files were deferred, up to `max_passes`
    /// passes in total. Counts are summed; `requeued` is what the last pass
    /// still could not settle, so callers see a still-busy source honestly.
    pub fn scan_settled(
        &self,
        catalog: &mut Catalog,
        cancel: &AtomicBool,
        max_passes: usize,
        progress: &dyn Fn(ScanProgress),
    ) -> Result<ScanReport, SourceError> {
        let mut total = ScanReport::default();
        for _ in 0..max_passes.max(1) {
            let pass = self.scan(catalog, cancel, progress)?;
            total.added += pass.added;
            total.updated += pass.updated;
            total.skipped = pass.skipped;
            total.removed += pass.removed;
            total.reassociated += pass.reassociated;
            total.ambiguous += pass.ambiguous;
            total.folder_ini_offered.extend(pass.folder_ini_offered);
            total.cancelled = pass.cancelled;
            total.requeued = pass.requeued;
            if total.cancelled || !total.needs_rescan() {
                break;
            }
        }
        Ok(total)
    }
}
