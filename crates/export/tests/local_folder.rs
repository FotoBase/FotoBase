//! TDD tests for TASK-08-008: the local-folder plugin passes the shared
//! contract suite against a temp-dir sandbox, writes atomically, resumes
//! without duplicates and maps albums to sanitized subfolders (PLAN §6 P4).

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use fotobase_export::contract::{run_contract_suite, ContractFixture, Fault, RemoteAlbum};
use fotobase_export::local_folder::{FaultPoints, LocalFolderPlugin};
use fotobase_export::{
    AlbumRef, AlbumTarget, CancelToken, ContentKey, ExportError, ExportPlugin, FileStatus,
    NoProgress, RetryPolicy, UploadFile, UploadJob, UploadOptions,
};

#[derive(Default)]
struct Faults {
    abort_after: Option<usize>,
    fail_once: bool,
    lose_upload_response_once: bool,
    lose_album_response_once: bool,
}

#[derive(Clone, Default)]
struct Injected(Arc<Mutex<Faults>>);

impl FaultPoints for Injected {
    fn before_commit(&self) -> Result<(), ExportError> {
        let mut faults = self.0.lock().unwrap();
        if std::mem::take(&mut faults.fail_once) {
            return Err(ExportError::Transient("connection refused".into()));
        }
        match faults.abort_after {
            Some(0) => return Err(ExportError::Transient("killed mid-upload".into())),
            Some(n) => faults.abort_after = Some(n - 1),
            None => {}
        }
        Ok(())
    }

    fn after_commit(&self) -> Result<(), ExportError> {
        if std::mem::take(&mut self.0.lock().unwrap().lose_upload_response_once) {
            return Err(ExportError::Transient("response lost".into()));
        }
        Ok(())
    }

    fn after_create_album(&self) -> Result<(), ExportError> {
        if std::mem::take(&mut self.0.lock().unwrap().lose_album_response_once) {
            return Err(ExportError::Transient("album response lost".into()));
        }
        Ok(())
    }
}

struct FolderFixture {
    dir: tempfile::TempDir,
    faults: Injected,
}

impl FolderFixture {
    fn new() -> Self {
        Self { dir: tempfile::tempdir().unwrap(), faults: Injected::default() }
    }

    fn root(&self) -> &Path {
        self.dir.path()
    }
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    let mut entries: Vec<_> = fs::read_dir(dir).unwrap().map(|e| e.unwrap().path()).collect();
    entries.sort();
    for path in entries {
        if path.is_dir() {
            walk(&path, out);
        } else {
            out.push(path);
        }
    }
}

fn all_files(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    walk(root, &mut out);
    out
}

fn rel(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap()
        .components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join("/")
}

impl ContractFixture for FolderFixture {
    type Plugin = LocalFolderPlugin;

    fn plugin(&self) -> LocalFolderPlugin {
        LocalFolderPlugin::new(self.root()).with_fault_points(Arc::new(self.faults.clone()))
    }

    fn inject(&self, fault: Fault) {
        let mut faults = self.faults.0.lock().unwrap();
        match fault {
            Fault::AbortUploadsAfter(n) => faults.abort_after = Some(n),
            Fault::FailUploadOnce => faults.fail_once = true,
            Fault::LoseUploadResponseOnce => faults.lose_upload_response_once = true,
            Fault::LoseAlbumResponseOnce => faults.lose_album_response_once = true,
        }
    }

    fn clear_faults(&self) {
        *self.faults.0.lock().unwrap() = Faults::default();
    }

    // Every file on disk, staging leftovers included, one entry per object.
    fn remote_files(&self) -> Vec<ContentKey> {
        all_files(self.root())
            .iter()
            .map(|p| ContentKey::from_reader(fs::File::open(p).unwrap()).unwrap())
            .collect()
    }

    fn remote_albums(&self) -> Vec<RemoteAlbum> {
        let mut albums = Vec::new();
        let mut stack = vec![self.root().to_path_buf()];
        while let Some(dir) = stack.pop() {
            let mut files = Vec::new();
            for entry in fs::read_dir(&dir).unwrap() {
                let path = entry.unwrap().path();
                if path.is_dir() {
                    stack.push(path);
                } else {
                    files.push(ContentKey::from_reader(fs::File::open(path).unwrap()).unwrap());
                }
            }
            if dir != self.root() {
                albums.push(RemoteAlbum { name: rel(self.root(), &dir), files });
            }
        }
        albums
    }
}

fn payloads(dir: &Path, count: usize) -> Vec<UploadFile> {
    (0..count)
        .map(|i| {
            let path = dir.join(format!("IMG_{i}.jpg"));
            fs::write(&path, format!("payload {i}")).unwrap();
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
        retry: RetryPolicy { max_attempts: 2, base_delay: Duration::ZERO },
    }
}

fn names(root: &Path) -> Vec<String> {
    all_files(root).iter().map(|p| rel(root, p)).collect()
}

#[tokio::test]
async fn local_folder_plugin_passes_the_contract_suite() {
    run_contract_suite(FolderFixture::new).await.unwrap();
}

#[tokio::test]
async fn killed_batch_resumes_to_five_files_without_partials() {
    let fx = FolderFixture::new();
    let src = tempfile::tempdir().unwrap();
    let files = payloads(src.path(), 5);

    fx.inject(Fault::AbortUploadsAfter(2));
    let killed =
        fx.plugin().upload(job("j", files.clone(), None), &NoProgress, &CancelToken::new()).await;
    assert!(killed.is_err());
    assert_eq!(names(fx.root()), ["IMG_0.jpg", "IMG_1.jpg"], "no partial file after the kill");

    fx.clear_faults();
    let report =
        fx.plugin().upload(job("j", files, None), &NoProgress, &CancelToken::new()).await.unwrap();
    assert_eq!(report.count(FileStatus::AlreadyPresent), 2);
    assert_eq!(report.count(FileStatus::Uploaded), 3);
    assert_eq!(names(fx.root()), ["IMG_0.jpg", "IMG_1.jpg", "IMG_2.jpg", "IMG_3.jpg", "IMG_4.jpg"]);
}

#[tokio::test]
async fn stale_staging_file_from_a_hard_kill_is_swept_on_resume() {
    let fx = FolderFixture::new();
    let src = tempfile::tempdir().unwrap();
    let files = payloads(src.path(), 1);
    fs::write(fx.root().join(".IMG_0.jpg.fb-part"), b"half").unwrap();

    fx.plugin().upload(job("j", files, None), &NoProgress, &CancelToken::new()).await.unwrap();
    assert_eq!(names(fx.root()), ["IMG_0.jpg"]);
}

#[tokio::test]
async fn rerun_leaves_existing_files_untouched() {
    let fx = FolderFixture::new();
    let src = tempfile::tempdir().unwrap();
    let files = payloads(src.path(), 1);
    let mut plugin = fx.plugin();
    plugin.upload(job("a", files.clone(), None), &NoProgress, &CancelToken::new()).await.unwrap();
    let dest = fx.root().join("IMG_0.jpg");
    let before = fs::metadata(&dest).unwrap().modified().unwrap();

    let again =
        plugin.upload(job("b", files, None), &NoProgress, &CancelToken::new()).await.unwrap();
    assert_eq!(again.files[0].status, FileStatus::AlreadyPresent);
    assert_eq!(fs::metadata(&dest).unwrap().modified().unwrap(), before);
}

#[tokio::test]
async fn same_name_different_content_is_kept_side_by_side_and_stays_idempotent() {
    let fx = FolderFixture::new();
    let a = tempfile::tempdir().unwrap();
    let b = tempfile::tempdir().unwrap();
    fs::write(a.path().join("IMG.jpg"), b"first").unwrap();
    fs::write(b.path().join("IMG.jpg"), b"second").unwrap();
    let both = vec![
        UploadFile::from_path(&a.path().join("IMG.jpg")).unwrap(),
        UploadFile::from_path(&b.path().join("IMG.jpg")).unwrap(),
    ];

    for _ in 0..2 {
        fx.plugin()
            .upload(job("j", both.clone(), None), &NoProgress, &CancelToken::new())
            .await
            .unwrap();
    }

    let found = names(fx.root());
    assert_eq!(found.len(), 2, "{found:?}");
    assert_eq!(fs::read(fx.root().join("IMG.jpg")).unwrap(), b"first");
    let suffixed = found.iter().find(|n| *n != "IMG.jpg").unwrap();
    assert!(suffixed.starts_with("IMG-") && suffixed.ends_with(".jpg"), "{suffixed}");
    assert_eq!(fs::read(fx.root().join(suffixed)).unwrap(), b"second");
}

#[tokio::test]
async fn album_names_map_to_nested_sanitized_subfolders() {
    let fx = FolderFixture::new();
    let src = tempfile::tempdir().unwrap();
    let report = fx
        .plugin()
        .upload(
            job(
                "j",
                payloads(src.path(), 1),
                Some(AlbumTarget::Create("Trips/2024: Rome?/../CON".into())),
            ),
            &NoProgress,
            &CancelToken::new(),
        )
        .await
        .unwrap();

    assert_eq!(names(fx.root()), ["Trips/2024_ Rome_/_/_CON/IMG_0.jpg"]);
    let album = report.album.unwrap();
    assert_eq!(album.id, "Trips/2024_ Rome_/_/_CON");
    assert_eq!(report.files[0].remote_id, "Trips/2024_ Rome_/_/_CON/IMG_0.jpg");
}

#[tokio::test]
async fn existing_album_ref_cannot_escape_the_destination_root() {
    let fx = FolderFixture::new();
    let outer = fx.root().join("dest");
    fs::create_dir(&outer).unwrap();
    let src = tempfile::tempdir().unwrap();
    let mut plugin = LocalFolderPlugin::new(&outer);

    plugin
        .upload(
            job(
                "j",
                payloads(src.path(), 1),
                Some(AlbumTarget::Existing(AlbumRef { id: "../../evil".into(), name: "x".into() })),
            ),
            &NoProgress,
            &CancelToken::new(),
        )
        .await
        .unwrap();

    assert_eq!(names(fx.root()), ["dest/_/_/evil/IMG_0.jpg"]);
}

#[tokio::test]
async fn list_albums_reports_subfolders_and_ignores_staging_dirs() {
    let fx = FolderFixture::new();
    let mut plugin = fx.plugin();
    assert!(plugin.list_albums().await.unwrap().is_empty());
    plugin.create_album("j1", "Holiday/Day 1").await.unwrap();
    plugin.create_album("j2", "Family").await.unwrap();
    fs::create_dir(fx.root().join(".hidden")).unwrap();

    let ids: Vec<_> = plugin.list_albums().await.unwrap().into_iter().map(|a| a.id).collect();
    assert_eq!(ids, ["Family", "Holiday", "Holiday/Day 1"]);
}

#[tokio::test]
async fn file_names_are_sanitized_to_a_single_path_component() {
    let fx = FolderFixture::new();
    let src = tempfile::tempdir().unwrap();
    fs::write(src.path().join("a.jpg"), b"x").unwrap();
    let mut file = UploadFile::from_path(&src.path().join("a.jpg")).unwrap();
    file.name = "../sub/na:me.jpg".into();

    fx.plugin().upload(job("j", vec![file], None), &NoProgress, &CancelToken::new()).await.unwrap();
    assert_eq!(names(fx.root()), ["_sub_na_me.jpg"]);
}
