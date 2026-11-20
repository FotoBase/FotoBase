//! WebDAV / Nextcloud / ownCloud export plugin (TASK-08-011, PLAN §6 P4
//! "plugins: … WebDAV/Nextcloud").
//!
//! - **Settings** ([`WebDavConfig`]) are validated without touching the
//!   network; [`WebDavPlugin::validate_connection`] then checks address and
//!   login and fails with an error that says which one is wrong. For Nextcloud
//!   and ownCloud the URL is `https://<host>/remote.php/dav/files/<user>/`.
//!   Plain `http://` would send the password in the clear, so it is refused
//!   unless the host is local or [`WebDavConfig::with_insecure_http`] says the
//!   network is trusted.
//! - **Authentication follows the server.** Requests start without
//!   credentials; the `401` answer names the scheme and the plugin continues
//!   with Basic (preferred on https) or Digest (MD5 and SHA-256, `qop=auth`),
//!   re-negotiating when a Digest nonce goes stale.
//! - **Layout `<folder>/<album>/<content-key>_<file name>`**, so the file
//!   name *is* the idempotency record: a retry, a re-run or a second job
//!   addresses the same file. Albums are directories (`/` nests), created with
//!   `MKCOL`; creating one that exists is a no-op, hence idempotent per job.
//! - **Atomic per file.** Data goes to `<name>.fb-upload` and is renamed to
//!   the final name with `MOVE`, so the final name never holds a half-written
//!   file. A leftover staging file of a killed upload is overwritten by the
//!   next attempt and never reported as uploaded. Servers without `MOVE` get a
//!   direct `PUT` that is size-checked afterwards and deleted when it does not
//!   match; a final file of the wrong size is also what makes a later run
//!   upload again instead of trusting it.
//! - **No telemetry; credentials go only to the configured server.**
//!   Redirects are refused and error messages never carry credentials.
//!
//! Sources are only ever read, and verified against their content key before
//! the first byte is sent.

use std::collections::HashSet;
use std::error::Error as _;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use instant_xml::FromXml;
use percent_encoding::percent_decode_str;
use reqwest::header::{HeaderMap, AUTHORIZATION, CONTENT_LENGTH, LOCATION};
use reqwest::{redirect, Client, Method, RequestBuilder, Response, StatusCode};
use tokio_util::io::ReaderStream;
use url::{Host, Url};

use crate::s3::{blocking, content_type_of, parse_object_name, verify_source};
use crate::{
    AlbumRef, ContentKey, ExportError, ExportPlugin, FileOutcome, FileStatus, FileUpload,
    PluginCapabilities, PluginInfo, RemoteFile,
};

pub const PLUGIN_ID: &str = "webdav";

const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
/// Longest silence tolerated mid-transfer; a file may take far longer overall.
const READ_TIMEOUT: Duration = Duration::from_secs(120);
/// Suffix of the staging name; never a name Nextcloud reserves (`.part`).
const STAGING_SUFFIX: &str = ".fb-upload";
const DEFAULT_ALBUM: &str = "Untitled album";
const MAX_ALBUM_BYTES: usize = 400;
const MAX_COMPONENT_BYTES: usize = 100;
const MAX_FILE_NAME_BYTES: usize = 150;
const MAX_FOLDER_BYTES: usize = 200;
/// Directory levels `list_albums`/`list_files` descend below the folder.
const MAX_DEPTH: usize = 8;
const DAV_NS: &str = "DAV:";
const NOT_DAV: &str = "the address answered, but not like a WebDAV server; check the URL \
    (Nextcloud: https://<host>/remote.php/dav/files/<user>/)";
const PROPFIND_BODY: &str = "<?xml version=\"1.0\" encoding=\"utf-8\"?>\
    <d:propfind xmlns:d=\"DAV:\"><d:prop><d:resourcetype/><d:getcontentlength/></d:prop>\
    </d:propfind>";

/// Where the server is and how to log in.
#[derive(Clone)]
pub struct WebDavConfig {
    url: Url,
    username: String,
    password: String,
    folder: Vec<String>,
    insecure_http: bool,
}

impl WebDavConfig {
    /// Validates the settings without any network access. `url` is the
    /// collection uploads go below, e.g.
    /// `https://cloud.example.com/remote.php/dav/files/alice/`.
    pub fn new(url: &str, username: &str, password: &str) -> Result<Self, ExportError> {
        let invalid = |what: &str| ExportError::InvalidConfig(what.to_owned());

        let raw = url.trim();
        if raw.is_empty() {
            return Err(invalid(
                "enter the WebDAV URL (e.g. https://cloud.example.com/remote.php/dav/files/alice/)",
            ));
        }
        let mut url = Url::parse(raw).map_err(|_| {
            invalid("the WebDAV URL is not a valid URL (expected e.g. https://dav.example.com/)")
        })?;
        if !matches!(url.scheme(), "http" | "https") {
            return Err(invalid("the WebDAV URL must start with http:// or https://"));
        }
        if url.host_str().is_none() {
            return Err(invalid("the WebDAV URL has no host name"));
        }
        if !url.username().is_empty() || url.password().is_some() {
            return Err(invalid(
                "do not put credentials in the WebDAV URL; use the username and password fields",
            ));
        }
        if url.query().is_some() || url.fragment().is_some() {
            return Err(invalid("the WebDAV URL must not have a query or fragment"));
        }
        if !url.path().ends_with('/') {
            let path = format!("{}/", url.path());
            url.set_path(&path);
        }

        let username = username.trim();
        if username.is_empty() {
            return Err(invalid("enter the username"));
        }
        if username.contains(':') || username.chars().any(char::is_control) {
            return Err(invalid("the username must not contain `:` or control characters"));
        }
        if password.chars().any(char::is_control) {
            return Err(invalid("the password contains control characters"));
        }

        Ok(Self {
            url,
            username: username.to_owned(),
            password: password.to_owned(),
            folder: Vec::new(),
            insecure_http: false,
        })
    }

    /// Folder below the URL every upload goes into (`photos/fotobase`);
    /// created on first use.
    pub fn with_folder(mut self, folder: &str) -> Result<Self, ExportError> {
        let mut parts = Vec::new();
        for part in folder.split(['/', '\\']).map(str::trim).filter(|p| !p.is_empty()) {
            if part == "." || part == ".." {
                return Err(ExportError::InvalidConfig(
                    "the folder must not contain `.` or `..` segments".into(),
                ));
            }
            if part.chars().any(char::is_control) {
                return Err(ExportError::InvalidConfig(
                    "the folder contains control characters".into(),
                ));
            }
            parts.push(part.to_owned());
        }
        if parts.join("/").len() > MAX_FOLDER_BYTES {
            return Err(ExportError::InvalidConfig(format!(
                "the folder is longer than {MAX_FOLDER_BYTES} bytes"
            )));
        }
        self.folder = parts;
        Ok(self)
    }

    /// Allows plain `http://` to a non-local host. The password then crosses
    /// the network unencrypted; only for a network you trust.
    pub fn with_insecure_http(mut self) -> Self {
        self.insecure_http = true;
        self
    }

    pub fn url(&self) -> &str {
        self.url.as_str()
    }

    fn is_loopback(&self) -> bool {
        match self.url.host() {
            Some(Host::Domain(name)) => name.eq_ignore_ascii_case("localhost"),
            Some(Host::Ipv4(ip)) => ip.is_loopback(),
            Some(Host::Ipv6(ip)) => ip.is_loopback(),
            None => false,
        }
    }
}

impl std::fmt::Debug for WebDavConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WebDavConfig")
            .field("url", &self.url.as_str())
            .field("username", &"<redacted>")
            .field("password", &"<redacted>")
            .field("folder", &self.folder.join("/"))
            .finish()
    }
}

/// Steps of an upload at which tests can simulate a crash or a lost
/// response. Production code uses the no-op default.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FaultPoint {
    /// Right before the file's data is sent.
    BeforeCommit,
    /// Data is on the server under the staging name; the rename is next.
    BeforeMove,
    /// Right after the file got its final name; an error makes a stored file
    /// look failed.
    AfterCommit,
}

pub trait FaultPoints: Send + Sync {
    /// An error aborts the current attempt at that point.
    fn reached(&self, _point: FaultPoint) -> Result<(), ExportError> {
        Ok(())
    }

    /// Called once the album's directory exists.
    fn after_create_album(&self) -> Result<(), ExportError> {
        Ok(())
    }
}

struct NoFaults;

impl FaultPoints for NoFaults {}

pub struct WebDavPlugin {
    config: WebDavConfig,
    base_segments: Vec<String>,
    client: Client,
    auth: Mutex<AuthState>,
    /// Directories (relative paths) known to exist, so each is created once.
    made_dirs: Mutex<HashSet<String>>,
    move_supported: AtomicBool,
    faults: Arc<dyn FaultPoints>,
}

impl WebDavPlugin {
    pub fn new(config: WebDavConfig) -> Result<Self, ExportError> {
        if config.url.scheme() == "http" && !config.insecure_http && !config.is_loopback() {
            return Err(ExportError::InvalidConfig(
                "the WebDAV URL uses plain http, which would send the password unencrypted; \
                 use https:// (or explicitly allow http for a trusted network)"
                    .into(),
            ));
        }
        // reqwest is built without a bundled TLS provider. Another one
        // already installed process-wide is fine, so the error is ignored.
        let _ = rustls::crypto::ring::default_provider().install_default();
        let client = Client::builder()
            .user_agent(concat!("Fotobase/", env!("CARGO_PKG_VERSION")))
            .redirect(redirect::Policy::none())
            .connect_timeout(CONNECT_TIMEOUT)
            .read_timeout(READ_TIMEOUT)
            .build()
            .map_err(|e| ExportError::Fatal(format!("cannot set up the HTTP client: {e}")))?;
        let base_segments = path_segments(config.url.path());
        Ok(Self {
            config,
            base_segments,
            client,
            auth: Mutex::new(AuthState::Unprobed),
            made_dirs: Mutex::new(HashSet::new()),
            move_supported: AtomicBool::new(true),
            faults: Arc::new(NoFaults),
        })
    }

    pub fn with_fault_points(mut self, faults: Arc<dyn FaultPoints>) -> Self {
        self.faults = faults;
        self
    }

    /// Checks that the address is a WebDAV collection and the login is
    /// accepted (a read; write permission is only proven by the first upload).
    pub async fn validate_connection(&self) -> Result<(), ExportError> {
        match self.propfind(&self.config.url, "0").await? {
            Some(_) => Ok(()),
            None => Err(self.not_found()),
        }
    }

    // ------------------------------------------------------------ HTTP

    fn host(&self) -> &str {
        self.config.url.host_str().unwrap_or("?")
    }

    /// Sends one request. A `401` is answered once by logging in the way the
    /// server asks; every other status is returned for the caller to judge.
    async fn send(
        &self,
        method: &Method,
        url: &Url,
        headers: &[(&str, String)],
        body: &Body<'_>,
    ) -> Result<Response, ExportError> {
        if matches!(body, Body::File { .. }) {
            // Learn the login scheme with a small request first: a server
            // that refuses a streamed body early resets the connection.
            self.probe().await?;
        }
        self.send_authenticated(method, url, headers, body).await
    }

    async fn send_authenticated(
        &self,
        method: &Method,
        url: &Url,
        headers: &[(&str, String)],
        body: &Body<'_>,
    ) -> Result<Response, ExportError> {
        let mut challenged = false;
        loop {
            let mut request = self.client.request(method.clone(), url.clone());
            for (name, value) in headers {
                request = request.header(*name, value);
            }
            request = match body {
                Body::Empty => request,
                Body::Text(text) => request.body(*text),
                Body::File { path, len } => {
                    let file = tokio::fs::File::open(path).await?;
                    request
                        .header(CONTENT_LENGTH, *len)
                        .body(reqwest::Body::wrap_stream(ReaderStream::new(file)))
                }
            };
            let response = self
                .authorize(request, method, url)
                .send()
                .await
                .map_err(|e| self.unreachable(e))?;
            if response.status() == StatusCode::UNAUTHORIZED && !challenged {
                challenged = true;
                if self.adopt_challenge(response.headers())? {
                    continue;
                }
            }
            return Ok(response);
        }
    }

    async fn probe(&self) -> Result<(), ExportError> {
        if !matches!(*lock(&self.auth), AuthState::Unprobed) {
            return Ok(());
        }
        let url = self.config.url.clone();
        let headers = [("Depth", "0".to_owned())];
        self.send_authenticated(&propfind_method(), &url, &headers, &Body::Empty).await?;
        let mut auth = lock(&self.auth);
        if matches!(*auth, AuthState::Unprobed) {
            *auth = AuthState::Anonymous;
        }
        Ok(())
    }

    fn authorize(&self, request: RequestBuilder, method: &Method, url: &Url) -> RequestBuilder {
        let mut auth = lock(&self.auth);
        match &mut *auth {
            AuthState::Basic => {
                request.basic_auth(&self.config.username, Some(&self.config.password))
            }
            AuthState::Digest(session) => {
                session.nc += 1;
                let uri = match url.query() {
                    Some(query) => format!("{}?{query}", url.path()),
                    None => url.path().to_owned(),
                };
                let header = session.authorization(
                    &self.config.username,
                    &self.config.password,
                    method.as_str(),
                    &uri,
                    &new_cnonce(),
                );
                request.header(AUTHORIZATION, header)
            }
            AuthState::Unprobed | AuthState::Anonymous => request,
        }
    }

    /// Picks the login scheme named by a `401`. `Ok(false)` means the login
    /// was already tried (or the server named nothing usable): the answer is
    /// a refusal.
    fn adopt_challenge(&self, headers: &HeaderMap) -> Result<bool, ExportError> {
        let found = parse_challenges(headers);
        let mut auth = lock(&self.auth);
        match &*auth {
            AuthState::Basic => return Ok(false),
            AuthState::Digest(_) if !found.stale => return Ok(false),
            _ => {}
        }
        let secure = self.config.url.scheme() == "https";
        let use_digest = found.digest.is_some() && (!found.basic || !secure);
        if use_digest {
            *auth = AuthState::Digest(DigestSession { challenge: found.digest.unwrap(), nc: 0 });
        } else if found.basic {
            *auth = AuthState::Basic;
        } else if let Some(challenge) = found.digest {
            *auth = AuthState::Digest(DigestSession { challenge, nc: 0 });
        } else if found.other.is_empty() {
            return Ok(false);
        } else {
            return Err(ExportError::Fatal(format!(
                "the server wants {} authentication; Fotobase supports Basic and Digest",
                found.other.join("/")
            )));
        }
        Ok(true)
    }

    fn unreachable(&self, error: reqwest::Error) -> ExportError {
        let error = error.without_url();
        let mut detail = error.to_string();
        let mut source = error.source();
        while let Some(cause) = source {
            detail.push_str(": ");
            detail.push_str(&cause.to_string());
            source = cause.source();
        }
        ExportError::Transient(format!("cannot reach the WebDAV server {}: {detail}", self.host()))
    }

    fn not_found(&self) -> ExportError {
        ExportError::Fatal(format!(
            "not found: this address does not exist on {}; check the WebDAV URL (HTTP 404)",
            self.host()
        ))
    }

    /// Maps an unexpected status to the retry classes of [`ExportError`].
    fn failure(&self, response: &Response) -> ExportError {
        let status = response.status();
        let code = status.as_u16();
        match code {
            401 => ExportError::Fatal("the server rejected the username or password".into()),
            403 => ExportError::Fatal(
                "access denied: this account may not read or write here (HTTP 403)".into(),
            ),
            404 => self.not_found(),
            301 | 302 | 303 | 307 | 308 => {
                let target = response
                    .headers()
                    .get(LOCATION)
                    .and_then(|v| v.to_str().ok())
                    .map(|l| format!(" to {l}"))
                    .unwrap_or_default();
                ExportError::Fatal(format!(
                    "the server redirected the request{target}; use the final address as the \
                     WebDAV URL (HTTP {code})"
                ))
            }
            405 | 501 => ExportError::Fatal(format!(
                "the server does not support this WebDAV operation (HTTP {code})"
            )),
            409 => {
                lock(&self.made_dirs).clear();
                ExportError::Transient(
                    "a folder is missing on the server; creating it again (HTTP 409)".into(),
                )
            }
            413 => ExportError::Fatal(
                "the server refuses files this large (HTTP 413); raise its upload limit or \
                 choose a smaller upload size"
                    .into(),
            ),
            507 => ExportError::Fatal("the server is out of storage space (HTTP 507)".into()),
            423 => ExportError::Transient("the file is locked on the server (HTTP 423)".into()),
            408 | 429 | 500..=599 => {
                ExportError::Transient(format!("the server answered HTTP {code}"))
            }
            _ => ExportError::Fatal(format!("the server rejected the request (HTTP {code})")),
        }
    }

    // ------------------------------------------------------- operations

    fn url_for(&self, rel: &[String], collection: bool) -> Url {
        let mut url = self.config.url.clone();
        {
            let mut segments = url.path_segments_mut().expect("http(s) URLs have segments");
            segments.pop_if_empty().extend(rel);
            if collection {
                segments.push("");
            }
        }
        url
    }

    /// `PROPFIND`; `None` when the resource does not exist.
    async fn propfind(&self, url: &Url, depth: &str) -> Result<Option<Vec<Entry>>, ExportError> {
        let headers = [("Depth", depth.to_owned()), ("Content-Type", "application/xml".to_owned())];
        let response =
            self.send(&propfind_method(), url, &headers, &Body::Text(PROPFIND_BODY)).await?;
        if response.status() == StatusCode::NOT_FOUND {
            return Ok(None);
        }
        if !response.status().is_success() {
            return Err(self.failure(&response));
        }
        let text = response.text().await.map_err(|e| self.unreachable(e))?;
        parse_multistatus(&text, &self.base_segments).map(Some)
    }

    /// Entries below `rel` (without `rel` itself); `None` when it is missing.
    async fn list_dir(&self, rel: &[String]) -> Result<Option<Vec<Entry>>, ExportError> {
        let Some(entries) = self.propfind(&self.url_for(rel, true), "1").await? else {
            return Ok(None);
        };
        Ok(Some(entries.into_iter().filter(|e| e.rel != rel).collect()))
    }

    /// Directories and files below `root`, descending `recursive`ly.
    async fn walk(
        &self,
        root: &[String],
        recursive: bool,
    ) -> Result<(Vec<Entry>, Vec<Entry>), ExportError> {
        let (mut dirs, mut files) = (Vec::new(), Vec::new());
        let mut pending = vec![(root.to_vec(), 0usize)];
        while let Some((dir, depth)) = pending.pop() {
            for entry in self.list_dir(&dir).await?.unwrap_or_default() {
                if entry.dir {
                    if recursive && depth < MAX_DEPTH {
                        pending.push((entry.rel.clone(), depth + 1));
                    }
                    dirs.push(entry);
                } else if !is_staging(entry.name()) {
                    files.push(entry);
                }
            }
        }
        dirs.sort_by(|a, b| a.rel.cmp(&b.rel));
        files.sort_by(|a, b| a.rel.cmp(&b.rel));
        Ok((dirs, files))
    }

    /// Size of the file at `rel`, `None` when it is missing (or its size is
    /// not reported, so it cannot be trusted).
    async fn remote_size(&self, rel: &[String]) -> Result<Option<u64>, ExportError> {
        let Some(entries) = self.propfind(&self.url_for(rel, false), "0").await? else {
            return Ok(None);
        };
        match entries.into_iter().find(|e| e.rel == rel) {
            Some(e) if e.dir => Err(ExportError::Fatal(format!(
                "a folder named `{}` is in the way on the server",
                e.name()
            ))),
            Some(e) => Ok(e.size),
            None => Ok(None),
        }
    }

    /// Creates every directory of `rel` that is not known to exist.
    async fn ensure_dirs(&self, rel: &[String]) -> Result<(), ExportError> {
        for depth in 1..=rel.len() {
            let key = rel[..depth].join("/");
            if lock(&self.made_dirs).contains(&key) {
                continue;
            }
            let response = self
                .send(
                    &Method::from_bytes(b"MKCOL").unwrap(),
                    &self.url_for(&rel[..depth], true),
                    &[],
                    &Body::Empty,
                )
                .await?;
            // 405: the collection already exists.
            if !response.status().is_success()
                && response.status() != StatusCode::METHOD_NOT_ALLOWED
            {
                return Err(self.failure(&response));
            }
            lock(&self.made_dirs).insert(key);
        }
        Ok(())
    }

    async fn put(
        &self,
        rel: &[String],
        name: &str,
        path: &Path,
        len: u64,
    ) -> Result<(), ExportError> {
        let headers = [("Content-Type", content_type_of(name).to_owned())];
        let response = self
            .send(&Method::PUT, &self.url_for(rel, false), &headers, &Body::File { path, len })
            .await?;
        match response.status() {
            status if status.is_success() => Ok(()),
            // The parent vanished since it was created.
            StatusCode::NOT_FOUND | StatusCode::CONFLICT => {
                lock(&self.made_dirs).clear();
                Err(ExportError::Transient(
                    "a folder disappeared from the server; creating it again".into(),
                ))
            }
            _ => Err(self.failure(&response)),
        }
    }

    /// Renames `from` to `to`, replacing any file there.
    async fn rename(&self, from: &[String], to: &[String]) -> Result<Rename, ExportError> {
        let headers =
            [("Destination", self.url_for(to, false).to_string()), ("Overwrite", "T".to_owned())];
        let response = self
            .send(
                &Method::from_bytes(b"MOVE").unwrap(),
                &self.url_for(from, false),
                &headers,
                &Body::Empty,
            )
            .await?;
        match response.status() {
            status if status.is_success() => Ok(Rename::Done),
            StatusCode::METHOD_NOT_ALLOWED | StatusCode::NOT_IMPLEMENTED => Ok(Rename::Unsupported),
            StatusCode::NOT_FOUND => {
                lock(&self.made_dirs).clear();
                Err(ExportError::Transient("the staged file vanished from the server".into()))
            }
            _ => Err(self.failure(&response)),
        }
    }

    /// Best effort: a leftover is overwritten by the next attempt anyway.
    async fn remove(&self, rel: &[String]) {
        let _ = self.send(&Method::DELETE, &self.url_for(rel, false), &[], &Body::Empty).await;
    }

    /// Direct `PUT` for servers without `MOVE`, size-checked afterwards.
    async fn put_in_place(
        &self,
        rel: &[String],
        name: &str,
        path: &Path,
        size: u64,
    ) -> Result<(), ExportError> {
        if let Err(e) = self.put(rel, name, path, size).await {
            self.remove(rel).await;
            return Err(e);
        }
        if self.remote_size(rel).await? != Some(size) {
            self.remove(rel).await;
            return Err(ExportError::Transient(format!(
                "`{name}` arrived with the wrong size; uploading it again"
            )));
        }
        Ok(())
    }

    // ------------------------------------------------------- locations

    /// Directory `album` maps to, relative to the URL.
    fn album_dir(&self, album: Option<&AlbumRef>) -> Result<Vec<String>, ExportError> {
        let Some(album) = album else { return Ok(self.config.folder.clone()) };
        let segments = path_segments(&album.id);
        let escapes = |s: &String| s == "." || s == ".." || s.chars().any(char::is_control);
        if segments.iter().any(escapes) || album.id.contains('\\') {
            return Err(ExportError::Fatal(format!("`{}` is not a valid album id", album.id)));
        }
        Ok(segments)
    }

    fn album_ref(&self, dir: &[String]) -> AlbumRef {
        let name = dir[self.config.folder.len().min(dir.len())..].join("/");
        AlbumRef { id: dir.join("/"), name }
    }
}

enum Rename {
    Done,
    Unsupported,
}

enum Body<'a> {
    Empty,
    Text(&'static str),
    File { path: &'a Path, len: u64 },
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn propfind_method() -> Method {
    Method::from_bytes(b"PROPFIND").expect("valid method")
}

#[async_trait]
impl ExportPlugin for WebDavPlugin {
    fn info(&self) -> PluginInfo {
        PluginInfo {
            id: PLUGIN_ID.into(),
            name: "WebDAV / Nextcloud".into(),
            capabilities: PluginCapabilities { albums: true, size_choice: true },
        }
    }

    async fn list_albums(&mut self) -> Result<Vec<AlbumRef>, ExportError> {
        let (dirs, _) = self.walk(&self.config.folder, true).await?;
        Ok(dirs.iter().map(|d| self.album_ref(&d.rel)).collect())
    }

    async fn create_album(&mut self, _job_id: &str, name: &str) -> Result<AlbumRef, ExportError> {
        // An album is a directory: the same name always maps to the same
        // one, and `MKCOL` on an existing one is a no-op. That makes this
        // idempotent for any job id.
        let mut dir = self.config.folder.clone();
        dir.extend(clean_album(name));
        self.ensure_dirs(&dir).await?;
        let album = self.album_ref(&dir);
        self.faults.after_create_album()?;
        Ok(album)
    }

    async fn list_files(
        &mut self,
        album: Option<&AlbumRef>,
    ) -> Result<Vec<RemoteFile>, ExportError> {
        let (_, files) = self.walk(&self.album_dir(album)?, true).await?;
        Ok(files
            .into_iter()
            .map(|entry| {
                let (content_key, name) = match parse_object_name(entry.name()) {
                    Some((content_key, name)) => (Some(content_key), name.to_owned()),
                    None => (None, entry.name().to_owned()),
                };
                RemoteFile { remote_id: entry.rel.join("/"), name, content_key }
            })
            .collect())
    }

    async fn exists(
        &mut self,
        key: &ContentKey,
        album: Option<&AlbumRef>,
    ) -> Result<bool, ExportError> {
        let prefix = format!("{key}_");
        let listed = self.list_dir(&self.album_dir(album)?).await?.unwrap_or_default();
        Ok(listed.iter().any(|e| !e.dir && e.name().starts_with(&prefix) && !is_staging(e.name())))
    }

    async fn upload_file(&mut self, request: FileUpload<'_>) -> Result<FileOutcome, ExportError> {
        let dir = self.album_dir(request.album)?;
        let name = remote_name(&request.file.content_key, &request.file.name);
        let mut rel = dir.clone();
        rel.push(name.clone());
        let remote_id = rel.join("/");
        let path = request.file.path.clone();
        let size = blocking({
            let path = path.clone();
            move || Ok(std::fs::metadata(&path)?.len())
        })
        .await?;

        if self.remote_size(&rel).await? == Some(size) {
            return Ok(FileOutcome { remote_id, status: FileStatus::AlreadyPresent });
        }

        let expected = request.file.content_key.clone();
        blocking({
            let path = path.clone();
            move || verify_source(&path, &expected)
        })
        .await?;

        self.ensure_dirs(&dir).await?;
        self.faults.reached(FaultPoint::BeforeCommit)?;

        let mut renamed = false;
        if self.move_supported.load(Ordering::SeqCst) {
            let mut staging = dir;
            staging.push(format!("{name}{STAGING_SUFFIX}"));
            if let Err(e) = self.put(&staging, &request.file.name, &path, size).await {
                self.remove(&staging).await;
                return Err(e);
            }
            self.faults.reached(FaultPoint::BeforeMove)?;
            match self.rename(&staging, &rel).await {
                Ok(Rename::Done) => renamed = true,
                Ok(Rename::Unsupported) => {
                    self.move_supported.store(false, Ordering::SeqCst);
                    self.remove(&staging).await;
                }
                Err(e) => {
                    self.remove(&staging).await;
                    return Err(e);
                }
            }
        }
        if !renamed {
            self.put_in_place(&rel, &request.file.name, &path, size).await?;
        }
        self.faults.reached(FaultPoint::AfterCommit)?;
        Ok(FileOutcome { remote_id, status: FileStatus::Uploaded })
    }
}

// ------------------------------------------------------------- multistatus

#[derive(FromXml)]
#[xml(rename = "multistatus", ns(DAV_NS))]
struct Multistatus {
    responses: Vec<DavResponse>,
}

#[derive(FromXml)]
#[xml(rename = "response", ns(DAV_NS))]
struct DavResponse {
    href: String,
    propstat: Vec<Propstat>,
}

#[derive(FromXml)]
#[xml(rename = "propstat", ns(DAV_NS))]
struct Propstat {
    prop: Prop,
}

#[derive(FromXml)]
#[xml(rename = "prop", ns(DAV_NS))]
struct Prop {
    getcontentlength: Option<String>,
    resourcetype: Option<ResourceType>,
}

#[derive(FromXml)]
#[xml(rename = "resourcetype", ns(DAV_NS))]
struct ResourceType {
    collection: Option<Collection>,
}

#[derive(FromXml)]
#[xml(rename = "collection", ns(DAV_NS))]
struct Collection {}

/// A file or directory below the configured URL.
#[derive(Debug, Clone)]
struct Entry {
    /// Decoded path segments relative to the URL.
    rel: Vec<String>,
    dir: bool,
    size: Option<u64>,
}

impl Entry {
    fn name(&self) -> &str {
        self.rel.last().map(String::as_str).unwrap_or("")
    }
}

fn parse_multistatus(body: &str, base: &[String]) -> Result<Vec<Entry>, ExportError> {
    let parsed: Multistatus =
        instant_xml::from_str(body).map_err(|_| ExportError::Fatal(NOT_DAV.into()))?;
    let mut entries = Vec::new();
    for response in parsed.responses {
        let segments = match Url::parse(&response.href) {
            Ok(url) => path_segments(url.path()),
            Err(_) => path_segments(response.href.split(['?', '#']).next().unwrap_or("")),
        };
        // Hrefs outside the configured collection are of no interest.
        let Some(rel) = segments.strip_prefix(base) else { continue };
        let props: Vec<&Prop> = response.propstat.iter().map(|p| &p.prop).collect();
        let dir =
            props.iter().any(|p| p.resourcetype.as_ref().is_some_and(|r| r.collection.is_some()));
        let size = props
            .iter()
            .find_map(|p| p.getcontentlength.as_deref())
            .and_then(|s| s.trim().parse().ok());
        entries.push(Entry { rel: rel.to_vec(), dir, size });
    }
    Ok(entries)
}

/// Non-empty, percent-decoded segments of a URL path.
fn path_segments(path: &str) -> Vec<String> {
    path.split('/')
        .filter(|s| !s.is_empty())
        .map(|s| percent_decode_str(s).decode_utf8_lossy().into_owned())
        .collect()
}

// ------------------------------------------------------------------- auth

enum AuthState {
    /// Nothing sent yet.
    Unprobed,
    /// The server asked for no login (so far).
    Anonymous,
    Basic,
    Digest(DigestSession),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DigestAlgorithm {
    Md5,
    Md5Sess,
    Sha256,
    Sha256Sess,
}

impl DigestAlgorithm {
    fn parse(token: &str) -> Option<Self> {
        match token.to_ascii_uppercase().as_str() {
            "MD5" => Some(Self::Md5),
            "MD5-SESS" => Some(Self::Md5Sess),
            "SHA-256" => Some(Self::Sha256),
            "SHA-256-SESS" => Some(Self::Sha256Sess),
            _ => None,
        }
    }

    fn token(self) -> &'static str {
        match self {
            Self::Md5 => "MD5",
            Self::Md5Sess => "MD5-sess",
            Self::Sha256 => "SHA-256",
            Self::Sha256Sess => "SHA-256-sess",
        }
    }

    fn session(self) -> bool {
        matches!(self, Self::Md5Sess | Self::Sha256Sess)
    }

    fn hash(self, text: &str) -> String {
        match self {
            Self::Md5 | Self::Md5Sess => md5_hex(text),
            Self::Sha256 | Self::Sha256Sess => sha256_hex(text),
        }
    }
}

fn md5_hex(text: &str) -> String {
    use md5::Digest as _;
    to_hex(&md5::Md5::digest(text.as_bytes()))
}

fn sha256_hex(text: &str) -> String {
    use sha2::Digest as _;
    to_hex(&sha2::Sha256::digest(text.as_bytes()))
}

fn to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

struct DigestChallenge {
    realm: String,
    nonce: String,
    opaque: Option<String>,
    algorithm: DigestAlgorithm,
    /// The server offers `qop=auth` (without it, the RFC 2069 form is used).
    qop_auth: bool,
}

struct DigestSession {
    challenge: DigestChallenge,
    nc: u32,
}

impl DigestSession {
    fn authorization(
        &self,
        user: &str,
        password: &str,
        method: &str,
        uri: &str,
        cnonce: &str,
    ) -> String {
        let c = &self.challenge;
        let nc = format!("{:08x}", self.nc);
        let response = digest_response(
            c.algorithm,
            user,
            &c.realm,
            password,
            method,
            uri,
            &c.nonce,
            cnonce,
            c.qop_auth.then_some(nc.as_str()),
        );
        let quote = |v: &str| v.replace('\\', "\\\\").replace('"', "\\\"");
        let mut header = format!(
            "Digest username=\"{}\", realm=\"{}\", nonce=\"{}\", uri=\"{}\", algorithm={}, \
             response=\"{response}\"",
            quote(user),
            quote(&c.realm),
            quote(&c.nonce),
            quote(uri),
            c.algorithm.token(),
        );
        if let Some(opaque) = &c.opaque {
            header.push_str(&format!(", opaque=\"{}\"", quote(opaque)));
        }
        if c.qop_auth {
            header.push_str(&format!(", qop=auth, nc={nc}, cnonce=\"{cnonce}\""));
        } else if c.algorithm.session() {
            header.push_str(&format!(", cnonce=\"{cnonce}\""));
        }
        header
    }
}

/// The `response` value of RFC 7616 §3.4.1 (`qop=auth` when `nc` is given).
#[allow(clippy::too_many_arguments)]
fn digest_response(
    algorithm: DigestAlgorithm,
    user: &str,
    realm: &str,
    password: &str,
    method: &str,
    uri: &str,
    nonce: &str,
    cnonce: &str,
    nc: Option<&str>,
) -> String {
    let mut ha1 = algorithm.hash(&format!("{user}:{realm}:{password}"));
    if algorithm.session() {
        ha1 = algorithm.hash(&format!("{ha1}:{nonce}:{cnonce}"));
    }
    let ha2 = algorithm.hash(&format!("{method}:{uri}"));
    match nc {
        Some(nc) => algorithm.hash(&format!("{ha1}:{nonce}:{nc}:{cnonce}:auth:{ha2}")),
        None => algorithm.hash(&format!("{ha1}:{nonce}:{ha2}")),
    }
}

fn new_cnonce() -> String {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let nanos = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    let seed =
        format!("{nanos}-{}-{}", COUNTER.fetch_add(1, Ordering::Relaxed), std::process::id());
    sha256_hex(&seed)[..16].to_owned()
}

#[derive(Default)]
struct Challenges {
    basic: bool,
    digest: Option<DigestChallenge>,
    stale: bool,
    /// Schemes that were offered but cannot be used.
    other: Vec<String>,
}

/// The `WWW-Authenticate` challenges of a `401` (one per header line).
fn parse_challenges(headers: &HeaderMap) -> Challenges {
    let mut found = Challenges::default();
    for value in headers.get_all(reqwest::header::WWW_AUTHENTICATE) {
        let Ok(value) = value.to_str() else { continue };
        let (scheme, rest) = value.trim().split_once(' ').unwrap_or((value.trim(), ""));
        match scheme.to_ascii_lowercase().as_str() {
            "basic" => found.basic = true,
            "digest" => {
                let params = parse_params(rest);
                let get = |name: &str| {
                    params
                        .iter()
                        .find(|(k, _)| k.eq_ignore_ascii_case(name))
                        .map(|(_, v)| v.as_str())
                };
                let algorithm = match get("algorithm") {
                    None => Some(DigestAlgorithm::Md5),
                    Some(token) => DigestAlgorithm::parse(token),
                };
                let qop_auth = match get("qop") {
                    None => Some(false),
                    Some(list) => list
                        .split(',')
                        .any(|q| q.trim().eq_ignore_ascii_case("auth"))
                        .then_some(true),
                };
                match (get("realm"), get("nonce"), algorithm, qop_auth) {
                    (Some(realm), Some(nonce), Some(algorithm), Some(qop_auth)) => {
                        found.stale = get("stale").is_some_and(|s| s.eq_ignore_ascii_case("true"));
                        found.digest = Some(DigestChallenge {
                            realm: realm.to_owned(),
                            nonce: nonce.to_owned(),
                            opaque: get("opaque").map(str::to_owned),
                            algorithm,
                            qop_auth,
                        });
                    }
                    _ => found.other.push("Digest (unsupported variant)".into()),
                }
            }
            "" => {}
            _ => found.other.push(scheme.to_owned()),
        }
    }
    found
}

/// `key=value` pairs separated by commas; values may be quoted strings.
fn parse_params(text: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut chars = text.chars().peekable();
    loop {
        while chars.next_if(|c| c.is_whitespace() || *c == ',').is_some() {}
        let mut key = String::new();
        while let Some(c) = chars.next_if(|c| *c != '=' && *c != ',') {
            key.push(c);
        }
        if chars.next_if_eq(&'=').is_none() {
            if key.is_empty() {
                return out;
            }
            continue;
        }
        let mut value = String::new();
        if chars.next_if_eq(&'"').is_some() {
            while let Some(c) = chars.next() {
                match c {
                    '"' => break,
                    '\\' => value.extend(chars.next()),
                    c => value.push(c),
                }
            }
        } else {
            while let Some(c) = chars.next_if(|c| *c != ',') {
                value.push(c);
            }
        }
        out.push((key.trim().to_owned(), value.trim().to_owned()));
    }
}

// ------------------------------------------------------------ names

/// `<content-key>_<file name>` as stored on the server.
fn remote_name(content_key: &ContentKey, file_name: &str) -> String {
    format!("{content_key}_{}", clean_file_name(file_name))
}

fn is_staging(name: &str) -> bool {
    name.ends_with(STAGING_SUFFIX)
}

/// Directory name for an album (`/` or `\` nests), cleaned so every common
/// server accepts it: characters Windows and Nextcloud forbid become `_`,
/// `.`-names, device names and names Nextcloud treats as in-progress uploads
/// are altered, and every component is length-capped. Stable under
/// re-cleaning.
pub fn sanitize_album_name(name: &str) -> String {
    clean_album(name).join("/")
}

fn clean_album(name: &str) -> Vec<String> {
    let mut parts: Vec<String> = Vec::new();
    let mut total = 0;
    for part in name.split(['/', '\\']).map(str::trim).filter(|p| !p.is_empty()) {
        let part = clean_component(part);
        total += part.len() + 1;
        if total > MAX_ALBUM_BYTES {
            break;
        }
        parts.push(part);
    }
    if parts.is_empty() {
        parts.push(DEFAULT_ALBUM.into());
    }
    parts
}

const FORBIDDEN: [char; 7] = ['<', '>', ':', '"', '|', '?', '*'];

/// Control and forbidden characters replaced, cut to `max` bytes.
fn clean_chars(text: &str, max: usize) -> String {
    let replaced: String = text
        .chars()
        .map(|c| {
            if c.is_control() || FORBIDDEN.contains(&c) || c == '\\' || c == '/' {
                '_'
            } else {
                c
            }
        })
        .collect();
    let mut end = replaced.len().min(max.max(1));
    while !replaced.is_char_boundary(end) {
        end -= 1;
    }
    replaced[..end].to_owned()
}

fn has_reserved_suffix(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower.ends_with(".part") || lower.ends_with(".filepart") || lower.ends_with(STAGING_SUFFIX)
}

fn is_device_name(name: &str) -> bool {
    let stem = name.split('.').next().unwrap_or(name).to_ascii_uppercase();
    matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || (stem.len() == 4
            && (stem.starts_with("COM") || stem.starts_with("LPT"))
            && stem.as_bytes()[3].is_ascii_digit()
            && stem.as_bytes()[3] != b'0')
}

fn clean_component(part: &str) -> String {
    let cleaned = clean_chars(part.trim(), MAX_COMPONENT_BYTES);
    let trimmed = cleaned.trim_end_matches(['.', ' ']);
    // `.`/`..` would be collapsed by URL handling and leading dots hide
    // folders.
    let visible = trimmed.trim_start_matches('.');
    let mut name = if visible.len() == trimmed.len() || visible.is_empty() {
        visible.to_owned()
    } else {
        format!("_{visible}")
    };
    if name.is_empty() {
        return "_".into();
    }
    if is_device_name(&name) || has_reserved_suffix(&name) {
        name.push('_');
    }
    name
}

/// File name with the extension kept when it is cut for length.
fn clean_file_name(name: &str) -> String {
    let name = name.replace(['/', '\\'], "_");
    let (stem, ext) = match name.rfind('.') {
        Some(dot) if dot > 0 && name.len() - dot <= 16 => name.split_at(dot),
        _ => (name.as_str(), ""),
    };
    let ext = clean_chars(ext, 16);
    let stem = clean_chars(stem, MAX_FILE_NAME_BYTES.saturating_sub(ext.len()));
    let mut cleaned = format!("{stem}{ext}").trim_end_matches(['.', ' ']).to_owned();
    if cleaned.is_empty() {
        cleaned = "file".into();
    }
    if has_reserved_suffix(&cleaned) {
        cleaned.push('_');
    }
    cleaned
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn album_names_nest_and_sanitize_each_component() {
        assert_eq!(sanitize_album_name("Holiday"), "Holiday");
        assert_eq!(sanitize_album_name("Trips/2024\\Rome"), "Trips/2024/Rome");
        assert_eq!(sanitize_album_name("../../etc"), "_/_/etc");
        assert_eq!(sanitize_album_name("  //  "), DEFAULT_ALBUM);
        assert_eq!(sanitize_album_name("tab\there"), "tab_here");
        assert_eq!(
            sanitize_album_name("Rome: 2024? <best> *all* \"x\" a|b"),
            "Rome_ 2024_ _best_ _all_ _x_ a_b"
        );
        assert_eq!(sanitize_album_name("Sommer 2024..."), "Sommer 2024");
        assert_eq!(sanitize_album_name(".hidden"), "_hidden");
        assert_eq!(sanitize_album_name("é/ü"), "é/ü");
    }

    #[test]
    fn names_that_servers_treat_specially_are_altered() {
        for (name, expected) in [
            ("CON", "CON_"),
            ("nul.txt", "nul.txt_"),
            ("COM1", "COM1_"),
            ("COM0", "COM0"),
            ("COMX", "COMX"),
            ("upload.part", "upload.part_"),
            ("x.FILEPART", "x.FILEPART_"),
            ("x.fb-upload", "x.fb-upload_"),
        ] {
            assert_eq!(sanitize_album_name(name), expected, "{name}");
        }
    }

    #[test]
    fn cleaned_album_names_are_stable() {
        for name in [
            "a\u{1}b",
            "../x",
            "Trips/2024: Rome?",
            "é/ü",
            "CON",
            ".part",
            "...",
            &"é".repeat(300),
            &format!("{}.part", "a".repeat(95)),
            &format!("{}.", "b".repeat(99)),
        ] {
            let once = sanitize_album_name(name);
            assert_eq!(sanitize_album_name(&once), once, "{name}");
            assert!(once.split('/').all(|c| c.len() <= MAX_COMPONENT_BYTES + 1), "{name}");
        }
    }

    #[test]
    fn long_file_names_keep_their_extension() {
        let name = clean_file_name(&format!("{}.jpeg", "é".repeat(300)));
        assert!(name.len() <= MAX_FILE_NAME_BYTES);
        assert!(name.ends_with(".jpeg"));
        assert_eq!(clean_file_name("a/b.jpg"), "a_b.jpg");
        assert_eq!(clean_file_name("what?.jpg"), "what_.jpg");
        assert_eq!(clean_file_name("photo."), "photo");
        assert_eq!(clean_file_name("raw.part"), "raw.part_");
    }

    #[test]
    fn remote_names_round_trip() {
        let key = ContentKey::from_bytes(b"x");
        let name = remote_name(&key, "IMG 1.jpg");
        assert_eq!(name, format!("{key}_IMG 1.jpg"));
        let (parsed, original) = parse_object_name(&name).unwrap();
        assert_eq!((parsed, original), (key, "IMG 1.jpg"));
        assert!(is_staging(&format!("{name}{STAGING_SUFFIX}")));
        assert!(!is_staging(&name));
    }

    // RFC 2617 §3.5 example, and its SHA-256 counterpart from RFC 7616 §3.9.1.
    #[test]
    fn digest_responses_match_the_rfc_vectors() {
        assert_eq!(
            digest_response(
                DigestAlgorithm::Md5,
                "Mufasa",
                "testrealm@host.com",
                "Circle Of Life",
                "GET",
                "/dir/index.html",
                "dcd98b7102dd2f0e8b11d0f600bfb0c093",
                "0a4f113b",
                Some("00000001"),
            ),
            "6629fae49393a05397450978507c4ef1"
        );
        assert_eq!(
            digest_response(
                DigestAlgorithm::Sha256,
                "Mufasa",
                "http-auth@example.org",
                "Circle of Life",
                "GET",
                "/dir/index.html",
                "7ypf/xlj9XXwfDPEoM4URrv/xwf94BcCAzFZH4GiTo0v",
                "f2/wE4q74E6zIJEtWaHKaf5wv/H5QzzpXusqGemxURZJ",
                Some("00000001"),
            ),
            "753927fa0e85d155564e2e272a28d1802ca10daf4496794697cf8db5856cb6c1"
        );
    }

    #[test]
    fn challenges_are_parsed_per_header_line() {
        let mut headers = HeaderMap::new();
        headers.append(
            reqwest::header::WWW_AUTHENTICATE,
            "Digest realm=\"a, b\", qop=\"auth,auth-int\", nonce=\"n\\\"1\", algorithm=SHA-256, stale=TRUE"
                .parse()
                .unwrap(),
        );
        headers.append(reqwest::header::WWW_AUTHENTICATE, "Basic realm=\"x\"".parse().unwrap());
        headers.append(reqwest::header::WWW_AUTHENTICATE, "Negotiate".parse().unwrap());
        let found = parse_challenges(&headers);
        let digest = found.digest.unwrap();
        assert_eq!((digest.realm.as_str(), digest.nonce.as_str()), ("a, b", "n\"1"));
        assert_eq!(digest.algorithm, DigestAlgorithm::Sha256);
        assert!(digest.qop_auth && found.stale && found.basic);
        assert_eq!(found.other, ["Negotiate"]);
    }

    #[test]
    fn digest_with_only_auth_int_is_unusable() {
        let mut headers = HeaderMap::new();
        headers.append(
            reqwest::header::WWW_AUTHENTICATE,
            "Digest realm=\"r\", nonce=\"n\", qop=\"auth-int\"".parse().unwrap(),
        );
        let found = parse_challenges(&headers);
        assert!(found.digest.is_none() && found.other.len() == 1);
    }

    #[test]
    fn multistatus_hrefs_are_decoded_and_made_relative() {
        let xml = "<D:multistatus xmlns:D=\"DAV:\">\
            <D:response><D:href>http://h/dav/Trip%20A/</D:href><D:propstat><D:prop>\
            <D:resourcetype><D:collection/></D:resourcetype></D:prop></D:propstat></D:response>\
            <D:response><D:href>/dav/Trip%20A/p%C3%A9.jpg</D:href><D:propstat><D:prop>\
            <D:getcontentlength>12</D:getcontentlength><D:resourcetype/></D:prop></D:propstat></D:response>\
            <D:response><D:href>/elsewhere/x</D:href></D:response>\
            </D:multistatus>";
        let entries = parse_multistatus(xml, &["dav".to_owned()]).unwrap();
        assert_eq!(entries.len(), 2);
        assert!(entries[0].dir && entries[0].rel == ["Trip A"]);
        assert_eq!((entries[1].rel.join("/"), entries[1].size), ("Trip A/pé.jpg".into(), Some(12)));
        assert!(matches!(parse_multistatus("<html/>", &[]), Err(ExportError::Fatal(_))));
    }
}
