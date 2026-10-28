//! TASK-03-016: `list_folders` carries the per-folder aggregates the folder
//! sort needs (file count, total size, newest mtime), so sorting stays a pure
//! frontend concern over the already-loaded folder list.

use fotobase_catalog::Catalog;

fn add(cat: &Catalog, folder: i64, name: &str, size: i64, mtime: f64) {
    cat.conn()
        .execute(
            "INSERT INTO files (folder_id, path, filename, size, mtime, media_type)
             VALUES (?1, ?2, ?3, ?4, ?5, 'image')",
            rusqlite::params![folder, format!("/x/{folder}/{name}"), name, size, mtime],
        )
        .unwrap();
}

#[test]
fn folders_report_count_total_size_and_newest_mtime() {
    let dir = tempfile::tempdir().unwrap();
    let mut cat = Catalog::open(dir.path().join("c.db")).unwrap();
    let full = cat.seed_folder("/p/full").unwrap();
    let empty = cat.seed_folder("/p/empty").unwrap();
    add(&cat, full, "a.jpg", 100, 1_000.0);
    add(&cat, full, "b.jpg", 250, 3_000.9);

    let folders = cat.list_folders().unwrap();
    let full = folders.iter().find(|f| f.id == full).unwrap();
    assert_eq!(full.file_count, 2);
    assert_eq!(full.total_size, 350);
    assert_eq!(full.newest_mtime, Some(3_000));

    let empty = folders.iter().find(|f| f.id == empty).unwrap();
    assert_eq!(empty.file_count, 0);
    assert_eq!(empty.total_size, 0);
    assert_eq!(empty.newest_mtime, None);
}
