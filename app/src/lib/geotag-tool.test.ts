// Geotag tool frontend logic (TASK-06-006): coordinate validation, target
// construction and report formatting. Pure logic, no DOM.

import { describe, expect, it, vi } from "vitest";
import {
  GeotagTargetScope,
  formatGeotagReport,
  geotagTarget,
  parseLatitude,
  parseLongitude,
  type GeotagToolReport,
} from "./geotag-tool";

describe("parseLatitude", () => {
  it("accepts decimal degrees inside ±90", () => {
    expect(parseLatitude("52.52")).toBeCloseTo(52.52);
    expect(parseLatitude(" -90 ")).toBe(-90);
    expect(parseLatitude("+0")).toBe(0);
  });

  it("rejects blanks, junk and out-of-range values", () => {
    expect(parseLatitude("")).toBeNull();
    expect(parseLatitude("north")).toBeNull();
    expect(parseLatitude("90.0001")).toBeNull();
    expect(parseLatitude("NaN")).toBeNull();
  });
});

describe("parseLongitude", () => {
  it("accepts decimal degrees inside ±180", () => {
    expect(parseLongitude("-13.405")).toBeCloseTo(-13.405);
    expect(parseLongitude("180")).toBe(180);
  });

  it("rejects out-of-range values", () => {
    expect(parseLongitude("180.5")).toBeNull();
    expect(parseLongitude("")).toBeNull();
  });
});

describe("geotagTarget", () => {
  it("sends the selection ids as an ids target", () => {
    expect(geotagTarget(GeotagTargetScope.Selection, [3, 1], 7, 8)).toEqual({
      kind: "ids",
      ids: [3, 1],
    });
  });

  it("sends the folder id for the whole-folder scope", () => {
    expect(geotagTarget(GeotagTargetScope.Folder, [1, 2], 7, 8)).toEqual({
      kind: "folder",
      folder_id: 7,
    });
  });

  it("sends the album id for the whole-album scope", () => {
    expect(geotagTarget(GeotagTargetScope.Album, [1, 2], 7, 8)).toEqual({
      kind: "album",
      album_id: 8,
    });
  });

  it("falls back to the selection when there is no folder or album", () => {
    expect(geotagTarget(GeotagTargetScope.Folder, [1], null, null)).toEqual({ kind: "ids", ids: [1] });
    expect(geotagTarget(GeotagTargetScope.Album, [1], 7, null)).toEqual({ kind: "ids", ids: [1] });
  });
});

describe("formatGeotagReport", () => {
  const base: GeotagToolReport = { applied: 0, failed: [], cancelled: false };

  it("counts applied photos", () => {
    expect(formatGeotagReport({ ...base, applied: 4 })).toBe("done:4");
  });

  it("mentions failures", () => {
    expect(formatGeotagReport({ ...base, applied: 3, failed: [{ id: 9, error: "boom" }] })).toBe(
      "done:3 failed:1",
    );
  });

  it("mentions cancellation", () => {
    expect(formatGeotagReport({ ...base, applied: 2, cancelled: true })).toBe("done:2 cancelled");
  });
});
