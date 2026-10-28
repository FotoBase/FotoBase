//! Batch rename / move commands' logic (TASK-03-011, PLAN §2 B). Planning
//! and file work live in `fotobase_scanner::batch`; these helpers add the
//! app-facing contract: previews never touch disk, a plan with collisions is
//! refused whole, and files only move into folders the library knows.

use fotobase_catalog::Catalog;
use fotobase_scanner::batch::{
    apply_batch, plan_move, plan_rename, BatchPlan, CollisionPolicy, PlannedMove, RenameSpec,
};

pub fn preview_rename(
    cat: &Catalog,
    ids: &[i64],
    spec: &RenameSpec,
    policy: CollisionPolicy,
) -> Result<BatchPlan, String> {
    let items = cat.batch_files(ids).map_err(|e| e.to_string())?;
    plan_rename(&items, spec, policy).map_err(|e| e.to_string())
}

pub fn rename(
    cat: &mut Catalog,
    ids: &[i64],
    spec: &RenameSpec,
    policy: CollisionPolicy,
) -> Result<Vec<PlannedMove>, String> {
    let plan = preview_rename(cat, ids, spec, policy)?;
    apply(cat, plan)
}

pub fn preview_move(
    cat: &Catalog,
    ids: &[i64],
    dest: &str,
    policy: CollisionPolicy,
) -> Result<BatchPlan, String> {
    ensure_cataloged_folder(cat, dest)?;
    let items = cat.batch_files(ids).map_err(|e| e.to_string())?;
    Ok(plan_move(&items, dest, policy))
}

pub fn move_photos(
    cat: &mut Catalog,
    ids: &[i64],
    dest: &str,
    policy: CollisionPolicy,
) -> Result<Vec<PlannedMove>, String> {
    let plan = preview_move(cat, ids, dest, policy)?;
    apply(cat, plan)
}

fn apply(cat: &mut Catalog, plan: BatchPlan) -> Result<Vec<PlannedMove>, String> {
    if !plan.is_applicable() {
        return Err(format!("{} name collision(s); nothing was changed", plan.collisions.len()));
    }
    apply_batch(cat, &plan.moves).map_err(|e| e.to_string())
}

fn ensure_cataloged_folder(cat: &Catalog, dest: &str) -> Result<(), String> {
    let known = cat.list_folders().map_err(|e| e.to_string())?;
    if known.iter().any(|f| f.path == dest) {
        Ok(())
    } else {
        Err(format!("{dest} is not in the library"))
    }
}
