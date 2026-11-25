//! TASK-10-01-03 integration: confirmed face names travel from one device's
//! sidecar to another's; unnamed clusters and unconfirmed suggestions never
//! do; fields outside the sync set and unknown XMP stay per device.

use std::path::Path;

use fotobase_metadata::{FaceRegion, Sidecar};
use fotobase_sync::{syncable_faces, DetectedFace, MetadataDoc, PhotoMeta};

const KEY: &str = "sha256:abc";

const DEVICE_B_SIDECAR: &str = r#"<?xpacket begin="" id="W5M0MpCehiHzreSzNTczkc9d"?>
<x:xmpmeta xmlns:x="adobe:ns:meta/">
 <rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
  <rdf:Description rdf:about=""
    xmlns:xmp="http://ns.adobe.com/xap/1.0/"
    xmlns:digiKam="http://www.digikam.org/ns/1.0/">
   <xmp:Rating>2</xmp:Rating>
   <digiKam:ColorLabel>3</digiKam:ColorLabel>
  </rdf:Description>
 </rdf:RDF>
</x:xmpmeta>
<?xpacket end="w"?>"#;

fn face(name: &str, x: f64) -> FaceRegion {
    FaceRegion { name: name.into(), x, y: 0.5, w: 0.1, h: 0.2 }
}

fn names(faces: &[FaceRegion]) -> Vec<&str> {
    let mut n: Vec<&str> = faces.iter().map(|f| f.name.as_str()).collect();
    n.sort_unstable();
    n
}

/// Device B: import A's state and write it into B's own sidecar.
fn sync_into(a: &MetadataDoc, path: &Path) -> Sidecar {
    let mut b = MetadataDoc::with_peer(2);
    b.import(&a.export_snapshot().unwrap()).unwrap();
    let mut sc = Sidecar::read(path).unwrap();
    if b.photo(KEY).unwrap().apply_to_sidecar(&mut sc) {
        sc.write(path).unwrap();
    }
    Sidecar::read(path).unwrap()
}

#[test]
fn only_confirmed_named_faces_are_syncable() {
    let faces = syncable_faces([
        DetectedFace { region: face("Anna", 0.2), confirmed: true },
        DetectedFace { region: face("Ben", 0.4), confirmed: false },
        DetectedFace { region: face("", 0.6), confirmed: false },
        DetectedFace { region: face("  ", 0.8), confirmed: true },
    ]);
    assert_eq!(names(&faces), vec!["Anna"]);
}

#[test]
fn a_confirmed_face_name_reaches_the_other_devices_sidecar() {
    let dir = tempfile::tempdir().unwrap();
    let a_path = dir.path().join("a.jpg.xmp");
    let mut a_sc = Sidecar::read(&a_path).unwrap();
    a_sc.add_face_regions(&[face("Anna", 0.3)], None).unwrap();
    a_sc.set_tags(vec!["family".into()]);
    a_sc.set_gps(48.85, 2.35);
    a_sc.write(&a_path).unwrap();

    let mut a = MetadataDoc::with_peer(1);
    a.update_photo(KEY, &PhotoMeta::from_sidecar(&Sidecar::read(&a_path).unwrap())).unwrap();

    let b_path = dir.path().join("b.jpg.xmp");
    std::fs::write(&b_path, DEVICE_B_SIDECAR).unwrap();
    let b_sc = sync_into(&a, &b_path);

    assert_eq!(b_sc.face_regions(), vec![face("Anna", 0.3)]);
    assert_eq!(b_sc.tags(), vec!["family"]);
    // A had no rating: the sync set clears B's to match.
    assert_eq!(b_sc.rating(), None);
    // GPS is not part of the sync set; unknown XMP survives the write.
    assert_eq!(b_sc.gps(), None);
    assert!(b_sc.raw().contains("<digiKam:ColorLabel>3</digiKam:ColorLabel>"));
}

#[test]
fn unconfirmed_cluster_edits_do_not_sync() {
    let mut a = MetadataDoc::with_peer(1);
    let confirmed = DetectedFace { region: face("Anna", 0.2), confirmed: true };
    let mut cluster = DetectedFace { region: face("", 0.6), confirmed: false };
    let meta =
        |faces: Vec<DetectedFace>| PhotoMeta { faces: syncable_faces(faces), ..Default::default() };
    assert!(a.update_photo(KEY, &meta(vec![confirmed.clone(), cluster.clone()])).unwrap());

    // The user labels the cluster with a suggestion but does not confirm it:
    // nothing to record.
    cluster.region.name = "Ben".into();
    assert!(!a.update_photo(KEY, &meta(vec![confirmed.clone(), cluster.clone()])).unwrap());

    let dir = tempfile::tempdir().unwrap();
    let b_sc = sync_into(&a, &dir.path().join("b.jpg.xmp"));
    assert_eq!(names(&b_sc.face_regions()), vec!["Anna"]);

    // Once confirmed, the name syncs.
    cluster.confirmed = true;
    assert!(a.update_photo(KEY, &meta(vec![confirmed, cluster])).unwrap());
    let b_sc = sync_into(&a, &dir.path().join("b2.jpg.xmp"));
    assert_eq!(names(&b_sc.face_regions()), vec!["Anna", "Ben"]);
}

#[test]
fn a_renamed_face_replaces_the_old_name_on_the_other_device() {
    let dir = tempfile::tempdir().unwrap();
    let b_path = dir.path().join("b.jpg.xmp");
    let mut a = MetadataDoc::with_peer(1);
    a.update_photo(KEY, &PhotoMeta { faces: vec![face("Anna", 0.2)], ..Default::default() })
        .unwrap();
    sync_into(&a, &b_path);

    a.update_photo(KEY, &PhotoMeta { faces: vec![face("Anne", 0.2)], ..Default::default() })
        .unwrap();
    assert_eq!(sync_into(&a, &b_path).face_regions(), vec![face("Anne", 0.2)]);
}

#[test]
fn sidecar_round_trip_without_changes_writes_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("p.jpg.xmp");
    std::fs::write(&path, DEVICE_B_SIDECAR).unwrap();
    let mut sc = Sidecar::read(&path).unwrap();
    let meta = PhotoMeta::from_sidecar(&sc);
    assert_eq!(meta.rating, Some(2));
    assert!(!meta.apply_to_sidecar(&mut sc));
}
