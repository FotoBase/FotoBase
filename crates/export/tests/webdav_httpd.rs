//! TASK-08-011 contract run against a real WebDAV server — Apache `mod_dav`
//! in a container, once with Basic and once with Digest authentication:
//! proves the plugin's verbs (`PROPFIND`, `MKCOL`, `PUT`, `MOVE`), the
//! multistatus parsing and the digest handshake against an actual
//! implementation. Needs a Docker daemon; without one the test is skipped
//! unless `FOTOBASE_REQUIRE_DOCKER` is set to a non-empty value (CI sets it
//! on Linux).

mod dav_support;

use dav_support::scenarios;
use dav_support::{DavFixture, Endpoint};
use fotobase_export::contract::run_contract_suite;
use testcontainers::core::wait::HttpWaitStrategy;
use testcontainers::core::{IntoContainerPort, WaitFor};
use testcontainers::runners::AsyncRunner;
use testcontainers::{ContainerAsync, GenericImage, ImageExt};

const IMAGE: &str = "bytemark/webdav";
const TAG: &str = "2.4";
const USERNAME: &str = "fotobase";
const PASSWORD: &str = "fotobase-test-secret";

async fn start(auth: &str) -> Option<(ContainerAsync<GenericImage>, Endpoint)> {
    // The container readiness probe builds a reqwest client.
    let _ = rustls::crypto::ring::default_provider().install_default();
    let started = GenericImage::new(IMAGE, TAG)
        .with_exposed_port(80.tcp())
        .with_wait_for(WaitFor::http(
            HttpWaitStrategy::new("/").with_port(80.tcp()).with_expected_status_code(401u16),
        ))
        .with_env_var("AUTH_TYPE", auth)
        .with_env_var("USERNAME", USERNAME)
        .with_env_var("PASSWORD", PASSWORD)
        .start()
        .await;
    let container = match started {
        Ok(container) => container,
        // CI passes an empty value on runners without Docker.
        Err(e) if std::env::var("FOTOBASE_REQUIRE_DOCKER").unwrap_or_default().is_empty() => {
            eprintln!("skipping the Apache WebDAV contract run (no usable Docker): {e}");
            return None;
        }
        Err(e) => panic!("cannot start the Apache WebDAV container: {e}"),
    };
    let host = container.get_host().await.unwrap();
    let port = container.get_host_port_ipv4(80.tcp()).await.unwrap();
    let endpoint = Endpoint {
        url: format!("http://{host}:{port}/"),
        username: USERNAME.into(),
        password: PASSWORD.into(),
    };
    Some((container, endpoint))
}

#[tokio::test]
async fn apache_mod_dav_passes_the_contract_suite() {
    let Some((_basic, endpoint)) = start("Basic").await else { return };
    run_contract_suite(|| DavFixture::new(&endpoint)).await.unwrap();
    scenarios::kill_before_rename_then_resume(&endpoint).await;
    scenarios::stale_remote_file_is_replaced(&endpoint).await;
    scenarios::album_directories_and_read_back(&endpoint).await;
    scenarios::existing_album_ids_are_confined(&endpoint).await;
    scenarios::static_site_publish_replaces_and_prunes_exact_paths(&endpoint).await;
}

#[tokio::test]
async fn apache_mod_dav_digest_auth_works() {
    let Some((_digest, endpoint)) = start("Digest").await else { return };
    scenarios::digest_round_trip(&endpoint).await;
}
