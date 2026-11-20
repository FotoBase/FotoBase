use std::fmt;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use sha2::{Digest, Sha256};
use thiserror::Error;

/// Stable per-file identity (hex SHA-256 of the uploaded bytes). Targets use
/// it to recognise a retried or resumed upload of a file they already hold.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ContentKey(String);

impl ContentKey {
    pub fn from_bytes(bytes: &[u8]) -> Self {
        Self(hex(&Sha256::digest(bytes)))
    }

    pub fn from_reader(mut reader: impl Read) -> io::Result<Self> {
        let mut hasher = Sha256::new();
        let mut buf = [0u8; 64 * 1024];
        loop {
            let n = reader.read(&mut buf)?;
            if n == 0 {
                break;
            }
            hasher.update(&buf[..n]);
        }
        Ok(Self(hex(&hasher.finalize())))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Wraps an arbitrary string; for test doubles that fabricate keys.
    pub(crate) fn raw(s: String) -> Self {
        Self(s)
    }
}

impl fmt::Display for ContentKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// A file to upload, already rendered at the chosen size.
#[derive(Debug, Clone)]
pub struct UploadFile {
    pub path: PathBuf,
    /// Target-side file name.
    pub name: String,
    pub content_key: ContentKey,
}

impl UploadFile {
    /// Hashes the file (blocking I/O — call from a blocking context).
    pub fn from_path(path: &Path) -> io::Result<Self> {
        let content_key = ContentKey::from_reader(std::fs::File::open(path)?)?;
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "path has no file name"))?;
        Ok(Self { path: path.to_path_buf(), name, content_key })
    }
}

/// Size choice the user made in the upload dialog (PLAN §6 P4 "Web Albums
/// upload: batch upload with size choice").
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SizeChoice {
    Original,
    /// Longest edge in pixels.
    MaxEdge(u32),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UploadOptions {
    pub size: SizeChoice,
    /// JPEG quality (0–100) the files were rendered with.
    pub quality: u8,
}

impl Default for UploadOptions {
    fn default() -> Self {
        Self { size: SizeChoice::Original, quality: 90 }
    }
}

/// An album on the target.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AlbumRef {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AlbumTarget {
    /// Add to an album that already exists.
    Existing(AlbumRef),
    /// Create an album for this job (idempotent per job id).
    Create(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetryPolicy {
    /// Total attempts per request, first try included (minimum 1).
    pub max_attempts: u32,
    /// Delay before the second attempt; doubles on each further attempt.
    pub base_delay: Duration,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self { max_attempts: 3, base_delay: Duration::from_millis(250) }
    }
}

/// A batch upload. Re-running a job with the same `id` and files resumes it.
#[derive(Debug, Clone)]
pub struct UploadJob {
    pub id: String,
    pub files: Vec<UploadFile>,
    pub album: Option<AlbumTarget>,
    pub options: UploadOptions,
    pub retry: RetryPolicy,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PluginCapabilities {
    pub albums: bool,
    pub size_choice: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginInfo {
    /// Registry key, e.g. `immich`.
    pub id: String,
    /// Display name shown wherever upload targets are listed.
    pub name: String,
    pub capabilities: PluginCapabilities,
}

/// Arguments of [`crate::ExportPlugin::upload_file`].
#[derive(Debug, Clone, Copy)]
pub struct FileUpload<'a> {
    pub job_id: &'a str,
    pub file: &'a UploadFile,
    pub album: Option<&'a AlbumRef>,
    pub options: &'a UploadOptions,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileStatus {
    Uploaded,
    /// The target already held this content key; nothing was stored again.
    AlreadyPresent,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileOutcome {
    pub remote_id: String,
    pub status: FileStatus,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileReport {
    pub content_key: ContentKey,
    pub name: String,
    pub remote_id: String,
    pub status: FileStatus,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UploadReport {
    pub album: Option<AlbumRef>,
    pub files: Vec<FileReport>,
}

impl UploadReport {
    pub fn count(&self, status: FileStatus) -> usize {
        self.files.iter().filter(|f| f.status == status).count()
    }
}

/// Per-file progress for the UI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProgressEvent {
    AlbumReady {
        album: AlbumRef,
    },
    FileStarted {
        index: usize,
        total: usize,
        name: String,
    },
    /// A transient failure; `attempt` is the one that just failed.
    Retrying {
        name: Option<String>,
        attempt: u32,
        error: String,
    },
    FileDone {
        index: usize,
        total: usize,
        name: String,
        status: FileStatus,
    },
}

pub trait ProgressSink: Send + Sync {
    fn emit(&self, event: ProgressEvent);
}

impl<F: Fn(ProgressEvent) + Send + Sync> ProgressSink for F {
    fn emit(&self, event: ProgressEvent) {
        self(event)
    }
}

/// Discards progress events.
pub struct NoProgress;

impl ProgressSink for NoProgress {
    fn emit(&self, _event: ProgressEvent) {}
}

/// Cooperative cancellation shared between the UI and a running job.
#[derive(Debug, Clone, Default)]
pub struct CancelToken(Arc<AtomicBool>);

impl CancelToken {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn cancel(&self) {
        self.0.store(true, Ordering::SeqCst);
    }

    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }
}

#[derive(Debug, Error)]
pub enum ExportError {
    /// Worth retrying (network drop, 5xx, lost response).
    #[error("transient upload failure: {0}")]
    Transient(String),
    /// Retrying cannot help (bad credentials, rejected file).
    #[error("upload failed: {0}")]
    Fatal(String),
    #[error("upload cancelled")]
    Cancelled,
    #[error("this upload target does not support albums")]
    AlbumsUnsupported,
    #[error("export plugin `{0}` is already registered")]
    DuplicatePlugin(String),
    #[error("no export plugin `{0}`")]
    UnknownPlugin(String),
    #[error("I/O: {0}")]
    Io(#[from] io::Error),
}
