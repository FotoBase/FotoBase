//! Folder-level metadata file (ADR-0007, docs/sidecar-merge-on-write.md §2):
//! `.fotobase` next to a folder's photos, one JSON document
//! `{ "version": 1, "hidden": false }`. Folder hide never touches member
//! sidecars; the scanner inherits the flag at scan time. Writes follow the
//! same merge-on-write discipline as XMP sidecars (§3): fresh read, only the
//! known key changes, unknown keys survive, one `.fb-bak` backup, atomic
//! `.tmp` + rename.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use serde_json::{Map, Value};

/// File name of the folder metadata document.
pub const FOLDER_META_FILE: &str = ".fotobase";

const FORMAT_VERSION: u64 = 1;

#[derive(Debug, thiserror::Error)]
pub enum FolderMetaError {
    #[error("I/O error on {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("malformed folder metadata {path}: {message}")]
    Malformed { path: PathBuf, message: String },
}

/// The folder flags Fotobase understands.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FolderMeta {
    pub hidden: bool,
}

impl FolderMeta {
    /// `<dir>/.fotobase`.
    pub fn path(dir: &Path) -> PathBuf {
        dir.join(FOLDER_META_FILE)
    }

    /// Read a folder's flags. An absent file means "not hidden"; a malformed
    /// one is an error, never a silent default.
    pub fn read(dir: &Path) -> Result<Self, FolderMetaError> {
        let doc = read_doc(&Self::path(dir))?;
        Ok(Self { hidden: doc.get("hidden").and_then(Value::as_bool).unwrap_or(false) })
    }

    /// Hide or unhide a folder: read-modify-write of the `hidden` key only.
    /// Unhiding a folder that has no file writes nothing — absent means
    /// "not hidden" — and a malformed file is left untouched.
    pub fn set_hidden(dir: &Path, hidden: bool) -> Result<(), FolderMetaError> {
        let path = Self::path(dir);
        let exists = path.exists();
        let mut doc = read_doc(&path)?;
        if !exists && !hidden {
            return Ok(());
        }
        doc.insert("hidden".to_string(), Value::Bool(hidden));
        doc.entry("version".to_string()).or_insert_with(|| Value::from(FORMAT_VERSION));
        write_doc(&path, &doc)
    }
}

fn read_doc(path: &Path) -> Result<Map<String, Value>, FolderMetaError> {
    let bytes = match fs::read(path) {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Map::new()),
        Err(e) => return Err(FolderMetaError::Io { path: path.to_path_buf(), source: e }),
    };
    match serde_json::from_slice::<Value>(&bytes) {
        Ok(Value::Object(map)) => Ok(map),
        Ok(_) => Err(FolderMetaError::Malformed {
            path: path.to_path_buf(),
            message: "not a JSON object".to_string(),
        }),
        Err(e) => {
            Err(FolderMetaError::Malformed { path: path.to_path_buf(), message: e.to_string() })
        }
    }
}

fn write_doc(path: &Path, doc: &Map<String, Value>) -> Result<(), FolderMetaError> {
    let io = |p: &Path| {
        let p = p.to_path_buf();
        move |source| FolderMetaError::Io { path: p.clone(), source }
    };
    if path.exists() {
        let bak = suffixed(path, ".fb-bak");
        if let Err(err) = fs::copy(path, &bak) {
            tracing::warn!(
                path = %path.display(),
                error = %err,
                "folder metadata backup failed; continuing (non-fatal per 00-010 §3.4)"
            );
        }
    }
    let tmp = suffixed(path, ".tmp");
    let bytes = serde_json::to_vec_pretty(doc).map_err(|e| FolderMetaError::Malformed {
        path: path.to_path_buf(),
        message: e.to_string(),
    })?;
    {
        let mut f = fs::File::create(&tmp).map_err(io(&tmp))?;
        f.write_all(&bytes).map_err(io(&tmp))?;
        f.sync_all().map_err(io(&tmp))?;
    }
    if let Err(err) = fs::rename(&tmp, path) {
        let _ = fs::remove_file(&tmp);
        return Err(io(path)(err));
    }
    Ok(())
}

fn suffixed(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.file_name().map(|n| n.to_os_string()).unwrap_or_default();
    name.push(suffix);
    path.with_file_name(name)
}
