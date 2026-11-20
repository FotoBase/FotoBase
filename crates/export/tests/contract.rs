//! TDD tests for TASK-08-007: the shared contract suite (upload, resume,
//! retry, idempotency, album mapping) and the upload driver (PLAN §6 P4
//! acceptance, §9 "Shared contract tests per plugin").

use std::fs;
use std::sync::Mutex;

use fotobase_export::contract::{run_contract_suite, ContractFixture, Fault};
use fotobase_export::mock::{MockBehavior, MockFixture};
use fotobase_export::{
    AlbumTarget, CancelToken, ContentKey, ExportError, ExportPlugin, FileOutcome, FileStatus,
    FileUpload, NoProgress, PluginCapabilities, PluginInfo, ProgressEvent, RetryPolicy, UploadFile,
    UploadJob, UploadOptions,
};

fn files(dir: &std::path::Path, count: usize) -> Vec<UploadFile> {
    (0..count)
        .map(|i| {
            let path = dir.join(format!("p{i}.jpg"));
            fs::write(&path, format!("bytes-{i}")).unwrap();
            UploadFile::from_path(&path).unwrap()
        })
        .collect()
}

fn job(id: &str, files: Vec<UploadFile>, album: Option<AlbumTarget>) -> UploadJob {
    UploadJob {
        id: id.into(),
        files,
        album,
        options: UploadOptions::default(),
        retry: RetryPolicy { max_attempts: 2, base_delay: std::time::Duration::ZERO },
    }
}

#[tokio::test]
async fn mock_plugin_passes_the_contract_suite() {
    run_contract_suite(|| MockFixture::new(MockBehavior::default())).await.unwrap();
}

#[tokio::test]
async fn plugin_without_content_key_dedupe_fails_the_suite() {
    let failure = run_contract_suite(|| {
        MockFixture::new(MockBehavior { dedupe: false, ..Default::default() })
    })
    .await
    .unwrap_err();
    assert!(
        ["reupload_dedupes", "resume_after_interrupt", "lost_response_no_duplicate"]
            .contains(&failure.check),
        "unexpected check: {failure:?}"
    );
}

#[tokio::test]
async fn plugin_with_non_idempotent_album_create_fails_the_suite() {
    let failure = run_contract_suite(|| {
        MockFixture::new(MockBehavior { idempotent_album: false, ..Default::default() })
    })
    .await
    .unwrap_err();
    assert_eq!(failure.check, "album_create_retry_single_album");
}

#[tokio::test]
async fn plugin_that_leaves_partial_files_fails_the_suite() {
    let failure = run_contract_suite(|| {
        MockFixture::new(MockBehavior { atomic_upload: false, ..Default::default() })
    })
    .await
    .unwrap_err();
    assert_eq!(failure.check, "resume_after_interrupt");
}

#[test]
fn content_key_is_stable_per_content_not_per_path() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("a.jpg"), b"same").unwrap();
    fs::write(dir.path().join("b.jpg"), b"same").unwrap();
    fs::write(dir.path().join("c.jpg"), b"different").unwrap();
    let a = UploadFile::from_path(&dir.path().join("a.jpg")).unwrap();
    let b = UploadFile::from_path(&dir.path().join("b.jpg")).unwrap();
    let c = UploadFile::from_path(&dir.path().join("c.jpg")).unwrap();
    assert_eq!(a.content_key, b.content_key);
    assert_ne!(a.content_key, c.content_key);
    assert_eq!(a.content_key, ContentKey::from_bytes(b"same"));
    assert_eq!(a.name, "a.jpg");
}

#[tokio::test]
async fn retried_reupload_of_identical_file_is_deduped() {
    let dir = tempfile::tempdir().unwrap();
    let fixture = MockFixture::new(MockBehavior::default());
    let mut plugin = fixture.plugin();
    let batch = files(dir.path(), 1);
    let first = plugin
        .upload(job("j1", batch.clone(), None), &NoProgress, &CancelToken::new())
        .await
        .unwrap();
    let second =
        plugin.upload(job("j2", batch, None), &NoProgress, &CancelToken::new()).await.unwrap();
    assert_eq!(first.files[0].status, FileStatus::Uploaded);
    assert_eq!(second.files[0].status, FileStatus::AlreadyPresent);
    assert_eq!(first.files[0].remote_id, second.files[0].remote_id);
    assert_eq!(fixture.remote_files().len(), 1);
}

#[tokio::test]
async fn emits_per_file_progress_in_order() {
    let dir = tempfile::tempdir().unwrap();
    let fixture = MockFixture::new(MockBehavior::default());
    let events = Mutex::new(Vec::new());
    let sink = |e: ProgressEvent| events.lock().unwrap().push(e);
    fixture
        .plugin()
        .upload(job("j", files(dir.path(), 2), None), &sink, &CancelToken::new())
        .await
        .unwrap();

    let events = events.into_inner().unwrap();
    let shape: Vec<_> = events
        .iter()
        .map(|e| match e {
            ProgressEvent::FileStarted { index, total, .. } => format!("start {index}/{total}"),
            ProgressEvent::FileDone { index, total, status, .. } => {
                format!("done {index}/{total} {status:?}")
            }
            other => format!("{other:?}"),
        })
        .collect();
    assert_eq!(shape, ["start 0/2", "done 0/2 Uploaded", "start 1/2", "done 1/2 Uploaded"]);
}

#[tokio::test]
async fn retry_policy_gives_up_after_max_attempts_with_transient_error() {
    let dir = tempfile::tempdir().unwrap();
    let fixture = MockFixture::new(MockBehavior::default());
    fixture.inject(Fault::AbortUploadsAfter(0));
    let err = fixture
        .plugin()
        .upload(job("j", files(dir.path(), 1), None), &NoProgress, &CancelToken::new())
        .await
        .unwrap_err();
    assert!(matches!(err, ExportError::Transient(_)), "{err:?}");
    assert_eq!(fixture.upload_attempts(), 2, "max_attempts = 2");
}

#[tokio::test]
async fn fatal_errors_are_not_retried() {
    let dir = tempfile::tempdir().unwrap();
    let mut plugin = FatalPlugin { attempts: 0 };
    let err = plugin
        .upload(job("j", files(dir.path(), 1), None), &NoProgress, &CancelToken::new())
        .await
        .unwrap_err();
    assert!(matches!(err, ExportError::Fatal(_)), "{err:?}");
    assert_eq!(plugin.attempts, 1);
}

struct FatalPlugin {
    attempts: usize,
}

#[async_trait::async_trait]
impl ExportPlugin for FatalPlugin {
    fn info(&self) -> PluginInfo {
        PluginInfo {
            id: "fatal".into(),
            name: "Fatal".into(),
            capabilities: PluginCapabilities::default(),
        }
    }

    async fn upload_file(&mut self, _request: FileUpload<'_>) -> Result<FileOutcome, ExportError> {
        self.attempts += 1;
        Err(ExportError::Fatal("bad credentials".into()))
    }
}

#[tokio::test]
async fn album_target_on_plugin_without_album_support_is_rejected_before_uploading() {
    let dir = tempfile::tempdir().unwrap();
    let fixture = MockFixture::new(MockBehavior { albums: false, ..Default::default() });
    let err = fixture
        .plugin()
        .upload(
            job("j", files(dir.path(), 1), Some(AlbumTarget::Create("Trip".into()))),
            &NoProgress,
            &CancelToken::new(),
        )
        .await
        .unwrap_err();
    assert!(matches!(err, ExportError::AlbumsUnsupported));
    assert!(fixture.remote_files().is_empty());
}
