//! Static HTML gallery generator (TASK-08-016, PLAN §2 §J "HTML export",
//! §7 F1 layer 1, §7 F4).
//!
//! [`generate`] turns a list of photos into a self-contained static site
//! below a destination folder: `index.html`, a shared viewer script and
//! stylesheets from the chosen [`Template`], resized images in `images/`,
//! thumbnails in `thumbs/`, and `manifest.json`. Nothing in the site loads
//! from the network, so it works when `index.html` is opened from disk, a
//! USB stick or any static host. The generator only produces the folder:
//! uploading it is the export plugins' job (F1.1), and F4 gift export reuses
//! it unchanged.
//!
//! **Incremental by construction.** Image and thumbnail file names are
//! derived from a hash of the source bytes, the recipe and the render
//! settings, so an unchanged photo keeps its file names and is neither
//! re-rendered nor rewritten; only a changed photo gets new files (its old
//! ones are pruned) plus the pages that link them. Output files are written
//! atomically (`.fb-part` + rename) and only when their bytes differ.
//! Files this generator did not create — anything outside the managed
//! names — are never touched or removed.
//!
//! Sources are only read. Resizing goes through an [`ImageRenderer`]; the
//! production one is [`RenderPipeline`], the shared EPIC-04 export engine
//! (recipe applied, color managed, never upscaled). The work is blocking and
//! CPU-heavy: callers run it as a queue job (PLAN §3 rule 4).

mod render;
mod template;

use std::collections::{HashMap, HashSet};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use thiserror::Error;

pub use render::{ImageRenderer, RenderPipeline, RenderRequest, RenderedImage, EMPTY_RECIPE};
pub use template::{
    builtin_template, builtin_templates, escape_html, render as render_template, Template,
    TemplateAsset,
};

const MANIFEST: &str = "manifest.json";
const INDEX: &str = "index.html";
const IMAGES_DIR: &str = "images";
const THUMBS_DIR: &str = "thumbs";
const STAGING_SUFFIX: &str = ".fb-part";
/// Bump when the rendering of a given input changes, so old outputs are not
/// mistaken for current ones.
const KEY_REVISION: &[u8] = b"fotobase-gallery-1";
const KEY_HEX_LEN: usize = 16;

#[derive(Debug, Error)]
pub enum GalleryError {
    #[error("no photos to export")]
    NoPhotos,
    #[error("invalid gallery options: {0}")]
    Options(String),
    #[error("gallery template: {0}")]
    Template(String),
    #[error("render failed for {path}: {message}")]
    Render { path: PathBuf, message: String },
    #[error("I/O: {0}")]
    Io(#[from] io::Error),
}

fn io_at(path: &Path, e: io::Error) -> GalleryError {
    GalleryError::Io(io::Error::new(e.kind(), format!("{}: {e}", path.display())))
}

/// One photo to publish.
#[derive(Debug, Clone)]
pub struct GalleryPhoto {
    pub source: PathBuf,
    pub title: Option<String>,
    pub caption: Option<String>,
    /// Edit recipe JSON applied when rendering; `None` renders the original
    /// pixels.
    pub recipe: Option<String>,
}

#[derive(Debug, Clone)]
pub struct GalleryOptions {
    pub title: String,
    pub description: Option<String>,
    /// Informational: the template to render is the one passed to
    /// [`generate`]; this id is recorded in the manifest.
    pub template: String,
    /// Longest edge of the viewer images.
    pub image_max_edge: u32,
    pub image_quality: u8,
    /// Longest edge of the grid thumbnails.
    pub thumb_max_edge: u32,
    pub thumb_quality: u8,
}

impl Default for GalleryOptions {
    fn default() -> Self {
        Self {
            title: "Gallery".into(),
            description: None,
            template: "grid".into(),
            image_max_edge: 2048,
            image_quality: 85,
            thumb_max_edge: 320,
            thumb_quality: 80,
        }
    }
}

impl GalleryOptions {
    fn validate(&self) -> Result<(), GalleryError> {
        let bad = |what: &str| Err(GalleryError::Options(what.to_owned()));
        if self.image_max_edge == 0 || self.thumb_max_edge == 0 {
            return bad("max edges must be > 0");
        }
        if !(1..=100).contains(&self.image_quality) || !(1..=100).contains(&self.thumb_quality) {
            return bad("quality must be 1-100");
        }
        Ok(())
    }
}

/// What a [`generate`] run did. File counts cover every managed output.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GalleryReport {
    pub photos: usize,
    /// Files created or whose bytes changed.
    pub written: usize,
    /// Files already up to date and left alone.
    pub unchanged: usize,
    /// Stale images/thumbnails pruned.
    pub removed: usize,
}

#[derive(Clone, Copy)]
struct Dims {
    width: u32,
    height: u32,
}

struct Rendition {
    /// Site-relative path, `/`-separated.
    path: String,
    dims: Dims,
}

/// Exports `photos` as a static site into `dest` (created when missing).
///
/// Re-running with the same inputs rewrites nothing; see the module docs.
pub fn generate(
    dest: &Path,
    photos: &[GalleryPhoto],
    options: &GalleryOptions,
    template: &Template,
    renderer: &dyn ImageRenderer,
) -> Result<GalleryReport, GalleryError> {
    if photos.is_empty() {
        return Err(GalleryError::NoPhotos);
    }
    options.validate()?;
    for asset in &template.assets {
        template::validate_asset_path(&asset.path)?;
        if is_managed_name(&asset.path) {
            return Err(GalleryError::Template(format!("asset `{}` is reserved", asset.path)));
        }
    }

    fs::create_dir_all(dest).map_err(|e| io_at(dest, e))?;
    let mut known = previous_dims(dest);
    let mut report = GalleryReport { photos: photos.len(), ..GalleryReport::default() };
    let mut live: HashSet<String> = HashSet::new();
    let mut entries = Vec::with_capacity(photos.len());

    for photo in photos {
        let source_bytes = fs::read(&photo.source).map_err(|e| io_at(&photo.source, e))?;
        let source_digest = Sha256::digest(&source_bytes);
        drop(source_bytes);
        let recipe = photo.recipe.as_deref().unwrap_or(EMPTY_RECIPE);

        let mut renditions = Vec::with_capacity(2);
        for (dir, edge, quality) in [
            (IMAGES_DIR, options.image_max_edge, options.image_quality),
            (THUMBS_DIR, options.thumb_max_edge, options.thumb_quality),
        ] {
            let name = format!("{dir}/{}.jpg", file_key(&source_digest, recipe, edge, quality));
            live.insert(name.clone());
            let target = dest.join(&name);
            let dims = match known.get(&name) {
                Some(dims) if target.is_file() => {
                    report.unchanged += 1;
                    *dims
                }
                _ => {
                    let rendered = renderer.render(&RenderRequest {
                        source: &photo.source,
                        recipe,
                        max_edge: edge,
                        quality,
                    })?;
                    let dims = Dims { width: rendered.width, height: rendered.height };
                    tally(&mut report, write_if_changed(&target, &rendered.jpeg)?);
                    known.insert(name.clone(), dims);
                    dims
                }
            };
            renditions.push(Rendition { path: name, dims });
        }
        let [image, thumb]: [Rendition; 2] = renditions.try_into().ok().expect("two renditions");
        entries.push((photo, image, thumb));
    }

    let manifest = manifest_json(options, template, &entries);
    let data = embeddable_json(&manifest);

    let mut items = String::new();
    for (i, (photo, image, thumb)) in entries.iter().enumerate() {
        let alt = photo
            .title
            .clone()
            .or_else(|| photo.caption.clone())
            .or_else(|| photo.source.file_stem().map(|s| s.to_string_lossy().into_owned()))
            .unwrap_or_default();
        let vars: Vec<(&str, String)> = vec![
            ("index", i.to_string()),
            ("image", image.path.clone()),
            ("thumb", thumb.path.clone()),
            ("width", image.dims.width.to_string()),
            ("height", image.dims.height.to_string()),
            ("thumb_width", thumb.dims.width.to_string()),
            ("thumb_height", thumb.dims.height.to_string()),
            ("title", photo.title.clone().unwrap_or_default()),
            ("caption", photo.caption.clone().unwrap_or_default()),
            ("alt", alt),
        ];
        let vars: Vec<(&str, &str)> = vars.iter().map(|(k, v)| (*k, v.as_str())).collect();
        items.push_str(&template::render(&template.item, &vars)?);
        items.push('\n');
    }

    let count = photos.len().to_string();
    let index = template::render(
        &template.index,
        &[
            ("title", &options.title),
            ("description", options.description.as_deref().unwrap_or("")),
            ("count", &count),
            ("items", &items),
            ("data", &data),
        ],
    )?;

    // The page goes last so an interrupted run leaves the previous, fully
    // consistent site in place.
    for asset in &template.assets {
        tally(&mut report, write_if_changed(&dest.join(&asset.path), asset.contents.as_bytes())?);
    }
    tally(&mut report, write_if_changed(&dest.join(MANIFEST), data.as_bytes())?);
    tally(&mut report, write_if_changed(&dest.join(INDEX), index.as_bytes())?);

    report.removed = prune(dest, &live)?;
    Ok(report)
}

fn tally(report: &mut GalleryReport, written: bool) {
    if written {
        report.written += 1;
    } else {
        report.unchanged += 1;
    }
}

fn is_managed_name(path: &str) -> bool {
    path == INDEX
        || path == MANIFEST
        || path.starts_with(&format!("{IMAGES_DIR}/"))
        || path.starts_with(&format!("{THUMBS_DIR}/"))
}

fn file_key(source_digest: &[u8], recipe: &str, edge: u32, quality: u8) -> String {
    let mut hasher = Sha256::new();
    hasher.update(KEY_REVISION);
    hasher.update(source_digest);
    hasher.update((recipe.len() as u64).to_le_bytes());
    hasher.update(recipe.as_bytes());
    hasher.update(edge.to_le_bytes());
    hasher.update([quality]);
    let digest = hasher.finalize();
    let mut hex: String = digest.iter().map(|b| format!("{b:02x}")).collect();
    hex.truncate(KEY_HEX_LEN);
    hex
}

fn manifest_json(
    options: &GalleryOptions,
    template: &Template,
    entries: &[(&GalleryPhoto, Rendition, Rendition)],
) -> Value {
    let rendition =
        |r: &Rendition| json!({ "src": r.path, "width": r.dims.width, "height": r.dims.height });
    let photos: Vec<Value> = entries
        .iter()
        .map(|(photo, image, thumb)| {
            let mut entry = serde_json::Map::new();
            if let Some(title) = &photo.title {
                entry.insert("title".into(), json!(title));
            }
            if let Some(caption) = &photo.caption {
                entry.insert("caption".into(), json!(caption));
            }
            entry.insert("image".into(), rendition(image));
            entry.insert("thumb".into(), rendition(thumb));
            Value::Object(entry)
        })
        .collect();
    json!({
        "version": 1,
        "title": options.title,
        "description": options.description,
        "template": template.id,
        "photos": photos,
    })
}

/// Serializes `value` so the text is also safe inside a `<script>` element
/// (no `</script>` or `<!--` sequences, no JS line terminators).
fn embeddable_json(value: &Value) -> String {
    let text = serde_json::to_string(value).expect("serializing JSON value");
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '<' => out.push_str("\\u003c"),
            '>' => out.push_str("\\u003e"),
            '&' => out.push_str("\\u0026"),
            '\u{2028}' => out.push_str("\\u2028"),
            '\u{2029}' => out.push_str("\\u2029"),
            c => out.push(c),
        }
    }
    out
}

/// Dimensions recorded by the previous run, so unchanged outputs need no
/// decode. A missing or unreadable manifest just means everything re-renders.
fn previous_dims(dest: &Path) -> HashMap<String, Dims> {
    let mut known = HashMap::new();
    let Ok(text) = fs::read_to_string(dest.join(MANIFEST)) else { return known };
    let Ok(manifest) = serde_json::from_str::<Value>(&text) else { return known };
    let Some(photos) = manifest.get("photos").and_then(Value::as_array) else { return known };
    for photo in photos {
        for kind in ["image", "thumb"] {
            let Some(r) = photo.get(kind) else { continue };
            let (Some(src), Some(w), Some(h)) = (
                r.get("src").and_then(Value::as_str),
                r.get("width").and_then(Value::as_u64),
                r.get("height").and_then(Value::as_u64),
            ) else {
                continue;
            };
            let (Ok(width), Ok(height)) = (u32::try_from(w), u32::try_from(h)) else { continue };
            // Only trust names this generator could have produced.
            if is_generated_name(src) {
                known.insert(src.to_owned(), Dims { width, height });
            }
        }
    }
    known
}

/// `images/<16 hex>.jpg` or `thumbs/<16 hex>.jpg`.
fn is_generated_name(rel: &str) -> bool {
    let Some((dir, file)) = rel.split_once('/') else { return false };
    if dir != IMAGES_DIR && dir != THUMBS_DIR {
        return false;
    }
    let Some(stem) = file.strip_suffix(".jpg") else { return false };
    stem.len() == KEY_HEX_LEN && stem.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

/// Removes generated images/thumbnails no longer part of the site.
fn prune(dest: &Path, live: &HashSet<String>) -> Result<usize, GalleryError> {
    let mut removed = 0;
    for dir in [IMAGES_DIR, THUMBS_DIR] {
        let folder = dest.join(dir);
        let entries = match fs::read_dir(&folder) {
            Ok(entries) => entries,
            Err(e) if e.kind() == io::ErrorKind::NotFound => continue,
            Err(e) => return Err(io_at(&folder, e)),
        };
        for entry in entries {
            let entry = entry.map_err(|e| io_at(&folder, e))?;
            let Some(file) = entry.file_name().to_str().map(str::to_owned) else { continue };
            let rel = format!("{dir}/{file}");
            if is_generated_name(&rel) && !live.contains(&rel) {
                fs::remove_file(entry.path()).map_err(|e| io_at(&entry.path(), e))?;
                removed += 1;
            }
        }
    }
    Ok(removed)
}

/// Atomically writes `bytes` unless the file already holds exactly them.
/// Returns whether the file was (re)written.
fn write_if_changed(path: &Path, bytes: &[u8]) -> Result<bool, GalleryError> {
    if fs::read(path).map(|existing| existing == bytes).unwrap_or(false) {
        return Ok(false);
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| io_at(parent, e))?;
    }
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let staging = path.with_file_name(format!(".{name}{STAGING_SUFFIX}"));
    fs::write(&staging, bytes).map_err(|e| io_at(&staging, e))?;
    fs::rename(&staging, path).map_err(|e| {
        let _ = fs::remove_file(&staging);
        io_at(path, e)
    })?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    /// Renders a deterministic stub "image" and records every call.
    struct Stub(RefCell<Vec<(PathBuf, u32)>>);

    impl ImageRenderer for Stub {
        fn render(&self, req: &RenderRequest<'_>) -> Result<RenderedImage, GalleryError> {
            self.0.borrow_mut().push((req.source.to_path_buf(), req.max_edge));
            Ok(RenderedImage {
                jpeg: format!("{}@{}", req.source.display(), req.max_edge).into_bytes(),
                width: req.max_edge,
                height: req.max_edge / 2,
            })
        }
    }

    fn photo(dir: &Path, name: &str, content: &str, caption: Option<&str>) -> GalleryPhoto {
        let source = dir.join(name);
        fs::write(&source, content).unwrap();
        GalleryPhoto {
            source,
            title: Some(name.to_owned()),
            caption: caption.map(str::to_owned),
            recipe: None,
        }
    }

    #[test]
    fn template_render_with_captions_produces_expected_snippets() {
        let src = tempfile::tempdir().unwrap();
        let out = tempfile::tempdir().unwrap();
        let photos = vec![photo(src.path(), "a.jpg", "A", Some("Sea & sky"))];
        let template = builtin_template("grid").unwrap();
        let stub = Stub(RefCell::default());
        generate(out.path(), &photos, &GalleryOptions::default(), &template, &stub).unwrap();

        let html = fs::read_to_string(out.path().join("index.html")).unwrap();
        assert!(html.contains("<span class=\"fb-title\">a.jpg</span>"), "{html}");
        assert!(html.contains("<span class=\"fb-caption\">Sea &amp; sky</span>"), "{html}");
        assert!(html.contains("data-fb-index=\"0\""));
        assert!(html.contains("width=\"320\" height=\"160\""), "thumb dimensions: {html}");
    }

    #[test]
    fn unchanged_outputs_are_not_re_rendered() {
        let src = tempfile::tempdir().unwrap();
        let out = tempfile::tempdir().unwrap();
        let photos =
            vec![photo(src.path(), "a.jpg", "A", None), photo(src.path(), "b.jpg", "B", None)];
        let template = builtin_template("dark").unwrap();
        let options = GalleryOptions::default();

        let first = Stub(RefCell::default());
        generate(out.path(), &photos, &options, &template, &first).unwrap();
        assert_eq!(first.0.borrow().len(), 4, "image + thumb per photo");

        let second = Stub(RefCell::default());
        generate(out.path(), &photos, &options, &template, &second).unwrap();
        assert!(second.0.borrow().is_empty(), "{:?}", second.0.borrow());

        fs::write(&photos[0].source, "A2").unwrap();
        let third = Stub(RefCell::default());
        generate(out.path(), &photos, &options, &template, &third).unwrap();
        let rendered = third.0.borrow();
        assert_eq!(rendered.len(), 2);
        assert!(rendered.iter().all(|(p, _)| p == &photos[0].source));
    }

    #[test]
    fn render_settings_change_the_file_names() {
        let digest = Sha256::digest(b"x");
        let a = file_key(&digest, EMPTY_RECIPE, 100, 80);
        assert_eq!(a, file_key(&digest, EMPTY_RECIPE, 100, 80));
        assert_ne!(a, file_key(&digest, EMPTY_RECIPE, 101, 80));
        assert_ne!(a, file_key(&digest, EMPTY_RECIPE, 100, 81));
        assert_ne!(a, file_key(&digest, "{}", 100, 80));
        assert!(is_generated_name(&format!("images/{a}.jpg")));
    }

    #[test]
    fn embedded_json_cannot_close_the_script_element() {
        let text = embeddable_json(&json!({ "caption": "</script><!-- \u{2028} & x" }));
        assert!(!text.contains("</script"), "{text}");
        assert!(!text.contains('<') && !text.contains('&') && !text.contains('\u{2028}'));
        let back: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(back["caption"], "</script><!-- \u{2028} & x");
    }

    #[test]
    fn reserved_and_escaping_asset_paths_are_rejected() {
        let src = tempfile::tempdir().unwrap();
        let out = tempfile::tempdir().unwrap();
        let photos = vec![photo(src.path(), "a.jpg", "A", None)];
        for bad in ["index.html", "images/x.jpg", "../evil.css"] {
            let mut template = builtin_template("grid").unwrap();
            template.assets.push(TemplateAsset { path: bad.into(), contents: String::new() });
            let err = generate(
                out.path(),
                &photos,
                &GalleryOptions::default(),
                &template,
                &Stub(RefCell::default()),
            )
            .unwrap_err();
            assert!(matches!(err, GalleryError::Template(_)), "{bad}: {err}");
        }
    }

    #[test]
    fn invalid_options_are_rejected() {
        let src = tempfile::tempdir().unwrap();
        let out = tempfile::tempdir().unwrap();
        let photos = vec![photo(src.path(), "a.jpg", "A", None)];
        let template = builtin_template("grid").unwrap();
        for options in [
            GalleryOptions { image_max_edge: 0, ..GalleryOptions::default() },
            GalleryOptions { thumb_quality: 0, ..GalleryOptions::default() },
            GalleryOptions { image_quality: 101, ..GalleryOptions::default() },
        ] {
            let err = generate(out.path(), &photos, &options, &template, &Stub(RefCell::default()))
                .unwrap_err();
            assert!(matches!(err, GalleryError::Options(_)), "{err}");
        }
    }
}
