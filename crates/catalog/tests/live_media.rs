//! Live/Motion photo catalog index (TASK-08-017): `media_subtype` is recorded
//! at scan time so the grid badge and the viewer need no per-view probing.

use fotobase_catalog::{Catalog, LiveMedia};

fn open() -> Catalog {
    let dir = tempfile::tempdir().unwrap();
    let cat = Catalog::open(dir.path().join("catalog.db")).unwrap();
    std::mem::forget(dir);
    cat
}

fn insert(cat: &mut Catalog, path: &str, media_type: &str) -> i64 {
    cat.seed_folder("/photos").unwrap();
    cat.conn()
        .execute(
            "INSERT INTO files(folder_id, path, filename, size, mtime, media_type)
             VALUES (1, ?1, ?1, 1, 5.0, ?2)",
            rusqlite::params![path, media_type],
        )
        .unwrap();
    cat.conn().last_insert_rowid()
}

#[test]
fn live_media_roundtrips_and_surfaces_in_the_grid_row() {
    let mut cat = open();
    let motion = insert(&mut cat, "/photos/m.jpg", "image");
    let live = insert(&mut cat, "/photos/l.heic", "image");
    let plain = insert(&mut cat, "/photos/p.jpg", "image");

    cat.set_live_media(motion, 5.0, Some(&LiveMedia::Motion { offset: 100, len: 40 })).unwrap();
    cat.set_live_media(live, 5.0, Some(&LiveMedia::Live { video_path: "/photos/l.mov".into() }))
        .unwrap();
    cat.set_live_media(plain, 5.0, None).unwrap();

    assert_eq!(cat.live_media(motion).unwrap(), Some(LiveMedia::Motion { offset: 100, len: 40 }));
    assert_eq!(
        cat.live_media(live).unwrap(),
        Some(LiveMedia::Live { video_path: "/photos/l.mov".into() })
    );
    assert_eq!(cat.live_media(plain).unwrap(), None);

    let rows = cat.list_files(100).unwrap();
    let kind = |id: i64| rows.iter().find(|r| r.id == id).unwrap().live_kind.clone();
    assert_eq!(kind(motion).as_deref(), Some("motion"));
    assert_eq!(kind(live).as_deref(), Some("live"));
    assert_eq!(kind(plain), None);
}

#[test]
fn set_live_media_replaces_and_clears() {
    let mut cat = open();
    let id = insert(&mut cat, "/photos/a.jpg", "image");
    cat.set_live_media(id, 5.0, Some(&LiveMedia::Motion { offset: 1, len: 2 })).unwrap();
    cat.set_live_media(id, 6.0, Some(&LiveMedia::Live { video_path: "/photos/a.mov".into() }))
        .unwrap();
    assert_eq!(
        cat.live_media(id).unwrap(),
        Some(LiveMedia::Live { video_path: "/photos/a.mov".into() })
    );
    cat.set_live_media(id, 7.0, None).unwrap();
    assert_eq!(cat.live_media(id).unwrap(), None);
}

#[test]
fn live_candidates_list_only_stale_images_and_unpaired_stills() {
    let mut cat = open();
    let fresh = insert(&mut cat, "/photos/fresh.jpg", "image");
    let stale = insert(&mut cat, "/photos/stale.jpg", "image");
    let _unchecked = insert(&mut cat, "/photos/new.heic", "image");
    let _video = insert(&mut cat, "/photos/v.mov", "video");

    cat.set_live_media(fresh, 5.0, None).unwrap();
    cat.set_live_media(stale, 4.0, None).unwrap(); // file mtime moved on to 5.0

    let rows = cat.live_candidates_under("/photos").unwrap();
    let mut names: Vec<&str> = rows.iter().map(|r| r.path.as_str()).collect();
    names.sort();
    // Every image is listed with its check state; videos are not.
    assert_eq!(names, ["/photos/fresh.jpg", "/photos/new.heic", "/photos/stale.jpg"]);
    let by = |p: &str| rows.iter().find(|r| r.path == p).unwrap();
    assert!(!by("/photos/fresh.jpg").needs_check());
    assert!(by("/photos/stale.jpg").needs_check());
    assert!(by("/photos/new.heic").needs_check());

    let videos = cat.video_files_under("/photos").unwrap();
    assert_eq!(videos.len(), 1);
    assert_eq!(videos[0].0, "/photos/v.mov");
}
