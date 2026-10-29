//! TASK-03-021 / ADR-0006: stack membership is `fb:StackId` (UUID) plus
//! `fb:StackRole` (`top`/`member`) in each member's own sidecar, merged in
//! without disturbing anything else (docs/sidecar-merge-on-write.md §2/§3).

use std::path::Path;

use fotobase_metadata::{Sidecar, StackMembership, StackRole, FB_NS};

const STACK: &str = "7d3c6a52-5a0e-4b3e-9a55-0d6a9f0f1a11";

fn sidecar_with(dir: &Path, body: &str) -> std::path::PathBuf {
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

fn membership(role: StackRole) -> StackMembership {
    StackMembership { id: STACK.to_string(), role }
}

#[test]
fn a_fresh_sidecar_is_in_no_stack() {
    let dir = tempfile::tempdir().unwrap();
    let sc = Sidecar::read(&dir.path().join("photo.jpg.xmp")).unwrap();
    assert_eq!(sc.stack(), None);
}

#[test]
fn set_stack_writes_both_fields_in_the_fb_namespace_and_reads_back() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("photo.jpg.xmp");

    let mut sc = Sidecar::read(&path).unwrap();
    sc.set_stack(&membership(StackRole::Top));
    sc.write(&path).unwrap();

    let raw = std::fs::read_to_string(&path).unwrap();
    assert!(raw.contains(&format!(r#"xmlns:fb="{FB_NS}""#)), "{raw}");
    assert!(raw.contains(&format!("<fb:StackId>{STACK}</fb:StackId>")), "{raw}");
    assert!(raw.contains("<fb:StackRole>top</fb:StackRole>"), "{raw}");
    assert_eq!(Sidecar::read(&path).unwrap().stack(), Some(membership(StackRole::Top)));
}

#[test]
fn switching_the_role_replaces_it_without_duplicating_the_fields() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("photo.jpg.xmp");
    let mut sc = Sidecar::read(&path).unwrap();
    sc.set_stack(&membership(StackRole::Top));
    sc.write(&path).unwrap();

    let mut sc = Sidecar::read(&path).unwrap();
    sc.set_stack(&membership(StackRole::Member));
    sc.write(&path).unwrap();

    let raw = std::fs::read_to_string(&path).unwrap();
    assert_eq!(raw.matches("fb:StackRole>").count(), 2, "one element: {raw}");
    assert_eq!(raw.matches("fb:StackId>").count(), 2, "one element: {raw}");
    assert_eq!(Sidecar::read(&path).unwrap().stack(), Some(membership(StackRole::Member)));
}

#[test]
fn clearing_removes_both_fields_and_keeps_everything_else() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("photo.jpg.xmp");
    let mut sc = Sidecar::read(&path).unwrap();
    sc.set_rating(4);
    sc.set_tags(vec!["trip".to_string()]);
    sc.set_stack(&membership(StackRole::Member));
    sc.write(&path).unwrap();

    let mut sc = Sidecar::read(&path).unwrap();
    sc.clear_stack();
    sc.write(&path).unwrap();

    let raw = std::fs::read_to_string(&path).unwrap();
    assert!(!raw.contains("fb:Stack"), "{raw}");
    let sc = Sidecar::read(&path).unwrap();
    assert_eq!(sc.stack(), None);
    assert_eq!(sc.rating(), Some(4));
    assert_eq!(sc.tags(), vec!["trip"]);
}

#[test]
fn stacking_preserves_unknown_nodes_verbatim() {
    let dir = tempfile::tempdir().unwrap();
    let path = sidecar_with(dir.path(), r#"    <other:Keep>precious</other:Keep>"#);

    let mut sc = Sidecar::read(&path).unwrap();
    sc.set_stack(&membership(StackRole::Top));
    sc.write(&path).unwrap();

    let raw = std::fs::read_to_string(&path).unwrap();
    assert!(raw.contains("<other:Keep>precious</other:Keep>"), "{raw}");
    assert!(Sidecar::read(&path).unwrap().stack().is_some());
}

#[test]
fn an_id_without_a_role_reads_as_a_plain_member() {
    let dir = tempfile::tempdir().unwrap();
    let path = sidecar_with(
        dir.path(),
        &format!(r#"    <fb:StackId xmlns:fb="{FB_NS}">{STACK}</fb:StackId>"#),
    );
    assert_eq!(Sidecar::read(&path).unwrap().stack(), Some(membership(StackRole::Member)));
}

#[test]
fn a_role_without_an_id_is_no_membership() {
    let dir = tempfile::tempdir().unwrap();
    let path = sidecar_with(
        dir.path(),
        &format!(r#"    <fb:StackRole xmlns:fb="{FB_NS}">top</fb:StackRole>"#),
    );
    assert_eq!(Sidecar::read(&path).unwrap().stack(), None);
}

#[test]
fn an_unknown_role_text_reads_as_a_plain_member() {
    let dir = tempfile::tempdir().unwrap();
    let path = sidecar_with(
        dir.path(),
        &format!(
            r#"    <fb:StackId xmlns:fb="{FB_NS}">{STACK}</fb:StackId>
    <fb:StackRole xmlns:fb="{FB_NS}">cover</fb:StackRole>"#
        ),
    );
    assert_eq!(Sidecar::read(&path).unwrap().stack(), Some(membership(StackRole::Member)));
}

#[test]
fn the_recipe_marker_is_readable_for_edited_version_detection() {
    let dir = tempfile::tempdir().unwrap();
    let plain = sidecar_with(dir.path(), "");
    assert!(!Sidecar::read(&plain).unwrap().has_recipe());

    let edited = sidecar_with(
        dir.path(),
        &format!(r#"    <fb:Recipe xmlns:fb="{FB_NS}">{{"version":1,"ops":[]}}</fb:Recipe>"#),
    );
    assert!(Sidecar::read(&edited).unwrap().has_recipe());
}

#[test]
fn the_recipe_json_text_is_readable_verbatim_for_portable_export() {
    let dir = tempfile::tempdir().unwrap();
    let plain = sidecar_with(dir.path(), "");
    assert_eq!(Sidecar::read(&plain).unwrap().recipe(), None);

    let edited = sidecar_with(
        dir.path(),
        &format!(
            r#"    <fb:Recipe xmlns:fb="{FB_NS}">{{"spec":"fotobase.recipe","version":1,"ops":[{{"type":"sepia"}}]}}</fb:Recipe>"#
        ),
    );
    assert_eq!(
        Sidecar::read(&edited).unwrap().recipe().as_deref(),
        Some(r#"{"spec":"fotobase.recipe","version":1,"ops":[{"type":"sepia"}]}"#)
    );
}
