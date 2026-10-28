//! darktable / digiKam interop suite (TASK-03-004, 00-010 §6 M11/M12).
//!
//! Fixtures under `tests/fixtures/interop/` are produced by the real tools:
//! `darktable-3.8.1-fixture.xmp` by darktable-cli 3.8.1 (the ubuntu-22.04 floor
//! version, PLAN §4), `digikam-8-fixture.xmp` by the exiv2 write path digiKam
//! itself uses (digiKam has no headless CLI; 00-010 fallback: real-tool
//! serialization). `tools/interop_fixtures.sh` regenerates both in CI and
//! diffs against the checked-in copies.
//!
//! Assertions compare parsed XMP, never bytes (tool ordering differs).

use std::path::PathBuf;

use fotobase_metadata::Sidecar;

fn fixture(name: &str) -> PathBuf {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.push("tests/fixtures/interop");
    p.push(name);
    p
}

fn approx(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-3
}

/// M12: a sidecar written by darktable parses with the expected
/// rating / caption / tags / GPS in Fotobase (PLAN §5 standard fields).
#[test]
fn darktable_fixture_parses_expected_fields() {
    let sc = Sidecar::read(&fixture("darktable-3.8.1-fixture.xmp")).unwrap();

    assert_eq!(sc.rating(), Some(4), "xmp:Rating written by darktable");
    assert_eq!(sc.caption().as_deref(), Some("A beach sunset"), "dc:description");
    let tags = sc.tags();
    assert!(tags.contains(&"Vacation".to_string()), "dc:subject = {tags:?}");
    assert!(tags.contains(&"2024".to_string()), "dc:subject = {tags:?}");
    // darktable rewrites DMS to deg,decimal-min ("52,30.9999990N") — the
    // parser must read both forms.
    let (lat, lon) = sc.gps().expect("GPS present");
    assert!(approx(lat, 52.0 + 30.999999 / 60.0), "lat = {lat}");
    assert!(approx(lon, 13.0 + 24.0 / 60.0), "lon = {lon}");
}

/// M12: a sidecar written by digiKam's serialization path (exiv2) parses with
/// the expected fields, including lr:hierarchicalSubject.
#[test]
fn digikam_fixture_parses_expected_fields() {
    let sc = Sidecar::read(&fixture("digikam-8-fixture.xmp")).unwrap();

    assert_eq!(sc.rating(), Some(4));
    assert_eq!(sc.caption().as_deref(), Some("A beach sunset"));
    assert_eq!(sc.tags(), vec!["Family".to_string(), "Vacation/2024".to_string()]);
    assert_eq!(sc.hierarchical_tags(), vec!["Vacation|2024".to_string()]);
    let (lat, lon) = sc.gps().expect("GPS present");
    assert!(approx(lat, 52.0 + 31.0 / 60.0), "lat = {lat}");
    assert!(approx(lon, 13.0 + 24.0 / 60.0), "lon = {lon}");
}

/// M11: a digiKam-edited sidecar re-written by Fotobase keeps digiKam's own
/// custom fields after our next write (merge-on-write proof against real
/// third-party output).
#[test]
fn fotobase_write_preserves_digikam_fields() {
    let src = fixture("digikam-8-fixture.xmp");
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("photo.jpg.xmp");
    std::fs::copy(&src, &path).unwrap();

    let mut sc = Sidecar::read(&path).unwrap();
    sc.set_rating(5);
    sc.set_tags(vec!["Fotobase".to_string()]);
    sc.set_gps(48.8584, 2.2945);
    sc.write(&path).unwrap();

    let reread = Sidecar::read(&path).unwrap();
    assert_eq!(reread.rating(), Some(5));
    assert_eq!(reread.tags(), vec!["Fotobase".to_string()]);

    // digiKam's custom namespace must survive verbatim in tree order.
    let raw = reread.raw();
    assert!(raw.contains("http://www.digikam.org/ns/1.0/"), "digiKam ns kept");
    assert!(raw.contains("<digiKam:TagsList>"), "TagsList kept");
    assert!(raw.contains("<rdf:li>Family</rdf:li>"), "TagsList values kept");
    assert!(raw.contains("diffedit=1"), "digiKam:ImageHistory kept");
    assert!(raw.contains("ImagePickLabel=\"3\""), "ImagePickLabel kept");
}

/// M12 (reverse direction): a Fotobase-written sidecar, parsed back through
/// the same reader semantics the other tools use (exiv2-compatible standard
/// fields), yields the values we wrote — the fields darktable/digiKam read.
#[test]
fn fotobase_written_fields_round_trip() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("photo.jpg.xmp");

    let mut sc = Sidecar::read(&path).unwrap();
    sc.set_rating(3);
    sc.set_tags(vec!["Aurora".to_string(), "Iceland/2025".to_string()]);
    sc.set_caption("Northern lights");
    sc.set_gps(64.1466, -21.9426);
    sc.write(&path).unwrap();

    let reread = Sidecar::read(&path).unwrap();
    assert_eq!(reread.rating(), Some(3));
    assert_eq!(reread.tags(), vec!["Aurora".to_string(), "Iceland/2025".to_string()]);
    assert_eq!(reread.caption().as_deref(), Some("Northern lights"));
    let (lat, lon) = reread.gps().expect("GPS present");
    assert!(approx(lat, 64.1466), "lat = {lat}");
    assert!(approx(lon, -21.9426), "lon = {lon}");
}
