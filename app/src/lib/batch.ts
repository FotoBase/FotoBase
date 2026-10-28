// Batch rename / move (TASK-03-011, PLAN §2 B "Batch rename / move"). The
// backend plans and performs the real file moves — sidecars and catalog rows
// follow, all-or-nothing. This module is the IPC wrapper plus the pure bits
// the dialog needs.

export type CollisionPolicy = "refuse" | "auto_suffix";

export type CollisionReason = "duplicate_target" | "target_exists" | "sidecar_exists";

/// Rename pattern: `{stem}` original name, `{n}` counter, `{date}` capture
/// date. The extension is always kept.
export interface RenameSpec {
  pattern: string;
  start: number;
  step: number;
  pad: number;
}

export interface PlannedMove {
  id: number;
  from: string;
  to: string;
}

export interface Collision extends PlannedMove {
  reason: CollisionReason;
}

export interface BatchPlan {
  moves: PlannedMove[];
  collisions: Collision[];
}

export const DEFAULT_SPEC: RenameSpec = { pattern: "photo-{n}", start: 1, step: 1, pad: 3 };

/// File name of a path, whichever separator it uses.
export function baseName(path: string): string {
  return path.split(/[\\/]/).pop() ?? path;
}

/// A plan can be applied when it changes something and has no collision.
export function isApplicable(plan: BatchPlan): boolean {
  return plan.moves.length > 0 && plan.collisions.length === 0;
}

async function call<T>(cmd: string, args: Record<string, unknown>): Promise<T> {
  const { invoke } = await import("@tauri-apps/api/core");
  return await invoke<T>(cmd, args);
}

export function previewBatchRename(
  ids: number[],
  spec: RenameSpec,
  policy: CollisionPolicy,
): Promise<BatchPlan> {
  return call("preview_batch_rename", { ids, spec, policy });
}

export function batchRename(
  ids: number[],
  spec: RenameSpec,
  policy: CollisionPolicy,
): Promise<PlannedMove[]> {
  return call("batch_rename", { ids, spec, policy });
}

export function previewBatchMove(
  ids: number[],
  dest: string,
  policy: CollisionPolicy,
): Promise<BatchPlan> {
  return call("preview_batch_move", { ids, dest, policy });
}

export function batchMove(
  ids: number[],
  dest: string,
  policy: CollisionPolicy,
): Promise<PlannedMove[]> {
  return call("batch_move", { ids, dest, policy });
}
