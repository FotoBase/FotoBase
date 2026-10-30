//! TASK-04-003 unit tests: edit history = stack of recipe states in the
//! editor session (undo / redo / undo all / revert), per-photo.

use fotobase_core::history::{EditHistory, Sessions};
use fotobase_core::recipe::{parse, Recipe};

fn recipe(ops: &[&str]) -> Recipe {
    let ops: Vec<String> = ops.iter().map(|t| format!(r#"{{"type":"{t}"}}"#)).collect();
    let json = format!(r#"{{"spec":"fotobase.recipe","version":1,"ops":[{}]}}"#, ops.join(","));
    parse(&json).unwrap().0
}

#[test]
fn apply_three_undo_two_redo_one_leaves_the_expected_recipe() {
    let mut h = EditHistory::new(None);
    h.apply(&recipe(&["sepia"]));
    h.apply(&recipe(&["sepia", "grayscale"]));
    h.apply(&recipe(&["sepia", "grayscale", "sharpen"]));
    assert_eq!(h.current(), Some(&recipe(&["sepia", "grayscale", "sharpen"])));

    h.undo();
    h.undo();
    assert_eq!(h.current(), Some(&recipe(&["sepia"])));
    assert!(h.can_redo());

    h.redo();
    assert_eq!(h.current(), Some(&recipe(&["sepia", "grayscale"])));
    assert!(h.can_undo(), "undoing past the last edit returns to the saved state");
}

#[test]
fn undo_past_the_first_edit_returns_to_the_last_saved_state() {
    let mut h = EditHistory::new(None);
    h.apply(&recipe(&["sepia"]));
    h.apply(&recipe(&["sepia", "sharpen"]));
    h.undo();
    h.undo();
    assert_eq!(h.current(), None, "no saved recipe → undo all the way drops the recipe");
    assert!(!h.can_undo());
    h.redo();
    assert_eq!(h.current(), Some(&recipe(&["sepia"])));
}

#[test]
fn a_new_edit_drops_the_redo_branch() {
    let mut h = EditHistory::new(None);
    h.apply(&recipe(&["sepia"]));
    h.apply(&recipe(&["sepia", "sharpen"]));
    h.undo();
    h.apply(&recipe(&["sepia", "grayscale"]));
    assert!(!h.can_redo());
    assert_eq!(h.current(), Some(&recipe(&["sepia", "grayscale"])));
}

#[test]
fn undo_all_and_revert_restore_the_last_saved_recipe() {
    let saved = recipe(&["sepia"]);
    let mut h = EditHistory::new(Some(saved.clone()));
    h.apply(&recipe(&["sepia", "sharpen"]));
    h.apply(&recipe(&["sepia", "sharpen", "grayscale"]));

    h.undo_all();
    assert_eq!(h.current(), Some(&saved), "undo all clears pending edits back to the saved recipe");
    assert!(!h.can_redo(), "undo all is not redoable (menu semantics)");

    h.apply(&recipe(&["sepia", "warm_colors"]));
    h.revert();
    assert_eq!(h.current(), Some(&saved), "revert restores the last saved recipe at any time");
}

#[test]
fn revert_with_no_saved_recipe_drops_the_recipe_entirely() {
    let mut h = EditHistory::new(None);
    h.apply(&recipe(&["sepia"]));
    h.revert();
    assert_eq!(h.current(), None);
    assert!(!h.can_undo() && !h.can_redo());
}

#[test]
fn history_is_per_photo_and_survives_switching() {
    let mut s = Sessions::new();
    {
        let a = s.session("photo-a");
        a.apply(&recipe(&["sepia"]));
        a.apply(&recipe(&["sepia", "sharpen"]));
    }
    {
        let b = s.session("photo-b");
        b.apply(&recipe(&["grayscale"]));
    }
    // Switching back to photo-a preserves its own history (spec: per-photo).
    {
        let a = s.session("photo-a");
        a.undo();
        assert_eq!(a.current(), Some(&recipe(&["sepia"])));
    }
    {
        let b = s.session("photo-b");
        assert_eq!(b.current(), Some(&recipe(&["grayscale"])), "photo-b history untouched");
    }
}
