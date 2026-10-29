//! TASK-03-020: info-pane metadata editing — GPS, artist and copyright in the
//! sidecar (`exif:GPSLatitude/Longitude`, `dc:creator`, `dc:rights`), merged
//! into the existing packet without disturbing anything else (00-010 §3).

use std::path::PathBuf;

use fotobase_metadata::{parse_coordinate, GpsAxis, GpsParseError, Sidecar};

fn lat(text: &str) -> f64 {
    parse_coordinate(text, GpsAxis::Latitude).unwrap_or_else(|e| panic!("lat {text:?}: {e}"))
}

fn lon(text: &str) -> f64 {
    parse_coordinate(text, GpsAxis::Longitude).unwrap_or_else(|e| panic!("lon {text:?}: {e}"))
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-9
}

/// The packet without the serializer's indentation, for fragment checks.
fn compact(raw: &str) -> String {
    raw.lines().map(str::trim).collect()
}

fn fixture(name: &str) -> PathBuf {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.push("tests/fixtures/interop");
    p.push(name);
    p
}

// ---- GPS input: decimal degrees and DMS ---------------------------------

#[test]
fn decimal_degrees_parse_with_sign_or_hemisphere() {
    assert!(close(lat("52.5331"), 52.5331));
    assert!(close(lat("-33.8688"), -33.8688));
    assert!(close(lat("+12.5"), 12.5));
    assert!(close(lat("52.5331 N"), 52.5331));
    assert!(close(lat("52.5331N"), 52.5331));
    assert!(close(lat("33.8688 S"), -33.8688));
    assert!(close(lat("S 33.8688"), -33.8688));
    assert!(close(lon("13.405 e"), 13.405));
    assert!(close(lon("21.9426 W"), -21.9426));
    assert!(close(lon("52.5331°"), 52.5331));
    assert!(close(lat("  52.5331  "), 52.5331));
    assert!(close(lat("90"), 90.0));
    assert!(close(lon("-180"), -180.0));
}

#[test]
fn dms_input_parses_to_the_decimal_value() {
    // 52° 31' 59.9" = 52 + 31/60 + 59.9/3600
    let want = 52.0 + 31.0 / 60.0 + 59.9 / 3600.0;
    for text in [
        "52°31'59.9\"N",
        "52° 31' 59.9\" N",
        "N 52° 31' 59.9\"",
        "52 31 59.9 N",
        "52°31′59.9″N",
        "52º31'59.9\"N",
    ] {
        assert!(close(lat(text), want), "{text:?} -> {}", lat(text));
    }
    assert!(close(lat("52°31'59.9\"S"), -want));
    assert!(close(lat("-52°31'59.9\""), -want));
    // Degrees + decimal minutes.
    assert!(close(lon("13°24.5'E"), 13.0 + 24.5 / 60.0));
    assert!(close(lon("21° 56' 33.36\" W"), -(21.0 + 56.0 / 60.0 + 33.36 / 3600.0)));
}

#[test]
fn invalid_coordinates_are_rejected_with_a_reason() {
    let bad_lat = [
        "",
        "   ",
        "abc",
        "52.5.3",
        "52,5331",
        "91",
        "-90.0001",
        "52°60'0\"N",
        "52°31'60\"N",
        "52.5°31'N",
        "52°31.5'10\"N",
        "52°31'59\"10\"N",
        "52 31 59 10",
        "E 52",
        "52 E",
        "-52 S",
        "N 52 N",
        "NN",
    ];
    for text in bad_lat {
        assert!(parse_coordinate(text, GpsAxis::Latitude).is_err(), "{text:?} must be rejected");
    }
    assert!(parse_coordinate("181", GpsAxis::Longitude).is_err());
    assert!(parse_coordinate("-180.5", GpsAxis::Longitude).is_err());
    assert!(parse_coordinate("13 N", GpsAxis::Longitude).is_err(), "N is not a longitude");
    assert!(parse_coordinate("NaN", GpsAxis::Latitude).is_err());
    assert!(parse_coordinate("inf", GpsAxis::Latitude).is_err());
    assert!(parse_coordinate("1e1", GpsAxis::Latitude).is_err());
}

#[test]
fn errors_say_what_is_wrong() {
    assert_eq!(parse_coordinate("", GpsAxis::Latitude), Err(GpsParseError::Empty));
    assert!(matches!(
        parse_coordinate("95", GpsAxis::Latitude),
        Err(GpsParseError::OutOfRange { max: 90 })
    ));
    assert!(matches!(
        parse_coordinate("190", GpsAxis::Longitude),
        Err(GpsParseError::OutOfRange { max: 180 })
    ));
    assert!(matches!(
        parse_coordinate("13 N", GpsAxis::Longitude),
        Err(GpsParseError::WrongHemisphere('N'))
    ));
}

// ---- sidecar GPS ---------------------------------------------------------

#[test]
fn gps_is_written_in_the_xmp_coordinate_form_other_tools_read() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("a.jpg.xmp");
    let mut sc = Sidecar::read(&path).unwrap();
    sc.set_gps(52.5331, -21.9426);
    sc.write(&path).unwrap();

    let raw = compact(Sidecar::read(&path).unwrap().raw());
    assert!(raw.contains("<exif:GPSLatitude>52,31.986N</exif:GPSLatitude>"), "{raw}");
    assert!(raw.contains("<exif:GPSLongitude>21,56.556W</exif:GPSLongitude>"), "{raw}");
}

#[test]
fn gps_round_trips_without_losing_precision() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("a.jpg.xmp");
    for (la, lo) in [
        (52.5331, 13.405),
        (-33.868_820_1, 151.209_295_5),
        (0.0, 0.0),
        (90.0, 180.0),
        (-90.0, -180.0),
        (64.146_600_12, -21.942_600_34),
        (59.999_999_99, 29.999_999_999),
    ] {
        let mut sc = Sidecar::read(&path).unwrap();
        sc.set_gps(la, lo);
        sc.write(&path).unwrap();
        let (rla, rlo) = Sidecar::read(&path).unwrap().gps().expect("gps");
        assert!((rla - la).abs() < 1e-8, "lat {la} -> {rla}");
        assert!((rlo - lo).abs() < 1e-8, "lon {lo} -> {rlo}");
    }
}

#[test]
fn rewriting_gps_replaces_an_attribute_form_value_instead_of_duplicating_it() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("a.jpg.xmp");
    std::fs::write(
        &path,
        r#"<x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"><rdf:Description rdf:about="" xmlns:exif="http://ns.adobe.com/exif/1.0/" exif:GPSLatitude="1,0N" exif:GPSLongitude="2,0E"/></rdf:RDF></x:xmpmeta>"#,
    )
    .unwrap();

    let mut sc = Sidecar::read(&path).unwrap();
    sc.set_gps(10.0, 20.0);
    sc.write(&path).unwrap();

    let reread = Sidecar::read(&path).unwrap();
    assert_eq!(reread.gps(), Some((10.0, 20.0)));
    assert_eq!(reread.raw().matches("GPSLatitude").count(), 2, "one open + one close tag");
    assert!(!reread.raw().contains("GPSLatitude=\""), "attribute form dropped");
}

#[test]
fn clearing_gps_removes_both_coordinates() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("a.jpg.xmp");
    let mut sc = Sidecar::read(&path).unwrap();
    sc.set_gps(10.0, 20.0);
    sc.set_rating(3);
    sc.write(&path).unwrap();

    let mut sc = Sidecar::read(&path).unwrap();
    sc.clear_gps();
    sc.write(&path).unwrap();

    let reread = Sidecar::read(&path).unwrap();
    assert_eq!(reread.gps(), None);
    assert!(!reread.raw().contains("GPSLatitude"));
    assert!(!reread.raw().contains("GPSLongitude"));
    assert_eq!(reread.rating(), Some(3), "other fields untouched");
}

// ---- artist / copyright --------------------------------------------------

#[test]
fn artist_and_copyright_write_the_standard_dublin_core_fields() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("a.jpg.xmp");
    let mut sc = Sidecar::read(&path).unwrap();
    sc.set_artist("Ada Lovelace");
    sc.set_copyright("© 2024 Ada Lovelace");
    sc.write(&path).unwrap();

    let reread = Sidecar::read(&path).unwrap();
    let raw = &compact(reread.raw());
    // dc:creator is an ordered array; dc:rights a language alternative.
    assert!(
        raw.contains("<dc:creator><rdf:Seq><rdf:li>Ada Lovelace</rdf:li></rdf:Seq></dc:creator>"),
        "{raw}"
    );
    assert!(
        raw.contains(
            "<dc:rights><rdf:Alt><rdf:li xml:lang=\"x-default\">© 2024 Ada Lovelace</rdf:li></rdf:Alt></dc:rights>"
        ),
        "{raw}"
    );
    assert!(raw.contains("xmlns:dc=\"http://purl.org/dc/elements/1.1/\""), "{raw}");
    assert_eq!(reread.artist().as_deref(), Some("Ada Lovelace"));
    assert_eq!(reread.copyright().as_deref(), Some("© 2024 Ada Lovelace"));
}

#[test]
fn text_is_xml_escaped_and_round_trips() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("a.jpg.xmp");
    let mut sc = Sidecar::read(&path).unwrap();
    sc.set_artist("Tom & <Jerry>");
    sc.set_copyright("\"All\" rights 'reserved'");
    sc.write(&path).unwrap();

    let reread = Sidecar::read(&path).unwrap();
    assert_eq!(reread.artist().as_deref(), Some("Tom & <Jerry>"));
    assert_eq!(reread.copyright().as_deref(), Some("\"All\" rights 'reserved'"));
}

#[test]
fn clearing_removes_the_fields_and_nothing_else() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("a.jpg.xmp");
    let mut sc = Sidecar::read(&path).unwrap();
    sc.set_artist("A");
    sc.set_copyright("C");
    sc.set_caption("keep");
    sc.write(&path).unwrap();

    let mut sc = Sidecar::read(&path).unwrap();
    sc.clear_artist();
    sc.clear_copyright();
    sc.write(&path).unwrap();

    let reread = Sidecar::read(&path).unwrap();
    assert_eq!(reread.artist(), None);
    assert_eq!(reread.copyright(), None);
    assert!(!reread.raw().contains("dc:creator"));
    assert!(!reread.raw().contains("dc:rights"));
    assert_eq!(reread.caption().as_deref(), Some("keep"));
}

#[test]
fn several_creators_from_another_tool_read_joined_and_an_edit_replaces_them() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("a.jpg.xmp");
    std::fs::write(
        &path,
        r#"<x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"><rdf:Description rdf:about="" xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:creator><rdf:Seq><rdf:li>Ann</rdf:li><rdf:li>Bob</rdf:li></rdf:Seq></dc:creator></rdf:Description></rdf:RDF></x:xmpmeta>"#,
    )
    .unwrap();

    let mut sc = Sidecar::read(&path).unwrap();
    assert_eq!(sc.artist().as_deref(), Some("Ann, Bob"));
    sc.set_artist("Cy");
    sc.write(&path).unwrap();

    let reread = Sidecar::read(&path).unwrap();
    assert_eq!(reread.artist().as_deref(), Some("Cy"));
    assert_eq!(reread.raw().matches("<dc:creator>").count(), 1);
}

#[test]
fn copyright_keeps_other_languages_of_an_existing_alternative() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("a.jpg.xmp");
    std::fs::write(
        &path,
        r#"<x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"><rdf:Description rdf:about="" xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:rights><rdf:Alt><rdf:li xml:lang="de">Alle Rechte</rdf:li></rdf:Alt></dc:rights></rdf:Description></rdf:RDF></x:xmpmeta>"#,
    )
    .unwrap();

    let mut sc = Sidecar::read(&path).unwrap();
    assert_eq!(sc.copyright().as_deref(), Some("Alle Rechte"));
    sc.set_copyright("All rights reserved");
    sc.write(&path).unwrap();

    let reread = Sidecar::read(&path).unwrap();
    assert_eq!(reread.copyright().as_deref(), Some("All rights reserved"));
    assert!(reread.raw().contains("Alle Rechte"), "other languages survive");
}

// ---- merge-on-write against real third-party output ----------------------

#[test]
fn editing_all_fields_preserves_a_digikam_packet() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("photo.jpg.xmp");
    std::fs::copy(fixture("digikam-8-fixture.xmp"), &path).unwrap();

    let mut sc = Sidecar::read(&path).unwrap();
    sc.set_gps(48.8584, 2.2945);
    sc.set_artist("Ada");
    sc.set_copyright("© Ada");
    sc.write(&path).unwrap();

    let reread = Sidecar::read(&path).unwrap();
    let (la, lo) = reread.gps().unwrap();
    assert!((la - 48.8584).abs() < 1e-8 && (lo - 2.2945).abs() < 1e-8);
    assert_eq!(reread.artist().as_deref(), Some("Ada"));
    assert_eq!(reread.copyright().as_deref(), Some("© Ada"));
    // Untouched fields and digiKam's own nodes survive.
    assert_eq!(reread.rating(), Some(4));
    assert_eq!(reread.caption().as_deref(), Some("A beach sunset"));
    assert!(reread.raw().contains("<digiKam:TagsList>"));
    assert!(reread.raw().contains("diffedit=1"));
}
