import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import UploadDialog from "./UploadDialog.svelte";
import { setLocale } from "./i18n.svelte.js";

const invoke = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: unknown[]) => invoke(...args),
}));
vi.mock("@tauri-apps/api/event", () => ({
  listen: () => Promise.resolve(() => {}),
}));
const pickFolder = vi.fn();
vi.mock("@tauri-apps/plugin-dialog", () => ({
  open: (...args: unknown[]) => pickFolder(...args),
}));

const targets = [
  { id: "local-folder", name: "Local folder", capabilities: { albums: true, sizeChoice: true } },
  { id: "immich", name: "Immich", capabilities: { albums: true, sizeChoice: true } },
];

const initialResult = {
  jobId: "upload-job",
  cancelled: false,
  paused: false,
  files: [
    { fileId: 1, name: "done.jpg", state: "uploaded", error: null },
    { fileId: 2, name: "retry.jpg", state: "failed", error: "server unavailable" },
  ],
};

type Handler = (args: Record<string, unknown>) => unknown;

function backend(runResults: unknown[]): void {
  const handlers: Record<string, Handler> = {
    list_upload_targets: () => targets,
    upload_photos: () => runResults.shift(),
  };
  invoke.mockImplementation((cmd: string, args: Record<string, unknown> = {}) =>
    Promise.resolve().then(() => handlers[cmd]?.(args)),
  );
}

function calls(cmd: string): Record<string, unknown>[] {
  return invoke.mock.calls.filter(([name]) => name === cmd).map(([, args]) => args);
}

beforeEach(() => {
  setLocale("en");
  invoke.mockReset();
  pickFolder.mockReset();
});
afterEach(cleanup);

describe("UploadDialog", () => {
  it("retries a failed file without submitting successful files again", async () => {
    backend([initialResult, { ...initialResult, files: [initialResult.files[1]] }]);
    render(UploadDialog, { open: true, ids: [1, 2] });

    await screen.findByRole("option", { name: "Immich" });
    await fireEvent.change(screen.getByTestId("upload-target"), {
      target: { value: "local-folder" },
    });
    await fireEvent.input(screen.getByTestId("upload-local-folder"), {
      target: { value: "/tmp/MixedCaseUpload" },
    });
    await fireEvent.click(screen.getByTestId("upload-size"));
    await fireEvent.change(screen.getByTestId("upload-max-edge"), {
      target: { value: "2048" },
    });
    await fireEvent.click(screen.getByTestId("upload-run"));

    await screen.findByTestId("upload-file-2");
    await fireEvent.click(screen.getByTestId("upload-retry-2"));

    await waitFor(() => expect(calls("upload_photos")).toHaveLength(2));
    expect(calls("upload_photos")[0]).toMatchObject({
      ids: [1, 2],
      request: {
        config: { plugin: "local-folder", root: "/tmp/MixedCaseUpload" },
        size: { mode: "resized", max_edge: 2048 },
      },
    });
    expect(calls("upload_photos")[1]).toMatchObject({
      ids: [2],
      request: {
        jobId: "upload-job",
        size: { mode: "resized", max_edge: 2048 },
      },
    });
  });

  it("can cancel before the first progress event", async () => {
    let resolveUpload: ((value: unknown) => void) | undefined;
    invoke.mockImplementation((cmd: string) => {
      if (cmd === "list_upload_targets") return Promise.resolve(targets);
      if (cmd === "upload_photos") {
        return new Promise((resolve) => {
          resolveUpload = resolve;
        });
      }
      return Promise.resolve();
    });
    render(UploadDialog, { open: true, ids: [1] });

    await screen.findByRole("option", { name: "Immich" });
    await fireEvent.input(screen.getByTestId("upload-local-folder"), {
      target: { value: "/tmp/fotobase-upload" },
    });
    await fireEvent.click(screen.getByTestId("upload-run"));

    await waitFor(() => expect(calls("upload_photos")).toHaveLength(1));
    const requestId = (calls("upload_photos")[0].request as { jobId: string }).jobId;
    expect(requestId).toBeTruthy();
    expect(screen.getByTestId("upload-cancel").hasAttribute("disabled")).toBe(false);
    await fireEvent.click(screen.getByTestId("upload-cancel"));

    await waitFor(() => expect(calls("cancel_upload")).toEqual([{ jobId: requestId }]));
    resolveUpload?.({
      jobId: requestId,
      cancelled: true,
      paused: false,
      files: [],
    });
    await waitFor(() => expect(screen.queryByTestId("upload-cancel")).toBeNull());
  });
});
