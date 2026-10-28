// Albums (TASK-03-008, PLAN §2 B Albums). The backend owns every write:
// membership goes to each photo's `fb:Albums` sidecar bag first, the catalog
// mirror second, and the album definitions to `albums.json` beside the
// catalog. This module is the IPC wrapper plus the pure bits the UI needs.

/// One album as served by `list_albums`.
export interface Album {
  id: number;
  uuid: string;
  name: string;
  /// ISO `YYYY-MM-DD`.
  date: string | null;
  place: string | null;
  description: string | null;
  count: number;
  cover_file_id: number | null;
  /// Content hash of the cover: the key of its `thumb://` URI.
  cover_hash: string | null;
}

/// The editable fields of an album.
export interface AlbumFields {
  name: string;
  date: string;
  place: string;
  description: string;
}

/// Today as `YYYY-MM-DD` in local time: the default date of a new album.
export function todayIso(now: Date = new Date()): string {
  const pad = (n: number): string => String(n).padStart(2, "0");
  return `${now.getFullYear()}-${pad(now.getMonth() + 1)}-${pad(now.getDate())}`;
}

/// A form is submittable with a name and a date (PLAN: name + date required,
/// place and description optional).
export function isValidAlbumFields(fields: AlbumFields): boolean {
  return fields.name.trim() !== "" && /^\d{4}-\d{2}-\d{2}$/.test(fields.date.trim());
}

/// Photo ids a menu action applies to: the whole selection when the
/// right-clicked photo is part of it, otherwise just that photo.
export function targetIds(clicked: number, selection: ReadonlySet<number>): number[] {
  return selection.has(clicked) ? [...selection] : [clicked];
}

/// Secondary line of an album row: date and place, when present.
export function albumSubtitle(album: Pick<Album, "date" | "place">): string {
  return [album.date, album.place].filter((v): v is string => !!v).join(" · ");
}

async function call<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  const { invoke } = await import("@tauri-apps/api/core");
  return await invoke<T>(cmd, args);
}

const optional = (v: string): string | null => (v.trim() === "" ? null : v.trim());

export async function listAlbums(): Promise<Album[]> {
  return (await call<Album[] | null>("list_albums")) ?? [];
}

export function createAlbum(fields: AlbumFields): Promise<Album> {
  return call<Album>("create_album", {
    name: fields.name,
    date: fields.date,
    place: optional(fields.place),
    description: optional(fields.description),
  });
}

export function updateAlbum(albumId: number, fields: AlbumFields): Promise<Album> {
  return call<Album>("update_album", {
    albumId,
    name: fields.name,
    date: fields.date,
    place: optional(fields.place),
    description: optional(fields.description),
  });
}

/// Delete the album only; the photos stay.
export function deleteAlbum(albumId: number): Promise<void> {
  return call<void>("delete_album", { albumId });
}

/// Returns how many photos were newly added.
export function addToAlbum(albumId: number, fileIds: number[]): Promise<number> {
  return call<number>("add_to_album", { albumId, fileIds });
}

/// Returns how many photos were removed from the album.
export function removeFromAlbum(albumId: number, fileIds: number[]): Promise<number> {
  return call<number>("remove_from_album", { albumId, fileIds });
}

export function setAlbumCover(albumId: number, fileId: number): Promise<Album> {
  return call<Album>("set_album_cover", { albumId, fileId });
}

/// Grid items of one album (id + hash; no pixels over IPC), narrowed by the
/// live-search query when it is not blank.
export function listAlbumFiles<T>(albumId: number, query = ""): Promise<T[]> {
  const q = query.trim();
  return q === ""
    ? call<T[]>("list_album_files", { albumId })
    : call<T[]>("list_album_files", { albumId, query: q });
}

/// Ids of the albums a photo is in.
export async function getFileAlbums(fileId: number): Promise<number[]> {
  return (await call<number[] | null>("get_file_albums", { fileId })) ?? [];
}
