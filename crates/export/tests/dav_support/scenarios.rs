//! Scenarios beyond the shared contract suite; the mock server and the
//! Apache `mod_dav` container both run them.
#![allow(dead_code)]

use std::fs;
use std::path::Path;
use std::time::Duration;

use fotobase_export::{
    AlbumTarget, CancelToken, ExportError, ExportPlugin, FileStatus, NoProgress, RetryPolicy,
    UploadFile, UploadJob, UploadOptions,
};

use fotobase_export::contract::ContractFixture;
use fotobase_export::publish::PublishToken;
use fotobase_export::webdav::{WebDavConfig, WebDavPlugin};

use super::{DavFixture, Endpoint};
use fotobase_export::Registry;
use url::Url;

pub fn file(dir: &Path, name: &str, content: &str) -> UploadFile {
    let path = dir.join(name);
    fs::write(&path, content).unwrap();
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

pub async fn static_site_publish_replaces_and_prunes_exact_paths(endpoint: &Endpoint) {
    let fx = DavFixture::new(endpoint);
    let source = tempfile::tempdir().unwrap();
    let token = PublishToken::parse("abcdef0123456789abcdef0123456789").unwrap();
    let base_url = Url::parse("https://photos.example/").unwrap();
    fs::create_dir_all(source.path().join("images")).unwrap();
    fs::write(source.path().join("index.html"), b"v1").unwrap();
    fs::write(source.path().join("images/old.jpg"), b"old").unwrap();
    fs::write(source.path().join("images/unchanged.jpg"), b"same").unwrap();
    let mut registry = Registry::new();
    registry.register(Box::new(fx.plugin())).unwrap();

    registry.publish_directory("webdav", source.path(), &base_url, &token).await.unwrap();
    let prefix = token.as_str();
    assert_eq!(fx.admin.get(&format!("{prefix}/images/old.jpg")), b"old");

    fs::write(source.path().join("index.html"), b"v2").unwrap();
    fs::remove_file(source.path().join("images/old.jpg")).unwrap();
    fs::write(source.path().join("images/new.jpg"), b"new").unwrap();
    let report =
        registry.publish_directory("webdav", source.path(), &base_url, &token).await.unwrap();

    assert_eq!((report.uploaded, report.unchanged, report.removed), (2, 1, 1));
    let files = fx.admin.files();
    assert!(files.contains(&format!("{prefix}/index.html")));
    assert!(files.contains(&format!("{prefix}/images/new.jpg")));
    assert!(files.contains(&format!("{prefix}/images/unchanged.jpg")));
    assert!(!files.contains(&format!("{prefix}/images/old.jpg")));
    assert_eq!(fx.admin.get(&format!("{prefix}/index.html")), b"v2");
}

/// Killed between the data transfer and the rename: only a staging file
/// exists, never the final name; the resume leaves exactly one file.
pub async fn kill_before_rename_then_resume(endpoint: &Endpoint) {
    let fx = DavFixture::new(endpoint);
    let dir = tempfile::tempdir().unwrap();
    let photo = file(dir.path(), "IMG 1.jpg", "bytes of the first photo");
    let wanted = format!("{}_IMG 1.jpg", photo.content_key);
    let upload = job("rename", vec![photo.clone()], None, 1);

    fx.faults.0.lock().unwrap().kill_before_move = true;
    let killed = fx.plugin().upload(upload.clone(), &NoProgress, &CancelToken::new()).await;
    assert!(killed.is_err());
    assert!(!fx.admin.files().contains(&wanted), "final name must not exist yet");
    assert_eq!(
        fx.admin.files().len(),
        1,
        "only the staging file may exist: {:?}",
        fx.admin.files()
    );
    let mut plugin = fx.plugin();
    assert!(
        plugin.list_files(None).await.unwrap().is_empty(),
        "staging files are not uploaded files"
    );
    assert!(!plugin.exists(&photo.content_key, None).await.unwrap());

    fx.clear_faults();
    let report = plugin.upload(upload, &NoProgress, &CancelToken::new()).await.unwrap();
    assert_eq!(report.count(FileStatus::Uploaded), 1);
    assert_eq!(fx.admin.files(), vec![wanted.clone()]);
    assert_eq!(fx.admin.get(&wanted), b"bytes of the first photo");
}

/// A same-named file of another size (a half-written leftover) is replaced,
/// one of the same size is trusted.
pub async fn stale_remote_file_is_replaced(endpoint: &Endpoint) {
    let fx = DavFixture::new(endpoint);
    let dir = tempfile::tempdir().unwrap();
    let photo = file(dir.path(), "a.jpg", "the complete photo");
    let name = format!("{}_a.jpg", photo.content_key);
    fx.admin.put(&name, b"the comp".to_vec());

    let mut plugin = fx.plugin();
    let report = plugin
        .upload(job("stale", vec![photo.clone()], None, 2), &NoProgress, &CancelToken::new())
        .await
        .unwrap();
    assert_eq!(report.files[0].status, FileStatus::Uploaded);
    assert_eq!(fx.admin.get(&name), b"the complete photo");
    assert_eq!(fx.admin.files().len(), 1, "{:?}", fx.admin.files());

    let again = plugin
        .upload(job("stale2", vec![photo], None, 2), &NoProgress, &CancelToken::new())
        .await
        .unwrap();
    assert_eq!(again.files[0].status, FileStatus::AlreadyPresent);
}

/// Folder prefix, sanitized nested album directories and read-back.
pub async fn album_directories_and_read_back(endpoint: &Endpoint) {
    let fx = DavFixture::new(endpoint);
    let dir = tempfile::tempdir().unwrap();
    let a = file(dir.path(), "a b.jpg", "photo a");
    let b = file(dir.path(), "b.jpg", "photo b");
    let c = file(dir.path(), "c.jpg", "photo c");
    let config = fx.config().with_folder("photos/fotobase").unwrap();
    let mut plugin = fx.plugin_with(config);

    let report = plugin
        .upload(
            job(
                "layout",
                vec![a.clone(), b.clone()],
                Some(AlbumTarget::Create("Trips/2024: Rome?".into())),
                2,
            ),
            &NoProgress,
            &CancelToken::new(),
        )
        .await
        .unwrap();
    let album = report.album.unwrap();
    assert_eq!(album.name, "Trips/2024_ Rome_");
    assert_eq!(album.id, "photos/fotobase/Trips/2024_ Rome_");
    plugin
        .upload(job("root", vec![c.clone()], None, 2), &NoProgress, &CancelToken::new())
        .await
        .unwrap();

    let mut expected = vec![
        format!("photos/fotobase/Trips/2024_ Rome_/{}_a b.jpg", a.content_key),
        format!("photos/fotobase/Trips/2024_ Rome_/{}_b.jpg", b.content_key),
        format!("photos/fotobase/{}_c.jpg", c.content_key),
    ];
    expected.sort();
    assert_eq!(fx.admin.files(), expected);

    let in_album = plugin.list_files(Some(&album)).await.unwrap();
    assert_eq!(in_album.len(), 2);
    assert!(in_album
        .iter()
        .any(|f| f.name == "a b.jpg" && f.content_key.as_ref() == Some(&a.content_key)));
    assert_eq!(plugin.list_files(None).await.unwrap().len(), 3);
    assert!(plugin.exists(&a.content_key, Some(&album)).await.unwrap());
    assert!(!plugin.exists(&c.content_key, Some(&album)).await.unwrap());

    let names: Vec<_> = plugin.list_albums().await.unwrap().into_iter().map(|a| a.name).collect();
    assert_eq!(names, ["Trips", "Trips/2024_ Rome_"]);
}

/// Existing albums can be targeted by the id `list_albums` reported, and a
/// forged id cannot leave the configured folder.
pub async fn existing_album_ids_are_confined(endpoint: &Endpoint) {
    let fx = DavFixture::new(endpoint);
    let dir = tempfile::tempdir().unwrap();
    let a = file(dir.path(), "a.jpg", "photo a");
    let mut plugin = fx.plugin();
    plugin.create_album("j", "Family").await.unwrap();
    let albums = plugin.list_albums().await.unwrap();
    assert_eq!(albums.len(), 1);
    plugin
        .upload(
            job("existing", vec![a.clone()], Some(AlbumTarget::Existing(albums[0].clone())), 2),
            &NoProgress,
            &CancelToken::new(),
        )
        .await
        .unwrap();
    assert_eq!(fx.admin.files(), vec![format!("Family/{}_a.jpg", a.content_key)]);

    let forged = fotobase_export::AlbumRef { id: "../escape".into(), name: "x".into() };
    let result = plugin
        .upload(
            job("forged", vec![a], Some(AlbumTarget::Existing(forged)), 1),
            &NoProgress,
            &CancelToken::new(),
        )
        .await;
    assert!(matches!(result, Err(ExportError::Fatal(_))), "{result:?}");
}

/// Digest-only servers: the plugin alone (the test admin speaks Basic).
pub async fn digest_round_trip(endpoint: &Endpoint) {
    let config = WebDavConfig::new(&endpoint.url, &endpoint.username, &endpoint.password).unwrap();
    let mut plugin = WebDavPlugin::new(config).unwrap();
    plugin.validate_connection().await.unwrap();

    let dir = tempfile::tempdir().unwrap();
    let photo = file(dir.path(), "d.jpg", "digest protected photo");
    let target = Some(AlbumTarget::Create("Digest album".into()));
    let report = plugin
        .upload(
            job("digest", vec![photo.clone()], target.clone(), 2),
            &NoProgress,
            &CancelToken::new(),
        )
        .await
        .unwrap();
    assert_eq!(report.files[0].status, FileStatus::Uploaded);
    let album = report.album.unwrap();
    assert!(plugin.exists(&photo.content_key, Some(&album)).await.unwrap());
    let again = plugin
        .upload(job("digest", vec![photo], target, 2), &NoProgress, &CancelToken::new())
        .await
        .unwrap();
    assert_eq!(again.files[0].status, FileStatus::AlreadyPresent);

    let wrong = WebDavConfig::new(&endpoint.url, &endpoint.username, "nope").unwrap();
    let err = WebDavPlugin::new(wrong).unwrap().validate_connection().await.unwrap_err();
    assert!(matches!(&err, ExportError::Fatal(m) if m.contains("username or password")), "{err}");
}
