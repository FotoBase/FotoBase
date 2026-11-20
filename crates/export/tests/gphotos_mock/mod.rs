//! Recorded-API mock of the Google Photos Library API and Google's OAuth token
//! endpoint (TASK-08-012): the request/response shapes follow the published
//! REST reference — resumable `uploads` (`X-Goog-Upload-*` headers), the
//! `mediaItems:batchCreate` step, albums, and "the same bytes are the same
//! media item". No live Google in CI.

#![allow(dead_code)]

use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread;
use std::time::Duration;

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine as _;
use fotobase_export::google_photos::{GooglePhotosConfig, SCOPES};
use fotobase_export::ContentKey;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

pub const CLIENT_ID: &str = "client-id";
const PAGE_SIZE: usize = 2;

#[derive(Default)]
pub struct Faults {
    pub abort_after: Option<usize>,
    pub fail_once: bool,
    pub lose_upload_response_once: bool,
    pub lose_album_response_once: bool,
}

pub struct Item {
    pub id: String,
    pub key: ContentKey,
    pub name: String,
}

pub struct Album {
    pub id: String,
    pub title: String,
    pub items: Vec<String>,
}

struct Session {
    size: u64,
    data: Vec<u8>,
}

pub struct Logged {
    pub route: &'static str,
    pub command: String,
    pub offset: Option<u64>,
}

pub struct State {
    pub items: Vec<Item>,
    pub albums: Vec<Album>,
    sessions: BTreeMap<String, Session>,
    tokens: BTreeMap<String, Vec<u8>>,
    pub faults: Faults,
    pub access_token: String,
    pub refresh_token: String,
    pub granted_scope: String,
    pub expected_challenge: Option<String>,
    pub granularity: u64,
    /// Drop the connection of the next chunk request at this offset, once.
    pub drop_chunk_at: Option<u64>,
    /// Answer every request with a redirect to this URL.
    pub redirect_to: Option<String>,
    /// Advertise upload session URLs on this origin instead of ours.
    pub upload_url_origin: Option<String>,
    pub token_requests: usize,
    pub refreshes: usize,
    pub auth_headers: Vec<Option<String>>,
    pub log: Vec<Logged>,
    issued: usize,
}

impl State {
    pub fn count(&self, route: &str, command: &str) -> usize {
        self.log.iter().filter(|l| l.route == route && l.command == command).count()
    }
}

impl Default for State {
    fn default() -> Self {
        Self {
            items: Vec::new(),
            albums: Vec::new(),
            sessions: BTreeMap::new(),
            tokens: BTreeMap::new(),
            faults: Faults::default(),
            access_token: "access-0".into(),
            refresh_token: "refresh-1".into(),
            granted_scope: SCOPES.join(" "),
            expected_challenge: None,
            granularity: 256 * 1024,
            drop_chunk_at: None,
            redirect_to: None,
            upload_url_origin: None,
            token_requests: 0,
            refreshes: 0,
            auth_headers: Vec::new(),
            log: Vec::new(),
            issued: 0,
        }
    }
}

pub struct MockPhotos {
    addr: String,
    state: Arc<Mutex<State>>,
    stop: Arc<AtomicBool>,
}

impl MockPhotos {
    pub fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        let state = Arc::new(Mutex::new(State::default()));
        let stop = Arc::new(AtomicBool::new(false));
        let (accept_state, accept_stop) = (state.clone(), stop.clone());
        let base = format!("http://{addr}");
        thread::spawn(move || {
            for stream in listener.incoming() {
                if accept_stop.load(Ordering::SeqCst) {
                    break;
                }
                let Ok(stream) = stream else { continue };
                let (state, base) = (accept_state.clone(), base.clone());
                thread::spawn(move || serve(stream, &state, &base));
            }
        });
        Self { addr, state, stop }
    }

    pub fn url(&self) -> String {
        format!("http://{}", self.addr)
    }

    pub fn state(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap()
    }

    pub fn access_token(&self) -> String {
        self.state().access_token.clone()
    }

    pub fn config(&self) -> GooglePhotosConfig {
        let url = self.url();
        GooglePhotosConfig::new(CLIENT_ID, Some("client-secret"))
            .unwrap()
            .with_endpoints(&format!("{url}/auth"), &format!("{url}/token"), &url)
            .unwrap()
    }
}

impl Drop for MockPhotos {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        let _ = TcpStream::connect(&self.addr);
    }
}

struct Request {
    method: String,
    target: String,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

impl Request {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_str())
    }

    fn path(&self) -> &str {
        self.target.split('?').next().unwrap()
    }

    fn query(&self, name: &str) -> Option<String> {
        let query = self.target.split_once('?')?.1;
        url::form_urlencoded::parse(query.as_bytes())
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.into_owned())
    }

    fn json(&self) -> Value {
        serde_json::from_slice(&self.body).unwrap_or(Value::Null)
    }
}

fn read_request(stream: &mut TcpStream) -> Option<Request> {
    stream.set_read_timeout(Some(Duration::from_secs(10))).ok()?;
    let mut buf = Vec::new();
    let mut chunk = [0u8; 16 * 1024];
    let head_end = loop {
        if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
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
    let target = request_line.next()?.to_owned();
    let headers: Vec<(String, String)> = lines
        .filter_map(|l| l.split_once(':'))
        .map(|(k, v)| (k.trim().to_ascii_lowercase(), v.trim().to_owned()))
        .collect();
    let len: usize = headers
        .iter()
        .find(|(k, _)| k == "content-length")
        .and_then(|(_, v)| v.parse().ok())
        .unwrap_or(0);
    let mut body = buf[head_end + 4..].to_vec();
    while body.len() < len {
        let n = stream.read(&mut chunk).ok()?;
        if n == 0 {
            return None;
        }
        body.extend_from_slice(&chunk[..n]);
    }
    body.truncate(len);
    Some(Request { method, target, headers, body })
}

fn respond(stream: &mut TcpStream, status: u16, headers: &[(&str, String)], body: &str) {
    let mut reply = format!("HTTP/1.1 {status} X\r\n");
    for (name, value) in headers {
        reply.push_str(&format!("{name}: {value}\r\n"));
    }
    reply.push_str(&format!("content-length: {}\r\nconnection: close\r\n\r\n{body}", body.len()));
    let _ = stream.write_all(reply.as_bytes());
}

fn respond_json(stream: &mut TcpStream, status: u16, body: &Value) {
    respond(stream, status, &[("content-type", "application/json".into())], &body.to_string());
}

fn api_error(stream: &mut TcpStream, status: u16, code: &str) {
    respond_json(
        stream,
        status,
        &json!({"error": {"code": status, "message": code, "status": code}}),
    );
}

fn serve(mut stream: TcpStream, state: &Mutex<State>, base: &str) {
    let Some(request) = read_request(&mut stream) else { return };
    let mut state = state.lock().unwrap();
    state.auth_headers.push(request.header("authorization").map(str::to_owned));

    if let Some(target) = state.redirect_to.clone() {
        let reply = format!(
            "HTTP/1.1 302 Found\r\nlocation: {target}{}\r\ncontent-length: 0\r\nconnection: close\r\n\r\n",
            request.target
        );
        let _ = stream.write_all(reply.as_bytes());
        return;
    }
    if request.method == "POST" && request.path() == "/token" {
        return token_endpoint(&mut state, &request, &mut stream);
    }
    if request.header("authorization") != Some(format!("Bearer {}", state.access_token).as_str()) {
        return api_error(&mut stream, 401, "UNAUTHENTICATED");
    }

    let path = request.path().to_owned();
    match (request.method.as_str(), path.as_str()) {
        ("POST", "/v1/uploads") => uploads(&mut state, &request, &mut stream, base),
        ("POST", "/v1/mediaItems:batchCreate") => batch_create(&mut state, &request, &mut stream),
        ("GET", "/v1/albums") => list_albums(&state, &request, &mut stream),
        ("POST", "/v1/albums") => create_album(&mut state, &request, &mut stream),
        ("POST", p) if p.starts_with("/v1/albums/") && p.ends_with(":batchAddMediaItems") => {
            let id = &p["/v1/albums/".len()..p.len() - ":batchAddMediaItems".len()];
            batch_add(&mut state, id, &request, &mut stream)
        }
        _ => api_error(&mut stream, 404, "NOT_FOUND"),
    }
}

fn token_endpoint(state: &mut State, request: &Request, stream: &mut TcpStream) {
    state.token_requests += 1;
    let form: BTreeMap<String, String> =
        url::form_urlencoded::parse(&request.body).into_owned().collect();
    let invalid = |stream: &mut TcpStream| {
        respond_json(
            stream,
            400,
            &json!({"error": "invalid_grant", "error_description": "Bad Request"}),
        )
    };
    if form.get("client_id").map(String::as_str) != Some(CLIENT_ID)
        || form.get("client_secret").map(String::as_str) != Some("client-secret")
    {
        return respond_json(stream, 401, &json!({"error": "invalid_client"}));
    }
    match form.get("grant_type").map(String::as_str) {
        Some("authorization_code") => {
            let challenge_ok = state.expected_challenge.as_ref().is_none_or(|expected| {
                let verifier = form.get("code_verifier").map(String::as_str).unwrap_or_default();
                &URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes())) == expected
            });
            if form.get("code").map(String::as_str) != Some("auth-code")
                || !form.contains_key("redirect_uri")
                || !challenge_ok
            {
                return invalid(stream);
            }
            state.issued += 1;
            state.access_token = format!("access-{}", state.issued);
            respond_json(
                stream,
                200,
                &json!({
                    "access_token": state.access_token,
                    "refresh_token": state.refresh_token,
                    "expires_in": 3599,
                    "scope": state.granted_scope,
                    "token_type": "Bearer",
                }),
            )
        }
        Some("refresh_token") => {
            if form.get("refresh_token") != Some(&state.refresh_token) {
                return invalid(stream);
            }
            state.refreshes += 1;
            state.issued += 1;
            state.access_token = format!("access-{}", state.issued);
            respond_json(
                stream,
                200,
                &json!({
                    "access_token": state.access_token,
                    "expires_in": 3599,
                    "scope": state.granted_scope,
                    "token_type": "Bearer",
                }),
            )
        }
        _ => respond_json(stream, 400, &json!({"error": "unsupported_grant_type"})),
    }
}

fn uploads(state: &mut State, request: &Request, stream: &mut TcpStream, base: &str) {
    let command = request.header("x-goog-upload-command").unwrap_or_default().to_owned();
    let offset: Option<u64> = request.header("x-goog-upload-offset").and_then(|v| v.parse().ok());
    state.log.push(Logged { route: "upload", command: command.clone(), offset });
    let upload_id = request.query("upload_id");

    if command == "start" {
        if std::mem::take(&mut state.faults.fail_once) {
            return api_error(stream, 503, "UNAVAILABLE");
        }
        let size: u64 = request
            .header("x-goog-upload-raw-size")
            .and_then(|v| v.parse().ok())
            .expect("start must announce the raw size");
        assert_eq!(request.header("x-goog-upload-protocol"), Some("resumable"));
        assert!(request.header("x-goog-upload-content-type").is_some());
        let id = format!("session-{}", state.sessions.len() + 1);
        state.sessions.insert(id.clone(), Session { size, data: Vec::new() });
        let origin = state.upload_url_origin.clone().unwrap_or_else(|| base.to_owned());
        return respond(
            stream,
            200,
            &[
                (
                    "x-goog-upload-url",
                    format!("{origin}/v1/uploads?upload_id={id}&upload_protocol=resumable"),
                ),
                ("x-goog-upload-chunk-granularity", state.granularity.to_string()),
                ("x-goog-upload-status", "active".into()),
            ],
            "",
        );
    }

    let Some(id) = upload_id else { return api_error(stream, 400, "INVALID_ARGUMENT") };
    let Some(session) = state.sessions.get_mut(&id) else {
        return api_error(stream, 404, "NOT_FOUND");
    };
    if command == "query" {
        return respond(
            stream,
            200,
            &[
                ("x-goog-upload-status", "active".into()),
                ("x-goog-upload-size-received", session.data.len().to_string()),
            ],
            "",
        );
    }

    let finalize = match command.as_str() {
        "upload" => false,
        "upload, finalize" => true,
        _ => return api_error(stream, 400, "INVALID_ARGUMENT"),
    };
    let offset = offset.expect("chunks carry their offset");
    if state.drop_chunk_at == Some(offset) {
        state.drop_chunk_at = None;
        return;
    }
    if offset != session.data.len() as u64 {
        return api_error(stream, 400, "INVALID_ARGUMENT");
    }
    if finalize {
        match state.faults.abort_after {
            Some(0) => return,
            Some(n) => state.faults.abort_after = Some(n - 1),
            None => {}
        }
    }
    let session = state.sessions.get_mut(&id).unwrap();
    session.data.extend_from_slice(&request.body);
    if !finalize {
        return respond(stream, 200, &[("x-goog-upload-status", "active".into())], "");
    }
    assert_eq!(session.data.len() as u64, session.size, "finalised with the wrong size");
    let bytes = std::mem::take(&mut session.data);
    let token = format!("token-{}", state.tokens.len() + 1);
    state.tokens.insert(token.clone(), bytes);
    respond(stream, 200, &[("x-goog-upload-status", "final".into())], &token)
}

fn batch_create(state: &mut State, request: &Request, stream: &mut TcpStream) {
    state.log.push(Logged { route: "batchCreate", command: String::new(), offset: None });
    let body = request.json();
    let album_id = body["albumId"].as_str().map(str::to_owned);
    if let Some(id) = &album_id {
        if !state.albums.iter().any(|a| &a.id == id) {
            return api_error(stream, 400, "INVALID_ARGUMENT");
        }
    }
    let mut results = Vec::new();
    for new in body["newMediaItems"].as_array().expect("newMediaItems") {
        let token = new["simpleMediaItem"]["uploadToken"].as_str().unwrap().to_owned();
        let name = new["simpleMediaItem"]["fileName"].as_str().unwrap_or_default().to_owned();
        let Some(bytes) = state.tokens.get(&token) else {
            results.push(json!({"uploadToken": token, "status": {"code": 3, "message": "Invalid upload token"}}));
            continue;
        };
        let key = ContentKey::from_bytes(bytes);
        let id = match state.items.iter().find(|i| i.key == key) {
            Some(existing) => existing.id.clone(),
            None => {
                let id = format!("item-{}", state.items.len() + 1);
                state.items.push(Item { id: id.clone(), key, name: name.clone() });
                id
            }
        };
        if let Some(album) =
            album_id.as_ref().and_then(|a| state.albums.iter_mut().find(|x| &x.id == a))
        {
            if !album.items.contains(&id) {
                album.items.push(id.clone());
            }
        }
        results.push(json!({
            "uploadToken": token,
            "status": {"message": "Success"},
            "mediaItem": {"id": id, "filename": name},
        }));
    }
    if std::mem::take(&mut state.faults.lose_upload_response_once) {
        return;
    }
    respond_json(stream, 200, &json!({"newMediaItemResults": results}))
}

fn list_albums(state: &State, request: &Request, stream: &mut TcpStream) {
    let start: usize = request.query("pageToken").and_then(|t| t.parse().ok()).unwrap_or(0);
    let page: Vec<Value> = state
        .albums
        .iter()
        .skip(start)
        .take(PAGE_SIZE)
        .map(|a| json!({"id": a.id, "title": a.title, "isWriteable": true}))
        .collect();
    let mut body = json!({});
    if !page.is_empty() {
        body["albums"] = json!(page);
    }
    if start + PAGE_SIZE < state.albums.len() {
        body["nextPageToken"] = json!((start + PAGE_SIZE).to_string());
    }
    respond_json(stream, 200, &body)
}

fn create_album(state: &mut State, request: &Request, stream: &mut TcpStream) {
    let title = request.json()["album"]["title"].as_str().expect("album title").to_owned();
    let id = format!("album-{}", state.albums.len() + 1);
    state.albums.push(Album { id: id.clone(), title: title.clone(), items: Vec::new() });
    if std::mem::take(&mut state.faults.lose_album_response_once) {
        return;
    }
    respond_json(stream, 200, &json!({"id": id, "title": title, "isWriteable": true}))
}

fn batch_add(state: &mut State, album_id: &str, request: &Request, stream: &mut TcpStream) {
    state.log.push(Logged { route: "batchAdd", command: String::new(), offset: None });
    let ids: Vec<String> = request.json()["mediaItemIds"]
        .as_array()
        .expect("mediaItemIds")
        .iter()
        .map(|v| v.as_str().unwrap().to_owned())
        .collect();
    let known: Vec<String> = state.items.iter().map(|i| i.id.clone()).collect();
    let Some(album) = state.albums.iter_mut().find(|a| a.id == album_id) else {
        return api_error(stream, 400, "INVALID_ARGUMENT");
    };
    if ids.iter().any(|id| !known.contains(id)) {
        return api_error(stream, 400, "INVALID_ARGUMENT");
    }
    for id in ids {
        if !album.items.contains(&id) {
            album.items.push(id);
        }
    }
    respond_json(stream, 200, &json!({}))
}
