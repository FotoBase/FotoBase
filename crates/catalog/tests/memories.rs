//! TASK-07-008: "On this day" memories (PLAN §2 2026 additions). A live,
//! read-only catalog query: photos whose capture month/day equals today's,
//! grouped by year (newest first), hidden photos and empty years excluded,
//! and the grid's filters AND-combined.

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
    fn add(&self, name: &str, taken_at: Option<&str>, mtime: f64) -> i64 {
        let path = self._dir.path().join(name);
        self.cat
            .conn()
            .execute(
                "INSERT INTO files (folder_id, path, filename, size, mtime, taken_at, media_type)
                 VALUES (?1, ?2, ?3, 10, ?4, ?5, 'image')",
                rusqlite::params![self.folder, path.to_str().unwrap(), name, mtime, taken_at],
            )
            .unwrap();
        self.cat.conn().last_insert_rowid()
    }
}

fn years(cat: &Catalog, q: &FileQuery, today: &str) -> Vec<(i32, Vec<i64>)> {
    cat.memories(q, today)
        .unwrap()
        .into_iter()
        .map(|y| (y.year, y.files.iter().map(|f| f.id).collect()))
        .collect()
}

#[test]
fn groups_same_day_photos_by_year_newest_first() {
    let fx = fixture();
    let a = fx.add("a.jpg", Some("2023-10-20T09:00:00"), 0.0);
    let b = fx.add("b.jpg", Some("2024-10-20T09:00:00"), 0.0);
    let _other_day = fx.add("c.jpg", Some("2024-10-21T09:00:00"), 0.0);

    assert_eq!(
        years(&fx.cat, &FileQuery::default(), "2024-10-20"),
        vec![(2024, vec![b]), (2023, vec![a])]
    );
}

#[test]
fn exif_colon_dates_match_and_undated_photos_never_do() {
    let fx = fixture();
    let exif = fx.add("exif.jpg", Some("2021:10:20 08:15:00"), 0.0);
    // No capture date: the mtime fallback must not invent a memory.
    let _undated = fx.add("undated.jpg", None, 1_729_414_800.0);
    let _garbage = fx.add("garbage.jpg", Some("sometime in october"), 0.0);

    assert_eq!(years(&fx.cat, &FileQuery::default(), "2024-10-20"), vec![(2021, vec![exif])]);
}

#[test]
fn photos_from_after_today_are_not_memories() {
    let fx = fixture();
    let _future = fx.add("future.jpg", Some("2031-10-20T09:00:00"), 0.0);
    assert!(years(&fx.cat, &FileQuery::default(), "2024-10-20").is_empty());
}

#[test]
fn within_a_year_photos_are_chronological() {
    let fx = fixture();
    let late = fx.add("a-late.jpg", Some("2022-10-20T18:00:00"), 0.0);
    let early = fx.add("z-early.jpg", Some("2022-10-20T07:00:00"), 0.0);
    assert_eq!(
        years(&fx.cat, &FileQuery::default(), "2024-10-20"),
        vec![(2022, vec![early, late])]
    );
}

#[test]
fn hidden_photos_are_excluded_and_empty_years_skipped() {
    let fx = fixture();
    let visible = fx.add("v.jpg", Some("2023-10-20T09:00:00"), 0.0);
    let hidden = fx.add("h.jpg", Some("2022-10-20T09:00:00"), 0.0);
    fx.cat.conn().execute("UPDATE files SET hidden = 1 WHERE id = ?1", [hidden]).unwrap();

    // 2022 holds only a hidden photo → the year disappears, not an empty group.
    assert_eq!(years(&fx.cat, &FileQuery::default(), "2024-10-20"), vec![(2023, vec![visible])]);
}

#[test]
fn no_memories_is_an_empty_list() {
    let fx = fixture();
    let _ = fx.add("x.jpg", Some("2023-05-01T09:00:00"), 0.0);
    assert!(fx.cat.memories(&FileQuery::default(), "2024-10-20").unwrap().is_empty());
}

#[test]
fn grid_filters_narrow_the_memories() {
    let fx = fixture();
    let in_folder = fx.add("needle.jpg", Some("2023-10-20T09:00:00"), 0.0);
    let _ = fx.add("also.jpg", Some("2022-10-20T09:00:00"), 0.0);
    let q = FileQuery { text: Some("needle".into()), ..FileQuery::default() };
    assert_eq!(years(&fx.cat, &q, "2024-10-20"), vec![(2023, vec![in_folder])]);
    // A filter matching nothing short-circuits to no memories.
    let none = FileQuery { folders: Some(vec![]), ..FileQuery::default() };
    assert!(years(&fx.cat, &none, "2024-10-20").is_empty());
}

#[test]
fn malformed_today_is_rejected() {
    let fx = fixture();
    assert!(fx.cat.memories(&FileQuery::default(), "20241020").is_err());
    assert!(fx.cat.memories(&FileQuery::default(), "2024-13-40").is_err());
}

#[test]
fn memories_never_modify_the_catalog() {
    let fx = fixture();
    fx.add("a.jpg", Some("2023-10-20T09:00:00"), 0.0);
    let before = fx.cat.conn().total_changes();
    let _ = fx.cat.memories(&FileQuery::default(), "2024-10-20").unwrap();
    assert_eq!(fx.cat.conn().total_changes(), before);
}
