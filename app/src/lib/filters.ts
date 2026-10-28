// Library filters (TASK-03-014, PLAN §2 C "Filter buttons" + "Date slider").
// The slider is a discrete scale from "last 7 days" (left) up to "all time"
// (right, the default). A position maps to a `taken_at` cutoff date that the
// backend applies in SQL — derived, catalog-side data, no sidecar involved.

import { t } from "./i18n.svelte.js";

export type DateStep =
  | { unit: "all" }
  | { unit: "days"; n: number }
  | { unit: "months"; n: number };

/// Slider positions, narrowest first; the last one is "all time".
export const DATE_STEPS: readonly DateStep[] = [
  { unit: "days", n: 7 },
  { unit: "days", n: 30 },
  { unit: "months", n: 3 },
  { unit: "months", n: 6 },
  { unit: "months", n: 12 },
  { unit: "months", n: 24 },
  { unit: "months", n: 60 },
  { unit: "all" },
];

/// The "no restriction" position — also the slider's default.
export const ALL_TIME = DATE_STEPS.length - 1;

function pad(n: number): string {
  return String(n).padStart(2, "0");
}

function iso(d: Date): string {
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`;
}

function step(position: number): DateStep {
  const i = Math.min(Math.max(Math.trunc(position), 0), DATE_STEPS.length - 1);
  return DATE_STEPS[i];
}

/// `YYYY-MM-DD` of the oldest day still shown at `position` (local calendar
/// date, inclusive), or `null` for "all time". Out-of-range positions clamp.
export function cutoffDate(position: number, now: Date = new Date()): string | null {
  const s = step(position);
  if (s.unit === "all") return null;
  if (s.unit === "days") {
    return iso(new Date(now.getFullYear(), now.getMonth(), now.getDate() - s.n));
  }
  // Calendar months; a day past the target month's end clamps to its last day.
  const first = new Date(now.getFullYear(), now.getMonth() - s.n, 1);
  const lastDay = new Date(first.getFullYear(), first.getMonth() + 1, 0).getDate();
  return iso(new Date(first.getFullYear(), first.getMonth(), Math.min(now.getDate(), lastDay)));
}

/// Human label for a slider position ("All time", "Last 7 days", …).
export function dateStepLabel(position: number): string {
  const s = step(position);
  if (s.unit === "all") return t("filters.allTime");
  if (s.unit === "days") return t("filters.lastDays", { n: s.n });
  if (s.n === 12) return t("filters.lastYear");
  if (s.n % 12 === 0) return t("filters.lastYears", { n: s.n / 12 });
  return t("filters.lastMonths", { n: s.n });
}
