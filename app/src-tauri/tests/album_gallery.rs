//! TASK-10-01-01: an album becomes a self-contained static site that a
//! plain file server can host — expected files exist, captions render, the
//! output is byte-for-byte deterministic for the same album content, and the
//! site works over HTTP with no backend.

use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};

use fotobase_catalog::Catalog;
use fotobase_lib::album_gallery::{export_album_gallery, AlbumGalleryError, AlbumGalleryRequest};
use sha2::{Digest, Sha256};

fn gen_jpeg(dir: &Path, name: &str, w: u32, h: u32, seed: u8) -> PathBuf {
    std::fs::create_dir_all(dir).unwrap();
    let p = dir.join(name);
    let mut buf = Vec::with_capacity((w * h * 3) as usize);
    for y in 0..h {
        for x in 0..w {
            buf.extend_from_slice(&[(x % 64) as u8 * 4, (y % 32) as u8 * 8, seed]);
        }
    }
    let mut out = Vec::new();
    {
        use image::ImageEncoder as _;
        let enc = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 92);
        enc.write_image(&buf, w, h, image::ExtendedColorType::Rgb8).unwrap();
    }
    std::fs::write(&p, &out).unwrap();
    p
}

/// A catalog with an album of three photos (the middle one captioned) and
/// one photo outside the album.
fn fixture(dir: &Path) -> (Catalog, i64) {
    let mut cat = Catalog::open(dir.join("catalog.db")).unwrap();
    let folder = cat.seed_folder(dir.to_str().unwrap()).unwrap();
    let mut ids = Vec::new();
    for (i, name) in ["a.jpg", "b.jpg", "c.jpg", "outside.jpg"].iter().enumerate() {
        let src = gen_jpeg(&dir.join("src"), name, 200, 120, 40 * i as u8);
        cat.conn()
            .execute(
                "INSERT INTO files (folder_id, path, filename, size, mtime, media_type)
                 VALUES (?1, ?2, ?3, 10, 0.0, 'image')",
                rusqlite::params![folder, src.to_string_lossy(), name],
            )
            .unwrap();
        ids.push(cat.conn().last_insert_rowid());
    }
    cat.set_caption(ids[1], "Sunset <on> the pier").unwrap();
    let album = cat.create_album("Summer 2026", "2026-08-01", None, Some("Holiday")).unwrap();
    cat.add_to_album(album.id, &ids[..3]).unwrap();
    (cat, album.id)
}

fn request() -> AlbumGalleryRequest {
    AlbumGalleryRequest {
        template: "grid".into(),
        title: None,
        image_max_edge: 128,
        thumb_max_edge: 32,
    }
}

fn hashes(root: &Path) -> BTreeMap<String, String> {
    fn walk(root: &Path, dir: &Path, out: &mut BTreeMap<String, String>) {
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                walk(root, &path, out);
            } else {
                let rel = path.strip_prefix(root).unwrap().to_string_lossy().replace('\\', "/");
                out.insert(
                    rel,
                    Sha256::digest(std::fs::read(&path).unwrap())
                        .iter()
                        .map(|b| format!("{b:02x}"))
                        .collect(),
                );
            }
        }
    }
    let mut out = BTreeMap::new();
    walk(root, root, &mut out);
    out
}

/// Minimal static file server: `GET /path` → file below `root`, no
/// backend logic. Serves until the process exits.
fn serve(root: PathBuf) -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let mut stream = stream.unwrap();
            // Drain the whole request head so closing never resets the peer.
            let mut head = Vec::new();
            let mut buf = [0u8; 512];
            while !head.windows(4).any(|w| w == b"\r\n\r\n") {
                let n = stream.read(&mut buf).unwrap();
                if n == 0 {
                    break;
                }
                head.extend_from_slice(&buf[..n]);
            }
            let head = String::from_utf8_lossy(&head).into_owned();
            let target = head.split_whitespace().nth(1).unwrap_or("/");
            let rel = target.trim_start_matches('/');
            let rel = if rel.is_empty() { "index.html" } else { rel };
            let (status, body) = match std::fs::read(root.join(rel)) {
                Ok(body) => ("200 OK", body),
                Err(_) => ("404 Not Found", Vec::new()),
            };
            let _ = write!(
                stream,
                "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            let _ = stream.write_all(&body);
        }
    });
    port
}

fn get(port: u16, path: &str) -> (u16, Vec<u8>) {
    let mut stream = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
    write!(stream, "GET {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n").unwrap();
    let mut raw = Vec::new();
    stream.read_to_end(&mut raw).unwrap();
    let split = raw.windows(4).position(|w| w == b"\r\n\r\n").unwrap();
    let status =
        String::from_utf8_lossy(&raw[..split]).split_whitespace().nth(1).unwrap().parse().unwrap();
    (status, raw[split + 4..].to_vec())
}

#[test]
fn album_exports_a_static_site_with_its_members_only() {
    let tmp = tempfile::tempdir().unwrap();
    let (cat, album) = fixture(tmp.path());
    let dest = tmp.path().join("site");

    let report = export_album_gallery(&cat, album, &dest, &request()).unwrap();
    assert_eq!(report.photos, 3, "outside.jpg is not in the album");

    let files = hashes(&dest);
    for name in ["index.html", "viewer.js", "manifest.json"] {
        assert!(files.contains_key(name), "{name} missing: {files:?}");
    }
    assert_eq!(files.keys().filter(|n| n.starts_with("images/")).count(), 3);
    assert_eq!(files.keys().filter(|n| n.starts_with("thumbs/")).count(), 3);

    let index = std::fs::read_to_string(dest.join("index.html")).unwrap();
    assert!(index.contains("<title>Summer 2026</title>"), "album name is the default title");
    assert!(index.contains("Holiday"), "album description rendered");
    assert!(index.contains("Sunset &lt;on&gt; the pier"), "caption rendered, escaped");
}

#[test]
fn same_album_content_yields_identical_hashes_in_a_fresh_destination() {
    let tmp = tempfile::tempdir().unwrap();
    let (cat, album) = fixture(tmp.path());
    let (one, two) = (tmp.path().join("one"), tmp.path().join("two"));

    export_album_gallery(&cat, album, &one, &request()).unwrap();
    export_album_gallery(&cat, album, &two, &request()).unwrap();

    assert_eq!(hashes(&one), hashes(&two));
}

#[test]
fn site_works_from_a_plain_file_server() {
    let tmp = tempfile::tempdir().unwrap();
    let (cat, album) = fixture(tmp.path());
    let dest = tmp.path().join("site");
    export_album_gallery(&cat, album, &dest, &request()).unwrap();
    let port = serve(dest.clone());

    let (status, index) = get(port, "/");
    assert_eq!(status, 200);
    let index = String::from_utf8(index).unwrap();
    // The viewer navigates over the photos embedded in the page.
    let data = index.split(r#"id="fb-data">"#).nth(1).unwrap().split("</script>").next().unwrap();
    let data: serde_json::Value = serde_json::from_str(data).unwrap();
    let photos = data["photos"].as_array().unwrap();
    assert_eq!(photos.len(), 3);

    for asset in ["/viewer.js", "/style.css", "/lightbox.css"] {
        assert_eq!(get(port, asset).0, 200, "{asset}");
    }
    for photo in photos {
        let src = photo["image"]["src"].as_str().unwrap();
        let (status, bytes) = get(port, &format!("/{src}"));
        assert_eq!(status, 200, "{src}");
        assert!(image::load_from_memory(&bytes).is_ok(), "{src} decodes");
    }
}

#[test]
fn rejects_empty_and_unknown_albums_and_templates() {
    let tmp = tempfile::tempdir().unwrap();
    let (mut cat, album) = fixture(tmp.path());
    let dest = tmp.path().join("site");

    assert!(matches!(
        export_album_gallery(&cat, album + 100, &dest, &request()),
        Err(AlbumGalleryError::UnknownAlbum(_))
    ));
    let empty = cat.create_album("Empty", "2026-08-02", None, None).unwrap();
    assert!(matches!(
        export_album_gallery(&cat, empty.id, &dest, &request()),
        Err(AlbumGalleryError::EmptyAlbum)
    ));
    let mut bad = request();
    bad.template = "nope".into();
    assert!(matches!(
        export_album_gallery(&cat, album, &dest, &bad),
        Err(AlbumGalleryError::UnknownTemplate(_))
    ));
    assert!(!dest.exists(), "nothing written on failure");
}
