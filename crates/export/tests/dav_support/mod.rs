//! Shared by the WebDAV plugin test binaries: a [`ContractFixture`] that runs
//! the plugin against any WebDAV endpoint (the in-process mock, or Apache
//! `mod_dav` in a container) with failures injected through the plugin's
//! [`FaultPoints`]. The [`Admin`] inspects the server with its own HTTP
//! client, independent of the plugin.
#![allow(dead_code)]

pub mod mock;
pub mod scenarios;

use std::collections::BTreeMap;
use std::future::Future;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use fotobase_export::contract::{ContractFixture, Fault, RemoteAlbum};
use fotobase_export::webdav::{FaultPoint, FaultPoints, WebDavConfig, WebDavPlugin};
use fotobase_export::{ContentKey, ExportError};
use reqwest::Method;
use url::Url;

#[derive(Debug, Clone)]
pub struct Endpoint {
    /// A collection that exists, with trailing slash.
    pub url: String,
    pub username: String,
    pub password: String,
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Listed {
    /// Decoded path relative to the admin's root, no slashes at the ends.
    pub rel: String,
    pub dir: bool,
}

/// Looks at a collection the way an independent WebDAV client would.
#[derive(Clone)]
pub struct Admin {
    root: Url,
    username: String,
    password: String,
}

impl Admin {
    pub fn new(endpoint: &Endpoint) -> Self {
        let _ = rustls::crypto::ring::default_provider().install_default();
        Self {
            root: endpoint.url.parse().unwrap(),
            username: endpoint.username.clone(),
            password: endpoint.password.clone(),
        }
    }

    fn url(&self, rel: &str, dir: bool) -> Url {
        let mut url = self.root.clone();
        {
            let mut segments = url.path_segments_mut().unwrap();
            segments.pop_if_empty();
            segments.extend(rel.split('/').filter(|s| !s.is_empty()));
            if dir {
                segments.push("");
            }
        }
        url
    }

    fn call(&self, method: &str, url: Url, depth: Option<&str>, body: Vec<u8>) -> (u16, Vec<u8>) {
        let (user, password) = (self.username.clone(), self.password.clone());
        let method = Method::from_bytes(method.as_bytes()).unwrap();
        let depth = depth.map(str::to_owned);
        run(async move {
            let mut request = reqwest::Client::new()
                .request(method, url)
                .basic_auth(user, Some(password))
                .body(body);
            if let Some(depth) = depth {
                request = request.header("Depth", depth);
            }
            let response = request.send().await.unwrap();
            let status = response.status().as_u16();
            (status, response.bytes().await.unwrap().to_vec())
        })
    }

    /// A fresh sub-collection, so checks never see each other's data.
    pub fn make_root(&self) -> Admin {
        let nanos = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        let name = format!("fb-{nanos:x}-{}", COUNTER.fetch_add(1, Ordering::SeqCst));
        let url = self.url(&name, true);
        let (status, _) = self.call("MKCOL", url.clone(), None, Vec::new());
        assert_eq!(status, 201, "creating test collection {name}");
        Admin { root: url, ..self.clone() }
    }

    pub fn root_url(&self) -> String {
        self.root.to_string()
    }

    /// Every file and folder below the root, recursively.
    pub fn tree(&self) -> Vec<Listed> {
        let mut out = Vec::new();
        let mut pending = vec![String::new()];
        while let Some(dir) = pending.pop() {
            let body = vec![];
            let (status, xml) = self.call("PROPFIND", self.url(&dir, true), Some("1"), body);
            assert!(status == 207 || status == 404, "PROPFIND {dir:?}: {status}");
            for (href, is_dir) in scan_multistatus(&String::from_utf8_lossy(&xml)) {
                let rel = self.relative(&href);
                if rel == dir || rel.is_empty() {
                    continue;
                }
                if is_dir {
                    pending.push(rel.clone());
                }
                out.push(Listed { rel, dir: is_dir });
            }
        }
        out.sort_by(|a, b| a.rel.cmp(&b.rel));
        out
    }

    pub fn files(&self) -> Vec<String> {
        self.tree().into_iter().filter(|l| !l.dir).map(|l| l.rel).collect()
    }

    fn relative(&self, href: &str) -> String {
        let base: Vec<_> = self.root.path_segments().unwrap().filter(|s| !s.is_empty()).collect();
        let path = Url::parse(href).map(|u| u.path().to_owned()).unwrap_or_else(|_| href.into());
        let decoded: Vec<String> = path
            .split('/')
            .filter(|s| !s.is_empty())
            .map(|s| percent_encoding::percent_decode_str(s).decode_utf8_lossy().into_owned())
            .collect();
        decoded[base.len().min(decoded.len())..].join("/")
    }

    pub fn get(&self, rel: &str) -> Vec<u8> {
        let (status, body) = self.call("GET", self.url(rel, false), None, Vec::new());
        assert_eq!(status, 200, "GET {rel}");
        body
    }

    pub fn assert_missing(&self, rel: &str) {
        let (status, _) = self.call("GET", self.url(rel, false), None, Vec::new());
        assert_eq!(status, 404, "GET {rel}");
    }

    pub fn put(&self, rel: &str, body: Vec<u8>) {
        let (status, _) = self.call("PUT", self.url(rel, false), None, body);
        assert!((200..300).contains(&status), "PUT {rel}: {status}");
    }

    pub fn mkcol(&self, rel: &str) {
        let (status, _) = self.call("MKCOL", self.url(rel, true), None, Vec::new());
        assert!((200..300).contains(&status), "MKCOL {rel}: {status}");
    }

    pub fn delete(&self, rel: &str) {
        let (status, _) = self.call("DELETE", self.url(rel, false), None, Vec::new());
        assert!((200..300).contains(&status), "DELETE {rel}: {status}");
    }
}

/// `(href, is_collection)` per `<response>`, ignoring namespace prefixes.
fn scan_multistatus(xml: &str) -> Vec<(String, bool)> {
    let mut out: Vec<(String, bool)> = Vec::new();
    let mut rest = xml;
    while let Some(open) = rest.find('<') {
        let Some(close) = rest[open..].find('>') else { break };
        let tag = &rest[open + 1..open + close];
        rest = &rest[open + close + 1..];
        let name = tag.trim_end_matches('/').split_whitespace().next().unwrap_or("");
        let local = name.rsplit(':').next().unwrap_or("");
        match local {
            "response" => out.push((String::new(), false)),
            "href" if !tag.starts_with('/') => {
                let end = rest.find('<').unwrap_or(rest.len());
                if let Some(last) = out.last_mut() {
                    last.0 = rest[..end].to_owned();
                }
            }
            "collection" if !tag.starts_with('/') => {
                if let Some(last) = out.last_mut() {
                    last.1 = true;
                }
            }
            _ => {}
        }
    }
    out
}

#[derive(Default)]
pub struct Faults {
    pub abort_after: Option<usize>,
    pub fail_once: bool,
    pub lose_upload_response_once: bool,
    pub lose_album_response_once: bool,
    pub kill_before_move: bool,
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
            FaultPoint::BeforeMove => {
                if faults.kill_before_move {
                    return Err(ExportError::Transient("killed before the rename".into()));
                }
            }
            FaultPoint::AfterCommit => {
                if std::mem::take(&mut faults.lose_upload_response_once) {
                    return Err(ExportError::Transient("response lost".into()));
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

pub struct DavFixture {
    pub endpoint: Endpoint,
    /// The collection this fixture owns; also the plugin's WebDAV URL.
    pub admin: Admin,
    pub faults: Injected,
}

impl DavFixture {
    pub fn new(endpoint: &Endpoint) -> Self {
        let admin = Admin::new(endpoint).make_root();
        let endpoint = Endpoint { url: admin.root_url(), ..endpoint.clone() };
        Self { endpoint, admin, faults: Injected::default() }
    }

    pub fn config(&self) -> WebDavConfig {
        WebDavConfig::new(&self.endpoint.url, &self.endpoint.username, &self.endpoint.password)
            .unwrap()
    }

    pub fn plugin_with(&self, config: WebDavConfig) -> WebDavPlugin {
        WebDavPlugin::new(config).unwrap().with_fault_points(Arc::new(self.faults.clone()))
    }
}

impl ContractFixture for DavFixture {
    type Plugin = WebDavPlugin;

    fn plugin(&self) -> WebDavPlugin {
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
        *self.faults.0.lock().unwrap() = Faults::default();
    }

    // Every file on the server, staging leftovers included; hashes what is
    // actually stored, so a mangled or half-written upload cannot pass.
    fn remote_files(&self) -> Vec<ContentKey> {
        self.admin.files().iter().map(|f| ContentKey::from_bytes(&self.admin.get(f))).collect()
    }

    fn remote_albums(&self) -> Vec<RemoteAlbum> {
        let tree = self.admin.tree();
        let mut albums: BTreeMap<String, Vec<ContentKey>> = BTreeMap::new();
        for dir in tree.iter().filter(|l| l.dir) {
            albums.entry(dir.rel.clone()).or_default();
        }
        for file in tree.iter().filter(|l| !l.dir) {
            if let Some((dir, _)) = file.rel.rsplit_once('/') {
                albums
                    .entry(dir.to_owned())
                    .or_default()
                    .push(ContentKey::from_bytes(&self.admin.get(&file.rel)));
            }
        }
        albums.into_iter().map(|(name, files)| RemoteAlbum { name, files }).collect()
    }
}
