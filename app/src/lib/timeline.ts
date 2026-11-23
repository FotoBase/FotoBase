// Timeline view pure logic (TASK-08-018): the layout, hit-testing and
// keyboard-stepping the full-screen canvas shares with its tests. No DOM,
// no Svelte — directly unit-testable.

import type { LibraryView } from "./search";

/// One calendar day, as the backend groups it (oldest first).
export interface TimelineGroup {
  /// `YYYY-MM-DD`.
  date: string;
  count: number;
  /// Earliest photo of the day: the group's thumbnail, and the photo the
  /// grid reveals when the group is opened.
  first_id: number;
  hash: string | null;
}

/// Thumbnail size and spacing, px.
export const CELL = 112;
export const LEFT_PAD = 48;
/// Gap between neighbouring groups: a minimum, plus a little per empty day,
/// capped — a 3-year hole must not become a 100k px desert.
export const GAP_MIN = 12;
export const GAP_PER_DAY = 1.5;
export const GAP_MAX = 96;
/// Groups skipped by PageUp / PageDown.
export const PAGE_STEP = 10;

export interface TimelineTick {
  /// Year (`2023`) or month (`Jul`) label.
  label: string;
  x: number;
  kind: "year" | "month";
}

export interface TimelineLayout {
  groups: TimelineGroup[];
  /// Left edge of each group's cell; ascending.
  xs: number[];
  totalWidth: number;
  ticks: TimelineTick[];
}

function dayNumber(date: string): number {
  return Date.UTC(+date.slice(0, 4), +date.slice(5, 7) - 1, +date.slice(8, 10)) / 86_400_000;
}

function monthName(month: number): string {
  return new Intl.DateTimeFormat(undefined, { month: "short", timeZone: "UTC" }).format(
    new Date(Date.UTC(2000, month - 1, 1)),
  );
}

/// Lay the groups out left to right: date order, with the space between two
/// groups growing with the days that separate them (capped), so the axis
/// reads as time without empty stretches. Year and month boundaries become
/// ticks at the first group of the new period.
export function layoutTimeline(groups: TimelineGroup[]): TimelineLayout {
  const xs: number[] = [];
  const ticks: TimelineTick[] = [];
  let x = LEFT_PAD;
  groups.forEach((g, i) => {
    if (i > 0) {
      const days = dayNumber(g.date) - dayNumber(groups[i - 1].date);
      x += CELL + Math.min(GAP_MAX, GAP_MIN + Math.max(0, days - 1) * GAP_PER_DAY);
    }
    xs.push(x);
    const prev = i > 0 ? groups[i - 1].date : null;
    if (prev === null || prev.slice(0, 4) !== g.date.slice(0, 4)) {
      ticks.push({ label: g.date.slice(0, 4), x, kind: "year" });
    } else if (prev.slice(5, 7) !== g.date.slice(5, 7)) {
      ticks.push({ label: monthName(+g.date.slice(5, 7)), x, kind: "month" });
    }
  });
  const totalWidth = groups.length === 0 ? 0 : x + CELL + LEFT_PAD;
  return { groups, xs, totalWidth, ticks };
}

/// Half-open index range `[start, end)` of the groups intersecting
/// `[scrollLeft - overscan, scrollLeft + viewport + overscan]`. Binary
/// search: a 50k-group timeline costs O(log n) per frame, not O(n).
export function visibleRange(
  layout: TimelineLayout,
  scrollLeft: number,
  viewport: number,
  overscan = CELL,
): [number, number] {
  const { xs } = layout;
  const lo = scrollLeft - overscan;
  const hi = scrollLeft + viewport + overscan;
  let a = 0;
  let b = xs.length;
  while (a < b) {
    const m = (a + b) >> 1;
    if (xs[m] + CELL < lo) a = m + 1;
    else b = m;
  }
  const start = a;
  b = xs.length;
  while (a < b) {
    const m = (a + b) >> 1;
    if (xs[m] <= hi) a = m + 1;
    else b = m;
  }
  return [start, a];
}

/// Index of the group whose cell contains content-space `x`, else -1.
export function groupIndexAt(layout: TimelineLayout, x: number): number {
  const [start, end] = visibleRange(layout, x, 0, 0);
  for (let i = start; i < end; i++) {
    if (x >= layout.xs[i] && x <= layout.xs[i] + CELL) return i;
  }
  return -1;
}

/// The group a key moves the focus to, or `null` for a key the timeline
/// does not handle. Clamped to the ends; `current` -1 means "nothing yet".
export function stepIndex(current: number, key: string, count: number): number | null {
  if (count === 0) return null;
  const last = count - 1;
  const from = Math.max(0, current);
  switch (key) {
    case "ArrowRight":
      return current < 0 ? 0 : Math.min(last, from + 1);
    case "ArrowLeft":
      return current < 0 ? 0 : Math.max(0, from - 1);
    case "PageDown":
      return Math.min(last, from + PAGE_STEP);
    case "PageUp":
      return Math.max(0, from - PAGE_STEP);
    case "Home":
      return 0;
    case "End":
      return last;
    default:
      return null;
  }
}

/// `scrollLeft` that centres group `index` in a `viewport`-wide scroller.
export function scrollLeftFor(layout: TimelineLayout, index: number, viewport: number): number {
  const centre = layout.xs[index] + CELL / 2;
  return Math.max(0, Math.min(centre - viewport / 2, layout.totalWidth - viewport));
}

/// The timeline for the grid's current view, so its filters apply (hidden
/// photos are excluded backend-side). Read-only; thumbnails stay on `thumb://`.
export async function loadTimeline(view?: LibraryView): Promise<TimelineGroup[]> {
  const { invoke } = await import("@tauri-apps/api/core");
  const text = view?.query.trim() ?? "";
  return await invoke<TimelineGroup[]>("timeline_groups", {
    text: text === "" ? null : text,
    folders: view?.folders.length ? view.folders : null,
    album: view?.album ?? null,
    starred: view?.starred ?? null,
    videosOnly: view?.videosOnly ?? null,
    hasFaces: view?.hasFaces ?? null,
    geotagged: view?.geotagged ?? null,
    takenSince: view?.takenSince ?? null,
  });
}

/// Browser decode of a `thumb://` URI for `drawImage` — bytes come from the
/// disk cache through the scheme handler, never over IPC (§3 rule 3).
export function decodeThumb(uri: string): Promise<HTMLImageElement | null> {
  return new Promise((resolve) => {
    const img = new Image();
    img.onload = () => resolve(img);
    img.onerror = () => resolve(null);
    img.src = uri;
  });
}
