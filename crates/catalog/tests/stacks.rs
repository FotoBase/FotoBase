//! TASK-03-021 / ADR-0006: stacking. Membership lives in every member's
//! sidecar (`fb:StackId` + `fb:StackRole`); the catalog only mirrors it as a
//! derived index. Stacking is metadata-only, the grid collapses a stack to
//! its top, and unstacking is lossless.

use std::path::{Path, PathBuf};

use fotobase_catalog::{
    sidecar_path, Catalog, FileQuery, StackKind, SyncError, DEFAULT_PAIR_WINDOW_SECS,
};
use fotobase_metadata::{Sidecar, StackRole, FB_NS};

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
    fn photo(&mut self, name: &str, taken_at: Option<&str>) -> i64 {
        self.photo_in("", name, taken_at)
    }

    fn photo_in(&mut self, sub: &str, name: &str, taken_at: Option<&str>) -> i64 {
        let folder = self.dir.path().join(sub);
        std::fs::create_dir_all(&folder).unwrap();
        let original = folder.join(name);
        std::fs::write(&original, b"fake image bytes").unwrap();
        let folder_id = self.cat.seed_folder(folder.to_str().unwrap()).unwrap();
        self.cat
            .conn()
            .execute(
                "INSERT INTO files (folder_id, path, filename, size, mtime, taken_at, media_type)
                 VALUES (?1, ?2, ?3, 10, 0.0, ?4, 'image')",
                rusqlite::params![folder_id, original.to_str().unwrap(), name, taken_at],
            )
            .unwrap();
        self.cat.conn().last_insert_rowid()
    }

    fn path(&self, name: &str) -> PathBuf {
        self.dir.path().join(name)
    }

    fn sidecar(&self, name: &str) -> Sidecar {
        Sidecar::read(&sidecar_path(self.path(name).to_str().unwrap())).unwrap()
    }

    fn shown(&self) -> Vec<String> {
        self.cat
            .query_files(&FileQuery::default(), 100)
            .unwrap()
            .into_iter()
            .map(|f| f.filename)
            .collect()
    }
}

fn write_recipe(dir: &Path, name: &str) {
    let path = sidecar_path(dir.join(name).to_str().unwrap());
    std::fs::write(
        &path,
        format!(
            r#"<?xml version="1.0"?>
<x:xmpmeta xmlns:x="adobe:ns:meta/">
 <rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
  <rdf:Description rdf:about="" xmlns:fb="{FB_NS}"><fb:Recipe>{{"version":1,"ops":[]}}</fb:Recipe></rdf:Description>
 </rdf:RDF>
</x:xmpmeta>"#
        ),
    )
    .unwrap();
}

// ---- stacking ------------------------------------------------------------

#[test]
fn stacking_writes_every_members_sidecar_with_one_id_and_one_top() {
    let mut l = lib();
    let a = l.photo("a.jpg", None);
    let b = l.photo("a.cr2", None);

    let stack_id = l.cat.stack_photos(&[a, b]).unwrap();

    let (sa, sb) = (l.sidecar("a.jpg").stack().unwrap(), l.sidecar("a.cr2").stack().unwrap());
    assert_eq!((sa.id.as_str(), sb.id.as_str()), (stack_id.as_str(), stack_id.as_str()));
    assert_eq!((sa.role, sb.role), (StackRole::Top, StackRole::Member));
}

#[test]
fn the_grid_shows_one_thumbnail_per_stack_with_its_size() {
    let mut l = lib();
    let a = l.photo("a.jpg", None);
    let b = l.photo("a.cr2", None);
    l.photo("solo.jpg", None);
    let stack_id = l.cat.stack_photos(&[a, b]).unwrap();

    let rows = l.cat.query_files(&FileQuery::default(), 100).unwrap();

    let names: Vec<_> = rows.iter().map(|f| f.filename.as_str()).collect();
    assert_eq!(names, ["a.jpg", "solo.jpg"]);
    assert_eq!(rows[0].stack_id.as_deref(), Some(stack_id.as_str()));
    assert_eq!(rows[0].stack_count, 2);
    assert_eq!((rows[1].stack_id.as_deref(), rows[1].stack_count), (None, 0));
}

#[test]
fn a_stack_is_a_query_dimension_the_picker_expands_top_first() {
    let mut l = lib();
    let a = l.photo("a.cr2", None);
    let b = l.photo("a.jpg", None);
    let c = l.photo("a-edit.jpg", None);
    let stack_id = l.cat.stack_photos(&[b, a, c]).unwrap();

    let expanded = l
        .cat
        .query_files(&FileQuery { stack: Some(stack_id.clone()), ..FileQuery::default() }, 100)
        .unwrap();

    let names: Vec<_> = expanded.iter().map(|f| f.filename.as_str()).collect();
    assert_eq!(names, ["a.jpg", "a-edit.jpg", "a.cr2"], "top first, then by name");
    assert_eq!(l.cat.stack_members(&stack_id).unwrap().len(), 3);
}

#[test]
fn switching_the_top_swaps_the_roles_in_both_sidecars() {
    let mut l = lib();
    let a = l.photo("a.jpg", None);
    let b = l.photo("a.cr2", None);
    l.cat.stack_photos(&[a, b]).unwrap();

    l.cat.set_stack_top(b).unwrap();

    assert_eq!(l.sidecar("a.jpg").stack().unwrap().role, StackRole::Member);
    assert_eq!(l.sidecar("a.cr2").stack().unwrap().role, StackRole::Top);
    assert_eq!(l.shown(), ["a.cr2"]);
}

#[test]
fn unstacking_is_lossless_both_photos_come_back_and_nothing_else_changes() {
    let mut l = lib();
    let a = l.photo("a.jpg", None);
    let b = l.photo("a.cr2", None);
    l.cat.set_rating(a, 4).unwrap();
    let stack_id = l.cat.stack_photos(&[a, b]).unwrap();
    assert_eq!(l.shown(), ["a.jpg"]);

    l.cat.unstack(&stack_id).unwrap();

    let mut shown = l.shown();
    shown.sort();
    assert_eq!(shown, ["a.cr2", "a.jpg"]);
    assert_eq!(l.sidecar("a.jpg").stack(), None);
    assert_eq!(l.sidecar("a.cr2").stack(), None);
    assert!(!std::fs::read_to_string(sidecar_path(l.path("a.jpg").to_str().unwrap()))
        .unwrap()
        .contains("fb:Stack"));
    assert_eq!(l.sidecar("a.jpg").rating(), Some(4), "other user data survives");
}

#[test]
fn stacking_a_new_photo_with_a_stacked_one_joins_that_stack() {
    let mut l = lib();
    let a = l.photo("a.jpg", None);
    let b = l.photo("a.cr2", None);
    let c = l.photo("a-edit.jpg", None);
    let stack_id = l.cat.stack_photos(&[a, b]).unwrap();

    let joined = l.cat.stack_photos(&[c, a]).unwrap();

    assert_eq!(joined, stack_id);
    assert_eq!(l.sidecar("a-edit.jpg").stack().unwrap().role, StackRole::Member);
    assert_eq!(l.sidecar("a.jpg").stack().unwrap().role, StackRole::Top, "top stays");
    assert_eq!(l.cat.stack_members(&stack_id).unwrap().len(), 3);
}

#[test]
fn photos_from_two_different_stacks_are_not_merged() {
    let mut l = lib();
    let ids: Vec<i64> = ["a.jpg", "b.jpg", "c.jpg", "d.jpg"].map(|n| l.photo(n, None)).to_vec();
    l.cat.stack_photos(&ids[..2]).unwrap();
    l.cat.stack_photos(&ids[2..]).unwrap();

    let err = l.cat.stack_photos(&[ids[0], ids[2]]).unwrap_err();

    assert!(matches!(err, SyncError::StackConflict), "{err:?}");
}

#[test]
fn a_stack_needs_two_distinct_known_photos() {
    let mut l = lib();
    let a = l.photo("a.jpg", None);

    assert!(matches!(l.cat.stack_photos(&[a]).unwrap_err(), SyncError::StackTooSmall));
    assert!(matches!(l.cat.stack_photos(&[a, a]).unwrap_err(), SyncError::StackTooSmall));
    assert!(matches!(l.cat.stack_photos(&[a, 9999]).unwrap_err(), SyncError::UnknownFile(9999)));
    assert_eq!(l.sidecar("a.jpg").stack(), None);
}

#[test]
fn a_failed_sidecar_write_leaves_no_half_built_stack() {
    let mut l = lib();
    let a = l.photo("a.jpg", None);
    let b = l.photo("b.jpg", None);
    // A directory where b's sidecar should be: reading it fails.
    std::fs::create_dir(sidecar_path(l.path("b.jpg").to_str().unwrap())).unwrap();

    assert!(l.cat.stack_photos(&[a, b]).is_err());

    assert_eq!(l.sidecar("a.jpg").stack(), None, "a's sidecar rolled back");
    let mut shown = l.shown();
    shown.sort();
    assert_eq!(shown, ["a.jpg", "b.jpg"]);
}

#[test]
fn a_stack_whose_top_vanished_still_shows_its_remaining_members() {
    let mut l = lib();
    let a = l.photo("a.jpg", None);
    let b = l.photo("a.cr2", None);
    l.cat.stack_photos(&[a, b]).unwrap();

    l.cat.remove_file(a).unwrap();

    assert_eq!(l.shown(), ["a.cr2"]);
}

// ---- suggestions (pair detector) ----------------------------------------

#[test]
fn a_raw_jpeg_pair_with_the_same_stem_and_time_is_suggested_jpeg_on_top() {
    let mut l = lib();
    let raw = l.photo("IMG_0001.CR2", Some("2024-06-01T10:00:00"));
    let jpg = l.photo("IMG_0001.JPG", Some("2024-06-01T10:00:01"));

    let found = l.cat.stack_suggestions(DEFAULT_PAIR_WINDOW_SECS).unwrap();

    assert_eq!(found.len(), 1);
    assert_eq!(found[0].kind, StackKind::RawJpeg);
    assert_eq!(found[0].file_ids, [jpg, raw]);
    assert_eq!(found[0].filenames, ["IMG_0001.JPG", "IMG_0001.CR2"]);
}

#[test]
fn same_stem_but_a_different_timestamp_is_rejected() {
    let mut l = lib();
    l.photo("IMG_0001.CR2", Some("2024-06-01T10:00:00"));
    // The camera counter wrapped: same name, another day.
    l.photo("IMG_0001.JPG", Some("2025-01-15T18:30:00"));

    assert!(l.cat.stack_suggestions(DEFAULT_PAIR_WINDOW_SECS).unwrap().is_empty());
}

#[test]
fn the_window_is_a_calibration_knob() {
    let mut l = lib();
    l.photo("a.nef", Some("2024-06-01T10:00:00"));
    l.photo("a.jpg", Some("2024-06-01T10:00:30"));

    assert!(l.cat.stack_suggestions(5).unwrap().is_empty());
    assert_eq!(l.cat.stack_suggestions(60).unwrap().len(), 1);
}

#[test]
fn same_stem_in_different_folders_or_without_a_raw_is_not_a_pair() {
    let mut l = lib();
    l.photo_in("one", "a.cr2", Some("2024-06-01T10:00:00"));
    l.photo_in("two", "a.jpg", Some("2024-06-01T10:00:00"));
    l.photo("b.jpg", Some("2024-06-01T10:00:00"));
    l.photo("b.png", Some("2024-06-01T10:00:00"));

    assert!(l.cat.stack_suggestions(DEFAULT_PAIR_WINDOW_SECS).unwrap().is_empty());
}

#[test]
fn already_stacked_photos_are_not_suggested_again() {
    let mut l = lib();
    let raw = l.photo("a.cr2", Some("2024-06-01T10:00:00"));
    let jpg = l.photo("a.jpg", Some("2024-06-01T10:00:00"));
    l.cat.stack_photos(&[jpg, raw]).unwrap();

    assert!(l.cat.stack_suggestions(DEFAULT_PAIR_WINDOW_SECS).unwrap().is_empty());
}

#[test]
fn a_file_carrying_a_recipe_is_suggested_under_its_original() {
    let mut l = lib();
    let original = l.photo("beach.jpg", Some("2024-06-01T10:00:00"));
    let edited = l.photo("beach-edit.jpg", Some("2024-06-01T10:00:00"));
    l.photo("beach-2.jpg", Some("2024-06-01T10:00:00"));
    write_recipe(l.dir.path(), "beach-edit.jpg");
    l.cat.sync_file(edited).unwrap();

    let found = l.cat.stack_suggestions(DEFAULT_PAIR_WINDOW_SECS).unwrap();

    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0].kind, StackKind::Edited);
    assert_eq!(found[0].file_ids, [original, edited], "original stays on top");
}

#[test]
fn accepting_a_suggestion_is_just_stacking_its_files() {
    let mut l = lib();
    l.photo("IMG_0001.CR2", Some("2024-06-01T10:00:00"));
    l.photo("IMG_0001.JPG", Some("2024-06-01T10:00:00"));
    let found = l.cat.stack_suggestions(DEFAULT_PAIR_WINDOW_SECS).unwrap();

    l.cat.stack_photos(&found[0].file_ids).unwrap();

    assert_eq!(l.shown(), ["IMG_0001.JPG"]);
    assert!(l.cat.stack_suggestions(DEFAULT_PAIR_WINDOW_SECS).unwrap().is_empty());
}
