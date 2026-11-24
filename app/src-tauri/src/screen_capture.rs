//! Screen capture folder (TASK-08-019, PLAN §2 "Screen capture").
//!
//! Opt-in: with the `screen_capture_enabled` setting on, a global hotkey
//! (PrtScn; F13 on macOS, whose keyboards have no PrtScn) saves a PNG of the
//! screen into `<catalog dir>/Screen Captures/`. That folder is registered as
//! a normal Scan Always folder, so the existing watcher puts the capture in
//! the grid — nothing here touches the catalog directly.
//!
//! Nothing registers the hotkey and nothing captures unless the user opted
//! in. The OS grabbers go through the platform's own permission flow
//! (macOS prompts once; without permission the tool fails and no file is
//! written). The pure parts — naming, consent gating, hotkey sync — are
//! unit-tested through the [`ScreenGrabber`] / [`HotkeyRegistrar`] seams.

use std::path::{Path, PathBuf};
use std::process::Command;
#[cfg(desktop)]
use std::sync::atomic::{AtomicBool, Ordering};

use serde::{Deserialize, Serialize};
use tauri::AppHandle;
#[cfg(desktop)]
use tauri::Manager;
#[cfg(desktop)]
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};

use fotobase_catalog::ScanMode;

use crate::folder_manager::FolderManager;
use crate::settings::Settings;

/// Folder name under the catalog directory.
pub const FOLDER_NAME: &str = "Screen Captures";

/// The capture hotkey: PrtScn, or F13 (the PrtScn position on extended Mac
/// keyboards) on macOS, where Cmd+Shift+3/4/5 belong to the system.
pub const HOTKEY: &str = if cfg!(target_os = "macos") { "F13" } else { "PrintScreen" };

/// Multi-monitor choice (a simple setting, TASK-08-019).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CaptureDisplay {
    /// Every display (one image per display on macOS, one stitched image
    /// elsewhere).
    #[default]
    All,
    /// A single display: the one under the pointer on Windows, the main one
    /// on macOS, the compositor's current one where the tool supports it.
    Active,
}

#[derive(Debug, thiserror::Error)]
pub enum CaptureError {
    /// The OS refused (no screen-recording permission) or no capture tool
    /// is available; nothing was written.
    #[error("screen capture failed: {0}")]
    Failed(String),
    #[error("saving screen capture: {0}")]
    Io(#[from] std::io::Error),
}

/// What a capture attempt did.
#[derive(Debug, PartialEq, Eq)]
pub enum CaptureOutcome {
    /// The feature is off: no consent, so no capture was attempted.
    Disabled,
    Saved(Vec<PathBuf>),
}

/// Takes the screenshot: writes one or more PNGs into `out_dir`.
pub trait ScreenGrabber {
    fn grab(&self, display: CaptureDisplay, out_dir: &Path) -> Result<Vec<PathBuf>, CaptureError>;
}

/// `<catalog dir>/Screen Captures`.
pub fn captures_dir(catalog_dir: &Path) -> PathBuf {
    catalog_dir.join(FOLDER_NAME)
}

/// `Screen Capture 2026-11-23 17.13.47` (no extension; `.` keeps the name
/// valid on Windows). `n` is the 0-based image index when one capture
/// produces several.
fn capture_stem(now: &jiff::ZonedDateTime, n: usize) -> String {
    let base = format!("Screen Capture {}", now.strftime("%Y-%m-%d %H.%M.%S"));
    if n == 0 {
        base
    } else {
        format!("{base} ({})", n + 1)
    }
}

/// A path under `folder` that does not exist yet (two captures within the
/// same second must not overwrite each other).
fn unique_path(folder: &Path, stem: &str) -> PathBuf {
    let first = folder.join(format!("{stem}.png"));
    if !first.exists() {
        return first;
    }
    (2..)
        .map(|i| folder.join(format!("{stem} - {i}.png")))
        .find(|p| !p.exists())
        .expect("unbounded range")
}

/// Capture into `folder` — only when the user opted in. Images are grabbed
/// into a staging directory beside the folder and renamed in, so the watcher
/// never sees a half-written PNG.
pub fn capture_to_folder(
    settings: &Settings,
    folder: &Path,
    grabber: &dyn ScreenGrabber,
    now: &jiff::ZonedDateTime,
) -> Result<CaptureOutcome, CaptureError> {
    if !settings.screen_capture_enabled {
        return Ok(CaptureOutcome::Disabled);
    }
    std::fs::create_dir_all(folder)?;
    let staging_root = folder.parent().unwrap_or(folder);
    let staging = tempfile::Builder::new().prefix(".fb-capture-").tempdir_in(staging_root)?;
    let mut images = grabber.grab(settings.screen_capture_display, staging.path())?;
    images.sort();
    if images.is_empty() {
        return Err(CaptureError::Failed("the OS produced no image".into()));
    }
    let mut saved = Vec::with_capacity(images.len());
    for (i, image) in images.iter().enumerate() {
        let dest = unique_path(folder, &capture_stem(now, i));
        std::fs::rename(image, &dest)?;
        saved.push(dest);
    }
    Ok(CaptureOutcome::Saved(saved))
}

/// The OS capture tools, one implementation per platform.
pub struct SystemGrabber;

impl ScreenGrabber for SystemGrabber {
    fn grab(&self, display: CaptureDisplay, out_dir: &Path) -> Result<Vec<PathBuf>, CaptureError> {
        grab_system(display, out_dir)
    }
}

fn run_ok(cmd: &mut Command) -> Result<(), CaptureError> {
    let out = cmd.output().map_err(|e| CaptureError::Failed(format!("{cmd:?}: {e}")))?;
    if out.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&out.stderr);
    Err(CaptureError::Failed(format!("{:?} exited with {}: {}", cmd, out.status, stderr.trim())))
}

fn non_empty_files(paths: impl IntoIterator<Item = PathBuf>) -> Vec<PathBuf> {
    paths.into_iter().filter(|p| p.metadata().is_ok_and(|m| m.len() > 0)).collect()
}

#[cfg(target_os = "macos")]
fn grab_system(display: CaptureDisplay, out_dir: &Path) -> Result<Vec<PathBuf>, CaptureError> {
    // `screencapture` is the system tool: it triggers the one-time
    // screen-recording prompt and fails when permission is missing.
    if display == CaptureDisplay::Active {
        let out = out_dir.join("main.png");
        run_ok(Command::new("screencapture").args(["-x", "-m", "-t", "png"]).arg(&out))?;
        return Ok(non_empty_files([out]));
    }
    // One file per display (`-D n`), until the first index that does not
    // exist.
    let mut images = Vec::new();
    for n in 1..=16 {
        let out = out_dir.join(format!("display-{n:02}.png"));
        let res = run_ok(
            Command::new("screencapture").args(["-x", "-t", "png", "-D", &n.to_string()]).arg(&out),
        );
        match res {
            Ok(()) => images.push(out),
            Err(e) if images.is_empty() => return Err(e),
            Err(_) => break,
        }
    }
    Ok(non_empty_files(images))
}

#[cfg(windows)]
fn grab_system(display: CaptureDisplay, out_dir: &Path) -> Result<Vec<PathBuf>, CaptureError> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    // GDI `CopyFromScreen`; DPI-aware so the bounds are physical pixels.
    const SCRIPT: &str = r#"
Add-Type -AssemblyName System.Windows.Forms,System.Drawing
Add-Type -TypeDefinition 'using System.Runtime.InteropServices; public class FbDpi { [DllImport("user32.dll")] public static extern bool SetProcessDPIAware(); }'
[void][FbDpi]::SetProcessDPIAware()
if ($env:FB_CAPTURE_DISPLAY -eq 'active') {
  $b = [System.Windows.Forms.Screen]::FromPoint([System.Windows.Forms.Cursor]::Position).Bounds
} else {
  $b = [System.Windows.Forms.SystemInformation]::VirtualScreen
}
$bmp = New-Object System.Drawing.Bitmap $b.Width, $b.Height
$g = [System.Drawing.Graphics]::FromImage($bmp)
$g.CopyFromScreen($b.Location, [System.Drawing.Point]::Empty, $b.Size)
$bmp.Save($env:FB_CAPTURE_OUT, [System.Drawing.Imaging.ImageFormat]::Png)
"#;
    let out = out_dir.join("screen.png");
    run_ok(
        Command::new("powershell.exe")
            .args(["-NoProfile", "-NonInteractive", "-Command", SCRIPT])
            .env("FB_CAPTURE_OUT", &out)
            .env(
                "FB_CAPTURE_DISPLAY",
                if display == CaptureDisplay::Active { "active" } else { "all" },
            )
            .creation_flags(CREATE_NO_WINDOW),
    )?;
    Ok(non_empty_files([out]))
}

#[cfg(all(unix, not(target_os = "macos")))]
fn grab_system(display: CaptureDisplay, out_dir: &Path) -> Result<Vec<PathBuf>, CaptureError> {
    let out = out_dir.join("screen.png");
    let o = out.to_string_lossy().into_owned();
    let mut tools: Vec<(&str, Vec<String>)> = Vec::new();
    if display == CaptureDisplay::Active {
        tools.push(("spectacle", ["-b", "-n", "-m", "-o", &o].map(String::from).to_vec()));
    }
    tools.push(("gnome-screenshot", vec!["-f".into(), o.clone()]));
    tools.push(("spectacle", ["-b", "-n", "-f", "-o", &o].map(String::from).to_vec()));
    tools.push(("grim", vec![o.clone()]));
    tools.push(("scrot", vec![o]));
    let mut last = CaptureError::Failed(
        "no screenshot tool found (tried gnome-screenshot, spectacle, grim, scrot)".into(),
    );
    for (program, args) in tools {
        match run_ok(Command::new(program).args(&args)) {
            Ok(()) if !non_empty_files([out.clone()]).is_empty() => {
                return Ok(vec![out]);
            }
            Ok(()) => {}
            Err(e) => {
                // Not installed → try the next tool; keep the most useful error.
                if !e.to_string().contains("No such file") {
                    last = e;
                }
            }
        }
    }
    Err(last)
}

/// Hotkey registration seam (the real one is the Tauri global-shortcut
/// plugin).
pub trait HotkeyRegistrar {
    fn is_registered(&self, shortcut: &str) -> bool;
    fn register(&self, shortcut: &str) -> Result<(), String>;
    fn unregister(&self, shortcut: &str) -> Result<(), String>;
}

/// Make the registered hotkey match the opt-in: disabled registers nothing
/// (and releases a hotkey a previous opt-in left behind).
pub fn sync_hotkey(registrar: &impl HotkeyRegistrar, enabled: bool) -> Result<(), String> {
    let registered = registrar.is_registered(HOTKEY);
    match (enabled, registered) {
        (true, false) => registrar.register(HOTKEY),
        (false, true) => registrar.unregister(HOTKEY),
        _ => Ok(()),
    }
}

/// The Tauri global-shortcut plugin as a [`HotkeyRegistrar`].
pub struct TauriHotkeys<'a>(pub &'a AppHandle);

/// Mobile has no global shortcuts and no screen-capture folder
/// (TASK-09-001): nothing is ever registered, enabling it reports why.
#[cfg(mobile)]
impl HotkeyRegistrar for TauriHotkeys<'_> {
    fn is_registered(&self, _shortcut: &str) -> bool {
        false
    }
    fn register(&self, _shortcut: &str) -> Result<(), String> {
        Err("screen capture hotkey is not available on mobile".into())
    }
    fn unregister(&self, _shortcut: &str) -> Result<(), String> {
        Ok(())
    }
}

#[cfg(desktop)]
impl HotkeyRegistrar for TauriHotkeys<'_> {
    fn is_registered(&self, shortcut: &str) -> bool {
        self.0.global_shortcut().is_registered(shortcut)
    }
    fn register(&self, shortcut: &str) -> Result<(), String> {
        self.0.global_shortcut().register(shortcut).map_err(|e| format!("register {shortcut}: {e}"))
    }
    fn unregister(&self, shortcut: &str) -> Result<(), String> {
        self.0
            .global_shortcut()
            .unregister(shortcut)
            .map_err(|e| format!("unregister {shortcut}: {e}"))
    }
}

/// The plugin: its handler runs a capture on every key press. It registers
/// no shortcut itself — that only happens through [`sync_hotkey`].
#[cfg(desktop)]
pub fn plugin() -> tauri::plugin::TauriPlugin<tauri::Wry> {
    tauri_plugin_global_shortcut::Builder::new()
        .with_handler(|app, _shortcut, event| {
            if event.state() == ShortcutState::Pressed {
                spawn_capture(app.clone());
            }
        })
        .build()
}

#[cfg(desktop)]
static CAPTURING: AtomicBool = AtomicBool::new(false);

/// Capture on a blocking thread; a press while one is running is dropped
/// (key repeat must not stack grabs).
#[cfg(desktop)]
fn spawn_capture(app: AppHandle) {
    if CAPTURING.swap(true, Ordering::SeqCst) {
        return;
    }
    tauri::async_runtime::spawn_blocking(move || {
        if let Err(e) = capture_now(&app) {
            tracing::warn!("screen capture: {e}");
        }
        CAPTURING.store(false, Ordering::SeqCst);
    });
}

#[cfg(desktop)]
fn capture_now(app: &AppHandle) -> Result<(), String> {
    let state = app.state::<crate::AppState>();
    let settings =
        state.settings.lock().map_err(|e| format!("settings mutex poisoned: {e}"))?.clone();
    let catalog_dir = state.catalog_dir();
    crate::with_folder_manager(&state, |fm| {
        capture_and_watch(&settings, &catalog_dir, fm, &SystemGrabber, &jiff::ZonedDateTime::now())
            .map(|_| ())
    })
}

/// One hotkey press: make sure `Screen Captures` is a Scan Always folder,
/// then capture into it. The watcher lists the new PNG in the grid. Off
/// (no consent): neither the folder nor the capture happens.
pub fn capture_and_watch(
    settings: &Settings,
    catalog_dir: &Path,
    folders: &FolderManager,
    grabber: &dyn ScreenGrabber,
    now: &jiff::ZonedDateTime,
) -> Result<CaptureOutcome, String> {
    if !settings.screen_capture_enabled {
        return Ok(CaptureOutcome::Disabled);
    }
    let folder = captures_dir(catalog_dir);
    std::fs::create_dir_all(&folder).map_err(|e| e.to_string())?;
    folders.set_mode(&folder.to_string_lossy(), ScanMode::Always)?;
    capture_to_folder(settings, &folder, grabber, now).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    struct FakeGrabber {
        images: usize,
        seen: RefCell<Vec<CaptureDisplay>>,
    }

    impl ScreenGrabber for FakeGrabber {
        fn grab(
            &self,
            display: CaptureDisplay,
            out_dir: &Path,
        ) -> Result<Vec<PathBuf>, CaptureError> {
            self.seen.borrow_mut().push(display);
            (0..self.images)
                .map(|i| {
                    let p = out_dir.join(format!("d{i}.png"));
                    std::fs::write(&p, b"\x89PNG").map(|()| p).map_err(Into::into)
                })
                .collect()
        }
    }

    fn at(ts: &str) -> jiff::ZonedDateTime {
        ts.parse().unwrap()
    }

    fn opted_in() -> Settings {
        Settings { screen_capture_enabled: true, ..Settings::default() }
    }

    #[test]
    fn disabled_means_no_grab_and_no_folder() {
        let dir = tempfile::tempdir().unwrap();
        let folder = captures_dir(dir.path());
        let grabber = FakeGrabber { images: 1, seen: RefCell::default() };
        let out = capture_to_folder(
            &Settings::default(),
            &folder,
            &grabber,
            &at("2026-11-23T17:13:47+01:00[+01:00]"),
        )
        .unwrap();
        assert_eq!(out, CaptureOutcome::Disabled);
        assert!(grabber.seen.borrow().is_empty());
        assert!(!folder.exists());
    }

    #[test]
    fn saves_a_timestamped_png_and_leaves_no_staging() {
        let dir = tempfile::tempdir().unwrap();
        let folder = captures_dir(dir.path());
        let grabber = FakeGrabber { images: 1, seen: RefCell::default() };
        let out = capture_to_folder(
            &opted_in(),
            &folder,
            &grabber,
            &at("2026-11-23T17:13:47+01:00[+01:00]"),
        )
        .unwrap();
        let expected = folder.join("Screen Capture 2026-11-23 17.13.47.png");
        assert_eq!(out, CaptureOutcome::Saved(vec![expected.clone()]));
        assert!(expected.is_file());
        let leftovers: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(leftovers, vec![FOLDER_NAME.to_string()]);
    }

    #[test]
    fn same_second_and_multi_display_never_overwrite() {
        let dir = tempfile::tempdir().unwrap();
        let folder = captures_dir(dir.path());
        let now = at("2026-11-23T17:13:47+01:00[+01:00]");
        let grabber = FakeGrabber { images: 2, seen: RefCell::default() };
        capture_to_folder(&opted_in(), &folder, &grabber, &now).unwrap();
        capture_to_folder(&opted_in(), &folder, &grabber, &now).unwrap();
        let mut names: Vec<_> = std::fs::read_dir(&folder)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        assert_eq!(names.len(), 4, "{names:?}");
    }

    #[test]
    fn the_display_setting_reaches_the_grabber() {
        let dir = tempfile::tempdir().unwrap();
        let grabber = FakeGrabber { images: 1, seen: RefCell::default() };
        let settings = Settings { screen_capture_display: CaptureDisplay::Active, ..opted_in() };
        capture_to_folder(
            &settings,
            &captures_dir(dir.path()),
            &grabber,
            &at("2026-11-23T17:13:47+01:00[+01:00]"),
        )
        .unwrap();
        assert_eq!(*grabber.seen.borrow(), vec![CaptureDisplay::Active]);
    }

    #[test]
    fn a_failed_grab_writes_nothing() {
        struct Denied;
        impl ScreenGrabber for Denied {
            fn grab(&self, _: CaptureDisplay, _: &Path) -> Result<Vec<PathBuf>, CaptureError> {
                Err(CaptureError::Failed("permission denied".into()))
            }
        }
        let dir = tempfile::tempdir().unwrap();
        let folder = captures_dir(dir.path());
        let err = capture_to_folder(
            &opted_in(),
            &folder,
            &Denied,
            &at("2026-11-23T17:13:47+01:00[+01:00]"),
        )
        .unwrap_err();
        assert!(err.to_string().contains("permission denied"));
        assert_eq!(std::fs::read_dir(&folder).unwrap().count(), 0);
    }

    #[derive(Default)]
    struct FakeRegistrar {
        registered: RefCell<bool>,
        calls: RefCell<Vec<&'static str>>,
    }

    impl HotkeyRegistrar for FakeRegistrar {
        fn is_registered(&self, _: &str) -> bool {
            *self.registered.borrow()
        }
        fn register(&self, _: &str) -> Result<(), String> {
            self.calls.borrow_mut().push("register");
            *self.registered.borrow_mut() = true;
            Ok(())
        }
        fn unregister(&self, _: &str) -> Result<(), String> {
            self.calls.borrow_mut().push("unregister");
            *self.registered.borrow_mut() = false;
            Ok(())
        }
    }

    #[test]
    fn opt_out_registers_no_hotkey() {
        let reg = FakeRegistrar::default();
        sync_hotkey(&reg, false).unwrap();
        assert!(reg.calls.borrow().is_empty());
    }

    #[test]
    fn opt_in_registers_once_and_opt_out_releases() {
        let reg = FakeRegistrar::default();
        sync_hotkey(&reg, true).unwrap();
        sync_hotkey(&reg, true).unwrap();
        sync_hotkey(&reg, false).unwrap();
        assert_eq!(*reg.calls.borrow(), vec!["register", "unregister"]);
    }
}
