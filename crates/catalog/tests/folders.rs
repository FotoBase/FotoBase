//! Folder-tree data tests (TASK-01-015, PLAN §2B): the Folders view groups
//! real on-disk folders by year, derived from the dates of the files inside
//! them (`taken_at` preferred, `mtime` fallback), and folder selection
//! filters the grid by `folder_id`.

use fotobase_catalog::Catalog;
use rusqlite::params;
use tempfile::TempDir;

fn open_in(dir: &TempDir) -> Catalog {
    Catalog::open(dir.path().join("catalog.db")).expect("open catalog")
}

/// Insert a file row directly (scanner-shaped: path unique, mtime epoch
/// seconds, taken_at an ISO timestamp or NULL).
fn add_file(cat: &mut Catalog, folder: i64, path: &str, mtime: f64, taken_at: Option<&str>) {
    let filename = path.rsplit('/').next().unwrap().to_string();
    cat.conn()
        .execute(
            "INSERT INTO files(folder_id, path, filename, size, mtime, hash, taken_at, media_type)
             VALUES (?1, ?2, ?3, 1, ?4, NULL, ?5, 'image')",
            params![folder, path, filename, mtime, taken_at],
        )
        .unwrap();
}

#[test]
fn folders_group_under_the_year_of_their_newest_file() {
    let dir = TempDir::new().unwrap();
    let mut cat = open_in(&dir);

    // taken_at wins over mtime when both are present.
    let trip = cat.seed_folder("/photos/2023 trip").unwrap();
    add_file(
        &mut cat,
        trip,
        "/photos/2023 trip/a.jpg",
        1_600_000_000.0,
        Some("2023-05-01T10:00:00"),
    );
    add_file(
        &mut cat,
        trip,
        "/photos/2023 trip/b.jpg",
        1_700_000_000.0,
        Some("2024-01-15T08:00:00"),
    );

    // No taken_at: the year falls back to mtime (2020-01-01 UTC).
    let legacy = cat.seed_folder("/photos/legacy").unwrap();
    add_file(&mut cat, legacy, "/photos/legacy/c.jpg", 1_577_836_800.0, None);

    // No files at all: no year can be derived.
    cat.seed_folder("/photos/empty").unwrap();

    let mut folders = cat.list_folders().unwrap();
    folders.sort_by(|a, b| a.path.cmp(&b.path));

    let by_path = |p: &str| folders.iter().find(|f| f.path == p).expect("folder listed");
    assert_eq!(by_path("/photos/2023 trip").year, Some(2024));
    assert_eq!(by_path("/photos/legacy").year, Some(2020));
    assert_eq!(by_path("/photos/empty").year, None);
}

#[test]
fn folders_are_listed_with_ids_and_paths() {
    let dir = TempDir::new().unwrap();
    let mut cat = open_in(&dir);
    let a = cat.seed_folder("/a").unwrap();
    let b = cat.seed_folder("/b").unwrap();
    // Re-seeding the same path must not duplicate rows.
    cat.seed_folder("/a").unwrap();

    let folders = cat.list_folders().unwrap();
    assert_eq!(folders.len(), 2);
    assert!(folders.iter().any(|f| f.id == a && f.path == "/a"));
    assert!(folders.iter().any(|f| f.id == b && f.path == "/b"));
}

#[test]
fn files_filter_to_the_selected_folders() {
    let dir = TempDir::new().unwrap();
    let mut cat = open_in(&dir);
    let a = cat.seed_folder("/a").unwrap();
    let b = cat.seed_folder("/b").unwrap();
    add_file(&mut cat, a, "/a/one.jpg", 1.0, None);
    add_file(&mut cat, a, "/a/two.jpg", 2.0, None);
    add_file(&mut cat, b, "/b/three.jpg", 3.0, None);

    let in_a = cat.list_files_in_folders(&[a], usize::MAX).unwrap();
    assert_eq!(
        in_a.iter().map(|f| f.filename.as_str()).collect::<Vec<_>>(),
        vec!["one.jpg", "two.jpg"]
    );

    let in_both = cat.list_files_in_folders(&[b, a], usize::MAX).unwrap();
    assert_eq!(in_both.len(), 3);

    // Empty selection filters everything out (never an unbounded query).
    assert!(cat.list_files_in_folders(&[], usize::MAX).unwrap().is_empty());
    assert!(cat.list_files_in_folders(&[999_999], usize::MAX).unwrap().is_empty());
}
