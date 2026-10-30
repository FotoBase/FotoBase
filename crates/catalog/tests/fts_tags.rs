//! TASK-03-013: live search covers captions and tags (PLAN §5 `files_fts`,
//! §2 C). Tag edits refresh the FTS row through the same write-through path
//! as every other user-data mutation, so no manual rebuild is needed.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use fotobase_catalog::{sidecar_path, Catalog};
use fotobase_metadata::Sidecar;

fn add_photo(cat: &mut Catalog, dir: &Path, name: &str) -> (i64, PathBuf) {
    let original = dir.join(name);
    std::fs::write(&original, b"fake image bytes").unwrap();
    let folder_id = cat.seed_folder(dir.to_str().unwrap()).unwrap();
    let path = original.to_str().unwrap().to_string();
    cat.conn()
        .execute(
            "INSERT INTO files (folder_id, path, filename, size, mtime, media_type)
             VALUES (?1, ?2, ?3, 10, 0.0, 'image')",
            rusqlite::params![folder_id, path, name],
        )
        .unwrap();
    (cat.conn().last_insert_rowid(), original)
}

fn open(dir: &tempfile::TempDir) -> Catalog {
    Catalog::open(dir.path().join("c.db")).unwrap()
}

#[test]
fn tag_name_matches_with_prefix_and_case_insensitively() {
    let dir = tempfile::tempdir().unwrap();
    let mut cat = open(&dir);
    let (a, _) = add_photo(&mut cat, dir.path(), "a.jpg");
    let (b, _) = add_photo(&mut cat, dir.path(), "b.jpg");
    cat.add_tag(a, "Seaside").unwrap();
    cat.add_tag(b, "mountain").unwrap();

    assert_eq!(cat.search_files("seaside", 10).unwrap(), vec![a]);
    assert_eq!(cat.search_files("SEA", 10).unwrap(), vec![a]);
    assert_eq!(cat.search_files("mount", 10).unwrap(), vec![b]);
    assert!(cat.search_files("desert", 10).unwrap().is_empty());
}

#[test]
fn caption_substring_and_tag_name_both_find_the_photo() {
    let dir = tempfile::tempdir().unwrap();
    let mut cat = open(&dir);
    let (a, _) = add_photo(&mut cat, dir.path(), "a.jpg");
    let (b, _) = add_photo(&mut cat, dir.path(), "b.jpg");
    cat.set_caption(a, "sunset over the bay").unwrap();
    cat.add_tag(b, "bay").unwrap();

    let mut hits = cat.search_files("bay", 10).unwrap();
    hits.sort_unstable();
    assert_eq!(hits, vec![a, b]);
    assert_eq!(cat.search_files("sunset", 10).unwrap(), vec![a]);
}

#[test]
fn nested_tag_segments_are_each_searchable() {
    let dir = tempfile::tempdir().unwrap();
    let mut cat = open(&dir);
    let (id, _) = add_photo(&mut cat, dir.path(), "a.jpg");
    cat.add_tag(id, "Vacation|Italy").unwrap();

    assert_eq!(cat.search_files("vacation", 10).unwrap(), vec![id]);
    assert_eq!(cat.search_files("italy", 10).unwrap(), vec![id]);
}

#[test]
fn tokens_combine_across_filename_caption_and_tags() {
    let dir = tempfile::tempdir().unwrap();
    let mut cat = open(&dir);
    let (a, _) = add_photo(&mut cat, dir.path(), "beach.jpg");
    let (b, _) = add_photo(&mut cat, dir.path(), "other.jpg");
    cat.set_caption(a, "golden hour").unwrap();
    cat.add_tag(a, "holiday").unwrap();
    cat.add_tag(b, "holiday").unwrap();

    assert_eq!(cat.search_files("beach golden holiday", 10).unwrap(), vec![a]);
    let rows = cat.search_files_detailed("holiday golden", None, 10).unwrap();
    assert_eq!(rows.iter().map(|f| f.id).collect::<Vec<_>>(), vec![a]);
}

#[test]
fn adding_a_tag_is_searchable_immediately_without_rebuild() {
    let dir = tempfile::tempdir().unwrap();
    let mut cat = open(&dir);
    let (id, _) = add_photo(&mut cat, dir.path(), "a.jpg");
    assert!(cat.search_files("sea", 10).unwrap().is_empty());

    cat.add_tag(id, "sea").unwrap();

    assert_eq!(cat.search_files("sea", 10).unwrap(), vec![id]);
}

#[test]
fn removing_a_tag_stops_matching_but_keeps_other_tags_and_caption() {
    let dir = tempfile::tempdir().unwrap();
    let mut cat = open(&dir);
    let (id, _) = add_photo(&mut cat, dir.path(), "a.jpg");
    cat.set_caption(id, "sunset").unwrap();
    cat.add_tag(id, "sea").unwrap();
    cat.add_tag(id, "boat").unwrap();

    cat.remove_tag(id, "sea").unwrap();

    assert!(cat.search_files("sea", 10).unwrap().is_empty());
    assert_eq!(cat.search_files("boat", 10).unwrap(), vec![id]);
    assert_eq!(cat.search_files("sunset", 10).unwrap(), vec![id]);
}

#[test]
fn editing_a_caption_keeps_the_tags_searchable() {
    let dir = tempfile::tempdir().unwrap();
    let mut cat = open(&dir);
    let (id, _) = add_photo(&mut cat, dir.path(), "a.jpg");
    cat.add_tag(id, "sea").unwrap();

    cat.set_caption(id, "first").unwrap();
    cat.set_caption(id, "second").unwrap();
    cat.clear_caption(id).unwrap();

    assert_eq!(cat.search_files("sea", 10).unwrap(), vec![id]);
    assert!(cat.search_files("second", 10).unwrap().is_empty());
}

#[test]
fn rename_tag_moves_the_search_hits() {
    let dir = tempfile::tempdir().unwrap();
    let mut cat = open(&dir);
    let (id, _) = add_photo(&mut cat, dir.path(), "a.jpg");
    cat.add_tag(id, "sea").unwrap();

    cat.rename_tag("sea", "ocean").unwrap();

    assert!(cat.search_files("sea", 10).unwrap().is_empty());
    assert_eq!(cat.search_files("ocean", 10).unwrap(), vec![id]);
}

#[test]
fn set_tags_replaces_the_indexed_tags() {
    let dir = tempfile::tempdir().unwrap();
    let mut cat = open(&dir);
    let (id, _) = add_photo(&mut cat, dir.path(), "a.jpg");
    cat.set_tags(id, vec!["red".into(), "green".into()]).unwrap();
    assert_eq!(cat.search_files("green", 10).unwrap(), vec![id]);

    cat.set_tags(id, vec!["blue".into()]).unwrap();

    assert!(cat.search_files("red", 10).unwrap().is_empty());
    assert!(cat.search_files("green", 10).unwrap().is_empty());
    assert_eq!(cat.search_files("blue", 10).unwrap(), vec![id]);
}

#[test]
fn rebuild_from_sidecars_repopulates_tag_search() {
    let dir = tempfile::tempdir().unwrap();
    let mut cat = open(&dir);
    let (id, original) = add_photo(&mut cat, dir.path(), "a.jpg");
    cat.add_tag(id, "sea").unwrap();

    // Wipe the derived mirror and FTS text; the sidecar is the truth.
    cat.conn().execute("DELETE FROM file_tags", []).unwrap();
    cat.conn().execute("DELETE FROM tags", []).unwrap();
    assert!(cat.search_files("sea", 10).unwrap().is_empty());
    cat.rebuild_from_sidecars(true).unwrap();
    assert_eq!(cat.search_files("sea", 10).unwrap(), vec![id]);

    // A tag added by another tool converges into search on rebuild too.
    let sc_path = sidecar_path(original.to_str().unwrap());
    let mut sc = Sidecar::read(&sc_path).unwrap();
    sc.set_tags(vec!["lighthouse".into()]);
    sc.write(&sc_path).unwrap();
    cat.rebuild_from_sidecars(true).unwrap();
    assert_eq!(cat.search_files("lighthouse", 10).unwrap(), vec![id]);
    assert!(cat.search_files("sea", 10).unwrap().is_empty());
}

#[test]
fn deleting_the_file_row_drops_tags_from_search() {
    let dir = tempfile::tempdir().unwrap();
    let mut cat = open(&dir);
    let (id, _) = add_photo(&mut cat, dir.path(), "a.jpg");
    cat.add_tag(id, "sea").unwrap();

    cat.conn().execute("DELETE FROM files WHERE id = ?1", [id]).unwrap();

    assert!(cat.search_files("sea", 10).unwrap().is_empty());
    let rows: i64 =
        cat.conn().query_row("SELECT COUNT(*) FROM files_fts", [], |r| r.get(0)).unwrap();
    assert_eq!(rows, 0);
}

#[test]
fn no_duplicate_fts_rows_after_tag_churn() {
    let dir = tempfile::tempdir().unwrap();
    let mut cat = open(&dir);
    let (id, _) = add_photo(&mut cat, dir.path(), "a.jpg");
    for tag in ["a", "b", "c"] {
        cat.add_tag(id, tag).unwrap();
    }
    cat.remove_tag(id, "b").unwrap();

    let rows: i64 =
        cat.conn().query_row("SELECT COUNT(*) FROM files_fts", [], |r| r.get(0)).unwrap();
    assert_eq!(rows, 1);
}

#[test]
fn migration_backfills_tags_of_existing_catalogs() {
    // A v8 catalog already holds tags; the FTS migration must index them,
    // not only tags written afterwards.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("catalog.db");
    {
        let conn = rusqlite::Connection::open(&path).unwrap();
        for sql in &fotobase_catalog::MIGRATIONS[..8] {
            conn.execute_batch(sql).unwrap();
        }
        conn.execute_batch("PRAGMA user_version = 8").unwrap();
        conn.execute("INSERT INTO folders(path, scan_mode) VALUES ('/photos', 'once')", [])
            .unwrap();
        conn.execute(
            "INSERT INTO files(folder_id, path, filename, size, mtime, media_type)
             VALUES (1, '/photos/a.jpg', 'a.jpg', 1, 0.0, 'image')",
            [],
        )
        .unwrap();
        conn.execute("INSERT INTO metadata(file_id, caption) VALUES (1, 'old caption')", [])
            .unwrap();
        conn.execute("INSERT INTO tags(name) VALUES ('lighthouse'), ('Trip|Spain')", []).unwrap();
        conn.execute("INSERT INTO file_tags(file_id, tag_id) VALUES (1, 1), (1, 2)", []).unwrap();
    }

    let cat = Catalog::open(&path).unwrap();

    assert_eq!(cat.search_files("lighthouse", 10).unwrap(), vec![1]);
    assert_eq!(cat.search_files("spain", 10).unwrap(), vec![1]);
    assert_eq!(cat.search_files("caption", 10).unwrap(), vec![1]);
}

/// Keystroke latency on a 50k-photo catalog (P1 live-search bar). The grid
/// issues one query per keystroke, so every prefix of a typed word must
/// answer well inside a frame budget. The threshold is deliberately loose
/// (debug build, shared CI runners); it exists to catch an accidental
/// table scan or per-row trigger regression, not to benchmark.
#[test]
fn keystroke_latency_on_50k_photos_stays_interactive() {
    const PHOTOS: i64 = 50_000;
    const BUDGET: Duration = Duration::from_millis(250);

    let dir = tempfile::tempdir().unwrap();
    let mut cat = open(&dir);
    let folder_id = cat.seed_folder("/photos").unwrap();
    {
        let tx = cat.conn();
        tx.execute_batch("BEGIN").unwrap();
        for i in 0..PHOTOS {
            tx.execute(
                "INSERT INTO files (folder_id, path, filename, size, mtime, media_type)
                 VALUES (?1, ?2, ?3, 1, 0.0, 'image')",
                rusqlite::params![
                    folder_id,
                    format!("/photos/IMG_{i:05}.jpg"),
                    format!("IMG_{i:05}.jpg")
                ],
            )
            .unwrap();
            let id = tx.last_insert_rowid();
            tx.execute(
                "INSERT INTO metadata (file_id, caption) VALUES (?1, ?2)",
                rusqlite::params![id, format!("family dinner number {}", i % 977)],
            )
            .unwrap();
        }
        for t in 0..200 {
            tx.execute("INSERT INTO tags (name) VALUES (?1)", [format!("tag{t:03}")]).unwrap();
        }
        tx.execute(
            "INSERT INTO file_tags (file_id, tag_id)
             SELECT f.id, 1 + (f.id % 200) FROM files f",
            [],
        )
        .unwrap();
        tx.execute_batch("COMMIT").unwrap();
    }
    let needle: i64 = cat
        .conn()
        .query_row("SELECT id FROM files WHERE filename = 'IMG_01234.jpg'", [], |r| r.get(0))
        .unwrap();
    cat.conn().execute("INSERT INTO tags (name) VALUES ('zebrafish')", []).unwrap();
    cat.conn()
        .execute(
            "INSERT INTO file_tags (file_id, tag_id) VALUES (?1, last_insert_rowid())",
            [needle],
        )
        .unwrap();

    // Warm the FTS index into the page cache first: the intent is
    // steady-state per-keystroke latency, not cold-start cost, and a shared
    // CI runner can push the first (cold) query past the budget (seen as a
    // 253.6ms vs 250ms flake on macos-15).
    let _ = cat.search_files_detailed("warmup", None, 500);

    for query in ["z", "ze", "zeb", "zebr", "zebrafish", "tag1", "tag150", "dinner", "dinner num"] {
        let start = Instant::now();
        let hits = cat.search_files_detailed(query, None, 500).unwrap();
        let elapsed = start.elapsed();
        assert!(!hits.is_empty(), "{query:?} found nothing");
        assert!(elapsed < BUDGET, "{query:?} took {elapsed:?} (budget {BUDGET:?})");
    }
}
