//! TASK-03-026 integration: rebuild acceptance harness (PLAN §9
//! "Rebuildability" row). Snapshot user data → delete the catalog DB →
//! rebuild from sidecars → per-field semantic diff. Zero loss must pass;
//! a seeded regression (a dropped sidecar) must fail the harness.
//!
//! The harness drives the existing rebuild headless: `rebuild()` from
//! `fotobase_lib::rebuild`, no Tauri app handle involved. Originals and
//! sidecars are only ever read.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use fotobase_catalog::{Catalog, MetadataEdit};
use fotobase_lib::rebuild::{rebuild, watched_folders};
use fotobase_lib::rebuild_harness::{delete_catalog_db, restore_folders, snapshot, Snapshot};
use fotobase_scanner::scan_folder;

fn open_catalog(data: &Path) -> Catalog {
    Catalog::open(data.join("catalog.db")).expect("open catalog")
}

fn normalize(path: &Path) -> String {
    path.components().collect::<PathBuf>().to_string_lossy().into_owned()
}

/// A library of five photos across two folders; the fixture the harness
/// runs on. Bytes are placeholders — the harness reads sidecars, not pixels.
fn make_library(root: &Path) {
    fs::create_dir_all(root.join("2024")).unwrap();
    fs::create_dir_all(root.join("2025")).unwrap();
    for (rel, n) in [
        ("2024/a.jpg", 10),
        ("2024/b.png", 20),
        ("2024/c.jpg", 30),
        ("2025/d.jpg", 40),
        ("2025/e.jpg", 50),
    ] {
        fs::write(root.join(rel), vec![0xff; n]).unwrap();
    }
}

fn scan(lib: &Path, cat: &mut Catalog) {
    scan_folder(cat, lib, &std::sync::atomic::AtomicBool::new(false)).unwrap();
}

fn id_of(cat: &Catalog, lib: &Path, name: &str) -> i64 {
    cat.file_id_by_path(&normalize(&lib.join(name))).expect("by path").expect("file exists")
}

/// Seed user data through the single mutation pathway (sidecar first,
/// catalog mirror second), the way the app's edit commands do.
fn seed_user_data(lib: &Path, cat: &mut Catalog) {
    let a = id_of(cat, lib, "2024/a.jpg");
    cat.set_rating(a, 5).unwrap();
    cat.set_caption(a, "sunset over the bay").unwrap();
    cat.set_tags(a, vec!["family".into(), "beach".into()]).unwrap();
    cat.set_gps(a, 54.123_456, -4.654_321).unwrap();
    let errors = cat
        .edit_metadata(
            a,
            &MetadataEdit {
                taken_at: Some("2024-07-01T18:30:00+02:00".into()),
                gps: None,
                artist: Some("A. Photographer".into()),
                copyright: Some("© 2024 A. Photographer".into()),
            },
        )
        .unwrap();
    assert!(errors.is_empty(), "edit errors: {errors:?}");

    // b.png: lightly decorated, and stacked with d.jpg.
    let b = id_of(cat, lib, "2024/b.png");
    cat.set_rating(b, 2).unwrap();
    cat.set_tags(b, vec!["city".into()]).unwrap();

    // c.jpg: hidden via its sidecar (fb:Hidden), then converged.
    let c = id_of(cat, lib, "2024/c.jpg");
    cat.set_caption(c, "private").unwrap();
    let sc = fotobase_catalog::sidecar_path(&normalize(&lib.join("2024/c.jpg")));
    let mut side = fotobase_metadata::Sidecar::read(&sc).unwrap();
    side.set_hidden(true);
    side.write(&sc).unwrap();
    cat.sync_file(c).unwrap();

    // d.jpg + e.jpg: one stack, e on top.
    let d = id_of(cat, lib, "2025/d.jpg");
    let e = id_of(cat, lib, "2025/e.jpg");
    cat.set_rating(d, 3).unwrap();
    let stack_id = cat.stack_photos(&[d, e]).unwrap();
    cat.set_stack_top(e).unwrap();

    // Album with a full definition; a.jpg is a member.
    let album = cat
        .create_album("Trip 2024", "2024-07-01", Some("Isle of Man"), Some("the good days"))
        .unwrap()
        .id;
    cat.add_to_album(album, &[a, b]).unwrap();

    // An fb:Recipe marker on a raw sidecar: the edit-recipe engine writes
    // these; the harness must survive (and diff) them.
    let recipe_sc = fotobase_catalog::sidecar_path(&normalize(&lib.join("2025/e.jpg")));
    let raw = fs::read_to_string(&recipe_sc).unwrap();
    let marked =
        raw.replace("</rdf:Description>", "<fb:Recipe>{\"v\":1}</fb:Recipe></rdf:Description>");
    fs::write(&recipe_sc, marked).unwrap();
    cat.sync_file(e).unwrap();

    let _ = stack_id;
}

/// The sidecar of `rel` must exist after seeding: the mutations above write
/// through to disk, and the harness snapshots those bytes.
fn sidecar_of(lib: &Path, rel: &str) -> PathBuf {
    fotobase_catalog::sidecar_path(&normalize(&lib.join(rel)))
}

#[test]
fn full_cycle_delete_catalog_rebuild_reports_zero_loss() {
    let lib = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    make_library(lib.path());
    let mut cat = open_catalog(data.path());
    scan(lib.path(), &mut cat);
    seed_user_data(lib.path(), &mut cat);

    let before: Snapshot = snapshot(&cat).unwrap();
    assert!(before.files.contains_key(&normalize(&lib.path().join("2024/a.jpg"))));
    assert_eq!(before.files.len(), 5);
    assert_eq!(before.albums.len(), 1);
    assert!(before.sidecars.contains_key(&normalize(&sidecar_of(lib.path(), "2024/a.jpg"))));

    // Wipe the catalog: the rebuildable cache is gone (§9).
    drop(cat);
    delete_catalog_db(&data.path().join("catalog.db")).unwrap();
    assert!(!data.path().join("catalog.db").exists());

    // Rebuild headless: fresh catalog, watched folders restored from the
    // snapshot, then the existing rebuild command re-walks them in full
    // mode and converges every sidecar.
    let mut cat = open_catalog(data.path());
    restore_folders(&mut cat, &before).unwrap();
    let roots = watched_folders(&cat);
    assert_eq!(roots.len(), 1, "one top-level root: {roots:?}");
    let report = rebuild(&mut cat, &roots, &|_| {}).expect("rebuild succeeds");
    assert_eq!(report.added, 5, "every photo is re-upserted: {report:?}");

    let after: Snapshot = snapshot(&cat).unwrap();
    let diff = fotobase_lib::rebuild_harness::diff(&before, &after);
    assert!(diff.pass, "losses after rebuild: {:#?}", diff.diffs);

    // And the machine-readable report serializes to JSON.
    let json = serde_json::to_string_pretty(&diff).unwrap();
    assert!(json.contains("\"pass\": true"));
}

#[test]
fn harness_detects_a_seeded_sidecar_loss() {
    let lib = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    make_library(lib.path());
    let mut cat = open_catalog(data.path());
    scan(lib.path(), &mut cat);
    seed_user_data(lib.path(), &mut cat);

    let before: Snapshot = snapshot(&cat).unwrap();
    drop(cat);
    delete_catalog_db(&data.path().join("catalog.db")).unwrap();

    // Seeded regression: one mutation's sidecar write "never happened" —
    // the sidecar of a.jpg is gone. A rebuild cannot restore what was
    // never written; the harness must catch the loss.
    fs::remove_file(sidecar_of(lib.path(), "2024/a.jpg")).unwrap();

    let mut cat = open_catalog(data.path());
    restore_folders(&mut cat, &before).unwrap();
    let a_path = normalize(&lib.path().join("2024/a.jpg"));
    let roots = watched_folders(&cat);
    rebuild(&mut cat, &roots, &|_| {}).unwrap();

    let after = snapshot(&cat).unwrap();
    let diff = fotobase_lib::rebuild_harness::diff(&before, &after);
    assert!(!diff.pass, "the seeded regression must FAIL the harness");
    let lost_fields: Vec<&str> =
        diff.diffs.iter().filter(|d| d.path == a_path).map(|d| d.field.as_str()).collect();
    assert!(lost_fields.contains(&"rating"), "fields lost: {lost_fields:?}");
    assert!(lost_fields.contains(&"caption"), "fields lost: {lost_fields:?}");
    assert!(lost_fields.contains(&"tags"), "fields lost: {lost_fields:?}");
    // Sidecars that did survive are byte-identical after the rebuild.
    assert!(diff.diffs.iter().all(|d| d.path != normalize(&sidecar_of(lib.path(), "2024/b.png"))));
}

#[test]
fn sidecar_hashes_catch_rebuild_side_effects() {
    let lib = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    make_library(lib.path());
    let mut cat = open_catalog(data.path());
    scan(lib.path(), &mut cat);
    seed_user_data(lib.path(), &mut cat);

    let before = snapshot(&cat).unwrap();
    drop(cat);

    // A buggy rebuild that rewrote a sidecar (violating "originals and
    // sidecars are read-only here") is caught by the content hash even
    // when every catalog field happens to round-trip.
    let sc = sidecar_of(lib.path(), "2024/b.png");
    let raw = fs::read_to_string(&sc).unwrap();
    fs::write(&sc, raw.replace("city", "city!")).unwrap();

    delete_catalog_db(&data.path().join("catalog.db")).unwrap();

    let mut cat = open_catalog(data.path());
    restore_folders(&mut cat, &before).unwrap();
    let roots = watched_folders(&cat);
    rebuild(&mut cat, &roots, &|_| {}).unwrap();

    let after = snapshot(&cat).unwrap();
    let diff = fotobase_lib::rebuild_harness::diff(&before, &after);
    assert!(!diff.pass, "a mutated sidecar must fail the hash check");
    assert!(diff.diffs.iter().any(|d| d.path == normalize(&sc) && d.field == "sidecar_sha256"));
}

#[test]
fn incremental_rebuild_skips_unchanged_and_keeps_user_data() {
    let lib = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    make_library(lib.path());
    let mut cat = open_catalog(data.path());
    scan(lib.path(), &mut cat);
    seed_user_data(lib.path(), &mut cat);

    let before = snapshot(&cat).unwrap();

    // External drift: one sidecar changes (another tool, or our own write
    // path). The incremental rebuild (mtime+size skip, §5) converges only
    // that file and leaves the rest alone.
    let sc = sidecar_of(lib.path(), "2024/b.png");
    let mut side = fotobase_metadata::Sidecar::read(&sc).unwrap();
    side.set_rating(4);
    side.write(&sc).unwrap();

    let stats = cat.rebuild_from_sidecars(false).unwrap();
    assert_eq!(stats.converged, 1, "only the changed sidecar: {stats:?}");
    assert_eq!(stats.skipped, 4, "untouched sidecars are skipped");

    let after = snapshot(&cat).unwrap();
    // The external edit is *not* a loss: the snapshot is retaken semantics —
    // compare against a fresh pre-state that includes the drift.
    let mut expected = before.clone();
    if let Some(f) = expected.files.get_mut(&normalize(&lib.path().join("2024/b.png"))) {
        f.rating = Some(4);
    }
    // The drifted sidecar's hash moves with it (the external edit, not the
    // rebuild, changed the bytes).
    let sc_key = normalize(&sc);
    expected.sidecars.insert(
        sc_key,
        after.sidecars.iter().find(|(k, _)| k.ends_with("2024/b.png.xmp")).unwrap().1.clone(),
    );
    let diff = fotobase_lib::rebuild_harness::diff(&expected, &after);
    assert!(diff.pass, "incremental rebuild must not lose data: {:#?}", diff.diffs);
}

#[test]
fn external_fixture_round_trips_when_provided() {
    // Manual/perf-harness entry point (FEAT-01-05): point the env var at a
    // generated fixture library (`tools/fixture_gen.py --out <dir>`) and the
    // same full cycle runs on it — the 50k decision run stays out of CI.
    let Some(lib) = std::env::var_os("FOTOBASE_REBUILD_HARNESS_FIXTURE") else {
        eprintln!("skipped: FOTOBASE_REBUILD_HARNESS_FIXTURE not set");
        return;
    };
    let lib = PathBuf::from(lib);
    assert!(lib.is_dir(), "fixture library must exist: {}", lib.display());

    let data = tempfile::tempdir().unwrap();
    let mut cat = open_catalog(data.path());
    scan(&lib, &mut cat);

    // Seed whatever the fixture already carries in its sidecars: converge,
    // snapshot, wipe, rebuild, diff. Fixture sidecars with fb: user data
    // must round-trip untouched.
    cat.rebuild_from_sidecars(true).unwrap();
    let before = snapshot(&cat).unwrap();
    drop(cat);
    delete_catalog_db(&data.path().join("catalog.db")).unwrap();

    let mut cat = open_catalog(data.path());
    restore_folders(&mut cat, &before).unwrap();
    let roots = watched_folders(&cat);
    rebuild(&mut cat, &roots, &|_| {}).unwrap();
    let after = snapshot(&cat).unwrap();
    let diff = fotobase_lib::rebuild_harness::diff(&before, &after);
    assert!(diff.pass, "losses on the external fixture: {:#?}", diff.diffs);
    let _ = BTreeMap::<String, String>::new;
}

#[test]
fn recipe_and_hidden_markers_round_trip() {
    let lib = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    make_library(lib.path());
    let mut cat = open_catalog(data.path());
    scan(lib.path(), &mut cat);
    seed_user_data(lib.path(), &mut cat);

    // The raw fb:Recipe injection in the seeder must actually land.
    let e = id_of(&cat, lib.path(), "2025/e.jpg");
    let has_recipe: i64 = cat
        .conn()
        .query_row("SELECT has_recipe FROM files WHERE id = ?1", [e], |r| r.get(0))
        .unwrap();
    assert_eq!(has_recipe, 1, "seeded fb:Recipe marker must be mirrored");
    let c = id_of(&cat, lib.path(), "2024/c.jpg");
    let hidden: i64 =
        cat.conn().query_row("SELECT hidden FROM files WHERE id = ?1", [c], |r| r.get(0)).unwrap();
    assert_eq!(hidden, 1, "seeded fb:Hidden must be mirrored");
}
