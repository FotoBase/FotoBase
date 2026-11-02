// TASK-04-015 tests: the export dialog — resize limits (slider + input),
// quality presets (Auto/Normal/Maximum/Minimum/Custom %), numbered file
// names, watermark text; the backend bakes the edits. IPC is mocked at the
// Tauri core module.

import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import ExportDialog from "./ExportDialog.svelte";
import { PRESET_QUALITY, exportPhotos, watermarkLine } from "./export-photos";
import { setLocale } from "./i18n.svelte.js";

const invoke = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...a: unknown[]) => invoke(...a),
}));

beforeEach(() => {
  setLocale("en");
  invoke.mockReset();
  invoke.mockResolvedValue({ exported: [], failed: [] });
});
afterEach(cleanup);

describe("export IPC wrapper", () => {
  it("maps dialog options onto the backend request", async () => {
    await exportPhotos([7, 3], "/out", {
      resizeMaxWidth: 1024,
      resizeMaxHeight: null,
      quality: 85,
      numbered: true,
      watermark: "A · b.example",
    });

    expect(invoke).toHaveBeenLastCalledWith("export_photos", {
      ids: [7, 3],
      dest: "/out",
      request: {
        resize_max_width: 1024,
        resize_max_height: null,
        quality: 85,
        numbered: true,
        watermark: "A · b.example",
      },
    });
  });

  it("resolves the §2J preset table", () => {
    expect(PRESET_QUALITY.auto).toBe(100);
    expect(PRESET_QUALITY.maximum).toBe(100);
    expect(PRESET_QUALITY.normal).toBe(85);
    expect(PRESET_QUALITY.minimum).toBe(50);
  });

  it("joins only the filled watermark parts", () => {
    expect(watermarkLine("A", "b.example", "© 2026")).toBe(
      "A · b.example · © 2026",
    );
    expect(watermarkLine("  A  ", "", "© 2026")).toBe("A · © 2026");
    expect(watermarkLine("", "", "")).toBe("");
  });
});

describe("ExportDialog", () => {
  it("renders nothing when closed", () => {
    render(ExportDialog, { open: false, ids: [1] });
    expect(screen.queryByRole("dialog")).toBeNull();
  });

  it("exports original size, Auto quality by default", async () => {
    render(ExportDialog, { open: true, ids: [1, 2] });
    await fireEvent.input(screen.getByTestId("imgexport-folder"), {
      target: { value: "/out" },
    });
    await fireEvent.click(screen.getByTestId("imgexport-run"));

    await waitFor(() =>
      expect(invoke).toHaveBeenCalledWith("export_photos", {
        ids: [1, 2],
        dest: "/out",
        request: {
          resize_max_width: null,
          resize_max_height: null,
          quality: 100,
          numbered: false,
          watermark: null,
        },
      }),
    );
  });

  it("sends the custom horizontal/vertical limits when custom size is picked", async () => {
    render(ExportDialog, { open: true, ids: [5] });
    await fireEvent.input(screen.getByTestId("imgexport-folder"), {
      target: { value: "/out" },
    });
    await fireEvent.click(screen.getByText("Custom resize"));
    await fireEvent.input(screen.getByTestId("imgexport-limit-h"), {
      target: { value: "1024" },
    });
    await fireEvent.input(screen.getByTestId("imgexport-limit-v"), {
      target: { value: "768" },
    });
    await fireEvent.click(screen.getByTestId("imgexport-run"));

    await waitFor(() => expect(invoke).toHaveBeenCalled());
    const request = invoke.mock.calls[0][1].request;
    expect(request.resize_max_width).toBe(1024);
    expect(request.resize_max_height).toBe(768);
  });

  it("sends the Custom preset's slider quality", async () => {
    render(ExportDialog, { open: true, ids: [5] });
    await fireEvent.input(screen.getByTestId("imgexport-folder"), {
      target: { value: "/out" },
    });
    await fireEvent.click(screen.getByTestId("imgexport-quality-custom"));
    await fireEvent.input(screen.getByTestId("imgexport-quality-slider"), {
      target: { value: "42" },
    });
    await fireEvent.click(screen.getByTestId("imgexport-run"));

    await waitFor(() => expect(invoke).toHaveBeenCalled());
    expect(invoke.mock.calls[0][1].request.quality).toBe(42);
  });

  it("sends numbered file names and the watermark line when enabled", async () => {
    render(ExportDialog, { open: true, ids: [5] });
    await fireEvent.input(screen.getByTestId("imgexport-folder"), {
      target: { value: "/out" },
    });
    await fireEvent.click(screen.getByTestId("imgexport-numbered"));
    await fireEvent.click(screen.getByTestId("imgexport-watermark"));
    await fireEvent.input(screen.getByTestId("imgexport-watermark-name"), {
      target: { value: "Ada" },
    });
    await fireEvent.input(screen.getByTestId("imgexport-watermark-domain"), {
      target: { value: "ada.example" },
    });
    await fireEvent.click(screen.getByTestId("imgexport-run"));

    await waitFor(() => expect(invoke).toHaveBeenCalled());
    const request = invoke.mock.calls[0][1].request;
    expect(request.numbered).toBe(true);
    expect(request.watermark).toBe("Ada · ada.example");
  });

  it("shows the partial-failure notice when the backend reports failures", async () => {
    invoke.mockResolvedValueOnce({
      exported: [{ file_id: 1, path: "/out/a.jpg" }],
      failed: [{ file_id: 2, error: "boom" }],
    });
    render(ExportDialog, { open: true, ids: [1, 2] });
    await fireEvent.input(screen.getByTestId("imgexport-folder"), {
      target: { value: "/out" },
    });
    await fireEvent.click(screen.getByTestId("imgexport-run"));

    await waitFor(() =>
      expect(screen.getByTestId("imgexport-error").textContent).toContain("1"),
    );
  });
});
