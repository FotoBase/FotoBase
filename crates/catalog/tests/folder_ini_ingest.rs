//! TASK-03-022: folder ini ingest. Stars, albums and faces become XMP
//! sidecar data through the normal merge-on-write path; the ini files are
//! never touched and a rerun changes nothing (PLAN §1 onboarding, §5).

use std::path::{Path, PathBuf};

use fotobase_catalog::{sidecar_path, Catalog, FolderIniStatus};
use fotobase_metadata::{Sidecar, FOLDER_INI_FILE, FOLDER_INI_HIDDEN_FILE};

const INI: &str = "[Folder]
name=Holiday
[IMG_0001.JPG]
star=yes
caption=At the beach
keywords=beach,family
albums=aaa111
faces=rect64(2000400060008000),c0ffee0000000001;rect64(1000100020002000),ffffffffffffffff
[img_0002.jpg]
albums=aaa111,bbb222
[gone.jpg]
star=yes
[.album:aaa111]
name=Italy 2009
token=aaa111
date=2009-04-14T21:05:42+00:00
location=Rome
[Contacts2]
c0ffee0000000001=Anna Smith;anna@example.com
";

fn open(dir: &Path) -> Catalog {
    Catalog::open(dir.join("catalog.db")).unwrap()
}

fn add_photo(cat: &mut Catalog, dir: &Path, name: &str) -> (i64, PathBuf) {
    add_photo_bytes(cat, dir, name, format!("fake image bytes of {name}").as_bytes())
}

fn add_photo_bytes(cat: &mut Catalog, dir: &Path, name: &str, bytes: &[u8]) -> (i64, PathBuf) {
    let original = dir.join(name);
    std::fs::write(&original, bytes).unwrap();
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

fn sidecar_of(original: &Path) -> Sidecar {
    Sidecar::read(&sidecar_path(original.to_str().unwrap())).unwrap()
}

fn sidecar_bytes(original: &Path) -> Option<Vec<u8>> {
    std::fs::read(sidecar_path(original.to_str().unwrap())).ok()
}

/// A library folder with an ini, two catalogued photos and the catalog.
fn library() -> (tempfile::TempDir, tempfile::TempDir, Catalog, (i64, PathBuf), (i64, PathBuf)) {
    let lib = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    let mut cat = open(data.path());
    let a = add_photo(&mut cat, lib.path(), "IMG_0001.JPG");
    // Catalogued under a different case than the ini's section name.
    let b = add_photo(&mut cat, lib.path(), "IMG_0002.jpg");
    std::fs::write(lib.path().join(FOLDER_INI_FILE), INI).unwrap();
    (lib, data, cat, a, b)
}

#[test]
fn converts_stars_tags_caption_albums_and_faces_into_the_sidecar() {
    let (lib, _data, mut cat, (a_id, a), (_b_id, b)) = library();

    let report = cat.ingest_folder_ini(lib.path().to_str().unwrap()).unwrap();

    let sc = sidecar_of(&a);
    assert_eq!(sc.rating(), Some(5), "star → xmp:Rating = 5 (ADR-0002)");
    assert_eq!(sc.tags(), vec!["beach", "family"]);
    assert_eq!(sc.caption().as_deref(), Some("At the beach"));
    assert_eq!(sc.albums().len(), 1);
    let faces = sc.face_regions();
    assert_eq!(faces.len(), 1, "only the named face is migrated: {faces:?}");
    assert_eq!(faces[0].name, "Anna Smith");
    // rect64(2000 4000 6000 8000): centre x = (0x2000+0x6000)/2/65535, w = 0x4000/65535.
    assert!((faces[0].x - 0x4000 as f64 / 65535.0).abs() < 1e-5, "{faces:?}");
    assert!((faces[0].y - 0x6000 as f64 / 65535.0).abs() < 1e-5);
    assert!((faces[0].w - 0x4000 as f64 / 65535.0).abs() < 1e-5);

    // The catalog mirror follows the sidecar.
    assert_eq!(cat.file_tags(a_id).unwrap(), vec!["beach", "family"]);
    assert_eq!(cat.file_caption(a_id).unwrap().as_deref(), Some("At the beach"));
    let albums = cat.list_albums().unwrap();
    let italy = albums.iter().find(|x| x.name == "Italy 2009").expect("album defined");
    assert_eq!(italy.date.as_deref(), Some("2009-04-14"));
    assert_eq!(italy.place.as_deref(), Some("Rome"));
    assert_eq!(italy.count, 2, "both photos reference the token");
    assert_eq!(sc.albums(), vec![italy.uuid.clone()]);

    // bbb222 has no definition: membership is kept in a placeholder album.
    assert_eq!(albums.len(), 2, "{albums:?}");
    assert_eq!(sidecar_of(&b).albums().len(), 2);

    assert_eq!(report.ini_files, 1);
    assert_eq!(report.matched, 2);
    assert_eq!(report.written, 2);
    assert_eq!(report.unmatched, 1, "the `gone.jpg` section has no catalogued photo");
    assert_eq!(report.stars, 1);
    assert_eq!(report.faces, 1);
    assert_eq!(report.faces_skipped, 1, "the unnamed face");
    assert_eq!(report.albums_created, 2);
    assert!(report.failed.is_empty());
}

#[test]
fn rerunning_is_idempotent_and_writes_nothing() {
    let (lib, _data, mut cat, (_, a), (_, b)) = library();
    let dir = lib.path().to_str().unwrap();
    cat.ingest_folder_ini(dir).unwrap();

    let (a_bytes, b_bytes) = (sidecar_bytes(&a).unwrap(), sidecar_bytes(&b).unwrap());
    let a_mtime = std::fs::metadata(sidecar_path(a.to_str().unwrap())).unwrap().modified().unwrap();
    let albums_before = cat.list_albums().unwrap();

    let again = cat.ingest_folder_ini(dir).unwrap();

    assert_eq!(again.written, 0, "{again:?}");
    assert_eq!(again.unchanged, 2);
    assert_eq!(again.albums_created, 0);
    assert_eq!(sidecar_bytes(&a).unwrap(), a_bytes);
    assert_eq!(sidecar_bytes(&b).unwrap(), b_bytes);
    let a_mtime_after =
        std::fs::metadata(sidecar_path(a.to_str().unwrap())).unwrap().modified().unwrap();
    assert_eq!(a_mtime_after, a_mtime, "no rewrite on a rerun");
    assert_eq!(cat.list_albums().unwrap(), albums_before, "no duplicate albums");
    assert_eq!(sidecar_of(&a).face_regions().len(), 1, "no duplicate faces");
}

#[test]
fn the_ini_files_are_never_modified_or_deleted() {
    let (lib, _data, mut cat, _a, _b) = library();
    let ini = lib.path().join(FOLDER_INI_FILE);
    let before = std::fs::read(&ini).unwrap();
    let mtime = std::fs::metadata(&ini).unwrap().modified().unwrap();

    cat.ingest_folder_ini(lib.path().to_str().unwrap()).unwrap();
    cat.ingest_folder_ini(lib.path().to_str().unwrap()).unwrap();

    assert_eq!(std::fs::read(&ini).unwrap(), before);
    assert_eq!(std::fs::metadata(&ini).unwrap().modified().unwrap(), mtime);
    let names: Vec<String> = std::fs::read_dir(lib.path())
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    assert!(!names.iter().any(|n| n.starts_with(&format!("{FOLDER_INI_FILE}."))), "{names:?}");
}

#[test]
fn originals_are_untouched() {
    let (lib, _data, mut cat, (_, a), _b) = library();
    let before = std::fs::read(&a).unwrap();
    cat.ingest_folder_ini(lib.path().to_str().unwrap()).unwrap();
    assert_eq!(std::fs::read(&a).unwrap(), before);
}

#[test]
fn existing_user_data_wins_and_unknown_xmp_survives() {
    let (lib, _data, mut cat, (_, a), _b) = library();
    let existing = r#"<?xml version="1.0" encoding="UTF-8"?>
<x:xmpmeta xmlns:x="adobe:ns:meta/">
 <rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
  <rdf:Description rdf:about=""
    xmlns:xmp="http://ns.adobe.com/xap/1.0/"
    xmlns:dc="http://purl.org/dc/elements/1.1/"
    xmlns:digiKam="http://www.digikam.org/ns/1.0/"
    xmp:Rating="3">
   <dc:description><rdf:Alt><rdf:li xml:lang="x-default">my own caption</rdf:li></rdf:Alt></dc:description>
   <dc:subject><rdf:Bag><rdf:li>sun</rdf:li><rdf:li>beach</rdf:li></rdf:Bag></dc:subject>
   <digiKam:Keep>unknown node</digiKam:Keep>
  </rdf:Description>
 </rdf:RDF>
</x:xmpmeta>
"#;
    std::fs::write(sidecar_path(a.to_str().unwrap()), existing).unwrap();

    cat.ingest_folder_ini(lib.path().to_str().unwrap()).unwrap();

    let sc = sidecar_of(&a);
    assert_eq!(sc.rating(), Some(3), "an existing rating is never overwritten");
    assert_eq!(sc.caption().as_deref(), Some("my own caption"));
    assert_eq!(sc.tags(), vec!["sun", "beach", "family"], "keywords are unioned");
    let raw = std::fs::read_to_string(sidecar_path(a.to_str().unwrap())).unwrap();
    assert!(raw.contains("digiKam:Keep"), "{raw}");
    let bak = std::fs::read_to_string(format!("{}.xmp.fb-bak", a.display())).unwrap();
    assert!(bak.contains("my own caption"), "previous version backed up");
}

#[test]
fn matching_a_section_to_a_photo_ignores_case() {
    let (lib, _data, mut cat, (_, a), (_, b)) = library();
    cat.ingest_folder_ini(lib.path().to_str().unwrap()).unwrap();
    assert_eq!(sidecar_of(&a).rating(), Some(5), "IMG_0001.JPG");
    assert_eq!(sidecar_of(&b).albums().len(), 2, "img_0002.jpg ↔ IMG_0002.jpg");
}

#[test]
fn a_photo_the_ini_does_not_mention_gets_no_sidecar() {
    let (lib, _data, mut cat, _a, _b) = library();
    let (_, c) = add_photo(&mut cat, lib.path(), "other.jpg");
    cat.ingest_folder_ini(lib.path().to_str().unwrap()).unwrap();
    assert!(sidecar_bytes(&c).is_none());
}

#[test]
fn migrated_data_survives_deleting_the_catalog() {
    let lib = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    let mut cat = open(data.path());
    let (_, a) = add_photo(&mut cat, lib.path(), "IMG_0001.JPG");
    add_photo(&mut cat, lib.path(), "IMG_0002.jpg");
    std::fs::write(lib.path().join(FOLDER_INI_FILE), INI).unwrap();
    cat.ingest_folder_ini(lib.path().to_str().unwrap()).unwrap();
    let a_bytes = sidecar_bytes(&a).unwrap();
    drop(cat);

    // Catalog gone; albums.json (the definitions) stays next to where it was.
    for name in ["catalog.db", "catalog.db-wal", "catalog.db-shm"] {
        let _ = std::fs::remove_file(data.path().join(name));
    }
    let mut fresh = open(data.path());
    add_photo(&mut fresh, lib.path(), "IMG_0001.JPG");
    add_photo(&mut fresh, lib.path(), "IMG_0002.jpg");
    fresh.rebuild_from_sidecars(true).unwrap();
    let a_id = fresh.file_id_by_path(a.to_str().unwrap()).unwrap().unwrap();
    assert_eq!(fresh.file_tags(a_id).unwrap(), vec!["beach", "family"]);
    assert_eq!(fresh.list_albums().unwrap().iter().filter(|x| x.name == "Italy 2009").count(), 1);

    // Running the migration again is still a no-op (albums keep their ids).
    let again = fresh.ingest_folder_ini(lib.path().to_str().unwrap()).unwrap();
    assert_eq!(again.written, 0, "{again:?}");
    assert_eq!(sidecar_bytes(&a).unwrap(), a_bytes);
}

#[test]
fn a_later_definition_names_a_placeholder_album() {
    let lib = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    let mut cat = open(data.path());
    add_photo(&mut cat, lib.path(), "a.jpg");
    std::fs::write(lib.path().join(FOLDER_INI_FILE), "[a.jpg]\nalbums=tok\n").unwrap();
    cat.ingest_folder_ini(lib.path().to_str().unwrap()).unwrap();
    assert_eq!(cat.list_albums().unwrap()[0].name, "Imported album");

    std::fs::write(
        lib.path().join(FOLDER_INI_FILE),
        "[a.jpg]\nalbums=tok\n[.album:tok]\nname=Real name\n",
    )
    .unwrap();
    cat.ingest_folder_ini(lib.path().to_str().unwrap()).unwrap();
    let albums = cat.list_albums().unwrap();
    assert_eq!(albums.len(), 1);
    assert_eq!(albums[0].name, "Real name");
}

#[test]
fn both_ini_spellings_are_read() {
    let lib = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    let mut cat = open(data.path());
    let (_, a) = add_photo(&mut cat, lib.path(), "a.jpg");
    let (_, b) = add_photo(&mut cat, lib.path(), "b.jpg");
    std::fs::write(lib.path().join(FOLDER_INI_FILE), "[a.jpg]\nstar=yes\n").unwrap();
    std::fs::write(lib.path().join(FOLDER_INI_HIDDEN_FILE), "[b.jpg]\nstar=yes\n").unwrap();
    let report = cat.ingest_folder_ini(lib.path().to_str().unwrap()).unwrap();
    assert_eq!(report.ini_files, 2);
    assert_eq!(sidecar_of(&a).rating(), Some(5));
    assert_eq!(sidecar_of(&b).rating(), Some(5));
}

fn png_header(w: u32, h: u32) -> Vec<u8> {
    let mut b = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".to_vec();
    b.extend(w.to_be_bytes());
    b.extend(h.to_be_bytes());
    b.extend([8, 2, 0, 0, 0]);
    b
}

#[test]
fn face_regions_record_the_image_size_when_it_can_be_read() {
    let lib = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    let mut cat = open(data.path());
    let (_, a) = add_photo_bytes(&mut cat, lib.path(), "a.png", &png_header(40, 30));
    std::fs::write(
        lib.path().join(FOLDER_INI_FILE),
        "[a.png]\nfaces=rect64(2000400060008000),01\n[Contacts2]\n01=Anna;\n",
    )
    .unwrap();
    cat.ingest_folder_ini(lib.path().to_str().unwrap()).unwrap();
    let raw = std::fs::read_to_string(sidecar_path(a.to_str().unwrap())).unwrap();
    assert!(raw.contains(r#"stDim:w="40""#) && raw.contains(r#"stDim:h="30""#), "{raw}");
}

#[test]
fn one_unreadable_sidecar_is_reported_and_the_rest_still_migrate() {
    let (lib, _data, mut cat, (_, a), (_, b)) = library();
    std::fs::write(sidecar_path(a.to_str().unwrap()), "<not-xmp").unwrap();
    let report = cat.ingest_folder_ini(lib.path().to_str().unwrap()).unwrap();
    assert_eq!(report.failed, vec!["IMG_0001.JPG"]);
    assert_eq!(sidecar_of(&b).albums().len(), 2);
    assert_eq!(
        std::fs::read_to_string(sidecar_path(a.to_str().unwrap())).unwrap(),
        "<not-xmp",
        "a malformed sidecar is never overwritten"
    );
    assert_eq!(
        cat.folder_ini_status(lib.path().to_str().unwrap()).unwrap(),
        None,
        "a run with failures stays open for a retry"
    );
}

// ---- offer / decline state ---------------------------------------------------

#[test]
fn offering_marks_a_folder_pending_once() {
    let (lib, _data, mut cat, _a, _b) = library();
    let dir = lib.path().to_str().unwrap();
    assert!(cat.folder_ini_pending_folders().unwrap().is_empty());
    assert!(cat.offer_folder_ini_ingest(dir).unwrap(), "first sighting");
    assert!(!cat.offer_folder_ini_ingest(dir).unwrap(), "already offered");
    assert_eq!(cat.folder_ini_pending_folders().unwrap(), vec![dir.to_string()]);
    assert_eq!(cat.folder_ini_status(dir).unwrap(), Some(FolderIniStatus::Pending));
}

#[test]
fn declining_is_per_folder_and_sticky() {
    let (lib, _data, mut cat, _a, _b) = library();
    let dir = lib.path().to_str().unwrap();
    cat.offer_folder_ini_ingest(dir).unwrap();
    cat.decline_folder_ini_ingest(dir).unwrap();
    assert_eq!(cat.folder_ini_status(dir).unwrap(), Some(FolderIniStatus::Declined));
    assert!(cat.folder_ini_pending_folders().unwrap().is_empty());
    assert!(!cat.offer_folder_ini_ingest(dir).unwrap(), "a scan never re-offers a declined folder");
    assert_eq!(cat.folder_ini_status(dir).unwrap(), Some(FolderIniStatus::Declined));
}

#[test]
fn ingesting_marks_the_folder_done_and_leaves_no_pending_offer() {
    let (lib, _data, mut cat, _a, _b) = library();
    let dir = lib.path().to_str().unwrap();
    cat.offer_folder_ini_ingest(dir).unwrap();
    cat.ingest_folder_ini(dir).unwrap();
    assert_eq!(cat.folder_ini_status(dir).unwrap(), Some(FolderIniStatus::Done));
    assert!(cat.folder_ini_pending_folders().unwrap().is_empty());
    assert!(!cat.offer_folder_ini_ingest(dir).unwrap());
}

#[test]
fn ingesting_an_uncatalogued_folder_is_an_error() {
    let dir = tempfile::tempdir().unwrap();
    let mut cat = open(dir.path());
    assert!(cat.ingest_folder_ini("/nowhere").is_err());
}
