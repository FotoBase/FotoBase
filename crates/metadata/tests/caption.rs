//! TASK-03-007: captions live in `dc:description` as an XMP language
//! alternative (the form darktable/digiKam/exiv2 read and write), merged into
//! the existing sidecar without disturbing anything else.

use std::path::Path;

use fotobase_metadata::Sidecar;

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
fn set_caption_writes_a_language_alternative() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("photo.jpg.xmp");

    let mut sc = Sidecar::read(&path).unwrap();
    sc.set_caption("sunset over the bay");
    sc.write(&path).unwrap();

    let raw = std::fs::read_to_string(&path).unwrap();
    assert!(raw.contains("<dc:description>"), "{raw}");
    assert!(raw.contains("<rdf:Alt>"), "{raw}");
    assert!(raw.contains(r#"<rdf:li xml:lang="x-default">sunset over the bay</rdf:li>"#), "{raw}");
    assert_eq!(Sidecar::read(&path).unwrap().caption().as_deref(), Some("sunset over the bay"));
}

#[test]
fn multi_line_and_markup_characters_round_trip() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("photo.jpg.xmp");
    let caption = "line one\nline two & <three>\n\nlast";

    let mut sc = Sidecar::read(&path).unwrap();
    sc.set_caption(caption);
    sc.write(&path).unwrap();

    assert_eq!(Sidecar::read(&path).unwrap().caption().as_deref(), Some(caption));
}

#[test]
fn set_caption_replaces_x_default_and_keeps_other_languages() {
    let dir = tempfile::tempdir().unwrap();
    let path = sidecar_with(
        dir.path(),
        r#"<dc:description><rdf:Alt>
     <rdf:li xml:lang="fr">coucher de soleil</rdf:li>
     <rdf:li xml:lang="x-default">old caption</rdf:li>
    </rdf:Alt></dc:description>
    <other:Keep>verbatim</other:Keep>"#,
    );

    let mut sc = Sidecar::read(&path).unwrap();
    assert_eq!(sc.caption().as_deref(), Some("old caption"), "x-default wins over the first li");
    sc.set_caption("new caption");
    sc.write(&path).unwrap();

    let raw = std::fs::read_to_string(&path).unwrap();
    assert!(raw.contains("coucher de soleil"), "other language kept: {raw}");
    assert!(raw.contains("<other:Keep>verbatim</other:Keep>"), "unknown node kept: {raw}");
    assert!(!raw.contains("old caption"), "{raw}");
    assert_eq!(Sidecar::read(&path).unwrap().caption().as_deref(), Some("new caption"));
}

#[test]
fn set_caption_upgrades_the_attribute_form_without_duplicating() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("photo.jpg.xmp");
    std::fs::write(
        &path,
        r#"<x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"><rdf:Description rdf:about="" xmlns:dc="http://purl.org/dc/elements/1.1/" dc:description="attribute-form"/></rdf:RDF></x:xmpmeta>"#,
    )
    .unwrap();

    let mut sc = Sidecar::read(&path).unwrap();
    assert_eq!(sc.caption().as_deref(), Some("attribute-form"));
    sc.set_caption("fresh");
    sc.write(&path).unwrap();

    let raw = std::fs::read_to_string(&path).unwrap();
    assert!(!raw.contains("attribute-form"), "{raw}");
    assert_eq!(raw.matches("fresh").count(), 1, "{raw}");
    assert_eq!(Sidecar::read(&path).unwrap().caption().as_deref(), Some("fresh"));
}

#[test]
fn clear_caption_removes_dc_description_and_keeps_everything_else() {
    let dir = tempfile::tempdir().unwrap();
    let path = sidecar_with(
        dir.path(),
        r#"<dc:description><rdf:Alt><rdf:li xml:lang="x-default">gone soon</rdf:li></rdf:Alt></dc:description>
    <dc:subject><rdf:Bag><rdf:li>sea</rdf:li></rdf:Bag></dc:subject>
    <xmp:Rating>4</xmp:Rating>
    <other:Keep>verbatim</other:Keep>"#,
    );

    let mut sc = Sidecar::read(&path).unwrap();
    sc.clear_caption();
    sc.write(&path).unwrap();

    let raw = std::fs::read_to_string(&path).unwrap();
    assert!(!raw.contains("dc:description"), "{raw}");
    assert!(!raw.contains("gone soon"), "{raw}");
    assert!(raw.contains("<other:Keep>verbatim</other:Keep>"), "{raw}");
    let reread = Sidecar::read(&path).unwrap();
    assert_eq!(reread.caption(), None);
    assert_eq!(reread.tags(), vec!["sea"]);
    assert_eq!(reread.rating(), Some(4));
}

#[test]
fn clear_caption_removes_every_language_so_nothing_resurfaces() {
    let dir = tempfile::tempdir().unwrap();
    let path = sidecar_with(
        dir.path(),
        r#"<dc:description><rdf:Alt>
     <rdf:li xml:lang="x-default">hello</rdf:li>
     <rdf:li xml:lang="fr">bonjour</rdf:li>
    </rdf:Alt></dc:description>"#,
    );

    let mut sc = Sidecar::read(&path).unwrap();
    sc.clear_caption();
    sc.write(&path).unwrap();

    assert_eq!(Sidecar::read(&path).unwrap().caption(), None);
}

#[test]
fn clear_caption_on_a_sidecar_without_one_is_a_no_op() {
    let dir = tempfile::tempdir().unwrap();
    let path = sidecar_with(dir.path(), "<xmp:Rating>2</xmp:Rating>");

    let mut sc = Sidecar::read(&path).unwrap();
    sc.clear_caption();

    assert_eq!(sc.caption(), None);
    assert_eq!(sc.rating(), Some(2));
    sc.write(&path).unwrap();
    let raw = std::fs::read_to_string(&path).unwrap();
    assert!(!raw.contains("dc:description"), "{raw}");
    assert!(raw.contains("<xmp:Rating>2</xmp:Rating>"), "{raw}");
}

#[test]
fn entities_in_existing_values_survive_a_caption_write() {
    let dir = tempfile::tempdir().unwrap();
    let path = sidecar_with(
        dir.path(),
        r#"<other:Keep>Tom &amp; Jerry &lt;3 &#233;</other:Keep>
    <dc:subject><rdf:Bag><rdf:li>R&amp;D</rdf:li></rdf:Bag></dc:subject>"#,
    );

    let mut sc = Sidecar::read(&path).unwrap();
    sc.set_caption("hi");
    sc.write(&path).unwrap();

    let raw = std::fs::read_to_string(&path).unwrap();
    assert!(raw.contains("<other:Keep>Tom &amp; Jerry &lt;3 é</other:Keep>"), "{raw}");
    assert_eq!(Sidecar::read(&path).unwrap().tags(), vec!["R&D"]);
}
