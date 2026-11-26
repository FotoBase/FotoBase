//! Edit-view backend (TASK-04-010, PLAN §2D, §3 rules 1/3/4).
//!
//! Three pieces:
//! - recipe persistence: validate a v1 recipe packet, then write it through
//!   the sidecar (source of truth, §5) via the catalog's write-through — the
//!   original file is never touched (§9 Data safety);
//! - preview rendering: decode → shared recipe engine → screen-resolution
//!   JPEG in the bounded job queue (visible-first priority), cached on disk
//!   under `<cache>/edit/`;
//! - serving: the existing `preview://` scheme gains an `?r=<recipe-id>`
//!   query that selects the edited cache entry — still zero image bytes
//!   over IPC.
//!
//! The pure/testable core lives here; the Tauri command adapters are thin
//! wrappers in `lib.rs`.

use fotobase_catalog::Catalog;
use fotobase_jobs::{Job, Queue};
use fotobase_render::{recipe_id, recipe_preview_job, RenderError};

/// Validate a recipe packet, returning the parsed recipe (unknown op types
/// are *not* an error — reader conformance, edit-recipe spec §5 — the
/// engine skips them).
pub fn validate_recipe(
    recipe_json: &str,
) -> Result<(fotobase_core::recipe::Recipe, Vec<String>), String> {
    fotobase_core::recipe::parse(recipe_json).map_err(|e| e.0)
}

/// Persist a photo's edit recipe: validate, then write through the sidecar
/// (sidecar first, catalog mirror second, originals untouched). A packet
/// with no ops (or blank input) clears any existing recipe — "no edits"
/// must not leave an empty `fb:Recipe` behind.
pub fn apply_recipe_to_photo(
    cat: &mut Catalog,
    file_id: i64,
    recipe_json: &str,
) -> Result<(), String> {
    let trimmed = recipe_json.trim();
    if trimmed.is_empty() {
        return cat.clear_recipe(file_id).map_err(|e| e.to_string());
    }
    let (recipe, _) = validate_recipe(trimmed)?;
    if recipe.ops.is_empty() {
        return cat.clear_recipe(file_id).map_err(|e| e.to_string());
    }
    cat.set_recipe(file_id, trimmed).map_err(|e| e.to_string())
}

/// `lens_correction` params for a photo (TASK-10-02-02): its EXIF picks the
/// profile in the Lensfun database; the result is stored in the recipe so
/// later renders never depend on the installed database. The original is
/// only read.
pub fn lens_profile_for_path(path: &std::path::Path) -> Result<serde_json::Value, String> {
    let exif = fotobase_metadata::read_lens_exif(path)?;
    fotobase_render::lens_profile(&fotobase_render::LensQuery {
        camera_maker: exif.camera_maker,
        camera_model: exif.camera_model,
        lens_maker: exif.lens_maker,
        lens_model: exif.lens_model,
        focal: exif.focal,
        aperture: exif.aperture,
        distance: exif.distance,
    })
}

/// The `preview://` URI for an edited preview: the file's content hash plus
/// the recipe id as the `r` query. The scheme handler resolves it to the
/// edited cache entry; no image bytes ever cross IPC.
pub fn edit_preview_uri(content_hash: &str, recipe_json: &str) -> String {
    format!("preview://{content_hash}?r={}", recipe_id(recipe_json))
}

/// Split an edit-preview URI back into `(content_hash, recipe_id)`: the
/// hash must be a well-formed 32-hex cache key (the path-traversal guard),
/// the recipe id 32 hex. `None` for anything else — never an error dialog.
pub fn parse_edit_uri(uri: &str) -> Option<(String, String)> {
    let rest = uri.strip_prefix("preview://")?;
    let (hash, query) = rest.split_once('?')?;
    let id = query.strip_prefix("r=")?;
    if !crate::thumb::is_cache_key(hash)
        || id.len() != 32
        || !id.bytes().all(|b| b.is_ascii_hexdigit())
    {
        return None;
    }
    Some((hash.to_string(), id.to_string()))
}

/// The content hash for a file id: SHA-256 of the file bytes, first 16
/// bytes, lowercase hex — the same key the render pipeline and `thumb://`
/// use. `None` when the row or the file is gone.
pub fn content_hash_for(cat: &Catalog, file_id: i64) -> Option<String> {
    let path: String = cat
        .conn()
        .query_row("SELECT path FROM files WHERE id = ?1", [file_id], |r| r.get(0))
        .ok()?;
    let bytes = std::fs::read(&path).ok()?;
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(&bytes);
    Some(digest[..16].iter().fold(String::new(), |mut acc, b| {
        use std::fmt::Write;
        let _ = write!(acc, "{b:02x}");
        acc
    }))
}

/// Source path for a file id, straight from the catalog row.
pub fn path_for(cat: &Catalog, file_id: i64) -> Option<String> {
    cat.conn().query_row("SELECT path FROM files WHERE id = ?1", [file_id], |r| r.get(0)).ok()
}

/// Build the queue job that renders a photo's edited preview at screen
/// resolution. The catalog is consulted *before* the job is built (the
/// connection is not `Send`); the job only touches the filesystem.
pub fn edit_preview_job(
    src: std::path::PathBuf,
    cache_dir: &std::path::Path,
    recipe_json: String,
) -> Result<Job<Result<fotobase_render::RecipePreview, RenderError>>, String> {
    // Validate up front so a malformed recipe fails the command instead of
    // burning a queue slot.
    validate_recipe(&recipe_json)?;
    Ok(recipe_preview_job(
        src,
        cache_dir.to_path_buf(),
        recipe_json,
        EDIT_PREVIEW_MAX_EDGE,
        EDIT_PREVIEW_QUALITY,
    ))
}

/// Longest edge of an edit preview (screen resolution, not full size:
/// PLAN §2D "renders at screen resolution in the job queue").
pub const EDIT_PREVIEW_MAX_EDGE: u32 = 2048;

/// JPEG quality of the cached edit preview.
pub const EDIT_PREVIEW_QUALITY: u8 = 90;

/// Everything an edit-preview render needs, resolved while the catalog
/// lock is held (the connection is not `Send`). `job` is `None` for the
/// no-edits case: the plain original preview serves.
pub struct PreparedEditPreview {
    /// Content hash (the `preview://` key).
    pub hash: String,
    /// Job to enqueue into the edit queue; `None` when the recipe is empty.
    pub job: Option<Job<Result<fotobase_render::RecipePreview, RenderError>>>,
}

/// Resolve the source path + content hash under the catalog lock and build
/// the render job. An empty recipe packet needs no job — the plain original
/// preview serves.
pub fn prepare_edit_preview(
    cat: &Catalog,
    cache_dir: &std::path::Path,
    file_id: i64,
    recipe_json: &str,
) -> Result<PreparedEditPreview, String> {
    let hash = content_hash_for(cat, file_id).ok_or("unknown photo")?;
    if recipe_json.trim().is_empty() {
        return Ok(PreparedEditPreview { hash, job: None });
    }
    let src = path_for(cat, file_id).ok_or("photo unreadable")?;
    let job = edit_preview_job(src.into(), cache_dir, recipe_json.to_string())?;
    Ok(PreparedEditPreview { hash, job: Some(job) })
}

/// Await a prepared render and map the outcome to the `preview://` URI.
/// The caller must have verified `prepared.job` is `Some`.
pub async fn finish_edit_preview(
    queue: &Queue<Result<fotobase_render::RecipePreview, RenderError>>,
    prepared: PreparedEditPreview,
    recipe_json: &str,
) -> Result<String, String> {
    let job = prepared.job.expect("finish_edit_preview requires a job");
    // Coalesce on (content hash, recipe id): identical requests share one
    // render regardless of which edit view asked.
    let key = format!("{}:{}", prepared.hash, recipe_id(recipe_json));
    let handle = queue
        .enqueue(key, fotobase_jobs::Priority::Visible, job)
        .await
        .map_err(|e| e.to_string())?;
    match &*handle.result().await {
        fotobase_jobs::Outcome::Done(Ok(_)) => Ok(edit_preview_uri(&prepared.hash, recipe_json)),
        fotobase_jobs::Outcome::Done(Err(e)) => Err(e.to_string()),
        fotobase_jobs::Outcome::Cancelled => Err("edit preview render cancelled".to_string()),
        fotobase_jobs::Outcome::Failed(e) => Err(e.clone()),
    }
}

/// Render (or fetch from cache) a photo's edited preview and return its
/// `preview://` URI. The render runs in the bounded job queue at visible
/// priority with recipe-key coalescing; the caller awaits the outcome so
/// the edit view can swap the image source as soon as pixels exist.
pub async fn render_edit_preview(
    cat: &Catalog,
    cache_dir: &std::path::Path,
    queue: &Queue<Result<fotobase_render::RecipePreview, RenderError>>,
    file_id: i64,
    recipe_json: &str,
) -> Result<String, String> {
    let prepared = prepare_edit_preview(cat, cache_dir, file_id, recipe_json)?;
    match prepared.job {
        Some(job) => {
            let p = PreparedEditPreview { hash: prepared.hash, job: Some(job) };
            finish_edit_preview(queue, p, recipe_json).await
        }
        None => Ok(format!("preview://{}", prepared.hash)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_edit_uri_round_trips_and_guards() {
        let uri = edit_preview_uri("e3b0c44298fc1c149afbf4c8996fb924", BW_TEST);
        let (hash, id) = parse_edit_uri(&uri).unwrap();
        assert_eq!(hash, "e3b0c44298fc1c149afbf4c8996fb924");
        assert_eq!(id, recipe_id(BW_TEST));

        assert!(parse_edit_uri("preview://../etc/passwd?r=x").is_none());
        assert!(parse_edit_uri("thumb://e3b0c44298fc1c149afbf4c8996fb924").is_none());
        assert!(parse_edit_uri("preview://e3b0c44298fc1c149afbf4c8996fb924").is_none());
    }

    const BW_TEST: &str =
        r#"{"spec":"fotobase.recipe","version":1,"ops":[{"type":"bw","params":{"amount":1.0}}]}"#;

    #[test]
    fn validate_recipe_accepts_known_ops_and_rejects_malformed() {
        assert!(validate_recipe(BW_TEST).is_ok());
        assert!(validate_recipe("{broken").is_err());
        assert!(validate_recipe(r#"{"spec":"x","version":1,"ops":[]}"#).is_err());
        // Unknown op type: skipped, not fatal (spec §5).
        assert!(validate_recipe(
            r#"{"spec":"fotobase.recipe","version":1,"ops":[{"type":"future_op","params":{}}]}"#
        )
        .is_ok());
    }

    /// A JPEG whose EXIF says what a Canon body writes for this lens.
    #[cfg(not(any(windows, target_os = "android", target_os = "ios")))]
    fn canon_jpeg(path: &std::path::Path, lens_model: &str) {
        use exif::{Field, In, Rational, Tag, Value};
        let ascii = |tag, s: &str| Field {
            tag,
            ifd_num: In::PRIMARY,
            value: Value::Ascii(vec![s.as_bytes().to_vec()]),
        };
        let ratio = |tag, num, denom| Field {
            tag,
            ifd_num: In::PRIMARY,
            value: Value::Rational(vec![Rational { num, denom }]),
        };
        let fields = [
            ascii(Tag::Make, "Canon"),
            ascii(Tag::Model, "Canon EOS 5D Mark II"),
            ascii(Tag::LensModel, lens_model),
            ratio(Tag::FocalLength, 50, 1),
            ratio(Tag::FNumber, 28, 10),
        ];
        let mut tiff = std::io::Cursor::new(Vec::new());
        let mut writer = exif::experimental::Writer::new();
        fields.iter().for_each(|f| writer.push_field(f));
        writer.write(&mut tiff, false).unwrap();
        let tiff = tiff.into_inner();
        let mut jpeg = Vec::new();
        image::codecs::jpeg::JpegEncoder::new(&mut jpeg)
            .encode(&[100u8; 8 * 8 * 3], 8, 8, image::ExtendedColorType::Rgb8)
            .unwrap();
        let mut out = jpeg[..2].to_vec();
        out.extend([0xFF, 0xE1]);
        out.extend(((tiff.len() + 8) as u16).to_be_bytes());
        out.extend(b"Exif\0\0");
        out.extend(&tiff);
        out.extend(&jpeg[2..]);
        std::fs::write(path, out).unwrap();
    }

    /// TASK-10-02-02 acceptance: the lens profile comes from the photo's
    /// EXIF via the installed Lensfun database, ready to store as a recipe op.
    #[cfg(not(any(windows, target_os = "android", target_os = "ios")))]
    #[test]
    fn lens_profile_is_picked_from_exif_via_the_lensfun_database() {
        let dir = tempfile::tempdir().unwrap();
        let photo = dir.path().join("IMG_0001.jpg");
        canon_jpeg(&photo, "EF50mm f/1.8 II");
        let before = std::fs::read(&photo).unwrap();

        let params = lens_profile_for_path(&photo).expect("profile from EXIF");
        assert!(params["lens"].as_str().unwrap().contains("50mm f/1.8"), "{params}");
        assert_eq!(params["crop"], 1.0);
        assert_eq!(params["focal"], 50.0);
        assert_eq!(params["aperture"], 2.8);
        let recipe = serde_json::json!({ "spec": "fotobase.recipe", "version": 1,
            "ops": [{ "type": "lens_correction", "params": params }] });
        assert!(validate_recipe(&recipe.to_string()).is_ok());
        assert_eq!(std::fs::read(&photo).unwrap(), before, "original untouched");

        let unknown = dir.path().join("IMG_0002.jpg");
        canon_jpeg(&unknown, "Nonexistent 13mm f/0.7");
        let err = lens_profile_for_path(&unknown).unwrap_err();
        assert!(err.contains("Nonexistent 13mm f/0.7"), "{err}");
    }
}
