//! Camera roll change observation (TASK-09-006, PLAN §6 Phase 5 "camera roll
//! as a watched-folder equivalent").
//!
//! The platform photo library is exposed to the scanner as folders of media
//! files: on Android the MediaStore directories under DCIM and Pictures (the
//! files are read in place), on iOS an app-owned mirror folder the plugin
//! keeps in step with the library (a `PHAsset` has no file path). The
//! plugin only *observes*: it never writes to the photo library.
//!
//! Observation is event-driven — `ContentObserver` on Android,
//! `PHPhotoLibraryChangeObserver` on iOS. Each notification arrives as a
//! [`Change`] on a channel; there is no timer and no polling loop. The app
//! turns a change into scan requests for the existing watcher.
//!
//! Desktop has no camera roll: every call reports [`Error::Unsupported`].

use std::path::PathBuf;

use serde::Deserialize;
use tauri::{
    plugin::{Builder, TauriPlugin},
    Manager, Runtime,
};

#[cfg(mobile)]
use tauri::{ipc::Channel, plugin::PluginHandle};

#[cfg(target_os = "ios")]
tauri::ios_plugin_binding!(init_plugin_camera_roll);

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("the camera roll is only available on iOS and Android")]
    Unsupported,
    #[error("unreadable camera roll message: {0}")]
    BadMessage(#[from] serde_json::Error),
    #[cfg(mobile)]
    #[error(transparent)]
    PluginInvoke(#[from] tauri::plugin::mobile::PluginInvokeError),
}

pub type Result<T> = std::result::Result<T, Error>;

/// Whether the app may read the photo library.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Access {
    Granted,
    /// iOS "selected photos" access: only what the user picked is visible.
    Limited,
    Denied,
}

impl Access {
    /// Can photos be read at all?
    pub fn is_readable(self) -> bool {
        !matches!(self, Access::Denied)
    }
}

#[cfg(mobile)]
#[derive(Deserialize)]
struct AccessReply {
    state: Access,
}

/// Folders that hold camera roll media and may have changed. The first
/// reply of [`CameraRoll::start_observing`] lists every folder; each later
/// notification lists the folders to look at again.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct Change {
    pub folders: Vec<PathBuf>,
}

impl Change {
    /// Parse the JSON body the native observer sends.
    pub fn parse(body: &str) -> Result<Self> {
        Ok(serde_json::from_str(body)?)
    }
}

#[cfg(mobile)]
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct ObserveArgs {
    on_change: Channel<serde_json::Value>,
}

/// Handle to the platform camera roll.
pub struct CameraRoll<R: Runtime> {
    #[cfg(mobile)]
    handle: PluginHandle<R>,
    #[cfg(desktop)]
    _runtime: std::marker::PhantomData<fn() -> R>,
}

impl<R: Runtime> CameraRoll<R> {
    /// Ask for read access to the photo library; shows the system prompt the
    /// first time. Blocks until the user answered, so call it off the async
    /// executor threads.
    pub fn request_access(&self) -> Result<Access> {
        #[cfg(mobile)]
        {
            let reply: AccessReply = self.handle.run_mobile_plugin("requestAccess", ())?;
            Ok(reply.state)
        }
        #[cfg(desktop)]
        Err(Error::Unsupported)
    }

    /// Start observing the library. Returns every folder that holds camera
    /// roll media now; `on_change` runs (on a platform thread) for each later
    /// change. Needs access, see [`request_access`](Self::request_access).
    pub fn start_observing(
        &self,
        on_change: impl Fn(Change) + Send + Sync + 'static,
    ) -> Result<Vec<PathBuf>> {
        #[cfg(mobile)]
        {
            use tauri::ipc::InvokeResponseBody;
            let channel = Channel::new(move |body| {
                if let InvokeResponseBody::Json(json) = body {
                    match Change::parse(&json) {
                        Ok(change) => on_change(change),
                        Err(e) => tracing::warn!(error = %e, "dropping camera roll message"),
                    }
                }
                Ok(())
            });
            let reply: Change = self
                .handle
                .run_mobile_plugin("startObserving", ObserveArgs { on_change: channel })?;
            Ok(reply.folders)
        }
        #[cfg(desktop)]
        {
            let _ = on_change;
            Err(Error::Unsupported)
        }
    }

    /// Stop observing. A no-op when nothing is observed.
    pub fn stop_observing(&self) -> Result<()> {
        #[cfg(mobile)]
        {
            self.handle.run_mobile_plugin::<()>("stopObserving", ())?;
            Ok(())
        }
        #[cfg(desktop)]
        Err(Error::Unsupported)
    }
}

/// Extension to reach the camera roll from an app handle or manager.
pub trait CameraRollExt<R: Runtime> {
    fn camera_roll(&self) -> &CameraRoll<R>;
}

impl<R: Runtime, T: Manager<R>> CameraRollExt<R> for T {
    fn camera_roll(&self) -> &CameraRoll<R> {
        self.state::<CameraRoll<R>>().inner()
    }
}

/// Initializes the plugin.
pub fn init<R: Runtime>() -> TauriPlugin<R> {
    Builder::new("camera-roll")
        .setup(|app, _api| {
            #[cfg(target_os = "android")]
            let handle =
                _api.register_android_plugin("io.github.fotobase.cameraroll", "CameraRollPlugin")?;
            #[cfg(target_os = "ios")]
            let handle = _api.register_ios_plugin(init_plugin_camera_roll)?;
            #[cfg(mobile)]
            app.manage(CameraRoll::<R> { handle });
            #[cfg(desktop)]
            app.manage(CameraRoll::<R> { _runtime: std::marker::PhantomData });
            Ok(())
        })
        .build()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn change_message_lists_folders() {
        let c = Change::parse(r#"{"folders":["/sdcard/DCIM/Camera","/sdcard/Pictures"]}"#).unwrap();
        assert_eq!(
            c.folders,
            vec![PathBuf::from("/sdcard/DCIM/Camera"), PathBuf::from("/sdcard/Pictures")]
        );
    }

    #[test]
    fn bad_message_is_an_error() {
        assert!(Change::parse("not json").is_err());
    }

    #[test]
    fn limited_access_is_readable_denied_is_not() {
        assert!(Access::Granted.is_readable());
        assert!(Access::Limited.is_readable());
        assert!(!Access::Denied.is_readable());
    }
}
