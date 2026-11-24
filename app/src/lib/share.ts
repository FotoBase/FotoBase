// Share sheet (TASK-09-005, PLAN §6 Phase 5): IPC wrapper + selection order.
// The backend renders the photos into a scratch folder and opens the native
// share sheet once with every file; no image bytes cross IPC.

export interface ShareReply {
  /// Photos attached to the share sheet.
  shared: number;
  /// Photos that could not be prepared and were left out.
  failed: number;
}

/// The selection as an id list in grid order, so the share sheet receives
/// photos in the order the user sees them. Selected ids the current view does
/// not list (e.g. picked in another folder) follow, in selection order.
export function shareIds(selection: ReadonlySet<number>, view: readonly { id: number }[]): number[] {
  const inView = view.map((p) => p.id).filter((id) => selection.has(id));
  const seen = new Set(inView);
  return [...inView, ...[...selection].filter((id) => !seen.has(id))];
}

export async function sharePhotos(ids: number[]): Promise<ShareReply> {
  const { invoke } = await import("@tauri-apps/api/core");
  return await invoke<ShareReply>("share_photos", { ids });
}
