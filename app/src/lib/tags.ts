// Tags (TASK-03-006, PLAN §2B). Pure decisions live here — nested-tag path
// handling and the add/remove toggle — while the backend owns the write
// (sidecar first via the sync engine, catalog mirror second).

/// Nested tags use the Lightroom `|` path separator (`Vacation|2024`), the
/// form darktable and digiKam read/write.
export function isHierarchical(tag: string): boolean {
  return tag.includes("|");
}

/// Display name of a nested tag: the last path segment.
export function leafName(tag: string): string {
  const parts = tag.split("|");
  return parts[parts.length - 1];
}

/// Normalize user input: trim, collapse separator runs and drop empty
/// segments. `""` for input without any tag text.
export function normalizeTag(tag: string): string {
  const cleaned = tag
    .split("|")
    .map((seg) => seg.trim())
    .filter((seg) => seg.length > 0)
    .join("|");
  return cleaned;
}

/// Toggle a tag in a local list (pure helper for optimistic UI).
export function toggleTagInList(tags: string[], tag: string): string[] {
  return tags.includes(tag) ? tags.filter((t) => t !== tag) : [...tags, tag];
}

/// Read a photo's tags from the catalog mirror.
export async function getTags(fileId: number): Promise<string[]> {
  const { invoke } = await import("@tauri-apps/api/core");
  return await invoke<string[]>("get_tags", { fileId });
}

/// Add a tag to one photo; returns the updated list.
export async function addTag(fileId: number, tag: string): Promise<string[]> {
  const { invoke } = await import("@tauri-apps/api/core");
  return await invoke<string[]>("add_tag", { fileId, tag });
}

/// Remove a tag from one photo; returns the remaining list.
export async function removeTag(
  fileId: number,
  tag: string,
): Promise<string[]> {
  const { invoke } = await import("@tauri-apps/api/core");
  return await invoke<string[]>("remove_tag", { fileId, tag });
}

/// Toggle a tag on one photo: remove when present, add when missing.
export async function toggleTag(
  fileId: number,
  tag: string,
): Promise<string[]> {
  const current = await getTags(fileId);
  return current.includes(tag) ? removeTag(fileId, tag) : addTag(fileId, tag);
}

/// Rename a tag across the library (rewrites every affected sidecar).
export async function renameTag(
  oldName: string,
  newName: string,
): Promise<number> {
  const { invoke } = await import("@tauri-apps/api/core");
  return await invoke<number>("rename_tag", { old: oldName, new: newName });
}

/// Every tag in the library with usage counts.
export async function listTags(): Promise<{ name: string; count: number }[]> {
  const { invoke } = await import("@tauri-apps/api/core");
  return await invoke<{ name: string; count: number }[]>("list_tags");
}
