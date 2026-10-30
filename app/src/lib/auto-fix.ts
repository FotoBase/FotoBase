// Pure op emission for the Basic Fixes one-click auto fixes (TASK-04-007,
// PLAN §2E). The pixel work is done by the shared Rust engine — `auto_color`
// (gray-world white balance) and `auto_contrast` (1%–99% percentile stretch)
// — so the panel only records which passes are active. I'm Feeling Lucky is
// the combined pass: color first, then contrast.
//
// Order matters: the color pass normalizes the cast, the contrast pass then
// stretches the neutralized image (edit-recipe spec §2, ops run in order).

/// The auto-fix state the panel edits and the edit view persists.
export interface AutoFixState {
  /// Gray-world white-balance pass (`auto_color`).
  autoColor: boolean;
  /// Percentile tone-stretch pass (`auto_contrast`).
  autoContrast: boolean;
}

/// Recipe ops for the current state. Default state emits no ops at all.
export function opsFromState(
  state: AutoFixState,
): Array<{ type: "auto_color" | "auto_contrast"; params: Record<string, unknown> }> {
  const ops: Array<{ type: "auto_color" | "auto_contrast"; params: Record<string, unknown> }> = [];
  if (state.autoColor) {
    ops.push({ type: "auto_color", params: {} });
  }
  if (state.autoContrast) {
    ops.push({ type: "auto_contrast", params: {} });
  }
  return ops;
}
