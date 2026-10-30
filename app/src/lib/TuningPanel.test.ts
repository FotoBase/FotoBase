// TASK-04-008 tests: the Tuning tab panel — four sliders and the
// neutral-color picker. The picker's sample → params math lives in the Rust
// engine (`neutral_balance` command); tests stub the sampler and the
// command and check the wiring + emissions.

import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import TuningPanel from "./TuningPanel.svelte";
import { balanceFromSample } from "./tuning";
import { DEFAULT_TUNING, type TuningState } from "./tuning";
import { setLocale } from "./i18n.svelte.js";

vi.mock("./tuning", async (importOriginal) => ({
  ...(await importOriginal<typeof import("./tuning")>()),
  balanceFromSample: vi.fn(),
}));

afterEach(cleanup);
beforeEach(() => {
  setLocale("en");
  // Deterministic pointer math (same stub as RedEyePanel.test.ts).
  Element.prototype.getBoundingClientRect = function () {
    return {
      left: 0,
      top: 0,
      width: 100,
      height: 50,
      right: 100,
      bottom: 50,
      x: 0,
      y: 0,
      toJSON() {},
    } as DOMRect;
  };
});

function props() {
  return {
    previewSrc: "preview://abc",
    onchange: vi.fn(),
    // Deterministic sampler: a warm orange pixel everywhere.
    samplePixel: vi.fn().mockResolvedValue([0.9, 0.5, 0.2]),
  };
}

describe("TuningPanel", () => {
  it("default state emits no ops (untouched photo)", async () => {
    const onchange = vi.fn();
    render(TuningPanel, { ...props(), onchange });
    await waitFor(() => expect(onchange).toHaveBeenCalled());
    expect(onchange).toHaveBeenLastCalledWith(DEFAULT_TUNING);
  });

  it.each([
    ["fill-light", "fillLight", 0.4],
    ["highlights", "highlights", 0.6],
    ["shadows", "shadows", 0.3],
  ] as const)(
    "the %s slider sets %s and emits the state",
    async (testid, key, value) => {
      const onchange = vi.fn();
      render(TuningPanel, { ...props(), onchange });
      await waitFor(() => expect(onchange).toHaveBeenCalled());
      const slider = screen.getByTestId(testid) as HTMLInputElement;
      await fireEvent.input(slider, { target: { value: String(value) } });
      expect(onchange).toHaveBeenLastCalledWith(
        expect.objectContaining({ [key]: value }),
      );
    },
  );

  it("the temperature slider accepts negative values", async () => {
    const onchange = vi.fn();
    render(TuningPanel, { ...props(), onchange });
    await waitFor(() => expect(onchange).toHaveBeenCalled());
    const slider = screen.getByTestId("temperature") as HTMLInputElement;
    await fireEvent.input(slider, { target: { value: "-0.5" } });
    expect(onchange).toHaveBeenLastCalledWith(
      expect.objectContaining({ temperature: -0.5 }),
    );
  });

  it("slider input is clamped to the op's param range", async () => {
    const onchange = vi.fn();
    render(TuningPanel, { ...props(), onchange });
    await waitFor(() => expect(onchange).toHaveBeenCalled());
    const slider = screen.getByTestId("temperature") as HTMLInputElement;
    await fireEvent.input(slider, { target: { value: "3" } });
    expect(onchange).toHaveBeenLastCalledWith(
      expect.objectContaining({ temperature: 1 }),
    );
  });

  it("picker mode: clicking the preview samples a pixel and applies the balance", async () => {
    const onchange = vi.fn();
    const p = props();
    vi.mocked(balanceFromSample).mockResolvedValue({
      kelvinShift: -0.6,
      greenMagenta: -0.04,
    });
    render(TuningPanel, { ...p, onchange });
    await waitFor(() => expect(onchange).toHaveBeenCalled());

    await fireEvent.click(screen.getByTestId("neutral-picker"));
    // Armed: the stage shows the picker hint.
    expect(screen.getByTestId("hint").textContent).toContain("Click");

    const stage = screen.getByTestId("stage");
    // Click at the middle of the stage (normalized 0.5, 0.5).
    await fireEvent.click(stage, { clientX: 50, clientY: 25 });
    expect(p.samplePixel).toHaveBeenCalledWith(0.5, 0.5);
    await waitFor(() =>
      expect(balanceFromSample).toHaveBeenCalledWith([0.9, 0.5, 0.2]),
    );
    const last = onchange.mock.lastCall![0] as TuningState;
    expect(last.temperature).toBeCloseTo(-0.6);
    expect(last.tint).toBeCloseTo(-0.04);
    // Disarmed after the pick.
    expect(screen.getByTestId("neutral-picker").className).not.toContain(
      "selected",
    );
  });

  it("picker can be cancelled without changing the state", async () => {
    const onchange = vi.fn();
    render(TuningPanel, { ...props(), onchange });
    await waitFor(() => expect(onchange).toHaveBeenCalled());
    await fireEvent.click(screen.getByTestId("neutral-picker"));
    await fireEvent.click(screen.getByTestId("neutral-picker"));
    expect(onchange).toHaveBeenLastCalledWith(DEFAULT_TUNING);
  });
});
