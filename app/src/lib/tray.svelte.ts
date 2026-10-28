// Photo Tray state (TASK-03-010, PLAN §2 B "Photo Tray"). A temporary
// cross-folder selection: session-scoped UI state only — never catalog data,
// never a sidecar write. Contents are photo ids (plus the hash/filename the
// panel needs to draw a `thumb://` thumbnail); actions read `ids` and
// dispatch to the existing command paths.

import type { GridItem } from "./grid";

export type TrayPhoto = Pick<GridItem, "id" | "filename" | "hash">;

export interface TrayItem extends TrayPhoto {
  /// "Hold" pins the item: it survives `clearUnheld`.
  held: boolean;
}

class TrayState {
  items = $state<TrayItem[]>([]);

  get ids(): number[] {
    return this.items.map((i) => i.id);
  }

  get count(): number {
    return this.items.length;
  }

  get heldCount(): number {
    return this.items.filter((i) => i.held).length;
  }

  has(id: number): boolean {
    return this.items.some((i) => i.id === id);
  }

  /// Append photos, skipping ones already in the tray (their hold stays).
  /// Returns how many were new.
  add(photos: readonly TrayPhoto[]): number {
    const known = new Set(this.items.map((i) => i.id));
    const fresh: TrayItem[] = [];
    for (const p of photos) {
      if (known.has(p.id)) continue;
      known.add(p.id);
      fresh.push({ id: p.id, filename: p.filename, hash: p.hash, held: false });
    }
    if (fresh.length > 0) this.items = [...this.items, ...fresh];
    return fresh.length;
  }

  remove(id: number): void {
    this.items = this.items.filter((i) => i.id !== id);
  }

  /// Re-read names and hashes from fresh library rows (a batch rename
  /// changes the filename of photos that sit in the tray). Hold stays.
  refresh(photos: readonly TrayPhoto[]): void {
    const byId = new Map(photos.map((p) => [p.id, p]));
    this.items = this.items.map((i) => {
      const p = byId.get(i.id);
      return p ? { ...i, filename: p.filename, hash: p.hash } : i;
    });
  }

  toggleHold(id: number): void {
    this.items = this.items.map((i) => (i.id === id ? { ...i, held: !i.held } : i));
  }

  /// Drop every unheld item; held ones stay. Returns how many were removed.
  clearUnheld(): number {
    const kept = this.items.filter((i) => i.held);
    const removed = this.items.length - kept.length;
    if (removed > 0) this.items = kept;
    return removed;
  }
}

export const tray = new TrayState();

/// Test hook: the store is module-global (it must survive browsing), so only
/// tests reset it.
export function resetTray(): void {
  tray.items = [];
}

/// Photo ids a tray-aware action (Quick Tags, album add, export…) applies to:
/// the tray whenever it has contents, otherwise the grid selection.
export function actionIds(selection: ReadonlySet<number>, trayIds: readonly number[]): number[] {
  return trayIds.length > 0 ? [...trayIds] : [...selection];
}
