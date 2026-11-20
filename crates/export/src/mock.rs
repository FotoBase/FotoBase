//! In-memory sandboxed endpoint + plugin used to exercise the contract suite
//! and as the reference implementation of the plugin semantics.
//!
//! [`MockBehavior`] can switch each guarantee off so tests can prove the
//! contract suite rejects plugins that break it.

use std::sync::{Arc, Mutex, MutexGuard};

use async_trait::async_trait;

use crate::contract::{ContractFixture, Fault, RemoteAlbum};
use crate::{
    AlbumRef, ContentKey, ExportError, ExportPlugin, FileOutcome, FileStatus, FileUpload,
    PluginCapabilities, PluginInfo,
};

/// Which contract guarantees the mock plugin honours.
#[derive(Debug, Clone, Copy)]
pub struct MockBehavior {
    /// Recognise an already-stored content key.
    pub dedupe: bool,
    /// `create_album` returns the existing album for a repeated job id.
    pub idempotent_album: bool,
    /// An aborted upload leaves nothing behind.
    pub atomic_upload: bool,
    /// Declares album support.
    pub albums: bool,
}

impl Default for MockBehavior {
    fn default() -> Self {
        Self { dedupe: true, idempotent_album: true, atomic_upload: true, albums: true }
    }
}

struct StoredFile {
    remote_id: String,
    key: ContentKey,
}

struct StoredAlbum {
    id: String,
    name: String,
    job_id: String,
    files: Vec<ContentKey>,
}

#[derive(Default)]
struct Faults {
    abort_after: Option<usize>,
    fail_once: bool,
    lose_upload_response_once: bool,
    lose_album_response_once: bool,
}

#[derive(Default)]
struct State {
    files: Vec<StoredFile>,
    albums: Vec<StoredAlbum>,
    faults: Faults,
    upload_attempts: usize,
    next_id: u64,
}

impl State {
    fn next_id(&mut self, prefix: &str) -> String {
        self.next_id += 1;
        format!("{prefix}-{}", self.next_id)
    }
}

/// Shared server-side state; clones see the same endpoint.
#[derive(Clone, Default)]
pub struct MockEndpoint {
    state: Arc<Mutex<State>>,
}

impl MockEndpoint {
    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().expect("mock endpoint poisoned")
    }
}

pub struct MockPlugin {
    endpoint: MockEndpoint,
    behavior: MockBehavior,
    info: Option<PluginInfo>,
}

impl MockPlugin {
    pub fn new(endpoint: MockEndpoint, behavior: MockBehavior) -> Self {
        Self { endpoint, behavior, info: None }
    }

    /// Overrides the advertised identity and capabilities.
    pub fn with_info(mut self, id: &str, name: &str, capabilities: PluginCapabilities) -> Self {
        self.info = Some(PluginInfo { id: id.into(), name: name.into(), capabilities });
        self
    }
}

#[async_trait]
impl ExportPlugin for MockPlugin {
    fn info(&self) -> PluginInfo {
        self.info.clone().unwrap_or_else(|| PluginInfo {
            id: "mock".into(),
            name: "Mock endpoint".into(),
            capabilities: PluginCapabilities { albums: self.behavior.albums, size_choice: true },
        })
    }

    async fn list_albums(&mut self) -> Result<Vec<AlbumRef>, ExportError> {
        let state = self.endpoint.lock();
        Ok(state
            .albums
            .iter()
            .map(|a| AlbumRef { id: a.id.clone(), name: a.name.clone() })
            .collect())
    }

    async fn create_album(&mut self, job_id: &str, name: &str) -> Result<AlbumRef, ExportError> {
        if !self.info().capabilities.albums {
            return Err(ExportError::AlbumsUnsupported);
        }
        let mut state = self.endpoint.lock();
        if self.behavior.idempotent_album {
            if let Some(existing) = state.albums.iter().find(|a| a.job_id == job_id) {
                return Ok(AlbumRef { id: existing.id.clone(), name: existing.name.clone() });
            }
        }
        let id = state.next_id("album");
        state.albums.push(StoredAlbum {
            id: id.clone(),
            name: name.into(),
            job_id: job_id.into(),
            files: Vec::new(),
        });
        if std::mem::take(&mut state.faults.lose_album_response_once) {
            return Err(ExportError::Transient("connection reset before album response".into()));
        }
        Ok(AlbumRef { id, name: name.into() })
    }

    async fn upload_file(&mut self, request: FileUpload<'_>) -> Result<FileOutcome, ExportError> {
        let key = request.file.content_key.clone();
        let mut state = self.endpoint.lock();
        state.upload_attempts += 1;

        if std::mem::take(&mut state.faults.fail_once) {
            return Err(ExportError::Transient("connection refused".into()));
        }
        match state.faults.abort_after {
            Some(0) => {
                if !self.behavior.atomic_upload {
                    let partial = ContentKey::raw(format!("partial:{key}"));
                    if !state.files.iter().any(|f| f.key == partial) {
                        let remote_id = state.next_id("partial");
                        state.files.push(StoredFile { remote_id, key: partial });
                    }
                }
                return Err(ExportError::Transient("connection reset mid-upload".into()));
            }
            Some(n) => state.faults.abort_after = Some(n - 1),
            None => {}
        }

        let existing = self
            .behavior
            .dedupe
            .then(|| state.files.iter().find(|f| f.key == key).map(|f| f.remote_id.clone()))
            .flatten();
        let (remote_id, status) = match existing {
            Some(remote_id) => (remote_id, FileStatus::AlreadyPresent),
            None => {
                let remote_id = state.next_id("file");
                state.files.push(StoredFile { remote_id: remote_id.clone(), key: key.clone() });
                (remote_id, FileStatus::Uploaded)
            }
        };

        if let Some(album) = request.album {
            let dedupe = self.behavior.dedupe;
            if let Some(stored) = state.albums.iter_mut().find(|a| a.id == album.id) {
                if !dedupe || !stored.files.contains(&key) {
                    stored.files.push(key);
                }
            }
        }

        if std::mem::take(&mut state.faults.lose_upload_response_once) {
            return Err(ExportError::Transient("connection reset before upload response".into()));
        }
        Ok(FileOutcome { remote_id, status })
    }
}

/// [`MockEndpoint`] + [`MockBehavior`] as a [`ContractFixture`].
pub struct MockFixture {
    endpoint: MockEndpoint,
    behavior: MockBehavior,
}

impl MockFixture {
    pub fn new(behavior: MockBehavior) -> Self {
        Self { endpoint: MockEndpoint::default(), behavior }
    }

    /// Upload attempts the endpoint has seen, retries included.
    pub fn upload_attempts(&self) -> usize {
        self.endpoint.lock().upload_attempts
    }
}

impl ContractFixture for MockFixture {
    type Plugin = MockPlugin;

    fn plugin(&self) -> MockPlugin {
        MockPlugin::new(self.endpoint.clone(), self.behavior)
    }

    fn inject(&self, fault: Fault) {
        let mut state = self.endpoint.lock();
        match fault {
            Fault::AbortUploadsAfter(n) => state.faults.abort_after = Some(n),
            Fault::FailUploadOnce => state.faults.fail_once = true,
            Fault::LoseUploadResponseOnce => state.faults.lose_upload_response_once = true,
            Fault::LoseAlbumResponseOnce => state.faults.lose_album_response_once = true,
        }
    }

    fn clear_faults(&self) {
        self.endpoint.lock().faults = Faults::default();
    }

    fn remote_files(&self) -> Vec<ContentKey> {
        self.endpoint.lock().files.iter().map(|f| f.key.clone()).collect()
    }

    fn remote_albums(&self) -> Vec<RemoteAlbum> {
        self.endpoint
            .lock()
            .albums
            .iter()
            .map(|a| RemoteAlbum { name: a.name.clone(), files: a.files.clone() })
            .collect()
    }
}
