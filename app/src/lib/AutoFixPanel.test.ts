// TASK-04-007 tests: the Basic Fixes one-click auto-fix panel. Three
// buttons (I'm Feeling Lucky, Auto Contrast, Auto Color) and the change
// emissions the edit view persists.

import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/svelte";
import { afterEach, describe, expect, it, vi } from "vitest";

import AutoFixPanel from "./AutoFixPanel.svelte";

afterEach(cleanup);

function props() {
  return {
    previewSrc: "preview://abc",
    onchange: vi.fn(),
  };
}

describe("AutoFixPanel", () => {
  it("default state emits no ops (untouched photo)", async () => {
    const onchange = vi.fn();
    render(AutoFixPanel, { ...props(), onchange });
    await waitFor(() => expect(onchange).toHaveBeenCalled());
    expect(onchange).toHaveBeenLastCalledWith({ autoColor: false, autoContrast: false });
  });

  it("Auto Color click toggles the auto_color op", async () => {
    const onchange = vi.fn();
    render(AutoFixPanel, { ...props(), onchange });
    await fireEvent.click(screen.getByTestId("auto-color"));
    expect(onchange).toHaveBeenLastCalledWith({ autoColor: true, autoContrast: false });
    await fireEvent.click(screen.getByTestId("auto-color"));
    expect(onchange).toHaveBeenLastCalledWith({ autoColor: false, autoContrast: false });
  });

  it("Auto Contrast click toggles the auto_contrast op", async () => {
    const onchange = vi.fn();
    render(AutoFixPanel, { ...props(), onchange });
    await fireEvent.click(screen.getByTestId("auto-contrast"));
    expect(onchange).toHaveBeenLastCalledWith({ autoColor: false, autoContrast: true });
  });

  it("I'm Feeling Lucky enables both passes in one click", async () => {
    const onchange = vi.fn();
    render(AutoFixPanel, { ...props(), onchange });
    await fireEvent.click(screen.getByTestId("feeling-lucky"));
    expect(onchange).toHaveBeenLastCalledWith({ autoColor: true, autoContrast: true });
    // Selected buttons reflect the state.
    expect(screen.getByTestId("feeling-lucky").className).toContain("selected");
    expect(screen.getByTestId("auto-color").className).toContain("selected");
    expect(screen.getByTestId("auto-contrast").className).toContain("selected");
  });

  it("Clear resets to no ops", async () => {
    const onchange = vi.fn();
    render(AutoFixPanel, { ...props(), onchange });
    await fireEvent.click(screen.getByTestId("feeling-lucky"));
    await fireEvent.click(screen.getByTestId("clear"));
    expect(onchange).toHaveBeenLastCalledWith({ autoColor: false, autoContrast: false });
  });

  it("individual toggles turn off when both are on (unlucky again)", async () => {
    const onchange = vi.fn();
    render(AutoFixPanel, { ...props(), onchange });
    await fireEvent.click(screen.getByTestId("feeling-lucky"));
    await fireEvent.click(screen.getByTestId("auto-contrast"));
    expect(onchange).toHaveBeenLastCalledWith({ autoColor: true, autoContrast: false });
  });
});
