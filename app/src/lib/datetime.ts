// Date/time adjustment (TASK-03-019, PLAN §6 P2a): shift capture time, fix a
// wrong timezone, merge multi-camera sets. The backend does the arithmetic
// and writes `exif:DateTimeOriginal` to the sidecar (catalog follows); this
// module maps the dialog's input to the adjustment it takes and formats
// what comes back. Pure helpers + IPC wrappers, no DOM.

/// What to do to the selection's capture times (tag = backend `kind`).
export type TimeAdjustment =
  /// Move the wall clock; also the multi-camera merge (see `mergeShiftSeconds`).
  | { kind: "shift"; seconds: number }
  /// Clock reading is right, zone label wrong: relabel, keep the reading.
  | { kind: "relabel_offset"; offset_minutes: number }
  /// Moment is right, shown in the wrong zone: same instant in this offset.
  | { kind: "convert_offset"; offset_minutes: number };

export interface TimePreview {
  id: number;
  before: string | null;
  after: string | null;
  /// Why the photo would be skipped; `after` is null then.
  error: string | null;
}

export interface AdjustFailure {
  id: number;
  error: string;
}

export interface AdjustReport {
  adjusted: number;
  failed: AdjustFailure[];
  cancelled: boolean;
}

export interface DateTimeProgress {
  done: number;
  total: number;
}

export interface ShiftParts {
  direction: "later" | "earlier";
  days: number;
  hours: number;
  minutes: number;
  seconds: number;
}

const MIN_OFFSET = -12 * 60;
const MAX_OFFSET = 14 * 60;

const whole = (n: number): number => (Number.isFinite(n) ? Math.max(0, Math.trunc(n)) : 0);

/// Signed seconds of a shift form. Blank, negative or fractional fields count
/// as whole non-negative numbers; the direction carries the sign.
export function shiftSeconds(parts: ShiftParts): number {
  const total =
    whole(parts.days) * 86_400 +
    whole(parts.hours) * 3600 +
    whole(parts.minutes) * 60 +
    whole(parts.seconds);
  return parts.direction === "earlier" && total !== 0 ? -total : total;
}

/// UTC offset in minutes from `+02:00`, `-0530`, `+2`, `Z`, `UTC`; `null`
/// when it is not one or lies outside the real-world −12:00…+14:00.
export function parseOffset(text: string): number | null {
  const t = text.trim();
  if (/^(z|utc)$/i.test(t)) return 0;
  const m = /^([+-])(\d{1,2})(?::?(\d{2}))?$/.exec(t);
  if (!m) return null;
  const minutes = Number(m[2]) * 60 + Number(m[3] ?? 0);
  if (Number(m[3] ?? 0) >= 60) return null;
  const signed = m[1] === "-" ? -minutes : minutes;
  return signed >= MIN_OFFSET && signed <= MAX_OFFSET ? signed : null;
}

export function formatOffset(minutes: number): string {
  const p = (n: number): string => String(n).padStart(2, "0");
  const abs = Math.abs(minutes);
  return `${minutes < 0 ? "-" : "+"}${p(Math.floor(abs / 60))}:${p(abs % 60)}`;
}

/// Wall-clock seconds of a `datetime-local` value (`YYYY-MM-DDTHH:MM[:SS]`),
/// read as if UTC so no machine timezone can leak in; `null` when it is not a
/// real date and time.
function wallClock(text: string): number | null {
  const m = /^(\d{4})-(\d{2})-(\d{2})T(\d{2}):(\d{2})(?::(\d{2}))?$/.exec(text.trim());
  if (!m) return null;
  const [y, mo, d, h, mi, s] = [m[1], m[2], m[3], m[4], m[5], m[6] ?? "0"].map(Number);
  const ms = Date.UTC(y, mo - 1, d, h, mi, s);
  const back = new Date(ms);
  const roundTrips =
    back.getUTCFullYear() === y &&
    back.getUTCMonth() === mo - 1 &&
    back.getUTCDate() === d &&
    h < 24 &&
    mi < 60 &&
    s < 60;
  return roundTrips && y >= 1 ? ms / 1000 : null;
}

/// Multi-camera merge: one moment reads `reference` on the right camera and
/// `other` on the camera being corrected; shifting the latter's photos by this
/// many seconds aligns the clocks. `null` while either entry is incomplete.
export function mergeShiftSeconds(reference: string, other: string): number | null {
  const a = wallClock(reference);
  const b = wallClock(other);
  return a === null || b === null ? null : a - b;
}

const ISO_TIME = /^(\d{4}-\d{2}-\d{2})T(\d{2}:\d{2}:\d{2}(?:\.\d+)?)(Z|[+-]\d{2}:\d{2})?$/;

/// "2024-06-01 12:00:00 +02:00" from the backend's ISO text; anything else is
/// shown as it is, and no value as a dash.
export function formatCaptureTime(iso: string | null): string {
  if (iso === null) return "—";
  const m = ISO_TIME.exec(iso);
  if (!m) return iso;
  return `${m[1]} ${m[2]}${m[3] ? ` ${m[3] === "Z" ? "+00:00" : m[3]}` : ""}`;
}

async function call<T>(cmd: string, args: Record<string, unknown>): Promise<T> {
  const { invoke } = await import("@tauri-apps/api/core");
  return await invoke<T>(cmd, args);
}

/// Before/after per photo, from catalog data only — nothing is written.
export function previewDateTime(
  ids: number[],
  adjustment: TimeAdjustment,
): Promise<TimePreview[]> {
  return call("preview_datetime_adjustment", { ids, adjustment });
}

/// Apply to the selection as a background job; resolves with the report.
export function adjustDateTime(ids: number[], adjustment: TimeAdjustment): Promise<AdjustReport> {
  return call("adjust_datetime", { ids, adjustment });
}

/// Subscribe to `datetime-progress`; returns the unsubscribe function.
export async function onDateTimeProgress(cb: (p: DateTimeProgress) => void): Promise<() => void> {
  const { listen } = await import("@tauri-apps/api/event");
  return await listen<DateTimeProgress>("datetime-progress", (e) => cb(e.payload));
}
