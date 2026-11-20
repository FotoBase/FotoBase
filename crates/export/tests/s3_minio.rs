//! TASK-08-010 contract run against a real MinIO in a container: proves the
//! plugin's SigV4 signing, multipart handling and listings against an actual
//! S3 implementation. Needs a Docker daemon; without one the test is skipped
//! unless `FOTOBASE_REQUIRE_DOCKER` is set (CI sets it on Linux).
//!
//! The image is a pinned community build of MinIO: the upstream
//! `minio/minio` images are no longer published.

mod support;

use fotobase_export::contract::run_contract_suite;
use support::scenarios;
use support::{Endpoint, S3Fixture};
use testcontainers::core::{IntoContainerPort, WaitFor};
use testcontainers::runners::AsyncRunner;
use testcontainers::{GenericImage, ImageExt};

const IMAGE: &str = "pgsty/minio";
const TAG: &str = "RELEASE.2026-08-04T00-00-00Z";
const ACCESS_KEY: &str = "fotobase-test";
const SECRET_KEY: &str = "fotobase-test-secret";

#[tokio::test]
async fn minio_passes_the_contract_suite_and_resumes_multipart_uploads() {
    let started = GenericImage::new(IMAGE, TAG)
        .with_exposed_port(9000.tcp())
        .with_wait_for(WaitFor::message_on_either_std("API:"))
        .with_env_var("MINIO_ROOT_USER", ACCESS_KEY)
        .with_env_var("MINIO_ROOT_PASSWORD", SECRET_KEY)
        .with_cmd(["server", "/data"])
        .start()
        .await;
    let container = match started {
        Ok(container) => container,
        Err(e) if std::env::var_os("FOTOBASE_REQUIRE_DOCKER").is_none() => {
            eprintln!("skipping the MinIO contract run (no usable Docker): {e}");
            return;
        }
        Err(e) => panic!("cannot start the MinIO container: {e}"),
    };
    let host = container.get_host().await.unwrap();
    let port = container.get_host_port_ipv4(9000.tcp()).await.unwrap();
    let endpoint = Endpoint {
        url: format!("http://{host}:{port}"),
        access_key: ACCESS_KEY.into(),
        secret_key: SECRET_KEY.into(),
    };

    run_contract_suite(|| S3Fixture::new(&endpoint, false)).await.unwrap();
    run_contract_suite(|| S3Fixture::new(&endpoint, true)).await.unwrap();
    scenarios::multipart_kill_then_resume(&endpoint).await;
    scenarios::multipart_resume_with_new_part_size(&endpoint).await;
    scenarios::multipart_fatal_failure_aborts(&endpoint).await;
    scenarios::key_layout_and_read_back(&endpoint).await;
    scenarios::retry_overwrites_the_same_key(&endpoint).await;
}
