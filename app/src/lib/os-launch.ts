// OS context-menu launches (TASK-08-020): "Upload with Fotobase" hands the
// app file paths; the shell resolves them against the catalog and opens the
// upload dialog on the photos it knows.

export interface UploadDialogState {
  open: boolean;
  ids: number[];
}

/// Resolve OS paths to catalog ids, dropping uncataloged files and
/// duplicates (a lookup that throws counts as "not found").
export async function resolveUploadIds(
  paths: string[],
  find: (path: string) => Promise<{ id: number } | null>,
): Promise<number[]> {
  const ids: number[] = [];
  for (const path of paths) {
    try {
      const match = await find(path);
      if (match && !ids.includes(match.id)) ids.push(match.id);
    } catch {
      // Backend down mid-lookup: skip this file.
    }
  }
  return ids;
}

/// Next dialog state for freshly resolved ids. Explorer starts one process
/// per selected file, so launches arriving while the dialog is open add to
/// it instead of replacing the selection. No ids: state unchanged.
export function mergeUploadRequest(
  current: UploadDialogState,
  ids: number[],
): UploadDialogState {
  if (ids.length === 0) return current;
  if (!current.open) return { open: true, ids };
  return { open: true, ids: [...new Set([...current.ids, ...ids])] };
}
