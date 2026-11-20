//! Shared by the S3 plugin test binaries: a [`ContractFixture`] that runs
//! the plugin against any S3 endpoint (the in-process mock, or MinIO in a
//! container) with failures injected through the plugin's [`FaultPoints`].
#![allow(dead_code)]

pub mod scenarios;

use std::collections::BTreeMap;
use std::future::Future;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use fotobase_export::contract::{ContractFixture, Fault, RemoteAlbum};
use fotobase_export::s3::{FaultPoint, FaultPoints, S3Config, S3Credentials, S3Plugin};
use fotobase_export::{ContentKey, ExportError};
use rusty_s3::actions::{CreateBucket, ListObjectsV2};
use rusty_s3::{Bucket, Credentials, S3Action, UrlStyle};

pub const PREFIX: &str = "photos";
pub const REGION: &str = "us-east-1";
const TTL: std::time::Duration = std::time::Duration::from_secs(600);

#[derive(Debug, Clone)]
pub struct Endpoint {
    pub url: String,
    pub access_key: String,
    pub secret_key: String,
}

/// Runs a future on its own thread and runtime, so synchronous fixture
/// methods can call HTTP from inside a `#[tokio::test]`.
pub fn run<T: Send + 'static>(future: impl Future<Output = T> + Send + 'static) -> T {
    std::thread::spawn(move || {
        tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap().block_on(future)
    })
    .join()
    .unwrap()
}

/// Looks at a bucket the way an independent S3 client would.
#[derive(Clone)]
pub struct Admin {
    bucket: Bucket,
    credentials: Credentials,
}

impl Admin {
    pub fn new(endpoint: &Endpoint, bucket: &str) -> Self {
        let _ = rustls::crypto::ring::default_provider().install_default();
        Self {
            bucket: Bucket::new(
                endpoint.url.parse().unwrap(),
                UrlStyle::Path,
                bucket.to_owned(),
                REGION,
            )
            .unwrap(),
            credentials: Credentials::new(&endpoint.access_key, &endpoint.secret_key),
        }
    }

    pub fn create_bucket(&self) {
        let url = CreateBucket::new(&self.bucket, &self.credentials).sign(TTL);
        let admin = self.clone();
        run(async move {
            let status = reqwest::Client::new().put(url).send().await.unwrap().status();
            assert!(status.is_success(), "creating bucket {}: {status}", admin.bucket.name());
        });
    }

    /// Every object key with its size.
    pub fn objects(&self) -> Vec<(String, u64)> {
        let admin = self.clone();
        run(async move {
            let mut found = Vec::new();
            let mut token: Option<String> = None;
            loop {
                let mut action = admin.bucket.list_objects_v2(Some(&admin.credentials));
                if let Some(token) = &token {
                    action.with_continuation_token(token.as_str());
                }
                let body = reqwest::get(action.sign(TTL)).await.unwrap().text().await.unwrap();
                let page = ListObjectsV2::parse_response(&body).unwrap();
                found.extend(page.contents.into_iter().map(|c| (c.key, c.size.unwrap_or(0))));
                token = page.next_continuation_token;
                if token.is_none() {
                    return found;
                }
            }
        })
    }

    pub fn get(&self, key: &str) -> Vec<u8> {
        let url = self.bucket.get_object(Some(&self.credentials), key).sign(TTL);
        run(async move {
            let response = reqwest::get(url).await.unwrap();
            assert!(response.status().is_success(), "GET {}", response.status());
            response.bytes().await.unwrap().to_vec()
        })
    }

    pub fn put(&self, key: &str, body: Vec<u8>) {
        let url = self.bucket.put_object(Some(&self.credentials), key).sign(TTL);
        run(async move {
            let response = reqwest::Client::new().put(url).body(body).send().await.unwrap();
            assert!(response.status().is_success(), "PUT {}", response.status());
        });
    }

    /// Number of unfinished multipart uploads in the bucket.
    pub fn open_uploads(&self) -> usize {
        let url = rusty_s3::signing::sign(
            &jiff::Timestamp::now(),
            rusty_s3::Method::Get,
            self.bucket.base_url().clone(),
            self.credentials.key(),
            self.credentials.secret(),
            None,
            REGION,
            TTL.as_secs(),
            [("uploads", "")].into_iter(),
            std::iter::empty(),
        );
        run(async move {
            let body = reqwest::get(url).await.unwrap().text().await.unwrap();
            body.matches("<Upload>").count()
        })
    }
}

#[derive(Default)]
pub struct Faults {
    pub abort_after: Option<usize>,
    pub fail_once: bool,
    pub lose_upload_response_once: bool,
    pub lose_album_response_once: bool,
    pub transient_before_part: Option<u16>,
    pub fatal_before_part: Option<u16>,
    /// Parts the plugin set out to send, in order.
    pub parts_sent: Vec<u16>,
}

#[derive(Clone, Default)]
pub struct Injected(pub Arc<Mutex<Faults>>);

impl FaultPoints for Injected {
    fn reached(&self, point: FaultPoint) -> Result<(), ExportError> {
        let mut faults = self.0.lock().unwrap();
        match point {
            FaultPoint::BeforeCommit => {
                if std::mem::take(&mut faults.fail_once) {
                    return Err(ExportError::Transient("connection refused".into()));
                }
                match faults.abort_after {
                    Some(0) => return Err(ExportError::Transient("killed mid-upload".into())),
                    Some(n) => faults.abort_after = Some(n - 1),
                    None => {}
                }
            }
            FaultPoint::AfterCommit => {
                if std::mem::take(&mut faults.lose_upload_response_once) {
                    return Err(ExportError::Transient("response lost".into()));
                }
            }
            FaultPoint::BeforePart(n) => {
                faults.parts_sent.push(n);
                if faults.transient_before_part == Some(n) {
                    return Err(ExportError::Transient("killed mid-part".into()));
                }
                if faults.fatal_before_part == Some(n) {
                    return Err(ExportError::Fatal("rejected part".into()));
                }
            }
        }
        Ok(())
    }

    fn after_create_album(&self) -> Result<(), ExportError> {
        if std::mem::take(&mut self.0.lock().unwrap().lose_album_response_once) {
            return Err(ExportError::Transient("album response lost".into()));
        }
        Ok(())
    }
}

static COUNTER: AtomicUsize = AtomicUsize::new(0);

pub struct S3Fixture {
    pub endpoint: Endpoint,
    pub bucket: String,
    pub admin: Admin,
    pub faults: Injected,
    /// Send every file as a multipart upload (a single part when small).
    pub multipart: bool,
}

impl S3Fixture {
    /// A fresh bucket on `endpoint`, so checks never see each other's data.
    pub fn new(endpoint: &Endpoint, multipart: bool) -> Self {
        let nanos = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        let bucket = format!("fb-{nanos:x}-{}", COUNTER.fetch_add(1, Ordering::SeqCst));
        let admin = Admin::new(endpoint, &bucket);
        admin.create_bucket();
        Self { endpoint: endpoint.clone(), bucket, admin, faults: Injected::default(), multipart }
    }

    pub fn credentials(&self) -> S3Credentials {
        S3Credentials::new(&self.endpoint.access_key, &self.endpoint.secret_key).unwrap()
    }

    pub fn config(&self) -> S3Config {
        let config = S3Config::new(&self.endpoint.url, &self.bucket, REGION, self.credentials())
            .unwrap()
            .with_prefix(PREFIX)
            .unwrap();
        if self.multipart {
            config.with_multipart_threshold(0)
        } else {
            config
        }
    }

    pub fn plugin_with(&self, config: S3Config) -> S3Plugin {
        S3Plugin::new(config).unwrap().with_fault_points(Arc::new(self.faults.clone()))
    }
}

impl ContractFixture for S3Fixture {
    type Plugin = S3Plugin;

    fn plugin(&self) -> S3Plugin {
        self.plugin_with(self.config())
    }

    fn inject(&self, fault: Fault) {
        let mut faults = self.faults.0.lock().unwrap();
        match fault {
            Fault::AbortUploadsAfter(n) => faults.abort_after = Some(n),
            Fault::FailUploadOnce => faults.fail_once = true,
            Fault::LoseUploadResponseOnce => faults.lose_upload_response_once = true,
            Fault::LoseAlbumResponseOnce => faults.lose_album_response_once = true,
        }
    }

    fn clear_faults(&self) {
        let mut faults = self.faults.0.lock().unwrap();
        let sent = std::mem::take(&mut faults.parts_sent);
        *faults = Faults { parts_sent: sent, ..Faults::default() };
    }

    // Hashes what is actually stored, so a mangled upload cannot pass.
    fn remote_files(&self) -> Vec<ContentKey> {
        self.admin
            .objects()
            .into_iter()
            .map(|(key, _)| ContentKey::from_bytes(&self.admin.get(&key)))
            .collect()
    }

    fn remote_albums(&self) -> Vec<RemoteAlbum> {
        let base = format!("{PREFIX}/");
        let mut albums: BTreeMap<String, Vec<ContentKey>> = BTreeMap::new();
        for (key, _) in self.admin.objects() {
            let Some((album, _)) = key.strip_prefix(&base).and_then(|r| r.rsplit_once('/')) else {
                continue;
            };
            albums
                .entry(album.to_owned())
                .or_default()
                .push(ContentKey::from_bytes(&self.admin.get(&key)));
        }
        albums.into_iter().map(|(name, files)| RemoteAlbum { name, files }).collect()
    }
}
