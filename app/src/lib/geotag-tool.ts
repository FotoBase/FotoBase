// Geotag tool (TASK-06-006, PLAN §2 Tools "Geotag"): one pass applies one
// location to a single photo, a multi-selection, or a whole folder/album,
// with undo. Pure logic + IPC wrappers, no DOM; the dialog wires it up.

export const GEOTAG_EVENT = "geotag-progress";

export enum GeotagTargetScope {
  Selection = "selection",
  Folder = "folder",
  Album = "album",
}

export type GeotagTarget =
  | { kind: "ids"; ids: number[] }
  | { kind: "folder"; folder_id: number }
  | { kind: "album"; album_id: number };

export interface GeotagFailure {
  id: number;
  error: string;
}

export interface GeotagToolReport {
  applied: number;
  failed: GeotagFailure[];
  cancelled: boolean;
}

export interface GeotagProgress {
  done: number;
  total: number;
}

/// Decimal degrees or `null`. The backend re-validates; this only keeps
/// obviously broken input out of the call.
export function parseLatitude(text: string): number | null {
  return parseDegrees(text, 90);
}

export function parseLongitude(text: string): number | null {
  return parseDegrees(text, 180);
}

function parseDegrees(text: string, limit: number): number | null {
  const t = text.trim();
  if (t === "") return null;
  const value = Number(t);
  if (!Number.isFinite(value) || Math.abs(value) > limit) return null;
  return value;
}

/// The backend target for the chosen scope. A folder/album scope without a
/// folder/album id (plain search view) falls back to the selection.
export function geotagTarget(
  scope: GeotagTargetScope,
  ids: readonly number[],
  folderId: number | null,
  albumId: number | null,
): GeotagTarget {
  switch (scope) {
    case GeotagTargetScope.Folder:
      return folderId !== null ? { kind: "folder", folder_id: folderId } : { kind: "ids", ids: [...ids] };
    case GeotagTargetScope.Album:
      return albumId !== null ? { kind: "album", album_id: albumId } : { kind: "ids", ids: [...ids] };
    default:
      return { kind: "ids", ids: [...ids] };
  }
}

/// One-line report summary for the dialog.
export function formatGeotagReport(report: GeotagToolReport): string {
  let text = `done:${report.applied}`;
  if (report.failed.length > 0) text += ` failed:${report.failed.length}`;
  if (report.cancelled) text += " cancelled";
  return text;
}

async function call<T>(cmd: string, args: Record<string, unknown>): Promise<T> {
  const { invoke } = await import("@tauri-apps/api/core");
  return await invoke<T>(cmd, args);
}

/// Run the tool over a target. Progress streams on `geotag-progress`; the
/// run keeps going past individual failures (they come back listed).
export function runGeotagTool(
  target: GeotagTarget,
  latitude: number,
  longitude: number,
): Promise<GeotagToolReport> {
  return call<GeotagToolReport>("geotag_tool", { request: { target, latitude, longitude } });
}

/// Cancel the running pass (best-effort, no-op without a run).
export function cancelGeotag(): Promise<void> {
  return call<void>("cancel_geotag", {});
}

/// Undo the latest pass: restores every photo's prior GPS.
export function undoGeotag(): Promise<number> {
  return call<number>("undo_geotag", {});
}

/// Subscribe to tool progress while a run is in flight.
export async function onGeotagProgress(cb: (p: GeotagProgress) => void): Promise<() => void> {
  const { listen } = await import("@tauri-apps/api/event");
  return await listen<GeotagProgress>(GEOTAG_EVENT, (e) => cb(e.payload));
}
