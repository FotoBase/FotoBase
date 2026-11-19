// TASK-08-014 tests: Backup Manager dialog (PLAN §2L "Backup Pictures").
// The user picks an external destination and a schedule, runs a backup with
// progress, and sees the sets already stored there. IPC is mocked at the
// Tauri core module; the set format itself is tested in Rust.

import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import BackupManagerDialog from "./BackupManagerDialog.svelte";
import { formatBytes, type BackupReport, type BackupSet } from "./backup";
import { setLocale } from "./i18n.svelte.js";

const invoke = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...a: unknown[]) => invoke(...a),
}));
let progressCb: ((e: { payload: unknown }) => void) | null = null;
vi.mock("@tauri-apps/api/event", () => ({
  listen: (_name: string, cb: (e: { payload: unknown }) => void) => {
    progressCb = cb;
    return Promise.resolve(() => {
      progressCb = null;
    });
  },
}));
const pickFolder = vi.fn();
vi.mock("@tauri-apps/plugin-dialog", () => ({
  open: (...a: unknown[]) => pickFolder(...a),
}));

const set1: BackupSet = {
  id: "20261119T080000Z",
  created_at: "2026-11-19T08:00:00Z",
  created_unix: 1_795_075_200,
  parent: null,
  files: 120,
  new_files: 120,
  new_bytes: 2_048_000,
};

const report: BackupReport = {
  set_id: "20261119T090000Z",
  set_path: "/Volumes/Backup/fotobase-backup/20261119T090000Z",
  parent: set1.id,
  files_total: 120,
  files_copied: 2,
  files_reused: 118,
  bytes_copied: 4096,
};

type Handler = (args: Record<string, unknown>) => unknown;

function backend(overrides: Record<string, Handler> = {}): void {
  const handlers: Record<string, Handler> = {
    get_backup_config: () => ({ destination: "/Volumes/Backup", schedule: "off" }),
    set_backup_config: () => undefined,
    list_backup_sets: () => [set1],
    run_backup: () => report,
    cancel_backup: () => undefined,
    ...overrides,
  };
  invoke.mockImplementation((cmd: string, args: Record<string, unknown>) =>
    Promise.resolve().then(() => handlers[cmd](args ?? {})),
  );
}

function calls(cmd: string): unknown[] {
  return invoke.mock.calls.filter((c) => c[0] === cmd).map((c) => c[1]);
}

beforeEach(() => {
  setLocale("en");
  invoke.mockReset();
  pickFolder.mockReset();
  progressCb = null;
});
afterEach(cleanup);

describe("BackupManagerDialog", () => {
  it("renders nothing when closed", () => {
    backend();
    render(BackupManagerDialog, { open: false });
    expect(screen.queryByRole("dialog")).toBeNull();
  });

  it("loads the saved destination and lists the sets stored there", async () => {
    backend();
    render(BackupManagerDialog, { open: true });

    const dest = (await screen.findByTestId("backup-destination")) as HTMLInputElement;
    await waitFor(() => expect(dest.value).toBe("/Volumes/Backup"));
    const rows = await screen.findAllByTestId("backup-set");
    expect(rows).toHaveLength(1);
    expect(rows[0].textContent).toContain("2026-11-19");
    expect(rows[0].textContent).toContain("120");
    expect(calls("list_backup_sets")).toEqual([{ dest: "/Volumes/Backup" }]);
  });

  it("backs up to the destination with progress, then refreshes the set list", async () => {
    let finish: (r: BackupReport) => void = () => {};
    backend({ run_backup: () => new Promise<BackupReport>((r) => (finish = r)) });
    render(BackupManagerDialog, { open: true });
    await screen.findAllByTestId("backup-set");

    await fireEvent.click(screen.getByTestId("backup-run"));
    await waitFor(() => expect(calls("run_backup")).toEqual([{ dest: "/Volumes/Backup" }]));
    progressCb?.({ payload: { done: 3, total: 10, current: "/p/a.jpg.xmp", bytes_copied: 9 } });
    const progress = (await screen.findByTestId("backup-progress")) as HTMLProgressElement;
    await waitFor(() => expect(progress.value).toBe(3));
    expect(progress.max).toBe(10);

    finish(report);
    const done = await screen.findByTestId("backup-result");
    expect(done.textContent).toContain("2 new");
    expect(done.textContent).toContain("118");
    await waitFor(() => expect(calls("list_backup_sets").length).toBe(2));
    expect(calls("set_backup_config")).toEqual([
      { config: { destination: "/Volumes/Backup", schedule: "off" } },
    ]);
  });

  it("persists a schedule change", async () => {
    backend();
    render(BackupManagerDialog, { open: true });
    const select = (await screen.findByTestId("backup-schedule")) as HTMLSelectElement;
    await waitFor(() =>
      expect((screen.getByTestId("backup-destination") as HTMLInputElement).value).not.toBe(""),
    );

    await fireEvent.change(select, { target: { value: "weekly" } });

    await waitFor(() =>
      expect(calls("set_backup_config")).toEqual([
        { config: { destination: "/Volumes/Backup", schedule: "weekly" } },
      ]),
    );
  });

  it("picks the destination with the folder dialog", async () => {
    backend({ get_backup_config: () => ({ destination: null, schedule: "off" }) });
    pickFolder.mockResolvedValueOnce("/Volumes/Other");
    render(BackupManagerDialog, { open: true });
    const run = (await screen.findByTestId("backup-run")) as HTMLButtonElement;
    expect(run.disabled).toBe(true);

    await fireEvent.click(screen.getByTestId("backup-choose"));

    const dest = screen.getByTestId("backup-destination") as HTMLInputElement;
    await waitFor(() => expect(dest.value).toBe("/Volumes/Other"));
    expect(pickFolder).toHaveBeenCalledWith({ directory: true, multiple: false });
    expect(run.disabled).toBe(false);
  });

  it("shows a clear error when the destination is full or unplugged", async () => {
    backend({
      run_backup: () =>
        Promise.reject("backup stopped writing /x: No space left on device — destination full"),
    });
    render(BackupManagerDialog, { open: true });
    await screen.findAllByTestId("backup-set");

    await fireEvent.click(screen.getByTestId("backup-run"));

    const alert = await screen.findByRole("alert");
    expect(alert.textContent).toContain("No space left on device");
    expect(screen.queryByTestId("backup-result")).toBeNull();
  });

  it("cancels a running backup", async () => {
    backend({ run_backup: () => new Promise(() => {}) });
    render(BackupManagerDialog, { open: true });
    await screen.findAllByTestId("backup-set");
    await fireEvent.click(screen.getByTestId("backup-run"));

    await fireEvent.click(await screen.findByTestId("backup-cancel"));

    await waitFor(() => expect(calls("cancel_backup")).toHaveLength(1));
  });
});

describe("formatBytes", () => {
  it("uses binary units", () => {
    expect(formatBytes(512)).toBe("512 B");
    expect(formatBytes(2_048_000)).toBe("2.0 MiB");
  });
});
