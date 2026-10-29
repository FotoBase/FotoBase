//! TASK-03-019: capture-time arithmetic (shift, timezone fix, clock merge)
//! and its sidecar form — `exif:DateTimeOriginal`, merged into the existing
//! packet without disturbing anything else (00-010 §3).

use std::path::Path;

use fotobase_metadata::{CaptureTime, Sidecar};

fn ct(s: &str) -> CaptureTime {
    CaptureTime::parse(s).unwrap_or_else(|| panic!("parse {s}"))
}

#[test]
fn parses_xmp_and_exif_spellings() {
    assert_eq!(ct("2024-06-01T12:00:00+02:00").to_xmp(), "2024-06-01T12:00:00+02:00");
    assert_eq!(ct("2024-06-01T12:00:00Z").to_xmp(), "2024-06-01T12:00:00+00:00");
    assert_eq!(ct("2024-06-01T12:00:00").to_xmp(), "2024-06-01T12:00:00");
    // EXIF's colon date form, space separator, no offset.
    assert_eq!(ct("2024:06:01 12:00:00").to_xmp(), "2024-06-01T12:00:00");
    assert_eq!(ct("2024-06-01T12:00:00.25-03:30").to_xmp(), "2024-06-01T12:00:00.25-03:30");
    // Minutes-precision XMP dates are valid too.
    assert_eq!(ct("2024-06-01T12:00").to_xmp(), "2024-06-01T12:00:00");
}

#[test]
fn rejects_garbage_and_impossible_dates() {
    for bad in [
        "",
        "yesterday",
        "2024-06-01",
        "2024-13-01T00:00:00",
        "2024-02-30T00:00:00",
        "2023-02-29T00:00:00",
        "2024-06-01T24:00:00",
        "2024-06-01T12:60:00",
        "2024-06-01T12:00:00+15:00",
        "0000-01-01T00:00:00",
    ] {
        assert!(CaptureTime::parse(bad).is_none(), "{bad:?} must not parse");
    }
    assert!(CaptureTime::parse("2024-02-29T00:00:00").is_some(), "leap day");
}

#[test]
fn shift_keeps_the_offset_and_moves_the_wall_clock() {
    // Spec: shifting a +02:00 photo by -2h yields the expected stored time.
    let shifted = ct("2024-06-01T12:00:00+02:00").shifted(-2 * 3600).unwrap();
    assert_eq!(shifted.to_xmp(), "2024-06-01T10:00:00+02:00");
}

#[test]
fn shift_carries_across_day_month_year_and_leap_boundaries() {
    assert_eq!(ct("2023-12-31T23:30:00").shifted(3600).unwrap().to_xmp(), "2024-01-01T00:30:00");
    assert_eq!(ct("2024-03-01T00:10:00").shifted(-1200).unwrap().to_xmp(), "2024-02-29T23:50:00");
    assert_eq!(
        ct("2024-01-31T10:00:00").shifted(30 * 86_400).unwrap().to_xmp(),
        "2024-03-01T10:00:00"
    );
    assert_eq!(
        ct("2024-06-01T12:00:00.5+01:00").shifted(90).unwrap().to_xmp(),
        "2024-06-01T12:01:30.5+01:00"
    );
}

#[test]
fn shift_out_of_the_representable_years_is_refused() {
    assert!(ct("9999-12-31T23:59:59").shifted(1).is_none());
    assert!(ct("0001-01-01T00:00:00").shifted(-1).is_none());
}

#[test]
fn relabelling_the_offset_keeps_the_wall_clock() {
    // The camera clock showed local time but tagged UTC: wall clock is right.
    let fixed = ct("2024-06-01T12:00:00+00:00").with_offset_keeping_wall_clock(120).unwrap();
    assert_eq!(fixed.to_xmp(), "2024-06-01T12:00:00+02:00");
    assert_eq!(fixed.instant(), ct("2024-06-01T10:00:00Z").instant());
    // A naive time gains the offset without moving.
    let naive = ct("2024-06-01T12:00:00").with_offset_keeping_wall_clock(-300).unwrap();
    assert_eq!(naive.to_xmp(), "2024-06-01T12:00:00-05:00");
}

#[test]
fn converting_the_offset_keeps_the_instant() {
    // The camera was left on UTC: the wall clock is UTC, show it in +02:00.
    let local = ct("2024-06-01T08:00:00Z").converted_to_offset(120).unwrap();
    assert_eq!(local.to_xmp(), "2024-06-01T10:00:00+02:00");
    assert_eq!(local.instant(), ct("2024-06-01T08:00:00Z").instant());
    // A naive time is read as UTC, the "left on UTC" case.
    let naive = ct("2024-06-01T08:00:00").converted_to_offset(-330).unwrap();
    assert_eq!(naive.to_xmp(), "2024-06-01T02:30:00-05:30");
    // Day boundary.
    let next_day = ct("2024-06-01T23:00:00Z").converted_to_offset(120).unwrap();
    assert_eq!(next_day.to_xmp(), "2024-06-02T01:00:00+02:00");
}

#[test]
fn offsets_outside_the_real_world_range_are_refused() {
    let t = ct("2024-06-01T12:00:00");
    assert!(t.with_offset_keeping_wall_clock(841).is_none());
    assert!(t.converted_to_offset(-721).is_none());
    assert!(t.with_offset_keeping_wall_clock(840).is_some());
    assert!(t.converted_to_offset(-720).is_some());
}

#[test]
fn from_unix_is_utc() {
    assert_eq!(
        CaptureTime::from_unix(1_717_243_200).unwrap().to_xmp(),
        "2024-06-01T12:00:00+00:00"
    );
}

#[test]
fn ordering_follows_the_instant() {
    let early = ct("2024-06-01T12:00:00+02:00"); // 10:00Z
    let late = ct("2024-06-01T11:00:00+00:00"); // 11:00Z
    assert!(early.instant() < late.instant());
}

fn sidecar_with(dir: &Path, body: &str) -> std::path::PathBuf {
    let path = dir.join("photo.jpg.xmp");
    let xmp = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<x:xmpmeta xmlns:x="adobe:ns:meta/">
 <rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
  <rdf:Description rdf:about=""
    xmlns:exif="http://ns.adobe.com/exif/1.0/"
    xmlns:xmp="http://ns.adobe.com/xap/1.0/"
    xmlns:photoshop="http://ns.adobe.com/photoshop/1.0/"
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
fn a_fresh_sidecar_gets_exif_date_time_original() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("photo.jpg.xmp");

    let mut sc = Sidecar::read(&path).unwrap();
    assert!(sc.capture_time().is_none());
    sc.set_capture_time(&ct("2024-06-01T10:00:00+02:00"));
    sc.write(&path).unwrap();

    let raw = std::fs::read_to_string(&path).unwrap();
    assert!(
        raw.contains("<exif:DateTimeOriginal>2024-06-01T10:00:00+02:00</exif:DateTimeOriginal>"),
        "{raw}"
    );
    assert!(raw.contains("xmlns:exif="), "{raw}");
    assert_eq!(
        Sidecar::read(&path).unwrap().capture_time().unwrap().to_xmp(),
        "2024-06-01T10:00:00+02:00"
    );
}

#[test]
fn set_capture_time_replaces_both_node_and_attribute_forms() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("photo.jpg.xmp");
    std::fs::write(
        &path,
        r#"<x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"><rdf:Description rdf:about="" xmlns:exif="http://ns.adobe.com/exif/1.0/" exif:DateTimeOriginal="2020-01-01T00:00:00"/></rdf:RDF></x:xmpmeta>"#,
    )
    .unwrap();

    let mut sc = Sidecar::read(&path).unwrap();
    assert_eq!(sc.capture_time().unwrap().to_xmp(), "2020-01-01T00:00:00");
    sc.set_capture_time(&ct("2021-02-03T04:05:06"));
    sc.write(&path).unwrap();

    let raw = std::fs::read_to_string(&path).unwrap();
    assert_eq!(raw.matches("DateTimeOriginal").count(), 2, "one open + one close tag: {raw}");
    assert_eq!(
        Sidecar::read(&path).unwrap().capture_time().unwrap().to_xmp(),
        "2021-02-03T04:05:06"
    );
}

#[test]
fn unknown_nodes_and_unrelated_fields_survive_the_write() {
    let dir = tempfile::tempdir().unwrap();
    let path = sidecar_with(
        dir.path(),
        r#"    <other:Thing>keep me</other:Thing>
    <xmp:Rating>4</xmp:Rating>
    <exif:DateTimeOriginal>2020-01-01T00:00:00</exif:DateTimeOriginal>"#,
    );

    let mut sc = Sidecar::read(&path).unwrap();
    sc.set_capture_time(&ct("2021-02-03T04:05:06"));
    sc.write(&path).unwrap();

    let raw = std::fs::read_to_string(&path).unwrap();
    assert!(raw.contains("<other:Thing>keep me</other:Thing>"), "{raw}");
    assert!(raw.contains("<xmp:Rating>4</xmp:Rating>"), "{raw}");
    assert_eq!(Sidecar::read(&path).unwrap().rating(), Some(4));
}

#[test]
fn other_apps_date_fields_already_in_the_packet_stay_consistent() {
    let dir = tempfile::tempdir().unwrap();
    let path = sidecar_with(
        dir.path(),
        r#"    <exif:DateTimeOriginal>2020-01-01T00:00:00</exif:DateTimeOriginal>
    <xmp:CreateDate>2020-01-01T00:00:00</xmp:CreateDate>
    <photoshop:DateCreated>2020-01-01T00:00:00</photoshop:DateCreated>
    <xmp:ModifyDate>2020-01-02T00:00:00</xmp:ModifyDate>"#,
    );

    let mut sc = Sidecar::read(&path).unwrap();
    sc.set_capture_time(&ct("2021-02-03T04:05:06"));
    sc.write(&path).unwrap();

    let raw = std::fs::read_to_string(&path).unwrap();
    assert!(raw.contains("<xmp:CreateDate>2021-02-03T04:05:06</xmp:CreateDate>"), "{raw}");
    assert!(
        raw.contains("<photoshop:DateCreated>2021-02-03T04:05:06</photoshop:DateCreated>"),
        "{raw}"
    );
    // Not a capture date: left alone.
    assert!(raw.contains("<xmp:ModifyDate>2020-01-02T00:00:00</xmp:ModifyDate>"), "{raw}");
}

#[test]
fn xmp_create_date_is_not_invented_when_absent() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("photo.jpg.xmp");
    let mut sc = Sidecar::read(&path).unwrap();
    sc.set_capture_time(&ct("2021-02-03T04:05:06"));
    sc.write(&path).unwrap();
    let raw = std::fs::read_to_string(&path).unwrap();
    assert!(!raw.contains("CreateDate"), "{raw}");
}

#[test]
fn an_unparseable_stored_date_reads_as_absent() {
    let dir = tempfile::tempdir().unwrap();
    let path =
        sidecar_with(dir.path(), "    <exif:DateTimeOriginal>not a date</exif:DateTimeOriginal>");
    assert!(Sidecar::read(&path).unwrap().capture_time().is_none());
}
