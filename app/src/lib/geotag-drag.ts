// Drag-to-geotag payload (TASK-06-005, PLAN §2H "drag-and-drop to
// geotag"). Pure logic, DOM-free apart from the standard DataTransfer /
// maplibre event types: the drag payload format, and turning a drop
// event's coordinates into the geotag call the backend receives.
//
// Photos carry a custom MIME type so only Fotobase targets react to the
// drag; `text/plain` mirrors the ids as a fallback for debugging.

import { invoke } from "@tauri-apps/api/core";
import type { TrayPhoto } from "./tray.svelte";

export const PHOTO_DRAG_MIME = "application/x-fotobase-photos";

/// Stamp a drag with the photos it carries (tray selection or grid
/// selection). Ids only — no image bytes over IPC (PLAN §3 rule 3).
export function setPhotoDrag(transfer: DataTransfer, photos: readonly TrayPhoto[]): void {
  const ids = photos.map((p) => p.id);
  transfer.setData(PHOTO_DRAG_MIME, JSON.stringify(ids));
  transfer.setData("text/plain", ids.join(","));
  transfer.effectAllowed = "copy";
}

/// Read a drop's photo ids; `null` when the drag is not ours.
export function readPhotoDrop(transfer: DataTransfer): number[] | null {
  const raw = transfer.getData(PHOTO_DRAG_MIME);
  if (!raw) return null;
  try {
    const parsed: unknown = JSON.parse(raw);
    if (!Array.isArray(parsed) || !parsed.every((v) => typeof v === "number")) return null;
    return parsed as number[];
  } catch {
    return null;
  }
}

/// Should the map accept this drag? Only Fotobase photo drags.
export function acceptsPhotoDrag(transfer: DataTransfer): boolean {
  return transfer.types.includes(PHOTO_DRAG_MIME);
}

/// Geotag the dropped photos at the drop point (decimal degrees; the
/// backend converts them into the XMP `exif:GPSLatitude/Longitude` forms
/// with their N/S/E/W conventions). Fires `library-changed` on success so
/// pins and the grid refresh.
export async function geotagPhotos(fileIds: number[], lat: number, lon: number): Promise<void> {
  if (fileIds.length === 0) return;
  await invoke("geotag_photos", { fileIds, latitude: lat, longitude: lon });
}
