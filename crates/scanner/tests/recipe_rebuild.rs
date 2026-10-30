//! TASK-04-002 integration: an edit recipe lives in the sidecar
//! (`fb:Recipe`), so deleting the catalog and re-scanning restores it —
//! the catalog only mirrors `has_recipe` (PLAN §5 authority split).

use std::fs;
use std::sync::atomic::AtomicBool;

use fotobase_catalog::Catalog;
use fotobase_metadata::Sidecar;
use fotobase_scanner::scan_folder;

const RECIPE: &str =
    r#"{"spec":"fotobase.recipe","version":1,"ops":[{"type":"sepia","params":{"amount":0.5}}]}"#;

fn has_recipe(cat: &Catalog, id: i64) -> bool {
    cat.conn()
        .query_row("SELECT has_recipe FROM files WHERE id = ?1", [id], |r| r.get::<_, i64>(0))
        .unwrap()
        == 1
}

#[test]
fn a_recipe_survives_catalog_delete_and_rebuild_from_the_sidecar() {
    let lib = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    let jpg = lib.path().join("IMG_0001.jpg");
    fs::write(&jpg, vec![0xff; 64]).unwrap();

    // The user edits the photo: the recipe lands in the sidecar.
    let sidecar = lib.path().join("IMG_0001.jpg.xmp");
    let mut sc = Sidecar::read(&sidecar).unwrap();
    sc.set_recipe(RECIPE);
    sc.write(&sidecar).unwrap();

    let db = data.path().join("catalog.db");
    let mut cat = Catalog::open(&db).unwrap();
    scan_folder(&mut cat, lib.path(), &AtomicBool::new(false)).unwrap();
    assert!(
        cat.file_id_by_path(jpg.to_str().unwrap()).unwrap().is_some_and(|id| has_recipe(&cat, id)),
        "first scan mirrors has_recipe"
    );
    drop(cat);

    // Delete the catalog and rebuild: the recipe comes back from the sidecar.
    for ext in ["", "-wal", "-shm"] {
        let _ = fs::remove_file(format!("{}{ext}", db.display()));
    }
    let mut fresh = Catalog::open(&db).unwrap();
    scan_folder(&mut fresh, lib.path(), &AtomicBool::new(false)).unwrap();
    let id = fresh.file_id_by_path(jpg.to_str().unwrap()).unwrap().unwrap();
    assert!(has_recipe(&fresh, id), "recipe survived the rebuild");

    // The recipe itself is still readable from the sidecar, verbatim.
    assert_eq!(Sidecar::read(&sidecar).unwrap().recipe().as_deref(), Some(RECIPE));
}
