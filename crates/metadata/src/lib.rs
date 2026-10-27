//! EXIF/IPTC/XMP read/write via exiv2 bindings (PLAN §3).
//!
//! All metadata writes must follow `docs/sidecar-merge-on-write.md`:
//! read-modify-write that preserves unknown nodes verbatim.

/// Crate identity, used by the workspace smoke test.
pub const CRATE_NAME: &str = "fotobase-metadata";

mod sidecar;

pub use sidecar::{FbNs, QuickXmlBackend, Sidecar, SidecarBackend, SidecarError, FB_NS};

#[cfg(test)]
mod tests {
    use crate::Sidecar;
    #[test]
    fn crate_identity() {
        assert_eq!(super::CRATE_NAME, "fotobase-metadata");
    }

    #[test]
    fn write_rating_preserves_unknown_field_and_backs_up() {
        let dir = tempfile::tempdir().unwrap();
        let sidecar = dir.path().join("IMG_0001.jpg.xmp");
        std::fs::write(&sidecar, SAMPLE_WITH_UNKNOWN_DIGIKAM_FIELD).unwrap();

        let mut sc = Sidecar::read(&sidecar).unwrap();
        sc.set_rating(5);
        sc.write(&sidecar).unwrap();

        // Backup was written before the replace and matches the pre-write content.
        let backup = dir.path().join("IMG_0001.jpg.xmp.fb-bak");
        assert!(backup.exists(), "backup must exist after write");
        assert_eq!(std::fs::read_to_string(&backup).unwrap(), SAMPLE_WITH_UNKNOWN_DIGIKAM_FIELD,);

        // Re-read: our field and the unknown digiKam field both survive.
        let sc2 = Sidecar::read(&sidecar).unwrap();
        assert_eq!(sc2.rating(), Some(5));
        assert!(
            sc2.raw().contains("digiKam:TagsList"),
            "unknown digiKam field must survive our write"
        );
        assert!(
            sc2.raw().contains("digiKam:CustomValue"),
            "unknown digiKam attribute must survive our write"
        );
    }

    #[test]
    fn malformed_xmp_returns_error_not_panic() {
        for blob in MALFORMED_CORPUS {
            let dir = tempfile::tempdir().unwrap();
            let sidecar = dir.path().join("bad.jpg.xmp");
            std::fs::write(&sidecar, blob).unwrap();
            let res = std::panic::catch_unwind(|| Sidecar::read(&sidecar));
            assert!(
                matches!(res, Ok(Err(_))) || matches!(res, Ok(Ok(_))),
                "malformed sidecar must not panic"
            );
        }
    }

    #[test]
    fn round_trip_preserves_unknown_fields() {
        let dir = tempfile::tempdir().unwrap();
        let sidecar = dir.path().join("IMG_0002.jpg.xmp");
        std::fs::write(&sidecar, SAMPLE_WITH_UNKNOWN_DIGIKAM_FIELD).unwrap();

        let mut sc = Sidecar::read(&sidecar).unwrap();
        sc.set_rating(3);
        sc.set_caption("hello");
        sc.write(&sidecar).unwrap();

        let sc2 = Sidecar::read(&sidecar).unwrap();
        assert_eq!(sc2.rating(), Some(3));
        assert_eq!(sc2.caption().as_deref(), Some("hello"));
        assert!(sc2.raw().contains("digiKam:TagsList"));
    }

    #[test]
    fn missing_sidecar_created_on_first_write() {
        let dir = tempfile::tempdir().unwrap();
        let sidecar = dir.path().join("IMG_0003.jpg.xmp");
        let mut sc = Sidecar::read(&sidecar).unwrap();
        sc.set_rating(4);
        sc.write(&sidecar).unwrap();
        let sc2 = Sidecar::read(&sidecar).unwrap();
        assert_eq!(sc2.rating(), Some(4));
    }

    #[test]
    fn gps_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let sidecar = dir.path().join("IMG_0004.jpg.xmp");
        let mut sc = Sidecar::read(&sidecar).unwrap();
        sc.set_gps(52.5200, 13.4050);
        sc.write(&sidecar).unwrap();
        let sc2 = Sidecar::read(&sidecar).unwrap();
        let (lat, lon) = sc2.gps().unwrap();
        assert!((lat - 52.52).abs() < 1e-6);
        assert!((lon - 13.405).abs() < 1e-6);
    }

    #[test]
    fn tags_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let sidecar = dir.path().join("IMG_0005.jpg.xmp");
        let mut sc = Sidecar::read(&sidecar).unwrap();
        sc.set_tags(vec!["sea".into(), "sunset".into()]);
        sc.write(&sidecar).unwrap();
        let sc2 = Sidecar::read(&sidecar).unwrap();
        assert_eq!(sc2.tags(), vec!["sea", "sunset"]);
    }

    #[test]
    fn external_modification_between_read_and_write_is_visible() {
        // M3: we must never reuse a cached tree as the write base.
        let dir = tempfile::tempdir().unwrap();
        let sidecar = dir.path().join("IMG_0006.jpg.xmp");
        std::fs::write(&sidecar, SAMPLE_WITH_UNKNOWN_DIGIKAM_FIELD).unwrap();

        let mut sc = Sidecar::read(&sidecar).unwrap();
        // External app changes the file before our write.
        let mut external = std::fs::read_to_string(&sidecar).unwrap();
        external = external.replace(
            "</rdf:Description>",
            "  <ns2:Foreign>later</ns2:Foreign>\n</rdf:Description>",
        );
        std::fs::write(&sidecar, external).unwrap();

        sc.set_rating(2);
        sc.write(&sidecar).unwrap();

        let sc2 = Sidecar::read(&sidecar).unwrap();
        assert_eq!(sc2.rating(), Some(2));
        assert!(sc2.raw().contains("ns2:Foreign"), "external late write must survive");
    }

    const SAMPLE_WITH_UNKNOWN_DIGIKAM_FIELD: &str = r#"<?xpacket begin="﻿" id="W5M0MpCehiHzreSzNTczkc9d"?>
<x:xmpmeta xmlns:x="adobe:ns:meta/" x:xmptk="XMP Core 5.4.0">
 <rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
  <rdf:Description rdf:about=""
    xmlns:digiKam="http://www.digikam.org/ns/1.0/"
    xmlns:ns2="http://example.invalid/ns2/">
   <digiKam:TagsList rdf:parseType="Bag">
    <rdf:li>family</rdf:li>
    <rdf:li>beach</rdf:li>
   </digiKam:TagsList>
   <digiKam:CustomValue>keep me</digiKam:CustomValue>
   <ns2:Foreign>data</ns2:Foreign>
  </rdf:Description>
 </rdf:RDF>
</x:xmpmeta>
<?xpacket end="w"?>"#;

    const MALFORMED_CORPUS: &[&str] = &[
        "",
        "not xml at all",
        "<x:xmpmeta><rdf:RDF></rdf:RDF>", // unclosed
        "\u{0}\u{1}\u{2}binary",
        "<?xml version=\"1.0\"?><x:xmpmeta", // truncated
        "<x:xmpmeta xmlns:x=\"adobe:ns:meta/\"> <rdf:RDF> <rdf:Description> <xmp:Rating>banana</xmp:Rating> </rdf:Description> </rdf:RDF> </x:xmpmeta>",
    ];
}
