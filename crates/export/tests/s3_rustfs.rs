//! TASK-08-010 contract run against a real RustFS (S3-compatible, Apache-2.0)
//! in a container: proves the plugin's SigV4 signing, multipart handling and
//! listings against an actual S3 implementation. Needs a Docker daemon;
//! without one the test is skipped unless `FOTOBASE_REQUIRE_DOCKER` is set to
//! a non-empty value (CI sets it on Linux).

mod support;

use fotobase_export::contract::run_contract_suite;
use support::scenarios;
use support::{Endpoint, S3Fixture};
use testcontainers::core::wait::HttpWaitStrategy;
use testcontainers::core::{IntoContainerPort, WaitFor};
use testcontainers::runners::AsyncRunner;
use testcontainers::{GenericImage, ImageExt};

const IMAGE: &str = "rustfs/rustfs";
const TAG: &str = "1.0.6";
const ACCESS_KEY: &str = "fotobase-test";
const SECRET_KEY: &str = "fotobase-test-secret";

#[tokio::test]
async fn rustfs_passes_the_contract_suite_and_resumes_multipart_uploads() {
    // The container readiness probe builds a reqwest client.
    let _ = rustls::crypto::ring::default_provider().install_default();
    let started = GenericImage::new(IMAGE, TAG)
        .with_exposed_port(9000.tcp())
        .with_wait_for(WaitFor::http(
            // `/health` is liveness-only and returns 200 before S3 requests
            // can succeed. Wait for storage, IAM and startup to be ready.
            HttpWaitStrategy::new("/health/ready")
                .with_port(9000.tcp())
                .with_expected_status_code(200u16),
        ))
        .with_env_var("RUSTFS_ACCESS_KEY", ACCESS_KEY)
        .with_env_var("RUSTFS_SECRET_KEY", SECRET_KEY)
        .with_cmd(["/data"])
        .start()
        .await;
    let container = match started {
        Ok(container) => container,
        // CI passes an empty value on runners without Docker.
        Err(e) if std::env::var("FOTOBASE_REQUIRE_DOCKER").unwrap_or_default().is_empty() => {
            eprintln!("skipping the RustFS contract run (no usable Docker): {e}");
            return;
        }
        Err(e) => panic!("cannot start the RustFS container: {e}"),
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
    scenarios::static_site_publish_replaces_and_prunes_exact_paths(&endpoint).await;
}
