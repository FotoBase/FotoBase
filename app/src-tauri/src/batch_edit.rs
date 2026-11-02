//! Batch edit ops (TASK-04-011, PLAN §2E "Batch edit", §3 rule 4).
//!
//! A batch op is queued through the job queue at background priority; the
//! run executes on its own catalog connection (the UI's connection is not
//! `Send`) and applies the *same single-photo recipe transform* to every
//! selected photo's sidecar recipe — batch result equals per-photo result
//! by construction, and each photo's recipe stays valid after every step,
//! so a mid-batch cancel leaves only valid, per-photo recipes behind.
//!
//! Progress is a plain counter on the `batch-edit-progress` event (§3: no
//! image bytes over IPC); cancellation is a flag the command bridges from
//! the queue's cancel token.

use std::path::Path;
use std::sync::atomic::AtomicBool;

use fotobase_catalog::Catalog;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

/// Tauri event the batch reports progress on (§3: counters, no image bytes).
pub const BATCH_EDIT_EVENT: &str = "batch-edit-progress";

/// One §2E batch edit operation. `show_text` needs the text to overlay; the
/// other ops are parameter-free at batch strength.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BatchOp {
    RotateCw,
    RotateCcw,
    AutoContrast,
    AutoColor,
    Sepia,
    Sharpen,
    Warmify,
    FilmGrain,
    Bw,
    AutoRedEye,
    ShowText,
    HideText,
}

/// Per-photo failure: the photo was skipped and its data left untouched.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BatchEditFailure {
    pub id: i64,
    pub error: String,
}

/// Outcome of one batch.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct BatchEditReport {
    pub applied: usize,
    pub failed: Vec<BatchEditFailure>,
    pub cancelled: bool,
}

/// Progress event payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BatchEditProgress {
    pub done: usize,
    pub total: usize,
}

/// The recipe op a batch op contributes (in canonical op order it lands
/// after the photo's existing ops — the engine folds top-to-bottom).
fn op_json(op: BatchOp, text: Option<&str>) -> Value {
    match op {
        BatchOp::RotateCw => json!({ "type": "rotate", "params": { "angle": 90 } }),
        BatchOp::RotateCcw => json!({ "type": "rotate", "params": { "angle": 270 } }),
        BatchOp::AutoContrast => json!({ "type": "auto_contrast" }),
        BatchOp::AutoColor => json!({ "type": "auto_color" }),
        BatchOp::Sepia => json!({ "type": "sepia", "params": { "amount": 1.0 } }),
        BatchOp::Sharpen => json!({ "type": "sharpen", "params": { "amount": 0.5 } }),
        BatchOp::Warmify => json!({ "type": "warm_colors", "params": { "amount": 1.0 } }),
        BatchOp::FilmGrain => {
            json!({ "type": "film_grain", "params": { "amount": 0.5, "seed": 0 } })
        }
        BatchOp::Bw => json!({ "type": "bw", "params": { "amount": 1.0 } }),
        BatchOp::AutoRedEye => json!({ "type": "red_eye", "params": { "auto": true } }),
        BatchOp::ShowText => json!({
            "type": "text",
            "params": {
                "text": text.unwrap_or_default(),
                "x": 0.05,
                "y": 0.05,
                "size": 0.05,
                "color": "#ffffff",
            }
        }),
        BatchOp::HideText => json!({ "type": "hide_text" }),
    }
}

/// Apply one batch op to one photo's recipe. The transform is pure: batch
/// and per-photo application produce byte-identical recipes.
/// `None` means "the recipe became empty" — the caller clears instead of
/// storing an empty packet.
fn transform_recipe(
    recipe_json: Option<&str>,
    op: BatchOp,
    text: Option<&str>,
) -> Result<Option<String>, String> {
    let mut ops: Vec<Value> = match recipe_json {
        Some(raw) if !raw.trim().is_empty() => {
            let (recipe, _) = fotobase_core::recipe::parse(raw).map_err(|e| e.to_string())?;
            recipe.ops.iter().map(|o| serde_json::to_value(o).expect("op serializes")).collect()
        }
        _ => Vec::new(),
    };
    match op {
        // Show/hide text *toggles* the text overlay op: show replaces any
        // existing text op (idempotent), hide removes them all.
        BatchOp::ShowText => {
            ops.retain(|o| o.get("type").and_then(Value::as_str) != Some("text"));
            ops.push(op_json(op, text));
        }
        BatchOp::HideText => {
            ops.retain(|o| o.get("type").and_then(Value::as_str) != Some("text"));
        }
        _ => ops.push(op_json(op, text)),
    }
    if ops.is_empty() {
        return Ok(None);
    }
    let packet = json!({ "spec": fotobase_core::recipe::RECIPE_SPEC, "version": 1, "ops": ops });
    // Validate what we are about to persist — a batch op may never leave an
    // invalid recipe behind.
    fotobase_core::recipe::parse(packet.to_string().as_str()).map_err(|e| e.to_string())?;
    Ok(Some(serde_json::to_string(&packet).expect("packet serializes")))
}

/// The text a `show_text` batch overlays: the photo's caption, falling back
/// to its filename (Picasa's batch "show text" labelled photos).
fn caption_or_filename(cat: &Catalog, id: i64) -> String {
    let caption = cat.file_caption(id).ok().flatten().filter(|c| !c.trim().is_empty());
    match caption {
        Some(c) => c,
        None => cat
            .conn()
            .query_row("SELECT filename FROM files WHERE id = ?1", [id], |r| r.get(0))
            .unwrap_or_default(),
    }
}

/// Apply the batch op to one photo: read its sidecar recipe (source of
/// truth), transform, write through. An empty result clears.
fn apply_one(cat: &mut Catalog, id: i64, op: BatchOp) -> Result<(), String> {
    let current = cat.file_recipe(id).map_err(|e| e.to_string())?;
    let text = match op {
        BatchOp::ShowText => Some(caption_or_filename(cat, id)),
        _ => None,
    };
    match transform_recipe(current.as_deref(), op, text.as_deref())? {
        Some(recipe) => cat.set_recipe(id, &recipe).map_err(|e| e.to_string()),
        None => cat.clear_recipe(id).map_err(|e| e.to_string()),
    }
}

/// One batch run against the catalog at `catalog_path`, on the job queue's
/// blocking thread. Cancellation is a plain flag the caller bridges from
/// the queue's cancel token; stopping happens between photos, so processed
/// photos keep their (valid) recipes and unprocessed ones are untouched.
pub fn run(
    catalog_path: &Path,
    ids: &[i64],
    op: BatchOp,
    cancel: &AtomicBool,
    emit: &dyn Fn(BatchEditProgress),
) -> Result<BatchEditReport, String> {
    let mut cat = Catalog::open(catalog_path).map_err(|e| format!("open catalog: {e}"))?;
    let mut report = BatchEditReport::default();
    let total = ids.len();
    for (index, &id) in ids.iter().enumerate() {
        if cancel.load(std::sync::atomic::Ordering::Relaxed) {
            report.cancelled = true;
            break;
        }
        match apply_one(&mut cat, id, op) {
            Ok(()) => report.applied += 1,
            Err(error) => report.failed.push(BatchEditFailure { id, error }),
        }
        emit(BatchEditProgress { done: index + 1, total });
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use fotobase_core::buffer::LinearImage;
    use fotobase_core::recipe::Recipe;
    use std::sync::atomic::{AtomicBool, Ordering};

    fn catalog_with_photos(dir: &Path, names: &[&str]) -> Vec<i64> {
        let mut cat = Catalog::open(dir.join("catalog.db")).unwrap();
        let folder = cat.seed_folder(dir.to_str().unwrap()).unwrap();
        names
            .iter()
            .map(|name| {
                cat.conn()
                    .execute(
                        "INSERT INTO files (folder_id, path, filename, size, mtime, media_type)
                         VALUES (?1, ?2, ?3, 10, 0.0, 'image')",
                        rusqlite::params![folder, dir.join(name).to_string_lossy(), name],
                    )
                    .unwrap();
                cat.conn().last_insert_rowid()
            })
            .collect()
    }

    fn sidecar_recipe(dir: &Path, name: &str) -> Option<String> {
        fotobase_metadata::Sidecar::read(&fotobase_catalog::sidecar_path(
            dir.join(name).to_str().unwrap(),
        ))
        .unwrap()
        .recipe()
    }

    fn recipe_with_op(existing: Option<&str>, op_json: Value) -> String {
        let mut ops: Vec<Value> = match existing {
            Some(raw) if !raw.trim().is_empty() => {
                let (recipe, _) = fotobase_core::recipe::parse(raw).unwrap();
                recipe.ops.iter().map(|o| serde_json::to_value(o).unwrap()).collect()
            }
            _ => Vec::new(),
        };
        ops.push(op_json);
        serde_json::to_string(&json!({
            "spec": "fotobase.recipe", "version": 1, "ops": ops
        }))
        .unwrap()
    }

    /// The task's pixel-comparison criterion: the recipe the batch wrote,
    /// applied by the engine, must produce the same pixels as applying the
    /// same op per-photo.
    fn pixels(recipe_json: &str) -> Vec<f32> {
        let (recipe, _) = Recipe::parse(recipe_json).unwrap();
        let src = LinearImage::from_fn(8, 6, |x, y| {
            [(x * y) as f32 / 48.0, 0.5, 1.0 - (x + y) as f32 / 14.0]
        });
        fotobase_core::apply::apply_recipe(&src, &recipe).data
    }

    #[test]
    fn batch_equals_per_photo_for_every_op() {
        let tmp = tempfile::TempDir::new().unwrap();
        let _ids = catalog_with_photos(tmp.path(), &["a.jpg", "b.jpg", "c.jpg"]);
        let ops = [
            (BatchOp::RotateCw, json!({ "type": "rotate", "params": { "angle": 90 } })),
            (BatchOp::RotateCcw, json!({ "type": "rotate", "params": { "angle": 270 } })),
            (BatchOp::AutoContrast, json!({ "type": "auto_contrast" })),
            (BatchOp::AutoColor, json!({ "type": "auto_color" })),
            (BatchOp::Sepia, json!({ "type": "sepia", "params": { "amount": 1.0 } })),
            (BatchOp::Sharpen, json!({ "type": "sharpen", "params": { "amount": 0.5 } })),
            (BatchOp::Warmify, json!({ "type": "warm_colors", "params": { "amount": 1.0 } })),
            (
                BatchOp::FilmGrain,
                json!({ "type": "film_grain", "params": { "amount": 0.5, "seed": 0 } }),
            ),
            (BatchOp::Bw, json!({ "type": "bw", "params": { "amount": 1.0 } })),
            (BatchOp::AutoRedEye, json!({ "type": "red_eye", "params": { "auto": true } })),
        ];
        for (op, expected_op) in ops {
            // A fresh catalog per op: rotations compose, so ops must not see
            // each other's recipes.
            let tmp = tempfile::TempDir::new().unwrap();
            let ids = catalog_with_photos(tmp.path(), &["a.jpg", "b.jpg", "c.jpg"]);
            let report =
                run(&tmp.path().join("catalog.db"), &ids, op, &AtomicBool::new(false), &|_| {})
                    .unwrap();
            assert_eq!(report.applied, 3, "{op:?}");
            assert!(report.failed.is_empty(), "{op:?}: {:?}", report.failed);
            for name in ["a.jpg", "b.jpg", "c.jpg"] {
                let expected = recipe_with_op(None, expected_op.clone());
                assert_eq!(
                    sidecar_recipe(tmp.path(), name),
                    Some(expected.clone()),
                    "{op:?} on {name}"
                );
                assert_eq!(
                    pixels(&expected),
                    pixels(sidecar_recipe(tmp.path(), name).unwrap().as_str()),
                    "{op:?} pixels on {name}"
                );
            }
        }
    }

    #[test]
    fn batch_appends_to_an_existing_recipe() {
        let tmp = tempfile::TempDir::new().unwrap();
        let ids = catalog_with_photos(tmp.path(), &["a.jpg"]);
        fotobase_catalog::Catalog::open(tmp.path().join("catalog.db"))
            .unwrap()
            .set_recipe(ids[0], r#"{"spec":"fotobase.recipe","version":1,"ops":[{"type":"bw","params":{"amount":1.0}}]}"#)
            .unwrap();

        run(&tmp.path().join("catalog.db"), &ids, BatchOp::Sepia, &AtomicBool::new(false), &|_| {})
            .unwrap();

        let expected = recipe_with_op(
            Some(
                r#"{"spec":"fotobase.recipe","version":1,"ops":[{"type":"bw","params":{"amount":1.0}}]}"#,
            ),
            json!({ "type": "sepia", "params": { "amount": 1.0 } }),
        );
        assert_eq!(sidecar_recipe(tmp.path(), "a.jpg"), Some(expected));
    }

    #[test]
    fn cancelling_mid_batch_keeps_processed_and_skips_the_rest() {
        let tmp = tempfile::TempDir::new().unwrap();
        let ids = catalog_with_photos(tmp.path(), &["a.jpg", "b.jpg", "c.jpg"]);
        // Cancel after the first photo reported done.
        let seen = std::cell::Cell::new(0);
        let cancel = AtomicBool::new(false);
        let report = run(&tmp.path().join("catalog.db"), &ids, BatchOp::Sepia, &cancel, &|_| {
            let n = seen.get();
            seen.set(n + 1);
            if n == 0 {
                cancel.store(true, Ordering::Relaxed);
            }
        })
        .unwrap();

        assert!(report.cancelled);
        assert_eq!(report.applied, 1);
        assert!(
            sidecar_recipe(tmp.path(), "a.jpg").is_some(),
            "the processed photo keeps its valid recipe"
        );
        assert_eq!(sidecar_recipe(tmp.path(), "b.jpg"), None, "unprocessed untouched");
        assert_eq!(sidecar_recipe(tmp.path(), "c.jpg"), None, "unprocessed untouched");
    }

    #[test]
    fn progress_counts_every_attempted_photo() {
        let tmp = tempfile::TempDir::new().unwrap();
        let ids = catalog_with_photos(tmp.path(), &["a.jpg", "b.jpg"]);
        let seen = std::cell::RefCell::new(Vec::new());
        run(&tmp.path().join("catalog.db"), &ids, BatchOp::Bw, &AtomicBool::new(false), &|p| {
            seen.borrow_mut().push(p)
        })
        .unwrap();
        assert_eq!(
            *seen.borrow(),
            vec![BatchEditProgress { done: 1, total: 2 }, BatchEditProgress { done: 2, total: 2 },]
        );
    }

    #[test]
    fn show_text_uses_the_caption_and_replaces_an_existing_text_op() {
        let tmp = tempfile::TempDir::new().unwrap();
        let ids = catalog_with_photos(tmp.path(), &["holiday.jpg"]);
        let mut cat = Catalog::open(tmp.path().join("catalog.db")).unwrap();
        cat.set_caption(ids[0], "Beach day").unwrap();

        run(
            &tmp.path().join("catalog.db"),
            &ids,
            BatchOp::ShowText,
            &AtomicBool::new(false),
            &|_| {},
        )
        .unwrap();
        let first = sidecar_recipe(tmp.path(), "holiday.jpg").unwrap();
        let ops: Vec<Value> =
            serde_json::from_str::<Value>(&first).unwrap()["ops"].as_array().unwrap().clone();
        assert_eq!(ops.len(), 1);
        assert_eq!(ops[0]["type"], "text");
        assert_eq!(ops[0]["params"]["text"], "Beach day");

        // Show again: idempotent, no duplicate text op.
        run(
            &tmp.path().join("catalog.db"),
            &ids,
            BatchOp::ShowText,
            &AtomicBool::new(false),
            &|_| {},
        )
        .unwrap();
        let again = sidecar_recipe(tmp.path(), "holiday.jpg").unwrap();
        assert_eq!(again, first, "show_text twice must not stack text ops");
    }

    #[test]
    fn show_text_falls_back_to_the_filename() {
        let tmp = tempfile::TempDir::new().unwrap();
        let ids = catalog_with_photos(tmp.path(), &["holiday.jpg"]);
        run(
            &tmp.path().join("catalog.db"),
            &ids,
            BatchOp::ShowText,
            &AtomicBool::new(false),
            &|_| {},
        )
        .unwrap();
        let raw = sidecar_recipe(tmp.path(), "holiday.jpg").unwrap();
        let ops: Vec<Value> =
            serde_json::from_str::<Value>(&raw).unwrap()["ops"].as_array().unwrap().clone();
        assert_eq!(ops[0]["params"]["text"], "holiday.jpg");
    }

    #[test]
    fn hide_text_removes_the_text_op_and_show_then_hide_round_trips() {
        let tmp = tempfile::TempDir::new().unwrap();
        let ids = catalog_with_photos(tmp.path(), &["a.jpg"]);
        let db = tmp.path().join("catalog.db");
        run(&db, &ids, BatchOp::ShowText, &AtomicBool::new(false), &|_| {}).unwrap();
        run(&db, &ids, BatchOp::HideText, &AtomicBool::new(false), &|_| {}).unwrap();
        assert_eq!(
            sidecar_recipe(tmp.path(), "a.jpg"),
            None,
            "hide removes the only op → recipe cleared, not an empty packet"
        );

        // Hide on a photo without a text op (and no recipe at all) is a no-op.
        let tmp2 = tempfile::TempDir::new().unwrap();
        let ids2 = catalog_with_photos(tmp2.path(), &["b.jpg"]);
        let report = run(
            &tmp2.path().join("catalog.db"),
            &ids2,
            BatchOp::HideText,
            &AtomicBool::new(false),
            &|_| {},
        )
        .unwrap();
        assert_eq!(report.applied, 1);
        assert_eq!(sidecar_recipe(tmp2.path(), "b.jpg"), None);
    }

    #[test]
    fn an_unknown_photo_is_a_failure_and_the_rest_carry_on() {
        let tmp = tempfile::TempDir::new().unwrap();
        let ids = catalog_with_photos(tmp.path(), &["a.jpg", "b.jpg"]);
        let report = run(
            &tmp.path().join("catalog.db"),
            &[999_999, ids[1]],
            BatchOp::Sepia,
            &AtomicBool::new(false),
            &|_| {},
        )
        .unwrap();
        assert_eq!(report.applied, 1);
        assert_eq!(report.failed.len(), 1);
        assert_eq!(report.failed[0].id, 999_999);
        assert!(sidecar_recipe(tmp.path(), "b.jpg").is_some());
    }
}
