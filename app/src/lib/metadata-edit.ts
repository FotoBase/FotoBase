// Info-pane metadata editing (TASK-03-020, PLAN §6 P2): capture date, GPS,
// artist, copyright of the photo being viewed. The backend validates the
// input and writes the sidecar (catalog mirror second); this module maps the
// form's draft to the edit it takes — only what changed — and wraps the IPC.
// Pure helpers, no DOM.

import type { PhotoInfo } from "./info-pane";

/// What the form holds: every field as the text in its input.
export interface MetadataDraft {
  takenAt: string;
  latitude: string;
  longitude: string;
  artist: string;
  copyright: string;
}

/// What to change on the photo (backend `MetadataEdit`); an absent key is
/// left alone.
export interface MetadataEdit {
  taken_at?: string;
  gps?: { kind: "set"; latitude: string; longitude: string } | { kind: "clear" };
  artist?: string;
  copyright?: string;
}

export type MetadataField = "taken_at" | "latitude" | "longitude";

export interface FieldError {
  field: MetadataField;
  message: string;
}

/// Rejected input comes back as `errors` with nothing written; otherwise
/// `info` is the photo as now stored.
export interface EditResult {
  errors: FieldError[];
  info: PhotoInfo | null;
}

/// Decimal degrees, at most six places (about 10 cm); `""` for no value.
export function formatCoordinate(value: number | null): string {
  return value === null ? "" : String(Number(value.toFixed(6)));
}

const WALL_CLOCK = /^(\d{4}-\d{2}-\d{2})[T ](\d{2}:\d{2}:\d{2})/;

/// "2024-06-01 12:00:00" — the wall clock of a stored capture time, without
/// fraction or offset (the backend keeps the photo's offset). Text that is
/// not a date shows as it is.
function wallClock(takenAt: string | null): string {
  if (takenAt === null) return "";
  const m = WALL_CLOCK.exec(takenAt);
  return m ? `${m[1]} ${m[2]}` : takenAt;
}

export function draftFromInfo(info: PhotoInfo): MetadataDraft {
  return {
    takenAt: wallClock(info.taken_at),
    latitude: formatCoordinate(info.latitude),
    longitude: formatCoordinate(info.longitude),
    artist: info.artist ?? "",
    copyright: info.copyright ?? "",
  };
}

/// The edit that turns `info` into `draft`; `{}` when nothing changed.
export function buildEdit(info: PhotoInfo, draft: MetadataDraft): MetadataEdit {
  const before = draftFromInfo(info);
  const edit: MetadataEdit = {};
  if (draft.takenAt.trim() !== before.takenAt) edit.taken_at = draft.takenAt.trim();
  if (draft.latitude.trim() !== before.latitude || draft.longitude.trim() !== before.longitude) {
    edit.gps =
      draft.latitude.trim() === "" && draft.longitude.trim() === ""
        ? { kind: "clear" }
        : { kind: "set", latitude: draft.latitude.trim(), longitude: draft.longitude.trim() };
  }
  if (draft.artist !== before.artist) edit.artist = draft.artist;
  if (draft.copyright !== before.copyright) edit.copyright = draft.copyright;
  return edit;
}

/// Write one photo's edit through the backend (sidecar first).
export async function editPhotoMetadata(
  fileId: number,
  edit: MetadataEdit,
): Promise<EditResult> {
  const { invoke } = await import("@tauri-apps/api/core");
  return await invoke<EditResult>("edit_photo_metadata", { fileId, edit });
}
