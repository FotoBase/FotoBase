//! TDD tests for TASK-08-010: the S3 plugin passes the shared contract suite
//! (single-request and multipart paths) against an in-process S3 server,
//! resumes multipart uploads without exposing partial objects, maps albums to
//! key prefixes and rejects bad settings with clear errors (PLAN §6 P4).
//! `s3_rustfs.rs` runs the same checks against a real RustFS.

#[path = "support/mock_s3.rs"]
mod mock_s3;
mod support;

use fotobase_export::contract::{run_contract_suite, ContractFixture};
use fotobase_export::s3::{S3Config, S3Credentials, S3Plugin};
use fotobase_export::{CancelToken, ExportError, ExportPlugin, NoProgress, Registry};
use mock_s3::{MockS3, ACCESS_KEY, SECRET_KEY};
use support::scenarios::{self, big_file, job};
use support::{Endpoint, S3Fixture, REGION};

fn endpoint(mock: &MockS3) -> Endpoint {
    Endpoint { url: mock.url(), access_key: ACCESS_KEY.into(), secret_key: SECRET_KEY.into() }
}

fn creds() -> S3Credentials {
    S3Credentials::new("key", "secret").unwrap()
}

#[tokio::test]
async fn single_request_uploads_pass_the_contract_suite() {
    let mock = MockS3::start();
    let endpoint = endpoint(&mock);
    run_contract_suite(|| S3Fixture::new(&endpoint, false)).await.unwrap();
}

#[tokio::test]
async fn multipart_uploads_pass_the_contract_suite() {
    let mock = MockS3::start();
    let endpoint = endpoint(&mock);
    run_contract_suite(|| S3Fixture::new(&endpoint, true)).await.unwrap();
    assert!(
        mock.state().log.iter().any(|l| l.contains("uploadId=") && l.starts_with("POST")),
        "the multipart path should have been exercised"
    );
}

#[tokio::test]
async fn multipart_kill_then_resume_sends_only_missing_parts() {
    let mock = MockS3::start();
    scenarios::multipart_kill_then_resume(&endpoint(&mock)).await;
}

#[tokio::test]
async fn multipart_resume_discards_parts_of_another_size() {
    let mock = MockS3::start();
    scenarios::multipart_resume_with_new_part_size(&endpoint(&mock)).await;
}

#[tokio::test]
async fn fatal_failure_aborts_the_multipart_upload() {
    let mock = MockS3::start();
    scenarios::multipart_fatal_failure_aborts(&endpoint(&mock)).await;
}

#[tokio::test]
async fn keys_follow_the_layout_and_read_back_works() {
    let mock = MockS3::start();
    scenarios::key_layout_and_read_back(&endpoint(&mock)).await;
}

#[tokio::test]
async fn retry_replaces_a_stale_object_under_the_same_key() {
    let mock = MockS3::start();
    scenarios::retry_overwrites_the_same_key(&endpoint(&mock)).await;
}

#[tokio::test]
async fn static_site_publish_replaces_and_prunes_exact_paths() {
    let mock = MockS3::start();
    scenarios::static_site_publish_replaces_and_prunes_exact_paths(&endpoint(&mock)).await;
}

#[tokio::test]
async fn listings_follow_continuation_tokens() {
    let mock = MockS3::start();
    let fx = S3Fixture::new(&endpoint(&mock), false);
    mock.state().page_size = Some(2);
    let dir = tempfile::tempdir().unwrap();
    let files: Vec<_> =
        (0..5).map(|i| big_file(dir.path(), &format!("p{i}.jpg"), 100 + i)).collect();
    let mut plugin = fx.plugin();
    plugin.upload(job("p", files, None, 1), &NoProgress, &CancelToken::new()).await.unwrap();
    assert_eq!(plugin.list_files(None).await.unwrap().len(), 5);
}

#[tokio::test]
async fn a_file_changed_after_hashing_is_refused_before_upload() {
    let mock = MockS3::start();
    let fx = S3Fixture::new(&endpoint(&mock), false);
    let dir = tempfile::tempdir().unwrap();
    let file = big_file(dir.path(), "a.jpg", 500);
    std::fs::write(&file.path, b"edited behind our back").unwrap();
    let result =
        fx.plugin().upload(job("a", vec![file], None, 3), &NoProgress, &CancelToken::new()).await;
    assert!(matches!(&result, Err(ExportError::Fatal(m)) if m.contains("changed")), "{result:?}");
    assert!(fx.admin.objects().is_empty());
}

#[tokio::test]
async fn validate_connection_accepts_good_settings_only() {
    let mock = MockS3::start();
    let fx = S3Fixture::new(&endpoint(&mock), false);
    fx.plugin().validate_connection().await.unwrap();

    let wrong_key = S3Credentials::new("nope", "nope").unwrap();
    let config = S3Config::new(&mock.url(), &fx.bucket, REGION, wrong_key).unwrap();
    let err = S3Plugin::new(config).unwrap().validate_connection().await.unwrap_err();
    assert!(matches!(&err, ExportError::Fatal(m) if m.contains("access key")), "{err}");

    let config = S3Config::new(&mock.url(), "no-such-bucket", REGION, fx.credentials()).unwrap();
    let err = S3Plugin::new(config).unwrap().validate_connection().await.unwrap_err();
    assert!(
        matches!(&err, ExportError::Fatal(m) if m.contains("`no-such-bucket` does not exist")),
        "{err}"
    );
}

#[tokio::test]
async fn server_errors_are_transient_and_unreachable_endpoints_say_so() {
    let mock = MockS3::start();
    let fx = S3Fixture::new(&endpoint(&mock), false);
    mock.state().fail_next = Some(503);
    let err = fx.plugin().validate_connection().await.unwrap_err();
    assert!(matches!(err, ExportError::Transient(_)), "{err}");

    let dead = {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        format!("http://{}", listener.local_addr().unwrap())
    };
    let config = S3Config::new(&dead, "photos-bucket", REGION, creds()).unwrap();
    let err = S3Plugin::new(config).unwrap().validate_connection().await.unwrap_err();
    assert!(matches!(&err, ExportError::Transient(m) if m.contains("cannot reach")), "{err}");
    assert!(!err.to_string().contains("X-Amz-Signature"), "{err}");
}

#[tokio::test]
async fn the_plugin_registers_under_its_id() {
    let mock = MockS3::start();
    let fx = S3Fixture::new(&endpoint(&mock), false);
    let mut registry = Registry::new();
    registry.register(Box::new(fx.plugin())).unwrap();
    let info = registry.list();
    assert_eq!(info[0].id, "s3");
    assert!(info[0].capabilities.albums);
}

#[test]
fn settings_are_validated_without_network_access() {
    let ok = |endpoint: &str, bucket: &str, region: &str| {
        S3Config::new(endpoint, bucket, region, creds())
    };
    let rejected = |result: Result<S3Config, ExportError>, needle: &str| {
        match result {
            Err(ExportError::InvalidConfig(m)) => {
                assert!(m.contains(needle), "`{m}` should mention `{needle}`")
            }
            other => panic!("expected InvalidConfig mentioning `{needle}`, got {other:?}"),
        };
    };

    ok("https://s3.eu-west-1.amazonaws.com", "my-photos", "eu-west-1").unwrap();
    ok("http://localhost:9000", "my.photos", "auto").unwrap();
    rejected(ok("", "my-photos", "r"), "endpoint");
    rejected(ok("not a url", "my-photos", "r"), "valid URL");
    rejected(ok("ftp://host", "my-photos", "r"), "http://");
    rejected(ok("https://user:pw@host", "my-photos", "r"), "credentials");
    rejected(ok("https://host/?x=1", "my-photos", "r"), "query");
    rejected(ok("https://host", "", "r"), "bucket");
    rejected(ok("https://host", "AB", "r"), "bucket name");
    rejected(ok("https://host", "Upper_Case", "r"), "bucket name");
    rejected(ok("https://host", "-dash", "r"), "bucket name");
    rejected(ok("https://host", "my-photos", ""), "region");
    rejected(ok("https://host", "my-photos", "eu west"), "region");

    assert!(matches!(S3Credentials::new("", "s"), Err(ExportError::InvalidConfig(_))));
    assert!(matches!(S3Credentials::new("k", "has space"), Err(ExportError::InvalidConfig(_))));
    let base = || ok("https://host", "my-photos", "r").unwrap();
    assert!(base().with_prefix("a/../b").is_err());
    assert!(base().with_prefix("a/b/").is_ok());
    assert!(base().with_part_size(1024).is_err());
    assert!(base().with_part_size(8 * 1024 * 1024).is_ok());
    assert!(ok("http://127.0.0.1:9000", "my-photos", "r")
        .unwrap()
        .with_virtual_host_style()
        .is_err());
}

#[test]
fn secrets_stay_out_of_debug_output() {
    let credentials = S3Credentials::new("AKIAEXAMPLE", "topsecret").unwrap();
    let config = S3Config::new("https://host", "my-photos", "r", credentials).unwrap();
    let shown = format!("{config:?}");
    assert!(!shown.contains("AKIAEXAMPLE") && !shown.contains("topsecret"), "{shown}");
}

#[test]
fn credentials_are_read_from_an_aws_style_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("credentials");
    std::fs::write(
        &path,
        "# comment\n[default]\naws_access_key_id = AKIADEFAULT\naws_secret_access_key = s3cret\n\n\
         [photos]\naws_access_key_id=AKIAPHOTOS\naws_secret_access_key=other\naws_session_token=tok\n\
         [broken]\naws_access_key_id=AKIABROKEN\n",
    )
    .unwrap();

    let default = S3Credentials::from_file(&path, "default").unwrap();
    let photos = S3Credentials::from_file(&path, "photos").unwrap();
    assert_ne!(format!("{default:?}"), format!("{photos:?}"));
    assert!(format!("{photos:?}").contains("session_token: Some"));
    assert!(format!("{default:?}").contains("session_token: None"));

    for (profile, needle) in [("missing", "no profile"), ("broken", "aws_secret_access_key")] {
        match S3Credentials::from_file(&path, profile) {
            Err(ExportError::InvalidConfig(m)) => assert!(m.contains(needle), "{m}"),
            other => panic!("{other:?}"),
        }
    }
    match S3Credentials::from_file(&dir.path().join("nope"), "default") {
        Err(ExportError::InvalidConfig(m)) => assert!(m.contains("cannot read"), "{m}"),
        other => panic!("{other:?}"),
    }
}
