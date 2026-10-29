//! TASK-03-022: folder ini parser (stars, albums, faces) and the percent →
//! MWG region conversion (PLAN §5 mapping).

use fotobase_metadata::{
    folder_ini_files, FaceRect, FolderIni, FOLDER_INI_FILE, FOLDER_INI_HIDDEN_FILE, UNNAMED_CONTACT,
};

const INI: &str = "\u{feff}[Folder]\r
name=Vacation\r
P2category=Folders on Disk\r
\r
[IMG_0001.JPG]\r
star=yes\r
caption=At the beach\r
keywords=beach, family ,sun\r
albums=aaa111,bbb222\r
faces=rect64(2000400060008000),c0ffee0000000001;rect64(123456789abc),c0ffee0000000002;rect64(1000100020002000),ffffffffffffffff\r
backuphash=12345\r
\r
[img_0002.jpg]\r
keywords=\r
backuphash=999\r
\r
[IMG_0003.JPG]\r
star=yes\r
\r
[.album:aaa111]\r
name=Italy 2009\r
token=aaa111\r
P2category=Albums\r
date=2009-04-14T21:05:42+00:00\r
location=Rome\r
description=Spring trip\r
\r
[.album:bbb222]\r
name=Serial date\r
token=bbb222\r
date=39917.8789\r
\r
[Contacts2]\r
c0ffee0000000001=Anna Smith;anna@example.com\r
c0ffee0000000002=Bob;\r
";

#[test]
fn parses_stars_captions_keywords_and_album_tokens() {
    let ini = FolderIni::parse(INI);
    let p = ini.photo("IMG_0001.JPG").expect("IMG_0001 present");
    assert!(p.star);
    assert_eq!(p.caption.as_deref(), Some("At the beach"));
    assert_eq!(p.keywords, vec!["beach", "family", "sun"]);
    assert_eq!(p.albums, vec!["aaa111", "bbb222"]);
}

#[test]
fn photo_sections_without_migratable_data_are_dropped() {
    let ini = FolderIni::parse(INI);
    assert!(ini.photo("img_0002.jpg").is_none());
    assert!(ini.photo("IMG_0003.JPG").is_some(), "a star alone is data");
}

#[test]
fn photo_lookup_is_case_insensitive() {
    let ini = FolderIni::parse(INI);
    assert!(ini.photo("img_0001.jpg").is_some());
    assert!(ini.photo("IMG_0001.jpg").is_some());
}

#[test]
fn star_only_counts_when_yes() {
    let ini = FolderIni::parse("[a.jpg]\nstar=no\ncaption=x\n[b.jpg]\nstar=YES\n");
    assert!(!ini.photo("a.jpg").unwrap().star);
    assert!(ini.photo("b.jpg").unwrap().star);
}

#[test]
fn faces_resolve_names_through_contacts_and_pad_short_hex() {
    let ini = FolderIni::parse(INI);
    let faces = &ini.photo("IMG_0001.JPG").unwrap().faces;
    assert_eq!(faces.len(), 3);

    assert_eq!(faces[0].name.as_deref(), Some("Anna Smith"));
    let r = faces[0].rect;
    assert!((r.left - 0x2000 as f64 / 65535.0).abs() < 1e-9);
    assert!((r.top - 0x4000 as f64 / 65535.0).abs() < 1e-9);
    assert!((r.right - 0x6000 as f64 / 65535.0).abs() < 1e-9);
    assert!((r.bottom - 0x8000 as f64 / 65535.0).abs() < 1e-9);

    // Leading zeros are stripped by the writer: 12 hex digits = 0x0000 1234 5678 9abc.
    assert_eq!(faces[1].name.as_deref(), Some("Bob"));
    assert_eq!(faces[1].rect.left, 0.0);
    assert!((faces[1].rect.top - 0x1234 as f64 / 65535.0).abs() < 1e-9);
    assert!((faces[1].rect.bottom - 0x9abc as f64 / 65535.0).abs() < 1e-9);

    // The all-ones contact is the "not named" marker.
    assert_eq!(faces[2].name, None);
    assert_eq!(UNNAMED_CONTACT, "ffffffffffffffff");
}

#[test]
fn a_contact_missing_from_the_contact_tables_has_no_name() {
    let ini = FolderIni::parse("[a.jpg]\nfaces=rect64(2000400060008000),deadbeef00000000\n");
    assert_eq!(ini.photo("a.jpg").unwrap().faces[0].name, None);
}

#[test]
fn old_contacts_section_is_read_too() {
    let ini = FolderIni::parse(
        "[a.jpg]\nfaces=rect64(2000400060008000),01\n[Contacts]\n01=Old Name\n[Contacts2]\n",
    );
    assert_eq!(ini.photo("a.jpg").unwrap().faces[0].name.as_deref(), Some("Old Name"));
}

#[test]
fn malformed_faces_are_skipped_not_fatal() {
    let ini = FolderIni::parse(
        "[a.jpg]\nstar=yes\nfaces=garbage;rect64(zz),01;rect64(2000400060008000),01;rect64()\n[Contacts2]\n01=Z;\n",
    );
    let faces = &ini.photo("a.jpg").unwrap().faces;
    assert_eq!(faces.len(), 1);
    assert_eq!(faces[0].name.as_deref(), Some("Z"));
}

#[test]
fn album_definitions_read_name_date_place_description() {
    let ini = FolderIni::parse(INI);
    assert_eq!(ini.albums.len(), 2);
    let a = ini.albums.iter().find(|a| a.token == "aaa111").unwrap();
    assert_eq!(a.name, "Italy 2009");
    assert_eq!(a.date.as_deref(), Some("2009-04-14"));
    assert_eq!(a.place.as_deref(), Some("Rome"));
    assert_eq!(a.description.as_deref(), Some("Spring trip"));
}

#[test]
fn album_serial_dates_convert_to_calendar_dates() {
    // Days since 1899-12-30 (the spreadsheet epoch used for old dates).
    let ini = FolderIni::parse(INI);
    let b = ini.albums.iter().find(|a| a.token == "bbb222").unwrap();
    assert_eq!(b.date.as_deref(), Some("2009-04-14"));
}

#[test]
fn album_without_a_token_line_uses_the_section_name() {
    let ini = FolderIni::parse("[.album:zzz]\nname=No token key\n");
    assert_eq!(ini.albums[0].token, "zzz");
}

#[test]
fn unknown_keys_sections_and_junk_lines_are_ignored() {
    let ini = FolderIni::parse(
        "garbage before any section\n[a.jpg]\nP2WS=whatever\nrotate=rotate(1)\nfilters=enhance=1\nstar=yes\n=novalue\n",
    );
    assert!(ini.photo("a.jpg").unwrap().star);
}

#[test]
fn empty_input_parses_to_nothing() {
    let ini = FolderIni::parse("");
    assert!(ini.photos.is_empty() && ini.albums.is_empty());
}

#[test]
fn percent_rect_converts_to_mwg_centre_point_area() {
    let rect = FaceRect { left: 0.2, top: 0.1, right: 0.6, bottom: 0.5 };
    let region = rect.to_region("Anna");
    assert_eq!(region.name, "Anna");
    assert!((region.x - 0.4).abs() < 1e-12, "centre x");
    assert!((region.y - 0.3).abs() < 1e-12, "centre y");
    assert!((region.w - 0.4).abs() < 1e-12);
    assert!((region.h - 0.4).abs() < 1e-12);
}

#[test]
fn degenerate_or_reversed_rects_are_rejected() {
    assert!(FaceRect::from_rect64("6000800020004000").is_none(), "right < left");
    assert!(FaceRect::from_rect64("2000400020008000").is_none(), "zero width");
    assert!(FaceRect::from_rect64("not hex").is_none());
    assert!(FaceRect::from_rect64("00000000000000001").is_none(), "17 digits");
}

#[test]
fn folder_ini_files_finds_both_spellings_case_insensitively() {
    let dir = tempfile::tempdir().unwrap();
    assert!(folder_ini_files(dir.path()).is_empty());
    std::fs::write(dir.path().join(FOLDER_INI_FILE), "[Folder]\n").unwrap();
    std::fs::write(dir.path().join(FOLDER_INI_HIDDEN_FILE), "[Folder]\n").unwrap();
    std::fs::write(dir.path().join("folderIni.ini.bak"), "x").unwrap();
    std::fs::write(dir.path().join("photo.jpg"), "x").unwrap();
    let found = folder_ini_files(dir.path());
    assert_eq!(found.len(), 2, "{found:?}");
}
