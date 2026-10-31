//! TASK-04-010 integration tests: the edit-view backend. Recipes persist
//! through the sidecar write-through path (originals untouched, checksum
//! verified — §9 Data safety), edited previews render in the job queue and
//! serve through `preview://?r=<recipe-id>` with zero image bytes over IPC.

use fotobase_catalog::Catalog;
use fotobase_jobs::{Outcome, Priority, Queue};
use fotobase_lib::edit;
use fotobase_metadata::Sidecar;

fn gen_jpeg(dir: &std::path::Path, name: &str) -> std::path::PathBuf {
    std::fs::create_dir_all(dir).unwrap();
    let p = dir.join(name);
    let mut buf = Vec::with_capacity(16 * 12 * 3);
    for y in 0..12u32 {
        for x in 0..16u32 {
            buf.extend_from_slice(&[x as u8 * 16, y as u8 * 20, 128]);
        }
    }
    let mut out = Vec::new();
    {
        use image::ImageEncoder as _;
        let enc = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 92);
        enc.write_image(&buf, 16, 12, image::ExtendedColorType::Rgb8).unwrap();
    }
    std::fs::write(&p, &out).unwrap();
    p
}

fn catalog_with_photo(dir: &std::path::Path, src: &std::path::Path) -> (Catalog, i64) {
    let mut cat = Catalog::open(dir.join("catalog.db")).unwrap();
    let folder = cat.seed_folder(dir.to_str().unwrap()).unwrap();
    cat.conn()
        .execute(
            "INSERT INTO files (folder_id, path, filename, size, mtime, media_type)
             VALUES (?1, ?2, ?3, 10, 0.0, 'image')",
            rusqlite::params![
                folder,
                src.to_string_lossy(),
                src.file_name().unwrap().to_str().unwrap()
            ],
        )
        .unwrap();
    let id = cat.conn().last_insert_rowid();
    (cat, id)
}

const BW: &str =
    r#"{"spec":"fotobase.recipe","version":1,"ops":[{"type":"bw","params":{"amount":1.0}}]}"#;

#[test]
fn applying_a_recipe_never_touches_the_original_checksum() {
    let tmp = tempfile::TempDir::new().unwrap();
    let src = gen_jpeg(&tmp.path().join("photos"), "a.jpg");
    let (mut cat, id) = catalog_with_photo(tmp.path(), &src);
    let before = std::fs::read(&src).unwrap();

    edit::apply_recipe_to_photo(&mut cat, id, BW).unwrap();

    assert_eq!(
        std::fs::read(&src).unwrap(),
        before,
        "§9 Data safety: the edit session must not modify the original"
    );
    let sc = fotobase_catalog::sync::sidecar_path(src.to_str().unwrap());
    let side = Sidecar::read(&sc).unwrap();
    assert!(side.has_recipe());
    assert!(cat.file_recipe(id).unwrap().is_some());
}

#[test]
fn a_malformed_recipe_is_refused_and_writes_nothing() {
    let tmp = tempfile::TempDir::new().unwrap();
    let src = gen_jpeg(&tmp.path().join("photos"), "b.jpg");
    let (mut cat, id) = catalog_with_photo(tmp.path(), &src);
    let sc = fotobase_catalog::sync::sidecar_path(src.to_str().unwrap());

    assert!(edit::apply_recipe_to_photo(&mut cat, id, "{not json").is_err());
    assert!(!sc.exists(), "a rejected recipe must not leave a sidecar behind");
    assert!(cat.file_recipe(id).unwrap().is_none());
}

#[test]
fn an_empty_recipe_clears_a_previous_one() {
    let tmp = tempfile::TempDir::new().unwrap();
    let src = gen_jpeg(&tmp.path().join("photos"), "c.jpg");
    let (mut cat, id) = catalog_with_photo(tmp.path(), &src);

    edit::apply_recipe_to_photo(&mut cat, id, BW).unwrap();
    edit::apply_recipe_to_photo(&mut cat, id, r#"{"spec":"fotobase.recipe","version":1,"ops":[]}"#)
        .unwrap();

    assert!(cat.file_recipe(id).unwrap().is_none());
    let sc = fotobase_catalog::sync::sidecar_path(src.to_str().unwrap());
    assert!(!Sidecar::read(&sc).unwrap().has_recipe());
    let has: bool = cat
        .conn()
        .query_row("SELECT has_recipe FROM files WHERE id = ?1", [id], |r| r.get(0))
        .unwrap();
    assert!(!has);
}

#[tokio::test]
async fn render_edit_preview_runs_in_the_queue_and_serves_via_preview_uri() {
    let tmp = tempfile::TempDir::new().unwrap();
    let src = gen_jpeg(&tmp.path().join("photos"), "d.jpg");
    let bytes = std::fs::read(&src).unwrap();
    let (cat, id) = catalog_with_photo(tmp.path(), &src);
    let cache = tmp.path().join("cache");
    let queue = Queue::new(4);

    let uri = edit::render_edit_preview(&cat, &cache, &queue, id, BW).await.unwrap();
    assert!(
        uri.starts_with("preview://"),
        "edited previews must use the preview:// scheme (PLAN §3 rule 3), got {uri}"
    );
    assert!(uri.contains("?r="), "the recipe id selects the edited cache entry");
    // The scheme handler resolves this URI to the edited pixels, not the
    // original bytes.
    let (hash, recipe_id) = edit::parse_edit_uri(&uri).unwrap();
    let reply =
        fotobase_lib::preview::resolve_preview_with_recipe(&hash, Some(&recipe_id), &cache, &cat);
    assert_eq!(reply.status, 200);
    assert_eq!(reply.content_type, "image/jpeg");
    assert_ne!(reply.body, bytes, "the edited preview is not the original file");
}

#[tokio::test]
async fn render_edit_preview_coalesces_on_the_recipe_key() {
    let tmp = tempfile::TempDir::new().unwrap();
    let src = gen_jpeg(&tmp.path().join("photos"), "e.jpg");
    let (cat, id) = catalog_with_photo(tmp.path(), &src);
    let cache = tmp.path().join("cache");
    let queue = Queue::new(4);

    let uri = edit::render_edit_preview(&cat, &cache, &queue, id, BW).await.unwrap();
    // Same recipe again: resolves from cache without a second render.
    let again = edit::render_edit_preview(&cat, &cache, &queue, id, BW).await.unwrap();
    assert_eq!(uri, again);
    // A different recipe yields a different URI.
    let warm = r#"{"spec":"fotobase.recipe","version":1,"ops":[{"type":"temperature","params":{"kelvin_shift":0.4}}]}"#;
    let other = edit::render_edit_preview(&cat, &cache, &queue, id, warm).await.unwrap();
    assert_ne!(uri, other);
}

#[tokio::test]
async fn render_edit_preview_for_an_unknown_file_fails_without_panicking() {
    let tmp = tempfile::TempDir::new().unwrap();
    let cat = Catalog::open(tmp.path().join("catalog.db")).unwrap();
    let queue = Queue::new(4);
    let err = edit::render_edit_preview(&cat, &tmp.path().join("cache"), &queue, 999, BW).await;
    assert!(err.is_err());
}

#[tokio::test]
async fn a_cancelled_render_job_reports_cancelled_not_an_error() {
    let tmp = tempfile::TempDir::new().unwrap();
    let src = gen_jpeg(&tmp.path().join("photos"), "f.jpg");
    let queue = Queue::new(4);
    let job =
        edit::edit_preview_job(src.clone(), &tmp.path().join("cache"), BW.to_string()).unwrap();
    let handle = queue.enqueue("k", Priority::Visible, job).await.unwrap();
    handle.cancel();
    assert!(matches!(*handle.result().await, Outcome::Cancelled));
}
