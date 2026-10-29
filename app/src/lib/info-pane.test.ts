// TASK-01-020 tests: the info pane. Three layers in one file (matching the
// viewer.test.ts pattern):
//
// 1. Pure model — EXIF/file-property formatting and histogram binning over
//    RGBA bytes (spec: "downsampled bins are fine", computed in the
//    frontend from the rendered preview — no full-res IPC trips).
// 2. The InfoPane component — shows the fixture EXIF payload + file
//    properties, swaps when the photo changes.
// 3. Viewer integration — View → Show image properties toggle (`i`), pane
//    content follows the current photo.

import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/svelte";
import { flushSync } from "svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import InfoPane from "./InfoPane.svelte";
import Viewer from "./Viewer.svelte";
import { formatBytes, formatDimensions, formatDate, formatExposure, histogram, loadPhotoInfo } from "./info-pane";
import type { GridItem } from "./grid";

// --- Pure model -------------------------------------------------------------

describe("info pane model", () => {
  it("formats a full EXIF exposure triplet", () => {
    expect(formatExposure({ f_number: 2.8, iso: 100, exposure: "1/250" })).toBe(
      "f/2.8 · 1/250 s · ISO 100",
    );
  });

  it("skips missing EXIF parts instead of rendering blanks", () => {
    expect(formatExposure({ f_number: 4, iso: null, exposure: null })).toBe("f/4");
    expect(formatExposure({ f_number: null, iso: 800, exposure: "1/60" })).toBe("1/60 s · ISO 800");
  });

  it("returns null when no EXIF part is present", () => {
    expect(formatExposure({ f_number: null, iso: null, exposure: null })).toBeNull();
  });

  it("formats file sizes in binary units", () => {
    expect(formatBytes(0)).toBe("0 B");
    expect(formatBytes(512)).toBe("512 B");
    expect(formatBytes(1536)).toBe("1.5 KB");
    expect(formatBytes(1_234_567)).toBe("1.2 MB");
    expect(formatBytes(2 * 1024 ** 3)).toBe("2.0 GB");
  });

  it("formats pixel dimensions", () => {
    expect(formatDimensions(6000, 4000)).toBe("6000 × 4000");
    expect(formatDimensions(null, 4000)).toBeNull();
    expect(formatDimensions(6000, null)).toBeNull();
  });

  it("formats the modified date deterministically (UTC)", () => {
    // 2025-10-09 08:13:20 UTC
    expect(formatDate(1_759_997_600)).toBe("2025-10-09 08:13");
  });

  it("bins a solid-color image into its channel", () => {
    // 2×2 solid red, RGBA. Red is at full scale — the last bin, not the first.
    const px = new Uint8ClampedArray([255, 0, 0, 255, 255, 0, 0, 255, 255, 0, 0, 255, 255, 0, 0, 255]);
    const h = histogram(px, 8);
    expect(h.r).toEqual([0, 0, 0, 0, 0, 0, 0, 4]);
    // Zero-valued channels land in bin 0 — black is a real color.
    expect(h.g).toEqual([4, 0, 0, 0, 0, 0, 0, 0]);
    expect(h.b).toEqual([4, 0, 0, 0, 0, 0, 0, 0]);
  });

  it("bins a gray gradient across the whole range and downsamples", () => {
    // 256 gray pixels (1×256), values 0..255 — every of 8 bins gets 32.
    const px = new Uint8ClampedArray(256 * 4);
    for (let i = 0; i < 256; i++) px.set([i, i, i, 255], i * 4);
    const h = histogram(px, 8);
    expect(h.r).toEqual([32, 32, 32, 32, 32, 32, 32, 32]);
    expect(h.g).toEqual(h.r);
    expect(h.b).toEqual(h.r);
  });

  it("never counts the alpha channel", () => {
    // Transparent black pixels must not light up any color bin.
    const px = new Uint8ClampedArray([0, 0, 0, 0, 0, 0, 0, 0]);
    const h = histogram(px, 8);
    expect(h.r.every((n) => n === 0)).toBe(true);
    expect(h.g.every((n) => n === 0)).toBe(true);
    expect(h.b.every((n) => n === 0)).toBe(true);
  });
});

// --- Backend payload loading ------------------------------------------------

const PHOTO_A = {
  id: 1,
  path: "/photos/a.jpg",
  filename: "a.jpg",
  size: 1_234_567,
  mtime: 1_759_997_600,
  width: 6000,
  height: 4000,
  media_type: "image",
  camera: "Canon EOS R5",
  lens: "RF 24-70mm F2.8",
  exposure: "1/250",
  f_number: 2.8,
  iso: 100,
  focal_length: 24,
  taken_at: null,
};

const PHOTO_B = { ...PHOTO_A, id: 2, path: "/photos/b.jpg", filename: "b.jpg", camera: "Nikon Z8" };

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
import { invoke } from "@tauri-apps/api/core";

beforeEach(() => {
  vi.mocked(invoke).mockReset();
});

afterEach(() => {
  cleanup();
  vi.useRealTimers();
});

describe("loadPhotoInfo", () => {
  it("round-trips through the photo_info command", async () => {
    vi.mocked(invoke).mockResolvedValue(PHOTO_A);
    const info = await loadPhotoInfo(1);
    expect(invoke).toHaveBeenCalledWith("photo_info", { fileId: 1 });
    expect(info?.camera).toBe("Canon EOS R5");
    expect(info?.size).toBe(1_234_567);
  });

  it("returns null when the backend is missing (browser dev)", async () => {
    vi.mocked(invoke).mockRejectedValue("no backend");
    expect(await loadPhotoInfo(1)).toBeNull();
  });
});

// --- Component --------------------------------------------------------------

function renderPane(photo: GridItem | null) {
  const result = render(InfoPane, { photo });
  flushSync();
  return result;
}

describe("InfoPane", () => {
  it("shows EXIF and file properties from the backend payload", async () => {
    vi.mocked(invoke).mockResolvedValue(PHOTO_A);
    renderPane({ id: 1, filename: "a.jpg", hash: "a".repeat(32) });

    await waitFor(() => expect(screen.getByTestId("info-camera").textContent).toBe("Canon EOS R5"));
    expect(screen.getByTestId("info-lens").textContent).toBe("RF 24-70mm F2.8");
    expect(screen.getByTestId("info-exposure").textContent).toBe("f/2.8 · 1/250 s · ISO 100");
    expect(screen.getByTestId("info-path").textContent).toBe("/photos/a.jpg");
    expect(screen.getByTestId("info-size").textContent).toBe("1.2 MB");
    expect(screen.getByTestId("info-dimensions").textContent).toBe("6000 × 4000");
    expect(screen.getByTestId("info-format").textContent).toBe("JPEG");
    expect(screen.getByTestId("info-modified").textContent).toBe("2025-10-09 08:13");
  });

  it("swaps EXIF when the photo changes (fixture payload per photo)", async () => {
    vi.mocked(invoke).mockImplementation((_cmd, args: { fileId: number }) =>
      Promise.resolve(args.fileId === 1 ? PHOTO_A : PHOTO_B),
    );
    const { rerender } = renderPane({ id: 1, filename: "a.jpg", hash: "a".repeat(32) });
    await waitFor(() => expect(screen.getByTestId("info-camera").textContent).toBe("Canon EOS R5"));

    rerender({ photo: { id: 2, filename: "b.jpg", hash: "b".repeat(32) } });
    flushSync();
    await waitFor(() => expect(screen.getByTestId("info-camera").textContent).toBe("Nikon Z8"));
  });

  it("shows placeholders when the backend has no EXIF for the photo", async () => {
    vi.mocked(invoke).mockResolvedValue({ ...PHOTO_A, camera: null, lens: null, f_number: null, iso: null });
    renderPane({ id: 1, filename: "a.jpg", hash: "a".repeat(32) });

    await waitFor(() => expect(screen.getByTestId("info-camera").textContent).toBe("—"));
    expect(screen.getByTestId("info-exposure").textContent).toBe("—");
  });

  it("shows placeholders when there is no backend at all", async () => {
    vi.mocked(invoke).mockRejectedValue("no backend");
    renderPane({ id: 1, filename: "a.jpg", hash: "a".repeat(32) });
    await waitFor(() => expect(screen.getByTestId("info-camera").textContent).toBe("—"));
  });

  // TASK-03-019: capture time + the date/time adjustment entry point.
  it("shows the capture time, or a dash when none is known", async () => {
    vi.mocked(invoke).mockResolvedValue({ ...PHOTO_A, taken_at: "2024-06-01T12:00:00+02:00" });
    renderPane({ id: 1, filename: "a.jpg", hash: "a".repeat(32) });
    await waitFor(() =>
      expect(screen.getByTestId("info-captured").textContent?.trim()).toBe(
        "2024-06-01 12:00:00 +02:00",
      ),
    );
    cleanup();

    vi.mocked(invoke).mockResolvedValue(PHOTO_A);
    renderPane({ id: 1, filename: "a.jpg", hash: "a".repeat(32) });
    await waitFor(() => expect(screen.getByTestId("info-camera").textContent).toBe("Canon EOS R5"));
    expect(screen.getByTestId("info-captured").textContent?.trim()).toBe("—");
  });

  it("opens the date/time dialog for the shown photo and re-reads the info after applying", async () => {
    let taken: string | null = "2024-06-01T12:00:00+02:00";
    vi.mocked(invoke).mockImplementation(async (cmd: string) => {
      if (cmd === "photo_info") return { ...PHOTO_A, taken_at: taken };
      if (cmd === "preview_datetime_adjustment") return [];
      if (cmd === "adjust_datetime") {
        taken = "2024-06-01T10:00:00+02:00";
        return { adjusted: 1, failed: [], cancelled: false };
      }
      return null;
    });
    renderPane({ id: 1, filename: "a.jpg", hash: "a".repeat(32) });
    await waitFor(() =>
      expect(screen.getByTestId("info-captured").textContent?.trim()).toBe(
        "2024-06-01 12:00:00 +02:00",
      ),
    );

    await fireEvent.click(screen.getByTestId("info-adjust-time"));
    await fireEvent.change(screen.getByTestId("dt-direction"), { target: { value: "earlier" } });
    await fireEvent.input(screen.getByTestId("dt-hours"), { target: { value: "2" } });
    await fireEvent.click(screen.getByTestId("dt-apply"));

    await waitFor(() =>
      expect(vi.mocked(invoke)).toHaveBeenCalledWith("adjust_datetime", {
        ids: [1],
        adjustment: { kind: "shift", seconds: -7200 },
      }),
    );
    await waitFor(() =>
      expect(screen.getByTestId("info-captured").textContent?.trim()).toBe(
        "2024-06-01 10:00:00 +02:00",
      ),
    );
  });

  it("disables Adjust without a photo", () => {
    renderPane(null);
    expect((screen.getByTestId("info-adjust-time") as HTMLButtonElement).disabled).toBe(true);
  });
});

// --- Viewer integration -----------------------------------------------------

const HASH = (n: number) => n.toString(16).padStart(32, "0");

function makeItems(count: number): GridItem[] {
  return Array.from({ length: count }, (_, i) => ({
    id: i + 1,
    filename: `photo-${i}.jpg`,
    hash: HASH(i),
  }));
}

function pressKey(key: string): void {
  fireEvent.keyDown(window, { key });
  flushSync();
}

describe("Viewer info pane", () => {
  it("toggles via the `i` key and the status-bar button", async () => {
    vi.mocked(invoke).mockResolvedValue(PHOTO_A);
    render(Viewer, { items: makeItems(2) });
    flushSync();

    expect(screen.queryByTestId("info-pane")).toBeNull();

    // Status-bar button.
    fireEvent.click(screen.getByTestId("toggle-info"));
    flushSync();
    expect(screen.getByTestId("info-pane")).toBeDefined();
    await waitFor(() => expect(screen.getByTestId("info-camera").textContent).toBe("Canon EOS R5"));

    // Same key closes it again.
    pressKey("i");
    expect(screen.queryByTestId("info-pane")).toBeNull();
  });

  it("pane content follows the current photo as the viewer navigates", async () => {
    vi.mocked(invoke).mockImplementation((_cmd, args: { fileId: number }) =>
      Promise.resolve(args.fileId === 1 ? PHOTO_A : { ...PHOTO_B, id: 2, camera: "Sony A7 IV" }),
    );
    render(Viewer, { items: makeItems(2), startIndex: 0 });
    flushSync();

    fireEvent.click(screen.getByTestId("toggle-info"));
    flushSync();
    await waitFor(() => expect(screen.getByTestId("info-camera").textContent).toBe("Canon EOS R5"));

    pressKey("ArrowRight");
    await waitFor(() => expect(screen.getByTestId("info-camera").textContent).toBe("Sony A7 IV"));
    expect(screen.getByTestId("info-path").textContent).toBe("/photos/b.jpg");
  });
});
