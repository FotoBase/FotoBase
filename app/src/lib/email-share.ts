// Email dialog (TASK-08-006, PLAN §2J Email): IPC wrapper + size choices.
// The backend resizes into a scratch folder through the shared export path
// and opens the OS mail flow; no image bytes cross IPC.

/// Longest-edge choices (px) offered in the dialog.
export const EMAIL_SIZES = [640, 1024, 1600, 2048] as const;

/// Sensible default: readable on screen, small enough for mail limits.
export const DEFAULT_EMAIL_EDGE = 1600;

export interface EmailReply {
  /// `attached`: compose window opened with the photos attached.
  /// `mailto_revealed`: compose window without attachments; the resized
  /// photos' folder was revealed for drag-and-drop.
  delivery: "attached" | "mailto_revealed";
  attached: number;
  failed: number;
}

export async function emailPhotos(
  ids: number[],
  maxEdge: number,
): Promise<EmailReply> {
  const { invoke } = await import("@tauri-apps/api/core");
  return await invoke<EmailReply>("email_photos", {
    ids,
    request: { max_edge: maxEdge },
  });
}
