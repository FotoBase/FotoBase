//! TASK-03-014: library filters (PLAN §2 C "Filter buttons" + "Date slider").
//! Starred, videos-only and a `taken_at` cutoff compile into the one query
//! pipeline and AND-combine with each other, with live search, with the
//! folder selection and with an album view.

use fotobase_catalog::{Catalog, FileQuery};

struct Fixture {
    _dir: tempfile::TempDir,
    cat: Catalog,
    folder: i64,
}

fn fixture() -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let mut cat = Catalog::open(dir.path().join("c.db")).unwrap();
    let folder = cat.seed_folder(dir.path().to_str().unwrap()).unwrap();
    Fixture { _dir: dir, cat, folder }
}

impl Fixture {
    fn add(
        &mut self,
        name: &str,
        media: &str,
        taken_at: Option<&str>,
        mtime: f64,
        rating: Option<u8>,
    ) -> i64 {
        let path = self._dir.path().join(name);
        std::fs::write(&path, b"fake bytes").unwrap();
        let path = path.to_str().unwrap().to_string();
        self.cat
            .conn()
            .execute(
                "INSERT INTO files (folder_id, path, filename, size, mtime, taken_at, media_type)
                 VALUES (?1, ?2, ?3, 10, ?4, ?5, ?6)",
                rusqlite::params![self.folder, path, name, mtime, taken_at, media],
            )
            .unwrap();
        let id = self.cat.conn().last_insert_rowid();
        if let Some(r) = rating {
            self.cat
                .conn()
                .execute(
                    "INSERT INTO metadata (file_id, rating) VALUES (?1, ?2)
                     ON CONFLICT(file_id) DO UPDATE SET rating = excluded.rating",
                    rusqlite::params![id, r],
                )
                .unwrap();
        }
        id
    }

    fn ids(&self, q: &FileQuery) -> Vec<i64> {
        let mut ids: Vec<i64> =
            self.cat.query_files(q, usize::MAX).unwrap().iter().map(|f| f.id).collect();
        ids.sort_unstable();
        ids
    }
}

/// 2025-06-01 00:00 UTC as a unix mtime — the fallback date for undated files.
const MTIME_2025_06: f64 = 1_748_736_000.0;

#[test]
fn starred_video_and_date_cutoff_return_exactly_the_expected_set() {
    let mut f = fixture();
    // star, video, recent
    let hit = f.add("hit.mp4", "video", Some("2026-03-10 12:00:00"), 0.0, Some(5));
    // fails exactly one predicate each
    let _not_starred = f.add("unstarred.mp4", "video", Some("2026-03-10 12:00:00"), 0.0, Some(0));
    let _not_video = f.add("photo.jpg", "image", Some("2026-03-10 12:00:00"), 0.0, Some(5));
    let _too_old = f.add("old.mp4", "video", Some("2020-03-10 12:00:00"), 0.0, Some(5));
    let _unrated = f.add("norating.mp4", "video", Some("2026-03-10 12:00:00"), 0.0, None);

    let q = FileQuery {
        min_rating: Some(5),
        videos_only: true,
        taken_since: Some("2026-01-01".into()),
        ..FileQuery::default()
    };
    assert_eq!(f.ids(&q), vec![hit]);
}

#[test]
fn each_filter_works_alone() {
    let mut f = fixture();
    let starred_photo = f.add("a.jpg", "image", Some("2020-01-01 00:00:00"), 0.0, Some(5));
    let plain_video = f.add("b.mp4", "video", Some("2020-01-01 00:00:00"), 0.0, None);
    let recent_photo = f.add("c.jpg", "image", Some("2026-05-05 00:00:00"), 0.0, Some(0));

    let starred = FileQuery { min_rating: Some(5), ..FileQuery::default() };
    assert_eq!(f.ids(&starred), vec![starred_photo]);
    let videos = FileQuery { videos_only: true, ..FileQuery::default() };
    assert_eq!(f.ids(&videos), vec![plain_video]);
    let recent = FileQuery { taken_since: Some("2026-01-01".into()), ..FileQuery::default() };
    assert_eq!(f.ids(&recent), vec![recent_photo]);
    assert_eq!(f.ids(&FileQuery::default()).len(), 3);
}

#[test]
fn rating_threshold_follows_the_caller() {
    let mut f = fixture();
    let three = f.add("three.jpg", "image", None, 0.0, Some(3));
    let five = f.add("five.jpg", "image", None, 0.0, Some(5));
    // Ratings enabled: any 1–5 counts as starred; star-only mode needs 5.
    assert_eq!(
        f.ids(&FileQuery { min_rating: Some(1), ..FileQuery::default() }),
        vec![three, five]
    );
    assert_eq!(f.ids(&FileQuery { min_rating: Some(5), ..FileQuery::default() }), vec![five]);
}

#[test]
fn cutoff_is_inclusive_and_reads_both_exif_and_iso_stamps() {
    let mut f = fixture();
    let exif = f.add("exif.jpg", "image", Some("2026-01-01 08:00:00"), 0.0, None);
    let iso = f.add("iso.jpg", "image", Some("2026-01-01T08:00:00"), 0.0, None);
    let exif_colons = f.add("colons.jpg", "image", Some("2026:02:03 08:00:00"), 0.0, None);
    let _before = f.add("before.jpg", "image", Some("2025-12-31 23:59:59"), 0.0, None);

    let q = FileQuery { taken_since: Some("2026-01-01".into()), ..FileQuery::default() };
    assert_eq!(f.ids(&q), vec![exif, iso, exif_colons]);
}

#[test]
fn undated_files_fall_back_to_mtime() {
    let mut f = fixture();
    let recent = f.add("recent.jpg", "image", None, MTIME_2025_06, None);
    let _old = f.add("old.jpg", "image", None, 1_000_000.0, None);
    let bad_stamp = f.add("bad.jpg", "image", Some("not a date"), MTIME_2025_06, None);

    let q = FileQuery { taken_since: Some("2025-01-01".into()), ..FileQuery::default() };
    assert_eq!(f.ids(&q), vec![recent, bad_stamp]);
}

#[test]
fn filters_and_combine_with_live_search() {
    let mut f = fixture();
    let beach_star = f.add("beach1.jpg", "image", Some("2026-02-02 00:00:00"), 0.0, Some(5));
    let _beach_plain = f.add("beach2.jpg", "image", Some("2026-02-02 00:00:00"), 0.0, Some(0));
    let _other_star = f.add("city.jpg", "image", Some("2026-02-02 00:00:00"), 0.0, Some(5));
    let beach_video = f.add("beach.mp4", "video", Some("2026-02-02 00:00:00"), 0.0, Some(5));

    let q = FileQuery { text: Some("beach".into()), min_rating: Some(5), ..FileQuery::default() };
    assert_eq!(f.ids(&q), vec![beach_star, beach_video]);
    let q = FileQuery { videos_only: true, ..q };
    assert_eq!(f.ids(&q), vec![beach_video]);
}

#[test]
fn blank_search_text_matches_nothing_like_search_files() {
    let mut f = fixture();
    f.add("a.jpg", "image", None, 0.0, None);
    let q = FileQuery { text: Some("   ".into()), ..FileQuery::default() };
    assert!(f.ids(&q).is_empty());
}

#[test]
fn filters_and_combine_with_folders_and_albums() {
    let mut f = fixture();
    let star = f.add("s.jpg", "image", None, 0.0, None);
    let plain = f.add("p.jpg", "image", None, 0.0, None);
    let album = f.cat.create_album("Trip", "2026-01-01", None, None).unwrap();
    f.cat.add_to_album(album.id, &[star, plain]).unwrap();
    // Membership writes the sidecars and re-mirrors them; rate afterwards.
    f.cat.set_rating(star, 5).unwrap();

    let in_album = FileQuery { album: Some(album.id), min_rating: Some(5), ..FileQuery::default() };
    assert_eq!(f.ids(&in_album), vec![star]);
    let all_in_album = FileQuery { album: Some(album.id), ..FileQuery::default() };
    assert_eq!(f.ids(&all_in_album), vec![star, plain]);

    let in_folder =
        FileQuery { folders: Some(vec![f.folder]), min_rating: Some(5), ..FileQuery::default() };
    assert_eq!(f.ids(&in_folder), vec![star]);
    // An empty folder selection filters everything out, never an unbounded IN ().
    let none = FileQuery { folders: Some(vec![]), ..FileQuery::default() };
    assert!(f.ids(&none).is_empty());
}

#[test]
fn hidden_photos_stay_out_unless_show_hidden() {
    let mut f = fixture();
    let id = f.add("h.jpg", "image", None, 0.0, Some(5));
    f.cat.conn().execute("UPDATE files SET hidden = 1 WHERE id = ?1", [id]).unwrap();
    let q = FileQuery { min_rating: Some(5), ..FileQuery::default() };
    assert!(f.ids(&q).is_empty());
    f.cat.set_show_hidden(true);
    assert_eq!(f.ids(&q), vec![id]);
}

#[test]
fn malformed_cutoff_is_rejected() {
    let mut f = fixture();
    f.add("a.jpg", "image", Some("2026-01-01 00:00:00"), 0.0, None);
    for bad in ["x' OR '1'='1", "2026-1-1", "", "2026-13-01", "2026-01-32"] {
        let q = FileQuery { taken_since: Some(bad.into()), ..FileQuery::default() };
        assert!(f.cat.query_files(&q, 10).is_err(), "{bad:?} must be rejected");
    }
}

#[test]
fn rows_without_text_are_path_ordered_and_carry_rating() {
    let mut f = fixture();
    f.add("b.jpg", "image", None, 0.0, Some(5));
    f.add("a.jpg", "image", None, 0.0, Some(5));
    let rows =
        f.cat.query_files(&FileQuery { min_rating: Some(5), ..FileQuery::default() }, 10).unwrap();
    assert_eq!(
        rows.iter().map(|r| r.filename.as_str()).collect::<Vec<_>>(),
        vec!["a.jpg", "b.jpg"]
    );
    assert!(rows.iter().all(|r| r.rating == 5));
}

/// Perf (TASK-03-014): a filter toggle on a 50k-photo catalog must query well
/// inside a frame-budget-friendly window. Ignored by default (timing); run
/// with `cargo test -p fotobase-catalog --test filters -- --ignored`.
#[test]
#[ignore = "timing-sensitive; run on demand"]
fn filter_query_on_50k_rows_stays_interactive() {
    let dir = tempfile::tempdir().unwrap();
    let mut cat = Catalog::open(dir.path().join("c.db")).unwrap();
    let folder = cat.seed_folder(dir.path().to_str().unwrap()).unwrap();
    let conn = cat.conn();
    conn.execute_batch("BEGIN").unwrap();
    for i in 0..50_000 {
        let media = if i % 10 == 0 { "video" } else { "image" };
        let taken = format!("{}-{:02}-{:02} 12:00:00", 2015 + i % 12, 1 + i % 12, 1 + i % 28);
        conn.execute(
            "INSERT INTO files (folder_id, path, filename, size, mtime, taken_at, media_type)
             VALUES (?1, ?2, ?3, 10, 0.0, ?4, ?5)",
            rusqlite::params![
                folder,
                format!("/p/img{i:05}.jpg"),
                format!("img{i:05}.jpg"),
                taken,
                media
            ],
        )
        .unwrap();
        if i % 7 == 0 {
            let id = conn.last_insert_rowid();
            conn.execute("INSERT INTO metadata (file_id, rating) VALUES (?1, 5)", [id]).unwrap();
        }
    }
    conn.execute_batch("COMMIT").unwrap();

    let q = FileQuery {
        text: Some("img1".into()),
        min_rating: Some(5),
        videos_only: false,
        taken_since: Some("2024-01-01".into()),
        ..FileQuery::default()
    };
    let start = std::time::Instant::now();
    let rows = cat.query_files(&q, usize::MAX).unwrap();
    let elapsed = start.elapsed();
    eprintln!("50k filtered query: {} rows in {elapsed:?}", rows.len());
    assert!(elapsed < std::time::Duration::from_millis(250), "took {elapsed:?}");

    let start = std::time::Instant::now();
    let rows = cat
        .query_files(
            &FileQuery { videos_only: true, min_rating: Some(5), ..FileQuery::default() },
            usize::MAX,
        )
        .unwrap();
    let elapsed = start.elapsed();
    eprintln!("50k starred+video: {} rows in {elapsed:?}", rows.len());
    assert!(elapsed < std::time::Duration::from_millis(250), "took {elapsed:?}");
}
