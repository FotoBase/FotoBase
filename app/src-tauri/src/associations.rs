//! File-association registration (TASK-01-025, PLAN §2M "File associations:
//! default viewer for image types", P1).
//!
//! Fotobase registers itself as the OS default viewer for the image types
//! the user picks in the Options dialog, and unregistering restores the
//! previous handler exactly — no orphans. To make that reversible on every
//! OS, the *previous* handler per extension is captured at registration
//! time into `associations.json` in the app data dir and written back on
//! unregister.
//!
//! The per-OS write backends are thin shells (Windows `winreg`, macOS
//! LaunchServices via the `osascript` JXA bridge, Linux `mimeapps.list` +
//! a desktop file); everything they operate on — the ProgID/command
//! strings, the LSHandlers array JSON, the `mimeapps.list` text — lives in
//! pure functions below and is unit-tested on every platform.

use std::collections::BTreeMap;
use std::path::Path;
#[cfg(target_os = "linux")]
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// The app's bundle identifier (matches `tauri.conf.json`). macOS
/// LaunchServices and the Linux desktop file key registration off it.
pub const BUNDLE_ID: &str = "io.github.fotobase.app";

/// The Linux desktop-file id Fotobase registers as a default handler.
pub const DESKTOP_ID: &str = "fotobase.desktop";

/// Extensions Fotobase can register as the default viewer for — mirrors the
/// scanner's cataloged image types (`crates/scanner`): the OS can only be
/// asked to open file types the app actually reads.
pub const IMAGE_EXTS: &[&str] = &[
    "jpg", "jpeg", "png", "gif", "tif", "tiff", "webp", "heic", "heif", "avif", "bmp", "dng",
    "cr2", "cr3", "nef", "arw", "raf", "orf", "rw2",
];

pub fn supported_extensions() -> &'static [&'static str] {
    IMAGE_EXTS
}

/// MIME type for a supported extension (Linux `mimeapps.list` keys defaults
/// by MIME type, not extension). `None` for unsupported extensions.
pub fn ext_to_mime(ext: &str) -> Option<&'static str> {
    match ext {
        "jpg" | "jpeg" => Some("image/jpeg"),
        "png" => Some("image/png"),
        "gif" => Some("image/gif"),
        "tif" | "tiff" => Some("image/tiff"),
        "webp" => Some("image/webp"),
        "heic" => Some("image/heic"),
        "heif" => Some("image/heif"),
        "avif" => Some("image/avif"),
        "bmp" => Some("image/bmp"),
        "dng" => Some("image/x-adobe-dng"),
        "cr2" => Some("image/x-canon-cr2"),
        "cr3" => Some("image/x-canon-cr3"),
        "nef" => Some("image/x-nikon-nef"),
        "arw" => Some("image/x-sony-arw"),
        "raf" => Some("image/x-fuji-raf"),
        "orf" => Some("image/x-olympus-orf"),
        "rw2" => Some("image/x-panasonic-rw2"),
        _ => None,
    }
}

/// Normalize a UI-supplied extension: trim the dot, lowercase, and accept
/// only types we actually support. `None` → not a type we register for.
pub fn normalize_ext(ext: &str) -> Option<String> {
    let ext = ext.trim().trim_start_matches('.').to_ascii_lowercase();
    if IMAGE_EXTS.contains(&ext.as_str()) {
        Some(ext)
    } else {
        None
    }
}

/// Validate a registration request up front: every entry must be a
/// supported type, else the error names the offenders.
pub fn validate(exts: &[String]) -> Result<(), String> {
    let unknown: Vec<&String> = exts.iter().filter(|e| normalize_ext(e).is_none()).collect();
    if unknown.is_empty() {
        Ok(())
    } else {
        let names: Vec<String> = unknown.iter().map(|s| s.to_string()).collect();
        Err(format!("unsupported file type(s): {}", names.join(", ")))
    }
}

/// One extension's registration status, for the Options dialog.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AssociationStatus {
    pub ext: String,
    pub registered: bool,
}

/// Prior handler state, captured at registration time so unregistering can
/// restore exactly what the OS had before (`associations.json` in the app
/// data dir). A `None` entry means "nothing was registered before" and is
/// remembered as explicitly as a value — unregistering then removes our
/// entry instead of guessing.
#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
pub struct PriorState {
    entries: BTreeMap<String, serde_json::Value>,
}

impl PriorState {
    /// Load from `dir/associations.json`; missing or corrupt file → empty.
    pub fn load(dir: &Path) -> Self {
        match std::fs::read(dir.join("associations.json")) {
            Ok(bytes) => serde_json::from_slice(&bytes).unwrap_or_default(),
            Err(_) => Self::default(),
        }
    }

    /// Atomically persist (`.tmp` + rename, same discipline as settings).
    pub fn save(&self, dir: &Path) -> std::io::Result<()> {
        std::fs::create_dir_all(dir)?;
        let json = serde_json::to_vec_pretty(self).expect("prior state serializes");
        let tmp = dir.join("associations.json.tmp");
        std::fs::write(&tmp, json)?;
        std::fs::rename(&tmp, dir.join("associations.json"))
    }

    /// The captured prior handler for `ext`, if any was recorded.
    pub fn prior(&self, ext: &str) -> Option<&serde_json::Value> {
        self.entries.get(ext)
    }

    /// Record (or clear) the prior handler for `ext` before registering it.
    pub fn set_prior(&mut self, ext: &str, prior: Option<serde_json::Value>) {
        match prior {
            Some(v) => {
                self.entries.insert(ext.to_string(), v);
            }
            None => {
                self.entries.remove(ext);
            }
        }
    }
}

// --- Windows (registry) ------------------------------------------------------

/// The ProgID Fotobase registers per extension: `FotoBase.jpg` etc. One
/// ProgID per type keeps unregistration surgical — dropping our key never
/// touches another app's.
pub fn win_prog_id(ext: &str) -> String {
    format!("FotoBase.{ext}")
}

/// The `shell\open\command` value: quoted exe (paths with spaces) taking
/// the opened file as `%1`.
pub fn win_open_command(exe: &Path) -> String {
    format!("\"{}\" \"%1\"", exe.display())
}

// --- macOS (LaunchServices) --------------------------------------------------

/// The LSHandlers array entry for one extension: keyed by filename
/// extension (`public.filename-extension`), Viewer role for all.
pub fn mac_handler_entry(ext: &str, bundle_id: &str) -> serde_json::Value {
    serde_json::json!({
        "LSHandlerContentTag": ext,
        "LSHandlerContentTagClass": "public.filename-extension",
        "LSHandlerRoleAll": bundle_id,
    })
}

/// Insert/replace our entry for `ext` in a serialized LSHandlers array.
/// Returns the new array JSON plus the prior entry (the handler being
/// replaced), which the caller stores for unregistering. Other apps' entries
/// are preserved verbatim; our own stale entry is replaced in place, never
/// duplicated.
pub fn mac_set_handler(
    handlers_json: &str,
    ext: &str,
    bundle_id: &str,
) -> Result<(String, Option<serde_json::Value>), String> {
    let mut arr: Vec<serde_json::Value> = serde_json::from_str(handlers_json)
        .map_err(|e| format!("LSHandlers is not an array: {e}"))?;
    let entry = mac_handler_entry(ext, bundle_id);
    let mut prior = None;
    let mut replaced = false;
    for slot in arr.iter_mut() {
        let tag = slot.get("LSHandlerContentTag").and_then(|t| t.as_str());
        if tag == Some(ext) {
            prior = Some(slot.clone());
            *slot = entry.clone();
            replaced = true;
            break;
        }
    }
    if !replaced {
        arr.push(entry);
    }
    let json = serde_json::to_string(&arr).expect("array serializes");
    Ok((json, prior))
}

/// Remove our entry for `ext` from a serialized LSHandlers array, restoring
/// the captured prior entry when one was recorded. Unregistered extensions
/// are left untouched (no-op, not an error).
pub fn mac_clear_handler(
    handlers_json: &str,
    ext: &str,
    prior: Option<serde_json::Value>,
) -> Result<String, String> {
    let mut arr: Vec<serde_json::Value> = serde_json::from_str(handlers_json)
        .map_err(|e| format!("LSHandlers is not an array: {e}"))?;
    arr.retain(|slot| {
        let tag = slot.get("LSHandlerContentTag").and_then(|t| t.as_str());
        !(tag == Some(ext) && prior.is_none())
    });
    if let Some(prior) = prior {
        // Replace our entry with the captured prior (insert at the first
        // position whose tag matches, else append).
        let pos = arr
            .iter()
            .position(|slot| slot.get("LSHandlerContentTag").and_then(|t| t.as_str()) == Some(ext));
        match pos {
            Some(i) => arr[i] = prior,
            None => arr.push(prior),
        }
    }
    serde_json::to_string(&arr).map_err(|e| format!("serialize LSHandlers: {e}"))
}

// --- Linux (mimeapps.list + desktop file) ------------------------------------

/// Set the `[Default Applications]` entry for `mime` in
/// `<config_dir>/mimeapps.list`, returning the previous value (if any) so
/// the caller can restore it on unregister. Creates the file/section when
/// missing; writes atomically. Pure file I/O — testable on every OS.
pub fn linux_set_default(
    config_dir: &Path,
    mime: &str,
    desktop_id: &str,
) -> Result<Option<String>, String> {
    let path = config_dir.join("mimeapps.list");
    let text = std::fs::read_to_string(&path).unwrap_or_default();
    let (lines, prior) = set_mimeapps_line(&text, mime, desktop_id);
    write_atomic(&path, &join_lines(&lines))?;
    Ok(prior)
}

/// Remove (or restore) the `[Default Applications]` entry for `mime`.
/// `prior = Some(v)` writes `v` back; `prior = None` deletes our line,
/// leaving every other entry untouched. A missing file is a no-op —
/// unregistering must never create files.
pub fn linux_clear_default(
    config_dir: &Path,
    mime: &str,
    prior: Option<&str>,
) -> Result<(), String> {
    let path = config_dir.join("mimeapps.list");
    let Ok(text) = std::fs::read_to_string(&path) else {
        return Ok(());
    };
    let (lines, _) = match prior {
        Some(v) => set_mimeapps_line(&text, mime, v),
        None => remove_mimeapps_line(&text, mime),
    };
    write_atomic(&path, &join_lines(&lines))?;
    Ok(())
}

/// Join the rewritten line list back to file text (trailing newline).
fn join_lines(lines: &[String]) -> String {
    let mut text = lines.join("\n");
    if !lines.is_empty() {
        text.push('\n');
    }
    text
}

/// The current `[Default Applications]` value for `mime`, or `None`.
fn mimeapps_prior_value(text: &str, mime: &str) -> Option<String> {
    let mut in_defaults = false;
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            in_defaults = trimmed == "[Default Applications]";
            continue;
        }
        if in_defaults && !trimmed.starts_with('#') && trimmed.starts_with(mime) {
            if let Some(rest) = trimmed.strip_prefix(mime) {
                if let Some(value) = rest.strip_prefix('=') {
                    return Some(value.trim().to_string());
                }
            }
        }
    }
    None
}

/// Rewrite one `mime=<value>` line inside `[Default Applications]`,
/// preserving every other line (and their order). Returns the new line list.
fn set_mimeapps_line(text: &str, mime: &str, value: &str) -> (Vec<String>, Option<String>) {
    let prior = mimeapps_prior_value(text, mime);
    let new_line = format!("{mime}={value}");
    let mut lines: Vec<String> = Vec::new();
    let mut in_defaults = false;
    let mut replaced = false;
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            if in_defaults && !replaced {
                lines.push(new_line.clone());
                replaced = true;
            }
            in_defaults = trimmed == "[Default Applications]";
            lines.push(line.to_string());
            continue;
        }
        if in_defaults && !trimmed.starts_with('#') && trimmed.starts_with(mime) {
            if let Some(rest) = trimmed.strip_prefix(mime) {
                if rest.starts_with('=') {
                    if !replaced {
                        lines.push(new_line.clone());
                        replaced = true;
                    }
                    continue; // drop the old line (ours or the prior one)
                }
            }
        }
        lines.push(line.to_string());
    }
    if !replaced {
        if !in_defaults {
            if !lines.is_empty() {
                lines.push(String::new());
            }
            lines.push("[Default Applications]".to_string());
        }
        lines.push(new_line);
    }
    (lines, prior)
}

/// Delete the `mime=<value>` line for `mime` from `[Default Applications]`.
fn remove_mimeapps_line(text: &str, mime: &str) -> (Vec<String>, Option<String>) {
    let prior = mimeapps_prior_value(text, mime);
    let mut lines: Vec<String> = Vec::new();
    let mut in_defaults = false;
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            in_defaults = trimmed == "[Default Applications]";
            lines.push(line.to_string());
            continue;
        }
        if in_defaults && !trimmed.starts_with('#') && trimmed.starts_with(mime) {
            if let Some(rest) = trimmed.strip_prefix(mime) {
                if rest.starts_with('=') {
                    continue; // drop our line
                }
            }
        }
        lines.push(line.to_string());
    }
    (lines, prior)
}

/// Write `text` to `path` atomically (`.tmp` + rename): a crash mid-write
/// can never leave a truncated `mimeapps.list`.
fn write_atomic(path: &Path, text: &str) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("create {}: {e}", parent.display()))?;
    }
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, text).map_err(|e| format!("write {}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, path).map_err(|e| format!("rename {}: {e}", path.display()))?;
    Ok(())
}

/// Write the Fotobase desktop entry (the thing `mimeapps.list` points at)
/// into `<applications_dir>/fotobase.desktop`, advertising every MIME type
/// we register for.
pub fn linux_write_desktop_file(
    applications_dir: &Path,
    exe: &Path,
    mimes: &[&str],
) -> Result<(), String> {
    std::fs::create_dir_all(applications_dir)
        .map_err(|e| format!("create {}: {e}", applications_dir.display()))?;
    let mime_list = format!("{};", mimes.join(";"));
    let content = format!(
        "[Desktop Entry]\nType=Application\nName=Fotobase\nExec={} %f\n\
         NoDisplay=true\nMimeType={}\n",
        exe.display(),
        mime_list,
    );
    std::fs::write(applications_dir.join(DESKTOP_ID), content)
        .map_err(|e| format!("write desktop file: {e}"))
}

/// Remove the desktop entry (last unregister). Missing file is fine.
pub fn linux_remove_desktop_file(applications_dir: &Path) -> Result<(), String> {
    match std::fs::remove_file(applications_dir.join(DESKTOP_ID)) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(format!("remove desktop file: {e}")),
    }
}

// --- OS-launched file routing --------------------------------------------------

/// Collect the command-line args that name an existing, cataloged-type
/// image: what the OS passes when the user opens a file with Fotobase
/// (Windows argv, Linux desktop `Exec %f`). Flags and missing/unsupported
/// paths are ignored — a bad argument must never block startup.
pub fn collect_open_paths(args: &[String]) -> Vec<String> {
    args.iter()
        .filter(|a| !a.starts_with('-'))
        .filter(|a| is_supported_image_path(a))
        .filter(|a| Path::new(a).is_file())
        .cloned()
        .collect()
}

/// `true` when `path` has an extension we register for (used to filter
/// the files the OS hands over at launch — flags and other files are
/// ignored, never blocking startup).
pub fn is_supported_image_path(path: &str) -> bool {
    Path::new(path).extension().and_then(|e| e.to_str()).and_then(normalize_ext).is_some()
}

// --- Per-OS dispatch -----------------------------------------------------------

/// Production Linux config dir (`$XDG_CONFIG_HOME` or `~/.config`).
#[cfg(target_os = "linux")]
fn linux_config_dir() -> PathBuf {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .unwrap_or_else(|| home_dir().join(".config"))
}

/// Production Linux data dir (`$XDG_DATA_HOME` or `~/.local/share`).
#[cfg(target_os = "linux")]
fn linux_data_dir() -> PathBuf {
    std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .unwrap_or_else(|| home_dir().join(".local/share"))
}

#[cfg(target_os = "linux")]
fn home_dir() -> PathBuf {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/"))
}

#[cfg(windows)]
mod win {
    use super::*;
    use winreg::enums::{HKEY_CURRENT_USER, KEY_READ, KEY_WRITE};
    use winreg::RegKey;

    /// `Software\Classes` under HKCU: user-level registrations that shadow
    /// the machine-wide ones. KEY_WRITE carries KEY_SET_VALUE +
    /// KEY_CREATE_SUB_KEY, KEY_READ the value queries.
    fn classes() -> RegKey {
        RegKey::predef(HKEY_CURRENT_USER)
            .open_subkey_with_flags(r"Software\Classes", KEY_READ | KEY_WRITE)
            .expect(r"HKCU\Software\Classes always exists")
    }

    /// Register one ext: create `FotoBase.<ext>` (display name + open
    /// command), then point `.<ext>`'s default value at it. The prior
    /// default value is returned so unregistering can restore it.
    pub fn register_ext(ext: &str, exe: &Path) -> Result<Option<String>, String> {
        let classes = classes();
        let prog_id = win_prog_id(ext);
        let key = classes.create_subkey(&prog_id).map_err(|e| format!("create {prog_id}: {e}"))?.0;
        key.set_value("", "Fotobase image").map_err(|e| format!("set {prog_id}: {e}"))?;
        let cmd = key
            .create_subkey(r"shell\open\command")
            .map_err(|e| format!("create command key: {e}"))?
            .0;
        cmd.set_value("", &win_open_command(exe)).map_err(|e| format!("set command: {e}"))?;

        let ext_key_name = format!(".{ext}");
        let prior = classes
            .open_subkey_with_flags(&ext_key_name, KEY_READ)
            .ok()
            .and_then(|k| k.get_value::<String, _>("").ok());
        let ext_key =
            classes.create_subkey(&ext_key_name).map_err(|e| format!("open .{ext}: {e}"))?.0;
        ext_key.set_value("", &prog_id).map_err(|e| format!("associate .{ext}: {e}"))?;
        Ok(prior)
    }

    /// Restore `.<ext>`'s prior default value (deleting ours when there was
    /// none — the merged HKLM default resurfaces) and drop our ProgID tree.
    pub fn unregister_ext(ext: &str, prior: Option<&str>) -> Result<(), String> {
        let classes = classes();
        let ext_key_name = format!(".{ext}");
        if let Ok(ext_key) = classes.open_subkey_with_flags(&ext_key_name, KEY_WRITE) {
            match prior {
                Some(v) => ext_key.set_value("", &v).map_err(|e| format!("restore .{ext}: {e}"))?,
                None => {
                    let _ = ext_key.delete_value("");
                }
            }
        }
        delete_subkey_tree(&classes, &win_prog_id(ext));
        Ok(())
    }

    pub fn is_registered(ext: &str) -> bool {
        RegKey::predef(HKEY_CURRENT_USER)
            .open_subkey_with_flags(r"Software\Classes", KEY_READ)
            .ok()
            .and_then(|classes| {
                classes
                    .open_subkey_with_flags(format!(".{ext}"), KEY_READ)
                    .ok()
                    .and_then(|k| k.get_value::<String, _>("").ok())
            })
            .is_some_and(|v| v == win_prog_id(ext))
    }

    /// Depth-first ProgID tree delete (`shell\open\command` nests three
    /// levels; a leaf-first walk never leaves orphans behind).
    fn delete_subkey_tree(root: &RegKey, name: &str) {
        if let Ok(key) = root.open_subkey_with_flags(name, KEY_READ) {
            for child in key.enum_keys().flatten() {
                delete_subkey_tree(root, &format!("{name}\\{child}"));
            }
        }
        let _ = root.delete_subkey(name);
    }
}

#[cfg(target_os = "macos")]
mod mac {
    use super::*;

    /// Read the current LSHandlers array as JSON via `defaults export`.
    fn read_handlers() -> Result<String, String> {
        let out = std::process::Command::new("defaults")
            .args(["export", "com.apple.LaunchServices/com.apple.launchservices.secure", "-"])
            .output()
            .map_err(|e| format!("run defaults: {e}"))?;
        if !out.status.success() {
            return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
        }
        // `defaults export` emits a plist; convert to JSON with plutil.
        let plist = String::from_utf8_lossy(&out.stdout).into_owned();
        let json = std::process::Command::new("plutil")
            .args(["-convert", "json", "-o", "-", "-"])
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .spawn()
            .map_err(|e| format!("run plutil: {e}"))
            .and_then(|mut child| {
                use std::io::Write;
                child
                    .stdin
                    .as_mut()
                    .expect("piped")
                    .write_all(plist.as_bytes())
                    .map_err(|e| format!("pipe to plutil: {e}"))?;
                let out = child.wait_with_output().map_err(|e| format!("plutil: {e}"))?;
                if out.status.success() {
                    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
                } else {
                    Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
                }
            })?;
        // The domain may not exist yet: an empty plist converts to `{}`.
        let root: serde_json::Value =
            serde_json::from_str(&json).map_err(|e| format!("parse LSHandlers: {e}"))?;
        let arr = root.get("LSHandlers").cloned().unwrap_or_else(|| serde_json::json!([]));
        serde_json::to_string(&arr).map_err(|e| format!("serialize LSHandlers: {e}"))
    }

    /// Write the LSHandlers array back through `defaults` (which routes the
    /// write through cfprefsd, so the preference never fights the cache).
    fn write_handlers(json: &str) -> Result<(), String> {
        let arr: Vec<serde_json::Value> =
            serde_json::from_str(json).map_err(|e| format!("parse LSHandlers: {e}"))?;
        let mut args = vec![
            "write".to_string(),
            "com.apple.LaunchServices/com.apple.launchservices.secure".to_string(),
            "LSHandlers".to_string(),
            "-array".to_string(),
        ];
        for entry in &arr {
            // Each item as a plist fragment; `defaults` accepts XML items.
            let xml = json_to_plist_xml(entry)?;
            args.push(xml);
        }
        let out = std::process::Command::new("defaults")
            .args(&args)
            .output()
            .map_err(|e| format!("run defaults: {e}"))?;
        if !out.status.success() {
            return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
        }
        Ok(())
    }

    /// Serialize one JSON object as a plist `<dict>` XML fragment, the value
    /// form `defaults write ... -array` accepts per item.
    fn json_to_plist_xml(value: &serde_json::Value) -> Result<String, String> {
        let obj = value.as_object().ok_or("LSHandlers entry is not an object")?;
        let mut xml = String::from("<dict>");
        for (k, v) in obj {
            let s = v.as_str().ok_or("LSHandlers entry values must be strings")?;
            xml.push_str(&format!(
                "<key>{}</key><string>{}</string>",
                xml_escape(k),
                xml_escape(s)
            ));
        }
        xml.push_str("</dict>");
        Ok(xml)
    }

    fn xml_escape(s: &str) -> String {
        s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
    }

    /// Make LaunchServices notice the change without a restart: the
    /// documented force-rebuild of the LaunchServices database.
    fn refresh() -> Result<(), String> {
        let lsregister = "/System/Library/Frameworks/CoreServices.framework/Frameworks/\
                          LaunchServices.framework/Support/lsregister";
        let out = std::process::Command::new(lsregister)
            .args(["-kill", "-r", "-domain", "local", "-domain", "system", "-domain", "user"])
            .output()
            .map_err(|e| format!("run lsregister: {e}"))?;
        if !out.status.success() {
            return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
        }
        Ok(())
    }

    pub fn register_ext(ext: &str) -> Result<Option<serde_json::Value>, String> {
        let handlers = read_handlers()?;
        let (new_json, prior) = mac_set_handler(&handlers, ext, BUNDLE_ID)?;
        if prior.as_ref().and_then(|p| p.get("LSHandlerRoleAll")).and_then(|v| v.as_str())
            == Some(BUNDLE_ID)
        {
            // Already ours: nothing to write, and our own entry is never
            // the "prior state" — report no capture.
            return Ok(None);
        }
        write_handlers(&new_json)?;
        Ok(prior)
    }

    pub fn unregister_ext(ext: &str, prior: Option<serde_json::Value>) -> Result<(), String> {
        let handlers = read_handlers()?;
        let new_json = mac_clear_handler(&handlers, ext, prior)?;
        write_handlers(&new_json)?;
        Ok(())
    }

    pub fn is_registered(ext: &str) -> bool {
        let handlers = match read_handlers() {
            Ok(h) => h,
            Err(_) => return false,
        };
        let arr: Vec<serde_json::Value> = serde_json::from_str(&handlers).unwrap_or_default();
        arr.iter().any(|slot| {
            slot.get("LSHandlerContentTag").and_then(|t| t.as_str()) == Some(ext)
                && slot.get("LSHandlerRoleAll").and_then(|v| v.as_str()) == Some(BUNDLE_ID)
        })
    }

    pub fn refresh_database() -> Result<(), String> {
        refresh()
    }
}

#[cfg(target_os = "linux")]
mod linux {
    use super::*;

    pub fn register_ext(ext: &str, exe: &Path) -> Result<Option<String>, String> {
        let _ = exe;
        let mime = ext_to_mime(ext).ok_or_else(|| format!("unsupported type: {ext}"))?;
        linux_set_default(&linux_config_dir(), mime, DESKTOP_ID)
    }

    pub fn unregister_ext(ext: &str, prior: Option<&str>) -> Result<(), String> {
        let mime = ext_to_mime(ext).ok_or_else(|| format!("unsupported type: {ext}"))?;
        linux_clear_default(&linux_config_dir(), mime, prior)
    }

    pub fn is_registered(ext: &str) -> bool {
        let Some(mime) = ext_to_mime(ext) else { return false };
        let text =
            std::fs::read_to_string(linux_config_dir().join("mimeapps.list")).unwrap_or_default();
        mimeapps_prior_value(&text, mime).as_deref() == Some(DESKTOP_ID)
    }

    /// Write the desktop file advertising exactly the currently-registered
    /// MIME types (call after every change so the entry never lies).
    pub fn sync_desktop_file(exts: &[String], exe: &Path) -> Result<(), String> {
        if exts.is_empty() {
            return linux_remove_desktop_file(&linux_data_dir().join("applications"));
        }
        let mimes: Vec<&str> = exts.iter().filter_map(|e| ext_to_mime(e)).collect();
        linux_write_desktop_file(&linux_data_dir().join("applications"), exe, &mimes)
    }
}

/// Register Fotobase as the default viewer for `exts`, capturing the prior
/// handler of each type first. `data_dir` receives `associations.json`.
pub fn register(exts: &[String], exe: &Path, data_dir: &Path) -> Result<(), String> {
    validate(exts)?;
    let mut prior_state = PriorState::load(data_dir);
    for raw in exts {
        let Some(ext) = normalize_ext(raw) else { continue };
        let prior = do_register(&ext, exe)?;
        // First registration of this type captures its prior handler; a
        // re-register over an already-recorded prior keeps the original.
        if prior_state.prior(&ext).is_none() {
            prior_state.set_prior(&ext, prior);
        }
    }
    #[cfg(target_os = "linux")]
    linux::sync_desktop_file(&live_registered(), exe)?;
    #[cfg(target_os = "macos")]
    mac::refresh_database()?;
    prior_state.save(data_dir).map_err(|e| format!("save prior state: {e}"))
}

/// Unregister `exts`, restoring each type's captured prior handler. Types
/// with no captured prior lose our entry outright (no orphans).
pub fn unregister(exts: &[String], exe: &Path, data_dir: &Path) -> Result<(), String> {
    let _ = exe; // only the Linux desktop-file sync needs it
    validate(exts)?;
    let mut prior_state = PriorState::load(data_dir);
    for raw in exts {
        let Some(ext) = normalize_ext(raw) else { continue };
        let prior = prior_state.prior(&ext).cloned();
        do_unregister(&ext, prior)?;
        prior_state.set_prior(&ext, None);
    }
    #[cfg(target_os = "linux")]
    linux::sync_desktop_file(&live_registered(), exe)?;
    #[cfg(target_os = "macos")]
    mac::refresh_database()?;
    prior_state.save(data_dir).map_err(|e| format!("save prior state: {e}"))
}

/// Registration status of every supported type (for the Options dialog).
pub fn status() -> Vec<AssociationStatus> {
    supported_extensions()
        .iter()
        .map(|&ext| AssociationStatus { ext: ext.to_string(), registered: is_registered(ext) })
        .collect()
}

#[cfg(windows)]
fn do_register(ext: &str, exe: &Path) -> Result<Option<serde_json::Value>, String> {
    win::register_ext(ext, exe).map(|p| p.map(serde_json::Value::String))
}
#[cfg(windows)]
fn do_unregister(ext: &str, prior: Option<serde_json::Value>) -> Result<(), String> {
    win::unregister_ext(ext, prior.as_ref().and_then(|v| v.as_str()))
}
#[cfg(windows)]
fn is_registered(ext: &str) -> bool {
    win::is_registered(ext)
}

#[cfg(target_os = "macos")]
fn do_register(ext: &str, _exe: &Path) -> Result<Option<serde_json::Value>, String> {
    mac::register_ext(ext)
}
#[cfg(target_os = "macos")]
fn do_unregister(ext: &str, prior: Option<serde_json::Value>) -> Result<(), String> {
    mac::unregister_ext(ext, prior)
}
#[cfg(target_os = "macos")]
fn is_registered(ext: &str) -> bool {
    mac::is_registered(ext)
}

#[cfg(target_os = "linux")]
fn do_register(ext: &str, exe: &Path) -> Result<Option<serde_json::Value>, String> {
    linux::register_ext(ext, exe).map(|p| p.map(serde_json::Value::String))
}
#[cfg(target_os = "linux")]
fn do_unregister(ext: &str, prior: Option<serde_json::Value>) -> Result<(), String> {
    linux::unregister_ext(ext, prior.as_ref().and_then(|v| v.as_str()))
}
#[cfg(target_os = "linux")]
fn is_registered(ext: &str) -> bool {
    linux::is_registered(ext)
}

/// The types currently registered at the OS level (desktop-file sync uses
/// this so the Linux desktop entry always matches reality).
#[cfg(target_os = "linux")]
fn live_registered() -> Vec<String> {
    supported_extensions()
        .iter()
        .filter(|&&ext| is_registered(ext))
        .map(|e| e.to_string())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_is_idempotent_over_the_supported_list() {
        for ext in supported_extensions() {
            assert_eq!(normalize_ext(ext).as_deref(), Some(*ext));
        }
    }
}
