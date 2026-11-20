//! The content-key ledger: Fotobase's local record of what it has put into
//! one Google Photos account (TASK-08-012).
//!
//! Google Photos offers no way to ask "do you already hold these bytes?" —
//! reading a library back is not allowed beyond the app's own uploads — so
//! the answer has to be kept here. Upload retries and re-runs consult the
//! ledger **before** touching the network: a key already in scope is
//! answered locally, and a key known to the library but not yet in the
//! requested album is only added to that album, never uploaded again.
//!
//! One ledger belongs to one Google account (ids from another account are
//! meaningless). It is a rebuildable cache, never user data: losing it costs
//! at worst a repeated upload, which Google resolves to the same media item
//! because identical bytes are one object.
//!
//! On disk it is an append-only journal of JSON lines, so recording a file
//! stays cheap on very large batches and a crash can only damage the last,
//! partially written line — which is dropped on the next open.

use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use serde_json::{json, Value};

use crate::{ContentKey, ExportError};

#[derive(Default)]
struct Entry {
    media_item_id: String,
    albums: BTreeSet<String>,
}

#[derive(Default)]
struct Inner {
    path: Option<PathBuf>,
    entries: BTreeMap<String, Entry>,
}

/// Cheap to clone; clones share the same records.
#[derive(Clone, Default)]
pub struct UploadLedger {
    inner: Arc<Mutex<Inner>>,
}

impl UploadLedger {
    /// A ledger that lives only as long as the process.
    pub fn in_memory() -> Self {
        Self::default()
    }

    /// Opens (or creates) the journal at `path`.
    pub fn open(path: &Path) -> Result<Self, ExportError> {
        let mut inner = Inner { path: Some(path.to_path_buf()), ..Inner::default() };
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        if path.exists() {
            let text = fs::read_to_string(path)?;
            let valid_len = inner.replay(&text, path)?;
            if valid_len < text.len() {
                OpenOptions::new().write(true).open(path)?.set_len(valid_len as u64)?;
            }
        }
        Ok(Self { inner: Arc::new(Mutex::new(inner)) })
    }

    /// The media item holding `key` in the library, whatever album it is in.
    pub fn media_item(&self, key: &ContentKey) -> Option<String> {
        self.lock().entries.get(key.as_str()).map(|e| e.media_item_id.clone())
    }

    /// The media item holding `key` in `album` (anywhere, for `None`).
    pub fn holds(&self, key: &ContentKey, album: Option<&str>) -> Option<String> {
        let inner = self.lock();
        let entry = inner.entries.get(key.as_str())?;
        match album {
            None => Some(entry.media_item_id.clone()),
            Some(album) => entry.albums.contains(album).then(|| entry.media_item_id.clone()),
        }
    }

    /// Records that `media_item_id` now holds `key`, in `album` when given.
    /// Call only after Google confirmed the upload.
    pub fn record(
        &self,
        key: &ContentKey,
        media_item_id: &str,
        album: Option<&str>,
    ) -> Result<(), ExportError> {
        let mut inner = self.lock();
        let entry = inner.entries.entry(key.to_string()).or_default();
        if entry.media_item_id != media_item_id {
            entry.albums.clear();
            entry.media_item_id = media_item_id.to_owned();
        }
        entry.albums.extend(album.map(str::to_owned));
        inner.append(&json!({ "key": key.as_str(), "item": media_item_id, "album": album }))
    }

    /// Drops `key`, e.g. when its media item no longer exists in the account.
    pub fn forget(&self, key: &ContentKey) -> Result<(), ExportError> {
        let mut inner = self.lock();
        if inner.entries.remove(key.as_str()).is_none() {
            return Ok(());
        }
        inner.append(&json!({ "key": key.as_str(), "forget": true }))
    }

    pub fn len(&self) -> usize {
        self.lock().entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

impl std::fmt::Debug for UploadLedger {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("UploadLedger").field("entries", &self.len()).finish()
    }
}

impl Inner {
    /// Applies every complete journal line; returns the length of the valid
    /// prefix. Only the final line may be damaged (a crash mid-append).
    fn replay(&mut self, text: &str, path: &Path) -> Result<usize, ExportError> {
        let mut valid_len = 0;
        let mut rest = text;
        while !rest.is_empty() {
            let (line, next, complete) = match rest.split_once('\n') {
                Some((line, next)) => (line, next, true),
                None => (rest, "", false),
            };
            // A line without its newline was never fully written.
            let parsed = complete.then(|| serde_json::from_str::<Value>(line).ok()).flatten();
            match parsed {
                Some(record) if self.apply(&record) => valid_len += line.len() + 1,
                _ if next.is_empty() => break,
                _ => {
                    return Err(ExportError::Fatal(format!(
                        "the upload ledger `{}` is damaged; move it aside to start a new one \
                         (already uploaded photos may then be uploaded again)",
                        path.display()
                    )))
                }
            }
            rest = next;
        }
        Ok(valid_len)
    }

    /// Applies one journal record; `false` when it is not a valid record.
    fn apply(&mut self, record: &Value) -> bool {
        let Some(key) = record["key"].as_str() else { return false };
        if record["forget"].as_bool() == Some(true) {
            self.entries.remove(key);
            return true;
        }
        let Some(item) = record["item"].as_str() else { return false };
        let entry = self.entries.entry(key.to_owned()).or_default();
        if entry.media_item_id != item {
            entry.albums.clear();
            entry.media_item_id = item.to_owned();
        }
        entry.albums.extend(record["album"].as_str().map(str::to_owned));
        true
    }

    fn append(&self, record: &Value) -> Result<(), ExportError> {
        let Some(path) = &self.path else { return Ok(()) };
        let mut line = record.to_string();
        line.push('\n');
        OpenOptions::new().create(true).append(true).open(path)?.write_all(line.as_bytes())?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(n: u8) -> ContentKey {
        ContentKey::from_bytes(&[n])
    }

    #[test]
    fn scopes_membership_by_album() {
        let ledger = UploadLedger::in_memory();
        ledger.record(&key(1), "item-1", None).unwrap();
        assert_eq!(ledger.holds(&key(1), None).as_deref(), Some("item-1"));
        assert_eq!(ledger.holds(&key(1), Some("a")), None);
        assert_eq!(ledger.media_item(&key(1)).as_deref(), Some("item-1"));

        ledger.record(&key(1), "item-1", Some("a")).unwrap();
        assert_eq!(ledger.holds(&key(1), Some("a")).as_deref(), Some("item-1"));
        assert_eq!(ledger.holds(&key(1), Some("b")), None);
        assert_eq!(ledger.holds(&key(2), None), None);
    }

    #[test]
    fn a_new_media_item_resets_album_membership() {
        let ledger = UploadLedger::in_memory();
        ledger.record(&key(1), "old", Some("a")).unwrap();
        ledger.record(&key(1), "new", None).unwrap();
        assert_eq!(ledger.holds(&key(1), Some("a")), None);
    }

    #[test]
    fn journal_survives_a_restart_and_forgetting() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested/ledger.jsonl");
        let ledger = UploadLedger::open(&path).unwrap();
        ledger.record(&key(1), "item-1", Some("a")).unwrap();
        ledger.record(&key(2), "item-2", None).unwrap();
        ledger.forget(&key(2)).unwrap();

        let reopened = UploadLedger::open(&path).unwrap();
        assert_eq!(reopened.len(), 1);
        assert_eq!(reopened.holds(&key(1), Some("a")).as_deref(), Some("item-1"));
        assert_eq!(reopened.media_item(&key(2)), None);
    }

    #[test]
    fn a_torn_last_line_is_dropped_and_later_appends_stay_valid() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("ledger.jsonl");
        let ledger = UploadLedger::open(&path).unwrap();
        ledger.record(&key(1), "item-1", None).unwrap();
        fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap()
            .write_all(b"{\"key\":\"ab")
            .unwrap();

        let reopened = UploadLedger::open(&path).unwrap();
        assert_eq!(reopened.len(), 1);
        reopened.record(&key(2), "item-2", None).unwrap();
        assert_eq!(UploadLedger::open(&path).unwrap().len(), 2);
    }

    #[test]
    fn damage_in_the_middle_is_refused_not_silently_reset() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("ledger.jsonl");
        let good = json!({"key": key(1).as_str(), "item": "i", "album": null});
        fs::write(&path, format!("garbage\n{good}\n")).unwrap();
        let error = UploadLedger::open(&path).unwrap_err();
        assert!(error.to_string().contains("damaged"), "{error}");
    }
}
