//! Immich export plugin (TASK-08-009, PLAN §6 P4 "plugins: … Immich"): uploads
//! to an Immich server through its REST API, authenticated with an API key.
//!
//! - **Settings** ([`ImmichConfig`]) are validated without touching the
//!   network; [`ImmichPlugin::validate_connection`] then checks the server and
//!   key and fails with an error that says what is wrong (unreachable, not an
//!   Immich server, rejected key…).
//! - **Idempotent per [`ContentKey`].** Immich de-duplicates uploads by
//!   checksum, so uploading a file the server already holds stores nothing
//!   and answers with the existing asset (`duplicate` →
//!   [`FileStatus::AlreadyPresent`]). The key is also sent as `deviceAssetId`
//!   so a later read-back can map assets to Fotobase files again.
//! - **Albums** are looked up by name before one is created, so a retry after
//!   a lost response, or a re-run of an interrupted job, never creates a
//!   second album. An album is only filled after its asset exists, and adding
//!   an asset that is already a member counts as success.
//! - **Read-back** ([`ExportPlugin::list_files`]) already works per album; the
//!   upload path does not depend on it, so a "cloud mirror" view can grow here
//!   without changing the trait.
//! - **No telemetry; the key stays on the server.** Requests go only to the
//!   configured server and redirects are refused: the `x-api-key` header
//!   would otherwise follow a redirect to another host.
//!
//! Sources are only ever read.

use std::error::Error as _;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use reqwest::header::{HeaderMap, HeaderValue};
use reqwest::multipart::{Form, Part};
use reqwest::{redirect, Body, Client, Method, Response, StatusCode};
use serde_json::{json, Value};
use tokio_util::io::ReaderStream;
use url::Url;

use crate::{
    AlbumRef, ContentKey, ExportError, ExportPlugin, FileOutcome, FileStatus, FileUpload,
    PluginCapabilities, PluginInfo, RemoteFile,
};

pub const PLUGIN_ID: &str = "immich";

/// Identifies Fotobase as the uploading "device" in Immich.
const DEVICE_ID: &str = "fotobase";
const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
/// Longest silence tolerated mid-transfer; large uploads may take far longer
/// overall.
const READ_TIMEOUT: Duration = Duration::from_secs(120);

/// Where the Immich server is and the key to talk to it.
#[derive(Clone)]
pub struct ImmichConfig {
    base: Url,
    api_key: String,
}

impl ImmichConfig {
    /// Validates the settings without any network access. `server_url` may be
    /// the instance root or its `/api` address, with or without a trailing
    /// slash.
    pub fn new(server_url: &str, api_key: &str) -> Result<Self, ExportError> {
        let invalid = |what: &str| ExportError::InvalidConfig(what.to_owned());

        let raw = server_url.trim();
        if raw.is_empty() {
            return Err(invalid("enter the address of your Immich server"));
        }
        let mut base = Url::parse(raw).map_err(|_| {
            invalid(
                "the server address is not a valid URL (expected e.g. https://photos.example.com)",
            )
        })?;
        if !matches!(base.scheme(), "http" | "https") {
            return Err(invalid("the server address must start with http:// or https://"));
        }
        if base.host_str().is_none() {
            return Err(invalid("the server address has no host name"));
        }
        if !base.username().is_empty() || base.password().is_some() {
            return Err(invalid("do not put credentials in the server address; use an API key"));
        }
        if base.query().is_some() || base.fragment().is_some() {
            return Err(invalid("the server address must not have a query or fragment"));
        }
        let mut path = base.path().trim_end_matches('/');
        if let Some(root) = path.strip_suffix("/api") {
            path = root;
        } else if path == "api" {
            path = "";
        }
        let path = path.to_owned();
        base.set_path(&path);

        let api_key = api_key.trim();
        if api_key.is_empty() {
            return Err(invalid("enter an Immich API key"));
        }
        if !api_key.bytes().all(|b| b.is_ascii_graphic()) {
            return Err(invalid("the API key contains spaces or invalid characters"));
        }
        Ok(Self { base, api_key: api_key.to_owned() })
    }

    /// The instance root, without trailing slash.
    pub fn server_url(&self) -> &str {
        self.base.as_str().trim_end_matches('/')
    }
}

impl std::fmt::Debug for ImmichConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ImmichConfig")
            .field("server_url", &self.server_url())
            .field("api_key", &"<redacted>")
            .finish()
    }
}

pub struct ImmichPlugin {
    config: ImmichConfig,
    client: Client,
}

impl ImmichPlugin {
    pub fn new(config: ImmichConfig) -> Result<Self, ExportError> {
        // reqwest is built without a bundled TLS provider. Another one
        // already installed process-wide is fine, so the error is ignored.
        let _ = rustls::crypto::ring::default_provider().install_default();
        let mut key = HeaderValue::from_str(&config.api_key)
            .map_err(|_| ExportError::InvalidConfig("the API key is not a valid header".into()))?;
        key.set_sensitive(true);
        let mut headers = HeaderMap::new();
        headers.insert("x-api-key", key);
        headers.insert("accept", HeaderValue::from_static("application/json"));

        let client = Client::builder()
            .default_headers(headers)
            .user_agent(concat!("Fotobase/", env!("CARGO_PKG_VERSION")))
            .redirect(redirect::Policy::none())
            .connect_timeout(CONNECT_TIMEOUT)
            .read_timeout(READ_TIMEOUT)
            .build()
            .map_err(|e| ExportError::Fatal(format!("cannot set up the HTTP client: {e}")))?;
        Ok(Self { config, client })
    }

    /// Checks that the server answers as Immich and accepts the API key.
    pub async fn validate_connection(&self) -> Result<(), ExportError> {
        self.send(Method::GET, &["users", "me"], |r| r).await.map(|_| ())
    }

    fn endpoint(&self, segments: &[&str]) -> Url {
        let mut url = self.config.base.clone();
        if let Ok(mut path) = url.path_segments_mut() {
            path.pop_if_empty().push("api").extend(segments);
        }
        url
    }

    async fn send(
        &self,
        method: Method,
        segments: &[&str],
        build: impl FnOnce(reqwest::RequestBuilder) -> reqwest::RequestBuilder,
    ) -> Result<Response, ExportError> {
        let request = self.client.request(method, self.endpoint(segments));
        let response = build(request).send().await.map_err(|e| self.unreachable(e))?;
        self.check(response).await
    }

    async fn send_json(
        &self,
        method: Method,
        segments: &[&str],
        body: Option<Value>,
    ) -> Result<Value, ExportError> {
        let response = self
            .send(method, segments, |r| match body {
                Some(body) => r.json(&body),
                None => r,
            })
            .await?;
        response.json().await.map_err(|e| self.unreachable(e))
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
        ExportError::Transient(format!(
            "cannot reach the Immich server at {}: {detail}",
            self.config.server_url()
        ))
    }

    async fn check(&self, response: Response) -> Result<Response, ExportError> {
        let status = response.status();
        if status.is_success() {
            return Ok(response);
        }
        let server = self.config.server_url();
        let location =
            response.headers().get("location").and_then(|v| v.to_str().ok()).map(str::to_owned);
        let detail = server_message(response).await;
        let at_status = format!("HTTP {}{detail}", status.as_u16());
        Err(match status {
            s if s.is_redirection() => ExportError::Fatal(format!(
                "the Immich server at {server} redirected to {}; Fotobase does not follow \
                 redirects so the API key stays on the server you entered. Use the final \
                 address as the server URL",
                location.as_deref().unwrap_or("another address")
            )),
            StatusCode::UNAUTHORIZED => {
                ExportError::Fatal(format!("Immich rejected the API key ({at_status})"))
            }
            StatusCode::FORBIDDEN => ExportError::Fatal(format!(
                "the Immich API key lacks permission for this action ({at_status}); \
                 it needs asset upload and album permissions"
            )),
            StatusCode::NOT_FOUND => ExportError::Fatal(format!(
                "{server} does not look like an Immich server ({at_status}); check the address"
            )),
            StatusCode::REQUEST_TIMEOUT | StatusCode::TOO_MANY_REQUESTS => {
                ExportError::Transient(format!("the Immich server is busy ({at_status})"))
            }
            s if s.is_server_error() => {
                ExportError::Transient(format!("the Immich server failed ({at_status})"))
            }
            _ => ExportError::Fatal(format!("Immich rejected the request ({at_status})")),
        })
    }

    async fn add_to_album(&self, album: &AlbumRef, asset_id: &str) -> Result<(), ExportError> {
        let results = self
            .send_json(
                Method::PUT,
                &["albums", &album.id, "assets"],
                Some(json!({ "ids": [asset_id] })),
            )
            .await?;
        let results = results.as_array().ok_or_else(|| {
            ExportError::Fatal("unexpected answer from Immich when adding to the album".into())
        })?;
        for result in results {
            let ok = result["success"].as_bool().unwrap_or(false)
                || result["error"].as_str() == Some("duplicate");
            if !ok {
                return Err(ExportError::Fatal(format!(
                    "Immich could not add the photo to album \"{}\": {}",
                    album.name,
                    result["error"].as_str().unwrap_or("unknown error")
                )));
            }
        }
        Ok(())
    }
}

/// `": <message>"` from an Immich error body, or empty.
async fn server_message(response: Response) -> String {
    let Ok(body) = response.text().await else { return String::new() };
    let message = match serde_json::from_str::<Value>(&body) {
        Ok(json) => match &json["message"] {
            Value::String(s) => s.clone(),
            Value::Array(items) => {
                items.iter().filter_map(Value::as_str).collect::<Vec<_>>().join("; ")
            }
            _ => String::new(),
        },
        Err(_) => String::new(),
    };
    if message.is_empty() {
        String::new()
    } else {
        format!(": {}", message.chars().take(200).collect::<String>())
    }
}

#[async_trait]
impl ExportPlugin for ImmichPlugin {
    fn info(&self) -> PluginInfo {
        PluginInfo {
            id: PLUGIN_ID.into(),
            name: "Immich".into(),
            capabilities: PluginCapabilities { albums: true, size_choice: true },
        }
    }

    async fn list_albums(&mut self) -> Result<Vec<AlbumRef>, ExportError> {
        let albums = self.send_json(Method::GET, &["albums"], None).await?;
        Ok(albums
            .as_array()
            .map(Vec::as_slice)
            .unwrap_or_default()
            .iter()
            .filter_map(|a| {
                Some(AlbumRef {
                    id: a["id"].as_str()?.into(),
                    name: a["albumName"].as_str()?.into(),
                })
            })
            .collect())
    }

    async fn create_album(&mut self, _job_id: &str, name: &str) -> Result<AlbumRef, ExportError> {
        if let Some(existing) = self.list_albums().await?.into_iter().find(|a| a.name == name) {
            return Ok(existing);
        }
        let created =
            self.send_json(Method::POST, &["albums"], Some(json!({ "albumName": name }))).await?;
        let id = created["id"]
            .as_str()
            .ok_or_else(|| ExportError::Fatal("Immich answered without an album id".into()))?;
        Ok(AlbumRef { id: id.into(), name: name.into() })
    }

    async fn list_files(
        &mut self,
        album: Option<&AlbumRef>,
    ) -> Result<Vec<RemoteFile>, ExportError> {
        let Some(album) = album else { return Err(ExportError::ReadBackUnsupported) };
        let album = self.send_json(Method::GET, &["albums", &album.id], None).await?;
        Ok(album["assets"]
            .as_array()
            .map(Vec::as_slice)
            .unwrap_or_default()
            .iter()
            .filter_map(|a| {
                Some(RemoteFile {
                    remote_id: a["id"].as_str()?.into(),
                    name: a["originalFileName"].as_str().unwrap_or_default().into(),
                    content_key: a["deviceAssetId"].as_str().and_then(content_key),
                })
            })
            .collect())
    }

    async fn upload_file(&mut self, request: FileUpload<'_>) -> Result<FileOutcome, ExportError> {
        let file = tokio::fs::File::open(&request.file.path).await?;
        let meta = file.metadata().await?;
        let modified = meta.modified().unwrap_or(UNIX_EPOCH);
        let created = meta.created().unwrap_or(modified);

        let data = Part::stream_with_length(Body::wrap_stream(ReaderStream::new(file)), meta.len())
            .file_name(request.file.name.clone())
            .mime_str("application/octet-stream")
            .map_err(|e| ExportError::Fatal(e.to_string()))?;
        let form = Form::new()
            .text("deviceAssetId", request.file.content_key.to_string())
            .text("deviceId", DEVICE_ID)
            .text("fileCreatedAt", iso8601(created))
            .text("fileModifiedAt", iso8601(modified))
            .part("assetData", data);

        let response = self.send(Method::POST, &["assets"], |r| r.multipart(form)).await?;
        let http_status = response.status();
        let body: Value = response.json().await.map_err(|e| self.unreachable(e))?;
        let id = body["id"]
            .as_str()
            .ok_or_else(|| {
                ExportError::Fatal("Immich answered the upload without an asset id".into())
            })?
            .to_owned();
        let status = match body["status"].as_str() {
            Some("duplicate") => FileStatus::AlreadyPresent,
            Some(_) => FileStatus::Uploaded,
            None if http_status == StatusCode::OK => FileStatus::AlreadyPresent,
            None => FileStatus::Uploaded,
        };

        if let Some(album) = request.album {
            self.add_to_album(album, &id).await?;
        }
        Ok(FileOutcome { remote_id: id, status })
    }
}

/// A `deviceAssetId` Fotobase wrote: lowercase hex SHA-256.
fn content_key(device_asset_id: &str) -> Option<ContentKey> {
    let valid = device_asset_id.len() == 64
        && device_asset_id.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'));
    valid.then(|| ContentKey::raw(device_asset_id.to_owned()))
}

/// UTC timestamp as Immich expects it (`2024-05-01T12:30:00.000Z`).
fn iso8601(time: SystemTime) -> String {
    let secs = time.duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs());
    let (days, rem) = ((secs / 86_400) as i64, secs % 86_400);
    // Days since 1970-01-01 to civil date (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}.000Z",
        rem / 3_600,
        rem % 3_600 / 60,
        rem % 60
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(secs: u64) -> SystemTime {
        UNIX_EPOCH + Duration::from_secs(secs)
    }

    #[test]
    fn timestamps_are_formatted_as_utc_iso8601() {
        assert_eq!(iso8601(at(0)), "1970-01-01T00:00:00.000Z");
        assert_eq!(iso8601(at(951_782_400 + 86_399)), "2000-02-29T23:59:59.000Z");
        assert_eq!(iso8601(at(1_714_566_600)), "2024-05-01T12:30:00.000Z");
    }

    #[test]
    fn only_fotobase_device_asset_ids_become_content_keys() {
        let key = ContentKey::from_bytes(b"x");
        assert_eq!(content_key(key.as_str()), Some(key));
        assert_eq!(content_key("IMG_0001"), None);
        assert_eq!(content_key(&"A".repeat(64)), None);
    }
}
