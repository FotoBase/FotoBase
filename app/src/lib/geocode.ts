// Reverse geocoding (TASK-06-008, PLAN §6 P3 Places): IPC wrappers for the
// background offline GeoNames pass. Pure logic + IPC, no DOM.

export const GEOCODE_EVENT = "geocode-progress";

export interface GeocodeProgress {
  done: number;
  total: number;
}

async function call<T>(cmd: string, args: Record<string, unknown>): Promise<T> {
  const { invoke } = await import("@tauri-apps/api/core");
  return await invoke<T>(cmd, args);
}

/// Run one reverse-geocode pass over the whole library. The backend queues
/// it at background priority (PLAN §3 rule 4) and emits `library-changed`
/// when the places/FTS mirror has converged.
export function reverseGeocodeAll(): Promise<number> {
  return call<number>("reverse_geocode_all", {});
}

/// Subscribe to pass progress while a bulk geocode is in flight.
export async function onGeocodeProgress(
  cb: (p: GeocodeProgress) => void,
): Promise<() => void> {
  const { listen } = await import("@tauri-apps/api/event");
  return await listen<GeocodeProgress>(GEOCODE_EVENT, (e) => cb(e.payload));
}
