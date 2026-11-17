//! TASK-07-006: burst grouping and AI culling in the catalog. Bursts cluster
//! by capture-time gaps with the same camera; the suggested best ranks by the
//! derived quality score. Stacking the user's pick goes through the EPIC-03
//! sidecar path, so an override survives a catalog rebuild (PLAN §5).

use std::collections::BTreeMap;

use fotobase_catalog::{group_bursts, BurstCandidate, BurstSuggestion, Catalog};

struct Lib {
    dir: tempfile::TempDir,
    cat: Catalog,
}

fn lib() -> Lib {
    let dir = tempfile::tempdir().unwrap();
    let cat = Catalog::open(dir.path().join("c.db")).unwrap();
    Lib { dir, cat }
}

impl Lib {
    fn photo(&mut self, name: &str, taken_at: Option<&str>, camera: Option<&str>) -> i64 {
        let original = self.dir.path().join(name);
        std::fs::write(&original, b"fake image bytes").unwrap();
        let folder_id = self.cat.seed_folder(self.dir.path().to_str().unwrap()).unwrap();
        self.cat
            .conn()
            .execute(
                "INSERT INTO files (folder_id, path, filename, size, mtime, taken_at, media_type)
                 VALUES (?1, ?2, ?3, 10, 0.0, ?4, 'image')",
                rusqlite::params![folder_id, original.to_str().unwrap(), name, taken_at],
            )
            .unwrap();
        let id = self.cat.conn().last_insert_rowid();
        if let Some(camera) = camera {
            self.cat
                .conn()
                .execute(
                    "INSERT INTO metadata (file_id, camera) VALUES (?1, ?2)",
                    rusqlite::params![id, camera],
                )
                .unwrap();
        }
        id
    }
}

// ---- grouping (pure) ------------------------------------------------------

fn cand(id: i64, captured: i64, camera: Option<&str>) -> BurstCandidate {
    BurstCandidate { id, captured, camera: camera.map(str::to_string) }
}

#[test]
fn a_continuous_run_clusters_into_one_group() {
    // 0 s, 1 s, 2 s apart: one burst. Then a 10 s hole: a separate photo.
    let groups = group_bursts(
        &[
            cand(4, 12, Some("Cam")),
            cand(1, 0, Some("Cam")),
            cand(2, 1, Some("Cam")),
            cand(3, 2, Some("Cam")),
        ],
        2,
        &BTreeMap::new(),
    );
    assert_eq!(groups.len(), 1);
    // Best-first with no scores: capture order, earliest first. The 12 s
    // outlier is a lone run, never a suggestion.
    assert_eq!(groups[0].file_ids, [1, 2, 3]);
}

#[test]
fn a_gap_above_the_threshold_splitss_runs() {
    let groups = group_bursts(
        &[
            cand(1, 0, Some("Cam")),
            cand(2, 1, Some("Cam")),
            cand(3, 2, Some("Cam")),
            cand(4, 5, Some("Cam")),
        ],
        2,
        &BTreeMap::new(),
    );
    // 0→1→2 is one run (each gap ≤ 2 s); 2→5 (3 s) splits; the tail run is a
    // lone photo, so it is not a suggestion.
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].file_ids, [1, 2, 3]);
    // A single-photo run never becomes a suggestion.
    let two =
        group_bursts(&[cand(1, 0, Some("Cam")), cand(2, 9, Some("Cam"))], 2, &BTreeMap::new());
    assert!(two.is_empty());
}

#[test]
fn different_cameras_never_share_a_burst() {
    let groups = group_bursts(
        &[
            cand(1, 0, Some("A")),
            cand(2, 1, Some("B")),
            cand(3, 2, Some("A")),
            cand(4, 3, Some("B")),
        ],
        2,
        &BTreeMap::new(),
    );
    assert_eq!(groups.len(), 2);
    assert_eq!(groups[0].file_ids, [1, 3]);
    assert_eq!(groups[1].file_ids, [2, 4]);
}

#[test]
fn the_sharpest_frame_ranks_first_and_is_the_suggested_best() {
    let scores = BTreeMap::from([(1, 0.9), (2, 50.0), (3, 0.1)]);
    let groups = group_bursts(
        &[cand(1, 0, Some("Cam")), cand(2, 1, Some("Cam")), cand(3, 2, Some("Cam"))],
        2,
        &scores,
    );
    assert_eq!(groups.len(), 1);
    let BurstSuggestion { file_ids, best_file_id, .. } = &groups[0];
    assert_eq!(*file_ids, vec![2, 1, 3]);
    assert_eq!(*best_file_id, 2);
}

#[test]
fn an_unscored_frame_ranks_below_every_scored_one() {
    let scores = BTreeMap::from([(2, 5.0)]);
    let groups = group_bursts(
        &[cand(1, 0, Some("Cam")), cand(2, 1, Some("Cam")), cand(3, 2, Some("Cam"))],
        2,
        &scores,
    );
    assert_eq!(groups[0].file_ids, [2, 1, 3]);
    assert_eq!(groups[0].best_file_id, 2);
}

#[test]
fn equal_scores_fall_back_to_capture_order() {
    let scores = BTreeMap::from([(3, 1.0), (1, 1.0)]);
    let groups = group_bursts(
        &[cand(1, 0, Some("Cam")), cand(2, 1, Some("Cam")), cand(3, 2, Some("Cam"))],
        2,
        &scores,
    );
    assert_eq!(groups[0].file_ids, [1, 3, 2]);
}

// ---- catalog surface ------------------------------------------------------

#[test]
fn burst_suggestions_rank_by_the_stored_quality_score() {
    let mut l = lib();
    let blurry = l.photo("b1.jpg", Some("2026-05-01T10:00:00"), Some("Canon"));
    let sharp = l.photo("b2.jpg", Some("2026-05-01T10:00:01"), Some("Canon"));
    let third = l.photo("b3.jpg", Some("2026-05-01T10:00:02"), Some("Canon"));
    l.photo("other.jpg", Some("2026-05-01T10:30:00"), Some("Canon"));
    l.photo("noise.jpg", Some("2026-05-01T10:00:00"), Some("Nikon"));

    for (id, score) in [(blurry, 1.0), (sharp, 9.0), (third, 3.0)] {
        l.cat.store_quality_score(id, score).unwrap();
    }

    let groups = l.cat.burst_suggestions(2).unwrap();
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].file_ids, [sharp, third, blurry]);
    assert_eq!(groups[0].best_file_id, sharp);
    assert_eq!(groups[0].filenames, ["b2.jpg", "b3.jpg", "b1.jpg"]);
}

#[test]
fn stacked_photos_are_no_longer_burst_candidates() {
    let mut l = lib();
    let a = l.photo("a.jpg", Some("2026-05-01T10:00:00"), Some("Canon"));
    let b = l.photo("b.jpg", Some("2026-05-01T10:00:01"), Some("Canon"));
    l.cat.store_quality_score(a, 1.0).unwrap();
    l.cat.store_quality_score(b, 2.0).unwrap();
    l.cat.stack_photos(&[a, b]).unwrap();

    assert!(l.cat.burst_suggestions(2).unwrap().is_empty());
}

#[test]
fn scores_are_derived_data_only_the_catalog_holds_them() {
    // The score table is derived: a user-data sidecar carries no quality
    // score, so a rebuild never re-derives a "pick" from it.
    let mut l = lib();
    let id = l.photo("a.jpg", Some("2026-05-01T10:00:00"), Some("Canon"));
    l.cat.store_quality_score(id, 7.5).unwrap();
    assert_eq!(l.cat.quality_score(id).unwrap(), Some(7.5));
    // Nothing was written next to the original: no sidecar file exists at
    // all, let alone one carrying a score.
    assert!(!l.dir.path().join("a.jpg.xmp").exists());
}

#[test]
fn the_users_pick_survives_a_catalog_rebuild() {
    // The user promotes the second shot: stacking with it first makes it the
    // top. A rebuild re-derives derived data (scores) from nothing user-owned
    // and never touches the sidecar stack membership — the pick stays.
    let mut l = lib();
    let first = l.photo("a.jpg", Some("2026-05-01T10:00:00"), Some("Canon"));
    let second = l.photo("b.jpg", Some("2026-05-01T10:00:01"), Some("Canon"));
    l.cat.store_quality_score(first, 9.0).unwrap();
    l.cat.store_quality_score(second, 1.0).unwrap();

    let groups = l.cat.burst_suggestions(2).unwrap();
    assert_eq!(groups[0].best_file_id, first, "the machine pick is the sharp frame");

    // The user overrides: promote `second` by stacking with it on top.
    l.cat.stack_photos(&[second, first]).unwrap();
    let stack_id = groups[0].file_ids.iter().find_map(|id| {
        l.cat
            .conn()
            .query_row("SELECT stack_id FROM files WHERE id = ?1", [id], |r| r.get::<_, String>(0))
            .ok()
    });
    let stack_id = stack_id.unwrap();
    assert_eq!(l.cat.stack_members(&stack_id).unwrap()[0].filename, "b.jpg");

    l.cat.rebuild_from_sidecars(true).unwrap();

    let members = l.cat.stack_members(&stack_id).unwrap();
    assert_eq!(members[0].filename, "b.jpg", "the user's pick is still the top");
    assert_eq!(members.len(), 2);
}
