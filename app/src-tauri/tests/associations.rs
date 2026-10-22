//! File-association registration tests (TASK-01-025, PLAN §2M).
//!
//! Registering Fotobase as the OS default viewer for image types must be
//! reversible: unregistering restores the previous handler exactly ("no
//! orphans"). The per-OS write backends (Windows registry, macOS
//! LaunchServices, Linux `mimeapps.list`) are thin shells over pure logic
//! that is testable on every platform: the tests below drive the same
//! functions the real backends call, against temp dirs / in-memory JSON.

use std::path::PathBuf;

use fotobase_lib::associations::{
    collect_open_paths, ext_to_mime, linux_clear_default, linux_remove_desktop_file,
    linux_set_default, linux_write_desktop_file, mac_clear_handler, mac_handler_entry,
    mac_set_handler, normalize_ext, supported_extensions, validate, win_open_command, win_prog_id,
    PriorState, BUNDLE_ID,
};

// --- Supported types ---------------------------------------------------------

#[test]
fn supported_extensions_cover_the_core_raster_types() {
    let exts = supported_extensions();
    for core in ["jpg", "jpeg", "png", "gif", "tif", "tiff", "webp", "heic", "bmp"] {
        assert!(exts.contains(&core), "missing core extension: {core}");
    }
    for ext in exts {
        assert!(!ext.starts_with('.'), "extensions are stored without the dot: {ext}");
        assert_eq!(*ext, ext.to_ascii_lowercase());
    }
}

#[test]
fn mime_mapping_covers_every_supported_extension() {
    for ext in supported_extensions() {
        assert!(ext_to_mime(ext).is_some(), "no MIME type for supported extension {ext}");
    }
    assert_eq!(ext_to_mime("jpg"), Some("image/jpeg"));
    assert_eq!(ext_to_mime("jpeg"), Some("image/jpeg"));
    assert_eq!(ext_to_mime("png"), Some("image/png"));
    assert_eq!(ext_to_mime("txt"), None);
}

#[test]
fn normalize_trims_dot_and_case_and_rejects_unknown() {
    assert_eq!(normalize_ext(".JPG").as_deref(), Some("jpg"));
    assert_eq!(normalize_ext("Png").as_deref(), Some("png"));
    assert_eq!(normalize_ext("exe"), None);
    assert_eq!(normalize_ext(""), None);
}

#[test]
fn validate_accepts_supported_and_names_the_unknown() {
    let ok = ["png".to_string(), ".JPG".to_string()];
    assert!(validate(&ok).is_ok());
    let bad = ["png".to_string(), "exe".to_string()];
    let err = validate(&bad).unwrap_err();
    assert!(err.contains("exe"), "error must name the unknown extension: {err}");
}

// --- Windows (pure helpers; the registry I/O is a thin shell) ----------------

#[test]
fn windows_prog_id_and_open_command_are_well_formed() {
    assert_eq!(win_prog_id("jpg"), "FotoBase.jpg");
    let cmd = win_open_command(&PathBuf::from(r"C:\Apps\Fotobase.exe"));
    // The exe must be quoted (paths with spaces) and take the file as %1.
    assert!(cmd.contains(r#""C:\Apps\Fotobase.exe""#), "quoted exe: {cmd}");
    assert!(cmd.contains(r#""%1""#), "file argument: {cmd}");
}

// --- macOS (pure LSHandlers-array manipulation over JSON) --------------------

#[test]
fn mac_set_handler_adds_entry_and_preserves_others() {
    let handlers =
        r#"[{"LSHandlerContentType":"public.html","LSHandlerRoleAll":"com.apple.safari"}]"#;
    let entry = mac_handler_entry("jpg", BUNDLE_ID);
    assert_eq!(entry["LSHandlerContentTag"], "jpg");
    assert_eq!(entry["LSHandlerRoleAll"], BUNDLE_ID);

    let (new_json, prior) = mac_set_handler(handlers, "jpg", BUNDLE_ID).unwrap();
    assert!(prior.is_none(), "no prior handler for jpg in the fixture");
    let parsed: serde_json::Value = serde_json::from_str(&new_json).unwrap();
    let arr = parsed.as_array().unwrap();
    assert_eq!(arr.len(), 2, "safari entry preserved, fotobase entry added");
    assert!(arr.iter().any(|e| e["LSHandlerContentTag"] == "jpg"));
    assert!(arr.iter().any(|e| e["LSHandlerContentType"] == "public.html"));
}

#[test]
fn mac_set_handler_replaces_existing_entry_and_reports_it_as_prior() {
    let handlers = r#"[{"LSHandlerContentTag":"jpg","LSHandlerRoleAll":"org.other.viewer"}]"#;
    let (new_json, prior) = mac_set_handler(handlers, "jpg", BUNDLE_ID).unwrap();
    let prior = prior.expect("the existing handler is the prior state");
    assert_eq!(prior["LSHandlerRoleAll"], "org.other.viewer");
    let parsed: serde_json::Value = serde_json::from_str(&new_json).unwrap();
    let arr = parsed.as_array().unwrap();
    assert_eq!(arr.len(), 1, "replaced in place, never duplicated");
    assert_eq!(arr[0]["LSHandlerRoleAll"], BUNDLE_ID);
}

#[test]
fn mac_clear_handler_restores_prior_or_removes() {
    let ours = r#"[{"LSHandlerContentTag":"jpg","LSHandlerRoleAll":"io.github.fotobase.app"}]"#;
    // Prior known: restore it exactly.
    let prior_entry =
        serde_json::json!({"LSHandlerContentTag":"jpg","LSHandlerRoleAll":"org.other.viewer"});
    let restored = mac_clear_handler(ours, "jpg", Some(prior_entry)).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&restored).unwrap();
    assert_eq!(parsed[0]["LSHandlerRoleAll"], "org.other.viewer");
    // No prior: our entry is simply gone (no orphans).
    let cleared = mac_clear_handler(ours, "jpg", None).unwrap();
    assert_eq!(serde_json::from_str::<serde_json::Value>(&cleared).unwrap(), serde_json::json!([]));
    // Clearing an ext we never registered is a no-op, not an error.
    let untouched = mac_clear_handler(ours, "png", None).unwrap();
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&untouched).unwrap().as_array().unwrap().len(),
        1
    );
}

// --- Linux (mimeapps.list + desktop file, real file I/O in a temp dir) -------

#[test]
fn linux_mimeapps_set_then_clear_roundtrip() {
    let dir = tempfile::tempdir().unwrap();
    // First registration: no prior default existed.
    let prior = linux_set_default(dir.path(), "image/jpeg", "fotobase.desktop").unwrap();
    assert!(prior.is_none());
    let text = std::fs::read_to_string(dir.path().join("mimeapps.list")).unwrap();
    assert!(text.contains("[Default Applications]"));
    assert!(text.contains("image/jpeg=fotobase.desktop"));

    // Registering over an existing default reports it as the prior state.
    let prior = linux_set_default(dir.path(), "image/jpeg", "fotobase.desktop").unwrap();
    assert_eq!(prior.as_deref(), Some("fotobase.desktop"));

    // Restore the prior value explicitly.
    linux_clear_default(dir.path(), "image/jpeg", Some("org.gnome.eog.desktop")).unwrap();
    let text = std::fs::read_to_string(dir.path().join("mimeapps.list")).unwrap();
    assert!(text.contains("image/jpeg=org.gnome.eog.desktop"));
    assert!(!text.contains("fotobase.desktop"));

    // No prior: the line disappears, other entries survive.
    linux_set_default(dir.path(), "image/png", "fotobase.desktop").unwrap();
    linux_clear_default(dir.path(), "image/png", None).unwrap();
    let text = std::fs::read_to_string(dir.path().join("mimeapps.list")).unwrap();
    assert!(!text.contains("image/png="));
    assert!(text.contains("image/jpeg=org.gnome.eog.desktop"), "unrelated entries survive");
}

#[test]
fn linux_mimeapps_clear_of_unknown_mime_is_a_noop() {
    let dir = tempfile::tempdir().unwrap();
    linux_clear_default(dir.path(), "image/jpeg", None).unwrap();
    assert!(!dir.path().join("mimeapps.list").exists(), "no file created by a pure unregister");
}

#[test]
fn linux_desktop_file_written_with_mimes_and_removed() {
    let dir = tempfile::tempdir().unwrap();
    let apps = dir.path().join("applications");
    linux_write_desktop_file(
        &apps,
        &PathBuf::from("/usr/bin/fotobase"),
        &["image/jpeg", "image/png"],
    )
    .unwrap();
    let text = std::fs::read_to_string(apps.join("fotobase.desktop")).unwrap();
    assert!(text.contains("Exec=/usr/bin/fotobase %f"), "Exec with %f: {text}");
    assert!(text.contains("image/jpeg"), "MimeType list: {text}");
    assert!(text.contains("image/png"), "MimeType list: {text}");
    assert!(text.contains("image/jpeg;image/png"), "semicolon-terminated list: {text}");
    linux_remove_desktop_file(&apps).unwrap();
    assert!(!apps.join("fotobase.desktop").exists());
}

// --- Prior-state store --------------------------------------------------------

#[test]
fn prior_state_roundtrip_and_missing_file_defaults() {
    let dir = tempfile::tempdir().unwrap();
    let empty = PriorState::load(dir.path());
    assert!(empty.prior("jpg").is_none());

    let mut state = PriorState::default();
    state.set_prior("jpg", Some(serde_json::json!({"LSHandlerRoleAll": "org.other.viewer"})));
    state.set_prior("png", None);
    state.save(dir.path()).unwrap();

    let reloaded = PriorState::load(dir.path());
    assert_eq!(
        reloaded.prior("jpg").unwrap(),
        &serde_json::json!({"LSHandlerRoleAll": "org.other.viewer"})
    );
    assert!(reloaded.prior("png").is_none(), "explicit 'no prior' is remembered, not re-queried");
}

// --- OS-launched file routing --------------------------------------------------

#[test]
fn collect_open_paths_keeps_only_existing_supported_images() {
    let dir = tempfile::tempdir().unwrap();
    let photo = dir.path().join("IMG_0001.JPG");
    std::fs::write(&photo, b"x").unwrap();
    let missing = dir.path().join("gone.png");
    let note = dir.path().join("notes.txt");
    std::fs::write(&note, b"x").unwrap();

    let args = [
        photo.to_string_lossy().into_owned(),
        missing.to_string_lossy().into_owned(),
        note.to_string_lossy().into_owned(),
        "--flag".to_string(),
    ];
    let paths = collect_open_paths(&args);
    assert_eq!(paths, vec![photo.to_string_lossy().into_owned()]);
}
