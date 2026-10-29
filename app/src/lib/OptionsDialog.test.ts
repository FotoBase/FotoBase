// OptionsDialog tests (TASK-01-022 base; TASK-01-024 adds the theme
// setting; TASK-01-025 adds file associations). IPC is mocked — the
// component's job is decisions and ordering.

import { flushSync } from "svelte";
import { cleanup, fireEvent, render, screen } from "@testing-library/svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import OptionsDialog from "./OptionsDialog.svelte";
import { t } from "./i18n.svelte.js";
import {
  DEFAULT_SETTINGS,
  registerAssociations,
  saveSettings,
  unregisterAssociations,
  type Settings,
} from "./settings";

vi.mock("./settings", async (importOriginal) => {
  const actual = await importOriginal<typeof import("./settings")>();
  return {
    ...actual,
    saveSettings: vi.fn(),
    registerAssociations: vi.fn(),
    unregisterAssociations: vi.fn(),
  };
});

afterEach(cleanup);

beforeEach(() => {
  vi.mocked(saveSettings).mockReset().mockResolvedValue(undefined);
  vi.mocked(registerAssociations).mockReset().mockResolvedValue(undefined);
  vi.mocked(unregisterAssociations).mockReset().mockResolvedValue(undefined);
});

const settle = async (): Promise<void> => {
  await flushSync();
  await new Promise((resolve) => setTimeout(resolve, 0));
};

describe("OptionsDialog theme (TASK-01-024)", () => {
  it("defaults to following the OS (system)", () => {
    render(OptionsDialog, {
      open: true,
      settings: { ...DEFAULT_SETTINGS },
      onchange: vi.fn(),
    });
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
    render(OptionsDialog, {
      open: true,
      settings: { ...DEFAULT_SETTINGS },
      onchange: vi.fn(),
    });
    const select = screen.getByTestId("theme-select") as HTMLSelectElement;
    const values = [...select.options].map((o) => o.value);
    expect(values).toEqual(["system", "light", "dark"]);
  });
});

describe("OptionsDialog ratings (TASK-03-005)", () => {
  it("is off by default (single star, ADR-0002)", () => {
    render(OptionsDialog, {
      open: true,
      settings: { ...DEFAULT_SETTINGS },
      onchange: vi.fn(),
    });
    expect(
      (screen.getByTestId("ratings-enabled") as HTMLInputElement).checked,
    ).toBe(false);
  });

  it("enabling 1-5 ratings applies and persists", async () => {
    const base: Settings = { ...DEFAULT_SETTINGS };
    const onchange = vi.fn();
    render(OptionsDialog, { open: true, settings: base, onchange });

    await fireEvent.click(screen.getByTestId("ratings-enabled"));
    await settle();

    expect(onchange).toHaveBeenCalledWith({ ...base, ratings_enabled: true });
    expect(saveSettings).toHaveBeenCalledWith({
      ...base,
      ratings_enabled: true,
    });
  });
});

describe("OptionsDialog quick tags (TASK-03-006)", () => {
  it("persists one tag per line, trimmed, capped at 10", async () => {
    const base: Settings = { ...DEFAULT_SETTINGS };
    const onchange = vi.fn();
    render(OptionsDialog, { open: true, settings: base, onchange });

    const input = screen.getByTestId("quick-tags-input") as HTMLTextAreaElement;
    const lines = ["sea", "  family ", "", "Vacation|2024"].concat(
      Array.from({ length: 9 }, (_, i) => `t${i}`),
    );
    await fireEvent.change(input, { target: { value: lines.join("\n") } });
    await settle();

    // Empty lines dropped, whitespace trimmed, at most 10 kept.
    const applied = vi.mocked(onchange).mock.calls.at(-1)![0] as Settings;
    expect(applied.quick_tags).toHaveLength(10);
    expect(applied.quick_tags[1]).toBe("family");
    expect(applied.quick_tags[2]).toBe("Vacation|2024");
    expect(saveSettings).toHaveBeenLastCalledWith(applied);
  });
});

describe("OptionsDialog file associations (TASK-01-025)", () => {
  it("lists the supported image types with checkboxes", () => {
    render(OptionsDialog, {
      open: true,
      settings: { ...DEFAULT_SETTINGS },
      onchange: vi.fn(),
    });
    for (const ext of ["jpg", "png", "heic"]) {
      const box = screen.getByTestId(`assoc-${ext}`) as HTMLInputElement;
      expect(box.type).toBe("checkbox");
    }
  });

  it("reflects the persisted association list as checked state", () => {
    const settings: Settings = {
      ...DEFAULT_SETTINGS,
      file_associations: ["jpg", "png"],
    };
    render(OptionsDialog, { open: true, settings, onchange: vi.fn() });
    expect((screen.getByTestId("assoc-jpg") as HTMLInputElement).checked).toBe(
      true,
    );
    expect((screen.getByTestId("assoc-png") as HTMLInputElement).checked).toBe(
      true,
    );
    expect((screen.getByTestId("assoc-gif") as HTMLInputElement).checked).toBe(
      false,
    );
  });

  it("checking a type registers it with the OS and persists the list", async () => {
    const base: Settings = { ...DEFAULT_SETTINGS };
    const onchange = vi.fn();
    render(OptionsDialog, { open: true, settings: base, onchange });

    fireEvent.click(screen.getByTestId("assoc-jpg"));
    await settle();

    expect(registerAssociations).toHaveBeenCalledWith(["jpg"]);
    expect(unregisterAssociations).not.toHaveBeenCalled();
    const next = { ...base, file_associations: ["jpg"] };
    expect(onchange).toHaveBeenCalledWith(next);
    expect(saveSettings).toHaveBeenCalledWith(next);
  });

  it("unchecking a type unregisters it and drops it from the list", async () => {
    const base: Settings = {
      ...DEFAULT_SETTINGS,
      file_associations: ["jpg", "png"],
    };
    const onchange = vi.fn();
    render(OptionsDialog, { open: true, settings: base, onchange });

    fireEvent.click(screen.getByTestId("assoc-jpg"));
    await settle();

    expect(unregisterAssociations).toHaveBeenCalledWith(["jpg"]);
    expect(registerAssociations).not.toHaveBeenCalled();
    const next = { ...base, file_associations: ["png"] };
    expect(onchange).toHaveBeenCalledWith(next);
    expect(saveSettings).toHaveBeenCalledWith(next);
  });

  it("a failed registration is reported, not swallowed", async () => {
    vi.mocked(registerAssociations).mockRejectedValue(new Error("boom"));
    const base: Settings = { ...DEFAULT_SETTINGS };
    const onchange = vi.fn();
    render(OptionsDialog, { open: true, settings: base, onchange });

    fireEvent.click(screen.getByTestId("assoc-jpg"));
    await settle();

    expect(screen.getByTestId("assoc-result").textContent).toContain("boom");
  });
});

describe("OptionsDialog hidden items and undo window (TASK-03-009, TASK-03-012)", () => {
  it("show hidden is off by default and toggling applies and persists", async () => {
    const base: Settings = { ...DEFAULT_SETTINGS };
    const onchange = vi.fn();
    render(OptionsDialog, { open: true, settings: base, onchange });
    expect((screen.getByTestId("show-hidden") as HTMLInputElement).checked).toBe(false);

    await fireEvent.click(screen.getByTestId("show-hidden"));
    await settle();

    expect(onchange).toHaveBeenCalledWith({ ...base, show_hidden: true });
    expect(saveSettings).toHaveBeenCalledWith({ ...base, show_hidden: true });
  });

  it("the RAW+JPEG pair window is configurable in whole seconds, zero allowed", async () => {
    const base: Settings = { ...DEFAULT_SETTINGS };
    const onchange = vi.fn();
    render(OptionsDialog, { open: true, settings: base, onchange });
    const input = screen.getByTestId("stack-window") as HTMLInputElement;
    expect(input.value).toBe("5");

    await fireEvent.change(input, { target: { value: "30" } });
    await settle();
    expect(onchange).toHaveBeenLastCalledWith({ ...base, stack_window_secs: 30 });

    await fireEvent.change(input, { target: { value: "0" } });
    await settle();
    expect(onchange).toHaveBeenLastCalledWith({ ...base, stack_window_secs: 0 });

    await fireEvent.change(input, { target: { value: "abc" } });
    await settle();
    expect(onchange).toHaveBeenLastCalledWith({ ...base, stack_window_secs: 5 });
  });

  it("the undo window is configurable in whole days, at least one", async () => {
    const base: Settings = { ...DEFAULT_SETTINGS };
    const onchange = vi.fn();
    render(OptionsDialog, { open: true, settings: base, onchange });
    const input = screen.getByTestId("trash-window") as HTMLInputElement;
    expect(input.value).toBe("30");

    await fireEvent.change(input, { target: { value: "7" } });
    await settle();
    expect(onchange).toHaveBeenLastCalledWith({ ...base, trash_window_days: 7 });

    await fireEvent.change(input, { target: { value: "0" } });
    await settle();
    expect(onchange).toHaveBeenLastCalledWith({ ...base, trash_window_days: 1 });

    await fireEvent.change(input, { target: { value: "abc" } });
    await settle();
    expect(onchange).toHaveBeenLastCalledWith({ ...base, trash_window_days: 30 });
  });
});

describe("OptionsDialog show hidden refresh (TASK-03-009)", () => {
  it("asks the shell to re-list once the setting is saved, and only then", async () => {
    const order: string[] = [];
    vi.mocked(saveSettings).mockImplementation(async () => {
      order.push("saved");
    });
    const onrevealchanged = vi.fn(() => order.push("refresh"));
    render(OptionsDialog, {
      open: true,
      settings: { ...DEFAULT_SETTINGS },
      onchange: vi.fn(),
      onrevealchanged,
    });

    await fireEvent.click(screen.getByTestId("ratings-enabled"));
    await settle();
    expect(onrevealchanged).not.toHaveBeenCalled();

    await fireEvent.click(screen.getByTestId("show-hidden"));
    await settle();
    expect(order.slice(-2)).toEqual(["saved", "refresh"]);
  });
});

describe("OptionsDialog single-click-to-edit (TASK-03-017)", () => {
  it("is off by default; toggling applies and persists", async () => {
    expect(DEFAULT_SETTINGS.single_click_edit).toBe(false);
    expect(t("options.singleClickEdit")).not.toBe("options.singleClickEdit");
    const base: Settings = { ...DEFAULT_SETTINGS };
    const onchange = vi.fn();
    render(OptionsDialog, { open: true, settings: base, onchange });
    expect((screen.getByTestId("single-click-edit") as HTMLInputElement).checked).toBe(false);

    await fireEvent.click(screen.getByTestId("single-click-edit"));
    await settle();

    expect(onchange).toHaveBeenCalledWith({ ...base, single_click_edit: true });
    expect(saveSettings).toHaveBeenCalledWith({ ...base, single_click_edit: true });
  });
});
