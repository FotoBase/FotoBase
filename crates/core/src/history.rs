//! Per-photo edit history (TASK-04-003, PLAN §2E "Undo/Redo/Undo All;
//! Revert", §6 P2b).
//!
//! History is a stack of recipe states held in the editor session
//! (session-scoped, never persisted); the persisted state is just the
//! current recipe in the sidecar (PLAN §5). `EditHistory` seeds from the
//! last saved recipe; `apply` pushes a state, `undo`/`redo` step through
//! the stack, `undo_all` and `revert` return to the last saved state —
//! `revert` with no saved recipe drops the recipe entirely (Revert =
//! restore original). None of these ever touch the original file (§9).

use crate::recipe::Recipe;

/// An editor session's history for one photo.
#[derive(Clone, Debug, Default)]
pub struct EditHistory {
    /// The recipe as last persisted (sidecar) when the session started, or
    /// the last explicit save. `None` = the photo is unedited.
    saved: Option<Recipe>,
    /// States before the current one (for undo). The bottom entry is the
    /// saved state, kept implicitly via `saved`.
    undo: Vec<Recipe>,
    /// States undone (for redo), dropped by any new edit.
    redo: Vec<Recipe>,
    current: Option<Recipe>,
}

impl EditHistory {
    /// Start a session for a photo whose persisted recipe is `saved`.
    pub fn new(saved: Option<Recipe>) -> Self {
        EditHistory { saved: saved.clone(), undo: Vec::new(), redo: Vec::new(), current: saved }
    }

    /// The current recipe state — what the editor shows and what a save
    /// would persist. `None` = original (no recipe).
    pub fn current(&self) -> Option<&Recipe> {
        self.current.as_ref()
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty() || self.current != self.saved
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    /// Apply an edit: push the new state, drop the redo branch.
    pub fn apply(&mut self, recipe: &Recipe) {
        if let Some(prev) = self.current.clone() {
            self.undo.push(prev);
        }
        self.current = Some(recipe.clone());
        self.redo.clear();
    }

    /// Step back one edit. Undoing past the first edit returns to the last
    /// saved state (and stays there: `can_undo` is false only when already
    /// at the saved state). Undoing never touches the original file.
    pub fn undo(&mut self) {
        if let Some(prev) = self.undo.pop() {
            if let Some(cur) = self.current.take() {
                self.redo.push(cur);
            }
            self.current = Some(prev);
        } else if self.current != self.saved {
            if let Some(cur) = self.current.take() {
                self.redo.push(cur);
            }
            self.current = self.saved.clone();
        }
    }

    /// Re-apply the most recently undone edit.
    pub fn redo(&mut self) {
        if let Some(next) = self.redo.pop() {
            if let Some(cur) = self.current.clone() {
                self.undo.push(cur);
            }
            self.current = Some(next);
        }
    }

    /// "Undo all edits" (§2 Picture context menu): clear the pending edits,
    /// back to the last saved recipe. Not redoable (menu semantics).
    pub fn undo_all(&mut self) {
        self.undo.clear();
        self.redo.clear();
        self.current = self.saved.clone();
    }

    /// Revert to original at any time: restore the last saved recipe, or
    /// drop the recipe entirely when the photo has none (Revert = drop
    /// recipe / restore last saved recipe).
    pub fn revert(&mut self) {
        self.undo_all();
    }
}

/// Histories for every photo opened this session. Keyed by photo path so
/// switching photos preserves each photo's own history.
#[derive(Debug, Default)]
pub struct Sessions {
    by_photo: std::collections::HashMap<String, EditHistory>,
}

impl Sessions {
    pub fn new() -> Self {
        Sessions { by_photo: std::collections::HashMap::new() }
    }

    /// The history for `photo`, created empty (unedited) on first touch.
    pub fn session(&mut self, photo: &str) -> &mut EditHistory {
        self.by_photo.entry(photo.to_string()).or_default()
    }

    /// Seed (or replace) the saved state for a photo — call when a photo is
    /// opened, with the recipe read from its sidecar.
    pub fn seed(&mut self, photo: &str, saved: Option<Recipe>) {
        self.by_photo.insert(photo.to_string(), EditHistory::new(saved));
    }
}
