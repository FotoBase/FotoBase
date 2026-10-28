// TASK-03-015: thumbnail caption modes — the pure mode → text resolver and
// the fixed caption-strip geometry.

import { describe, expect, it } from "vitest";
import {
  CAPTION_MODES,
  captionModeFromSettings,
  captionStripHeight,
  settingsWithCaptionMode,
  captionText,
  ellipsize,
  isCaptionMode,
} from "./captions";
import type { GridItem } from "./grid";
import { DEFAULT_SETTINGS } from "./settings";

const item: GridItem = {
  id: 1,
  filename: "IMG_0001.jpg",
  hash: null,
  caption: "Sunset at the pier",
  tags: ["beach", "Vacation|Italy"],
  width: 4000,
  height: 3000,
};

describe("captionText", () => {
  it("none prints nothing", () => {
    expect(captionText(item, "none")).toBe("");
  });

  it("filename prints the file name", () => {
    expect(captionText(item, "filename")).toBe("IMG_0001.jpg");
  });

  it("caption prints the photo caption, first line only", () => {
    expect(captionText(item, "caption")).toBe("Sunset at the pier");
    expect(captionText({ ...item, caption: "  first\nsecond " }, "caption")).toBe("first");
  });

  it("caption is empty for an uncaptioned photo", () => {
    expect(captionText({ ...item, caption: null }, "caption")).toBe("");
    expect(captionText({ ...item, caption: undefined }, "caption")).toBe("");
  });

  it("tags prints a compact list: hierarchy leaves, comma separated", () => {
    expect(captionText(item, "tags")).toBe("beach, Italy");
  });

  it("tags is empty without tags (or on an older payload)", () => {
    expect(captionText({ ...item, tags: [] }, "tags")).toBe("");
    expect(captionText({ ...item, tags: undefined }, "tags")).toBe("");
  });

  it("resolution prints width × height", () => {
    expect(captionText(item, "resolution")).toBe("4000 × 3000");
  });

  it("resolution is empty until dimensions are known", () => {
    expect(captionText({ ...item, width: null, height: 3000 }, "resolution")).toBe("");
    expect(captionText({ ...item, width: undefined, height: undefined }, "resolution")).toBe("");
  });
});

describe("caption modes", () => {
  it("lists all five modes, none first", () => {
    expect(CAPTION_MODES.map((m) => m.value)).toEqual([
      "none",
      "filename",
      "caption",
      "tags",
      "resolution",
    ]);
  });

  it("isCaptionMode guards unknown persisted values", () => {
    expect(isCaptionMode("tags")).toBe(true);
    expect(isCaptionMode("bogus")).toBe(false);
    expect(isCaptionMode(undefined)).toBe(false);
  });
});

describe("captionStripHeight", () => {
  it("is zero for none, so the grid keeps its original rhythm", () => {
    expect(captionStripHeight("none", 160)).toBe(0);
  });

  it("is constant for every mode at a given zoom level (no layout thrash)", () => {
    for (const size of [64, 160, 512]) {
      const heights = new Set(
        (["filename", "caption", "tags", "resolution"] as const).map((m) =>
          captionStripHeight(m, size),
        ),
      );
      expect(heights.size).toBe(1);
      expect([...heights][0]).toBeGreaterThan(0);
    }
  });

  it("grows with the zoom level", () => {
    expect(captionStripHeight("filename", 512)).toBeGreaterThan(
      captionStripHeight("filename", 64),
    );
  });
});

describe("ellipsize (canvas truncation)", () => {
  const measure = (s: string) => s.length * 10;

  it("returns text that fits unchanged", () => {
    expect(ellipsize("abc", 100, measure)).toBe("abc");
  });

  it("truncates with an ellipsis so the result fits", () => {
    const out = ellipsize("abcdefghijklmnop", 80, measure);
    expect(out.endsWith("…")).toBe(true);
    expect(measure(out)).toBeLessThanOrEqual(80);
    expect(out.length).toBeGreaterThan(1);
  });

  it("returns empty when not even the ellipsis fits", () => {
    expect(ellipsize("abcdef", 5, measure)).toBe("");
  });

  it("returns empty for empty text", () => {
    expect(ellipsize("", 100, measure)).toBe("");
  });
});

describe("caption mode persistence helpers", () => {
  it("restores the persisted mode on startup", () => {
    expect(captionModeFromSettings({ ...DEFAULT_SETTINGS, caption_mode: "tags" })).toBe("tags");
  });

  it("degrades an unknown persisted value to none", () => {
    const bogus = { ...DEFAULT_SETTINGS, caption_mode: "sparkles" } as unknown as typeof DEFAULT_SETTINGS;
    expect(captionModeFromSettings(bogus)).toBe("none");
  });

  it("builds the settings to save only when the mode changed", () => {
    expect(settingsWithCaptionMode(DEFAULT_SETTINGS, "none")).toBeNull();
    expect(settingsWithCaptionMode(DEFAULT_SETTINGS, "filename")).toEqual({
      ...DEFAULT_SETTINGS,
      caption_mode: "filename",
    });
  });
});
