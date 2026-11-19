// TASK-08-002 tests: the heal-brush panel — paint strokes over the preview,
// adjustable brush size, editable stroke list with per-stroke delete, undo of
// the last stroke, and the state emissions the edit view persists.

import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/svelte";
import { afterEach, describe, expect, it, vi } from "vitest";

import RetouchPanel from "./RetouchPanel.svelte";
import { DEFAULT_RADIUS } from "./retouch";

// jsdom has no layout engine: give every element a fixed 100×50 box so the
// pointer math is deterministic.
Element.prototype.getBoundingClientRect = function () {
  return { left: 0, top: 0, width: 100, height: 50, right: 100, bottom: 50, x: 0, y: 0, toJSON() {} } as DOMRect;
};

afterEach(cleanup);

function renderPanel(extra: Record<string, unknown> = {}) {
  const onchange = vi.fn();
  render(RetouchPanel, { previewSrc: "preview://abc", onchange, ...extra });
  return onchange;
}

async function paint(points: Array<[number, number]>): Promise<void> {
  const stage = screen.getByTestId("retouch-stage");
  const [first, ...rest] = points;
  await fireEvent.pointerDown(stage, { clientX: first[0], clientY: first[1], pointerId: 1 });
  for (const [x, y] of rest) {
    await fireEvent.pointerMove(stage, { clientX: x, clientY: y, pointerId: 1 });
  }
  const last = points[points.length - 1];
  await fireEvent.pointerUp(stage, { clientX: last[0], clientY: last[1], pointerId: 1 });
}

describe("RetouchPanel", () => {
  it("announces the untouched state (no strokes → no op)", async () => {
    const onchange = renderPanel();
    await waitFor(() => expect(onchange).toHaveBeenCalled());
    expect(onchange).toHaveBeenLastCalledWith({ strokes: [] });
  });

  it("painting records one normalized stroke", async () => {
    const onchange = renderPanel();
    await paint([
      [20, 25],
      [30, 25],
      [40, 25],
    ]);
    const last = onchange.mock.calls.at(-1)![0];
    expect(last.strokes).toHaveLength(1);
    expect(last.strokes[0].points).toEqual([
      [0.2, 0.5],
      [0.3, 0.5],
      [0.4, 0.5],
    ]);
    expect(last.strokes[0].radius).toBe(DEFAULT_RADIUS);
  });

  it("a plain click records a one-point stroke", async () => {
    const onchange = renderPanel();
    await paint([[50, 10]]);
    expect(onchange.mock.calls.at(-1)![0].strokes).toEqual([
      { points: [[0.5, 0.2]], radius: DEFAULT_RADIUS },
    ]);
  });

  it("brush size applies to the next stroke", async () => {
    const onchange = renderPanel();
    await fireEvent.input(screen.getByTestId("retouch-size"), { target: { value: "0.08" } });
    await paint([[50, 25]]);
    expect(onchange.mock.calls.at(-1)![0].strokes[0].radius).toBeCloseTo(0.08);
  });

  it("lists strokes and deletes an individual one", async () => {
    const onchange = renderPanel();
    await paint([[10, 10]]);
    await paint([[50, 25]]);
    await paint([[90, 40]]);
    expect(screen.getAllByTestId(/^retouch-stroke-\d$/)).toHaveLength(3);

    await fireEvent.click(screen.getByTestId("retouch-delete-1"));
    const strokes = onchange.mock.calls.at(-1)![0].strokes;
    expect(strokes.map((s: { points: number[][] }) => s.points[0])).toEqual([
      [0.1, 0.2],
      [0.9, 0.8],
    ]);
    expect(screen.getAllByTestId(/^retouch-stroke-\d$/)).toHaveLength(2);
  });

  it("undo removes the last stroke; clear removes all", async () => {
    const onchange = renderPanel();
    await paint([[10, 10]]);
    await paint([[50, 25]]);
    await fireEvent.click(screen.getByTestId("retouch-undo"));
    expect(onchange.mock.calls.at(-1)![0].strokes).toHaveLength(1);
    await fireEvent.click(screen.getByTestId("retouch-clear"));
    expect(onchange.mock.calls.at(-1)![0].strokes).toEqual([]);
    expect((screen.getByTestId("retouch-undo") as HTMLButtonElement).disabled).toBe(true);
  });

  it("starts from the restored state", () => {
    renderPanel({ initial: { strokes: [{ points: [[0.3, 0.3]], radius: 0.04 }] } });
    expect(screen.getAllByTestId(/^retouch-stroke-\d$/)).toHaveLength(1);
  });
});
