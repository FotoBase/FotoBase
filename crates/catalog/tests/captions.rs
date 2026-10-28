//! TASK-03-007: captions write through to `dc:description`, are searchable
//! via `files_fts`, and survive a rebuild from sidecars.

use std::path::{Path, PathBuf};

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

fn sidecar_caption(original: &Path) -> Option<String> {
    Sidecar::read(&sidecar_path(original.to_str().unwrap())).unwrap().caption()
}

#[test]
fn set_caption_writes_dc_description_and_the_catalog_mirror() {
    let dir = tempfile::tempdir().unwrap();
    let mut cat = Catalog::open(dir.path().join("c.db")).unwrap();
    let (id, original) = add_photo(&mut cat, dir.path(), "a.jpg");

    cat.set_caption(id, "sunset over the bay").unwrap();

    assert_eq!(sidecar_caption(&original).as_deref(), Some("sunset over the bay"));
    assert_eq!(cat.file_caption(id).unwrap().as_deref(), Some("sunset over the bay"));
    let raw = std::fs::read_to_string(sidecar_path(original.to_str().unwrap())).unwrap();
    assert!(raw.contains(r#"xml:lang="x-default""#), "{raw}");
}

#[test]
fn caption_is_searchable_as_you_type() {
    let dir = tempfile::tempdir().unwrap();
    let mut cat = Catalog::open(dir.path().join("c.db")).unwrap();
    let (id, _) = add_photo(&mut cat, dir.path(), "a.jpg");
    let (other, _) = add_photo(&mut cat, dir.path(), "b.jpg");
    cat.set_caption(id, "Grandma blowing out candles").unwrap();

    assert_eq!(cat.search_files("gran", 10).unwrap(), vec![id]);
    assert_eq!(cat.search_files("candles blowing", 10).unwrap(), vec![id]);
    assert!(!cat.search_files("candles", 10).unwrap().contains(&other));
    // Filename search keeps working next to captions.
    assert_eq!(cat.search_files("b.jpg", 10).unwrap(), vec![other]);
    assert_eq!(cat.search_files_detailed("candles", None, 10).unwrap()[0].id, id);
}

#[test]
fn editing_a_caption_replaces_the_searchable_text() {
    let dir = tempfile::tempdir().unwrap();
    let mut cat = Catalog::open(dir.path().join("c.db")).unwrap();
    let (id, _) = add_photo(&mut cat, dir.path(), "a.jpg");
    cat.set_caption(id, "first draft").unwrap();

    cat.set_caption(id, "final words").unwrap();

    assert!(cat.search_files("draft", 10).unwrap().is_empty());
    assert_eq!(cat.search_files("final", 10).unwrap(), vec![id]);
}

#[test]
fn multi_line_caption_round_trips() {
    let dir = tempfile::tempdir().unwrap();
    let mut cat = Catalog::open(dir.path().join("c.db")).unwrap();
    let (id, original) = add_photo(&mut cat, dir.path(), "a.jpg");
    let caption = "line one\nline two\n\nline four";

    cat.set_caption(id, caption).unwrap();

    assert_eq!(sidecar_caption(&original).as_deref(), Some(caption));
    assert_eq!(cat.file_caption(id).unwrap().as_deref(), Some(caption));
    assert_eq!(cat.search_files("two", 10).unwrap(), vec![id]);
}

#[test]
fn clearing_a_caption_removes_it_from_sidecar_catalog_and_search() {
    let dir = tempfile::tempdir().unwrap();
    let mut cat = Catalog::open(dir.path().join("c.db")).unwrap();
    let (id, original) = add_photo(&mut cat, dir.path(), "a.jpg");
    cat.set_caption(id, "sunset").unwrap();

    cat.clear_caption(id).unwrap();

    assert_eq!(sidecar_caption(&original), None);
    let raw = std::fs::read_to_string(sidecar_path(original.to_str().unwrap())).unwrap();
    assert!(!raw.contains("dc:description"), "{raw}");
    assert_eq!(cat.file_caption(id).unwrap(), None);
    assert!(cat.search_files("sunset", 10).unwrap().is_empty());
}

#[test]
fn setting_a_blank_caption_clears_it() {
    let dir = tempfile::tempdir().unwrap();
    let mut cat = Catalog::open(dir.path().join("c.db")).unwrap();
    let (id, original) = add_photo(&mut cat, dir.path(), "a.jpg");
    cat.set_caption(id, "sunset").unwrap();

    cat.set_caption(id, " \n\t ").unwrap();

    assert_eq!(sidecar_caption(&original), None);
    assert_eq!(cat.file_caption(id).unwrap(), None);
    assert!(cat.search_files("sunset", 10).unwrap().is_empty());
}

#[test]
fn caption_and_searchability_survive_catalog_rebuild() {
    let dir = tempfile::tempdir().unwrap();
    let mut cat = Catalog::open(dir.path().join("c.db")).unwrap();
    let (id, _) = add_photo(&mut cat, dir.path(), "a.jpg");
    cat.set_caption(id, "sunset over the bay").unwrap();

    // Wipe the derived mirror (and with it the FTS text), then rebuild from
    // the sidecars — the sidecar is the source of truth (PLAN §5).
    cat.conn().execute("DELETE FROM metadata", []).unwrap();
    assert!(cat.search_files("sunset", 10).unwrap().is_empty());
    cat.rebuild_from_sidecars(true).unwrap();

    assert_eq!(cat.file_caption(id).unwrap().as_deref(), Some("sunset over the bay"));
    assert_eq!(cat.search_files("sunset", 10).unwrap(), vec![id]);
}

#[test]
fn caption_edited_by_another_tool_converges_into_search() {
    let dir = tempfile::tempdir().unwrap();
    let mut cat = Catalog::open(dir.path().join("c.db")).unwrap();
    let (id, original) = add_photo(&mut cat, dir.path(), "a.jpg");
    cat.set_caption(id, "mine").unwrap();

    let sc_path = sidecar_path(original.to_str().unwrap());
    let mut sc = Sidecar::read(&sc_path).unwrap();
    sc.set_caption("edited elsewhere");
    sc.write(&sc_path).unwrap();
    cat.rebuild_from_sidecars(true).unwrap();

    assert_eq!(cat.search_files("elsewhere", 10).unwrap(), vec![id]);
    assert!(cat.search_files("mine", 10).unwrap().is_empty());
}

#[test]
fn originals_are_never_touched_by_caption_writes() {
    let dir = tempfile::tempdir().unwrap();
    let mut cat = Catalog::open(dir.path().join("c.db")).unwrap();
    let (id, original) = add_photo(&mut cat, dir.path(), "a.jpg");
    let before = std::fs::read(&original).unwrap();

    cat.set_caption(id, "sunset").unwrap();
    cat.clear_caption(id).unwrap();

    assert_eq!(std::fs::read(&original).unwrap(), before);
}

#[test]
fn deleting_the_file_row_drops_its_caption_from_search() {
    let dir = tempfile::tempdir().unwrap();
    let mut cat = Catalog::open(dir.path().join("c.db")).unwrap();
    let (id, _) = add_photo(&mut cat, dir.path(), "a.jpg");
    cat.set_caption(id, "sunset").unwrap();

    cat.conn().execute("DELETE FROM files WHERE id = ?1", [id]).unwrap();

    assert!(cat.search_files("sunset", 10).unwrap().is_empty());
}

#[test]
fn migration_backfills_captions_of_existing_catalogs() {
    // A v5 catalog already holds a caption in `metadata`; the FTS migration
    // must index it, not only captions written afterwards.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("catalog.db");
    {
        let conn = rusqlite::Connection::open(&path).unwrap();
        for sql in &fotobase_catalog::MIGRATIONS[..5] {
            conn.execute_batch(sql).unwrap();
        }
        conn.execute_batch("PRAGMA user_version = 5").unwrap();
        conn.execute("INSERT INTO folders(path, scan_mode) VALUES ('/photos', 'once')", [])
            .unwrap();
        conn.execute(
            "INSERT INTO files(folder_id, path, filename, size, mtime, media_type)
             VALUES (1, '/photos/a.jpg', 'a.jpg', 1, 0.0, 'image')",
            [],
        )
        .unwrap();
        conn.execute("INSERT INTO metadata(file_id, caption) VALUES (1, 'old sunset')", [])
            .unwrap();
    }

    let cat = Catalog::open(&path).unwrap();

    assert_eq!(cat.search_files("sunset", 10).unwrap(), vec![1]);
}
