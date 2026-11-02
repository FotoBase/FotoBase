// Batch edit ops (TASK-04-011, PLAN §2E "Batch edit"): one op applied to a
// multi-selection's sidecar recipes. The backend queues the run through the
// job queue (§3 rule 4) at background priority and streams a progress
// counter; this module is the op catalog the dialog renders plus the IPC
// wrappers. No image bytes, no files — the backend owns the sidecar
// write-through (source of truth, §5).

/// Per-photo failure: the photo was skipped and its data left untouched.
export interface BatchEditFailure {
  id: number;
  error: string;
}

/// Outcome of one batch run.
export interface BatchEditReport {
  applied: number;
  failed: BatchEditFailure[];
  cancelled: boolean;
}

/// Progress event payload.
export interface BatchEditProgress {
  done: number;
  total: number;
}

/// Tauri event the backend streams batch progress on (§3: counters, no
/// image bytes over IPC).
export const BATCH_EDIT_EVENT = "batch-edit-progress";

/// Every §2E batch op, in menu order: rotate CW/CCW, auto contrast, auto
/// color, sepia, sharpen, warmify, film grain, B&W, auto red-eye, show and
/// hide text. Values are the backend `BatchOp` enum's snake_case serde
/// names — never hand-mangled.
export const BATCH_OPS: ReadonlyArray<{ id: string; labelKey: string }> = [
  { id: "rotate_cw", labelKey: "batchEdit.rotateCw" },
  { id: "rotate_ccw", labelKey: "batchEdit.rotateCcw" },
  { id: "auto_contrast", labelKey: "batchEdit.autoContrast" },
  { id: "auto_color", labelKey: "batchEdit.autoColor" },
  { id: "sepia", labelKey: "batchEdit.sepia" },
  { id: "sharpen", labelKey: "batchEdit.sharpen" },
  { id: "warmify", labelKey: "batchEdit.warmify" },
  { id: "film_grain", labelKey: "batchEdit.filmGrain" },
  { id: "bw", labelKey: "batchEdit.bw" },
  { id: "auto_red_eye", labelKey: "batchEdit.autoRedEye" },
  { id: "show_text", labelKey: "batchEdit.showText" },
  { id: "hide_text", labelKey: "batchEdit.hideText" },
];

async function call<T>(cmd: string, args: Record<string, unknown>): Promise<T> {
  const { invoke } = await import("@tauri-apps/api/core");
  return await invoke<T>(cmd, args);
}

/// Apply one batch op to the selection. Resolves with the per-photo report;
/// the run keeps going past individual failures (they come back listed).
export function batchEdit(ids: number[], op: string): Promise<BatchEditReport> {
  return call<BatchEditReport>("batch_edit", { ids, op });
}

/// Cancel the running batch (best-effort, no-op without a run).
export function cancelBatchEdit(): Promise<void> {
  return call<void>("cancel_batch_edit", {});
}

/// Subscribe to batch progress while a run is in flight.
export async function onBatchEditProgress(
  cb: (p: BatchEditProgress) => void,
): Promise<() => void> {
  const { listen } = await import("@tauri-apps/api/event");
  return await listen<BatchEditProgress>(BATCH_EDIT_EVENT, (e) => cb(e.payload));
}
