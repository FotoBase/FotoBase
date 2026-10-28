//! TASK-03-009 / TASK-03-012: the shell applies the user's "show hidden" and
//! undo-window settings to its catalog connection.

use fotobase_catalog::Catalog;
use fotobase_lib::settings::Settings;
use fotobase_lib::trash_commands::apply_settings;

fn catalog_with_hidden_photo() -> (tempfile::TempDir, Catalog) {
    let tmp = tempfile::tempdir().unwrap();
    let photos = tmp.path().join("photos");
    std::fs::create_dir_all(&photos).unwrap();
    std::fs::write(photos.join("a.jpg"), b"x").unwrap();
    let mut cat = Catalog::open(tmp.path().join("catalog.db")).unwrap();
    let folder = cat.seed_folder(photos.to_str().unwrap()).unwrap();
    cat.conn()
        .execute(
            "INSERT INTO files (folder_id, path, filename, size, mtime, media_type)
             VALUES (?1, ?2, 'a.jpg', 1, 0.0, 'image')",
            rusqlite::params![folder, photos.join("a.jpg").to_str().unwrap()],
        )
        .unwrap();
    let id = cat.conn().last_insert_rowid();
    cat.hide_photos(&[id]).unwrap();
    (tmp, cat)
}

#[test]
fn show_hidden_follows_the_setting() {
    let (_tmp, mut cat) = catalog_with_hidden_photo();
    assert!(cat.list_files(10).unwrap().is_empty());

    apply_settings(&mut cat, &Settings { show_hidden: true, ..Settings::default() }).unwrap();
    assert_eq!(cat.list_files(10).unwrap().len(), 1);

    apply_settings(&mut cat, &Settings::default()).unwrap();
    assert!(cat.list_files(10).unwrap().is_empty());
}

#[test]
fn the_startup_sweep_uses_the_configured_window() {
    let (_tmp, mut cat) = catalog_with_hidden_photo();
    cat.conn().execute("UPDATE trash SET created_at = created_at - 10 * 86400", []).unwrap();

    let long = Settings { trash_window_days: 30, ..Settings::default() };
    assert_eq!(apply_settings(&mut cat, &long).unwrap(), 0);
    assert_eq!(cat.trash_list().unwrap().len(), 1);

    let short = Settings { trash_window_days: 7, ..Settings::default() };
    assert_eq!(apply_settings(&mut cat, &short).unwrap(), 1);
    assert!(cat.trash_list().unwrap().is_empty());
}
