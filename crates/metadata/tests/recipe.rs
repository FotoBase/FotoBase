//! TASK-04-002: edit recipes stored in `fb:Recipe` XMP (docs/PLAN §5,
//! docs/sidecar-merge-on-write.md §2). Merge-on-write: a recipe write must
//! not disturb any other field, known or unknown, and leaves a `.fb-bak`
//! when it replaces an existing sidecar.

use fotobase_metadata::{Sidecar, FB_NS};

const RECIPE: &str =
    r#"{"spec":"fotobase.recipe","version":1,"ops":[{"type":"sepia","params":{"amount":0.5}}]}"#;

fn sidecar_with(dir: &std::path::Path, body: &str) -> std::path::PathBuf {
    let path = dir.join("photo.jpg.xmp");
    let xmp = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<x:xmpmeta xmlns:x="adobe:ns:meta/">
 <rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
  <rdf:Description rdf:about=""
    xmlns:xmp="http://ns.adobe.com/xap/1.0/"
    xmlns:other="http://example.com/other/1.0/">
{body}
  </rdf:Description>
 </rdf:RDF>
</x:xmpmeta>
"#
    );
    std::fs::write(&path, xmp).unwrap();
    path
}

#[test]
fn a_fresh_sidecar_has_no_recipe() {
    let dir = tempfile::tempdir().unwrap();
    let sc = Sidecar::read(&dir.path().join("photo.jpg.xmp")).unwrap();
    assert!(!sc.has_recipe());
    assert_eq!(sc.recipe(), None);
}

#[test]
fn setting_a_recipe_writes_fb_recipe_and_reads_back_verbatim() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("photo.jpg.xmp");

    let mut sc = Sidecar::read(&path).unwrap();
    sc.set_recipe(RECIPE);
    sc.write(&path).unwrap();

    let raw = std::fs::read_to_string(&path).unwrap();
    assert!(raw.contains(&format!(r#"xmlns:fb="{FB_NS}""#)), "{raw}");
    assert!(raw.contains("<fb:Recipe>"), "{raw}");
    let sc = Sidecar::read(&path).unwrap();
    assert!(sc.has_recipe());
    assert_eq!(sc.recipe().as_deref(), Some(RECIPE));
}

#[test]
fn replacing_a_recipe_replaces_it_without_duplicating_the_field() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("photo.jpg.xmp");
    let mut sc = Sidecar::read(&path).unwrap();
    sc.set_recipe(r#"{"spec":"fotobase.recipe","version":1,"ops":[]}"#);
    sc.write(&path).unwrap();

    let mut sc = Sidecar::read(&path).unwrap();
    sc.set_recipe(RECIPE);
    sc.write(&path).unwrap();

    let raw = std::fs::read_to_string(&path).unwrap();
    assert_eq!(raw.matches("fb:Recipe>").count(), 2, "one element: {raw}");
    assert_eq!(Sidecar::read(&path).unwrap().recipe().as_deref(), Some(RECIPE));
}

#[test]
fn clearing_removes_the_recipe_and_keeps_everything_else() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("photo.jpg.xmp");
    let mut sc = Sidecar::read(&path).unwrap();
    sc.set_rating(4);
    sc.set_tags(vec!["trip".to_string()]);
    sc.set_recipe(RECIPE);
    sc.write(&path).unwrap();

    let mut sc = Sidecar::read(&path).unwrap();
    sc.clear_recipe();
    sc.write(&path).unwrap();

    let raw = std::fs::read_to_string(&path).unwrap();
    assert!(!raw.contains("fb:Recipe"), "{raw}");
    let sc = Sidecar::read(&path).unwrap();
    assert!(!sc.has_recipe());
    assert_eq!(sc.rating(), Some(4));
    assert_eq!(sc.tags(), vec!["trip"]);
}

#[test]
fn a_recipe_write_preserves_unknown_nodes_and_other_fields_verbatim() {
    let dir = tempfile::tempdir().unwrap();
    let path = sidecar_with(
        dir.path(),
        r#"    <other:Keep>precious</other:Keep>
    <xmp:Rating>3</xmp:Rating>"#,
    );

    let mut sc = Sidecar::read(&path).unwrap();
    sc.set_recipe(RECIPE);
    sc.write(&path).unwrap();

    let raw = std::fs::read_to_string(&path).unwrap();
    assert!(raw.contains("<other:Keep>precious</other:Keep>"), "{raw}");
    let sc = Sidecar::read(&path).unwrap();
    assert_eq!(sc.rating(), Some(3));
    assert_eq!(sc.recipe().as_deref(), Some(RECIPE));
}

#[test]
fn recipe_json_with_markup_characters_survives_a_round_trip() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("photo.jpg.xmp");
    let recipe = r#"{"spec":"fotobase.recipe","version":1,"ops":[{"type":"text","params":{"text":"a<b&c>d"}}]}"#;

    let mut sc = Sidecar::read(&path).unwrap();
    sc.set_recipe(recipe);
    sc.write(&path).unwrap();

    assert_eq!(Sidecar::read(&path).unwrap().recipe().as_deref(), Some(recipe));
}

#[test]
fn rewriting_a_recipe_leaves_a_backup_of_the_previous_sidecar() {
    let dir = tempfile::tempdir().unwrap();
    let path = sidecar_with(dir.path(), r#"    <xmp:Rating>2</xmp:Rating>"#);

    let mut sc = Sidecar::read(&path).unwrap();
    sc.set_recipe(RECIPE);
    sc.write(&path).unwrap();

    let bak = path.with_file_name("photo.jpg.xmp.fb-bak");
    assert!(bak.exists(), "backup must exist");
    let before = std::fs::read_to_string(&bak).unwrap();
    assert!(before.contains("<xmp:Rating>2</xmp:Rating>"), "{before}");
    assert!(!before.contains("fb:Recipe"), "{before}");

    // A second write refreshes the backup to the previous state.
    let mut sc = Sidecar::read(&path).unwrap();
    sc.set_recipe(r#"{"spec":"fotobase.recipe","version":1,"ops":[]}"#);
    sc.write(&path).unwrap();
    let again = std::fs::read_to_string(&bak).unwrap();
    assert!(again.contains("fb:Recipe"), "{again}");
}
