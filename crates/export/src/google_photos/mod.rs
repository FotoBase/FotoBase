//! Google Photos export plugin (TASK-08-012, PLAN §6 P4 "Google Photos
//! (upload-only)", §10 API-restriction risk).
//!
//! - **Upload-only, on purpose.** Since 2025 the Photos Library API lets an
//!   app see only what it uploaded itself. The plugin therefore never reads a
//!   library back: [`ExportPlugin::list_files`] refuses, the OAuth scopes
//!   ([`SCOPES`]) cannot see other photos, and the display name says
//!   "upload only" wherever targets are listed. There is no sync.
//! - **Sign-in** is OAuth 2.0 in the system browser with PKCE and a loopback
//!   redirect ([`GoogleAuth`], [`LoopbackReceiver`]); tokens are kept in a
//!   [`TokenStore`] (the OS credential store in the app) and refreshed
//!   transparently, once more if Google answers `401` mid-job.
//! - **Two-step upload.** Bytes go to `v1/uploads` with the resumable
//!   protocol, in chunks, yielding an upload token; `mediaItems:batchCreate`
//!   turns the token into a media item (and puts it in the album). A transient
//!   failure keeps the upload session, so the retry asks Google how much it
//!   received and continues from there instead of starting over.
//! - **Idempotent per [`ContentKey`] through the [`UploadLedger`].** Google
//!   cannot be asked what it holds, so the ledger records every confirmed
//!   upload per album. A retry or re-run consults it first: known content
//!   costs no upload, and content known to the library but not to the album is
//!   only added to the album. Should the ledger be lost, Google still answers
//!   a repeated upload of identical bytes with the same media item.
//! - **Albums** are found by title among the albums this app created (all the
//!   API exposes) before one is created, so retries and re-runs never make a
//!   second one.
//! - **Tokens go only to the configured Google endpoints.** Redirects are
//!   refused, an upload session URL on another origin is rejected, and no
//!   error message or `Debug` output carries a secret.
//!
//! Sources are only ever read, and verified against their content key before
//! the first byte is sent.

mod auth;
mod ledger;

use std::collections::HashMap;
use std::error::Error as _;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use reqwest::{redirect, Client, Method, RequestBuilder, Response, StatusCode};
use serde_json::{json, Value};
use url::Url;

pub use auth::{
    AuthorizationRequest, GoogleAuth, LoopbackReceiver, MemoryTokenStore, OAuthTokens, TokenStore,
    SCOPES,
};
pub use ledger::UploadLedger;

use crate::s3::{blocking, content_type_of, read_range, verify_source};
use crate::{
    AlbumRef, ContentKey, ExportError, ExportPlugin, FileOutcome, FileStatus, FileUpload,
    PluginCapabilities, PluginInfo,
};

pub const PLUGIN_ID: &str = "google-photos";

const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
/// Longest silence tolerated mid-transfer; a chunk may take far longer
/// overall on a slow line.
const READ_TIMEOUT: Duration = Duration::from_secs(120);
const DEFAULT_CHUNK_SIZE: u64 = 16 * 1024 * 1024;
const ALBUM_PAGE_SIZE: &str = "50";

/// Which Google app Fotobase signs in as, and where Google's endpoints are.
#[derive(Clone)]
pub struct GooglePhotosConfig {
    client_id: String,
    client_secret: Option<String>,
    auth_endpoint: Url,
    token_endpoint: Url,
    api_base: Url,
    chunk_size: u64,
}

impl GooglePhotosConfig {
    /// Validates the settings without any network access. The client id (and
    /// secret, which Google issues even for desktop apps) come from the
    /// Google Cloud project Fotobase is registered under.
    pub fn new(client_id: &str, client_secret: Option<&str>) -> Result<Self, ExportError> {
        let client_id = client_id.trim();
        if client_id.is_empty() || !client_id.bytes().all(|b| b.is_ascii_graphic()) {
            return Err(ExportError::InvalidConfig("enter a valid Google client id".into()));
        }
        let client_secret = client_secret.map(str::trim).filter(|s| !s.is_empty());
        if client_secret.is_some_and(|s| !s.bytes().all(|b| b.is_ascii_graphic())) {
            return Err(ExportError::InvalidConfig("the Google client secret is invalid".into()));
        }
        Ok(Self {
            client_id: client_id.to_owned(),
            client_secret: client_secret.map(str::to_owned),
            auth_endpoint: Url::parse("https://accounts.google.com/o/oauth2/v2/auth")
                .expect("static URL"),
            token_endpoint: Url::parse("https://oauth2.googleapis.com/token").expect("static URL"),
            api_base: Url::parse("https://photoslibrary.googleapis.com").expect("static URL"),
            chunk_size: DEFAULT_CHUNK_SIZE,
        })
    }

    /// Points the plugin at other endpoints — a sandbox or recorded-API mock.
    pub fn with_endpoints(
        mut self,
        auth_endpoint: &str,
        token_endpoint: &str,
        api_base: &str,
    ) -> Result<Self, ExportError> {
        self.auth_endpoint = endpoint("authorization", auth_endpoint)?;
        self.token_endpoint = endpoint("token", token_endpoint)?;
        self.api_base = endpoint("API", api_base)?;
        Ok(self)
    }

    /// Bytes per upload request. Google may demand a coarser granularity,
    /// which wins.
    pub fn with_chunk_size(mut self, bytes: u64) -> Result<Self, ExportError> {
        if bytes == 0 {
            return Err(ExportError::InvalidConfig("the upload chunk size must not be 0".into()));
        }
        self.chunk_size = bytes;
        Ok(self)
    }

    pub(crate) fn client_id(&self) -> &str {
        &self.client_id
    }

    pub(crate) fn client_secret(&self) -> Option<&str> {
        self.client_secret.as_deref()
    }

    pub(crate) fn auth_endpoint(&self) -> &Url {
        &self.auth_endpoint
    }

    pub(crate) fn token_endpoint(&self) -> &Url {
        &self.token_endpoint
    }
}

impl std::fmt::Debug for GooglePhotosConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GooglePhotosConfig")
            .field("client_id", &self.client_id)
            .field("client_secret", &self.client_secret.as_ref().map(|_| "<redacted>"))
            .field("api_base", &self.api_base.as_str())
            .finish()
    }
}

fn endpoint(what: &str, raw: &str) -> Result<Url, ExportError> {
    let invalid = |why: &str| ExportError::InvalidConfig(format!("the {what} address {why}"));
    let url = Url::parse(raw.trim()).map_err(|_| invalid("is not a valid URL"))?;
    if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
        return Err(invalid("must be an http:// or https:// address"));
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err(invalid("must not contain credentials"));
    }
    Ok(url)
}

/// The shared HTTP client: no redirects (a token must not follow one to
/// another host) and no telemetry.
fn http_client() -> Result<Client, ExportError> {
    // reqwest is built without a bundled TLS provider. Another one already
    // installed process-wide is fine, so the error is ignored.
    let _ = rustls::crypto::ring::default_provider().install_default();
    Client::builder()
        .user_agent(concat!("Fotobase/", env!("CARGO_PKG_VERSION")))
        .redirect(redirect::Policy::none())
        .connect_timeout(CONNECT_TIMEOUT)
        .read_timeout(READ_TIMEOUT)
        .build()
        .map_err(|e| ExportError::Fatal(format!("cannot set up the HTTP client: {e}")))
}

fn transport_error(error: reqwest::Error, what: &str) -> ExportError {
    let error = error.without_url();
    let mut detail = error.to_string();
    let mut source = error.source();
    while let Some(cause) = source {
        detail.push_str(": ");
        detail.push_str(&cause.to_string());
        source = cause.source();
    }
    ExportError::Transient(format!("cannot reach {what}: {detail}"))
}

/// An upload session Google opened for one file.
struct Session {
    url: Url,
    size: u64,
    chunk: u64,
}

pub struct GooglePhotosPlugin {
    config: GooglePhotosConfig,
    auth: GoogleAuth,
    client: Client,
    ledger: UploadLedger,
    /// Interrupted uploads, so a retry continues instead of restarting.
    sessions: HashMap<ContentKey, Session>,
}

impl GooglePhotosPlugin {
    pub fn new(
        config: GooglePhotosConfig,
        tokens: Arc<dyn TokenStore>,
        ledger: UploadLedger,
    ) -> Result<Self, ExportError> {
        let auth = GoogleAuth::new(config.clone(), tokens)?;
        Ok(Self { config, auth, client: http_client()?, ledger, sessions: HashMap::new() })
    }

    fn api_url(&self, segments: &[&str]) -> Url {
        let mut url = self.config.api_base.clone();
        if let Ok(mut path) = url.path_segments_mut() {
            path.pop_if_empty().extend(segments);
        }
        url
    }

    /// Sends the request `make` builds with a valid access token; a `401`
    /// forces one refresh and a second try. The response is returned as is.
    async fn authed(&self, make: impl Fn() -> RequestBuilder) -> Result<Response, ExportError> {
        let mut force_refresh = false;
        loop {
            let token = self.auth.access_token(force_refresh).await?;
            let response = make()
                .bearer_auth(token)
                .send()
                .await
                .map_err(|e| transport_error(e, "Google Photos"))?;
            if response.status() == StatusCode::UNAUTHORIZED && !force_refresh {
                force_refresh = true;
                continue;
            }
            return Ok(response);
        }
    }

    async fn checked(&self, make: impl Fn() -> RequestBuilder) -> Result<Response, ExportError> {
        let response = self.authed(make).await?;
        check(response).await
    }

    async fn json(&self, make: impl Fn() -> RequestBuilder) -> Result<Value, ExportError> {
        let response = self.checked(make).await?;
        response.json().await.map_err(|e| transport_error(e, "Google Photos"))
    }

    // ------------------------------------------------------------------ upload

    /// Opens an upload session for `file`, or returns the interrupted one.
    async fn session_for(
        &mut self,
        file: &crate::UploadFile,
    ) -> Result<(Session, u64), ExportError> {
        if let Some(session) = self.sessions.remove(&file.content_key) {
            return match self.received(&session).await {
                Ok(Some(offset)) => Ok((session, offset)),
                // Expired or already finalised (its token is gone): start over.
                Ok(None) => self.start_session(file).await,
                Err(e) => {
                    self.sessions.insert(file.content_key.clone(), session);
                    Err(e)
                }
            };
        }
        self.start_session(file).await
    }

    async fn start_session(&self, file: &crate::UploadFile) -> Result<(Session, u64), ExportError> {
        let path = file.path.clone();
        let expected = file.content_key.clone();
        let size = blocking(move || {
            verify_source(&path, &expected)?;
            Ok(std::fs::metadata(&path)?.len())
        })
        .await?;

        let url = self.api_url(&["v1", "uploads"]);
        let content_type = content_type_of(&file.name);
        let response = self
            .checked(|| {
                self.client
                    .post(url.clone())
                    .header("content-length", "0")
                    .header("x-goog-upload-command", "start")
                    .header("x-goog-upload-protocol", "resumable")
                    .header("x-goog-upload-content-type", content_type)
                    .header("x-goog-upload-raw-size", size.to_string())
            })
            .await?;

        let headers = response.headers();
        let session_url = headers
            .get("x-goog-upload-url")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| Url::parse(v).ok())
            .ok_or_else(|| ExportError::Fatal("Google did not open an upload session".into()))?;
        if session_url.origin() != self.config.api_base.origin() {
            return Err(ExportError::Fatal(
                "Google answered with an upload address on another server; refusing to send \
                 the sign-in there"
                    .into(),
            ));
        }
        let granularity: u64 = headers
            .get("x-goog-upload-chunk-granularity")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse().ok())
            .filter(|g| *g > 0)
            .unwrap_or(1);
        let chunk = (self.config.chunk_size / granularity).max(1) * granularity;
        Ok((Session { url: session_url, size, chunk }, 0))
    }

    /// How many bytes Google holds of an interrupted upload; `None` when the
    /// session is gone or cannot be continued.
    async fn received(&self, session: &Session) -> Result<Option<u64>, ExportError> {
        let response = self
            .authed(|| {
                self.client
                    .post(session.url.clone())
                    .header("content-length", "0")
                    .header("x-goog-upload-command", "query")
            })
            .await?;
        if matches!(response.status(), StatusCode::NOT_FOUND | StatusCode::GONE) {
            return Ok(None);
        }
        let response = check(response).await?;
        let header = |name: &str| response.headers().get(name).and_then(|v| v.to_str().ok());
        if header("x-goog-upload-status") != Some("active") {
            return Ok(None);
        }
        Ok(header("x-goog-upload-size-received").and_then(|v| v.parse().ok()))
    }

    /// Sends the rest of the file; returns the upload token.
    async fn send_chunks(
        &self,
        file: &crate::UploadFile,
        session: &Session,
        mut offset: u64,
    ) -> Result<String, ExportError> {
        loop {
            let len = session.chunk.min(session.size.saturating_sub(offset));
            let last = offset + len >= session.size;
            let data =
                if len == 0 { Vec::new() } else { read_range(&file.path, offset, len).await? };
            let command = if last { "upload, finalize" } else { "upload" };
            let response = self
                .checked(|| {
                    self.client
                        .post(session.url.clone())
                        .header("x-goog-upload-command", command)
                        .header("x-goog-upload-offset", offset.to_string())
                        .body(data.clone())
                })
                .await?;
            if !last {
                offset += len;
                continue;
            }
            let token = response.text().await.map_err(|e| transport_error(e, "Google Photos"))?;
            let token = token.trim();
            if token.is_empty() {
                return Err(ExportError::Fatal(
                    "Google finished the upload without a token".into(),
                ));
            }
            return Ok(token.to_owned());
        }
    }

    async fn upload_bytes(&mut self, file: &crate::UploadFile) -> Result<String, ExportError> {
        let (session, offset) = self.session_for(file).await?;
        let result = self.send_chunks(file, &session, offset).await;
        if matches!(result, Err(ExportError::Transient(_))) {
            self.sessions.insert(file.content_key.clone(), session);
        }
        result
    }

    async fn create_media_item(
        &self,
        file: &crate::UploadFile,
        token: &str,
        album: Option<&AlbumRef>,
    ) -> Result<String, ExportError> {
        let mut body = json!({
            "newMediaItems": [{
                "simpleMediaItem": { "fileName": file.name, "uploadToken": token },
            }],
        });
        if let Some(album) = album {
            body["albumId"] = json!(album.id);
        }
        let url = self.api_url(&["v1", "mediaItems:batchCreate"]);
        let answer = self.json(|| self.client.post(url.clone()).json(&body)).await?;

        let result = &answer["newMediaItemResults"][0];
        let code = result["status"]["code"].as_i64().unwrap_or(0);
        if code != 0 {
            let message = result["status"]["message"].as_str().unwrap_or("unknown error");
            let text = format!("Google Photos could not create the photo: {message} (code {code})");
            // gRPC: DEADLINE_EXCEEDED, RESOURCE_EXHAUSTED, INTERNAL, UNAVAILABLE.
            return Err(if matches!(code, 4 | 8 | 13 | 14) {
                ExportError::Transient(text)
            } else {
                ExportError::Fatal(text)
            });
        }
        result["mediaItem"]["id"].as_str().map(str::to_owned).ok_or_else(|| {
            ExportError::Fatal("Google Photos answered without a media item id".into())
        })
    }

    /// Adds an existing media item to `album`; `false` when Google rejects
    /// the item (deleted since).
    async fn add_to_album(&self, album: &AlbumRef, item: &str) -> Result<bool, ExportError> {
        let url = self.api_url(&["v1", "albums", &format!("{}:batchAddMediaItems", album.id)]);
        let body = json!({ "mediaItemIds": [item] });
        let response = self.authed(|| self.client.post(url.clone()).json(&body)).await?;
        if matches!(response.status(), StatusCode::BAD_REQUEST | StatusCode::NOT_FOUND) {
            return Ok(false);
        }
        check(response).await?;
        Ok(true)
    }
}

/// `": <message>"` from a Google error body, or empty.
async fn server_message(response: Response) -> String {
    let Ok(body) = response.text().await else { return String::new() };
    match serde_json::from_str::<Value>(&body).ok().and_then(|v| {
        v["error"]["message"].as_str().map(|m| m.chars().take(200).collect::<String>())
    }) {
        Some(message) => format!(": {message}"),
        None => String::new(),
    }
}

async fn check(response: Response) -> Result<Response, ExportError> {
    let status = response.status();
    if status.is_success() {
        return Ok(response);
    }
    let detail = server_message(response).await;
    let at = format!("HTTP {}{detail}", status.as_u16());
    Err(match status {
        s if s.is_redirection() => ExportError::Fatal(
            "Google Photos redirected the request; Fotobase does not follow redirects so the \
             sign-in stays with Google"
                .into(),
        ),
        StatusCode::UNAUTHORIZED => ExportError::Fatal(format!(
            "Google no longer accepts the sign-in ({at}); sign in again"
        )),
        StatusCode::FORBIDDEN => ExportError::Fatal(format!(
            "Google Photos refused access ({at}); sign in again and allow Fotobase to add photos"
        )),
        StatusCode::REQUEST_TIMEOUT => {
            ExportError::Transient(format!("Google Photos timed out ({at})"))
        }
        StatusCode::TOO_MANY_REQUESTS => ExportError::Transient(format!(
            "Google Photos is rate limiting uploads ({at}); it asks for at least 30 seconds"
        )),
        s if s.is_server_error() => ExportError::Transient(format!("Google Photos failed ({at})")),
        _ => ExportError::Fatal(format!("Google Photos rejected the request ({at})")),
    })
}

#[async_trait]
impl ExportPlugin for GooglePhotosPlugin {
    fn info(&self) -> PluginInfo {
        PluginInfo {
            id: PLUGIN_ID.into(),
            name: "Google Photos (upload only)".into(),
            capabilities: PluginCapabilities { albums: true, size_choice: true },
        }
    }

    /// Only the albums Fotobase created: all the API shows an app.
    async fn list_albums(&mut self) -> Result<Vec<AlbumRef>, ExportError> {
        let mut albums = Vec::new();
        let mut page_token: Option<String> = None;
        loop {
            let mut url = self.api_url(&["v1", "albums"]);
            {
                let mut query = url.query_pairs_mut();
                query.append_pair("pageSize", ALBUM_PAGE_SIZE);
                if let Some(token) = &page_token {
                    query.append_pair("pageToken", token);
                }
            }
            let page = self.json(|| self.client.request(Method::GET, url.clone())).await?;
            albums.extend(
                page["albums"].as_array().map(Vec::as_slice).unwrap_or_default().iter().filter_map(
                    |a| {
                        Some(AlbumRef {
                            id: a["id"].as_str()?.into(),
                            name: a["title"].as_str()?.into(),
                        })
                    },
                ),
            );
            match page["nextPageToken"].as_str() {
                Some(token) if !token.is_empty() => page_token = Some(token.to_owned()),
                _ => return Ok(albums),
            }
        }
    }

    async fn create_album(&mut self, _job_id: &str, name: &str) -> Result<AlbumRef, ExportError> {
        if let Some(existing) = self.list_albums().await?.into_iter().find(|a| a.name == name) {
            return Ok(existing);
        }
        let url = self.api_url(&["v1", "albums"]);
        let body = json!({ "album": { "title": name } });
        let created = self.json(|| self.client.post(url.clone()).json(&body)).await?;
        let id = created["id"].as_str().ok_or_else(|| {
            ExportError::Fatal("Google Photos answered without an album id".into())
        })?;
        Ok(AlbumRef { id: id.into(), name: name.into() })
    }

    /// Answered from the ledger: reading the library is not allowed.
    async fn exists(
        &mut self,
        key: &ContentKey,
        album: Option<&AlbumRef>,
    ) -> Result<bool, ExportError> {
        Ok(self.ledger.holds(key, album.map(|a| a.id.as_str())).is_some())
    }

    async fn upload_file(&mut self, request: FileUpload<'_>) -> Result<FileOutcome, ExportError> {
        let key = &request.file.content_key;
        let album_id = request.album.map(|a| a.id.as_str());
        let already =
            |remote_id: String| FileOutcome { remote_id, status: FileStatus::AlreadyPresent };

        if let Some(item) = self.ledger.holds(key, album_id) {
            return Ok(already(item));
        }
        if let (Some(album), Some(item)) = (request.album, self.ledger.media_item(key)) {
            if self.add_to_album(album, &item).await? {
                self.ledger.record(key, &item, Some(&album.id))?;
                return Ok(already(item));
            }
            // The photo is gone from the account; the ledger was stale.
            self.ledger.forget(key)?;
        }

        let token = self.upload_bytes(request.file).await?;
        let item = self.create_media_item(request.file, &token, request.album).await?;
        self.ledger.record(key, &item, album_id)?;
        Ok(FileOutcome { remote_id: item, status: FileStatus::Uploaded })
    }
}
