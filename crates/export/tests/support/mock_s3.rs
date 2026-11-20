//! In-process S3 server for the plugin tests: the subset of the S3 REST API
//! the plugin uses (bucket create, object put/head/get/list, multipart
//! create/part/list-parts/list-uploads/complete/abort), path-style. It checks
//! that requests are presigned for the configured access key but does not
//! verify signatures — real signing is covered by the MinIO run.
#![allow(dead_code)]

use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread;
use std::time::Duration;

use md5::{Digest, Md5};

pub const ACCESS_KEY: &str = "mock-access";
pub const SECRET_KEY: &str = "mock-secret";

pub struct Upload {
    pub key: String,
    pub parts: BTreeMap<u16, Vec<u8>>,
}

#[derive(Default)]
pub struct State {
    pub buckets: BTreeMap<String, BTreeMap<String, Vec<u8>>>,
    pub uploads: BTreeMap<String, (String, Upload)>,
    /// Keys per `ListObjectsV2` page.
    pub page_size: Option<usize>,
    /// `METHOD path?query` of every request, in arrival order.
    pub log: Vec<String>,
    /// Answer the next request with this status instead of serving it.
    pub fail_next: Option<u16>,
    next_upload: usize,
}

pub struct MockS3 {
    addr: String,
    state: Arc<Mutex<State>>,
    stop: Arc<AtomicBool>,
}

impl MockS3 {
    pub fn start() -> Self {
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

    pub fn url(&self) -> String {
        format!("http://{}", self.addr)
    }

    pub fn state(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap()
    }
}

impl Drop for MockS3 {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        let _ = TcpStream::connect(&self.addr);
    }
}

struct Request {
    method: String,
    path: String,
    query: Vec<(String, String)>,
    body: Vec<u8>,
}

impl Request {
    fn q(&self, name: &str) -> Option<&str> {
        self.query.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_str())
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
    let len: usize = lines
        .filter_map(|l| l.split_once(':'))
        .find(|(k, _)| k.trim().eq_ignore_ascii_case("content-length"))
        .and_then(|(_, v)| v.trim().parse().ok())
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
    let (path, query) = target.split_once('?').unwrap_or((&target, ""));
    let query = query
        .split('&')
        .filter(|p| !p.is_empty())
        .map(|p| {
            let (k, v) = p.split_once('=').unwrap_or((p, ""));
            (pdecode(k), pdecode(v))
        })
        .collect();
    Some(Request { method, path: pdecode(path), query, body })
}

struct Reply {
    status: u16,
    headers: Vec<(&'static str, String)>,
    body: Vec<u8>,
}

impl Reply {
    fn xml(status: u16, body: String) -> Self {
        Self {
            status,
            headers: vec![("content-type", "application/xml".into())],
            body: body.into_bytes(),
        }
    }

    fn empty(status: u16) -> Self {
        Self { status, headers: Vec::new(), body: Vec::new() }
    }

    fn error(status: u16, code: &str, message: &str) -> Self {
        Self::xml(status, format!("<Error><Code>{code}</Code><Message>{message}</Message></Error>"))
    }
}

fn write_reply(stream: &mut TcpStream, head: bool, content_length: Option<usize>, reply: Reply) {
    let mut out = format!("HTTP/1.1 {} X\r\n", reply.status);
    for (k, v) in &reply.headers {
        out.push_str(&format!("{k}: {v}\r\n"));
    }
    out.push_str(&format!(
        "content-length: {}\r\nconnection: close\r\n\r\n",
        content_length.unwrap_or(reply.body.len())
    ));
    let _ = stream.write_all(out.as_bytes());
    if !head {
        let _ = stream.write_all(&reply.body);
    }
}

fn serve(mut stream: TcpStream, state: &Mutex<State>) {
    let Some(request) = read_request(&mut stream) else { return };
    let mut state = state.lock().unwrap();
    let query: Vec<String> = request
        .query
        .iter()
        .filter(|(k, _)| !k.starts_with("X-Amz-"))
        .map(|(k, v)| if v.is_empty() { k.clone() } else { format!("{k}={v}") })
        .collect();
    let logged = if query.is_empty() {
        format!("{} {}", request.method, request.path)
    } else {
        format!("{} {}?{}", request.method, request.path, query.join("&"))
    };
    state.log.push(logged);
    let head = request.method == "HEAD";

    if let Some(status) = state.fail_next.take() {
        return write_reply(&mut stream, head, None, Reply::error(status, "InternalError", "boom"));
    }
    let credential = request.q("X-Amz-Credential").unwrap_or_default();
    if credential.split('/').next() != Some(ACCESS_KEY) {
        let code = if credential.is_empty() { "AccessDenied" } else { "InvalidAccessKeyId" };
        return write_reply(&mut stream, head, None, Reply::error(403, code, "bad key"));
    }

    let (reply, object_size) = route(&mut state, &request);
    write_reply(&mut stream, head, object_size, reply);
}

fn route(state: &mut State, request: &Request) -> (Reply, Option<usize>) {
    let trimmed = request.path.trim_start_matches('/');
    let (bucket, key) = trimmed.split_once('/').unwrap_or((trimmed, ""));
    let method = request.method.as_str();

    if method == "PUT" && key.is_empty() {
        state.buckets.entry(bucket.to_owned()).or_default();
        return (Reply::empty(200), None);
    }
    if !state.buckets.contains_key(bucket) {
        return (Reply::error(404, "NoSuchBucket", "no such bucket"), None);
    }

    let upload_id = request.q("uploadId").map(str::to_owned);
    match (method, key.is_empty(), upload_id) {
        ("GET", true, _) if request.q("uploads").is_some() => (list_uploads(state, request), None),
        ("GET", true, _) => (list_objects(state, bucket, request), None),
        ("POST", false, None) if request.q("uploads").is_some() => {
            state.next_upload += 1;
            let id = format!("upload-{}", state.next_upload);
            state.uploads.insert(
                id.clone(),
                (bucket.to_owned(), Upload { key: key.to_owned(), parts: BTreeMap::new() }),
            );
            let body = format!(
                "<InitiateMultipartUploadResult xmlns=\"http://s3.amazonaws.com/doc/2006-03-01/\">\
                 <Bucket>{bucket}</Bucket><Key>{key}</Key><UploadId>{id}</UploadId>\
                 </InitiateMultipartUploadResult>"
            );
            (Reply::xml(200, body), None)
        }
        ("PUT", false, Some(id)) => {
            let Some(number) = request.q("partNumber").and_then(|n| n.parse::<u16>().ok()) else {
                return (Reply::error(400, "InvalidArgument", "part number"), None);
            };
            let Some((_, upload)) = state.uploads.get_mut(&id) else {
                return (Reply::error(404, "NoSuchUpload", "gone"), None);
            };
            let etag = format!("\"{}\"", hex(&Md5::digest(&request.body)));
            upload.parts.insert(number, request.body.clone());
            (Reply { status: 200, headers: vec![("etag", etag)], body: Vec::new() }, None)
        }
        ("GET", false, Some(id)) => {
            let Some((_, upload)) = state.uploads.get(&id) else {
                return (Reply::error(404, "NoSuchUpload", "gone"), None);
            };
            let parts: String = upload
                .parts
                .iter()
                .map(|(n, data)| {
                    format!(
                        "<Part><PartNumber>{n}</PartNumber><LastModified>2026-01-01T00:00:00.000Z</LastModified>\
                         <ETag>\"{}\"</ETag><Size>{}</Size></Part>",
                        hex(&Md5::digest(data)),
                        data.len()
                    )
                })
                .collect();
            let body = format!(
                "<ListPartsResult xmlns=\"http://s3.amazonaws.com/doc/2006-03-01/\">\
                 <Bucket>{bucket}</Bucket><Key>{key}</Key><UploadId>{id}</UploadId>\
                 <MaxParts>1000</MaxParts><IsTruncated>false</IsTruncated>{parts}</ListPartsResult>"
            );
            (Reply::xml(200, body), None)
        }
        ("POST", false, Some(id)) => {
            let Some((_, upload)) = state.uploads.get(&id) else {
                return (Reply::error(404, "NoSuchUpload", "gone"), None);
            };
            let body = String::from_utf8_lossy(&request.body).into_owned();
            let numbers: Vec<u16> = body
                .split("<PartNumber>")
                .skip(1)
                .map(|s| s.split('<').next().unwrap().parse().unwrap())
                .collect();
            let mut object = Vec::new();
            for (i, n) in numbers.iter().enumerate() {
                let Some(data) = upload.parts.get(n) else {
                    return (Reply::error(400, "InvalidPart", "missing part"), None);
                };
                if i + 1 < numbers.len() && data.len() < 5 * 1024 * 1024 {
                    return (Reply::error(400, "EntityTooSmall", "part too small"), None);
                }
                object.extend_from_slice(data);
            }
            let etag = format!("\"{}-{}\"", hex(&Md5::digest(&object)), numbers.len());
            let upload_key = upload.key.clone();
            state.buckets.get_mut(bucket).unwrap().insert(upload_key, object);
            state.uploads.remove(&id);
            let body = format!(
                "<CompleteMultipartUploadResult xmlns=\"http://s3.amazonaws.com/doc/2006-03-01/\">\
                 <Bucket>{bucket}</Bucket><Key>{key}</Key><ETag>{etag}</ETag>\
                 </CompleteMultipartUploadResult>"
            );
            (Reply::xml(200, body), None)
        }
        ("DELETE", false, Some(id)) => {
            state.uploads.remove(&id);
            (Reply::empty(204), None)
        }
        ("PUT", false, None) => {
            state.buckets.get_mut(bucket).unwrap().insert(key.to_owned(), request.body.clone());
            (Reply::empty(200), None)
        }
        ("HEAD", false, None) => match state.buckets[bucket].get(key) {
            Some(data) => (Reply::empty(200), Some(data.len())),
            None => (Reply::empty(404), Some(0)),
        },
        ("GET", false, None) => match state.buckets[bucket].get(key) {
            Some(data) => (Reply { status: 200, headers: Vec::new(), body: data.clone() }, None),
            None => (Reply::error(404, "NoSuchKey", "missing"), None),
        },
        _ => (Reply::error(400, "NotImplemented", "mock does not implement this"), None),
    }
}

fn list_uploads(state: &State, request: &Request) -> Reply {
    let prefix = request.q("prefix").unwrap_or_default();
    let uploads: String = state
        .uploads
        .iter()
        .filter(|(_, (_, u))| u.key.starts_with(prefix))
        .map(|(id, (_, u))| {
            format!(
                "<Upload><Key>{}</Key><UploadId>{id}</UploadId>\
                 <Initiated>2026-01-01T00:00:00.000Z</Initiated></Upload>",
                urlenc(&u.key)
            )
        })
        .collect();
    Reply::xml(
        200,
        format!(
            "<ListMultipartUploadsResult xmlns=\"http://s3.amazonaws.com/doc/2006-03-01/\">\
             <EncodingType>url</EncodingType><IsTruncated>false</IsTruncated>{uploads}\
             </ListMultipartUploadsResult>"
        ),
    )
}

fn list_objects(state: &State, bucket: &str, request: &Request) -> Reply {
    let prefix = request.q("prefix").unwrap_or_default();
    let max = request.q("max-keys").and_then(|m| m.parse().ok()).unwrap_or(1000);
    let page = state.page_size.unwrap_or(1000).min(max);
    let after = request.q("continuation-token").unwrap_or_default();
    let matching: Vec<(&String, &Vec<u8>)> = state.buckets[bucket]
        .iter()
        .filter(|(k, _)| k.starts_with(prefix) && k.as_str() > after)
        .collect();
    let truncated = matching.len() > page;
    let shown = &matching[..matching.len().min(page)];
    let contents: String = shown
        .iter()
        .map(|(k, v)| {
            format!(
                "<Contents><Key>{}</Key><Size>{}</Size><ETag>\"x\"</ETag></Contents>",
                urlenc(k),
                v.len()
            )
        })
        .collect();
    let next = match (truncated, shown.last()) {
        (true, Some((k, _))) => format!("<NextContinuationToken>{k}</NextContinuationToken>"),
        _ => String::new(),
    };
    Reply::xml(
        200,
        format!(
            "<ListBucketResult xmlns=\"http://s3.amazonaws.com/doc/2006-03-01/\">\
             <Name>{bucket}</Name><KeyCount>{}</KeyCount><MaxKeys>{page}</MaxKeys>\
             <EncodingType>url</EncodingType><IsTruncated>{truncated}</IsTruncated>{next}{contents}\
             </ListBucketResult>",
            shown.len()
        ),
    )
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
