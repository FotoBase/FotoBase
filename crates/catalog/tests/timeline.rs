//! TASK-08-018: full-screen timeline view (PLAN §2 §B). A live, read-only
//! catalog query: one group per calendar day, oldest first, narrowed by the
//! grid's filters. Nothing is stored and no sidecar is touched.

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
    fn add(&self, name: &str, taken_at: Option<&str>, mtime: f64, media: &str) -> i64 {
        let path = self._dir.path().join(name);
        self.cat
            .conn()
            .execute(
                "INSERT INTO files (folder_id, path, filename, size, mtime, taken_at, media_type)
                 VALUES (?1, ?2, ?3, 10, ?4, ?5, ?6)",
                rusqlite::params![
                    self.folder,
                    path.to_str().unwrap(),
                    name,
                    mtime,
                    taken_at,
                    media
                ],
            )
            .unwrap();
        self.cat.conn().last_insert_rowid()
    }
}

fn dates(cat: &Catalog, q: &FileQuery) -> Vec<(String, i64)> {
    cat.timeline_groups(q).unwrap().into_iter().map(|g| (g.date, g.count)).collect()
}

#[test]
fn three_years_group_by_day_in_chronological_order() {
    let fx = fixture();
    // Inserted out of order on purpose; mixed EXIF-colon and ISO shapes.
    fx.add("c.jpg", Some("2024-03-09T10:00:00"), 0.0, "image");
    fx.add("a1.jpg", Some("2022:01:15 08:00:00"), 0.0, "image");
    fx.add("a2.jpg", Some("2022-01-15T18:30:00"), 0.0, "image");
    fx.add("b.jpg", Some("2023-07-01T12:00:00"), 0.0, "image");

    assert_eq!(
        dates(&fx.cat, &FileQuery::default()),
        vec![
            ("2022-01-15".to_string(), 2),
            ("2023-07-01".to_string(), 1),
            ("2024-03-09".to_string(), 1),
        ]
    );
}

#[test]
fn group_representative_is_the_earliest_photo_of_the_day() {
    let fx = fixture();
    let _late = fx.add("late.jpg", Some("2023-05-05T20:00:00"), 0.0, "image");
    let early = fx.add("early.jpg", Some("2023-05-05T07:00:00"), 0.0, "image");

    let groups = fx.cat.timeline_groups(&FileQuery::default()).unwrap();
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].first_id, early);
}

#[test]
fn undated_photo_falls_back_to_its_mtime_day() {
    let fx = fixture();
    // 2024-10-20 09:00:00 UTC.
    fx.add("m.jpg", None, 1_729_414_800.0, "image");

    assert_eq!(dates(&fx.cat, &FileQuery::default()), vec![("2024-10-20".to_string(), 1)]);
}

#[test]
fn grid_filters_narrow_the_timeline() {
    let fx = fixture();
    fx.add("old.jpg", Some("2021-02-02T10:00:00"), 0.0, "image");
    fx.add("clip.mp4", Some("2023-02-02T10:00:00"), 0.0, "video");
    fx.add("new.jpg", Some("2024-02-02T10:00:00"), 0.0, "image");

    let since = FileQuery { taken_since: Some("2023-01-01".into()), ..FileQuery::default() };
    assert_eq!(
        dates(&fx.cat, &since),
        vec![("2023-02-02".to_string(), 1), ("2024-02-02".to_string(), 1)]
    );

    let videos = FileQuery { videos_only: true, ..FileQuery::default() };
    assert_eq!(dates(&fx.cat, &videos), vec![("2023-02-02".to_string(), 1)]);

    let nowhere = FileQuery { folders: Some(vec![]), ..FileQuery::default() };
    assert!(dates(&fx.cat, &nowhere).is_empty());
}

#[test]
fn hidden_photos_never_appear() {
    let fx = fixture();
    fx.add("shown.jpg", Some("2023-04-04T10:00:00"), 0.0, "image");
    let hidden = fx.add("hidden.jpg", Some("2022-04-04T10:00:00"), 0.0, "image");
    fx.cat.conn().execute("UPDATE files SET hidden = 1 WHERE id = ?1", [hidden]).unwrap();

    assert_eq!(dates(&fx.cat, &FileQuery::default()), vec![("2023-04-04".to_string(), 1)]);
}
