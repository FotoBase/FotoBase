// `stream://` URL builder + stop command (TASK-02-006, PLAN §2 Video
// strategy). The transcode fallback is served through the custom scheme
// handler: no media bytes cross IPC — the `<video>` element fetches the
// transcoded stream through the scheme, exactly like `thumb://` for images.

/// Build the `<video src>` for the on-the-fly transcode fallback.
///
/// `startSecs` restarts the transcode at a keyframe at or before that
/// position (the seek mechanism): the scheme serves a fresh fragmented
/// stream from there. Degenerate offsets clamp to 0 instead of building a
/// broken URI.
export function streamSrc(fileId: number, startSecs?: number): string {
  if (startSecs === undefined) {
    return `stream://${fileId}`;
  }
  const t = Number.isFinite(startSecs) && startSecs > 0 ? startSecs : 0;
  return `stream://${fileId}?t=${t}`;
}

/// Tell the backend to cancel the in-flight transcode for a file (player
/// stop/destroy). Best-effort, like the other fire-and-forget IPC calls: a
/// missing backend (browser dev) must never break the player teardown.
export async function stopStream(fileId: number): Promise<void> {
  try {
    const { invoke } = await import("@tauri-apps/api/core");
    await invoke("stop_stream", { fileId });
  } catch {
    // Backend down: the janitor's idle timeout reaps the transcode anyway.
  }
}
