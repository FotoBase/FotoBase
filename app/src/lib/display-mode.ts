// View-time display modes (TASK-03-018, PLAN §6 P2a "display modes
// (sepia/B&W view modes)"). Pure display state: the viewer maps a mode to a
// CSS filter on the stage. No edit recipe, no sidecar write, nothing
// persisted — so this module deliberately has no I/O and no recipe types.

export const DISPLAY_MODES = ["normal", "sepia", "bw"] as const;

export type DisplayMode = (typeof DISPLAY_MODES)[number];

/// A mode the user can switch on (everything but `normal`).
export type ActiveDisplayMode = Exclude<DisplayMode, "normal">;

/// Toggle button semantics: picking the active mode turns it off, picking
/// the other one switches straight over.
export function toggleDisplayMode(current: DisplayMode, picked: ActiveDisplayMode): DisplayMode {
  return current === picked ? "normal" : picked;
}

/// The CSS `filter` value for a mode (`none` = untouched colors).
export function displayFilter(mode: DisplayMode): string {
  switch (mode) {
    case "sepia":
      return "sepia(0.9)";
    case "bw":
      return "grayscale(1)";
    default:
      return "none";
  }
}
