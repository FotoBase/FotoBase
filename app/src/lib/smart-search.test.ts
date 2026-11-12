// TASK-07-001: the opt-in consent rules for the CLIP model download.
// Pure logic only — the invoke seam is stubbed so no backend is needed.

import { describe, expect, it, vi } from "vitest";

const invoke = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({ invoke }));

import {
  clipModelStatus,
  downloadClipModel,
  mayDownload,
  modelSizeLabel,
  type ClipModelStatus,
} from "./smart-search";

describe("smart search model opt-in", () => {
  it("mayDownload requires consent and a not-yet-installed model", () => {
    const notInstalled: ClipModelStatus = {
      installed: false,
      size_bytes: 351_529_890,
      license: "MIT",
    };
    const installed: ClipModelStatus = { ...notInstalled, installed: true };
    expect(mayDownload(notInstalled, false)).toBe(false);
    expect(mayDownload(notInstalled, true)).toBe(true);
    expect(mayDownload(installed, true)).toBe(false);
  });

  it("downloadClipModel refuses to call the backend without consent", async () => {
    await expect(downloadClipModel(false)).rejects.toThrow("explicit consent");
    expect(invoke).not.toHaveBeenCalled();
  });

  it("downloadClipModel invokes the backend command with consent", async () => {
    invoke.mockResolvedValueOnce(undefined);
    await downloadClipModel(true);
    expect(invoke).toHaveBeenCalledWith("download_clip_model", undefined);
  });

  it("clipModelStatus falls back to not-installed without a backend", async () => {
    invoke.mockRejectedValueOnce(new Error("no backend"));
    const status = await clipModelStatus();
    expect(status).toEqual({ installed: false, size_bytes: 0, license: "" });
  });

  it("modelSizeLabel formats the pinned size", () => {
    expect(modelSizeLabel(0)).toBe("");
    expect(modelSizeLabel(351_529_890)).toBe("335 MB");
    expect(modelSizeLabel(2 * 1024 * 1024 * 1024)).toBe("2.0 GB");
  });
});
