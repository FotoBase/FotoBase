//! Video metadata columns in the `metadata` table (task 02-003, TDD).
//!
//! Migration V3 adds the ffprobe-derived columns; the catalog API stores a
//! successful probe or marks a failed one (corrupt file — never fatal,
//! PLAN §9). `video_probe_mtime` records the file mtime at probe time so a
//! later scan re-probes only changed videos.

use fotobase_catalog::{Catalog, VideoMeta};
use tempfile::tempdir;

fn open(dir: &std::path::Path) -> Catalog {
    Catalog::open(dir.join("catalog.db")).unwrap()
}

fn seed_video_file(cat: &mut Catalog, path: &str, mtime: f64) -> i64 {
    let folder = cat.seed_folder(".").unwrap();
    cat.conn()
        .execute(
            "INSERT INTO files(folder_id, path, filename, size, mtime, media_type)
             VALUES (?1, ?2, 'clip.mp4', 100, ?3, 'video')",
            rusqlite::params![folder, path, mtime],
        )
        .unwrap();
    cat.conn().query_row("SELECT last_insert_rowid()", [], |r| r.get(0)).unwrap()
}

#[test]
fn migration_v3_present() {
    let dir = tempdir().unwrap();
    let cat = open(dir.path());
    assert_eq!(cat.schema_version().unwrap(), 5);
    let cols: Vec<String> = {
        let mut stmt = cat
            .conn()
            .prepare("SELECT name FROM pragma_table_info('metadata') WHERE name LIKE 'video_%'")
            .unwrap();
        stmt.query_map([], |r| r.get(0)).unwrap().collect::<Result<_, _>>().unwrap()
    };
    for col in [
        "video_duration",
        "video_codec",
        "video_profile",
        "video_rotation",
        "video_probe_ok",
        "video_probe_mtime",
    ] {
        assert!(cols.iter().any(|c| c == col), "missing column {col}");
    }
}

#[test]
fn set_and_read_video_metadata() {
    let dir = tempdir().unwrap();
    let mut cat = open(dir.path());
    let id = seed_video_file(&mut cat, "clip.mp4", 100.0);

    cat.set_video_metadata(
        id,
        &VideoMeta {
            duration_secs: 6.033_333,
            codec: "h264".into(),
            profile: Some("High".into()),
            rotation_deg: 270,
        },
        100.0,
    )
    .unwrap();

    let meta = cat.video_metadata(id).unwrap().unwrap();
    assert_eq!(meta.duration_secs, 6.033_333);
    assert_eq!(meta.codec, "h264");
    assert_eq!(meta.profile.as_deref(), Some("High"));
    assert_eq!(meta.rotation_deg, 270);
    assert_eq!(cat.video_probe_ok(id).unwrap(), Some(1));
    assert_eq!(cat.video_probe_mtime(id).unwrap(), Some(100.0));

    // Older schemas had no profile: re-probing without one clears it.
    cat.set_video_metadata(
        id,
        &VideoMeta { duration_secs: 1.0, codec: "vp9".into(), profile: None, rotation_deg: 0 },
        200.0,
    )
    .unwrap();
    let meta = cat.video_metadata(id).unwrap().unwrap();
    assert_eq!(meta.profile, None);
    assert_eq!(cat.video_probe_mtime(id).unwrap(), Some(200.0));
}

#[test]
fn mark_probe_failed_is_not_fatal_and_reprobes_changed_files() {
    let dir = tempdir().unwrap();
    let mut cat = open(dir.path());
    let id = seed_video_file(&mut cat, "corrupt.mp4", 5.0);

    cat.mark_video_probe_failed(id, 5.0).unwrap();
    assert_eq!(cat.video_probe_ok(id).unwrap(), Some(0));
    assert_eq!(cat.video_metadata(id).unwrap(), None);

    // A failed probe is not retried while the file is unchanged (probe mtime
    // matches) — a permanently corrupt file is never hammered on every scan.
    // A *changed* mtime puts the file back in the re-probe set.
    assert_eq!(cat.videos_needing_probe(10).unwrap(), vec![]);
    cat.conn().execute("UPDATE files SET mtime = 9.0 WHERE id = ?1", [id]).unwrap();
    assert_eq!(cat.videos_needing_probe(10).unwrap(), vec![(id, "corrupt.mp4".to_owned(), 9.0)]);
    cat.set_video_metadata(
        id,
        &VideoMeta { duration_secs: 2.0, codec: "h264".into(), profile: None, rotation_deg: 0 },
        9.0,
    )
    .unwrap();
    assert_eq!(cat.videos_needing_probe(10).unwrap(), vec![]);
}

#[test]
fn images_never_enter_the_probe_set() {
    let dir = tempdir().unwrap();
    let mut cat = open(dir.path());
    let folder = cat.seed_folder(".").unwrap();
    cat.conn()
        .execute(
            "INSERT INTO files(folder_id, path, filename, size, mtime, media_type)
             VALUES (?1, 'a.jpg', 'a.jpg', 1, 1.0, 'image')",
            [&folder],
        )
        .unwrap();
    assert_eq!(cat.videos_needing_probe(10).unwrap(), vec![]);
}
