//! In-process WebDAV server for the plugin tests: the subset of RFC 4918 the
//! plugin uses (PROPFIND, MKCOL, PUT, MOVE, DELETE, GET, OPTIONS) under
//! `/dav/`, with Basic or RFC 7616 Digest authentication. Responses mimic
//! Nextcloud/sabre (`d:` prefix, foreign `oc:` properties, absolute-path
//! hrefs, 405 for MKCOL on an existing collection).
#![allow(dead_code)]

use std::collections::{BTreeMap, BTreeSet};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread;
use std::time::Duration;

use md5::{Digest, Md5};

pub const USER: &str = "alice";
pub const PASSWORD: &str = "wonder land";
const REALM: &str = "fotobase-test";
const ROOT: &str = "/dav";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthMode {
    Open,
    Basic,
    Digest,
}

pub struct State {
    pub auth: AuthMode,
    pub files: BTreeMap<String, Vec<u8>>,
    pub dirs: BTreeSet<String>,
    /// `METHOD /decoded/path` of every request, in arrival order.
    pub log: Vec<String>,
    pub move_supported: bool,
    /// Answer the next request with this status instead of serving it.
    pub fail_next: Option<u16>,
    /// The next PUT keeps the first half of its body, then the connection
    /// drops without an answer (a transfer killed midway).
    pub drop_next_put: bool,
    /// Make the current Digest nonce stale before the next request.
    pub stale_nonce_next: bool,
    /// Namespace prefix used for DAV elements in PROPFIND answers.
    pub prefix: String,
    /// Number of requests that passed authentication with Digest.
    pub digest_requests: usize,
    nonce: usize,
}

impl State {
    fn new(auth: AuthMode) -> Self {
        Self {
            auth,
            files: BTreeMap::new(),
            dirs: BTreeSet::from([ROOT.to_owned()]),
            log: Vec::new(),
            move_supported: true,
            fail_next: None,
            drop_next_put: false,
            stale_nonce_next: false,
            prefix: "d".into(),
            digest_requests: 0,
            nonce: 1,
        }
    }

    fn nonce(&self) -> String {
        format!("nonce-{}", self.nonce)
    }

    pub fn count(&self, method: &str) -> usize {
        self.log.iter().filter(|l| l.starts_with(&format!("{method} "))).count()
    }
}

pub struct MockDav {
    addr: String,
    state: Arc<Mutex<State>>,
    stop: Arc<AtomicBool>,
}

impl MockDav {
    pub fn start(auth: AuthMode) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        let state = Arc::new(Mutex::new(State::new(auth)));
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

    /// The collection users enter as the WebDAV URL.
    pub fn url(&self) -> String {
        format!("http://{}{ROOT}/", self.addr)
    }

    pub fn state(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap()
    }
}

impl Drop for MockDav {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        let _ = TcpStream::connect(&self.addr);
    }
}

struct Request {
    method: String,
    path: String,
    headers: BTreeMap<String, String>,
    body: Vec<u8>,
}

impl Request {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers.get(name).map(String::as_str)
    }
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

fn pdecode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            out.push(u8::from_str_radix(&s[i + 1..i + 3], 16).unwrap());
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).unwrap()
}

fn urlenc(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b'/' => {
                (b as char).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect()
}

fn read_request(stream: &mut TcpStream) -> Option<Request> {
    stream.set_read_timeout(Some(Duration::from_secs(10))).ok()?;
    let mut buf = Vec::new();
    let mut chunk = [0u8; 64 * 1024];
    let head_end = loop {
        if let Some(i) = find(&buf, b"\r\n\r\n") {
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
    let headers: BTreeMap<String, String> = lines
        .filter_map(|l| l.split_once(':'))
        .map(|(k, v)| (k.trim().to_ascii_lowercase(), v.trim().to_owned()))
        .collect();
    let len: usize = headers.get("content-length").and_then(|v| v.parse().ok()).unwrap_or(0);
    let mut body = buf[head_end + 4..].to_vec();
    while body.len() < len {
        let n = stream.read(&mut chunk).ok()?;
        if n == 0 {
            return None;
        }
        body.extend_from_slice(&chunk[..n]);
    }
    body.truncate(len);
    let path = pdecode(target.split('?').next().unwrap_or(""));
    Some(Request { method, path, headers, body })
}

struct Reply {
    status: u16,
    headers: Vec<(&'static str, String)>,
    body: Vec<u8>,
}

impl Reply {
    fn empty(status: u16) -> Self {
        Self { status, headers: Vec::new(), body: Vec::new() }
    }

    fn xml(status: u16, body: String) -> Self {
        Self {
            status,
            headers: vec![("content-type", "application/xml; charset=utf-8".into())],
            body: body.into_bytes(),
        }
    }
}

fn write_reply(stream: &mut TcpStream, reply: Reply) {
    let mut out = format!("HTTP/1.1 {} X\r\n", reply.status);
    for (k, v) in &reply.headers {
        out.push_str(&format!("{k}: {v}\r\n"));
    }
    out.push_str(&format!("content-length: {}\r\nconnection: close\r\n\r\n", reply.body.len()));
    let _ = stream.write_all(out.as_bytes());
    let _ = stream.write_all(&reply.body);
}

fn serve(mut stream: TcpStream, state: &Mutex<State>) {
    let Some(request) = read_request(&mut stream) else { return };
    let mut state = state.lock().unwrap();
    state.log.push(format!("{} {}", request.method, request.path));

    if let Some(reply) = authenticate(&mut state, &request) {
        return write_reply(&mut stream, reply);
    }
    if let Some(status) = state.fail_next.take() {
        return write_reply(&mut stream, Reply::empty(status));
    }
    if request.method == "PUT" && std::mem::take(&mut state.drop_next_put) {
        let path = normalize(&request.path);
        let half = request.body[..request.body.len() / 2].to_vec();
        state.files.insert(path, half);
        return; // dropping the stream closes the connection unanswered
    }
    let reply = route(&mut state, &request);
    write_reply(&mut stream, reply);
}

// -------------------------------------------------------------------- auth

fn challenge(state: &State, stale: bool) -> Reply {
    let header = match state.auth {
        AuthMode::Digest => format!(
            "Digest realm=\"{REALM}\", qop=\"auth\", algorithm=MD5, nonce=\"{}\", \
             opaque=\"op4que\"{}",
            state.nonce(),
            if stale { ", stale=true" } else { "" }
        ),
        _ => format!("Basic realm=\"{REALM}\", charset=\"UTF-8\""),
    };
    Reply { status: 401, headers: vec![("www-authenticate", header)], body: Vec::new() }
}

fn authenticate(state: &mut State, request: &Request) -> Option<Reply> {
    match state.auth {
        AuthMode::Open => None,
        AuthMode::Basic => {
            let expected = format!("Basic {}", base64(format!("{USER}:{PASSWORD}").as_bytes()));
            (request.header("authorization") != Some(expected.as_str()))
                .then(|| challenge(state, false))
        }
        AuthMode::Digest => {
            // The test admin only speaks Basic; the plugin is only offered Digest.
            let basic = format!("Basic {}", base64(format!("{USER}:{PASSWORD}").as_bytes()));
            if request.header("authorization") == Some(basic.as_str()) {
                return None;
            }
            if std::mem::take(&mut state.stale_nonce_next) {
                state.nonce += 1;
                return Some(challenge(state, true));
            }
            let Some(header) = request.header("authorization") else {
                return Some(challenge(state, false));
            };
            let Some(header) = header.strip_prefix("Digest ") else {
                return Some(challenge(state, false));
            };
            let params = digest_params(header);
            let get = |k: &str| params.get(k).map(String::as_str).unwrap_or_default();
            if get("nonce") != state.nonce() {
                return Some(challenge(state, true));
            }
            let ha1 = md5_hex(format!("{USER}:{REALM}:{PASSWORD}"));
            let ha2 = md5_hex(format!("{}:{}", request.method, get("uri")));
            let expected = md5_hex(format!(
                "{ha1}:{}:{}:{}:{}:{ha2}",
                get("nonce"),
                get("nc"),
                get("cnonce"),
                get("qop")
            ));
            let valid = get("username") == USER
                && get("realm") == REALM
                && get("opaque") == "op4que"
                && get("qop") == "auth"
                && get("algorithm") == "MD5"
                && get("response") == expected
                && request.path == pdecode(get("uri"));
            if !valid {
                return Some(challenge(state, false));
            }
            state.digest_requests += 1;
            None
        }
    }
}

fn digest_params(text: &str) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    let mut rest = text;
    while let Some((key, after)) = rest.split_once('=') {
        let key = key.trim().trim_start_matches(',').trim().to_owned();
        let (value, tail) = if let Some(quoted) = after.strip_prefix('"') {
            let end = quoted.find('"').unwrap();
            (quoted[..end].to_owned(), &quoted[end + 1..])
        } else {
            let end = after.find(',').unwrap_or(after.len());
            (after[..end].trim().to_owned(), &after[end..])
        };
        out.insert(key, value);
        rest = tail;
    }
    out
}

fn md5_hex(text: String) -> String {
    Md5::digest(text.as_bytes()).iter().map(|b| format!("{b:02x}")).collect()
}

pub fn base64(data: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in data.chunks(3) {
        let n = chunk.iter().enumerate().fold(0u32, |n, (i, b)| n | u32::from(*b) << (16 - 8 * i));
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(ALPHABET[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

// ----------------------------------------------------------------- routing

fn normalize(path: &str) -> String {
    format!("/{}", path.split('/').filter(|s| !s.is_empty()).collect::<Vec<_>>().join("/"))
}

fn parent(path: &str) -> String {
    path.rsplit_once('/').map(|(p, _)| p.to_owned()).unwrap_or_default()
}

fn route(state: &mut State, request: &Request) -> Reply {
    let path = normalize(&request.path);
    if path != ROOT && !path.starts_with(&format!("{ROOT}/")) {
        return Reply::empty(404);
    }
    match request.method.as_str() {
        "OPTIONS" => Reply {
            status: 200,
            headers: vec![("allow", "OPTIONS, GET, PUT, DELETE, MKCOL, PROPFIND, MOVE".to_owned())],
            body: Vec::new(),
        },
        "PROPFIND" => propfind(state, request, &path),
        "MKCOL" => {
            if state.dirs.contains(&path) || state.files.contains_key(&path) {
                Reply::empty(405)
            } else if !state.dirs.contains(&parent(&path)) {
                Reply::empty(409)
            } else {
                state.dirs.insert(path);
                Reply::empty(201)
            }
        }
        "PUT" => {
            if !state.dirs.contains(&parent(&path)) {
                return Reply::empty(409);
            }
            if state.dirs.contains(&path) {
                return Reply::empty(405);
            }
            let existed = state.files.insert(path, request.body.clone()).is_some();
            Reply::empty(if existed { 204 } else { 201 })
        }
        "GET" => match state.files.get(&path) {
            Some(body) => Reply { status: 200, headers: Vec::new(), body: body.clone() },
            None => Reply::empty(404),
        },
        "DELETE" => {
            let prefix = format!("{path}/");
            let existed = state.files.remove(&path).is_some() | state.dirs.remove(&path);
            state.files.retain(|k, _| !k.starts_with(&prefix));
            state.dirs.retain(|k| !k.starts_with(&prefix));
            Reply::empty(if existed { 204 } else { 404 })
        }
        "MOVE" => mv(state, request, &path),
        _ => Reply::empty(501),
    }
}

fn mv(state: &mut State, request: &Request, from: &str) -> Reply {
    if !state.move_supported {
        return Reply::empty(405);
    }
    let Some(destination) = request.header("destination") else { return Reply::empty(400) };
    let target = destination
        .strip_prefix("http://")
        .and_then(|rest| rest.split_once('/'))
        .map(|(_, path)| pdecode(&format!("/{path}")))
        .unwrap_or_default();
    let to = normalize(&target);
    let Some(body) = state.files.get(from).cloned() else { return Reply::empty(404) };
    if !state.dirs.contains(&parent(&to)) {
        return Reply::empty(409);
    }
    let exists = state.files.contains_key(&to);
    if exists && request.header("overwrite") == Some("F") {
        return Reply::empty(412);
    }
    state.files.remove(from);
    state.files.insert(to, body);
    Reply::empty(if exists { 204 } else { 201 })
}

fn propfind(state: &State, request: &Request, path: &str) -> Reply {
    let is_dir = state.dirs.contains(path);
    let size = state.files.get(path).map(Vec::len);
    if !is_dir && size.is_none() {
        return Reply::empty(404);
    }
    let p = &state.prefix;
    let entry = |href: &str, size: Option<usize>, dir: bool| {
        let href = if dir { format!("{}/", urlenc(href)) } else { urlenc(href) };
        let kind = if dir { format!("<{p}:collection/>") } else { String::new() };
        let length = size.map(|n| format!("<{p}:getcontentlength>{n}</{p}:getcontentlength>"));
        format!(
            "<{p}:response><{p}:href>{href}</{p}:href><{p}:propstat><{p}:prop>\
             <{p}:getlastmodified>Fri, 20 Nov 2026 12:00:00 GMT</{p}:getlastmodified>\
             <{p}:resourcetype>{kind}</{p}:resourcetype>{}\
             <oc:permissions>RGDNVW</oc:permissions><oc:id>00000123abc</oc:id>\
             </{p}:prop><{p}:status>HTTP/1.1 200 OK</{p}:status></{p}:propstat></{p}:response>",
            length.unwrap_or_default()
        )
    };
    let mut body = entry(path, size, is_dir);
    if is_dir && request.header("depth") == Some("1") {
        let prefix = format!("{path}/");
        for dir in state.dirs.iter().filter(|d| parent(d) == path && d.as_str() != path) {
            body.push_str(&entry(dir, None, true));
        }
        for (file, data) in state.files.iter().filter(|(f, _)| f.starts_with(&prefix)) {
            if !file[prefix.len()..].contains('/') {
                body.push_str(&entry(file, Some(data.len()), false));
            }
        }
    }
    Reply::xml(
        207,
        format!(
            "<?xml version=\"1.0\" encoding=\"utf-8\"?>\
             <{p}:multistatus xmlns:{p}=\"DAV:\" xmlns:oc=\"http://owncloud.org/ns\">{body}\
             </{p}:multistatus>"
        ),
    )
}
