//! TASK-03-008: album membership lives in the custom `fb:Albums` bag (album
//! UUIDs), merged into the existing sidecar without disturbing anything else
//! (docs/sidecar-merge-on-write.md §2/§3).

use std::path::Path;

use fotobase_metadata::{Sidecar, FB_NS};

const A: &str = "7d3c6a52-5a0e-4b3e-9a55-0d6a9f0f1a11";
const B: &str = "0f1e2d3c-4b5a-4968-8776-a5b4c3d2e1f0";

fn sidecar_with(dir: &Path, body: &str) -> std::path::PathBuf {
    let path = dir.join("photo.jpg.xmp");
    let xmp = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<x:xmpmeta xmlns:x="adobe:ns:meta/">
 <rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
  <rdf:Description rdf:about=""
    xmlns:dc="http://purl.org/dc/elements/1.1/"
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
fn a_fresh_sidecar_has_no_albums() {
    let dir = tempfile::tempdir().unwrap();
    let sc = Sidecar::read(&dir.path().join("photo.jpg.xmp")).unwrap();
    assert!(sc.albums().is_empty());
}

#[test]
fn set_albums_writes_a_bag_in_the_fb_namespace() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("photo.jpg.xmp");

    let mut sc = Sidecar::read(&path).unwrap();
    sc.set_albums(vec![A.to_string(), B.to_string()]);
    sc.write(&path).unwrap();

    let raw = std::fs::read_to_string(&path).unwrap();
    assert!(raw.contains(&format!(r#"xmlns:fb="{FB_NS}""#)), "{raw}");
    assert!(raw.contains("<fb:Albums>"), "{raw}");
    assert!(raw.contains("<rdf:Bag>"), "{raw}");
    assert!(raw.contains(&format!("<rdf:li>{A}</rdf:li>")), "{raw}");
    assert_eq!(Sidecar::read(&path).unwrap().albums(), vec![A, B]);
}

#[test]
fn set_albums_replaces_the_previous_membership() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("photo.jpg.xmp");
    let mut sc = Sidecar::read(&path).unwrap();
    sc.set_albums(vec![A.to_string(), B.to_string()]);
    sc.write(&path).unwrap();

    let mut sc = Sidecar::read(&path).unwrap();
    sc.set_albums(vec![B.to_string()]);
    sc.write(&path).unwrap();

    let raw = std::fs::read_to_string(&path).unwrap();
    assert!(!raw.contains(A), "{raw}");
    assert_eq!(Sidecar::read(&path).unwrap().albums(), vec![B]);
    assert_eq!(raw.matches("<fb:Albums>").count(), 1, "{raw}");
}

#[test]
fn an_empty_membership_removes_the_property() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("photo.jpg.xmp");
    let mut sc = Sidecar::read(&path).unwrap();
    sc.set_albums(vec![A.to_string()]);
    sc.write(&path).unwrap();

    let mut sc = Sidecar::read(&path).unwrap();
    sc.set_albums(Vec::new());
    sc.write(&path).unwrap();

    let raw = std::fs::read_to_string(&path).unwrap();
    assert!(!raw.contains("fb:Albums"), "{raw}");
    assert!(Sidecar::read(&path).unwrap().albums().is_empty());
}

#[test]
fn album_writes_keep_unknown_nodes_and_other_user_data() {
    let dir = tempfile::tempdir().unwrap();
    let path = sidecar_with(
        dir.path(),
        r#"    <xmp:Rating>5</xmp:Rating>
    <other:Secret>keep me</other:Secret>
    <dc:subject><rdf:Bag><rdf:li>sea</rdf:li></rdf:Bag></dc:subject>"#,
    );

    let mut sc = Sidecar::read(&path).unwrap();
    sc.set_albums(vec![A.to_string()]);
    sc.write(&path).unwrap();

    let raw = std::fs::read_to_string(&path).unwrap();
    assert!(raw.contains("<other:Secret>keep me</other:Secret>"), "{raw}");
    let sc = Sidecar::read(&path).unwrap();
    assert_eq!(sc.rating(), Some(5));
    assert_eq!(sc.tags(), vec!["sea"]);
    assert_eq!(sc.albums(), vec![A]);
}

#[test]
fn foreign_album_ids_survive_a_membership_edit() {
    // Another Fotobase install (or a future version) may have written IDs we
    // do not know; set_albums is a plain replace, the *caller* merges. The
    // sidecar layer must still read every id verbatim.
    let dir = tempfile::tempdir().unwrap();
    let path = sidecar_with(
        dir.path(),
        &format!(
            r#"    <fb:Albums xmlns:fb="{FB_NS}"><rdf:Bag><rdf:li>{A}</rdf:li><rdf:li>{B}</rdf:li></rdf:Bag></fb:Albums>"#
        ),
    );
    let sc = Sidecar::read(&path).unwrap();
    assert_eq!(sc.albums(), vec![A, B]);
}

#[test]
fn the_fb_namespace_is_declared_once() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("photo.jpg.xmp");
    for id in [A, B] {
        let mut sc = Sidecar::read(&path).unwrap();
        let mut albums = sc.albums();
        albums.push(id.to_string());
        sc.set_albums(albums);
        sc.write(&path).unwrap();
    }
    let raw = std::fs::read_to_string(&path).unwrap();
    assert_eq!(raw.matches("xmlns:fb=").count(), 1, "{raw}");
}
