//! ffprobe JSON fixture parsing (TDD, task 02-003 testing criteria).
//!
//! Fixtures are recorded ffprobe `-print_format json` documents: h264, hevc,
//! a rotated clip (both rotation encodings), and a corrupt file (audio-only
//! container). Parsing must yield the expected metadata; the corrupt fixture
//! yields an `Err` — never a panic (PLAN §9).

use fotobase_video::{parse_probe_json, VideoError};

fn fixture(name: &str) -> String {
    std::fs::read_to_string(format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR")))
        .unwrap()
}

#[test]
fn h264_fixture_parses() {
    let meta = parse_probe_json(&fixture("h264.json")).unwrap();
    assert_eq!(meta.codec, "h264");
    assert_eq!(meta.profile.as_deref(), Some("High"));
    assert!((meta.duration_secs - 6.033_333).abs() < 1e-4);
    assert_eq!(meta.rotation_deg, 0);
}

#[test]
fn hevc_fixture_parses() {
    let meta = parse_probe_json(&fixture("hevc.json")).unwrap();
    assert_eq!(meta.codec, "hevc");
    assert_eq!(meta.profile.as_deref(), Some("Main 10"));
    assert!((meta.duration_secs - 12.5).abs() < 1e-9);
    assert_eq!(meta.rotation_deg, 0);
}

#[test]
fn side_data_rotation_parses() {
    // Display-matrix side data: rotation: -90 → normalized to 270.
    let meta = parse_probe_json(&fixture("rotated-side-data.json")).unwrap();
    assert_eq!(meta.codec, "h264");
    assert_eq!(meta.rotation_deg, 270);
}

#[test]
fn legacy_tag_rotation_parses() {
    // Older ffprobe: tags.rotate = "180".
    let meta = parse_probe_json(&fixture("rotated-tag.json")).unwrap();
    assert_eq!(meta.rotation_deg, 180);
}

#[test]
fn corrupt_fixture_yields_error_not_panic() {
    // Audio-only container: no video stream → a marked failure, not a crash.
    match parse_probe_json(&fixture("no-video-stream.json")) {
        Err(VideoError::NoVideoStream) => {}
        other => panic!("expected NoVideoStream, got {other:?}"),
    }
}

#[test]
fn garbage_json_yields_parse_error() {
    assert!(parse_probe_json("<html>not json</html>").is_err());
}

#[test]
fn empty_input_yields_no_video_stream() {
    assert!(matches!(parse_probe_json("{}"), Err(VideoError::NoVideoStream)));
}
