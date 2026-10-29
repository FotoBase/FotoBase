//! TASK-03-022: MWG face regions (`mwg-rs:Regions`) in the sidecar — faces
//! from a folder ini land here, merged without touching anything we don't own
//! (docs/sidecar-merge-on-write.md §3).

use fotobase_metadata::{FaceRegion, Sidecar};

fn region(name: &str, x: f64, y: f64, w: f64, h: f64) -> FaceRegion {
    FaceRegion { name: name.to_string(), x, y, w, h }
}

#[test]
fn a_fresh_sidecar_has_no_regions() {
    let dir = tempfile::tempdir().unwrap();
    let sc = Sidecar::read(&dir.path().join("p.jpg.xmp")).unwrap();
    assert!(sc.face_regions().is_empty());
}

#[test]
fn regions_round_trip_with_dimensions_and_declared_namespaces() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("p.jpg.xmp");
    let mut sc = Sidecar::read(&path).unwrap();
    let added = sc
        .add_face_regions(
            &[region("Anna", 0.4, 0.3, 0.25, 0.5), region("Bob", 0.75, 0.5, 0.1, 0.2)],
            Some((4000, 3000)),
        )
        .unwrap();
    assert_eq!(added, 2);
    sc.write(&path).unwrap();

    let raw = std::fs::read_to_string(&path).unwrap();
    for needle in [
        r#"xmlns:mwg-rs="http://www.metadataworkinggroup.com/schemas/regions/""#,
        r#"xmlns:stArea="http://ns.adobe.com/xmp/sType/Area#""#,
        r#"xmlns:stDim="http://ns.adobe.com/xap/1.0/sType/Dimensions#""#,
        r#"stDim:w="4000""#,
        r#"stDim:h="3000""#,
        r#"stDim:unit="pixel""#,
        "<mwg-rs:Name>Anna</mwg-rs:Name>",
        "<mwg-rs:Type>Face</mwg-rs:Type>",
        r#"stArea:unit="normalized""#,
        r#"stArea:x="0.4""#,
    ] {
        assert!(raw.contains(needle), "missing {needle}: {raw}");
    }

    let back = Sidecar::read(&path).unwrap().face_regions();
    assert_eq!(back.len(), 2);
    assert_eq!(back[0].name, "Anna");
    assert!((back[0].w - 0.25).abs() < 1e-9);
    assert_eq!(back[1].name, "Bob");
}

#[test]
fn dimensions_are_optional() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("p.jpg.xmp");
    let mut sc = Sidecar::read(&path).unwrap();
    sc.add_face_regions(&[region("Anna", 0.5, 0.5, 0.2, 0.2)], None).unwrap();
    sc.write(&path).unwrap();
    let raw = std::fs::read_to_string(&path).unwrap();
    assert!(!raw.contains("AppliedToDimensions"), "{raw}");
    assert_eq!(Sidecar::read(&path).unwrap().face_regions().len(), 1);
}

#[test]
fn adding_the_same_region_twice_is_a_no_op() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("p.jpg.xmp");
    let mut sc = Sidecar::read(&path).unwrap();
    let r = region("Anna", 0.4, 0.3, 0.25, 0.5);
    assert_eq!(sc.add_face_regions(std::slice::from_ref(&r), Some((10, 10))), Some(1));
    sc.write(&path).unwrap();

    let mut again = Sidecar::read(&path).unwrap();
    // Same face, name differing only by case, rounding noise below 1e-4.
    let noisy = region("anna", 0.40004, 0.29996, 0.25, 0.5);
    assert_eq!(again.add_face_regions(&[r.clone(), noisy], Some((10, 10))), Some(0));
    assert_eq!(again.face_regions().len(), 1);
}

const DIGIKAM_STYLE: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<x:xmpmeta xmlns:x="adobe:ns:meta/">
 <rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
  <rdf:Description rdf:about=""
    xmlns:mwg-rs="http://www.metadataworkinggroup.com/schemas/regions/"
    xmlns:stArea="http://ns.adobe.com/xmp/sType/Area#"
    xmlns:stDim="http://ns.adobe.com/xap/1.0/sType/Dimensions#"
    xmlns:digiKam="http://www.digikam.org/ns/1.0/">
   <digiKam:Keep>unknown node</digiKam:Keep>
   <mwg-rs:Regions rdf:parseType="Resource">
    <mwg-rs:AppliedToDimensions stDim:w="800" stDim:h="600" stDim:unit="pixel"/>
    <mwg-rs:RegionList>
     <rdf:Bag>
      <rdf:li>
       <rdf:Description mwg-rs:Name="Carol" mwg-rs:Type="Face">
        <mwg-rs:Area stArea:x="0.5" stArea:y="0.5" stArea:w="0.2" stArea:h="0.3" stArea:unit="normalized"/>
       </rdf:Description>
      </rdf:li>
      <rdf:li>
       <rdf:Description mwg-rs:Name="Fence" mwg-rs:Type="Pet">
        <mwg-rs:Area stArea:x="0.1" stArea:y="0.1" stArea:w="0.1" stArea:h="0.1" stArea:unit="normalized"/>
       </rdf:Description>
      </rdf:li>
     </rdf:Bag>
    </mwg-rs:RegionList>
   </mwg-rs:Regions>
  </rdf:Description>
 </rdf:RDF>
</x:xmpmeta>
"#;

#[test]
fn reads_regions_written_by_other_tools_and_ignores_non_faces() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("p.jpg.xmp");
    std::fs::write(&path, DIGIKAM_STYLE).unwrap();
    let faces = Sidecar::read(&path).unwrap().face_regions();
    assert_eq!(faces.len(), 1, "{faces:?}");
    assert_eq!(faces[0].name, "Carol");
    assert!((faces[0].h - 0.3).abs() < 1e-12);
}

#[test]
fn existing_regions_and_unknown_nodes_survive_an_added_region() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("p.jpg.xmp");
    std::fs::write(&path, DIGIKAM_STYLE).unwrap();

    let mut sc = Sidecar::read(&path).unwrap();
    assert_eq!(sc.add_face_regions(&[region("Anna", 0.3, 0.3, 0.1, 0.1)], Some((9, 9))), Some(1));
    sc.write(&path).unwrap();

    let raw = std::fs::read_to_string(&path).unwrap();
    assert!(raw.contains("digiKam:Keep"), "{raw}");
    assert!(raw.contains(r#"mwg-rs:Name="Carol""#), "other tool's region kept verbatim: {raw}");
    assert!(raw.contains(r#"mwg-rs:Name="Fence""#), "non-face region kept: {raw}");
    assert!(raw.contains(r#"stDim:w="800""#), "existing dimensions kept: {raw}");
    assert!(!raw.contains(r#"stDim:w="9""#), "dimensions are never overwritten: {raw}");
    assert_eq!(raw.matches("<mwg-rs:Regions").count(), 1, "{raw}");
    let names: Vec<_> =
        Sidecar::read(&path).unwrap().face_regions().into_iter().map(|r| r.name).collect();
    assert_eq!(names, vec!["Carol", "Anna"]);
}

#[test]
fn an_unrecognised_regions_structure_is_left_alone() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("p.jpg.xmp");
    let odd = DIGIKAM_STYLE
        .replace("<mwg-rs:RegionList>", "<mwg-rs:Other>")
        .replace("</mwg-rs:RegionList>", "</mwg-rs:Other>");
    std::fs::write(&path, &odd).unwrap();
    let mut sc = Sidecar::read(&path).unwrap();
    assert_eq!(sc.add_face_regions(&[region("Anna", 0.3, 0.3, 0.1, 0.1)], None), None);
    sc.write(&path).unwrap();
    assert!(!std::fs::read_to_string(&path).unwrap().contains("Anna"));
}
