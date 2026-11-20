//! The shared contract suite every export plugin must pass (PLAN §6 P4
//! acceptance, §9 "Shared contract tests per plugin"): upload, resume,
//! retry, idempotency, album mapping, cancellation.
//!
//! A plugin's test file wires its **sandboxed local endpoint** (a mock
//! server, a temp folder…) behind [`ContractFixture`] and calls
//! [`run_contract_suite`]:
//!
//! ```ignore
//! #[tokio::test]
//! async fn passes_contract_suite() {
//!     run_contract_suite(|| MyFixture::new()).await.unwrap();
//! }
//! ```
//!
//! Each check gets a fresh fixture, so state never leaks between checks.

use std::collections::BTreeSet;
use std::fs;
use std::path::Path;
use std::sync::Mutex;
use std::time::Duration;

use crate::{
    AlbumTarget, CancelToken, ContentKey, ExportError, ExportPlugin, FileStatus, NoProgress,
    ProgressEvent, RetryPolicy, UploadFile, UploadJob, UploadOptions,
};

/// A failure the sandboxed endpoint can be told to simulate. Faults trigger
/// on the plugin's *next* matching request(s) and stay armed until consumed
/// or [`ContractFixture::clear_faults`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fault {
    /// Let `n` more uploads commit, then drop the connection mid-upload on
    /// every further attempt (a killed upload). Nothing of an aborted upload
    /// may remain on the target.
    AbortUploadsAfter(usize),
    /// Fail the next upload attempt before anything is stored.
    FailUploadOnce,
    /// Store the next upload, then lose the response (the client sees a
    /// transient error although the file landed).
    LoseUploadResponseOnce,
    /// Create the next album, then lose the response.
    LoseAlbumResponseOnce,
}

/// An album as stored on the sandboxed target.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteAlbum {
    pub name: String,
    pub files: Vec<ContentKey>,
}

/// A sandboxed endpoint plus a way to build plugin instances against it.
pub trait ContractFixture {
    type Plugin: ExportPlugin;

    /// A fresh plugin instance on the same endpoint state (also how the
    /// suite simulates an app restart between an interrupted run and its
    /// resume).
    fn plugin(&self) -> Self::Plugin;
    fn inject(&self, fault: Fault);
    fn clear_faults(&self);
    /// Every file object on the target, one entry per stored object
    /// (duplicates and partial files show up as extra entries).
    fn remote_files(&self) -> Vec<ContentKey>;
    fn remote_albums(&self) -> Vec<RemoteAlbum>;
}

/// A failed contract check.
#[derive(Debug)]
pub struct ContractFailure {
    pub check: &'static str,
    pub reason: String,
}

impl std::fmt::Display for ContractFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "contract check `{}` failed: {}", self.check, self.reason)
    }
}

impl std::error::Error for ContractFailure {}

type Check = Result<(), String>;

/// Runs every check against a fresh fixture from `make`; stops at the first
/// failure.
pub async fn run_contract_suite<F, M>(make: M) -> Result<(), ContractFailure>
where
    F: ContractFixture,
    M: Fn() -> F,
{
    macro_rules! check {
        ($name:literal, $f:ident) => {
            $f(&make()).await.map_err(|reason| ContractFailure { check: $name, reason })?
        };
    }
    check!("upload_basic", upload_basic);
    check!("resume_after_interrupt", resume_after_interrupt);
    check!("retry_transient_failure", retry_transient_failure);
    check!("lost_response_no_duplicate", lost_response_no_duplicate);
    check!("reupload_dedupes", reupload_dedupes);
    check!("album_mapping", album_mapping);
    check!("album_create_retry_single_album", album_create_retry_single_album);
    check!("album_rerun_single_album", album_rerun_single_album);
    check!("cancel_then_resume", cancel_then_resume);
    Ok(())
}

fn ensure(cond: bool, msg: impl FnOnce() -> String) -> Check {
    if cond {
        Ok(())
    } else {
        Err(msg())
    }
}

fn batch(dir: &Path, tag: &str, count: usize) -> Vec<UploadFile> {
    (0..count)
        .map(|i| {
            let path = dir.join(format!("{tag}-{i}.jpg"));
            fs::write(&path, format!("fotobase contract payload {tag} {i}"))
                .expect("writing contract file");
            UploadFile::from_path(&path).expect("hashing contract file")
        })
        .collect()
}

fn job(id: &str, files: &[UploadFile], album: Option<AlbumTarget>, max_attempts: u32) -> UploadJob {
    UploadJob {
        id: id.into(),
        files: files.to_vec(),
        album,
        options: UploadOptions::default(),
        retry: RetryPolicy { max_attempts, base_delay: Duration::ZERO },
    }
}

fn keys(files: &[UploadFile]) -> BTreeSet<ContentKey> {
    files.iter().map(|f| f.content_key.clone()).collect()
}

/// The target holds exactly `expected`: each key once, nothing partial.
fn ensure_remote_is<F: ContractFixture>(
    fx: &F,
    expected: &BTreeSet<ContentKey>,
    when: &str,
) -> Check {
    let remote = fx.remote_files();
    let set: BTreeSet<_> = remote.iter().cloned().collect();
    ensure(remote.len() == set.len() && &set == expected, || {
        format!(
            "{when}: target should hold exactly {} distinct files but holds {} objects ({} distinct)",
            expected.len(),
            remote.len(),
            set.len()
        )
    })
}

fn tempdir() -> tempfile::TempDir {
    tempfile::tempdir().expect("creating contract temp dir")
}

fn err(e: ExportError) -> String {
    e.to_string()
}

async fn upload_basic<F: ContractFixture>(fx: &F) -> Check {
    let dir = tempdir();
    let files = batch(dir.path(), "basic", 3);
    let events = Mutex::new(Vec::new());
    let sink = |e: ProgressEvent| events.lock().unwrap().push(e);
    let report = fx
        .plugin()
        .upload(job("basic", &files, None, 2), &sink, &CancelToken::new())
        .await
        .map_err(err)?;
    ensure(report.count(FileStatus::Uploaded) == 3, || {
        "all three files should report Uploaded".into()
    })?;
    ensure_remote_is(fx, &keys(&files), "after upload")?;
    let events = events.into_inner().unwrap();
    let started = events.iter().filter(|e| matches!(e, ProgressEvent::FileStarted { .. })).count();
    let done = events.iter().filter(|e| matches!(e, ProgressEvent::FileDone { .. })).count();
    ensure(started == 3 && done == 3, || {
        format!("expected 3 started + 3 done progress events, got {started} + {done}")
    })
}

/// Kill mid-upload → resume (even from a new plugin instance) → no duplicates.
async fn resume_after_interrupt<F: ContractFixture>(fx: &F) -> Check {
    let dir = tempdir();
    let files = batch(dir.path(), "resume", 5);
    let resumable = job("resume", &files, None, 2);

    fx.inject(Fault::AbortUploadsAfter(2));
    let interrupted = fx.plugin().upload(resumable.clone(), &NoProgress, &CancelToken::new()).await;
    ensure(interrupted.is_err(), || {
        "a job whose connection keeps dropping must fail, not succeed".into()
    })?;

    fx.clear_faults();
    let report =
        fx.plugin().upload(resumable, &NoProgress, &CancelToken::new()).await.map_err(err)?;
    ensure_remote_is(fx, &keys(&files), "after resume")?;
    ensure(
        report.count(FileStatus::AlreadyPresent) == 2 && report.count(FileStatus::Uploaded) == 3,
        || {
            format!(
            "resume should skip the 2 committed files and upload 3, got {} present / {} uploaded",
            report.count(FileStatus::AlreadyPresent),
            report.count(FileStatus::Uploaded)
        )
        },
    )
}

async fn retry_transient_failure<F: ContractFixture>(fx: &F) -> Check {
    let dir = tempdir();
    let files = batch(dir.path(), "retry", 3);
    fx.inject(Fault::FailUploadOnce);
    let retries = Mutex::new(0usize);
    let sink = |e: ProgressEvent| {
        if matches!(e, ProgressEvent::Retrying { .. }) {
            *retries.lock().unwrap() += 1;
        }
    };
    fx.plugin()
        .upload(job("retry", &files, None, 2), &sink, &CancelToken::new())
        .await
        .map_err(err)?;
    ensure(*retries.lock().unwrap() == 1, || {
        "a single transient failure should produce exactly one retry".into()
    })?;
    ensure_remote_is(fx, &keys(&files), "after retry")
}

/// The first attempt landed but its response was lost: the retry must be
/// recognised by content key, not stored a second time.
async fn lost_response_no_duplicate<F: ContractFixture>(fx: &F) -> Check {
    let dir = tempdir();
    let files = batch(dir.path(), "lost", 3);
    fx.inject(Fault::LoseUploadResponseOnce);
    fx.plugin()
        .upload(job("lost", &files, None, 2), &NoProgress, &CancelToken::new())
        .await
        .map_err(err)?;
    ensure_remote_is(fx, &keys(&files), "after retry of a lost response")
}

async fn reupload_dedupes<F: ContractFixture>(fx: &F) -> Check {
    let dir = tempdir();
    let files = batch(dir.path(), "dedupe", 2);
    let mut plugin = fx.plugin();
    plugin
        .upload(job("first", &files, None, 2), &NoProgress, &CancelToken::new())
        .await
        .map_err(err)?;
    let again = plugin
        .upload(job("second", &files, None, 2), &NoProgress, &CancelToken::new())
        .await
        .map_err(err)?;
    ensure(again.count(FileStatus::AlreadyPresent) == 2, || {
        "re-uploading identical content should report AlreadyPresent".into()
    })?;
    ensure_remote_is(fx, &keys(&files), "after re-upload")
}

async fn album_mapping<F: ContractFixture>(fx: &F) -> Check {
    let dir = tempdir();
    let first = batch(dir.path(), "album-a", 3);
    let mut plugin = fx.plugin();
    if !plugin.info().capabilities.albums {
        let refused = plugin
            .upload(
                job("album", &first, Some(AlbumTarget::Create("Holiday".into())), 2),
                &NoProgress,
                &CancelToken::new(),
            )
            .await;
        ensure(matches!(refused, Err(ExportError::AlbumsUnsupported)), || {
            "a plugin without album support must refuse album targets".into()
        })?;
        return ensure(fx.remote_files().is_empty(), || {
            "a refused job must not upload anything".into()
        });
    }

    let report = plugin
        .upload(
            job("album", &first, Some(AlbumTarget::Create("Holiday".into())), 2),
            &NoProgress,
            &CancelToken::new(),
        )
        .await
        .map_err(err)?;
    let album = report.album.ok_or("report should carry the created album")?;
    ensure(album.name == "Holiday", || {
        format!("album named {:?}, expected \"Holiday\"", album.name)
    })?;
    let listed = plugin.list_albums().await.map_err(err)?;
    ensure(listed.contains(&album), || "list_albums should include the created album".into())?;

    let more = batch(dir.path(), "album-b", 2);
    plugin
        .upload(
            job("album-more", &more, Some(AlbumTarget::Existing(album)), 2),
            &NoProgress,
            &CancelToken::new(),
        )
        .await
        .map_err(err)?;

    let albums = fx.remote_albums();
    ensure(albums.len() == 1, || format!("expected exactly one album, found {}", albums.len()))?;
    let members: BTreeSet<_> = albums[0].files.iter().cloned().collect();
    let all: BTreeSet<_> = keys(&first).union(&keys(&more)).cloned().collect();
    ensure(
        albums[0].name == "Holiday" && albums[0].files.len() == members.len() && members == all,
        || "album should hold every uploaded file exactly once".into(),
    )?;
    ensure_remote_is(fx, &all, "after album uploads")
}

/// Album created server-side but the response was lost: the retry must find
/// that album, not create a second.
async fn album_create_retry_single_album<F: ContractFixture>(fx: &F) -> Check {
    let mut plugin = fx.plugin();
    if !plugin.info().capabilities.albums {
        return Ok(());
    }
    let dir = tempdir();
    let files = batch(dir.path(), "album-retry", 2);
    fx.inject(Fault::LoseAlbumResponseOnce);
    plugin
        .upload(
            job("album-retry", &files, Some(AlbumTarget::Create("Trip".into())), 2),
            &NoProgress,
            &CancelToken::new(),
        )
        .await
        .map_err(err)?;
    ensure_single_album(fx, "Trip", &keys(&files))
}

/// The job gave up (retries exhausted) after the album was created; re-running
/// the same job must reuse that album — no partial albums left behind.
async fn album_rerun_single_album<F: ContractFixture>(fx: &F) -> Check {
    let mut plugin = fx.plugin();
    if !plugin.info().capabilities.albums {
        return Ok(());
    }
    let dir = tempdir();
    let files = batch(dir.path(), "album-rerun", 2);
    let rerun = job("album-rerun", &files, Some(AlbumTarget::Create("Trip".into())), 1);
    fx.inject(Fault::LoseAlbumResponseOnce);
    let first = plugin.upload(rerun.clone(), &NoProgress, &CancelToken::new()).await;
    ensure(first.is_err(), || {
        "with no retries left the lost album response should fail the job".into()
    })?;
    fx.clear_faults();
    fx.plugin().upload(rerun, &NoProgress, &CancelToken::new()).await.map_err(err)?;
    ensure_single_album(fx, "Trip", &keys(&files))
}

fn ensure_single_album<F: ContractFixture>(
    fx: &F,
    name: &str,
    expected: &BTreeSet<ContentKey>,
) -> Check {
    let albums = fx.remote_albums();
    ensure(albums.len() == 1, || format!("expected exactly one album, found {}", albums.len()))?;
    let members: BTreeSet<_> = albums[0].files.iter().cloned().collect();
    ensure(
        albums[0].name == name && albums[0].files.len() == members.len() && &members == expected,
        || "the single album should hold every job file exactly once".into(),
    )
}

async fn cancel_then_resume<F: ContractFixture>(fx: &F) -> Check {
    let dir = tempdir();
    let files = batch(dir.path(), "cancel", 3);
    let cancellable = job("cancel", &files, None, 2);

    let cancelled = CancelToken::new();
    cancelled.cancel();
    let early = fx.plugin().upload(cancellable.clone(), &NoProgress, &cancelled).await;
    ensure(matches!(early, Err(ExportError::Cancelled)), || {
        "a cancelled job must return Cancelled".into()
    })?;
    ensure(fx.remote_files().is_empty(), || "a job cancelled up front must upload nothing".into())?;

    let token = CancelToken::new();
    let sink = |e: ProgressEvent| {
        if matches!(e, ProgressEvent::FileDone { .. }) {
            token.cancel();
        }
    };
    let midway = fx.plugin().upload(cancellable.clone(), &sink, &token).await;
    ensure(matches!(midway, Err(ExportError::Cancelled)), || {
        "cancelling mid-job must stop it".into()
    })?;
    ensure(fx.remote_files().len() == 1, || {
        "cancelling after the first file must leave exactly that file".into()
    })?;

    fx.plugin().upload(cancellable, &NoProgress, &CancelToken::new()).await.map_err(err)?;
    ensure_remote_is(fx, &keys(&files), "after resuming a cancelled job")
}
