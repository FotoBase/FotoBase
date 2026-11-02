// Near-duplicate finder (TASK-04-019, PLAN §2 2026 additions): the IPC
// wrapper plus the pure review-state rules the dialog shares. Nothing here
// deletes anything — deletion happens on explicit user confirmation, routed
// through the backend's OS-trash path ("Delete from disk"). No image bytes
// cross IPC; the dialog shows `thumb://` URIs like the grid.

/// One reviewable group from the backend. `hashes` are the content hashes
/// backing `thumb://` URIs (null until the lazy hash backfill fills them).
export interface DuplicateGroup {
  file_ids: number[];
  paths: string[];
  filenames: string[];
  hashes: (string | null)[];
}

/// Outcome of a hashing pass over the unhashed images.
export interface HashReport {
  hashed: number;
  failed: number;
}

async function call<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  const { invoke } = await import("@tauri-apps/api/core");
  return await invoke<T>(cmd, args);
}

/// Hash everything still unhashed (background job on the backend) and
/// return the pass report.
export function findDuplicates(): Promise<HashReport> {
  return call<HashReport>("find_duplicates");
}

/// The current near-duplicate groups across the catalog.
export async function duplicateGroups(): Promise<DuplicateGroup[]> {
  return (await call<DuplicateGroup[] | null>("duplicate_groups")) ?? [];
}

/// Move the user-confirmed subset of a group to the OS trash and drop the
/// catalog rows. Returns how many were deleted.
export function deleteDuplicateFiles(ids: number[]): Promise<number> {
  return call<number>("delete_duplicate_files", { ids });
}

/// Which members of a group are marked for deletion. Keepers are the
/// default: nothing is deleted unless the user marks it.
export function markedIds(group: DuplicateGroup, marked: ReadonlySet<number>): number[] {
  return group.file_ids.filter((id) => marked.has(id));
}

/// Never allow deleting every member of a group: at least one keeper stays.
export function canDelete(group: DuplicateGroup, marked: ReadonlySet<number>): boolean {
  const n = markedIds(group, marked).length;
  return n > 0 && n < group.file_ids.length;
}
