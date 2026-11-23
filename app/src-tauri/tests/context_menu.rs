//! TASK-08-020 integration: the file-manager entries ("View with Fotobase",
//! "Upload with Fotobase") install into the per-OS locations, launch the
//! app with the right flag and file, and uninstall without leaving anything
//! behind. The Windows registry backend is covered by the `cfg(windows)`
//! tests at the bottom; Finder/Nautilus UI appearance is a per-OS release
//! checklist item.

use std::path::{Path, PathBuf};

use fotobase_lib::context_menu::{
    linux_install, linux_installed, linux_remove, mac_install, mac_installed, mac_remove,
    parse_launch_args, LaunchAction,
};

fn args(items: &[&str]) -> Vec<String> {
    items.iter().map(|s| s.to_string()).collect()
}

fn jpeg_in(dir: &Path, name: &str) -> String {
    let path = dir.join(name);
    std::fs::write(&path, b"\xff\xd8\xff").unwrap();
    path.to_string_lossy().into_owned()
}

fn is_empty_dir(dir: &Path) -> bool {
    std::fs::read_dir(dir).map(|mut d| d.next().is_none()).unwrap_or(true)
}

#[test]
fn view_flag_routes_files_to_the_viewer() {
    let dir = tempfile::tempdir().unwrap();
    let photo = jpeg_in(dir.path(), "a.jpg");
    let req = parse_launch_args(&args(&["--view", &photo]));
    assert_eq!(req.action, LaunchAction::View);
    assert_eq!(req.paths, vec![photo]);
}

#[test]
fn upload_flag_routes_files_to_the_upload_dialog() {
    let dir = tempfile::tempdir().unwrap();
    let a = jpeg_in(dir.path(), "a.jpg");
    let b = jpeg_in(dir.path(), "b.PNG");
    let req = parse_launch_args(&args(&["--upload", &a, &b]));
    assert_eq!(req.action, LaunchAction::Upload);
    assert_eq!(req.paths, vec![a, b]);
}

#[test]
fn bare_paths_keep_the_file_association_behavior() {
    // TASK-01-025: `Exec %f` / `"%1"` pass the file with no flag.
    let dir = tempfile::tempdir().unwrap();
    let photo = jpeg_in(dir.path(), "a.jpg");
    let req = parse_launch_args(&args(&[&photo]));
    assert_eq!(req.action, LaunchAction::View);
    assert_eq!(req.paths, vec![photo]);
}

#[test]
fn missing_unsupported_and_unknown_flag_args_never_block_startup() {
    let dir = tempfile::tempdir().unwrap();
    let text = dir.path().join("notes.txt");
    std::fs::write(&text, b"x").unwrap();
    let ok = jpeg_in(dir.path(), "ok.jpg");
    let req = parse_launch_args(&args(&[
        "--upload",
        "--bogus",
        "/no/such/file.jpg",
        text.to_str().unwrap(),
        &ok,
    ]));
    assert_eq!(req.action, LaunchAction::Upload);
    assert_eq!(req.paths, vec![ok]);
    assert!(parse_launch_args(&[]).paths.is_empty());
}

#[test]
fn nautilus_scripts_install_launch_the_exe_with_the_flag_and_uninstall_clean() {
    let dir = tempfile::tempdir().unwrap();
    let scripts = dir.path().join("nautilus/scripts");
    let exe = Path::new("/opt/Foto base/it's");

    assert!(!linux_installed(&scripts));
    linux_install(&scripts, exe).unwrap();
    assert!(linux_installed(&scripts));

    let view = std::fs::read_to_string(scripts.join("View with Fotobase")).unwrap();
    let upload = std::fs::read_to_string(scripts.join("Upload with Fotobase")).unwrap();
    assert!(view.starts_with("#!/bin/sh\n"));
    // Path with a space and a quote survives shell quoting.
    assert!(view.contains(r#"EXE='/opt/Foto base/it'\''s'"#), "{view}");
    assert!(view.contains(r#"exec "$EXE" --view "$@""#));
    assert!(upload.contains(r#"exec "$EXE" --upload "$@""#));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(scripts.join("View with Fotobase")).unwrap().permissions();
        assert_eq!(mode.mode() & 0o111, 0o111, "Nautilus only lists executable scripts");
    }

    // Foreign scripts in the folder are never touched.
    std::fs::write(scripts.join("mine.sh"), "#!/bin/sh\n").unwrap();
    linux_remove(&scripts).unwrap();
    assert!(!linux_installed(&scripts));
    assert!(scripts.join("mine.sh").exists());
    // Removing twice is fine.
    linux_remove(&scripts).unwrap();
}

#[cfg(unix)]
#[test]
fn nautilus_script_removes_itself_when_the_app_is_gone() {
    // No uninstall hook exists for user-level files on Linux: the script
    // cleans itself up the first time it runs without the app.
    use std::process::Command;
    let dir = tempfile::tempdir().unwrap();
    let scripts = dir.path().join("scripts");
    linux_install(&scripts, &dir.path().join("gone/fotobase")).unwrap();
    let script = scripts.join("View with Fotobase");
    let status = Command::new("/bin/sh").arg(&script).status().unwrap();
    assert!(status.success());
    assert!(!script.exists());
}

#[test]
fn mac_services_install_with_valid_plists_and_uninstall_clean() {
    let dir = tempfile::tempdir().unwrap();
    let services = dir.path().join("Library/Services");
    let exe = PathBuf::from("/Applications/Fotobase.app/Contents/MacOS/fotobase");

    assert!(!mac_installed(&services));
    mac_install(&services, &exe).unwrap();
    assert!(mac_installed(&services));

    for (name, flag) in [("View with Fotobase", "--view"), ("Upload with Fotobase", "--upload")] {
        let wf = services.join(format!("{name}.workflow/Contents"));
        let info = std::fs::read_to_string(wf.join("Info.plist")).unwrap();
        assert!(info.contains(name), "{info}");
        assert!(info.contains("public.image"));
        assert!(info.contains("com.apple.finder"));
        let doc = std::fs::read_to_string(wf.join("document.wflow")).unwrap();
        assert!(doc.contains(&format!("{flag} \"$@\"")), "{doc}");
        assert!(doc.contains("/Applications/Fotobase.app/Contents/MacOS/fotobase"));
        #[cfg(target_os = "macos")]
        for plist in ["Info.plist", "document.wflow"] {
            let out = std::process::Command::new("plutil")
                .arg("-lint")
                .arg(wf.join(plist))
                .output()
                .unwrap();
            assert!(out.status.success(), "{plist}: {}", String::from_utf8_lossy(&out.stdout));
        }
    }

    mac_remove(&services).unwrap();
    assert!(!mac_installed(&services));
    assert!(is_empty_dir(&services));
    mac_remove(&services).unwrap();
}

#[cfg(windows)]
mod windows {
    use fotobase_lib::context_menu::{win_install, win_installed, win_remove};
    use std::path::Path;

    // A throwaway HKCU subtree: the real SystemFileAssociations keys are
    // never touched by tests.
    const ROOT: &str = r"Software\Fotobase-Test\Classes";

    #[test]
    fn registry_verbs_install_and_uninstall_without_orphans() {
        let exe = Path::new(r"C:\Program Files\Fotobase\fotobase.exe");
        win_install(ROOT, exe).unwrap();
        assert!(win_installed(ROOT));
        win_remove(ROOT).unwrap();
        assert!(!win_installed(ROOT));
        win_remove(ROOT).unwrap();
        let _ = winreg::RegKey::predef(winreg::enums::HKEY_CURRENT_USER)
            .delete_subkey_all(r"Software\Fotobase-Test");
    }
}

#[test]
fn nsis_uninstall_hook_removes_every_registered_verb() {
    // The installer cannot call into Rust: its hand-kept key list must cover
    // every extension we register for, or uninstall leaves orphans.
    let hook =
        std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("windows/hooks.nsh"))
            .unwrap();
    assert!(hook.contains("!macro NSIS_HOOK_POSTUNINSTALL"));
    for ext in fotobase_lib::associations::IMAGE_EXTS {
        for verb in ["FotobaseView", "FotobaseUpload"] {
            let key = format!(r"Software\Classes\SystemFileAssociations\.{ext}\shell\{verb}");
            assert!(hook.contains(&format!("DeleteRegKey HKCU \"{key}\"")), "missing {key}");
        }
    }
}
