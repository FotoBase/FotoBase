//! TASK-01-003 acceptance gates for the signed release pipeline.
//!
//! 1. Updater signing: a valid signature verifies and an artifact whose bytes
//!    are tampered with after signing is rejected. These tests use
//!    `minisign-verify` — the same verifier `tauri-plugin-updater` uses — so
//!    passing here means the updater will reject the same tampered bytes.
//! 2. Updater wiring: `tauri.conf.json` must enable the bundler + updater
//!    artifacts, pin the committed public key, and point at the GitHub
//!    Releases feed (ADR-0010, ADR-0012).
//! 3. Release workflow: `.github/workflows/release.yml` must trigger on `v*`
//!    tags, build on the ADR-0005 floor matrix, and wire the signing /
//!    notarization secrets. Secrets live in GitHub Actions secrets only —
//!    never in the repo (only the *public* updater key is committed).

use base64::Engine;
use minisign_verify::{PublicKey, Signature};
use std::path::PathBuf;

// Fixed test payload signed with the project updater key via
// `tauri signer sign`. The private key is NOT in the repo — it lives with the
// release manager and in the CI secret `TAURI_SIGNING_PRIVATE_KEY`.
const PAYLOAD: &[u8] = b"fotobase-updater-test-payload-v1";
// Same encoding as tauri.conf.json `plugins.updater.pubkey` and the
// `signature` field of the updater feed: base64 of the minisign key/signature
// file text (what `tauri signer generate`/`sign` emit).
const PUBKEY_B64: &str = "dW50cnVzdGVkIGNvbW1lbnQ6IG1pbmlzaWduIHB1YmxpYyBrZXk6IDBEREFBREJDMzc3REY1NDUKUldSRjlYMDN2SzNhRFNEWTErSit6dWRjSlZ4aXlqMlhaWDQyZjJIMGpSVUR4VDIwcU13TE9SejEK";
const SIGNATURE_B64: &str = "dW50cnVzdGVkIGNvbW1lbnQ6IHNpZ25hdHVyZSBmcm9tIHRhdXJpIHNlY3JldCBrZXkKUlVSRjlYMDN2SzNhRFdJM2hKYmVxN0FGbzBmRWpLN2lQd3laOXVldWJwNVZwU2tqSWtzYjl0ZnRwWjdlU2FDc1F6MDdpV1dPL2grSnpDeUFMdFNsV2JObnk4akhlYjUwYWdJPQp0cnVzdGVkIGNvbW1lbnQ6IHRpbWVzdGFtcDoxNzkyNTcxNzcyCWZpbGU6ZmItdXBkYXRlci10ZXN0LXBheWxvYWQuYmluCmM1NUtTbjlvWkVqc3gyWXFSNHZLRE5nS2xqcThRUlpyUWJsWFordFZzbkdZdWw3R044VDZ5UmN0WWk2T3I0QmQzVE9id1lCL2x0bjNSSTJ5bTkxMEFnPT0K";

fn decode_outer(b64: &str) -> String {
    let bytes =
        base64::engine::general_purpose::STANDARD.decode(b64).expect("outer base64 must decode");
    String::from_utf8(bytes).expect("decoded material must be UTF-8")
}

fn pubkey() -> PublicKey {
    // Mirrors tauri-plugin-updater `verify_signature`: base64-outer decode,
    // then `PublicKey::decode` over the minisign file text.
    PublicKey::decode(&decode_outer(PUBKEY_B64))
        .expect("committed updater public key must be valid")
}

fn signature() -> Signature {
    Signature::decode(&decode_outer(SIGNATURE_B64)).expect("test signature must be valid")
}

fn repo_root() -> PathBuf {
    // tests/ lives in app/src-tauri -> repo root is two levels up.
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn read(rel: &str) -> String {
    std::fs::read_to_string(repo_root().join(rel))
        .unwrap_or_else(|e| panic!("cannot read {rel}: {e}"))
}

#[test]
fn valid_signature_verifies() {
    assert!(pubkey().verify(PAYLOAD, &signature(), true).is_ok());
}

#[test]
fn tampered_artifact_is_rejected() {
    let mut tampered = PAYLOAD.to_vec();
    tampered[0] ^= 0xff;
    assert!(
        pubkey().verify(&tampered, &signature(), true).is_err(),
        "a modified artifact must fail signature verification"
    );
}

#[test]
fn tampered_signature_is_rejected() {
    let mut sig = SIGNATURE_B64.as_bytes().to_vec();
    let idx = sig.len() - 2; // keep base64 padding intact
    sig[idx] = if sig[idx] == b'A' { b'B' } else { b'A' };
    // A tampered signature must be rejected — either it fails to decode or it
    // fails verification. Either way nothing may pass.
    let outcome = Signature::decode(&decode_outer(std::str::from_utf8(&sig).unwrap()))
        .map(|sig| pubkey().verify(PAYLOAD, &sig, true));
    assert!(
        outcome.map(|r| r.is_err()).unwrap_or(true),
        "a modified signature must fail verification"
    );
}

#[test]
fn updater_and_bundler_are_wired_in_tauri_conf() {
    let cfg: serde_json::Value =
        serde_json::from_str(&read("app/src-tauri/tauri.conf.json")).expect("valid JSON config");

    assert_eq!(cfg["bundle"]["active"], true, "bundler must be active");
    assert_eq!(
        cfg["bundle"]["createUpdaterArtifacts"], true,
        "bundler must produce signed updater artifacts"
    );
    // macOS floor (ADR-0005) enforced at the bundler level, as promised in ci.yml.
    assert_eq!(
        cfg["bundle"]["macOS"]["minimumSystemVersion"], "12.0",
        "macOS 12 floor must be enforced by the bundler"
    );

    let updater = &cfg["plugins"]["updater"];
    let pubkey = updater["pubkey"].as_str().expect("updater pubkey must be set");
    assert!(
        // Same path the updater uses to consume this config value.
        PublicKey::decode(&decode_outer(pubkey)).is_ok(),
        "committed pubkey must be a valid minisign public key"
    );
    assert_eq!(pubkey, PUBKEY_B64, "config pubkey must be the committed updater key");

    let endpoints = updater["endpoints"].as_array().expect("endpoints array");
    assert!(
        endpoints.iter().any(|e| e.as_str()
            == Some("https://github.com/FotoBase/FotoBase/releases/latest/download/latest.json")),
        "updater must use the GitHub Releases static feed (ADR-0010)"
    );
}

#[test]
fn updater_plugin_is_registered() {
    let cargo = read("app/src-tauri/Cargo.toml");
    assert!(cargo.contains("tauri-plugin-updater"), "updater plugin dependency must be declared");
    let caps = read("app/src-tauri/capabilities/default.json");
    assert!(caps.contains("updater:default"), "main window capability must grant updater:default");
}

#[test]
fn release_workflow_is_wired() {
    let yml = read(".github/workflows/release.yml");

    for needle in [
        "\"v*\"",                     // ADR-0012: v* tags trigger releases
        "tauri-apps/tauri-action@v0", // Tauri v2 bundler action
        "TAURI_SIGNING_PRIVATE_KEY",  // updater signing key (CI secret)
        "TAURI_SIGNING_PRIVATE_KEY_PASSWORD",
        "APPLE_CERTIFICATE", // macOS codesigning + notarization
        "APPLE_CERTIFICATE_PASSWORD",
        "APPLE_SIGNING_IDENTITY",
        "APPLE_ID",
        "APPLE_PASSWORD",
        "APPLE_TEAM_ID",
        "WINDOWS_PFX_BASE64", // Windows Authenticode (CI secret)
        "windows-latest",
        "macos-15",
        "macos-15-intel",
        "ubuntu-22.04",    // ADR-0005 floor
        "contents: write", // publish the GitHub Release
    ] {
        assert!(yml.contains(needle), "release.yml must contain {needle:?}");
    }

    // No secret material may ever be committed: the workflow reads secrets,
    // it never contains key bytes.
    assert!(!yml.contains("dW50cnVzdGVk"), "release.yml must not inline key material");
}
