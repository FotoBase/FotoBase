//! OS file-manager context menu (TASK-08-020, PLAN §2M "Context menu", P4):
//! "View with Fotobase" and "Upload with Fotobase" in Explorer / Finder /
//! Nautilus.
//!
//! Every entry runs the same thing — `<fotobase exe> --view|--upload
//! <files…>` — on every OS. A second process is turned away by the
//! single-instance plugin, which forwards its argv to the running window
//! (focus + route); with no window running, the same argv starts the app
//! (the launch contract is just argv, see [`parse_launch_args`]).
//!
//! Backends are thin shells over pure builders:
//! - Windows: `HKCU\Software\Classes\SystemFileAssociations\.<ext>\shell\…`
//!   verbs. The NSIS uninstaller removes them too (`windows/hooks.nsh`).
//! - macOS: Quick Action workflows in `~/Library/Services` (Finder →
//!   Quick Actions / Services).
//! - Linux: executable scripts in `~/.local/share/nautilus/scripts`.
//!
//! macOS and Linux entries live in the user's home, which no uninstaller
//! can reach, so each one deletes itself the first time it runs and finds
//! the app gone.

use std::path::{Path, PathBuf};

use crate::associations::collect_open_paths;
#[cfg(windows)]
use crate::associations::IMAGE_EXTS;

/// Launch flag for "View with Fotobase".
pub const VIEW_FLAG: &str = "--view";
/// Launch flag for "Upload with Fotobase".
pub const UPLOAD_FLAG: &str = "--upload";

pub const VIEW_LABEL: &str = "View with Fotobase";
pub const UPLOAD_LABEL: &str = "Upload with Fotobase";

/// What the user asked the OS to do with the files.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LaunchAction {
    View,
    Upload,
}

impl LaunchAction {
    fn flag(self) -> &'static str {
        match self {
            LaunchAction::View => VIEW_FLAG,
            LaunchAction::Upload => UPLOAD_FLAG,
        }
    }

    fn label(self) -> &'static str {
        match self {
            LaunchAction::View => VIEW_LABEL,
            LaunchAction::Upload => UPLOAD_LABEL,
        }
    }
}

const ACTIONS: [LaunchAction; 2] = [LaunchAction::View, LaunchAction::Upload];

/// A parsed launch: the action and the existing supported image files.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaunchRequest {
    pub action: LaunchAction,
    pub paths: Vec<String>,
}

/// Parse the argv after the executable name. No flag means a plain "open
/// with" (file associations, TASK-01-025) → view. Missing/unsupported paths
/// and unknown flags are dropped — a bad argument must never block startup.
pub fn parse_launch_args(args: &[String]) -> LaunchRequest {
    let action = if args.iter().any(|a| a == UPLOAD_FLAG) {
        LaunchAction::Upload
    } else {
        LaunchAction::View
    };
    LaunchRequest { action, paths: collect_open_paths(args) }
}

// --- Linux (Nautilus scripts) ----------------------------------------------------

/// POSIX single-quote `s` (a `'` becomes `'\''`).
fn sh_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

/// The Nautilus script for one action. Nautilus passes the selection as
/// arguments; the script drops itself when the app was uninstalled.
fn nautilus_script(exe: &Path, action: LaunchAction) -> String {
    format!(
        "#!/bin/sh\n\
         # Fotobase context menu entry (TASK-08-020).\n\
         EXE={}\n\
         [ -x \"$EXE\" ] || {{ rm -f -- \"$0\"; exit 0; }}\n\
         exec \"$EXE\" {} \"$@\"\n",
        sh_quote(&exe.to_string_lossy()),
        action.flag(),
    )
}

pub fn linux_install(scripts_dir: &Path, exe: &Path) -> Result<(), String> {
    std::fs::create_dir_all(scripts_dir)
        .map_err(|e| format!("create {}: {e}", scripts_dir.display()))?;
    for action in ACTIONS {
        let path = scripts_dir.join(action.label());
        std::fs::write(&path, nautilus_script(exe, action))
            .map_err(|e| format!("write {}: {e}", path.display()))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))
                .map_err(|e| format!("chmod {}: {e}", path.display()))?;
        }
    }
    Ok(())
}

/// Remove only our scripts; a missing file is fine.
pub fn linux_remove(scripts_dir: &Path) -> Result<(), String> {
    for action in ACTIONS {
        remove_file_if_present(&scripts_dir.join(action.label()))?;
    }
    Ok(())
}

pub fn linux_installed(scripts_dir: &Path) -> bool {
    ACTIONS.iter().all(|a| scripts_dir.join(a.label()).is_file())
}

fn remove_file_if_present(path: &Path) -> Result<(), String> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(format!("remove {}: {e}", path.display())),
    }
}

// --- macOS (Quick Action workflows) ----------------------------------------------

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

fn workflow_dir(services_dir: &Path, action: LaunchAction) -> PathBuf {
    services_dir.join(format!("{}.workflow", action.label()))
}

/// The workflow's `Info.plist`: a Finder-only service for image files.
fn mac_info_plist(action: LaunchAction) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
	<key>NSServices</key>
	<array>
		<dict>
			<key>NSBackgroundColorName</key>
			<string>background</string>
			<key>NSIconName</key>
			<string>NSActionTemplate</string>
			<key>NSMenuItem</key>
			<dict>
				<key>default</key>
				<string>{label}</string>
			</dict>
			<key>NSMessage</key>
			<string>runWorkflowAsService</string>
			<key>NSRequiredContext</key>
			<dict>
				<key>NSApplicationIdentifier</key>
				<string>com.apple.finder</string>
			</dict>
			<key>NSSendFileTypes</key>
			<array>
				<string>public.image</string>
			</array>
		</dict>
	</array>
</dict>
</plist>
"#,
        label = xml_escape(action.label()),
    )
}

/// The Automator document: one "Run Shell Script" action fed the selected
/// files as arguments. The script removes the workflow when the app is gone.
fn mac_document(exe: &Path, workflow: &Path, action: LaunchAction) -> String {
    let script = format!(
        "EXE={}\n[ -x \"$EXE\" ] || {{ rm -rf {}; exit 0; }}\nexec \"$EXE\" {} \"$@\"\n",
        sh_quote(&exe.to_string_lossy()),
        sh_quote(&workflow.to_string_lossy()),
        action.flag(),
    );
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
	<key>AMApplicationBuild</key>
	<string>523</string>
	<key>AMApplicationVersion</key>
	<string>2.10</string>
	<key>AMDocumentVersion</key>
	<string>2</string>
	<key>actions</key>
	<array>
		<dict>
			<key>action</key>
			<dict>
				<key>AMAccepts</key>
				<dict>
					<key>Container</key>
					<string>List</string>
					<key>Optional</key>
					<true/>
					<key>Types</key>
					<array>
						<string>com.apple.cocoa.string</string>
					</array>
				</dict>
				<key>AMActionVersion</key>
				<string>2.0.3</string>
				<key>AMApplication</key>
				<array>
					<string>Automator</string>
				</array>
				<key>AMParameterProperties</key>
				<dict>
					<key>COMMAND_STRING</key>
					<dict/>
					<key>CheckedForUserDefaultShell</key>
					<dict/>
					<key>inputMethod</key>
					<dict/>
					<key>shell</key>
					<dict/>
					<key>source</key>
					<dict/>
				</dict>
				<key>AMProvides</key>
				<dict>
					<key>Container</key>
					<string>List</string>
					<key>Types</key>
					<array>
						<string>com.apple.cocoa.string</string>
					</array>
				</dict>
				<key>ActionBundlePath</key>
				<string>/System/Library/Automator/Run Shell Script.action</string>
				<key>ActionName</key>
				<string>Run Shell Script</string>
				<key>ActionParameters</key>
				<dict>
					<key>COMMAND_STRING</key>
					<string>{script}</string>
					<key>CheckedForUserDefaultShell</key>
					<true/>
					<key>inputMethod</key>
					<integer>1</integer>
					<key>shell</key>
					<string>/bin/bash</string>
					<key>source</key>
					<string></string>
				</dict>
				<key>BundleIdentifier</key>
				<string>com.apple.RunShellScript</string>
				<key>CFBundleVersion</key>
				<string>2.0.3</string>
				<key>CanShowSelectedItemsWhenRun</key>
				<false/>
				<key>CanShowWhenRun</key>
				<true/>
				<key>Category</key>
				<array>
					<string>AMCategoryUtilities</string>
				</array>
				<key>Class Name</key>
				<string>RunShellScriptAction</string>
				<key>UnlocalizedApplications</key>
				<array>
					<string>Automator</string>
				</array>
				<key>arguments</key>
				<dict/>
				<key>isViewVisible</key>
				<integer>1</integer>
			</dict>
			<key>isViewVisible</key>
			<integer>1</integer>
		</dict>
	</array>
	<key>connectors</key>
	<dict/>
	<key>workflowMetaData</key>
	<dict>
		<key>serviceApplicationBundleID</key>
		<string>com.apple.finder</string>
		<key>serviceInputTypeIdentifier</key>
		<string>com.apple.Automator.fileSystemObject</string>
		<key>serviceOutputTypeIdentifier</key>
		<string>com.apple.Automator.nothing</string>
		<key>serviceProcessesInput</key>
		<integer>0</integer>
		<key>workflowTypeIdentifier</key>
		<string>com.apple.Automator.servicesMenu</string>
	</dict>
</dict>
</plist>
"#,
        script = xml_escape(&script),
    )
}

pub fn mac_install(services_dir: &Path, exe: &Path) -> Result<(), String> {
    for action in ACTIONS {
        let workflow = workflow_dir(services_dir, action);
        let contents = workflow.join("Contents");
        std::fs::create_dir_all(&contents)
            .map_err(|e| format!("create {}: {e}", contents.display()))?;
        for (name, text) in [
            ("Info.plist", mac_info_plist(action)),
            ("document.wflow", mac_document(exe, &workflow, action)),
        ] {
            std::fs::write(contents.join(name), text).map_err(|e| format!("write {name}: {e}"))?;
        }
    }
    Ok(())
}

pub fn mac_remove(services_dir: &Path) -> Result<(), String> {
    for action in ACTIONS {
        let workflow = workflow_dir(services_dir, action);
        match std::fs::remove_dir_all(&workflow) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(format!("remove {}: {e}", workflow.display())),
        }
    }
    Ok(())
}

pub fn mac_installed(services_dir: &Path) -> bool {
    ACTIONS.iter().all(|a| workflow_dir(services_dir, *a).join("Contents/document.wflow").is_file())
}

// --- Windows (registry verbs) ----------------------------------------------------

/// The command line behind a verb: quoted exe, flag, the clicked file.
pub fn win_verb_command(exe: &Path, action: LaunchAction) -> String {
    format!("\"{}\" {} \"%1\"", exe.display(), action.flag())
}

/// Registry key name of a verb under an extension's `shell` key.
pub fn win_verb_name(action: LaunchAction) -> &'static str {
    match action {
        LaunchAction::View => "FotobaseView",
        LaunchAction::Upload => "FotobaseUpload",
    }
}

/// Subkey path (below the Classes root) of one extension's `shell` key.
pub fn win_shell_key(ext: &str) -> String {
    format!(r"SystemFileAssociations\.{ext}\shell")
}

/// Production Classes root: user-level, no elevation needed.
pub const WIN_CLASSES_ROOT: &str = r"Software\Classes";

#[cfg(windows)]
mod win {
    use super::*;
    use winreg::enums::{HKEY_CURRENT_USER, KEY_READ, KEY_WRITE};
    use winreg::RegKey;

    fn root(path: &str) -> Result<RegKey, String> {
        RegKey::predef(HKEY_CURRENT_USER)
            .create_subkey_with_flags(path, KEY_READ | KEY_WRITE)
            .map(|(k, _)| k)
            .map_err(|e| format!("open {path}: {e}"))
    }

    pub fn install(classes: &str, exe: &Path) -> Result<(), String> {
        let root = root(classes)?;
        for ext in IMAGE_EXTS {
            let shell = win_shell_key(ext);
            for action in ACTIONS {
                let verb = format!(r"{shell}\{}", win_verb_name(action));
                let key = root.create_subkey(&verb).map_err(|e| format!("create {verb}: {e}"))?.0;
                key.set_value("", &action.label()).map_err(|e| format!("set {verb}: {e}"))?;
                key.set_value("Icon", &format!("\"{}\",0", exe.display()))
                    .map_err(|e| format!("set icon {verb}: {e}"))?;
                let cmd = key
                    .create_subkey("command")
                    .map_err(|e| format!("create {verb}\\command: {e}"))?
                    .0;
                cmd.set_value("", &win_verb_command(exe, action))
                    .map_err(|e| format!("set {verb} command: {e}"))?;
            }
        }
        Ok(())
    }

    fn delete_tree(root: &RegKey, name: &str) {
        if let Ok(key) = root.open_subkey_with_flags(name, KEY_READ) {
            for child in key.enum_keys().flatten() {
                delete_tree(root, &format!("{name}\\{child}"));
            }
        }
        let _ = root.delete_subkey(name);
    }

    fn is_empty(root: &RegKey, name: &str) -> bool {
        root.open_subkey_with_flags(name, KEY_READ)
            .map(|k| k.enum_keys().next().is_none() && k.enum_values().next().is_none())
            .unwrap_or(false)
    }

    /// Drop our verbs, then any `shell` / `.ext` key that is now empty, so
    /// an uninstall leaves no orphans (keys other apps use stay).
    pub fn remove(classes: &str) -> Result<(), String> {
        let root = root(classes)?;
        for ext in IMAGE_EXTS {
            let shell = win_shell_key(ext);
            for action in ACTIONS {
                delete_tree(&root, &format!(r"{shell}\{}", win_verb_name(action)));
            }
            let ext_key = format!(r"SystemFileAssociations\.{ext}");
            for key in [shell, ext_key] {
                if is_empty(&root, &key) {
                    let _ = root.delete_subkey(&key);
                }
            }
        }
        Ok(())
    }

    pub fn installed(classes: &str) -> bool {
        let Ok(root) = RegKey::predef(HKEY_CURRENT_USER).open_subkey_with_flags(classes, KEY_READ)
        else {
            return false;
        };
        IMAGE_EXTS.iter().all(|ext| {
            ACTIONS.iter().all(|a| {
                root.open_subkey_with_flags(
                    format!(r"{}\{}\command", win_shell_key(ext), win_verb_name(*a)),
                    KEY_READ,
                )
                .is_ok()
            })
        })
    }
}

#[cfg(windows)]
pub use win::{install as win_install, installed as win_installed, remove as win_remove};

// --- Per-OS dispatch -------------------------------------------------------------

#[cfg(target_os = "macos")]
fn services_dir() -> PathBuf {
    PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join("Library/Services")
}

#[cfg(target_os = "linux")]
fn nautilus_dir() -> PathBuf {
    let data = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .unwrap_or_else(|| {
            PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".local/share")
        });
    data.join("nautilus/scripts")
}

/// Install the entries for `exe` (idempotent: re-running repoints them at a
/// moved app).
pub fn register(exe: &Path) -> Result<(), String> {
    #[cfg(windows)]
    return win_install(WIN_CLASSES_ROOT, exe);
    #[cfg(target_os = "macos")]
    return mac_install(&services_dir(), exe);
    #[cfg(target_os = "linux")]
    return linux_install(&nautilus_dir(), exe);
    #[allow(unreachable_code)]
    {
        let _ = exe;
        Ok(())
    }
}

/// Remove every entry this module installed.
pub fn unregister() -> Result<(), String> {
    #[cfg(windows)]
    return win_remove(WIN_CLASSES_ROOT);
    #[cfg(target_os = "macos")]
    return mac_remove(&services_dir());
    #[cfg(target_os = "linux")]
    return linux_remove(&nautilus_dir());
    #[allow(unreachable_code)]
    Ok(())
}

/// Make the OS entries match the setting (toggle from the Options dialog).
pub fn apply(enabled: bool) -> Result<(), String> {
    if enabled {
        let exe = std::env::current_exe().map_err(|e| format!("current exe: {e}"))?;
        register(&exe)
    } else {
        unregister()
    }
}

pub fn is_registered() -> bool {
    #[cfg(windows)]
    return win_installed(WIN_CLASSES_ROOT);
    #[cfg(target_os = "macos")]
    return mac_installed(&services_dir());
    #[cfg(target_os = "linux")]
    return linux_installed(&nautilus_dir());
    #[allow(unreachable_code)]
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_verb_command_quotes_the_exe_and_passes_the_file() {
        let cmd = win_verb_command(
            Path::new(r"C:\Program Files\Fotobase\fotobase.exe"),
            LaunchAction::Upload,
        );
        assert_eq!(cmd, r#""C:\Program Files\Fotobase\fotobase.exe" --upload "%1""#);
    }

    #[test]
    fn windows_shell_key_is_per_extension_under_system_file_associations() {
        assert_eq!(win_shell_key("jpg"), r"SystemFileAssociations\.jpg\shell");
    }
}
