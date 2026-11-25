//! TDD tests for TASK-08-011: the WebDAV plugin passes the shared contract
//! suite against an in-process WebDAV server (Open, Basic and Digest auth,
//! servers with and without `MOVE`), maps albums to directories, never leaves
//! a half-written file under its final name and rejects bad settings with
//! clear errors (PLAN §6 P4). `webdav_httpd.rs` runs the same checks against
//! Apache `mod_dav` in a container.

mod dav_support;

use dav_support::mock::{AuthMode, MockDav, PASSWORD, USER};
use dav_support::scenarios::{self, file, job};
use dav_support::{DavFixture, Endpoint};
use fotobase_export::contract::{run_contract_suite, ContractFixture};
use fotobase_export::webdav::{WebDavConfig, WebDavPlugin};
use fotobase_export::{
    AlbumTarget, CancelToken, ExportError, ExportPlugin, FileStatus, NoProgress, Registry,
};

fn endpoint(mock: &MockDav) -> Endpoint {
    Endpoint { url: mock.url(), username: USER.into(), password: PASSWORD.into() }
}

#[tokio::test]
async fn open_servers_pass_the_contract_suite() {
    let mock = MockDav::start(AuthMode::Open);
    let endpoint = endpoint(&mock);
    run_contract_suite(|| DavFixture::new(&endpoint)).await.unwrap();
}

#[tokio::test]
async fn basic_auth_passes_the_contract_suite() {
    let mock = MockDav::start(AuthMode::Basic);
    let endpoint = endpoint(&mock);
    run_contract_suite(|| DavFixture::new(&endpoint)).await.unwrap();
    let state = mock.state();
    assert!(state.count("MKCOL") > 0 && state.count("MOVE") > 0 && state.count("PROPFIND") > 0);
}

#[tokio::test]
async fn digest_auth_passes_the_contract_suite() {
    let mock = MockDav::start(AuthMode::Digest);
    let endpoint = endpoint(&mock);
    run_contract_suite(|| DavFixture::new(&endpoint)).await.unwrap();
    assert!(mock.state().digest_requests > 20);
}

#[tokio::test]
async fn servers_without_move_overwrite_in_place_with_a_size_check() {
    let mock = MockDav::start(AuthMode::Basic);
    mock.state().move_supported = false;
    let endpoint = endpoint(&mock);
    run_contract_suite(|| DavFixture::new(&endpoint)).await.unwrap();
    scenarios::stale_remote_file_is_replaced(&endpoint).await;
}

#[tokio::test]
async fn move_support_is_probed_once_per_plugin() {
    let mock = MockDav::start(AuthMode::Open);
    mock.state().move_supported = false;
    let fx = DavFixture::new(&endpoint(&mock));
    let dir = tempfile::tempdir().unwrap();
    let files: Vec<_> =
        (0..3).map(|i| file(dir.path(), &format!("{i}.jpg"), &format!("p{i}"))).collect();
    fx.plugin().upload(job("m", files, None, 1), &NoProgress, &CancelToken::new()).await.unwrap();
    assert_eq!(mock.state().count("MOVE"), 1);
    assert_eq!(fx.admin.files().len(), 3);
}

#[tokio::test]
async fn kill_before_the_rename_then_resume_leaves_one_file() {
    let mock = MockDav::start(AuthMode::Basic);
    scenarios::kill_before_rename_then_resume(&endpoint(&mock)).await;
}

#[tokio::test]
async fn static_site_publish_replaces_and_prunes_exact_paths_without_move() {
    let mock = MockDav::start(AuthMode::Basic);
    mock.state().move_supported = false;
    scenarios::static_site_publish_replaces_and_prunes_exact_paths(&endpoint(&mock)).await;
}

#[tokio::test]
async fn a_transfer_killed_midway_never_leaves_a_partial_file() {
    let mock = MockDav::start(AuthMode::Basic);
    let fx = DavFixture::new(&endpoint(&mock));
    let dir = tempfile::tempdir().unwrap();
    let photo = file(dir.path(), "big.jpg", &"x".repeat(100_000));

    mock.state().drop_next_put = true;
    let first = fx
        .plugin()
        .upload(job("kill", vec![photo.clone()], None, 1), &NoProgress, &CancelToken::new())
        .await;
    assert!(matches!(first, Err(ExportError::Transient(_))), "{first:?}");
    assert_eq!(fx.admin.files(), Vec::<String>::new(), "the staged half must be cleaned up");

    let report = fx
        .plugin()
        .upload(job("kill", vec![photo.clone()], None, 2), &NoProgress, &CancelToken::new())
        .await
        .unwrap();
    assert_eq!(report.files[0].status, FileStatus::Uploaded);
    assert_eq!(fx.admin.files().len(), 1);
    assert_eq!(fx.admin.get(&format!("{}_big.jpg", photo.content_key)).len(), 100_000);
}

#[tokio::test]
async fn an_in_place_upload_killed_midway_is_replaced_on_retry() {
    let mock = MockDav::start(AuthMode::Open);
    mock.state().move_supported = false;
    let fx = DavFixture::new(&endpoint(&mock));
    let dir = tempfile::tempdir().unwrap();
    let photo = file(dir.path(), "big.jpg", &"y".repeat(50_000));
    let name = format!("{}_big.jpg", photo.content_key);

    // First plugin learns that MOVE is missing (upload one other file).
    let probe = file(dir.path(), "probe.jpg", "probe");
    let mut plugin = fx.plugin();
    plugin.upload(job("p", vec![probe], None, 1), &NoProgress, &CancelToken::new()).await.unwrap();

    mock.state().drop_next_put = true;
    let result = plugin
        .upload(job("k", vec![photo.clone()], None, 1), &NoProgress, &CancelToken::new())
        .await;
    assert!(result.is_err());
    assert!(
        !fx.admin.files().contains(&name),
        "a half-written file must be removed: {:?}",
        fx.admin.files()
    );

    let report = plugin
        .upload(job("k", vec![photo], None, 2), &NoProgress, &CancelToken::new())
        .await
        .unwrap();
    assert_eq!(report.files[0].status, FileStatus::Uploaded);
    assert_eq!(fx.admin.get(&name).len(), 50_000);
}

#[tokio::test]
async fn directory_layout_albums_and_read_back() {
    let mock = MockDav::start(AuthMode::Basic);
    scenarios::album_directories_and_read_back(&endpoint(&mock)).await;
}

#[tokio::test]
async fn existing_albums_stay_inside_the_folder() {
    let mock = MockDav::start(AuthMode::Basic);
    scenarios::existing_album_ids_are_confined(&endpoint(&mock)).await;
}

#[tokio::test]
async fn any_namespace_prefix_in_multistatus_answers_is_understood() {
    let mock = MockDav::start(AuthMode::Open);
    mock.state().prefix = "ns0".into();
    let fx = DavFixture::new(&endpoint(&mock));
    let dir = tempfile::tempdir().unwrap();
    let photo = file(dir.path(), "a.jpg", "photo");
    let mut plugin = fx.plugin();
    plugin
        .upload(
            job("ns", vec![photo.clone()], Some(AlbumTarget::Create("Album".into())), 1),
            &NoProgress,
            &CancelToken::new(),
        )
        .await
        .unwrap();
    let albums = plugin.list_albums().await.unwrap();
    assert_eq!(albums.len(), 1);
    let listed = plugin.list_files(Some(&albums[0])).await.unwrap();
    assert_eq!(listed[0].content_key.as_ref(), Some(&photo.content_key));
}

#[tokio::test]
async fn digest_nonce_expiry_is_renegotiated() {
    let mock = MockDav::start(AuthMode::Digest);
    let fx = DavFixture::new(&endpoint(&mock));
    let dir = tempfile::tempdir().unwrap();
    let files = vec![file(dir.path(), "a.jpg", "a"), file(dir.path(), "b.jpg", "b")];
    let mut plugin = fx.plugin();
    plugin
        .upload(job("d1", files[..1].to_vec(), None, 1), &NoProgress, &CancelToken::new())
        .await
        .unwrap();
    let before = mock.state().digest_requests;
    mock.state().stale_nonce_next = true;
    plugin.upload(job("d2", files, None, 1), &NoProgress, &CancelToken::new()).await.unwrap();
    assert!(mock.state().digest_requests > before);
}

#[tokio::test]
async fn validate_connection_accepts_good_settings_only() {
    let mock = MockDav::start(AuthMode::Basic);
    let ok = endpoint(&mock);
    let plugin = |url: &str, user: &str, password: &str| {
        WebDavPlugin::new(WebDavConfig::new(url, user, password).unwrap()).unwrap()
    };
    plugin(&ok.url, USER, PASSWORD).validate_connection().await.unwrap();

    let err = plugin(&ok.url, USER, "wrong").validate_connection().await.unwrap_err();
    assert!(matches!(&err, ExportError::Fatal(m) if m.contains("username or password")), "{err}");

    let missing = format!("{}nothing-here/", ok.url);
    let err = plugin(&missing, USER, PASSWORD).validate_connection().await.unwrap_err();
    assert!(matches!(&err, ExportError::Fatal(m) if m.contains("not found")), "{err}");

    mock.state().fail_next = Some(405);
    let err = plugin(&ok.url, USER, PASSWORD).validate_connection().await.unwrap_err();
    assert!(matches!(&err, ExportError::Fatal(m) if m.contains("WebDAV")), "{err}");

    mock.state().fail_next = Some(301);
    let err = plugin(&ok.url, USER, PASSWORD).validate_connection().await.unwrap_err();
    assert!(matches!(&err, ExportError::Fatal(m) if m.contains("redirect")), "{err}");
}

#[tokio::test]
async fn server_errors_are_transient_and_unreachable_servers_say_so() {
    let mock = MockDav::start(AuthMode::Open);
    let ok = endpoint(&mock);
    let plugin =
        |url: &str| WebDavPlugin::new(WebDavConfig::new(url, USER, PASSWORD).unwrap()).unwrap();
    for (status, transient) in [(503, true), (429, true), (423, true), (507, false), (413, false)] {
        mock.state().fail_next = Some(status);
        let err = plugin(&ok.url).validate_connection().await.unwrap_err();
        assert_eq!(matches!(err, ExportError::Transient(_)), transient, "{status}: {err}");
    }

    let dead = {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        format!("http://{}/", listener.local_addr().unwrap())
    };
    let err = plugin(&dead).validate_connection().await.unwrap_err();
    assert!(matches!(&err, ExportError::Transient(m) if m.contains("cannot reach")), "{err}");
}

#[tokio::test]
async fn a_file_changed_after_hashing_is_refused_before_upload() {
    let mock = MockDav::start(AuthMode::Open);
    let fx = DavFixture::new(&endpoint(&mock));
    let dir = tempfile::tempdir().unwrap();
    let photo = file(dir.path(), "a.jpg", "original");
    std::fs::write(&photo.path, b"edited behind our back").unwrap();
    let result =
        fx.plugin().upload(job("a", vec![photo], None, 3), &NoProgress, &CancelToken::new()).await;
    assert!(matches!(&result, Err(ExportError::Fatal(m)) if m.contains("changed")), "{result:?}");
    assert!(fx.admin.files().is_empty());
}

#[tokio::test]
async fn the_plugin_registers_under_its_id() {
    let mock = MockDav::start(AuthMode::Open);
    let fx = DavFixture::new(&endpoint(&mock));
    let mut registry = Registry::new();
    registry.register(Box::new(fx.plugin())).unwrap();
    let info = registry.list();
    assert_eq!(info[0].id, "webdav");
    assert!(info[0].capabilities.albums && info[0].capabilities.size_choice);
}

#[test]
fn settings_are_validated_without_network_access() {
    let ok = |url: &str, user: &str| WebDavConfig::new(url, user, "pw");
    let rejected = |result: Result<WebDavConfig, ExportError>, needle: &str| {
        match result {
            Err(ExportError::InvalidConfig(m)) => {
                assert!(m.contains(needle), "`{m}` should mention `{needle}`")
            }
            other => panic!("expected InvalidConfig mentioning `{needle}`, got {other:?}"),
        };
    };

    ok("https://cloud.example.com/remote.php/dav/files/alice/", "alice").unwrap();
    ok("https://cloud.example.com/remote.php/dav/files/alice", "alice").unwrap();
    rejected(ok("", "alice"), "enter the WebDAV URL");
    rejected(ok("not a url", "alice"), "valid URL");
    rejected(ok("ftp://host/dav/", "alice"), "http://");
    rejected(ok("https://alice:pw@host/dav/", "alice"), "credentials");
    rejected(ok("https://host/dav/?x=1", "alice"), "query");
    rejected(ok("https://host/dav/", ""), "username");
    rejected(WebDavConfig::new("https://host/dav/", "alice", "bad\npw"), "password");

    let base = || ok("https://host/dav/", "alice").unwrap();
    assert!(base().with_folder("a/../b").is_err());
    assert!(base().with_folder("a/.\\b").is_err());
    assert!(base().with_folder("photos/fotobase/").is_ok());

    // Plain http would send the password in the clear.
    let lan = ok("http://nas.local/dav/", "alice").unwrap();
    assert!(matches!(
        WebDavPlugin::new(lan.clone()),
        Err(ExportError::InvalidConfig(m)) if m.contains("https://")
    ));
    assert!(WebDavPlugin::new(lan.with_insecure_http()).is_ok());
    for local in ["http://localhost:8080/", "http://127.0.0.1/dav/", "http://[::1]:8080/"] {
        assert!(WebDavPlugin::new(ok(local, "alice").unwrap()).is_ok(), "{local}");
    }
}

#[test]
fn secrets_stay_out_of_debug_output() {
    let config = WebDavConfig::new("https://host/dav/", "alice-the-user", "topsecret").unwrap();
    let shown = format!("{config:?}");
    assert!(!shown.contains("topsecret") && !shown.contains("alice-the-user"), "{shown}");
}
