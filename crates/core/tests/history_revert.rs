//! TASK-04-003 integration: revert after a series of edits leaves the
//! original byte-identical (PLAN §9 data safety) and restores the last
//! saved recipe in the sidecar.

use std::fs;
use std::path::Path;

use fotobase_core::history::EditHistory;
use fotobase_core::recipe::parse;
use fotobase_metadata::Sidecar;

fn recipe(ops: &[&str]) -> String {
    let ops: Vec<String> = ops.iter().map(|t| format!(r#"{{"type":"{t}"}}"#)).collect();
    format!(r#"{{"spec":"fotobase.recipe","version":1,"ops":[{}]}}"#, ops.join(","))
}

fn checksum(path: &Path) -> u64 {
    let bytes = fs::read(path).unwrap();
    bytes.iter().fold(0xcbf29ce484222325u64, |h, b| (h ^ (*b as u64)).wrapping_mul(0x100000001b3))
}

#[test]
fn revert_after_edits_restores_the_saved_recipe_and_leaves_the_original_untouched() {
    let dir = tempfile::tempdir().unwrap();
    let jpg = dir.path().join("IMG_0001.jpg");
    fs::write(&jpg, vec![0x42; 256]).unwrap();
    let before = checksum(&jpg);

    // The photo already has a saved recipe from an earlier session.
    let sidecar_path = dir.path().join("IMG_0001.jpg.xmp");
    let mut sc = Sidecar::read(&sidecar_path).unwrap();
    sc.set_recipe(&recipe(&["sepia"]));
    sc.write(&sidecar_path).unwrap();

    // Editor session: history seeds from the last saved recipe; each edit
    // pushes a state (and a real save would persist it to the sidecar).
    let saved = Sidecar::read(&sidecar_path).unwrap().recipe();
    let mut h = EditHistory::new(saved.as_deref().map(|j| parse(j).unwrap().0));
    for ops in [
        &["sepia", "sharpen"][..],
        &["sepia", "sharpen", "grayscale"][..],
        &["sepia", "warm_colors"][..],
    ] {
        h.apply(&parse(&recipe(ops)).unwrap().0);
    }

    // Revert: drop the pending edits back to the last saved recipe and
    // persist exactly that to the sidecar (sidecar-wins, 00-010 §3).
    h.revert();
    let restored = h.current().map(|r| r.to_json()).unwrap_or_default();
    let mut sc = Sidecar::read(&sidecar_path).unwrap();
    match h.current() {
        Some(_) => sc.set_recipe(&restored),
        None => sc.clear_recipe(),
    }
    sc.write(&sidecar_path).unwrap();

    let restored = Sidecar::read(&sidecar_path).unwrap().recipe().unwrap();
    assert_eq!(
        parse(&restored).unwrap().0,
        parse(&recipe(&["sepia"])).unwrap().0,
        "revert restores the last saved recipe"
    );
    assert_eq!(checksum(&jpg), before, "history ops never touch the original file");
}

#[test]
fn revert_with_no_saved_recipe_clears_the_recipe_from_the_sidecar() {
    let dir = tempfile::tempdir().unwrap();
    let jpg = dir.path().join("IMG_0002.jpg");
    fs::write(&jpg, vec![0x7f; 128]).unwrap();
    let before = checksum(&jpg);

    let sidecar_path = dir.path().join("IMG_0002.jpg.xmp");
    // The sidecar exists on disk: the earlier edits in this session saved.
    let mut seeded = Sidecar::read(&sidecar_path).unwrap();
    seeded.set_recipe(&recipe(&["sepia", "sharpen"]));
    seeded.write(&sidecar_path).unwrap();

    let mut h = EditHistory::new(None);
    h.apply(&parse(&recipe(&["sepia"])).unwrap().0);
    h.apply(&parse(&recipe(&["sepia", "sharpen"])).unwrap().0);

    h.revert();
    let mut sc = Sidecar::read(&sidecar_path).unwrap();
    match h.current() {
        Some(_) => sc.set_recipe(&h.current().unwrap().to_json()),
        None => sc.clear_recipe(),
    }
    sc.write(&sidecar_path).unwrap();

    assert!(
        Sidecar::read(&sidecar_path).unwrap().recipe().is_none(),
        "revert with no saved recipe drops the recipe entirely"
    );
    assert_eq!(checksum(&jpg), before);
}
