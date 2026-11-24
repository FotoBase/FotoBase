//! Native share sheet for files (TASK-09-005, PLAN §6 Phase 5 "share").
//!
//! One call, [`ShareSheet::share_files`], opens the OS share sheet with every
//! file attached in a single invocation: `UIActivityViewController` on iOS,
//! `ACTION_SEND` / `ACTION_SEND_MULTIPLE` through a chooser on Android. There
//! is no custom share UI. The plugin only reads the files it is given.
//!
//! Desktop has no share sheet here (the Email dialog covers desktop), so the
//! call reports [`Error::Unsupported`] instead of doing nothing silently.

use std::path::PathBuf;

use tauri::{
    plugin::{Builder, TauriPlugin},
    Manager, Runtime,
};

#[cfg(mobile)]
use tauri::plugin::PluginHandle;

#[cfg(target_os = "ios")]
tauri::ios_plugin_binding!(init_plugin_share_sheet);

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("the share sheet is only available on iOS and Android")]
    Unsupported,
    #[error("no files to share")]
    NoFiles,
    #[error("path is not valid UTF-8: {0}")]
    NonUtf8Path(PathBuf),
    #[cfg(mobile)]
    #[error(transparent)]
    PluginInvoke(#[from] tauri::plugin::mobile::PluginInvokeError),
}

pub type Result<T> = std::result::Result<T, Error>;

#[cfg(mobile)]
#[derive(serde::Serialize)]
struct ShareFilesArgs<'a> {
    files: Vec<&'a str>,
}

/// Handle to the platform share sheet.
pub struct ShareSheet<R: Runtime> {
    #[cfg(mobile)]
    handle: PluginHandle<R>,
    #[cfg(desktop)]
    _runtime: std::marker::PhantomData<fn() -> R>,
}

impl<R: Runtime> ShareSheet<R> {
    /// Open the share sheet with `files` attached (all in one sheet).
    ///
    /// Blocks until the platform has presented the sheet, not until the user
    /// picks a target, so call it off the async executor threads.
    pub fn share_files(&self, files: &[PathBuf]) -> Result<()> {
        if files.is_empty() {
            return Err(Error::NoFiles);
        }
        #[cfg(mobile)]
        {
            // A lossy conversion would name a different file, so refuse.
            let files = files
                .iter()
                .map(|f| f.to_str().ok_or_else(|| Error::NonUtf8Path(f.clone())))
                .collect::<Result<Vec<_>>>()?;
            self.handle.run_mobile_plugin::<()>("shareFiles", ShareFilesArgs { files })?;
            Ok(())
        }
        #[cfg(desktop)]
        Err(Error::Unsupported)
    }
}

/// Extension to reach the share sheet from an app handle or manager.
pub trait ShareSheetExt<R: Runtime> {
    fn share_sheet(&self) -> &ShareSheet<R>;
}

impl<R: Runtime, T: Manager<R>> ShareSheetExt<R> for T {
    fn share_sheet(&self) -> &ShareSheet<R> {
        self.state::<ShareSheet<R>>().inner()
    }
}

/// Initializes the plugin.
pub fn init<R: Runtime>() -> TauriPlugin<R> {
    Builder::new("share-sheet")
        .setup(|app, _api| {
            #[cfg(target_os = "android")]
            let handle =
                _api.register_android_plugin("io.github.fotobase.sharesheet", "SharePlugin")?;
            #[cfg(target_os = "ios")]
            let handle = _api.register_ios_plugin(init_plugin_share_sheet)?;
            #[cfg(mobile)]
            app.manage(ShareSheet::<R> { handle });
            #[cfg(desktop)]
            app.manage(ShareSheet::<R> { _runtime: std::marker::PhantomData });
            Ok(())
        })
        .build()
}
