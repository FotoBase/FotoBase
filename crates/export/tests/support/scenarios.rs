//! Scenarios that only make sense against a real S3 protocol implementation;
//! the mock and the RustFS container both run them.
#![allow(dead_code)]

use std::fs;
use std::path::Path;
use std::time::Duration;

use fotobase_export::{
    AlbumTarget, CancelToken, ExportError, ExportPlugin, FileStatus, NoProgress, Registry,
    RetryPolicy, UploadFile, UploadJob, UploadOptions,
};

use fotobase_export::contract::ContractFixture;
use fotobase_export::publish::PublishToken;

use super::{Endpoint, S3Fixture, PREFIX};
use url::Url;

const MIB: u64 = 1024 * 1024;

/// Deterministic, non-repeating-per-part bytes.
pub fn big_file(dir: &Path, name: &str, len: u64) -> UploadFile {
    let mut state = 0x2545_f491_4f6c_dd1du64;
    let bytes: Vec<u8> = (0..len)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            (state >> 32) as u8
        })
        .collect();
    let path = dir.join(name);
    fs::write(&path, bytes).unwrap();
    UploadFile::from_path(&path).unwrap()
}

pub fn job(
    id: &str,
    files: Vec<UploadFile>,
    album: Option<AlbumTarget>,
    attempts: u32,
) -> UploadJob {
    UploadJob {
        id: id.into(),
        files,
        album,
        options: UploadOptions::default(),
        retry: RetryPolicy { max_attempts: attempts, base_delay: Duration::ZERO },
    }
}

fn parts_sent(fx: &S3Fixture) -> Vec<u16> {
    std::mem::take(&mut fx.faults.0.lock().unwrap().parts_sent)
}

pub async fn static_site_publish_replaces_and_prunes_exact_paths(endpoint: &Endpoint) {
    let fx = S3Fixture::new(endpoint, false);
    let source = tempfile::tempdir().unwrap();
    let token = PublishToken::parse("abcdef0123456789abcdef0123456789").unwrap();
    let base_url = Url::parse("https://photos.example/").unwrap();
    fs::create_dir_all(source.path().join("images")).unwrap();
    fs::write(source.path().join("index.html"), b"v1").unwrap();
    fs::write(source.path().join("images/old.jpg"), b"old").unwrap();
    fs::write(source.path().join("images/unchanged.jpg"), b"same").unwrap();
    let mut registry = Registry::new();
    registry.register(Box::new(fx.plugin())).unwrap();

    registry.publish_directory("s3", source.path(), &base_url, &token).await.unwrap();
    let prefix = format!("{PREFIX}/{}/", token.as_str());
    assert_eq!(fx.admin.get(&format!("{prefix}images/old.jpg")), b"old");

    fs::write(source.path().join("index.html"), b"v2").unwrap();
    fs::remove_file(source.path().join("images/old.jpg")).unwrap();
    fs::write(source.path().join("images/new.jpg"), b"new").unwrap();
    let report = registry.publish_directory("s3", source.path(), &base_url, &token).await.unwrap();

    assert_eq!((report.uploaded, report.unchanged, report.removed), (2, 1, 1));
    let keys: Vec<_> = fx.admin.objects().into_iter().map(|(key, _)| key).collect();
    assert!(keys.contains(&format!("{prefix}index.html")));
    assert!(keys.contains(&format!("{prefix}images/new.jpg")));
    assert!(keys.contains(&format!("{prefix}images/unchanged.jpg")));
    assert!(!keys.contains(&format!("{prefix}images/old.jpg")));
    assert_eq!(fx.admin.get(&format!("{prefix}index.html")), b"v2");
}

/// Kill mid-multipart → resume from a fresh plugin: only the missing part is
/// sent, nothing partial is ever visible, one complete object results.
pub async fn multipart_kill_then_resume(endpoint: &Endpoint) {
    let fx = S3Fixture::new(endpoint, false);
    let dir = tempfile::tempdir().unwrap();
    let file = big_file(dir.path(), "big.jpg", 12 * MIB + 123);
    let config = fx.config().with_part_size(5 * MIB).unwrap();
    let upload = job("big", vec![file.clone()], None, 1);

    fx.faults.0.lock().unwrap().transient_before_part = Some(3);
    let killed = fx
        .plugin_with(config.clone())
        .upload(upload.clone(), &NoProgress, &CancelToken::new())
        .await;
    assert!(matches!(killed, Err(ExportError::Transient(_))), "{killed:?}");
    assert_eq!(parts_sent(&fx), [1, 2, 3]);
    assert!(fx.admin.objects().is_empty(), "an unfinished upload must not be visible");
    assert_eq!(fx.admin.open_uploads(), 1, "the unfinished upload is kept for resuming");

    fx.faults.0.lock().unwrap().transient_before_part = None;
    let report =
        fx.plugin_with(config).upload(upload, &NoProgress, &CancelToken::new()).await.unwrap();
    assert_eq!(report.count(FileStatus::Uploaded), 1);
    assert_eq!(parts_sent(&fx), [3], "parts 1 and 2 were already stored");

    let objects = fx.admin.objects();
    assert_eq!(objects.len(), 1, "{objects:?}");
    assert_eq!(objects[0].1, 12 * MIB + 123);
    assert_eq!(fx.admin.get(&objects[0].0), fs::read(&file.path).unwrap());
    assert_eq!(fx.admin.open_uploads(), 0);
}

/// Parts stored with another part size cannot be reused; they are replaced,
/// not mixed into the object.
pub async fn multipart_resume_with_new_part_size(endpoint: &Endpoint) {
    let fx = S3Fixture::new(endpoint, false);
    let dir = tempfile::tempdir().unwrap();
    let file = big_file(dir.path(), "big.jpg", 12 * MIB + 123);
    let upload = job("big", vec![file.clone()], None, 1);

    fx.faults.0.lock().unwrap().transient_before_part = Some(3);
    let first = fx
        .plugin_with(fx.config().with_part_size(5 * MIB).unwrap())
        .upload(upload.clone(), &NoProgress, &CancelToken::new())
        .await;
    assert!(first.is_err());
    parts_sent(&fx);

    fx.faults.0.lock().unwrap().transient_before_part = None;
    fx.plugin_with(fx.config().with_part_size(6 * MIB).unwrap())
        .upload(upload, &NoProgress, &CancelToken::new())
        .await
        .unwrap();
    assert_eq!(parts_sent(&fx), [1, 2, 3]);
    let objects = fx.admin.objects();
    assert_eq!(objects.len(), 1);
    assert_eq!(fx.admin.get(&objects[0].0), fs::read(&file.path).unwrap());
    assert_eq!(fx.admin.open_uploads(), 0);
}

/// A failure retrying cannot fix aborts the multipart upload.
pub async fn multipart_fatal_failure_aborts(endpoint: &Endpoint) {
    let fx = S3Fixture::new(endpoint, false);
    let dir = tempfile::tempdir().unwrap();
    let file = big_file(dir.path(), "big.jpg", 11 * MIB);
    let config = fx.config().with_part_size(5 * MIB).unwrap();

    fx.faults.0.lock().unwrap().fatal_before_part = Some(2);
    let result = fx
        .plugin_with(config)
        .upload(job("big", vec![file], None, 3), &NoProgress, &CancelToken::new())
        .await;
    assert!(matches!(result, Err(ExportError::Fatal(_))), "{result:?}");
    assert!(fx.admin.objects().is_empty());
    assert_eq!(fx.admin.open_uploads(), 0, "a fatal failure must not leave an upload behind");
}

/// Object keys are `<prefix>/<album>/<content-key>_<filename>`, albums are
/// prefixes, and read-back sees what was uploaded.
pub async fn key_layout_and_read_back(endpoint: &Endpoint) {
    let fx = S3Fixture::new(endpoint, false);
    let dir = tempfile::tempdir().unwrap();
    let in_album = big_file(dir.path(), "in album.jpg", 1000);
    let loose = big_file(dir.path(), "loose.png", 2000);
    let mut plugin = fx.plugin();

    let report = plugin
        .upload(
            job("a", vec![in_album.clone()], Some(AlbumTarget::Create("Trips/Rome".into())), 1),
            &NoProgress,
            &CancelToken::new(),
        )
        .await
        .unwrap();
    plugin
        .upload(job("b", vec![loose.clone()], None, 1), &NoProgress, &CancelToken::new())
        .await
        .unwrap();

    let mut keys: Vec<String> = fx.admin.objects().into_iter().map(|(k, _)| k).collect();
    keys.sort();
    let mut expected = vec![
        format!("{PREFIX}/Trips/Rome/{}_in album.jpg", in_album.content_key),
        format!("{PREFIX}/{}_loose.png", loose.content_key),
    ];
    expected.sort();
    assert_eq!(keys, expected);

    let album = report.album.unwrap();
    assert_eq!(album.name, "Trips/Rome");
    assert_eq!(plugin.list_albums().await.unwrap(), std::slice::from_ref(&album));

    let files = plugin.list_files(Some(&album)).await.unwrap();
    assert_eq!(files.len(), 1);
    assert_eq!(files[0].name, "in album.jpg");
    assert_eq!(files[0].content_key.as_ref(), Some(&in_album.content_key));
    assert_eq!(plugin.list_files(None).await.unwrap().len(), 2);
    assert!(plugin.exists(&in_album.content_key, Some(&album)).await.unwrap());
    assert!(!plugin.exists(&in_album.content_key, None).await.unwrap());
}

/// Same key, wrong size (a leftover of an older failed write) is replaced;
/// the same key with the right size is left alone.
pub async fn retry_overwrites_the_same_key(endpoint: &Endpoint) {
    let fx = S3Fixture::new(endpoint, false);
    let dir = tempfile::tempdir().unwrap();
    let file = big_file(dir.path(), "a.jpg", 3000);
    let key = format!("{PREFIX}/{}_a.jpg", file.content_key);
    fx.admin.put(&key, b"stale".to_vec());

    let report = fx
        .plugin()
        .upload(job("a", vec![file.clone()], None, 1), &NoProgress, &CancelToken::new())
        .await
        .unwrap();
    assert_eq!(report.count(FileStatus::Uploaded), 1);
    assert_eq!(fx.admin.get(&key), fs::read(&file.path).unwrap());

    let again = fx
        .plugin()
        .upload(job("a2", vec![file], None, 1), &NoProgress, &CancelToken::new())
        .await
        .unwrap();
    assert_eq!(again.count(FileStatus::AlreadyPresent), 1);
}
