// TASK-03-014: pure filter model — the date slider's discrete steps map to a
// `taken_at` cutoff (derived data, catalog-side), and the library query
// carries the filter state to the backend.

import { describe, expect, it } from "vitest";

import { ALL_TIME, DATE_STEPS, cutoffDate, dateStepLabel } from "./filters";
import { locales, setLocale, t } from "./i18n.svelte.js";

// Local-time constructor: the cutoff is a calendar date in the user's zone.
const noon = (y: number, m: number, d: number) => new Date(y, m - 1, d, 12, 0, 0);

describe("DATE_STEPS", () => {
  it("grows monotonically and ends at 'all time'", () => {
    expect(DATE_STEPS[ALL_TIME]).toEqual({ unit: "all" });
    expect(ALL_TIME).toBe(DATE_STEPS.length - 1);
    const spans = DATE_STEPS.slice(0, ALL_TIME).map((s) =>
      s.unit === "days" ? s.n : s.unit === "months" ? s.n * 30 : NaN,
    );
    expect(spans.every((n) => Number.isFinite(n))).toBe(true);
    expect([...spans].sort((a, b) => a - b)).toEqual(spans);
  });
});

describe("cutoffDate", () => {
  it("is null at the all-time position (no restriction)", () => {
    expect(cutoffDate(ALL_TIME, noon(2026, 6, 15))).toBeNull();
  });

  it("subtracts days for day steps", () => {
    const i = DATE_STEPS.findIndex((s) => s.unit === "days" && s.n === 7);
    expect(cutoffDate(i, noon(2026, 6, 15))).toBe("2026-06-08");
  });

  it("subtracts calendar months and crosses year boundaries", () => {
    const i = DATE_STEPS.findIndex((s) => s.unit === "months" && s.n === 3);
    expect(cutoffDate(i, noon(2026, 6, 15))).toBe("2026-03-15");
    expect(cutoffDate(i, noon(2026, 1, 20))).toBe("2025-10-20");
  });

  it("clamps the day to the target month's length", () => {
    const i = DATE_STEPS.findIndex((s) => s.unit === "months" && s.n === 3);
    expect(cutoffDate(i, noon(2026, 5, 31))).toBe("2026-02-28");
    const year = DATE_STEPS.findIndex((s) => s.unit === "months" && s.n === 12);
    expect(cutoffDate(year, noon(2024, 2, 29))).toBe("2023-02-28");
  });

  it("clamps out-of-range positions instead of throwing", () => {
    expect(cutoffDate(-3, noon(2026, 6, 15))).toBe(cutoffDate(0, noon(2026, 6, 15)));
    expect(cutoffDate(999, noon(2026, 6, 15))).toBeNull();
  });
});

describe("dateStepLabel", () => {
  it("labels every step with a non-empty string", () => {
    setLocale("en");
    for (let i = 0; i < DATE_STEPS.length; i++) {
      expect(dateStepLabel(i)).not.toBe("");
    }
    expect(dateStepLabel(ALL_TIME)).toBe(t("filters.allTime"));
    expect(dateStepLabel(0)).toBe(t("filters.lastDays", { n: 7 }));
    const year = DATE_STEPS.findIndex((s) => s.unit === "months" && s.n === 12);
    expect(dateStepLabel(year)).toBe(t("filters.lastYear"));
  });
});

describe("filter i18n", () => {
  it("every filter string exists in every locale", () => {
    const keys = [
      "filters.label",
      "filters.starred",
      "filters.videos",
      "filters.date",
      "filters.allTime",
      "filters.lastDays",
      "filters.lastMonths",
      "filters.lastYear",
      "filters.lastYears",
      "filters.clearDate",
    ];
    for (const tag of locales) {
      setLocale(tag);
      for (const key of keys) expect(t(key), `${tag}:${key}`).not.toBe(key);
    }
    setLocale("en");
  });
});
