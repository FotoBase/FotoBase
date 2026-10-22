//! `thumb://` URI scheme resolution (TASK-01-011, PLAN §3 rule 3).
//!
//! Images never travel over IPC: the grid sets `<img src="thumb://<hash>">`
//! and the WebView fetches the bytes from the on-disk thumbnail cache through
//! the custom scheme handler. This module holds the pure, app-free core —
//! URI parsing, cache resolution, and the cache-miss → render-job trigger —
//! so it can be tested without booting a Tauri runtime. The Tauri
//! registration in `lib.rs` is a thin adapter.
//!
//! Hashes are the render crate's cache keys: the first 16 bytes of the
//! SHA-256 of the source content, lowercase hex (32 chars). Only
//! well-formed keys are honored; anything else degrades to the placeholder,
//! never an error dialog.

use std::path::{Path, PathBuf};

use fotobase_catalog::Catalog;
use fotobase_jobs::{JobHandle, Priority, Queue};
use fotobase_render::{thumbnail_job, RenderError, Thumbnail};
use tauri::http::Uri;

/// Embedded placeholder shown while a thumbnail is not (yet) cached: an
/// 8×8 mid-gray PNG. A cache miss returns this instead of an error so the
/// UI never pops an error dialog.
pub const PLACEHOLDER_PNG: &[u8] = include_bytes!("placeholder.png");

/// Queue outcome type for thumbnail render jobs.
pub type ThumbOutcome = Result<Thumbnail, RenderError>;

/// A resolved `thumb://` response: an HTTP-shaped reply the Tauri scheme
/// handler turns into a response without any IPC involvement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThumbReply {
    pub status: u16,
    pub content_type: &'static str,
    pub body: Vec<u8>,
}

/// Extract the cache-key hash from a scheme request URI.
///
/// Accepts both WebView spellings of a custom scheme:
/// - `thumb://<hash>` (macOS/Linux: the hash is the authority),
/// - `http://thumb.localhost/<hash>` (Windows WebView2),
/// - `thumb://localhost/<hash>` (defensive).
///
/// A query string is ignored. Only exactly 32 lowercase hex chars pass —
/// this is also the path-traversal guard, since anything else can never
/// name a cache entry.
pub fn hash_from_uri(uri: &Uri) -> Option<String> {
    let host = uri.host().unwrap_or("");
    let path_hash = uri.path().trim_start_matches('/');
    let candidate = if path_hash.is_empty() {
        // Direct form `thumb://<hash>`: the hash is the authority.
        host
    } else if host == "thumb.localhost" || host == "localhost" {
        // Windows form `http://thumb.localhost/<hash>`; only the first path
        // segment counts.
        path_hash.split('/').next().unwrap_or("")
    } else {
        // Any other authority is not a fotobase scheme spelling.
        return None;
    };
    is_cache_key(candidate).then(|| candidate.to_string())
}

/// A well-formed cache key: exactly 32 lowercase hex chars. Public so the
/// IPC commands can apply the same guard before touching the queue.
pub fn is_cache_key(s: &str) -> bool {
    s.len() == 32 && s.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// A parsed `thumb://` request: which cache entry and which size variant.
///
/// HiDPI (TASK-01-024): `?s=2` selects the 2x entry (`<hash>@2x.jpg`, 1024px
/// max edge) so Retina/4K displays stay sharp; no query means the legacy 1x
/// entry (`<hash>.jpg`, 512px). Unknown scale values are rejected — the UI
/// gets the placeholder instead of silently mis-sized pixels.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThumbRequest {
    pub hash: String,
    pub scale: u32,
}

/// Cache-file suffix for a scale variant: 1x keeps the legacy name (existing
/// caches stay valid), 2x is tagged so both sizes coexist.
fn cache_file(hash: &str, scale: u32) -> String {
    match scale {
        1 => format!("{hash}.jpg"),
        2 => format!("{hash}@2x.jpg"),
        _ => unreachable!("request_from_uri only admits 1 and 2"),
    }
}

/// The render-job key for a request: scale 2 gets its own key so repeated
/// 2x misses coalesce separately from 1x renders.
pub fn job_key(req: &ThumbRequest) -> String {
    match req.scale {
        1 => req.hash.clone(),
        2 => format!("{}@2x", req.hash),
        _ => unreachable!("request_from_uri only admits 1 and 2"),
    }
}

/// Parse a scheme request URI into a [`ThumbRequest`]. Accepts both WebView
/// spellings (see [`hash_from_uri`]) plus the optional `?s=2` HiDPI query.
pub fn request_from_uri(uri: &Uri) -> Option<ThumbRequest> {
    let hash = hash_from_uri(uri)?;
    let scale = match uri.query() {
        None | Some("s=1") => 1,
        Some("s=2") => 2,
        Some(_) => return None,
    };
    Some(ThumbRequest { hash, scale })
}

/// Resolve a request against the thumbnail cache directory.
///
/// Hit → the cached JPEG bytes. Miss (or an unreadable entry) → the
/// placeholder PNG. Both are displayable images: the UI never sees an error.
pub fn resolve_thumb(req: &ThumbRequest, cache_dir: &Path) -> ThumbReply {
    if !is_cache_key(&req.hash) {
        return placeholder();
    }
    match std::fs::read(cache_dir.join(cache_file(&req.hash, req.scale))) {
        Ok(bytes) => ThumbReply { status: 200, content_type: "image/jpeg", body: bytes },
        Err(_) => placeholder(),
    }
}

pub fn placeholder() -> ThumbReply {
    ThumbReply { status: 200, content_type: "image/png", body: PLACEHOLDER_PNG.to_vec() }
}

/// Trigger render jobs for a cache miss (TASK-01-011 functional spec).
///
/// Looks the hash up in the catalog and enqueues a [`thumbnail_job`] per
/// source path at [`Priority::Visible`] (the user is looking at it right
/// now). Jobs are keyed by [`job_key`], so repeated requests for the same
/// thumbnail coalesce onto one running render — and 2x requests coalesce
/// separately from 1x (TASK-01-024). Returns the job handles — empty when
/// the hash is unknown (nothing cached *and* nothing catalogued carries it
/// yet).
pub fn enqueue_missing_thumbs(
    req: &ThumbRequest,
    catalog: &Catalog,
    cache_dir: &Path,
    queue: &Queue<ThumbOutcome>,
) -> Vec<JobHandle<ThumbOutcome>> {
    if !is_cache_key(&req.hash) {
        return Vec::new();
    }
    let opts = fotobase_render::ThumbOptions {
        // 2x variant: 1024px max edge keeps a ≤512 CSS-px cell sharp at a
        // device pixel ratio of 2 (Retina / 150%-scaled 4K).
        max_edge: if req.scale == 2 { 1024 } else { 512 },
        cache_tag: (req.scale == 2).then_some("@2x".to_string()),
        ..Default::default()
    };
    let key = job_key(req);
    let paths: Vec<PathBuf> = match catalog.paths_for_hash(&req.hash) {
        Ok(paths) => paths.into_iter().map(PathBuf::from).collect(),
        // A poisoned/closed catalog must not turn into an error dialog; the
        // placeholder already went out, the render can be re-triggered later.
        Err(_) => return Vec::new(),
    };
    paths
        .into_iter()
        .filter_map(|src| {
            let cache_dir = cache_dir.to_path_buf();
            queue
                .try_enqueue(
                    key.as_str(),
                    Priority::Visible,
                    thumbnail_job(src, cache_dir, opts.clone()),
                )
                // Full queue: the bounded queue protects memory (§3 rule 4);
                // dropping the request is the correct degradation — the next
                // miss re-triggers the render.
                .ok()
        })
        .collect()
}

/// "Clear cache files" (TASK-01-022, PLAN §2 General tab): empty the
/// thumbnail cache safely.
///
/// Order matters. The queue is drained first — pending renders cancelled,
/// the in-flight render cancelled and *waited out* — so no render can write
/// a cache entry while (or after) the entries are deleted. Only then are
/// `*.jpg` / `*.jpg.tmp` files removed; anything else in the directory is
/// not ours to touch. Returns the number of removed files. The library and
/// the catalog are never touched: this only deletes rebuildable cache data.
///
/// After a clear the pipeline keeps working: the next `thumb://` miss
/// re-triggers a render job and repopulates the cache.
pub async fn clear_thumbnail_cache(
    cache_dir: &Path,
    queue: &Queue<ThumbOutcome>,
) -> std::io::Result<u64> {
    queue.drain().await;
    let mut removed = 0u64;
    for entry in std::fs::read_dir(cache_dir)? {
        let entry = entry?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.ends_with(".jpg") || name.ends_with(".jpg.tmp") {
            match std::fs::remove_file(entry.path()) {
                Ok(()) => removed += 1,
                // A file that vanished between readdir and unlink (or is
                // locked by a finished-but-still-open writer) is not an
                // error: the goal is an empty cache, and a later clear or a
                // re-render converges there.
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(e),
            }
        }
    }
    Ok(removed)
}

#[cfg(test)]
mod tests {
    #[test]
    fn placeholder_is_a_nonempty_png() {
        assert!(super::PLACEHOLDER_PNG.starts_with(&[0x89, b'P', b'N', b'G']));
        assert!(!super::PLACEHOLDER_PNG.is_empty());
    }

    #[test]
    fn uri_parses_scale_2x_requests() {
        // TASK-01-024 HiDPI: `?s=2` selects the 2x cache entry; no query
        // means the legacy 1x thumb. Unknown scale values are rejected, not
        // silently mis-sized.
        let hash = "a".repeat(32);
        let u1: tauri::http::Uri = format!("thumb://{hash}").parse().unwrap();
        let r1 = super::request_from_uri(&u1).unwrap();
        assert_eq!(r1.hash, hash);
        assert_eq!(r1.scale, 1);

        let u2: tauri::http::Uri = format!("thumb://{hash}?s=2").parse().unwrap();
        let r2 = super::request_from_uri(&u2).unwrap();
        assert_eq!(r2.hash, hash);
        assert_eq!(r2.scale, 2);

        let uw: tauri::http::Uri = format!("http://thumb.localhost/{hash}?s=2").parse().unwrap();
        assert_eq!(super::request_from_uri(&uw).unwrap().scale, 2);

        let ub: tauri::http::Uri = format!("thumb://{hash}?s=9").parse().unwrap();
        assert!(super::request_from_uri(&ub).is_none());
    }

    #[test]
    fn scale2_resolves_the_at2x_cache_entry() {
        let hash = "a".repeat(32);
        let dir = std::env::temp_dir().join(format!("fb-thumb-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(format!("{hash}@2x.jpg")), b"jpeg2x").unwrap();

        let req: tauri::http::Uri = format!("thumb://{hash}?s=2").parse().unwrap();
        let reply = super::resolve_thumb(&super::request_from_uri(&req).unwrap(), &dir);
        assert_eq!(reply.body, b"jpeg2x".to_vec());

        // Misses still degrade to the placeholder, never an error.
        let miss: tauri::http::Uri = format!("thumb://{hash}").parse().unwrap();
        let reply = super::resolve_thumb(&super::request_from_uri(&miss).unwrap(), &dir);
        assert!(reply.body.starts_with(&[0x89, b'P', b'N', b'G']));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn job_key_distinguishes_scale_variants() {
        let hash = "a".repeat(32);
        let req1 = super::ThumbRequest { hash: hash.clone(), scale: 1 };
        let req2 = super::ThumbRequest { hash: hash.clone(), scale: 2 };
        assert_eq!(super::job_key(&req1), hash);
        assert_eq!(super::job_key(&req2), format!("{hash}@2x"));
    }

    #[test]
    fn cache_key_shape_is_strict() {
        let valid = "a".repeat(32);
        assert!(super::is_cache_key(&valid));
        assert!(!super::is_cache_key(&valid.to_uppercase()));
        assert!(!super::is_cache_key("../etc/passwd"));
        assert!(!super::is_cache_key(&"a".repeat(31)));
        assert!(!super::is_cache_key(&"a".repeat(33)));
    }
}
