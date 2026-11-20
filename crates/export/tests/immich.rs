//! TDD tests for TASK-08-009: the Immich plugin passes the shared contract
//! suite against a recorded-API mock of the Immich server, rejects malformed
//! settings before any network call, surfaces connection errors clearly and
//! keeps read-back possible (PLAN §1 strategic note, §6 P4).

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use fotobase_export::contract::{run_contract_suite, ContractFixture, Fault, RemoteAlbum};
use fotobase_export::immich::{ImmichConfig, ImmichPlugin};
use fotobase_export::{
    AlbumTarget, CancelToken, ContentKey, ExportError, ExportPlugin, FileStatus, NoProgress,
    ProgressEvent, RetryPolicy, UploadFile, UploadJob, UploadOptions,
};
use serde_json::{json, Value};

const API_KEY: &str = "test-api-key";

// ---------------------------------------------------------------- mock server

#[derive(Default)]
struct Faults {
    abort_after: Option<usize>,
    fail_once: bool,
    lose_upload_response_once: bool,
    lose_album_response_once: bool,
}

struct Asset {
    id: String,
    key: ContentKey,
    name: String,
    device_asset_id: String,
}

struct Album {
    id: String,
    name: String,
    assets: Vec<String>,
}

#[derive(Default)]
struct State {
    assets: Vec<Asset>,
    albums: Vec<Album>,
    faults: Faults,
    /// Answer every request with a redirect to this URL.
    redirect_to: Option<String>,
    requests: usize,
    api_keys: Vec<Option<String>>,
}

struct MockImmich {
    addr: String,
    state: Arc<Mutex<State>>,
    stop: Arc<AtomicBool>,
}

impl MockImmich {
    fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        let state = Arc::new(Mutex::new(State::default()));
        let stop = Arc::new(AtomicBool::new(false));
        let (accept_state, accept_stop) = (state.clone(), stop.clone());
        thread::spawn(move || {
            for stream in listener.incoming() {
                if accept_stop.load(Ordering::SeqCst) {
                    break;
                }
                let Ok(stream) = stream else { continue };
                let state = accept_state.clone();
                thread::spawn(move || serve(stream, &state));
            }
        });
        Self { addr, state, stop }
    }

    fn url(&self) -> String {
        format!("http://{}", self.addr)
    }

    fn state(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap()
    }

    fn plugin(&self) -> ImmichPlugin {
        self.plugin_with_key(API_KEY)
    }

    fn plugin_with_key(&self, key: &str) -> ImmichPlugin {
        ImmichPlugin::new(ImmichConfig::new(&self.url(), key).unwrap()).unwrap()
    }
}

impl Drop for MockImmich {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        let _ = TcpStream::connect(&self.addr);
    }
}

struct Request {
    method: String,
    path: String,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

impl Request {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_str())
    }
}

fn find(haystack: &[u8], needle: &[u8], from: usize) -> Option<usize> {
    haystack[from..].windows(needle.len()).position(|w| w == needle).map(|i| i + from)
}

fn read_request(stream: &mut TcpStream) -> Option<Request> {
    stream.set_read_timeout(Some(Duration::from_secs(10))).ok()?;
    let mut buf = Vec::new();
    let mut chunk = [0u8; 16 * 1024];
    let head_end = loop {
        if let Some(i) = find(&buf, b"\r\n\r\n", 0) {
            break i;
        }
        let n = stream.read(&mut chunk).ok()?;
        if n == 0 {
            return None;
        }
        buf.extend_from_slice(&chunk[..n]);
    };
    let head = String::from_utf8_lossy(&buf[..head_end]).into_owned();
    let mut lines = head.split("\r\n");
    let mut request_line = lines.next()?.split(' ');
    let method = request_line.next()?.to_owned();
    let path = request_line.next()?.to_owned();
    let headers: Vec<(String, String)> = lines
        .filter_map(|l| l.split_once(':'))
        .map(|(k, v)| (k.trim().to_ascii_lowercase(), v.trim().to_owned()))
        .collect();
    let mut rest = buf[head_end + 4..].to_vec();
    let get = |name: &str| headers.iter().find(|(k, _)| k == name).map(|(_, v)| v.clone());

    let body = if get("transfer-encoding").is_some_and(|v| v.contains("chunked")) {
        let mut body = Vec::new();
        loop {
            while find(&rest, b"\r\n", 0).is_none() {
                let n = stream.read(&mut chunk).ok()?;
                rest.extend_from_slice(&chunk[..n]);
            }
            let line_end = find(&rest, b"\r\n", 0)?;
            let size =
                usize::from_str_radix(std::str::from_utf8(&rest[..line_end]).ok()?.trim(), 16)
                    .ok()?;
            rest.drain(..line_end + 2);
            while rest.len() < size + 2 {
                let n = stream.read(&mut chunk).ok()?;
                if n == 0 {
                    return None;
                }
                rest.extend_from_slice(&chunk[..n]);
            }
            if size == 0 {
                break;
            }
            body.extend_from_slice(&rest[..size]);
            rest.drain(..size + 2);
        }
        body
    } else {
        let len: usize = get("content-length").and_then(|v| v.parse().ok()).unwrap_or(0);
        while rest.len() < len {
            let n = stream.read(&mut chunk).ok()?;
            if n == 0 {
                return None;
            }
            rest.extend_from_slice(&chunk[..n]);
        }
        rest.truncate(len);
        rest
    };
    Some(Request { method, path, headers, body })
}

struct Part {
    name: String,
    filename: Option<String>,
    data: Vec<u8>,
}

fn multipart(request: &Request) -> Vec<Part> {
    let content_type = request.header("content-type").unwrap_or_default();
    let boundary = content_type.split("boundary=").nth(1).unwrap().trim_matches('"');
    let delimiter = format!("--{boundary}").into_bytes();
    let body = &request.body;
    let mut parts = Vec::new();
    let mut at = find(body, &delimiter, 0).unwrap() + delimiter.len();
    while body[at..].starts_with(b"\r\n") {
        let head_end = find(body, b"\r\n\r\n", at).unwrap();
        let head = String::from_utf8_lossy(&body[at + 2..head_end]).into_owned();
        let next = find(body, &delimiter, head_end).unwrap();
        let data = body[head_end + 4..next - 2].to_vec();
        let field = |key: &str| {
            head.split(&format!("{key}=\""))
                .nth(1)
                .and_then(|s| s.split('"').next())
                .map(str::to_owned)
        };
        parts.push(Part { name: field("name").unwrap(), filename: field("filename"), data });
        at = next + delimiter.len();
    }
    parts
}

fn respond(stream: &mut TcpStream, status: u16, body: &Value) {
    let text = body.to_string();
    let reply = format!(
        "HTTP/1.1 {status} X\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{text}",
        text.len()
    );
    let _ = stream.write_all(reply.as_bytes());
}

/// Dropping the stream without answering is how the mock "loses" a response.
fn serve(mut stream: TcpStream, state: &Mutex<State>) {
    let Some(request) = read_request(&mut stream) else { return };
    let mut state = state.lock().unwrap();
    state.requests += 1;
    state.api_keys.push(request.header("x-api-key").map(str::to_owned));

    if let Some(target) = state.redirect_to.clone() {
        let reply = format!(
            "HTTP/1.1 302 Found\r\nlocation: {target}{}\r\ncontent-length: 0\r\nconnection: close\r\n\r\n",
            request.path
        );
        let _ = stream.write_all(reply.as_bytes());
        return;
    }
    if request.header("x-api-key") != Some(API_KEY) {
        return respond(&mut stream, 401, &json!({"message": "Invalid API key"}));
    }

    let path = request.path.split('?').next().unwrap().to_owned();
    let json_body = || serde_json::from_slice::<Value>(&request.body).unwrap_or(Value::Null);
    match (request.method.as_str(), path.as_str()) {
        ("GET", "/api/users/me") => {
            respond(&mut stream, 200, &json!({"id": "user-1", "email": "me@example.com"}))
        }
        ("GET", "/api/albums") => {
            let albums: Vec<Value> = state
                .albums
                .iter()
                .map(|a| json!({"id": a.id, "albumName": a.name, "assetCount": a.assets.len()}))
                .collect();
            respond(&mut stream, 200, &json!(albums))
        }
        ("POST", "/api/albums") => {
            let name = json_body()["albumName"].as_str().unwrap().to_owned();
            let id = format!("album-{}", state.albums.len() + 1);
            state.albums.push(Album { id: id.clone(), name: name.clone(), assets: Vec::new() });
            if std::mem::take(&mut state.faults.lose_album_response_once) {
                return;
            }
            respond(&mut stream, 201, &json!({"id": id, "albumName": name, "assets": []}))
        }
        ("POST", "/api/assets") => upload(&mut state, &request, &mut stream),
        (method, p) if p.starts_with("/api/albums/") => {
            let rest = &p["/api/albums/".len()..];
            let (id, tail) = rest.split_once('/').unwrap_or((rest, ""));
            let Some(i) = state.albums.iter().position(|a| a.id == id) else {
                return respond(
                    &mut stream,
                    400,
                    &json!({"message": "Not found or no album.read access"}),
                );
            };
            match (method, tail) {
                ("GET", "") => {
                    let assets: Vec<Value> = state.albums[i]
                        .assets
                        .iter()
                        .map(|aid| {
                            let a = state.assets.iter().find(|a| &a.id == aid).unwrap();
                            json!({"id": a.id, "originalFileName": a.name, "deviceAssetId": a.device_asset_id})
                        })
                        .collect();
                    let album = &state.albums[i];
                    respond(
                        &mut stream,
                        200,
                        &json!({"id": album.id, "albumName": album.name, "assets": assets}),
                    )
                }
                ("PUT", "assets") => {
                    let ids: Vec<String> = json_body()["ids"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .map(|v| v.as_str().unwrap().to_owned())
                        .collect();
                    let results: Vec<Value> = ids
                        .into_iter()
                        .map(|id| {
                            if state.albums[i].assets.contains(&id) {
                                json!({"id": id, "success": false, "error": "duplicate"})
                            } else {
                                state.albums[i].assets.push(id.clone());
                                json!({"id": id, "success": true})
                            }
                        })
                        .collect();
                    respond(&mut stream, 200, &json!(results))
                }
                _ => respond(&mut stream, 404, &json!({"message": "Cannot route"})),
            }
        }
        _ => respond(&mut stream, 404, &json!({"message": "Cannot route"})),
    }
}

fn upload(state: &mut State, request: &Request, stream: &mut TcpStream) {
    if std::mem::take(&mut state.faults.fail_once) {
        return respond(stream, 503, &json!({"message": "Service Unavailable"}));
    }
    match state.faults.abort_after {
        Some(0) => return,
        Some(n) => state.faults.abort_after = Some(n - 1),
        None => {}
    }

    let parts = multipart(request);
    let field = |name: &str| parts.iter().find(|p| p.name == name);
    for required in ["deviceAssetId", "deviceId", "fileCreatedAt", "fileModifiedAt", "assetData"] {
        if field(required).is_none() {
            return respond(
                stream,
                400,
                &json!({"message": format!("{required} should not be empty")}),
            );
        }
    }
    let data = field("assetData").unwrap();
    let key = ContentKey::from_bytes(&data.data);
    let device_asset_id = String::from_utf8(field("deviceAssetId").unwrap().data.clone()).unwrap();

    let (status, id, label) = match state.assets.iter().find(|a| a.key == key) {
        Some(existing) => (200, existing.id.clone(), "duplicate"),
        None => {
            let id = format!("asset-{}", state.assets.len() + 1);
            state.assets.push(Asset {
                id: id.clone(),
                key,
                name: data.filename.clone().unwrap_or_default(),
                device_asset_id,
            });
            (201, id, "created")
        }
    };
    if std::mem::take(&mut state.faults.lose_upload_response_once) {
        return;
    }
    respond(stream, status, &json!({"id": id, "status": label}))
}

// ------------------------------------------------------------------- fixtures

struct ImmichFixture {
    server: MockImmich,
}

impl ImmichFixture {
    fn new() -> Self {
        Self { server: MockImmich::start() }
    }
}

impl ContractFixture for ImmichFixture {
    type Plugin = ImmichPlugin;

    fn plugin(&self) -> ImmichPlugin {
        self.server.plugin()
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
        self.server.state().assets.iter().map(|a| a.key.clone()).collect()
    }

    fn remote_albums(&self) -> Vec<RemoteAlbum> {
        let state = self.server.state();
        state
            .albums
            .iter()
            .map(|album| RemoteAlbum {
                name: album.name.clone(),
                files: album
                    .assets
                    .iter()
                    .map(|id| state.assets.iter().find(|a| &a.id == id).unwrap().key.clone())
                    .collect(),
            })
            .collect()
    }
}

fn files(count: usize) -> (tempfile::TempDir, Vec<UploadFile>) {
    let dir = tempfile::tempdir().unwrap();
    let files = (0..count)
        .map(|i| {
            let path = dir.path().join(format!("IMG_{i}.jpg"));
            std::fs::write(&path, format!("immich payload {i}")).unwrap();
            UploadFile::from_path(&path).unwrap()
        })
        .collect();
    (dir, files)
}

fn job(id: &str, files: Vec<UploadFile>, album: Option<AlbumTarget>) -> UploadJob {
    UploadJob {
        id: id.into(),
        files,
        album,
        options: UploadOptions::default(),
        retry: RetryPolicy { max_attempts: 3, base_delay: Duration::ZERO },
    }
}

// ---------------------------------------------------------------------- tests

#[tokio::test]
async fn immich_plugin_passes_the_contract_suite() {
    run_contract_suite(ImmichFixture::new).await.unwrap();
}

#[test]
fn malformed_settings_are_rejected_before_any_network_call() {
    let bad_urls = [
        "",
        "   ",
        "immich.local",
        "ftp://immich.local",
        "http://",
        "https://user:secret@immich.local",
        "https://immich.local/?next=evil",
        "https://immich.local/#frag",
    ];
    for url in bad_urls {
        let error = ImmichConfig::new(url, "key").unwrap_err();
        assert!(matches!(error, ExportError::InvalidConfig(_)), "{url:?} -> {error}");
    }
    for key in ["", "  ", "has space", "line\nbreak", "tab\tkey"] {
        let error = ImmichConfig::new("https://immich.local", key).unwrap_err();
        assert!(matches!(error, ExportError::InvalidConfig(_)), "{key:?} -> {error}");
    }
}

#[test]
fn server_url_is_normalised_to_the_instance_root() {
    for url in [
        "https://photos.example.com",
        "https://photos.example.com/",
        " https://photos.example.com/api ",
        "https://photos.example.com/api/",
    ] {
        assert_eq!(
            ImmichConfig::new(url, "key").unwrap().server_url(),
            "https://photos.example.com",
            "{url:?}"
        );
    }
    assert_eq!(
        ImmichConfig::new("http://nas.lan:2283/immich/", "key").unwrap().server_url(),
        "http://nas.lan:2283/immich"
    );
}

#[test]
fn api_key_never_shows_in_debug_output() {
    let config = ImmichConfig::new("https://immich.local", "super-secret-key").unwrap();
    assert!(!format!("{config:?}").contains("super-secret-key"));
}

#[tokio::test]
async fn valid_connection_is_confirmed() {
    let server = MockImmich::start();
    server.plugin().validate_connection().await.unwrap();
}

#[tokio::test]
async fn wrong_api_key_is_reported_as_such_and_not_retried() {
    let server = MockImmich::start();
    let plugin = server.plugin_with_key("wrong");
    let error = plugin.validate_connection().await.unwrap_err();
    assert!(matches!(error, ExportError::Fatal(_)), "{error}");
    assert!(error.to_string().contains("API key"), "{error}");

    let (_dir, files) = files(1);
    let retries = Mutex::new(0);
    let sink = |e: ProgressEvent| {
        if matches!(e, ProgressEvent::Retrying { .. }) {
            *retries.lock().unwrap() += 1;
        }
    };
    let result = server
        .plugin_with_key("wrong")
        .upload(job("j", files, None), &sink, &CancelToken::new())
        .await;
    assert!(matches!(result, Err(ExportError::Fatal(_))));
    assert_eq!(*retries.lock().unwrap(), 0);
    assert!(server.state().assets.is_empty());
}

#[tokio::test]
async fn unreachable_server_error_names_the_server() {
    let url = {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        format!("http://{}", listener.local_addr().unwrap())
    };
    let plugin = ImmichPlugin::new(ImmichConfig::new(&url, API_KEY).unwrap()).unwrap();
    let error = plugin.validate_connection().await.unwrap_err();
    assert!(error.to_string().contains(&url), "{error}");
}

#[tokio::test]
async fn non_immich_server_is_reported() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    thread::spawn(move || {
        for stream in listener.incoming() {
            let mut stream = stream.unwrap();
            if read_request(&mut stream).is_some() {
                respond(&mut stream, 404, &json!({"error": "nope"}));
            }
        }
    });
    let plugin = ImmichPlugin::new(ImmichConfig::new(&url, API_KEY).unwrap()).unwrap();
    let error = plugin.validate_connection().await.unwrap_err();
    assert!(error.to_string().contains("Immich"), "{error}");
}

#[tokio::test]
async fn redirects_are_refused_so_the_api_key_never_leaves_the_server() {
    let elsewhere = MockImmich::start();
    let server = MockImmich::start();
    server.state().redirect_to = Some(elsewhere.url());

    let error = server.plugin().validate_connection().await.unwrap_err();
    assert!(matches!(error, ExportError::Fatal(_)), "{error}");
    assert_eq!(elsewhere.state().requests, 0);
}

#[tokio::test]
async fn every_request_carries_the_api_key_and_goes_to_the_configured_server() {
    let fx = ImmichFixture::new();
    let (_dir, files) = files(2);
    fx.plugin()
        .upload(
            job("j", files, Some(AlbumTarget::Create("Trip".into()))),
            &NoProgress,
            &CancelToken::new(),
        )
        .await
        .unwrap();
    let state = fx.server.state();
    assert!(state.requests > 0);
    assert!(state.api_keys.iter().all(|k| k.as_deref() == Some(API_KEY)));
}

#[tokio::test]
async fn album_lookup_reuses_an_existing_album_with_the_same_name() {
    let server = MockImmich::start();
    server.state().albums.push(Album {
        id: "existing".into(),
        name: "Trip".into(),
        assets: Vec::new(),
    });
    let album = server.plugin().create_album("job-1", "Trip").await.unwrap();
    assert_eq!(album.id, "existing");
    assert_eq!(server.state().albums.len(), 1);
}

#[tokio::test]
async fn duplicate_uploads_produce_one_asset() {
    let server = MockImmich::start();
    let (_dir, files) = files(1);
    let mut plugin = server.plugin();
    let first = plugin
        .upload(job("a", files.clone(), None), &NoProgress, &CancelToken::new())
        .await
        .unwrap();
    let second =
        plugin.upload(job("b", files, None), &NoProgress, &CancelToken::new()).await.unwrap();
    assert_eq!(first.files[0].status, FileStatus::Uploaded);
    assert_eq!(second.files[0].status, FileStatus::AlreadyPresent);
    assert_eq!(first.files[0].remote_id, second.files[0].remote_id);
    assert_eq!(server.state().assets.len(), 1);
}

#[tokio::test]
async fn read_back_lists_album_files_with_their_content_keys() {
    let server = MockImmich::start();
    let (_dir, files) = files(2);
    let mut plugin = server.plugin();
    let report = plugin
        .upload(
            job("j", files.clone(), Some(AlbumTarget::Create("Trip".into()))),
            &NoProgress,
            &CancelToken::new(),
        )
        .await
        .unwrap();
    let album = report.album.unwrap();

    let listed = plugin.list_files(Some(&album)).await.unwrap();
    assert_eq!(listed.len(), 2);
    for file in &files {
        let remote = listed.iter().find(|r| r.name == file.name).unwrap();
        assert_eq!(remote.content_key.as_ref(), Some(&file.content_key));
        assert!(plugin.exists(&file.content_key, Some(&album)).await.unwrap());
    }
    assert!(!plugin.exists(&ContentKey::from_bytes(b"other"), Some(&album)).await.unwrap());
}

#[tokio::test]
async fn read_back_across_the_whole_library_is_not_supported_yet() {
    let server = MockImmich::start();
    let error = server.plugin().list_files(None).await.unwrap_err();
    assert!(matches!(error, ExportError::ReadBackUnsupported), "{error}");
}

#[tokio::test]
async fn upload_sends_the_content_key_as_device_asset_id() {
    let server = MockImmich::start();
    let (_dir, files) = files(1);
    server
        .plugin()
        .upload(job("j", files.clone(), None), &NoProgress, &CancelToken::new())
        .await
        .unwrap();
    let state = server.state();
    assert_eq!(state.assets[0].device_asset_id, files[0].content_key.as_str());
    assert_eq!(
        state.assets[0].key,
        ContentKey::from_bytes(&std::fs::read(&files[0].path).unwrap())
    );
}
