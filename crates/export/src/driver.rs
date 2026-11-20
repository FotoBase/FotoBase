use crate::{
    AlbumRef, AlbumTarget, CancelToken, ExportError, ExportPlugin, FileReport, FileUpload,
    ProgressEvent, ProgressSink, RetryPolicy, UploadJob, UploadReport,
};

/// Runs `$call` (an expression producing a future of `Result<_, ExportError>`)
/// under `$policy`: transient failures are retried with exponential backoff,
/// everything else is returned at once. Cancellation is checked before every
/// attempt. A macro because the call mutably
/// borrows the plugin and must be re-created per attempt.
macro_rules! with_retry {
    ($policy:expr, $cancel:expr, $progress:expr, $name:expr, $call:expr) => {{
        let policy: RetryPolicy = $policy;
        let max_attempts = policy.max_attempts.max(1);
        let mut attempt = 1u32;
        loop {
            if $cancel.is_cancelled() {
                break Err(ExportError::Cancelled);
            }
            match $call.await {
                Err(ExportError::Transient(error)) if attempt < max_attempts => {
                    $progress.emit(ProgressEvent::Retrying { name: $name, attempt, error });
                    let delay = policy.base_delay.saturating_mul(1u32 << (attempt - 1).min(16));
                    if !delay.is_zero() {
                        tokio::time::sleep(delay).await;
                    }
                    attempt += 1;
                }
                other => break other,
            }
        }
    }};
}

/// The shared upload driver behind [`ExportPlugin::upload`].
pub async fn run_upload<P: ExportPlugin + ?Sized>(
    plugin: &mut P,
    job: UploadJob,
    progress: &dyn ProgressSink,
    cancel: &CancelToken,
) -> Result<UploadReport, ExportError> {
    if job.album.is_some() && !plugin.info().capabilities.albums {
        return Err(ExportError::AlbumsUnsupported);
    }

    let album: Option<AlbumRef> = match &job.album {
        None => None,
        Some(AlbumTarget::Existing(album)) => Some(album.clone()),
        Some(AlbumTarget::Create(name)) => {
            let album =
                with_retry!(job.retry, cancel, progress, None, plugin.create_album(&job.id, name))?;
            progress.emit(ProgressEvent::AlbumReady { album: album.clone() });
            Some(album)
        }
    };

    let total = job.files.len();
    let mut report = UploadReport { album: album.clone(), files: Vec::with_capacity(total) };
    for (index, file) in job.files.iter().enumerate() {
        if cancel.is_cancelled() {
            return Err(ExportError::Cancelled);
        }
        progress.emit(ProgressEvent::FileStarted { index, total, name: file.name.clone() });
        let outcome = with_retry!(
            job.retry,
            cancel,
            progress,
            Some(file.name.clone()),
            plugin.upload_file(FileUpload {
                job_id: &job.id,
                file,
                album: album.as_ref(),
                options: &job.options,
            })
        )?;
        progress.emit(ProgressEvent::FileDone {
            index,
            total,
            name: file.name.clone(),
            status: outcome.status,
        });
        report.files.push(FileReport {
            content_key: file.content_key.clone(),
            name: file.name.clone(),
            remote_id: outcome.remote_id,
            status: outcome.status,
        });
    }
    Ok(report)
}
