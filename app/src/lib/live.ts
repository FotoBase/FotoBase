// Live/Motion photo helpers (TASK-08-017, PLAN §2). The backend detects
// Apple Live Photos and Android Motion Photos at scan time and stores the
// kind in the catalog; this module holds the badge rule and the clip lookup.
// Only a file path crosses IPC — the `<video>` loads it via the asset
// protocol like any other video (no media bytes over IPC, PLAN §3 rule 3).

import type { GridItem } from "./grid";

export type LiveKind = "motion" | "live";

/// Badge geometry in px, shared by the DOM badge's CSS and the canvas
/// renderer's drawing. Sits in the top-left corner (stacks use top-right).
export const LIVE_BADGE = { width: 44, height: 16, margin: 4 } as const;

/// Live and Motion photos wear a badge; everything else (and payloads from
/// before detection existed) does not.
export function hasLiveBadge(item: GridItem): boolean {
  return item.live_kind === "live" || item.live_kind === "motion";
}

/// i18n key for the badge/title of a kind.
export function liveBadgeKey(kind: LiveKind): string {
  return kind === "live" ? "live.badge.live" : "live.badge.motion";
}

/// Playable `<video src>` for a photo's clip, or `null` when it has none or
/// the backend is unavailable (browser dev) — the viewer then just shows the
/// still.
export async function fetchLiveClipSrc(fileId: number): Promise<string | null> {
  try {
    const { invoke, convertFileSrc } = await import("@tauri-apps/api/core");
    const clip = await invoke<{ path: string; kind: LiveKind } | null>("live_clip", { fileId });
    return clip ? convertFileSrc(clip.path) : null;
  } catch {
    return null;
  }
}
