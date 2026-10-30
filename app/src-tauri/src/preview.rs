//! `preview://` URI scheme resolution (TASK-01-019, PLAN §3 rule 3).
//!
//! The single-photo viewer serves full-size images the same way the grid
//! serves thumbnails: the WebView fetches the original bytes straight from
//! disk through the custom scheme handler — zero image bytes cross IPC.
//! This module holds the pure, app-free core (catalog lookup + file read +
//! content-type mapping) so it can be tested without booting a Tauri
//! runtime; the Tauri registration in `lib.rs` is a thin adapter.
//!
//! URI parsing is shared with `thumb://` (`thumb::hash_from_uri`): the same
//! 32-hex-char content-hash key, the same path-traversal guard. A miss
//! degrades to the placeholder image, never an error dialog.

use std::path::Path;

use fotobase_catalog::Catalog;

/// Embedded placeholder shown when the original cannot be served (unknown
/// hash, vanished file): the same 8×8 mid-gray PNG the grid uses.
pub const PLACEHOLDER_PNG: &[u8] = include_bytes!("placeholder.png");

/// A resolved `preview://` response: an HTTP-shaped reply the Tauri scheme
/// handler turns into a response without any IPC involvement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreviewReply {
    pub status: u16,
    pub content_type: &'static str,
    pub body: Vec<u8>,
}

/// Map a file extension to the MIME type the WebView needs for an `<img>`.
/// Covers the supported formats (PLAN §4 P1); anything else is a generic
/// octet stream — the WebView still renders it if it can sniff the type.
pub fn content_type_for(path: &str) -> &'static str {
    let ext =
        Path::new(path).extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
    match ext.as_str() {
        "jpg" | "jpeg" => "image/jpeg",
        "png" => "image/png",
        "webp" => "image/webp",
        "gif" => "image/gif",
        "bmp" => "image/bmp",
        "tif" | "tiff" => "image/tiff",
        "heic" | "heif" => "image/heic",
        "avif" => "image/avif",
        "psd" => "image/vnd.adobe.photoshop",
        _ => "application/octet-stream",
    }
}

pub fn placeholder() -> PreviewReply {
    PreviewReply { status: 200, content_type: "image/png", body: PLACEHOLDER_PNG.to_vec() }
}

/// Resolve a content hash against the catalog and serve the original file's
/// bytes. Duplicates legitimately share a hash; the first path (deterministic
/// `ORDER BY path`) wins. Unknown hashes, malformed keys, and files that
/// vanished from disk all degrade to the placeholder — the viewer never
/// pops an error dialog.
pub fn resolve_preview(hash: &str, catalog: &Catalog) -> PreviewReply {
    resolve_preview_with_recipe(hash, None, Path::new(""), catalog)
}

/// [`resolve_preview`] with an edit-preview override (TASK-04-010): when
/// `recipe_id` names an edited cache entry (`<cache_dir>/edit/<hash>-r<id>.jpg`),
/// its bytes are served instead of the original's — the edit view's live
/// preview, still fetched by the WebView straight from disk (PLAN §3 rule 3).
/// A missing/corrupt cache entry falls back to the original bytes so the
/// image never blanks out while a render is in flight.
pub fn resolve_preview_with_recipe(
    hash: &str,
    recipe_id: Option<&str>,
    cache_dir: &Path,
    catalog: &Catalog,
) -> PreviewReply {
    if let Some(id) = recipe_id {
        if crate::thumb::is_cache_key(hash)
            && id.len() == 32
            && id.bytes().all(|b| b.is_ascii_hexdigit())
        {
            let edited = cache_dir.join("edit").join(format!("{hash}-r{id}.jpg"));
            if let Ok(bytes) = std::fs::read(&edited) {
                return PreviewReply { status: 200, content_type: "image/jpeg", body: bytes };
            }
        }
    }
    resolve_preview_original(hash, catalog)
}

fn resolve_preview_original(hash: &str, catalog: &Catalog) -> PreviewReply {
    if !crate::thumb::is_cache_key(hash) {
        return placeholder();
    }
    let paths = match catalog.paths_for_hash(hash) {
        Ok(paths) => paths,
        // A poisoned/closed catalog must not turn into an error dialog; the
        // placeholder already went out, the render can be re-triggered later.
        Err(_) => return placeholder(),
    };
    let Some(path) = paths.into_iter().next() else { return placeholder() };
    match std::fs::read(&path) {
        Ok(bytes) => {
            PreviewReply { status: 200, content_type: content_type_for(&path), body: bytes }
        }
        Err(_) => placeholder(),
    }
}
