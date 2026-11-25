//! TASK-10-01-03 unit: CRDT merge matrix — concurrent star, tag, caption,
//! album and recipe edits from two replicas converge (PLAN §7 F1 layer 2).

use std::collections::BTreeSet;

use fotobase_sync::{MetadataDoc, PhotoMeta, SyncError};

const KEY: &str = "sha256:0f1e2d";

fn set(items: &[&str]) -> BTreeSet<String> {
    items.iter().map(|s| s.to_string()).collect()
}

/// Two replicas that already share `base` for `KEY`.
fn pair(base: &PhotoMeta) -> (MetadataDoc, MetadataDoc) {
    let mut a = MetadataDoc::with_peer(1);
    a.update_photo(KEY, base).unwrap();
    let mut b = MetadataDoc::with_peer(2);
    b.import(&a.export_snapshot().unwrap()).unwrap();
    assert_eq!(b.photo(KEY).as_ref(), Some(base));
    (a, b)
}

/// Exchange full state both ways; return the converged photo.
fn exchange(a: &mut MetadataDoc, b: &mut MetadataDoc) -> PhotoMeta {
    let from_a = a.export_snapshot().unwrap();
    let from_b = b.export_snapshot().unwrap();
    a.import(&from_b).unwrap();
    b.import(&from_a).unwrap();
    let merged = a.photo(KEY).unwrap();
    assert_eq!(b.photo(KEY).unwrap(), merged, "replicas diverged");
    merged
}

fn edit(doc: &mut MetadataDoc, f: impl FnOnce(&mut PhotoMeta)) {
    let mut meta = doc.photo(KEY).unwrap_or_default();
    f(&mut meta);
    assert!(doc.update_photo(KEY, &meta).unwrap(), "edit recorded");
}

#[test]
fn different_fields_of_the_same_photo_both_survive() {
    let (mut a, mut b) = pair(&PhotoMeta { tags: set(&["trip"]), ..Default::default() });
    edit(&mut a, |m| m.rating = Some(5));
    edit(&mut b, |m| m.caption = Some("Beach at dusk".into()));
    edit(&mut b, |m| m.recipe = Some(r#"{"version":1,"ops":[]}"#.into()));

    let merged = exchange(&mut a, &mut b);
    assert_eq!(merged.rating, Some(5));
    assert_eq!(merged.caption.as_deref(), Some("Beach at dusk"));
    assert_eq!(merged.recipe.as_deref(), Some(r#"{"version":1,"ops":[]}"#));
    assert_eq!(merged.tags, set(&["trip"]));
}

#[test]
fn first_touch_on_two_devices_merges_instead_of_racing() {
    // Neither replica has seen the photo before: both create its state.
    let mut a = MetadataDoc::with_peer(1);
    let mut b = MetadataDoc::with_peer(2);
    edit(&mut a, |m| m.tags = set(&["dog"]));
    edit(&mut b, |m| {
        m.rating = Some(4);
        m.tags = set(&["beach"]);
    });

    let merged = exchange(&mut a, &mut b);
    assert_eq!(merged.rating, Some(4));
    assert_eq!(merged.tags, set(&["beach", "dog"]));
    assert_eq!(a.photos(), vec![KEY.to_string()]);
}

#[test]
fn concurrent_star_edits_converge_to_one_of_them() {
    let (mut a, mut b) = pair(&PhotoMeta { rating: Some(1), ..Default::default() });
    edit(&mut a, |m| m.rating = Some(3));
    edit(&mut b, |m| m.rating = Some(5));
    let merged = exchange(&mut a, &mut b);
    assert!(matches!(merged.rating, Some(3) | Some(5)), "{merged:?}");
}

#[test]
fn concurrent_caption_and_recipe_edits_converge() {
    let (mut a, mut b) = pair(&PhotoMeta::default());
    edit(&mut a, |m| {
        m.caption = Some("from A".into());
        m.recipe = Some(r#"{"a":1}"#.into());
    });
    edit(&mut b, |m| {
        m.caption = Some("from B".into());
        m.recipe = Some(r#"{"b":1}"#.into());
    });
    let merged = exchange(&mut a, &mut b);
    assert!(matches!(merged.caption.as_deref(), Some("from A" | "from B")));
    assert!(matches!(merged.recipe.as_deref(), Some(r#"{"a":1}"# | r#"{"b":1}"#)));
}

#[test]
fn concurrent_merges_are_deterministic_whatever_the_import_order() {
    let run = |a_first: bool| {
        let (mut a, mut b) = pair(&PhotoMeta::default());
        edit(&mut a, |m| m.rating = Some(2));
        edit(&mut b, |m| m.rating = Some(4));
        let mut c = MetadataDoc::with_peer(3);
        let (x, y) = (a.export_snapshot().unwrap(), b.export_snapshot().unwrap());
        if a_first {
            c.import(&x).unwrap();
            c.import(&y).unwrap();
        } else {
            c.import(&y).unwrap();
            c.import(&x).unwrap();
        }
        c.photo(KEY).unwrap()
    };
    assert_eq!(run(true), run(false));
}

#[test]
fn concurrent_tag_and_album_additions_all_survive() {
    let (mut a, mut b) =
        pair(&PhotoMeta { tags: set(&["base"]), albums: set(&["album-0"]), ..Default::default() });
    edit(&mut a, |m| {
        m.tags.insert("dog".into());
        m.albums.insert("album-a".into());
    });
    edit(&mut b, |m| {
        m.tags.insert("beach".into());
        m.albums.insert("album-b".into());
    });
    let merged = exchange(&mut a, &mut b);
    assert_eq!(merged.tags, set(&["base", "beach", "dog"]));
    assert_eq!(merged.albums, set(&["album-0", "album-a", "album-b"]));
}

#[test]
fn a_removal_on_one_device_and_an_addition_on_the_other_both_apply() {
    let (mut a, mut b) = pair(&PhotoMeta { tags: set(&["old", "keep"]), ..Default::default() });
    edit(&mut a, |m| {
        m.tags.remove("old");
    });
    edit(&mut b, |m| {
        m.tags.insert("new".into());
    });
    assert_eq!(exchange(&mut a, &mut b).tags, set(&["keep", "new"]));
}

#[test]
fn clearing_a_field_syncs() {
    let (mut a, mut b) = pair(&PhotoMeta {
        rating: Some(4),
        caption: Some("x".into()),
        recipe: Some("{}".into()),
        ..Default::default()
    });
    edit(&mut a, |m| {
        m.rating = None;
        m.caption = None;
        m.recipe = None;
    });
    let merged = exchange(&mut a, &mut b);
    assert_eq!(merged, PhotoMeta::default());
}

#[test]
fn an_unchanged_update_records_nothing() {
    let base = PhotoMeta { rating: Some(3), tags: set(&["a"]), ..Default::default() };
    let (mut a, mut b) = pair(&base);
    // B re-reads its unchanged sidecar while A edits the rating: B's no-op
    // must not override A's edit.
    assert!(!b.update_photo(KEY, &base).unwrap());
    edit(&mut a, |m| m.rating = Some(5));
    assert_eq!(exchange(&mut a, &mut b).rating, Some(5));
}

#[test]
fn import_is_idempotent() {
    let (mut a, mut b) = pair(&PhotoMeta { rating: Some(2), ..Default::default() });
    let snap = a.export_snapshot().unwrap();
    b.import(&snap).unwrap();
    b.import(&snap).unwrap();
    assert_eq!(exchange(&mut a, &mut b).rating, Some(2));
}

#[test]
fn garbage_and_bad_keys_are_errors_not_panics() {
    let mut doc = MetadataDoc::with_peer(1);
    assert!(matches!(doc.import(b"not a loro blob"), Err(SyncError::Import(_))));
    assert!(matches!(doc.update_photo("", &PhotoMeta::default()), Err(SyncError::InvalidKey(_))));
    assert!(doc.photo("never-seen").is_none());
}
