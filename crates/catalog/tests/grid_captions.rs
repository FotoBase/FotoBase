//! TASK-03-015: the grid row carries what every thumbnail caption mode shows
//! (filename, caption, tags, resolution), so switching modes is a pure
//! frontend re-render — no extra query, no rescan.

use fotobase_catalog::{Catalog, FileQuery};

fn catalog() -> (tempfile::TempDir, Catalog, i64) {
    let dir = tempfile::tempdir().unwrap();
    let mut cat = Catalog::open(dir.path().join("c.db")).unwrap();
    let folder = cat.seed_folder(dir.path().to_str().unwrap()).unwrap();
    (dir, cat, folder)
}

fn add(cat: &Catalog, folder: i64, name: &str, dims: Option<(i64, i64)>) -> i64 {
    cat.conn()
        .execute(
            "INSERT INTO files (folder_id, path, filename, size, mtime, media_type, width, height)
             VALUES (?1, ?2, ?3, 10, 1.0, 'image', ?4, ?5)",
            rusqlite::params![
                folder,
                format!("/x/{name}"),
                name,
                dims.map(|d| d.0),
                dims.map(|d| d.1)
            ],
        )
        .unwrap();
    cat.conn().last_insert_rowid()
}

#[test]
fn rows_carry_caption_tags_and_dimensions() {
    let (_dir, cat, folder) = catalog();
    let id = add(&cat, folder, "a.jpg", Some((4000, 3000)));
    cat.conn()
        .execute("INSERT INTO metadata (file_id, caption) VALUES (?1, 'Sunset at the pier')", [id])
        .unwrap();
    for tag in ["Vacation|Italy", "beach"] {
        cat.conn().execute("INSERT OR IGNORE INTO tags (name) VALUES (?1)", [tag]).unwrap();
        cat.conn()
            .execute(
                "INSERT INTO file_tags (file_id, tag_id) SELECT ?1, id FROM tags WHERE name = ?2",
                rusqlite::params![id, tag],
            )
            .unwrap();
    }

    let rows = cat.query_files(&FileQuery::default(), usize::MAX).unwrap();
    assert_eq!(rows.len(), 1);
    let row = &rows[0];
    assert_eq!(row.caption.as_deref(), Some("Sunset at the pier"));
    assert_eq!(row.tags, vec!["Vacation|Italy".to_string(), "beach".to_string()]);
    assert_eq!((row.width, row.height), (Some(4000), Some(3000)));
}

#[test]
fn rows_without_metadata_have_empty_caption_tags_and_dimensions() {
    let (_dir, cat, folder) = catalog();
    add(&cat, folder, "bare.jpg", None);

    let rows = cat.query_files(&FileQuery::default(), usize::MAX).unwrap();
    assert_eq!(rows[0].caption, None);
    assert!(rows[0].tags.is_empty());
    assert_eq!((rows[0].width, rows[0].height), (None, None));
}

#[test]
fn every_grid_listing_carries_the_caption_data() {
    let (_dir, cat, folder) = catalog();
    let id = add(&cat, folder, "a.jpg", Some((10, 20)));
    cat.conn()
        .execute("INSERT INTO metadata (file_id, caption) VALUES (?1, 'hello')", [id])
        .unwrap();

    let listed = cat.search_files_detailed("hello", None, usize::MAX).unwrap();
    assert_eq!(listed[0].caption.as_deref(), Some("hello"));
    assert_eq!(listed[0].width, Some(10));
}
