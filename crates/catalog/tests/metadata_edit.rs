//! TASK-03-020: info-pane metadata editing — date, GPS, artist and copyright
//! are written to the sidecar first (00-010 §3), the catalog mirror follows,
//! and invalid input is refused field by field with nothing written.

use std::path::{Path, PathBuf};

use fotobase_catalog::{
    sidecar_path, Catalog, FieldError, GpsEdit, MetadataEdit, MetadataField, PhotoInfo,
};
use fotobase_metadata::Sidecar;

fn add_photo(cat: &mut Catalog, dir: &Path, name: &str, taken_at: Option<&str>) -> (i64, PathBuf) {
    let original = dir.join(name);
    std::fs::write(&original, b"fake image bytes").unwrap();
    let folder_id = cat.seed_folder(dir.to_str().unwrap()).unwrap();
    cat.conn()
        .execute(
            "INSERT INTO files (folder_id, path, filename, size, mtime, taken_at, media_type)
             VALUES (?1, ?2, ?3, 10, 0.0, ?4, 'image')",
            rusqlite::params![folder_id, original.to_str().unwrap(), name, taken_at],
        )
        .unwrap();
    (cat.conn().last_insert_rowid(), original)
}

fn sidecar(original: &Path) -> Sidecar {
    Sidecar::read(&sidecar_path(original.to_str().unwrap())).unwrap()
}

fn info(cat: &Catalog, id: i64) -> PhotoInfo {
    cat.photo_info(id).unwrap().unwrap()
}

fn full_edit() -> MetadataEdit {
    MetadataEdit {
        taken_at: Some("2024-06-01T09:30:00".into()),
        gps: Some(GpsEdit::Set { latitude: "52°31'59.9\"N".into(), longitude: "13.405".into() }),
        artist: Some("Ada Lovelace".into()),
        copyright: Some("© 2024 Ada".into()),
    }
}

fn fields(errors: &[FieldError]) -> Vec<MetadataField> {
    errors.iter().map(|e| e.field).collect()
}

#[test]
fn editing_all_four_fields_writes_the_sidecar_and_the_catalog_mirror() {
    let dir = tempfile::tempdir().unwrap();
    let mut cat = Catalog::open(dir.path().join("c.db")).unwrap();
    let (id, original) = add_photo(&mut cat, dir.path(), "a.jpg", Some("2024-06-01T12:00:00"));

    let errors = cat.edit_metadata(id, &full_edit()).unwrap();

    assert!(errors.is_empty(), "{errors:?}");
    let sc = sidecar(&original);
    assert_eq!(sc.capture_time().unwrap().to_xmp(), "2024-06-01T09:30:00");
    let (lat, lon) = sc.gps().unwrap();
    assert!((lat - (52.0 + 31.0 / 60.0 + 59.9 / 3600.0)).abs() < 1e-6, "{lat}");
    assert!((lon - 13.405).abs() < 1e-6, "{lon}");
    assert_eq!(sc.artist().as_deref(), Some("Ada Lovelace"));
    assert_eq!(sc.copyright().as_deref(), Some("© 2024 Ada"));

    let shown = info(&cat, id);
    assert_eq!(shown.taken_at.as_deref(), Some("2024-06-01T09:30:00"));
    assert!((shown.latitude.unwrap() - lat).abs() < 1e-9);
    assert!((shown.longitude.unwrap() - lon).abs() < 1e-9);
    assert_eq!(shown.artist.as_deref(), Some("Ada Lovelace"));
    assert_eq!(shown.copyright.as_deref(), Some("© 2024 Ada"));
}

#[test]
fn a_date_without_an_offset_keeps_the_photos_existing_offset() {
    let dir = tempfile::tempdir().unwrap();
    let mut cat = Catalog::open(dir.path().join("c.db")).unwrap();
    let (id, original) =
        add_photo(&mut cat, dir.path(), "a.jpg", Some("2024-06-01T12:00:00.25+02:00"));

    let edit = MetadataEdit { taken_at: Some("2024-06-02T08:00:00".into()), ..Default::default() };
    assert!(cat.edit_metadata(id, &edit).unwrap().is_empty());

    assert_eq!(sidecar(&original).capture_time().unwrap().to_xmp(), "2024-06-02T08:00:00+02:00");
}

#[test]
fn a_date_with_its_own_offset_wins() {
    let dir = tempfile::tempdir().unwrap();
    let mut cat = Catalog::open(dir.path().join("c.db")).unwrap();
    let (id, original) =
        add_photo(&mut cat, dir.path(), "a.jpg", Some("2024-06-01T12:00:00+02:00"));

    let edit =
        MetadataEdit { taken_at: Some("2024-06-02T08:00:00-05:00".into()), ..Default::default() };
    assert!(cat.edit_metadata(id, &edit).unwrap().is_empty());

    assert_eq!(sidecar(&original).capture_time().unwrap().to_xmp(), "2024-06-02T08:00:00-05:00");
}

#[test]
fn untouched_fields_stay_untouched() {
    let dir = tempfile::tempdir().unwrap();
    let mut cat = Catalog::open(dir.path().join("c.db")).unwrap();
    let (id, original) = add_photo(&mut cat, dir.path(), "a.jpg", Some("2024-06-01T12:00:00"));
    cat.edit_metadata(id, &full_edit()).unwrap();
    cat.set_rating(id, 4).unwrap();
    cat.set_caption(id, "beach").unwrap();

    let edit = MetadataEdit { artist: Some("Grace".into()), ..Default::default() };
    cat.edit_metadata(id, &edit).unwrap();

    let sc = sidecar(&original);
    assert_eq!(sc.artist().as_deref(), Some("Grace"));
    assert_eq!(sc.copyright().as_deref(), Some("© 2024 Ada"));
    assert!(sc.gps().is_some());
    assert_eq!(sc.capture_time().unwrap().to_xmp(), "2024-06-01T09:30:00");
    assert_eq!(sc.rating(), Some(4));
    assert_eq!(sc.caption().as_deref(), Some("beach"));
}

#[test]
fn blank_artist_and_copyright_and_a_gps_clear_remove_the_fields() {
    let dir = tempfile::tempdir().unwrap();
    let mut cat = Catalog::open(dir.path().join("c.db")).unwrap();
    let (id, original) = add_photo(&mut cat, dir.path(), "a.jpg", None);
    cat.edit_metadata(id, &full_edit()).unwrap();

    let edit = MetadataEdit {
        gps: Some(GpsEdit::Clear),
        artist: Some("   ".into()),
        copyright: Some(String::new()),
        ..Default::default()
    };
    assert!(cat.edit_metadata(id, &edit).unwrap().is_empty());

    let sc = sidecar(&original);
    assert_eq!((sc.gps(), sc.artist(), sc.copyright()), (None, None, None));
    let shown = info(&cat, id);
    assert_eq!((shown.latitude, shown.longitude), (None, None));
    assert_eq!((shown.artist, shown.copyright), (None, None));
}

#[test]
fn invalid_input_is_rejected_per_field_and_nothing_is_written() {
    let dir = tempfile::tempdir().unwrap();
    let mut cat = Catalog::open(dir.path().join("c.db")).unwrap();
    let (id, original) = add_photo(&mut cat, dir.path(), "a.jpg", Some("2024-06-01T12:00:00"));

    let edit = MetadataEdit {
        taken_at: Some("last tuesday".into()),
        gps: Some(GpsEdit::Set { latitude: "95".into(), longitude: "13.4 N".into() }),
        artist: Some("Ada".into()),
        copyright: None,
    };
    let errors = cat.edit_metadata(id, &edit).unwrap();

    assert_eq!(
        fields(&errors),
        vec![MetadataField::TakenAt, MetadataField::Latitude, MetadataField::Longitude]
    );
    assert!(errors.iter().all(|e| !e.message.is_empty()));
    assert!(!sidecar_path(original.to_str().unwrap()).exists(), "no sidecar written");
    assert_eq!(info(&cat, id).taken_at.as_deref(), Some("2024-06-01T12:00:00"));
    assert_eq!(info(&cat, id).artist, None, "valid fields are not applied either");
}

#[test]
fn an_empty_edit_writes_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let mut cat = Catalog::open(dir.path().join("c.db")).unwrap();
    let (id, original) = add_photo(&mut cat, dir.path(), "a.jpg", None);

    assert!(cat.edit_metadata(id, &MetadataEdit::default()).unwrap().is_empty());

    assert!(!sidecar_path(original.to_str().unwrap()).exists());
}

#[test]
fn an_unreadable_sidecar_value_for_the_date_base_does_not_block_other_fields() {
    let dir = tempfile::tempdir().unwrap();
    let mut cat = Catalog::open(dir.path().join("c.db")).unwrap();
    let (id, original) = add_photo(&mut cat, dir.path(), "a.jpg", Some("sometime in june"));

    let edit = MetadataEdit { artist: Some("Ada".into()), ..Default::default() };
    assert!(cat.edit_metadata(id, &edit).unwrap().is_empty());

    assert_eq!(sidecar(&original).artist().as_deref(), Some("Ada"));
}

#[test]
fn an_unknown_photo_is_an_error() {
    let dir = tempfile::tempdir().unwrap();
    let mut cat = Catalog::open(dir.path().join("c.db")).unwrap();
    assert!(cat.edit_metadata(999, &full_edit()).is_err());
}

#[test]
fn other_applications_nodes_survive_an_edit() {
    let dir = tempfile::tempdir().unwrap();
    let mut cat = Catalog::open(dir.path().join("c.db")).unwrap();
    let (id, original) = add_photo(&mut cat, dir.path(), "a.jpg", None);
    std::fs::write(
        sidecar_path(original.to_str().unwrap()),
        r#"<x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"><rdf:Description rdf:about="" xmlns:acme="http://acme.example/ns/" acme:Mood="calm"><acme:Custom>keep me</acme:Custom></rdf:Description></rdf:RDF></x:xmpmeta>"#,
    )
    .unwrap();

    cat.edit_metadata(id, &full_edit()).unwrap();

    let raw = sidecar(&original).raw().to_string();
    assert!(raw.contains("acme:Mood=\"calm\""), "{raw}");
    assert!(raw.contains("<acme:Custom>keep me</acme:Custom>"), "{raw}");
}

#[test]
fn rebuild_from_sidecars_restores_every_edited_field() {
    let dir = tempfile::tempdir().unwrap();
    let mut cat = Catalog::open(dir.path().join("c.db")).unwrap();
    let (id, _) = add_photo(&mut cat, dir.path(), "a.jpg", Some("2024-06-01T12:00:00"));
    cat.edit_metadata(id, &full_edit()).unwrap();
    let before = info(&cat, id);

    cat.conn()
        .execute(
            "UPDATE metadata SET latitude = NULL, longitude = NULL, artist = NULL, copyright = NULL",
            [],
        )
        .unwrap();
    cat.conn().execute("UPDATE files SET taken_at = NULL", []).unwrap();
    cat.rebuild_from_sidecars(true).unwrap();

    assert_eq!(info(&cat, id), before);
}

#[test]
fn a_sidecar_edited_by_another_tool_wins_on_the_next_sync() {
    let dir = tempfile::tempdir().unwrap();
    let mut cat = Catalog::open(dir.path().join("c.db")).unwrap();
    let (id, original) = add_photo(&mut cat, dir.path(), "a.jpg", None);
    cat.edit_metadata(id, &full_edit()).unwrap();

    let path = sidecar_path(original.to_str().unwrap());
    let mut sc = Sidecar::read(&path).unwrap();
    sc.set_artist("Someone Else");
    sc.write(&path).unwrap();
    cat.rebuild_from_sidecars(true).unwrap();

    assert_eq!(info(&cat, id).artist.as_deref(), Some("Someone Else"));
}

#[test]
fn migrating_an_older_catalog_makes_the_next_rebuild_reread_sidecars() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("c.db");
    let mut cat = Catalog::open(&db).unwrap();
    let (id, original) = add_photo(&mut cat, dir.path(), "a.jpg", None);
    cat.set_rating(id, 3).unwrap();
    // Another tool adds an artist, then the catalog is rolled back to the
    // schema before the artist column existed.
    let path = sidecar_path(original.to_str().unwrap());
    let mut sc = Sidecar::read(&path).unwrap();
    sc.set_artist("Ada");
    sc.write(&path).unwrap();
    drop(cat);
    let conn = rusqlite::Connection::open(&db).unwrap();
    let previous = (fotobase_catalog::MIGRATIONS.len() - 1) as i64;
    conn.execute_batch(
        "ALTER TABLE metadata DROP COLUMN artist; ALTER TABLE metadata DROP COLUMN copyright;",
    )
    .unwrap();
    conn.pragma_update(None, "user_version", previous).unwrap();
    drop(conn);

    let mut cat = Catalog::open(&db).unwrap();
    cat.rebuild_from_sidecars(false).unwrap();

    assert_eq!(info(&cat, id).artist.as_deref(), Some("Ada"));
}
