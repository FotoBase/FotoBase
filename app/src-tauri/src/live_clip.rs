//! Live/Motion photo playback source (TASK-08-017, PLAN §2 Video strategy).
//!
//! The viewer asks for the clip behind a photo it is showing. An Apple Live
//! Photo's clip is the paired MOV already on disk; an Android Motion Photo's
//! clip is embedded in the JPEG, so it is copied byte-for-byte into the
//! thumbnail cache's `live/` temp folder first (the original is never
//! written, PLAN §3 rule 1). The viewer then plays the returned path through
//! the same asset-protocol `<video>` path as ordinary videos — no media bytes
//! cross IPC (§3 rule 3).

use std::path::{Path, PathBuf};

use fotobase_catalog::{Catalog, LiveMedia};
use fotobase_video::live::{extract_motion_clip, MotionClip};

/// Cache sub-folder holding extracted Motion Photo clips.
pub const LIVE_CACHE_DIR: &str = "live";

/// Where to find a photo's playable clip.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct LiveClip {
    /// Absolute path of a playable video file (feeds the asset-protocol URL).
    pub path: String,
    /// `motion` or `live`.
    pub kind: &'static str,
}

/// Resolve (extracting if needed) the clip for `file_id`. `Ok(None)` when the
/// photo is not a Live/Motion photo, or its clip is gone — the viewer then
/// simply shows the still.
pub fn resolve_live_clip(
    cat: &Catalog,
    cache_dir: &Path,
    file_id: i64,
) -> Result<Option<LiveClip>, String> {
    let Some(live) = cat.live_media(file_id).map_err(|e| e.to_string())? else {
        return Ok(None);
    };
    match live {
        LiveMedia::Live { video_path } => Ok(Path::new(&video_path)
            .is_file()
            .then_some(LiveClip { path: video_path, kind: "live" })),
        LiveMedia::Motion { offset, len } => {
            let Some(still) = cat.file_path(file_id).map_err(|e| e.to_string())? else {
                return Ok(None);
            };
            let dir = cache_dir.join(LIVE_CACHE_DIR);
            std::fs::create_dir_all(&dir).map_err(|e| format!("live cache: {e}"))?;
            // The byte range is part of the name: an edited still with a
            // different clip never serves a stale extraction.
            let out: PathBuf = dir.join(format!("{file_id}-{offset}-{len}.mp4"));
            if !out.is_file() {
                extract_motion_clip(Path::new(&still), MotionClip { offset, len }, &out)
                    .map_err(|e| format!("extract motion clip: {e}"))?;
            }
            Ok(Some(LiveClip { path: out.to_string_lossy().into_owned(), kind: "motion" }))
        }
    }
}
