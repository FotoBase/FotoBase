// TASK-01-021: transport wrappers for the file-menu / folder-context
// commands. The backend owns the OS work (reveal, trash, queue re-render,
// clean exit); this module only forwards the decision. IPC is mocked here.

import { beforeEach, describe, expect, it, vi } from "vitest";

import { invoke } from "@tauri-apps/api/core";
import { deleteFromDisk, exitApp, refreshFolderThumbnails, revealInDisk } from "./file-menu";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

beforeEach(() => {
  vi.mocked(invoke).mockReset().mockResolvedValue(undefined);
});

describe("file-menu wrappers", () => {
  it("revealInDisk reveals a path in the OS file manager", async () => {
    await revealInDisk("/photos/a.jpg");
    expect(invoke).toHaveBeenCalledWith("reveal_path", { path: "/photos/a.jpg" });
  });

  it("deleteFromDisk trashes the photo by id", async () => {
    await deleteFromDisk(42);
    expect(invoke).toHaveBeenCalledWith("delete_to_trash", { fileId: 42 });
  });

  it("refreshFolderThumbnails re-queues one folder's renders", async () => {
    await refreshFolderThumbnails("/photos/2024");
    expect(invoke).toHaveBeenCalledWith("refresh_folder_thumbnails", { path: "/photos/2024" });
  });

  it("exitApp quits cleanly", async () => {
    await exitApp();
    expect(invoke).toHaveBeenCalledWith("exit_app");
  });
});
