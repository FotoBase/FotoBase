// TASK-08-004: print preview geometry and IPC wrappers.

import { beforeEach, describe, expect, it, vi } from "vitest";

import {
  DEFAULT_OPTIONS,
  clampInt,
  fontStack,
  pageRectStyle,
  photoStyle,
  printPages,
  printPlan,
  type CellPlan,
} from "./print";

const invoke = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...a: unknown[]) => invoke(...a),
}));

beforeEach(() => invoke.mockReset());

function cell(over: Partial<CellPlan> = {}): CellPlan {
  const box = { x: 1, y: 2, w: 4, h: 6 };
  return {
    file_id: 1,
    cell: box,
    image_box: box,
    fit: { src: { x: 0, y: 0, w: 1, h: 1 }, dst: box },
    rotated: false,
    border: null,
    text: null,
    ...over,
  };
}

describe("preview geometry", () => {
  it("positions page rectangles as percentages of the page", () => {
    expect(pageRectStyle({ x: 0.25, y: 2.5, w: 4, h: 5.5 }, { width: 8.5, height: 11 })).toBe(
      "left:2.9412%;top:22.7273%;width:47.0588%;height:50%",
    );
  });

  it("fills the clip box when the whole photo shows", () => {
    expect(photoStyle(cell())).toBe("left:0%;top:0%;width:100%;height:100%");
  });

  it("offsets and enlarges the photo for a crop-to-fit source rectangle", () => {
    // Middle 50 % of the width visible: image is twice the box, shifted by 50 %.
    const c = cell({ fit: { src: { x: 0.25, y: 0, w: 0.5, h: 1 }, dst: cell().cell } });
    expect(photoStyle(c)).toBe("left:-50%;top:0%;width:200%;height:100%");
  });

  it("lays a rotated photo out unrotated and turns it 90° clockwise", () => {
    // Box 4×6 shows the whole rotated photo: the element is 6 wide × 4 tall,
    // centered in the box, then rotated.
    expect(photoStyle(cell({ rotated: true }))).toBe(
      "left:-25%;top:16.6667%;width:150%;height:66.6667%;transform:rotate(90deg)",
    );
  });
});

describe("helpers", () => {
  it("clamps integers and rejects non-numbers", () => {
    expect(clampInt(250, 1, 100)).toBe(100);
    expect(clampInt(0, 1, 99)).toBe(1);
    expect(clampInt(Number.NaN, 1, 99)).toBe(1);
    expect(clampInt(2.6, 1, 99)).toBe(3);
  });

  it("maps print fonts onto CSS stacks", () => {
    expect(fontStack("sans")).toBe("sans-serif");
    expect(fontStack("serif")).toBe("serif");
    expect(fontStack("mono")).toBe("monospace");
    expect(fontStack('Gill "Sans"')).toBe('"Gill Sans", sans-serif');
  });
});

describe("print IPC", () => {
  it("sends ids, copies and options as the backend request", async () => {
    invoke.mockResolvedValue({ pages: [], photos: [] });
    await printPlan([3, 1], 2, DEFAULT_OPTIONS);
    expect(invoke).toHaveBeenLastCalledWith("print_plan", {
      ids: [3, 1],
      request: { copies: 2, options: DEFAULT_OPTIONS },
    });

    invoke.mockResolvedValue({ files: [], failed: [] });
    await printPages([3], "/out", 1, DEFAULT_OPTIONS);
    expect(invoke).toHaveBeenLastCalledWith("print_pages", {
      ids: [3],
      dest: "/out",
      request: { copies: 1, options: DEFAULT_OPTIONS },
    });
  });
});
