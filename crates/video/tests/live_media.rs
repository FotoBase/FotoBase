//! Live/Motion Photo detection (TDD, task 08-017 testing criteria): Android
//! Motion Photos (JPEG + MP4 trailer) and Apple Live Photos (still + MOV
//! paired by ContentIdentifier). Plain JPEGs with trailing garbage must never
//! be detected (PLAN §9: malformed media never misfires).

#[path = "support/live_fixtures.rs"]
mod fx;

use std::fs;

use fotobase_video::live::{
    detect_motion_photo, extract_motion_clip, live_pair_matches, still_content_id,
    video_content_id, MotionClip,
};

fn write(dir: &std::path::Path, name: &str, bytes: &[u8]) -> std::path::PathBuf {
    let p = dir.join(name);
    fs::write(&p, bytes).unwrap();
    p
}

#[test]
fn micro_video_motion_photo_is_detected_and_extracted() {
    let tmp = tempfile::tempdir().unwrap();
    let clip = fx::fake_mp4(300);
    let still = fx::micro_video_jpeg(&clip);
    let path = write(tmp.path(), "MVIMG_0001.jpg", &still);

    let found = detect_motion_photo(&path).unwrap().expect("motion photo");
    assert_eq!(
        found,
        MotionClip { offset: (still.len() - clip.len()) as u64, len: clip.len() as u64 }
    );

    let out = tmp.path().join("clip.mp4");
    extract_motion_clip(&path, found, &out).unwrap();
    assert_eq!(fs::read(&out).unwrap(), clip);
}

#[test]
fn container_directory_motion_photo_is_detected() {
    let tmp = tempfile::tempdir().unwrap();
    let clip = fx::fake_mp4(64);
    let path = write(tmp.path(), "PXL_0001.MP.jpg", &fx::container_jpeg(&clip));
    let found = detect_motion_photo(&path).unwrap().expect("motion photo");
    assert_eq!(found.len, clip.len() as u64);
}

#[test]
fn plain_jpeg_with_appended_garbage_is_not_a_motion_photo() {
    let tmp = tempfile::tempdir().unwrap();
    let garbage = noise(512);
    let path = write(tmp.path(), "plain.jpg", &fx::jpeg_with_garbage(&garbage));
    assert_eq!(detect_motion_photo(&path).unwrap(), None);

    // Even a trailing MP4 is not enough without the XMP markers.
    let path = write(tmp.path(), "mp4-tail.jpg", &fx::jpeg_with_garbage(&fx::fake_mp4(64)));
    assert_eq!(detect_motion_photo(&path).unwrap(), None);
}

#[test]
fn motion_markers_with_a_non_mp4_trailer_are_rejected() {
    let tmp = tempfile::tempdir().unwrap();
    let path = write(tmp.path(), "bad.jpg", &fx::micro_video_jpeg_bad_trailer());
    assert_eq!(detect_motion_photo(&path).unwrap(), None);
}

#[test]
fn oversized_or_zero_offsets_are_rejected() {
    let tmp = tempfile::tempdir().unwrap();
    let mut still = fx::micro_video_jpeg(&fx::fake_mp4(8));
    // Truncate: the declared offset now exceeds what the file can hold.
    still.truncate(40);
    let path = write(tmp.path(), "cut.jpg", &still);
    assert_eq!(detect_motion_photo(&path).unwrap(), None);
}

#[test]
fn non_jpeg_and_empty_files_are_not_motion_photos() {
    let tmp = tempfile::tempdir().unwrap();
    assert_eq!(detect_motion_photo(&write(tmp.path(), "e.jpg", b"")).unwrap(), None);
    assert_eq!(detect_motion_photo(&write(tmp.path(), "t.jpg", b"hello world")).unwrap(), None);
}

#[test]
fn apple_pair_with_matching_content_identifier_is_paired() {
    let tmp = tempfile::tempdir().unwrap();
    let heic = write(tmp.path(), "IMG_0001.HEIC", &fx::apple_heic(Some(fx::UUID)));
    let mov = write(tmp.path(), "IMG_0001.MOV", &fx::apple_mov(Some(fx::UUID)));

    assert_eq!(still_content_id(&heic).unwrap().as_deref(), Some(fx::UUID));
    assert_eq!(video_content_id(&mov).unwrap().as_deref(), Some(fx::UUID));
    assert!(live_pair_matches(&heic, &mov).unwrap());
}

#[test]
fn apple_pair_with_different_or_missing_identifier_is_not_paired() {
    let tmp = tempfile::tempdir().unwrap();
    let other = "AAAAAAAA-9C4D-4E7A-8B21-0D5E7F9A1C33";
    let heic = write(tmp.path(), "IMG_0002.HEIC", &fx::apple_heic(Some(fx::UUID)));
    let mov_other = write(tmp.path(), "a.MOV", &fx::apple_mov(Some(other)));
    let mov_none = write(tmp.path(), "b.MOV", &fx::apple_mov(None));
    let heic_none = write(tmp.path(), "IMG_0003.HEIC", &fx::apple_heic(None));
    let mov_ok = write(tmp.path(), "c.MOV", &fx::apple_mov(Some(fx::UUID)));

    assert!(!live_pair_matches(&heic, &mov_other).unwrap());
    assert!(!live_pair_matches(&heic, &mov_none).unwrap());
    assert!(!live_pair_matches(&heic_none, &mov_ok).unwrap());
    assert_eq!(still_content_id(&heic_none).unwrap(), None);
    assert_eq!(video_content_id(&mov_none).unwrap(), None);
}

#[test]
fn garbage_inputs_never_panic() {
    let tmp = tempfile::tempdir().unwrap();
    let junk = write(tmp.path(), "junk.mov", &noise(4096));
    assert_eq!(video_content_id(&junk).unwrap(), None);
    let junk_still = write(tmp.path(), "junk.heic", b"Apple iOS\0\0\x01MM\xff\xff\xff");
    assert_eq!(still_content_id(&junk_still).unwrap(), None);
}

fn noise(n: usize) -> Vec<u8> {
    (0..n).map(|i| (i * 31 % 251) as u8).collect()
}
