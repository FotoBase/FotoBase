//! Live/Motion photo detection at scan time (TASK-08-017, TDD): fixture
//! Motion Photo JPEG → clip recorded; fixture Apple pair → paired; plain
//! media and unprovable pairs are untouched; a late-arriving MOV pairs on
//! the next scan; unchanged files are not re-detected.

#[path = "../../video/tests/support/live_fixtures.rs"]
mod fx;

use std::fs;
use std::path::Path;
use std::sync::atomic::AtomicBool;

use fotobase_catalog::{Catalog, LiveMedia};
use fotobase_scanner::scan_folder;

fn open(dir: &Path) -> Catalog {
    Catalog::open(dir.join("catalog.db")).unwrap()
}

fn scan(cat: &mut Catalog, root: &Path) {
    scan_folder(cat, root, &AtomicBool::new(false)).unwrap();
}

fn id_of(cat: &Catalog, root: &Path, name: &str) -> i64 {
    cat.conn()
        .query_row(
            "SELECT id FROM files WHERE path = ?1",
            [root.join(name).to_string_lossy().as_ref()],
            |r| r.get(0),
        )
        .unwrap()
}

#[test]
fn motion_photo_is_detected_at_scan_time() {
    let tmp = tempfile::tempdir().unwrap();
    let lib = tmp.path().join("lib");
    fs::create_dir(&lib).unwrap();
    let clip = fx::fake_mp4(200);
    let still = fx::micro_video_jpeg(&clip);
    fs::write(lib.join("MVIMG_1.jpg"), &still).unwrap();
    fs::write(lib.join("plain.jpg"), fx::jpeg_with_garbage(&[7u8; 300])).unwrap();

    let mut cat = open(tmp.path());
    scan(&mut cat, &lib);

    let motion = id_of(&cat, &lib, "MVIMG_1.jpg");
    assert_eq!(
        cat.live_media(motion).unwrap(),
        Some(LiveMedia::Motion {
            offset: (still.len() - clip.len()) as u64,
            len: clip.len() as u64
        })
    );
    let plain = id_of(&cat, &lib, "plain.jpg");
    assert_eq!(cat.live_media(plain).unwrap(), None);

    let kinds: Vec<(String, Option<String>)> =
        cat.list_files(10).unwrap().into_iter().map(|f| (f.filename, f.live_kind)).collect();
    assert!(kinds.contains(&("MVIMG_1.jpg".into(), Some("motion".into()))));
    assert!(kinds.contains(&("plain.jpg".into(), None)));
}

#[test]
fn apple_pair_is_detected_and_the_video_stays_a_plain_video() {
    let tmp = tempfile::tempdir().unwrap();
    let lib = tmp.path().join("lib");
    fs::create_dir(&lib).unwrap();
    fs::write(lib.join("IMG_0001.HEIC"), fx::apple_heic(Some(fx::UUID))).unwrap();
    fs::write(lib.join("IMG_0001.MOV"), fx::apple_mov(Some(fx::UUID))).unwrap();

    let mut cat = open(tmp.path());
    scan(&mut cat, &lib);

    let still = id_of(&cat, &lib, "IMG_0001.HEIC");
    assert_eq!(
        cat.live_media(still).unwrap(),
        Some(LiveMedia::Live {
            video_path: lib.join("IMG_0001.MOV").to_string_lossy().into_owned()
        })
    );
    let mov = id_of(&cat, &lib, "IMG_0001.MOV");
    assert_eq!(cat.live_media(mov).unwrap(), None);
}

#[test]
fn unprovable_apple_pairs_degrade_to_plain_still_plus_video() {
    let tmp = tempfile::tempdir().unwrap();
    let lib = tmp.path().join("lib");
    fs::create_dir(&lib).unwrap();
    let other = "AAAAAAAA-9C4D-4E7A-8B21-0D5E7F9A1C33";
    // Same stem but different identifiers.
    fs::write(lib.join("a.heic"), fx::apple_heic(Some(fx::UUID))).unwrap();
    fs::write(lib.join("a.mov"), fx::apple_mov(Some(other))).unwrap();
    // Same stem, no identifier on the still.
    fs::write(lib.join("b.heic"), fx::apple_heic(None)).unwrap();
    fs::write(lib.join("b.mov"), fx::apple_mov(Some(fx::UUID))).unwrap();
    // Matching identifiers but different stems: not a sibling pair.
    fs::write(lib.join("c.heic"), fx::apple_heic(Some(fx::UUID))).unwrap();
    fs::write(lib.join("d.mov"), fx::apple_mov(Some(fx::UUID))).unwrap();

    let mut cat = open(tmp.path());
    scan(&mut cat, &lib);

    for name in ["a.heic", "b.heic", "c.heic"] {
        assert_eq!(cat.live_media(id_of(&cat, &lib, name)).unwrap(), None, "{name}");
    }
}

#[test]
fn a_mov_arriving_after_the_still_is_paired_on_the_next_scan() {
    let tmp = tempfile::tempdir().unwrap();
    let lib = tmp.path().join("lib");
    fs::create_dir(&lib).unwrap();
    fs::write(lib.join("IMG_9.heic"), fx::apple_heic(Some(fx::UUID))).unwrap();

    let mut cat = open(tmp.path());
    scan(&mut cat, &lib);
    let still = id_of(&cat, &lib, "IMG_9.heic");
    assert_eq!(cat.live_media(still).unwrap(), None);

    fs::write(lib.join("IMG_9.mov"), fx::apple_mov(Some(fx::UUID))).unwrap();
    scan(&mut cat, &lib);
    assert!(matches!(cat.live_media(still).unwrap(), Some(LiveMedia::Live { .. })));
}

#[test]
fn unchanged_files_are_not_reopened_and_changes_are_redetected() {
    let tmp = tempfile::tempdir().unwrap();
    let lib = tmp.path().join("lib");
    fs::create_dir(&lib).unwrap();
    let path = lib.join("m.jpg");
    fs::write(&path, fx::micro_video_jpeg(&fx::fake_mp4(100))).unwrap();

    let mut cat = open(tmp.path());
    scan(&mut cat, &lib);
    let id = id_of(&cat, &lib, "m.jpg");
    assert!(cat.live_media(id).unwrap().is_some());

    // Poison the stored result: an unchanged file must keep it (no re-read).
    cat.conn().execute("DELETE FROM live_media WHERE file_id = ?1", [id]).unwrap();
    scan(&mut cat, &lib);
    assert_eq!(cat.live_media(id).unwrap(), None);

    // Overwrite with a plain JPEG and bump the mtime: re-detected, now plain.
    cat.set_live_media(id, 0.0, Some(&LiveMedia::Motion { offset: 1, len: 1 })).unwrap();
    fs::write(&path, fx::jpeg_with_garbage(&[1u8; 64])).unwrap();
    filetime::set_file_mtime(&path, filetime::FileTime::from_unix_time(1_900_000_000, 0)).unwrap();
    scan(&mut cat, &lib);
    assert_eq!(cat.live_media(id).unwrap(), None);
}
