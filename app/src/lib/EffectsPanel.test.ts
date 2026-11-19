// TASK-04-009 tests: the Effects tab panel — one-click effect buttons and
// strength sliders for the active effects. Emissions are recipe states; the
// pixel work is the shared Rust engine's (golden-tested).

import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import EffectsPanel from "./EffectsPanel.svelte";
import { DEFAULT_STRENGTH, type EffectsState } from "./effects";
import { setLocale } from "./i18n.svelte.js";

afterEach(cleanup);
beforeEach(() => setLocale("en"));

function props() {
  return { previewSrc: "preview://abc", onchange: vi.fn() };
}

describe("EffectsPanel", () => {
  it("renders a one-click button for each starter effect", () => {
    render(EffectsPanel, props());
    for (const name of ["Sharpen", "Sepia", "B&W", "Saturation"]) {
      expect(screen.getByRole("button", { name })).toBeTruthy();
    }
  });

  it("clicking an effect emits its op at the default strength", async () => {
    const onchange = vi.fn();
    render(EffectsPanel, { ...props(), onchange });
    await fireEvent.click(screen.getByRole("button", { name: "B&W" }));
    expect(onchange).toHaveBeenLastCalledWith({
      bw: DEFAULT_STRENGTH.bw,
    } satisfies Partial<EffectsState>);
  });

  it("clicking an active effect turns it off (one-click toggle)", async () => {
    const onchange = vi.fn();
    render(EffectsPanel, { ...props(), onchange });
    await fireEvent.click(screen.getByRole("button", { name: "Sepia" }));
    await fireEvent.click(screen.getByRole("button", { name: "Sepia" }));
    expect(onchange).toHaveBeenLastCalledWith({});
  });

  it("an active effect shows a strength slider that updates the op", async () => {
    const onchange = vi.fn();
    render(EffectsPanel, { ...props(), onchange });
    await fireEvent.click(screen.getByRole("button", { name: "Sharpen" }));
    const slider = screen.getByTestId("strength-sharpen") as HTMLInputElement;
    await fireEvent.input(slider, { target: { value: "0.9" } });
    expect(onchange).toHaveBeenLastCalledWith({ sharpen: 0.9 });
  });

  it("inactive effects expose no strength slider", () => {
    render(EffectsPanel, props());
    expect(screen.queryByTestId("strength-bw")).toBeNull();
  });

  it("effects combine: two active effects emit both ops", async () => {
    const onchange = vi.fn();
    render(EffectsPanel, { ...props(), onchange });
    await fireEvent.click(screen.getByRole("button", { name: "B&W" }));
    await fireEvent.click(screen.getByRole("button", { name: "Sharpen" }));
    expect(onchange).toHaveBeenLastCalledWith({
      bw: DEFAULT_STRENGTH.bw,
      sharpen: DEFAULT_STRENGTH.sharpen,
    });
  });

  // TASK-08-001
  it("renders buttons for the remaining effects and frames", () => {
    render(EffectsPanel, props());
    for (const name of [
      "Warmify",
      "Tint",
      "Film Grain",
      "Soft Focus",
      "Vignette",
      "Border",
      "Drop Shadow",
    ]) {
      expect(screen.getByRole("button", { name })).toBeTruthy();
    }
  });

  it("each new effect applies at its default and exposes a strength slider", async () => {
    const onchange = vi.fn();
    render(EffectsPanel, { ...props(), onchange });
    await fireEvent.click(screen.getByRole("button", { name: "Vignette" }));
    expect(onchange).toHaveBeenLastCalledWith({
      vignette: DEFAULT_STRENGTH.vignette,
    });
    const slider = screen.getByTestId("strength-vignette") as HTMLInputElement;
    await fireEvent.input(slider, { target: { value: "0.8" } });
    expect(onchange).toHaveBeenLastCalledWith({ vignette: 0.8 });
  });

  it("frames compose with photo effects in one state", async () => {
    const onchange = vi.fn();
    render(EffectsPanel, { ...props(), onchange });
    await fireEvent.click(screen.getByRole("button", { name: "Soft Focus" }));
    await fireEvent.click(screen.getByRole("button", { name: "Border" }));
    expect(onchange).toHaveBeenLastCalledWith({
      soft_focus: DEFAULT_STRENGTH.soft_focus,
      border: DEFAULT_STRENGTH.border,
    });
  });
});
