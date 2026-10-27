//! TASK-02-005 unit tests: the codec-capability matrix the WebView reports
//! at startup deserializes from the frontend's per-codec booleans, stores in
//! app state, and formats for the startup debug log / diagnostics.

use fotobase_lib::codec::CodecSupport;

#[test]
fn deserializes_full_matrix() {
    let json = r#"{"h264":true,"hevc":false,"vp9":true,"av1":true}"#;
    let m: CodecSupport = serde_json::from_str(json).expect("parse");
    assert!(m.h264);
    assert!(!m.hevc);
    assert!(m.vp9);
    assert!(m.av1);
}

#[test]
fn missing_codec_fails_closed() {
    // A WebView omitting a field (older frontend / partial probe) must not
    // read as "supported" — playback decisions fail closed to the fallback.
    let json = r#"{"h264":true}"#;
    let m: CodecSupport = serde_json::from_str(json).expect("parse");
    assert!(m.h264);
    assert!(!m.hevc);
    assert!(!m.vp9);
    assert!(!m.av1);
}

#[test]
fn log_line_lists_every_codec() {
    let m = CodecSupport { h264: true, hevc: false, vp9: true, av1: false };
    let line = m.log_line();
    for codec in ["h264", "hevc", "vp9", "av1"] {
        assert!(line.contains(codec), "missing {codec} in {line}");
    }
    assert!(line.contains("yes"));
    assert!(line.contains("no"));
}
