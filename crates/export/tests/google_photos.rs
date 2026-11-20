//! TDD tests for TASK-08-012: the Google Photos plugin passes the shared
//! contract suite against a recorded-API mock of the Photos Library API,
//! resumes chunked uploads, keeps duplicates out through the local content-key
//! ledger, runs the OAuth (PKCE, loopback) flow, refreshes tokens, and stays
//! strictly upload-only (PLAN §6 P4, §10).

mod gphotos_mock;

use std::sync::{Arc, Mutex};
use std::time::Duration;

use fotobase_export::contract::{run_contract_suite, ContractFixture, Fault, RemoteAlbum};
use fotobase_export::google_photos::{
    GoogleAuth, GooglePhotosConfig, GooglePhotosPlugin, LoopbackReceiver, MemoryTokenStore,
    OAuthTokens, TokenStore, UploadLedger, SCOPES,
};
use fotobase_export::{
    AlbumTarget, CancelToken, ContentKey, ExportError, ExportPlugin, FileStatus, NoProgress,
    ProgressEvent, RetryPolicy, UploadFile, UploadJob, UploadOptions,
};
use gphotos_mock::{Faults, MockPhotos, CLIENT_ID};

fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs()
}

fn signed_in_store(server: &MockPhotos) -> Arc<MemoryTokenStore> {
    let store = Arc::new(MemoryTokenStore::default());
    store
        .save(&OAuthTokens {
            access_token: server.access_token(),
            refresh_token: "refresh-1".into(),
            expires_at_unix: now() + 3600,
        })
        .unwrap();
    store
}

fn config(server: &MockPhotos) -> GooglePhotosConfig {
    server.config()
}

fn plugin_with(
    server: &MockPhotos,
    store: Arc<MemoryTokenStore>,
    ledger: &UploadLedger,
) -> GooglePhotosPlugin {
    GooglePhotosPlugin::new(config(server), store, ledger.clone()).unwrap()
}

fn files_of(sizes: &[usize]) -> (tempfile::TempDir, Vec<UploadFile>) {
    let dir = tempfile::tempdir().unwrap();
    let files = sizes
        .iter()
        .enumerate()
        .map(|(i, size)| {
            let path = dir.path().join(format!("IMG_{i}.jpg"));
            let data: Vec<u8> = (0..*size).map(|b| (b + i * 7) as u8).collect();
            std::fs::write(&path, data).unwrap();
            UploadFile::from_path(&path).unwrap()
        })
        .collect();
    (dir, files)
}

fn job(id: &str, files: &[UploadFile], album: Option<AlbumTarget>) -> UploadJob {
    UploadJob {
        id: id.into(),
        files: files.to_vec(),
        album,
        options: UploadOptions::default(),
        retry: RetryPolicy { max_attempts: 3, base_delay: Duration::ZERO },
    }
}

// ------------------------------------------------------------------- fixtures

struct PhotosFixture {
    server: MockPhotos,
    store: Arc<MemoryTokenStore>,
    ledger: UploadLedger,
    chunk_size: u64,
}

impl PhotosFixture {
    fn new() -> Self {
        Self::chunked(256 * 1024, 256 * 1024)
    }

    /// Tiny chunks, so even the contract suite's small files take several
    /// requests.
    fn chunked(granularity: u64, chunk_size: u64) -> Self {
        let server = MockPhotos::start();
        server.state().granularity = granularity;
        let store = signed_in_store(&server);
        Self { server, store, ledger: UploadLedger::in_memory(), chunk_size }
    }
}

impl ContractFixture for PhotosFixture {
    type Plugin = GooglePhotosPlugin;

    // A new plugin shares the ledger and token store, like the app does
    // across a restart.
    fn plugin(&self) -> GooglePhotosPlugin {
        GooglePhotosPlugin::new(
            config(&self.server).with_chunk_size(self.chunk_size).unwrap(),
            self.store.clone(),
            self.ledger.clone(),
        )
        .unwrap()
    }

    fn inject(&self, fault: Fault) {
        let faults = &mut self.server.state().faults;
        match fault {
            Fault::AbortUploadsAfter(n) => faults.abort_after = Some(n),
            Fault::FailUploadOnce => faults.fail_once = true,
            Fault::LoseUploadResponseOnce => faults.lose_upload_response_once = true,
            Fault::LoseAlbumResponseOnce => faults.lose_album_response_once = true,
        }
    }

    fn clear_faults(&self) {
        self.server.state().faults = Faults::default();
    }

    fn remote_files(&self) -> Vec<ContentKey> {
        self.server.state().items.iter().map(|i| i.key.clone()).collect()
    }

    fn remote_albums(&self) -> Vec<RemoteAlbum> {
        let state = self.server.state();
        state
            .albums
            .iter()
            .map(|album| RemoteAlbum {
                name: album.title.clone(),
                files: album
                    .items
                    .iter()
                    .map(|id| state.items.iter().find(|i| &i.id == id).unwrap().key.clone())
                    .collect(),
            })
            .collect()
    }
}

// ------------------------------------------------------------ contract + upload

#[tokio::test]
async fn google_photos_plugin_passes_the_contract_suite() {
    run_contract_suite(PhotosFixture::new).await.unwrap();
}

#[tokio::test]
async fn contract_suite_also_passes_with_multi_chunk_uploads() {
    run_contract_suite(|| PhotosFixture::chunked(16, 16)).await.unwrap();
}

#[tokio::test]
async fn an_interrupted_chunked_upload_resumes_from_the_confirmed_offset() {
    let server = MockPhotos::start();
    {
        let mut state = server.state();
        state.granularity = 16;
        state.drop_chunk_at = Some(48);
    }
    let store = signed_in_store(&server);
    let (_dir, files) = files_of(&[100]);
    let mut plugin = GooglePhotosPlugin::new(
        config(&server).with_chunk_size(16).unwrap(),
        store,
        UploadLedger::in_memory(),
    )
    .unwrap();

    let retries = Mutex::new(0);
    let sink = |e: ProgressEvent| {
        if matches!(e, ProgressEvent::Retrying { .. }) {
            *retries.lock().unwrap() += 1;
        }
    };
    plugin.upload(job("j", &files, None), &sink, &CancelToken::new()).await.unwrap();

    assert_eq!(*retries.lock().unwrap(), 1);
    let state = server.state();
    assert_eq!(state.items.len(), 1);
    assert_eq!(state.count("upload", "start"), 1, "the session is reused, not restarted");
    let offsets: Vec<u64> = state.log.iter().filter_map(|l| l.offset).collect();
    assert_eq!(offsets.iter().filter(|o| **o == 0).count(), 1, "offsets {offsets:?}");
    assert_eq!(*offsets.last().unwrap(), 96, "offsets {offsets:?}");
}

#[tokio::test]
async fn uploaded_bytes_reach_google_unchanged_and_named() {
    let fx = PhotosFixture::chunked(16, 32);
    let (_dir, files) = files_of(&[1, 33, 100]);
    fx.plugin().upload(job("j", &files, None), &NoProgress, &CancelToken::new()).await.unwrap();
    let state = fx.server.state();
    let mut stored: Vec<_> = state.items.iter().map(|i| i.key.clone()).collect();
    stored.sort();
    let mut expected: Vec<_> = files.iter().map(|f| f.content_key.clone()).collect();
    expected.sort();
    assert_eq!(stored, expected);
    let mut names: Vec<_> = state.items.iter().map(|i| i.name.as_str()).collect();
    names.sort();
    assert_eq!(names, ["IMG_0.jpg", "IMG_1.jpg", "IMG_2.jpg"]);
}

// --------------------------------------------------------------- content ledger

#[tokio::test]
async fn ledger_prevents_a_duplicate_upload_on_retry() {
    let fx = PhotosFixture::new();
    let (_dir, files) = files_of(&[40, 50]);
    fx.plugin().upload(job("first", &files, None), &NoProgress, &CancelToken::new()).await.unwrap();
    let before = fx.server.state().log.len();

    let again = fx
        .plugin()
        .upload(job("retry", &files, None), &NoProgress, &CancelToken::new())
        .await
        .unwrap();

    assert_eq!(again.count(FileStatus::AlreadyPresent), 2);
    assert_eq!(fx.server.state().log.len(), before, "no request at all for known content");
    assert_eq!(fx.server.state().items.len(), 2);
}

#[tokio::test]
async fn ledger_adds_known_content_to_a_new_album_without_uploading_again() {
    let fx = PhotosFixture::new();
    let (_dir, files) = files_of(&[40]);
    fx.plugin().upload(job("lib", &files, None), &NoProgress, &CancelToken::new()).await.unwrap();
    let uploads = fx.server.state().count("upload", "start");

    let report = fx
        .plugin()
        .upload(
            job("album", &files, Some(AlbumTarget::Create("Trip".into()))),
            &NoProgress,
            &CancelToken::new(),
        )
        .await
        .unwrap();

    assert_eq!(report.files[0].status, FileStatus::AlreadyPresent);
    let state = fx.server.state();
    assert_eq!(state.count("upload", "start"), uploads, "bytes are not sent twice");
    assert_eq!(state.items.len(), 1);
    assert_eq!(state.albums.len(), 1);
    assert_eq!(state.albums[0].items.len(), 1);
}

#[tokio::test]
async fn ledger_scopes_albums_so_a_missing_album_membership_is_not_assumed() {
    let fx = PhotosFixture::new();
    let (_dir, files) = files_of(&[40]);
    let mut plugin = fx.plugin();
    let first = plugin
        .upload(
            job("a", &files, Some(AlbumTarget::Create("A".into()))),
            &NoProgress,
            &CancelToken::new(),
        )
        .await
        .unwrap();
    let a = first.album.unwrap();
    let key = &files[0].content_key;
    assert!(plugin.exists(key, Some(&a)).await.unwrap());

    let b = plugin.create_album("b", "B").await.unwrap();
    assert!(!plugin.exists(key, Some(&b)).await.unwrap());
    assert!(plugin.exists(key, None).await.unwrap());
}

#[tokio::test]
async fn stale_ledger_entry_for_a_deleted_photo_uploads_again() {
    let fx = PhotosFixture::new();
    let (_dir, files) = files_of(&[40]);
    fx.plugin().upload(job("lib", &files, None), &NoProgress, &CancelToken::new()).await.unwrap();
    // The user deletes the photo in Google Photos, then adds it to an album.
    fx.server.state().items.clear();

    let report = fx
        .plugin()
        .upload(
            job("album", &files, Some(AlbumTarget::Create("Trip".into()))),
            &NoProgress,
            &CancelToken::new(),
        )
        .await
        .unwrap();

    assert_eq!(report.files[0].status, FileStatus::Uploaded);
    let state = fx.server.state();
    assert_eq!(state.items.len(), 1);
    assert_eq!(state.albums[0].items.len(), 1);
}

// ----------------------------------------------------------------------- albums

#[tokio::test]
async fn album_is_found_by_name_across_pages_before_one_is_created() {
    let fx = PhotosFixture::new();
    {
        let mut state = fx.server.state();
        for (id, title) in [("a1", "One"), ("a2", "Two"), ("a3", "Trip"), ("a4", "Four")] {
            state.albums.push(gphotos_mock::Album {
                id: id.into(),
                title: title.into(),
                items: Vec::new(),
            });
        }
    }
    let mut plugin = fx.plugin();
    let album = plugin.create_album("job", "Trip").await.unwrap();
    assert_eq!((album.id.as_str(), album.name.as_str()), ("a3", "Trip"));
    assert_eq!(fx.server.state().albums.len(), 4);
    assert_eq!(plugin.list_albums().await.unwrap().len(), 4);

    let created = plugin.create_album("job", "New").await.unwrap();
    assert_eq!(created.name, "New");
    assert_eq!(fx.server.state().albums.len(), 5);
}

// -------------------------------------------------------------------- one-way

#[tokio::test]
async fn plugin_is_upload_only_and_says_so() {
    let fx = PhotosFixture::new();
    let mut plugin = fx.plugin();
    let info = plugin.info();
    assert_eq!(info.id, "google-photos");
    assert!(info.name.to_lowercase().contains("upload only"), "{}", info.name);
    assert!(info.capabilities.albums && info.capabilities.size_choice);
    assert!(matches!(plugin.list_files(None).await, Err(ExportError::ReadBackUnsupported)));
    assert!(SCOPES.iter().all(|s| !s.ends_with("photoslibrary") && !s.ends_with("readonly")));
}

// ------------------------------------------------------------------------ OAuth

#[tokio::test]
async fn oauth_pkce_flow_stores_tokens_after_a_matching_state() {
    let server = MockPhotos::start();
    let store = Arc::new(MemoryTokenStore::default());
    let auth = GoogleAuth::new(config(&server), store.clone()).unwrap();
    assert!(!auth.is_signed_in().unwrap());

    let request = auth.authorization_request("http://127.0.0.1:4242/").unwrap();
    let query: std::collections::HashMap<_, _> = request.url().query_pairs().into_owned().collect();
    assert_eq!(query["client_id"], CLIENT_ID);
    assert_eq!(query["response_type"], "code");
    assert_eq!(query["redirect_uri"], "http://127.0.0.1:4242/");
    assert_eq!(query["code_challenge_method"], "S256");
    assert_eq!(query["access_type"], "offline");
    assert_eq!(query["state"], request.state());
    for scope in SCOPES {
        assert!(query["scope"].split(' ').any(|s| s == scope), "{scope}");
    }
    server.state().expected_challenge = Some(query["code_challenge"].clone());

    let error = auth.complete(&request, "auth-code", "someone-elses-state").await.unwrap_err();
    assert!(matches!(error, ExportError::Fatal(_)), "{error}");
    assert!(!auth.is_signed_in().unwrap());
    assert_eq!(server.state().token_requests, 0, "a forged state never reaches the token endpoint");

    auth.complete(&request, "auth-code", request.state()).await.unwrap();
    assert!(auth.is_signed_in().unwrap());
    let tokens = store.load().unwrap().unwrap();
    assert_eq!(tokens.refresh_token, "refresh-1");
    assert!(tokens.expires_at_unix > now() + 3000);

    auth.sign_out().unwrap();
    assert!(!auth.is_signed_in().unwrap());
}

#[tokio::test]
async fn consent_without_the_photos_scopes_is_rejected() {
    let server = MockPhotos::start();
    server.state().granted_scope = "openid".into();
    let store = Arc::new(MemoryTokenStore::default());
    let auth = GoogleAuth::new(config(&server), store.clone()).unwrap();
    let request = auth.authorization_request("http://127.0.0.1:4242/").unwrap();

    let error = auth.complete(&request, "auth-code", request.state()).await.unwrap_err();
    assert!(error.to_string().contains("Google Photos"), "{error}");
    assert!(store.load().unwrap().is_none(), "no half-authorised tokens are kept");
}

#[tokio::test]
async fn rejected_authorization_code_is_fatal() {
    let server = MockPhotos::start();
    let auth = GoogleAuth::new(config(&server), Arc::new(MemoryTokenStore::default())).unwrap();
    let request = auth.authorization_request("http://127.0.0.1:4242/").unwrap();
    let error = auth.complete(&request, "stale-code", request.state()).await.unwrap_err();
    assert!(matches!(error, ExportError::Fatal(_)), "{error}");
}

#[tokio::test]
async fn expired_access_token_is_refreshed_and_the_new_token_saved() {
    let fx = PhotosFixture::new();
    fx.store
        .save(&OAuthTokens {
            access_token: "expired".into(),
            refresh_token: "refresh-1".into(),
            expires_at_unix: now() - 10,
        })
        .unwrap();
    let (_dir, files) = files_of(&[40]);

    fx.plugin().upload(job("j", &files, None), &NoProgress, &CancelToken::new()).await.unwrap();

    let state = fx.server.state();
    assert_eq!(state.refreshes, 1);
    assert_eq!(state.items.len(), 1);
    assert!(state
        .auth_headers
        .iter()
        .flatten()
        .all(|h| *h == format!("Bearer {}", state.access_token)));
    let saved = fx.store.load().unwrap().unwrap();
    assert_eq!(saved.access_token, state.access_token);
    assert_eq!(saved.refresh_token, "refresh-1", "the refresh token survives a refresh");
    assert!(saved.expires_at_unix > now());
}

#[tokio::test]
async fn a_token_revoked_mid_session_is_refreshed_once_and_the_call_retried() {
    let fx = PhotosFixture::new();
    fx.server.state().access_token = "rotated-server-side".into();
    let (_dir, files) = files_of(&[40]);

    fx.plugin().upload(job("j", &files, None), &NoProgress, &CancelToken::new()).await.unwrap();

    assert_eq!(fx.server.state().refreshes, 1);
    assert_eq!(fx.server.state().items.len(), 1);
}

#[tokio::test]
async fn revoked_refresh_token_asks_for_a_new_sign_in_and_is_not_retried() {
    let fx = PhotosFixture::new();
    {
        let mut state = fx.server.state();
        state.access_token = "rotated-server-side".into();
        state.refresh_token = "a-different-one".into();
    }
    let (_dir, files) = files_of(&[40]);
    let retries = Mutex::new(0);
    let sink = |e: ProgressEvent| {
        if matches!(e, ProgressEvent::Retrying { .. }) {
            *retries.lock().unwrap() += 1;
        }
    };

    let error =
        fx.plugin().upload(job("j", &files, None), &sink, &CancelToken::new()).await.unwrap_err();

    assert!(matches!(error, ExportError::Fatal(_)), "{error}");
    assert!(error.to_string().contains("sign in"), "{error}");
    assert_eq!(*retries.lock().unwrap(), 0);
    assert!(fx.server.state().items.is_empty());
}

#[tokio::test]
async fn upload_without_sign_in_says_so_before_any_request() {
    let server = MockPhotos::start();
    let (_dir, files) = files_of(&[40]);
    let mut plugin =
        plugin_with(&server, Arc::new(MemoryTokenStore::default()), &UploadLedger::in_memory());
    let error =
        plugin.upload(job("j", &files, None), &NoProgress, &CancelToken::new()).await.unwrap_err();
    assert!(matches!(error, ExportError::Fatal(_)), "{error}");
    assert!(error.to_string().contains("sign in"), "{error}");
    assert!(server.state().log.is_empty());
}

#[tokio::test]
async fn tokens_never_leave_the_configured_server() {
    // Redirects are refused.
    let elsewhere = MockPhotos::start();
    let server = MockPhotos::start();
    server.state().redirect_to = Some(elsewhere.url());
    let (_dir, files) = files_of(&[40]);
    let store = signed_in_store(&server);
    let error = plugin_with(&server, store.clone(), &UploadLedger::in_memory())
        .upload(job("j", &files, None), &NoProgress, &CancelToken::new())
        .await
        .unwrap_err();
    assert!(matches!(error, ExportError::Fatal(_)), "{error}");
    assert_eq!(elsewhere.state().auth_headers.len(), 0);

    // An upload session URL on another origin is not trusted either.
    let server = MockPhotos::start();
    server.state().upload_url_origin = Some(elsewhere.url());
    let store = signed_in_store(&server);
    let error = plugin_with(&server, store, &UploadLedger::in_memory())
        .upload(job("j", &files, None), &NoProgress, &CancelToken::new())
        .await
        .unwrap_err();
    assert!(matches!(error, ExportError::Fatal(_)), "{error}");
    assert_eq!(elsewhere.state().auth_headers.len(), 0);
    assert!(elsewhere.state().log.is_empty());
}

#[test]
fn secrets_never_show_in_debug_output() {
    let config = GooglePhotosConfig::new("client", Some("super-secret")).unwrap();
    assert!(!format!("{config:?}").contains("super-secret"));
    let tokens = OAuthTokens {
        access_token: "access-secret".into(),
        refresh_token: "refresh-secret".into(),
        expires_at_unix: 1,
    };
    let debug = format!("{tokens:?}");
    assert!(!debug.contains("access-secret") && !debug.contains("refresh-secret"));
}

#[test]
fn token_json_round_trips_for_credential_stores() {
    let tokens =
        OAuthTokens { access_token: "a".into(), refresh_token: "r".into(), expires_at_unix: 42 };
    assert_eq!(OAuthTokens::from_json(&tokens.to_json()).unwrap(), tokens);
    assert!(OAuthTokens::from_json("{}").is_err());
}

#[test]
fn malformed_settings_are_rejected_before_any_network_call() {
    for id in ["", "  ", "has space"] {
        let error = GooglePhotosConfig::new(id, None).unwrap_err();
        assert!(matches!(error, ExportError::InvalidConfig(_)), "{id:?} -> {error}");
    }
    let config = GooglePhotosConfig::new("client", None).unwrap();
    assert!(matches!(config.with_chunk_size(0), Err(ExportError::InvalidConfig(_))));
}

// -------------------------------------------------------------- loopback receiver

async fn browser_hits(redirect: &str, query: &str) -> u16 {
    let _ = rustls::crypto::ring::default_provider().install_default();
    reqwest::get(format!("{redirect}{query}")).await.unwrap().status().as_u16()
}

#[tokio::test]
async fn loopback_receiver_returns_the_code_for_the_expected_state() {
    let receiver = LoopbackReceiver::bind().await.unwrap();
    let redirect = receiver.redirect_uri().to_owned();
    assert!(redirect.starts_with("http://127.0.0.1:"), "{redirect}");

    let waiting = tokio::spawn(receiver.wait_for_code("good-state", Duration::from_secs(10)));
    assert_eq!(browser_hits(&redirect, "favicon.ico").await, 404);
    assert_eq!(browser_hits(&redirect, "?code=evil&state=forged").await, 400);
    assert_eq!(browser_hits(&redirect, "?code=real-code&state=good-state").await, 200);
    assert_eq!(waiting.await.unwrap().unwrap(), "real-code");
}

#[tokio::test]
async fn loopback_receiver_reports_a_denied_consent() {
    let receiver = LoopbackReceiver::bind().await.unwrap();
    let redirect = receiver.redirect_uri().to_owned();
    let waiting = tokio::spawn(receiver.wait_for_code("s", Duration::from_secs(10)));
    browser_hits(&redirect, "?error=access_denied&state=s").await;
    let error = waiting.await.unwrap().unwrap_err();
    assert!(matches!(error, ExportError::Fatal(_)), "{error}");
    assert!(error.to_string().contains("denied"), "{error}");
}

#[tokio::test]
async fn loopback_receiver_times_out() {
    let receiver = LoopbackReceiver::bind().await.unwrap();
    let error = receiver.wait_for_code("s", Duration::from_millis(100)).await.unwrap_err();
    assert!(error.to_string().contains("timed out"), "{error}");
}
