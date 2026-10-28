//! TASK-03-009 / ADR-0007: the hidden flag. A photo carries `fb:Hidden` in
//! its own sidecar; a folder carries `{"hidden": true}` in one `.fotobase`
//! JSON file (docs/sidecar-merge-on-write.md §2). Both writes merge, keep
//! what they do not understand and back up the previous version.

use fotobase_metadata::{FolderMeta, Sidecar, FB_NS, FOLDER_META_FILE};

#[test]
fn a_fresh_sidecar_is_not_hidden() {
    let dir = tempfile::tempdir().unwrap();
    let sc = Sidecar::read(&dir.path().join("photo.jpg.xmp")).unwrap();
    assert!(!sc.hidden());
}

#[test]
fn set_hidden_writes_fb_hidden_and_reads_back() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("photo.jpg.xmp");

    let mut sc = Sidecar::read(&path).unwrap();
    sc.set_hidden(true);
    sc.write(&path).unwrap();

    let raw = std::fs::read_to_string(&path).unwrap();
    assert!(raw.contains(&format!(r#"xmlns:fb="{FB_NS}""#)), "{raw}");
    assert!(raw.contains("<fb:Hidden>True</fb:Hidden>"), "{raw}");
    assert!(Sidecar::read(&path).unwrap().hidden());
}

#[test]
fn unhiding_removes_the_property_and_keeps_everything_else() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("photo.jpg.xmp");
    let mut sc = Sidecar::read(&path).unwrap();
    sc.set_rating(4);
    sc.set_tags(vec!["trip".to_string()]);
    sc.set_hidden(true);
    sc.write(&path).unwrap();

    let mut sc = Sidecar::read(&path).unwrap();
    sc.set_hidden(false);
    sc.write(&path).unwrap();

    let raw = std::fs::read_to_string(&path).unwrap();
    assert!(!raw.contains("fb:Hidden"), "{raw}");
    let sc = Sidecar::read(&path).unwrap();
    assert!(!sc.hidden());
    assert_eq!(sc.rating(), Some(4));
    assert_eq!(sc.tags(), vec!["trip"]);
}

#[test]
fn hidden_accepts_the_xmp_boolean_spellings() {
    let dir = tempfile::tempdir().unwrap();
    for (text, want) in [("True", true), ("true", true), ("1", true), ("False", false), ("", false)]
    {
        let path = dir.path().join("p.jpg.xmp");
        std::fs::write(
            &path,
            format!(
                r#"<?xml version="1.0"?>
<x:xmpmeta xmlns:x="adobe:ns:meta/">
 <rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
  <rdf:Description rdf:about="" xmlns:fb="{FB_NS}"><fb:Hidden>{text}</fb:Hidden></rdf:Description>
 </rdf:RDF>
</x:xmpmeta>"#
            ),
        )
        .unwrap();
        assert_eq!(Sidecar::read(&path).unwrap().hidden(), want, "{text:?}");
    }
}

#[test]
fn a_folder_without_the_file_is_not_hidden_and_reading_creates_nothing() {
    let dir = tempfile::tempdir().unwrap();
    assert!(!FolderMeta::read(dir.path()).unwrap().hidden);
    assert!(!dir.path().join(FOLDER_META_FILE).exists());
}

#[test]
fn set_folder_hidden_writes_the_json_flag() {
    let dir = tempfile::tempdir().unwrap();
    FolderMeta::set_hidden(dir.path(), true).unwrap();

    let raw = std::fs::read_to_string(dir.path().join(FOLDER_META_FILE)).unwrap();
    let json: serde_json::Value = serde_json::from_str(&raw).unwrap();
    assert_eq!(json["hidden"], true);
    assert_eq!(json["version"], 1);
    assert!(FolderMeta::read(dir.path()).unwrap().hidden);
}

#[test]
fn unhiding_an_unmarked_folder_does_not_create_the_file() {
    let dir = tempfile::tempdir().unwrap();
    FolderMeta::set_hidden(dir.path(), false).unwrap();
    assert!(!dir.path().join(FOLDER_META_FILE).exists());
}

#[test]
fn folder_write_preserves_unknown_fields_and_backs_up() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join(FOLDER_META_FILE);
    let before = r#"{"version":1,"hidden":false,"future":{"a":[1,2]}}"#;
    std::fs::write(&file, before).unwrap();

    FolderMeta::set_hidden(dir.path(), true).unwrap();

    let json: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
    assert_eq!(json["hidden"], true);
    assert_eq!(json["future"]["a"][1], 2);
    let bak = dir.path().join(format!("{FOLDER_META_FILE}.fb-bak"));
    assert_eq!(std::fs::read_to_string(bak).unwrap(), before);
    assert!(!dir.path().join(format!("{FOLDER_META_FILE}.tmp")).exists());
}

#[test]
fn a_malformed_folder_file_is_an_error_and_is_never_overwritten() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join(FOLDER_META_FILE);
    std::fs::write(&file, "{ not json").unwrap();

    assert!(FolderMeta::read(dir.path()).is_err());
    assert!(FolderMeta::set_hidden(dir.path(), true).is_err());
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "{ not json");
}
