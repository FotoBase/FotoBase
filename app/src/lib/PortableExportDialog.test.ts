// TASK-03-023 tests: portable export of user data (PLAN §5 anti-lock-in).
// The user picks the sections and the format and names a folder; the backend
// writes the files. IPC is mocked at the Tauri core module.

import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { setLocale } from "./i18n.svelte.js";
import PortableExportDialog from "./PortableExportDialog.svelte";
import { exportUserData, hasSection, type ExportReport } from "./portable-export";

const invoke = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...a: unknown[]) => invoke(...a),
}));

const report = (extra: Partial<ExportReport> = {}): ExportReport => ({
  files: ["/out/fotobase-export-20261029-160523.json"],
  albums: 2,
  tags: 5,
  recipes: 3,
  unreadable_recipes: 0,
  ...extra,
});

beforeEach(() => {
  setLocale("en");
  invoke.mockReset();
});
afterEach(cleanup);

describe("portable export IPC wrapper", () => {
  it("forwards the folder, sections and format", async () => {
    invoke.mockResolvedValueOnce(report());
    const sections = { albums: true, tags: false, recipes: true };

    const r = await exportUserData("/out", sections, "csv");

    expect(r.recipes).toBe(3);
    expect(invoke).toHaveBeenLastCalledWith("export_user_data", {
      dir: "/out",
      sections,
      format: "csv",
    });
  });

  it("hasSection is false only when nothing is picked", () => {
    expect(hasSection({ albums: false, tags: false, recipes: false })).toBe(false);
    expect(hasSection({ albums: false, tags: true, recipes: false })).toBe(true);
  });
});

describe("PortableExportDialog", () => {
  it("renders nothing when closed", () => {
    render(PortableExportDialog, { open: false });
    expect(screen.queryByRole("dialog")).toBeNull();
  });

  it("exports every section as JSON by default once a folder is named", async () => {
    invoke.mockResolvedValueOnce(report());
    render(PortableExportDialog, { open: true });
    const go = screen.getByTestId("export-run") as HTMLButtonElement;
    expect(go.disabled).toBe(true);

    await fireEvent.input(screen.getByTestId("export-folder"), {
      target: { value: " /out " },
    });
    expect(go.disabled).toBe(false);
    await fireEvent.click(go);

    await waitFor(() => expect(screen.getByTestId("export-result")).toBeTruthy());
    expect(invoke).toHaveBeenCalledWith("export_user_data", {
      dir: "/out",
      sections: { albums: true, tags: true, recipes: true },
      format: "json",
    });
    const result = screen.getByTestId("export-result").textContent ?? "";
    expect(result).toContain("2 albums");
    expect(result).toContain("5 tags");
    expect(result).toContain("3 recipes");
    expect(screen.getByTestId("export-file").textContent).toBe(
      "/out/fotobase-export-20261029-160523.json",
    );
  });

  it("sends only the picked sections and the picked format", async () => {
    invoke.mockResolvedValueOnce(report({ albums: 0, recipes: 0 }));
    render(PortableExportDialog, { open: true });

    await fireEvent.click(screen.getByTestId("export-section-albums"));
    await fireEvent.click(screen.getByTestId("export-section-recipes"));
    await fireEvent.click(screen.getByTestId("export-format-csv"));
    await fireEvent.input(screen.getByTestId("export-folder"), {
      target: { value: "/out" },
    });
    await fireEvent.click(screen.getByTestId("export-run"));

    await waitFor(() =>
      expect(invoke).toHaveBeenCalledWith("export_user_data", {
        dir: "/out",
        sections: { albums: false, tags: true, recipes: false },
        format: "csv",
      }),
    );
  });

  it("cannot export with no section picked", async () => {
    render(PortableExportDialog, { open: true });
    await fireEvent.input(screen.getByTestId("export-folder"), {
      target: { value: "/out" },
    });

    for (const s of ["albums", "tags", "recipes"]) {
      await fireEvent.click(screen.getByTestId(`export-section-${s}`));
    }

    expect((screen.getByTestId("export-run") as HTMLButtonElement).disabled).toBe(true);
  });

  it("warns about recipes kept raw in the JSON", async () => {
    invoke.mockResolvedValueOnce(report({ unreadable_recipes: 2 }));
    render(PortableExportDialog, { open: true });
    await fireEvent.input(screen.getByTestId("export-folder"), {
      target: { value: "/out" },
    });
    await fireEvent.click(screen.getByTestId("export-run"));

    await waitFor(() => expect(screen.getByTestId("export-unreadable")).toBeTruthy());
    expect(screen.getByTestId("export-unreadable").textContent).toContain("2");
  });

  it("shows a backend failure", async () => {
    invoke.mockRejectedValueOnce("export failed: not a folder: /nope");
    render(PortableExportDialog, { open: true });
    await fireEvent.input(screen.getByTestId("export-folder"), {
      target: { value: "/nope" },
    });
    await fireEvent.click(screen.getByTestId("export-run"));

    await waitFor(() => expect(screen.getByRole("alert").textContent).toContain("not a folder"));
  });

  it("closes", async () => {
    render(PortableExportDialog, { open: true });
    await fireEvent.click(screen.getByTestId("export-close"));
    expect(screen.queryByRole("dialog")).toBeNull();
  });
});
