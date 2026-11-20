//! Email photos (TASK-08-006, PLAN §2J Email, P4).
//!
//! The photos are resized into a scratch folder by the same export path the
//! Export dialog uses (edits baked in, never upscaled), then handed to the
//! operating system's mail flow. Originals are never opened for writing.
//!
//! Delivery tries, in order:
//! 1. a native composer that takes attachments — Mail.app through
//!    `osascript` on macOS, `xdg-email --attach` on Linux;
//! 2. a `mailto:` compose window plus the scratch folder revealed in the
//!    file manager, so the user can drag the files in. This is the only
//!    flow on Windows, and the safety net everywhere else.
//!
//! A true share sheet (NSSharingService / WinRT DataTransferManager) is not
//! wired: Tauri v2 has no official share plugin for desktop, and the flows
//! above work on every supported OS without a new dependency.
//!
//! Every external program is run with an argument vector (never a shell
//! string), and file paths travel as separate arguments — never as script
//! text.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime};

use fotobase_catalog::Catalog;

use crate::export_dialog::{
    export_photos, ExportDialogError, ExportRequest, ExportedFile, FailedExport,
};

/// Longest edge of an emailed photo unless the user picks another size.
pub const DEFAULT_MAX_EDGE: u32 = 1600;
/// JPEG quality of the "Normal" export preset.
pub const EMAIL_QUALITY: u8 = 85;
/// Scratch folders older than this are removed on the next email.
pub const STALE_AFTER: Duration = Duration::from_secs(24 * 3600);

/// Size the user confirmed in the Email dialog.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize)]
pub struct EmailRequest {
    /// Longest-edge pixel limit of the resized copies.
    pub max_edge: u32,
}

impl Default for EmailRequest {
    fn default() -> Self {
        Self { max_edge: DEFAULT_MAX_EDGE }
    }
}

/// Resized copies waiting in the scratch folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreparedEmail {
    pub dir: PathBuf,
    pub exported: Vec<ExportedFile>,
    /// Photos that could not be resized; the rest of the batch goes on.
    pub failed: Vec<FailedExport>,
}

impl PreparedEmail {
    /// The resized files, in the caller's photo order.
    pub fn files(&self) -> Vec<PathBuf> {
        self.exported.iter().map(|f| f.path.clone()).collect()
    }
}

#[derive(Debug, thiserror::Error)]
pub enum EmailError {
    #[error("could not create the scratch folder: {0}")]
    Scratch(#[from] std::io::Error),
    #[error(transparent)]
    Export(#[from] ExportDialogError),
    #[error("no photo could be resized for email")]
    NothingExported,
}

#[derive(Debug, thiserror::Error)]
pub enum MailError {
    #[error("{0}")]
    Unavailable(String),
    #[error("could not run {program}: {source}")]
    Spawn { program: String, source: std::io::Error },
    #[error("{program} failed: {message}")]
    Failed { program: String, message: String },
}

/// How the photos reached the mail flow.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Delivery {
    /// A compose window opened with the photos attached.
    Attached,
    /// A compose window opened without attachments and the scratch folder
    /// was revealed for drag-and-drop.
    MailtoRevealed,
}

/// The OS mail flow behind [`deliver`].
pub trait Mailer {
    /// Open a compose window with `files` attached.
    fn compose_with_attachments(&self, files: &[PathBuf]) -> Result<(), MailError>;
    /// Open a compose window for a `mailto:` URL.
    fn open_mailto(&self, url: &str) -> Result<(), MailError>;
    /// Show `path` in the file manager.
    fn reveal(&self, path: &Path) -> Result<(), MailError>;
}

static SEQ: AtomicU64 = AtomicU64::new(0);

/// Remove scratch folders under `root` that are older than `max_age` at
/// `now`. Best effort: anything that cannot be read or removed is left.
pub fn sweep_stale(root: &Path, now: SystemTime, max_age: Duration) {
    let Ok(entries) = std::fs::read_dir(root) else { return };
    for entry in entries.flatten() {
        let stale = entry
            .metadata()
            .and_then(|m| m.modified())
            .ok()
            .and_then(|mtime| now.duration_since(mtime).ok())
            .is_some_and(|age| age >= max_age);
        if stale && entry.path().is_dir() {
            let _ = std::fs::remove_dir_all(entry.path());
        }
    }
}

fn new_scratch_dir(root: &Path) -> std::io::Result<PathBuf> {
    std::fs::create_dir_all(root)?;
    let nanos =
        SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).map_or(0, |d| d.as_nanos());
    loop {
        let dir = root.join(format!("email-{nanos}-{}", SEQ.fetch_add(1, Ordering::Relaxed)));
        match std::fs::create_dir(&dir) {
            Ok(()) => return Ok(dir),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        }
    }
}

/// Resize the photos `ids` (in the caller's order) into a fresh folder under
/// `root`, reporting progress `(done, total)` after each photo.
pub fn prepare_email(
    cat: &Catalog,
    ids: &[i64],
    req: &EmailRequest,
    root: &Path,
    progress: &mut dyn FnMut(usize, usize),
) -> Result<PreparedEmail, EmailError> {
    sweep_stale(root, SystemTime::now(), STALE_AFTER);
    let dir = new_scratch_dir(root)?;
    let export = ExportRequest {
        resize_max_width: Some(req.max_edge),
        resize_max_height: Some(req.max_edge),
        quality: EMAIL_QUALITY,
        numbered: false,
        watermark: None,
    };
    let batch = match export_photos(cat, ids, &dir, &export, progress) {
        Ok(batch) => batch,
        Err(e) => {
            let _ = std::fs::remove_dir_all(&dir);
            return Err(e.into());
        }
    };
    if batch.exported.is_empty() {
        let _ = std::fs::remove_dir_all(&dir);
        return Err(EmailError::NothingExported);
    }
    Ok(PreparedEmail { dir, exported: batch.exported, failed: batch.failed })
}

/// Hand `files` to the mail flow: native composer first, `mailto:` plus a
/// revealed folder when that is unavailable or fails.
pub fn deliver(files: &[PathBuf], mailer: &dyn Mailer) -> Result<Delivery, MailError> {
    if mailer.compose_with_attachments(files).is_ok() {
        return Ok(Delivery::Attached);
    }
    mailer.open_mailto(&mailto_url(
        "Photos",
        "The photos are in the folder that just opened: drag them into this message.",
    ))?;
    if let Some(first) = files.first() {
        // The message is open already; a failed reveal must not hide that.
        let _ = mailer.reveal(first);
    }
    Ok(Delivery::MailtoRevealed)
}

/// `mailto:` URL with percent-encoded subject and body.
pub fn mailto_url(subject: &str, body: &str) -> String {
    format!("mailto:?subject={}&body={}", percent_encode(subject), percent_encode(body))
}

fn percent_encode(s: &str) -> String {
    use std::fmt::Write as _;
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(b as char);
        } else {
            let _ = write!(out, "%{b:02X}");
        }
    }
    out
}

/// `xdg-email` arguments attaching each file.
pub fn xdg_email_args(files: &[PathBuf]) -> Vec<String> {
    files.iter().flat_map(|f| ["--attach".to_owned(), f.to_string_lossy().into_owned()]).collect()
}

/// AppleScript run by `osascript`; the files arrive as `argv`.
const MAIL_SCRIPT: &str = r#"on run argv
  tell application "Mail"
    set msg to make new outgoing message with properties {visible:true}
    tell msg
      repeat with f in argv
        make new attachment with properties {file name:((contents of f) as POSIX file)} at after the last paragraph
      end repeat
    end tell
    activate
  end tell
end run"#;

/// `osascript` arguments: the script, then each file path as `argv`.
pub fn osascript_args(files: &[PathBuf]) -> Vec<String> {
    let mut args = vec!["-e".to_owned(), MAIL_SCRIPT.to_owned()];
    args.extend(files.iter().map(|f| f.to_string_lossy().into_owned()));
    args
}

fn run(program: &str, args: &[String]) -> Result<(), MailError> {
    let out = Command::new(program)
        .args(args)
        .output()
        .map_err(|source| MailError::Spawn { program: program.to_owned(), source })?;
    if out.status.success() {
        Ok(())
    } else {
        Err(MailError::Failed {
            program: program.to_owned(),
            message: String::from_utf8_lossy(&out.stderr).trim().to_owned(),
        })
    }
}

/// The real platform launchers.
pub struct SystemMailer;

impl Mailer for SystemMailer {
    fn compose_with_attachments(&self, files: &[PathBuf]) -> Result<(), MailError> {
        if cfg!(target_os = "macos") {
            run("osascript", &osascript_args(files))
        } else if cfg!(target_os = "linux") {
            run("xdg-email", &xdg_email_args(files))
        } else {
            Err(MailError::Unavailable("no attachment-capable mail launcher".into()))
        }
    }

    fn open_mailto(&self, url: &str) -> Result<(), MailError> {
        tauri_plugin_opener::open_url(url, None::<&str>)
            .map_err(|e| MailError::Unavailable(format!("open mailto: {e}")))
    }

    fn reveal(&self, path: &Path) -> Result<(), MailError> {
        tauri_plugin_opener::reveal_item_in_dir(path)
            .map_err(|e| MailError::Unavailable(format!("reveal {}: {e}", path.display())))
    }
}
