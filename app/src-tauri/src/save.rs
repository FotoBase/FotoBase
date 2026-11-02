//! Save & Revert (TASK-04-016, PLAN §2F, §3 rule 1).
//!
//! Save is the *only* path that ever modifies an original. Order of
//! operations is chosen so a failure at any point leaves the user's pixels
//! recoverable:
//!
//! 1. the untouched original moves to a sibling `.originals/` folder (it is
//!    safe from that instant on — nothing else writes before this);
//! 2. the recipe renders at full resolution through the shared export
//!    engine (04-018) and lands at the original path via the engine's
//!    atomic `.tmp` + rename write — a kill mid-write can never truncate
//!    anything (the original is already archived; the destination either
//!    holds the old or the new complete file);
//! 3. the archived original is cataloged (hidden) and stacked under the
//!    saved photo per EPIC-03 conventions: `fb:StackId` / `fb:StackRole`
//!    sidecars, catalog mirror.
//!
//! Re-Save renders from the archived original, never from the already-edited
//! pixels — a recipe must never apply twice. Revert copies the archived
//! original back atomically, clears the recipe and dissolves the save's
//! stack record.
//!
//! Metadata stays in sidecars (ADR-0003): the sidecar keeps living next to
//! the file it describes, so after Save it describes the saved pixels. The
//! opt-in "embed XMP in the original on Save" setting (00-002) has no
//! backend writer yet; until it exists the sidecars-only default applies.

use std::path::{Path, PathBuf};

use fotobase_catalog::{Catalog, ORIGINALS_DIR};
use fotobase_render::{export_image, ExportFormat, ExportOptions};

/// JPEG quality of a Save: Maximum-ish per the §2J preset table — the saved
/// file is the user's only copy of the edited pixels.
pub const SAVE_JPEG_QUALITY: u8 = 95;

/// Container the saved pixels are written in: the original's own, so Save
/// never changes the file's identity. `None` for formats Save refuses
/// (RAW and everything else — replacing a RAW with rendered pixels would
/// destroy it; RAW edits stay non-destructive until a future spec).
fn save_format(path: &Path) -> Option<ExportFormat> {
    match path.extension()?.to_string_lossy().to_ascii_lowercase().as_str() {
        "jpg" | "jpeg" => Some(ExportFormat::Jpeg),
        "png" => Some(ExportFormat::Png),
        _ => None,
    }
}

pub fn originals_dir(photo: &Path) -> PathBuf {
    photo.parent().unwrap_or(Path::new(".")).join(ORIGINALS_DIR)
}

/// Where the untouched original of `photo` lives once it has been saved:
/// always the same filename inside the sibling `.originals/` folder.
pub fn archived_path(photo: &Path) -> PathBuf {
    originals_dir(photo).join(photo.file_name().unwrap_or_default())
}

/// Is `path` inside a `.originals/` folder (i.e. an archived original)?
pub fn is_archived(path: &Path) -> bool {
    path.parent().is_some_and(|p| p.file_name().is_some_and(|n| n == ORIGINALS_DIR))
}

#[derive(Debug, serde::Serialize)]
pub struct SavedPhoto {
    pub file_id: i64,
    pub archived_id: i64,
    pub stack_id: String,
}

/// Failure of a Save step after the original was already archived — the
/// caller (and the user) can always Revert.
#[derive(Debug, thiserror::Error)]
pub enum SaveError {
    #[error("no edits to save: the photo has no recipe")]
    NoRecipe,
    #[error("Save supports JPEG and PNG originals only: {0}")]
    UnsupportedFormat(String),
    #[error(transparent)]
    Catalog(#[from] fotobase_catalog::CatalogError),
    #[error(transparent)]
    Sync(#[from] fotobase_catalog::SyncError),
    #[error(transparent)]
    Sql(#[from] rusqlite::Error),
    #[error(transparent)]
    Export(#[from] fotobase_render::ExportError),
    #[error("{0}")]
    Io(#[from] std::io::Error),
}

fn file_path(cat: &Catalog, file_id: i64) -> Result<PathBuf, SaveError> {
    let path: String =
        cat.conn().query_row("SELECT path FROM files WHERE id = ?1", [file_id], |r| r.get(0))?;
    Ok(PathBuf::from(path))
}

/// Save one photo per the §F spec. See the module docs for the ordering.
pub fn save_photo(cat: &mut Catalog, file_id: i64) -> Result<SavedPhoto, SaveError> {
    let path = file_path(cat, file_id)?;
    let Some(recipe) = cat.file_recipe(file_id)? else { return Err(SaveError::NoRecipe) };
    let Some(format) = save_format(&path) else {
        return Err(SaveError::UnsupportedFormat(
            path.extension().and_then(|e| e.to_str()).unwrap_or("").to_string(),
        ));
    };

    let archived = archived_path(&path);
    std::fs::create_dir_all(originals_dir(&path))?;

    // 1. Archive first — after this instant the pixels are safe. On a
    //    re-save the archive already holds the untouched original; render
    //    from it so the recipe is never applied to edited pixels twice.
    let render_src = if archived.exists() {
        archived.clone()
    } else {
        std::fs::rename(&path, &archived)?;
        archived.clone()
    };

    // 2. Render into the original path (atomic inside the export engine).
    //    A failure here is recoverable: the original sits untouched in
    //    `.originals/`.
    let opts = ExportOptions { format, quality: SAVE_JPEG_QUALITY, resize_max_edge: None };
    if let Err(e) = export_image(&render_src, &path, &recipe, &opts) {
        if !archived.exists() || path.exists() {
            // Best effort: put the original back so the photo is exactly
            // what it was before Save was attempted.
            let _ = std::fs::rename(&archived, &path);
        }
        return Err(e.into());
    }

    // 3. Catalog the archived original (hidden) and stack it under the
    //    saved photo — the saved file leads, the grid keeps showing the
    //    edited pixels with a stack badge. Re-save keeps the existing stack.
    let archived_id = cat.register_file(&archived, "image")?;
    cat.hide_photos(&[archived_id])?;
    let stack_id = match existing_save_stack(cat, file_id)? {
        Some(stack_id) => stack_id,
        None => {
            let stack_id = cat.stack_photos(&[file_id, archived_id])?;
            refresh_saved_row(cat, file_id, &path)?;
            stack_id
        }
    };
    if stack_id_present_but_member_missing(cat, &stack_id, archived_id)? {
        cat.stack_photos(&[file_id, archived_id])?;
    }
    refresh_saved_row(cat, file_id, &path)?;

    Ok(SavedPhoto { file_id, archived_id, stack_id })
}

/// The stack a previous Save created for this photo, if any. Re-Save must
/// reuse it instead of stacking the (already stacked) saved file again.
fn existing_save_stack(cat: &Catalog, file_id: i64) -> Result<Option<String>, SaveError> {
    let stack_id: Option<String> =
        cat.conn()
            .query_row("SELECT stack_id FROM files WHERE id = ?1", [file_id], |r| r.get(0))?;
    Ok(stack_id)
}

fn stack_id_present_but_member_missing(
    cat: &Catalog,
    stack_id: &str,
    archived_id: i64,
) -> Result<bool, SaveError> {
    let members = cat.stack_members(stack_id)?;
    Ok(!members.iter().any(|m| m.id == archived_id))
}

/// The saved file's bytes changed on disk: refresh size/mtime and drop the
/// cached content hash so the lazy backfill re-hashes and `thumb://` serves
/// the edited pixels, not the pre-save ones.
fn refresh_saved_row(cat: &Catalog, file_id: i64, path: &Path) -> Result<(), SaveError> {
    let meta = std::fs::metadata(path)?;
    cat.conn().execute(
        "UPDATE files SET size = ?1, mtime = ?2, hash = NULL WHERE id = ?3",
        rusqlite::params![
            meta.len() as i64,
            meta.modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map_or(0.0, |d| d.as_secs_f64()),
            file_id
        ],
    )?;
    Ok(())
}

/// Revert: restore the archived original to the photo's path byte-identical,
/// clear the recipe (the edits lived on the overwritten pixels) and remove
/// the save's stack record and archive row.
pub fn revert_photo(cat: &mut Catalog, file_id: i64) -> Result<(), SaveError> {
    let path = file_path(cat, file_id)?;
    let archived = archived_path(&path);
    if !archived.exists() {
        return Err(SaveError::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "no archived original to revert to",
        )));
    }

    // Atomic restore: copy to a temp name in the destination folder, then
    // rename over the saved pixels. The archive stays intact until the
    // rename succeeded.
    let tmp = path.with_extension(format!(
        "{}.revert.tmp",
        path.extension().and_then(|e| e.to_str()).unwrap_or("jpg")
    ));
    std::fs::copy(&archived, &tmp)?;
    if let Err(e) = std::fs::rename(&tmp, &path) {
        let _ = std::fs::remove_file(&tmp);
        return Err(e.into());
    }

    // Catalog cleanup: dissolve the save's stack (clears both members'
    // sidecar records; the archived row goes right after), drop the
    // archived row, clear the recipe, refresh the row's size/mtime/hash.
    if let Some(stack_id) = existing_save_stack(cat, file_id)? {
        let members = cat.stack_members(&stack_id)?;
        if members.len() == 2 && members.iter().any(|m| m.id == archived_id_of(cat, file_id)) {
            cat.unstack(&stack_id)?;
        }
    }
    if let Some(archived_id) = archived_row_id(cat, file_id) {
        cat.remove_file(archived_id)?;
    }
    let _ = std::fs::remove_file(&archived);
    // The archived member's own sidecar (stack membership + hidden flag)
    // and its backup go with it.
    let _ =
        std::fs::remove_file(fotobase_catalog::sidecar_path(archived.to_string_lossy().as_ref()));
    let _ = std::fs::remove_file(
        fotobase_catalog::sidecar_path(archived.to_string_lossy().as_ref())
            .with_extension("xmp.fb-bak"),
    );
    cat.clear_recipe(file_id)?;
    refresh_saved_row(cat, file_id, &path)?;
    Ok(())
}

fn archived_id_of(cat: &Catalog, file_id: i64) -> i64 {
    archived_row_id(cat, file_id).unwrap_or(-1)
}

/// The catalog row of this photo's archived original, if Save recorded one.
fn archived_row_id(cat: &Catalog, file_id: i64) -> Option<i64> {
    let path: String = cat
        .conn()
        .query_row("SELECT path FROM files WHERE id = ?1", [file_id], |r| r.get(0))
        .ok()?;
    let archived = archived_path(Path::new(&path));
    cat.conn()
        .query_row("SELECT id FROM files WHERE path = ?1", [archived.to_string_lossy()], |r| {
            r.get(0)
        })
        .ok()
}
