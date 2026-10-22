// OptionsDialog tests (TASK-01-022 base; TASK-01-024 adds the theme
// setting). IPC is mocked — the component's job is decisions and ordering.

import { flushSync } from "svelte";
import { cleanup, fireEvent, render, screen } from "@testing-library/svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import OptionsDialog from "./OptionsDialog.svelte";
import { DEFAULT_SETTINGS, saveSettings, type Settings } from "./settings";

vi.mock("./settings", async (importOriginal) => {
  const actual = await importOriginal<typeof import("./settings")>();
  return { ...actual, saveSettings: vi.fn() };
});

afterEach(cleanup);

beforeEach(() => {
  vi.mocked(saveSettings).mockReset().mockResolvedValue(undefined);
});

const settle = async (): Promise<void> => {
  await flushSync();
  await new Promise((resolve) => setTimeout(resolve, 0));
};

describe("OptionsDialog theme (TASK-01-024)", () => {
  it("defaults to following the OS (system)", () => {
    render(OptionsDialog, { open: true, settings: { ...DEFAULT_SETTINGS }, onchange: vi.fn() });
    const select = screen.getByTestId("theme-select") as HTMLSelectElement;
    expect(select.value).toBe("system");
  });

  it("applies and persists an explicit theme choice", async () => {
    const base: Settings = { ...DEFAULT_SETTINGS };
    const onchange = vi.fn();
    render(OptionsDialog, { open: true, settings: base, onchange });

    const select = screen.getByTestId("theme-select") as HTMLSelectElement;
    fireEvent.change(select, { target: { value: "light" } });
    await settle();

    expect(onchange).toHaveBeenCalledWith({ ...base, theme: "light" });
    expect(saveSettings).toHaveBeenCalledWith({ ...base, theme: "light" });
  });

  it("offers system, light, and dark choices", () => {
    render(OptionsDialog, { open: true, settings: { ...DEFAULT_SETTINGS }, onchange: vi.fn() });
    const select = screen.getByTestId("theme-select") as HTMLSelectElement;
    const values = [...select.options].map((o) => o.value);
    expect(values).toEqual(["system", "light", "dark"]);
  });
});
