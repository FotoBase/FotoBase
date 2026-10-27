//! `video_info` (TASK-02-007): the player payload for one video — path plus
//! the ffprobe result from the `metadata` table. Non-video files and unknown
//! ids are `None`; videos whose probe has not succeeded come back with
//! `meta: None` so the player can still fall back to `stream://`.

use fotobase_catalog::Catalog;

fn open() -> Catalog {
    let dir = tempfile::tempdir().unwrap();
    let cat = Catalog::open(dir.path().join("catalog.db")).unwrap();
    // Keep the connection valid for the rest of the test: leak the tempdir.
    std::mem::forget(dir);
    cat
}

fn insert_video(cat: &mut Catalog, path: &str) -> i64 {
    cat.seed_folder("/photos").unwrap();
    cat.conn()
        .execute(
            "INSERT INTO files(folder_id, path, filename, size, mtime, media_type)
             VALUES (1, ?1, 'clip.mov', 1, 0.0, 'video')",
            rusqlite::params![path],
        )
        .unwrap();
    cat.conn().last_insert_rowid()
}

#[test]
fn video_info_returns_none_for_images_and_unknown_ids() {
    let mut cat = open();
    cat.seed_folder("/photos").unwrap();
    cat.conn()
        .execute(
            "INSERT INTO files(folder_id, path, filename, size, mtime, media_type)
             VALUES (1, '/photos/a.jpg', 'a.jpg', 1, 0.0, 'image')",
            [],
        )
        .unwrap();
    assert_eq!(cat.video_info(1).unwrap(), None);
    assert_eq!(cat.video_info(999).unwrap(), None);
}

#[test]
fn video_info_carries_path_and_probe_result() {
    let mut cat = open();
    let id = insert_video(&mut cat, "/photos/clip.mov");
    cat.set_video_metadata(
        id,
        &fotobase_catalog::VideoMeta {
            duration_secs: 12.5,
            codec: "h264".into(),
            profile: Some("High".into()),
            rotation_deg: 90,
        },
        0.0,
    )
    .unwrap();
    let info = cat.video_info(id).unwrap().expect("video row");
    assert_eq!(info.path, "/photos/clip.mov");
    let meta = info.meta.expect("probed");
    assert_eq!(meta.duration_secs, 12.5);
    assert_eq!(meta.codec, "h264");
    assert_eq!(meta.rotation_deg, 90);
}

#[test]
fn unprobed_video_still_has_a_path_but_no_meta() {
    let mut cat = open();
    let id = insert_video(&mut cat, "/photos/clip.mov");
    let info = cat.video_info(id).unwrap().expect("video row");
    assert_eq!(info.path, "/photos/clip.mov");
    assert_eq!(info.meta, None);
}

#[test]
fn failed_probe_has_no_meta() {
    let mut cat = open();
    let id = insert_video(&mut cat, "/photos/clip.mov");
    cat.mark_video_probe_failed(id, 0.0).unwrap();
    let info = cat.video_info(id).unwrap().expect("video row");
    assert_eq!(info.meta, None);
}
