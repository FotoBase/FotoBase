//! Export plugin trait, registry and the shared contract test suite
//! (TASK-08-007, PLAN §3 module layout, §6 P4, §9 "Shared contract tests per
//! plugin").
//!
//! An [`ExportPlugin`] uploads photos to one target (local folder, Immich,
//! S3, WebDAV, Google Photos…). Plugins implement only the target-specific
//! primitives — [`ExportPlugin::create_album`] and
//! [`ExportPlugin::upload_file`] — and inherit the shared upload driver
//! ([`run_upload`]) through the provided [`ExportPlugin::upload`] method, so
//! the cross-plugin guarantees live in one place:
//!
//! - **Resumable, no duplicates.** Every file carries a stable
//!   [`ContentKey`]. Plugins must recognise a key they already hold and answer
//!   [`FileStatus::AlreadyPresent`]; resuming an interrupted job is therefore
//!   just running the same job again.
//! - **Retry.** [`ExportError::Transient`] failures are retried with
//!   exponential backoff up to [`RetryPolicy::max_attempts`]; any other error
//!   aborts at once. A retried request whose first attempt actually landed
//!   server-side is safe because both primitives are idempotent.
//! - **No partial albums.** [`ExportPlugin::create_album`] is idempotent per
//!   job id, so retries (and re-runs) of a job never create a second album.
//! - **Cancellation** is cooperative and checked between files and attempts.
//!
//! Every plugin must pass [`contract::run_contract_suite`] against a
//! sandboxed endpoint; a plugin that fails it cannot ship.
//!
//! Resized outputs (size choice, quality) are produced by the caller with the
//! shared export pipeline (`fotobase-render`); the job carries the resulting
//! files plus the chosen [`UploadOptions`] so plugins can record or forward
//! them.

pub mod contract;
mod driver;
pub mod mock;
mod registry;
mod types;

pub use driver::run_upload;
pub use registry::Registry;
pub use types::*;

use async_trait::async_trait;

/// One upload target.
#[async_trait]
pub trait ExportPlugin: Send {
    /// Static identity and capabilities.
    fn info(&self) -> PluginInfo;

    /// Albums that already exist on the target. Plugins without album
    /// support keep the default (empty).
    async fn list_albums(&mut self) -> Result<Vec<AlbumRef>, ExportError> {
        Ok(Vec::new())
    }

    /// Create the album for upload job `job_id`. **Idempotent per job id:**
    /// calling again with the same id (a retry after a lost response, or a
    /// re-run of an interrupted job) returns the album created the first
    /// time instead of creating another. Plugins without album support keep
    /// the default, which refuses.
    async fn create_album(&mut self, _job_id: &str, _name: &str) -> Result<AlbumRef, ExportError> {
        Err(ExportError::AlbumsUnsupported)
    }

    /// Upload one file. **Idempotent per [`ContentKey`]:** a file the target
    /// already holds is not stored again but is still added to
    /// `request.album` when given, and answers [`FileStatus::AlreadyPresent`].
    /// A failed upload must not leave a partial file behind (stage, then
    /// commit atomically).
    async fn upload_file(&mut self, request: FileUpload<'_>) -> Result<FileOutcome, ExportError>;

    /// Upload a whole job: resolve the album, then each file in order with
    /// progress events, retries and cancellation. Plugins do not override
    /// this; the contract suite relies on the shared driver semantics.
    async fn upload(
        &mut self,
        job: UploadJob,
        progress: &dyn ProgressSink,
        cancel: &CancelToken,
    ) -> Result<UploadReport, ExportError> {
        run_upload(self, job, progress, cancel).await
    }
}
