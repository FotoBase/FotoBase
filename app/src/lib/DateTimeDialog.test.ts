// TASK-03-019 tests: the date/time dialog. The backend previews and applies;
// the dialog maps the three modes (shift / timezone / merge) to one
// adjustment, shows the before → after preview, reports progress and the
// per-photo failures. IPC is mocked at the wrapper module.

import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import DateTimeDialog from "./DateTimeDialog.svelte";
import {
  adjustDateTime,
  onDateTimeProgress,
  previewDateTime,
  type AdjustReport,
  type DateTimeProgress,
  type TimePreview,
} from "./datetime";
import { locales, setLocale, t } from "./i18n.svelte.js";

vi.mock("./datetime", async (importOriginal) => ({
  ...(await importOriginal<typeof import("./datetime")>()),
  previewDateTime: vi.fn(),
  adjustDateTime: vi.fn(),
  onDateTimeProgress: vi.fn(),
}));

const okReport: AdjustReport = { adjusted: 2, failed: [], cancelled: false };

let progressCb: ((p: DateTimeProgress) => void) | null = null;

beforeEach(() => {
  setLocale("en");
  vi.mocked(previewDateTime)
    .mockReset()
    .mockImplementation(async (ids): Promise<TimePreview[]> =>
      ids.map((id) => ({
        id,
        before: "2024-06-01T12:00:00+02:00",
        after: "2024-06-01T10:00:00+02:00",
        error: null,
      })),
    );
  vi.mocked(adjustDateTime).mockReset().mockResolvedValue(okReport);
  progressCb = null;
  vi.mocked(onDateTimeProgress).mockReset().mockImplementation(async (cb) => {
    progressCb = cb;
    return () => {};
  });
});

afterEach(cleanup);

function renderOpen(ids = [1, 2], onapplied = vi.fn()) {
  render(DateTimeDialog, { open: true, ids, onapplied });
  return onapplied;
}

const apply = (): HTMLButtonElement => screen.getByTestId("dt-apply") as HTMLButtonElement;

async function setShift(direction: "later" | "earlier", parts: Partial<Record<string, number>>) {
  await fireEvent.change(screen.getByTestId("dt-direction"), { target: { value: direction } });
  for (const [key, value] of Object.entries(parts)) {
    await fireEvent.input(screen.getByTestId(`dt-${key}`), { target: { value: String(value) } });
  }
}

describe("DateTimeDialog", () => {
  it("renders nothing when closed", () => {
    render(DateTimeDialog, { open: false, ids: [1], onapplied: vi.fn() });
    expect(screen.queryByTestId("dt-dialog")).toBeNull();
  });

  it("opens in shift mode with Apply disabled until something would change", () => {
    renderOpen();
    expect(screen.getByTestId("dt-shift")).toBeTruthy();
    expect(apply().disabled).toBe(true);
    expect(screen.getByTestId("dt-nothing")).toBeTruthy();
    expect(previewDateTime).not.toHaveBeenCalled();
  });

  it("previews then applies a shift of two hours earlier", async () => {
    const onapplied = renderOpen([1, 2]);
    await setShift("earlier", { hours: 2 });

    await waitFor(() => expect(screen.getAllByTestId("dt-row")).toHaveLength(2));
    expect(previewDateTime).toHaveBeenLastCalledWith([1, 2], { kind: "shift", seconds: -7200 });
    expect(screen.getAllByTestId("dt-row")[0].textContent).toContain(
      "2024-06-01 12:00:00 +02:00 → 2024-06-01 10:00:00 +02:00",
    );

    await fireEvent.click(apply());
    await waitFor(() => expect(onapplied).toHaveBeenCalledWith(okReport));
    expect(adjustDateTime).toHaveBeenCalledWith([1, 2], { kind: "shift", seconds: -7200 });
    expect(screen.getByTestId("dt-result").textContent).toContain("Adjusted 2 photo(s).");
  });

  it("previews only a sample of a big selection and says how many more", async () => {
    const ids = Array.from({ length: 500 }, (_, i) => i + 1);
    renderOpen(ids);
    await setShift("later", { minutes: 5 });

    await waitFor(() => expect(previewDateTime).toHaveBeenCalled());
    expect(vi.mocked(previewDateTime).mock.calls.at(-1)![0]).toHaveLength(20);
    await waitFor(() => expect(screen.getByTestId("dt-more").textContent).toContain("480"));

    await fireEvent.click(apply());
    await waitFor(() => expect(adjustDateTime).toHaveBeenCalled());
    expect(vi.mocked(adjustDateTime).mock.calls[0][0]).toHaveLength(500);
  });

  it("shows why a photo would be skipped", async () => {
    vi.mocked(previewDateTime).mockResolvedValue([
      { id: 1, before: null, after: null, error: "capture time \"x\" is not readable" },
    ]);
    renderOpen([1]);
    await setShift("later", { hours: 1 });
    await waitFor(() => expect(screen.getByTestId("dt-skipped")).toBeTruthy());
    expect(screen.getByTestId("dt-skipped").textContent).toContain("not readable");
  });

  describe("timezone mode", () => {
    async function openTimezone() {
      const onapplied = renderOpen([7]);
      await fireEvent.click(screen.getByTestId("dt-mode-timezone"));
      return onapplied;
    }

    it("refuses an invalid offset and explains", async () => {
      await openTimezone();
      await fireEvent.input(screen.getByTestId("dt-offset"), { target: { value: "+15:00" } });
      expect(apply().disabled).toBe(true);
      expect(screen.getByTestId("dt-offset-error")).toBeTruthy();
    });

    it("converts by default: camera left on UTC keeps the moment", async () => {
      await openTimezone();
      await fireEvent.input(screen.getByTestId("dt-offset"), { target: { value: "+02:00" } });
      await waitFor(() =>
        expect(previewDateTime).toHaveBeenLastCalledWith([7], {
          kind: "convert_offset",
          offset_minutes: 120,
        }),
      );
      await fireEvent.click(apply());
      await waitFor(() =>
        expect(adjustDateTime).toHaveBeenCalledWith([7], {
          kind: "convert_offset",
          offset_minutes: 120,
        }),
      );
    });

    it("relabels when the clock time is the right one", async () => {
      await openTimezone();
      await fireEvent.input(screen.getByTestId("dt-offset"), { target: { value: "-05:30" } });
      await fireEvent.click(screen.getByTestId("dt-keep-clock"));
      await waitFor(() =>
        expect(previewDateTime).toHaveBeenLastCalledWith([7], {
          kind: "relabel_offset",
          offset_minutes: -330,
        }),
      );
    });
  });

  describe("merge mode", () => {
    async function openMerge() {
      const onapplied = renderOpen([3, 4]);
      await fireEvent.click(screen.getByTestId("dt-mode-merge"));
      return onapplied;
    }

    it("needs both readings", async () => {
      await openMerge();
      await fireEvent.input(screen.getByTestId("dt-reference"), {
        target: { value: "2024-06-01T10:00:00" },
      });
      expect(apply().disabled).toBe(true);
    });

    it("shifts the selection by the difference between the two clocks", async () => {
      const onapplied = await openMerge();
      await fireEvent.input(screen.getByTestId("dt-reference"), {
        target: { value: "2024-06-01T10:00:00" },
      });
      await fireEvent.input(screen.getByTestId("dt-other"), {
        target: { value: "2024-06-01T08:59:30" },
      });
      await waitFor(() =>
        expect(previewDateTime).toHaveBeenLastCalledWith([3, 4], { kind: "shift", seconds: 3630 }),
      );
      expect(screen.getByTestId("dt-merge-shift").textContent).toContain("3630");

      await fireEvent.click(apply());
      await waitFor(() => expect(onapplied).toHaveBeenCalled());
      expect(adjustDateTime).toHaveBeenCalledWith([3, 4], { kind: "shift", seconds: 3630 });
    });

    it("disables Apply when the clocks already agree", async () => {
      await openMerge();
      for (const id of ["dt-reference", "dt-other"]) {
        await fireEvent.input(screen.getByTestId(id), { target: { value: "2024-06-01T10:00:00" } });
      }
      expect(apply().disabled).toBe(true);
    });
  });

  it("shows progress while the job runs", async () => {
    let finish: (r: AdjustReport) => void = () => {};
    vi.mocked(adjustDateTime).mockReturnValue(new Promise((resolve) => (finish = resolve)));
    renderOpen([1, 2, 3]);
    await setShift("later", { hours: 1 });
    await waitFor(() => expect(previewDateTime).toHaveBeenCalled());
    await fireEvent.click(apply());

    await waitFor(() => expect(progressCb).not.toBeNull());
    progressCb!({ done: 2, total: 3 });
    await waitFor(() =>
      expect(screen.getByTestId("dt-progress").textContent).toContain("Adjusting 2 of 3"),
    );
    expect(apply().disabled).toBe(true);

    finish({ adjusted: 3, failed: [], cancelled: false });
    await waitFor(() => expect(screen.getByTestId("dt-result")).toBeTruthy());
    expect(screen.queryByTestId("dt-progress")).toBeNull();
  });

  it("reports photos that could not be adjusted", async () => {
    vi.mocked(adjustDateTime).mockResolvedValue({
      adjusted: 1,
      failed: [{ id: 2, error: "boom" }],
      cancelled: false,
    });
    const onapplied = renderOpen([1, 2]);
    await setShift("later", { hours: 1 });
    await fireEvent.click(apply());

    await waitFor(() => expect(onapplied).toHaveBeenCalled());
    expect(screen.getByTestId("dt-result").textContent).toContain("Adjusted 1, skipped 1.");
  });

  it("keeps the dialog open and shows the error when the job fails", async () => {
    vi.mocked(adjustDateTime).mockRejectedValue("disk full");
    const onapplied = renderOpen([1]);
    await setShift("later", { hours: 1 });
    await fireEvent.click(apply());

    await waitFor(() => expect(screen.getByTestId("dt-error")).toBeTruthy());
    expect(screen.getByTestId("dt-error").textContent).toContain("disk full");
    expect(onapplied).not.toHaveBeenCalled();
    expect(screen.getByTestId("dt-dialog")).toBeTruthy();
  });

  it("closes with the Cancel button", async () => {
    renderOpen();
    await fireEvent.click(screen.getByTestId("dt-cancel"));
    expect(screen.queryByTestId("dt-dialog")).toBeNull();
  });

  it("closes on Escape without the key reaching listeners underneath", async () => {
    const below = vi.fn();
    window.addEventListener("keydown", below);
    renderOpen();
    await fireEvent.keyDown(screen.getByTestId("dt-dialog"), { key: "Escape" });
    window.removeEventListener("keydown", below);
    expect(screen.queryByTestId("dt-dialog")).toBeNull();
    expect(below).not.toHaveBeenCalled();
  });

  it("has every string in every locale", () => {
    const keys = [
      "menu.adjustDateTime",
      "info.captured",
      "info.adjustTime",
      "filters.sort",
      "filters.sortPath",
      "filters.sortCapture",
      "datetime.title",
      "datetime.count",
      "datetime.mode",
      "datetime.mode.shift",
      "datetime.mode.timezone",
      "datetime.mode.merge",
      "datetime.shift.direction",
      "datetime.shift.later",
      "datetime.shift.earlier",
      "datetime.shift.days",
      "datetime.shift.hours",
      "datetime.shift.minutes",
      "datetime.shift.seconds",
      "datetime.tz.offset",
      "datetime.tz.hint",
      "datetime.tz.keepClock",
      "datetime.tz.keepMoment",
      "datetime.tz.invalid",
      "datetime.merge.hint",
      "datetime.merge.reference",
      "datetime.merge.other",
      "datetime.merge.shift",
      "datetime.preview",
      "datetime.preview.more",
      "datetime.preview.skipped",
      "datetime.nothing",
      "datetime.apply",
      "datetime.cancel",
      "datetime.close",
      "datetime.running",
      "datetime.done",
      "datetime.doneFailed",
      "datetime.error",
    ];
    for (const locale of locales) {
      setLocale(locale);
      for (const key of keys) expect(t(key), `${locale}:${key}`).not.toBe(key);
    }
  });
});
