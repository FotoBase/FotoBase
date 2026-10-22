//! `preview://` URI scheme resolution tests (TASK-01-019, PLAN §3 rule 3).
//!
//! The single-photo viewer serves full-size images the same way the grid
//! serves thumbnails: the WebView fetches the original bytes straight from
//! disk through the custom scheme handler — zero image bytes cross IPC.
//! The URI parsing is shared with `thumb://` (tested there); these tests
//! cover the catalog lookup, the byte serving, and the content-type rules.

use fotobase_catalog::Catalog;
use fotobase_lib::preview::{content_type_for, resolve_preview, PLACEHOLDER_PNG};
use image::ImageEncoder as _;
use tempfile::TempDir;

fn gen_jpeg(dir: &std::path::Path, name: &str) -> std::path::PathBuf {
    std::fs::create_dir_all(dir).unwrap();
    let p = dir.join(name);
    let mut buf = Vec::with_capacity(4 * 4 * 3);
    for y in 0..4u32 {
        for x in 0..4u32 {
            buf.extend_from_slice(&[x as u8 * 40, y as u8 * 40, 128]);
        }
    }
    let mut out = Vec::new();
    {
        let enc = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 90);
        enc.write_image(&buf, 4, 4, image::ExtendedColorType::Rgb8).unwrap();
    }
    std::fs::write(&p, &out).unwrap();
    p
}

/// The content hash the render pipeline writes: first 16 bytes of SHA-256,
/// lowercase hex (32 chars).
fn cache_key(src: &std::path::Path) -> String {
    use sha2::{Digest, Sha256};
    use std::fmt::Write as _;
    let bytes = std::fs::read(src).unwrap();
    Sha256::digest(&bytes)[..16].iter().fold(String::new(), |mut acc, b| {
        let _ = write!(acc, "{b:02x}");
        acc
    })
}

fn catalog_with_hash(dir: &TempDir, hash: &str, path: &str) -> Catalog {
    let mut cat = Catalog::open(dir.path().join("catalog.db")).unwrap();
    let folder = cat.seed_folder("/photos").unwrap();
    cat.conn()
        .execute(
            "INSERT INTO files(folder_id, path, filename, size, mtime, hash, media_type)
             VALUES (?1, ?2, ?3, 1, 0.0, ?4, 'image')",
            rusqlite::params![folder, path, "unused", Some(hash)],
        )
        .unwrap();
    cat
}

#[test]
fn resolve_preview_serves_the_original_bytes_with_the_right_type() {
    let tmp = TempDir::new().unwrap();
    let src = gen_jpeg(&tmp.path().join("photos"), "img.jpg");
    let bytes = std::fs::read(&src).unwrap();
    let cat = catalog_with_hash(&tmp, &cache_key(&src), &src.to_string_lossy());

    let reply = resolve_preview(&cache_key(&src), &cat);
    assert_eq!(reply.status, 200);
    assert_eq!(reply.content_type, "image/jpeg");
    assert_eq!(reply.body, bytes, "the original file's bytes, unmodified");
}

#[test]
fn resolve_preview_for_an_unknown_hash_serves_the_placeholder() {
    let tmp = TempDir::new().unwrap();
    let cat = Catalog::open(tmp.path().join("catalog.db")).unwrap();

    let reply = resolve_preview("e3b0c44298fc1c149afbf4c8996fb924", &cat);
    assert_eq!(reply.content_type, "image/png");
    assert_eq!(reply.body, PLACEHOLDER_PNG);
}

#[test]
fn resolve_preview_rejects_a_malformed_hash() {
    let tmp = TempDir::new().unwrap();
    let src = gen_jpeg(&tmp.path().join("photos"), "img.jpg");
    let cat = catalog_with_hash(&tmp, &cache_key(&src), &src.to_string_lossy());

    let reply = resolve_preview("../../etc/passwd", &cat);
    assert_eq!(reply.content_type, "image/png");
    assert_eq!(reply.body, PLACEHOLDER_PNG);
}

#[test]
fn resolve_preview_handles_a_source_that_vanished_from_disk() {
    let tmp = TempDir::new().unwrap();
    let src = gen_jpeg(&tmp.path().join("photos"), "img.jpg");
    let key = cache_key(&src);
    let cat = catalog_with_hash(&tmp, &key, &src.to_string_lossy());
    std::fs::remove_file(&src).unwrap();

    let reply = resolve_preview(&key, &cat);
    assert_eq!(reply.content_type, "image/png");
    assert_eq!(reply.body, PLACEHOLDER_PNG);
}

#[test]
fn content_type_covers_the_supported_formats() {
    assert_eq!(content_type_for("a.jpg"), "image/jpeg");
    assert_eq!(content_type_for("a.JPEG"), "image/jpeg");
    assert_eq!(content_type_for("a.png"), "image/png");
    assert_eq!(content_type_for("a.webp"), "image/webp");
    assert_eq!(content_type_for("a.gif"), "image/gif");
    assert_eq!(content_type_for("a.bmp"), "image/bmp");
    assert_eq!(content_type_for("a.tif"), "image/tiff");
    assert_eq!(content_type_for("a.tiff"), "image/tiff");
    assert_eq!(content_type_for("a.heic"), "image/heic");
    assert_eq!(content_type_for("a.heif"), "image/heic");
    assert_eq!(content_type_for("a.avif"), "image/avif");
    assert_eq!(content_type_for("a.psd"), "image/vnd.adobe.photoshop");
    assert_eq!(content_type_for("a.unknown"), "application/octet-stream");
    assert_eq!(content_type_for("noextension"), "application/octet-stream");
}
