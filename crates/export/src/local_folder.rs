//! Local-folder export plugin (TASK-08-008, PLAN §6 P4 "plugins: local
//! folder"): the "remote endpoint" is a directory the user picked.
//!
//! - **Albums are subfolders.** An album name maps to a path below the
//!   destination root; `/` (or `\`) in the name nests. Every component is
//!   sanitized so names are valid on all three OSes and can never escape the
//!   root. [`AlbumRef::id`] and [`AlbumRef::name`] are both that sanitized
//!   relative path (always `/`-separated).
//! - **Idempotent per [`ContentKey`].** A file already at its destination
//!   with the same content is left untouched and reported
//!   [`FileStatus::AlreadyPresent`]. Different content under the same name is
//!   never overwritten: the new file is stored as `<stem>-<key prefix>.<ext>`
//!   instead, which is also found again on re-runs.
//! - **Atomic.** Bytes are staged in a hidden `.<name>.fb-part` file next to
//!   the destination, verified against the content key, then renamed into
//!   place. Staging files left by a hard kill are swept the first time the
//!   plugin writes to a directory.
//!
//! Sources are only ever read; existing files are never replaced.

use std::collections::HashSet;
use std::fs::{self, File};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;

use crate::{
    AlbumRef, ContentKey, ExportError, ExportPlugin, FileOutcome, FileStatus, FileUpload,
    PluginCapabilities, PluginInfo,
};

pub const PLUGIN_ID: &str = "local-folder";

const STAGING_SUFFIX: &str = ".fb-part";
const DEFAULT_ALBUM: &str = "Untitled album";
/// Per path component; leaves headroom under the 255-byte filesystem limit
/// for the collision suffix and the staging name.
const MAX_COMPONENT_BYTES: usize = 100;
const MAX_EXTENSION_BYTES: usize = 16;

/// Test seam for simulating crashes and lost responses between the steps of
/// a write. Production code uses the no-op default.
pub trait FaultPoints: Send + Sync {
    /// Called once the file is staged and verified, right before the rename.
    /// An error aborts the write and discards the staged file.
    fn before_commit(&self) -> Result<(), ExportError> {
        Ok(())
    }

    /// Called after the rename; an error makes a stored file look failed.
    fn after_commit(&self) -> Result<(), ExportError> {
        Ok(())
    }

    /// Called after the album folder exists.
    fn after_create_album(&self) -> Result<(), ExportError> {
        Ok(())
    }
}

struct NoFaults;

impl FaultPoints for NoFaults {}

pub struct LocalFolderPlugin {
    root: PathBuf,
    faults: Arc<dyn FaultPoints>,
    swept: HashSet<PathBuf>,
}

impl LocalFolderPlugin {
    /// Exports below `root`, created on first write when missing.
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into(), faults: Arc::new(NoFaults), swept: HashSet::new() }
    }

    pub fn with_fault_points(mut self, faults: Arc<dyn FaultPoints>) -> Self {
        self.faults = faults;
        self
    }
}

#[async_trait]
impl ExportPlugin for LocalFolderPlugin {
    fn info(&self) -> PluginInfo {
        PluginInfo {
            id: PLUGIN_ID.into(),
            name: "Local folder".into(),
            capabilities: PluginCapabilities { albums: true, size_choice: true },
        }
    }

    async fn list_albums(&mut self) -> Result<Vec<AlbumRef>, ExportError> {
        let root = self.root.clone();
        run_blocking(move || {
            let mut albums = Vec::new();
            collect_albums(&root, "", &mut albums)?;
            Ok(albums)
        })
        .await
    }

    async fn create_album(&mut self, _job_id: &str, name: &str) -> Result<AlbumRef, ExportError> {
        // The folder is the album: the same name always maps to the same
        // folder, which makes this idempotent for any job id.
        let id = album_id(name);
        fs::create_dir_all(self.root.join(&id))?;
        self.faults.after_create_album()?;
        Ok(AlbumRef { name: id.clone(), id })
    }

    async fn upload_file(&mut self, request: FileUpload<'_>) -> Result<FileOutcome, ExportError> {
        let album_id = request.album.map(|a| album_id(&a.id));
        let dir = match &album_id {
            Some(id) => self.root.join(id),
            None => self.root.clone(),
        };
        let write = Write {
            sweep: self.swept.insert(dir.clone()),
            dir,
            name: sanitize_file_name(&request.file.name),
            source: request.file.path.clone(),
            key: request.file.content_key.clone(),
            faults: Arc::clone(&self.faults),
        };
        let (name, status) = run_blocking(move || write.store()).await?;
        let remote_id = match album_id {
            Some(id) => format!("{id}/{name}"),
            None => name,
        };
        Ok(FileOutcome { remote_id, status })
    }
}

async fn run_blocking<T: Send + 'static>(
    work: impl FnOnce() -> Result<T, ExportError> + Send + 'static,
) -> Result<T, ExportError> {
    tokio::task::spawn_blocking(work)
        .await
        .map_err(|e| ExportError::Fatal(format!("export worker failed: {e}")))?
}

struct Write {
    dir: PathBuf,
    name: String,
    source: PathBuf,
    key: ContentKey,
    sweep: bool,
    faults: Arc<dyn FaultPoints>,
}

impl Write {
    fn store(self) -> Result<(String, FileStatus), ExportError> {
        fs::create_dir_all(&self.dir)?;
        if self.sweep {
            sweep_staging(&self.dir)?;
        }
        let (name, present) = self.pick_name()?;
        if present {
            return Ok((name, FileStatus::AlreadyPresent));
        }

        let staged = self.dir.join(format!(".{name}{STAGING_SUFFIX}"));
        let committed = self
            .stage(&staged)
            .and_then(|()| self.faults.before_commit())
            .and_then(|()| fs::rename(&staged, self.dir.join(&name)).map_err(ExportError::from));
        if let Err(e) = committed {
            let _ = fs::remove_file(&staged);
            return Err(e);
        }
        self.faults.after_commit()?;
        Ok((name, FileStatus::Uploaded))
    }

    /// First candidate name that is free or already holds this content.
    fn pick_name(&self) -> Result<(String, bool), ExportError> {
        let (stem, ext) = split_extension(&self.name);
        let key = self.key.as_str();
        let candidates =
            [self.name.clone(), format!("{stem}-{}{ext}", &key[..8]), format!("{stem}-{key}{ext}")];
        for name in candidates {
            match File::open(self.dir.join(&name)) {
                Ok(file) => {
                    if file.metadata()?.is_file() && ContentKey::from_reader(file)? == self.key {
                        return Ok((name, true));
                    }
                }
                Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok((name, false)),
                Err(e) => return Err(e.into()),
            }
        }
        Err(ExportError::Fatal(format!("no free destination name for `{}`", self.name)))
    }

    fn stage(&self, staged: &Path) -> Result<(), ExportError> {
        let mut source = File::open(&self.source)?;
        let mut out = File::create(staged)?;
        io::copy(&mut source, &mut out)?;
        out.sync_all()?;
        drop(out);
        if ContentKey::from_reader(File::open(staged)?)? != self.key {
            return Err(ExportError::Fatal(format!(
                "`{}` changed after it was hashed",
                self.source.display()
            )));
        }
        Ok(())
    }
}

fn sweep_staging(dir: &Path) -> io::Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.starts_with('.') && name.ends_with(STAGING_SUFFIX) && entry.file_type()?.is_file() {
            fs::remove_file(entry.path())?;
        }
    }
    Ok(())
}

fn collect_albums(dir: &Path, prefix: &str, out: &mut Vec<AlbumRef>) -> Result<(), ExportError> {
    let mut folders = match fs::read_dir(dir) {
        Ok(entries) => entries
            .map(|e| e.and_then(|e| Ok((e.file_name(), e.file_type()?))))
            .collect::<io::Result<Vec<_>>>()?,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e.into()),
    };
    folders.retain(|(name, kind)| kind.is_dir() && !name.to_string_lossy().starts_with('.'));
    folders.sort_by(|a, b| a.0.cmp(&b.0));
    for (name, _) in folders {
        let name = name.to_string_lossy();
        let id = if prefix.is_empty() { name.to_string() } else { format!("{prefix}/{name}") };
        out.push(AlbumRef { id: id.clone(), name: id.clone() });
        collect_albums(&dir.join(name.as_ref()), &id, out)?;
    }
    Ok(())
}

/// Sanitized relative album path for an album name or id.
fn album_id(name: &str) -> String {
    let parts: Vec<String> = name
        .split(['/', '\\'])
        .filter(|part| !part.trim().is_empty())
        .map(|part| finish_component(&clean(part)))
        .collect();
    if parts.is_empty() {
        DEFAULT_ALBUM.into()
    } else {
        parts.join("/")
    }
}

fn sanitize_file_name(name: &str) -> String {
    let cleaned = clean(name);
    let (stem, ext) = split_extension(&cleaned);
    let stem = truncate(stem, MAX_COMPONENT_BYTES - ext.len()).trim_end_matches([' ', '.']);
    finish_component(&format!("{stem}{ext}"))
}

/// Replaces characters that are invalid on some OS, and leading dots (no
/// hidden files, no `.`/`..`).
fn clean(part: &str) -> String {
    let replaced: String = part
        .chars()
        .map(|c| if c.is_control() || "<>:\"/\\|?*".contains(c) { '_' } else { c })
        .collect();
    let trimmed = replaced.trim().trim_start_matches('.').trim_end_matches([' ', '.']);
    if trimmed.is_empty() {
        "_".into()
    } else {
        trimmed.into()
    }
}

/// Length cap and Windows device-name guard.
fn finish_component(cleaned: &str) -> String {
    let capped = truncate(cleaned, MAX_COMPONENT_BYTES).trim_end_matches([' ', '.']);
    let capped = if capped.is_empty() { "_" } else { capped };
    let base = capped.split('.').next().unwrap_or(capped).to_ascii_uppercase();
    let reserved = matches!(base.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || ((base.starts_with("COM") || base.starts_with("LPT"))
            && base.len() == 4
            && base.as_bytes()[3].is_ascii_digit()
            && base.as_bytes()[3] != b'0');
    if reserved {
        format!("_{capped}")
    } else {
        capped.into()
    }
}

fn truncate(s: &str, max_bytes: usize) -> &str {
    let mut end = s.len().min(max_bytes);
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

/// `("photo", ".jpg")`; no extension when the dot leads or the tail is long.
fn split_extension(name: &str) -> (&str, &str) {
    match name.rfind('.') {
        Some(dot) if dot > 0 && name.len() - dot <= MAX_EXTENSION_BYTES => name.split_at(dot),
        _ => (name, ""),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn album_paths_nest_and_sanitize_each_component() {
        assert_eq!(album_id("Holiday"), "Holiday");
        assert_eq!(album_id("Trips/2024\\Rome"), "Trips/2024/Rome");
        assert_eq!(album_id("a<b>:c?"), "a_b__c_");
        assert_eq!(album_id("../../etc"), "_/_/etc");
        assert_eq!(album_id("  //  "), DEFAULT_ALBUM);
        assert_eq!(album_id("Trailing dot. /x. "), "Trailing dot/x");
        assert_eq!(album_id("com1/Lpt9.txt/COM0"), "_com1/_Lpt9.txt/COM0");
    }

    #[test]
    fn album_id_is_stable_under_resanitizing() {
        for name in ["a<b", "../x", "CON", "Trips/2024: Rome?", "  .hidden ", "é/ü"] {
            let once = album_id(name);
            assert_eq!(album_id(&once), once, "{name}");
        }
    }

    #[test]
    fn long_names_keep_their_extension_and_fit_the_limit() {
        let name = sanitize_file_name(&format!("{}.jpeg", "é".repeat(300)));
        assert!(name.len() <= MAX_COMPONENT_BYTES, "{}", name.len());
        assert!(name.ends_with(".jpeg"));
    }

    #[test]
    fn file_names_lose_separators_and_leading_dots() {
        assert_eq!(sanitize_file_name("../a/b.jpg"), "_a_b.jpg");
        assert_eq!(sanitize_file_name("IMG 1.JPG"), "IMG 1.JPG");
        assert_eq!(sanitize_file_name("NUL.txt"), "_NUL.txt");
        assert_eq!(sanitize_file_name("..."), "_");
    }
}
