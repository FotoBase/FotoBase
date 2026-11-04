// Unnamed people queue (TASK-05-005, PLAN §2G): the IPC wrapper plus the
// pure queue rules the panel shares. Faces are never sent over IPC — the
// representative thumbnail comes from the photo's content-hash `thumb://`
// URI like the grid; naming happens on explicit confirm only.

/// One reviewable cluster from the backend: the representative face's box
/// (fractions of the photo) plus the photo's content hash backing the
/// `thumb://` URI (null until the lazy hash backfill fills it in).
export interface QueueCluster {
  cluster_id: number;
  face_count: number;
  face_id: number;
  file_id: number;
  x: number;
  y: number;
  width: number;
  height: number;
  hash: string | null;
}

async function call<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  const { invoke } = await import("@tauri-apps/api/core");
  return await invoke<T>(cmd, args);
}

/// The unnamed clusters, queue-ordered.
export async function peopleQueue(): Promise<QueueCluster[]> {
  return (await call<QueueCluster[] | null>("people_queue")) ?? [];
}

/// Confirm a cluster as `name`: every actionable face is stamped with the
/// person and the name lands in each photo's sidecar MWG region. Returns
/// how many faces were confirmed.
export function confirmCluster(clusterId: number, name: string): Promise<number> {
  return call<number>("people_queue_bulk", {
    clusterId,
    action: { confirm: { name } },
  });
}

/// Reject a cluster: the suggestion is suppressed for every still-unnamed
/// face (they stay detected). Returns how many faces were rejected.
export function rejectCluster(clusterId: number): Promise<number> {
  return call<number>("people_queue_bulk", {
    clusterId,
    action: "reject",
  });
}

/// `thumb://` URI for the representative face's photo (same shape the grid
/// uses; the face id cache-busts after a write).
export function faceThumbUri(cluster: QueueCluster): string {
  return cluster.hash ? `thumb://${cluster.hash}?v=${cluster.face_id}` : "";
}

/// A name must be non-blank to confirm a cluster.
export function canConfirm(name: string): boolean {
  return name.trim().length > 0;
}

/// The face box as CSS percentages inside the photo thumbnail.
export function faceBox(cluster: QueueCluster): {
  left: string;
  top: string;
  width: string;
  height: string;
} {
  return {
    left: `${cluster.x * 100}%`,
    top: `${cluster.y * 100}%`,
    width: `${cluster.width * 100}%`,
    height: `${cluster.height * 100}%`,
  };
}

// ---- People Manager (TASK-05-006, PLAN §2 Tools menu, §6 P3 People) ----

/// One People Manager entry: a named person (`person_id`) or an unnamed
/// cluster (`cluster_id`), with a face count and a representative face in
/// the same shape the queue uses (null when a person has no faces).
export interface PeopleEntry {
  person_id: number | null;
  cluster_id: number | null;
  name: string | null;
  face_count: number;
  representative: QueueCluster | null;
}

/// Every named person and every unnamed cluster, named first.
export async function peopleEntries(): Promise<PeopleEntry[]> {
  return (await call<PeopleEntry[] | null>("people_manager_list")) ?? [];
}

/// Rename a person: every confirmed face's sidecar MWG region is rewritten
/// (sidecar-first), then the persons row. Returns the renamed person.
export function renamePerson(
  personId: number,
  name: string,
): Promise<{ id: number; name: string }> {
  return call<{ id: number; name: string }>("people_manager_rename", {
    personId,
    name,
  });
}

/// "Reset faces" for one photo (TASK-05-009, PLAN §2 Picture context
/// menu): clear every detected/named face — sidecar MWG regions first,
/// then the catalog rows. Returns how many face rows were removed.
export function resetFaces(fileId: number): Promise<number> {
  return call<number>("reset_faces", { fileId });
}

/// `thumb://` URI for an entry's representative face; empty without one.
export function entryThumbUri(entry: PeopleEntry): string {
  return entry.representative ? faceThumbUri(entry.representative) : "";
}

/// A non-blank, changed name may be saved.
export function canRename(current: string | null, name: string): boolean {
  return name.trim().length > 0 && name.trim() !== (current ?? "").trim();
}
