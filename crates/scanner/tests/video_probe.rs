//! Video-probe integration (TASK-02-003, TDD): scanning a folder with mixed
//! videos populates duration/codec/rotation rows; corrupt videos mark the
//! file, never crash the scan; unchanged videos are not re-probed.

use std::fs;
use std::path::Path;
use std::sync::atomic::AtomicBool;

use fotobase_catalog::{Catalog, VideoMeta};
use fotobase_scanner::{
    refresh_video_metadata, scan_folder_with_probes, ScanOptions, VideoError, VideoProbe,
};
use fotobase_video::VideoMeta as ProbeMeta;

/// Deterministic fake prober: matches on file name. `ok` files probe to
/// fixture values; anything else (listed in `fails` or unknown) fails, like a
/// real prober would on a corrupt file.
struct FakeProbe {
    ok: std::collections::HashMap<String, ProbeMeta>,
    fails: Vec<String>,
}

fn file_name(path: &Path) -> String {
    path.file_name().unwrap_or_default().to_string_lossy().into_owned()
}

impl VideoProbe for FakeProbe {
    fn probe(&self, path: &Path) -> Result<ProbeMeta, VideoError> {
        let name = file_name(path);
        if self.fails.contains(&name) {
            return Err(VideoError::NoVideoStream);
        }
        self.ok.get(&name).cloned().ok_or(VideoError::NoVideoStream)
    }
}

fn meta(duration: f64, codec: &str, profile: Option<&str>, rot: u32) -> ProbeMeta {
    ProbeMeta {
        duration_secs: duration,
        codec: codec.to_owned(),
        profile: profile.map(str::to_owned),
        rotation_deg: rot,
    }
}

fn open(dir: &Path) -> Catalog {
    Catalog::open(dir.join("catalog.db")).unwrap()
}

fn seed_video_file(cat: &mut Catalog, filename: &str, mtime: f64) -> i64 {
    let folder = cat.seed_folder(".").unwrap();
    cat.conn()
        .execute(
            "INSERT INTO files(folder_id, path, filename, size, mtime, media_type)
             VALUES (?1, ?2, ?2, 10, ?3, 'video')",
            rusqlite::params![folder, filename, mtime],
        )
        .unwrap();
    cat.conn().query_row("SELECT last_insert_rowid()", [], |r| r.get(0)).unwrap()
}

fn file_id(cat: &Catalog, path: &str) -> i64 {
    cat.conn()
        .query_row(
            "SELECT id FROM files WHERE filename = ?1",
            [Path::new(path).file_name().unwrap().to_string_lossy().as_ref()],
            |r| r.get(0),
        )
        .unwrap()
}

#[test]
fn scan_populates_video_metadata_rows() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    fs::write(root.join("a.mp4"), vec![0; 10]).unwrap();
    fs::write(root.join("b.mov"), vec![0; 10]).unwrap();
    fs::write(root.join("c.jpg"), vec![0; 10]).unwrap();

    let mut cat = open(root);
    let probe = FakeProbe {
        ok: [
            ("a.mp4", meta(6.033_333, "h264", Some("High"), 0)),
            ("b.mov", meta(3.2, "hevc", Some("Main 10"), 270)),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_owned(), v))
        .collect(),
        fails: vec![],
    };
    let report = scan_folder_with_probes(
        &mut cat,
        root,
        &AtomicBool::new(false),
        &ScanOptions::default(),
        &|_| {},
        &probe,
    )
    .unwrap();
    assert_eq!(report.added, 3);

    let id_a = file_id(&cat, "a.mp4");
    let id_b = file_id(&cat, "b.mov");
    assert_eq!(
        cat.video_metadata(id_a).unwrap(),
        Some(VideoMeta {
            duration_secs: 6.033_333,
            codec: "h264".into(),
            profile: Some("High".into()),
            rotation_deg: 0
        })
    );
    assert_eq!(
        cat.video_metadata(id_b).unwrap(),
        Some(VideoMeta {
            duration_secs: 3.2,
            codec: "hevc".into(),
            profile: Some("Main 10".into()),
            rotation_deg: 270
        })
    );
    // The image has no video metadata and is not in the probe set.
    let id_c = file_id(&cat, "c.jpg");
    assert_eq!(cat.video_metadata(id_c).unwrap(), None);
    assert_eq!(cat.videos_needing_probe(10).unwrap(), vec![]);
}

#[test]
fn corrupt_video_is_marked_not_fatal() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    fs::write(root.join("good.mp4"), vec![0; 10]).unwrap();
    fs::write(root.join("corrupt.mp4"), vec![0; 10]).unwrap();

    let mut cat = open(root);
    let probe = FakeProbe {
        ok: [("good.mp4", meta(1.0, "h264", None, 0))]
            .into_iter()
            .map(|(k, v)| (k.to_owned(), v))
            .collect(),
        fails: vec!["corrupt.mp4".into()],
    };
    // The scan itself must succeed despite the corrupt file (PLAN §9).
    let report = scan_folder_with_probes(
        &mut cat,
        root,
        &AtomicBool::new(false),
        &ScanOptions::default(),
        &|_| {},
        &probe,
    )
    .unwrap();
    assert_eq!(report.added, 2);

    let id_good = file_id(&cat, "good.mp4");
    let id_bad = file_id(&cat, "corrupt.mp4");
    assert!(cat.video_metadata(id_good).unwrap().is_some());
    assert_eq!(cat.video_metadata(id_bad).unwrap(), None);
    assert_eq!(cat.video_probe_ok(id_bad).unwrap(), Some(0), "corrupt file is marked");
}

#[test]
fn unchanged_videos_are_not_reprobed_but_changed_ones_are() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    fs::write(root.join("v.mp4"), vec![0; 10]).unwrap();

    let mut cat = open(root);
    let probe = FakeProbe {
        ok: [("v.mp4", meta(2.0, "vp9", None, 0))]
            .into_iter()
            .map(|(k, v)| (k.to_owned(), v))
            .collect(),
        fails: vec![],
    };
    scan_folder_with_probes(
        &mut cat,
        root,
        &AtomicBool::new(false),
        &ScanOptions::default(),
        &|_| {},
        &probe,
    )
    .unwrap();
    let id = file_id(&cat, "v.mp4");
    let mtime = cat
        .conn()
        .query_row("SELECT mtime FROM files WHERE id = ?1", [id], |r| r.get::<_, f64>(0))
        .unwrap();
    assert_eq!(cat.video_probe_mtime(id).unwrap(), Some(mtime));

    // Touch the file on disk: next scan re-probes and refreshes metadata.
    let new_mtime = mtime + 100.0;
    filetime::set_file_mtime(
        root.join("v.mp4"),
        filetime::FileTime::from_unix_time(new_mtime as i64, 0),
    )
    .unwrap();
    scan_folder_with_probes(
        &mut cat,
        root,
        &AtomicBool::new(false),
        &ScanOptions::default(),
        &|_| {},
        &probe,
    )
    .unwrap();
    assert_eq!(cat.videos_needing_probe(10).unwrap(), vec![], "touched video was re-probed");
    let db_mtime: f64 =
        cat.conn().query_row("SELECT mtime FROM files WHERE id = ?1", [id], |r| r.get(0)).unwrap();
    assert_eq!(cat.video_probe_mtime(id).unwrap(), Some(db_mtime));
}

#[test]
fn refresh_video_metadata_backfills_and_batches() {
    let tmp = tempfile::tempdir().unwrap();
    let mut cat = open(tmp.path());
    for name in ["1.mp4", "2.mp4", "3.mp4"] {
        seed_video_file(&mut cat, name, 1.0);
    }
    let probe = FakeProbe {
        ok: (1..=3).map(|i| (format!("{i}.mp4"), meta(i as f64, "h264", None, 0))).collect(),
        fails: vec![],
    };
    // Batch of 2: first pass probes 2, second pass probes the last one, then 0.
    assert_eq!(refresh_video_metadata(&mut cat, 2, &AtomicBool::new(false), &probe).unwrap(), 2);
    assert_eq!(refresh_video_metadata(&mut cat, 2, &AtomicBool::new(false), &probe).unwrap(), 1);
    assert_eq!(
        refresh_video_metadata(&mut cat, 2, &AtomicBool::new(false), &probe).unwrap(),
        0,
        "0 = done"
    );
    let id = file_id(&cat, "2.mp4");
    assert_eq!(cat.video_metadata(id).unwrap().unwrap().duration_secs, 2.0);
}

#[test]
fn refresh_marks_failures_and_still_finishes() {
    let tmp = tempfile::tempdir().unwrap();
    let mut cat = open(tmp.path());
    seed_video_file(&mut cat, "ok.mp4", 1.0);
    seed_video_file(&mut cat, "bad.mp4", 1.0);
    let probe = FakeProbe {
        ok: [("ok.mp4".to_owned(), meta(1.0, "h264", None, 0))].into_iter().collect(),
        fails: vec!["bad.mp4".into()],
    };
    // The failing probe must not abort the pass; both videos end marked.
    assert_eq!(refresh_video_metadata(&mut cat, 10, &AtomicBool::new(false), &probe).unwrap(), 1);
    assert_eq!(cat.video_probe_ok(file_id(&cat, "bad.mp4")).unwrap(), Some(0));
    assert_eq!(cat.videos_needing_probe(10).unwrap(), vec![]);
}

#[test]
fn cancel_stops_probing() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    for name in ["1.mp4", "2.mp4"] {
        fs::write(root.join(name), vec![0; 10]).unwrap();
    }
    let mut cat = open(root);
    let probe = FakeProbe { ok: std::collections::HashMap::new(), fails: vec![] };
    let cancel = AtomicBool::new(true);
    assert_eq!(refresh_video_metadata(&mut cat, 10, &cancel, &probe).unwrap(), 0);
}
