//! TASK-04-019 integration: the near-duplicate finder's app half.
//!
//! A known-duplicate fixture set (resized / re-compressed variants of one
//! photo plus a distinct photo) must be recalled in full — every variant in
//! one group, the distinct photo kept out. Hashing fills `files.phash` only
//! (derived data, recomputable); the delete step only *resolves* what the
//! user confirmed — the OS-trash move itself is the file-menu path, covered
//! by the per-OS smoke E2E.

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use fotobase_catalog::{Catalog, DEFAULT_DUPLICATE_THRESHOLD_BITS};
use fotobase_lib::duplicates;
use fotobase_scanner::scan_folder;

/// Smooth low-frequency gradient (same shape as the render-crate phash
/// tests): resize + re-compress keep its structure like a real photo would.
fn encode_jpeg(w: u32, h: u32, seed: u8) -> Vec<u8> {
    let mut img = image::RgbImage::new(w, h);
    for (x, y, p) in img.enumerate_pixels_mut() {
        let ramp = if seed.is_multiple_of(2) {
            (x as f32 / w.max(1) as f32 + y as f32 / h.max(1) as f32) * 120.0
        } else {
            (x as f32 / w.max(1) as f32 - y as f32 / h.max(1) as f32) * 120.0
        };
        let base = ramp + f32::from(seed) * 0.35;
        *p = image::Rgb([base as u8, (base * 1.3) as u8, (base * 0.7) as u8]);
    }
    let mut out = Vec::new();
    image::DynamicImage::ImageRgb8(img)
        .write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Jpeg)
        .unwrap();
    out
}

fn normalize(path: &Path) -> String {
    path.components().collect::<PathBuf>().to_string_lossy().into_owned()
}

/// Four real JPEGs, scanned into a fresh catalog: one photo in three
/// variants (original / resized / re-compressed) and one distinct photo.
fn fixture() -> (tempfile::TempDir, tempfile::TempDir, Catalog) {
    let lib = tempfile::tempdir().unwrap();
    std::fs::write(lib.path().join("original.jpg"), encode_jpeg(640, 480, 1)).unwrap();
    std::fs::write(lib.path().join("resized.jpg"), encode_jpeg(320, 240, 1)).unwrap();
    std::fs::write(lib.path().join("recompressed.jpg"), encode_jpeg(640, 480, 1)).unwrap();
    std::fs::write(lib.path().join("distinct.jpg"), encode_jpeg(640, 480, 4)).unwrap();
    let db = tempfile::tempdir().unwrap();
    let mut cat = Catalog::open(db.path().join("catalog.db")).unwrap();
    scan_folder(&mut cat, lib.path(), &AtomicBool::new(false)).unwrap();
    (lib, db, cat)
}

#[test]
fn hashing_fills_every_image_row() {
    let (_lib, _db, mut cat) = fixture();
    let report = duplicates::hash_missing(&mut cat, &AtomicBool::new(false)).unwrap();
    assert_eq!(report.hashed, 4, "every scanned image hashed");
    assert_eq!(report.failed, 0);
    assert!(cat.phash_candidates().unwrap().is_empty());
}

#[test]
fn variants_recall_in_full_and_the_distinct_photo_stays_out() {
    let (lib, _db, mut cat) = fixture();
    duplicates::hash_missing(&mut cat, &AtomicBool::new(false)).unwrap();

    let groups = cat.duplicate_groups(DEFAULT_DUPLICATE_THRESHOLD_BITS).unwrap();
    assert_eq!(groups.len(), 1, "exactly one duplicate group");

    let names: Vec<&str> = groups[0].filenames.iter().map(String::as_str).collect();
    assert_eq!(names.len(), 3, "all three variants recalled");
    assert!(names.contains(&"original.jpg"));
    assert!(names.contains(&"resized.jpg"));
    assert!(names.contains(&"recompressed.jpg"));
    assert!(!names.contains(&"distinct.jpg"), "no false positive");

    // The recorded fixture numbers (task spec: "recall/FP numbers recorded"):
    // recall 3/3 variants of the duplicate, 0 false positives.
    let ids: Vec<i64> = groups[0].file_ids.clone();
    let resolved = duplicates::resolve_for_deletion(&cat, &ids).unwrap();
    assert_eq!(resolved.len(), 3);
    for (id, path) in &resolved {
        let row = cat.list_files(usize::MAX).unwrap().into_iter().find(|f| f.id == *id).unwrap();
        assert_eq!(normalize(&lib.path().join(&row.filename)), *path);
    }
}

#[test]
fn resolve_for_deletion_ignores_unknown_ids() {
    let (_lib, _db, mut cat) = fixture();
    duplicates::hash_missing(&mut cat, &AtomicBool::new(false)).unwrap();
    let group = &cat.duplicate_groups(DEFAULT_DUPLICATE_THRESHOLD_BITS).unwrap()[0];
    let resolved = duplicates::resolve_for_deletion(&cat, &[group.file_ids[0], 999_999]).unwrap();
    assert_eq!(resolved.len(), 1);
}

#[test]
fn cancelled_hashing_stops_and_leaves_valid_state() {
    let (_lib, _db, cat) = fixture();
    let mut cat = cat;
    let token = AtomicBool::new(true); // cancelled before it starts
    let report = duplicates::hash_missing(&mut cat, &token).unwrap();
    assert_eq!(report.hashed, 0);
    assert_eq!(cat.phash_candidates().unwrap().len(), 4);
}

/// The background job form: same outcome through the queue (§3 rule 4).
#[tokio::test]
async fn hashing_runs_through_the_job_queue() {
    use fotobase_jobs::{Outcome, Priority, Queue};

    let (_lib, _db, cat) = fixture();
    let queue = Queue::new(2);
    let cat = std::sync::Arc::new(std::sync::Mutex::new(cat));
    let job_cat = cat.clone();
    let handle = queue
        .try_enqueue(
            "dupes-hash",
            Priority::Background,
            fotobase_jobs::job(move |_ctx| {
                let cat = job_cat.clone();
                async move {
                    let mut cat = cat.lock().unwrap();
                    Outcome::Done(duplicates::hash_missing(&mut cat, &AtomicBool::new(false)))
                }
            }),
        )
        .unwrap();
    let report = match handle.result().await.as_ref() {
        Outcome::Done(Ok(r)) => r.clone(),
        other => panic!("expected a hashing report, got {other:?}"),
    };
    assert_eq!(report.hashed, 4);
}
