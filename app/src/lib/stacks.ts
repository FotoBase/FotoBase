// Stacking (TASK-03-021, PLAN §6 P2a, ADR-0006): RAW+JPEG pairs, bursts and
// edited versions under one thumbnail. The backend owns every mutation —
// membership is written to each member's sidecar and the catalog mirrors it;
// nothing here touches a file. This module is the IPC wrapper plus the pure
// rules the grids and menus share. No image bytes cross IPC.

import type { GridItem } from "./grid";

/// A group the backend offers to stack (nothing is stacked until accepted).
export interface StackSuggestion {
  kind: "raw_jpeg" | "edited";
  /// Proposed order: the top first.
  file_ids: number[];
  filenames: string[];
}

/// Badge geometry in px, shared by the DOM badge's CSS and the canvas
/// renderer's drawing and hit test.
export const STACK_BADGE = { width: 24, height: 16, margin: 4 } as const;

/// A stack of two or more photos wears a badge; a lone leftover member or a
/// photo from an older payload does not.
export function hasStackBadge(item: GridItem): boolean {
  return (item.stack_count ?? 0) >= 2;
}

export function badgeText(item: GridItem): string {
  return String(item.stack_count ?? 0);
}

/// Is a point (relative to a thumbnail's top-left) on its stack badge,
/// which sits in the top-right corner?
export function inStackBadge(localX: number, localY: number, thumbSize: number): boolean {
  const { width, height, margin } = STACK_BADGE;
  const left = thumbSize - margin - width;
  return localX >= left && localX <= thumbSize - margin && localY >= margin && localY <= margin + height;
}

/// May these photos be stacked together? Two or more; at most one existing
/// stack among them (the others join it); not all already in one stack.
export function canStack(items: GridItem[]): boolean {
  if (items.length < 2) return false;
  const stacks = new Set(items.map((i) => i.stack_id).filter((s): s is string => !!s));
  if (stacks.size > 1) return false;
  return stacks.size === 0 || items.some((i) => !i.stack_id);
}

async function call<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  const { invoke } = await import("@tauri-apps/api/core");
  return await invoke<T>(cmd, args);
}

/// Stack the photos (the first becomes the top of a new stack) or add them
/// to the stack one of them already belongs to. Returns the stack id.
export function stackPhotos(fileIds: number[]): Promise<string> {
  return call<string>("stack_photos", { fileIds });
}

export async function setStackTop(fileId: number): Promise<void> {
  await call<void>("set_stack_top", { fileId });
}

/// Dissolve the stack: every photo shows on its own again.
export async function unstack(stackId: string): Promise<void> {
  await call<void>("unstack", { stackId });
}

/// The stack's photos, top first, for the picker.
export async function listStack(stackId: string): Promise<GridItem[]> {
  return (await call<GridItem[] | null>("list_stack", { stackId })) ?? [];
}

export async function stackSuggestions(): Promise<StackSuggestion[]> {
  return (await call<StackSuggestion[] | null>("stack_suggestions")) ?? [];
}
