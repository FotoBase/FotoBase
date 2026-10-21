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

/// A well-formed cache key: exactly 32 lowercase hex chars.
fn is_cache_key(s: &str) -> bool {
    s.len() == 32 && s.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// Resolve a cache key against the thumbnail cache directory.
///
/// Hit → the cached JPEG bytes. Miss (or an unreadable entry) → the
/// placeholder PNG. Both are displayable images: the UI never sees an error.
pub fn resolve_thumb(hash: &str, cache_dir: &Path) -> ThumbReply {
    if !is_cache_key(hash) {
        return placeholder();
    }
    match std::fs::read(cache_dir.join(format!("{hash}.jpg"))) {
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
/// now). Jobs are keyed by the hash, so repeated requests for the same
/// thumbnail coalesce onto one running render. Returns the job handles —
/// empty when the hash is unknown (nothing cached *and* nothing catalogued
/// carries it yet).
pub fn enqueue_missing_thumbs(
    hash: &str,
    catalog: &Catalog,
    cache_dir: &Path,
    queue: &Queue<ThumbOutcome>,
) -> Vec<JobHandle<ThumbOutcome>> {
    if !is_cache_key(hash) {
        return Vec::new();
    }
    let paths: Vec<PathBuf> = match catalog.paths_for_hash(hash) {
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
                    hash,
                    Priority::Visible,
                    thumbnail_job(src, cache_dir, Default::default()),
                )
                // Full queue: the bounded queue protects memory (§3 rule 4);
                // dropping the request is the correct degradation — the next
                // miss re-triggers the render.
                .ok()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    #[test]
    fn placeholder_is_a_nonempty_png() {
        assert!(super::PLACEHOLDER_PNG.starts_with(&[0x89, b'P', b'N', b'G']));
        assert!(!super::PLACEHOLDER_PNG.is_empty());
    }
}
