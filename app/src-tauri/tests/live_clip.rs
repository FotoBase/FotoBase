//! TASK-08-017: the viewer's Live/Motion clip source. A Motion Photo's clip is
//! extracted into the cache's `live/` folder; an Apple Live Photo plays its
//! paired MOV in place; plain photos have no clip. Originals are untouched.

#[path = "../../../crates/video/tests/support/live_fixtures.rs"]
mod fx;

use fotobase_catalog::{Catalog, LiveMedia};
use fotobase_lib::live_clip::{resolve_live_clip, LiveClip};

fn catalog_with(tmp: &std::path::Path, name: &str, bytes: &[u8]) -> (Catalog, i64, String) {
    let mut cat = Catalog::open(tmp.join("catalog.db")).unwrap();
    let path = tmp.join(name);
    std::fs::write(&path, bytes).unwrap();
    let id = cat.register_file(&path, "image").unwrap();
    (cat, id, path.to_string_lossy().into_owned())
}

#[test]
fn motion_clip_is_extracted_into_the_cache_and_reused() {
    let tmp = tempfile::tempdir().unwrap();
    let clip = fx::fake_mp4(150);
    let still = fx::micro_video_jpeg(&clip);
    let (mut cat, id, path) = catalog_with(tmp.path(), "m.jpg", &still);
    cat.set_live_media(
        id,
        0.0,
        Some(&LiveMedia::Motion {
            offset: (still.len() - clip.len()) as u64,
            len: clip.len() as u64,
        }),
    )
    .unwrap();
    let cache = tmp.path().join("cache");

    let got = resolve_live_clip(&cat, &cache, id).unwrap().expect("clip");
    assert_eq!(got.kind, "motion");
    assert!(got.path.starts_with(cache.join("live").to_str().unwrap()));
    assert_eq!(std::fs::read(&got.path).unwrap(), clip);
    // Second call reuses the extraction; the original is byte-identical.
    assert_eq!(resolve_live_clip(&cat, &cache, id).unwrap().unwrap(), got);
    assert_eq!(std::fs::read(path).unwrap(), still);
}

#[test]
fn live_photo_plays_its_paired_mov_in_place() {
    let tmp = tempfile::tempdir().unwrap();
    let (mut cat, id, _) = catalog_with(tmp.path(), "a.heic", &fx::apple_heic(Some(fx::UUID)));
    let mov = tmp.path().join("a.mov");
    std::fs::write(&mov, fx::apple_mov(Some(fx::UUID))).unwrap();
    let mov_str = mov.to_string_lossy().into_owned();
    cat.set_live_media(id, 0.0, Some(&LiveMedia::Live { video_path: mov_str.clone() })).unwrap();

    let got = resolve_live_clip(&cat, &tmp.path().join("cache"), id).unwrap();
    assert_eq!(got, Some(LiveClip { path: mov_str, kind: "live" }));

    // The MOV vanished: degrade to the still, no error.
    std::fs::remove_file(&mov).unwrap();
    assert_eq!(resolve_live_clip(&cat, &tmp.path().join("cache"), id).unwrap(), None);
}

#[test]
fn plain_photos_and_unknown_ids_have_no_clip() {
    let tmp = tempfile::tempdir().unwrap();
    let (cat, id, _) = catalog_with(tmp.path(), "p.jpg", &fx::jpeg_with_garbage(&[1; 32]));
    let cache = tmp.path().join("cache");
    assert_eq!(resolve_live_clip(&cat, &cache, id).unwrap(), None);
    assert_eq!(resolve_live_clip(&cat, &cache, 9999).unwrap(), None);
}
