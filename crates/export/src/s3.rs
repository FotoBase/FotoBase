//! S3-compatible export plugin (TASK-08-010, PLAN §6 P4 "plugins: …
//! S3-compatible", §7 F1 layer 1): uploads to AWS S3, Cloudflare R2, MinIO or
//! any other server that speaks the S3 API.
//!
//! - **Settings** ([`S3Config`]) are validated without touching the network;
//!   [`S3Plugin::validate_connection`] then checks endpoint, bucket and keys
//!   and fails with an error that says which one is wrong. Keys are given
//!   directly or read from an AWS-style credentials file
//!   ([`S3Credentials::from_file`]).
//! - **Object keys are `<prefix>/<album>/<content-key>_<filename>`**, so the
//!   key *is* the idempotency record: a retry, a re-run or a second job
//!   addresses the same object. An existing object of the same size answers
//!   [`FileStatus::AlreadyPresent`]; a lost response is harmless because the
//!   retry finds the committed object. Albums are key prefixes (the album name
//!   below the configured prefix; `/` nests), so creating one needs no
//!   request and is idempotent by construction.
//! - **Resumable multipart.** Files above [`S3Config::with_multipart_threshold`]
//!   go up in parts. A transient failure leaves the multipart upload open:
//!   the next attempt — from the same plugin or a fresh one after an app
//!   restart — finds it again through `ListMultipartUploads`, keeps the parts
//!   whose size and MD5 match the local file and sends only the rest. An
//!   unfinished upload is never visible as an object, and a failure that
//!   cannot be retried aborts it. Servers keep the parts of an upload that is
//!   never resumed until it is aborted, so buckets should carry an
//!   "abort incomplete multipart uploads" lifecycle rule.
//! - **No telemetry; secrets stay on the wire to the endpoint.** Requests are
//!   presigned (SigV4) with `rusty-s3` and sent only to the configured
//!   endpoint; redirects are refused and error messages never carry the
//!   signed URL.
//!
//! Sources are only ever read, and verified against their content key before
//! the first byte is sent.

use std::collections::{BTreeMap, BTreeSet};
use std::error::Error as _;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use instant_xml::FromXml;
use md5::{Digest, Md5};
use reqwest::header::{CONTENT_LENGTH, CONTENT_TYPE, ETAG};
use reqwest::{redirect, Client, Method, Response, StatusCode};
use rusty_s3::actions::{
    CompleteMultipartUpload, CompleteMultipartUploadResponse, CreateMultipartUpload, ListParts,
};
use rusty_s3::{Bucket, Credentials, S3Action, UrlStyle};
use url::Url;

use crate::{
    AlbumRef, ContentKey, ExportError, ExportPlugin, FileOutcome, FileStatus, FileUpload,
    PluginCapabilities, PluginInfo, RemoteFile,
};

pub const PLUGIN_ID: &str = "s3";

const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
/// Longest silence tolerated mid-transfer; a part may take far longer overall.
const READ_TIMEOUT: Duration = Duration::from_secs(120);
const SIGN_TTL: Duration = Duration::from_secs(60 * 60);
/// Smallest part S3 accepts (the last part may be smaller).
pub const MIN_PART_SIZE: u64 = 5 * 1024 * 1024;
const MAX_PART_SIZE: u64 = 5 * 1024 * 1024 * 1024;
const MAX_PARTS: u64 = 10_000;
const DEFAULT_PART_SIZE: u64 = 8 * 1024 * 1024;
/// A single PUT is buffered in memory, so its size is capped.
const MAX_SINGLE_PUT: u64 = 64 * 1024 * 1024;
const MAX_KEY_BYTES: usize = 1024;
const MAX_PREFIX_BYTES: usize = 200;
const MAX_ALBUM_BYTES: usize = 400;
const MAX_COMPONENT_BYTES: usize = 100;
const MAX_FILE_NAME_BYTES: usize = 150;
const DEFAULT_ALBUM: &str = "Untitled album";
const S3_XML_NS: &str = "http://s3.amazonaws.com/doc/2006-03-01/";

/// Access key pair, optionally with a session token (temporary credentials).
#[derive(Clone)]
pub struct S3Credentials {
    access_key: String,
    secret_key: String,
    session_token: Option<String>,
}

impl S3Credentials {
    pub fn new(access_key: &str, secret_key: &str) -> Result<Self, ExportError> {
        Ok(Self {
            access_key: secret_field(access_key, "access key")?,
            secret_key: secret_field(secret_key, "secret key")?,
            session_token: None,
        })
    }

    pub fn with_session_token(mut self, token: &str) -> Result<Self, ExportError> {
        self.session_token = Some(secret_field(token, "session token")?);
        Ok(self)
    }

    /// Reads `profile` from an AWS-style credentials file (`[profile]`
    /// sections with `aws_access_key_id`, `aws_secret_access_key` and an
    /// optional `aws_session_token`).
    pub fn from_file(path: &Path, profile: &str) -> Result<Self, ExportError> {
        let text = std::fs::read_to_string(path).map_err(|e| {
            ExportError::InvalidConfig(format!(
                "cannot read the credentials file {}: {e}",
                path.display()
            ))
        })?;
        let section = parse_ini_section(&text, profile).ok_or_else(|| {
            ExportError::InvalidConfig(format!(
                "the credentials file has no profile `{profile}` (looked in {})",
                path.display()
            ))
        })?;
        let get = |name: &str| section.get(name).map(String::as_str);
        let access = get("aws_access_key_id").ok_or_else(|| {
            ExportError::InvalidConfig(format!(
                "profile `{profile}` has no aws_access_key_id in {}",
                path.display()
            ))
        })?;
        let secret = get("aws_secret_access_key").ok_or_else(|| {
            ExportError::InvalidConfig(format!(
                "profile `{profile}` has no aws_secret_access_key in {}",
                path.display()
            ))
        })?;
        let credentials = Self::new(access, secret)?;
        match get("aws_session_token") {
            Some(token) if !token.is_empty() => credentials.with_session_token(token),
            _ => Ok(credentials),
        }
    }
}

impl std::fmt::Debug for S3Credentials {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("S3Credentials")
            .field("access_key", &"<redacted>")
            .field("secret_key", &"<redacted>")
            .field("session_token", &self.session_token.as_ref().map(|_| "<redacted>"))
            .finish()
    }
}

fn secret_field(value: &str, what: &str) -> Result<String, ExportError> {
    let value = value.trim();
    if value.is_empty() {
        return Err(ExportError::InvalidConfig(format!("enter the S3 {what}")));
    }
    if !value.bytes().all(|b| b.is_ascii_graphic()) {
        return Err(ExportError::InvalidConfig(format!(
            "the S3 {what} contains spaces or invalid characters"
        )));
    }
    Ok(value.to_owned())
}

/// Key/value pairs of one `[section]` of an INI file (`#`/`;` comments).
fn parse_ini_section(text: &str, wanted: &str) -> Option<BTreeMap<String, String>> {
    let mut in_section = false;
    let mut found = None;
    for line in text.lines().map(str::trim) {
        if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
            continue;
        }
        if let Some(name) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            in_section = name.trim() == wanted;
            if in_section {
                found.get_or_insert_with(BTreeMap::new);
            }
        } else if in_section {
            if let Some((key, value)) = line.split_once('=') {
                if let Some(map) = found.as_mut() {
                    map.insert(key.trim().to_ascii_lowercase(), value.trim().to_owned());
                }
            }
        }
    }
    found
}

/// Where the bucket is and how to reach it.
#[derive(Debug, Clone)]
pub struct S3Config {
    endpoint: Url,
    bucket: String,
    region: String,
    credentials: S3Credentials,
    prefix: String,
    url_style: UrlStyle,
    part_size: u64,
    multipart_threshold: u64,
}

impl S3Config {
    /// Validates the settings without any network access. `endpoint` is the
    /// service address, e.g. `https://s3.eu-west-1.amazonaws.com`,
    /// `https://<account>.r2.cloudflarestorage.com` or `http://localhost:9000`
    /// for MinIO. Buckets are addressed path-style (`endpoint/bucket/key`),
    /// which every S3-compatible server accepts.
    pub fn new(
        endpoint: &str,
        bucket: &str,
        region: &str,
        credentials: S3Credentials,
    ) -> Result<Self, ExportError> {
        let invalid = |what: &str| ExportError::InvalidConfig(what.to_owned());

        let raw = endpoint.trim();
        if raw.is_empty() {
            return Err(invalid("enter the S3 endpoint (e.g. https://s3.eu-west-1.amazonaws.com)"));
        }
        let endpoint = Url::parse(raw).map_err(|_| {
            invalid("the S3 endpoint is not a valid URL (expected e.g. https://s3.example.com)")
        })?;
        if !matches!(endpoint.scheme(), "http" | "https") {
            return Err(invalid("the S3 endpoint must start with http:// or https://"));
        }
        if endpoint.host_str().is_none() {
            return Err(invalid("the S3 endpoint has no host name"));
        }
        if !endpoint.username().is_empty() || endpoint.password().is_some() {
            return Err(invalid("do not put credentials in the S3 endpoint; use the key fields"));
        }
        if endpoint.query().is_some() || endpoint.fragment().is_some() {
            return Err(invalid("the S3 endpoint must not have a query or fragment"));
        }

        let bucket = bucket.trim();
        if bucket.is_empty() {
            return Err(invalid("enter the bucket name"));
        }
        validate_bucket_name(bucket)?;

        let region = region.trim();
        if region.is_empty() {
            return Err(invalid("enter the region (use `auto` for Cloudflare R2)"));
        }
        if !region.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_') {
            return Err(invalid("the region may only contain letters, digits, `-` and `_`"));
        }

        let config = Self {
            endpoint,
            bucket: bucket.to_owned(),
            region: region.to_owned(),
            credentials,
            prefix: String::new(),
            url_style: UrlStyle::Path,
            part_size: DEFAULT_PART_SIZE,
            multipart_threshold: DEFAULT_PART_SIZE,
        };
        config.bucket()?;
        Ok(config)
    }

    /// Plain AWS S3 in `region`, addressed virtual-host-style.
    pub fn aws(
        bucket: &str,
        region: &str,
        credentials: S3Credentials,
    ) -> Result<Self, ExportError> {
        let region = region.trim();
        Self::new(&format!("https://s3.{region}.amazonaws.com"), bucket, region, credentials)?
            .with_virtual_host_style()
    }

    /// Folder-like prefix every object goes below (`photos/fotobase`).
    pub fn with_prefix(mut self, prefix: &str) -> Result<Self, ExportError> {
        let mut parts = Vec::new();
        for part in prefix.split(['/', '\\']).map(str::trim).filter(|p| !p.is_empty()) {
            if part == "." || part == ".." {
                return Err(ExportError::InvalidConfig(
                    "the key prefix must not contain `.` or `..` segments".into(),
                ));
            }
            if part.chars().any(char::is_control) {
                return Err(ExportError::InvalidConfig(
                    "the key prefix contains control characters".into(),
                ));
            }
            parts.push(part);
        }
        let prefix = parts.join("/");
        if prefix.len() > MAX_PREFIX_BYTES {
            return Err(ExportError::InvalidConfig(format!(
                "the key prefix is longer than {MAX_PREFIX_BYTES} bytes"
            )));
        }
        self.prefix = prefix;
        Ok(self)
    }

    /// Address the bucket as `<bucket>.<endpoint host>` (AWS's preferred
    /// style) instead of `<endpoint>/<bucket>`.
    pub fn with_virtual_host_style(mut self) -> Result<Self, ExportError> {
        self.url_style = UrlStyle::VirtualHost;
        self.bucket().map_err(|_| {
            ExportError::InvalidConfig(
                "this endpoint and bucket name cannot be addressed virtual-host-style".into(),
            )
        })?;
        Ok(self)
    }

    /// Size of each multipart part; S3 requires at least [`MIN_PART_SIZE`]
    /// and at most 10 000 parts per object.
    pub fn with_part_size(mut self, bytes: u64) -> Result<Self, ExportError> {
        if !(MIN_PART_SIZE..=MAX_PART_SIZE).contains(&bytes) {
            return Err(ExportError::InvalidConfig(format!(
                "the part size must be between {} MiB and {} GiB",
                MIN_PART_SIZE >> 20,
                MAX_PART_SIZE >> 30
            )));
        }
        self.part_size = bytes;
        Ok(self)
    }

    /// Files larger than this are uploaded in parts. `0` uses parts for every
    /// non-empty file; the value is capped because smaller files are sent in
    /// one request held in memory.
    pub fn with_multipart_threshold(mut self, bytes: u64) -> Self {
        self.multipart_threshold = bytes.min(MAX_SINGLE_PUT);
        self
    }

    pub fn endpoint(&self) -> &str {
        self.endpoint.as_str()
    }

    pub fn bucket_name(&self) -> &str {
        &self.bucket
    }

    fn bucket(&self) -> Result<Bucket, ExportError> {
        Bucket::new(self.endpoint.clone(), self.url_style, self.bucket.clone(), self.region.clone())
            .map_err(|e| ExportError::InvalidConfig(format!("unusable S3 endpoint: {e:?}")))
    }
}

fn validate_bucket_name(name: &str) -> Result<(), ExportError> {
    let ok_chars =
        name.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"-.".contains(&b));
    let ends_ok = name.bytes().next().is_some_and(|b| b.is_ascii_alphanumeric())
        && name.bytes().last().is_some_and(|b| b.is_ascii_alphanumeric());
    if !(3..=63).contains(&name.len()) || !ok_chars || !ends_ok {
        return Err(ExportError::InvalidConfig(
            "the bucket name must be 3–63 characters of lowercase letters, digits, `-` and `.`, \
             starting and ending with a letter or digit"
                .into(),
        ));
    }
    Ok(())
}

/// Steps of an upload at which tests can simulate a crash or a lost
/// response. Production code uses the no-op default.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FaultPoint {
    /// Right before the object becomes visible: the single PUT, or the
    /// `CompleteMultipartUpload` request once every part is stored.
    BeforeCommit,
    /// Right after the object was committed; an error makes a stored file
    /// look failed.
    AfterCommit,
    /// Right before part `n` is sent.
    BeforePart(u16),
}

pub trait FaultPoints: Send + Sync {
    /// An error aborts the current attempt at that point.
    fn reached(&self, _point: FaultPoint) -> Result<(), ExportError> {
        Ok(())
    }

    /// Called once the album's prefix is resolved (albums need no request).
    fn after_create_album(&self) -> Result<(), ExportError> {
        Ok(())
    }
}

struct NoFaults;

impl FaultPoints for NoFaults {}

pub struct S3Plugin {
    config: S3Config,
    bucket: Bucket,
    credentials: Credentials,
    client: Client,
    faults: Arc<dyn FaultPoints>,
}

impl S3Plugin {
    pub fn new(config: S3Config) -> Result<Self, ExportError> {
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
        let bucket = config.bucket()?;
        let credentials = match &config.credentials.session_token {
            Some(token) => Credentials::new_with_token(
                config.credentials.access_key.clone(),
                config.credentials.secret_key.clone(),
                token.clone(),
            ),
            None => Credentials::new(
                config.credentials.access_key.clone(),
                config.credentials.secret_key.clone(),
            ),
        };
        Ok(Self { config, bucket, credentials, client, faults: Arc::new(NoFaults) })
    }

    pub fn with_fault_points(mut self, faults: Arc<dyn FaultPoints>) -> Self {
        self.faults = faults;
        self
    }

    /// Checks that the endpoint answers, the bucket exists and the keys are
    /// accepted (a read; write permission is only proven by the first upload).
    pub async fn validate_connection(&self) -> Result<(), ExportError> {
        self.list_objects(&self.base_prefix(), Some(1)).await.map(|_| ())
    }

    fn base_prefix(&self) -> String {
        if self.config.prefix.is_empty() {
            String::new()
        } else {
            format!("{}/", self.config.prefix)
        }
    }

    /// Directory part of the keys of `album` (no trailing slash; empty when
    /// the objects sit at the bucket root).
    fn dir_of(&self, album: Option<&AlbumRef>) -> String {
        let album = album.map(|a| clean_album(&a.name));
        match (self.config.prefix.as_str(), album) {
            ("", None) => String::new(),
            (prefix, None) => prefix.to_owned(),
            ("", Some(album)) => album,
            (prefix, Some(album)) => format!("{prefix}/{album}"),
        }
    }

    // ------------------------------------------------------------ HTTP

    async fn call(
        &self,
        method: Method,
        url: Url,
        content_type: Option<&str>,
        body: Option<Vec<u8>>,
    ) -> Result<Response, ExportError> {
        let mut request = self.client.request(method, url);
        if let Some(content_type) = content_type {
            request = request.header(CONTENT_TYPE, content_type);
        }
        if let Some(body) = body {
            request = request.body(body);
        }
        let response = request.send().await.map_err(|e| self.unreachable(e))?;
        if response.status().is_success() {
            return Ok(response);
        }
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        Err(self.failure(status, tag(&body, "Code"), tag(&body, "Message")))
    }

    fn unreachable(&self, error: reqwest::Error) -> ExportError {
        // The signed URL carries the access key id and signature.
        let error = error.without_url();
        let mut detail = error.to_string();
        let mut source = error.source();
        while let Some(cause) = source {
            detail.push_str(": ");
            detail.push_str(&cause.to_string());
            source = cause.source();
        }
        ExportError::Transient(format!(
            "cannot reach the S3 endpoint {}: {detail}",
            self.config.endpoint.host_str().unwrap_or("?")
        ))
    }

    /// Maps an S3 error response to the retry classes of [`ExportError`].
    fn failure(&self, status: StatusCode, code: &str, message: &str) -> ExportError {
        let detail = match (code.is_empty(), message.is_empty()) {
            (true, _) => format!("HTTP {status}"),
            (false, true) => format!("{code} (HTTP {status})"),
            (false, false) => format!("{code}: {message} (HTTP {status})"),
        };
        let fatal = |what: String| ExportError::Fatal(what);
        match (status.as_u16(), code) {
            (_, "NoSuchBucket") => fatal(format!(
                "bucket `{}` does not exist at {}",
                self.config.bucket,
                self.config.endpoint.host_str().unwrap_or("the endpoint")
            )),
            (_, "InvalidAccessKeyId") => {
                fatal("the access key is not known to the server; check the access key".into())
            }
            (_, "SignatureDoesNotMatch") => {
                fatal("the server rejected the signature; check the secret key and region".into())
            }
            (_, "ExpiredToken" | "InvalidToken") => {
                fatal("the session token is expired or invalid".into())
            }
            (_, "RequestTimeTooSkewed") => {
                fatal("this computer's clock differs too much from the server's".into())
            }
            (
                _,
                "AuthorizationHeaderMalformed"
                | "PermanentRedirect"
                | "IllegalLocationConstraintException",
            )
            | (301 | 307 | 308, _) => fatal(format!(
                "the bucket is in a different region than `{}` or the endpoint is wrong: {detail}",
                self.config.region
            )),
            (_, "AccessDenied" | "AllAccessDisabled") | (403, _) => {
                fatal(format!("access denied: the keys lack permission for this bucket ({detail})"))
            }
            (_, "NoSuchUpload") => ExportError::Transient(
                "the multipart upload no longer exists on the server; starting it again".into(),
            ),
            (408 | 429 | 500..=599, _) => ExportError::Transient(detail),
            (_, "SlowDown" | "RequestTimeout" | "InternalError" | "ServiceUnavailable") => {
                ExportError::Transient(detail)
            }
            _ => fatal(format!("the S3 server rejected the request: {detail}")),
        }
    }

    // ------------------------------------------------------- operations

    /// Size of the object at `key`, `None` when it does not exist.
    async fn head(&self, key: &str) -> Result<Option<u64>, ExportError> {
        let url = self.bucket.head_object(Some(&self.credentials), key).sign(SIGN_TTL);
        let response = self.client.head(url).send().await.map_err(|e| self.unreachable(e))?;
        match response.status() {
            StatusCode::NOT_FOUND => Ok(None),
            // A HEAD answer has no body, so only the status classifies it.
            status if !status.is_success() => Err(self.failure(status, "", "")),
            // `Response::content_length` reports the (empty) body of a HEAD
            // answer, not the header.
            _ => Ok(Some(
                response
                    .headers()
                    .get(CONTENT_LENGTH)
                    .and_then(|v| v.to_str().ok())
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(0),
            )),
        }
    }

    async fn list_objects(
        &self,
        prefix: &str,
        max_keys: Option<usize>,
    ) -> Result<Vec<(String, u64)>, ExportError> {
        let mut found = Vec::new();
        let mut token: Option<String> = None;
        loop {
            let mut action = self.bucket.list_objects_v2(Some(&self.credentials));
            action.with_prefix(prefix);
            if let Some(max) = max_keys {
                action.with_max_keys(max);
            }
            if let Some(token) = &token {
                action.with_continuation_token(token.as_str());
            }
            let url = action.sign(SIGN_TTL);
            let body = self
                .call(Method::GET, url, None, None)
                .await?
                .text()
                .await
                .map_err(|e| self.unreachable(e))?;
            let page = rusty_s3::actions::ListObjectsV2::parse_response(&body).map_err(|e| {
                ExportError::Fatal(format!("unexpected answer to a bucket listing: {e}"))
            })?;
            found.extend(page.contents.into_iter().map(|c| (c.key, c.size.unwrap_or(0))));
            token = page.next_continuation_token;
            if token.is_none() || max_keys.is_some() {
                return Ok(found);
            }
        }
    }

    async fn put(&self, key: &str, name: &str, body: Vec<u8>) -> Result<(), ExportError> {
        let content_type = content_type_of(name);
        let mut action = self.bucket.put_object(Some(&self.credentials), key);
        action.headers_mut().insert("content-type", content_type);
        let url = action.sign(SIGN_TTL);
        self.call(Method::PUT, url, Some(content_type), Some(body)).await.map(|_| ())
    }

    // -------------------------------------------------------- multipart

    async fn upload_multipart(
        &self,
        key: &str,
        name: &str,
        path: &Path,
        size: u64,
    ) -> Result<(), ExportError> {
        let part_size = self.config.part_size;
        let part_count = size.div_ceil(part_size);
        if part_count > MAX_PARTS {
            return Err(ExportError::Fatal(format!(
                "`{name}` needs {part_count} parts; S3 allows {MAX_PARTS}. \
                 Raise the part size in the upload settings"
            )));
        }
        let (upload_id, stored) = self.open_upload(key, name).await?;
        let result = self.send_parts(key, &upload_id, path, size, part_count, &stored).await;
        if let Err(ExportError::Fatal(_)) = &result {
            self.abort(key, &upload_id).await;
        }
        result
    }

    async fn send_parts(
        &self,
        key: &str,
        upload_id: &str,
        path: &Path,
        size: u64,
        part_count: u64,
        stored: &BTreeMap<u16, StoredPart>,
    ) -> Result<(), ExportError> {
        let part_size = self.config.part_size;
        let mut etags = Vec::with_capacity(part_count as usize);
        for number in 1..=part_count as u16 {
            let offset = (u64::from(number) - 1) * part_size;
            let len = part_size.min(size - offset);
            let data = read_range(path, offset, len).await?;
            let md5 = hex(&Md5::digest(&data));
            if let Some(part) = stored.get(&number) {
                if part.size == len && part.etag.trim_matches('"').eq_ignore_ascii_case(&md5) {
                    etags.push(part.etag.clone());
                    continue;
                }
            }
            self.faults.reached(FaultPoint::BeforePart(number))?;
            etags.push(self.upload_part(key, upload_id, number, data).await?);
        }
        self.faults.reached(FaultPoint::BeforeCommit)?;
        self.complete(key, upload_id, &etags).await
    }

    /// The unfinished upload for `key` (with the parts it already holds), or
    /// a new one. Older leftovers for the same key are aborted.
    async fn open_upload(
        &self,
        key: &str,
        name: &str,
    ) -> Result<(String, BTreeMap<u16, StoredPart>), ExportError> {
        let mut open = self.list_uploads(key).await?;
        let Some(newest) = open.pop() else {
            let content_type = content_type_of(name);
            let mut action = self.bucket.create_multipart_upload(Some(&self.credentials), key);
            action.headers_mut().insert("content-type", content_type);
            let url = action.sign(SIGN_TTL);
            let body = self
                .call(Method::POST, url, Some(content_type), None)
                .await?
                .text()
                .await
                .map_err(|e| self.unreachable(e))?;
            let created = CreateMultipartUpload::parse_response(&body).map_err(|e| {
                ExportError::Fatal(format!("unexpected answer to starting an upload: {e}"))
            })?;
            return Ok((created.upload_id().to_owned(), BTreeMap::new()));
        };
        for stale in &open {
            self.abort(key, stale).await;
        }
        let parts = self.list_parts(key, &newest).await?;
        Ok((newest, parts))
    }

    /// Ids of the unfinished uploads for exactly `key`, oldest first.
    async fn list_uploads(&self, key: &str) -> Result<Vec<String>, ExportError> {
        let mut url = self.bucket.base_url().clone();
        let signed = rusty_s3::signing::sign(
            &jiff::Timestamp::now(),
            rusty_s3::Method::Get,
            url.clone(),
            self.credentials.key(),
            self.credentials.secret(),
            self.credentials.token(),
            self.bucket.region(),
            SIGN_TTL.as_secs(),
            [("encoding-type", "url"), ("prefix", key), ("uploads", "")].into_iter(),
            std::iter::empty(),
        );
        url = signed;
        let body = self
            .call(Method::GET, url, None, None)
            .await?
            .text()
            .await
            .map_err(|e| self.unreachable(e))?;
        let parsed: ListUploadsResponse = instant_xml::from_str(&body).map_err(|e| {
            ExportError::Fatal(format!("unexpected answer to listing unfinished uploads: {e}"))
        })?;
        Ok(parsed
            .uploads
            .into_iter()
            .filter(|u| url_decode(&u.key) == key)
            .map(|u| u.upload_id)
            .collect())
    }

    async fn list_parts(
        &self,
        key: &str,
        upload_id: &str,
    ) -> Result<BTreeMap<u16, StoredPart>, ExportError> {
        let mut parts = BTreeMap::new();
        let mut marker = None;
        loop {
            let mut action = ListParts::new(&self.bucket, Some(&self.credentials), key, upload_id);
            if let Some(marker) = marker {
                action.set_part_number_marker(marker);
            }
            let url = action.sign(SIGN_TTL);
            let body = self
                .call(Method::GET, url, None, None)
                .await?
                .text()
                .await
                .map_err(|e| self.unreachable(e))?;
            let page = ListParts::parse_response(&body).map_err(|e| {
                ExportError::Fatal(format!("unexpected answer to listing uploaded parts: {e}"))
            })?;
            for part in page.parts {
                parts.insert(part.number, StoredPart { etag: part.etag, size: part.size });
            }
            marker = page.next_part_number_marker;
            if marker.is_none() {
                return Ok(parts);
            }
        }
    }

    async fn upload_part(
        &self,
        key: &str,
        upload_id: &str,
        number: u16,
        data: Vec<u8>,
    ) -> Result<String, ExportError> {
        let url =
            self.bucket.upload_part(Some(&self.credentials), key, number, upload_id).sign(SIGN_TTL);
        let response = self.call(Method::PUT, url, None, Some(data)).await?;
        response.headers().get(ETAG).and_then(|v| v.to_str().ok()).map(str::to_owned).ok_or_else(
            || ExportError::Fatal("the server did not return an ETag for a part".into()),
        )
    }

    async fn complete(
        &self,
        key: &str,
        upload_id: &str,
        etags: &[String],
    ) -> Result<(), ExportError> {
        let action = CompleteMultipartUpload::new(
            &self.bucket,
            Some(&self.credentials),
            key,
            upload_id,
            etags.iter().map(String::as_str),
        );
        let url = action.sign(SIGN_TTL);
        let body = action.body();
        let text = self
            .call(Method::POST, url, Some("application/xml"), Some(body.into_bytes()))
            .await?
            .text()
            .await
            .map_err(|e| self.unreachable(e))?;
        // S3 may answer 200 and still carry an <Error> in the body.
        if CompleteMultipartUploadResponse::parse(&text).is_ok() {
            return Ok(());
        }
        Err(match tag(&text, "Code") {
            "" => ExportError::Fatal("unexpected answer to finishing an upload".into()),
            code => self.failure(StatusCode::OK, code, tag(&text, "Message")),
        })
    }

    async fn abort(&self, key: &str, upload_id: &str) {
        let url = self
            .bucket
            .abort_multipart_upload(Some(&self.credentials), key, upload_id)
            .sign(SIGN_TTL);
        let _ = self.call(Method::DELETE, url, None, None).await;
    }
}

struct StoredPart {
    etag: String,
    size: u64,
}

#[derive(FromXml)]
#[xml(rename = "ListMultipartUploadsResult", ns(S3_XML_NS))]
struct ListUploadsResponse {
    uploads: Vec<ListedUpload>,
}

#[derive(FromXml)]
#[xml(rename = "Upload", ns(S3_XML_NS))]
struct ListedUpload {
    #[xml(rename = "Key")]
    key: String,
    #[xml(rename = "UploadId")]
    upload_id: String,
}

#[async_trait]
impl ExportPlugin for S3Plugin {
    fn info(&self) -> PluginInfo {
        PluginInfo {
            id: PLUGIN_ID.into(),
            name: "S3-compatible storage".into(),
            capabilities: PluginCapabilities { albums: true, size_choice: true },
        }
    }

    async fn list_albums(&mut self) -> Result<Vec<AlbumRef>, ExportError> {
        let base = self.base_prefix();
        let mut names = BTreeSet::new();
        for (key, _) in self.list_objects(&base, None).await? {
            let rest = &key[base.len()..];
            if let Some((album, file)) = rest.rsplit_once('/') {
                if parse_object_name(file).is_some() {
                    names.insert(album.to_owned());
                }
            }
        }
        Ok(names
            .into_iter()
            .map(|name| AlbumRef { id: join_key(&self.config.prefix, &name), name })
            .collect())
    }

    async fn create_album(&mut self, _job_id: &str, name: &str) -> Result<AlbumRef, ExportError> {
        // An album is a key prefix: the same name always maps to the same
        // prefix, which makes this idempotent for any job id.
        let name = clean_album(name);
        let album = AlbumRef { id: join_key(&self.config.prefix, &name), name };
        self.faults.after_create_album()?;
        Ok(album)
    }

    async fn list_files(
        &mut self,
        album: Option<&AlbumRef>,
    ) -> Result<Vec<RemoteFile>, ExportError> {
        let dir = self.dir_of(album);
        let prefix = if dir.is_empty() { dir } else { format!("{dir}/") };
        let listed = self.list_objects(&prefix, None).await?;
        Ok(listed
            .into_iter()
            .map(|(key, _)| {
                let file = key.rsplit('/').next().unwrap_or(&key);
                let (content_key, name) = match parse_object_name(file) {
                    Some((content_key, name)) => (Some(content_key), name.to_owned()),
                    None => (None, file.to_owned()),
                };
                RemoteFile { remote_id: key, name, content_key }
            })
            .collect())
    }

    async fn exists(
        &mut self,
        key: &ContentKey,
        album: Option<&AlbumRef>,
    ) -> Result<bool, ExportError> {
        let dir = self.dir_of(album);
        let prefix =
            format!("{}{key}_", if dir.is_empty() { String::new() } else { format!("{dir}/") });
        Ok(!self.list_objects(&prefix, Some(1)).await?.is_empty())
    }

    async fn upload_file(&mut self, request: FileUpload<'_>) -> Result<FileOutcome, ExportError> {
        let dir = self.dir_of(request.album);
        let key = object_key(&dir, &request.file.content_key, &request.file.name)?;
        let path = request.file.path.clone();
        let size = blocking(move || Ok(std::fs::metadata(&path)?.len())).await?;

        if self.head(&key).await? == Some(size) {
            return Ok(FileOutcome { remote_id: key, status: FileStatus::AlreadyPresent });
        }

        let (path, expected) = (request.file.path.clone(), request.file.content_key.clone());
        blocking(move || verify_source(&path, &expected)).await?;

        if size > 0 && size > self.config.multipart_threshold {
            self.upload_multipart(&key, &request.file.name, &request.file.path, size).await?;
        } else {
            let body = read_range(&request.file.path, 0, size).await?;
            self.faults.reached(FaultPoint::BeforeCommit)?;
            self.put(&key, &request.file.name, body).await?;
        }
        self.faults.reached(FaultPoint::AfterCommit)?;
        Ok(FileOutcome { remote_id: key, status: FileStatus::Uploaded })
    }
}

// ------------------------------------------------------------ keys and names

fn join_key(prefix: &str, rest: &str) -> String {
    if prefix.is_empty() {
        rest.to_owned()
    } else {
        format!("{prefix}/{rest}")
    }
}

/// `<dir>/<content-key>_<file name>`.
fn object_key(dir: &str, content_key: &ContentKey, file_name: &str) -> Result<String, ExportError> {
    let key = join_key(dir, &format!("{content_key}_{}", clean_file_name(file_name)));
    if key.len() > MAX_KEY_BYTES {
        return Err(ExportError::Fatal(format!(
            "the object key for `{file_name}` would exceed {MAX_KEY_BYTES} bytes; \
             shorten the key prefix or the album name"
        )));
    }
    Ok(key)
}

/// Sanitized album path (`/` or `\` nests); stable under re-cleaning.
fn clean_album(name: &str) -> String {
    let mut parts: Vec<String> = Vec::new();
    let mut total = 0;
    for part in name.split(['/', '\\']).map(str::trim).filter(|p| !p.is_empty()) {
        let part = clean_component(part, MAX_COMPONENT_BYTES);
        total += part.len() + 1;
        if total > MAX_ALBUM_BYTES {
            break;
        }
        parts.push(part);
    }
    if parts.is_empty() {
        DEFAULT_ALBUM.into()
    } else {
        parts.join("/")
    }
}

fn clean_file_name(name: &str) -> String {
    let name = name.replace(['/', '\\'], "_");
    let (stem, ext) = match name.rfind('.') {
        Some(dot) if dot > 0 && name.len() - dot <= 16 => name.split_at(dot),
        _ => (name.as_str(), ""),
    };
    let stem = clean_component(stem, MAX_FILE_NAME_BYTES.saturating_sub(ext.len()));
    format!("{stem}{ext}")
}

/// Control characters become `_`; `.`/`..` (which URL handling would
/// collapse) are replaced; the result is capped at `max` bytes.
fn clean_component(part: &str, max: usize) -> String {
    let replaced: String = part.chars().map(|c| if c.is_control() { '_' } else { c }).collect();
    let trimmed = replaced.trim();
    if trimmed.is_empty() || trimmed == "." || trimmed == ".." {
        return "_".into();
    }
    let mut end = trimmed.len().min(max.max(1));
    while !trimmed.is_char_boundary(end) {
        end -= 1;
    }
    trimmed[..end].trim_end().to_owned()
}

/// Splits `<64 hex>_<name>` back into its parts.
pub(crate) fn parse_object_name(file: &str) -> Option<(ContentKey, &str)> {
    let (key, name) = file.split_once('_')?;
    let is_key =
        key.len() == 64 && key.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b));
    is_key.then(|| (ContentKey::raw(key.to_owned()), name))
}

pub(crate) fn content_type_of(name: &str) -> &'static str {
    match name.rsplit('.').next().map(str::to_ascii_lowercase).as_deref() {
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("png") => "image/png",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        Some("avif") => "image/avif",
        Some("heic") => "image/heic",
        Some("heif") => "image/heif",
        Some("tif" | "tiff") => "image/tiff",
        Some("mp4" | "m4v") => "video/mp4",
        Some("mov") => "video/quicktime",
        _ => "application/octet-stream",
    }
}

// ----------------------------------------------------------------- helpers

/// Text of the first `<name>…</name>` in `xml`, or `""`.
fn tag<'a>(xml: &'a str, name: &str) -> &'a str {
    let open = format!("<{name}>");
    let close = format!("</{name}>");
    let Some(start) = xml.find(&open).map(|i| i + open.len()) else { return "" };
    match xml[start..].find(&close) {
        Some(len) => &xml[start..start + len],
        None => "",
    }
}

/// Reverses `encoding-type=url` (form-style: `+` is a space).
fn url_decode(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => out.push(b' '),
            b'%' if i + 2 < bytes.len()
                && bytes[i + 1].is_ascii_hexdigit()
                && bytes[i + 2].is_ascii_hexdigit() =>
            {
                out.push(u8::from_str_radix(&value[i + 1..i + 3], 16).unwrap_or(b'%'));
                i += 2;
            }
            byte => out.push(byte),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

pub(crate) async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> Result<T, ExportError> + Send + 'static,
) -> Result<T, ExportError> {
    tokio::task::spawn_blocking(work)
        .await
        .map_err(|e| ExportError::Fatal(format!("export worker failed: {e}")))?
}

pub(crate) async fn read_range(path: &Path, offset: u64, len: u64) -> Result<Vec<u8>, ExportError> {
    let path: PathBuf = path.to_path_buf();
    blocking(move || {
        let mut file = File::open(&path)?;
        file.seek(SeekFrom::Start(offset))?;
        let mut data = vec![0u8; len as usize];
        file.read_exact(&mut data)?;
        Ok(data)
    })
    .await
}

pub(crate) fn verify_source(path: &Path, expected: &ContentKey) -> Result<(), ExportError> {
    if &ContentKey::from_reader(File::open(path)?)? != expected {
        return Err(ExportError::Fatal(format!(
            "`{}` changed after it was hashed",
            path.display()
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn album_names_nest_and_sanitize_each_component() {
        assert_eq!(clean_album("Holiday"), "Holiday");
        assert_eq!(clean_album("Trips/2024\\Rome"), "Trips/2024/Rome");
        assert_eq!(clean_album("../../etc"), "_/_/etc");
        assert_eq!(clean_album("  //  "), DEFAULT_ALBUM);
        assert_eq!(clean_album("tab\there"), "tab_here");
    }

    #[test]
    fn cleaned_album_names_are_stable() {
        for name in ["a\u{1}b", "../x", "Trips/2024: Rome?", "é/ü", &"é".repeat(300)] {
            let once = clean_album(name);
            assert_eq!(clean_album(&once), once, "{name}");
        }
    }

    #[test]
    fn long_file_names_keep_their_extension() {
        let name = clean_file_name(&format!("{}.jpeg", "é".repeat(300)));
        assert!(name.len() <= MAX_FILE_NAME_BYTES);
        assert!(name.ends_with(".jpeg"));
        assert_eq!(clean_file_name("a/b.jpg"), "a_b.jpg");
    }

    #[test]
    fn object_names_round_trip() {
        let key = ContentKey::from_bytes(b"x");
        let object = object_key("p/Album", &key, "IMG 1.jpg").unwrap();
        assert_eq!(object, format!("p/Album/{key}_IMG 1.jpg"));
        let file = object.rsplit('/').next().unwrap();
        let (parsed, name) = parse_object_name(file).unwrap();
        assert_eq!((parsed, name), (key, "IMG 1.jpg"));
        assert!(parse_object_name("holiday_photo.jpg").is_none());
    }

    #[test]
    fn xml_helpers() {
        let xml = "<Error><Code>NoSuchBucket</Code><Message>gone</Message></Error>";
        assert_eq!(tag(xml, "Code"), "NoSuchBucket");
        assert_eq!(tag(xml, "Missing"), "");
        assert_eq!(url_decode("a+b%2Bc%2Fd"), "a b+c/d");
    }
}
